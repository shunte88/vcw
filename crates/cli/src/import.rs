/*
 *  import.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `vcw import` verb: read an Audacity project, write a VCW one.
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
//! The `vcw import` verb: read an Audacity project, write a VCW one (§12).
//!
//! Two modes, and the default is to do the work. That is the opposite of
//! `vcw recover`, deliberately: recovery rewrites a project that already
//! exists, so it reports first and writes when told to, whereas an import
//! creates a new file and destroys nothing. The destination is refused if it
//! already exists and there is no flag to override that - a `.vcw` is somebody's
//! work, and an import is never worth losing it for.
//!
//! `--dry-run` is worth having anyway, because an import is a long operation on
//! a large file: it reads the document, cross-checks it against the audio table
//! and prints what *would* land, in about a second, without writing a byte.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use vcw_import::land::Options;
use vcw_import::{Landed, Timeline, audit, model, read, sniff};
use vcw_types::vinyl::Side;

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// The Audacity project to read.
    pub(crate) source: PathBuf,
    /// Where to write the `.vcw`. Defaults to the source with the extension
    /// changed.
    pub(crate) output: Option<PathBuf>,
    /// Which side the capture is. Nothing in an Audacity project says.
    pub(crate) side: char,
    /// Report what would land, and write nothing.
    pub(crate) dry_run: bool,
    /// Do not turn labels into tracks.
    pub(crate) no_labels: bool,
    /// Do not copy the tags onto the release.
    pub(crate) no_tags: bool,
    /// Blocks per transaction. Defaults to [`vcw_import::land::BATCH_BLOCKS`].
    pub(crate) batch_blocks: usize,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.source.exists() {
        bail!("{} does not exist", args.source.display());
    }
    let side = Side::from_letter(args.side.to_ascii_uppercase())
        .with_context(|| format!("'{}' is not a side letter", args.side))?;
    let destination = args
        .output
        .clone()
        .unwrap_or_else(|| args.source.with_extension(vcw_project::EXTENSION));
    if !args.dry_run && destination.exists() {
        bail!(
            "{} already exists. Import writes a new project and will not overwrite one.",
            destination.display()
        );
    }

    // Every field named, which clippy insists on now that there are only four
    // of them. The base is still `Options::default()`: the batch size is the
    // import default unless the flag says otherwise, and zero is not refused
    // here because the writer reads it as one - the slowest possible import,
    // which is a thing somebody reproducing a timing might want.
    let options = Options {
        side,
        labels: !args.no_labels,
        tags: !args.no_tags,
        config: vcw_project::persistence::Config {
            batch_blocks: args.batch_blocks,
            ..Options::default().config
        },
    };

    if args.dry_run {
        return survey(args, &destination, &options);
    }

    let landed = vcw_import::land(&args.source, &destination, &options)
        .with_context(|| format!("importing {}", args.source.display()))?;
    if args.json {
        print_json(&landed, &args.source);
    } else {
        print_human(&landed, &args.source);
    }
    Ok(())
}

/// Reads the project and reports what would land, writing nothing.
///
/// The audit runs here as well as in the landing: a dangling block reference is
/// the one thing worth knowing about a 271 MB project *before* spending five
/// minutes copying it.
fn survey(args: &Args, destination: &Path, options: &Options) -> Result<()> {
    let (conn, sniffed) = sniff::open(&args.source)?;
    let surveyed = read::survey(&conn, &args.source)?;
    let document = model::Project::from_events(&surveyed.document.events)?;
    let audited = audit(&conn, &document)?;
    let timeline = Timeline::plan(&conn, &document)?;

    let labels: usize = document
        .label_tracks
        .iter()
        .map(|list| list.labels.len())
        .sum();
    let seconds = if timeline.rate().hz() == 0 {
        0.0
    } else {
        timeline.frames() as f64 / f64::from(timeline.rate().hz())
    };
    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "source": args.source.display().to_string(),
                "destination": destination.display().to_string(),
                "dry_run": true,
                "format": sniffed.version.as_str(),
                "audacity_version": document.audacity_version,
                "rate": timeline.rate().hz(),
                "editor_rate_preference": document.editor_rate_preference,
                "channels": timeline.channels(),
                "storage_format": format!("{:?}", timeline.storage_format()),
                "frames": timeline.frames(),
                "seconds": seconds,
                "clips": document.tracks.iter().map(|t| t.clips.len()).sum::<usize>(),
                "labels": labels,
                "tags": document.tags.len(),
                "block_refs": audited.refs,
                "blocks": audited.distinct,
                "orphan_blocks": audited.orphans.len(),
                "side": options.side.letter().to_string(),
            })
        );
        return Ok(());
    }

    println!("{}", args.source.display());
    println!(
        "  format      {} written by Audacity {}",
        sniffed.version.as_str(),
        document
            .audacity_version
            .as_deref()
            .unwrap_or("of an unrecorded version")
    );
    println!(
        "  audio       {} Hz, {} ch, {:?}",
        timeline.rate().hz(),
        timeline.channels(),
        timeline.storage_format()
    );
    // Printed together, always, because they disagree in 22 of the 25 corpus
    // projects and the wrong one of the two plays a rip at four times speed.
    if let Some(preference) = document.editor_rate_preference
        && (preference - f64::from(timeline.rate().hz())).abs() > f64::EPSILON
    {
        println!(
            "              project/@rate says {preference} Hz; that is an editor \
             preference and is ignored"
        );
    }
    println!(
        "  timeline    {} frames, {:.3} s, from {} clip(s)",
        timeline.frames(),
        seconds,
        document.tracks.iter().map(|t| t.clips.len()).sum::<usize>()
    );
    println!(
        "  blocks      {} reference(s) to {} block(s){}",
        audited.refs,
        audited.distinct,
        match audited.most_shared {
            Some((blockid, count)) if count > 1 =>
                format!(", one of them ({blockid}) used {count} times"),
            _ => String::new(),
        }
    );
    if !audited.orphans.is_empty() {
        println!(
            "              {} stored block(s) no clip refers to; they are not imported",
            audited.orphans.len()
        );
    }
    println!("  labels      {labels}");
    for (name, value) in &document.tags {
        println!("  tag         {name} = {value}");
    }
    println!(
        "  would write {} as side {}",
        destination.display(),
        options.side.letter()
    );
    Ok(())
}

fn print_human(landed: &Landed, source: &Path) {
    println!("{}", landed.path.display());
    println!(
        "  capture {:<3} {} frames on every channel, {:.3} s, {} block(s)",
        landed.capture_id,
        landed.frames,
        landed.seconds(),
        landed.blocks
    );
    println!(
        "              written in {} transaction(s) of up to {} block(s)",
        landed.commits, landed.batch_blocks
    );
    println!(
        "              {} Hz, {} ch, {:?}, imported from {} clip(s)",
        landed.rate.hz(),
        landed.channels,
        landed.storage_format,
        landed.clips
    );
    println!(
        "  side {}      attached to capture {}",
        landed.side.letter(),
        landed.capture_id
    );
    println!("  tracks      {} from label(s)", landed.tracks);
    for (title, why) in &landed.labels_skipped {
        println!("  !           '{title}' was not made a track: {why}");
    }
    println!("  tags        {} recorded", landed.tags);
    if landed.envelopes_dropped > 0 {
        println!(
            "  !           {} clip(s) carry a volume envelope, which is editor \
             state and is not imported",
            landed.envelopes_dropped
        );
    }
    println!(
        "  source      {} ({})",
        source.display(),
        landed.version.as_str()
    );
}

fn print_json(landed: &Landed, source: &Path) {
    println!(
        "{}",
        serde_json::json!({
            "project": landed.path.display().to_string(),
            "source": source.display().to_string(),
            "format": landed.version.as_str(),
            "capture_id": landed.capture_id,
            "side": landed.side.letter().to_string(),
            "rate": landed.rate.hz(),
            "channels": landed.channels,
            "storage_format": format!("{:?}", landed.storage_format),
            "frames": landed.frames,
            "seconds": landed.seconds(),
            "blocks": landed.blocks,
            "commits": landed.commits,
            "batch_blocks": landed.batch_blocks,
            "clips": landed.clips,
            "tracks": landed.tracks,
            "labels_skipped": landed.labels_skipped.iter()
                .map(|(title, why)| serde_json::json!({"title": title, "why": why}))
                .collect::<Vec<_>>(),
            "tags": landed.tags,
            "envelopes_dropped": landed.envelopes_dropped,
        })
    );
}
