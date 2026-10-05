/*
 *  silence.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The RMS energy detector: threshold with hysteresis, live and post-capture.
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

//! The RMS energy detector: threshold with hysteresis, live and post-capture.
//!
//! The first of §22's three required detectors, and the only one cheap enough to run
//! while the record is still turning. It looks at one number per window - the level -
//! and decides where the music is. [`crate::regions`] does everything after that.
//!
//! §22 asks for two things from this: "Live analysis creates provisional markers. A
//! post-capture pass may refine them using the complete recording." Both are here, and
//! they are deliberately *the same code*: [`Live`] accumulates feature frames and calls
//! [`scan`] on what it has so far, so a provisional marker is not an approximation of
//! the final answer, it is the final answer computed from less audio. The only thing
//! [`Live`] adds is knowing when a boundary has stopped being able to move - see
//! [`Live::settled`], which is what makes it safe to show a marker to the user before
//! the side has finished.
//!
//! Requirements: §22 (RMS energy, live and refine), §23 (observations, not edits),
//! §24 (provenance and confidence).

use crate::features::{Frame, Shape, Windows, db_to_linear};
use crate::regions::{
    Config, Diagnostics, Outcome, Region, Trace, boundaries_for, shape, threshold_for,
};
use vcw_types::{BoundaryObservation, Provenance};

/// Classifies every window as music or groove and returns the raw spans.
///
/// VRipr's phase 3, in frames. Two levels matter, not one: a window enters music at the
/// threshold and has to fall a whole `hysteresis_db` below it to leave again. Without
/// that, a fade or a quiet bar flickers across the threshold and the pipeline sees a
/// dozen boundaries where there is one.
///
/// A span that is still open at the end of the trace is closed at the capture's last
/// frame, not left out. A side that ends mid-groove is the normal case when a capture
/// is stopped by hand.
#[must_use]
pub fn classify(trace: &Trace<'_>, threshold_db: f64, cfg: &Config) -> Vec<Region> {
    let enter = db_to_linear(threshold_db);
    let leave = db_to_linear(threshold_db - cfg.hysteresis_db);

    let mut raw: Vec<Region> = Vec::new();
    let mut start: Option<u64> = None;
    for (window, frame) in trace.frames.iter().enumerate() {
        let at = trace.frame_at(window);
        match start {
            None if frame.rms >= enter => start = Some(at),
            Some(from) if frame.rms < leave => {
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
/// The entry point for both passes. The live pass calls it repeatedly on a growing
/// trace through [`Live`]; the refine pass calls it once on the whole side.
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
    let regions = shape(classify(trace, threshold_db, cfg), trace, cfg);
    let boundaries = boundaries_for(&regions, trace, cfg, Provenance::Silence, &diagnostics);
    Outcome {
        regions,
        boundaries,
        diagnostics,
    }
}

/// The live pass: feature frames in, provisional boundaries out.
///
/// Owns a levels-only [`Windows`] - no FFT, so it is the cheap extractor - and the trace
/// it has built so far. A twenty-five minute side is fifteen thousand windows, which is
/// a couple of hundred kilobytes and re-scans in well under a millisecond, so there is
/// no incremental state machine here and no need for one: keeping the trace and
/// re-running the real detector is both simpler and exactly consistent with the refine
/// pass.
#[derive(Debug)]
pub struct Live {
    cfg: Config,
    windows: Windows,
    frames: Vec<Frame>,
    /// The positions already handed out by [`Live::fresh`], so a marker is announced
    /// once. Small: two per track.
    announced: Vec<u64>,
}

impl Live {
    /// A live pass over audio of the given shape.
    #[must_use]
    pub fn new(shape: &Shape, cfg: Config) -> Self {
        Self {
            cfg,
            windows: Windows::new(shape),
            frames: Vec::new(),
            announced: Vec::new(),
        }
    }

    /// Consumes interleaved bytes in the capture's storage format.
    ///
    /// Returns how many windows this call completed. Zero is the common answer: a
    /// callback's worth of audio is a fraction of a window.
    pub fn push(&mut self, bytes: &[u8]) -> usize {
        self.windows.push(bytes, &mut self.frames)
    }

    /// Consumes already-decoded interleaved samples.
    pub fn push_samples(&mut self, interleaved: &[f32]) -> usize {
        self.windows.push_samples(interleaved, &mut self.frames)
    }

    /// The detector's current answer over everything pushed so far.
    #[must_use]
    pub fn outcome(&self) -> Outcome {
        scan(&Trace::from_windows(&self.frames, &self.windows), &self.cfg)
    }

    /// How many frames of audio have been analyzed.
    #[must_use]
    pub const fn analyzed(&self) -> u64 {
        self.windows.frames()
    }

    /// The frame before which a boundary can no longer move.
    ///
    /// A boundary is not final the moment the level crosses: a later span starting
    /// within `min_silence_secs` merges straight through it and the boundary ceases to
    /// exist, and padding and de-overlap can shift it by `pre_padding + post_padding`.
    /// Once the trace extends past all of that, nothing later can reach back, so the
    /// boundary is safe to show. At the defaults that is 1.2 s of lag, which is the
    /// price of never having to retract a marker.
    ///
    /// With an adaptive threshold nothing is ever settled, and this returns zero: the
    /// threshold itself is re-estimated from every new window, so a marker placed early
    /// can move when the quietest 3% of the side changes. That is a real limitation of
    /// adaptive detection live, not an implementation shortcut, and the caller should
    /// treat the whole answer as provisional.
    #[must_use]
    pub fn settled(&self) -> u64 {
        if self.cfg.adaptive {
            return 0;
        }
        let hz = f64::from(self.windows.shape().rate.hz());
        let lag = ((self.cfg.min_silence_secs
            + self.cfg.gap_fill_secs
            + self.cfg.pre_padding
            + self.cfg.post_padding)
            * hz) as u64;
        self.analyzed().saturating_sub(lag)
    }

    /// The settled boundaries not yet returned by an earlier call.
    ///
    /// This is what publishes §22's provisional markers: call it after each push and
    /// send what comes back. Boundaries past [`Live::settled`] are withheld rather than
    /// announced and later corrected, because a marker that jumps around while a record
    /// plays is worse than a marker that arrives a second late.
    pub fn fresh(&mut self) -> Vec<BoundaryObservation> {
        let settled = self.settled();
        let mut new = Vec::new();
        for boundary in self.outcome().boundaries {
            if boundary.at <= settled && !self.announced.contains(&boundary.at) {
                self.announced.push(boundary.at);
                new.push(boundary);
            }
        }
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{DEFAULT_WINDOW_MILLIS, linear_to_db};
    use std::f64::consts::PI;
    use vcw_types::{Edge, SampleRate, StorageFormat};

    const RATE: SampleRate = SampleRate(48_000);
    /// One window at the default 100 ms.
    const W: u64 = 4_800;

    fn layout() -> Shape {
        Shape::new(RATE, 2, StorageFormat::Int16, DEFAULT_WINDOW_MILLIS)
    }

    /// A trace from a run-length description in dB and windows.
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

    /// Interleaved stereo audio from a run-length description in dB and seconds.
    ///
    /// A 1 kHz sine scaled so its RMS is exactly the level asked for, which makes a
    /// level assertion a statement about the detector rather than about the generator.
    fn side(runs: &[(f64, f64)]) -> Vec<f32> {
        let hz = f64::from(RATE.hz());
        let mut out = Vec::new();
        let mut n = 0u64;
        for &(db, secs) in runs {
            let amplitude = db_to_linear(db) * 2.0f64.sqrt();
            for _ in 0..(secs * hz) as u64 {
                let sample = (amplitude * (2.0 * PI * 1_000.0 * n as f64 / hz).sin()) as f32;
                out.push(sample);
                out.push(sample);
                n += 1;
            }
        }
        out
    }

    fn windows_of(region: &Region) -> (u64, u64) {
        (region.start / W, region.end / W)
    }

    #[test]
    fn three_tracks_come_out_of_a_side_with_two_gaps() {
        let layout = layout();
        let frames = trace_of(&[
            (-70.0, 20),  // 2 s lead-in
            (-14.0, 300), // track one, 30 s
            (-70.0, 20),  // 2 s gap
            (-14.0, 380), // track two, 38 s
            (-70.0, 15),  // 1.5 s gap
            (-14.0, 290), // track three
            (-70.0, 25),  // lead-out
        ]);
        let trace = view(&frames, &layout);
        let outcome = scan(&trace, &Config::new());

        assert_eq!(outcome.regions.len(), 3, "got {:?}", outcome.regions);
        // Each region starts a tenth of a second early and ends a tenth late, which is
        // the padding, and no boundary has drifted further than that.
        assert_eq!(windows_of(&outcome.regions[0]), (19, 321));
        assert_eq!(windows_of(&outcome.regions[1]), (339, 721));
        assert_eq!(windows_of(&outcome.regions[2]), (734, 1_026));

        assert_eq!(outcome.boundaries.len(), 6);
        for boundary in &outcome.boundaries {
            assert_eq!(boundary.provenance, Provenance::Silence);
            assert!(
                boundary.confidence > 0.99,
                "{} scored {}",
                boundary.at,
                boundary.confidence
            );
            assert!(boundary.measurement("contrast_db").unwrap() > 50.0);
        }
        assert_eq!(outcome.boundaries[0].edge, Edge::Start);
        assert_eq!(outcome.boundaries[1].edge, Edge::End);
        assert_eq!(outcome.diagnostics.threshold_db, -40.0);
        assert_eq!(outcome.diagnostics.windows, frames.len());
    }

    #[test]
    fn a_quiet_bar_does_not_split_a_track_in_two() {
        let layout = layout();
        // The threshold is -40 and the band is 6 dB wide. A passage at -43 is below
        // the level that would have *started* a track and nowhere near the level that
        // ends one, which is the entire purpose of the band.
        let frames = trace_of(&[
            (-70.0, 20),
            (-14.0, 100),
            (-43.0, 40),
            (-14.0, 100),
            (-70.0, 20),
        ]);
        let trace = view(&frames, &layout);
        let outcome = scan(&trace, &Config::new());
        assert_eq!(
            outcome.regions.len(),
            1,
            "the quiet passage split the track: {:?}",
            outcome.regions
        );

        // Without the band it splits, which is what makes this a property of the
        // hysteresis rather than of the numbers happening to line up.
        let brittle = Config {
            hysteresis_db: 0.0,
            ..Config::new()
        };
        assert_eq!(scan(&trace, &brittle).regions.len(), 2);
    }

    #[test]
    fn a_pop_in_the_groove_is_not_a_track_and_a_crackle_does_not_end_one() {
        let layout = layout();
        // A single loud window in the middle of the lead-out, and a single quiet
        // window in the middle of a track.
        let frames = trace_of(&[
            (-14.0, 300),
            (-70.0, 1),
            (-14.0, 300), // a crackle, bridged by gap_fill
            (-70.0, 30),
            (-8.0, 1), // a pop, dropped by min_sound
            (-70.0, 30),
        ]);
        let trace = view(&frames, &layout);
        let outcome = scan(&trace, &Config::new());
        assert_eq!(outcome.regions.len(), 1, "got {:?}", outcome.regions);
        assert_eq!(windows_of(&outcome.regions[0]), (0, 602));
    }

    #[test]
    fn a_side_still_turning_is_closed_at_the_last_frame_captured() {
        let layout = layout();
        let frames = trace_of(&[(-70.0, 20), (-14.0, 300)]);
        // Stopped by hand, part way through a window.
        let total = 320 * W - 913;
        let trace = Trace::new(&frames, &layout, total);
        let outcome = scan(&trace, &Config::new());
        assert_eq!(outcome.regions.len(), 1);
        assert_eq!(
            outcome.regions[0].end, total,
            "the open region was dropped or over-ran"
        );
        assert_eq!(outcome.diagnostics.total_frames, total);
    }

    #[test]
    fn nothing_but_groove_is_no_tracks_rather_than_one_long_one() {
        let layout = layout();
        let frames = trace_of(&[(-70.0, 600)]);
        let trace = view(&frames, &layout);
        assert!(scan(&trace, &Config::new()).regions.is_empty());

        // And an empty trace is an empty answer that still reports its threshold.
        let empty: Vec<Frame> = Vec::new();
        let outcome = scan(&Trace::new(&empty, &layout, 0), &Config::new());
        assert!(outcome.boundaries.is_empty());
        assert_eq!(outcome.diagnostics.threshold_db, -40.0);
    }

    #[test]
    fn an_adaptive_threshold_finds_gaps_a_fixed_one_sits_below() {
        let layout = layout();
        // A noisy pressing: the groove between tracks is at -35 dBFS, above the fixed
        // -40 dB threshold. The fixed detector hears one continuous forty-minute
        // track. This is the case §22's "adaptive" is for.
        let frames = trace_of(&[
            (-35.0, 30),
            (-14.0, 200),
            (-35.0, 30),
            (-14.0, 200),
            (-35.0, 30),
        ]);
        let trace = view(&frames, &layout);

        let fixed = scan(&trace, &Config::new());
        assert_eq!(
            fixed.regions.len(),
            1,
            "a fixed threshold should have been fooled"
        );

        let adaptive = scan(&trace, &Config::adaptive());
        assert_eq!(adaptive.regions.len(), 2, "got {:?}", adaptive.regions);
        let floor = adaptive.diagnostics.floor_db.unwrap();
        assert!((floor + 35.0).abs() < 0.5, "the floor landed at {floor} dB");
        assert!((adaptive.diagnostics.threshold_db - (floor + 12.0)).abs() < 1e-9);
        // The derived threshold is in the evidence, because a boundary found at -23 dB
        // means something different from one found at -40.
        assert_eq!(
            adaptive.boundaries[0].measurement("threshold_db"),
            Some(floor + 12.0)
        );
        assert_eq!(adaptive.boundaries[0].measurement("floor_db"), Some(floor));
    }

    #[test]
    fn the_live_pass_and_the_refine_pass_agree() {
        // The claim §22 rests on: a provisional marker is not a cheaper approximation,
        // it is the same detector with less audio. Feed one path a synthesised side in
        // ragged pieces and the other the whole thing, and every boundary must match
        // to the frame.
        let layout = layout();
        let audio = side(&[
            (-70.0, 2.0),
            (-14.0, 12.0),
            (-70.0, 2.0),
            (-14.0, 9.0),
            (-70.0, 2.0),
        ]);

        let mut live = Live::new(&layout, Config::new());
        for piece in audio.chunks(7_331) {
            live.push_samples(piece);
        }

        let mut frames = Vec::new();
        let mut windows = Windows::new(&layout);
        windows.push_samples(&audio, &mut frames);
        let once = scan(&Trace::from_windows(&frames, &windows), &Config::new());

        let live_outcome = live.outcome();
        assert_eq!(
            live_outcome.regions.len(),
            2,
            "got {:?}",
            live_outcome.regions
        );
        assert_eq!(live_outcome.regions, once.regions);
        assert_eq!(live_outcome.boundaries, once.boundaries);
        assert_eq!(live.analyzed(), audio.len() as u64 / 2);

        // And the levels came out where the generator put them.
        let music = linear_to_db(frames[50].rms);
        assert!((music + 14.0).abs() < 0.1, "the music read {music} dB");
    }

    #[test]
    fn a_marker_is_announced_once_and_never_before_it_is_safe() {
        let layout = layout();
        let mut live = Live::new(&layout, Config::new());

        // Two seconds of groove then eight of music. The start is old enough that
        // nothing arriving later can move it, so it goes out; the end is wherever the
        // audio happens to stop right now, so it does not.
        live.push_samples(&side(&[(-70.0, 2.0), (-14.0, 8.0)]));
        let announced = live.fresh();
        assert_eq!(announced.len(), 1, "got {announced:?}");
        assert_eq!(announced[0].edge, Edge::Start);
        assert_eq!(announced[0].at, 19 * W, "2 s in, less the pre-padding");
        let open_end = live.outcome().boundaries[1].at;
        assert!(
            open_end > live.settled(),
            "the end moves with every window and went out anyway"
        );

        // More music: the start is not re-announced and the end is still moving.
        live.push_samples(&side(&[(-14.0, 2.0)]));
        assert!(
            live.fresh().is_empty(),
            "the same marker was announced twice"
        );

        // A real gap closes the track, and two seconds later the end has settled.
        live.push_samples(&side(&[(-70.0, 2.0)]));
        let announced = live.fresh();
        assert_eq!(announced.len(), 1, "got {announced:?}");
        assert_eq!(announced[0].edge, Edge::End);
        assert_eq!(announced[0].at, 121 * W, "12 s in, plus the post-padding");
        assert!(live.fresh().is_empty());
    }

    #[test]
    fn an_adaptive_live_pass_settles_nothing_and_says_so() {
        // Honesty about a real limitation rather than a comfortable answer: the
        // threshold is re-estimated from the quietest 3% of everything captured so
        // far, so any marker can move when the next gap arrives.
        let layout = layout();
        let mut live = Live::new(&layout, Config::adaptive());
        live.push_samples(&side(&[(-70.0, 2.0), (-14.0, 20.0)]));
        assert_eq!(live.settled(), 0);
        assert!(live.fresh().is_empty());
        assert_eq!(
            live.outcome().regions.len(),
            1,
            "it still detects, it just cannot commit"
        );
    }

    #[test]
    fn the_live_pass_takes_bytes_in_the_capture_s_own_format() {
        // What the fan-out tap actually hands over: interleaved storage-format bytes,
        // a ring's worth at a time, never window-aligned.
        let layout = layout();
        let audio = side(&[(-70.0, 1.0), (-14.0, 6.0)]);
        let bytes: Vec<u8> = audio
            .iter()
            .flat_map(|&sample| ((sample * 32_767.0) as i16).to_le_bytes())
            .collect();

        let mut live = Live::new(&layout, Config::new());
        let mut completed = 0;
        for piece in bytes.chunks(4_093) {
            completed += live.push(piece);
        }
        assert_eq!(completed, 70, "a window went missing between pushes");
        assert_eq!(live.analyzed(), 7 * 48_000);
        let outcome = live.outcome();
        assert_eq!(outcome.regions.len(), 1);
        assert_eq!(windows_of(&outcome.regions[0]), (9, 70));
    }
}
