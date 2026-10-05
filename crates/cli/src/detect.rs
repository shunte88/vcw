/*
 *  detect.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The 'vcw detect' verb: run the post-capture detection pass over a stored side (22, 23, 24).
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

//! The `vcw detect` verb: run the post-capture pass over a stored side (§22, §24).
//!
//! §4.5 asks for the whole workflow to be drivable with no UI, and detection is
//! the first part of it whose *output is an argument*. So this prints the
//! argument: not just where the boundaries are, but which detectors reported
//! each one, how confident each was, and the measurements they took. §24 wants a
//! boundary to carry its evidence, and evidence a person cannot read is evidence
//! only in name.
//!
//! Nothing here writes. A boundary becomes a track in WP-13, and the tracks this
//! prints are labeled implied for that reason: they are the pairing a UI would
//! draw, not rows in the project.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vcw_core::detection::{self, Refined};
use vcw_project::{Project, session};
use vcw_signal::regions::Config;
use vcw_signal::resolve::Decision;
use vcw_types::Edge;

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to analyze.
    pub(crate) project: PathBuf,
    /// Capture to analyze. `None` takes the most recent.
    pub(crate) capture: Option<i64>,
    /// Level a window must reach to count as music, in dBFS.
    pub(crate) threshold_db: Option<f64>,
    /// Derive the threshold from the side's own noise floor instead.
    pub(crate) adaptive: bool,
    /// Shortest gap that can separate two tracks, in seconds.
    pub(crate) min_silence: Option<f64>,
    /// Shortest span that can be a track, in seconds.
    pub(crate) min_sound: Option<f64>,
    /// Report only boundaries this many detectors reported.
    ///
    /// The HMM finds a boundary at every quiet bar of a real side - faithfully, it
    /// is what VRipr does - so this is how to see the ones a second detector
    /// seconded. See `docs/STATUS.md` on WP-11.
    pub(crate) min_sources: usize,
    /// Print every measurement behind every boundary.
    pub(crate) evidence: bool,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Builds the detector configuration the flags asked for.
fn config(args: &Args) -> Config {
    let mut cfg = if args.adaptive {
        Config::adaptive()
    } else {
        Config::new()
    };
    if let Some(db) = args.threshold_db {
        cfg.threshold_db = db;
    }
    if let Some(secs) = args.min_silence {
        cfg.min_silence_secs = secs;
    }
    if let Some(secs) = args.min_sound {
        cfg.min_sound_secs = secs;
    }
    cfg
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }

    let project = Project::open(&args.project)
        .with_context(|| format!("opening {}", args.project.display()))?;
    let capture_id = match args.capture {
        Some(id) => id,
        None => match session::all(project.conn())?.last() {
            Some(record) => record.id,
            None => bail!("{} has no captures", args.project.display()),
        },
    };
    let record = session::load(project.conn(), capture_id)?
        .ok_or_else(|| anyhow::anyhow!("capture {capture_id} is not in this project"))?;
    let rate = f64::from(record.info.rate.hz()).max(1.0);

    let cfg = config(args);
    // Read-only, because analysis has nothing to write: the same reason a side can
    // be examined while another one is being recorded.
    drop(project);
    let refined = detection::refine_project(&args.project, capture_id, &cfg, &[])?;

    if args.json {
        print_json(args, capture_id, rate, &cfg, &refined);
        return Ok(());
    }
    print_report(args, &record, &cfg, &refined);
    Ok(())
}

/// The decisions the caller asked to see.
fn reported<'a>(args: &Args, refined: &'a Refined) -> Vec<&'a Decision> {
    refined
        .decisions
        .iter()
        .filter(|decision| decision.agreement() >= args.min_sources.max(1))
        .collect()
}

/// The tracks the boundaries imply, as start-end pairs in seconds.
///
/// A start with no end after it is the side running out mid-track, which is what a
/// capture stopped early looks like, so it is reported rather than dropped.
fn implied(decisions: &[&Decision], rate: f64) -> Vec<(f64, Option<f64>)> {
    let mut out: Vec<(f64, Option<f64>)> = Vec::new();
    for decision in decisions {
        let at = decision.at as f64 / rate;
        match decision.edge {
            Edge::Start => out.push((at, None)),
            Edge::End => match out.last_mut() {
                Some(last) if last.1.is_none() => last.1 = Some(at),
                _ => {}
            },
        }
    }
    out
}

/// One line describing where a boundary came from: `silence+hmm (2 of 3)`.
fn agreement(decision: &Decision) -> String {
    let names: Vec<&str> = decision.sources.iter().map(|p| p.as_str()).collect();
    format!("{} ({})", names.join("+"), decision.agreement())
}

fn print_report(args: &Args, record: &session::Record, cfg: &Config, refined: &Refined) {
    let rate = f64::from(record.info.rate.hz()).max(1.0);
    println!(
        "  capture    {}, {} Hz, {} ch, {:.3} s",
        record.id,
        record.info.rate.hz(),
        record.info.channels,
        record.duration_secs()
    );
    println!(
        "  analysis   {} window(s), {} detector(s), {:.3} s",
        refined.windows,
        refined.diagnostics.len(),
        refined.took.as_secs_f64()
    );
    println!(
        "  settings   threshold {:.1} dB {}, hysteresis {:.1} dB, min sound {:.2} s, \
         min silence {:.2} s",
        cfg.threshold_db,
        if cfg.adaptive { "adaptive" } else { "fixed" },
        cfg.hysteresis_db,
        cfg.min_sound_secs,
        cfg.min_silence_secs
    );

    for (provenance, diagnostics) in &refined.diagnostics {
        let found = refined
            .observations
            .iter()
            .filter(|o| o.provenance == *provenance)
            .count();
        let floor = diagnostics
            .floor_db
            .map_or_else(|| "-".to_string(), |db| format!("{db:.1} dB"));
        println!(
            "  {:<15} {:>2} boundary/ies at {:>7.1} dB, floor {floor}",
            provenance.as_str(),
            found,
            diagnostics.threshold_db
        );
    }

    let shown = reported(args, refined);
    if shown.is_empty() {
        println!("  boundaries none - the side reads as one continuous take");
        return;
    }

    if args.min_sources > 1 {
        println!(
            "  showing    {} of {} boundary/ies, those {} or more detectors reported",
            shown.len(),
            refined.decisions.len(),
            args.min_sources
        );
    }
    println!("  boundaries");
    for (index, decision) in shown.iter().enumerate() {
        println!(
            "    {:>2}  {:<5}  {:>9.3} s  conf {:.2}{}  {}",
            index + 1,
            decision.edge.as_str(),
            decision.at as f64 / rate,
            decision.confidence,
            if decision.locked { "  locked" } else { "" },
            agreement(decision)
        );
        if args.evidence {
            for item in &decision.evidence {
                println!("          {:<34} {:>12.4}", item.name, item.value);
            }
        }
    }

    let tracks = implied(&shown, rate);
    println!("  implied    {} track(s), nothing written", tracks.len());
    for (index, (start, end)) in tracks.iter().enumerate() {
        match end {
            Some(end) => println!(
                "    {:>2}  {start:>9.3} s to {end:>9.3} s  ({:.3} s)",
                index + 1,
                end - start
            ),
            None => println!(
                "    {:>2}  {start:>9.3} s to       -      (the capture stopped mid-track)",
                index + 1
            ),
        }
    }
}

fn print_json(args: &Args, capture_id: i64, rate: f64, cfg: &Config, refined: &Refined) {
    let value = serde_json::json!({
        "capture_id": capture_id,
        "rate": rate as u32,
        "windows": refined.windows,
        "took_micros": refined.took.as_micros() as u64,
        "settings": {
            "threshold_db": cfg.threshold_db,
            "adaptive": cfg.adaptive,
            "hysteresis_db": cfg.hysteresis_db,
            "min_sound_secs": cfg.min_sound_secs,
            "min_silence_secs": cfg.min_silence_secs,
        },
        "detectors": refined.diagnostics.iter().map(|(provenance, diagnostics)| {
            serde_json::json!({
                "provenance": provenance.as_str(),
                "threshold_db": diagnostics.threshold_db,
                "floor_db": diagnostics.floor_db,
                "windows": diagnostics.windows,
                "total_frames": diagnostics.total_frames,
                "boundaries": refined.observations.iter()
                    .filter(|o| o.provenance == *provenance).count(),
            })
        }).collect::<Vec<_>>(),
        "min_sources": args.min_sources,
        "boundaries": reported(args, refined).iter().map(|decision| {
            serde_json::json!({
                "frame": decision.at,
                "seconds": decision.at as f64 / rate,
                "edge": decision.edge.as_str(),
                "confidence": decision.confidence,
                "provenance": decision.provenance.as_str(),
                "sources": decision.sources.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
                "agreement": decision.agreement(),
                "locked": decision.locked,
                "evidence": if args.evidence {
                    serde_json::json!(decision.evidence.iter().map(|e| {
                        serde_json::json!({ "name": e.name, "value": e.value })
                    }).collect::<Vec<_>>())
                } else {
                    serde_json::Value::Null
                },
            })
        }).collect::<Vec<_>>(),
        "implied_tracks": implied(&reported(args, refined), rate).iter().map(|(start, end)| {
            serde_json::json!({ "start_secs": start, "end_secs": end })
        }).collect::<Vec<_>>(),
    });
    println!("{value}");
}

#[cfg(test)]
mod tests {
    use vcw_types::Provenance;

    use super::*;

    fn args() -> Args {
        Args {
            project: PathBuf::new(),
            capture: None,
            threshold_db: None,
            adaptive: false,
            min_silence: None,
            min_sound: None,
            min_sources: 1,
            evidence: false,
            json: false,
        }
    }

    #[test]
    fn the_flags_reach_the_detectors() {
        let cfg = config(&Args {
            adaptive: true,
            threshold_db: Some(-52.0),
            min_silence: Some(1.5),
            min_sound: Some(4.0),
            ..args()
        });
        assert!(cfg.adaptive, "--adaptive was not honored");
        assert!((cfg.threshold_db + 52.0).abs() < f64::EPSILON);
        assert!((cfg.min_silence_secs - 1.5).abs() < f64::EPSILON);
        assert!((cfg.min_sound_secs - 4.0).abs() < f64::EPSILON);
        // Untouched by the flags, so still VRipr's figure.
        assert!((cfg.hysteresis_db - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_default_is_vripr_s_own_configuration() {
        assert_eq!(config(&args()), Config::new());
    }

    #[test]
    fn a_boundary_only_one_detector_saw_can_be_left_out() {
        // The reason the flag exists: on a real side the HMM reports hundreds of
        // boundaries nothing else can see, and a caller wants the seconded ones.
        let refined = Refined {
            decisions: vec![
                Decision {
                    at: 0,
                    edge: Edge::Start,
                    confidence: 1.0,
                    provenance: Provenance::Silence,
                    sources: vec![Provenance::Silence, Provenance::Hmm],
                    evidence: Vec::new(),
                    locked: false,
                },
                Decision {
                    at: 48_000,
                    edge: Edge::End,
                    confidence: 0.5,
                    provenance: Provenance::Hmm,
                    sources: vec![Provenance::Hmm],
                    evidence: Vec::new(),
                    locked: false,
                },
            ],
            observations: Vec::new(),
            diagnostics: Vec::new(),
            windows: 0,
            took: std::time::Duration::ZERO,
        };
        assert_eq!(
            reported(&args(), &refined).len(),
            2,
            "the default hides nothing"
        );
        let seconded = reported(
            &Args {
                min_sources: 2,
                ..args()
            },
            &refined,
        );
        assert_eq!(seconded.len(), 1);
        assert_eq!(seconded[0].at, 0);
        // A floor of zero is a floor of one: nothing has no sources.
        assert_eq!(
            reported(
                &Args {
                    min_sources: 0,
                    ..args()
                },
                &refined
            )
            .len(),
            2
        );
    }

    #[test]
    fn a_start_with_no_end_is_a_capture_that_stopped_mid_track() {
        let at = |frame, edge| Decision {
            at: frame,
            edge,
            confidence: 1.0,
            provenance: Provenance::Silence,
            sources: vec![Provenance::Silence],
            evidence: Vec::new(),
            locked: false,
        };
        let decisions = [
            at(0, Edge::Start),
            at(48_000, Edge::End),
            at(96_000, Edge::Start),
        ];
        let tracks = implied(&decisions.iter().collect::<Vec<_>>(), 48_000.0);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0], (0.0, Some(1.0)));
        assert_eq!(tracks[1], (2.0, None), "the unfinished track was dropped");
    }
}
