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
use crate::host::Hosted;
use crate::state::{Error, Shell};

/// One side to look at, with everything the pass needs about it.
///
/// Resolved on the command thread so the refusals happen there. A side with no
/// capture is not an error at this point - it is simply not in the list, which
/// is what makes `detect_tracks` with no side argument mean "every side there
/// is something to analyze on" rather than a failure on the empty ones. A
/// project with no sides at all is not an error either: see [`resolve`].
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
/// holding no capture at all, and [`Error::Project`] if the project will not
/// open.
pub fn detect_tracks(shell: &Shell, host: &Hosted, detect: Detect) -> Result<(), Error> {
    let path = shell.project_path()?;
    let wanted = detect.side()?;

    // Read before spawning, so a settings file with a bad detector name is a
    // refused command naming `algorithm` rather than a `detection-failed`
    // event. `policy` needs a rate and each side has its own, so only the
    // parse is done here and the policy is built per target below.
    let detection = config::load(host)?.detection;
    let rate = SampleRate(44_100);
    detection.policy(rate)?;

    let targets = resolve(&path, wanted)?;
    if targets.is_empty() {
        // `resolve` falls back to the project's captures, so reaching here
        // means there are none - not that nothing is attached to a side. The
        // old wording blamed the side for the absence of a recording, which
        // sent a user looking for a side letter to fix.
        return Err(Error::Invalid {
            field: "side".to_owned(),
            why: "this project holds no capture to analyze - record one first".to_owned(),
        });
    }

    let promote = detect.promote;
    let host = host.clone();
    std::thread::Builder::new()
        .name("vcw-detect".to_owned())
        .spawn(move || run(&host, &path, &targets, &detection, promote))
        .map_err(|error| Error::Project(vcw_project::Error::Io(error)))?;
    Ok(())
}

/// The sides worth analyzing, with their captures.
///
/// # Why an unattached capture still resolves
///
/// Nothing in the capture path creates a side. `side::attach` has exactly two
/// production callers - the CLI's `vcw tracks attach` and
/// [`adopt`](vcw_core::adopt) - so a project recorded by `vcw session` holds a
/// capture and no sides at all, and this was the only reader in the product
/// that insisted on one. Every other one falls back to the project's captures:
/// `vcw detect`, `vcw waveform`, `vcw play`, `vcw fingerprint`, and this
/// shell's own view layer through `vcw_contract::read`. Which is why a
/// piCorePlayer user could open a CLI-recorded project in the window, draw its
/// waveform and play it, and still be told there was nothing to analyze - the
/// view found the capture and the detector would not look.
///
/// So when no side carries a capture, the project's own captures are the
/// targets, under the side that was asked for or `A` if none was. Detection
/// does not invent the attachment: [`adopt`](vcw_core::adopt) already calls
/// `side::ensure`, so promoting a boundary is what makes the side real, and a
/// pass that promotes nothing leaves the project exactly as it found it.
///
/// The last capture rather than all of them, matching `vcw detect`: a project
/// with several unattached captures has had something unusual done to it, and
/// analyzing every take at once is a worse guess than analyzing the newest.
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
    if targets.is_empty()
        && let Some(record) = session::all(project.conn())?.last()
    {
        targets.push(Target {
            side: wanted.unwrap_or(Side::A),
            capture: record.id,
            rate: record.info.rate,
        });
    }
    project.close()?;
    Ok(targets)
}

/// The detection thread.
fn run(host: &Hosted, path: &Path, targets: &[Target], detection: &Detection, promote: bool) {
    let began = std::time::Instant::now();
    let cfg = detection.config();
    let mut total = Adopted::default();
    let mut done: Vec<String> = Vec::new();

    for target in targets {
        match one(host, path, target, &cfg, detection, promote) {
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
                let _ = host.emit(&event);
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
    let _ = host.emit(&event);
}

/// One side: refine, adopt, and announce each boundary as it is written.
fn one(
    host: &Hosted,
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
    claim(&mut project, target)?;

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
            let _ = host.emit(&event);
        }
    }
    project.close()?;
    Ok(adopted)
}

