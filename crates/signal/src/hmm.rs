/*
 *  hmm.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The adaptive two-state HMM, and the only confidence in the port that is a probability.
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

//! The adaptive two-state HMM, and the only confidence in the port that is a
//! probability.
//!
//! The third of §22's required detectors, and the one that does not need a threshold.
//! The other two ask whether each window is above a line; this one fits two Gaussians
//! to the side itself - one for the groove, one for the music - and then asks which
//! sequence of states best explains the whole series. That is why it survives a
//! pressing the other two need retuning for: the line is wherever the record says it
//! is.
//!
//! "Adaptive" in §22 means the emissions are estimated from the recording rather than
//! configured. The quietest 15% of windows are taken as groove and the loudest 40% as
//! music, which sounds arbitrary and is: it is a heuristic that works because a side is
//! mostly music with a little groove in it, and it is VRipr's, kept for parity.
//!
//! One thing this module has that the original does not: it runs
//! [forward-backward](posteriors) as well as Viterbi, so every boundary carries the
//! posterior probability of a state change happening near it. That is a real
//! probability, and the only one in the port - the confidences in [`crate::silence`]
//! and [`crate::spectral`] are invented scales.
//!
//! It is also, measured, almost always 1.0, and the reason is instructive enough to
//! state here rather than bury. The emissions are fitted to the quietest 15% and the
//! loudest 40% *of the very windows being classified*, so the two Gaussians are well
//! separated by construction and the log likelihood is quadratic in the distance
//! between them. By the time Viterbi has committed to a transition, forward-backward
//! agrees to within a rounding error - even across a sixty-second fade, and even when
//! the real level contrast is only three decibels. This module's own tests pin that.
//!
//! So the posterior is used as a *veto*, not as a score: it can lower the level-derived
//! confidence a boundary already has and never raise it. In practice it rarely lowers
//! it, which is the honest summary of what a self-fitted two-state model can tell you
//! about its own output. What it cannot tell you is whether the side gave it anything
//! to work with, and that is what [`Emissions::separation`] is in the evidence for.
//!
//! Requirements: §22 (adaptive HMM), §23, §24 (confidence and evidence).

use crate::features::{Frame, linear_to_db};
use crate::regions::{
    Config, Diagnostics, Outcome, Region, Trace, boundaries_for, shape, threshold_for,
};
use vcw_types::{BoundaryObservation, Edge, Provenance};

/// Which of the two states a window is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Between tracks: lead-in, lead-out, or the groove between two songs.
    Gap,
    /// Inside a track.
    Music,
}

impl State {
    /// The index this state occupies in every array here.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Gap => 0,
            Self::Music => 1,
        }
    }
}

/// The fraction of the quietest windows taken as groove when estimating emissions.
const GAP_SHARE: f64 = 0.15;
/// The fraction of the loudest windows taken as music.
const MUSIC_SHARE: f64 = 0.40;
/// The narrowest a level distribution may be, in dB.
///
/// Without a floor, a side with a long stretch of digital silence gives the groove
/// state a standard deviation near zero, and every window that is not *exactly* that
/// level then becomes infinitely unlikely to be groove. The floor is what keeps the
/// model from being certain about something it only saw one value of.
const LEVEL_FLOOR_DB: f64 = 2.0;
/// The narrowest a flatness distribution may be, on flatness's own 0..=1 scale.
const FLATNESS_FLOOR: f64 = 0.08;
/// How strongly the model expects a side to begin in the groove, in log space.
///
/// Twenty in log space is a factor of half a billion, which is not a subtle preference.
/// It is right anyway: a capture starts with the stylus dropping into the lead-in, and
/// the one place a recording is *guaranteed* not to start is half way through a song.
const LEAD_IN_BIAS: f64 = 20.0;
/// How far either side of a boundary the transition posterior is gathered, in windows.
///
/// Half a second. A posterior at a single window is a statement about that window, and
/// the position being scored has already been moved by the padding, so a single-window
/// reading would be answering a question nobody asked. Gathered over half a second it
/// answers the question a user has: is there a boundary about here?
const POSTERIOR_RADIUS: usize = 5;

/// How close to the ends of a trace a boundary may be before the posterior stops
/// meaning anything, in windows.
///
/// A side that begins in music has a track starting at frame zero, and no state
/// *change* happens there - the capture simply started. The posterior of a transition
/// near frame zero is therefore near zero, which would read as "no confidence" about
/// the one boundary in the whole side that is not in doubt. Same at the other end. So
/// within this distance of either end the veto is not applied, and the evidence says
/// so rather than leaving a reader to wonder why.
const EDGE_WINDOWS: usize = POSTERIOR_RADIUS + 1;

/// One Gaussian: a mean and a standard deviation that is never allowed to collapse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gaussian {
    /// The center of the distribution.
    pub mean: f64,
    /// The spread, floored at construction.
    pub sigma: f64,
}

impl Gaussian {
    /// Fits a Gaussian to some values, with a floor on the spread.
    #[must_use]
    pub fn fit(values: &[f64], floor: f64) -> Self {
        if values.is_empty() {
            return Self {
                mean: 0.0,
                sigma: floor,
            };
        }
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        let variance = values
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / n;
        Self {
            mean,
            sigma: variance.sqrt().max(floor),
        }
    }

    /// The log likelihood of a value, less the constant every state shares.
    ///
    /// `-z²/2 - ln(sigma)`, without the `-ln(sqrt(2 pi))`. The constant cancels in
    /// every comparison the model makes, and VRipr left it out; leaving it out here too
    /// means the numbers in a log are the same numbers, which matters when the two are
    /// being compared window by window.
    #[must_use]
    pub fn log_prob(&self, value: f64) -> f64 {
        let z = (value - self.mean) / self.sigma;
        -0.5 * z * z - self.sigma.ln()
    }
}

/// What the model believes the groove and the music look like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emissions {
    /// The level of the groove, in dBFS.
    pub gap_level: Gaussian,
    /// The level of the music, in dBFS.
    pub music_level: Gaussian,
    /// The flatness of the groove.
    pub gap_flatness: Gaussian,
    /// The flatness of the music.
    pub music_flatness: Gaussian,
}

impl Emissions {
    /// Estimates both states from the recording itself.
    ///
    /// Windows are ranked by level, the quietest `GAP_SHARE` become the groove's
    /// evidence and the loudest `MUSIC_SHARE` the music's, and the flatness of those
    /// same windows comes along with them. Nothing in the middle is used by either,
    /// which is deliberate: the windows that are hard to classify should not be the
    /// ones defining what the classes look like.
    #[must_use]
    pub fn estimate(levels_db: &[f64], flatness: &[f64]) -> Self {
        let n = levels_db.len();
        if n == 0 {
            return Self {
                gap_level: Gaussian::fit(&[], LEVEL_FLOOR_DB),
                music_level: Gaussian::fit(&[], LEVEL_FLOOR_DB),
                gap_flatness: Gaussian::fit(&[], FLATNESS_FLOOR),
                music_flatness: Gaussian::fit(&[], FLATNESS_FLOOR),
            };
        }
        let mut ranked: Vec<usize> = (0..n).collect();
        ranked.sort_by(|&a, &b| {
            levels_db[a]
                .partial_cmp(&levels_db[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let quiet = ((n as f64 * GAP_SHARE) as usize).max(1);
        let loud = ((n as f64 * MUSIC_SHARE) as usize).max(1);
        let gap = &ranked[..quiet];
        let music = &ranked[n - loud..];

        let pick = |from: &[usize], series: &[f64]| -> Vec<f64> {
            from.iter().map(|&window| series[window]).collect()
        };
        Self {
            gap_level: Gaussian::fit(&pick(gap, levels_db), LEVEL_FLOOR_DB),
            music_level: Gaussian::fit(&pick(music, levels_db), LEVEL_FLOOR_DB),
            gap_flatness: Gaussian::fit(&pick(gap, flatness), FLATNESS_FLOOR),
            music_flatness: Gaussian::fit(&pick(music, flatness), FLATNESS_FLOOR),
        }
    }

    /// The log likelihood of one window under each state.
    ///
    /// Level and flatness are treated as independent, which they are not - loud windows
    /// are usually tonal - so the model is over-confident by some unknown factor. That
    /// is the standard naive-Bayes bargain and it is worth naming, because it is the
    /// main reason the posterior in [`posteriors`] should be read as a ranking rather
    /// than a calibrated probability.
    ///
    /// A levels-only trace reports every window's flatness as zero, so both flatness
    /// terms take the same value and cancel. The model degrades to levels alone rather
    /// than to nonsense, which is why [`scan`] is willing to run on either trace.
    #[must_use]
    pub fn log_prob(&self, frame: &Frame) -> [f64; 2] {
        let level = linear_to_db(frame.rms);
        [
            self.gap_level.log_prob(level) + self.gap_flatness.log_prob(frame.flatness),
            self.music_level.log_prob(level) + self.music_flatness.log_prob(frame.flatness),
        ]
    }

    /// How far apart the two level distributions are, in standard deviations.
    ///
    /// The single number that says whether this model had anything to work with. Near
    /// zero means the side gave it no contrast and every boundary it reports is a guess
    /// dressed as a probability, so it goes into the evidence of every boundary.
    #[must_use]
    pub fn separation(&self) -> f64 {
        let spread = (self.gap_level.sigma + self.music_level.sigma) / 2.0;
        if spread <= 0.0 {
            return 0.0;
        }
        (self.music_level.mean - self.gap_level.mean) / spread
    }
}

/// The log probabilities of staying in a state or leaving it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transitions {
    /// Indexed `[from][to]`, in log space.
    log: [[f64; 2]; 2],
}

impl Transitions {
    /// Derives the transition probabilities from the durations the user configured.
    ///
    /// The insight worth keeping from VRipr: `min_silence_secs` and `min_sound_secs`
    /// are already statements about how long a state lasts, and the expected dwell time
    /// of a two-state chain is one over its leaving probability. So the same two numbers
    /// that the other detectors use as hard filters become this model's prior, and a
    /// user who lengthens the minimum gap makes the HMM *reluctant* to leave the groove
    /// rather than merely deleting its answers afterwards.
    ///
    /// Capped at a half: a state that is more likely to be left than kept is not a
    /// state, and a window length longer than the configured minimum would ask for
    /// exactly that.
    #[must_use]
    pub fn from_durations(cfg: &Config, window_seconds: f64) -> Self {
        let windows_of = |secs: f64| (secs / window_seconds).max(2.0);
        let leave_gap = (1.0 / windows_of(cfg.min_silence_secs)).min(0.5);
        let leave_music = (1.0 / windows_of(cfg.min_sound_secs)).min(0.5);
        Self {
            log: [
                [(1.0 - leave_gap).ln(), leave_gap.ln()],
                [leave_music.ln(), (1.0 - leave_music).ln()],
            ],
        }
    }

    /// The log probability of going from one state to another.
    #[must_use]
    pub const fn log(&self, from: State, to: State) -> f64 {
        self.log[from.index()][to.index()]
    }
}

/// The most likely state of every window, by Viterbi.
///
/// Rolling, because the only thing the recursion needs from the past is one pair of
/// scores and the backtrace. A forty-minute side is twenty-four thousand windows and
/// the backtrace is two bytes each.
#[must_use]
pub fn viterbi(emissions: &[[f64; 2]], transitions: &Transitions) -> Vec<State> {
    let n = emissions.len();
    if n == 0 {
        return Vec::new();
    }
    let mut score = [emissions[0][0], emissions[0][1] - LEAD_IN_BIAS];
    let mut back: Vec<[u8; 2]> = Vec::with_capacity(n);
    back.push([0, 1]);

    for emission in &emissions[1..] {
        let mut next = [0.0f64; 2];
        let mut from = [0u8; 2];
        for (to, slot) in next.iter_mut().enumerate() {
            let by_gap = score[0] + transitions.log[0][to];
            let by_music = score[1] + transitions.log[1][to];
            // `>=` rather than `>`: a tie goes to the groove, which is the same
            // direction the start bias points and the same direction a detector should
            // err in. Calling music groove loses a moment of lead-in; calling groove
            // music glues two tracks together.
            if by_gap >= by_music {
                *slot = by_gap + emission[to];
                from[to] = 0;
            } else {
                *slot = by_music + emission[to];
                from[to] = 1;
            }
        }
        score = next;
        back.push(from);
    }

    let mut states = vec![State::Gap; n];
    states[n - 1] = if score[1] > score[0] {
        State::Music
    } else {
        State::Gap
    };
    for t in (0..n - 1).rev() {
        states[t] = match back[t + 1][states[t + 1].index()] {
            0 => State::Gap,
            _ => State::Music,
        };
    }
    states
}

/// The posterior probability of each kind of state change, window by window.
///
/// Forward-backward, in log space. Returns one pair per window: the probability that
/// this window is where the groove gave way to music, and the probability that it is
/// where music gave way to the groove. Window zero is always `[0, 0]`, there being no
/// transition into the first window.
///
/// This is what Viterbi cannot tell you. Viterbi returns the single best path and says
/// nothing about how much better it was than the next one, so a boundary found in a
/// dead-flat fade and a boundary found at a needle lift look identical coming out of
/// it. The posterior separates them, and that is the whole reason this module carries
/// a second pass over the same data.
#[must_use]
pub fn posteriors(emissions: &[[f64; 2]], transitions: &Transitions) -> Vec<[f64; 2]> {
    let n = emissions.len();
    if n == 0 {
        return Vec::new();
    }
    let mut alpha = vec![[0.0f64; 2]; n];
    alpha[0] = [emissions[0][0], emissions[0][1] - LEAD_IN_BIAS];
    for t in 1..n {
        for to in 0..2 {
            alpha[t][to] = log_sum(
                alpha[t - 1][0] + transitions.log[0][to],
                alpha[t - 1][1] + transitions.log[1][to],
            ) + emissions[t][to];
        }
    }

    let mut beta = vec![[0.0f64; 2]; n];
    for t in (0..n - 1).rev() {
        for from in 0..2 {
            beta[t][from] = log_sum(
                transitions.log[from][0] + emissions[t + 1][0] + beta[t + 1][0],
                transitions.log[from][1] + emissions[t + 1][1] + beta[t + 1][1],
            );
        }
    }

    let total = log_sum(alpha[n - 1][0], alpha[n - 1][1]);
    let mut out = vec![[0.0f64; 2]; n];
    for t in 1..n {
        let into_music =
            alpha[t - 1][0] + transitions.log[0][1] + emissions[t][1] + beta[t][1] - total;
        let into_gap =
            alpha[t - 1][1] + transitions.log[1][0] + emissions[t][0] + beta[t][0] - total;
        out[t] = [into_music.exp(), into_gap.exp()];
    }
    out
}

/// `ln(e^a + e^b)`, without overflowing on the way.
fn log_sum(a: f64, b: f64) -> f64 {
    let (high, low) = if a >= b { (a, b) } else { (b, a) };
    if high == f64::NEG_INFINITY {
        return high;
    }
    high + (low - high).exp().ln_1p()
}

/// Turns a state sequence into the spans that were music.
#[must_use]
pub fn runs(states: &[State], trace: &Trace<'_>) -> Vec<Region> {
    let mut raw: Vec<Region> = Vec::new();
    let mut start: Option<u64> = None;
    for (window, &state) in states.iter().enumerate() {
        let at = trace.frame_at(window);
        match (start, state) {
            (None, State::Music) => start = Some(at),
            (Some(from), State::Gap) => {
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
/// Works on a spectral trace or a levels-only one - see [`Emissions::log_prob`] - and
/// on a side no threshold suits, which is the reason it exists.
#[must_use]
pub fn scan(trace: &Trace<'_>, cfg: &Config) -> Outcome {
    // Only ever reported, never used to classify: the HMM has no threshold. It is
    // carried so that a boundary from this detector can be compared with one from
    // another by a resolver that only has the evidence to go on.
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

    let levels_db: Vec<f64> = trace.frames.iter().map(Frame::level_db).collect();
    let flatness: Vec<f64> = trace.frames.iter().map(|frame| frame.flatness).collect();
    let emissions = Emissions::estimate(&levels_db, &flatness);
    let per_window: Vec<[f64; 2]> = trace
        .frames
        .iter()
        .map(|frame| emissions.log_prob(frame))
        .collect();
    let transitions = Transitions::from_durations(cfg, trace.shape.window_seconds());

    let states = viterbi(&per_window, &transitions);
    let posterior = posteriors(&per_window, &transitions);
    let regions = shape(runs(&states, trace), trace, cfg);

    let mut boundaries = boundaries_for(&regions, trace, cfg, Provenance::Hmm, &diagnostics);
    let window_frames = trace.shape.window_frames() as u64;
    let separation = emissions.separation();
    let last = trace.frames.len().saturating_sub(1);
    for boundary in &mut boundaries {
        let window = (boundary.at / window_frames) as usize;
        let probability = gathered(&posterior, window, boundary.edge);
        let at_edge = window < EDGE_WINDOWS || window + EDGE_WINDOWS > last;
        score(boundary, probability, at_edge, separation, &emissions);
    }
    Outcome {
        regions,
        boundaries,
        diagnostics,
    }
}

/// The posterior of the right kind of transition within [`POSTERIOR_RADIUS`] windows.
fn gathered(posterior: &[[f64; 2]], window: usize, edge: Edge) -> f64 {
    let which = match edge {
        Edge::Start => 0,
        Edge::End => 1,
    };
    let from = window.saturating_sub(POSTERIOR_RADIUS);
    let to = (window + POSTERIOR_RADIUS + 1).min(posterior.len());
    if from >= to {
        return 0.0;
    }
    posterior[from..to]
        .iter()
        .map(|pair| pair[which])
        .sum::<f64>()
        .clamp(0.0, 1.0)
}

/// Lets the model veto the level-derived confidence, and records what it was told.
///
/// The level contrast from [`crate::regions::boundaries_for`] is already on the
/// boundary. Taking the lower of the two is the arrangement that survives both of this
/// model's habits: it is separated by construction, so its posterior cannot be trusted
/// to *raise* anything, and when it does hedge the hedge is real and should be heard.
/// At the ends of a trace it has nothing to say at all - see [`EDGE_WINDOWS`] - and
/// then the level score stands alone.
///
/// Everything the decision used goes into the evidence either way, which is what §24
/// carries evidence for: a resolver can disagree with this weighting later without
/// touching the audio again.
fn score(
    boundary: &mut BoundaryObservation,
    probability: f64,
    at_edge: bool,
    separation: f64,
    emissions: &Emissions,
) {
    boundary.note("posterior", probability);
    boundary.note("posterior_applies", f64::from(u8::from(!at_edge)));
    boundary.note("separation", separation);
    boundary.note("gap_level_db", emissions.gap_level.mean);
    boundary.note("music_level_db", emissions.music_level.mean);
    if !at_edge {
        boundary.confidence = boundary.confidence.min(probability as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{DEFAULT_WINDOW_MILLIS, Shape, db_to_linear};
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

    #[test]
    fn the_model_fits_itself_to_the_side_rather_than_to_a_threshold() {
        // The same noisy pressing that needs `Config::adaptive` to be detected by
        // level and needs flatness to be detected spectrally. The HMM is handed the
        // stock settings and no help at all.
        let layout = layout();
        let frames = trace_of(&[
            (-20.0, 0.90, 30),
            (-14.0, 0.30, 300),
            (-20.0, 0.90, 30),
            (-14.0, 0.30, 300),
            (-20.0, 0.90, 30),
        ]);
        let trace = view(&frames, &layout);

        assert_eq!(
            silence::scan(&trace, &Config::new()).regions.len(),
            1,
            "level was fooled"
        );

        let outcome = scan(&trace, &Config::new());
        assert_eq!(outcome.regions.len(), 2, "got {:?}", outcome.regions);
        for boundary in &outcome.boundaries {
            assert_eq!(boundary.provenance, Provenance::Hmm);
            // The model found the two levels for itself, to within a decibel.
            assert!((boundary.measurement("gap_level_db").unwrap() + 20.0).abs() < 1.0);
            assert!((boundary.measurement("music_level_db").unwrap() + 14.0).abs() < 1.0);
            assert!(boundary.measurement("separation").unwrap() > 1.0);
            // The model is certain and the audio is not: six decibels of contrast is
            // the least this pipeline will act on, so the boundary goes out at 0.5.
            // That is the right report. The HMM found a boundary neither of the other
            // detectors could, and it is still true that anyone looking at the levels
            // alone would call it marginal - which is what a resolver weighing this
            // against a fingerprint transition or a release duration needs to know.
            assert!(
                (boundary.confidence - 0.5).abs() < 1e-6,
                "scored {}",
                boundary.confidence
            );
            assert!((boundary.measurement("posterior").unwrap() - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn the_chain_begins_in_the_groove_and_only_the_matrix_can_talk_it_out() {
        // Emissions that say nothing: every window is exactly as likely to be music as
        // groove. What is left is the start bias and the transition matrix.
        let transitions = Transitions::from_durations(&Config::new(), 0.1);
        let indifferent = |n: usize| vec![[0.0f64; 2]; n];

        // Over a second, the bias holds and the whole thing is groove.
        assert!(
            viterbi(&indifferent(10), &transitions)
                .iter()
                .all(|&s| s == State::Gap)
        );

        // Over four seconds it does not, and that is not a bug. `min_sound_secs` is
        // 2.0 and `min_silence_secs` is 0.8, so the matrix says music lasts two and a
        // half times as long as groove, and over enough windows that preference
        // outweighs a one-off bias at the start. A side is mostly music; a model that
        // needed evidence to believe that would be the wrong model.
        let long = viterbi(&indifferent(40), &transitions);
        assert_eq!(long[0], State::Gap, "the needle drop was ignored");
        assert!(long[39] == State::Music, "the matrix never asserted itself");

        const { assert!(LEAD_IN_BIAS > 0.0) };
        assert!(
            transitions.log(State::Music, State::Music) > transitions.log(State::Gap, State::Gap)
        );
    }

    #[test]
    fn the_posterior_is_sure_of_a_fade_too_which_is_why_it_is_only_a_veto() {
        // The measurement that decided how this detector reports confidence. A needle
        // lift and a sixty-second fade both come back at 1.0, because the fitted
        // Gaussians are narrow and the log likelihood is quadratic in the distance
        // between them - so even a tenth of a decibel per window crosses decisively.
        // The posterior is a statement about the model's internal agreement, not about
        // how gradual the music was, and anything else written of it would be wishful.
        let layout = layout();

        let lift = trace_of(&[(-14.0, 0.30, 200), (-70.0, 1.00, 40), (-14.0, 0.30, 200)]);
        let mut fade = trace_of(&[(-14.0, 0.30, 200)]);
        for step in 0..600 {
            let db = -14.0 - (56.0 * f64::from(step) / 599.0);
            fade.push(Frame {
                rms: db_to_linear(db),
                flatness: 0.30,
            });
        }
        fade.extend(trace_of(&[(-70.0, 1.00, 40), (-14.0, 0.30, 200)]));

        for (name, frames) in [("lift", lift), ("fade", fade)] {
            let outcome = scan(&view(&frames, &layout), &Config::new());
            assert_eq!(outcome.regions.len(), 2, "{name}: {:?}", outcome.regions);
            // The interior boundary - the one the transition posterior is actually
            // about - in both cases.
            let interior = &outcome.boundaries[1];
            assert!(
                (interior.measurement("posterior").unwrap() - 1.0).abs() < 1e-3,
                "{name} posterior {:?}",
                interior.measurement("posterior")
            );
            assert_eq!(interior.measurement("posterior_applies"), Some(1.0));
            // And because the veto never fires, the confidence that comes out is the
            // level contrast, untouched.
            let contrast = interior.measurement("contrast_db").unwrap();
            let by_level = crate::regions::confidence_from(contrast, &Config::new());
            assert_eq!(
                interior.confidence, by_level,
                "{name}: the veto bound after all"
            );
        }
    }

    #[test]
    fn a_boundary_at_the_edge_of_a_capture_is_not_vetoed_by_a_transition_that_never_was() {
        // A side that begins in music: the track starts at frame zero, no state change
        // happens there, and the posterior is therefore nil. Before this was handled
        // the first boundary of every such side came back at a confidence of 0.0002 -
        // the least certain thing in the file, and the one thing not in doubt.
        let layout = layout();
        let frames = trace_of(&[(-14.0, 0.30, 300), (-70.0, 1.00, 40), (-14.0, 0.30, 300)]);
        let outcome = scan(&view(&frames, &layout), &Config::new());

        let first = &outcome.boundaries[0];
        assert_eq!(first.at, 0);
        assert_eq!(first.measurement("posterior_applies"), Some(0.0));
        assert!(
            first.confidence > 0.9,
            "the edge boundary scored {}",
            first.confidence
        );

        // The last boundary is the clearer illustration: the side ends in music, so
        // there is genuinely no music-to-groove transition anywhere near it and the
        // posterior is nil. Vetoing on that would have made the end of every side the
        // least trusted boundary in the project.
        let last = outcome.boundaries.last().unwrap();
        assert!(
            last.measurement("posterior").unwrap() < 0.01,
            "a transition after all"
        );
        assert_eq!(last.measurement("posterior_applies"), Some(0.0));
        assert!(
            last.confidence > 0.9,
            "the edge boundary scored {}",
            last.confidence
        );
    }

    #[test]
    fn the_posteriors_add_up_to_the_number_of_boundaries_there_are() {
        // The property that makes it a probability rather than a score: summed over the
        // side, the posterior of entering music counts the times the side entered
        // music. Two tracks, so two starts and two ends, to within a rounding error.
        let layout = layout();
        let frames = trace_of(&[
            (-70.0, 1.00, 20),
            (-14.0, 0.30, 300),
            (-70.0, 1.00, 20),
            (-14.0, 0.30, 300),
            (-70.0, 1.00, 20),
        ]);
        let trace = view(&frames, &layout);
        let levels: Vec<f64> = frames.iter().map(Frame::level_db).collect();
        let flatness: Vec<f64> = frames.iter().map(|frame| frame.flatness).collect();
        let emissions = Emissions::estimate(&levels, &flatness);
        let per_window: Vec<[f64; 2]> = frames
            .iter()
            .map(|frame| emissions.log_prob(frame))
            .collect();
        let transitions = Transitions::from_durations(&Config::new(), trace.shape.window_seconds());
        let posterior = posteriors(&per_window, &transitions);

        let starts: f64 = posterior.iter().map(|pair| pair[0]).sum();
        let ends: f64 = posterior.iter().map(|pair| pair[1]).sum();
        // Not exact: the residual is probability the model puts on paths that cross
        // more often than twice, which is what a hedge looks like when it is summed.
        assert!(
            (starts - 2.0).abs() < 0.05,
            "the side started music {starts} times"
        );
        assert!((ends - 2.0).abs() < 0.05, "the side stopped {ends} times");
        assert_eq!(
            posterior[0],
            [0.0, 0.0],
            "a transition into the first window"
        );
        for pair in &posterior {
            assert!(
                pair[0] >= 0.0 && pair[0] <= 1.0,
                "{pair:?} is not a probability"
            );
            assert!(
                pair[1] >= 0.0 && pair[1] <= 1.0,
                "{pair:?} is not a probability"
            );
        }
    }

    #[test]
    fn a_constant_flatness_cancels_and_the_model_falls_back_to_levels() {
        // Which is exactly what a levels-only trace looks like, and why this detector
        // can run on the cheap extractor's output if a caller asks it to.
        let layout = layout();
        let runs: &[(f64, f64, usize)] = &[
            (-70.0, 0.0, 20),
            (-14.0, 0.0, 300),
            (-70.0, 0.0, 20),
            (-14.0, 0.0, 300),
        ];
        let zero = trace_of(runs);
        let half: Vec<Frame> = zero
            .iter()
            .map(|frame| Frame {
                rms: frame.rms,
                flatness: 0.5,
            })
            .collect();

        let from_zero = scan(&view(&zero, &layout), &Config::new());
        let from_half = scan(&view(&half, &layout), &Config::new());
        assert_eq!(from_zero.regions.len(), 2, "got {:?}", from_zero.regions);
        assert_eq!(
            from_zero.regions, from_half.regions,
            "a constant offset changed the answer"
        );
    }

    #[test]
    fn a_featureless_side_comes_back_as_one_track_and_admits_it_had_nothing_to_go_on() {
        let layout = layout();
        // Forty seconds at one level: no groove, no gaps, nothing to fit two
        // distributions to.
        let frames = trace_of(&[(-14.0, 0.30, 400)]);
        let trace = view(&frames, &layout);
        let outcome = scan(&trace, &Config::new());

        // One track spanning the lot. That is the right failure: a detector that
        // fabricated boundaries in a flat line would put them in front of a user as
        // though they meant something.
        assert_eq!(outcome.regions.len(), 1, "{:?}", outcome.regions);
        assert_eq!(
            outcome.regions[0],
            Region {
                start: 0,
                end: trace.total_frames
            }
        );

        // Both of its boundaries are the ends of the capture, which are certain by
        // construction - the audio does start and stop there - so the confidence says
        // 1.0 and is not lying. What disqualifies the *answer* is the evidence: the two
        // states it fitted are the same state, and a resolver reading a separation of
        // nought knows to give this detector no weight on this side.
        let separation = outcome.boundaries[0].measurement("separation").unwrap();
        assert!(separation.abs() < 0.1, "separation {separation}");
        for boundary in &outcome.boundaries {
            assert_eq!(boundary.measurement("posterior_applies"), Some(0.0));
        }

        let levels: Vec<f64> = frames.iter().map(Frame::level_db).collect();
        let flatness: Vec<f64> = frames.iter().map(|frame| frame.flatness).collect();
        let emissions = Emissions::estimate(&levels, &flatness);
        // The floors are what stop two identical distributions becoming infinitely
        // sharp and the model infinitely certain.
        assert_eq!(emissions.gap_level.sigma, LEVEL_FLOOR_DB);
        assert_eq!(emissions.gap_flatness.sigma, FLATNESS_FLOOR);
    }

    #[test]
    fn a_side_whose_two_states_overlap_declines_rather_than_guesses() {
        // The other direction of the same failure. Levels that swing twenty-four
        // decibels window to window put loud music and quiet groove inside both fitted
        // distributions, and the chain can no longer tell them apart. It reports no
        // tracks at all, which is the answer a resolver can do something with - the
        // other two detectors are still voting.
        let layout = layout();
        let mut frames = Vec::new();
        for (db, count) in [
            (-60.0, 20),
            (-14.0, 300),
            (-26.0, 20),
            (-14.0, 300),
            (-60.0, 20),
        ] {
            for window in 0..count {
                let jitter = if window % 2 == 0 { 12.0 } else { -12.0 };
                frames.push(Frame {
                    rms: db_to_linear(db + jitter),
                    flatness: 0.3,
                });
            }
        }
        let outcome = scan(&view(&frames, &layout), &Config::new());
        assert!(
            outcome.regions.is_empty(),
            "it guessed anyway: {:?}",
            outcome.regions
        );
        assert!(outcome.boundaries.is_empty());
        // The diagnostics still come back, so a caller knows the pass ran.
        assert_eq!(outcome.diagnostics.windows, frames.len());
    }

    #[test]
    fn the_transition_priors_are_the_durations_the_user_configured() {
        let cfg = Config::new();
        let transitions = Transitions::from_durations(&cfg, 0.1);
        // min_silence 0.8 s is eight windows, so a one-in-eight chance of leaving the
        // groove each window; min_sound 2.0 s is twenty.
        assert!((transitions.log(State::Gap, State::Music).exp() - 0.125).abs() < 1e-12);
        assert!((transitions.log(State::Music, State::Gap).exp() - 0.05).abs() < 1e-12);
        assert!((transitions.log(State::Gap, State::Gap).exp() - 0.875).abs() < 1e-12);

        // A minimum shorter than two windows would make leaving more likely than
        // staying, which is not a state at all.
        let hasty = Config {
            min_silence_secs: 0.05,
            ..cfg
        };
        let capped = Transitions::from_durations(&hasty, 0.1);
        assert!((capped.log(State::Gap, State::Music).exp() - 0.5).abs() < 1e-12);

        // And a longer minimum makes the model more reluctant, not just its output
        // more filtered.
        let patient = Config {
            min_silence_secs: 4.0,
            ..cfg
        };
        let slow = Transitions::from_durations(&patient, 0.1);
        assert!(slow.log(State::Gap, State::Music) < transitions.log(State::Gap, State::Music));
    }

    #[test]
    fn a_distribution_fitted_to_one_value_is_not_allowed_to_be_certain() {
        let same = Gaussian::fit(&[-70.0; 50], LEVEL_FLOOR_DB);
        assert!((same.mean + 70.0).abs() < 1e-12);
        assert_eq!(same.sigma, LEVEL_FLOOR_DB);
        // Two decibels out is then a little over half a standard deviation, not an
        // impossibility.
        assert!(same.log_prob(-72.0) > same.log_prob(-80.0));

        let spread = Gaussian::fit(&[-10.0, -20.0, -30.0], LEVEL_FLOOR_DB);
        assert!((spread.mean + 20.0).abs() < 1e-12);
        assert!((spread.sigma - (200.0f64 / 3.0).sqrt()).abs() < 1e-12);
        assert_eq!(Gaussian::fit(&[], 0.5).sigma, 0.5);
    }

    #[test]
    fn a_log_sum_survives_an_impossible_state() {
        assert!((log_sum(0.0, 0.0) - 2.0f64.ln()).abs() < 1e-12);
        assert_eq!(
            log_sum(f64::NEG_INFINITY, f64::NEG_INFINITY),
            f64::NEG_INFINITY
        );
        assert!((log_sum(-5.0, f64::NEG_INFINITY) + 5.0).abs() < 1e-12);
        // Well outside what an f64 can exponentiate, and still exact to a decibel.
        assert!((log_sum(-4_000.0, -4_010.0) - (-4_000.0 + (-10.0f64).exp().ln_1p())).abs() < 1e-9);
    }

    #[test]
    fn a_side_still_turning_leaves_its_last_run_open_to_the_last_frame() {
        let layout = layout();
        let frames = trace_of(&[(-70.0, 1.0, 20), (-14.0, 0.3, 300)]);
        let total = 320 * W - 913;
        let trace = Trace::new(&frames, &layout, total);
        let states: Vec<State> = (0..320)
            .map(|window| {
                if window < 20 {
                    State::Gap
                } else {
                    State::Music
                }
            })
            .collect();
        let raw = runs(&states, &trace);
        assert_eq!(raw.len(), 1);
        assert_eq!(
            raw[0],
            Region {
                start: 20 * W,
                end: total
            }
        );
    }

    #[test]
    fn an_empty_trace_produces_nothing_from_every_stage() {
        let layout = layout();
        let frames: Vec<Frame> = Vec::new();
        assert!(viterbi(&[], &Transitions::from_durations(&Config::new(), 0.1)).is_empty());
        assert!(posteriors(&[], &Transitions::from_durations(&Config::new(), 0.1)).is_empty());
        let outcome = scan(&Trace::new(&frames, &layout, 0), &Config::new());
        assert!(outcome.boundaries.is_empty());
        assert_eq!(outcome.diagnostics.windows, 0);
    }
}
