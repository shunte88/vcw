/*
 *  engine.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Worker supervision and the lifecycle of a capture session (§36).
 *
 * MIT License
 *
 * Copyright (c) 2026 Stue Hunter
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */

//! Worker supervision and the lifecycle of a capture session (§36).
//!
//! The engine is one dedicated OS thread that owns the transport. Commands go
//! in through a channel, events come out through a [`Bus`], and the
//! [`Machine`] it holds lives on that thread's stack -
//! so there is no lock around the transport, no shared mutable phase, and no
//! instant at which it is between states.
//!
//! # Why a thread and not a mutex
//!
//! Two reasons, and only one of them is taste.
//!
//! The taste one: §11's phases carry their resources, and moving a value
//! through `machine = machine.apply(step)` is what makes that work. Behind a
//! mutex the transport would have to be an `Option` so it could be taken out
//! and put back, and `None` would be a sixth state that §11 does not have.
//!
//! The one that is not taste: **a `vcw_audio::capture::Capture` is not
//! `Send`.** The thread that opens a device has to be the thread that keeps it
//! until it closes. So there was never a choice about whether the transport
//! gets a thread of its own - only about whether that thread is also the one
//! taking commands, and giving it two jobs is cheaper than giving it two
//! threads and a protocol between them.
//!
//! This is D8 as written: dedicated OS threads for the real-time path, tokio
//! nowhere near it. The engine thread never allocates in a callback, never
//! blocks on a network, and never awaits anything.
//!
//! # What the engine supervises
//!
//! §36 lists eleven workers. WP-07 wires the two that exist: the audio
//! callback, which CPAL runs, and the SQLite capture writer, which
//! [`vcw_project::persistence`] runs. The meter, waveform, detector,
//! fingerprint, identification, metadata, playback and export workers attach
//! here as their work packages land, and the pattern is set by these two -
//! each owns its thread, reports through the bus, and is allowed to fail
//! without taking the transport with it.
//!
//! # Failure is isolated, and reported
//!
//! §36 asks that task failure be isolated wherever possible, and this module
//! takes that literally in three places. A deck that refuses a transition
//! leaves the transport where it was, because
//! [`Refused`](crate::state::Refused) hands the phase back. A subscriber that
//! has gone away is pruned rather than propagated, because a reloaded webview
//! must not end a recording. And a command that cannot be carried out becomes
//! an event, not a panic and not a dropped connection - the operator is told,
//! and the side keeps recording.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use vcw_audio::buffers::Tee;
use vcw_audio::capture::{Capture, Negotiated, Request};
use vcw_audio::devices::{self, Direction};
use vcw_audio::source::{Pace, Simulated, Source};
use vcw_project::persistence::{self, Handle, Outcome};
use vcw_project::{Project, Session};
use vcw_types::{BoundaryObservation, CaptureInfo, CaptureState, Diagnostics, SampleRate};

use crate::commands::{Command, Setup};
use crate::detection::{self, Detectors};
use crate::events::{Bus, Event, Events};
use crate::fingerprinting::{self, Fingerprinted, Fingerprints};
use crate::metering::{self, Meters};
use crate::state::{Deck, Machine, Phase, Reply, Step, Yield};

/// How often the engine wakes when it has nothing to do but watch.
///
/// Also the resolution of [`Event::Position`]. 100 ms is four positions a
/// second, which is smoother than an operator can read and far below anything
/// S3 measured as a boundary cost.
const TICK: Duration = Duration::from_millis(100);