/// Points the side at the capture this pass analyzed, if nothing else claims it.
///
/// # Why detection attaches the side
///
/// [`adopt::adopt_decisions`] calls `side::ensure`, which inserts the row with
/// `capture_id` NULL - the audio is not its business. On a project recorded by
/// `vcw session` that leaves a half-row: tracks on side A and no capture under
/// it. The window then has no current side, because §21 lets two faces share a
/// capture so the shell will not guess which one, and that is what the marker
/// key and the mark list read. `relay` refuses outright, since a track's
/// boundaries are frames into *some* recording and a side naming none cannot be
/// laid out. `vcw tracks attach` is how a CLI operator says this; the window
/// has no such verb, so detection says it.
///
/// Only when the side names nothing yet. `side::attach` is documented as
/// allowed to replace, because that is what re-recording a face looks like -
/// and this is not that. [`resolve`] already prefers a side's own capture when
/// it has one, so arriving here with a different capture under the side means
/// the operator picked a face recorded separately, and re-pointing it at this
/// take would quietly disown their audio.
///
/// # Errors
///
/// [`Error::Project`] if the side cannot be read or written.
fn claim(project: &mut Project, target: &Target) -> Result<(), Error> {
    if side::load(project.conn(), target.side)?.is_none_or(|row| row.capture.is_none()) {
        side::attach(project, target.side, target.capture)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcw_project::Session;
    use vcw_types::{CaptureInfo, CaptureMode, StorageFormat};

    /// Side B, spelled the way `Side` allows.
    fn b() -> Side {
        Side::from_letter('B').expect("B")
    }

    fn info() -> CaptureInfo {
        CaptureInfo::unverified(
            SampleRate(44_100),
            2,
            StorageFormat::Int16,
            CaptureMode::Shared,
        )
    }

    /// A project the way `vcw session` leaves one: captures, no sides.
    fn recorded_by_the_cli(path: &Path) -> i64 {
        let mut project = Project::create(path).expect("create");
        let id = Session::begin(&mut project, &info()).expect("begin").id();
        project.close().expect("close");
        id
    }

    #[test]
    fn a_capture_no_side_points_at_is_still_something_to_analyze() {
        // The piCorePlayer report, 2026-10-09: a side recorded with the CLI,
        // opened in the window, waveform drawn, audio played - and `T`
        // refused it. Nothing in the capture path creates a side, and this
        // was the one reader that required one.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cli.vcw");
        let id = recorded_by_the_cli(&path);

        let any = resolve(&path, None).expect("resolve");
        assert_eq!(any.len(), 1, "the project's own capture is the target");
        assert_eq!(any[0].capture, id);
        assert_eq!(any[0].side, Side::A, "unasked, an unattached capture is A");

        // And it answers for the side actually asked for, rather than
        // reporting that side as empty while the project holds a recording.
        let asked = resolve(&path, Some(b())).expect("resolve B");
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].side, b());
        assert_eq!(asked[0].capture, id);
    }

    #[test]
    fn detection_points_an_unclaimed_side_at_the_capture_it_analyzed() {
        // Adoption creates the side row with a NULL capture, which leaves a
        // project with tracks on a face that names no audio - and then the
        // window has no current side, the marker key does nothing and `relay`
        // refuses to lay the release out. Found by reading what `side::ensure`
        // actually inserts after the first light run got detection to finish.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("claimed.vcw");
        let id = recorded_by_the_cli(&path);

        let mut project = Project::open(&path).expect("open");
        claim(
            &mut project,
            &Target {
                side: b(),
                capture: id,
                rate: SampleRate(44_100),
            },
        )
        .expect("claim");
        let row = side::load(project.conn(), b()).expect("load").expect("row");
        assert_eq!(row.capture, Some(id), "the face names the take");
        project.close().expect("close");
    }

    #[test]
    fn a_side_that_already_names_a_capture_is_left_alone() {
        // The negative twin, and the reason the guard is there rather than an
        // unconditional attach: `side::attach` is allowed to replace, which is
        // what re-recording a face means. Pointing a side at a different take
        // because somebody ran detection over it would disown their audio.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("spoken-for.vcw");
        let mut project = Project::create(&path).expect("create");
        let first = Session::begin(&mut project, &info()).expect("a").id();
        let second = Session::begin(&mut project, &info()).expect("b").id();
        side::attach(&mut project, Side::A, first).expect("attach");

        claim(
            &mut project,
            &Target {
                side: Side::A,
                capture: second,
                rate: SampleRate(44_100),
            },
        )
        .expect("claim");
        let row = side::load(project.conn(), Side::A)
            .expect("load")
            .expect("row");
        assert_eq!(
            row.capture,
            Some(first),
            "still the take it was recorded on"
        );
        project.close().expect("close");
    }

    #[test]
    fn an_attached_side_is_still_what_wins() {
        // The negative twin for the fallback: once sides exist they are the
        // answer, or a two-sided project would analyze its last capture twice.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("attached.vcw");
        let mut project = Project::create(&path).expect("create");
        let first = Session::begin(&mut project, &info()).expect("a").id();
        let second = Session::begin(&mut project, &info()).expect("b").id();
        side::attach(&mut project, Side::A, first).expect("attach A");
        side::attach(&mut project, b(), second).expect("attach B");
        project.close().expect("close");

        let all = resolve(&path, None).expect("resolve");
        assert_eq!(all.len(), 2, "both sides, not the fallback's one capture");
        let only_a = resolve(&path, Some(Side::A)).expect("resolve A");
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_a[0].capture, first, "side A keeps its own capture");
    }

    #[test]
    fn a_project_with_no_capture_at_all_resolves_to_nothing() {
        // What is left for the refusal to be about.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.vcw");
        Project::create(&path)
            .expect("create")
            .close()
            .expect("close");
        assert!(resolve(&path, None).expect("resolve").is_empty());
        assert!(resolve(&path, Some(Side::A)).expect("resolve").is_empty());
    }
}
