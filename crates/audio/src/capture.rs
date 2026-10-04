/*
 *  capture.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The capture stream: mode negotiation, the RT callback, and format
 *  verification.
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

//! The capture stream: mode negotiation, the RT callback, and what was really done.
//!
//! Three obligations shape everything here.
//!
//! **Nothing touches the samples (§9).** The stream is built with
//! [`cpal::traits::DeviceTrait::build_input_stream_raw`] and never the typed
//! variant, because the typed one converts. The bytes the converter produced go
//! into the ring and, at WP-05, into SQLite, unaltered. That is the only
//! definition of bit-perfect worth having.
//!
//! **The callback does the minimum possible work (§10).** No allocation, no
//! locking, no I/O, no blocking. The whole body is [`Sink::on_data`], which is
//! an ordinary function taking a byte slice - so it runs in a test with no sound
//! card, and `tests/rt_safety.rs` runs it under a counting allocator and asserts
//! the count is zero. A property that is asserted in a comment is a hope.
//!
//! **No bit-perfect claim without OS confirmation (§9, S1 finding 1).** The
//! backend's report of its own success is not evidence. [`Capture::verdict`]
//! returns [`BitPerfect::Confirmed`] only when [`crate::verify`] got a positive
//! answer from the operating system, every requested field was honoured, and no
//! counter moved. Any gap in that chain produces `Unconfirmed` with the gap
//! named, never a pass by default.
//!
//! # What an overrun means here
//!
//! If the writer falls behind, the ring fills and a whole callback is discarded
//! and counted. It is not buffered elsewhere, and it is never partially written:
//! see [`crate::buffers`]. A capture that overran is a capture with a hole in it,
//! and the counters are how that reaches the archive instead of being forgotten
//! when the process exits.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use vcw_types::{
    CaptureEq, CaptureInfo, CaptureMode, Diagnostics, SampleFormat, SampleRate, StorageFormat,
};

use crate::buffers::{self, RingReader, RingWriter};
use crate::devices::{self, DeviceKey, DeviceReport, Direction, Transport};
use crate::error::{Error, Result};
use crate::probe::{self, Capability, Matrix};
use crate::verify::{self, Expected, Verification};

/// How long to wait after starting before asking the OS what it negotiated.
///
/// ALSA publishes `hw_params` when the stream is prepared, not when it is
/// requested, and reading too early gets `closed`. Measured to be comfortable on
/// the development host; it costs nothing, because the stream is already
/// recording while we wait.
pub const VERIFY_SETTLE: Duration = Duration::from_millis(200);

/// What the caller wants recorded.
///
/// Every optional field means "the caller has no opinion", not "use the
/// default". The distinction matters: a field nobody asked for cannot be
/// dishonoured, so it never counts as a divergence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Which device, by the id [`crate::devices`] hands out. Never by name.
    pub device: DeviceKey,
    /// Sample rate, or `None` to take the best the device offers.
    pub rate: Option<SampleRate>,
    /// Channel count, or `None` to take the best on offer.
    pub channels: Option<u16>,
    /// Sample format, or `None` to take the widest integer format available.
    pub format: Option<SampleFormat>,
    /// How to open the device (§9). A request, not an outcome.
    pub mode: CaptureMode,
    /// Device buffer size in frames, or `None` for the backend's default.
    ///
    /// Worth leaving alone. This is the driver's buffer, and S1 finding 4 puts
    /// it in the crash-loss floor alongside commit granularity - but shrinking
    /// it below what the hardware likes buys xruns, not safety.
    pub buffer_frames: Option<u32>,
    /// Ring capacity in milliseconds. Raised to [`buffers::MIN_MILLIS`] if less.
    pub ring_millis: u32,
}

impl Request {
    /// A request that asks for nothing but the device, and takes the best
    /// configuration that device offers.
    ///
    /// The mode starts at [`CaptureMode::Exclusive`] because that is the only
    /// one that can be bit-perfect and falling back is reported; starting at
    /// `Shared` and never mentioning it would not be.
    pub fn new(device: DeviceKey) -> Self {
        Self {
            device,
            rate: None,
            channels: None,
            format: None,
            mode: CaptureMode::Exclusive,
            buffer_frames: None,
            ring_millis: buffers::DEFAULT_MILLIS,
        }
    }

    /// Pins the sample rate.
    #[must_use]
    pub const fn at(mut self, rate: SampleRate) -> Self {
        self.rate = Some(rate);
        self
    }

    /// Pins the channel count.
    #[must_use]
    pub const fn channels(mut self, channels: u16) -> Self {
        self.channels = Some(channels);
        self
    }

    /// Pins the sample format.
    #[must_use]
    pub const fn format(mut self, format: SampleFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Sets the capture mode to request.
    #[must_use]
    pub const fn mode(mut self, mode: CaptureMode) -> Self {
        self.mode = mode;
        self
    }
}

/// One field the caller asked for and did not get.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// Which field: `rate`, `channels`, `format`, `mode`.
    pub field: &'static str,
    /// What was asked for.
    pub requested: String,
    /// What was granted instead.
    pub granted: String,
}

impl std::fmt::Display for Divergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: asked {}, got {}",
            self.field, self.requested, self.granted
        )
    }
}

/// What the backend actually agreed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Negotiated {
    /// Rate the stream was built with.
    pub rate: SampleRate,
    /// Channel count the stream was built with.
    pub channels: u16,
    /// Sample representation the stream was built with.
    pub format: SampleFormat,
    /// How those samples are laid out on disk. Follows the device, per D4: CPAL's
    /// 24-bit is four bytes wide, so it stores as Audacity's padded code and is
    /// never repacked, because repacking would be a conversion.
    pub storage: StorageFormat,
    /// The mode that was granted, which may be weaker than the one requested.
    pub mode: CaptureMode,
    /// The mode that was requested, kept so the pair can be reported (§9).
    pub mode_requested: CaptureMode,
    /// What sits between the application and the converter.
    pub transport: Transport,
    /// Device buffer size, as the backend described it.
    pub buffer: String,
    /// Every field the caller asked for and did not get. Empty is the good case.
    pub divergences: Vec<Divergence>,
}

impl Negotiated {
    /// Whether everything the caller asked for came back unchanged.
    ///
    /// A request that specified nothing is honoured trivially, which is correct:
    /// nothing was promised, so nothing was broken.
    pub fn honoured(&self) -> bool {
        self.divergences.is_empty()
    }

    /// Bytes in one frame: one sample for each channel, at the storage width.
    pub const fn frame_bytes(&self) -> usize {
        self.storage.bytes_per_sample() * self.channels as usize
    }

    /// A configuration for a source that is not a device.
    ///
    /// Reports [`Transport::Unknown`] and [`CaptureMode::Shared`], because a
    /// simulated capture is not bit-perfect and must never be mistaken for one:
    /// [`verdict`] refuses the claim on both counts, whatever the counters say.
    #[must_use]
    pub fn simulated(rate: SampleRate, channels: u16, format: SampleFormat) -> Self {
        Self {
            rate,
            channels,
            format,
            storage: StorageFormat::native_for(format),
            mode: CaptureMode::Shared,
            mode_requested: CaptureMode::Shared,
            transport: Transport::Unknown,
            buffer: "simulated".to_owned(),
            divergences: Vec::new(),
        }
    }

    /// Bytes per second at this configuration, for sizing anything downstream.
    pub const fn bytes_per_second(&self) -> u64 {
        self.frame_bytes() as u64 * self.rate.hz() as u64
    }
}

/// The counters, live. Shared with the audio callback, so every field is an
/// atomic and every update is `Relaxed`: these are statistics, and ordering
/// between them buys nothing worth a fence on the audio thread.
#[derive(Debug, Default)]
pub struct Counters {
    overruns: AtomicU64,
    underruns: AtomicU64,
    dropped_frames: AtomicU64,
    stream_errors: AtomicU64,
    frames: AtomicU64,
    callbacks: AtomicU64,
}

impl Counters {
    /// The four §38 counters, as a value that can be persisted.
    pub fn snapshot(&self) -> Diagnostics {
        Diagnostics {
            overruns: self.overruns.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
            stream_errors: self.stream_errors.load(Ordering::Relaxed),
        }
    }

    /// Frames successfully handed to the ring.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Callbacks the device has delivered. Not persisted; useful for working out
    /// whether a stream is alive at all.
    pub fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Relaxed)
    }

    /// Records a stream error from the backend's error callback.
    ///
    /// Classifies as an underrun where the backend's wording identifies a
    /// device-side xrun. See [`Sink`] for why that is the best available signal.
    pub fn record_error(&self, text: &str) {
        self.stream_errors.fetch_add(1, Ordering::Relaxed);
        let lower = text.to_ascii_lowercase();
        if lower.contains("xrun") || lower.contains("underrun") || lower.contains("overrun") {
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The body of the audio callback, with CPAL taken out of it.
///
/// Separated so the real-time contract can be *tested*: `tests/rt_safety.rs`
/// drives [`Sink::on_data`] directly under a counting global allocator, which is
/// not a thing that can be done to a closure living inside a CPAL stream.
///
/// # What it does not do
///
/// No allocation, no locking, no syscalls, no floating-point work, no
/// unbounded loops. Three relaxed atomic increments and a memcpy into a
/// preallocated ring is the entire budget.
pub struct Sink {
    ring: RingWriter,
    counters: Arc<Counters>,
    frame_bytes: usize,
}

impl Sink {
    /// Builds a sink over the writing end of a ring.
    pub fn new(ring: RingWriter, counters: Arc<Counters>, frame_bytes: usize) -> Self {
        Self {
            ring,
            counters,
            frame_bytes: frame_bytes.max(1),
        }
    }

    /// Handles one callback's worth of device bytes.
    ///
    /// The bytes are whatever the converter produced, in whatever format it
    /// produced them. Nothing here inspects or interprets them.
    pub fn on_data(&mut self, bytes: &[u8]) {
        self.counters.callbacks.fetch_add(1, Ordering::Relaxed);
        if bytes.is_empty() {
            // A callback with nothing in it is the only device-side starvation
            // visible from this side of CPAL. It is rare and it is not the same
            // as an ALSA xrun, which the backend reports through the error
            // callback instead - see Counters::record_error.
            self.counters.underruns.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let frames = (bytes.len() / self.frame_bytes) as u64;
        if self.ring.push(bytes) {
            self.counters.frames.fetch_add(frames, Ordering::Relaxed);
        } else {
            // The writer is behind. Drop the whole callback and say so; a
            // partial write would tear a frame and corrupt everything after it.
            self.counters.overruns.fetch_add(1, Ordering::Relaxed);
            self.counters
                .dropped_frames
                .fetch_add(frames, Ordering::Relaxed);
        }
    }

    /// Bytes in one frame.
    pub const fn frame_bytes(&self) -> usize {
        self.frame_bytes
    }

    /// Bytes the ring would accept right now.
    ///
    /// Not for the callback's use - a device callback that asked this and then
    /// waited would be the exact real-time violation [`Sink::on_data`] is
    /// written to avoid. It exists for a *simulated* feeder running at
    /// [`crate::source::Pace::Metered`], which is not a callback, may block, and
    /// needs to know when pushing would cost a frame.
    pub fn free_bytes(&self) -> usize {
        self.ring.free_bytes()
    }
}

/// Whether this capture can be called bit-perfect, and why or why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BitPerfect {
    /// The OS confirmed the format, the request was honoured in full, and no
    /// counter moved. The only outcome that supports the claim.
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

impl BitPerfect {
    /// Whether the claim may be made. True for exactly one variant.
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed)
    }

    /// A one-line summary suitable for a report or a UI badge.
    pub fn summary(&self) -> String {
        match self {
            Self::Confirmed => "bit-perfect, confirmed against the OS".to_owned(),
            Self::Refuted { reasons } => format!("not bit-perfect: {}", reasons.join("; ")),
            Self::Unconfirmed { reasons } => {
                format!("bit-perfect not established: {}", reasons.join("; "))
            }
        }
    }
}

/// Weighs the evidence. The one place a bit-perfect claim can come from.
///
/// Kept a free function over plain inputs so every branch is testable without a
/// device: the rule is more important than the plumbing around it.
pub fn verdict(
    negotiated: &Negotiated,
    diagnostics: Diagnostics,
    verification: &Verification,
) -> BitPerfect {
    let mut refuting = Vec::new();
    let mut missing = Vec::new();

    if let Verification::Disagrees { divergences, .. } = verification {
        refuting.push(format!(
            "the operating system reports a different format ({})",
            divergences.join(", ")
        ));
    }
    if !negotiated.mode.could_be_bit_perfect() {
        refuting.push(format!(
            "the stream was opened {}, where the OS mixer owns the device",
            negotiated.mode.as_str()
        ));
    }
    if negotiated.transport == Transport::Converting {
        refuting.push("the device was reached through a converting path".to_owned());
    }
    if !diagnostics.is_clean() {
        refuting.push(format!(
            "the capture lost data: {} overruns, {} dropped frames, {} stream errors",
            diagnostics.overruns, diagnostics.dropped_frames, diagnostics.stream_errors
        ));
    }
    if !refuting.is_empty() {
        return BitPerfect::Refuted { reasons: refuting };
    }

    if let Verification::Unavailable { why } = verification {
        missing.push(why.clone());
    }
    if !negotiated.honoured() {
        missing.push(format!(
            "the request was not honoured in full ({})",
            negotiated
                .divergences
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if negotiated.transport == Transport::Unknown {
        missing.push("the platform does not say what sits in front of the device".to_owned());
    }
    if missing.is_empty() {
        BitPerfect::Confirmed
    } else {
        BitPerfect::Unconfirmed { reasons: missing }
    }
}

/// A running capture.
///
/// Not `Send`: a [`cpal::Stream`] is bound to the thread that built it on some
/// backends, so the whole handle stays put. The ring's reading end is the thing
/// that crosses threads, and it is returned separately by [`Capture::start`] for
/// exactly that reason.
pub struct Capture {
    stream: cpal::Stream,
    negotiated: Negotiated,
    counters: Arc<Counters>,
    verification: Verification,
    key: DeviceKey,
    name: String,
}

impl Capture {
    /// Opens the device, starts the stream, and asks the OS what it really did.
    ///
    /// Returns the handle and the reading end of the ring. The caller owns the
    /// draining: this crate's job ends where the bytes leave the callback.
    ///
    /// # Errors
    ///
    /// If the device is absent, offers no §8 configuration matching the request,
    /// or refuses the stream.
    pub fn start(request: &Request) -> Result<(Self, RingReader)> {
        let snapshot = devices::enumerate();
        let report = snapshot
            .get(&request.device)
            .ok_or_else(|| Error::NoSuchDevice {
                direction: Direction::Input,
                key: request.device.clone(),
            })?
            .clone();
        Self::start_on(&report, request)
    }

    /// As [`Capture::start`], for a caller that already has the device report
    /// and would rather not enumerate again.
    ///
    /// # Errors
    ///
    /// As [`Capture::start`].
    pub fn start_on(report: &DeviceReport, request: &Request) -> Result<(Self, RingReader)> {
        let matrix = Matrix::from_report(&report.input, Direction::Input);
        let chosen = choose(&matrix, request, report)?;
        let (mode, mode_divergence) = negotiate_mode(request.mode, report.transport);

        let mut divergences = Vec::new();
        if let Some(rate) = request.rate
            && rate != chosen.rate
        {
            divergences.push(Divergence {
                field: "rate",
                requested: format!("{} Hz", rate.hz()),
                granted: format!("{} Hz", chosen.rate.hz()),
            });
        }
        if let Some(channels) = request.channels
            && channels != chosen.channels
        {
            divergences.push(Divergence {
                field: "channels",
                requested: channels.to_string(),
                granted: chosen.channels.to_string(),
            });
        }
        if let Some(format) = request.format
            && format != chosen.format
        {
            divergences.push(Divergence {
                field: "format",
                requested: format!("{format:?}"),
                granted: format!("{:?}", chosen.format),
            });
        }
        divergences.extend(mode_divergence);

        let cpal_format = probe::to_cpal(chosen.format);
        let storage = probe::storage_format(cpal_format).ok_or_else(|| Error::NoConfiguration {
            device: report.label(),
            wanted: describe(request),
            offered: describe_matrix(&matrix),
        })?;
        let buffer_size = match request.buffer_frames {
            Some(frames) => cpal::BufferSize::Fixed(frames),
            None => cpal::BufferSize::Default,
        };
        let config = cpal::StreamConfig {
            channels: chosen.channels,
            sample_rate: chosen.rate.hz(),
            buffer_size,
        };

        let negotiated = Negotiated {
            rate: chosen.rate,
            channels: chosen.channels,
            format: chosen.format,
            storage,
            mode,
            mode_requested: request.mode,
            transport: report.transport,
            buffer: match buffer_size {
                cpal::BufferSize::Fixed(n) => format!("{n} frames, fixed"),
                cpal::BufferSize::Default => "backend default".to_owned(),
            },
            divergences,
        };

        let device = devices::open(&request.device, Direction::Input)?;
        let counters = Arc::new(Counters::default());
        let frame_bytes = negotiated.frame_bytes();
        let (writer, reader) =
            buffers::ring(frame_bytes, negotiated.rate.hz(), request.ring_millis);

        let mut sink = Sink::new(writer, Arc::clone(&counters), frame_bytes);
        let error_counters = Arc::clone(&counters);
        let key_for_log = request.device.to_string();
        let stream = device.build_input_stream_raw(
            config,
            cpal_format,
            move |data, _info| sink.on_data(data.bytes()),
            move |err| {
                // Not the audio thread. An allocation here is fine, and losing
                // the one message that explains a failure would not be.
                let text = err.to_string();
                // §42 forbids logging from a routine audio callback. This is the
                // error callback, which is not routine: it fires once per fault,
                // it is the only place the host's own words are available, and
                // the line above already allocates. A capture that lost frames
                // at 14:32 and a driver that complained at 14:32 are the same
                // event, and a log is where anybody sees that.
                tracing::error!(device = %key_for_log, error = %text, "capture stream error");
                error_counters.record_error(&text);
            },
            None,
        )?;
        stream.play()?;
        tracing::info!(
            device = %request.device,
            name = %report.name,
            rate = negotiated.rate.hz(),
            channels = negotiated.channels,
            format = ?negotiated.format,
            storage = ?negotiated.storage,
            mode = %negotiated.mode.as_str(),
            buffer = %negotiated.buffer,
            "capture stream started"
        );
        if !negotiated.honoured() {
            // A capture that is not what was asked for is the single most common
            // cause of a rip that sounds wrong, and §9 says so out loud rather
            // than quietly settling for less.
            tracing::warn!(
                device = %request.device,
                divergences = %negotiated
                    .divergences
                    .iter()
                    .map(|d| format!("{}: asked {}, got {}", d.field, d.requested, d.granted))
                    .collect::<Vec<_>>()
                    .join("; "),
                "the backend did not honour the request"
            );
        }

        // Ask the OS only once the stream is actually running: a PCM that has
        // not been prepared reports nothing to compare against.
        std::thread::sleep(VERIFY_SETTLE);
        let verification = verify::against(
            &request.device,
            Direction::Input,
            Expected {
                rate: negotiated.rate.hz(),
                channels: negotiated.channels,
                format: cpal_format,
            },
        );

        match &verification {
            Verification::Agrees(_) => tracing::info!(
                device = %request.device,
                report = %verification.evidence(),
                "the OS confirmed the stream configuration"
            ),
            // The OS contradicting the backend is the one case that is not
            // routine: it means the stream is not what the backend said it was.
            Verification::Disagrees { .. } => tracing::warn!(
                device = %request.device,
                report = %verification.evidence(),
                "the OS contradicted the stream configuration"
            ),
            // Not an error. Most hosts have nothing to ask, and §38's rule is
            // that an unverified capture is recorded as unverified, not refused.
            Verification::Unavailable { .. } => tracing::debug!(
                device = %request.device,
                report = %verification.evidence(),
                "the OS had nothing to confirm"
            ),
        }

        Ok((
            Self {
                stream,
                negotiated,
                counters,
                verification,
                key: request.device.clone(),
                name: report.name.clone(),
            },
            reader,
        ))
    }

    /// What the backend agreed to, and how it differs from the ask.
    pub const fn negotiated(&self) -> &Negotiated {
        &self.negotiated
    }

    /// What the operating system said, if anything.
    pub const fn verification(&self) -> &Verification {
        &self.verification
    }

    /// The four persisted counters, as of now.
    pub fn diagnostics(&self) -> Diagnostics {
        self.counters.snapshot()
    }

    /// Frames captured so far, per channel.
    pub fn frames(&self) -> u64 {
        self.counters.frames()
    }

    /// The live counters, for a worker that wants to watch them.
    pub fn counters(&self) -> &Arc<Counters> {
        &self.counters
    }

    /// Whether this capture can be called bit-perfect, as of now.
    pub fn verdict(&self) -> BitPerfect {
        verdict(&self.negotiated, self.diagnostics(), &self.verification)
    }

    /// Everything §38 wants persisted about this capture's provenance.
    pub fn info(&self) -> CaptureInfo {
        CaptureInfo {
            rate: self.negotiated.rate,
            channels: self.negotiated.channels,
            storage_format: self.negotiated.storage,
            capture_mode: self.negotiated.mode,
            host_api: Some(self.key.host().to_owned()),
            device_id: Some(self.key.to_string()),
            device_name: Some(self.name.clone()),
            os_verified: self.verification.confirms(),
            os_report: Some(self.verification.evidence()),
            // Not a negotiated fact: a phono stage does not announce its curve
            // over USB. The caller that knows what the operator declared puts it
            // on with `with_eq`; unknown until somebody says otherwise (§51).
            eq: CaptureEq::Unknown,
        }
    }

    /// Stops the stream and returns the final counters.
    ///
    /// Dropping the handle does the same thing without reporting; this exists so
    /// the caller has somewhere to get the numbers from.
    pub fn stop(self) -> Diagnostics {
        let final_counts = self.counters.snapshot();
        let level_matters = !final_counts.is_clean();
        drop(self.stream);
        if level_matters {
            // A lost frame is not a warning somewhere in a log, it is the reason
            // the rip has to be done again, so it goes out at `warn`.
            tracing::warn!(
                device = %self.key,
                overruns = final_counts.overruns,
                underruns = final_counts.underruns,
                dropped_frames = final_counts.dropped_frames,
                stream_errors = final_counts.stream_errors,
                "capture stream stopped with defects"
            );
        } else {
            tracing::info!(device = %self.key, frames = self.counters.frames(), "capture stream stopped clean");
        }
        final_counts
    }
}

/// Picks the configuration closest to what was asked for.
///
/// Anything the caller pinned is a hard filter, not a preference: a request for
/// 96 kHz that the device cannot do is an error, not a quiet 48 kHz. §9 exists
/// because the alternative is a capture that is worse than the one asked for and
/// says nothing about it.
fn choose(matrix: &Matrix, request: &Request, report: &DeviceReport) -> Result<Capability> {
    let filtered = Matrix {
        direction: matrix.direction,
        entries: matrix
            .entries
            .iter()
            .filter(|c| c.is_usable())
            .filter(|c| request.rate.is_none_or(|r| r == c.rate))
            .filter(|c| request.channels.is_none_or(|ch| ch == c.channels))
            .filter(|c| request.format.is_none_or(|f| f == c.format))
            .cloned()
            .collect(),
    };
    filtered
        .suggest()
        .cloned()
        .ok_or_else(|| Error::NoConfiguration {
            device: report.label(),
            wanted: describe(request),
            offered: describe_matrix(matrix),
        })
}

/// Maps a requested mode onto what the path can actually give.
///
/// On ALSA the mode *is* the device id: opening a `hw:` PCM takes exclusive
/// ownership of it and a second opener gets `EBUSY`, while `plughw:` and the
/// virtual PCMs are shared by construction. So there is nothing to ask the
/// backend for - the answer is already determined by which path the user picked,
/// and the honest thing is to say so rather than report the request back.
///
/// Shared with playback, which negotiates the same way for the same reason: the
/// rule is about the path to the device, not about which direction audio is
/// travelling along it.
pub(crate) fn negotiate_mode(
    requested: CaptureMode,
    transport: Transport,
) -> (CaptureMode, Option<Divergence>) {
    let granted = match (requested, transport) {
        (_, Transport::DirectHardware) => requested,
        (CaptureMode::Shared, _) => CaptureMode::Shared,
        // A converting or virtual path cannot give exclusive or native access,
        // whatever was asked for.
        (_, _) => CaptureMode::Shared,
    };
    let divergence = (granted != requested).then(|| Divergence {
        field: "mode",
        requested: requested.as_str().to_owned(),
        granted: granted.as_str().to_owned(),
    });
    (granted, divergence)
}

/// Renders a request for an error message.
fn describe(request: &Request) -> String {
    let mut parts = Vec::new();
    if let Some(rate) = request.rate {
        parts.push(format!("{} Hz", rate.hz()));
    }
    if let Some(format) = request.format {
        parts.push(format!("{format:?}"));
    }
    if let Some(channels) = request.channels {
        parts.push(format!("{channels} ch"));
    }
    if parts.is_empty() {
        "any §8 configuration".to_owned()
    } else {
        parts.join(" ")
    }
}

/// Renders what a device does offer, for the same message.
fn describe_matrix(matrix: &Matrix) -> String {
    if matrix.is_empty() {
        return "nothing §8 can use".to_owned();
    }
    let rates: Vec<String> = matrix.rates().iter().map(|r| r.hz().to_string()).collect();
    let formats: Vec<String> = matrix.formats().iter().map(|f| format!("{f:?}")).collect();
    let channels: Vec<String> = matrix.channel_counts().iter().map(u16::to_string).collect();
    format!(
        "{} Hz; {}; {} ch",
        rates.join(" "),
        formats.join(" "),
        channels.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{ConfigRange, DirectionReport};
    use crate::verify::OsReport;

    fn counters() -> Arc<Counters> {
        Arc::new(Counters::default())
    }

    /// 2 ch of 24-bit padded: CPAL's I24 is four bytes wide.
    const FRAME: usize = 8;

    fn sink(capacity_frames: u32) -> (Sink, RingReader, Arc<Counters>) {
        let c = counters();
        let (w, r) = buffers::ring(FRAME, capacity_frames * 2, buffers::MIN_MILLIS);
        (Sink::new(w, Arc::clone(&c), FRAME), r, c)
    }

    fn negotiated(
        mode: CaptureMode,
        transport: Transport,
        divergences: Vec<Divergence>,
    ) -> Negotiated {
        Negotiated {
            rate: SampleRate(96_000),
            channels: 2,
            format: SampleFormat::S32,
            storage: StorageFormat::Int32,
            mode,
            mode_requested: CaptureMode::Exclusive,
            transport,
            buffer: "backend default".to_owned(),
            divergences,
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

    fn range(channels: u16, min: u32, max: u32, format: SampleFormat) -> ConfigRange {
        ConfigRange {
            channels,
            min_rate: min,
            max_rate: max,
            sample_format: format!("{format:?}"),
            format: Some(format),
            bytes_per_sample: format.bytes_per_sample(),
            buffer_frames: Some((64, 8192)),
        }
    }

    fn device(configs: Vec<ConfigRange>, transport: Transport) -> DeviceReport {
        DeviceReport {
            key: DeviceKey::new("alsa", "hw:CARD=0,DEV=0"),
            name: "Test Converter".to_owned(),
            manufacturer: None,
            driver: None,
            device_type: String::new(),
            interface: String::new(),
            transport,
            is_default_input: false,
            is_default_output: false,
            input: DirectionReport {
                supported: !configs.is_empty(),
                configs,
                default: None,
            },
            output: DirectionReport::default(),
            problems: Vec::new(),
        }
    }

    #[test]
    fn the_callback_passes_bytes_through_untouched() {
        let (mut s, mut r, c) = sink(48_000);
        let payload: Vec<u8> = (0..=255u8).cycle().take(FRAME * 64).collect();
        s.on_data(&payload);
        let mut out = vec![0u8; payload.len()];
        assert!(r.read_exact(&mut out));
        assert_eq!(out, payload, "§9: nothing may touch the samples");
        assert_eq!(c.frames(), 64);
        assert_eq!(c.callbacks(), 1);
        assert!(c.snapshot().is_clean());
    }

    #[test]
    fn a_full_ring_costs_a_whole_callback_and_counts_every_frame_of_it() {
        let (mut s, _r, c) = sink(1_000);
        let capacity = s.ring.capacity();
        s.on_data(&vec![0u8; capacity]);
        assert!(c.snapshot().is_clean(), "the first one fits");

        s.on_data(&[0u8; FRAME * 10]);
        let d = c.snapshot();
        assert_eq!(d.overruns, 1);
        assert_eq!(d.dropped_frames, 10, "the whole callback, not a part of it");
        assert!(!d.is_clean());
        assert_eq!(c.frames(), capacity as u64 / FRAME as u64);
    }

    #[test]
    fn an_empty_callback_is_the_only_starvation_this_side_can_see() {
        let (mut s, _r, c) = sink(48_000);
        s.on_data(&[]);
        assert_eq!(c.snapshot().underruns, 1);
        assert_eq!(c.snapshot().overruns, 0);
        assert_eq!(c.callbacks(), 1);
    }

    #[test]
    fn a_backend_xrun_message_counts_as_an_underrun_as_well_as_an_error() {
        let c = counters();
        c.record_error("ALSA function 'snd_pcm_readi' failed: xrun");
        let d = c.snapshot();
        assert_eq!(d.stream_errors, 1);
        assert_eq!(d.underruns, 1);

        c.record_error("device disconnected");
        let d = c.snapshot();
        assert_eq!(d.stream_errors, 2);
        assert_eq!(d.underruns, 1, "not everything is an xrun");
    }

    #[test]
    fn bit_perfect_needs_the_os_to_say_so() {
        let n = negotiated(CaptureMode::Exclusive, Transport::DirectHardware, vec![]);
        assert!(verdict(&n, Diagnostics::default(), &agrees()).is_confirmed());

        // Same capture, same clean counters, no OS answer. Not a pass.
        let unchecked = Verification::Unavailable {
            why: "no verifier here".to_owned(),
        };
        let v = verdict(&n, Diagnostics::default(), &unchecked);
        assert!(!v.is_confirmed());
        assert!(matches!(v, BitPerfect::Unconfirmed { .. }));
        assert!(v.summary().contains("no verifier here"));
    }

    #[test]
    fn the_os_contradicting_the_backend_refutes_it_outright() {
        let n = negotiated(CaptureMode::Exclusive, Transport::DirectHardware, vec![]);
        let disagrees = Verification::Disagrees {
            report: Box::new(OsReport {
                source: "test".to_owned(),
                format: Some("S16_LE".to_owned()),
                rate: Some(8_000),
                channels: Some(1),
                period_frames: None,
                buffer_frames: None,
                access: None,
                raw: String::new(),
            }),
            divergences: vec!["rate: asked 96000 Hz, hardware 8000 Hz".to_owned()],
        };
        let v = verdict(&n, Diagnostics::default(), &disagrees);
        assert!(matches!(v, BitPerfect::Refuted { .. }));
        assert!(v.summary().contains("8000 Hz"));
    }

    #[test]
    fn dropped_frames_refute_it_however_good_the_format_was() {
        let n = negotiated(CaptureMode::Exclusive, Transport::DirectHardware, vec![]);
        let lossy = Diagnostics {
            overruns: 3,
            dropped_frames: 4096,
            ..Default::default()
        };
        let v = verdict(&n, lossy, &agrees());
        assert!(matches!(v, BitPerfect::Refuted { .. }));
        assert!(v.summary().contains("4096 dropped frames"));
    }

    #[test]
    fn a_shared_stream_is_refuted_before_anything_else_is_considered() {
        let n = negotiated(CaptureMode::Shared, Transport::Converting, vec![]);
        let v = verdict(&n, Diagnostics::default(), &agrees());
        assert!(matches!(v, BitPerfect::Refuted { .. }));
        assert!(v.summary().contains("converting path"));
    }

    #[test]
    fn an_unhonoured_request_leaves_the_claim_unestablished() {
        let n = negotiated(
            CaptureMode::Exclusive,
            Transport::DirectHardware,
            vec![Divergence {
                field: "rate",
                requested: "192000 Hz".to_owned(),
                granted: "96000 Hz".to_owned(),
            }],
        );
        assert!(!n.honoured());
        let v = verdict(&n, Diagnostics::default(), &agrees());
        assert!(matches!(v, BitPerfect::Unconfirmed { .. }));
        assert!(v.summary().contains("192000 Hz"));
    }

    #[test]
    fn a_converting_path_cannot_be_opened_exclusively_and_says_so() {
        let (mode, divergence) = negotiate_mode(CaptureMode::Exclusive, Transport::Converting);
        assert_eq!(mode, CaptureMode::Shared);
        let d = divergence.expect("the downgrade must be reported");
        assert_eq!(d.field, "mode");
        assert!(d.to_string().contains("exclusive"));

        let (mode, divergence) = negotiate_mode(CaptureMode::Exclusive, Transport::DirectHardware);
        assert_eq!(mode, CaptureMode::Exclusive);
        assert!(divergence.is_none());
    }

    #[test]
    fn asking_for_shared_on_hardware_gets_shared_without_complaint() {
        let (mode, divergence) = negotiate_mode(CaptureMode::Shared, Transport::DirectHardware);
        assert_eq!(mode, CaptureMode::Shared);
        assert!(divergence.is_none());
    }

    #[test]
    fn a_pinned_field_the_device_cannot_do_is_an_error_not_a_downgrade() {
        let report = device(
            vec![range(2, 44_100, 48_000, SampleFormat::S32)],
            Transport::DirectHardware,
        );
        let matrix = Matrix::from_report(&report.input, Direction::Input);
        let request = Request::new(report.key.clone()).at(SampleRate(192_000));
        let err = choose(&matrix, &request, &report).expect_err("192 kHz is not on offer");
        let text = err.to_string();
        assert!(text.contains("192000"), "{text}");
        assert!(
            text.contains("44100"),
            "the message says what is available: {text}"
        );
    }

    #[test]
    fn leaving_everything_open_takes_the_best_the_device_offers() {
        let report = device(
            vec![
                range(2, 44_100, 192_000, SampleFormat::S16),
                range(2, 44_100, 192_000, SampleFormat::S32),
            ],
            Transport::DirectHardware,
        );
        let matrix = Matrix::from_report(&report.input, Direction::Input);
        let chosen = choose(&matrix, &Request::new(report.key.clone()), &report).unwrap();
        assert_eq!(chosen.rate, SampleRate(96_000), "the archival default");
        assert_eq!(chosen.format, SampleFormat::S32, "widest integer on offer");
        assert_eq!(chosen.channels, 2);
    }

    #[test]
    fn a_device_with_nothing_usable_says_what_it_does_have() {
        let report = device(vec![], Transport::DirectHardware);
        let matrix = Matrix::from_report(&report.input, Direction::Input);
        let err = choose(&matrix, &Request::new(report.key.clone()), &report).unwrap_err();
        assert!(err.to_string().contains("nothing §8 can use"));
    }

    #[test]
    fn a_frame_is_measured_at_the_storage_width_not_the_logical_one() {
        let n = negotiated(CaptureMode::Exclusive, Transport::DirectHardware, vec![]);
        assert_eq!(n.frame_bytes(), 8);
        assert_eq!(n.bytes_per_second(), 8 * 96_000);
    }
}