/// Anything that stopped the engine getting as far as a transition.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The device could not be opened.
    #[error("opening the device: {0}")]
    Audio(#[from] vcw_audio::Error),
    /// The project could not be opened, written or closed.
    #[error("the project: {0}")]
    Project(#[from] vcw_project::Error),
    /// The engine thread is gone.
    #[error("the engine has stopped")]
    Stopped,
}

/// The outcome of one capture, as the engine reports it.
#[derive(Debug)]
pub struct Recorded {
    /// The capture's row id.
    pub capture_id: i64,
    /// What the writer did.
    pub outcome: Outcome,
    /// The ring's four counters at the end.
    pub diagnostics: Diagnostics,
    /// Whether the capture can honestly be called bit-perfect.
    pub bit_perfect: bool,
    /// The project it went into.
    pub project: PathBuf,
}

/// A real deck: an open device, an open project, and the writer between them.
///
/// Built before the transport is armed, because [`Idle::arm`](crate::state::Idle::arm)
/// is infallible by design - everything that can fail about opening a device or
/// a project has already happened by the time this exists, so a failure to open
/// never has to be modelled as a phase.
///
/// The writer thread starts immediately and **paused**. That is what makes
/// §11's `Armed` real: the device is running so levels can be set (§50's "Set
/// Level" comes before "Drop Needle"), the ring is being drained so the
/// counters cannot report an overrun the transport caused by not listening, and
/// nothing is committed. `RECORD` is then a flag, not a thread spawn.
///
/// The meter worker starts with it, on a lossy tap of the same stream, which
/// is what makes §50's "set the level" step possible before anything is being
/// recorded. The live detector starts on a second tap of the same stream, for
/// §22's provisional markers, and is as droppable as the meter is: everything
/// either of them sees, the refine pass can see again in the project. The
/// fingerprint worker starts on a third tap and listens to the detector over
/// the bus, which is §25's progressive regions.
pub struct Recorder {
    source: Box<dyn Source>,
    writer: Option<Handle>,
    meters: Option<Meters>,
    detectors: Option<Detectors>,
    fingerprints: Option<Fingerprints>,
    detected: Vec<BoundaryObservation>,
    fingerprinted: Fingerprinted,
    path: PathBuf,
    capture_id: i64,
    info: CaptureInfo,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("path", &self.path)
            .field("capture_id", &self.capture_id)
            .field("frames", &self.position())
            .finish()
    }
}

impl Recorder {
    /// Opens the device and the project and starts the writer, paused.
    ///
    /// # Errors
    ///
    /// If the device cannot be opened as asked, or the project cannot be
    /// created, opened or written to.
    pub fn open(setup: &Setup, bus: &Bus) -> Result<Self, Error> {
        let (source, reader): (Box<dyn Source>, _) = match &setup.device {
            Some(name) => {
                let snapshot = devices::enumerate();
                let device = snapshot.find(name, Direction::Input)?;
                let mut request = Request::new(device.key.clone()).mode(setup.mode);
                if let Some(millis) = setup.ring_millis {
                    request.ring_millis = millis;
                }
                if let Some(hz) = setup.rate {
                    request = request.at(SampleRate(hz));
                }
                if let Some(channels) = setup.channels {
                    request = request.channels(channels);
                }
                if let Some(format) = setup.format {
                    request = request.format(format);
                }
                let (capture, reader) = Capture::start_on(device, &request)?;
                (Box::new(capture), reader)
            }
            None => {
                // `setup.format` is honoured here as well as on a real device:
                // it was silently ignored, so `--format s16` produced an S32
                // capture and there was no way to make a project the FLAC
                // encoder would accept without hardware.
                let (simulated, reader) = Simulated::deterministic_as(
                    SampleRate(setup.rate.unwrap_or(48_000)),
                    setup.channels.unwrap_or(2),
                    setup.format.unwrap_or(vcw_types::SampleFormat::S32),
                    Pace::RealTime,
                )?;
                (Box::new(simulated), reader)
            }
        };

        // The device's negotiated facts, plus the one fact the device cannot
        // report: §51's equalisation provenance, which only the operator knows.
        let info = source.info().with_eq(setup.eq);
        let mut project = open_or_create(&setup.project)?;
        // The session row goes down before a single frame is drained, so a
        // process killed in the next second still leaves evidence that a
        // capture was attempted (§15).
        let session = Session::begin(&mut project, &info)?;
        let capture_id = session.id();
        let config = persistence::Config {
            start_paused: true,
            ..persistence::Config::default()
        };
        // §10's fan-out, at the one point both source kinds pass through. The
        // writer now reads through the tee, so the meter sees exactly the bytes
        // that were committed, in the order they were committed, and no second
        // reader of the device has to exist.
        let mut tee = Tee::new(reader);
        let meters = Meters::spawn(tee.tap(metering::tap_bytes(&info)), &info, bus.clone());
        // Before the detector, and not after: the fingerprint worker takes its
        // regions from `track-detected` on the bus, and a subscriber that
        // appears after a publish has missed it. §25 is not optional, but a
        // capture is worth more than an identification, so a format
        // chromaprint will not take costs the side its fingerprints and not
        // its audio - and says so on the bus rather than in a log nobody has.
        let fingerprints = match fingerprinting::accepts(&info) {
            Ok(()) => {
                Fingerprints::spawn(tee.tap(fingerprinting::tap_bytes(&info)), &info, bus).ok()
            }
            Err(error) => {
                bus.publish(&Event::Warning {
                    code: "fingerprint-unavailable",
                    detail: format!("this capture will not be fingerprinted: {error}"),
                });
                None
            }
        };
        let detectors = Detectors::spawn(tee.tap(detection::tap_bytes(&info)), &info, bus.clone());
        let writer = persistence::spawn_on(project, session, &info, config, tee)?;

        Ok(Self {
            source,
            writer: Some(writer),
            meters: Some(meters),
            detectors: Some(detectors),
            fingerprints,
            detected: Vec::new(),
            fingerprinted: Fingerprinted::default(),
            path: setup.project.clone(),
            capture_id,
            info,
        })
    }

