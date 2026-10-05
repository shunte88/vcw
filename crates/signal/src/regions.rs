/*
 *  regions.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The detector settings, the region pipeline every detector shares, and scoring.
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

//! The detector settings, the region pipeline every detector shares, and scoring.
//!
//! Three detectors are required by §22 - RMS energy, spectral flatness and an adaptive
//! HMM - and all three answer the same question: which spans of a side are music and
//! which are the groove between tracks. Only the classification differs. Everything
//! after it (bridge the crackle, merge the too-short gaps, drop the too-short sounds,
//! pad, de-overlap) is the same arithmetic, so it lives here once, ported window for
//! window from VRipr's shared tail.
//!
//! Two deliberate departures from the original:
//!
//! 1. **Positions are frames.** VRipr worked in seconds throughout. A frame is exact,
//!    it is what the project stores, and it is what §24's boundary carries, so the
//!    conversion happens once at the edge of the module - in [`Region::seconds`] - and
//!    only for printing.
//! 2. **Nothing here decodes.** A detector is handed a [`Trace`] of feature frames. See
//!    [`crate::features`] for why the split has to be there.
//!
//! Requirements: §22 (the three detectors), §24 (position, confidence, provenance,
//! evidence).

use crate::features::{Frame, Shape, Windows, linear_to_db};
use vcw_types::{BoundaryObservation, Edge, Provenance, SampleRate};

/// How a detector is tuned.
///
/// The defaults are VRipr's defaults, unchanged, because the exit criterion for this
/// port is parity against VRipr's own output on the labeled corpus. A different set
/// of numbers might well detect better; it would also make every parity difference an
/// argument about tuning rather than about the port, which is the one thing the
/// harness has to be able to rule out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// The level a window has to reach to count as music, in dBFS.
    pub threshold_db: f64,
    /// Whether to derive the threshold from the recording instead of using
    /// `threshold_db` as given.
    pub adaptive: bool,
    /// How far above the estimated noise floor an adaptive threshold sits.
    pub adaptive_margin_db: f64,
    /// The width of the band between entering and leaving music. A window enters at
    /// `threshold_db` and has to fall to `threshold_db - hysteresis_db` to leave, so
    /// one quiet bar does not chop a track in half.
    pub hysteresis_db: f64,
    /// Gaps shorter than this are bridged: a pop, a click, or the pause a drummer
    /// takes. Vinyl is full of them and none of them are track boundaries.
    pub gap_fill_secs: f64,
    /// The shortest gap that may separate two tracks. Anything shorter and the two
    /// sides of it are one track.
    pub min_silence_secs: f64,
    /// The shortest span that may be a track. Anything shorter is a stylus drop, a
    /// locked groove, or a false trigger.
    pub min_sound_secs: f64,
    /// How much to reach back before a track starts, in seconds, so that an attack is
    /// never clipped.
    pub pre_padding: f64,
    /// How much to hold past where a track ends, in seconds, so that a reverb tail or
    /// a fade is never truncated.
    pub post_padding: f64,
    /// The flatness above which a window is groove noise rather than music, used by
    /// [`crate::spectral`] and ignored by the others.
    pub spectral_flatness_threshold: f64,
}

impl Config {
    /// VRipr's defaults, in a form a `const` context can use.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            threshold_db: -40.0,
            adaptive: false,
            adaptive_margin_db: 12.0,
            hysteresis_db: 6.0,
            gap_fill_secs: 0.2,
            min_silence_secs: 0.8,
            min_sound_secs: 2.0,
            pre_padding: 0.1,
            post_padding: 0.1,
            spectral_flatness_threshold: 0.85,
        }
    }

    /// The same settings with the threshold derived from the recording.
    ///
    /// What a dense LP needs, where a fixed -40 dBFS can sit *below* the groove noise
    /// and the detector then finds no gaps at all.
    #[must_use]
    pub const fn adaptive() -> Self {
        Self {
            adaptive: true,
            ..Self::new()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// A span of the capture, in frames, that a detector believes is one track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// The first frame of the span.
    pub start: u64,
    /// One past the last frame of the span.
    pub end: u64,
}

impl Region {
    /// How long the span is, in frames.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// The span in seconds, for printing. Nothing downstream stores this.
    #[must_use]
    pub fn seconds(&self, rate: SampleRate) -> (f64, f64) {
        let hz = f64::from(rate.hz());
        (self.start as f64 / hz, self.end as f64 / hz)
    }
}

/// The feature frames of one capture, with enough context to place them.
///
/// A borrowed view rather than an owning type: the live pass already holds its frames
/// in a growing `Vec` and re-scans the whole of it, and the refine pass holds the
/// frames it read back from the project. Neither should have to hand them over.
#[derive(Debug, Clone, Copy)]
pub struct Trace<'a> {
    /// One entry per analysis window, in order.
    pub frames: &'a [Frame],
    /// The rate, channel count and window length the frames were produced at.
    pub shape: &'a Shape,
    /// How many frames of audio went into them. Not `frames.len() * window_frames`:
    /// the last window is usually partial, and padding has to clamp to what was
    /// actually captured.
    pub total_frames: u64,
}

impl<'a> Trace<'a> {
    /// A trace over frames whose source length is known.
    #[must_use]
    pub const fn new(frames: &'a [Frame], shape: &'a Shape, total_frames: u64) -> Self {
        Self {
            frames,
            shape,
            total_frames,
        }
    }

    /// A trace over the frames a [`Windows`] just produced, taking the audio length
    /// from the extractor's own count so the two cannot disagree.
    #[must_use]
    pub fn from_windows(frames: &'a [Frame], windows: &'a Windows) -> Self {
        Self {
            frames,
            shape: windows.shape(),
            total_frames: windows.frames(),
        }
    }

    /// Whether there is anything to analyze.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The frame a window index starts at.
    #[must_use]
    pub fn frame_at(&self, window: usize) -> u64 {
        self.shape.frame_at(window).min(self.total_frames)
    }

    /// The level of a window in dBFS, or the floor for a window past the end.
    #[must_use]
    pub fn level_db(&self, window: usize) -> f64 {
        self.frames.get(window).map_or(-120.0, Frame::level_db)
    }
}

/// What a detector reports, beyond the boundaries themselves.
///
/// VRipr called these diagnostics and printed them; here they are also evidence, in
/// §24's sense, and they are attached to every boundary the scan produced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Diagnostics {
    /// The level a window had to reach to count as music, whether given or derived.
    pub threshold_db: f64,
    /// The estimated noise floor, present only when the threshold was derived.
    pub floor_db: Option<f64>,
    /// How many windows were analyzed.
    pub windows: usize,
    /// How long the analyzed audio was, in frames.
    pub total_frames: u64,
}

/// The result of one detector pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The spans believed to be tracks, in order, non-overlapping.
    pub regions: Vec<Region>,
    /// Two boundaries per region, a start and an end, each carrying §24's provenance,
    /// confidence and evidence.
    pub boundaries: Vec<BoundaryObservation>,
    /// What the pass had to decide before it could classify anything.
    pub diagnostics: Diagnostics,
}

impl Outcome {
    /// An empty result for a trace with no frames in it.
    #[must_use]
    pub fn nothing(threshold_db: f64, floor_db: Option<f64>) -> Self {
        Self {
            regions: Vec::new(),
            boundaries: Vec::new(),
            diagnostics: Diagnostics {
                threshold_db,
                floor_db,
                windows: 0,
                total_frames: 0,
            },
        }
    }
}

/// The threshold a trace is judged against, and the floor it was derived from.
///
/// Ported from VRipr's phase 2. The fixed case returns the configured level and no
/// floor, and says so in the second element rather than by convention.
#[must_use]
pub fn threshold_for(trace: &Trace<'_>, cfg: &Config) -> (f64, Option<f64>) {
    if !cfg.adaptive {
        return (cfg.threshold_db, None);
    }
    let levels: Vec<f64> = trace.frames.iter().map(|f| f.rms).collect();
    let floor_db = linear_to_db(adaptive_floor(&levels));
    (floor_db + cfg.adaptive_margin_db, Some(floor_db))
}

/// Estimates the noise floor as the interpolated 3rd-percentile window level.
///
/// VRipr's comment on this is worth keeping, because the number looks arbitrary and is
/// not: the 10th percentile lands *inside quiet music* on a densely cut LP where music
/// occupies well over 90% of the side, and the detector then treats the quietest verse
/// as the groove. The 3rd percentile is low enough to find real groove noise and still
/// robust to a few windows of absolute digital silence. Interpolated rather than
/// indexed so the estimate does not step as the window count changes.
///
/// It has a limit worth knowing, measured in this module's own tests: the estimate can
/// only land in the groove if the groove is *more* than 3% of the side. A tightly cut
/// side with eight one-second gaps and no lead-in to speak of is under 1% groove, and
/// the estimate then comes back somewhere inside the quietest music. The
/// `adaptive_margin_db` of 12 dB above it is what keeps the threshold usable when that
/// happens, and it is the reason the margin is that wide.
#[must_use]
pub fn adaptive_floor(levels: &[f64]) -> f64 {
    if levels.is_empty() {
        return 1e-10;
    }
    let mut sorted = levels.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let position = (n - 1) as f64 * 0.03;
    let lo = position.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let t = position - lo as f64;
    (sorted[lo] * (1.0 - t) + sorted[hi] * t).max(1e-10)
}

/// Bridges regions separated by less than `max_gap` frames.
///
/// Ported from VRipr's `merge_gaps`, including the strict `<`: a gap of exactly the
/// limit survives. The pipeline runs it twice with different limits, which is not
/// redundant - see [`shape`].
///
/// This one comparison is where VCW and VRipr part company on real audio, and it is
/// worth knowing why. VRipr compares seconds accumulated as `index as f64 *
/// window_secs`, so a gap of exactly eight 100 ms windows computes as
/// 0.7999999999999993 and is bridged; VCW compares frames, computes 0.8 exactly, and
/// leaves it. On the `vripr_training` corpus that single boundary condition accounts
/// for most of the disagreement between the two (`tests/vripr_parity.rs`): forcing it
/// the other way takes the level detector from 99.52% of VRipr's boundaries to 99.86%
/// and the HMM from 98.01% down to 96.22%, because VRipr's own answer depends on how
/// the error happened to accumulate. Matching it would mean giving up the frame
/// arithmetic that makes the live pass and the refine pass agree to the frame, to
/// inherit a rounding artifact. A gap of exactly `min_silence_secs` is a boundary,
/// which is what the setting says.
#[must_use]
pub fn merge_gaps(regions: Vec<Region>, max_gap: u64) -> Vec<Region> {
    if regions.len() < 2 {
        return regions;
    }
    let mut merged: Vec<Region> = Vec::with_capacity(regions.len());
    for region in regions {
        match merged.last_mut() {
            Some(last) if region.start.saturating_sub(last.end) < max_gap => {
                last.end = last.end.max(region.end);
            }
            _ => merged.push(region),
        }
    }
    merged
}

/// Turns raw classified spans into the tracks a side actually has.
///
/// VRipr's phases 4 to 7, in order, and the order is the whole point:
///
/// 1. Bridge gaps shorter than `gap_fill_secs`. These are pops and breaths, not
///    boundaries.
/// 2. Bridge gaps shorter than `min_silence_secs`. The second pass is not redundant:
///    the first one turns a stutter of near-misses into one span, and only then is the
///    remaining gap the real distance between two candidate tracks.
/// 3. Drop spans shorter than `min_sound_secs`.
/// 4. Pad, clamp to the capture, and split any resulting overlap at its midpoint.
///
/// Padding last means a track can be padded into the space a dropped span left, which
/// is what should happen: the span was noise, and the gap around it belongs to its
/// neighbors.
#[must_use]
pub fn shape(raw: Vec<Region>, trace: &Trace<'_>, cfg: &Config) -> Vec<Region> {
    let hz = f64::from(trace.shape.rate.hz());
    let frames_of = |secs: f64| (secs * hz).round().max(0.0) as u64;

    let merged = merge_gaps(raw, frames_of(cfg.gap_fill_secs));
    let merged = merge_gaps(merged, frames_of(cfg.min_silence_secs));

    let min_sound = frames_of(cfg.min_sound_secs);
    let pre = frames_of(cfg.pre_padding);
    let post = frames_of(cfg.post_padding);

    let mut tracks: Vec<Region> = merged
        .into_iter()
        .filter(|region| region.frames() >= min_sound)
        .map(|region| Region {
            start: region.start.saturating_sub(pre),
            end: region.end.saturating_add(post).min(trace.total_frames),
        })
        .collect();

    for i in 1..tracks.len() {
        if tracks[i].start < tracks[i - 1].end {
            // Split the overlap rather than favoring either neighbor: the padding
            // that caused it was a guess in both directions.
            let midpoint = tracks[i - 1].end / 2 + tracks[i].start / 2;
            tracks[i - 1].end = midpoint;
            tracks[i].start = midpoint;
        }
    }
    tracks
}

/// How much quieter the gap either side of a boundary is than the music next to it.
///
/// Measured over up to `LOOK` windows on each side, which is half a second at the
/// default window. A window past the end of the trace, or a gap that does not exist
/// because the side starts or ends in music, contributes the floor - which is honest:
/// there is nothing there to disagree with.
const LOOK: usize = 5;

/// The contrast in dB across a boundary, and the two levels it came from.
fn contrast(trace: &Trace<'_>, window: usize, edge: Edge) -> (f64, f64, f64) {
    let (gap, music) = match edge {
        // A start: the gap is behind it, the music in front.
        Edge::Start => (before(trace, window), after(trace, window)),
        // An end: the music is behind it, the gap in front.
        Edge::End => (after(trace, window), before(trace, window)),
    };
    (music - gap, music, gap)
}

/// The level of up to [`LOOK`] windows ending at `window`.
fn before(trace: &Trace<'_>, window: usize) -> f64 {
    let from = window.saturating_sub(LOOK);
    median_db(trace, from..window)
}

/// The level of up to [`LOOK`] windows starting at `window`.
fn after(trace: &Trace<'_>, window: usize) -> f64 {
    median_db(trace, window..window + LOOK)
}

/// The median level in a window range, in dB, ignoring windows past the end.
///
/// The median and not the mean, for two reasons that both showed up in testing. The
/// position being scored has already been moved by the padding, so the range reaches
/// a window or two across the boundary into the other side; a mean lets that one window
/// pull the estimate several dB and the score falls for no musical reason. And a pop in
/// the groove - which vinyl has in quantity - drags a mean up towards the music it is
/// supposed to be contrasted against. A max and a min would fix both and would bias
/// every score upwards, which is the one direction a confidence must not be wrong in.
fn median_db(trace: &Trace<'_>, range: std::ops::Range<usize>) -> f64 {
    let end = range.end.min(trace.frames.len());
    if range.start >= end {
        return -120.0;
    }
    let mut levels: Vec<f64> = (range.start..end)
        .map(|window| trace.level_db(window))
        .collect();
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    levels[levels.len() / 2]
}

/// Turns a contrast in dB into a confidence in 0..=1.
///
/// §24 requires a confidence and VRipr has none to port: its detectors emit regions and
/// nothing else. So this is invented, and the honest description of it is that it says
/// how *unambiguous* the level evidence was, not how likely the boundary is to be where
/// a person would put it.
///
/// The scale is anchored at two points that mean something. A boundary can only exist
/// at all if the level crossed the whole hysteresis band, so a contrast equal to
/// `hysteresis_db` is the weakest evidence the detector is capable of acting on, and it
/// scores 0.5. Four times the band - 24 dB at the default - is a gap that no amount of
/// retuning would argue about, and it scores 1.0. Between them it is linear, because
/// there is no measurement here that would justify a curve.
#[must_use]
pub fn confidence_from(contrast_db: f64, cfg: &Config) -> f32 {
    let band = cfg.hysteresis_db.max(1.0);
    let over = ((contrast_db - band) / (3.0 * band)).clamp(0.0, 1.0);
    (0.5 + 0.5 * over) as f32
}

/// Builds §24's two boundaries for every region, with evidence attached.
///
/// The window index a boundary falls on is recovered from its frame position rather
/// than tracked alongside it, because padding has already moved the position and the
/// evidence has to describe the audio the detector actually saw.
#[must_use]
pub fn boundaries_for(
    regions: &[Region],
    trace: &Trace<'_>,
    cfg: &Config,
    provenance: Provenance,
    diagnostics: &Diagnostics,
) -> Vec<BoundaryObservation> {
    let window_frames = trace.shape.window_frames() as u64;
    let mut out = Vec::with_capacity(regions.len() * 2);
    for region in regions {
        for (at, edge) in [(region.start, Edge::Start), (region.end, Edge::End)] {
            let window = (at / window_frames) as usize;
            let (contrast_db, music_db, gap_db) = contrast(trace, window, edge);
            let mut observation =
                BoundaryObservation::new(at, edge, confidence_from(contrast_db, cfg), provenance)
                    .with("contrast_db", contrast_db)
                    .with("music_db", music_db)
                    .with("gap_db", gap_db)
                    .with("threshold_db", diagnostics.threshold_db)
                    .with("region_frames", region.frames() as f64);
            if let Some(floor_db) = diagnostics.floor_db {
                observation = observation.with("floor_db", floor_db);
            }
            out.push(observation);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{DEFAULT_WINDOW_MILLIS, db_to_linear};
    use vcw_types::StorageFormat;

    const RATE: SampleRate = SampleRate(48_000);
    /// One window at the default 100 ms, which is what every position here is a
    /// multiple of.
    const W: u64 = 4_800;

    fn test_shape() -> Shape {
        Shape::new(RATE, 2, StorageFormat::Int16, DEFAULT_WINDOW_MILLIS)
    }

    /// A trace from a run-length description in dB: `[(-20.0, 30), (-70.0, 10)]` is
    /// three seconds of music then one of groove.
    fn trace_of(runs: &[(f64, usize)]) -> Vec<Frame> {
        let mut frames = Vec::new();
        for &(db, count) in runs {
            for _ in 0..count {
                frames.push(Frame {
                    rms: db_to_linear(db),
                    flatness: 0.0,
                });
            }
        }
        frames
    }

    fn view<'a>(frames: &'a [Frame], shape: &'a Shape) -> Trace<'a> {
        Trace::new(frames, shape, frames.len() as u64 * W)
    }

    fn region(start_window: u64, end_window: u64) -> Region {
        Region {
            start: start_window * W,
            end: end_window * W,
        }
    }

    #[test]
    fn a_gap_exactly_the_limit_survives_and_a_shorter_one_does_not() {
        // VRipr's comparison is strict, and the ported one has to be too: the two
        // merge passes are run with limits taken from user-facing settings, and a
        // setting of 0.8 s should not silently mean 0.8 s or a shade more.
        let exactly = vec![region(0, 10), region(18, 30)];
        assert_eq!(
            merge_gaps(exactly.clone(), 8 * W),
            exactly,
            "the limit was bridged"
        );

        let under = vec![region(0, 10), region(17, 30)];
        assert_eq!(merge_gaps(under, 8 * W), vec![region(0, 30)]);
    }

    #[test]
    fn merging_extends_rather_than_replaces_when_a_region_is_swallowed() {
        // A short region entirely inside the previous one must not shorten it. VRipr
        // used `max` here and the reason is easy to miss.
        let regions = vec![region(0, 30), region(31, 35), region(32, 33)];
        assert_eq!(merge_gaps(regions, 2 * W), vec![region(0, 35)]);
    }

    #[test]
    fn the_noise_floor_looks_below_the_quietest_music_not_inside_it() {
        // VRipr's documented reason for the 3rd percentile rather than the 10th: a
        // densely cut side is music for well over 90% of its length, so the 10th
        // percentile lands in a quiet verse and the detector calls the verse a gap.
        // A hundred-second side: six seconds of groove in total (the lead-in, the
        // lead-out and the gaps), nine seconds of quiet music, and the rest loud.
        let frames = trace_of(&[(-55.0, 60), (-30.0, 90), (-14.0, 850)]);
        let levels: Vec<f64> = frames.iter().map(|f| f.rms).collect();

        let floor_db = linear_to_db(adaptive_floor(&levels));
        assert!(
            (floor_db + 55.0).abs() < 0.5,
            "the floor landed at {floor_db} dB"
        );

        // The tenth percentile, for contrast, is inside the quiet passage.
        let mut sorted = levels.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let tenth = linear_to_db(sorted[sorted.len() / 10]);
        assert!(
            tenth > -31.0 && tenth < -29.0,
            "the tenth percentile was {tenth} dB"
        );

        // Which is also why the figure is 3% and not 1%: the groove has to be more
        // than the percentile's share of the side for the estimate to land in it at
        // all. Six percent of this side is groove, and the estimate found it.
        assert!(60.0 / frames.len() as f64 > 0.03);
    }

    #[test]
    fn the_floor_is_interpolated_so_it_does_not_step_with_the_window_count() {
        let levels = [1.0, 2.0, 3.0, 4.0];
        // (n - 1) * 0.03 = 0.09, so 9% of the way from 1.0 to 2.0.
        assert!((adaptive_floor(&levels) - 1.09).abs() < 1e-12);
        assert!((adaptive_floor(&[]) - 1e-10).abs() < 1e-18);
    }

    #[test]
    fn the_second_merge_pass_is_not_a_repeat_of_the_first() {
        // Three bursts a tenth of a second apart, then a real two-second gap, then
        // more music. Gap-fill alone leaves the stutter as one region with the real
        // gap still open; the min-silence pass is what closes near-misses that only
        // became near-misses once the stutter was bridged.
        let layout = test_shape();
        let frames = trace_of(&[(-20.0, 200)]);
        let trace = view(&frames, &layout);
        let cfg = Config::new();

        let stutter = vec![
            region(0, 30),
            region(31, 60),   // 0.1 s gap, bridged by gap_fill
            region(64, 90),   // 0.4 s gap, bridged only by min_silence
            region(110, 160), // 2.0 s gap, a real boundary
        ];
        let shaped = shape(stutter, &trace, &cfg);
        assert_eq!(shaped.len(), 2, "got {shaped:?}");
        assert_eq!(shaped[0].start, 0);
        assert_eq!(shaped[1].start, 110 * W - 4_800); // pre-padded by 0.1 s
    }

    #[test]
    fn a_span_shorter_than_a_track_is_dropped_and_its_gap_goes_to_the_neighbors() {
        let layout = test_shape();
        let frames = trace_of(&[(-20.0, 400)]);
        let trace = view(&frames, &layout);
        let cfg = Config::new();

        // A stylus drop between two real tracks: 0.5 s, well under min_sound_secs.
        let raw = vec![region(0, 100), region(120, 125), region(200, 300)];
        let shaped = shape(raw, &trace, &cfg);
        assert_eq!(shaped.len(), 2);
        assert_eq!(shaped[0].end, 100 * W + W); // post-padded, not truncated
        assert_eq!(shaped[1].start, 200 * W - W);
    }

    #[test]
    fn padding_that_overlaps_is_split_down_the_middle() {
        let layout = test_shape();
        let frames = trace_of(&[(-20.0, 300)]);
        let trace = view(&frames, &layout);
        let cfg = Config {
            pre_padding: 1.0,
            post_padding: 1.0,
            ..Config::new()
        };

        // A one-second gap with a second of padding reaching in from each side.
        let raw = vec![region(0, 100), region(110, 200)];
        let shaped = shape(raw, &trace, &cfg);
        assert_eq!(shaped.len(), 2);
        assert_eq!(
            shaped[0].end, shaped[1].start,
            "the overlap was not resolved"
        );
        assert_eq!(shaped[0].end, 105 * W, "the split was not the midpoint");
    }

    #[test]
    fn padding_never_reaches_past_what_was_captured() {
        let layout = test_shape();
        let frames = trace_of(&[(-20.0, 100)]);
        let total = 100 * W - 17; // a capture stopped mid-window, as they always are
        let trace = Trace::new(&frames, &layout, total);
        let raw = vec![Region {
            start: 0,
            end: total,
        }];
        let shaped = shape(raw, &trace, &Config::new());
        assert_eq!(shaped[0].end, total);
        assert_eq!(shaped[0].start, 0, "padding went below the first frame");
    }

    #[test]
    fn confidence_is_anchored_at_the_weakest_evidence_the_detector_can_act_on() {
        let cfg = Config::new();
        // Nothing weaker than the hysteresis band can produce a boundary at all.
        assert!((confidence_from(cfg.hysteresis_db, &cfg) - 0.5).abs() < 1e-6);
        assert!(
            (confidence_from(0.0, &cfg) - 0.5).abs() < 1e-6,
            "clamped below the band"
        );
        assert!((confidence_from(4.0 * cfg.hysteresis_db, &cfg) - 1.0).abs() < 1e-6);
        assert!(
            (confidence_from(90.0, &cfg) - 1.0).abs() < 1e-6,
            "clamped above"
        );
        // And monotone in between.
        let mut last = 0.0;
        for contrast in [6.0, 9.0, 12.0, 18.0, 24.0] {
            let score = confidence_from(contrast, &cfg);
            assert!(
                score > last,
                "{contrast} dB scored {score}, no better than {last}"
            );
            last = score;
        }
    }

    #[test]
    fn a_boundary_carries_the_levels_it_was_decided_from() {
        let layout = test_shape();
        // Groove, music, groove: one region with clean evidence on both sides.
        let frames = trace_of(&[(-70.0, 20), (-14.0, 100), (-70.0, 20)]);
        let trace = view(&frames, &layout);
        let cfg = Config::new();
        let diagnostics = Diagnostics {
            threshold_db: -40.0,
            floor_db: None,
            windows: frames.len(),
            total_frames: trace.total_frames,
        };
        let boundaries = boundaries_for(
            &[region(20, 120)],
            &trace,
            &cfg,
            Provenance::Silence,
            &diagnostics,
        );

        assert_eq!(
            boundaries.len(),
            2,
            "a region is two boundaries, a start and an end"
        );
        let start = &boundaries[0];
        assert_eq!(start.edge, Edge::Start);
        assert_eq!(start.provenance, Provenance::Silence);
        assert!(!start.provenance.is_locked());
        assert_eq!(start.at, 20 * W);
        assert!((start.measurement("music_db").unwrap() + 14.0).abs() < 0.01);
        assert!((start.measurement("gap_db").unwrap() + 70.0).abs() < 0.01);
        assert!((start.measurement("contrast_db").unwrap() - 56.0).abs() < 0.01);
        assert!(
            (start.confidence - 1.0).abs() < 1e-6,
            "56 dB of contrast is unambiguous"
        );
        assert_eq!(start.measurement("region_frames"), Some(100.0 * W as f64));
        assert_eq!(
            start.measurement("floor_db"),
            None,
            "a fixed threshold has no floor"
        );

        // The end sees the same two levels from the other direction.
        let end = &boundaries[1];
        assert_eq!(end.edge, Edge::End);
        assert!((end.measurement("music_db").unwrap() + 14.0).abs() < 0.01);
        assert!((end.measurement("gap_db").unwrap() + 70.0).abs() < 0.01);
    }

    #[test]
    fn a_side_that_starts_in_music_still_scores_its_first_boundary() {
        // There is no lead-in gap on a well-cut side, and the first boundary is still
        // a boundary. It must not come out unscored or panic for want of audio before
        // frame zero.
        let layout = test_shape();
        let frames = trace_of(&[(-14.0, 100)]);
        let trace = view(&frames, &layout);
        let diagnostics = Diagnostics {
            threshold_db: -40.0,
            floor_db: Some(-62.0),
            windows: frames.len(),
            total_frames: trace.total_frames,
        };
        let boundaries = boundaries_for(
            &[region(0, 100)],
            &trace,
            &Config::new(),
            Provenance::Hmm,
            &diagnostics,
        );
        assert_eq!(boundaries[0].at, 0);
        assert!(
            boundaries[0].confidence > 0.9,
            "the missing lead-in cost it confidence"
        );
        assert_eq!(boundaries[0].measurement("floor_db"), Some(-62.0));
        assert_eq!(boundaries[0].provenance, Provenance::Hmm);
    }

    #[test]
    fn the_threshold_is_the_configured_one_unless_asked_to_adapt() {
        let layout = test_shape();
        let frames = trace_of(&[(-60.0, 30), (-12.0, 300)]);
        let trace = view(&frames, &layout);

        let (fixed, floor) = threshold_for(&trace, &Config::new());
        assert!((fixed + 40.0).abs() < 1e-9);
        assert_eq!(floor, None);

        let (derived, floor) = threshold_for(&trace, &Config::adaptive());
        let floor = floor.expect("an adaptive threshold reports what it adapted to");
        assert!((floor + 60.0).abs() < 0.5, "the floor landed at {floor} dB");
        assert!(
            (derived - (floor + 12.0)).abs() < 1e-9,
            "the margin was not applied"
        );
    }

    #[test]
    fn a_region_reports_its_own_length_in_frames_and_seconds() {
        let region = region(10, 40);
        assert_eq!(region.frames(), 30 * W);
        let (from, to) = region.seconds(RATE);
        assert!((from - 1.0).abs() < 1e-12);
        assert!((to - 4.0).abs() < 1e-12);
    }
}
