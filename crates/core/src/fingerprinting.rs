/*
 *  fingerprinting.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The fingerprint worker: candidate regions off the capture tap, spans afterwards.
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

//! The fingerprint worker: candidate regions off the capture tap, spans afterwards.
//!
//! §25 asks for two things this module answers and one it does not. "PCM shall be fed
//! asynchronously from the capture pipeline to the fingerprint worker" is
//! [`Fingerprints`], a third consumer of §10's tee in the shape of
//! [`crate::detection::Detectors`]. "Candidate regions shall be fingerprinted
//! progressively rather than repeatedly fingerprinting the entire recording" is what
//! makes it listen to the bus: a region is opened and closed by the live detector's
//! [`Event::Detected`], so the audio between two boundaries is fingerprinted once, as
//! it arrives, and the side is never re-read. What §25 leaves to WP-22 is what a
//! fingerprint is *for* - nothing here looks anything up, and nothing here is
//! persisted, because re-fingerprinting committed audio costs 0.6% of a core (S4) and
//! a cache with no lookup to serve is a schema change nobody can use yet.
//!
//! Three things about the live half are deliberate and none of them is obvious:
//!
//! - **A region starts late.** The detector announces a boundary only once nothing
//!   later can move it, about 1.2 s after the fact, and by then the tap has already
//!   handed that audio over and forgotten it. So a region's audio begins where the
//!   worker's cursor is when the news arrives, not at the frame the news is about -
//!   [`Region`] carries both. S4 measured what that costs: region-boundary error is
//!   bounded at **~0.064 BER** against **0.47-0.49 for unrelated audio**, so a late
//!   start is nowhere near enough to break a match, and buffering 1.2 s of every
//!   stream to avoid it would be 6.5 MiB at 192 kHz for no gain.
//! - **A holed region is thrown away, not published.** The tap is lossy by
//!   construction, which is §10's rule that no consumer may cost the recording a
//!   sample. A meter that misses 200 ms shows a stale needle; a fingerprint that
//!   misses 200 ms is *shifted from that point on* and will match nothing, while
//!   looking exactly like a fingerprint that works. So [`Tap::dropped_bytes`] is read
//!   when a region opens and again when it closes, and a region that lost audio is
//!   counted in [`Fingerprinted::holed`] rather than handed on.
//! - **Frames here are tap frames**, counted from the moment the tee was built, which
//!   is before `RECORD`. They are the same frames [`Event::Detected`] reports, because
//!   the detector counts from the same place, so a region and the boundaries that
//!   delimit it always agree with each other. They are *not* the capture's committed
//!   frames if the operator left the deck armed before recording, which is why the
//!   post-capture path takes a [`Span`] rather than reusing these numbers.
//!
//! [`of_span`] is the post-capture half: any span of committed audio, read back out of
//! the project. It is what a refined boundary gets fingerprinted with, and what the
//! tests check the live half against.
//!
//! Requirements: §25 (fingerprinting), §10 (fan-out), §36 (a worker per job).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use vcw_audio::buffers::Tap;
use vcw_fingerprint::chromaprint::{Builder, Fingerprint};
use vcw_project::Project;
use vcw_project::pcm::{Layout, Reader};
use vcw_types::{CaptureInfo, Edge, Span};

use crate::events::{Bus, Event, Events};

/// How often the tap and the bus are drained: a quarter second.
///
/// [`crate::detection`]'s interval, for the same reason - the detector it listens to
/// cannot announce anything faster than it settles - and deliberately the same number,
/// because a region's start is the cursor at drain time and a slower loop would push
/// it further from the boundary it belongs to.
const DRAIN: Duration = Duration::from_millis(250);

/// How much audio the tap can hold before it starts dropping: two seconds.
///
/// [`crate::detection::Detectors`]'s size. What is lost here is not a hole in a trace
/// but a whole region thrown away, so the ten-to-one margin over [`DRAIN`] is what
/// keeps an unlucky scheduling slice from costing a track its identification.
const TAP_MILLIS: u32 = 2_000;

/// How much of one region is fingerprinted at most: two minutes.
///
/// `fpcalc`'s default, and AcoustID's own indexing length. A side-long region - the
/// shape of a capture nobody detected any boundaries in - would otherwise feed the
/// algorithm forty minutes of audio to answer a question the first two minutes answer,
/// and the match is against the *start* of a recording either way.
const CAP_SECONDS: u64 = 120;

/// Fingerprinting failed on audio that was already committed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The project would not give up the audio.
    #[error(transparent)]
    Project(#[from] vcw_project::Error),
    /// The algorithm refused it.
    #[error(transparent)]
    Fingerprint(#[from] vcw_fingerprint::chromaprint::Error),
}

/// One fingerprinted candidate region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    /// The boundary that opened it, in tap frames: what the detector said.
    pub boundary: u64,
    /// Where the fingerprinted audio actually starts, in tap frames: where the worker
    /// was when it heard. Later than `boundary` by the detector's settling lag - see
    /// the module note.
    pub from_frame: u64,
    /// The fingerprint of the audio between there and the region's end.
    pub fingerprint: Fingerprint,
}

/// What the live pass came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fingerprinted {
    /// Every region that produced a fingerprint, in the order they were closed.
    pub regions: Vec<Region>,
    /// How many regions were thrown away because the tap lost audio inside them. Not
    /// a fault in itself - see §10 - but a fingerprint is only evidence if it is
    /// whole, so these are counted rather than published.
    pub holed: usize,
    /// How many regions the algorithm would not fingerprint: shorter than its warm-up,
    /// mostly, which is what a two-second run-out groove between two tracks is.
    pub refused: usize,
}

/// A running live fingerprint worker.
///
/// Dropping it stops the thread without waiting. [`Fingerprints::stop`] waits and
/// returns what it found.
pub struct Fingerprints {
    thread: Option<JoinHandle<Fingerprinted>>,
    running: Arc<AtomicBool>,
}

impl std::fmt::Debug for Fingerprints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fingerprints")
            .field("running", &self.running.load(Ordering::Relaxed))
            .finish()
    }
}

impl Fingerprints {
    /// Starts a live pass on this tap, taking its regions from this bus.
    ///
    /// The subscription is made here, on the caller's thread, rather than inside the
    /// worker: a subscriber that appears after the first publish has missed it, so the
    /// engine can spawn this before the detector and know the first boundary will be
    /// seen.
    ///
    /// # Errors
    ///
    /// If chromaprint will not accept the capture's rate or channel count, in which
    /// case the capture runs on without a fingerprint worker rather than not at all.
    pub fn spawn(tap: Tap, info: &CaptureInfo, bus: &Bus) -> Result<Self, Error> {
        accepts(info)?;
        let events = bus.subscribe();
        let info = info.clone();
        let running = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&running);
        let scratch = bytes_for(&info, TAP_MILLIS / 4);
        let thread = std::thread::Builder::new()
            .name("vcw-fingerprint".into())
            .spawn(move || run(tap, &info, &events, &flag, scratch))
            .ok();
        Ok(Self { thread, running })
    }

    /// Stops the worker, waits for it, and returns every region it fingerprinted.
    #[must_use]
    pub fn stop(mut self) -> Fingerprinted {
        self.running.store(false, Ordering::Relaxed);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
        self.thread
            .take()
            .and_then(|thread| thread.join().ok())
            .unwrap_or_default()
    }
}

impl Drop for Fingerprints {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

/// Whether chromaprint will take this capture's audio at all.
///
/// Worth asking *before* a tap is handed out, because a tee writes into every tap it
/// has whether or not anybody is reading the other end: a worker that cannot start
/// must never be given one, or the capture pays for a ring nobody drains.
///
/// There is nothing per-region about a builder, so one of them proves them all.
///
/// # Errors
///
/// If chromaprint will not accept the capture's rate or channel count.
pub fn accepts(info: &CaptureInfo) -> Result<(), Error> {
    Builder::open(info)?;
    Ok(())
}

/// How big a tap this capture's fingerprint worker wants, in bytes.
#[must_use]
pub fn tap_bytes(info: &CaptureInfo) -> usize {
    bytes_for(info, TAP_MILLIS)
}

/// Bytes of interleaved audio this capture produces in `millis`.
fn bytes_for(info: &CaptureInfo, millis: u32) -> usize {
    let frame = info.storage_format.bytes_per_sample() * info.channels as usize;
    let frames = (u64::from(info.rate.0) * u64::from(millis)).div_ceil(1000);
    (frames as usize * frame).max(frame)
}

/// A region with audio going into it.
struct Open {
    builder: Builder,
    boundary: u64,
    from_frame: u64,
    /// The tap's loss counter when this region opened. If it has moved by the time the
    /// region closes, the audio in the builder has a hole in it.
    dropped: u64,
}

/// The live worker loop.
fn run(
    mut tap: Tap,
    info: &CaptureInfo,
    events: &Events,
    running: &AtomicBool,
    scratch: usize,
) -> Fingerprinted {
    let frame_bytes = (info.storage_format.bytes_per_sample() * info.channels as usize).max(1);
    let cap = u64::from(info.rate.hz()) * CAP_SECONDS;
    let mut buffer = vec![0u8; scratch];
    let mut read_bytes = 0_u64;
    let mut open: Option<Open> = None;
    let mut out = Fingerprinted::default();

    loop {
        let carry_on = running.load(Ordering::Relaxed);

        // Boundaries first. An event names a frame the cursor has already passed, so
        // acting on it before this round's audio is drained opens the region as close
        // to the boundary as the detector's lag allows.
        for event in events.drain() {
            let Event::Detected { frame, edge, .. } = event else {
                continue;
            };
            let cursor = (read_bytes + tap.dropped_bytes()) / frame_bytes as u64;
            close(open.take(), &mut out, &tap);
            // A start opens the next region; an end only closes the one that was
            // open. Two starts in a row are the ordinary shape of a side whose
            // between-track groove was too quiet to call an end, and the first region
            // closes where the second begins.
            if edge == Edge::Start {
                match Builder::open(info) {
                    Ok(builder) => {
                        open = Some(Open {
                            builder,
                            boundary: frame,
                            from_frame: cursor,
                            dropped: tap.dropped_bytes(),
                        });
                    }
                    // Unreachable: `accepts` proved this exact call before the tap was
                    // handed over. Counted anyway, because a region that silently
                    // never opened reads as a side with no boundaries in it.
                    Err(_) => out.refused += 1,
                }
            }
        }

        // Drain everything waiting rather than one buffer of it, so a worker that was
        // late does not stay late and lose audio to the tap wrapping. Audio outside a
        // region is read and discarded, which is what keeps the cursor honest.
        loop {
            let read = tap.read(&mut buffer);
            if read == 0 {
                break;
            }
            read_bytes += read as u64;
            // Capped. Closed here rather than at the next boundary so the algorithm
            // stops being fed, and not reopened: the next region starts at the next
            // boundary. The drain carries on either way, because audio left in the tap
            // is audio the next region will be charged for as a hole.
            if open
                .as_ref()
                .is_some_and(|region| region.builder.frames() >= cap)
            {
                close(open.take(), &mut out, &tap);
                continue;
            }
            // A push cannot fail for any reason a later push would survive, so a
            // region whose audio was refused is closed and counted.
            if open
                .as_mut()
                .is_some_and(|region| region.builder.push(&buffer[..read]).is_err())
            {
                open = None;
                out.refused += 1;
            }
        }

        if !carry_on || (tap.is_abandoned() && tap.available() == 0) {
            // The last track of a side has no end boundary - the record simply stops
            // turning - so the open region is closed here or it is lost.
            close(open.take(), &mut out, &tap);
            break;
        }
        // Parked rather than slept, so `stop` can have it back at once. A worker
        // that has to be waited out is a quarter second of audio recorded *past*
        // the operator's stop, because `Recorder::halt` joins its workers before it
        // tells the writer to finish and the device goes on producing throughout -
        // which is exactly what two calibrated timing tests caught when this slept.
        std::thread::park_timeout(DRAIN);
    }
    out
}

/// Finishes a region, or counts why it could not be.
fn close(open: Option<Open>, out: &mut Fingerprinted, tap: &Tap) {
    let Some(region) = open else {
        return;
    };
    if tap.dropped_bytes() != region.dropped {
        out.holed += 1;
        return;
    }
    match region.builder.finish() {
        Ok(fingerprint) => out.regions.push(Region {
            boundary: region.boundary,
            from_frame: region.from_frame,
            fingerprint,
        }),
        Err(_) => out.refused += 1,
    }
}

/// Fingerprints a span of audio already committed to a project.
///
/// The post-capture half of §25, and the one a refined boundary uses: it reads the
/// frames the project actually holds, so nothing about it depends on a tap, a timing
/// or a worker still being alive. S4 measured the cost at 0.6% of a core per stream,
/// which is why nothing here is cached.
///
/// # Errors
///
/// If the capture is not in this project, its audio cannot be read, or the span is too
/// short for the algorithm to say anything about.
pub fn of_span(project: &Project, capture_id: i64, span: Span) -> Result<Fingerprint, Error> {
    let layout = Layout::of(project.conn(), capture_id)?;
    let mut builder = Builder::of(layout.rate, layout.channels, layout.format)?;
    let mut reader = Reader::open(project.conn(), capture_id, span)?;
    // A second of audio per read, as in `detection::refine`: large enough that the
    // per-call cost disappears, small enough that no side is ever held twice.
    let mut buffer = vec![0u8; (layout.frame_bytes() * layout.rate.hz() as usize).max(4_096)];
    loop {
        let read = reader.fill(&mut buffer)?;
        if read == 0 {
            break;
        }
        builder.push(&buffer[..read])?;
    }
    Ok(builder.finish()?)
}

/// Opens a project read-only and fingerprints one span of one capture.
///
/// # Errors
///
/// As [`of_span`], plus anything that stops the project being opened.
pub fn of_project_span(
    path: &std::path::Path,
    capture_id: i64,
    span: Span,
) -> Result<Fingerprint, Error> {
    let project = Project::open_read_only(path)?;
    let fingerprint = of_span(&project, capture_id, span);
    project.close()?;
    fingerprint
}