    /// How the device is actually running.
    #[must_use]
    pub fn negotiated(&self) -> &Negotiated {
        self.source.negotiated()
    }

    /// The capture's row id, which exists from the moment it is armed.
    #[must_use]
    pub const fn capture_id(&self) -> i64 {
        self.capture_id
    }

    /// The project being recorded into.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Everything §38 wants persisted about the capture's provenance.
    #[must_use]
    pub const fn info(&self) -> &CaptureInfo {
        &self.info
    }

    /// The ring's four counters, as of now.
    #[must_use]
    pub fn diagnostics(&self) -> Diagnostics {
        self.source.diagnostics()
    }

    /// Whether the capture can honestly be called bit-perfect, as of now.
    #[must_use]
    pub fn bit_perfect(&self) -> bool {
        self.source.verdict().is_confirmed()
    }

    /// Closes an armed capture that never recorded anything.
    ///
    /// An abandoned arm is not a capture, and leaving a zero-length row behind
    /// would litter the project with every time an operator changed their mind
    /// about the input. So the row goes if nothing was written, and stays if
    /// anything was.
    ///
    /// # Errors
    ///
    /// If the writer failed, or the project cannot be reopened to tidy up.
    pub fn abandon(mut self) -> Result<(), Error> {
        let outcome = self.halt(CaptureState::Finalised)?;
        if outcome.frames == 0 {
            let project = Project::open(&self.path)?;
            // Foreign keys first. `Session::begin` creates the diagnostics row
            // alongside the capture, so the capture cannot go until it has -
            // the same ordering recovery already uses for blocks. No
            // `capture_blocks` rows can exist here, because frames is zero and
            // a block is what makes frames; deleting them anyway would be
            // wrong, since their sampleblocks would be left orphaned.
            let conn = project.conn();
            conn.execute(
                "DELETE FROM capture_diagnostics WHERE capture_id = ?1",
                [self.capture_id],
            )
            .map_err(vcw_project::Error::from)?;
            conn.execute(
                "DELETE FROM captures WHERE capture_id = ?1",
                [self.capture_id],
            )
            .map_err(vcw_project::Error::from)?;
            project.close()?;
        }
        Ok(())
    }

    /// The provisional boundaries the live pass announced.
    ///
    /// Empty until the capture has been stopped, because the worker owns them
    /// until then and publishes each one on the bus as it settles. Kept so that
    /// a capture which is never refined still has boundaries, and so that the
    /// refine pass can be handed the live pass's answers to corroborate rather
    /// than starting from nothing.
    #[must_use]
    pub fn detected(&self) -> &[BoundaryObservation] {
        &self.detected
    }

    /// What the live fingerprint pass came to.
    ///
    /// Empty until the capture has been stopped, for the same reason
    /// [`Recorder::detected`] is: the worker owns its regions until then.
    /// Nothing is persisted - §25's regions are evidence for WP-22's lookup,
    /// and re-fingerprinting committed audio is cheap enough (S4: 0.6% of a
    /// core) that a cache with nothing to serve would be a schema change
    /// nobody can use yet.
    #[must_use]
    pub const fn fingerprinted(&self) -> &Fingerprinted {
        &self.fingerprinted
    }

