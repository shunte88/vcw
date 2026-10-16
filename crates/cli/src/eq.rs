/*
 *  eq.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  `vcw eq` - the §51 curve catalog, on a terminal.
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

//! `vcw eq` - the §51 curve catalog, on a terminal.
//!
//! `list` and `show` read the catalog; `provenance` writes the one §51 fact that
//! belongs to the signal chain rather than to the record, which is whether the
//! captured audio had already been equalized. Choosing a curve is a level down, at
//! `vcw release set --playback-eq`, because the characteristic is a property of the
//! pressing and the catalog is only where its numbers live.
//!
//! What `list` and `show` are for is making the catalog checkable. §51 requires that
//! every curve's parameters carry a citation to the published source they came from and
//! that the implemented response be measured against it - and a citation nobody can
//! read is not a citation. `show` prints the source beside the numbers, the response
//! at the frequencies the sources publish, and the realized error at every rate §8
//! allows, so the tolerance claim can be checked by the person relying on it rather
//! than only by the test suite.

use anyhow::{Context, Result};
use vcw_eq::{Catalog, TOLERANCE_DB, worst_error_db};
use vcw_types::STANDARD_RATES;

/// The frequencies the published tables state. Printed as a block so a VCW curve can
/// be read against the chart it came from without arithmetic.
const PUBLISHED_HZ: [f64; 9] = [
    20.0, 30.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, 10_000.0, 20_000.0,
];

/// The catalog VCW ships, with a user file merged over it if one was named.
fn catalog(overrides: Option<&str>) -> Result<Catalog> {
    let builtin = Catalog::builtin();
    let Some(path) = overrides else {
        return Ok(builtin);
    };
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let user = Catalog::parse(&text).with_context(|| format!("parsing {path}"))?;
    Ok(builtin.with_overrides(user))
}

/// Checks a curve id before it is written into a project, and hands it back.
///
/// Against the shipped catalog rather than one a `--curves` file extends: a curve
/// stored in a project has to resolve on whatever machine opens it next, and a name
/// that only this invocation knows would be a dangling reference in the file. A
/// user-defined curve becomes storable when the catalog has a settled home rather
/// than a per-command flag.
///
/// Absence is not an error anywhere this is called - a release that names no curve
/// is the usual case - but a typo is, which is the whole point of asking.
pub(crate) fn require(id: &str) -> Result<String> {
    Catalog::builtin().require(id)?;
    Ok(id.to_owned())
}

/// One line per curve: what it is, and the three numbers that define it.
pub(crate) fn list(overrides: Option<&str>, json: bool) -> Result<()> {
    let catalog = catalog(overrides)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&catalog.iter().collect::<Vec<_>>())?
        );
        return Ok(());
    }

    println!(
        "{:<18} {:<27} {:<11} {:>7} {:>9} {:>7}",
        "ID", "NAME", "CODE", "SHELF", "TURNOVER", "TREBLE"
    );
    for curve in catalog.iter() {
        println!(
            "{:<18} {:<27} {:<11} {:>7} {:>9} {:>7}",
            curve.id,
            curve.name,
            curve.code.as_deref().unwrap_or("-"),
            micros(curve.shelf_us),
            micros(curve.turnover_us),
            micros(curve.treble_us),
        );
    }
    println!(
        "\nTime constants in microseconds; a dash means the characteristic was cut\n\
         without that network. `vcw eq show <id>` prints one in full, with the\n\
         source it came from. `vcw release <project> set --playback-eq <id>`\n\
         states the one a record was cut with."
    );
    Ok(())
}

/// One curve in full: its parameters, its source, its response, its realized error.
pub(crate) fn show(id: &str, overrides: Option<&str>, json: bool) -> Result<()> {
    let catalog = catalog(overrides)?;
    let curve = catalog.require(id)?;

    if json {
        println!("{}", serde_json::to_string_pretty(curve)?);
        return Ok(());
    }

    println!("{}  ({})", curve.name, curve.id);
    field("code", curve.code.as_deref());
    constant("shelf", curve.shelf_us);
    constant("turnover", curve.turnover_us);
    constant("treble", curve.treble_us);
    if curve.shelf_us.is_none() && !curve.is_flat() {
        println!(
            "  {:<12} +{:.2} dB (synthesized: this characteristic was cut with no\n\
             {:<14} shelf, so replay needs one or the bass rises without limit)",
            "replay shelf", curve.replay_shelf_db, ""
        );
    }
    field("era", curve.era.as_deref());
    field("labels", curve.labels.as_deref());
    field("source", Some(curve.citation.as_str()));
    field("disputed", curve.disputed.as_deref());

    println!("\n  response, 0 dB at 1 kHz");
    for hz in PUBLISHED_HZ {
        println!("    {:>8.0} Hz  {:>+8.2} dB", hz, curve.response_db(hz));
    }

    println!("\n  realized error against those numbers, per §8 rate");
    let mut worst: f64 = 0.0;
    for rate in STANDARD_RATES {
        let error = worst_error_db(curve, rate.hz())?;
        worst = worst.max(error);
        println!("    {:>8} Hz  {:>8.3} dB", rate.hz(), error);
    }
    println!(
        "\n  Worst {worst:.3} dB against §51's budget of {TOLERANCE_DB:.1}.\n\n  \
         The response above is the definition, not a reading off a chart: in most\n  \
         published sources a figure given as \"dB at 10 kHz\" describes the treble\n  \
         section alone and not the finished curve, which is why VCW stores the time\n  \
         constants and measures against them."
    );
    Ok(())
}

/// Prints a time constant with its unit, or says the network is not there.
fn constant(label: &str, us: Option<f64>) {
    match us {
        Some(_) => println!("  {label:<12} {} us", micros(us)),
        None => println!("  {label:<12} none"),
    }
}

/// Prints a labelled line, or nothing when the curve does not carry the fact.
fn field(label: &str, value: Option<&str>) {
    if let Some(value) = value {
        println!("  {label:<12} {value}");
    }
}

/// A time constant for a column, or a dash where the network does not exist.
fn micros(us: Option<f64>) -> String {
    us.map_or_else(
        || "-".to_owned(),
        |v| {
            if v.fract() == 0.0 {
                format!("{v:.0}")
            } else {
                format!("{v:.1}")
            }
        },
    )
}

/// States what the signal chain had already applied, on a capture that is
/// already in a project (§51).
///
/// The gap this fills. `vcw session --capture-eq` asks at record time and the
/// window asks once in settings, but an imported project (§12) has no record
/// time to ask at - Audacity does not store the answer and VCW will not invent
/// it, so every imported capture is `unknown`. A curve over an unknown capture
/// is refused, which left an imported project able to name a characteristic and
/// unable to hear it.
///
/// Every capture unless one is named, because the answer is a property of the
/// hifi the records went through, not of one recording.
pub(crate) fn provenance(
    project: &std::path::Path,
    eq: vcw_types::CaptureEq,
    capture_id: Option<i64>,
    json: bool,
) -> Result<()> {
    let mut open = vcw_project::Project::open(project)
        .with_context(|| format!("opening {}", project.display()))?;
    let changed = vcw_project::session::set_capture_eq(&mut open, capture_id, eq)?;
    if changed == 0 {
        anyhow::bail!(match capture_id {
            Some(id) => format!("there is no capture {id} in this project"),
            None => "this project has no captures yet".to_owned(),
        });
    }
    if json {
        println!(
            "{}",
            serde_json::json!({ "captures": changed, "capture_eq": eq.as_str() })
        );
    } else {
        println!(
            "{changed} capture(s) now state the signal chain was {}",
            eq.as_str()
        );
    }
    Ok(())
}
