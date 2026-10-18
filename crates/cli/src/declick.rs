/*
 *  declick.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The vcw declick verb: find the clicks in a stored capture and record them.
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

//! The `vcw declick` verb: the click detector over a stored side (WP-30, §46).
//!
//! §4.5 asks for the whole workflow to be drivable with no UI, and this is the
//! verb the parameter re-derivation needs: the survey's section 4 says the
//! published defaults cannot be trusted on LP material until they have been run
//! against real rips, and five flags on a command line is how that gets done.
//!
//! Unlike `vcw detect`, this one **writes**. A defect row is not an edit to the
//! audio - §4.1 keeps the blocks immutable and the repair is computed on the way
//! out - but it is a conclusion stored in the project, so `--dry-run` exists and
//! prints exactly what the real run prints.
//!
//! The first line of the report is the fraction of the side that was flagged. It
//! is first because it is the number that decides whether the rest of the report
//! is worth reading.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vcw_core::declick::{Found, Intent};
use vcw_project::{Project, session};
use vcw_signal::declick::Config;
use vcw_types::Repair;

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to analyze.
    pub(crate) project: PathBuf,
    /// Capture to analyze. `None` takes the most recent.
    pub(crate) capture: Option<i64>,
    /// Analysis frame length in milliseconds.
    pub(crate) window_ms: Option<u32>,
    /// How much of a frame the next one repeats, as a percentage.
    pub(crate) overlap: Option<u32>,
    /// Model order as a percentage of the frame length.
    pub(crate) order: Option<f64>,
    /// How many standard deviations of the residual counts as damage.
    pub(crate) threshold: Option<f64>,
    /// Fusion distance in samples.
    pub(crate) burst: Option<usize>,
    /// How many times to re-fit the model.
    pub(crate) passes: Option<u32>,
    /// What to call the damage.
    pub(crate) kind: String,
    /// What to propose doing about each span.
    pub(crate) repair: String,
    /// Find and report, but write nothing.
    pub(crate) dry_run: bool,
    /// Print every span.
    pub(crate) spans: bool,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Builds the detector configuration the flags asked for.
fn config(args: &Args) -> Config {
    let mut cfg = Config::default();
    if let Some(ms) = args.window_ms {
        cfg.window_millis = ms;
    }
    if let Some(percent) = args.overlap {
        cfg.overlap_percent = percent;
    }
    if let Some(percent) = args.order {
        cfg.order_percent = percent;
    }
    if let Some(k) = args.threshold {
        cfg.threshold = k;
    }
    if let Some(b) = args.burst {
        cfg.burst = b;
    }
    if let Some(n) = args.passes {
        cfg.passes = n;
    }
    cfg
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }
    // Refused, not defaulted: the schema stores no default repair because a
    // repair nobody chose is one nobody can be held to.
    let Some(repair) = Repair::parse(&args.repair) else {
        bail!(
            "--repair {} is not a repair; choose interpolate, mute or none",
            args.repair
        );
    };
    if args.kind.trim().is_empty() {
        bail!("--kind cannot be empty: a span of damage has to be called something");
    }

    let project = Project::open_read_only(&args.project)
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
    project.close()?;

    let cfg = config(args);
    let intent = Intent {
        kind: args.kind.clone(),
        repair,
    };
    let found = if args.dry_run {
        vcw_core::declick::preview_project(&args.project, capture_id, &cfg, &intent)?
    } else {
        vcw_core::declick::scan_project(&args.project, capture_id, &cfg, &intent)?
    };

    if args.json {
        print_json(capture_id, rate, &cfg, &intent, &found);
        return Ok(());
    }
    print_report(args, &record, rate, &cfg, &intent, &found);
    Ok(())
}

fn print_report(
    args: &Args,
    record: &session::Record,
    rate: f64,
    cfg: &Config,
    intent: &Intent,
    found: &Found,
) {
    let diagnostics = &found.diagnostics;
    println!(
        "  capture    {}, {} Hz, {} ch, {:.3} s",
        record.id,
        record.info.rate.hz(),
        record.info.channels,
        record.duration_secs()
    );
    println!(
        "  settings   window {} ms, overlap {}%, order {:.1}% ({} taps), \
K {}, burst {}, {} pass(es)",
        cfg.window_millis,
        cfg.overlap_percent,
        cfg.order_percent,
        diagnostics.order,
        cfg.threshold,
        cfg.burst,
        cfg.passes
    );
    // First, because it decides whether the rest is worth reading. Oudre's
    // published defaults mark five to seven percent of every sample on the
    // material they were tuned on; anything near that is a detector setting and
    // not a record.
    println!(
        "  flagged    {:.4}% of all samples, {} span(s), {} damaged frames",
        found.flagged_fraction() * 100.0,
        found.spans.len(),
        found.flagged_frames()
    );
    println!(
        "  repair     {} proposed as '{}'",
        intent.repair.as_str(),
        intent.kind
    );
    if found.recorded {
        println!(
            "  written    {} row(s), {} stale row(s) removed",
            found.written, found.removed
        );
    } else {
        println!("  dry run    {} row(s) would be written", found.written);
    }
    println!("  took       {:.3} s", found.took.as_secs_f64());

    if found.spans.is_empty() {
        return;
    }
    if !args.spans {
        // A real side can carry thousands of these, and a screenful of them is
        // not the report: the fraction above is.
        println!("  (--spans to list them)");
        return;
    }
    println!();
    println!(
        "  {:>12}  {:>9}  {:>3}  {:>6}  {:>8}",
        "at", "length", "ch", "conf", "margin"
    );
    for span in &found.spans {
        println!(
            "  {:>12.4}  {:>7} fr  {:>3}  {:>6.2}  {:>8.2}",
            span.start as f64 / rate,
            span.frames(),
            span.channel,
            span.confidence,
            span.peak_ratio
        );
    }
}

fn print_json(capture_id: i64, rate: f64, cfg: &Config, intent: &Intent, found: &Found) {
    let spans: Vec<String> = found
        .spans
        .iter()
        .map(|span| {
            format!(
                r#"{{"start_frame":{},"end_frame":{},"at_seconds":{:.6},"channel":{},"confidence":{:.4},"peak_ratio":{:.4}}}"#,
                span.start,
                span.end,
                span.start as f64 / rate,
                span.channel,
                span.confidence,
                span.peak_ratio
            )
        })
        .collect();
    println!(
        concat!(
            r#"{{"capture_id":{},"recorded":{},"window_millis":{},"overlap_percent":{},"#,
            r#""order_percent":{},"order":{},"threshold":{},"burst":{},"passes":{},"#,
            r#""kind":"{}","repair":"{}","flagged_fraction":{:.8},"flagged_frames":{},"#,
            r#""spans":{},"written":{},"removed":{},"took_seconds":{:.6},"defects":[{}]}}"#
        ),
        capture_id,
        found.recorded,
        cfg.window_millis,
        cfg.overlap_percent,
        cfg.order_percent,
        found.diagnostics.order,
        cfg.threshold,
        cfg.burst,
        cfg.passes,
        intent.kind,
        intent.repair.as_str(),
        found.flagged_fraction(),
        found.flagged_frames(),
        found.spans.len(),
        found.written,
        found.removed,
        found.took.as_secs_f64(),
        spans.join(",")
    );
}
