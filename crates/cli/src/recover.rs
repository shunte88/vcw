/*
 *  recover.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `vcw recover` verb: find unfinished captures and close them honestly.
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

//! The `vcw recover` verb: find unfinished captures and close them honestly.
//!
//! §15 asks that the next launch *detect* unfinished sessions and *offer*
//! recovery. Offering is the operative word, so this reports by default and
//! writes only when told to. A recording that survived a crash is worth more
//! than the convenience of not typing `--apply`.
//!
//! The sidecars are read before the project is opened, because opening it is
//! what makes the evidence disappear.
//!
//! # What a dry run does and does not touch
//!
//! A dry run writes nothing to the *database*: no row changes, no block
//! removed, and the capture is still offered for recovery afterwards. It is not
//! side-effect-free on the *filesystem*, and cannot be. Opening a SQLite
//! database replays any log left behind, and closing it folds that log into the
//! main file and deletes the `-wal` and `-shm`. So the first `vcw recover` on a
//! crashed project consumes the hot log whatever flags it was given, and the
//! main file grows by roughly the log's size.
//!
//! That is the right behavior - the log is committed data, and folding it in
//! is how it stops being at risk - but it means a dry run is a report and not a
//! snapshot. Anyone who wants the crashed state kept has to copy the file and
//! both sidecars together, before running anything at all.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vcw_project::recovery::{self, Assessment, Plan, Sidecars};
use vcw_project::{Options, Project, validate};

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to examine.
    pub(crate) project: PathBuf,
    /// Write the results rather than only reporting them.
    pub(crate) apply: bool,
    /// Allow stranded blocks to be deleted. Implies `apply`.
    pub(crate) repair: bool,
    /// Recompute every block's checksum afterwards.
    pub(crate) verify: bool,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }
    // Before any connection: a hot log is evidence that the last process to
    // hold this file did not close it, and connecting replays it away.
    let sidecars = Sidecars::inspect(&args.project);

    let mut project = Project::open(&args.project)
        .with_context(|| format!("opening {}", args.project.display()))?;
    let found = recovery::survey(project.conn())?;

    let plan = match (args.repair, args.apply) {
        (true, _) => Plan::Repair,
        (false, true) => Plan::Commit,
        (false, false) => Plan::DryRun,
    };

    let mut done = Vec::with_capacity(found.len());
    for assessment in &found {
        done.push(recovery::recover(&mut project, assessment, plan)?);
    }

    let report = if args.verify {
        Some(validate(
            &project,
            Options {
                verify_checksums: true,
            },
        )?)
    } else {
        None
    };

    // Folding the log back is part of the lifecycle, not an optimization, and
    // a project that has just been recovered is exactly the one worth leaving
    // tidy. Only when something was written: a dry run must not touch the file.
    let folded = if plan == Plan::DryRun {
        0
    } else {
        recovery::checkpoint(&project)?
    };

    if args.json {
        print_json(&sidecars, &found, &done, report.as_ref(), folded, plan);
    } else {
        print_human(&sidecars, &found, &done, report.as_ref(), folded, plan);
    }

    project.close()?;

    if let Some(report) = report
        && !report.is_clean()
    {
        bail!("the project did not validate cleanly after recovery");
    }
    Ok(())
}

fn print_human(
    sidecars: &Sidecars,
    found: &[Assessment],
    done: &[recovery::Recovered],
    report: Option<&vcw_project::Report>,
    folded: u64,
    plan: Plan,
) {
    println!("{}", sidecars.project.display());
    if sidecars.log_left_behind() {
        println!(
            "  log         {} left behind: the last process to hold this file did not close it",
            mib(sidecars.wal_bytes)
        );
    } else {
        println!("  log         none left behind; the last close was clean");
    }

    if found.is_empty() {
        println!("  captures    nothing unfinished");
    }
    for (assessment, outcome) in found.iter().zip(done) {
        println!(
            "  capture {:<3} {} frames on every channel, {:.3} s, {} block(s)",
            assessment.capture_id,
            assessment.usable_frames,
            assessment.duration_secs(),
            assessment.blocks
        );
        println!(
            "              {} Hz, {} ch, {:?}, started {}",
            assessment.info.rate.hz(),
            assessment.info.channels,
            assessment.info.storage_format,
            assessment.started_at
        );
        let d = assessment.diagnostics;
        println!(
            "              {} overrun(s), {} underrun(s), {} dropped frame(s), {} stream error(s){}",
            d.overruns,
            d.underruns,
            d.dropped_frames,
            d.stream_errors,
            match assessment.counter_lag_secs() {
                Some(lag) if lag > 0 => format!(", last written {lag} s before the end"),
                _ => String::new(),
            }
        );
        for note in &assessment.notes {
            println!("  !           {:<20} {}", note.code, note.detail);
        }
        if outcome.applied {
            println!(
                "  applied     frames {}, state recovered, finished_at {}{}",
                outcome.frames,
                outcome.finished_at,
                if outcome.blocks_removed > 0 {
                    format!(", {} block(s) removed", outcome.blocks_removed)
                } else {
                    String::new()
                }
            );
        }
    }

    if let Some(report) = report {
        if report.is_clean() {
            println!(
                "  validate    clean over {} capture(s) and {} block(s), checksums recomputed",
                report.captures, report.blocks
            );
        } else {
            for finding in &report.findings {
                println!("  !           {:<20} {}", finding.code, finding.detail);
            }
        }
    }

    match plan {
        Plan::DryRun if found.is_empty() => println!("  verdict     nothing to do"),
        Plan::DryRun => println!(
            "  verdict     {} capture(s) recoverable; nothing written to the project. \
             Re-run with --apply",
            found.len()
        ),
        _ => println!(
            // `folded` is what *recovery* wrote, not what the crash left: any
            // log found on arrival was already folded when the project opened,
            // and the line above reports that separately.
            "  verdict     {} capture(s) recovered; {} written since it opened, now folded back",
            done.iter().filter(|d| d.applied).count(),
            mib(folded)
        ),
    }
}

fn print_json(
    sidecars: &Sidecars,
    found: &[Assessment],
    done: &[recovery::Recovered],
    report: Option<&vcw_project::Report>,
    folded: u64,
    plan: Plan,
) {
    let captures: Vec<_> = found
        .iter()
        .zip(done)
        .map(|(a, outcome)| {
            serde_json::json!({
                "capture_id": a.capture_id,
                "rate": a.info.rate.hz(),
                "channels": a.info.channels,
                "state": format!("{:?}", a.state),
                "started_at": a.started_at,
                "declared_frames": a.declared_frames,
                "usable_frames": a.usable_frames,
                "duration_secs": a.duration_secs(),
                "blocks": a.blocks,
                "surplus_blocks": a.surplus.len(),
                "channels_present": a.channels_present,
                "last_committed_at": a.last_committed_at,
                "consistent": a.is_consistent(),
                "counter_lag_secs": a.counter_lag_secs(),
                "diagnostics": {
                    "overruns": a.diagnostics.overruns,
                    "underruns": a.diagnostics.underruns,
                    "dropped_frames": a.diagnostics.dropped_frames,
                    "stream_errors": a.diagnostics.stream_errors,
                },
                "notes": a.notes.iter()
                    .map(|n| serde_json::json!({"code": n.code, "detail": n.detail}))
                    .collect::<Vec<_>>(),
                "applied": outcome.applied,
                "blocks_removed": outcome.blocks_removed,
                "finished_at": outcome.finished_at,
            })
        })
        .collect();

    let out = serde_json::json!({
        "project": sidecars.project.display().to_string(),
        "plan": format!("{plan:?}"),
        "sidecars": {
            "wal_bytes": sidecars.wal_bytes,
            "shm_bytes": sidecars.shm_bytes,
            "log_left_behind": sidecars.log_left_behind(),
        },
        "unfinished": found.len(),
        "captures": captures,
        "wal_folded_bytes": folded,
        "validated": report.map(|r| serde_json::json!({
            "clean": r.is_clean(),
            "captures": r.captures,
            "blocks": r.blocks,
            "findings": r.findings.iter()
                .map(|f| serde_json::json!({"code": f.code, "detail": f.detail}))
                .collect::<Vec<_>>(),
        })),
    });
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
}

fn mib(bytes: u64) -> String {
    format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
}
