/*
 *  source.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Capture without a sound card: the simulated source the tests record from.
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

//! Capture without a sound card: the simulated source the tests record from.
//!
//! The plan calls this the single highest-leverage testing decision in it, and
//! the reason is arithmetic. WP-05's exit criterion is a 90-minute 24/192 soak
//! and WP-06's is a kill-at-random-point suite that has to run many times. Tied
//! to real hardware, neither runs in CI, neither runs on a machine whose
//! converter is busy, and neither is deterministic. Driven from here they are
//! all three.
//!
//! [`Simulated`] presents the same surface as [`Capture`] through the [`Source`]
//! trait, pushes bytes through the same [`Sink`], and therefore exercises the
//! same overrun accounting and the same ring. What it does *not* share is the
//! real-time constraint: its feeding thread is an ordinary thread and may do
//! I/O, which is how [`Pattern::File`] can stream a corpus rip through the
//! pipeline.
//!
//! # It can never be bit-perfect
//!
//! [`Negotiated::simulated`] reports an unknown transport and a shared mode, so
//! [`crate::capture::verdict`] refuses the bit-perfect claim no matter how clean
//! the run was. A simulated capture that could pass for a verified one would be
//! a trap worth more than the tests are worth.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use vcw_types::{CaptureInfo, Diagnostics, SampleRate, StorageFormat};

use crate::buffers::{self, RingReader};
use crate::capture::{BitPerfect, Capture, Counters, Negotiated, Sink, verdict};
use crate::error::{Error, Result};
use crate::verify::Verification;

/// What a running capture can be asked, whatever is producing the bytes.
///
/// WP-05's writer and WP-06's recovery are written against this, so both can be
/// driven by a file in CI and by a turntable on the bench without a line
/// changing between them.
pub trait Source {
    /// The configuration the bytes are arriving in.
    fn negotiated(&self) -> &Negotiated;
    /// The four persisted counters, as of now.
    fn diagnostics(&self) -> Diagnostics;
    /// Frames delivered so far, per channel.
    fn frames(&self) -> u64;
    /// What the operating system said, if it was asked.
    fn verification(&self) -> &Verification;
    /// Everything §38 wants persisted about provenance.
    fn info(&self) -> CaptureInfo;

    /// Whether this capture can be called bit-perfect, as of now.
    ///
    /// Defaulted, because the rule belongs to [`verdict`] and not to any one
    /// source. A source that could override it could also lie.
    fn verdict(&self) -> BitPerfect {
        verdict(self.negotiated(), self.diagnostics(), self.verification())
    }
}

impl Source for Capture {
    fn negotiated(&self) -> &Negotiated {
        Self::negotiated(self)
    }
    fn diagnostics(&self) -> Diagnostics {
        Self::diagnostics(self)
    }
    fn frames(&self) -> u64 {
        Self::frames(self)
    }
    fn verification(&self) -> &Verification {
        Self::verification(self)
    }
    fn info(&self) -> CaptureInfo {
        Self::info(self)
    }
}

/// How fast a simulated capture delivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// Wall-clock, the way a device does. What a soak test needs, and the only
    /// setting under which overrun behaviour means anything.
    RealTime,
    /// Flat out, dropping whatever the consumer cannot take. Turns a 90-minute
    /// soak into seconds, at the cost of telling you nothing about timing.
    ///
    /// **A run at this pace cannot be byte-verified**, and that is not a defect
    /// in the verifier. The ring overruns almost immediately, every overrun
    /// discards a whole callback, and the writer's frame index therefore stops
    /// agreeing with the source's - so frame *n* in the project holds the sample
    /// the source produced for some later frame. What this pace is genuinely for
    /// is provoking that loss on purpose: it is how
    /// `a_reader_that_falls_behind_overruns_instead_of_blocking_the_producer`
    /// proves §10's contract, and it is §41's dropped-frame case.
    Fast,
    /// Flat out, but waiting for room instead of overrunning.
    ///
    /// No device behaves like this - a device never waits for its consumer, and
    /// [`Sink::on_data`] must never block, which is why the waiting is done
    /// *here*, on the feeding thread, and not in the sink. So this is a test
    /// instrument and nothing else.
    ///
    /// It is the only pace that is both fast and verifiable, which is what makes
    /// it the one a CI run can use: the source is driven by the writer's own
    /// drain rate, nothing is dropped, and the written frame index still equals
    /// the source frame index, so every byte can be recomputed and compared.
    /// What it measures is throughput, never latency.
    Metered,
}

/// Where a simulated capture's bytes come from.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Generated from the frame and channel index, so a reader can recompute
    /// every byte it should have received and compare. This is what makes an
    /// end-to-end capture test a *verification* rather than a smoke test.
    Deterministic,
    /// Interleaved PCM from a byte range of a file, cycled if the capture
    /// outlasts it.
    ///
    /// No header parsing here. The bytes in the range must already be in the
    /// negotiated format, and finding that range in a container is the caller's
    /// job - `cli::wavfile` does it for the WAVs in `/data2/source_rips`. This
    /// crate has no business knowing about RIFF, and a source that guessed at a
    /// header would be a source that could guess wrong.
    ///
    /// Range rather than whole file because a real rip is a container: the
    /// audio starts 44 bytes in at the earliest, and what follows the audio is
    /// metadata that would arrive as a burst of noise at the wrap.
    File {
        /// The file.
        path: PathBuf,
        /// First byte of audio.
        offset: u64,
        /// Bytes of audio, or `None` for everything to the end of the file.
        /// Clamped to what is actually there either way.
        bytes: Option<u64>,
    },
    /// Digital black. Cheap, and the right choice when the test is about
    /// plumbing rather than payload.
    Silence,
}

/// Faults to inject, for the failure paths that are hard to provoke on purpose.
///
/// R9 in the plan is "device removal mid-capture corrupts project", mitigated by
/// fault-injection tests. A USB cable cannot be pulled by CI, so it is pulled
/// here instead.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Faults {
    /// Stop delivering after this many frames, silently, the way an unplugged
    /// device does. The capture does not end; it simply stops receiving.
    pub vanish_after: Option<u64>,
    /// Report a stream error after this many frames, and keep going.
    pub error_after: Option<u64>,
    /// Deliver an empty callback after this many frames, which is the one form
    /// of device starvation visible from this side of CPAL.
    pub starve_after: Option<u64>,
}

impl Faults {
    /// No faults. The ordinary case.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            vanish_after: None,
            error_after: None,
            starve_after: None,
        }
    }

    /// A device unplugged after this many frames.
    #[must_use]
    pub const fn unplug_after(frames: u64) -> Self {
        Self {
            vanish_after: Some(frames),
            error_after: Some(frames),
            starve_after: None,
        }
    }
}

/// A capture that is not a capture: bytes into the same ring, from somewhere else.
pub struct Simulated {
    negotiated: Negotiated,
    counters: Arc<Counters>,
    verification: Verification,
    stop: Arc<AtomicBool>,
    feeder: Option<JoinHandle<()>>,
}

impl Simulated {
    /// Frames per simulated callback. 10 ms at the negotiated rate, which is the
    /// order of magnitude a real device delivers and small enough that the ring
    /// floor is not trivially large by comparison.
    const CALLBACK_MILLIS: u32 = 10;

    /// Starts feeding a ring, and returns the handle and the ring's reading end.
    ///
    /// # Errors
    ///
    /// If [`Pattern::File`] names a file that cannot be opened or is empty.
    pub fn start(
        negotiated: Negotiated,
        pattern: &Pattern,
        pace: Pace,
        faults: Faults,
        ring_millis: u32,
    ) -> Result<(Self, RingReader)> {
        let frame_bytes = negotiated.frame_bytes();
        let rate = negotiated.rate.hz();
        let (writer, reader) = buffers::ring(frame_bytes, rate, ring_millis);
        let counters = Arc::new(Counters::default());
        let stop = Arc::new(AtomicBool::new(false));

        let chunk_frames =
            ((u64::from(rate) * u64::from(Self::CALLBACK_MILLIS)) / 1_000).max(1) as usize;
        let mut generator = Generator::new(pattern, frame_bytes, negotiated.storage)?;
        let mut sink = Sink::new(writer, Arc::clone(&counters), frame_bytes);
        let period = Duration::from_secs_f64(chunk_frames as f64 / f64::from(rate));
        let thread_stop = Arc::clone(&stop);
        let thread_counters = Arc::clone(&counters);

        let feeder = std::thread::Builder::new()
            .name("simulated-capture".to_owned())
            .spawn(move || {
                let mut scratch = vec![0u8; chunk_frames * frame_bytes];
                let mut frame: u64 = 0;
                let mut reported = false;
                let mut starved = false;
                let mut deadline = Instant::now();
                while !thread_stop.load(Ordering::Relaxed) {
                    // The error goes out before the silence begins. A device
                    // that vanishes without a word is a different fault, and
                    // asking for both must produce both.
                    if !reported && faults.error_after.is_some_and(|f| frame >= f) {
                        reported = true;
                        thread_counters.record_error("simulated device disconnected");
                    }
                    if faults.vanish_after.is_some_and(|f| frame >= f) {
                        // Unplugged. The stream does not end, it goes quiet -
                        // which is exactly the failure that is easy to miss.
                        std::thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                    // Metered: hold the chunk back until it will fit. Checked
                    // before the fill so a stop is honoured while waiting -
                    // a consumer that has stopped reading would otherwise keep
                    // this thread here until the ring drained, which it never
                    // would.
                    if pace == Pace::Metered {
                        while sink.free_bytes() < scratch.len() {
                            if thread_stop.load(Ordering::Relaxed) {
                                return;
                            }
                            std::thread::yield_now();
                        }
                    }
                    if !starved && faults.starve_after.is_some_and(|f| frame >= f) {
                        starved = true;
                        sink.on_data(&[]);
                    } else {
                        generator.fill(&mut scratch, frame, negotiated.channels);
                        sink.on_data(&scratch);
                    }
                    frame += chunk_frames as u64;
                    if pace == Pace::RealTime {
                        deadline += period;
                        let now = Instant::now();
                        if deadline > now {
                            std::thread::sleep(deadline - now);
                        }
                    }
                }
            })
            .map_err(|e| Error::io(Path::new("simulated-capture"), e))?;

        Ok((
            Self {
                negotiated,
                counters,
                verification: Verification::Unavailable {
                    why: "this is a simulated capture; no hardware was involved".to_owned(),
                },
                stop,
                feeder: Some(feeder),
            },
            reader,
        ))
    }

    /// A deterministic simulated capture at the given configuration, running in
    /// real time with no faults. The common case in a test.
    ///
    /// # Errors
    ///
    /// Only if the feeding thread cannot be spawned.
    pub fn deterministic(
        rate: SampleRate,
        channels: u16,
        pace: Pace,
    ) -> Result<(Self, RingReader)> {
        Self::deterministic_as(rate, channels, vcw_types::SampleFormat::S32, pace)
    }

    /// The same, in a chosen sample format.
    ///
    /// S32 is the default because it is the widest integer format and what a
    /// device negotiation asks for first, but it is not the only one a project
    /// can hold, and a caller pinning a format has a reason: a 32-bit project
    /// cannot be exported as FLAC while `flacenc` stops at 24 bits, so a
    /// simulated capture that could only be S32 made half of §33 untestable
    /// without hardware. [`Pattern::Deterministic`] already writes the low
    /// bytes of [`Simulated::expected_sample`] at the stored width, so the
    /// capture stays recomputable frame by frame whichever format it is in.
    ///
    /// # Errors
    ///
    /// Only if the feeding thread cannot be spawned.
    pub fn deterministic_as(
        rate: SampleRate,
        channels: u16,
        format: vcw_types::SampleFormat,
        pace: Pace,
    ) -> Result<(Self, RingReader)> {
        Self::start(
            Negotiated::simulated(rate, channels, format),
            &Pattern::Deterministic,
            pace,
            Faults::none(),
            buffers::DEFAULT_MILLIS,
        )
    }

    /// The sample a deterministic source produces for this frame and channel.
    ///
    /// A reader that knows the frame index can recompute every sample it should
    /// have received, which turns "the capture completed" into "the capture
    /// contains exactly the right bytes in exactly the right order".
    #[must_use]
    pub const fn expected_sample(frame: u64, channel: u16) -> u32 {
        let x = frame
            .wrapping_mul(2_654_435_761)
            .wrapping_add(channel as u64 + 1);
        (x ^ (x >> 29)) as u32
    }

    /// Stops the feeder and returns the final counters.
    pub fn stop(mut self) -> Diagnostics {
        self.halt();
        self.counters.snapshot()
    }

    /// The live counters, for a worker that wants to watch them.
    pub fn counters(&self) -> &Arc<Counters> {
        &self.counters
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.feeder.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Simulated {
    fn drop(&mut self) {
        self.halt();
    }
}

impl Source for Simulated {
    fn negotiated(&self) -> &Negotiated {
        &self.negotiated
    }
    fn diagnostics(&self) -> Diagnostics {
        self.counters.snapshot()
    }
    fn frames(&self) -> u64 {
        self.counters.frames()
    }
    fn verification(&self) -> &Verification {
        &self.verification
    }
    fn info(&self) -> CaptureInfo {
        CaptureInfo {
            host_api: Some("simulated".to_owned()),
            device_id: None,
            device_name: Some("simulated source".to_owned()),
            ..CaptureInfo::unverified(
                self.negotiated.rate,
                self.negotiated.channels,
                self.negotiated.storage,
                self.negotiated.mode,
            )
        }
    }
}

/// Produces the bytes. Lives on the feeding thread, so it may do I/O.
#[derive(Debug)]
enum Generator {
    Deterministic {
        bytes_per_sample: usize,
    },
    File {
        handle: File,
        /// First byte of audio, and where a wrap seeks back to.
        offset: u64,
        /// Bytes of audio, already clamped to the file's real size.
        length: u64,
        /// Bytes taken since the last wrap.
        consumed: u64,
    },
    Silence,
}

impl Generator {
    fn new(pattern: &Pattern, frame_bytes: usize, storage: StorageFormat) -> Result<Self> {
        match pattern {
            Pattern::Deterministic => Ok(Self::Deterministic {
                bytes_per_sample: storage.bytes_per_sample(),
            }),
            Pattern::Silence => Ok(Self::Silence),
            Pattern::File {
                path,
                offset,
                bytes,
            } => {
                let mut handle = File::open(path).map_err(|e| Error::io(path, e))?;
                let on_disk = handle.metadata().map_err(|e| Error::io(path, e))?.len();
                // Clamped, not trusted. A WAV's declared data length is a number
                // in the file, and a rip that was cut short still declares the
                // length it meant to have - so a reader that believed it would
                // read past the end and call the short read a device fault.
                let available = on_disk.saturating_sub(*offset);
                let length = bytes.map_or(available, |want| want.min(available));
                if length < frame_bytes as u64 {
                    return Err(Error::NoConfiguration {
                        device: path.display().to_string(),
                        wanted: format!("at least one {frame_bytes}-byte frame"),
                        offered: format!("{length} bytes of audio from offset {offset}"),
                    });
                }
                handle
                    .seek(SeekFrom::Start(*offset))
                    .map_err(|e| Error::io(path, e))?;
                Ok(Self::File {
                    handle,
                    offset: *offset,
                    length,
                    consumed: 0,
                })
            }
        }
    }

    fn fill(&mut self, buffer: &mut [u8], first_frame: u64, channels: u16) {
        match self {
            Self::Silence => buffer.fill(0),
            Self::Deterministic { bytes_per_sample } => {
                let width = *bytes_per_sample;
                let mut offset = 0;
                let mut frame = first_frame;
                while offset + width * channels as usize <= buffer.len() {
                    for channel in 0..channels {
                        let value = Simulated::expected_sample(frame, channel).to_le_bytes();
                        buffer[offset..offset + width].copy_from_slice(&value[..width]);
                        offset += width;
                    }
                    frame += 1;
                }
            }
            Self::File {
                handle,
                offset,
                length,
                consumed,
            } => {
                let mut filled = 0;
                while filled < buffer.len() {
                    if *consumed >= *length {
                        // Wrap. A fixture shorter than the capture is the
                        // normal case, not an error.
                        if *length == 0 || handle.seek(SeekFrom::Start(*offset)).is_err() {
                            buffer[filled..].fill(0);
                            return;
                        }
                        *consumed = 0;
                    }
                    // Never past the end of the audio. Whatever follows a data
                    // chunk is metadata, and metadata read as samples is a burst
                    // of noise once per wrap.
                    let room = ((*length - *consumed) as usize).min(buffer.len() - filled);
                    match handle.read(&mut buffer[filled..filled + room]) {
                        // Short of the length the container declared. Silence
                        // from here rather than a wrap, because a wrap on a
                        // zero-length read is how a truncated file becomes an
                        // infinite loop.
                        Ok(0) => {
                            buffer[filled..].fill(0);
                            return;
                        }
                        Ok(n) => {
                            filled += n;
                            *consumed += n as u64;
                        }
                        Err(_) => {
                            buffer[filled..].fill(0);
                            return;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcw_types::SampleFormat;

    #[test]
    fn the_deterministic_pattern_is_reproducible_and_channel_aware() {
        assert_eq!(
            Simulated::expected_sample(12_345, 0),
            Simulated::expected_sample(12_345, 0)
        );
        assert_ne!(
            Simulated::expected_sample(12_345, 0),
            Simulated::expected_sample(12_345, 1),
            "channels must not be interchangeable, or a swap would go unnoticed"
        );
        assert_ne!(
            Simulated::expected_sample(0, 0),
            Simulated::expected_sample(1, 0)
        );
    }

    #[test]
    fn a_generated_buffer_decodes_back_to_the_pattern() {
        let mut g = Generator::new(&Pattern::Deterministic, 8, StorageFormat::Int32).unwrap();
        let mut buffer = vec![0u8; 8 * 4];
        g.fill(&mut buffer, 100, 2);
        for (i, frame) in (100..104u64).enumerate() {
            for channel in 0..2u16 {
                let at = i * 8 + channel as usize * 4;
                let got = u32::from_le_bytes(buffer[at..at + 4].try_into().unwrap());
                assert_eq!(got, Simulated::expected_sample(frame, channel));
            }
        }
    }

    #[test]
    fn a_short_file_wraps_rather_than_running_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pcm.raw");
        std::fs::write(&path, [1u8, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let whole = Pattern::File {
            path: path.clone(),
            offset: 0,
            bytes: None,
        };
        let mut g = Generator::new(&whole, 8, StorageFormat::Int32).unwrap();
        let mut buffer = vec![0u8; 24];
        g.fill(&mut buffer, 0, 2);
        assert_eq!(buffer, [1, 2, 3, 4, 5, 6, 7, 8].repeat(3));
    }

    #[test]
    fn a_range_skips_the_header_and_stops_before_the_trailer() {
        // The shape of a real rip: a header, the audio, and a metadata chunk
        // after it. Both ends matter. Reading the header would offset every
        // sample by its length; reading the trailer would deliver a burst of
        // noise once per wrap, and a soak that wrapped hourly would be finding
        // it hourly.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rip.wav");
        let mut file = b"HEADERXX".to_vec();
        file.extend_from_slice(&[1u8, 2, 3, 4, 5, 6, 7, 8]);
        file.extend_from_slice(b"LISTjunk");
        std::fs::write(&path, &file).unwrap();

        let ranged = Pattern::File {
            path: path.clone(),
            offset: 8,
            bytes: Some(8),
        };
        let mut g = Generator::new(&ranged, 8, StorageFormat::Int32).unwrap();
        let mut buffer = vec![0u8; 24];
        g.fill(&mut buffer, 0, 2);
        assert_eq!(
            buffer,
            [1, 2, 3, 4, 5, 6, 7, 8].repeat(3),
            "the range leaked into the header or the trailer"
        );

        // A declared length longer than the file is clamped, not trusted: a rip
        // cut short still declares the length it meant to have.
        let overclaimed = Pattern::File {
            path,
            offset: 8,
            bytes: Some(4_000),
        };
        let mut g = Generator::new(&overclaimed, 8, StorageFormat::Int32).unwrap();
        let mut buffer = vec![0u8; 16];
        g.fill(&mut buffer, 0, 2);
        assert_eq!(
            buffer,
            [
                1, 2, 3, 4, 5, 6, 7, 8, b'L', b'I', b'S', b'T', b'j', b'u', b'n', b'k'
            ],
            "a clamped range should still read to the end of the file and wrap"
        );
    }

    #[test]
    fn a_file_too_small_for_one_frame_is_refused_with_both_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tiny.raw");
        std::fs::write(&path, [1u8, 2]).unwrap();
        let tiny = Pattern::File {
            path,
            offset: 0,
            bytes: None,
        };
        let err = Generator::new(&tiny, 8, StorageFormat::Int32).unwrap_err();
        let text = err.to_string();
        assert!(text.contains('8'), "{text}");
        assert!(text.contains('2'), "{text}");
    }

    #[test]
    fn a_simulated_capture_can_never_be_called_bit_perfect() {
        let (sim, _reader) = Simulated::deterministic(SampleRate(48_000), 2, Pace::Fast).unwrap();
        let v = sim.verdict();
        assert!(!v.is_confirmed());
        assert!(!sim.info().os_verified);
        assert!(!sim.verification().was_checked());
    }

    #[test]
    fn a_fast_capture_delivers_the_bytes_the_pattern_promises() {
        let negotiated = Negotiated::simulated(SampleRate(48_000), 2, SampleFormat::S32);
        let frame_bytes = negotiated.frame_bytes();
        let (sim, mut reader) = Simulated::start(
            negotiated,
            &Pattern::Deterministic,
            Pace::Fast,
            Faults::none(),
            buffers::MIN_MILLIS,
        )
        .unwrap();

        // Drain the first 480 frames - one simulated callback - and check them.
        let wanted = 480 * frame_bytes;
        let mut got = vec![0u8; wanted];
        let deadline = Instant::now() + Duration::from_secs(5);
        while !reader.read_exact(&mut got) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        drop(sim);

        for frame in 0..480u64 {
            for channel in 0..2u16 {
                let at = frame as usize * frame_bytes + channel as usize * 4;
                let value = u32::from_le_bytes(got[at..at + 4].try_into().unwrap());
                assert_eq!(
                    value,
                    Simulated::expected_sample(frame, channel),
                    "frame {frame} channel {channel}"
                );
            }
        }
    }

    #[test]
    fn an_unplugged_device_goes_quiet_without_ending_the_capture() {
        let negotiated = Negotiated::simulated(SampleRate(48_000), 2, SampleFormat::S32);
        let (sim, mut reader) = Simulated::start(
            negotiated,
            &Pattern::Silence,
            Pace::Fast,
            Faults::unplug_after(480),
            buffers::MIN_MILLIS,
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while sim.diagnostics().stream_errors == 0 && Instant::now() < deadline {
            let mut sink = vec![0u8; 4096];
            reader.read(&mut sink);
            std::thread::yield_now();
        }
        let d = sim.stop();
        assert_eq!(d.stream_errors, 1, "the unplug is reported");
        assert!(
            !d.is_clean(),
            "and it is never mistaken for a clean capture"
        );
    }

    #[test]
    fn stopping_twice_is_harmless() {
        let (sim, _r) = Simulated::deterministic(SampleRate(48_000), 2, Pace::Fast).unwrap();
        let first = sim.stop();
        // Drop already halted it; this only has to not hang or panic.
        assert!(first.overruns < u64::MAX);
    }
}
