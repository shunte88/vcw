/*
 *  state.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The recording state machine: invalid transitions do not compile (§11).
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

//! The recording state machine: invalid transitions do not compile (§11).
//!
//! §11 gives the transport as `Idle -> Armed -> Recording <-> Paused -> Stopped`
//! and says invalid transitions "shall be impossible". WP-07's exit criterion
//! reads that literally: **unrepresentable in the type system, not rejected at
//! runtime.** So each phase is its own type, every transition is a method that
//! consumes one phase and returns the next, and a method that would perform an
//! illegal move simply does not exist. `Idle` has no `pause`. There is nothing
//! to call, nothing to test, and nothing to get wrong later.
//!
//! [`Machine`] wraps the five types in an enum so that a command arriving from a
//! UI at an arbitrary moment can be dispatched. That enum is a *doorway*, not a
//! second state machine: it has no transition logic of its own, it can only call
//! the typestate methods, and those exist only where the move is legal. An
//! illegal command therefore cannot take a wrong path - there is no wrong path
//! to take - it falls through to [`Reply::Refused`] with the machine handed back
//! untouched.
//!
//! # Where §11's diagram stops and this does not
//!
//! Two transitions are here that §11's arrow does not draw, and both are
//! additions rather than reinterpretations:
//!
//! - **`Armed -> Idle` ([`Armed::disarm`]).** §11 offers no way out of `Armed`
//!   except by recording. An operator who armed the wrong input would have to
//!   record something to escape, which cannot be the intent.
//! - **`Stopped -> Idle` ([`Stopped::reset`]).** §11 describes the life of *one
//!   capture*, and is explicit that "stopping finalizes the current capture but
//!   does not close the project". §50's workflow is
//!   `Record -> Flip -> Record`, so a session is at least two captures and the
//!   transport has to come back round.
//!
//! Everything else §11 draws is here, and nothing it does not draw is reachable.
//!
//! # What a phase owns
//!
//! Each type holds exactly the data that phase can legitimately have, so the
//! absence of a resource is as unrepresentable as an illegal move. `Idle` holds
//! nothing. `Recording` holds a [`Deck`] that is running - not an
//! `Option<Deck>`, so "recording with nothing to record into" cannot be built.
//! `Stopped` holds the report and no deck, so nothing can be written to a
//! finished capture.
//!
//! # Failure keeps the session
//!
//! A deck operation can fail: a disk fills, a device vanishes. If `pause()`
//! returned a bare `Err` the caller would have consumed the `Recording` and have
//! nothing to hold, which loses a capture that is still perfectly good. So a
//! refused transition returns [`Refused`], which carries the **original phase
//! back out** alongside the error. That is §36's "task failure shall be
//! isolated" expressed in the signature rather than in a comment.
//!
//! # The exit criterion, as compile tests
//!
//! WP-07 has to *show* that an invalid transition is unrepresentable, not assert
//! it. Each illegal move below is a `compile_fail` doctest; each is immediately
//! followed by the legal move from the same phase, written identically, as an
//! ordinary passing doctest.
//!
//! The pairing is deliberate and is not decoration. **`rustdoc` on stable
//! ignores the error code in ```` ```compile_fail,E0599``` ````** - a doctest
//! annotated `E0599` passes when the code fails with `E0308` instead, which was
//! measured rather than assumed. So a bare `compile_fail` proves only that the
//! snippet did not compile, and a misspelled method name would pass it while
//! proving nothing. The twin closes that hole: a typo breaks the passing half,
//! and only a genuinely absent method leaves the pair intact.
//!
//! Pausing something that is not recording:
//!
//! ```compile_fail
//! use vcw_core::state::Idle;
//! let transport = Idle;
//! let _ = transport.pause();
//! ```
//!
//! ```
//! use vcw_core::state::Idle;
//! use vcw_core::state::Rehearsal;
//! let transport = Idle;
//! let _ = transport.arm(Rehearsal::default());
//! ```
//!
//! Recording from `Idle`, skipping `Armed`:
//!
//! ```compile_fail
//! use vcw_core::state::Idle;
//! let transport = Idle;
//! let _ = transport.record();
//! ```
//!
//! Resuming something that is not paused:
//!
//! ```compile_fail
//! use vcw_core::state::{Idle, Rehearsal};
//! let recording = Idle.arm(Rehearsal::default()).record().ok().unwrap();
//! let _ = recording.resume();
//! ```
//!
//! ```
//! use vcw_core::state::{Idle, Rehearsal};
//! let recording = Idle.arm(Rehearsal::default()).record().ok().unwrap();
//! let _ = recording.pause();
//! ```
//!
//! Pausing something that is already paused:
//!
//! ```compile_fail
//! use vcw_core::state::{Idle, Rehearsal};
//! let paused = Idle.arm(Rehearsal::default()).record().ok().unwrap()
//!     .pause().ok().unwrap();
//! let _ = paused.pause();
//! ```
//!
//! Recording into a capture that has been stopped:
//!
//! ```compile_fail
//! use vcw_core::state::{Idle, Rehearsal};
//! use vcw_types::CaptureState;
//! let stopped = Idle.arm(Rehearsal::default()).record().ok().unwrap()
//!     .stop(CaptureState::Finalised).ok().unwrap();
//! let _ = stopped.record();
//! ```
//!
//! ```
//! use vcw_core::state::{Idle, Rehearsal};
//! use vcw_types::CaptureState;
//! let stopped = Idle.arm(Rehearsal::default()).record().ok().unwrap()
//!     .stop(CaptureState::Finalised).ok().unwrap();
//! let _ = stopped.reset();
//! ```
//!
//! Using a phase after it has been moved out of - the transition consumed it,
//! so there is no second `Recording` to stop twice:
//!
//! ```compile_fail
//! use vcw_core::state::{Idle, Rehearsal};
//! use vcw_types::CaptureState;
//! let recording = Idle.arm(Rehearsal::default()).record().ok().unwrap();
//! let _first = recording.stop(CaptureState::Finalised);
//! let _second = recording.stop(CaptureState::Finalised);
//! ```

use std::fmt;
use std::time::{Duration, Instant};

use vcw_types::CaptureState;

/// What the transport is actually driving.
///
/// The state machine sequences; the deck does. Keeping them apart is what lets
/// §11 be tested exhaustively with no sound card, no project and no disk - the
/// tests in this module drive a counting deck that does nothing but record
/// which calls it received and in what order.
///
/// # Contract
///
/// **A method that returns `Err` must leave the deck in the phase it was in.**
/// The state machine hands the caller the original phase back on failure, and
/// that is only honest if the deck agrees. A deck that half-pauses and then
/// reports failure makes [`Refused`] a lie.
pub trait Deck {
    /// Why an operation could not be carried out.
    ///
    /// `Debug` is required because this ends up in an event and in a log. An
    /// error nobody can print is an error nobody will read.
    type Error: fmt::Debug;
    /// What a finished capture leaves behind.
    ///
    /// `Debug` for the same reason.
    type Report: fmt::Debug;

    /// Begins committing audio. `Armed -> Recording`.
    ///
    /// # Errors
    ///
    /// If the capture could not be started. The deck must still be armed.
    fn start(&mut self) -> Result<(), Self::Error>;

    /// Stops committing audio without giving up the device. `Recording -> Paused`.
    ///
    /// # Errors
    ///
    /// If the capture could not be paused. The deck must still be recording.
    fn pause(&mut self) -> Result<(), Self::Error>;

    /// Begins committing again. `Paused -> Recording`.
    ///
    /// # Errors
    ///
    /// If the capture could not be resumed. The deck must still be paused.
    fn resume(&mut self) -> Result<(), Self::Error>;

    /// Finalizes the capture and gives up the device.
    ///
    /// Consumes the deck, because §11's `Stopped` is not a phase you can record
    /// from and a deck that outlived it would be a way to try.
    ///
    /// # Errors
    ///
    /// If the capture could not be finalized. There is nothing to hand back:
    /// the deck is gone either way, which is why this is the one transition
    /// that cannot be refused into its original phase.
    fn finish(self, state: CaptureState) -> Result<Self::Report, Self::Error>;

    /// Frames the finished capture ended up with, read from its report.
    ///
    /// An associated function rather than a method because by the time there
    /// is a report there is no deck: [`finish`](Deck::finish) consumed it. It
    /// exists because finalizing is not a no-op - a writer flushes the
    /// part-filled block on its way out - so the position taken *before* the
    /// stop is short by up to one block, and `Stopped::position` would
    /// otherwise disagree with the row in the project.
    fn frames(report: &Self::Report) -> u64;

    /// Frames committed so far, per channel.
    ///
    /// Must not advance while paused. The position a UI shows is a position in
    /// the *recording*, and a pause is not part of it.
    fn position(&self) -> u64;

    /// Whether the deck is still able to record.
    ///
    /// Asked on the transport's tick while there is a capture open, because a
    /// deck can fail without being asked to do anything: a writer that stops
    /// on a failed commit holds its error until it is joined, and in the
    /// meantime [`position`](Deck::position) simply stops advancing, which is
    /// indistinguishable from a pause or a quiet passage. False means the
    /// capture is over and [`finish`](Deck::finish) will say why.
    ///
    /// Defaults to true: a deck with nothing that can die out from under it -
    /// every test deck in this crate - should not have to say so.
    fn is_running(&self) -> bool {
        true
    }
}

/// The five phases of §11, as data.
///
/// The types are what enforce the machine; this is what it is called when it
/// has to be printed, serialized or compared. Deriving `PartialEq` on the
/// phases themselves would invite exactly the runtime branching the types exist
/// to remove, so the comparison lives here instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Nothing selected. The transport is at rest.
    Idle,
    /// A device and a project are chosen and open; no audio has been committed.
    Armed,
    /// Audio is being committed.
    Recording,
    /// The device is still held, and nothing is being committed.
    Paused,
    /// The capture is finalized. §11: the project is *not* closed.
    Stopped,
}

impl Phase {
    /// The lower-case name used in events, JSON and the CLI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Armed => "armed",
            Self::Recording => "recording",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A transition that the deck refused, with the phase handed back intact.
///
/// The point is the `from` field. Without it a failed `pause` would consume a
/// perfectly good `Recording` and leave the caller with an error and no
/// capture; with it, a disk hiccup costs a log line rather than a side of
/// vinyl.
#[derive(Debug)]
pub struct Refused<S, E> {
    /// The phase the transport is still in.
    pub from: S,
    /// Why the deck said no.
    pub error: E,
}

/// How long the transport has been recording, discounting pauses.
///
/// Wall clock since `record()` is the wrong number to show: a capture paused
/// for ten minutes while the record was flipped is not ten minutes longer. The
/// authoritative length is always the deck's frame count; this exists for the
/// things a frame count cannot answer, such as how long a still-empty capture
/// has been armed for.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    started: Instant,
    /// Total time spent paused, accumulated as each pause ends.
    paused_total: Duration,
    /// When the current pause began, if one is in progress.
    paused_since: Option<Instant>,
}

impl Clock {
    fn start() -> Self {
        Self {
            started: Instant::now(),
            paused_total: Duration::ZERO,
            paused_since: None,
        }
    }

    fn pause(mut self) -> Self {
        self.paused_since = Some(Instant::now());
        self
    }

    fn resume(mut self) -> Self {
        if let Some(since) = self.paused_since.take() {
            self.paused_total += since.elapsed();
        }
        self
    }

    /// Time spent actually recording.
    #[must_use]
    pub fn running(&self) -> Duration {
        let paused_now = self.paused_since.map_or(Duration::ZERO, |s| s.elapsed());
        self.started
            .elapsed()
            .saturating_sub(self.paused_total)
            .saturating_sub(paused_now)
    }

    /// Time spent paused, including any pause in progress.
    #[must_use]
    pub fn paused(&self) -> Duration {
        self.paused_total + self.paused_since.map_or(Duration::ZERO, |s| s.elapsed())
    }
}

/// The transport at rest.
///
/// Holds nothing, because nothing is open. Every other phase is reached from
/// here and returns here.
#[derive(Clone, Copy, Debug, Default)]
pub struct Idle;

/// A device and a project are open; no audio has been committed.
#[derive(Debug)]
pub struct Armed<D: Deck> {
    deck: D,
}

/// Audio is being committed.
#[derive(Debug)]
pub struct Recording<D: Deck> {
    deck: D,
    clock: Clock,
}

/// The device is still held and nothing is being committed.
#[derive(Debug)]
pub struct Paused<D: Deck> {
    deck: D,
    clock: Clock,
}

/// The capture is finalized and the deck is gone.
#[derive(Debug)]
pub struct Stopped<D: Deck> {
    report: D::Report,
    frames: u64,
    clock: Clock,
}

impl Idle {
    /// `Idle -> Armed`. Takes a deck that is open but not running.
    ///
    /// Infallible on purpose: whatever could fail - opening the device, opening
    /// the project - has already failed or succeeded by the time a `Deck`
    /// exists. Building the deck is the risky part, and it happens outside the
    /// state machine so that a failure to open never has to be modeled as a
    /// phase.
    #[must_use]
    pub fn arm<D: Deck>(self, deck: D) -> Armed<D> {
        Armed { deck }
    }
}

impl<D: Deck> Armed<D> {
    /// `Armed -> Recording`.
    ///
    /// # Errors
    ///
    /// [`Refused`], still armed, if the deck could not start.
    pub fn record(mut self) -> Result<Recording<D>, Refused<Self, D::Error>> {
        match self.deck.start() {
            Ok(()) => Ok(Recording {
                deck: self.deck,
                clock: Clock::start(),
            }),
            Err(error) => Err(Refused { from: self, error }),
        }
    }

    /// `Armed -> Idle`, giving the deck back so the caller can close it.
    ///
    /// Not in §11's diagram; see the module documentation for why it is here.
    #[must_use]
    pub fn disarm(self) -> (Idle, D) {
        (Idle, self.deck)
    }

    /// The deck, for reading its position or diagnostics.
    pub const fn deck(&self) -> &D {
        &self.deck
    }
}

impl<D: Deck> Recording<D> {
    /// `Recording -> Paused`.
    ///
    /// # Errors
    ///
    /// [`Refused`], still recording, if the deck could not pause.
    pub fn pause(mut self) -> Result<Paused<D>, Refused<Self, D::Error>> {
        match self.deck.pause() {
            Ok(()) => Ok(Paused {
                deck: self.deck,
                clock: self.clock.pause(),
            }),
            Err(error) => Err(Refused { from: self, error }),
        }
    }

    /// `Recording -> Stopped`.
    ///
    /// §11 draws `Recording <-> Paused -> Stopped`, which reads as a chain, but
    /// requiring a pause before every stop would be a transport nobody has ever
    /// used. Both arms stop.
    ///
    /// # Errors
    ///
    /// If the deck could not finalize. The deck is consumed either way, so
    /// there is no phase to hand back - this is the one transition that cannot
    /// be refused into its origin.
    pub fn stop(self, state: CaptureState) -> Result<Stopped<D>, D::Error> {
        let report = self.deck.finish(state)?;
        Ok(Stopped {
            frames: D::frames(&report),
            report,
            clock: self.clock,
        })
    }

    /// Frames committed so far, per channel.
    pub fn position(&self) -> u64 {
        self.deck.position()
    }

    /// How long the transport has been running and paused.
    pub const fn clock(&self) -> &Clock {
        &self.clock
    }

    /// The deck, for reading its position or diagnostics.
    pub const fn deck(&self) -> &D {
        &self.deck
    }
}

impl<D: Deck> Paused<D> {
    /// `Paused -> Recording`.
    ///
    /// # Errors
    ///
    /// [`Refused`], still paused, if the deck could not resume.
    pub fn resume(mut self) -> Result<Recording<D>, Refused<Self, D::Error>> {
        match self.deck.resume() {
            Ok(()) => Ok(Recording {
                deck: self.deck,
                clock: self.clock.resume(),
            }),
            Err(error) => Err(Refused { from: self, error }),
        }
    }

    /// `Paused -> Stopped`.
    ///
    /// # Errors
    ///
    /// If the deck could not finalize. See [`Recording::stop`].
    pub fn stop(self, state: CaptureState) -> Result<Stopped<D>, D::Error> {
        let report = self.deck.finish(state)?;
        Ok(Stopped {
            frames: D::frames(&report),
            report,
            clock: self.clock.resume(),
        })
    }

    /// Frames committed so far. Does not advance while paused.
    pub fn position(&self) -> u64 {
        self.deck.position()
    }

    /// How long the transport has been running and paused.
    pub const fn clock(&self) -> &Clock {
        &self.clock
    }

    /// The deck, for reading its position or diagnostics.
    pub const fn deck(&self) -> &D {
        &self.deck
    }
}

impl<D: Deck> Stopped<D> {
    /// `Stopped -> Idle`, ready for the next side.
    ///
    /// Not in §11's diagram; see the module documentation. Yields the report so
    /// that nothing is silently discarded on the way round.
    #[must_use]
    pub fn reset(self) -> (Idle, D::Report) {
        (Idle, self.report)
    }

    /// What the finished capture left behind.
    pub const fn report(&self) -> &D::Report {
        &self.report
    }

    /// Frames committed, as of the moment the deck was finalized.
    pub const fn position(&self) -> u64 {
        self.frames
    }

    /// How long the finished capture ran and was paused for.
    pub const fn clock(&self) -> &Clock {
        &self.clock
    }
}

/// A deck that goes through the motions and records nothing.
///
/// Three uses, all of them real. It is what the compile proofs in this module's
/// documentation are written against, since they need a concrete `Deck` and
/// must not need a sound card. It is what the tests below drive, so §11 can be
/// exercised exhaustively with no project, no disk and no audio. And it lets a
/// UI be developed and demonstrated with nothing plugged in, which is the
/// difference between §4.5 being a principle and being usable.
///
/// It counts calls rather than ignoring them, so a test can assert the deck was
/// driven in the right order and not merely that the types lined up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rehearsal {
    /// Every method called on this deck, in order, by name.
    pub calls: Vec<&'static str>,
    /// Frames the deck will claim to have committed.
    pub frames: u64,
    /// Which call, by name, should fail. `None` means none of them.
    pub refuse: Option<&'static str>,
    /// Whether the deck reports itself dead to [`Deck::is_running`].
    ///
    /// A deck does not die by being called, which is the whole difficulty: it
    /// dies on its own, between calls, and the transport only finds out by
    /// asking. So this is a flag rather than another entry in `refuse`.
    pub died: bool,
}

impl Rehearsal {
    /// A deck that refuses the named call and accepts every other.
    #[must_use]
    pub fn refusing(call: &'static str) -> Self {
        Self {
            refuse: Some(call),
            ..Self::default()
        }
    }

    /// A deck that reports this many frames committed.
    #[must_use]
    pub fn holding(frames: u64) -> Self {
        Self {
            frames,
            ..Self::default()
        }
    }

    /// A deck that has died where it stands, as a writer does on a failed
    /// commit: every call still works, and [`Deck::is_running`] is false.
    #[must_use]
    pub fn dead() -> Self {
        Self {
            died: true,
            ..Self::default()
        }
    }

    fn note(&mut self, call: &'static str) -> Result<(), String> {
        self.calls.push(call);
        if self.refuse == Some(call) {
            // The contract says a refused call leaves the deck where it was.
            // There is nothing to undo here, which is precisely why this is a
            // fair stand-in: it cannot accidentally half-succeed.
            return Err(format!("{call} was told to fail"));
        }
        Ok(())
    }
}

/// What a [`Rehearsal`] leaves behind when it is finalized.
///
/// A report is whatever the deck says a finished capture amounts to;
/// [`Recorder`](crate::engine::Recorder)'s carries a row id and a diagnostics
/// count, and this one carries the calls it received. Both carry the frame
/// count, because [`Deck::frames`] has to be able to find it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rehearsed {
    /// Every method the deck received, in order, by name.
    pub calls: Vec<&'static str>,
    /// Frames the deck claimed to have committed.
    pub frames: u64,
}

impl Deck for Rehearsal {
    type Error = String;
    type Report = Rehearsed;

    fn start(&mut self) -> Result<(), Self::Error> {
        self.note("start")
    }

    fn pause(&mut self) -> Result<(), Self::Error> {
        self.note("pause")
    }

    fn resume(&mut self) -> Result<(), Self::Error> {
        self.note("resume")
    }

    fn finish(mut self, _state: CaptureState) -> Result<Self::Report, Self::Error> {
        self.note("finish")?;
        Ok(Rehearsed {
            calls: self.calls,
            frames: self.frames,
        })
    }

    fn frames(report: &Self::Report) -> u64 {
        report.frames
    }

    fn position(&self) -> u64 {
        self.frames
    }

    fn is_running(&self) -> bool {
        !self.died
    }
}

/// A transition request, as it arrives from outside.
///
/// The typestate cannot be indexed by a runtime value, so this is the bridge: a
/// UI sends a `Step`, and [`Machine::apply`] finds out whether the phase it is
/// in has a method for it. Where it does not, there is nothing to call.
#[derive(Debug)]
pub enum Step<D: Deck> {
    /// `Idle -> Armed`, with the deck to drive.
    Arm(D),
    /// `Armed -> Idle`, giving the deck back.
    Disarm,
    /// `Armed -> Recording`.
    Record,
    /// `Recording -> Paused`.
    Pause,
    /// `Paused -> Recording`.
    Resume,
    /// `Recording | Paused -> Stopped`, finalizing with this state.
    Stop(CaptureState),
    /// `Stopped -> Idle`, yielding the report.
    Reset,
}

impl<D: Deck> Step<D> {
    /// The lower-case name used in events, JSON and the CLI.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Arm(_) => "arm",
            Self::Disarm => "disarm",
            Self::Record => "record",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Stop(_) => "stop",
            Self::Reset => "reset",
        }
    }
}

/// Whatever a transition handed back on the way through.
#[derive(Debug)]
pub enum Yield<D: Deck> {
    /// Nothing to hand back.
    Nothing,
    /// A disarmed deck, for the caller to close.
    Deck(D),
    /// The report from a finished capture, released by `reset`.
    Report(D::Report),
}

/// What happened to a [`Step`].
#[derive(Debug)]
pub enum Reply<D: Deck> {
    /// The transport moved.
    Moved {
        /// The phase it was in.
        from: Phase,
        /// The phase it is in now.
        to: Phase,
        /// Whatever the transition released.
        yielded: Yield<D>,
    },
    /// The move was legal and the deck said no. The transport did not move.
    Refused {
        /// The phase it is still in.
        phase: Phase,
        /// Why the deck said no.
        error: D::Error,
    },
    /// The move happened and failed. The transport moved anyway.
    ///
    /// Only `stop` can produce this, and it is the reason the variant exists.
    /// [`Deck::finish`] consumes the deck, so a failure leaves nothing to go
    /// on being `Recording` with and the transport falls to `Idle` - which
    /// [`Reply::Refused`] cannot describe, because its whole contract is that
    /// nothing moved.
    ///
    /// Reported as a refusal until 2026-10-09, and the conflation cost
    /// somebody a side. A piCorePlayer user whose card filled mid-capture read
    /// `stop refused while recording` as "I cannot stop", when the capture had
    /// in fact stopped and been closed - and because a refusal publishes no
    /// phase change, the transport they were watching still said `recording`
    /// while the engine sat in `Idle`. The audio was on disk the whole time.
    Failed {
        /// The phase the transport was in.
        from: Phase,
        /// The phase it is in now, the transition having happened regardless.
        to: Phase,
        /// What went wrong on the way.
        error: D::Error,
    },
    /// The move does not exist from this phase. Nothing was attempted.
    ///
    /// Distinct from [`Reply::Refused`] on purpose: refused is a fault worth
    /// reporting to the operator, illegal is a caller bug or a stale UI button,
    /// and conflating them would bury a disk failure under a double-click.
    Illegal {
        /// The phase the transport is in.
        phase: Phase,
        /// The step that has no meaning here.
        step: &'static str,
    },
}

/// The five phases behind one door, so a runtime command can reach them.
///
/// This is not a second state machine. It holds no transition table, makes no
/// decisions about what is legal, and cannot construct a phase except by
/// calling a typestate method - and those exist only where §11 allows. The
/// `match` arms that look like a transition table are the opposite: they are
/// the complete list of places where a method *was found*, and every other
/// combination falls through to [`Reply::Illegal`] because there was nothing to
/// call.
///
/// [`Machine::apply`] consumes the machine and returns the next one. That is
/// the whole reason there is no `Poisoned` variant and no `Option` around it: a
/// caller holds the machine on its own stack and moves it through the loop, so
/// there is never an instant at which the transport has no phase.
#[derive(Debug)]
pub enum Machine<D: Deck> {
    /// At rest.
    Idle(Idle),
    /// Open, nothing committed.
    Armed(Armed<D>),
    /// Committing.
    Recording(Recording<D>),
    /// Holding the device, committing nothing.
    Paused(Paused<D>),
    /// Finalised.
    Stopped(Stopped<D>),
}

impl<D: Deck> Default for Machine<D> {
    fn default() -> Self {
        Self::Idle(Idle)
    }
}

impl<D: Deck> Machine<D> {
    /// Which phase the transport is in.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        match self {
            Self::Idle(_) => Phase::Idle,
            Self::Armed(_) => Phase::Armed,
            Self::Recording(_) => Phase::Recording,
            Self::Paused(_) => Phase::Paused,
            Self::Stopped(_) => Phase::Stopped,
        }
    }

    /// Frames committed so far, where the phase has any.
    #[must_use]
    pub fn position(&self) -> u64 {
        match self {
            Self::Idle(_) => 0,
            Self::Armed(a) => a.deck().position(),
            Self::Recording(r) => r.position(),
            Self::Paused(p) => p.position(),
            Self::Stopped(s) => s.position(),
        }
    }

    /// Whether the deck behind an open capture is still able to record.
    ///
    /// True in the phases that hold no deck, because "the deck has died" is not
    /// a thing that can be true of a transport that has not got one. Only
    /// `Recording` and `Paused` can answer usefully, and they are the two the
    /// engine asks about - see [`Deck::is_running`].
    #[must_use]
    pub fn deck_running(&self) -> bool {
        match self {
            Self::Idle(_) | Self::Stopped(_) => true,
            Self::Armed(a) => a.deck().is_running(),
            Self::Recording(r) => r.deck().is_running(),
            Self::Paused(p) => p.deck().is_running(),
        }
    }

    /// Applies a step, returning the machine and what happened.
    ///
    /// Every arm below exists because the phase it names has that method. There
    /// is no arm for `Idle` + `Pause` because `Idle::pause` is not a thing that
    /// can be written, which is the point of the whole module.
    #[must_use]
    pub fn apply(self, step: Step<D>) -> (Self, Reply<D>) {
        let phase = self.phase();
        let name = step.as_str();
        match (self, step) {
            (Self::Idle(idle), Step::Arm(deck)) => (
                Self::Armed(idle.arm(deck)),
                Reply::Moved {
                    from: Phase::Idle,
                    to: Phase::Armed,
                    yielded: Yield::Nothing,
                },
            ),
            (Self::Armed(armed), Step::Disarm) => {
                let (idle, deck) = armed.disarm();
                (
                    Self::Idle(idle),
                    Reply::Moved {
                        from: Phase::Armed,
                        to: Phase::Idle,
                        yielded: Yield::Deck(deck),
                    },
                )
            }
            (Self::Armed(armed), Step::Record) => match armed.record() {
                Ok(recording) => (
                    Self::Recording(recording),
                    Reply::Moved {
                        from: Phase::Armed,
                        to: Phase::Recording,
                        yielded: Yield::Nothing,
                    },
                ),
                Err(Refused { from, error }) => (
                    Self::Armed(from),
                    Reply::Refused {
                        phase: Phase::Armed,
                        error,
                    },
                ),
            },
            (Self::Recording(recording), Step::Pause) => match recording.pause() {
                Ok(paused) => (
                    Self::Paused(paused),
                    Reply::Moved {
                        from: Phase::Recording,
                        to: Phase::Paused,
                        yielded: Yield::Nothing,
                    },
                ),
                Err(Refused { from, error }) => (
                    Self::Recording(from),
                    Reply::Refused {
                        phase: Phase::Recording,
                        error,
                    },
                ),
            },
            (Self::Paused(paused), Step::Resume) => match paused.resume() {
                Ok(recording) => (
                    Self::Recording(recording),
                    Reply::Moved {
                        from: Phase::Paused,
                        to: Phase::Recording,
                        yielded: Yield::Nothing,
                    },
                ),
                Err(Refused { from, error }) => (
                    Self::Paused(from),
                    Reply::Refused {
                        phase: Phase::Paused,
                        error,
                    },
                ),
            },
            // The one transition that cannot hand its phase back: `finish`
            // consumes the deck, so a failure leaves nothing to be recording
            // with. The transport falls to `Idle`, which is the truth - there
            // is no capture any more - and the error is still reported.
            (Self::Recording(recording), Step::Stop(state)) => match recording.stop(state) {
                Ok(stopped) => (
                    Self::Stopped(stopped),
                    Reply::Moved {
                        from: Phase::Recording,
                        to: Phase::Stopped,
                        yielded: Yield::Nothing,
                    },
                ),
                Err(error) => (
                    Self::Idle(Idle),
                    Reply::Failed {
                        from: Phase::Recording,
                        to: Phase::Idle,
                        error,
                    },
                ),
            },
            (Self::Paused(paused), Step::Stop(state)) => match paused.stop(state) {
                Ok(stopped) => (
                    Self::Stopped(stopped),
                    Reply::Moved {
                        from: Phase::Paused,
                        to: Phase::Stopped,
                        yielded: Yield::Nothing,
                    },
                ),
                Err(error) => (
                    Self::Idle(Idle),
                    Reply::Failed {
                        from: Phase::Paused,
                        to: Phase::Idle,
                        error,
                    },
                ),
            },
            (Self::Stopped(stopped), Step::Reset) => {
                let (idle, report) = stopped.reset();
                (
                    Self::Idle(idle),
                    Reply::Moved {
                        from: Phase::Stopped,
                        to: Phase::Idle,
                        yielded: Yield::Report(report),
                    },
                )
            }
            (machine, _) => (machine, Reply::Illegal { phase, step: name }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives one full side: arm, record, pause, resume, stop, reset.
    fn side(deck: Rehearsal) -> (Machine<Rehearsal>, Vec<&'static str>) {
        let mut machine = Machine::default();
        let mut moves = Vec::new();
        for step in [
            Step::Arm(deck),
            Step::Record,
            Step::Pause,
            Step::Resume,
            Step::Stop(CaptureState::Finalised),
        ] {
            let (next, reply) = machine.apply(step);
            machine = next;
            if let Reply::Moved { to, .. } = reply {
                moves.push(to.as_str());
            }
        }
        (machine, moves)
    }

    #[test]
    fn a_deck_that_has_died_is_reported_dead_in_the_phases_that_hold_one() {
        // The transport's only way of learning that a capture has ended
        // without anybody ending it. A writer that stops on a failed commit
        // keeps its error until it is joined, so between the failure and the
        // next `stop` the position simply stops advancing - which is what a
        // pause looks like, and what a quiet passage looks like.
        let mut machine = Machine::default();
        assert!(machine.deck_running(), "idle holds no deck to lose");

        let (next, _) = machine.apply(Step::Arm(Rehearsal::dead()));
        machine = next;
        assert!(!machine.deck_running(), "armed");

        let (next, _) = machine.apply(Step::Record);
        machine = next;
        assert!(!machine.deck_running(), "recording");

        // A commit can fail on the way into a pause as easily as during a
        // record, and a paused writer that has died would otherwise be found
        // by a `resume` that silently does nothing.
        let (next, _) = machine.apply(Step::Pause);
        machine = next;
        assert!(!machine.deck_running(), "paused");

        // The negative twin: a live deck says so in every one of them, or the
        // engine would end every capture on its first tick.
        let mut live = Machine::default();
        for step in [Step::Arm(Rehearsal::default()), Step::Record, Step::Pause] {
            let (next, _) = live.apply(step);
            live = next;
            assert!(live.deck_running(), "a live deck reported itself dead");
        }
    }

    #[test]
    fn the_whole_of_section_11s_diagram_is_walkable() {
        let (machine, moves) = side(Rehearsal::default());
        assert_eq!(
            moves,
            ["armed", "recording", "paused", "recording", "stopped"]
        );
        assert_eq!(machine.phase(), Phase::Stopped);

        // And the deck was driven in that order too, which the phase names
        // alone would not prove: the types could line up while the machine
        // called nothing.
        let Machine::Stopped(stopped) = &machine else {
            panic!("not stopped");
        };
        assert_eq!(
            stopped.report().calls,
            ["start", "pause", "resume", "finish"]
        );
    }

    #[test]
    fn stopping_does_not_close_the_project_it_only_ends_the_capture() {
        // §11 in one assertion: after a stop the transport can come back round
        // and record the other side, and the report from the first side is
        // handed over rather than dropped on the floor.
        let (machine, _) = side(Rehearsal::default());
        let (machine, reply) = machine.apply(Step::Reset);
        assert_eq!(machine.phase(), Phase::Idle);
        let Reply::Moved {
            yielded: Yield::Report(report),
            ..
        } = reply
        else {
            panic!("reset should release the report, got {reply:?}");
        };
        assert_eq!(report.calls, ["start", "pause", "resume", "finish"]);

        let (machine, _) = machine.apply(Step::Arm(Rehearsal::default()));
        assert_eq!(machine.phase(), Phase::Armed);
    }

    #[test]
    fn every_step_that_is_not_in_the_diagram_is_illegal_from_every_phase() {
        // The exhaustive statement the compile proofs make one at a time. What
        // is being asserted here is *not* that the machine rejects these - it
        // is that there was no method to call, so nothing was attempted and the
        // phase is unchanged.
        let legal = [
            (Phase::Idle, "arm"),
            (Phase::Armed, "disarm"),
            (Phase::Armed, "record"),
            (Phase::Recording, "pause"),
            (Phase::Recording, "stop"),
            (Phase::Paused, "resume"),
            (Phase::Paused, "stop"),
            (Phase::Stopped, "reset"),
        ];

        for phase in [
            Phase::Idle,
            Phase::Armed,
            Phase::Recording,
            Phase::Paused,
            Phase::Stopped,
        ] {
            for name in [
                "arm", "disarm", "record", "pause", "resume", "stop", "reset",
            ] {
                if legal.contains(&(phase, name)) {
                    continue;
                }
                let machine = at(phase);
                let (after, reply) = machine.apply(step_named(name));
                assert_eq!(
                    after.phase(),
                    phase,
                    "{name} from {phase} moved the transport"
                );
                assert!(
                    matches!(reply, Reply::Illegal { .. }),
                    "{name} from {phase} gave {reply:?}, expected Illegal"
                );
            }
        }
    }

    #[test]
    fn a_deck_that_refuses_hands_the_capture_back_rather_than_losing_it() {
        // The property that matters more than any other in this module: a disk
        // hiccup during a pause must not cost the side.
        let machine = Machine::default();
        let (machine, _) = machine.apply(Step::Arm(Rehearsal::refusing("pause")));
        let (machine, _) = machine.apply(Step::Record);
        assert_eq!(machine.phase(), Phase::Recording);

        let (machine, reply) = machine.apply(Step::Pause);
        assert!(
            matches!(
                reply,
                Reply::Refused {
                    phase: Phase::Recording,
                    ..
                }
            ),
            "{reply:?}"
        );
        assert_eq!(
            machine.phase(),
            Phase::Recording,
            "a refused pause must leave the transport recording"
        );

        // And it is still a working transport, not a husk: it stops cleanly.
        let (machine, reply) = machine.apply(Step::Stop(CaptureState::Finalised));
        assert!(matches!(reply, Reply::Moved { .. }), "{reply:?}");
        assert_eq!(machine.phase(), Phase::Stopped);
    }

    #[test]
    fn a_deck_that_refuses_to_start_leaves_the_transport_armed() {
        let machine = Machine::default();
        let (machine, _) = machine.apply(Step::Arm(Rehearsal::refusing("start")));
        let (machine, reply) = machine.apply(Step::Record);
        assert!(matches!(reply, Reply::Refused { .. }), "{reply:?}");
        assert_eq!(machine.phase(), Phase::Armed);

        // Armed and still disarmable, so the operator can fix the device and
        // try again without restarting the application.
        let (machine, reply) = machine.apply(Step::Disarm);
        assert_eq!(machine.phase(), Phase::Idle);
        assert!(
            matches!(
                reply,
                Reply::Moved {
                    yielded: Yield::Deck(_),
                    ..
                }
            ),
            "disarm must give the deck back so it can be closed: {reply:?}"
        );
    }

    #[test]
    fn a_stop_that_fails_reports_it_and_does_not_pretend_to_be_stopped() {
        // `finish` consumes the deck, so a failure here genuinely has no
        // capture to hand back. Falling to Idle is the honest answer; claiming
        // Stopped would tell the operator a finalized capture exists.
        //
        // And it is `Failed`, not `Refused`: a refusal's whole contract is
        // that the transport did not move, and this one did. Reported as a
        // refusal until 2026-10-09, when a piCorePlayer user read `stop
        // refused while recording` as "I cannot stop" - with no phase change
        // published either, so the window went on saying `recording` against
        // an engine that had gone to `Idle` and closed the capture.
        for (phase, steps) in [
            (Phase::Recording, vec![Step::Record]),
            (Phase::Paused, vec![Step::Record, Step::Pause]),
        ] {
            let mut machine = Machine::default();
            let (next, _) = machine.apply(Step::Arm(Rehearsal::refusing("finish")));
            machine = next;
            for step in steps {
                let (next, _) = machine.apply(step);
                machine = next;
            }
            let (machine, reply) = machine.apply(Step::Stop(CaptureState::Finalised));
            match reply {
                Reply::Failed { from, to, .. } => {
                    assert_eq!(from, phase, "the phase it failed out of");
                    assert_eq!(to, Phase::Idle, "and the one it landed in");
                }
                other => panic!("a failed stop from {phase} must not be {other:?}"),
            }
            assert_eq!(machine.phase(), Phase::Idle);
        }

        // The negative twin: every other failing transition is still a
        // refusal that moves nothing, or `Failed` would swallow them.
        let machine = Machine::default();
        let (machine, _) = machine.apply(Step::Arm(Rehearsal::refusing("pause")));
        let (machine, _) = machine.apply(Step::Record);
        let (machine, reply) = machine.apply(Step::Pause);
        assert!(matches!(reply, Reply::Refused { .. }), "{reply:?}");
        assert_eq!(machine.phase(), Phase::Recording, "a refusal moves nothing");
    }

    #[test]
    fn the_position_comes_from_the_deck_and_survives_a_stop() {
        let machine = Machine::default();
        let (machine, _) = machine.apply(Step::Arm(Rehearsal::holding(96_000)));
        let (machine, _) = machine.apply(Step::Record);
        assert_eq!(machine.position(), 96_000);
        let (machine, _) = machine.apply(Step::Pause);
        assert_eq!(
            machine.position(),
            96_000,
            "a pause must not move the position"
        );
        let (machine, _) = machine.apply(Step::Resume);
        let (machine, _) = machine.apply(Step::Stop(CaptureState::Finalised));
        assert_eq!(
            machine.position(),
            96_000,
            "the length of a finished capture is still readable"
        );
    }

    #[test]
    fn the_clock_discounts_the_time_spent_paused() {
        let armed = Idle.arm(Rehearsal::default());
        let recording = armed.record().expect("record");
        let paused = recording.pause().expect("pause");
        std::thread::sleep(Duration::from_millis(40));
        let recording = paused.resume().expect("resume");

        let clock = recording.clock();
        assert!(
            clock.paused() >= Duration::from_millis(35),
            "the pause was not counted: {:?}",
            clock.paused()
        );
        assert!(
            clock.running() < Duration::from_millis(30),
            "the pause leaked into the running time: {:?}",
            clock.running()
        );
    }

    #[test]
    fn phase_names_round_trip_into_the_strings_events_use() {
        for (phase, name) in [
            (Phase::Idle, "idle"),
            (Phase::Armed, "armed"),
            (Phase::Recording, "recording"),
            (Phase::Paused, "paused"),
            (Phase::Stopped, "stopped"),
        ] {
            assert_eq!(phase.as_str(), name);
            assert_eq!(phase.to_string(), name);
        }
    }

    /// Builds a machine sitting in the given phase.
    fn at(phase: Phase) -> Machine<Rehearsal> {
        if phase == Phase::Stopped {
            return side(Rehearsal::default()).0;
        }
        let path: &[&str] = match phase {
            Phase::Idle => &[],
            Phase::Armed => &["arm"],
            Phase::Recording => &["arm", "record"],
            Phase::Paused => &["arm", "record", "pause"],
            Phase::Stopped => unreachable!("handled above"),
        };
        let mut machine = Machine::default();
        for name in path {
            let (next, reply) = machine.apply(step_named(name));
            assert!(matches!(reply, Reply::Moved { .. }), "{name}: {reply:?}");
            machine = next;
        }
        machine
    }

    /// A step from its name, for the exhaustive table above.
    fn step_named(name: &str) -> Step<Rehearsal> {
        match name {
            "arm" => Step::Arm(Rehearsal::default()),
            "disarm" => Step::Disarm,
            "record" => Step::Record,
            "pause" => Step::Pause,
            "resume" => Step::Resume,
            "stop" => Step::Stop(CaptureState::Finalised),
            "reset" => Step::Reset,
            other => panic!("no such step: {other}"),
        }
    }
}
