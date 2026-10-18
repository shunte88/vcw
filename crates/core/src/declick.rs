/*
 *  declick.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Running the click detector over a committed capture and writing what it found.
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

//! The declick pass: a capture in, defect rows out (WP-30 §8 step 4).
//!
//! The same shape as [`crate::detection`] and for the same reasons - stream the
//! committed blocks a second at a time, run a pure detector from `vcw-signal` over
//! them, and write only what the detector concluded. The one difference is that
//! this one *writes*, so it needs a project opened for writing where boundary
//! refinement deliberately does not.
//!
//! It is re-runnable, which is a property of
//! [`vcw_project::defect::replace`] rather than of anything here: a second pass
//! removes the first pass's unlocked rows before writing its own, and leaves every
//! locked row exactly where it is. Running this twice over an unchanged capture
//! with unchanged settings writes the same list twice.
//!
//! **A run reports how much of the side it flagged.** The survey's section 4 is
//! about a published parameter set that marks five to seven percent of all samples,
//! and the lesson taken from it was that a defect rate is a reportable number and
//! not an internal one. [`Found::flagged_fraction`] is that number, and it is
//! printed before anybody is asked to approve anything.
//!
//! Requirements: §4.1 (immutable PCM), §23, §24, §46 Phase 3.

use std::path::Path;
use std::time::{Duration, Instant};

use vcw_project::defect::{self, NewDefect};
use vcw_project::pcm::Reader;
use vcw_project::{Layout, Project, Result as ProjectResult};
use vcw_signal::declick::{Config, Damage, Detector, Diagnostics};
use vcw_types::{Evidence, Provenance, Repair, Span};

/// What a detector was asked to find, beyond its own five parameters.
///
/// Separate from [`Config`] because these two are about the *list*, not about the
/// signal: what to propose doing with each span, and what to call the damage. A
/// detector that only ever wrote `click` and `interpolate` would be the same code
/// with the choice hidden inside it, and the schema is deliberate that a repair is
/// stated by whoever writes the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    /// What to call the damage: the `kind` column. Free text by design.
    pub kind: String,
    /// What to propose doing about each span.
    pub repair: Repair,
}

impl Default for Intent {
    fn default() -> Self {
        Self {
            kind: "click".to_owned(),
            repair: Repair::Interpolate,
        }
    }
}

/// What one pass found and what it did with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// The spans, in the order the detector emitted them.
    pub spans: Vec<Damage>,
    /// What the detector saw on the way.
    pub diagnostics: Diagnostics,
    /// Rows the previous pass had left behind, now gone.
    pub removed: usize,
    /// Rows written or refreshed. On a [`preview`] this is what *would* be
    /// written, which is the whole point of a dry run: a count that is not the
    /// count the real run produces is a prediction of nothing.
    pub written: usize,
    /// Whether the rows were actually written.
    pub recorded: bool,
    /// How long the pass took.
    pub took: Duration,
}

impl Found {
    /// Marked samples over examined samples, in `0.0..=1.0`.
    ///
    /// The number section 4 of the survey is about. Oudre's published defaults mark
    /// five to seven percent of every sample on the material they were tuned on,
    /// which at 44.1 kHz is two thousand samples a second; a run that reports
    /// anything near that has found a detector setting, not a record.
    #[must_use]
    pub fn flagged_fraction(&self) -> f64 {
        self.diagnostics.flagged_fraction()
    }

    /// Total damaged frames, counted once per channel.
    #[must_use]
    pub fn flagged_frames(&self) -> u64 {
        self.spans.iter().map(Damage::frames).sum()
    }
}

/// Runs the detector over one capture and writes what it found.
///
/// # Errors
///
/// If the capture is not in this project, its audio cannot be read, or the rows
/// cannot be written. A failed write leaves the previous pass's answer intact.
pub fn scan(
    project: &mut Project,
    capture_id: i64,
    cfg: &Config,
    intent: &Intent,
) -> ProjectResult<Found> {
    run(project, capture_id, cfg, intent, true)
}

/// The same pass, writing nothing.
///
/// A dry run is a prediction, so it is the same code and the same report: the
/// spans, the diagnostics and the row count are what a [`scan`] of the same
/// capture with the same settings produces, and only [`Found::recorded`] differs.
/// Opening the project read-only is what makes that a promise rather than an
/// intention.
///
/// # Errors
///
/// If the capture is not in this project, or its audio cannot be read.
pub fn preview(
    project: &mut Project,
    capture_id: i64,
    cfg: &Config,
    intent: &Intent,
) -> ProjectResult<Found> {
    run(project, capture_id, cfg, intent, false)
}

fn run(
    project: &mut Project,
    capture_id: i64,
    cfg: &Config,
    intent: &Intent,
    record: bool,
) -> ProjectResult<Found> {
    let began = Instant::now();
    let layout = Layout::of(project.conn(), capture_id)?;
    let mut detector = Detector::new(
        layout.rate.hz(),
        layout.channels as usize,
        layout.format,
        *cfg,
    );
    {
        let mut reader = Reader::open(project.conn(), capture_id, Span::whole(layout.frames))?;
        // A second of audio per read, the size `detection::refine` settled on: big
        // enough that the per-call overhead disappears against the arithmetic, small
        // enough that a forty-minute side is never held in memory twice over.
        let mut buffer = vec![0u8; (layout.frame_bytes() * layout.rate.hz() as usize).max(4_096)];
        loop {
            let read = reader.fill(&mut buffer)?;
            if read == 0 {
                break;
            }
            detector.push(&buffer[..read]);
        }
    }
    let (spans, diagnostics) = detector.finish();
    let rows: Vec<NewDefect> = spans.iter().map(|span| row(span, intent)).collect();
    let (removed, written) = if record {
        defect::replace(project, capture_id, Provenance::Declick, &rows)?
    } else {
        (0, rows.len())
    };
    Ok(Found {
        spans,
        diagnostics,
        removed,
        written,
        recorded: record,
        took: began.elapsed(),
    })
}

/// Opens a project and runs [`scan`] over one of its captures.
///
/// For writing, unlike [`crate::detection::refine_project`]: this pass has a
/// conclusion to record, so a side cannot be declicked while another one is being
/// captured into the same file.
///
/// # Errors
///
/// If the project cannot be opened, or [`scan`] fails.
pub fn scan_project(
    path: &Path,
    capture_id: i64,
    cfg: &Config,
    intent: &Intent,
) -> ProjectResult<Found> {
    let mut project = Project::open(path)?;
    let found = scan(&mut project, capture_id, cfg, intent)?;
    project.close()?;
    Ok(found)
}

/// Opens a project read-only and runs [`preview`] over one of its captures.
///
/// Read-only on purpose: a dry run that holds a writable handle is a dry run that
/// could have written, and a side can be examined while another one is recording.
///
/// # Errors
///
/// If the project cannot be opened, or [`preview`] fails.
pub fn preview_project(
    path: &Path,
    capture_id: i64,
    cfg: &Config,
    intent: &Intent,
) -> ProjectResult<Found> {
    let mut project = Project::open_read_only(path)?;
    let found = preview(&mut project, capture_id, cfg, intent)?;
    project.close()?;
    Ok(found)
}

/// One span as a row.
///
/// The evidence is the margin the span was found on and how long it is - the two
/// numbers a person asking "why is this a click" wants, and the two that make a
/// threshold change explicable after the fact rather than only before it.
fn row(span: &Damage, intent: &Intent) -> NewDefect {
    NewDefect {
        start_frame: span.start,
        end_frame: span.end,
        channels: 1u32 << span.channel.min(31),
        kind: intent.kind.clone(),
        confidence: span.confidence,
        provenance: Provenance::Declick,
        evidence: vec![
            Evidence::new("peak-ratio", span.peak_ratio),
            #[expect(
                clippy::cast_precision_loss,
                reason = "a span longer than 2^53 frames is not a click"
            )]
            Evidence::new("length-frames", span.frames() as f64),
        ],
        repair: intent.repair,
        locked: false,
    }
}
