/*
 *  fingerprint.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The 'vcw fingerprint' verb: fingerprint a span, or the implied tracks (25).
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

//! The `vcw fingerprint` verb: fingerprint a span, or the implied tracks (§25).
//!
//! §4.5 asks for the whole workflow to be drivable with no UI, and this is the offline
//! twin of what the capture does for itself: the live worker fingerprints the regions
//! the detector announces while the record turns, and this fingerprints any span of a
//! capture that has already been committed. The answers are the same - that is the
//! property `crates/core/tests/fingerprint_live.rs` asserts - so this is also how to
//! see what the live pass would have produced, on a side that was recorded before
//! there was a fingerprint worker at all.
//!
//! # The question, and optionally the answer
//!
//! Without `--identify` nothing here looks anything up, which is the default
//! deliberately: §40 says a network request is something a person asks for. With
//! it, every region that fingerprinted is sent to AcoustID and what came back is
//! printed under it - which is WP-22, and the only way to see this path run on a
//! real side rather than on a recorded fixture.
//!
//! The lookup is read-only. Nothing is written to the project: deciding which of
//! several candidate recordings a track *is* belongs to the evidence resolver, and
//! a verb that printed one answer and saved it would be making that decision by
//! being run.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vcw_core::detection;
use vcw_core::fingerprinting;
use vcw_fingerprint::chromaprint::{ALGORITHM, Fingerprint};
use vcw_metadata::acoustid::Match;
use vcw_metadata::credentials::Credentials;
use vcw_metadata::query::Fingerprint as Evidence;
use vcw_metadata::{Cancel, Setup};
use vcw_project::pcm::Layout;
use vcw_project::{Project, session};
use vcw_signal::regions::Config;
use vcw_types::{Edge, Span};

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to read.
    pub(crate) project: PathBuf,
    /// Capture to fingerprint. `None` takes the most recent.
    pub(crate) capture: Option<i64>,
    /// Where to start, in seconds. `None` starts at the beginning.
    pub(crate) from: Option<f64>,
    /// How much to fingerprint, in seconds. `None` runs to the end.
    pub(crate) length: Option<f64>,
    /// Fingerprint each track the detectors imply, rather than one span.
    pub(crate) tracks: bool,
    /// In `--tracks`, report only boundaries this many detectors reported.
    pub(crate) min_sources: usize,
    /// Machine-readable output.
    pub(crate) json: bool,
    /// Look each fingerprint up at AcoustID (§26).
    pub(crate) identify: bool,
}

/// One region, fingerprinted or refused.
struct Region {
    span: Span,
    outcome: Result<Fingerprint, fingerprinting::Error>,
    /// What AcoustID said, or why it could not be asked. `None` without
    /// `--identify`, which is not the same as an empty list: no question asked is
    /// not the same answer as "nobody has submitted this record".
    found: Option<Result<Vec<Match>, vcw_metadata::Error>>,
}

/// Seconds to a frame, clamped to the capture.
fn frame_at(seconds: f64, rate: f64, frames: u64) -> u64 {
    if seconds <= 0.0 {
        return 0;
    }
    ((seconds * rate).round() as u64).min(frames)
}

/// The spans the flags asked for: one, or one per implied track.
///
/// The pairing is in frames rather than in seconds, because a span is what the reader
/// takes and rounding a boundary through seconds and back would move it.
///
/// A start closes whatever was still open, which is the live worker's rule in
/// `vcw_core::fingerprinting` and not `vcw detect`'s: two starts in a row are the
/// ordinary shape of a side whose between-track groove was too quiet to call an end,
/// and leaving the first one open would fingerprint the rest of the side under the
/// first track's name. Driving this on a real ten-minute side produced exactly that -
/// a region 1 of 0 s to 600 s overlapping all thirteen after it. An unclosed *last*
/// start does run to the end, which is the ordinary shape of the last track.
fn spans(args: &Args, capture_id: i64, layout: &Layout) -> Result<Vec<Span>> {
    let rate = f64::from(layout.rate.hz()).max(1.0);
    if !args.tracks {
        let start = frame_at(args.from.unwrap_or(0.0), rate, layout.frames);
        let end = match args.length {
            Some(secs) => frame_at(args.from.unwrap_or(0.0) + secs, rate, layout.frames),
            None => layout.frames,
        };
        if end <= start {
            bail!("that span is empty: {start} to {end} frames");
        }
        return Ok(vec![Span::new(start, end)]);
    }

    let refined = detection::refine_project(&args.project, capture_id, &Config::new(), &[])?;
    let mut out: Vec<Span> = Vec::new();
    for decision in &refined.decisions {
        if decision.agreement() < args.min_sources.max(1) {
            continue;
        }
        let at = decision.at.min(layout.frames);
        // Open means "still running to the end of the side", which is how a start
        // leaves it and what either edge then closes.
        if let Some(last) = out.last_mut()
            && last.end == layout.frames
        {
            last.end = at.clamp(last.start, layout.frames);
        }
        if decision.edge == Edge::Start {
            out.push(Span::new(at, layout.frames));
        }
    }
    out.retain(|span| span.end > span.start);
    Ok(out)
}

impl Args {
    /// The capture this run is about: the one named, or the most recent.
    fn capture_id(&self) -> Result<i64> {
        let project = Project::open_read_only(&self.project)
            .with_context(|| format!("opening {}", self.project.display()))?;
        let id = match self.capture {
            Some(id) => id,
            None => match session::all(project.conn())?.last() {
                Some(record) => record.id,
                None => bail!("{} has no captures", self.project.display()),
            },
        };
        project.close()?;
        Ok(id)
    }
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }
    let capture_id = args.capture_id()?;
    // Read-only, for the reason analysis always is: a side can be examined while
    // another one is being recorded.
    let project = Project::open_read_only(&args.project)
        .with_context(|| format!("opening {}", args.project.display()))?;
    let layout = Layout::of(project.conn(), capture_id)?;
    let wanted = spans(args, capture_id, &layout)?;
    if wanted.is_empty() {
        bail!(
            "no track boundaries were agreed by {} detectors",
            args.min_sources.max(1)
        );
    }
    let mut regions: Vec<Region> = wanted
        .into_iter()
        .map(|span| Region {
            span,
            // A refusal is per region and not fatal: a two-second run-out groove
            // between two tracks has nothing to fingerprint, and the tracks either
            // side of it still do.
            outcome: fingerprinting::of_span(&project, capture_id, span),
            found: None,
        })
        .collect();
    project.close()?;
    if args.identify {
        identify(&mut regions);
    }

    if args.json {
        print_json(capture_id, &layout, &regions);
    } else {
        print_report(capture_id, &layout, &regions);
    }
    Ok(())
}

/// Asks AcoustID about every region that fingerprinted.
///
/// One provider for the whole run, so the rate limiter is the same one throughout:
/// a thirteen-track side is thirteen requests at a request a second, and thirteen
/// providers would be thirteen limiters that each think they are first.
///
/// A failure is recorded against the region rather than ending the run. The one
/// that would otherwise waste a person's time is a bad key - thirteen identical
/// refusals a second apart - so a rejected credential stops the loop and the
/// regions after it say so.
fn identify(regions: &mut [Region]) {
    let credentials = Credentials::from_env();
    let provider = Setup::new()
        .online(true)
        .with_timeout(TIMEOUT)
        .acoustid(&credentials);
    let cancel = Cancel::new();
    let mut refused = None;
    for region in regions.iter_mut() {
        let Ok(fingerprint) = &region.outcome else {
            continue;
        };
        if let Some(error) = &refused {
            region.found = Some(Err(vcw_metadata::Error::Rejected { provider: *error }));
            continue;
        }
        let evidence = Evidence::new(&fingerprint.encoded, seconds_of(fingerprint));
        let answer = provider.lookup(&evidence, &cancel);
        if let Err(vcw_metadata::Error::Rejected { provider }) = &answer {
            refused = Some(*provider);
        }
        region.found = Some(answer);
    }
}

/// A fingerprint's length in whole seconds, which is what a lookup sends.
fn seconds_of(fingerprint: &Fingerprint) -> u32 {
    fingerprint.seconds().round().max(0.0) as u32
}

/// How long to wait for one lookup.
///
/// Longer than the library default for the same reason `vcw metadata` is: a person
/// who typed a command is waiting on purpose.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The first of a fingerprint, which is all a person reads.
fn head(encoded: &str) -> String {
    let shown: String = encoded.chars().take(48).collect();
    if encoded.len() > shown.len() {
        format!("{shown}... ({} chars)", encoded.len())
    } else {
        shown
    }
}

fn print_report(capture_id: i64, layout: &Layout, regions: &[Region]) {
    let rate = f64::from(layout.rate.hz()).max(1.0);
    println!(
        "  capture    {capture_id}, {} Hz, {} ch, {:.3} s",
        layout.rate.hz(),
        layout.channels,
        layout.frames as f64 / rate
    );
    println!("  algorithm  {ALGORITHM:?}, chromaprint-next (LGPL-2.1-or-later)");
    let mut done = 0;
    for (index, region) in regions.iter().enumerate() {
        println!(
            "  region {:<3} {:.3} s .. {:.3} s",
            index + 1,
            region.span.start as f64 / rate,
            region.span.end as f64 / rate
        );
        match &region.outcome {
            Ok(fingerprint) => {
                done += 1;
                println!(
                    "             {} items, hash {:#010x}",
                    fingerprint.raw.len(),
                    fingerprint.hash()
                );
                println!("             {}", head(&fingerprint.encoded));
            }
            Err(error) => println!("             refused: {error}"),
        }
        print_matches(region);
    }
    println!(
        "  total      {done} fingerprinted, {} refused, nothing written",
        regions.len() - done
    );
}

/// What AcoustID said about one region, under the fingerprint it answered.
///
/// A vinyl transfer frequently matches nothing at all - AcoustID's index is
/// submitted from digital releases, and a record is a different master at a
/// slightly different speed - so "no match" is printed rather than left blank. A
/// person reading this needs to be able to tell a question with no answer from a
/// question that was never asked.
fn print_matches(region: &Region) {
    let Some(found) = &region.found else {
        return;
    };
    match found {
        Ok(matches) if matches.is_empty() => println!("             no match at AcoustID"),
        Ok(matches) => {
            for found in matches {
                let recording = &found.recording;
                println!(
                    "             {:.2}  {} - {}{}",
                    found.score,
                    if recording.artist.is_empty() {
                        "unknown artist"
                    } else {
                        &recording.artist
                    },
                    if recording.title.is_empty() {
                        "untitled"
                    } else {
                        &recording.title
                    },
                    match recording.seconds() {
                        Some(seconds) => format!(" ({seconds:.1} s)"),
                        None => String::new(),
                    }
                );
                // Records only, and the count of everything else: the pressing a
                // person is holding is a record, and thirteen CD reissues under it
                // would bury the two that matter.
                let records: Vec<&_> = recording.releases.iter().filter(|r| r.is_vinyl()).collect();
                for release in &records {
                    println!(
                        "                     {} [{}]{}",
                        release.title,
                        release.format,
                        match release.position {
                            Some(position) => format!(" track {position}"),
                            None => String::new(),
                        }
                    );
                }
                let others = recording.releases.len() - records.len();
                if others > 0 {
                    println!("                     and {others} release(s) that are not records");
                }
            }
        }
        Err(error) => println!("             not identified: {error}"),
    }
}

fn print_json(capture_id: i64, layout: &Layout, regions: &[Region]) {
    let rate = f64::from(layout.rate.hz()).max(1.0);
    let value = serde_json::json!({
        "capture_id": capture_id,
        "rate": layout.rate.hz(),
        "channels": layout.channels,
        "frames": layout.frames,
        "algorithm": format!("{ALGORITHM:?}"),
        "regions": regions.iter().map(|region| serde_json::json!({
            "from_frame": region.span.start,
            "to_frame": region.span.end,
            "from_seconds": region.span.start as f64 / rate,
            "seconds": region.span.frames() as f64 / rate,
            "fingerprint": region.outcome.as_ref().ok().map(|fingerprint| serde_json::json!({
                "items": fingerprint.raw.len(),
                "hash": fingerprint.hash(),
                "encoded": fingerprint.encoded,
                "duration": fingerprint.seconds(),
            })),
            "refused": region.outcome.as_ref().err().map(ToString::to_string),
            // Absent without `--identify`, and `[]` for a question that was asked
            // and came back empty. The two are different facts about the record.
            "matches": region.found.as_ref().and_then(|found| found.as_ref().ok())
                .map(|matches| matches.iter().map(|found| serde_json::json!({
                    "score": found.score,
                    "recording_id": found.recording.id,
                    "title": found.recording.title,
                    "artist": found.recording.artist,
                    "seconds": found.recording.seconds(),
                    "releases": found.recording.releases.iter().map(|release| serde_json::json!({
                        "release_id": release.id,
                        "title": release.title,
                        "format": release.format,
                        "vinyl": release.is_vinyl(),
                        "medium": release.medium,
                        "position": release.position,
                        "track_count": release.track_count,
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>()),
            "not_identified": region.found.as_ref()
                .and_then(|found| found.as_ref().err()).map(ToString::to_string),
        })).collect::<Vec<_>>(),
    });
    println!("{value:#}");
}
