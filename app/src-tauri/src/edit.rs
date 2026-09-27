/*
 *  edit.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The editing commands: moving a boundary, which is the one edit §35 lists (§31).
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

//! The editing commands: moving a boundary, which is the one edit §35 lists
//! (§31).
//!
//! The only command in the shell that opens the project for writing, and the
//! only one that has to think about who else has it. A boundary is moved while
//! a person listens to the join, which is to say while nothing is being
//! captured, so the writable open is uncontended in practice - and if it is
//! not, SQLite says so and the command refuses rather than waiting.
//!
//! Seconds in, frames out. The frontend has a waveform drawn in seconds and a
//! drag that ended at one, and the rate that converts the two is the *capture's*
//! rather than a default, which is why this reads the side and its capture
//! before it does any arithmetic. §31's locked boundary is honoured here too:
//! `force` is the frontend saying the person meant it.

use tauri::State;
use vcw_contract::command::Marker;
use vcw_project::{Project, session, side, track};

use crate::state::{Error, Shell};

/// Moves a boundary. §35's `move_marker`.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] if the boundary is
/// not in the project or its side has no capture to measure against, and
/// [`Error::Project`] for a locked boundary moved without `force`, a move past a
/// neighbour, or a file that will not open for writing.
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

    // The rate is the capture's, not 44100 and not the device's current one: a
    // boundary at 12.5 s is 600 000 frames at 48 kHz and 551 250 at 44.1, and
    // the wrong one would move the marker half a second.
    let rate = side::by_id(project.conn(), boundary.side_id)?
        .and_then(|side| side.capture)
        .and_then(|capture| session::load(project.conn(), capture).ok().flatten())
        .map(|record| record.info.rate)
        .ok_or_else(|| Error::Invalid {
            field: "boundaryId".to_owned(),
            why: "this side has no capture, so there is nothing to measure seconds against"
                .to_owned(),
        })?;

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
fn frames(seconds: f64, rate: vcw_types::SampleRate) -> u64 {
    if seconds <= 0.0 {
        return 0;
    }
    (seconds * f64::from(rate.hz())).round() as u64
}
