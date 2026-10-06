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

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use vcw_export::encoder::{Compression, Container, Dither, Narrowing, Quality, Width};
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
    /// How hard a lossy container compresses. Ignored by WAV and FLAC.
    pub(crate) quality: String,
    /// How hard FLAC compresses, 0 to 8. Ignored by the other three.
    pub(crate) compression: String,
    /// Naming template, or `None` for the default.
    pub(crate) template: Option<String>,
    /// Sides to export. Empty exports every side that has tracks.
    pub(crate) sides: Vec<char>,
    /// What to do with the release's front cover.
    pub(crate) artwork: String,
    /// What a float capture is rounded to: `refuse`, `24` or `32`.
    pub(crate) narrow: String,
    /// The noise added before rounding: `tpdf` or `none`.
    pub(crate) dither: String,
    /// Decibels of room left above full scale before rounding.
    pub(crate) headroom: String,
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
    let container = container(&args.format, &args.quality, &args.compression)?;
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
        narrowing: narrowing(&args.narrow, &args.dither, &args.headroom)?,
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
            print_paths(&plan, &request);
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
    //
    // Relative to `--into`, which is the line `print_plan` has just printed, so
    // these lines are the same text `--dry-run` showed for the same request -
    // the dry run is then a prediction a person can hold up against the run and
    // compare line for line, rather than a differently-formatted summary.
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
            under(&progress.item.path, &request.into).display()
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
    // `Display` rather than `name`, so a lossy container prints the codec's own
    // spelling of its quality - `MP3 V2` - and a dry run says exactly what a
    // bitrate reader will say afterwards.
    println!(
        "  format     {}, template {:?}",
        plan.container, request.template
    );
    println!(
        "  tracks     {} file(s), {} frame(s)",
        plan.items.len(),
        plan.frames()
    );
    // The one line that says the exported files will not sound like the
    // project does. A fold is invisible afterwards - the file is simply mono -
    // so it has to be visible before, and a dry run is where someone looks.
    if plan.fold_to_mono {
        println!(
            "  channels   summed to mono at -6 dB per channel (the release says mono; \
             the capture is unchanged)"
        );
    }
    // The second line of the same kind, and for the same reason: a file rounded
    // from float to fixed point is not the project's samples any more, and
    // afterwards there is nothing in the file to say so. Printed only when a
    // file really will be rounded - `Plan::narrowed_to` is the difference
    // between what was asked for and what will happen.
    if plan.narrowed_to.is_some() {
        println!(
            "  samples    32-bit float rounded to {}-bit integer{}{} (the capture is \
             unchanged)",
            request.narrowing.to,
            match request.narrowing.dither {
                Dither::None => ", no dither",
                Dither::Tpdf => ", triangular dither",
            },
            if request.narrowing.headroom_db == 0.0 {
                String::new()
            } else {
                format!(", {} dB of headroom", request.narrowing.headroom_db)
            }
        );
    }
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

/// Every path the plan resolved, relative to `--into`.
///
/// `--dry-run` is how to argue with a naming template without producing a
/// gigabyte of files to delete, and until this was here it printed the counts
/// and none of the names: the only way to find out what a template had done was
/// to run a real export and look at the directory afterwards. `--json` carried
/// them all along, which is no help to a person at a terminal.
///
/// Relative rather than absolute because the relative part *is* what the
/// template produced, and `--into` is on the line above. The gutter is the one a
/// real run's progress lines use, so a dry run and the run it predicts read the
/// same down the page.
fn print_paths(plan: &Plan, request: &Request) {
    let of = plan.items.len();
    for (index, item) in plan.items.iter().enumerate() {
        println!(
            "  {:>3}/{of}    {}",
            index + 1,
            under(&item.path, &request.into).display()
        );
    }
    // The covers are planned paths too, and the `artwork` line above says the
    // policy without saying where: one `folder.png` per directory is a fact
    // about the layout that a template argument is usually about.
    for cover in &plan.covers {
        println!("  cover      {}", under(cover, &request.into).display());
    }
}

/// `path` with `into` taken off the front, or whole if it is not under it.
fn under<'a>(path: &'a Path, into: &Path) -> &'a Path {
    path.strip_prefix(into).unwrap_or(path)
}

/// The plan, and the report where there is one, as JSON.
fn plan_json(plan: &Plan, report: Option<&Report>) -> serde_json::Value {
    serde_json::json!({
        "container": plan.container.extension(),
        // `null` for a lossless container rather than a missing key, so that a
        // script can read `.quality` on every report and get an answer. The
        // answer "this container has no such setting" is `null`, which is not
        // the same as "high" and not the same as a typo.
        "quality": plan.container.quality().map(|quality| quality.name()),
        // `null` on the three containers with no level, for the reason above.
        "compression": plan.container.compression().map(|level| level.level()),
        "lossless": !plan.container.is_lossy(),
        "fold_to_mono": plan.fold_to_mono,
        // What will happen, not what was asked for: `null` whenever no file is
        // rounded, which is every export of an integer capture whatever
        // `--narrow` said. The three knobs are reported beside it so a script
        // can record how a float master was brought down, which is the one
        // thing about an exported file that cannot be recovered from it.
        "narrowed_to": plan.narrowed_to.map(|_| plan.narrowing.to.name()),
        "dither": plan.narrowed_to.map(|_| plan.narrowing.dither.name()),
        "headroom_db": plan.narrowed_to.map(|_| plan.narrowing.headroom_db),
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

/// A container from what was typed, at the quality that was typed.
///
/// The quality is applied whatever the container is. `with_quality` is a no-op
/// on WAV and FLAC, so `--format flac --quality compact` is not an error: the
/// defaults put a quality on every invocation, and a flag that only becomes
/// legal once another flag changes is a flag people trip over.
fn container(given: &str, quality: &str, compression: &str) -> Result<Container> {
    let container = Container::from_extension(given).ok_or_else(|| {
        anyhow::anyhow!(
            "{given:?} is not a container VCW writes - {}",
            Container::spellings()
        )
    })?;
    let quality = Quality::parse(quality).ok_or_else(|| {
        anyhow::anyhow!("{quality:?} is not a quality - transparent, high or compact")
    })?;
    let compression = Compression::parse(compression).ok_or_else(|| {
        anyhow::anyhow!(
            "{compression:?} is not a FLAC compression level - a number from 0 to 8, \
             where 5 is the default"
        )
    })?;
    Ok(container
        .with_quality(quality)
        .with_compression(compression))
}

/// The three float-narrowing answers from what was typed.
///
/// Parsed even when the format is WAV and even when the capture is already
/// integers, for `container`'s reason: the defaults put all three on every
/// invocation, and a flag that is only checked when it applies is a flag whose
/// typos are discovered by the one person whose capture it applies to.
fn narrowing(width: &str, dither: &str, headroom: &str) -> Result<Narrowing> {
    Ok(Narrowing {
        to: Width::parse(width)
            .ok_or_else(|| anyhow::anyhow!("{width:?} is not a narrowing - refuse, 24 or 32"))?,
        dither: Dither::parse(dither)
            .ok_or_else(|| anyhow::anyhow!("{dither:?} is not a dither - tpdf or none"))?,
        headroom_db: Narrowing::parse_headroom(headroom).ok_or_else(|| {
            anyhow::anyhow!(
                "{headroom:?} is not a headroom - a number of decibels from 0 to {}",
                Narrowing::MAX_HEADROOM_DB
            )
        })?,
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