    /// Stops the writer and the device and reports the result.
    fn halt(&mut self, state: CaptureState) -> Result<Outcome, Error> {
        // The meter goes before the writer, so the last `meter-update` is on
        // the bus before `capture-finished` is. A UI that stops drawing when
        // the capture finishes should not then be handed one more frame.
        if let Some(meters) = self.meters.take() {
            meters.stop();
        }
        // And the detector before the writer for the same reason: a
        // `track-detected` after `capture-finished` would arrive at a UI that
        // has already drawn the final waveform. What it found is kept rather
        // than discarded - see `Recorder::detected`.
        if let Some(detectors) = self.detectors.take() {
            self.detected = detectors.stop();
        }
        // The fingerprint worker after the detector, because it is the
        // detector's last publish that closes the last region it can see - and
        // before the writer, because `capture-finished` should still be the
        // final word on a capture.
        if let Some(fingerprints) = self.fingerprints.take() {
            self.fingerprinted = fingerprints.stop();
        }
        let Some(writer) = self.writer.take() else {
            return Err(Error::Project(vcw_project::Error::WriterLost));
        };
        // The device's counters are the audio side's evidence, and the writer
        // persists them; it cannot read them for itself.
        writer.set_result(state, self.source.diagnostics());
        // The writer stops when it is asked to, not when the ring's writing end
        // goes, so the order here only affects how long the join waits.
        let outcome = writer.stop()?;
        Ok(outcome)
    }
}

impl Deck for Recorder {
    type Error = Error;
    type Report = Recorded;

    fn start(&mut self) -> Result<(), Self::Error> {
        match &self.writer {
            Some(writer) => {
                writer.resume();
                Ok(())
            }
            None => Err(Error::Project(vcw_project::Error::WriterLost)),
        }
    }

    fn pause(&mut self) -> Result<(), Self::Error> {
        match &self.writer {
            Some(writer) => {
                writer.pause();
                Ok(())
            }
            None => Err(Error::Project(vcw_project::Error::WriterLost)),
        }
    }

    fn resume(&mut self) -> Result<(), Self::Error> {
        self.start()
    }

    // The writer flushes the part-filled block as it finishes, so its outcome
    // is longer than the last position anyone read. This is the figure that
    // matches the row in the project, which is the one worth reporting.
    fn frames(report: &Self::Report) -> u64 {
        report.outcome.frames
    }

    fn finish(mut self, state: CaptureState) -> Result<Self::Report, Self::Error> {
        let diagnostics = self.source.diagnostics();
        let bit_perfect = self.bit_perfect();
        let verification = self.source.verification().evidence();
        let verified = self.source.verification().confirms();
        let outcome = self.halt(state)?;

        // The verification only exists once the stream has run, and by then the
        // writer owned the connection, so this is a second open rather than a
        // held one. §9's confirmation, recorded when it is actually known.
        let project = Project::open(&self.path)?;
        Session::with_id(self.capture_id).record_verification(
            project.conn(),
            verified,
            Some(verification.as_str()),
        )?;
        project.close()?;

        Ok(Recorded {
            capture_id: self.capture_id,
            outcome,
            diagnostics,
            bit_perfect,
            project: self.path.clone(),
        })
    }

    fn position(&self) -> u64 {
        self.writer.as_ref().map_or(0, |w| w.progress().frames())
    }
}

/// Opens a project, creating it if it is not there.
fn open_or_create(path: &Path) -> Result<Project, vcw_project::Error> {
    if path.exists() {
        Project::open(path)
    } else {
        Project::create(path)
    }
}

/// The engine: one thread, one transport, commands in and events out.
///
/// Dropping it shuts the engine down and waits for it, so a capture cannot be
/// left running by a caller that forgot. [`Engine::shutdown`] is the same thing
/// with the errors visible.
pub struct Engine {
    commands: Sender<Command>,
    bus: Bus,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("running", &self.thread.is_some())
            .field("bus", &self.bus)
            .finish()
    }
}

