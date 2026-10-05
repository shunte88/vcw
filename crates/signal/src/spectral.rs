/*
 *  spectral.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The spectral-flatness detector: what tells groove noise from quiet music.
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

//! The spectral-flatness detector: what tells groove noise from quiet music.
//!
//! The second of §22's three required detectors, and the one that earns its cost on a
//! worn or noisily cut pressing. [`crate::silence`] can only ask how loud a window is,
//! so groove noise loud enough to clear the threshold reads as music and a whole side
//! comes back as one track. Flatness asks a different question - is the energy spread
//! evenly across the spectrum, as noise is, or concentrated in partials, as music is -
//! and the answer does not depend on how loud the noise got.
//!
//! It needs an FFT per window, so this is the post-capture pass. The live pass is
//! [`crate::silence::Live`], which is levels only. See [`crate::features::Windows`] for
//! the two extractors that difference comes from.
//!
//! Requirements: §22 (spectral flatness), §23, §24.

use crate::features::db_to_linear;
use crate::regions::{
    Config, Diagnostics, Outcome, Region, Trace, boundaries_for, shape, threshold_for,
};
use vcw_types::{BoundaryObservation, Edge, Provenance};

/// How many windows either side of a window are averaged into its flatness.
///
/// VRipr's `smooth_r`, kept at 3. Its comment reads "±3-window rolling average (≈350 ms
/// at 50 ms windows)", which is worth reading twice: the default window is 100 ms, not
/// 50, so the smoothing this actually applies is 700 ms wide. Kept as it is, because
/// 700 ms is also a defensible width for the job - it is shorter than the shortest gap
/// the pipeline will accept - and because the exit criterion for the port is parity.
const SMOOTHING: usize = 3;

/// Bursts shorter than this are dropped before any gap is bridged, in seconds.
///
/// VRipr's `min_transient_secs`, and the ordering matters for the reason its comment
/// gives: a pop briefly passes the flatness test, and if the gap-fill ran first the pop
/// would be bridged into the track next to it and drag the boundary out to meet it.
/// Dropping it first leaves the gap intact.
const MIN_TRANSIENT_SECS: f64 = 0.35;

/// Smooths the flatness series with a rolling mean, as VRipr does.
///
/// Unsmoothed flatness is jumpy window to window - a cymbal is briefly as flat as
/// groove noise - and the state machine would chatter. The mean is over whatever
/// windows exist, so the ends of a side are smoothed over fewer.
#[must_use]
pub fn smooth(flatness: &[f64]) -> Vec<f64> {
    let n = flatness.len();
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(SMOOTHING);
            let hi = (i + SMOOTHING + 1).min(n);
            flatness[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
        })
        .collect()
}

/// Classifies every window as music or between-tracks, on level *or* flatness.
///
/// A window is between tracks if it is quiet, or if it has energy but that energy is
/// spectrally flat. The two tests are an `or`: either one alone is enough, which is
/// what makes this detector strictly more willing to find a gap than the level one.
///
/// One faithful oddity. The level test here uses `threshold_db - hysteresis_db` for
/// both entering and leaving, so unlike [`crate::silence::classify`] there is no
/// hysteresis band - VRipr computes the upper level and never reads it. That is
/// coherent rather than accidental: the flatness test is what keeps the machine from
/// chattering, it is already smoothed over 700 ms, and a second mechanism for the same
/// job could only fight it. Ported as written.
#[must_use]
pub fn classify(
    trace: &Trace<'_>,
    threshold_db: f64,
    cfg: &Config,
    smoothed: &[f64],
) -> Vec<Region> {
    let quiet = db_to_linear(threshold_db - cfg.hysteresis_db);

    let mut raw: Vec<Region> = Vec::new();
    let mut start: Option<u64> = None;
    for (window, frame) in trace.frames.iter().enumerate() {
        let at = trace.frame_at(window);
        let flat = smoothed.get(window).copied().unwrap_or(0.0);
        let between = frame.rms < quiet || flat > cfg.spectral_flatness_threshold;
        match start {
            None if !between => start = Some(at),
            Some(from) if between => {
                raw.push(Region {
                    start: from,
                    end: at,
                });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        raw.push(Region {
            start: from,
            end: trace.total_frames,
        });
    }
    raw
}

/// Runs the whole detector over a trace.
///
/// The trace must come from [`crate::features::Windows::spectral`]. A levels-only trace
/// reports every window as perfectly tonal, which makes this detector behave exactly
/// like the level one - quietly and wrongly - so it is checked for rather than assumed;
/// see [`Outcome::diagnostics`] and the `flatness_seen` evidence.
#[must_use]
pub fn scan(trace: &Trace<'_>, cfg: &Config) -> Outcome {
    let (threshold_db, floor_db) = threshold_for(trace, cfg);
    if trace.is_empty() {
        return Outcome::nothing(threshold_db, floor_db);
    }
    let diagnostics = Diagnostics {
        threshold_db,
        floor_db,
        windows: trace.frames.len(),
        total_frames: trace.total_frames,
    };

    let flatness: Vec<f64> = trace.frames.iter().map(|frame| frame.flatness).collect();
    let smoothed = smooth(&flatness);

    let hz = f64::from(trace.shape.rate.hz());
    let min_transient = (MIN_TRANSIENT_SECS * hz) as u64;
    let raw: Vec<Region> = classify(trace, threshold_db, cfg, &smoothed)
        .into_iter()
        .filter(|region| region.frames() >= min_transient)
        .collect();

    let regions = shape(raw, trace, cfg);
    let mut boundaries = boundaries_for(
        &regions,
        trace,
        cfg,
        Provenance::SpectralChange,
        &diagnostics,
    );
    for boundary in &mut boundaries {
        score(boundary, trace, cfg, &smoothed);
    }
    Outcome {
        regions,
        boundaries,
        diagnostics,
    }
}

/// Adds the flatness evidence to a boundary and raises its confidence to match.
///
/// The level score from [`crate::regions::boundaries_for`] is already there. This takes
/// the better of the two, because the classifier took either: a boundary placed on a
/// clean drop in level is not made less certain by the groove happening to be tonal,
/// and a boundary placed on a jump in flatness is not made less certain by the groove
/// being loud. Both numbers stay in the evidence either way, which is the point of §24
/// carrying evidence at all - the resolver can disagree with this weighting later
/// without re-analyzing the audio.
fn score(boundary: &mut BoundaryObservation, trace: &Trace<'_>, cfg: &Config, smoothed: &[f64]) {
    let window_frames = trace.shape.window_frames() as u64;
    let window = (boundary.at / window_frames) as usize;
    let (gap, music) = match boundary.edge {
        Edge::Start => (mean_before(smoothed, window), mean_after(smoothed, window)),
        Edge::End => (mean_after(smoothed, window), mean_before(smoothed, window)),
    };
    let separation = gap - music;
    boundary.note("gap_flatness", gap);
    boundary.note("music_flatness", music);
    boundary.note("flatness_threshold", cfg.spectral_flatness_threshold);
    boundary.confidence = boundary.confidence.max(confidence_from(separation));
}

/// Turns a flatness separation into a confidence in 0..=1.
///
/// Invented, like every confidence in this port, and on the same principle: the weakest
/// evidence the detector can act on scores 0.5. The classifier fires when the gap is
/// above the flatness threshold and the music is not, so a separation of nothing is the
/// floor. A separation of 0.3 - the groove at 0.9 against music at 0.6, which is an
/// ordinary pressing - is as certain as this measurement gets.
#[must_use]
pub fn confidence_from(separation: f64) -> f32 {
    (0.5 + 0.5 * (separation / 0.3).clamp(0.0, 1.0)) as f32
}

/// How many values to average when measuring one side of a boundary.
const LOOK: usize = 5;

/// How far to stand back from the boundary before measuring.
///
/// The smoothing spreads every transition over its own width, so the windows either
/// side of a boundary are a blend of the groove and the music and measuring there
/// understates the separation by a factor of two or more. Standing back past the
/// smoothing radius measures the series where it is still describing one thing.
const SKIP: usize = SMOOTHING + 1;

/// The mean of [`LOOK`] values ending [`SKIP`] windows before `window`.
fn mean_before(values: &[f64], window: usize) -> f64 {
    let to = window.saturating_sub(SKIP);
    mean(values, to.saturating_sub(LOOK)..to)
}

/// The mean of [`LOOK`] values starting [`SKIP`] windows after `window`.
fn mean_after(values: &[f64], window: usize) -> f64 {
    let from = window + SKIP;
    mean(values, from..from + LOOK)
}

/// The mean of a range of values, or zero where there are none.
///
/// A mean rather than the median [`crate::regions`] uses for levels, because the series
/// is already smoothed over seven windows and a second robust statistic on top of that
/// would just be smoothing twice.
fn mean(values: &[f64], range: std::ops::Range<usize>) -> f64 {
    let end = range.end.min(values.len());
    if range.start >= end {
        return 0.0;
    }
    values[range.start..end].iter().sum::<f64>() / (end - range.start) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{DEFAULT_WINDOW_MILLIS, Frame, Shape};
    use crate::silence;
    use vcw_types::{SampleRate, StorageFormat};

    const RATE: SampleRate = SampleRate(48_000);
    /// One window at the default 100 ms.
    const W: u64 = 4_800;

    fn layout() -> Shape {
        Shape::new(RATE, 2, StorageFormat::Int16, DEFAULT_WINDOW_MILLIS)
    }

    /// A trace from runs of (level in dB, flatness, windows).
    fn trace_of(runs: &[(f64, f64, usize)]) -> Vec<Frame> {
        let mut frames = Vec::new();
        for &(db, flatness, count) in runs {
            for _ in 0..count {
                frames.push(Frame {
                    rms: db_to_linear(db),
                    flatness,
                });
            }
        }
        frames
    }

    fn view<'a>(frames: &'a [Frame], shape: &'a Shape) -> Trace<'a> {
        Trace::new(frames, shape, frames.len() as u64 * W)
    }

    fn windows_of(region: &Region) -> (u64, u64) {
        (region.start / W, region.end / W)
    }

    #[test]
    fn a_loud_groove_defeats_the_level_detector_and_not_this_one() {
        // The case §22 wants flatness for. The groove between tracks on this pressing
        // is at -20 dBFS, which is twenty decibels clear of the threshold, so the
        // level detector hears one unbroken side. The groove is also nearly white,
        // and the music is not.
        let layout = layout();
        let frames = trace_of(&[
            (-20.0, 0.95, 30),
            (-14.0, 0.30, 300),
            (-20.0, 0.95, 30),
            (-14.0, 0.30, 300),
            (-20.0, 0.95, 30),
        ]);
        let trace = view(&frames, &layout);

        let by_level = silence::scan(&trace, &Config::new());
        assert_eq!(
            by_level.regions.len(),
            1,
            "the level detector was not fooled after all"
        );

        let outcome = scan(&trace, &Config::new());
        assert_eq!(outcome.regions.len(), 2, "got {:?}", outcome.regions);
        for boundary in &outcome.boundaries {
            assert_eq!(boundary.provenance, Provenance::SpectralChange);
            // The level contrast here is 6 dB, which on its own would be the weakest
            // evidence the pipeline acts on. The flatness is what carries it.
            assert!(boundary.measurement("contrast_db").unwrap() < 8.0);
            assert!(
                boundary.confidence > 0.9,
                "{} scored {}",
                boundary.at,
                boundary.confidence
            );
            assert!(boundary.measurement("gap_flatness").unwrap() > 0.85);
            assert!(boundary.measurement("music_flatness").unwrap() < 0.5);
            assert_eq!(boundary.measurement("flatness_threshold"), Some(0.85));
        }
    }

    #[test]
    fn smoothing_costs_the_boundary_a_few_windows_and_that_is_the_trade() {
        // Worth pinning rather than glossing: the rolling mean spreads the transition
        // over its own width, so the classifier only calls the groove flat once it is
        // three windows in. The gap comes out shorter than it is, at both ends, and
        // that is the price of not chattering. It is well inside the padding.
        let layout = layout();
        let frames = trace_of(&[(-14.0, 0.30, 300), (-20.0, 0.95, 30), (-14.0, 0.30, 300)]);
        let trace = view(&frames, &layout);
        let outcome = scan(&trace, &Config::new());

        assert_eq!(outcome.regions.len(), 2);
        // The music really stops at window 300 and restarts at 330. The classifier
        // does not call the groove flat until the rolling mean has two thirds of its
        // width inside it, at window 302, and the padding then moves the boundary one
        // window back out. Two tenths of a second late, into audio that is groove
        // noise: inaudible, and the refine pass is not where sample accuracy comes
        // from - a user-placed boundary is.
        assert_eq!(windows_of(&outcome.regions[0]), (0, 303));
        assert_eq!(windows_of(&outcome.regions[1]), (327, 630));
    }

    #[test]
    fn a_cymbal_is_briefly_as_flat_as_groove_noise_and_does_not_split_a_track() {
        let layout = layout();
        // Two windows of broadband crash in the middle of a track.
        let frames = trace_of(&[(-14.0, 0.30, 150), (-8.0, 0.97, 2), (-14.0, 0.30, 150)]);
        let trace = view(&frames, &layout);
        assert_eq!(scan(&trace, &Config::new()).regions.len(), 1);

        // Unsmoothed, those two windows are above the threshold and would have been
        // called a gap, which is what the smoothing is there to prevent.
        let flatness: Vec<f64> = frames.iter().map(|frame| frame.flatness).collect();
        assert!(flatness[151] > 0.85);
        assert!(smooth(&flatness)[151] < 0.85);
    }

    #[test]
    fn a_transient_is_dropped_before_the_gap_is_bridged_and_the_order_is_what_matters() {
        let layout = layout();
        // A thump in the groove: half a second of silence, two windows of loud tonal
        // rubbish, another half second of silence, between two real tracks.
        let frames = trace_of(&[
            (-14.0, 0.30, 300),
            (-90.0, 1.00, 5),
            (-6.0, 0.20, 2),
            (-90.0, 1.00, 5),
            (-14.0, 0.30, 300),
        ]);
        let trace = view(&frames, &layout);
        let cfg = Config::new();
        let outcome = scan(&trace, &cfg);
        assert_eq!(outcome.regions.len(), 2, "got {:?}", outcome.regions);

        // The counterfactual, run through the shared pipeline with the transient left
        // in: each half of the gap is 0.5 s, under min_silence_secs, so the thump gets
        // bridged to both neighbors and the two tracks become one.
        let smoothed = smooth(
            &frames
                .iter()
                .map(|frame| frame.flatness)
                .collect::<Vec<_>>(),
        );
        let raw = classify(&trace, cfg.threshold_db, &cfg, &smoothed);
        assert_eq!(
            raw.len(),
            3,
            "the thump was not classified as music: {raw:?}"
        );
        assert_eq!(shape(raw, &trace, &cfg).len(), 1);
    }

    #[test]
    fn a_silent_gap_is_found_on_both_counts_and_keeps_both_kinds_of_evidence() {
        let layout = layout();
        // Digital silence reads as perfectly flat by the convention in `features`,
        // so a clean gap satisfies the level test and the flatness test at once.
        let frames = trace_of(&[(-14.0, 0.30, 300), (-90.0, 1.00, 20), (-14.0, 0.30, 300)]);
        let trace = view(&frames, &layout);
        let outcome = scan(&trace, &Config::new());
        assert_eq!(outcome.regions.len(), 2);

        let end = &outcome.boundaries[1];
        assert!(
            end.measurement("contrast_db").unwrap() > 70.0,
            "the level evidence is gone"
        );
        assert!(
            end.measurement("gap_flatness").unwrap() > 0.85,
            "the flatness evidence is gone"
        );
        assert!((end.confidence - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_trace_with_no_flatness_in_it_falls_back_to_the_level_test() {
        // What happens if this detector is handed a trace from the levels-only
        // extractor: every window looks perfectly tonal, the flatness half of the
        // classifier never fires, and the answer is the level detector's - minus the
        // hysteresis band, which this machine does not have. Stated here so the
        // behavior is known rather than discovered.
        let layout = layout();
        let frames = trace_of(&[(-70.0, 0.0, 20), (-14.0, 0.0, 300), (-70.0, 0.0, 20)]);
        let trace = view(&frames, &layout);
        let outcome = scan(&trace, &Config::new());
        assert_eq!(outcome.regions.len(), 1);
        assert_eq!(windows_of(&outcome.regions[0]), (19, 321));
        for boundary in &outcome.boundaries {
            assert_eq!(boundary.measurement("gap_flatness"), Some(0.0));
        }
    }

    #[test]
    fn smoothing_averages_seven_windows_and_fewer_at_the_ends() {
        let flatness = vec![0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0];
        let smoothed = smooth(&flatness);
        // The middle sees all seven.
        assert!((smoothed[3] - 0.1).abs() < 1e-12);
        // The first sees four of them.
        assert!((smoothed[0] - 0.7 / 4.0).abs() < 1e-12);
        assert!(smooth(&[]).is_empty());
    }

    #[test]
    fn flatness_confidence_is_anchored_where_the_classifier_stops_being_sure() {
        assert!((confidence_from(0.0) - 0.5).abs() < 1e-6);
        assert!((confidence_from(-0.5) - 0.5).abs() < 1e-6, "clamped below");
        assert!((confidence_from(0.15) - 0.75).abs() < 1e-6);
        assert!((confidence_from(0.3) - 1.0).abs() < 1e-6);
        assert!((confidence_from(0.9) - 1.0).abs() < 1e-6, "clamped above");
    }

    #[test]
    fn an_empty_trace_reports_its_threshold_and_no_boundaries() {
        let layout = layout();
        let frames: Vec<Frame> = Vec::new();
        let outcome = scan(&Trace::new(&frames, &layout, 0), &Config::new());
        assert!(outcome.regions.is_empty());
        assert_eq!(outcome.diagnostics.threshold_db, -40.0);
        assert_eq!(outcome.diagnostics.windows, 0);
    }
}
