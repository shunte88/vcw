/*
 *  tracks.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The vcw tracks verb: the editing model without a UI (§4.5, §31).
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

//! The `vcw tracks` verb: the editing model without a UI (§4.5, §31).
//!
//! §4.5 asks for the whole workflow to be drivable headless, and this is the part
//! of it that decides where the tracks are. Every subcommand here is one call into
//! `vcw-project`, which is deliberate: if the model needed a layer of CLI logic to
//! be usable, the UI would need the same layer written twice.
//!
//! `adopt` is the exception and the interesting one. It runs a detection pass and
//! writes the result under a promotion policy, which is the only operation here
//! that can put a boundary in the project without a person naming a frame - so it
//! is also the only one that defaults to conservative (`--min-sources 2`) and the
//! only one that reports what it turned down.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vcw_core::adopt::{self, Policy};
use vcw_project::track::{Boundary, Record, Update};
use vcw_project::{Project, disc, release, session, side, track};
use vcw_signal::regions::Config;
use vcw_types::vinyl::{Numbering, Side};

/// What `vcw tracks` was asked to do.
#[derive(Debug, Clone)]
pub(crate) enum Task {
    /// Print the project's sides, tracks and boundaries.
    List {
        /// One side, or every side.
        side: Option<char>,
        /// Print boundaries as well as tracks, with their evidence (§24).
        boundaries: bool,
    },
    /// Point a side at the capture that recorded it.
    Attach {
        /// The side letter.
        side: char,
        /// The capture. `None` takes the most recent.
        capture: Option<i64>,
    },
    /// Add a track between two frames.
    Add {
        /// The side letter.
        side: char,
        /// Where it starts, in seconds.
        start: f64,
        /// Where it ends, in seconds.
        end: f64,
    },
    /// Split a track in two.
    Split {
        /// The track.
        track: i64,
        /// Where to cut, in seconds from the start of the side.
        at: f64,
    },
    /// Merge two adjacent tracks.
    Merge {
        /// The track that survives.
        left: i64,
        /// The track folded into it.
        right: i64,
    },
    /// Delete a track, keeping every sample (§4.1).
    Delete {
        /// The track.
        track: i64,
    },
    /// Move a boundary.
    Move {
        /// The boundary.
        boundary: i64,
        /// Where to, in seconds.
        to: f64,
        /// Move it even if it is locked, which also claims it (§24).
        force: bool,
    },
    /// Lock or unlock a boundary against analysis (§24).
    Lock {
        /// The boundary.
        boundary: i64,
        /// Hand it back to analysis instead.
        unlock: bool,
    },
    /// Set a track's metadata (§32).
    Set {
        /// The track.
        track: i64,
        /// The change to apply.
        update: Update,
    },
    /// Move a track to another side of the same capture.
    Reassign {
        /// The track.
        track: i64,
        /// The side letter.
        to: char,
    },
    /// Run a detection pass and write what the policy accepts.
    Adopt {
        /// The side letter.
        side: char,
        /// How many detectors must agree.
        min_sources: usize,
        /// The shortest span worth calling a track, in seconds.
        min_track: Option<f64>,
        /// Report what would be written without writing it.
        dry_run: bool,
        /// Level a window must reach to count as music, in dBFS.
        threshold_db: Option<f64>,
        /// Derive the threshold from the side's own noise floor (§22).
        adaptive: bool,
    },
}

/// What `vcw tracks` was asked to do, and to which project.
#[derive(Debug, Clone)]
pub(crate) struct Args {
    /// The project.
    pub(crate) project: PathBuf,
    /// The task.
    pub(crate) task: Task,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Runs one `vcw tracks` subcommand.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }
    // Every task but `list` writes, and `list` costs nothing extra opened
    // writable, so one open serves both rather than two code paths.
    let mut project = Project::open(&args.project)
        .with_context(|| format!("opening {}", args.project.display()))?;

    match &args.task {
        Task::List { side, boundaries } => list(&project, *side, *boundaries, args.json)?,
        Task::Attach { side, capture } => {
            let letter = letter(*side)?;
            let capture_id = match capture {
                Some(id) => *id,
                None => match session::all(project.conn())?.last() {
                    Some(record) => record.id,
                    None => bail!("{} has no captures", args.project.display()),
                },
            };
            let record = side::attach(&mut project, letter, capture_id)?;
            println!("side {} now reads capture {capture_id}", record.letter());
        }
        Task::Add { side, start, end } => {
            let letter = letter(*side)?;
            let rate = rate_of(&project, letter)?;
            let id = track::add_track(
                &mut project,
                letter,
                frames(*start, rate),
                frames(*end, rate),
            )?;
            println!("track {id} added to side {side}");
        }
        Task::Split { track: id, at } => {
            let rate = rate_of_track(&project, *id)?;
            let second = track::split(&mut project, *id, frames(*at, rate))?;
            println!("track {id} split at {at:.3} s, giving track {second}");
        }
        Task::Merge { left, right } => {
            track::merge(&mut project, *left, *right)?;
            println!("track {right} merged into track {left}");
        }
        Task::Delete { track: id } => {
            track::remove(&mut project, *id)?;
            println!("track {id} deleted - every sample it covered is still in the project");
        }
        Task::Move {
            boundary,
            to,
            force,
        } => {
            let rate = rate_of_boundary(&project, *boundary)?;
            let at = frames(*to, rate);
            if *force {
                track::move_boundary_forced(&mut project, *boundary, at)?;
                println!("boundary {boundary} moved to {to:.3} s and claimed as yours");
            } else {
                track::move_boundary(&mut project, *boundary, at)?;
                println!("boundary {boundary} moved to {to:.3} s");
            }
        }
        Task::Lock { boundary, unlock } => {
            track::set_lock(&mut project, *boundary, !*unlock)?;
            if *unlock {
                println!("boundary {boundary} unlocked - analysis may move it again");
            } else {
                println!("boundary {boundary} locked - analysis will leave it alone");
            }
        }
        Task::Set { track: id, update } => {
            track::update(&mut project, *id, update)?;
            let record = track::track(project.conn(), *id)?
                .ok_or_else(|| anyhow::anyhow!("track {id} is not in this project"))?;
            println!("track {id} is now {}", describe(&record));
        }
        Task::Reassign { track: id, to } => {
            track::move_to_side(&mut project, *id, letter(*to)?)?;
            println!("track {id} moved to side {to}");
        }
        Task::Adopt { .. } => adopt_pass(&mut project, &args.task, args.json)?,
    }
    project.close()?;
    Ok(())
}

/// Prints the project's topology.
fn list(project: &Project, only: Option<char>, boundaries: bool, json: bool) -> Result<()> {
    let numbering = release::load(project.conn())?.map_or(Numbering::Alpha, |r| r.numbering);
    // Rendered over the whole project even when one side was asked for, because
    // `sequence` counts across a disc's sides: side B's numbers depend on how
    // many tracks side A holds, so `--side B` alone cannot render them.
    let rendered: std::collections::HashMap<i64, String> =
        track::positions(project.conn(), numbering)?
            .into_iter()
            .map(|(record, text)| (record.id, text))
            .collect();
    let sides = match only {
        Some(letter) => vec![side::require(project.conn(), self::letter(letter)?)?],
        None => side::list(project.conn())?,
    };

    if json {
        print_json(project, &sides, boundaries, &rendered)?;
        return Ok(());
    }

    if sides.is_empty() {
        println!("no sides yet - `vcw tracks attach --side A` names one");
        return Ok(());
    }
    let expected = disc::expected(project.conn())?;
    let missing = disc::missing(project.conn())?;
    println!(
        "  release    {expected} disc(s) claimed, {} side(s) present, numbering {}",
        sides.len(),
        numbering.name()
    );
    if !missing.is_empty() {
        let letters: String = missing.iter().map(|s| s.letter()).collect();
        println!("  still to record  {letters}");
    }

    for record in &sides {
        let rate = record
            .capture
            .and_then(|id| session::load(project.conn(), id).ok().flatten())
            .map_or(1.0, |c| f64::from(c.info.rate.hz()).max(1.0));
        println!(
            "\n  side {} (disc {}, {})  {}",
            record.letter(),
            record.disc(),
            match record.face() {
                vcw_types::vinyl::Face::First => "first face",
                vcw_types::vinyl::Face::Second => "second face",
            },
            record.capture.map_or_else(
                || "no capture attached".to_string(),
                |id| format!("capture {id}")
            )
        );
        if let Some(title) = &record.title {
            println!("    titled {title}");
        }

        let tracks = track::tracks_of(project.conn(), record.id)?;
        if tracks.is_empty() {
            println!("    no tracks yet");
        }
        for one in &tracks {
            println!(
                "    {:<5} {:>9.3} - {:>9.3} s  ({:>7.3} s)  {}",
                rendered.get(&one.id).map_or("", String::as_str),
                one.start as f64 / rate,
                one.end as f64 / rate,
                one.frames() as f64 / rate,
                describe(one)
            );
        }
        if boundaries {
            for one in track::boundaries_of(project.conn(), record.id)? {
                print_boundary(&one, rate);
            }
        }
    }
    Ok(())
}

fn print_boundary(one: &Boundary, rate: f64) {
    let sources: Vec<&str> = one.sources.iter().map(|p| p.as_str()).collect();
    println!(
        "    #{:<4} {:>9.3} s  {:<5} {:<15} {:.2}  {}{}",
        one.id,
        one.at_frame as f64 / rate,
        one.edge.as_str(),
        one.provenance.as_str(),
        one.confidence,
        if one.locked { "locked " } else { "" },
        if sources.is_empty() {
            String::new()
        } else {
            format!("[{}]", sources.join(" "))
        }
    );
    for measurement in &one.evidence {
        println!(
            "           {:<24} {:>12.4}",
            measurement.name, measurement.value
        );
    }
}

/// Runs the detection pass and adopts it.
fn adopt_pass(project: &mut Project, task: &Task, json: bool) -> Result<()> {
    let Task::Adopt {
        side: letter_of,
        min_sources,
        min_track,
        dry_run,
        threshold_db,
        adaptive,
    } = task
    else {
        unreachable!("adopt_pass is only called for Task::Adopt");
    };
    let chosen = letter(*letter_of)?;
    let record = side::require(project.conn(), chosen)?;
    let capture = record.capture.ok_or_else(|| {
        anyhow::anyhow!(
            "side {} has no capture - `vcw tracks attach --side {}` first",
            chosen.letter(),
            chosen.letter()
        )
    })?;
    let rate = rate_of(project, chosen)?;

    let mut cfg = Config::new();
    if let Some(db) = threshold_db {
        cfg.threshold_db = *db;
    }
    cfg.adaptive = *adaptive;

    let mut policy = Policy::at(vcw_types::SampleRate(rate as u32));
    policy.min_sources = *min_sources;
    if let Some(secs) = min_track {
        policy.min_track_frames = frames(*secs, rate);
    }

    // The pass is handed what the project already believes, which is what keeps a
    // boundary the operator placed where they put it (§24).
    let already = adopt::observations(project, chosen)?;
    let refined = vcw_core::detection::refine(project, capture, &cfg, &already)?;

    if *dry_run {
        let kept = refined
            .decisions
            .iter()
            .filter(|d| policy.accepts(d))
            .count();
        if json {
            println!(
                "{{\"side\":\"{}\",\"decisions\":{},\"accepted\":{},\"rejected\":{},\"written\":false}}",
                chosen.letter(),
                refined.decisions.len(),
                kept,
                refined.decisions.len() - kept
            );
        } else {
            println!(
                "  side {}  {} decision(s), {kept} would be adopted, {} turned down by \
                 --min-sources {min_sources}",
                chosen.letter(),
                refined.decisions.len(),
                refined.decisions.len() - kept
            );
            for decision in &refined.decisions {
                let mark = if policy.accepts(decision) { "+" } else { "-" };
                println!(
                    "    {mark} {:>9.3} s  {:<5} {:<15} {:.2}  {} source(s)",
                    decision.at as f64 / rate,
                    decision.edge.as_str(),
                    decision.provenance.as_str(),
                    decision.confidence,
                    decision.agreement()
                );
            }
        }
        return Ok(());
    }

    let adopted = adopt::adopt(project, chosen, &refined, &policy)?;
    if json {
        println!(
            "{{\"side\":\"{}\",\"boundaries\":{},\"tracks\":{},\"rejected\":{},\
             \"already_locked\":{},\"too_short\":{}}}",
            chosen.letter(),
            adopted.written(),
            adopted.tracks.len(),
            adopted.rejected,
            adopted.already_locked,
            adopted.too_short
        );
    } else {
        println!(
            "  side {}  {} boundary/ies written, {} track(s) created",
            chosen.letter(),
            adopted.written(),
            adopted.tracks.len()
        );
        println!(
            "           {} turned down by --min-sources {min_sources}, {} already settled by \
             a locked boundary, {} too short to be a track",
            adopted.rejected, adopted.already_locked, adopted.too_short
        );
    }
    Ok(())
}

fn print_json(
    project: &Project,
    sides: &[side::Record],
    boundaries: bool,
    rendered: &std::collections::HashMap<i64, String>,
) -> Result<()> {
    let mut out = String::from("{\"sides\":[");
    for (index, record) in sides.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"side\":\"{}\",\"disc\":{},\"capture\":{},\"tracks\":[",
            record.letter(),
            record.disc(),
            record
                .capture
                .map_or_else(|| "null".to_string(), |id| id.to_string())
        ));
        for (n, one) in track::tracks_of(project.conn(), record.id)?
            .iter()
            .enumerate()
        {
            if n > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"id\":{},\"position\":\"{}\",\"start\":{},\"end\":{},\"title\":{}}}",
                one.id,
                rendered.get(&one.id).map_or("", String::as_str),
                one.start,
                one.end,
                quote(&one.title)
            ));
        }
        out.push(']');
        if boundaries {
            out.push_str(",\"boundaries\":[");
            for (n, one) in track::boundaries_of(project.conn(), record.id)?
                .iter()
                .enumerate()
            {
                if n > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    "{{\"id\":{},\"at\":{},\"edge\":\"{}\",\"provenance\":\"{}\",\
                     \"confidence\":{:.4},\"sources\":{},\"locked\":{}}}",
                    one.id,
                    one.at_frame,
                    one.edge.as_str(),
                    one.provenance.as_str(),
                    one.confidence,
                    one.agreement(),
                    one.locked
                ));
            }
            out.push(']');
        }
        out.push('}');
    }
    out.push_str("]}");
    println!("{out}");
    Ok(())
}

/// A track in one line, for a person.
fn describe(record: &Record) -> String {
    let mut text = if record.title.is_empty() {
        "(untitled)".to_string()
    } else {
        record.title.clone()
    };
    if let Some(artist) = &record.artist {
        text.push_str(&format!(" - {artist}"));
    }
    if record.confirmed {
        text.push_str(" [confirmed]");
    }
    text
}

/// A side letter, or a message naming what was wrong with it.
fn letter(given: char) -> Result<Side> {
    Side::from_letter(given)
        .ok_or_else(|| anyhow::anyhow!("{given:?} is not a side letter - A to Z, A being first"))
}

/// Seconds to frames, rounded to the nearest.
///
/// Rounding rather than truncating because an operator typing a boundary read off
/// a waveform is naming an instant, not a floor, and half a frame either way is
/// eleven microseconds.
fn frames(seconds: f64, rate: f64) -> u64 {
    (seconds.max(0.0) * rate).round() as u64
}

/// The sample rate of the capture behind a side.
fn rate_of(project: &Project, chosen: Side) -> Result<f64> {
    let record = side::require(project.conn(), chosen)?;
    let capture = record.capture.ok_or_else(|| {
        anyhow::anyhow!(
            "side {} has no capture, so a time in seconds means nothing yet - \
             `vcw tracks attach --side {}` first",
            chosen.letter(),
            chosen.letter()
        )
    })?;
    let info = session::load(project.conn(), capture)?
        .ok_or_else(|| anyhow::anyhow!("capture {capture} is not in this project"))?;
    Ok(f64::from(info.info.rate.hz()).max(1.0))
}

fn rate_of_track(project: &Project, id: i64) -> Result<f64> {
    let record = track::track(project.conn(), id)?
        .ok_or_else(|| anyhow::anyhow!("track {id} is not in this project"))?;
    rate_of_side_id(project, record.side_id)
}

fn rate_of_boundary(project: &Project, id: i64) -> Result<f64> {
    let record = track::boundary(project.conn(), id)?
        .ok_or_else(|| anyhow::anyhow!("boundary {id} is not in this project"))?;
    rate_of_side_id(project, record.side_id)
}

fn rate_of_side_id(project: &Project, side_id: i64) -> Result<f64> {
    let record = side::by_id(project.conn(), side_id)?
        .ok_or_else(|| anyhow::anyhow!("side {side_id} is not in this project"))?;
    rate_of(project, record.side)
}

/// A JSON string, escaped enough for the fields a project holds.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