impl Engine {
    /// Starts the engine thread.
    ///
    /// # Errors
    ///
    /// Only if the thread cannot be spawned.
    pub fn start() -> Result<Self, Error> {
        let (tx, rx) = channel();
        let bus = Bus::new();
        let thread_bus = bus.clone();
        let thread = std::thread::Builder::new()
            .name("vcw-engine".to_owned())
            .spawn(move || run(&rx, &thread_bus))
            .map_err(|e| Error::Project(vcw_project::Error::Io(e)))?;
        Ok(Self {
            commands: tx,
            bus,
            thread: Some(thread),
        })
    }

    /// Subscribes to the event stream.
    ///
    /// Every subscriber sees every event from the moment it subscribes. Nothing
    /// is replayed, which is what [`Command::Poll`] is for.
    #[must_use]
    pub fn events(&self) -> Events {
        self.bus.subscribe()
    }

    /// Sends a command. What happened comes back as an event.
    ///
    /// # Errors
    ///
    /// [`Error::Stopped`] if the engine thread has gone.
    pub fn send(&self, command: Command) -> Result<(), Error> {
        self.commands.send(command).map_err(|_| Error::Stopped)
    }

    /// Stops the engine and waits for the thread.
    ///
    /// Finalises a capture in progress rather than abandoning it: the audio is
    /// the part that cannot be recorded again.
    ///
    /// # Errors
    ///
    /// [`Error::Stopped`] if it had already gone.
    pub fn shutdown(mut self) -> Result<(), Error> {
        self.halt()
    }

