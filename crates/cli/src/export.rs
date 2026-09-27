/*
 *  export.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `vcw export` verb: a project becomes files on disk.
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

//! The `vcw export` verb: a project becomes files on disk (§33, §4.5).
//!
//! The last step of §50's workflow and the one that makes everything before it
//! worth having. Two phases, which is [`vcw_export::splitter`]'s shape and not
//! this verb's invention: the plan is resolved and printed, and only then is any
//! audio written. `--dry-run` stops between the two, which is how to argue with a
//! naming template without producing a gigabyte of files to delete.
//!
//! Nothing here decides anything. The container, the template, the tags and the
//! artwork policy are all arguments passed straight through, because the UI
//! (WP-16) has to make the same decisions from the same settings and a CLI that
//! quietly did something extra would be a second implementation to keep in step.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vcw_export::encoder::Container;
use vcw_export::splitter::{self, Artwork, Plan, Progress, Report, Request};
use vcw_project::Project;
use vcw_types::vinyl::Side;

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to export from.
    pub(crate) project: PathBuf,
    /// Directory everything goes under.
    pub(crate) into: PathBuf,
    /// Container to write.
    pub(crate) format: String,
    /// Naming template, or `None` for the default.
    pub(crate) template: Option<String>,
    /// Sides to export. Empty exports every side that has tracks.
    pub(crate) sides: Vec<char>,
    /// What to do with the release's front cover.
    pub(crate) artwork: String,
    /// Whether files already there may be replaced.
    pub(crate) overwrite: bool,
    /// Resolve and print the plan, and write nothing.
    pub(crate) dry_run: bool,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }
    let container = container(&args.format)?;
    let request = Request {
        container,
        template: args
            .template
            .clone()
            .unwrap_or_else(|| vcw_export::naming::DEFAULT_TEMPLATE.to_owned()),
        into: args.into.clone(),
        sides: args
            .sides
            .iter()
            .map(|&given| letter(given))
            .collect::<Result<_>>()?,
        artwork: artwork(&args.artwork)?,
        overwrite: args.overwrite,
    };

    // Read-only, because §33 says an export reads immutable blocks and edit
    // instructions. Opening it writable would make a crash mid-export a risk to
    // the capture, which is the one thing in the project that cannot be redone.
    let project = Project::open_read_only(&args.project)
        .with_context(|| format!("opening {}", args.project.display()))?;
    let plan = splitter::plan(project.conn(), &request)?;

    if args.dry_run {
        if args.json {
            println!("{}", plan_json(&plan, None));
        } else {
            print_plan(&plan, &request);
            println!("  dry run    nothing was written");
        }
        return Ok(());
    }

    if !args.json {
        print_plan(&plan, &request);
    }

    // A line as each file starts rather than as it finishes: an export is long,
    // and the file being worked on is the useful thing to show. Printed on the
    // first update for an index, which is the first read of that item.
    let mut started = 0usize;
    let mut on = |progress: Progress<'_>| {
        if args.json || progress.index < started {
            return;
        }
        started = progress.index + 1;
        println!(
            "  {:>3}/{}    {}",
            progress.index + 1,
            progress.of,
            progress.item.path.display()
        );
    };
    let report = splitter::run(project.conn(), &plan, &mut on)?;
    project.close()?;

    if args.json {
        println!("{}", plan_json(&plan, Some(&report)));
        return Ok(());
    }

    println!(
        "  wrote      {} file(s), {} cover(s), {} frame(s), {:.1} MiB",
        report.files,
        report.covers,
        report.frames,
        report.bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}

/// Prints what is about to happen, or what would have.
fn print_plan(plan: &Plan, request: &Request) {
    println!("  into       {}", request.into.display());
    println!(
        "  format     {}, template {:?}",
        plan.container.name(),
        request.template
    );
    println!(
        "  tracks     {} file(s), {} frame(s)",
        plan.items.len(),
        plan.frames()
    );
    println!(
        "  artwork    {}",
        match (&plan.cover, request.artwork) {
            (None, _) => "none in this project".to_owned(),
            (Some(cover), policy) => format!(
                "{} byte(s) of {}, {}",
                cover.bytes.len(),
                cover.mime,
                match policy {
                    Artwork::None => "not exported",
                    Artwork::Embed => "embedded",
                    Artwork::Folder => "beside the files",
                    Artwork::Both => "embedded and beside the files",
                }
            ),
        }
    );
}

/// The plan, and the report where there is one, as JSON.
fn plan_json(plan: &Plan, report: Option<&Report>) -> serde_json::Value {
    serde_json::json!({
        "container": plan.container.extension(),
        "frames": plan.frames(),
        "covers": plan.covers.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "items": plan.items.iter().map(|item| serde_json::json!({
            "track_id": item.track_id,
            "side": item.side.letter().to_string(),
            "number": item.number,
            "capture_id": item.capture_id,
            "start_frame": item.span.start,
            "end_frame": item.span.end,
            "frames": item.frames(),
            "path": item.path.display().to_string(),
            "title": item.tags.title,
            "artist": item.tags.artist,
        })).collect::<Vec<_>>(),
        "report": report.map(|report| serde_json::json!({
            "files": report.files,
            "covers": report.covers,
            "frames": report.frames,
            "bytes": report.bytes,
        })),
    })
}

/// A container from what was typed.
fn container(given: &str) -> Result<Container> {
    Container::from_extension(given).ok_or_else(|| {
        anyhow::anyhow!(
            "{given:?} is not a container VCW writes yet - wav or flac. MP3 and Ogg are release 0.2"
        )
    })
}

/// An artwork policy from what was typed.
fn artwork(given: &str) -> Result<Artwork> {
    match given.to_ascii_lowercase().as_str() {
        "none" => Ok(Artwork::None),
        "embed" => Ok(Artwork::Embed),
        "folder" => Ok(Artwork::Folder),
        "both" => Ok(Artwork::Both),
        other => bail!("{other:?} is not an artwork policy - none, embed, folder or both"),
    }
}

/// A side letter, or a message naming what one is.
fn letter(given: char) -> Result<Side> {
    Side::from_letter(given)
        .ok_or_else(|| anyhow::anyhow!("{given:?} is not a side letter - A to Z, A being first"))
}
