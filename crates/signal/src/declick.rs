/*
 *  declick.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The AR prediction residual click detector - damage found, never repaired (WP-30).
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

//! The click detector: fit an all-pole model, inverse-filter, threshold the residual.
//!
//! Written from the papers, which the survey insists on: Oudre's reference code is
//! GPL-3.0-or-later and FFmpeg's `af_adeclick.c` is LGPL, and VCW is MIT with a
//! permissive-only `deny.toml`. The method is Vaseghi and Rayner (1988, 1990),
//! refined by Esquef et al. (2000, 2002) and given a parameter-by-parameter
//! treatment by Oudre (2015); Levinson-Durbin is Levinson (1947). See
//! `docs/design/WP-30-declick-detector-survey.md` for the trail.
//!
//! The music is what the model predicts and the click is what it cannot. The model
//! is re-fitted every frame, so "what counts as a surprise here" is set by this
//! passage of this record rather than by a global assumption about frequency - which
//! is the whole reason this beats a high-pass and a threshold, and why a cymbal does
//! not read as damage.
//!
//! Two refinements the survey calls not optional are both here:
//!
//! - **Burst fusion.** Inside a real click the residual interferes with itself and
//!   dips back under the threshold, so thresholding alone marks rubble. Two marked
//!   samples closer than [`Config::burst`] have everything between them marked too
//!   (Alvarez et al. 2004, via Oudre).
//! - **Two passes.** The model is first fitted to the damaged signal, so it is
//!   biased by the clicks it is meant to find. The second pass re-fits it.
//!
//! **Where this departs from the paper, and why.** Oudre iterates by *repairing*
//! between passes and re-detecting on the repaired signal. There is no interpolator
//! yet - it is WP-30 §8 step 5 - so the second pass instead drops the samples the
//! first pass marked out of the autocorrelation sums and out of the standard
//! deviation the threshold is taken from. It serves the same purpose, and it is
//! cheaper, but it is not the published method and it is one of the things the
//! re-derivation on `/data2/source_rips` has to measure. Nothing here should be
//! trusted at its defaults until that has happened: see the survey's section 4.
//!
//! This detector reports spans of damage and never changes a sample (§4.1, §23).
//!
//! Requirements: §4.1 (immutable PCM), §23 (observations, not edits), §24
//! (provenance and confidence), §46 Phase 3.

use vcw_types::StorageFormat;

/// The five numbers that move the detector along its precision/recall curve.
///
/// The window, overlap, burst and pass count are the operating point Oudre
/// publishes - `b=20`, `Nw=2416`, two iterations, 75% overlap - which FFmpeg's
/// `adeclick` independently defaults to as well. Two of the five are **not** the
/// published values, and both departures are measured rather than preferred:
///
/// - **Threshold 4, not 2.** `K=2` is two standard deviations of a residual that
///   is very nearly Gaussian where nothing is wrong, so it marks about one sample
///   in twenty-two *by construction*, and burst fusion then joins the scattering
///   into near-continuous coverage. On two seconds of synthesized tonal material
///   with a dither floor and four injected clicks it flags 7.8% of all samples at
///   `b=1` and 24.2% at `b=20` - the survey's "five to seven percent" reproduced,
///   which is the best evidence there is that this implementation matches the
///   paper, and also the proof that the number cannot be shipped. The cliff is
///   sharp: `K=3` drops the same material to 0.001%, `K=4` to zero, and all four
///   clicks are still found at every value up to 10.
/// - **Order 2% of the window, not 12.5%.** This is the one parameter the two
///   published sources disagree about: Oudre's `p=302` against FFmpeg's 2%. On the
///   same material the cheap one found the four clicks as four spans where the
///   expensive one found five, and it runs six times faster - 48 ms per second of
///   stereo against 290 ms, which is the difference between a side in a minute and
///   a side in seven.
///
/// None of this is the re-derivation the survey asks for. That needs
/// `/data2/source_rips`, and until it has happened these are a defensible starting
/// point rather than a recommendation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// Analysis frame length in milliseconds. Oudre's `Nw`.
    pub window_millis: u32,
    /// How much of a frame the next one repeats, as a percentage.
    pub overlap_percent: u32,
    /// Model order as a percentage of the frame length. Oudre's `p`, expressed as
    /// a percentage the way FFmpeg expresses it so that it scales with the sample
    /// rate instead of meaning something different at 96 kHz than at 44.1.
    pub order_percent: f64,
    /// How many standard deviations of the residual counts as damage. Oudre's `K`.
    pub threshold: f64,
    /// Fusion distance in samples. Oudre's `b`.
    pub burst: usize,
    /// How many times to re-fit. Oudre measures two as clean, three about the same,
    /// and four or five as producing musical noise; this is a detector rather than a
    /// repair, so the ceiling is about wasted work rather than artefacts, but the
    /// number came from somewhere and is not raised past it without a reason.
    pub passes: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_millis: 55,
            overlap_percent: 75,
            order_percent: 2.0,
            threshold: 4.0,
            burst: 20,
            passes: 2,
        }
    }
}

/// One span of damage on one channel.
///
/// One channel and not a mask: a vinyl click is damage to one groove wall far more
/// often than both, and a span found on the left has no business claiming an extent
/// on the right. Two rows for a click that hit both walls is the honest answer, and
/// it is also the one that lets the repair leave the good channel alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Damage {
    /// First damaged frame, from the start of the capture.
    pub start: u64,
    /// One past the last. Exclusive, so the length is a subtraction.
    pub end: u64,
    /// Which channel, counted from zero.
    pub channel: u16,
    /// How far past the threshold the worst sample in the span got.
    pub peak_ratio: f64,
    /// `0.0..=1.0`, derived from that margin.
    pub confidence: f32,
}

impl Damage {
    /// Length in frames.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.end - self.start
    }
}

/// What the detector saw, so that a run can be judged before it is believed.
///
/// The flagged fraction is the number the survey's section 4 is about: Oudre's
/// defaults flag five to seven percent of all samples on the corpus they were tuned
/// on, which at 44.1 kHz is two thousand samples a second. A run reports it, and a
/// person decides whether that is a record or a detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Diagnostics {
    /// Capture frames examined.
    pub frames: u64,
    /// Channels examined.
    pub channels: usize,
    /// Samples marked, summed over channels.
    pub flagged_samples: u64,
    /// Analysis frames fitted, summed over channels and passes.
    pub fits: u64,
    /// The frame length actually used, in samples.
    pub window_samples: usize,
    /// The hop actually used, in samples.
    pub hop_samples: usize,
    /// The model order actually used.
    pub order: usize,
}

impl Diagnostics {
    /// Marked samples over examined samples, in `0.0..=1.0`.
    #[must_use]
    pub fn flagged_fraction(&self) -> f64 {
        let examined = self.frames.saturating_mul(self.channels as u64);
        if examined == 0 {
            return 0.0;
        }
        self.flagged_samples as f64 / examined as f64
    }
}

/// The streaming detector: stored-format bytes in, spans out.
///
/// Streaming for the reason [`crate::features::Windows`] is: a forty-minute side is
/// four hundred megabytes of samples, and nothing here needs more than one analysis
/// frame of it at a time. Bytes that end part way through an interleaved frame are
/// carried to the next call, so the spans do not depend on how the audio was chopped
/// up on the way in.
#[derive(Debug)]
pub struct Detector {
    cfg: Config,
    channels: usize,
    format: StorageFormat,
    window: usize,
    hop: usize,
    order: usize,
    /// Bytes of a straddling interleaved frame, at most one frame's worth.
    spare: Vec<u8>,
    /// Per channel: samples held back, starting at capture frame `base`.
    pending: Vec<Vec<f32>>,
    /// Per channel: whether each held-back sample has been marked by any frame so
    /// far. An analysis frame ORs into this and the hop retires the front of it, so
    /// a sample is judged by every frame that covers it before it is emitted.
    marked: Vec<Vec<bool>>,
    /// Per channel: the largest margin any frame reported for each held-back sample.
    ratio: Vec<Vec<f32>>,
    /// Per channel: the start of a span still being extended, as a capture frame.
    open: Vec<Option<Damage>>,
    base: u64,
    frames: u64,
    found: Vec<Damage>,
    diagnostics: Diagnostics,
    /// Scratch, reused across frames so that a forty-minute side allocates once.
    scratch: Scratch,
}

#[derive(Debug, Default)]
struct Scratch {
    autocorrelation: Vec<f64>,
    coefficients: Vec<f64>,
    previous: Vec<f64>,
    residual: Vec<f64>,
    hit: Vec<bool>,
}

impl Detector {
    /// A detector for a capture's layout.
    ///
    /// `rate_hz` and `channels` come from the capture; the frame length and hop are
    /// derived from them, so the same configuration means the same thing in
    /// milliseconds at every rate.
    #[must_use]
    pub fn new(rate_hz: u32, channels: usize, format: StorageFormat, cfg: Config) -> Self {
        let window = window_samples(rate_hz, cfg.window_millis);
        let hop = hop_samples(window, cfg.overlap_percent);
        // An order at or above the frame length has no data to be fitted from, and
        // one of zero is not a model. Both are configuration mistakes rather than
        // audio, so they are clamped rather than refused: a clamped order still
        // detects, and the diagnostics say which one ran.
        let order = order_samples(window, cfg.order_percent);
        Self {
            cfg,
            channels,
            format,
            window,
            hop,
            order,
            spare: Vec::new(),
            pending: vec![Vec::new(); channels],
            marked: vec![Vec::new(); channels],
            ratio: vec![Vec::new(); channels],
            open: vec![None; channels],
            base: 0,
            frames: 0,
            found: Vec::new(),
            diagnostics: Diagnostics {
                channels,
                window_samples: window,
                hop_samples: hop,
                order,
                ..Diagnostics::default()
            },
            scratch: Scratch::default(),
        }
    }

    /// Consumes interleaved bytes in the capture's storage format.
    pub fn push(&mut self, bytes: &[u8]) {
        let frame_bytes = self.format.bytes_per_sample() * self.channels;
        if frame_bytes == 0 {
            return;
        }
        if self.spare.is_empty() {
            let used = self.whole_frames(bytes);
            self.spare.extend_from_slice(&bytes[used..]);
        } else {
            let wanted = frame_bytes - self.spare.len();
            let taken = wanted.min(bytes.len());
            self.spare.extend_from_slice(&bytes[..taken]);
            if self.spare.len() == frame_bytes {
                let joined = std::mem::take(&mut self.spare);
                self.whole_frames(&joined);
                self.spare = joined;
                self.spare.clear();
                let rest = &bytes[taken..];
                let used = self.whole_frames(rest);
                self.spare.extend_from_slice(&rest[used..]);
            }
        }
        self.drain(false);
    }

    /// Ends the capture and returns everything found.
    ///
    /// The tail is analyzed as a short final frame if there is a model's worth of it
    /// left, and dropped if there is not: a frame shorter than its own order says
    /// more about the frame than about the audio.
    #[must_use]
    pub fn finish(mut self) -> (Vec<Damage>, Diagnostics) {
        self.drain(true);
        for channel in 0..self.channels {
            if let Some(span) = self.open[channel].take() {
                self.found.push(span);
            }
        }
        self.diagnostics.frames = self.frames;
        (self.found, self.diagnostics)
    }

    fn whole_frames(&mut self, bytes: &[u8]) -> usize {
        let frame_bytes = self.format.bytes_per_sample() * self.channels;
        let frames = bytes.len() / frame_bytes;
        for frame in 0..frames {
            let base = frame * self.channels;
            for channel in 0..self.channels {
                let sample = self
                    .format
                    .decode_sample(bytes, base + channel)
                    .unwrap_or(0.0);
                self.pending[channel].push(sample);
                self.marked[channel].push(false);
                self.ratio[channel].push(0.0);
            }
        }
        self.frames += frames as u64;
        frames * frame_bytes
    }

    /// Fits every analysis frame that is now complete, and retires what is settled.
    fn drain(&mut self, final_call: bool) {
        loop {
            let held = self.pending[0].len();
            let full = held >= self.window;
            if !full && !(final_call && held > self.order) {
                break;
            }
            let length = if full { self.window } else { held };
            for channel in 0..self.channels {
                fit(
                    &self.pending[channel][..length],
                    &mut self.marked[channel][..length],
                    &mut self.ratio[channel][..length],
                    self.order,
                    &self.cfg,
                    &mut self.scratch,
                    &mut self.diagnostics.fits,
                );
            }
            // A sample in the first hop of this frame is covered by no later frame,
            // so its mark is final and it can be emitted. A short tail has no later
            // frame at all, and the whole of it settles.
            let retire = if full { self.hop } else { held };
            for channel in 0..self.channels {
                self.retire(channel, retire);
            }
            self.base += retire as u64;
            if retire == 0 {
                break;
            }
        }
    }

    /// Emits the first `count` settled samples of a channel as spans.
    fn retire(&mut self, channel: usize, count: usize) {
        for index in 0..count {
            let at = self.base + index as u64;
            if self.marked[channel][index] {
                self.diagnostics.flagged_samples += 1;
                let margin = f64::from(self.ratio[channel][index]);
                match self.open[channel].as_mut() {
                    Some(span) => {
                        span.end = at + 1;
                        span.peak_ratio = span.peak_ratio.max(margin);
                    }
                    None => {
                        self.open[channel] = Some(Damage {
                            start: at,
                            end: at + 1,
                            channel: channel as u16,
                            peak_ratio: margin,
                            confidence: 0.0,
                        });
                    }
                }
            } else if let Some(span) = self.open[channel] {
                // Fusion again, across the frame boundary this time: two spans
                // closer than `burst` are one burst that a frame edge happened to
                // fall inside, and closing them separately would hand the repair
                // stage the rubble the fusion distance exists to prevent.
                if at.saturating_sub(span.end) >= self.cfg.burst as u64 {
                    self.close(channel);
                }
            }
        }
        self.pending[channel].drain(..count);
        self.marked[channel].drain(..count);
        self.ratio[channel].drain(..count);
    }

    fn close(&mut self, channel: usize) {
        if let Some(mut span) = self.open[channel].take() {
            // A margin, not a probability: `BoundaryObservation` already documents
            // that as the normal case. A sample exactly at the threshold scores
            // zero and a ten-fold one scores nine tenths.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "confidence is a 0..=1 quantity shown to a person"
            )]
            let confidence = (1.0 - 1.0 / span.peak_ratio.max(1.0)) as f32;
            span.confidence = confidence.clamp(0.0, 1.0);
            self.found.push(span);
        }
    }
}

/// One analysis frame on one channel: fit, inverse-filter, threshold, fuse.
///
/// A free function and not a method because it borrows four fields of the detector
/// at once and they have to be seen to be disjoint.
fn fit(
    samples: &[f32],
    marked: &mut [bool],
    ratio: &mut [f32],
    order: usize,
    cfg: &Config,
    scratch: &mut Scratch,
    fits: &mut u64,
) {
    let order = order.min(samples.len().saturating_sub(1));
    if order == 0 {
        return;
    }
    let hit = &mut scratch.hit;
    hit.clear();
    hit.resize(samples.len(), false);
    let mut limit = 0.0f64;
    for _ in 0..cfg.passes.max(1) {
        autocorrelation(samples, hit, order, &mut scratch.autocorrelation);
        levinson(
            &scratch.autocorrelation,
            order,
            &mut scratch.coefficients,
            &mut scratch.previous,
        );
        residual(samples, &scratch.coefficients, order, &mut scratch.residual);
        *fits += 1;
        limit = cfg.threshold * deviation(&scratch.residual, &hit[order..]);
        if limit <= 0.0 || !limit.is_finite() {
            // A digitally silent frame has no deviation to be surprised about, and
            // every sample in it would otherwise read as damage.
            hit.fill(false);
            continue;
        }
        for (offset, error) in scratch.residual.iter().enumerate() {
            hit[order + offset] = error.abs() > limit;
        }
        fuse(hit, cfg.burst);
    }
    if limit <= 0.0 || !limit.is_finite() {
        return;
    }
    // OR into the running marks, keeping the largest margin any frame reported:
    // with 75% overlap a sample is judged four times, and the frame that saw it
    // best is the one worth quoting as evidence.
    for (offset, error) in scratch.residual.iter().enumerate() {
        let index = order + offset;
        if !hit[index] {
            continue;
        }
        marked[index] = true;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a margin is quoted to a person, not accumulated"
        )]
        let margin = (error.abs() / limit) as f32;
        ratio[index] = ratio[index].max(margin);
    }
}

/// Frames in one analysis window, floored at a model's worth.
fn window_samples(rate_hz: u32, millis: u32) -> usize {
    let samples = u64::from(rate_hz) * u64::from(millis) / 1_000;
    (samples as usize).max(16)
}

/// The hop, floored at one sample so that the detector always advances.
fn hop_samples(window: usize, overlap_percent: u32) -> usize {
    let overlap = overlap_percent.min(99);
    let hop = window * usize::try_from(100 - overlap).unwrap_or(25) / 100;
    hop.clamp(1, window)
}

/// The model order, floored at one and kept below the frame it is fitted from.
fn order_samples(window: usize, percent: f64) -> usize {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a percentage of a frame length, clamped either side"
    )]
    let order = (window as f64 * percent.clamp(0.1, 50.0) / 100.0) as usize;
    order.clamp(1, window.saturating_sub(1).max(1))
}

/// Autocorrelation of a frame, skipping every lag that touches a marked sample.
///
/// The exclusion is what makes the second pass worth running: the first pass's
/// clicks are the largest numbers in the frame, and leaving them in the sums fits
/// the model to the damage.
fn autocorrelation(samples: &[f32], marked: &[bool], order: usize, out: &mut Vec<f64>) {
    out.clear();
    out.resize(order + 1, 0.0);
    for lag in 0..=order {
        let mut sum = 0.0f64;
        for n in lag..samples.len() {
            if marked[n] || marked[n - lag] {
                continue;
            }
            sum += f64::from(samples[n]) * f64::from(samples[n - lag]);
        }
        out[lag] = sum;
    }
    // White-noise regularization. Excluding samples breaks the Toeplitz consistency
    // the recursion assumes, so a frame can come out not quite positive definite and
    // the recursion can divide by a prediction error that has reached zero. A part
    // in a million of the signal's own power is inaudible to the threshold and makes
    // the solve total.
    out[0] = out[0].mul_add(1.0 + 1e-6, f64::MIN_POSITIVE);
}

/// Levinson-Durbin (Levinson 1947): autocorrelation in, `1 + a1 z^-1 + ...` out.
///
/// `out[0]` is always 1. A frame whose prediction error reaches zero - digital
/// silence, or a pure tone the model has captured exactly - stops early and leaves
/// the remaining coefficients at zero, which is the correct lower-order model rather
/// than a division by zero.
fn levinson(r: &[f64], order: usize, out: &mut Vec<f64>, previous: &mut Vec<f64>) {
    out.clear();
    out.resize(order + 1, 0.0);
    out[0] = 1.0;
    let mut error = r[0];
    for i in 1..=order {
        if error <= 0.0 {
            break;
        }
        let mut acc = r[i];
        for j in 1..i {
            acc += out[j] * r[i - j];
        }
        let reflection = -acc / error;
        if !reflection.is_finite() {
            break;
        }
        previous.clear();
        previous.extend_from_slice(&out[..i]);
        for j in 1..i {
            out[j] = previous[j] + reflection * previous[i - j];
        }
        out[i] = reflection;
        error *= 1.0 - reflection * reflection;
    }
}

/// The inverse filter: one residual per sample that has a full model behind it.
fn residual(samples: &[f32], coefficients: &[f64], order: usize, out: &mut Vec<f64>) {
    out.clear();
    if samples.len() <= order {
        return;
    }
    out.reserve(samples.len() - order);
    for n in order..samples.len() {
        let mut acc = f64::from(samples[n]);
        for (k, coefficient) in coefficients.iter().enumerate().skip(1) {
            acc += coefficient * f64::from(samples[n - k]);
        }
        out.push(acc);
    }
}

/// Standard deviation of the residual over the samples not already marked.
fn deviation(residual: &[f64], marked: &[bool]) -> f64 {
    let mut count = 0usize;
    let mut sum = 0.0f64;
    let mut square = 0.0f64;
    for (value, flagged) in residual.iter().zip(marked) {
        if *flagged {
            continue;
        }
        count += 1;
        sum += *value;
        square = value.mul_add(*value, square);
    }
    if count < 2 {
        return 0.0;
    }
    let n = count as f64;
    let variance = (square - sum * sum / n) / (n - 1.0);
    if variance > 0.0 { variance.sqrt() } else { 0.0 }
}

/// Burst fusion: two marked samples closer than `distance` have the gap marked too.
///
/// Alvarez et al. (2004), via Oudre, who measures what it is worth: on one frame of
/// real classical material at `K=2`, thirty-eight bursts at `b=1` and twenty-one at
/// `b=20`. Without it the interpolator is handed rubble instead of spans.
fn fuse(marked: &mut [bool], distance: usize) {
    if distance < 2 {
        return;
    }
    let mut last: Option<usize> = None;
    let mut index = 0;
    while index < marked.len() {
        if marked[index] {
            if let Some(previous) = last
                && index - previous < distance
            {
                marked[previous..index].fill(true);
            }
            last = Some(index);
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Damage, Detector};
    use vcw_types::StorageFormat;

    const RATE: u32 = 44_100;
    const FRAMES: usize = 44_100 * 2;
    /// Where the clicks go, on channel 0 only.
    const CLICKS: [usize; 4] = [20_000, 41_337, 60_000, 77_777];

    /// Two seconds of something an all-pole model can predict, plus a little noise.
    ///
    /// Three partials and a dither floor: tonal enough that the residual is small
    /// where nothing is wrong, noisy enough that the standard deviation the
    /// threshold is taken from is not zero. A pure sinusoid would be fitted
    /// exactly, and a detector tested against a signal with no residual at all
    /// proves nothing about one.
    fn material(with_clicks: bool) -> Vec<u8> {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut bytes = Vec::with_capacity(FRAMES * 4);
        for frame in 0..FRAMES {
            let t = frame as f64 / f64::from(RATE);
            let tone = 0.30 * (std::f64::consts::TAU * 220.0 * t).sin()
                + 0.15 * (std::f64::consts::TAU * 441.0 * t).sin()
                + 0.07 * (std::f64::consts::TAU * 1_337.0 * t).sin();
            let mut left = tone;
            let right = tone * 0.9;
            if with_clicks && CLICKS.iter().any(|at| frame.abs_diff(*at) < 2) {
                left += if frame % 2 == 0 { 0.55 } else { -0.55 };
            }
            for (channel, value) in [left, right].into_iter().enumerate() {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let dither = ((seed >> 40) as f64 / 16_777_216.0 - 0.5) * 0.004;
                let _ = channel;
                let sample = ((value + dither).clamp(-1.0, 1.0) * 32_000.0) as i16;
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
        }
        bytes
    }

    fn run(bytes: &[u8], chunk: usize) -> (Vec<Damage>, super::Diagnostics) {
        let mut detector = Detector::new(RATE, 2, StorageFormat::Int16, Config::default());
        for piece in bytes.chunks(chunk) {
            detector.push(piece);
        }
        detector.finish()
    }

    fn covers(spans: &[Damage], channel: u16, frame: u64) -> bool {
        spans
            .iter()
            .any(|s| s.channel == channel && (s.start..s.end).contains(&frame))
    }

    /// Every injected click is found, on the channel it was injected on, and the
    /// clean channel beside it is left alone.
    ///
    /// The second half is the half worth having: a detector that flags everything
    /// finds every click too. The clean channel carries the same tone at a
    /// different level, so anything that reads as damage there is the detector's
    /// own invention.
    #[test]
    fn a_click_is_found_and_the_other_wall_is_not() {
        let (spans, diagnostics) = run(&material(true), 65_536);
        for click in CLICKS {
            assert!(
                covers(&spans, 0, click as u64),
                "click at {click} was not found; spans {spans:?}"
            );
            assert!(
                !covers(&spans, 1, click as u64),
                "the clean channel was flagged at {click}"
            );
        }
        assert!(
            spans
                .iter()
                .all(|s| s.peak_ratio > 1.0 && s.confidence >= 0.0),
            "every span carries the margin it was found on"
        );
        assert_eq!(diagnostics.frames, FRAMES as u64);
        assert_eq!(diagnostics.channels, 2);
        // The number the survey's section 4 is about. Four clicks of four samples
        // in two seconds of stereo is far under a thousandth; the published
        // defaults flag five to seven percent on the corpus they came from, so a
        // run that is anywhere near that on clean material is a run to distrust.
        assert!(
            diagnostics.flagged_fraction() < 0.001,
            "flagged {:.5} of all samples",
            diagnostics.flagged_fraction()
        );
    }

    /// The same audio in awkward pieces gives the same spans.
    ///
    /// 1_001 bytes is not a multiple of the four-byte interleaved frame, so every
    /// push but the first starts part way through one. Without the carry the
    /// channels swap over and the detector is reading a different record.
    #[test]
    fn the_spans_do_not_depend_on_the_chunking() {
        let bytes = material(true);
        let (whole, _) = run(&bytes, bytes.len());
        let (ragged, _) = run(&bytes, 1_001);
        assert_eq!(whole, ragged);
        assert!(!whole.is_empty());
    }

    /// Clean material produces almost nothing, and silence produces nothing at all.
    #[test]
    fn nothing_wrong_reads_as_nothing_wrong() {
        let (spans, diagnostics) = run(&material(false), 65_536);
        assert!(
            diagnostics.flagged_fraction() < 0.0005,
            "clean material flagged {:.5}: spans {}",
            diagnostics.flagged_fraction(),
            spans.len()
        );
        let (quiet, quiet_diagnostics) = run(&vec![0u8; FRAMES * 4], 65_536);
        assert!(quiet.is_empty(), "digital silence is not damage");
        assert_eq!(quiet_diagnostics.flagged_samples, 0);
    }

    /// Burst fusion joins what the threshold leaves scattered.
    ///
    /// Oudre's measured effect, reproduced in miniature: the same audio at `b=1`
    /// produces more spans than at `b=20`, because a real click dips back under the
    /// threshold in the middle of itself. If this ever stops holding, fusion has
    /// stopped running.
    #[test]
    fn fusion_turns_rubble_into_spans() {
        let bytes = material(true);
        let scattered = {
            let cfg = Config {
                burst: 1,
                ..Config::default()
            };
            let mut detector = Detector::new(RATE, 2, StorageFormat::Int16, cfg);
            detector.push(&bytes);
            detector.finish().0
        };
        let (fused, _) = run(&bytes, 65_536);
        assert!(
            fused.len() < scattered.len(),
            "fusing {} spans into {} is backwards",
            scattered.len(),
            fused.len()
        );
    }
}
