/*
 *  edit.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §31's edits: placing, moving, locking and deleting a boundary, and the track ops.
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

//! §31's edits: placing, moving, locking and deleting a boundary, and the
//! track operations built on them.
//!
//! The only commands in the shell that open the project for writing, and the
//! only ones that have to think about who else has it. An edit happens while a
//! person listens to a join, which is to say while nothing is being captured,
//! so the writable open is uncontended in practice - and if it is not, SQLite
//! says so and the command refuses rather than waiting.
//!
//! # Seconds in, frames out
//!
//! Every position that crosses from the frontend is in seconds, because that is
//! what a waveform is drawn in and what a drag ends at. The rate that converts
//! it is the *capture's* rather than a default or the device's current one: a
//! boundary at 12.5 s is 600 000 frames at 48 kHz and 551 250 at 44.1, and the
//! wrong one would move the marker half a second. `rate_of_side` is why every
//! command here reads the side before it does any arithmetic.
//!
//! # What is not decided here
//!
//! A boundary a person places is [`vcw_types::Provenance::User`], full
//! confidence and locked. None of that is this file's choice - it is
//! [`vcw_project::track::NewBoundary::by_user`]'s, and going through the
//! constructor rather than filling the struct in means the CLI's `vcw tracks
//! add` and this command cannot drift apart on what "a person put it there"
//! means. §31's locked boundary is honored the same way: `force` is the
//! frontend saying the person meant it, and the refusal without it comes from
//! the project crate.

use vcw_contract::command::{
    CaptureProvenance, Lock, Marker, Merge, NewTrack, Placement, ReleaseEdit, Removal, SideEdit,
    Split, TrackEdit,
};
use vcw_project::track::NewBoundary;
use vcw_project::{Connection, Project, release, session, side, track};
use vcw_types::SampleRate;

use crate::state::{Error, Shell};

/// Moves a boundary. §35's `move_marker`.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] if the boundary is
/// not in the project or its side has no capture to measure against, and
/// [`Error::Project`] for a locked boundary moved without `force`, a move past a
/// neighbor, or a file that will not open for writing.
pub fn move_marker(shell: &Shell, marker: Marker) -> Result<(), Error> {
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;

    let boundary =
        track::boundary(project.conn(), marker.boundary_id)?.ok_or_else(|| Error::Invalid {
            field: "boundaryId".to_owned(),
            why: format!(
                "there is no boundary {} in this project",
                marker.boundary_id
            ),
        })?;

    let rate = rate_of_side(project.conn(), boundary.side_id, "boundaryId")?;
    let to = frames(marker.to, rate);
    if marker.force {
        track::move_boundary_forced(&mut project, marker.boundary_id, to)?;
    } else {
        track::move_boundary(&mut project, marker.boundary_id, to)?;
    }
    project.close()?;
    Ok(())
}

/// Seconds to frames, rounded to the nearest.
///
/// Rounded rather than truncated: a drag that lands a hair before a frame
/// boundary meant the frame it looks like, and truncating would pull every move
/// half a frame earlier than the pointer.
fn frames(seconds: f64, rate: SampleRate) -> u64 {
    if seconds <= 0.0 {
        return 0;
    }
    (seconds * f64::from(rate.hz())).round() as u64
}

/// Places a boundary. §31's "add".
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] if the side is
/// not in the project or has no capture to measure seconds against, and
/// [`Error::Project`] if the write fails.
pub fn place_marker(shell: &Shell, placement: Placement) -> Result<i64, Error> {
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    let rate = rate_of_side(project.conn(), placement.side_id, "sideId")?;
    let at = frames(placement.at, rate);
    // Returns the row id, unlike every other command here, because the
    // frontend has to select what it just placed - and the alternative is
    // re-reading the boundary list and guessing which row is the new one by
    // its position, which is wrong the moment two boundaries share a frame.
    let id = track::add_boundary_to(
        &mut project,
        placement.side_id,
        &NewBoundary::by_user(at, placement.edge.into()),
    )?;
    project.close()?;
    Ok(id)
}

/// Deletes a boundary. §31's "delete".
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, and [`Error::Project`] if the
/// boundary is absent or a track is still using it.
pub fn delete_marker(shell: &Shell, removal: Removal) -> Result<(), Error> {
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    // No seconds, so no rate, so no side lookup: this is the one boundary
    // command that needs nothing but the id.
    if removal.force {
        track::delete_boundary_forced(&mut project, removal.boundary_id)?;
    } else {
        track::delete_boundary(&mut project, removal.boundary_id)?;
    }
    project.close()?;
    Ok(())
}

/// Locks or unlocks a boundary. §31's "lock/unlock", §24's flag.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, and [`Error::Project`] if the
/// boundary is absent or the write fails.
pub fn lock_marker(shell: &Shell, lock: Lock) -> Result<(), Error> {
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    track::set_lock(&mut project, lock.boundary_id, lock.locked)?;
    project.close()?;
    Ok(())
}

/// Retitles or annotates a track. §31's "rename", §32's fields.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, and [`Error::Project`] if the track
/// is absent.
pub fn edit_track(shell: &Shell, edit: TrackEdit) -> Result<(), Error> {
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    // An edit that names no field is allowed through rather than refused. It
    // writes each column back as it was and bumps `updated_at`, which is an
    // honest no-op, and the alternative would be a refusal blaming `trackId`
    // for something that is not wrong with it.
    track::update(&mut project, edit.track_id, &(&edit).into())?;
    project.close()?;
    Ok(())
}

/// Changes the two release facts nobody else can supply (§29, §51).
///
/// Neither comes back from a provider and neither can be read out of the audio,
/// so both are asked at project setup - and until this command, answered once
/// and never again from a window. A person who ticks the wrong box, or learns
/// from the deadwax which characteristic a 1956 pressing was cut to, had to
/// reach for the CLI.
///
/// Read-modify-write rather than a narrow UPDATE, because
/// [`vcw_project::release::store`] replaces every column: that is the one shape
/// that cannot lose a field by forgetting to mention it, and the price is a
/// read this command would do anyway to create the row if it is absent.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] if the curve is
/// not one the shipped catalog knows, and [`Error::Project`] if the write
/// fails.
pub fn edit_release(shell: &Shell, edit: ReleaseEdit) -> Result<(), Error> {
    let curve = checked_curve(edit.playback_eq.as_deref())?;
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    let mut record = release::ensure(&mut project)?;
    if let Some(mono) = edit.is_mono {
        record.is_mono = mono;
    }
    if let Some(id) = curve {
        record.playback_eq = (!id.is_empty()).then(|| id.to_owned());
    }
    release::store(&mut project, &record)?;
    project.close()?;
    Ok(())
}

/// Gives one side its own characteristic, or clears it back to the release's (§51).
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] if the curve is
/// unknown or no side has that id, and [`Error::Project`] if the write fails.
pub fn edit_side(shell: &Shell, edit: SideEdit) -> Result<(), Error> {
    let curve = checked_curve(edit.playback_eq.as_deref())?;
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    let record = side::by_id(project.conn(), edit.side_id)?.ok_or_else(|| Error::Invalid {
        field: "sideId".to_owned(),
        why: format!("there is no side {} in this project", edit.side_id),
    })?;
    if let Some(id) = curve {
        side::set_playback_eq(&mut project, record.side, (!id.is_empty()).then_some(id))?;
    }
    project.close()?;
    Ok(())
}

/// Checks a curve id against the shipped catalog before it is stored.
///
/// The shipped catalog only, which is the rule `vcw eq` follows too: a curve
/// id written into a project has to resolve on whatever machine opens the
/// project next, so a name that only one `--curves` file knows would be a
/// dangling reference in the file. `None` and `""` pass through - the first is
/// "leave it alone" and the second "nobody has said" - and a typo does not.
///
/// # Errors
///
/// [`Error::Invalid`] naming `playbackEq`, listing what the catalog knows.
pub(crate) fn checked_curve(id: Option<&str>) -> Result<Option<&str>, Error> {
    match id {
        Some(named) if !named.is_empty() => match vcw_eq::Catalog::builtin().require(named) {
            Ok(_) => Ok(Some(named)),
            Err(refusal) => Err(Error::Invalid {
                field: "playbackEq".to_owned(),
                why: refusal.to_string(),
            }),
        },
        other => Ok(other),
    }
}

/// States what the signal chain had already applied to a capture (§51).
///
/// Separate from [`edit_track`] and from the release because it belongs to
/// neither: `captureEq` is a fact about the hardware a recording came through,
/// where `playbackEq` is a fact about the disc. A window that could only set
/// the second one could name a curve on an imported project and never be
/// allowed to hear it.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] if `eq` is not
/// one of the three answers or no capture has that id, and [`Error::Project`]
/// if the write fails.
pub fn set_provenance(shell: &Shell, stated: CaptureProvenance) -> Result<(), Error> {
    let eq = vcw_types::CaptureEq::parse(&stated.eq).ok_or_else(|| Error::Invalid {
        field: "eq".to_owned(),
        why: "state flat, riaa or unknown".to_owned(),
    })?;
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    let changed = session::set_capture_eq(&mut project, Some(stated.capture_id), eq)?;
    project.close()?;
    if changed == 0 {
        return Err(Error::Invalid {
            field: "captureId".to_owned(),
            why: format!("there is no capture {} in this project", stated.capture_id),
        });
    }
    Ok(())
}

/// Makes a track out of a stretch a person chose. §31's "add".
///
/// Two boundaries and a track, and then the whole side renumbered in timeline
/// order - so a track added in front of the others becomes the first and the
/// rest move down. That is the behavior a first user asked for in the same
/// breath as the verb itself: "how do I say that this highlighted area should
/// be A1 and all of the later ones should be renamed".
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] if the side is
/// not in the project or has no capture to measure seconds against, and
/// [`Error::Project`] if `end` is not after `start`, if the stretch lies over
/// a track that is already there, or if the write fails.
pub fn add_track(shell: &Shell, add: NewTrack) -> Result<i64, Error> {
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    let rate = rate_of_side(project.conn(), add.side_id, "sideId")?;
    // Returns the row id for the reason `place_marker` does: the frontend has
    // to select what it just made.
    let id = track::add_track_to(
        &mut project,
        add.side_id,
        frames(add.start, rate),
        frames(add.end, rate),
    )?;
    project.close()?;
    Ok(id)
}

/// Cuts a track in two. §31's "split".
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] if the track is
/// not in the project or its side has no capture, and [`Error::Project`] if the
/// cut is outside the track or the write fails.
pub fn split_track(shell: &Shell, split: Split) -> Result<i64, Error> {
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    let record = track::track(project.conn(), split.track_id)?.ok_or_else(|| Error::Invalid {
        field: "trackId".to_owned(),
        why: format!("there is no track {} in this project", split.track_id),
    })?;
    let rate = rate_of_side(project.conn(), record.side_id, "trackId")?;
    let id = track::split(&mut project, split.track_id, frames(split.at, rate))?;
    project.close()?;
    Ok(id)
}

/// Joins two adjacent tracks. §31's "merge".
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, and [`Error::Project`] if either
/// track is absent, they are not adjacent, or they are on different sides.
pub fn merge_tracks(shell: &Shell, merge: Merge) -> Result<(), Error> {
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    track::merge(&mut project, merge.left_id, merge.right_id)?;
    project.close()?;
    Ok(())
}

/// The sample rate of the capture a side was recorded from.
///
/// `field` is the argument to blame, because the same lookup is reached from a
/// boundary id, a side id and a track id, and a message naming `sideId` in
/// answer to a track command sends a person looking in the wrong place.
///
/// # Errors
///
/// [`Error::Invalid`] if the side is absent or has no capture attached, and
/// [`Error::Project`] if a row will not read.
fn rate_of_side(conn: &Connection, side_id: i64, field: &str) -> Result<SampleRate, Error> {
    let record = side::by_id(conn, side_id)?.ok_or_else(|| Error::Invalid {
        field: field.to_owned(),
        why: format!("there is no side {side_id} in this project"),
    })?;
    let capture = record.capture.ok_or_else(|| Error::Invalid {
        field: field.to_owned(),
        why: format!(
            "side {} has no capture, so there is nothing to measure seconds against",
            record.side.letter()
        ),
    })?;
    let session = session::load(conn, capture)?.ok_or_else(|| Error::Invalid {
        field: field.to_owned(),
        why: format!("capture {capture} is attached to side {side_id} but is not in the project"),
    })?;
    Ok(session.info.rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A shell holding an empty project, which is what both commands need.
    fn opened(dir: &std::path::Path) -> Shell {
        let path = dir.join("record.vcw");
        Project::create(&path)
            .expect("create")
            .close()
            .expect("close");
        Shell {
            project: Mutex::new(Some(path)),
            ..Shell::default()
        }
    }

    fn release_of(shell: &Shell) -> release::Record {
        let path = shell.project_path().expect("a project");
        let project = Project::open_read_only(&path).expect("open");
        release::load(project.conn())
            .expect("read")
            .expect("a release row")
    }

    #[test]
    fn a_typo_is_refused_rather_than_stored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shell = opened(dir.path());
        let refusal = edit_release(
            &shell,
            ReleaseEdit {
                is_mono: None,
                playback_eq: Some("riia".to_owned()),
            },
        )
        .expect_err("a curve nobody ships");
        assert!(
            matches!(&refusal, Error::Invalid { field, .. } if field == "playbackEq"),
            "blamed the wrong field: {refusal}"
        );
    }

    #[test]
    fn stating_one_fact_leaves_the_other_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shell = opened(dir.path());
        edit_release(
            &shell,
            ReleaseEdit {
                is_mono: Some(true),
                playback_eq: Some("columbia_lp".to_owned()),
            },
        )
        .expect("both");
        // The second edit names only the curve. Mono has to survive it, which
        // is the whole point of the three-state rule: `null` is not `false`.
        edit_release(
            &shell,
            ReleaseEdit {
                is_mono: None,
                playback_eq: Some(String::new()),
            },
        )
        .expect("clearing the curve");
        let record = release_of(&shell);
        assert!(
            record.is_mono,
            "mono was cleared by an edit that said nothing about it"
        );
        assert_eq!(
            record.playback_eq, None,
            "an empty string did not clear the curve"
        );
    }

    #[test]
    fn a_side_takes_its_own_curve_and_gives_it_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shell = opened(dir.path());
        let path = shell.project_path().expect("a project");
        let id = {
            let mut project = Project::open(&path).expect("open");
            let record = side::ensure(&mut project, vcw_types::vinyl::Side::A).expect("side A");
            project.close().expect("close");
            record.id
        };
        edit_side(
            &shell,
            SideEdit {
                side_id: id,
                playback_eq: Some("decca_lp".to_owned()),
            },
        )
        .expect("a side curve");
        let project = Project::open_read_only(&path).expect("reopen");
        let record = side::by_id(project.conn(), id)
            .expect("read")
            .expect("side A");
        assert_eq!(record.playback_eq.as_deref(), Some("decca_lp"));
    }

    #[test]
    fn an_absent_side_is_named_in_the_refusal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shell = opened(dir.path());
        let refusal = edit_side(
            &shell,
            SideEdit {
                side_id: 404,
                playback_eq: Some("riaa".to_owned()),
            },
        )
        .expect_err("no such side");
        assert!(
            matches!(&refusal, Error::Invalid { field, .. } if field == "sideId"),
            "blamed the wrong field: {refusal}"
        );
    }
}
