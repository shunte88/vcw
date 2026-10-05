/*
 *  features.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Windowed features from a stream of samples: the decode side of the split (§22).
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

//! Windowed features from a stream of samples: the decode side of the split.
//!
//! VRipr's detectors take a file path and decode it with Symphonia. VCW's cannot:
//! §22 asks for live analysis during a capture, and there is no file yet while a
//! record is turning. So the port splits in two, and this is the half that turns
//! samples into feature frames. Everything downstream - [`crate::silence`],
//! [`crate::spectral`], [`crate::hmm`] - sees only [`Frame`]s and never a sample,
//! a file or a device.
//!
//! That split is what makes the same detector serve both of §22's passes. Live, the
//! capture fan-out feeds [`Windows::push`] and frames appear as the audio does.
//! Afterwards, the project's stored blocks feed the same method and produce the same
//! frames, so the refine pass is not a second implementation of the first.
//!
//! Requirements: §22 (detection), §23 (what the frames are published as).
//!
//! # Why the numbers match VRipr's exactly
//!
//! WP-11's exit criterion is parity with VRipr on a labeled corpus, and parity is
//! only a meaningful claim if a difference in the *detector* cannot be hidden by a
//! difference in the *features*. So this module reproduces VRipr's arithmetic rather
//! than improving on it: channels are averaged into mono as `f32`, the sum of squares
//! accumulates in `f64`, the window is Hann over the real window length and then
//! zero-padded to the next power of two, and the magnitudes are the complex FFT's
//! first `n/2 + 1` bins. A real-input transform would be faster and would have made
//! every parity difference an argument about floating-point ordering.
//!
//! ```
//! use vcw_signal::features::{Frame, Shape, Windows};
//! use vcw_types::{SampleRate, StorageFormat};
//!
//! let shape = Shape::new(SampleRate(48_000), 2, StorageFormat::Int16, 100);
//! let mut windows = Windows::new(&shape);
//! let mut frames: Vec<Frame> = Vec::new();
//! // 4800 frames of stereo 16-bit silence is exactly one window.
//! windows.push(&vec![0u8; 4_800 * 2 * 2], &mut frames);
//! assert_eq!(frames.len(), 1);
//! ```

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

use vcw_types::{SampleRate, StorageFormat};

/// The analysis window VRipr used, and therefore the one parity is measured at.
///
/// 100 ms is a deliberate compromise rather than a tuned value: long enough that a
/// vinyl pop does not look like a frame of music, short enough that a boundary is
/// located to a tenth of a second before any interpolation. It is also the quantum
/// every position in this module is a multiple of, which is why
/// [`Frame`] carries no position of its own.
pub const DEFAULT_WINDOW_MILLIS: u32 = 100;

/// One analysis window, reduced to the two numbers §22 names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Root mean square of the mono mixdown, linear, `0.0..=1.0`.
    pub rms: f64,
    /// Spectral flatness, `0.0` perfectly tonal to `1.0` white noise.
    ///
    /// Zero when the extractor was built without spectral analysis, and zero for a
    /// trailing partial window, which is too short for a transform to say anything
    /// about. Both cases read as "tonal", which biases towards *music* and therefore
    /// towards not inventing a boundary.
    pub flatness: f64,
}

impl Frame {
    /// The level in dBFS, floored at -120 dB.
    ///
    /// The floor matters: an all-zero window has a true level of negative infinity,
    /// and a Gaussian fitted to negative infinity has no mean.
    #[must_use]
    pub fn level_db(&self) -> f64 {
        linear_to_db(self.rms)
    }
}

/// What the samples are, and how finely to chop them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// Frames per second.
    pub rate: SampleRate,
    /// Interleaved channel count.
    pub channels: usize,
    /// How the bytes are laid out.
    pub format: StorageFormat,
    /// Window length in milliseconds.
    pub window_millis: u32,
}

impl Shape {
    /// A shape for a capture.
    #[must_use]
    pub const fn new(
        rate: SampleRate,
        channels: usize,
        format: StorageFormat,
        window_millis: u32,
    ) -> Self {
        Self {
            rate,
            channels,
            format,
            window_millis,
        }
    }

    /// The same, at [`DEFAULT_WINDOW_MILLIS`].
    #[must_use]
    pub const fn at_default_window(
        rate: SampleRate,
        channels: usize,
        format: StorageFormat,
    ) -> Self {
        Self::new(rate, channels, format, DEFAULT_WINDOW_MILLIS)
    }

    /// Frames in one window.
    ///
    /// Truncated exactly as VRipr truncates it, so a rate that does not divide the
    /// window evenly produces the same window length in both.
    #[must_use]
    pub fn window_frames(&self) -> usize {
        let frames = f64::from(self.rate.hz()) * f64::from(self.window_millis) / 1_000.0;
        (frames as usize).max(1)
    }

    /// The window length in seconds, which is what a detector converts with.
    #[must_use]
    pub fn window_seconds(&self) -> f64 {
        f64::from(self.window_millis) / 1_000.0
    }

    /// Bytes in one frame of interleaved audio.
    #[must_use]
    pub const fn frame_bytes(&self) -> usize {
        self.format.bytes_per_sample() * self.channels
    }

    /// The frame a window index starts at, which is how a window number becomes a
    /// position in the capture's own timeline.
    #[must_use]
    pub fn frame_at(&self, window: usize) -> u64 {
        window as u64 * self.window_frames() as u64
    }
}

/// The spectral half of the extractor: a plan, a window and its scratch.
///
/// Separate so that a detector that only needs levels does not pay for a transform
/// it will not look at, and so that the allocation happens once at construction.
struct Spectral {
    fft: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    magnitudes: Vec<f32>,
}

impl Spectral {
    fn new(window_frames: usize) -> Self {
        let size = window_frames.next_power_of_two();
        let mut planner: FftPlanner<f32> = FftPlanner::new();
        let fft = planner.plan_fft_forward(size);
        let scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        // Hann over the real window length, not over the padded size: the padding
        // is zeroes and windowing them would do nothing but change the
        // denominator.
        let hann: Vec<f32> = (0..window_frames)
            .map(|i| {
                let divisor = (window_frames.saturating_sub(1)).max(1) as f64;
                let x = 2.0 * std::f64::consts::PI * i as f64 / divisor;
                (0.5 - 0.5 * x.cos()) as f32
            })
            .collect();
        Self {
            fft,
            hann,
            buffer: vec![Complex::new(0.0, 0.0); size],
            scratch,
            magnitudes: vec![0.0; size / 2 + 1],
        }
    }

    /// Flatness of one full window.
    fn flatness(&mut self, samples: &[f32]) -> f64 {
        for (slot, (sample, coefficient)) in self
            .buffer
            .iter_mut()
            .zip(samples.iter().zip(self.hann.iter()))
        {
            *slot = Complex::new(sample * coefficient, 0.0);
        }
        for slot in self
            .buffer
            .iter_mut()
            .skip(samples.len().min(self.hann.len()))
        {
            *slot = Complex::new(0.0, 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let bins = self.magnitudes.len();
        for (magnitude, bin) in self
            .magnitudes
            .iter_mut()
            .zip(self.buffer.iter().take(bins))
        {
            *magnitude = bin.norm();
        }
        flatness_of(&self.magnitudes)
    }
}

/// Turns a stream of samples into [`Frame`]s.
///
/// Fed in whatever sized pieces the caller has - a ring's worth during a capture, a
/// stored block afterwards - and it keeps the leftover part-window between calls, so
/// the frames it produces do not depend on how the audio was chopped up on the way
/// in. That property is what lets the live pass and the refine pass agree.
pub struct Windows {
    shape: Shape,
    window_frames: usize,
    pending: Vec<f32>,
    /// Bytes left over from a call that ended mid-frame, carried rather than
    /// dropped. See [`Windows::push`].
    spare_bytes: Vec<u8>,
    /// Samples left over from a call that ended mid-frame.
    spare_samples: Vec<f32>,
    spectral: Option<Spectral>,
    frames: u64,
}

impl std::fmt::Debug for Windows {
    /// Hand-written because the FFT plan behind [`Windows::spectral`] is a trait object
    /// with no `Debug`. What a caller wants to see anyway is the configuration and how
    /// far through the audio it is.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Windows")
            .field("shape", &self.shape)
            .field("window_frames", &self.window_frames)
            .field("spectral", &self.spectral.is_some())
            .field("frames", &self.frames)
            .field("pending", &self.pending.len())
            .finish()
    }
}

impl Windows {
    /// An extractor that reports levels only.
    ///
    /// What [`crate::silence`] needs, and about forty times cheaper than the
    /// spectral extractor, which is why the live pass uses it.
    #[must_use]
    pub fn new(shape: &Shape) -> Self {
        let window_frames = shape.window_frames();
        Self {
            shape: *shape,
            window_frames,
            pending: Vec::with_capacity(window_frames),
            spare_bytes: Vec::with_capacity(shape.frame_bytes()),
            spare_samples: Vec::with_capacity(shape.channels),
            spectral: None,
            frames: 0,
        }
    }

    /// An extractor that reports levels and spectral flatness.
    ///
    /// What [`crate::spectral`] and [`crate::hmm`] need.
    #[must_use]
    pub fn spectral(shape: &Shape) -> Self {
        let window_frames = shape.window_frames();
        Self {
            shape: *shape,
            window_frames,
            pending: Vec::with_capacity(window_frames),
            spare_bytes: Vec::with_capacity(shape.frame_bytes()),
            spare_samples: Vec::with_capacity(shape.channels),
            spectral: Some(Spectral::new(window_frames)),
            frames: 0,
        }
    }

    /// The shape it was built for.
    #[must_use]
    pub const fn shape(&self) -> &Shape {
        &self.shape
    }

    /// Frames in one window.
    #[must_use]
    pub const fn window_frames(&self) -> usize {
        self.window_frames
    }

    /// Whether flatness is being computed.
    #[must_use]
    pub const fn is_spectral(&self) -> bool {
        self.spectral.is_some()
    }

    /// Frames consumed so far, including the part-window in hand.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Consumes interleaved bytes in the configured storage format.
    ///
    /// Appends a [`Frame`] to `out` for every window completed by this call, and
    /// returns how many.
    ///
    /// A call that ends part way through a frame has its remainder **carried** to
    /// the next one. [`crate::meter::Meter::feed`] discards it instead, and the
    /// difference is not inconsistency: a meter that drops three bytes shows one
    /// slightly wrong needle, while an extractor that drops three bytes shifts the
    /// window alignment of everything after it and moves every boundary it later
    /// reports. The whole point of this module is that the frames do not depend on
    /// how the audio was chopped up on the way in.
    pub fn push(&mut self, bytes: &[u8], out: &mut Vec<Frame>) -> usize {
        let frame_bytes = self.shape.frame_bytes();
        if frame_bytes == 0 {
            return 0;
        }
        let before = out.len();
        if self.spare_bytes.is_empty() {
            let used = self.whole_frames(bytes, out);
            self.spare_bytes.extend_from_slice(&bytes[used..]);
        } else {
            // Complete the straddling frame first, out of a buffer that holds one
            // frame at most, then carry on with the rest of the slice in place.
            let wanted = frame_bytes - self.spare_bytes.len();
            let taken = wanted.min(bytes.len());
            self.spare_bytes.extend_from_slice(&bytes[..taken]);
            if self.spare_bytes.len() == frame_bytes {
                let joined = std::mem::take(&mut self.spare_bytes);
                self.whole_frames(&joined, out);
                self.spare_bytes = joined;
                self.spare_bytes.clear();
                let rest = &bytes[taken..];
                let used = self.whole_frames(rest, out);
                self.spare_bytes.extend_from_slice(&rest[used..]);
            }
        }
        out.len() - before
    }

    /// Consumes interleaved samples that have already been decoded.
    ///
    /// The path a synthesised signal takes in a test, and the path the parity
    /// harness takes for a WAV it decoded itself. Carries a straddling frame for
    /// the same reason [`Windows::push`] does.
    pub fn push_samples(&mut self, interleaved: &[f32], out: &mut Vec<Frame>) -> usize {
        let channels = self.shape.channels;
        if channels == 0 {
            return 0;
        }
        let before = out.len();
        let mut rest = interleaved;
        if !self.spare_samples.is_empty() {
            let wanted = channels - self.spare_samples.len();
            let taken = wanted.min(rest.len());
            self.spare_samples.extend_from_slice(&rest[..taken]);
            rest = &rest[taken..];
            if self.spare_samples.len() == channels {
                let sum: f32 = self.spare_samples.iter().sum();
                self.spare_samples.clear();
                self.accept(sum / channels as f32, out);
            }
        }
        let whole = rest.len() - rest.len() % channels;
        for frame in rest[..whole].chunks_exact(channels) {
            let sum: f32 = frame.iter().sum();
            self.accept(sum / channels as f32, out);
        }
        self.spare_samples.extend_from_slice(&rest[whole..]);
        out.len() - before
    }

    /// Decodes every whole frame in `bytes` and returns how many bytes that was.
    fn whole_frames(&mut self, bytes: &[u8], out: &mut Vec<Frame>) -> usize {
        let frame_bytes = self.shape.frame_bytes();
        let frames = bytes.len() / frame_bytes;
        for frame in 0..frames {
            let base = frame * self.shape.channels;
            let mut sum = 0.0f32;
            for channel in 0..self.shape.channels {
                sum += self
                    .shape
                    .format
                    .decode_sample(bytes, base + channel)
                    .unwrap_or(0.0);
            }
            self.accept(sum / self.shape.channels as f32, out);
        }
        frames * frame_bytes
    }

    /// Emits whatever is left as a final short window.
    ///
    /// Called at the end of a capture and nowhere else. The frame it produces is a
    /// true RMS over fewer samples and a flatness of zero, because a transform over
    /// a fifth of a window says more about the window than about the audio - which
    /// is what VRipr does, and reads as "tonal", so the last fraction of a second
    /// of a side is never mistaken for a gap.
    pub fn flush(&mut self, out: &mut Vec<Frame>) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let rms = root_mean_square(&self.pending);
        self.pending.clear();
        out.push(Frame { rms, flatness: 0.0 });
        true
    }

    /// Takes one mono sample, and emits a frame if it completed a window.
    fn accept(&mut self, sample: f32, out: &mut Vec<Frame>) {
        self.pending.push(sample);
        self.frames += 1;
        if self.pending.len() < self.window_frames {
            return;
        }
        let rms = root_mean_square(&self.pending);
        let flatness = match self.spectral.as_mut() {
            Some(spectral) => spectral.flatness(&self.pending),
            None => 0.0,
        };
        self.pending.clear();
        out.push(Frame { rms, flatness });
    }
}

/// RMS of a mono window, summed in `f64` because a long window of `f32` addition
/// loses the quiet samples that a noise floor estimate is made of.
fn root_mean_square(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|&s| f64::from(s) * f64::from(s))
        .sum::<f64>();
    (sum / samples.len() as f64).sqrt()
}

/// Spectral flatness: the geometric mean of the magnitudes over their arithmetic
/// mean.
///
/// `1.0` for silence, deliberately. A window with nothing in it has no spectrum, and
/// the question a detector is asking is "is this between tracks", to which silence is
/// unambiguously yes. Returning `0.0` there would make every gap look tonal.
///
/// The geometric mean goes through logs to avoid underflowing to zero on a few
/// thousand small magnitudes, and a magnitude below `1e-10` is floored at `ln(1e-10)`
/// rather than producing negative infinity.
#[must_use]
pub fn flatness_of(magnitudes: &[f32]) -> f64 {
    if magnitudes.is_empty() {
        return 1.0;
    }
    let arithmetic: f64 =
        magnitudes.iter().map(|&m| f64::from(m)).sum::<f64>() / magnitudes.len() as f64;
    if arithmetic < 1e-10 {
        return 1.0;
    }
    let log_mean: f64 = magnitudes
        .iter()
        .map(|&m| {
            let value = f64::from(m);
            if value > 1e-10 { value.ln() } else { -23.0 }
        })
        .sum::<f64>()
        / magnitudes.len() as f64;
    (log_mean.exp() / arithmetic).clamp(0.0, 1.0)
}

/// dBFS from a linear level, floored at -120 dB.
#[must_use]
pub fn linear_to_db(linear: f64) -> f64 {
    if linear <= 0.0 {
        return -120.0;
    }
    20.0 * linear.log10()
}

/// A linear level from dBFS.
#[must_use]
pub fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const RATE: SampleRate = SampleRate(48_000);

    fn shape(format: StorageFormat, channels: usize) -> Shape {
        Shape::at_default_window(RATE, channels, format)
    }

    fn sine(frames: usize, hz: f64, amplitude: f64) -> Vec<f32> {
        (0..frames)
            .map(|i| {
                let t = i as f64 / f64::from(RATE.hz());
                (amplitude * (2.0 * PI * hz * t).sin()) as f32
            })
            .collect()
    }

    #[test]
    fn a_window_is_a_tenth_of_a_second_whatever_the_rate() {
        for hz in [44_100, 48_000, 88_200, 96_000, 176_400, 192_000] {
            let shape = Shape::at_default_window(SampleRate(hz), 2, StorageFormat::Int32);
            assert_eq!(shape.window_frames(), hz as usize / 10);
            assert!((shape.window_seconds() - 0.1).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn frames_do_not_depend_on_how_the_audio_arrives() {
        // The property the whole split rests on: the live pass is fed a ring's
        // worth at a time and the refine pass a stored block at a time, and if
        // the windowing moved with the chunking they would disagree about where
        // every boundary is.
        let shape = shape(StorageFormat::Int16, 2);
        let samples = sine(48_000, 440.0, 0.5);
        let interleaved: Vec<f32> = samples.iter().flat_map(|&s| [s, s]).collect();

        let mut one = Windows::spectral(&shape);
        let mut all_at_once = Vec::new();
        one.push_samples(&interleaved, &mut all_at_once);

        let mut piecemeal = Vec::new();
        let mut dribbled = Windows::spectral(&shape);
        // Sizes chosen to straddle the window boundary rather than divide it.
        for chunk in interleaved.chunks(1_477) {
            dribbled.push_samples(chunk, &mut piecemeal);
        }
        assert_eq!(all_at_once.len(), 10);
        assert_eq!(all_at_once.len(), piecemeal.len());
        for (a, b) in all_at_once.iter().zip(piecemeal.iter()) {
            assert!((a.rms - b.rms).abs() < 1e-12, "{a:?} != {b:?}");
            assert!((a.flatness - b.flatness).abs() < 1e-12, "{a:?} != {b:?}");
        }
    }

    #[test]
    fn a_sine_is_tonal_and_noise_is_flat() {
        // The measurement the spectral detector turns on, stated as a fact about
        // two signals whose character is known before the FFT runs.
        let shape = shape(StorageFormat::Float32, 1);
        let mut frames = Vec::new();
        let mut windows = Windows::spectral(&shape);
        windows.push_samples(&sine(4_800, 1_000.0, 0.5), &mut frames);
        let tonal = frames[0].flatness;

        frames.clear();
        let mut windows = Windows::spectral(&shape);
        // A deterministic pseudo-random sequence, so the assertion is repeatable.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let noise: Vec<f32> = (0..4_800)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                ((state >> 33) as f32 / (1u64 << 31) as f32) - 0.5
            })
            .collect();
        windows.push_samples(&noise, &mut frames);
        let flat = frames[0].flatness;

        assert!(tonal < 0.02, "a 1 kHz sine measured {tonal} flat");
        assert!(flat > 0.2, "white noise measured only {flat} flat");
        assert!(flat > tonal * 10.0, "tonal {tonal} against noise {flat}");
    }

    #[test]
    fn silence_is_maximally_flat_so_a_gap_is_never_read_as_music() {
        let shape = shape(StorageFormat::Int32, 2);
        let mut frames = Vec::new();
        let mut windows = Windows::spectral(&shape);
        windows.push(&vec![0u8; 4_800 * 4 * 2], &mut frames);
        assert_eq!(frames.len(), 1);
        assert!((frames[0].rms - 0.0).abs() < f64::EPSILON);
        assert!(
            (frames[0].flatness - 1.0).abs() < f64::EPSILON,
            "silence measured {} flat",
            frames[0].flatness
        );
        assert!((frames[0].level_db() - -120.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_level_only_extractor_reports_no_flatness_rather_than_a_wrong_one() {
        let shape = shape(StorageFormat::Int16, 2);
        let mut frames = Vec::new();
        let mut windows = Windows::new(&shape);
        assert!(!windows.is_spectral());
        let interleaved: Vec<f32> = sine(4_800, 440.0, 0.5)
            .iter()
            .flat_map(|&s| [s, s])
            .collect();
        windows.push_samples(&interleaved, &mut frames);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].rms > 0.3);
        assert_eq!(frames[0].flatness, 0.0);
    }

    #[test]
    fn a_half_scale_sine_reads_minus_nine_dbfs() {
        // sqrt(2)/2 of half scale is 0.354, which is -9.03 dBFS. A figure from
        // the signal rather than from a previous run.
        let shape = shape(StorageFormat::Float32, 1);
        let mut frames = Vec::new();
        let mut windows = Windows::new(&shape);
        windows.push_samples(&sine(4_800, 480.0, 0.5), &mut frames);
        assert!(
            (frames[0].level_db() - -9.031).abs() < 0.01,
            "measured {}",
            frames[0].level_db()
        );
    }

    #[test]
    fn a_trailing_part_window_is_flushed_as_tonal_and_only_once() {
        let shape = shape(StorageFormat::Int16, 2);
        let mut frames = Vec::new();
        let mut windows = Windows::spectral(&shape);
        let interleaved: Vec<f32> = sine(5_000, 440.0, 0.8)
            .iter()
            .flat_map(|&s| [s, s])
            .collect();
        windows.push_samples(&interleaved, &mut frames);
        assert_eq!(frames.len(), 1, "one whole window and 200 frames over");

        assert!(windows.flush(&mut frames));
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].flatness, 0.0, "a 200-frame FFT was trusted");
        assert!(frames[1].rms > 0.0);
        assert!(!windows.flush(&mut frames), "the flush repeated itself");
    }

    #[test]
    fn a_frame_straddling_two_calls_is_carried_and_not_lost() {
        // A meter can afford to drop three bytes. This cannot: the dropped bytes
        // would shift the window alignment of the whole rest of the side, and
        // every boundary reported after it would be wrong by the same amount.
        let shape = shape(StorageFormat::Int16, 2);
        let whole = vec![0x11u8; 4 * 4_800];

        let mut expected = Vec::new();
        let mut reference = Windows::new(&shape);
        reference.push(&whole, &mut expected);
        assert_eq!(expected.len(), 1);

        let mut frames = Vec::new();
        let mut windows = Windows::new(&shape);
        // Odd sizes, none of them a multiple of the 4-byte frame.
        for chunk in whole.chunks(4 * 1_600 - 1) {
            windows.push(chunk, &mut frames);
        }
        assert_eq!(
            windows.frames(),
            4_800,
            "frames went missing at a call edge"
        );
        assert_eq!(frames.len(), 1);
        assert!((frames[0].rms - expected[0].rms).abs() < 1e-12);

        // And the same for pre-decoded samples, where the straddle is a sample
        // rather than a byte.
        let interleaved: Vec<f32> = vec![0.25; 4_800 * 2];
        let mut piecemeal = Vec::new();
        let mut windows = Windows::new(&shape);
        for chunk in interleaved.chunks(1_477) {
            windows.push_samples(chunk, &mut piecemeal);
        }
        assert_eq!(windows.frames(), 4_800);
        assert_eq!(piecemeal.len(), 1);
        assert!((piecemeal[0].rms - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_window_index_converts_to_a_frame_in_the_captures_own_timeline() {
        let shape = shape(StorageFormat::Int32, 2);
        assert_eq!(shape.frame_at(0), 0);
        assert_eq!(shape.frame_at(1), 4_800);
        assert_eq!(shape.frame_at(600), 2_880_000); // one minute in
    }

    #[test]
    fn the_decibel_conversions_are_inverses_and_the_floor_holds() {
        for db in [-90.0, -60.0, -40.0, -12.0, -6.0, 0.0] {
            let round_trip = linear_to_db(db_to_linear(db));
            assert!(
                (round_trip - db).abs() < 1e-9,
                "{db} came back {round_trip}"
            );
        }
        assert!((linear_to_db(0.0) - -120.0).abs() < f64::EPSILON);
        assert!((linear_to_db(-1.0) - -120.0).abs() < f64::EPSILON);
    }
}
