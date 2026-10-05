/*
 *  detect.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §22's detection pass, asked for from the UI and run on a thread of its own.
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

//! §22's detection pass, asked for from the UI and run on a thread of its own.
//!
//! The same three detectors the CLI's `vcw detect` runs and the same adoption
//! the engine's live analysis uses, reached from a keystroke. Nothing about the
//! signal work is here: this module resolves which sides to look at, turns
//! §39's settings into a [`Config`] and a [`Policy`], and reports.
//!
//! # Why it is on a thread
//!
//! An extraction is an FFT per window over the whole side, which is tens of
//! seconds on a forty-minute rip, and a synchronous `#[tauri::command]` runs on
//! the thread drawing the window. So the command validates its arguments, spawns,
//! and returns; the frontend learns what happened from `track-detected` per
//! boundary and then exactly one of `detection-finished` or `detection-failed`.
//!
//! # Why it opens the project twice per side
//!
//! [`refine_project`](detection::refine_project) opens it read-only, which is
//! what lets a side be analyzed while another one is being recorded, and
//! adoption needs a writable handle. Opening once writable for both would hold
//! a write lock across the whole extraction for no reason, and the pass has
//! nothing to write until it has finished thinking.
//!
//! # What is validated before the thread starts
//!
//! The settings, the side letter and the existence of a capture to analyze.
//! Those are the three things a person can get wrong, and each of them is worth
//! a refusal with a field name rather than an event several seconds later. What
//! is *not* pre-validated is anything about the audio, because finding out
//! costs the same as doing the work.

use std::path::Path;

use tauri::{AppHandle, Emitter, State};
use vcw_contract::command::Detect;
use vcw_contract::event::Wire;
use vcw_contract::settings::Detection;
use vcw_core::adopt::{self, Adopted, Policy};
use vcw_core::detection;
use vcw_project::{Project, session, side, track};
use vcw_signal::regions::Config;
use vcw_types::SampleRate;
use vcw_types::vinyl::Side;

use crate::config;
use crate::pump;
use crate::state::{Error, Shell};

/// One side to look at, with everything the pass needs about it.
///
/// Resolved on the command thread so the refusals happen there. A side with no
/// capture is not an error at this point - it is simply not in the list, which
/// is what makes `detect_tracks` with no side argument mean "every side there
/// is something to analyze on" rather than a failure on the empty ones.
struct Target {
    /// Which side.
    side: Side,
    /// The capture attached to it.
    capture: i64,
    /// That capture's rate, which the policy's frame floor needs.
    rate: SampleRate,
}

/// Runs detection over one side or all of them. §22, reached from §43's `T`.
///
/// Returns as soon as the thread is running. Everything after that is events.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] for a side letter
/// that is not one, a detector name that is not one of §39's four, or a project
/// with no capture on the side asked for, and [`Error::Project`] if the project
/// will not open.
#[tauri::command]
pub(crate) fn detect_tracks(
    shell: State<'_, Shell>,
    app: AppHandle,
    detect: Detect,
) -> Result<(), Error> {
    let path = shell.project_path()?;
    let wanted = detect.side()?;

    // Read before spawning, so a settings file with a bad detector name is a
    // refused command naming `algorithm` rather than a `detection-failed`
    // event. `policy` needs a rate and each side has its own, so only the
    // parse is done here and the policy is built per target below.
    let detection = config::load(&app)?.detection;
    let rate = SampleRate(44_100);
    detection.policy(rate)?;

    let targets = resolve(&path, wanted)?;
    if targets.is_empty() {
        return Err(Error::Invalid {
            field: "side".to_owned(),
            why: match wanted {
                Some(side) => format!(
                    "side {} has no capture to analyze - record it first",
                    side.letter()
                ),
                None => "no side in this project has a capture to analyze".to_owned(),
            },
        });
    }

    let promote = detect.promote;
    std::thread::Builder::new()
        .name("vcw-detect".to_owned())
        .spawn(move || run(&app, &path, &targets, &detection, promote))
        .map_err(|error| Error::Project(vcw_project::Error::Io(error)))?;
    Ok(())
}

/// The sides worth analyzing, with their captures.
fn resolve(path: &Path, wanted: Option<Side>) -> Result<Vec<Target>, Error> {
    let project = Project::open_read_only(path)?;
    let mut targets = Vec::new();
    for record in side::list(project.conn())? {
        if wanted.is_some_and(|only| only != record.side) {
            continue;
        }
        let Some(capture) = record.capture else {
            continue;
        };
        let Some(loaded) = session::load(project.conn(), capture)? else {
            continue;
        };
        targets.push(Target {
            side: record.side,
            capture,
            rate: loaded.info.rate,
        });
    }
    project.close()?;
    Ok(targets)
}

/// The detection thread.
fn run(app: &AppHandle, path: &Path, targets: &[Target], detection: &Detection, promote: bool) {
    let began = std::time::Instant::now();
    let cfg = detection.config();
    let mut total = Adopted::default();
    let mut done: Vec<String> = Vec::new();

    for target in targets {
        match one(app, path, target, &cfg, detection, promote) {
            Ok(adopted) => {
                total.boundaries.extend(adopted.boundaries);
                total.tracks.extend(adopted.tracks);
                total.rejected += adopted.rejected;
                total.already_locked += adopted.already_locked;
                done.push(target.side.letter().to_string());
            }
            Err(error) => {
                let event = Wire::DetectionFailed {
                    reason: error.to_string(),
                    side: Some(target.side.letter().to_string()),
                    completed: u32::try_from(done.len()).unwrap_or(u32::MAX),
                };
                let _ = app.emit(pump::EVENT, &event);
                return;
            }
        }
    }

    let event = Wire::DetectionFinished {
        sides: done,
        boundaries: u32::try_from(total.written()).unwrap_or(u32::MAX),
        tracks: u32::try_from(total.tracks.len()).unwrap_or(u32::MAX),
        rejected: u32::try_from(total.rejected).unwrap_or(u32::MAX),
        already_settled: u32::try_from(total.already_locked).unwrap_or(u32::MAX),
        seconds: began.elapsed().as_secs_f64(),
    };
    let _ = app.emit(pump::EVENT, &event);
}

/// One side: refine, adopt, and announce each boundary as it is written.
fn one(
    app: &AppHandle,
    path: &Path,
    target: &Target,
    cfg: &Config,
    detection: &Detection,
    promote: bool,
) -> Result<Adopted, Error> {
    // The operator's own boundaries, handed to the resolver as prior
    // observations. Without this the pass would not know they exist and would
    // put a rival boundary beside every one of them - §24's whole point.
    let already = {
        let project = Project::open_read_only(path)?;
        let observations = adopt::locked_observations(&project, target.side)?;
        project.close()?;
        observations
    };

    let refined = detection::refine_project(path, target.capture, cfg, &already)?;

    let policy = Policy {
        pair_tracks: promote,
        ..detection.policy(target.rate)?
    };
    let mut project = Project::open(path)?;
    let adopted = adopt::adopt_decisions(&mut project, target.side, &refined.decisions, &policy)?;

    // One event per boundary, read back rather than taken from the decisions:
    // adoption is what decided which ones exist, and a row is the only thing
    // that can report its own id, provenance and confidence after the policy
    // and §24's locking have had their say.
    for id in &adopted.boundaries {
        if let Some(row) = track::boundary(project.conn(), *id)? {
            let event = Wire::TrackDetected {
                frame: row.at_frame,
                seconds: row.at_frame as f64 / f64::from(target.rate.hz()),
                edge: row.edge.into(),
                confidence: row.confidence,
                provenance: row.provenance.into(),
            };
            let _ = app.emit(pump::EVENT, &event);
        }
    }
    project.close()?;
    Ok(adopted)
}
