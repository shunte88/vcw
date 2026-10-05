/*
 *  playback.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Playback transport: audition scopes, the feeder thread and the render path.
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

//! Playback transport: audition scopes, the feeder thread and the render path.
//!
//! Requirements: §21 (playback), §35 (commands and events), §36 (workers).
//!
//! This is where the three halves of playback meet. [`vcw_project::pcm`] turns
//! per-channel blocks back into interleaved frames, [`vcw_audio::convert`] puts
//! them in the format the device asked for, and [`vcw_audio::playback`] hands
//! them to the converter. None of those three knows about the other two, which
//! is ADR-0003 working as intended: the SQL stays in the crate that owns the
//! database, CPAL stays in the crate that owns the device, and the joining
//! happens here.
//!
//! # §21's four targets are one type
//!
//! Complete capture, selected region, individual track and boundary audition
//! differ in how their extent is worked out and in nothing else afterwards, so
//! [`Scope`] resolves all four to a [`Span`] and the transport below it has one
//! case to handle. A playback path that knew what a track was would have put
//! edit-model knowledge inside the audio path, where WP-13 would then have had
//! to change it.
//!
//! # Two drivers, one pump
//!
//! [`Player`] is the live one: it owns the output stream, and a feeder thread
//! owns the project connection and keeps the chunk queue full.
//! [`render`] is the device-free one: it runs the same [`Pump`] and the same
//! [`Source::on_data`] callback synchronously and writes the result to a file.
//!
//! The render path is not a convenience. §21's exit criterion is a gapless
//! seek, and a gap is a thing you cannot see from outside: a device that was
//! fed silence and a device that was fed audio both return the same "it
//! played". Rendering puts the bytes the converter would have received into a
//! file where a test can compare them, and the comparison runs in CI on a
//! machine with no sound card. What rendering cannot measure is how long the
//! device takes to join the new position after a seek; that is measured
//! separately and stated as a number rather than claimed to be zero.
//!
//! # The feeder thread is allowed to be slow, and never to lie
//!
//! It reads SQLite, so it can block for as long as a page fault on a cold
//! cache takes. That is why it is not the audio thread. If it falls behind, the
//! callback writes silence and counts an underrun; if it finds a hole in the
//! timeline it stops and says so, because inventing silence for missing audio
//! would make a damaged capture sound like a quiet one.

use std::cell::Cell;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use vcw_audio::chunks::{self, Feeder};
use vcw_audio::convert::{self, Conversion};
use vcw_audio::devices::{self, Direction};
use vcw_audio::playback::{
    Counters, Cursor, Fidelity, Health, Material, Opened, Playback, Request, Source,
};
use vcw_project::pcm::{Layout, Reader};
use vcw_project::{Connection, Project};
use vcw_signal::meter::{Config, Meter};
use vcw_types::span::frames_at;
use vcw_types::{SampleFormat, SampleRate, Span};

use crate::events::{Bus, Event};
use crate::metering;

/// How much either side of a boundary a boundary audition plays.
///
/// §21 requires the operation and does not say how much context it needs. Three
/// seconds is what §50's workflow implies: long enough to hear the run-out of
/// one track and the attack of the next, short enough that a user checking
/// twenty boundaries is not sitting through two minutes of music.
pub const BOUNDARY_CONTEXT_SECONDS: f64 = 3.0;

/// How far `SKIP FORWARD` and `SKIP BACK` move the playhead with no marks.
///
/// §21 names the operations without defining the step, and what they are
/// actually for is moving between tracks: [`Audition::marks`] carries the frames
/// to jump between, and a skip lands on the next one. Ten seconds is what a
/// skip means when there are none - an unanalysed side, or a capture whose
/// boundaries nobody has adopted - because a transport whose skip key did
/// nothing until detection had run would be worse than one that nudges.
pub const SKIP_SECONDS: f64 = 10.0;

/// Where a skip lands, given the marks and a position.
///
/// # Lands *on* the mark, and this is WP-13's deferred question
///
/// The alternative was a little before it - a second of lead-in, so the needle
/// drop is audible - and it is wrong. A person skipping forward is looking for
/// the top of a track, and a skip that lands early means the first thing they
/// hear is the end of the previous one. [`BOUNDARY_CONTEXT_SECONDS`] exists for
/// the other job, auditioning a boundary to judge whether it is in the right
/// place, and that is where the context belongs.
///
/// Strictly past the position in both directions, so holding the key walks the
/// side rather than sticking on the mark under the playhead.
fn next_mark(marks: &[u64], from: u64, forward: bool) -> Option<u64> {
    if forward {
        marks.iter().copied().find(|mark| *mark > from)
    } else {
        marks.iter().copied().rev().find(|mark| *mark < from)
    }
}

/// How long the feeder thread sleeps when the queue is already full.
///
/// Short relative to the queue's depth - [`chunks::QUEUE_CHUNKS`] chunks of
/// [`chunks::CHUNK_MILLIS`] is 160 ms - so the queue is topped up many times
/// over before it could run dry.
const REFILL_IDLE: Duration = Duration::from_millis(5);

/// How long the feeder thread sleeps once the span has been read to the end.
///
/// Longer, because the only thing that can make it useful again is a seek, and
/// a seek is a human pressing something. [`idle`] is what keeps the length of
/// this from being the length of a seek.
const DRAINED_IDLE: Duration = Duration::from_millis(50);

/// Anything that stops playback starting, or ends it early.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The device could not be opened for output.
    #[error("opening the output device: {0}")]
    Audio(#[from] vcw_audio::Error),
    /// The project could not be opened or read.
    #[error("the project: {0}")]
    Project(#[from] vcw_project::Error),
    /// The output could not be written.
    #[error("writing the render: {0}")]
    Io(#[from] std::io::Error),
    /// There is no output device at all.
    ///
    /// Distinct from a named device that is missing, which
    /// [`vcw_audio::devices::Snapshot::find`] reports with the name in it.
    #[error("this machine has no audio output device")]
    NoOutput,
    /// The scope resolved to no audio.
    ///
    /// A region outside the capture, a boundary audition of a capture with no
    /// frames, or a track whose extent is empty. Refused rather than played,
    /// because a transport that starts and immediately ends looks like a fault.
    #[error("capture {capture_id} has nothing to play in {scope}")]
    Nothing {
        /// The capture asked for.
        capture_id: i64,
        /// The scope that resolved to nothing.
        scope: String,
    },
    /// The feeder thread ended without reporting an outcome.
    #[error("the playback feeder ended without reporting")]
    FeederLost,
}

/// A playback result.
pub type Result<T> = std::result::Result<T, Error>;

/// What is being auditioned. §21's four required targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The complete capture.
    Whole,
    /// A selected region, in frames.
    Region(Span),
    /// One track's extent.
    ///
    /// The span is supplied by the caller until WP-13 records boundaries, at
    /// which point it comes from the track table. Kept apart from
    /// [`Scope::Region`] even though the two resolve the same way, because the
    /// event a listener sees should say which one they asked for.
    Track {
        /// Which track this is, as the user counts them.
        number: u32,
        /// Its extent.
        span: Span,
    },
    /// A boundary, plus context either side of it.
    Boundary {
        /// The frame the boundary sits on.
        at: u64,
        /// Frames of lead-in.
        before: u64,
        /// Frames of lead-out.
        after: u64,
    },
}

impl Scope {
    /// A boundary audition with [`BOUNDARY_CONTEXT_SECONDS`] either side.
    #[must_use]
    pub fn boundary(rate: SampleRate, at: u64) -> Self {
        let context = frames_at(rate, BOUNDARY_CONTEXT_SECONDS);
        Self::Boundary {
            at,
            before: context,
            after: context,
        }
    }

    /// A region given in seconds.
    #[must_use]
    pub fn seconds(rate: SampleRate, start: f64, end: f64) -> Self {
        Self::Region(Span::from_seconds(rate, start, end))
    }

    /// The frames this scope covers, clamped to what the capture actually has.
    ///
    /// Clamping rather than refusing: a region selected on a waveform can end
    /// one frame past the last one recorded, and "plays to the end" is the only
    /// sensible reading of that.
    #[must_use]
    pub fn span(&self, layout: &Layout) -> Span {
        match self {
            Self::Whole => layout.span(),
            Self::Region(span) | Self::Track { span, .. } => span.clamp_to(layout.frames),
            Self::Boundary { at, before, after } => {
                Span::around(*at, *before, *after).clamp_to(layout.frames)
            }
        }
    }

    /// What was asked for, without needing the project open.
    ///
    /// [`Scope::describe`] answers in seconds, which needs a [`Layout`], which
    /// needs the project. The one moment that is not available is the moment
    /// most worth reporting: an audition refused because the project could not
    /// be read has no rate to divide by. So this answers in frames.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Whole => "the whole capture".to_owned(),
            Self::Region(span) => {
                format!("the region {}-{} in frames", span.start, span.end)
            }
            Self::Track { number, .. } => format!("track {number}"),
            Self::Boundary { at, .. } => format!("the boundary at frame {at}"),
        }
    }

    /// How to say what is playing, for [`Event::Auditioning`].
    #[must_use]
    pub fn describe(&self, layout: &Layout) -> String {
        let span = self.span(layout);
        let seconds = span.seconds(layout.rate);
        match self {
            Self::Whole => format!("the whole capture ({seconds:.3} s)"),
            Self::Region(_) => format!(
                "the region {:.3}-{:.3} s",
                span.start as f64 / f64::from(layout.rate.hz().max(1)),
                span.end as f64 / f64::from(layout.rate.hz().max(1))
            ),
            Self::Track { number, .. } => format!("track {number} ({seconds:.3} s)"),
            Self::Boundary { at, .. } => format!(
                "the boundary at {:.3} s (+/-{:.3} s)",
                *at as f64 / f64::from(layout.rate.hz().max(1)),
                seconds / 2.0
            ),
        }
    }
}

/// One of §21's transport operations, as a value.
///
/// A value rather than six methods because both drivers need to accept the same
/// six from a script - the CLI's `--script`, and the render path's cue list -
/// and a script of strings has to become something before it can be applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verb {
    /// `PLAY`.
    Play,
    /// `PAUSE`.
    Pause,
    /// `STOP`.
    Stop,
    /// `SEEK`, in seconds from the start of the capture.
    Seek(f64),
    /// `SKIP FORWARD`, by [`SKIP_SECONDS`].
    SkipForward,
    /// `SKIP BACK`, by [`SKIP_SECONDS`].
    SkipBack,
}

impl Verb {
    /// Parses one step of a script.
    ///
    /// `play`, `pause`, `stop`, `seek 12.5`, `skip-forward`, `skip-back`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut words = text.split_whitespace();
        let verb = words.next()?;
        let rest = words.next();
        match (verb, rest) {
            ("play", _) => Some(Self::Play),
            ("pause", _) => Some(Self::Pause),
            ("stop", _) => Some(Self::Stop),
            ("seek", Some(at)) => at.parse().ok().map(Self::Seek),
            ("skip-forward" | "forward", _) => Some(Self::SkipForward),
            ("skip-back" | "back", _) => Some(Self::SkipBack),
            _ => None,
        }
    }

    /// The name §21 gives it, lower-cased.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Stop => "stop",
            Self::Seek(_) => "seek",
            Self::SkipForward => "skip-forward",
            Self::SkipBack => "skip-back",
        }
    }
}

/// A transport move the render path makes at a known output position.
///
/// The live transport is driven by a clock and the render path is not, so a
/// render needs its moves scheduled against the only thing it has: how much
/// audio it has produced. Cueing by frame also makes a render repeatable, which
/// is the entire reason it exists.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    /// Apply the verb once this many frames have been written.
    pub after_frames: u64,
    /// What to do.
    pub verb: Verb,
}

/// Reads a span, converts it, and hands over device-format bytes.
///
/// The one piece both drivers share. It holds no queue and no thread, so the
/// live feeder and the synchronous render differ only in what they do with what
/// it returns.
pub struct Pump<'a> {
    /// Where the frames come from.
    reader: Reader<'a>,
    /// What has to happen to them on the way out.
    conversion: Conversion,
    /// Stored-format bytes, in flight. Allocated once.
    staging: Vec<u8>,
    /// How much of `staging` the last [`Pump::fill`] read, so the feeder can
    /// meter what it just sent without reading the blocks a second time.
    staged: usize,
}

impl<'a> Pump<'a> {
    /// Opens a pump over one capture's span.
    ///
    /// `chunk_bytes` is the largest output buffer it will be asked to fill; the
    /// staging buffer is sized from it once and never grows.
    ///
    /// # Errors
    ///
    /// If the capture is not in the project, or its blocks cannot be read.
    pub fn open(
        conn: &'a Connection,
        capture_id: i64,
        span: Span,
        conversion: Conversion,
        chunk_bytes: usize,
    ) -> Result<Self> {
        let reader = Reader::open(conn, capture_id, span)?;
        let frames = chunk_bytes / conversion.to_frame_bytes().max(1);
        let staging = vec![0u8; frames.max(1) * conversion.from_frame_bytes().max(1)];
        Ok(Self {
            reader,
            conversion,
            staging,
            staged: 0,
        })
    }

    /// The frame the next [`Pump::fill`] will start at.
    #[must_use]
    pub const fn position(&self) -> u64 {
        self.reader.position()
    }

    /// What is being played.
    #[must_use]
    pub const fn layout(&self) -> &Layout {
        self.reader.layout()
    }

    /// Moves the read position. Clamped into the span.
    pub fn seek(&mut self, frame: u64) {
        self.reader.seek(frame);
    }

    /// The stored-format bytes the last [`Pump::fill`] read.
    ///
    /// Stored format rather than the device's, because that is what
    /// [`vcw_signal::meter::Meter`] is configured for and it is the same audio:
    /// playback applies no gain, so the only thing the conversion changes is
    /// the width of the samples and - on a mono capture through a stereo
    /// device - how many copies of them there are. Neither moves a needle.
    #[must_use]
    pub fn staged(&self) -> &[u8] {
        &self.staging[..self.staged]
    }

    /// Fills `dst` with device-format bytes and returns how many it wrote.
    ///
    /// Zero means the span has been read to the end. Whole frames only, in both
    /// formats: a partial frame handed to a device is a channel swap for the
    /// rest of the stream.
    ///
    /// # Errors
    ///
    /// If a block the span covers is missing or the wrong size.
    pub fn fill(&mut self, dst: &mut [u8]) -> Result<usize> {
        let frames = dst.len() / self.conversion.to_frame_bytes().max(1);
        let want = (frames * self.conversion.from_frame_bytes()).min(self.staging.len());
        if want == 0 {
            return Ok(0);
        }
        let read = self.reader.fill(&mut self.staging[..want])?;
        self.staged = read;
        if read == 0 {
            return Ok(0);
        }
        // The units change here and it matters: `Reader::fill` counts bytes in
        // the stored format, `Conversion::apply` counts frames, and a chunk is
        // marked in bytes of the device's format. Two of those three are the
        // same number only by coincidence.
        let converted = self.conversion.apply(&self.staging[..read], dst);
        Ok(converted * self.conversion.to_frame_bytes())
    }
}

/// What one pass of the feeder achieved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Progress {
    /// A chunk was filled and queued.
    Filled,
    /// The queue is full; nothing to do until the callback takes one.
    Full,
    /// The span has been read to the end.
    Drained,
}

/// Fills one chunk, if there is one spare and there is audio left.
fn step(pump: &mut Pump<'_>, feeder: &mut Feeder, epoch: u64) -> Result<Progress> {
    let Some(mut chunk) = feeder.take() else {
        return Ok(Progress::Full);
    };
    let start = pump.position();
    let filled = match pump.fill(chunk.spare_mut()) {
        Ok(filled) => filled,
        Err(error) => {
            feeder.give_back(chunk);
            return Err(error);
        }
    };
    if filled == 0 {
        feeder.give_back(chunk);
        return Ok(Progress::Drained);
    }
    chunk.mark(epoch, start, filled);
    if let Err(returned) = feeder.send(chunk) {
        // Unreachable while `take` only yields a chunk when a slot is free, but
        // rewinding rather than trusting that costs one indexed lookup and
        // means a future change to the queue cannot silently drop audio.
        feeder.give_back(returned);
        pump.seek(start);
        return Ok(Progress::Full);
    }
    Ok(Progress::Filled)
}

/// Everything the feeder thread needs to know that the cursor does not tell it.
///
/// A struct rather than five parameters because it crosses a thread boundary
/// and has to be moved into the closure as one thing anyway.
struct Job {
    /// The project to read.
    path: PathBuf,
    /// The capture within it.
    capture_id: i64,
    /// The span to read.
    span: Span,
    /// What the samples need on the way out.
    conversion: Conversion,
    /// How to meter what is played: the capture's own rate, channels and
    /// format, which is what the feeder reads before the conversion.
    metering: Config,
}

/// Keeps the queue full until the stop flag is set or the stream goes away.
///
/// Runs on its own thread, owns the project connection, and publishes nothing
/// except when it has to refuse.
/// Sleeps, but not through a seek.
///
/// A plain sleep here is what makes a seek cost a buffer of silence: the feeder
/// has a callback's worth of audio to queue and 80 ms to do it in, and it
/// cannot start while it is asleep for 50 of them. Waking every
/// [`REFILL_IDLE`] to look at the epoch costs a few hundred no-op wakeups a
/// second on an idle transport, which is less than the stream itself costs.
fn idle(cursor: &Cursor, epoch: u64, how_long: Duration) {
    let until = Instant::now() + how_long;
    let slice = REFILL_IDLE.min(how_long);
    loop {
        thread::sleep(slice);
        if cursor.epoch() != epoch || Instant::now() >= until {
            return;
        }
    }
}

fn feeding(
    job: &Job,
    mut feeder: Feeder,
    cursor: &Cursor,
    stop: &AtomicBool,
    bus: &Bus,
) -> Result<u64> {
    let project = Project::open_read_only(&job.path)?;
    let mut pump = Pump::open(
        project.conn(),
        job.capture_id,
        job.span,
        job.conversion.clone(),
        feeder.chunk_bytes(),
    )?;
    let mut epoch = cursor.epoch();
    pump.seek(cursor.frame());
    let mut fed = 0u64;

    // The output meter. Metering here rather than in the device callback is
    // §10's rule applied to playback: nothing that watches the audio may cost
    // it a frame, and the callback is the one place where it could.
    //
    // ponytail: the feeder runs ahead of the device by whatever the queue
    // holds, so these levels lead what is audible by up to a buffer or two -
    // around 80 ms at the default queue depth. For a level display that is
    // under a frame of video and nobody can see it. If a needle ever has to be
    // sample-accurate against the sound, the meter moves to a tap on the
    // chunk the callback has just handed back, which needs a ring the callback
    // can write to without allocating.
    let mut meter = Meter::new(job.metering);
    let mut due = Instant::now() + metering::INTERVAL;
    let mut reported = 0;

    while !stop.load(Ordering::Relaxed) && !feeder.is_abandoned() {
        let now = cursor.epoch();
        if now != epoch {
            // A seek. Everything already queued belongs to the old epoch and
            // the callback will discard it unplayed, so there is nothing to
            // clear from this side - which is the whole reason the queue is
            // epoch-tagged rather than a byte ring.
            epoch = now;
            pump.seek(cursor.frame());
            // The queue is now full of audio nobody will hear, and every empty
            // chunk with it. The reserve is the only thing that can be filled
            // before the next callback arrives, so spend it.
            feeder.release_reserve();
        }
        match step(&mut pump, &mut feeder, epoch) {
            Ok(Progress::Filled) => {
                fed += 1;
                meter.feed(pump.staged());
            }
            Ok(Progress::Full) => idle(cursor, epoch, REFILL_IDLE),
            Ok(Progress::Drained) => {
                cursor.mark_drained(epoch);
                idle(cursor, epoch, DRAINED_IDLE);
            }
            Err(error) => {
                // Refusing is the correct end of playback, but the listener is
                // owed a reason, and the callback is owed an end: without this
                // it would count underruns for ever.
                bus.publish(&Event::Warning {
                    code: "playback-unreadable",
                    detail: error.to_string(),
                });
                cursor.mark_drained(epoch);
                return Err(error);
            }
        }

        // Nothing new means nothing to say, for the same reason the capture
        // meter stays quiet on an empty tick: reading a snapshot resets the
        // instantaneous peak, so a tick that found no audio would report a
        // silence the stream never contained.
        if Instant::now() >= due && meter.frames() > reported {
            reported = meter.frames();
            bus.publish(&Event::Output {
                levels: meter.snapshot(),
            });
            due = Instant::now() + metering::INTERVAL;
        }
    }
    Ok(fed)
}

/// What to audition, and how.
///
/// The playback counterpart of [`Setup`](crate::commands::Setup), and shared by
/// both drivers: [`render`] takes the same description and ignores the fields
/// that only mean something to a device.
#[derive(Clone, Debug)]
pub struct Audition {
    /// The project to read.
    pub project: PathBuf,
    /// The capture within it.
    pub capture_id: i64,
    /// Which of §21's four targets.
    pub scope: Scope,
    /// The output device, or `None` for the system default.
    pub device: Option<String>,
    /// A stream format to insist on, or `None` to let playback pick the one
    /// that converts least.
    pub format: Option<SampleFormat>,
    /// Exclusive or shared access to the device.
    pub mode: vcw_types::CaptureMode,
    /// How deep the chunk queue is, in chunks.
    pub queue_chunks: usize,
    /// Frames a skip jumps between, ascending (§21).
    ///
    /// Both ends of every track, normally - which is what
    /// [`vcw_project::track::edges_of_capture`] returns, and what the shell and
    /// the CLI both hand in. Empty means a skip is [`SKIP_SECONDS`] instead,
    /// which is what an unanalysed side gets. They are frames and not boundary
    /// ids on purpose: playback has no business opening the project a second
    /// time to find out where a row is, and a caller that has already read the
    /// tracks has the numbers in hand.
    pub marks: Vec<u64>,
}

impl Audition {
    /// The whole of one capture, on the default device, exclusively.
    #[must_use]
    pub fn new(project: impl Into<PathBuf>, capture_id: i64) -> Self {
        Self {
            project: project.into(),
            capture_id,
            scope: Scope::Whole,
            device: None,
            format: None,
            mode: vcw_types::CaptureMode::Exclusive,
            queue_chunks: chunks::QUEUE_CHUNKS,
            marks: Vec::new(),
        }
    }

    /// Names the frames a skip jumps between.
    ///
    /// Sorted and deduplicated here rather than trusted, because the caller's
    /// natural source is a track list and two tracks that share a boundary
    /// would otherwise make a skip that appears to do nothing.
    #[must_use]
    pub fn marks(mut self, marks: impl IntoIterator<Item = u64>) -> Self {
        let mut frames: Vec<u64> = marks.into_iter().collect();
        frames.sort_unstable();
        frames.dedup();
        self.marks = frames;
        self
    }

    /// Narrows it to one of §21's targets.
    #[must_use]
    pub fn scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// Names a device.
    #[must_use]
    pub fn device(mut self, device: impl Into<String>) -> Self {
        self.device = Some(device.into());
        self
    }
}

/// How an audition went.
#[derive(Clone, Debug)]
pub struct Played {
    /// The capture that was playing.
    pub capture_id: i64,
    /// What was auditioned.
    pub span: Span,
    /// How the stream was actually running.
    pub opened: Opened,
    /// What the callback counted.
    pub health: Health,
    /// Whether the audio that reached the converter was the audio in the
    /// project, and on what evidence.
    pub fidelity: Fidelity,
}

impl Played {
    /// Whether playback was bit-perfect, as a confirmation rather than an
    /// absence of doubt.
    #[must_use]
    pub const fn bit_perfect(&self) -> bool {
        self.fidelity.is_confirmed()
    }

    /// Whether the listener heard the audio without a gap in it.
    #[must_use]
    pub const fn was_gapless(&self) -> bool {
        self.health.underruns == 0
    }
}

/// The live transport: an output stream and a thread keeping it fed.
///
/// Not `Send`. A `cpal::Stream` is not, so the thread that opens a player is
/// the thread that keeps it - the same constraint that gave
/// [`Engine`](crate::engine::Engine) its thread, for the same reason.
pub struct Player {
    /// The output stream and everything the device told us.
    playback: Playback,
    /// Shared with the callback and the feeder.
    cursor: Arc<Cursor>,
    /// What is being auditioned.
    span: Span,
    /// The rate the capture plays at, which is the only rate it plays at.
    rate: SampleRate,
    /// The capture being played.
    capture_id: i64,
    /// `SKIP FORWARD` and `SKIP BACK` in frames, when there are no marks.
    skip: u64,
    /// Frames a skip jumps between, ascending. See [`Audition::marks`].
    marks: Vec<u64>,
    /// Asks the feeder to finish.
    stop: Arc<AtomicBool>,
    /// The feeder thread, joined on [`Player::stop`].
    feeder: Option<JoinHandle<std::result::Result<u64, Error>>>,
    /// Where events go.
    bus: Bus,
    /// The last playhead published, so [`Player::tick`] can stay quiet when
    /// nothing has moved.
    published: Cell<u64>,
    /// Whether the end has already been announced.
    announced: Cell<bool>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("capture_id", &self.capture_id)
            .field("span", &self.span)
            .field("position", &self.position())
            .finish_non_exhaustive()
    }
}

impl Player {
    /// Opens the device, starts the feeder, and leaves the transport paused.
    ///
    /// Paused, not playing: §50's workflow has the operator pick a region and
    /// then press play, and a player that started the moment it opened would
    /// have played the first 160 ms of the wrong thing on the way to being told
    /// what to play.
    ///
    /// # Errors
    ///
    /// If the capture is not in the project, the scope covers no audio, there
    /// is no output device, or the device cannot play the capture's rate. That
    /// last one is a refusal rather than a resampling: see
    /// [`vcw_audio::Error::RateUnavailable`].
    pub fn open(audition: &Audition, bus: &Bus) -> Result<Self> {
        let layout = {
            let project = Project::open_read_only(&audition.project)?;
            Layout::of(project.conn(), audition.capture_id)?
        };
        let span = audition.scope.span(&layout);
        let described = audition.scope.describe(&layout);
        if span.is_empty() {
            return Err(Error::Nothing {
                capture_id: audition.capture_id,
                scope: described,
            });
        }

        let snapshot = devices::enumerate();
        let report = match &audition.device {
            Some(name) => snapshot.find(name, Direction::Output)?,
            None => snapshot
                .default_for(Direction::Output)
                .ok_or(Error::NoOutput)?,
        };

        let material = Material {
            rate: layout.rate,
            channels: layout.channels,
            format: layout.format,
        };
        let mut request = Request::new(report.key.clone(), material).mode(audition.mode);
        request.queue_chunks = audition.queue_chunks;
        if let Some(format) = audition.format {
            request = request.format(format);
        }
        let (playback, feeder) = Playback::open_on(report, &request)?;

        let cursor = Arc::clone(playback.cursor());
        cursor.set_frame(span.start);
        let conversion = playback.conversion().clone();
        let stop = Arc::new(AtomicBool::new(false));

        let thread = {
            let job = Job {
                path: audition.project.clone(),
                capture_id: audition.capture_id,
                span,
                conversion,
                metering: Config::new(layout.channels as usize, layout.rate.0, layout.format),
            };
            let cursor = Arc::clone(&cursor);
            let stop = Arc::clone(&stop);
            let bus = bus.clone();
            thread::Builder::new()
                .name("vcw-playback-feeder".into())
                .spawn(move || feeding(&job, feeder, &cursor, &stop, &bus))
                .map_err(Error::Io)?
        };

        let player = Self {
            span,
            rate: layout.rate,
            capture_id: audition.capture_id,
            skip: frames_at(layout.rate, SKIP_SECONDS).max(1),
            marks: audition.marks.clone(),
            cursor,
            stop,
            feeder: Some(thread),
            bus: bus.clone(),
            published: Cell::new(u64::MAX),
            announced: Cell::new(false),
            playback,
        };
        player.announce(&described);
        Ok(player)
    }

    /// Says what was opened and what it will cost the samples (§35).
    fn announce(&self, scope: &str) {
        let opened = self.playback.opened();
        self.bus.publish(&Event::Auditioning {
            capture_id: self.capture_id,
            scope: scope.to_string(),
            opened: format!(
                "{} at {} Hz, {} channel(s), {:?}, {:?}, buffer {}",
                self.playback.device_name(),
                opened.rate.hz(),
                opened.channels,
                opened.format,
                opened.mode,
                opened.buffer
            ),
            conversion: self.playback.conversion().to_string(),
            divergences: opened.divergences.iter().map(ToString::to_string).collect(),
        });
    }

    /// What is being auditioned.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// How the stream is actually running.
    #[must_use]
    pub const fn opened(&self) -> &Opened {
        self.playback.opened()
    }

    /// The frame now playing.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.playback.position()
    }

    /// The epoch the transport is asking to hear.
    ///
    /// Bumped by every seek. Paired with [`Player::delivered_epoch`] it is how
    /// a caller tells "the playhead has moved" from "the device is playing the
    /// new position", which are not the same instant and differ by the buffer.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.cursor.epoch()
    }

    /// The epoch the device has actually been given audio from.
    #[must_use]
    pub fn delivered_epoch(&self) -> u64 {
        self.cursor.delivered()
    }

    /// Whether the audio the device is playing is from the current position.
    ///
    /// False for as long as a seek is still in flight.
    #[must_use]
    pub fn has_joined(&self) -> bool {
        self.cursor.delivered() == self.cursor.epoch()
    }

    /// Whether the transport is running.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.cursor.is_playing()
    }

    /// Whether the span has been played to its end.
    #[must_use]
    pub fn has_ended(&self) -> bool {
        self.cursor.has_ended()
    }

    /// `PLAY`.
    ///
    /// # Errors
    ///
    /// If the device refuses to start.
    pub fn play(&self) -> Result<()> {
        self.cursor.set_playing(true);
        self.playback.play()?;
        Ok(())
    }

    /// `PAUSE`.
    ///
    /// Stops the stream where the backend allows it, so a paused transport
    /// costs no CPU, and sets the flag either way, so one that does not allow
    /// it goes silent without consuming what is queued.
    ///
    /// # Errors
    ///
    /// If the device refuses to pause.
    pub fn pause(&self) -> Result<()> {
        self.cursor.set_playing(false);
        self.playback.pause()?;
        Ok(())
    }

    /// `SEEK`, in frames, clamped into the span.
    ///
    /// Returns the frame it landed on. Audio already queued for the old
    /// position is discarded unplayed rather than heard.
    pub fn seek(&self, frame: u64) -> u64 {
        let landed = frame.clamp(self.span.start, self.span.end.saturating_sub(1));
        self.cursor.seek(landed);
        landed
    }

    /// `SEEK`, in seconds from the start of the capture.
    pub fn seek_seconds(&self, seconds: f64) -> u64 {
        self.seek(frames_at(self.rate, seconds))
    }

    /// `SKIP FORWARD`: to the next mark, or by [`SKIP_SECONDS`].
    pub fn skip_forward(&self) -> u64 {
        let from = self.position();
        self.seek(
            next_mark(&self.marks, from, true).unwrap_or_else(|| from.saturating_add(self.skip)),
        )
    }

    /// `SKIP BACK`: to the previous mark, or by [`SKIP_SECONDS`].
    pub fn skip_back(&self) -> u64 {
        let from = self.position();
        self.seek(
            next_mark(&self.marks, from, false).unwrap_or_else(|| from.saturating_sub(self.skip)),
        )
    }

    /// Applies one of §21's operations.
    ///
    /// [`Verb::Stop`] is the one this cannot carry out, because stopping
    /// consumes the player; it returns `false` so the caller knows to.
    ///
    /// # Errors
    ///
    /// If the device refuses to start or pause.
    pub fn apply(&self, verb: Verb) -> Result<bool> {
        match verb {
            Verb::Play => self.play()?,
            Verb::Pause => self.pause()?,
            Verb::Seek(seconds) => {
                self.seek_seconds(seconds);
            }
            Verb::SkipForward => {
                self.skip_forward();
            }
            Verb::SkipBack => {
                self.skip_back();
            }
            Verb::Stop => return Ok(false),
        }
        Ok(true)
    }

    /// Publishes the playhead if it has moved, and the end if it has arrived.
    ///
    /// Called by whatever is driving the transport. The player does not own a
    /// clock: a CLI polls this in its loop and the Tauri shell will poll it on
    /// its own timer, and neither wants a thread it did not ask for.
    pub fn tick(&self) {
        let frame = self.position();
        if frame != self.published.get() {
            self.published.set(frame);
            self.bus.publish(&Event::Playhead {
                frame,
                seconds: frame as f64 / f64::from(self.rate.hz().max(1)),
            });
        }
        if self.has_ended() && !self.announced.get() {
            self.announced.set(true);
            self.cursor.set_playing(false);
        }
    }

    /// `STOP`. Closes the stream, joins the feeder, and reports.
    ///
    /// The health counters and the fidelity verdict are read before the stream
    /// closes, because a closed stream cannot be asked anything.
    #[must_use]
    pub fn stop(mut self) -> Played {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.feeder.take() {
            // A feeder that failed has already published its reason; there is
            // nothing further to tell the operator here, and taking the process
            // down over it would lose the health report.
            let _ = thread.join();
        }
        let played = Played {
            capture_id: self.capture_id,
            span: self.span,
            opened: self.playback.opened().clone(),
            fidelity: self.playback.fidelity(),
            health: self.playback.health(),
        };
        self.bus.publish(&Event::Ended {
            capture_id: played.capture_id,
            frames: played.health.frames,
            underruns: played.health.underruns,
            fidelity: played.fidelity.summary(),
            bit_perfect: played.bit_perfect(),
        });
        let _ = self.playback.stop();
        played
    }
}

// There is deliberately no `Drop` for `Player`. Dropping one - a `?` above it,
// or a panic - drops the stream, which drops the `Source`, which drops the
// `Drain`; the feeder sees [`Feeder::is_abandoned`] and finishes on its own
// within one idle period. `Player::stop` joins it explicitly because a caller
// that asked for a report should not get one while a thread is still reading
// the project, but nothing depends on that happening.

/// What a render produced.
#[derive(Clone, Debug)]
pub struct Rendered {
    /// The capture that was rendered.
    pub capture_id: i64,
    /// What was rendered.
    pub span: Span,
    /// The rate the frames are at, which is the capture's own rate.
    pub rate: SampleRate,
    /// Channels per frame.
    pub channels: u16,
    /// The format the frames are in.
    pub format: SampleFormat,
    /// What had to happen to the stored samples, as a line of text.
    pub conversion: String,
    /// Frames written.
    pub frames: u64,
    /// Bytes written.
    pub bytes: u64,
    /// What the callback counted while it was being driven.
    pub health: Health,
    /// Every transport move that was carried out, and where it landed.
    pub applied: Vec<Applied>,
}

/// A transport move a render actually made.
///
/// A cue says "seek once 4,000 frames have been written"; the transport can
/// only act between callbacks, so it acts at the first period boundary at or
/// after that. `after` is where it really happened. Reported rather than
/// rounded away, because the difference is the granularity of a seek and a
/// caller comparing rendered bytes needs the real number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Applied {
    /// What was done.
    pub verb: Verb,
    /// Frames written before it happened.
    pub after: u64,
    /// The frame the playhead was moved to.
    pub landed: u64,
}

impl Rendered {
    /// The rendered audio's duration.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.rate.hz().max(1))
    }
}

/// Plays a span through the whole chain with no device, writing the result.
///
/// Everything the live path has is here except the sound card: the same
/// [`Pump`], the same epoch-tagged queue, the same [`Source::on_data`]
/// callback. The bytes written are the bytes the converter would have been
/// handed, which is what makes a gapless seek something a test can assert
/// rather than something a listener has to vouch for.
///
/// Silence is not written. A callback that starves produces a period of zeros
/// for the device, and those zeros are real to a listener - but in a file they
/// would be padding indistinguishable from recorded silence, so only the audio
/// is kept and the gap is reported in [`Rendered::health`] instead. With the
/// queue topped up before every callback the only thing that can starve is a
/// seek, which costs exactly one period while the invalidated chunks are
/// reclaimed, and the end of the span, which costs nothing.
///
/// [`Verb::Seek`], [`Verb::SkipForward`] and [`Verb::SkipBack`] apply as they
/// do live. [`Verb::Play`] and [`Verb::Pause`] are ignored, because a render has
/// no clock for them to act against. [`Verb::Stop`] ends it.
///
/// # Errors
///
/// If the capture is not in the project, the scope covers no audio, a block is
/// missing, or the output cannot be written.
pub fn render(audition: &Audition, cues: &[Cue], out: &mut dyn Write) -> Result<Rendered> {
    let project = Project::open_read_only(&audition.project)?;
    let layout = Layout::of(project.conn(), audition.capture_id)?;
    let span = audition.scope.span(&layout);
    if span.is_empty() {
        return Err(Error::Nothing {
            capture_id: audition.capture_id,
            scope: audition.scope.describe(&layout),
        });
    }

    let format = audition
        .format
        .unwrap_or_else(|| convert::natural(layout.format));
    let conversion = Conversion::new(layout.format, layout.channels, format, layout.channels);
    let frame_bytes = conversion.to_frame_bytes();
    let chunk_bytes = chunks::chunk_bytes(frame_bytes, layout.rate.hz());
    let (mut feeder, drain) = chunks::queue(chunk_bytes, audition.queue_chunks.max(2));

    let cursor = Arc::new(Cursor::at(span.start));
    cursor.set_playing(true);
    let counters = Arc::new(Counters::default());
    let mut source = Source::new(
        drain,
        Arc::clone(&cursor),
        Arc::clone(&counters),
        frame_bytes,
    );
    let mut pump = Pump::open(
        project.conn(),
        audition.capture_id,
        span,
        conversion.clone(),
        chunk_bytes,
    )?;
    pump.seek(span.start);

    let mut period = vec![0u8; chunk_bytes];
    let mut epoch = cursor.epoch();
    let mut frames = 0u64;
    let mut bytes = 0u64;
    let mut applied: Vec<Applied> = Vec::new();
    let mut pending = cues.iter();
    let mut next = pending.next();
    // A cue list that seeks backwards could otherwise be asked to render for
    // ever. Each cue fires once, so the audio a render can produce is bounded
    // by the span repeated once per cue, and the extra second is slack for the
    // period the last seek lands mid-way through.
    let ceiling = span
        .frames()
        .saturating_mul(cues.len() as u64 + 1)
        .saturating_add(u64::from(layout.rate.hz()));

    loop {
        while let Some(cue) = next {
            if cue.after_frames > frames {
                break;
            }
            next = pending.next();
            if cue.verb == Verb::Stop {
                applied.push(Applied {
                    verb: cue.verb,
                    after: frames,
                    landed: cursor.frame(),
                });
                return finish(
                    audition,
                    &layout,
                    span,
                    format,
                    &conversion,
                    frames,
                    bytes,
                    &counters,
                    applied,
                );
            }
            let landed = match cue.verb {
                Verb::Seek(seconds) => Some(frames_at(layout.rate, seconds)),
                // The same rule as the device path, and it has to be: a render
                // is how a skip is tested without a sound card, and a driver
                // that skipped differently would make that test worthless.
                Verb::SkipForward => Some(
                    next_mark(&audition.marks, cursor.frame(), true).unwrap_or_else(|| {
                        cursor
                            .frame()
                            .saturating_add(frames_at(layout.rate, SKIP_SECONDS).max(1))
                    }),
                ),
                Verb::SkipBack => Some(
                    next_mark(&audition.marks, cursor.frame(), false).unwrap_or_else(|| {
                        cursor
                            .frame()
                            .saturating_sub(frames_at(layout.rate, SKIP_SECONDS).max(1))
                    }),
                ),
                Verb::Play | Verb::Pause | Verb::Stop => None,
            };
            let landed = match landed {
                Some(to) => {
                    let to = to.clamp(span.start, span.end.saturating_sub(1));
                    cursor.seek(to);
                    to
                }
                None => cursor.frame(),
            };
            applied.push(Applied {
                verb: cue.verb,
                after: frames,
                landed,
            });
        }

        loop {
            let now = cursor.epoch();
            if now != epoch {
                epoch = now;
                pump.seek(cursor.frame());
            }
            match step(&mut pump, &mut feeder, epoch)? {
                Progress::Filled => {}
                Progress::Full => break,
                Progress::Drained => {
                    cursor.mark_drained(epoch);
                    break;
                }
            }
        }

        let before = counters.snapshot().frames;
        source.on_data(&mut period);
        let played = counters.snapshot().frames - before;
        if played > 0 {
            let written = played as usize * frame_bytes;
            out.write_all(&period[..written])?;
            frames += played;
            bytes += written as u64;
        }
        if cursor.has_ended() || frames > ceiling {
            break;
        }
    }
    out.flush()?;
    finish(
        audition,
        &layout,
        span,
        format,
        &conversion,
        frames,
        bytes,
        &counters,
        applied,
    )
}

/// Assembles a [`Rendered`]. Split out because the cue list can end a render
/// early and both endings owe the caller the same report.
#[allow(clippy::too_many_arguments)]
fn finish(
    audition: &Audition,
    layout: &Layout,
    span: Span,
    format: SampleFormat,
    conversion: &Conversion,
    frames: u64,
    bytes: u64,
    counters: &Counters,
    applied: Vec<Applied>,
) -> Result<Rendered> {
    Ok(Rendered {
        capture_id: audition.capture_id,
        span,
        rate: layout.rate,
        channels: layout.channels,
        format,
        conversion: conversion.to_string(),
        frames,
        bytes,
        health: counters.snapshot(),
        applied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcw_project::persistence::{Config, Writer};
    use vcw_types::{CaptureInfo, CaptureMode, CaptureState, StorageFormat};

    const RATE: SampleRate = SampleRate(48_000);
    /// Frames in one second, so the test spans read as times.
    const SECOND: u64 = 48_000;
    const CHANNELS: u16 = 2;
    const WIDTH: usize = 4;
    const FRAME: usize = WIDTH * CHANNELS as usize;

    /// Stored as `Int32`, which [`convert::natural`] plays as `S32` with no
    /// conversion at all. That is the point: with an identity conversion the
    /// rendered bytes are the stored bytes, so a comparison against what was
    /// written tests the whole chain rather than the conversion's own idea of
    /// itself.
    fn info() -> CaptureInfo {
        CaptureInfo {
            rate: RATE,
            channels: CHANNELS,
            storage_format: StorageFormat::Int32,
            capture_mode: CaptureMode::Exclusive,
            host_api: Some("ALSA".into()),
            device_id: Some("hw:CARD=0,DEV=0".into()),
            device_name: Some("Cirrus Analog".into()),
            os_verified: false,
            os_report: None,
            eq: vcw_types::CaptureEq::Unknown,
        }
    }

    /// Every byte is a function of its own offset.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    /// A project holding one capture of `frames` frames, and the bytes in it.
    fn recorded(dir: &tempfile::TempDir, frames: usize) -> (PathBuf, i64, Vec<u8>) {
        let path = dir.path().join("play.vcw");
        let project = Project::create(&path).expect("create");
        let mut writer = Writer::begin(project, &info(), Config::default()).expect("begin");
        let sent = pattern(frames * FRAME);
        writer.push(&sent).expect("push");
        let (outcome, project, _) = writer
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");
        project.close().expect("close");
        (path, outcome.capture_id, sent)
    }

    /// Renders a scope to a `Vec`, with no cues.
    fn rendered(path: &PathBuf, capture_id: i64, scope: Scope) -> (Vec<u8>, Rendered) {
        let mut out = Vec::new();
        let audition = Audition::new(path, capture_id).scope(scope);
        let report = render(&audition, &[], &mut out).expect("render");
        (out, report)
    }

    #[test]
    fn a_render_is_the_frames_that_were_recorded() {
        // The whole chain end to end: per-channel blocks reassembled into
        // frames, through an identity conversion, through the epoch-tagged
        // queue, through the real callback. If any of those five stages
        // reorders a byte this fails and says where.
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, sent) = recorded(&dir, 5_000);
        let (out, report) = rendered(&path, capture_id, Scope::Whole);

        assert_eq!(report.frames, 5_000);
        assert_eq!(out.len(), sent.len());
        assert_eq!(out, sent, "playback did not return what was recorded");
        assert_eq!(report.health.underruns, 0);
        assert!(report.conversion.contains("straight through"));
    }

    #[test]
    fn a_seek_joins_without_a_gap_and_without_repeating() {
        // §21's exit criterion. The assertion is not "it sounded fine": it is
        // that the bytes handed to the converter are the bytes before the seek
        // followed by the bytes after it, with nothing dropped at the join and
        // nothing played twice.
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, sent) = recorded(&dir, 10 * SECOND as usize);
        let to = 6 * SECOND;

        let mut out = Vec::new();
        let audition = Audition::new(&path, capture_id);
        let cues = [Cue {
            after_frames: 2 * SECOND,
            verb: Verb::Seek(6.0),
        }];
        let report = render(&audition, &cues, &mut out).expect("render");

        // A transport can only move between callbacks, so the cue lands at the
        // first period boundary at or after the frame it names, and the report
        // says which. Working the expectation out from that rather than from
        // the cue is the difference between testing the seek and testing the
        // period size.
        assert_eq!(report.applied.len(), 1);
        let join = report.applied[0];
        assert_eq!(join.landed, to);
        assert!(join.after >= 2 * SECOND);

        let mut expected = sent[..join.after as usize * FRAME].to_vec();
        expected.extend_from_slice(&sent[to as usize * FRAME..]);
        assert_eq!(out.len(), expected.len(), "the join lost or repeated audio");
        assert_eq!(out, expected, "the seek did not land where it said");

        // Not one sample of silence. The callback discards the invalidated
        // chunks and recycles them in the same pass, so the feeder has buffers
        // to refill before the next callback asks for audio - which is the
        // whole argument for tagging chunks with an epoch instead of clearing
        // a ring. What this cannot measure is the wall-clock delay a real
        // device adds before the new position reaches the converter; that is
        // measured on hardware and stated in `docs/STATUS.md`.
        //
        // The silence that is counted is the last callback after the span ran
        // out, which is the end of the audio and not a gap in it - the
        // distinction [`Cursor::is_drained`] exists to make.
        assert_eq!(report.health.underruns, 0, "the seek left a gap");
        assert!(report.health.stale_chunks > 0, "nothing was discarded");
    }

    #[test]
    fn a_seek_backwards_replays_from_where_it_landed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, sent) = recorded(&dir, 6 * SECOND as usize);
        let mut out = Vec::new();
        let cues = [Cue {
            after_frames: 4 * SECOND,
            verb: Verb::Seek(1.0),
        }];
        let report = render(&Audition::new(&path, capture_id), &cues, &mut out).expect("render");

        let join = report.applied[0];
        assert_eq!(join.landed, SECOND);
        let mut expected = sent[..join.after as usize * FRAME].to_vec();
        expected.extend_from_slice(&sent[SECOND as usize * FRAME..]);
        assert_eq!(out, expected);
    }

    #[test]
    fn the_four_audition_scopes_each_play_their_own_extent() {
        // §21 requires all four targets. They differ only in the span they
        // resolve to, and this is the test that says so.
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, sent) = recorded(&dir, 12 * SECOND as usize);
        let at = |frame: u64| frame as usize * FRAME;

        let (whole, _) = rendered(&path, capture_id, Scope::Whole);
        assert_eq!(whole.len(), sent.len());

        let region = Span::new(2 * SECOND, 5 * SECOND);
        let (played, report) = rendered(&path, capture_id, Scope::Region(region));
        assert_eq!(report.frames, 3 * SECOND);
        assert_eq!(played, sent[at(2 * SECOND)..at(5 * SECOND)]);

        let (track, report) = rendered(
            &path,
            capture_id,
            Scope::Track {
                number: 2,
                span: Span::new(6 * SECOND, 8 * SECOND),
            },
        );
        assert_eq!(report.frames, 2 * SECOND);
        assert_eq!(track, sent[at(6 * SECOND)..at(8 * SECOND)]);

        let boundary_at = 8 * SECOND;
        let context = frames_at(RATE, BOUNDARY_CONTEXT_SECONDS);
        let (boundary, report) = rendered(&path, capture_id, Scope::boundary(RATE, boundary_at));
        assert_eq!(report.frames, context * 2);
        assert_eq!(
            boundary,
            sent[at(boundary_at - context)..at(boundary_at + context)]
        );
    }

    #[test]
    fn a_region_sounds_the_same_played_alone_as_it_does_in_the_side() {
        // The property that makes region audition trustworthy, and the one a
        // block-boundary bug would break: a region starting mid-block has to
        // begin at the frame it names, not at the block that holds it.
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, _) = recorded(&dir, 30_000);
        let (whole, _) = rendered(&path, capture_id, Scope::Whole);
        for start in [1u64, 999, 4_097, 16_385] {
            let span = Span::new(start, start + 3_333);
            let (region, _) = rendered(&path, capture_id, Scope::Region(span));
            assert_eq!(
                region,
                whole[start as usize * FRAME..(start as usize + 3_333) * FRAME],
                "the region from frame {start} is not that part of the side"
            );
        }
    }

    #[test]
    fn a_boundary_at_the_start_of_a_side_does_not_run_backwards() {
        // A boundary two seconds in has only two seconds of lead-in, and the
        // audition has to be short rather than wrong.
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, sent) = recorded(&dir, 12 * SECOND as usize);
        let (played, report) = rendered(&path, capture_id, Scope::boundary(RATE, SECOND));
        let context = frames_at(RATE, BOUNDARY_CONTEXT_SECONDS);
        assert_eq!(report.span.start, 0, "the audition ran off the front");
        assert_eq!(report.frames, SECOND + context);
        assert_eq!(played, sent[..(SECOND + context) as usize * FRAME]);
    }

    #[test]
    fn a_region_past_the_end_plays_what_was_recorded_and_stops() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, sent) = recorded(&dir, 5_000);
        let (played, report) =
            rendered(&path, capture_id, Scope::Region(Span::new(4_000, 900_000)));
        assert_eq!(report.frames, 1_000);
        assert_eq!(played, sent[4_000 * FRAME..]);
        assert_eq!(report.span.end, 5_000, "the span was not clamped");
    }

    #[test]
    fn a_scope_with_no_audio_is_refused_rather_than_played() {
        // A transport that starts and immediately ends looks like a fault, so
        // it is one.
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, _) = recorded(&dir, 1_000);
        let audition =
            Audition::new(&path, capture_id).scope(Scope::Region(Span::new(5_000, 9_000)));
        let mut out = Vec::new();
        match render(&audition, &[], &mut out) {
            Err(Error::Nothing {
                capture_id: id,
                scope,
            }) => {
                assert_eq!(id, capture_id);
                assert!(scope.contains("region"), "{scope}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(out.is_empty());
    }

    #[test]
    fn a_capture_that_is_not_there_is_refused_at_the_door() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, _, _) = recorded(&dir, 100);
        let mut out = Vec::new();
        assert!(matches!(
            render(&Audition::new(&path, 4_242), &[], &mut out),
            Err(Error::Project(vcw_project::Error::NoSuchCapture { .. }))
        ));
    }

    #[test]
    fn a_hole_in_the_timeline_stops_the_render_rather_than_inventing_silence() {
        // Silence in place of a missing block would be indistinguishable from
        // silence that was recorded, which is the one lie a capture
        // workstation cannot tell.
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, _) = recorded(&dir, 40_000);
        {
            let project = Project::open(&path).expect("open");
            project
                .conn()
                .execute(
                    "DELETE FROM capture_blocks WHERE capture_id = ?1 AND sequence = \
                     (SELECT MAX(sequence) - 1 FROM capture_blocks WHERE capture_id = ?1)",
                    [capture_id],
                )
                .expect("delete");
            project.close().expect("close");
        }
        let mut out = Vec::new();
        let outcome = render(&Audition::new(&path, capture_id), &[], &mut out);
        assert!(
            matches!(
                outcome,
                Err(Error::Project(vcw_project::Error::Unplayable { .. }))
            ),
            "expected a refusal, got {outcome:?}"
        );
    }

    #[test]
    fn a_stop_cue_ends_a_render_where_it_says() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, sent) = recorded(&dir, 4 * SECOND as usize);
        let mut out = Vec::new();
        let cues = [Cue {
            after_frames: SECOND,
            verb: Verb::Stop,
        }];
        let report = render(&Audition::new(&path, capture_id), &cues, &mut out).expect("render");
        assert!(report.frames >= SECOND && report.frames < 2 * SECOND);
        assert_eq!(out, sent[..report.frames as usize * FRAME]);
    }

    #[test]
    fn the_skip_verbs_move_by_the_step_they_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, sent) = recorded(&dir, 25 * SECOND as usize);
        let skip = frames_at(RATE, SKIP_SECONDS);
        let mut out = Vec::new();
        let cues = [
            Cue {
                after_frames: SECOND,
                verb: Verb::SkipForward,
            },
            Cue {
                after_frames: 2 * SECOND,
                verb: Verb::SkipBack,
            },
        ];
        let report = render(&Audition::new(&path, capture_id), &cues, &mut out).expect("render");

        let forward = report.applied[0];
        let back = report.applied[1];
        assert_eq!(forward.landed, forward.after + skip, "forward by {skip}");
        assert_eq!(
            back.landed,
            forward.landed + (back.after - forward.after) - skip,
            "back by {skip}"
        );

        let mut expected = sent[..forward.after as usize * FRAME].to_vec();
        let second = forward.landed + (back.after - forward.after);
        expected.extend_from_slice(&sent[forward.landed as usize * FRAME..second as usize * FRAME]);
        expected.extend_from_slice(&sent[back.landed as usize * FRAME..]);
        assert_eq!(out.len(), expected.len());
        assert_eq!(out, expected);
    }

    #[test]
    fn with_no_marks_a_skip_is_the_fixed_step() {
        // The fallback, stated on its own so the four tests below are about the
        // marks and nothing else.
        assert_eq!(next_mark(&[], 1_000, true), None);
        assert_eq!(next_mark(&[], 1_000, false), None);
    }

    #[test]
    fn a_skip_lands_on_the_next_mark_and_not_before_it() {
        // §21's SKIP FORWARD is "the top of the next track". Landing a second
        // early would start it with the end of the previous one, which is the
        // question WP-13 left open and this is the answer.
        let marks = [0, 1_000, 2_500, 9_000];
        assert_eq!(next_mark(&marks, 0, true), Some(1_000));
        assert_eq!(next_mark(&marks, 999, true), Some(1_000));
        assert_eq!(next_mark(&marks, 1_000, true), Some(2_500));
        assert_eq!(next_mark(&marks, 2_499, true), Some(2_500));
    }

    #[test]
    fn a_skip_back_lands_on_the_previous_mark() {
        let marks = [0, 1_000, 2_500, 9_000];
        assert_eq!(next_mark(&marks, 9_000, false), Some(2_500));
        assert_eq!(next_mark(&marks, 2_501, false), Some(2_500));
        assert_eq!(next_mark(&marks, 2_500, false), Some(1_000));
        assert_eq!(next_mark(&marks, 1, false), Some(0));
    }

    #[test]
    fn a_mark_at_the_position_is_not_where_a_skip_lands() {
        // Strictly past, in both directions. A mark exactly under the playhead
        // that counted would make SKIP FORWARD a no-op held down, and SKIP BACK
        // at the top of a track would refuse to leave it.
        assert_eq!(next_mark(&[4_000], 4_000, true), None);
        assert_eq!(next_mark(&[4_000], 4_000, false), None);
    }

    #[test]
    fn holding_a_skip_walks_the_marks_rather_than_sticking() {
        let marks = [500, 1_500, 4_000];
        let mut at = 0;
        let mut visited = Vec::new();
        while let Some(mark) = next_mark(&marks, at, true) {
            visited.push(mark);
            at = mark;
        }
        assert_eq!(visited, vec![500, 1_500, 4_000]);
        let mut back = Vec::new();
        while let Some(mark) = next_mark(&marks, at, false) {
            back.push(mark);
            at = mark;
        }
        assert_eq!(back, vec![1_500, 500]);
    }

    #[test]
    fn marks_are_sorted_and_deduplicated_by_the_builder() {
        // Adjacent tracks share a frame - one's end is the next one's start -
        // so the natural caller hands in duplicates, and a duplicate mark is a
        // skip that appears to do nothing.
        let audition = Audition::new("/nowhere.vcw", 1).marks([4_000, 500, 4_000, 1_500, 500]);
        assert_eq!(audition.marks, vec![500, 1_500, 4_000]);
    }

    #[test]
    fn the_skip_verbs_move_between_marks_when_there_are_some() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, _) = recorded(&dir, 25 * SECOND as usize);
        // Deliberately not ten seconds apart, so a pass that ignored the marks
        // and used SKIP_SECONDS could not land on them by accident.
        let marks = [3 * SECOND, 7 * SECOND, 21 * SECOND];
        let audition = Audition::new(&path, capture_id).marks(marks);
        let mut out = Vec::new();
        let cues = [
            Cue {
                after_frames: SECOND,
                verb: Verb::SkipForward,
            },
            Cue {
                after_frames: 2 * SECOND,
                verb: Verb::SkipForward,
            },
            Cue {
                after_frames: 3 * SECOND,
                verb: Verb::SkipBack,
            },
        ];
        let report = render(&audition, &cues, &mut out).expect("render");
        let landed: Vec<u64> = report.applied.iter().map(|step| step.landed).collect();
        // A cue's clock is frames *rendered*, not the playhead, so the third
        // one fires a second after the second skip landed - which puts the
        // cursor at 8 s, inside the track that starts at 7 s. So SKIP BACK
        // lands on 7 s and not on 3 s: from inside a track it goes to that
        // track's top, which is what a transport's back button does. Walking
        // further back needs a second press, and that is
        // `holding_a_skip_walks_the_marks_rather_than_sticking`.
        assert_eq!(
            landed,
            vec![3 * SECOND, 7 * SECOND, 7 * SECOND],
            "{report:?}"
        );
    }

    #[test]
    fn a_render_asked_for_a_narrower_format_says_what_it_cost() {
        // Playback is allowed to convert, and never allowed to be quiet about
        // it. Sixteen bits out of a 32-bit capture is a real loss and the
        // report names it.
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, capture_id, _) = recorded(&dir, 2_000);
        let mut audition = Audition::new(&path, capture_id);
        audition.format = Some(SampleFormat::S16);
        let mut out = Vec::new();
        let report = render(&audition, &[], &mut out).expect("render");
        assert_eq!(report.frames, 2_000);
        assert_eq!(out.len(), 2_000 * 2 * CHANNELS as usize);
        assert!(
            report.conversion.contains("S16"),
            "the report hid the conversion: {}",
            report.conversion
        );
    }

    #[test]
    fn every_transport_verb_survives_a_round_trip_through_a_script() {
        for (text, verb) in [
            ("play", Verb::Play),
            ("pause", Verb::Pause),
            ("stop", Verb::Stop),
            ("seek 12.5", Verb::Seek(12.5)),
            ("skip-forward", Verb::SkipForward),
            ("skip-back", Verb::SkipBack),
        ] {
            assert_eq!(Verb::parse(text), Some(verb), "{text}");
        }
        assert_eq!(
            Verb::parse("skip-forward").as_ref().map(Verb::as_str),
            Some("skip-forward")
        );
        assert_eq!(
            Verb::parse("seek"),
            None,
            "a seek with no target is not a seek"
        );
        assert_eq!(Verb::parse("rewind"), None);
        assert_eq!(Verb::parse(""), None);
    }

    #[test]
    fn the_scope_a_listener_is_told_about_names_what_they_asked_for() {
        // Whole and Region resolve identically when the region is the whole
        // side, and the event still has to say which one was asked for.
        let layout = Layout {
            rate: RATE,
            channels: CHANNELS,
            format: StorageFormat::Int32,
            frames: 48_000,
        };
        assert!(Scope::Whole.describe(&layout).contains("whole"));
        assert!(
            Scope::Region(layout.span())
                .describe(&layout)
                .contains("region")
        );
        assert!(
            Scope::Track {
                number: 3,
                span: layout.span()
            }
            .describe(&layout)
            .contains("track 3")
        );
        assert!(
            Scope::boundary(RATE, 24_000)
                .describe(&layout)
                .contains("boundary")
        );
    }
}
