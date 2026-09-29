/*
 *  playback.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Playback engine, transport and audition (§21).
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

//! The playback stream: choosing a configuration, the RT callback, and what the
//! listener is really hearing (§21).
//!
//! Capture's obligations run in reverse here, and one of them changes shape.
//!
//! **The callback still does the minimum possible work (§10).** Its whole body is
//! [`Source::on_data`], an ordinary function over a byte slice, so it runs in a
//! test with no sound card and `tests/rt_safety.rs` asserts it allocates nothing.
//! It copies bytes out of chunks and, when there are none, writes silence. It
//! never converts a sample, never touches SQLite and never waits.
//!
//! **Bit-perfect playback is claimed the same way and no more easily.**
//! [`fidelity`] weighs the same kinds of evidence [`crate::capture::verdict`]
//! does - the mode the stream was opened in, what the operating system says the
//! device is doing, whether the request was honoured - and adds the one that only
//! exists on this side: whether anything had to be *converted* between what was
//! stored and what the device takes. A 24-bit side played on a float-only card
//! sounds fine and is not bit-perfect, and the UI has to be able to say so.
//!
//! **A capture plays at its own rate or not at all.** There is no resampler; see
//! [`crate::convert`] for why, `docs/adr/0006-playback-rate-policy.md` for the
//! decision, and [`crate::Error::RateUnavailable`] for what happens instead.
//!
//! # What an underrun means here
//!
//! If the feeder falls behind, the callback writes silence and counts it. Unlike
//! a capture overrun, nothing is lost for ever - the audio is still in the
//! project - but the listener heard a gap, so it is counted and reported rather
//! than smoothed over. [`Health::is_clean`] is what a UI should believe.
//!
//! # The three moving parts
//!
//! 1. [`Playback`] owns the CPAL stream and stays on the thread that built it.
//! 2. [`crate::chunks::Feeder`] is handed to the caller, which fills chunks from
//!    wherever the audio lives - `vcw-core` fills them from a project.
//! 3. [`Cursor`] is the shared state between them: the frame being played, the
//!    epoch a seek bumps, and whether the transport is running.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, StreamTrait};
use vcw_types::{SampleFormat, SampleRate, StorageFormat};

use crate::capture::{Divergence, VERIFY_SETTLE};
use crate::chunks::{self, Chunk, Drain, Feeder};
use crate::convert::Conversion;
use crate::devices::{self, DeviceKey, DeviceReport, Direction, DirectionReport, Transport};
use crate::error::{Error, Result};
use crate::probe::{self, Capability, Matrix};
use crate::verify::{self, Expected, Verification};

/// Sentinel for "the feeder has not run out of audio", in [`Cursor::drained`].
///
/// An epoch number rather than a flag, because "drained" is only true of the
/// epoch it happened in: after a seek there is more audio again, and a stale
/// flag would end playback the moment the listener moved the playhead.
const NOT_DRAINED: u64 = u64::MAX;

/// What the caller intends to play.
///
/// The device has to be opened *for* particular material, which is the whole
/// difference between this and [`crate::capture::Request`]: a capture takes the
/// best the converter offers, and playback has to match something that already
/// exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Material {
    /// The rate it was recorded at, which is the rate it will be played at.
    pub rate: SampleRate,
    /// How many channels it has.
    pub channels: u16,
    /// How its samples are laid out in the project.
    pub format: StorageFormat,
}

/// What the caller wants played, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Which device, by the id [`crate::devices`] hands out.
    pub device: DeviceKey,
    /// What is going to be played through it.
    pub material: Material,
    /// How to open the device (§9). A request, not an outcome.
    pub mode: vcw_types::CaptureMode,
    /// Sample format to insist on, or `None` to take the one that needs no
    /// conversion if the device offers it.
    pub format: Option<SampleFormat>,
    /// Device buffer size in frames, or `None` for the backend's default.
    pub buffer_frames: Option<u32>,
    /// How many chunks circulate. See [`chunks::QUEUE_CHUNKS`].
    pub queue_chunks: usize,
}

impl Request {
    /// A request to play this material on this device, converting as little as
    /// the device allows.
    #[must_use]
    pub fn new(device: DeviceKey, material: Material) -> Self {
        Self {
            device,
            material,
            mode: vcw_types::CaptureMode::Exclusive,
            format: None,
            buffer_frames: None,
            queue_chunks: chunks::QUEUE_CHUNKS,
        }
    }

    /// Insists on a sample format, rather than taking the one that converts least.
    #[must_use]
    pub const fn format(mut self, format: SampleFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Sets the mode to ask the device for.
    #[must_use]
    pub const fn mode(mut self, mode: vcw_types::CaptureMode) -> Self {
        self.mode = mode;
        self
    }
}

/// What the backend agreed to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// Rate the stream was built with. Always the material's rate - playback
    /// refuses rather than resampling - but reported so nothing has to assume it.
    pub rate: SampleRate,
    /// Channels the stream was built with, which need not match the material's.
    pub channels: u16,
    /// Sample representation the stream was built with.
    pub format: SampleFormat,
    /// The mode that was granted, which may be weaker than the one requested.
    pub mode: vcw_types::CaptureMode,
    /// The mode that was requested, kept so the pair can be reported (§9).
    pub mode_requested: vcw_types::CaptureMode,
    /// What sits between the application and the converter.
    pub transport: Transport,
    /// Device buffer size, as the backend described it.
    pub buffer: String,
    /// Every field the caller pinned and did not get.
    pub divergences: Vec<Divergence>,
}

impl Opened {
    /// Bytes in one frame of the *stream*, which is what the callback moves.
    #[must_use]
    pub const fn frame_bytes(&self) -> usize {
        self.format.bytes_per_sample() * self.channels as usize
    }

    /// Whether everything the caller pinned came back unchanged.
    #[must_use]
    pub fn honoured(&self) -> bool {
        self.divergences.is_empty()
    }

    /// A configuration for a sink that is not a device, so that a rendered file
    /// and a played stream can share one code path.
    ///
    /// Reports [`Transport::Unknown`] and [`vcw_types::CaptureMode::Shared`] for
    /// the same reason [`crate::capture::Negotiated::simulated`] does: a render
    /// is not a device and must never be mistaken for one.
    #[must_use]
    pub fn rendered(rate: SampleRate, channels: u16, format: SampleFormat) -> Self {
        Self {
            rate,
            channels,
            format,
            mode: vcw_types::CaptureMode::Shared,
            mode_requested: vcw_types::CaptureMode::Shared,
            transport: Transport::Unknown,
            buffer: "rendered".to_owned(),
            divergences: Vec::new(),
        }
    }
}

/// The counters, live. Shared with the audio callback, so every field is an
/// atomic and every update is `Relaxed`.
#[derive(Debug, Default)]
pub struct Counters {
    callbacks: AtomicU64,
    frames: AtomicU64,
    underruns: AtomicU64,
    silence_frames: AtomicU64,
    stale_chunks: AtomicU64,
    stream_errors: AtomicU64,
}

impl Counters {
    /// The counters as a value.
    #[must_use]
    pub fn snapshot(&self) -> Health {
        Health {
            callbacks: self.callbacks.load(Ordering::Relaxed),
            frames: self.frames.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            silence_frames: self.silence_frames.load(Ordering::Relaxed),
            stale_chunks: self.stale_chunks.load(Ordering::Relaxed),
            stream_errors: self.stream_errors.load(Ordering::Relaxed),
        }
    }

    /// Records a stream error from the backend's error callback.
    pub fn record_error(&self, text: &str) {
        self.stream_errors.fetch_add(1, Ordering::Relaxed);
        let lower = text.to_ascii_lowercase();
        if lower.contains("xrun") || lower.contains("underrun") {
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// What happened while the audio was playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Health {
    /// Callbacks the device has asked to be filled.
    pub callbacks: u64,
    /// Frames of real audio handed to the device.
    pub frames: u64,
    /// Times the callback had nothing to play and wrote silence.
    pub underruns: u64,
    /// Frames of silence written, whether from starvation or from a pause.
    pub silence_frames: u64,
    /// Chunks discarded because a seek made them stale. Not a fault: it is what
    /// stops a seek playing the old position.
    pub stale_chunks: u64,
    /// Errors the backend reported.
    pub stream_errors: u64,
}

impl Health {
    /// Whether the listener heard everything that was sent, in order, with no gaps.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.underruns == 0 && self.stream_errors == 0
    }
}

/// The state shared between the transport, the feeder and the audio callback.
///
/// Three questions, and all of them have to be answerable without a lock:
/// where the playhead is, which epoch is live, and whether the transport is
/// running. Everything else about playback is one of those three.
#[derive(Debug)]
pub struct Cursor {
    frame: AtomicU64,
    epoch: AtomicU64,
    /// The epoch of the last chunk the callback actually copied out. Lags
    /// `epoch` by however long the device takes to reach a new position.
    delivered: AtomicU64,
    playing: AtomicBool,
    drained: AtomicU64,
    ended: AtomicBool,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            frame: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            delivered: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            drained: AtomicU64::new(NOT_DRAINED),
            ended: AtomicBool::new(false),
        }
    }
}

impl Cursor {
    /// A cursor sitting at a frame, not yet playing.
    #[must_use]
    pub fn at(frame: u64) -> Self {
        let cursor = Self::default();
        cursor.frame.store(frame, Ordering::Relaxed);
        cursor
    }

    /// The frame the device is playing now.
    ///
    /// Exact, not inferred: the callback knows the frame number of the chunk in
    /// its hand, so nothing has to subtract a buffer depth it cannot see.
    ///
    /// Between a [`Cursor::seek`] and the callback that serves it this reads as
    /// the frame asked for, which is the right answer for a playhead and the
    /// wrong one for a latency measurement. [`Cursor::delivered`] is what says
    /// the device has actually got there.
    #[must_use]
    pub fn frame(&self) -> u64 {
        self.frame.load(Ordering::Relaxed)
    }

    /// The most recent epoch the callback has copied audio out of.
    ///
    /// `delivered() == epoch()` means the device is playing the position last
    /// asked for, rather than still working through what a seek invalidated.
    /// The join latency of a seek is the time between the two becoming equal.
    #[must_use]
    pub fn delivered(&self) -> u64 {
        self.delivered.load(Ordering::Acquire)
    }

    /// The live epoch. Chunks from any other epoch are discarded unplayed.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// Whether the transport is running.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// Starts or stops the flow of audio, without moving the playhead.
    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Relaxed);
    }

    /// Moves the playhead and invalidates everything already queued.
    ///
    /// Returns the new epoch. `Release` so that the frame, and the clearing of
    /// the end-of-audio flags, cannot be observed after the epoch bump that
    /// makes them relevant - the feeder reads the epoch to decide it must
    /// reseek, and must not then read a stale frame.
    pub fn seek(&self, frame: u64) -> u64 {
        self.frame.store(frame, Ordering::Relaxed);
        self.drained.store(NOT_DRAINED, Ordering::Relaxed);
        self.ended.store(false, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Release) + 1
    }

    /// The feeder saying it has queued the last of the audio for an epoch.
    ///
    /// What stops a finished playback being reported as an underrun: the
    /// callback running out of audio is starvation in the middle of a side and
    /// the end of it at the end, and only the feeder knows which.
    pub fn mark_drained(&self, epoch: u64) {
        self.drained.store(epoch, Ordering::Relaxed);
    }

    /// Whether the audio for this epoch has all been queued.
    #[must_use]
    pub fn is_drained(&self, epoch: u64) -> bool {
        self.drained.load(Ordering::Relaxed) == epoch
    }

    /// Whether the device has played the last of it.
    #[must_use]
    pub fn has_ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }

    /// Sets the playhead without invalidating anything, for a feeder reporting
    /// progress on a path that has no callback.
    pub fn set_frame(&self, frame: u64) {
        self.frame.store(frame, Ordering::Relaxed);
    }
}

/// The body of the audio callback, with CPAL taken out of it.
///
/// Separated for the same reason [`crate::capture::Sink`] is: a real-time
/// contract that is only asserted in a comment is a hope. This one is driven
/// directly by `tests/rt_safety.rs` under a counting allocator.
///
/// # What it does not do
///
/// No allocation, no locking, no syscalls, no sample arithmetic. Copying bytes
/// out of a chunk, three relaxed atomic increments, and - when there is nothing
/// to copy - filling silence.
pub struct Source {
    drain: Drain,
    cursor: Arc<Cursor>,
    counters: Arc<Counters>,
    frame_bytes: usize,
    /// The chunk being played, and how far into it.
    current: Option<(Chunk, usize)>,
}

impl Source {
    /// Builds a source over the reading end of a chunk queue.
    #[must_use]
    pub fn new(
        drain: Drain,
        cursor: Arc<Cursor>,
        counters: Arc<Counters>,
        frame_bytes: usize,
    ) -> Self {
        Self {
            drain,
            cursor,
            counters,
            frame_bytes: frame_bytes.max(1),
            current: None,
        }
    }

    /// Fills one callback's worth of device bytes.
    pub fn on_data(&mut self, out: &mut [u8]) {
        self.counters.callbacks.fetch_add(1, Ordering::Relaxed);
        if out.is_empty() {
            return;
        }
        let epoch = self.cursor.epoch();
        self.drop_stale(epoch);

        if !self.cursor.is_playing() {
            // Paused, and nothing is consumed: resume continues from exactly
            // this frame. A pause that ate a buffer would lose audio that is
            // still sitting in the queue.
            out.fill(0);
            self.count_silence(out.len());
            return;
        }

        let mut written = 0;
        while written < out.len() {
            if self.current.is_none() && !self.take_chunk(epoch) {
                // Nothing to play. Either the feeder is behind, or there is no
                // more audio - and those are different events.
                out[written..].fill(0);
                self.count_silence(out.len() - written);
                if self.cursor.is_drained(epoch) {
                    self.cursor.ended.store(true, Ordering::Relaxed);
                } else {
                    self.counters.underruns.fetch_add(1, Ordering::Relaxed);
                }
                return;
            }
            let Some((chunk, at)) = self.current.as_mut() else {
                return;
            };

            let take = (chunk.bytes().len() - *at).min(out.len() - written);
            out[written..written + take].copy_from_slice(&chunk.bytes()[*at..*at + take]);
            *at += take;
            written += take;

            let played = (*at / self.frame_bytes) as u64;
            self.cursor
                .frame
                .store(chunk.start_frame() + played, Ordering::Relaxed);
            // Recorded here rather than where the chunk was taken, because
            // what a latency measurement is owed is the moment audio from the
            // new position was handed to the device, not the moment it was
            // picked up. `Release` pairs with `Cursor::delivered`.
            self.cursor.delivered.store(epoch, Ordering::Release);
            self.counters
                .frames
                .fetch_add((take / self.frame_bytes) as u64, Ordering::Relaxed);

            if *at >= chunk.bytes().len() {
                let (spent, _) = self.current.take().expect("just checked");
                self.drain.recycle(spent);
            }
        }
    }

    /// Frames the device has been given so far.
    #[must_use]
    pub fn frames(&self) -> u64 {
        self.counters.frames.load(Ordering::Relaxed)
    }

    /// Bytes in one frame of the stream.
    #[must_use]
    pub const fn frame_bytes(&self) -> usize {
        self.frame_bytes
    }

    /// Throws away the chunk in hand if a seek has happened since it was filled.
    fn drop_stale(&mut self, epoch: u64) {
        if let Some((chunk, _)) = self.current.as_ref()
            && chunk.epoch() != epoch
        {
            let (stale, _) = self.current.take().expect("just checked");
            self.counters.stale_chunks.fetch_add(1, Ordering::Relaxed);
            self.drain.recycle(stale);
        }
    }

    /// Takes the next chunk of the live epoch, discarding any that are stale.
    ///
    /// The bounded loop matters: it is bounded by the queue, which is bounded by
    /// the number of chunks, so the audio thread cannot spin here.
    fn take_chunk(&mut self, epoch: u64) -> bool {
        while let Some(chunk) = self.drain.next_chunk() {
            if chunk.epoch() != epoch {
                self.counters.stale_chunks.fetch_add(1, Ordering::Relaxed);
                self.drain.recycle(chunk);
                continue;
            }
            if chunk.is_empty() {
                self.drain.recycle(chunk);
                continue;
            }
            self.current = Some((chunk, 0));
            return true;
        }
        false
    }

    /// Counts silence in frames, whatever put it there.
    fn count_silence(&self, bytes: usize) {
        self.counters
            .silence_frames
            .fetch_add((bytes / self.frame_bytes) as u64, Ordering::Relaxed);
    }
}

/// Whether what the listener heard was what was recorded, and why or why not.
///
/// The same three-way answer [`crate::capture::BitPerfect`] gives, for the same
/// reason: "nothing rules it out" is not a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fidelity {
    /// The device took the stored bytes unaltered, the OS confirmed the format,
    /// the request was honoured, and no counter moved.
    Confirmed,
    /// Something rules it out. Each reason is stated.
    Refuted {
        /// Why not, one per line.
        reasons: Vec<String>,
    },
    /// Nothing rules it out, and nothing establishes it either. Not a pass.
    Unconfirmed {
        /// What is missing from the evidence.
        reasons: Vec<String>,
    },
}

impl Fidelity {
    /// Whether the claim may be made. True for exactly one variant.
    #[must_use]
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed)
    }

    /// A one-line summary suitable for a UI badge.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            Self::Confirmed => "bit-perfect playback, confirmed against the OS".to_owned(),
            Self::Refuted { reasons } => format!("not bit-perfect: {}", reasons.join("; ")),
            Self::Unconfirmed { reasons } => {
                format!(
                    "bit-perfect playback not established: {}",
                    reasons.join("; ")
                )
            }
        }
    }
}

/// Weighs the evidence. The one place a bit-perfect playback claim can come from.
///
/// Kept a free function over plain inputs, like capture's, so every branch is
/// testable without a device - and so the rule can be read in one place instead
/// of being spread through the stream setup.
#[must_use]
pub fn fidelity(
    opened: &Opened,
    conversion: &Conversion,
    health: Health,
    verification: &Verification,
) -> Fidelity {
    let mut refuting = Vec::new();
    let mut missing = Vec::new();

    // The one that only exists on this side of the pipe. Everything else here
    // has a capture equivalent; this does not, and it is the most common reason
    // playback is not bit-perfect.
    if !conversion.is_identity() {
        let mut why = format!("the samples are converted for the device ({conversion})");
        for loss in conversion.losses() {
            why.push_str("; ");
            why.push_str(loss);
        }
        refuting.push(why);
    }
    if let Verification::Disagrees { divergences, .. } = verification {
        refuting.push(format!(
            "the operating system reports a different format ({})",
            divergences.join(", ")
        ));
    }
    if !opened.mode.could_be_bit_perfect() {
        refuting.push(format!(
            "the stream was opened {}, where the OS mixer owns the device",
            opened.mode.as_str()
        ));
    }
    if opened.transport == Transport::Converting {
        refuting.push("the device was reached through a converting path".to_owned());
    }
    if !health.is_clean() {
        refuting.push(format!(
            "the listener heard {} gap(s) and the backend reported {} error(s)",
            health.underruns, health.stream_errors
        ));
    }
    if !refuting.is_empty() {
        return Fidelity::Refuted { reasons: refuting };
    }

    if let Verification::Unavailable { why } = verification {
        missing.push(why.clone());
    }
    if !opened.honoured() {
        missing.push(format!(
            "the request was not honoured in full ({})",
            opened
                .divergences
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if opened.transport == Transport::Unknown {
        missing.push("the platform does not say what sits in front of the device".to_owned());
    }
    if missing.is_empty() {
        Fidelity::Confirmed
    } else {
        Fidelity::Unconfirmed { reasons: missing }
    }
}

/// A running playback stream.
///
/// Not `Send`, for the reason [`crate::capture::Capture`] is not: a
/// [`cpal::Stream`] belongs to the thread that built it. The [`Feeder`] is what
/// crosses threads, and it is returned separately by [`Playback::open`].
pub struct Playback {
    stream: cpal::Stream,
    opened: Opened,
    conversion: Conversion,
    counters: Arc<Counters>,
    cursor: Arc<Cursor>,
    verification: Verification,
    name: String,
}

impl Playback {
    /// Opens an output device for this material and starts the stream paused.
    ///
    /// Paused, not playing: the transport above decides when audio starts, and a
    /// stream that plays the instant it is built would emit whatever the queue
    /// happened to contain - which at that moment is nothing, counted as an
    /// underrun the listener never heard.
    ///
    /// # Errors
    ///
    /// If the device is absent, cannot play the material's rate, or refuses the
    /// stream.
    pub fn open(request: &Request) -> Result<(Self, Feeder)> {
        let snapshot = devices::enumerate();
        let report = snapshot
            .get(&request.device)
            .ok_or_else(|| Error::NoSuchDevice {
                direction: Direction::Output,
                key: request.device.clone(),
            })?
            .clone();
        Self::open_on(&report, request)
    }

    /// As [`Playback::open`], for a caller that already has the device report.
    ///
    /// # Errors
    ///
    /// As [`Playback::open`].
    pub fn open_on(report: &DeviceReport, request: &Request) -> Result<(Self, Feeder)> {
        let matrix = Matrix::from_report(&report.output, Direction::Output);
        let chosen = choose(&matrix, request, report)?;
        let (mode, mode_divergence) =
            crate::capture::negotiate_mode(request.mode, report.transport);

        let mut divergences = Vec::new();
        if let Some(format) = request.format
            && format != chosen.format
        {
            divergences.push(Divergence {
                field: "format",
                requested: format!("{format:?}"),
                granted: format!("{:?}", chosen.format),
            });
        }
        if chosen.channels != request.material.channels {
            divergences.push(Divergence {
                field: "channels",
                requested: request.material.channels.to_string(),
                granted: chosen.channels.to_string(),
            });
        }
        divergences.extend(mode_divergence);

        let mut opened = Opened {
            rate: chosen.rate,
            channels: chosen.channels,
            format: chosen.format,
            mode,
            mode_requested: request.mode,
            transport: report.transport,
            buffer: String::new(),
            divergences,
        };
        let conversion = Conversion::new(
            request.material.format,
            request.material.channels,
            opened.format,
            opened.channels,
        );

        let cpal_format = probe::to_cpal(chosen.format);
        let device = devices::open(&request.device, Direction::Output)?;
        let frame_bytes = opened.frame_bytes();
        let chunk_bytes = chunks::chunk_bytes(frame_bytes, opened.rate.hz());
        let chunk_frames = (chunk_bytes / frame_bytes.max(1)).max(1);

        // The buffer size is not a tuning knob here, it is a correctness
        // constraint. The queue has to be able to fill one callback in full,
        // and it can only be sized for a number VCW knows - so playback asks
        // for one rather than taking whatever the backend feels like. On this
        // machine ALSA's own default was 350 ms against a 160 ms queue, which
        // underran on every single callback while reporting a healthy device.
        let asked = request
            .buffer_frames
            .unwrap_or_else(|| target_buffer(&report.output, &chosen));
        let pinned = request.buffer_frames.is_some();
        let opening = start(
            &device,
            &opened,
            cpal_format,
            cpal::BufferSize::Fixed(asked),
            chunk_bytes,
            sizing(request.queue_chunks, asked as usize, chunk_frames),
        );
        let ready = match opening {
            Ok(ready) => {
                opened.buffer = format!("{asked} frames, fixed");
                ready
            }
            // A backend that will not be told - WASAPI in shared mode, some
            // ALSA plugins - gets its own way, and the queue is made deep
            // enough to cover a callback of any plausible size instead.
            Err(error) if pinned => return Err(error),
            Err(_) => {
                let fallback = u64::from(opened.rate.hz())
                    .saturating_mul(u64::from(FALLBACK_QUEUE_MILLIS))
                    / 1_000;
                opened.buffer = "backend default".to_owned();
                start(
                    &device,
                    &opened,
                    cpal_format,
                    cpal::BufferSize::Default,
                    chunk_bytes,
                    sizing(
                        request.queue_chunks,
                        usize::try_from(fallback).unwrap_or(usize::MAX),
                        chunk_frames,
                    ),
                )?
            }
        };
        let Ready {
            stream,
            feeder,
            cursor,
            counters,
        } = ready;
        stream.play()?;

        // Ask the OS once the stream is running, for the reason capture does:
        // a PCM that has not been prepared reports nothing to compare against.
        std::thread::sleep(VERIFY_SETTLE);
        let verification = verify::against(
            &request.device,
            Direction::Output,
            Expected {
                rate: opened.rate.hz(),
                channels: opened.channels,
                format: cpal_format,
            },
        );

        tracing::info!(
            device = %request.device,
            name = %report.name,
            rate = opened.rate.hz(),
            channels = opened.channels,
            format = ?opened.format,
            identity = conversion.is_identity(),
            losses = %conversion.losses().join("; "),
            "playback stream opened"
        );

        Ok((
            Self {
                stream,
                opened,
                conversion,
                counters,
                cursor,
                verification,
                name: report.name.clone(),
            },
            feeder,
        ))
    }

    /// What the backend agreed to.
    #[must_use]
    pub const fn opened(&self) -> &Opened {
        &self.opened
    }

    /// What has to happen to the project's samples on the way to this device.
    #[must_use]
    pub const fn conversion(&self) -> &Conversion {
        &self.conversion
    }

    /// The shared cursor, for the transport and the feeder.
    #[must_use]
    pub fn cursor(&self) -> &Arc<Cursor> {
        &self.cursor
    }

    /// What the operating system said, if anything.
    #[must_use]
    pub const fn verification(&self) -> &Verification {
        &self.verification
    }

    /// The counters, as of now.
    #[must_use]
    pub fn health(&self) -> Health {
        self.counters.snapshot()
    }

    /// The device, as a user would recognise it.
    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// Whether the listener is hearing the stored bytes, as of now.
    #[must_use]
    pub fn fidelity(&self) -> Fidelity {
        fidelity(
            &self.opened,
            &self.conversion,
            self.health(),
            &self.verification,
        )
    }

    /// Starts the audio flowing.
    ///
    /// # Errors
    ///
    /// If the backend refuses to resume the stream.
    pub fn play(&self) -> Result<()> {
        self.cursor.set_playing(true);
        self.stream.play()?;
        Ok(())
    }

    /// Stops the audio without moving the playhead.
    ///
    /// Both halves matter. The flag stops the callback consuming chunks, so
    /// resuming continues from the same frame; asking the backend to pause stops
    /// it calling at all, so a paused transport costs nothing. Backends that
    /// cannot pause still go silent, because of the flag.
    ///
    /// # Errors
    ///
    /// If the backend refuses to pause the stream.
    pub fn pause(&self) -> Result<()> {
        self.cursor.set_playing(false);
        self.stream.pause()?;
        Ok(())
    }

    /// The frame being played now.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.cursor.frame()
    }

    /// Stops the stream and returns the final counters.
    #[must_use]
    pub fn stop(self) -> Health {
        self.cursor.set_playing(false);
        let health = self.counters.snapshot();
        drop(self.stream);
        // Playback underruns are audible and nothing else records them, so they
        // go out at `warn`. Stale chunks do not: a seek discarding what it made
        // obsolete is the mechanism working, not a fault.
        if health.underruns > 0 || health.stream_errors > 0 {
            tracing::warn!(
                callbacks = health.callbacks,
                frames = health.frames,
                underruns = health.underruns,
                silence_frames = health.silence_frames,
                stream_errors = health.stream_errors,
                "playback stream stopped with defects"
            );
        } else {
            tracing::debug!(
                callbacks = health.callbacks,
                frames = health.frames,
                stale_chunks = health.stale_chunks,
                "playback stream stopped clean"
            );
        }
        health
    }
}

/// How much audio playback asks a device to take in one callback, when the
/// caller has not said.
///
/// Four chunks. Low enough that a seek reaches the converter promptly, high
/// enough that a busy machine does not starve it, and - the part that matters -
/// a number VCW knows, so `sizing` can make the queue deep enough to
/// serve one callback in full.
pub const TARGET_BUFFER_MILLIS: u32 = 4 * chunks::CHUNK_MILLIS;

/// How much queue to keep when the backend would not be told what to take.
///
/// A second. Deliberately extravagant: the cost of a queue too deep is some
/// memory and some wasted refilling after a seek, and the cost of a queue too
/// shallow is an underrun on every callback.
pub const FALLBACK_QUEUE_MILLIS: u32 = 1_000;

/// How many chunks the queue holds, and how many of them a seek may spend.
///
/// Both numbers come from the same measurement, so they are worked out in one
/// place: see [`sizing`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sizing {
    /// Chunks allocated, and therefore the most that can be in flight.
    depth: usize,
    /// Of those, how many are held back for a seek.
    reserve: usize,
}

/// Sizes the queue for a callback of `buffer_frames`.
///
/// Three callbacks' worth plus one, of which one callback's worth is held back
/// for a seek. That leaves two callbacks' worth plus one for ordinary running -
/// one for the callback that is running, one for the feeder to be filling while
/// it runs, and one so the two never have to be in step - and one callback's
/// worth that only a seek can spend, for the reason [`Feeder::hold_back`] gives.
/// Never fewer than the caller asked for.
fn sizing(asked: usize, buffer_frames: usize, chunk_frames: usize) -> Sizing {
    let per_callback = buffer_frames.div_ceil(chunk_frames.max(1)).max(1);
    let reserve = per_callback;
    let depth = asked
        .saturating_add(reserve)
        .max(per_callback.saturating_mul(3).saturating_add(1))
        .max(3);
    Sizing { depth, reserve }
}

/// The buffer size to ask for, clamped into what the device advertises.
fn target_buffer(report: &DirectionReport, chosen: &Capability) -> u32 {
    let wanted = (u64::from(chosen.rate.hz()) * u64::from(TARGET_BUFFER_MILLIS) / 1_000).max(1);
    let wanted = u32::try_from(wanted).unwrap_or(u32::MAX);
    report
        .configs
        .iter()
        .find(|range| {
            range.channels == chosen.channels
                && range.format == Some(chosen.format)
                && range.covers(chosen.rate)
        })
        .and_then(|range| range.buffer_frames)
        .map_or(wanted, |(min, max): (u32, u32)| {
            wanted.clamp(min, max.max(min))
        })
}

/// Everything building the stream produced.
///
/// A struct because the queue has to be built with the stream - the reading end
/// is moved into the callback - and the depth it is built at depends on whether
/// the device accepted the buffer size, so both have to happen together and
/// possibly twice.
struct Ready {
    /// The output stream, started but not yet playing audio.
    stream: cpal::Stream,
    /// The writing end of the chunk queue.
    feeder: Feeder,
    /// Shared with the callback.
    cursor: Arc<Cursor>,
    /// Shared with the callback.
    counters: Arc<Counters>,
}

/// Builds the queue and the stream together at one buffer size.
fn start(
    device: &cpal::Device,
    opened: &Opened,
    cpal_format: cpal::SampleFormat,
    buffer_size: cpal::BufferSize,
    chunk_bytes: usize,
    sizing: Sizing,
) -> Result<Ready> {
    let counters = Arc::new(Counters::default());
    let cursor = Arc::new(Cursor::default());
    let frame_bytes = opened.frame_bytes();
    let (mut feeder, drain) = chunks::queue(chunk_bytes, sizing.depth);
    feeder.hold_back(sizing.reserve);
    let mut source = Source::new(
        drain,
        Arc::clone(&cursor),
        Arc::clone(&counters),
        frame_bytes,
    );
    let error_counters = Arc::clone(&counters);
    let stream = device.build_output_stream_raw(
        cpal::StreamConfig {
            channels: opened.channels,
            sample_rate: opened.rate.hz(),
            buffer_size,
        },
        cpal_format,
        move |data, _info| source.on_data(data.bytes_mut()),
        move |err| {
            // Not the audio thread; an allocation here is fine and losing the
            // message that explains a failure would not be.
            error_counters.record_error(&err.to_string());
        },
        None,
    )?;
    Ok(Ready {
        stream,
        feeder,
        cursor,
        counters,
    })
}

/// Picks the configuration that converts least.
///
/// The preference order is the opposite of capture's, and deliberately so.
/// Capture takes the *best* the device offers, because a better capture is
/// strictly better. Playback wants the configuration that matches what is
/// already on disk, because anything else is a conversion - a 24-bit side on a
/// 32-bit stream sounds identical and can no longer be called bit-perfect.
///
/// The rate is not a preference. There is no resampler, so a device that
/// cannot play the material's rate is an error with a specific message.
fn choose(matrix: &Matrix, request: &Request, report: &DeviceReport) -> Result<Capability> {
    let material = request.material;
    let at_rate: Vec<&Capability> = matrix
        .entries
        .iter()
        .filter(|c| c.is_usable())
        .filter(|c| c.rate == material.rate)
        .filter(|c| request.format.is_none_or(|f| f == c.format))
        .collect();
    if at_rate.is_empty() {
        return Err(Error::RateUnavailable {
            device: report.label(),
            wanted: material.rate.hz(),
            offered: describe_rates(matrix),
        });
    }

    // Lower is better in every term: the channel count the material has, then a
    // format that needs no conversion, then one that at least loses nothing.
    at_rate
        .into_iter()
        .min_by_key(|c| {
            let channels = if c.channels == material.channels {
                0
            } else if c.channels > material.channels {
                // Padding with silence loses nothing; dropping a channel does.
                1 + u32::from(c.channels - material.channels)
            } else {
                1_000 + u32::from(material.channels - c.channels)
            };
            let conversion =
                Conversion::new(material.format, material.channels, c.format, c.channels);
            let format = if conversion.is_identity() {
                0
            } else if conversion.losses().is_empty() {
                1
            } else {
                2
            };
            (channels, format, format_width_rank(c.format))
        })
        .cloned()
        .ok_or_else(|| Error::RateUnavailable {
            device: report.label(),
            wanted: material.rate.hz(),
            offered: describe_rates(matrix),
        })
}

/// Widest first, among formats that are otherwise equivalent.
const fn format_width_rank(format: SampleFormat) -> u32 {
    match format {
        SampleFormat::S32 | SampleFormat::F32 => 0,
        SampleFormat::S24 => 1,
        SampleFormat::S16 => 2,
    }
}

/// The rates a device does offer, for the message that says it cannot do ours.
fn describe_rates(matrix: &Matrix) -> String {
    let rates = matrix.rates();
    if rates.is_empty() {
        return "nothing usable".to_owned();
    }
    rates
        .iter()
        .map(|r| format!("{} Hz", r.hz()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{ConfigRange, DirectionReport};
    use crate::verify::OsReport;
    use vcw_types::CaptureMode;

    const FRAME: usize = 8;

    /// A source over a fresh queue, with the feeder kept to fill it.
    fn wired(chunk_bytes: usize, chunks: usize) -> (Feeder, Source, Arc<Cursor>, Arc<Counters>) {
        let (feeder, drain) = chunks::queue(chunk_bytes, chunks);
        let cursor = Arc::new(Cursor::default());
        let counters = Arc::new(Counters::default());
        let source = Source::new(drain, Arc::clone(&cursor), Arc::clone(&counters), FRAME);
        (feeder, source, cursor, counters)
    }

    /// Queues one chunk of recognisable audio.
    fn queue_chunk(feeder: &mut Feeder, epoch: u64, start_frame: u64, byte: u8, frames: usize) {
        let mut chunk = feeder.take().expect("a spare chunk");
        chunk.spare_mut()[..frames * FRAME].fill(byte);
        chunk.mark(epoch, start_frame, frames * FRAME);
        feeder.send(chunk).expect("send");
    }

    fn opened(mode: CaptureMode, transport: Transport, format: SampleFormat) -> Opened {
        Opened {
            rate: SampleRate(96_000),
            channels: 2,
            format,
            mode,
            mode_requested: CaptureMode::Exclusive,
            transport,
            buffer: "backend default".to_owned(),
            divergences: Vec::new(),
        }
    }

    fn agrees() -> Verification {
        Verification::Agrees(Box::new(OsReport {
            source: "test".to_owned(),
            format: Some("S32_LE".to_owned()),
            rate: Some(96_000),
            channels: Some(2),
            period_frames: None,
            buffer_frames: None,
            access: None,
            raw: String::new(),
        }))
    }

    #[test]
    fn the_callback_plays_what_it_is_given_in_order_whatever_the_period_size() {
        // The device asks for whatever it likes, and chunk boundaries do not
        // line up with it. Every byte still has to arrive once, in order.
        let (mut feeder, mut source, cursor, counters) = wired(FRAME * 10, 4);
        cursor.set_playing(true);
        for (index, byte) in [0xA1u8, 0xB2, 0xC3].into_iter().enumerate() {
            queue_chunk(&mut feeder, 0, index as u64 * 10, byte, 10);
        }

        let mut played = Vec::new();
        let mut out = vec![0u8; FRAME * 7];
        for _ in 0..5 {
            out.fill(0xFF);
            source.on_data(&mut out);
            played.extend_from_slice(&out);
        }

        let mut expected = Vec::new();
        expected.extend(std::iter::repeat_n(0xA1u8, FRAME * 10));
        expected.extend(std::iter::repeat_n(0xB2u8, FRAME * 10));
        expected.extend(std::iter::repeat_n(0xC3u8, FRAME * 10));
        assert_eq!(&played[..expected.len()], &expected[..]);
        assert_eq!(counters.snapshot().frames, 30);
    }

    #[test]
    fn the_position_is_the_frame_being_played_and_not_an_estimate() {
        // The reason chunks carry a frame number. A playhead derived from
        // "frames sent minus buffer depth" is wrong by the queue after every
        // seek, and the queue is exactly what a seek discards.
        let (mut feeder, mut source, cursor, _) = wired(FRAME * 10, 4);
        cursor.set_playing(true);
        queue_chunk(&mut feeder, 0, 48_000, 0x11, 10);

        let mut out = vec![0u8; FRAME * 4];
        source.on_data(&mut out);
        assert_eq!(cursor.frame(), 48_004);
        source.on_data(&mut out);
        assert_eq!(cursor.frame(), 48_008);
    }

    #[test]
    fn a_chunk_from_before_a_seek_is_discarded_unplayed() {
        // The whole reason playback does not use a byte ring. Everything queued
        // at the old position has to go, including the chunk half-played in the
        // callback's hand, or a seek plays up to a queue's worth of the wrong
        // music before arriving.
        let (mut feeder, mut source, cursor, counters) = wired(FRAME * 10, 4);
        cursor.set_playing(true);
        for index in 0..3u64 {
            queue_chunk(&mut feeder, 0, index * 10, 0xAA, 10);
        }

        // Play into the first chunk, so one is in hand as well as queued.
        let mut out = vec![0u8; FRAME * 4];
        source.on_data(&mut out);
        assert_eq!(out, vec![0xAA; FRAME * 4]);

        let epoch = cursor.seek(900_000);
        assert_eq!(epoch, 1);
        queue_chunk(&mut feeder, epoch, 900_000, 0xBB, 10);

        let mut after = vec![0u8; FRAME * 10];
        source.on_data(&mut after);
        assert!(
            !after.contains(&0xAA),
            "audio from before the seek was played"
        );
        assert_eq!(after, vec![0xBB; FRAME * 10]);
        assert_eq!(cursor.frame(), 900_010);

        // Three stale chunks: the one in hand and the two still queued.
        let health = counters.snapshot();
        assert_eq!(health.stale_chunks, 3);
        assert_eq!(health.underruns, 0, "discarding stale audio is not a gap");
    }

    #[test]
    fn a_pause_consumes_nothing_so_resuming_loses_nothing() {
        // PAUSE in §21 is not "stop and seek back". The audio already queued is
        // still the audio that comes next.
        let (mut feeder, mut source, cursor, counters) = wired(FRAME * 10, 4);
        cursor.set_playing(true);
        queue_chunk(&mut feeder, 0, 0, 0x7C, 10);

        let mut out = vec![0u8; FRAME * 4];
        source.on_data(&mut out);
        assert_eq!(cursor.frame(), 4);

        cursor.set_playing(false);
        out.fill(0xFF);
        source.on_data(&mut out);
        assert_eq!(out, vec![0u8; FRAME * 4], "a paused callback made noise");
        assert_eq!(cursor.frame(), 4, "a pause moved the playhead");
        assert_eq!(counters.snapshot().underruns, 0, "a pause is not a gap");

        cursor.set_playing(true);
        out.fill(0xFF);
        source.on_data(&mut out);
        assert_eq!(out, vec![0x7C; FRAME * 4]);
        assert_eq!(cursor.frame(), 8);
    }

    #[test]
    fn starvation_writes_silence_and_counts_it() {
        // Nothing is lost - the audio is still in the project - but the listener
        // heard a gap, and a playback path that hides those is a playback path
        // that gets blamed for the recording.
        let (_feeder, mut source, cursor, counters) = wired(FRAME * 10, 4);
        cursor.set_playing(true);

        let mut out = vec![0xFFu8; FRAME * 4];
        source.on_data(&mut out);
        assert_eq!(out, vec![0u8; FRAME * 4]);
        let health = counters.snapshot();
        assert_eq!(health.underruns, 1);
        assert_eq!(health.silence_frames, 4);
        assert!(!health.is_clean());
    }

    #[test]
    fn running_out_at_the_end_of_the_audio_is_not_an_underrun() {
        // Otherwise every side ends with a reported fault. Only the feeder knows
        // whether there is more, so only the feeder can say.
        let (mut feeder, mut source, cursor, counters) = wired(FRAME * 10, 4);
        cursor.set_playing(true);
        queue_chunk(&mut feeder, 0, 0, 0x31, 2);
        cursor.mark_drained(0);

        let mut out = vec![0u8; FRAME * 8];
        source.on_data(&mut out);
        assert_eq!(&out[..FRAME * 2], &[0x31; FRAME * 2]);
        assert_eq!(&out[FRAME * 2..], &vec![0u8; FRAME * 6][..]);
        assert!(cursor.has_ended());
        assert_eq!(counters.snapshot().underruns, 0);
        assert!(counters.snapshot().is_clean());
    }

    #[test]
    fn a_seek_makes_the_end_of_the_audio_untrue_again() {
        // `drained` is per-epoch for this reason: a listener who seeks back into
        // a side that has finished is not at the end any more.
        let (mut feeder, mut source, cursor, _) = wired(FRAME * 10, 4);
        cursor.set_playing(true);
        queue_chunk(&mut feeder, 0, 0, 0x31, 2);
        cursor.mark_drained(0);
        let mut out = vec![0u8; FRAME * 8];
        source.on_data(&mut out);
        assert!(cursor.has_ended());

        let epoch = cursor.seek(0);
        assert!(!cursor.has_ended());
        assert!(!cursor.is_drained(epoch));
    }

    #[test]
    fn an_empty_callback_is_not_an_event() {
        let (_feeder, mut source, cursor, counters) = wired(FRAME * 10, 4);
        cursor.set_playing(true);
        source.on_data(&mut []);
        let health = counters.snapshot();
        assert_eq!(health.callbacks, 1);
        assert_eq!(health.underruns, 0);
    }

    /// An output device offering exactly these configurations.
    fn device(configs: Vec<ConfigRange>) -> DeviceReport {
        DeviceReport {
            key: DeviceKey::new("alsa", "hw:CARD=0,DEV=0"),
            name: "Test Converter".to_owned(),
            manufacturer: None,
            driver: None,
            device_type: String::new(),
            interface: String::new(),
            transport: Transport::DirectHardware,
            is_default_input: false,
            is_default_output: false,
            input: DirectionReport::default(),
            output: DirectionReport {
                supported: !configs.is_empty(),
                configs,
                default: None,
            },
            problems: Vec::new(),
        }
    }

    fn range(channels: u16, min: u32, max: u32, format: SampleFormat) -> ConfigRange {
        ConfigRange {
            channels,
            min_rate: min,
            max_rate: max,
            sample_format: format!("{format:?}"),
            format: Some(format),
            bytes_per_sample: format.bytes_per_sample(),
            buffer_frames: None,
        }
    }

    #[test]
    fn the_configuration_chosen_is_the_one_that_converts_least() {
        // Capture takes the best the device offers. Playback takes the one that
        // matches the project, which is often *not* the best: a 16-bit side on
        // a card that also does 32-bit should go out 16-bit, untouched.
        let report = device(vec![
            range(2, 44_100, 192_000, SampleFormat::S16),
            range(2, 44_100, 192_000, SampleFormat::S32),
            range(2, 44_100, 192_000, SampleFormat::F32),
        ]);
        let matrix = Matrix::from_report(&report.output, Direction::Output);

        for (stored, expected) in [
            (StorageFormat::Int16, SampleFormat::S16),
            (StorageFormat::Int32, SampleFormat::S32),
            (StorageFormat::Float32, SampleFormat::F32),
        ] {
            let request = Request::new(
                report.key.clone(),
                Material {
                    rate: SampleRate(96_000),
                    channels: 2,
                    format: stored,
                },
            );
            let chosen = choose(&matrix, &request, &report).expect("a configuration");
            assert_eq!(
                chosen.format, expected,
                "{stored:?} was converted needlessly"
            );
            assert_eq!(chosen.rate, SampleRate(96_000));
        }
    }

    #[test]
    fn a_format_the_device_does_not_have_falls_back_to_one_that_loses_nothing() {
        // No S24 on this card, so a packed 24-bit side has to widen. Widening
        // is exact, and it is preferred over narrowing to 16-bit even though
        // 16-bit is the same number of bytes the sample "needs".
        let report = device(vec![
            range(2, 44_100, 192_000, SampleFormat::S16),
            range(2, 44_100, 192_000, SampleFormat::S32),
        ]);
        let matrix = Matrix::from_report(&report.output, Direction::Output);
        let request = Request::new(
            report.key.clone(),
            Material {
                rate: SampleRate(96_000),
                channels: 2,
                format: StorageFormat::Int24Packed,
            },
        );
        let chosen = choose(&matrix, &request, &report).expect("a configuration");
        assert_eq!(chosen.format, SampleFormat::S32);
    }

    #[test]
    fn a_device_that_cannot_play_the_rate_says_which_rates_it_can() {
        // There is no resampler, so this is the end of the road - and the
        // message has to be the one that tells the user what to do about it.
        let report = device(vec![range(2, 44_100, 48_000, SampleFormat::S32)]);
        let matrix = Matrix::from_report(&report.output, Direction::Output);
        let request = Request::new(
            report.key.clone(),
            Material {
                rate: SampleRate(192_000),
                channels: 2,
                format: StorageFormat::Int32,
            },
        );
        match choose(&matrix, &request, &report) {
            Err(Error::RateUnavailable {
                wanted, offered, ..
            }) => {
                assert_eq!(wanted, 192_000);
                assert!(offered.contains("48000 Hz"), "{offered}");
            }
            Err(other) => panic!("wrong error: {other}"),
            Ok(chosen) => panic!("a 192 kHz side was played at {} Hz", chosen.rate.hz()),
        }
    }

    #[test]
    fn a_device_with_more_channels_than_the_side_is_preferred_to_one_with_fewer() {
        // Padding with silence loses nothing; dropping a channel loses half the
        // record. Between a mono stream and a four-channel one, a stereo side
        // takes the four.
        let report = device(vec![
            range(1, 44_100, 192_000, SampleFormat::S32),
            range(4, 44_100, 192_000, SampleFormat::S32),
        ]);
        let matrix = Matrix::from_report(&report.output, Direction::Output);
        let request = Request::new(
            report.key.clone(),
            Material {
                rate: SampleRate(96_000),
                channels: 2,
                format: StorageFormat::Int32,
            },
        );
        let chosen = choose(&matrix, &request, &report).expect("a configuration");
        assert_eq!(chosen.channels, 4);
    }

    #[test]
    fn bit_perfect_playback_needs_the_bytes_untouched_and_the_os_agreeing() {
        let identity = Conversion::new(StorageFormat::Int32, 2, SampleFormat::S32, 2);
        let verdict = fidelity(
            &opened(
                CaptureMode::Exclusive,
                Transport::DirectHardware,
                SampleFormat::S32,
            ),
            &identity,
            Health::default(),
            &agrees(),
        );
        assert!(verdict.is_confirmed(), "{verdict:?}");
        assert!(verdict.summary().contains("confirmed"));
    }

    #[test]
    fn a_converted_sample_refutes_the_claim_however_good_everything_else_is() {
        // The refutation that only exists on this side of the pipe: perfect
        // device, perfect mode, OS agreeing - and a 24-bit side going out as
        // float is still not the stored bytes.
        let converting = Conversion::new(StorageFormat::Int24Padded, 2, SampleFormat::F32, 2);
        let verdict = fidelity(
            &opened(
                CaptureMode::Exclusive,
                Transport::DirectHardware,
                SampleFormat::F32,
            ),
            &converting,
            Health::default(),
            &agrees(),
        );
        match &verdict {
            Fidelity::Refuted { reasons } => {
                assert!(
                    reasons[0].contains("converted for the device"),
                    "{reasons:?}"
                );
            }
            other => panic!("conversion did not refute the claim: {other:?}"),
        }
    }

    #[test]
    fn every_other_refutation_capture_has_applies_here_too() {
        let identity = Conversion::new(StorageFormat::Int32, 2, SampleFormat::S32, 2);
        let direct = |mode| opened(mode, Transport::DirectHardware, SampleFormat::S32);

        // The mixer owns the device.
        let shared = fidelity(
            &direct(CaptureMode::Shared),
            &identity,
            Health::default(),
            &agrees(),
        );
        assert!(matches!(shared, Fidelity::Refuted { .. }));

        // A converting path.
        let converting = fidelity(
            &opened(
                CaptureMode::Exclusive,
                Transport::Converting,
                SampleFormat::S32,
            ),
            &identity,
            Health::default(),
            &agrees(),
        );
        assert!(matches!(converting, Fidelity::Refuted { .. }));

        // The listener heard a gap.
        let gapped = fidelity(
            &direct(CaptureMode::Exclusive),
            &identity,
            Health {
                underruns: 1,
                ..Health::default()
            },
            &agrees(),
        );
        match gapped {
            Fidelity::Refuted { reasons } => assert!(reasons[0].contains("gap")),
            other => panic!("a gap did not refute the claim: {other:?}"),
        }

        // The OS contradicting the backend.
        let disagrees = Verification::Disagrees {
            report: Box::new(OsReport {
                source: "test".to_owned(),
                format: Some("S16_LE".to_owned()),
                rate: Some(48_000),
                channels: Some(2),
                period_frames: None,
                buffer_frames: None,
                access: None,
                raw: String::new(),
            }),
            divergences: vec!["format: expected S32_LE, got S16_LE".to_owned()],
        };
        let contradicted = fidelity(
            &direct(CaptureMode::Exclusive),
            &identity,
            Health::default(),
            &disagrees,
        );
        assert!(matches!(contradicted, Fidelity::Refuted { .. }));
    }

    #[test]
    fn missing_evidence_is_never_a_pass() {
        let identity = Conversion::new(StorageFormat::Int32, 2, SampleFormat::S32, 2);
        let unchecked = Verification::Unavailable {
            why: "this platform has no way to ask".to_owned(),
        };
        let verdict = fidelity(
            &opened(
                CaptureMode::Exclusive,
                Transport::DirectHardware,
                SampleFormat::S32,
            ),
            &identity,
            Health::default(),
            &unchecked,
        );
        match &verdict {
            Fidelity::Unconfirmed { reasons } => assert_eq!(reasons.len(), 1),
            other => panic!("an unchecked stream was not Unconfirmed: {other:?}"),
        }
        assert!(!verdict.is_confirmed());
        assert!(verdict.summary().contains("not established"));
    }

    #[test]
    fn the_queue_is_always_deep_enough_to_fill_one_callback() {
        // The bug this exists to prevent was measured, not imagined: ALSA's own
        // default buffer on this machine is 350 ms, the queue was 160 ms, and
        // the result was an underrun on every callback while every other
        // counter said the device was healthy. A queue shallower than one
        // callback cannot work, whatever else is right.
        for buffer in [64usize, 480, 1_024, 16_800, 48_000] {
            for chunk in [1usize, 96, 960, 3_840] {
                let Sizing { depth, reserve } = sizing(chunks::QUEUE_CHUNKS, buffer, chunk);
                // Twice over: once for the chunks a seek may not spend, and
                // once for the reserve on its own, because a seek has to be
                // able to fill a whole callback out of it.
                assert!(
                    (depth - reserve) * chunk >= buffer,
                    "a {chunk}-frame chunk queued {depth} deep, {reserve} reserved, \
                     cannot fill a {buffer}-frame callback"
                );
                assert!(
                    reserve * chunk >= buffer,
                    "a {reserve}-chunk reserve cannot cover a {buffer}-frame callback"
                );
                assert!(depth - reserve >= 2, "a queue of one cannot be refilled");
            }
        }
        // And it never shrinks below what the caller asked for, reserve aside.
        let Sizing { depth, reserve } = sizing(32, 64, 960);
        assert_eq!(depth - reserve, 32);
    }

    #[test]
    fn a_rendered_stream_is_never_mistaken_for_a_device() {
        // `vcw play --render` goes through the same fidelity rule, and a file is
        // not a converter however exact the bytes in it are.
        let identity = Conversion::new(StorageFormat::Int32, 2, SampleFormat::S32, 2);
        let rendered = Opened::rendered(SampleRate(96_000), 2, SampleFormat::S32);
        let verdict = fidelity(
            &rendered,
            &identity,
            Health::default(),
            &Verification::Unavailable {
                why: "rendered to a file".to_owned(),
            },
        );
        assert!(matches!(verdict, Fidelity::Refuted { .. }));
    }
}
