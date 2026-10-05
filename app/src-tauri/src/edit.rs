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
//! wrong one would move the marker half a second. [`rate_of_side`] is why every
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

use tauri::State;
use vcw_contract::command::{Lock, Marker, Merge, Placement, Removal, Split, TrackEdit};
use vcw_project::track::NewBoundary;
use vcw_project::{Connection, Project, session, side, track};
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
#[tauri::command]
pub(crate) fn move_marker(shell: State<'_, Shell>, marker: Marker) -> Result<(), Error> {
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
#[tauri::command]
pub(crate) fn place_marker(shell: State<'_, Shell>, placement: Placement) -> Result<i64, Error> {
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
#[tauri::command]
pub(crate) fn delete_marker(shell: State<'_, Shell>, removal: Removal) -> Result<(), Error> {
    let path = shell.project_path()?;
    let mut project = Project::open(&path)?;
    // No seconds, so no rate, so no side lookup: this is the one boundary
    // command that needs nothing but the id.
    track::delete_boundary(&mut project, removal.boundary_id)?;
    project.close()?;
    Ok(())
}

/// Locks or unlocks a boundary. §31's "lock/unlock", §24's flag.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, and [`Error::Project`] if the
/// boundary is absent or the write fails.
#[tauri::command]
pub(crate) fn lock_marker(shell: State<'_, Shell>, lock: Lock) -> Result<(), Error> {
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
#[tauri::command]
pub(crate) fn edit_track(shell: State<'_, Shell>, edit: TrackEdit) -> Result<(), Error> {
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

/// Cuts a track in two. §31's "split".
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] if the track is
/// not in the project or its side has no capture, and [`Error::Project`] if the
/// cut is outside the track or the write fails.
#[tauri::command]
pub(crate) fn split_track(shell: State<'_, Shell>, split: Split) -> Result<i64, Error> {
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
#[tauri::command]
pub(crate) fn merge_tracks(shell: State<'_, Shell>, merge: Merge) -> Result<(), Error> {
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