    fn halt(&mut self) -> Result<(), Error> {
        // A send that fails means the thread is already on its way out, which
        // is not a reason to skip the join.
        let _ = self.commands.send(Command::Shutdown);
        match self.thread.take() {
            Some(thread) => {
                let _ = thread.join();
                Ok(())
            }
            None => Err(Error::Stopped),
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.halt();
    }
}

/// The engine thread.
///
/// The transport is a local variable, moved through the loop. That is the whole
/// reason there is no lock on it and no sixth `None` phase: at every point
/// between two statements, `machine` is exactly one of §11's five.
fn run(commands: &Receiver<Command>, bus: &Bus) {
    // Declared first so it drops last: whatever happens below, including a
    // panic on this thread, the stream is terminated. A consumer blocked on
    // `Events::next` has no other way to learn that nothing more is coming,
    // and §36 asks for a failed worker to be isolated rather than fatal.
    let _farewell = Farewell(bus);
    let mut machine: Machine<Recorder> = Machine::default();
    let mut last_position = u64::MAX;

    loop {
        match commands.recv_timeout(TICK) {
            Ok(Command::Shutdown) => {
                machine = finalise(machine, bus);
                break;
            }
            Ok(Command::Poll) => {
                bus.publish(&Event::Status {
                    phase: machine.phase(),
                    frames: machine.position(),
                });
            }
            Ok(command) => {
                let (next, sent) = dispatch(machine, command, bus);
                machine = next;
                if sent {
                    // A transition changes the position's meaning as well as
                    // its value, so republish rather than wait for the tick.
                    last_position = u64::MAX;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // Every sender has gone. Nothing more can arrive, so finish the
            // capture rather than leave a thread holding a device for ever.
            Err(RecvTimeoutError::Disconnected) => {
                machine = finalise(machine, bus);
                break;
            }
        }

        // §35's `recording-position`, on the tick and on every change. Only
        // while there is something to report: a transport at rest publishing
        // "still zero" four times a second is noise a log has to be grepped
        // past.
        if matches!(machine.phase(), Phase::Recording) {
            let frames = machine.position();
            if frames != last_position {
                last_position = frames;
                bus.publish(&position(&machine, frames));
            }
        }
    }

    // The transport goes before `Farewell` does, so anything still open is
    // closed before `closed` is published rather than after it.
    drop(machine);
}

/// Publishes §35's terminator however the engine thread ends.
///
/// A guard rather than a statement at the foot of `run`, because the case that
/// needs it is the one where the foot of `run` is never reached.
struct Farewell<'a>(&'a Bus);

impl Drop for Farewell<'_> {
    fn drop(&mut self) {
        // Always the last event, and always sent. `Bus::publish` cannot panic
        // and cannot fail, which is what makes it safe to call from a drop
        // that may itself be running during a panic.
        self.0.publish(&Event::Closed);
    }
}

/// Turns a command into a step, applies it, and reports what happened.
///
/// Returns the machine and whether the transport actually moved.
fn dispatch(machine: Machine<Recorder>, command: Command, bus: &Bus) -> (Machine<Recorder>, bool) {
    let name = command.as_str();

    // `Arm` is the one command that has to build something before the state
    // machine can be asked, and building it can fail. A failure here is not a
    // refused transition - the transport never got as far as one - so it is
    // reported as refused *from the phase it is in*, and nothing moves.
    let step = match command {
        Command::Arm(setup) => match Recorder::open(&setup, bus) {
            Ok(recorder) => {
                announce(&recorder, bus);
                Step::Arm(recorder)
            }
            Err(error) => {
                bus.publish(&Event::Refused {
                    command: name,
                    phase: machine.phase(),
                    reason: error.to_string(),
                });
                return (machine, false);
            }
        },
        Command::Disarm => Step::Disarm,
        Command::Record => Step::Record,
        Command::Pause => Step::Pause,
        Command::Resume => Step::Resume,
        Command::Stop => Step::Stop(ending(&machine)),
        Command::Reset => Step::Reset,
        // Handled by the caller: neither is a transition.
        Command::Poll | Command::Shutdown => return (machine, false),
        // No catch-all arm, deliberately. `Command` is `#[non_exhaustive]` so
        // that WP-10 onwards can add `play`, `seek` and the rest without
        // breaking anyone downstream - but inside this crate the attribute does
        // nothing, so this match stays exhaustive and adding a variant will
        // fail to compile here until someone decides what it does. That is the
        // reminder worth having; a catch-all would silently swallow it.
    };

    let (machine, reply) = machine.apply(step);
    let moved = report(reply, name, bus);
    if moved {
        finished(&machine, bus);
    }
    (machine, moved)
}

/// Publishes §35's `capture-finished`, if the transport has just stopped.
///
/// The summary belongs to the stop, not to the reset: an operator who stops a
/// side and leaves it there has to be told what was recorded, and a transport
/// that only reported it on the way back to idle would have kept the one fact
/// that mattered to itself. `Stopped` holds the report rather than yielding
/// it, so this reads it in place.
fn finished(machine: &Machine<Recorder>, bus: &Bus) {
    if let Machine::Stopped(stopped) = machine {
        let recorded = stopped.report();
        bus.publish(&Event::Finished {
            capture_id: recorded.capture_id,
            frames: recorded.outcome.frames,
            state: recorded.outcome.state,
            diagnostics: recorded.diagnostics,
            bit_perfect: recorded.bit_perfect,
        });
    }
}

/// Publishes whatever the reply means, and says whether the transport moved.
///
/// Takes the reply by value because a transition can hand back a whole deck,
/// and a deck that is merely dropped leaves its capture row behind. Owning the
/// reply is what lets this close one properly.
fn report(reply: Reply<Recorder>, command: &'static str, bus: &Bus) -> bool {
    match reply {
        Reply::Moved { from, to, yielded } => {
            bus.publish(&Event::Phase { from, to });
            match yielded {
                // The report was published when the capture stopped, which is
                // when it was news. A reset only releases it, and publishing
                // it twice would have a UI show the side finishing twice.
                Yield::Report(_) => {}
                // A disarm hands the deck back rather than dropping it, so
                // that changing your mind about the input does not litter the
                // project with zero-length captures. Failing to tidy up is
                // worth saying out loud, but it is not worth refusing a
                // transition that has already happened.
                Yield::Deck(deck) => {
                    if let Err(error) = deck.abandon() {
                        bus.publish(&Event::Warning {
                            code: "abandon-failed",
                            detail: error.to_string(),
                        });
                    }
                }
                Yield::Nothing => {}
            }
            true
        }
        Reply::Refused { phase, error } => {
            bus.publish(&Event::Refused {
                command,
                phase,
                reason: format!("{error}"),
            });
            false
        }
        Reply::Illegal { phase, step } => {
            bus.publish(&Event::Rejected {
                command: step,
                phase,
            });
            false
        }
    }
}

/// Says what was opened, and what was not granted (§9).
fn announce(recorder: &Recorder, bus: &Bus) {
    let negotiated = recorder.negotiated();
    bus.publish(&Event::Armed {
        project: recorder.path().display().to_string(),
        negotiated: format!(
            "{} Hz, {} ch, {:?}, {}",
            negotiated.rate.hz(),
            negotiated.channels,
            negotiated.format,
            negotiated.mode.as_str()
        ),
        divergences: negotiated
            .divergences
            .iter()
            .map(ToString::to_string)
            .collect(),
        verified: recorder.bit_perfect(),
    });
}

/// How a capture should be recorded as having ended.
///
/// Interrupted, not finalised, if anything was lost. The stop was orderly
/// either way; the capture was not, and §38 wants the difference kept.
fn ending(machine: &Machine<Recorder>) -> CaptureState {
    let clean = match machine {
        Machine::Recording(r) => r.deck().diagnostics().is_clean(),
        Machine::Paused(p) => p.deck().diagnostics().is_clean(),
        _ => true,
    };
    if clean {
        CaptureState::Finalised
    } else {
        CaptureState::Interrupted
    }
}

/// Builds §35's `recording-position`.
fn position(machine: &Machine<Recorder>, frames: u64) -> Event {
    let rate = match machine {
        Machine::Armed(a) => a.deck().negotiated().rate.hz(),
        Machine::Recording(r) => r.deck().negotiated().rate.hz(),
        Machine::Paused(p) => p.deck().negotiated().rate.hz(),
        Machine::Idle(_) | Machine::Stopped(_) => 0,
    };
    Event::Position {
        frames,
        seconds: if rate == 0 {
            0.0
        } else {
            frames as f64 / f64::from(rate)
        },
    }
}

/// Brings the transport to rest, finishing whatever is in progress.
///
/// A shutdown must not lose a side. Anything recording or paused is stopped
/// properly; anything merely armed is abandoned, which removes the empty row
/// it would otherwise leave.
fn finalise(machine: Machine<Recorder>, bus: &Bus) -> Machine<Recorder> {
    let machine = match machine {
        Machine::Recording(_) | Machine::Paused(_) => {
            let state = ending(&machine);
            let (next, reply) = machine.apply(Step::Stop(state));
            report(reply, "shutdown", bus);
            finished(&next, bus);
            next
        }
        other => other,
    };
    match machine {
        Machine::Armed(armed) => {
            let (idle, recorder) = armed.disarm();
            if let Err(error) = recorder.abandon() {
                bus.publish(&Event::Warning {
                    code: "abandon-failed",
                    detail: error.to_string(),
                });
            }
            Machine::Idle(idle)
        }
        Machine::Stopped(stopped) => {
            // `capture-finished` went out when the capture stopped, whether
            // that was an operator's stop or the one just above. The reset
            // only releases the report, so nothing is published here: a
            // consumer counting sides must be able to count these events.
            let (idle, _) = stopped.reset();
            Machine::Idle(idle)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guarantee the fan-out bus rests on: the stream always terminates.
    ///
    /// Tested on the guard rather than by breaking the engine, because there is
    /// no command that makes the transport panic - and if there were, it would
    /// be a bug to fix rather than a fixture to keep.
    #[test]
    fn a_panic_on_the_engine_thread_still_closes_the_stream() {
        let bus = Bus::new();
        let events = bus.subscribe();

        let thread = std::thread::spawn({
            let bus = bus.clone();
            move || {
                let _farewell = Farewell(&bus);
                bus.publish(&Event::Warning {
                    code: "test",
                    detail: "about to fall over".into(),
                });
                // The panic message goes to stderr during this test. That is
                // the point of it.
                panic!("the transport fell over");
            }
        });
        assert!(thread.join().is_err(), "the thread was supposed to panic");

        let seen = events.collect_until_closed();
        assert!(
            seen.last().is_some_and(Event::is_last),
            "a panicking engine left its consumers waiting: {seen:?}"
        );
        assert_eq!(seen.len(), 2, "{seen:?}");
    }
}
