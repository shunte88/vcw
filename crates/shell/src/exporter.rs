/*
 *  exporter.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The export commands: plan first, then write, on a thread of its own (§33).
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

//! The export commands: plan first, then write, on a thread of its own (§33).
//!
//! Two commands, which is WP-14's shape and not this module's invention. The
//! plan resolves every filename and reports the collisions before a byte is
//! written, so a template can be argued with at no cost; the run then writes
//! what the plan said. A UI shows the plan as a list and the run as a progress
//! bar, which is exactly why the split is here rather than hidden behind one
//! button.
//!
//! The run is on a thread because exporting a two-hour side takes minutes and a
//! synchronous Tauri command runs on the main thread, which is the one drawing
//! the window. So it returns as soon as the thread starts, and the frontend
//! learns what happened from `export-progress`, then exactly one of
//! `export-finished` or `export-failed`.
//!
//! These three events are emitted straight to the webview rather than published
//! on the bus. That is where the exporter's information is: it reports through
//! a callback, and a bus event would be a second hop for no gain.

use std::path::Path;

use vcw_contract::command::Export;
use vcw_contract::event::Wire;
use vcw_contract::view::ExportPlan;
use vcw_export::splitter::{self, Progress};
use vcw_project::Project;

use crate::host::Hosted;
use crate::state::{Error, Shell};

/// Resolves the plan and returns it, writing nothing. §33's dry run.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] for a container
/// or an artwork policy that is not one of the accepted words, and
/// [`Error::Export`] for a plan that cannot be resolved - two tracks naming the
/// same file, a side with no tracks, a directory that cannot be created.
pub fn export_plan(shell: &Shell, export: Export) -> Result<ExportPlan, Error> {
    let path = shell.project_path()?;
    let request = export.request()?;
    let project = Project::open_read_only(&path)?;
    let plan = splitter::plan(project.conn(), &request)?;
    project.close()?;
    Ok(ExportPlan::of(&plan))
}

/// Writes the export. §35's `export`.
///
/// Returns as soon as the thread is running. Everything after that is events.
///
/// # Errors
///
/// As [`export_plan`]: the plan is resolved on this thread, so a template that
/// will not resolve is refused before anything starts. Once the thread is
/// running, failures arrive as `export-failed`.
pub fn export_run(shell: &Shell, host: &Hosted, export: Export) -> Result<(), Error> {
    let path = shell.project_path()?;
    let request = export.request()?;

    // Planned here rather than on the thread, so that a bad template is a
    // refused command with a field name on it instead of an event a second
    // later. It is planned again on the thread because `run` takes a plan and
    // a connection that belong to the same thread.
    {
        let project = Project::open_read_only(&path)?;
        let plan = splitter::plan(project.conn(), &request)?;
        project.close()?;
        if plan.items.is_empty() {
            return Err(Error::Invalid {
                field: "sides".to_owned(),
                why: "there are no tracks to export - detect or add some first".to_owned(),
            });
        }
    }

    let host = host.clone();
    std::thread::Builder::new()
        .name("vcw-export".to_owned())
        .spawn(move || run(&host, &path, &request))
        .map_err(|error| Error::Project(vcw_project::Error::Io(error)))?;
    Ok(())
}

/// The export thread.
fn run(host: &Hosted, path: &Path, request: &splitter::Request) {
    let written = std::cell::Cell::new(0u32);
    let outcome = export(host, path, request, &written);
    let event = match outcome {
        Ok(report) => Wire::ExportFinished {
            files: u32::try_from(report.files).unwrap_or(u32::MAX),
            covers: u32::try_from(report.covers).unwrap_or(u32::MAX),
            frames: report.frames,
            bytes_written: report.bytes,
        },
        Err(error) => Wire::ExportFailed {
            reason: error.to_string(),
            written: written.get(),
        },
    };
    let _ = host.emit(&event);
}

/// The part that can fail.
fn export(
    host: &Hosted,
    path: &Path,
    request: &splitter::Request,
    written: &std::cell::Cell<u32>,
) -> Result<splitter::Report, vcw_export::Error> {
    // Read-only, because §33 says an export reads immutable blocks and edit
    // instructions. Opening it writable would make a crash mid-export a risk to
    // the capture, which is the one thing that cannot be recorded again.
    let project = Project::open_read_only(path)?;
    let plan = splitter::plan(project.conn(), request)?;

    // One event as each file starts. The splitter calls this more than once per
    // item - it reports as it reads - so the index is the filter: the first
    // report for an item is the one that says a new file has begun.
    let started = std::cell::Cell::new(0usize);
    let mut on = |progress: Progress<'_>| {
        if progress.index < started.get() {
            return;
        }
        started.set(progress.index + 1);
        let index = u32::try_from(progress.index + 1).unwrap_or(u32::MAX);
        written.set(index.saturating_sub(1));
        let event = Wire::ExportProgress {
            index,
            of: u32::try_from(progress.of).unwrap_or(u32::MAX),
            path: progress.item.path.display().to_string(),
            frames: progress.frames,
            total: progress.total,
        };
        let _ = host.emit(&event);
    };
    let report = splitter::run(project.conn(), &plan, &mut on)?;
    project.close()?;
    Ok(report)
}
