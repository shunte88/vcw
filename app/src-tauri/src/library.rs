/*
 *  library.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The read commands: everything a UI draws, taken from the project rather than cached (§2, §35).
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

//! The read commands: everything a UI draws, taken from the project rather than
//! cached (§2, §35).
//!
//! Each of these opens the project read-only, reads, and closes. That is a
//! deliberate choice and not an oversight.
//!
//! A kept-open handle would be faster, and it would also be a second writer's
//! worth of risk for no gain: the engine holds the project while a capture is
//! running, SQLite's WAL lets a reader in beside it, and a read-only handle
//! cannot corrupt the one thing in the file that cannot be recorded again. The
//! reads are also small - a tracklist is tens of rows - and the one that is not
//! is the waveform, which reads summaries rather than audio precisely so that
//! this stays true.
//!
//! No command here returns audio. §35 forbids PCM crossing the boundary, and a
//! waveform's three arrays are one float per drawn column, which is a few
//! thousand numbers for a screen-width of a two-hour side.

use tauri::State;
use vcw_contract::command::Zoom;
use vcw_contract::read;
use vcw_contract::view::{Capture, Device, Release, Side, Track, Waveform};
use vcw_project::Project;

use crate::state::{Error, Shell};

/// Every audio device the host offers, with what each can do (§7).
///
/// Enumeration is a synchronous walk of every host API and takes long enough on
/// some machines to be visible, which is why this is the one read command that
/// does not touch the project: it is also the first one §50 calls, before
/// anything is open.
///
/// # Errors
///
/// Never. A host that will not answer becomes a line in `problems` on the
/// affected device, or in the snapshot's own list if a whole host API failed -
/// which is why the return type is a list and not a `Result` of one: a machine
/// with one broken USB interface should still show the other three.
#[tauri::command]
pub(crate) fn devices() -> Vec<Device> {
    vcw_audio::devices::enumerate()
        .devices
        .iter()
        .map(Device::from)
        .collect()
}

/// The release, or `null` in a project that has not had one filled in.
///
/// # Errors
///
/// [`Error::NoProject`] if nothing is open, [`Error::Project`] if the file will
/// not read.
#[tauri::command]
pub(crate) fn release(shell: State<'_, Shell>) -> Result<Option<Release>, Error> {
    with_project(&shell, read::release)
}

/// Every side, in playing order.
///
/// # Errors
///
/// As [`release`].
#[tauri::command]
pub(crate) fn sides(shell: State<'_, Shell>) -> Result<Vec<Side>, Error> {
    with_project(&shell, read::sides)
}

/// Every track, in playing order, with §29's positions already rendered.
///
/// # Errors
///
/// As [`release`].
#[tauri::command]
pub(crate) fn tracks(shell: State<'_, Shell>) -> Result<Vec<Track>, Error> {
    with_project(&shell, read::tracks)
}

/// Every capture in the project, oldest first.
///
/// # Errors
///
/// As [`release`].
#[tauri::command]
pub(crate) fn captures(shell: State<'_, Shell>) -> Result<Vec<Capture>, Error> {
    with_project(&shell, read::captures)
}

/// One channel of one capture, drawn to a given width (§17, §20).
///
/// The columns come from the stored summaries, so this is a few hundred rows of
/// index rather than a decode - which is what makes it safe to call on every
/// zoom and pan. The reader picks the summary level from the frames-per-pixel
/// the request works out to; a frontend does not choose it and should not.
///
/// # Errors
///
/// As [`release`], plus [`Error::Invalid`] if `pixels` is zero - a canvas no
/// pixels wide is a frontend bug worth naming rather than an empty answer.
#[tauri::command]
pub(crate) fn waveform(shell: State<'_, Shell>, zoom: Zoom) -> Result<Waveform, Error> {
    if zoom.pixels == 0 {
        return Err(Error::Invalid {
            field: "pixels".to_owned(),
            why: "a waveform needs at least one column".to_owned(),
        });
    }
    let path = shell.project_path()?;
    let project = Project::open_read_only(&path)?;
    let shape = vcw_project::waveform::Shape::of(project.conn(), zoom.capture_id)?;
    let end = zoom.end_frame.unwrap_or(shape.frames).min(shape.frames);
    let start = zoom.start_frame.min(end);
    let request = vcw_signal::waveform::Request::new(start, end, zoom.pixels);
    let drawn =
        vcw_project::waveform::read(project.conn(), zoom.capture_id, zoom.channel, &request)?;
    project.close()?;
    Ok(Waveform::of(zoom.capture_id, &drawn))
}

/// Opens the project read-only, runs a reader, closes it.
///
/// The close is not decoration: `SQLITE_OPEN_READONLY` still creates `-wal` and
/// `-shm` sidecars, and an explicit close is what deletes them. A handle
/// dropped without one leaves them behind, which looks like a crash to the next
/// recovery check.
fn with_project<T, F>(shell: &State<'_, Shell>, reader: F) -> Result<T, Error>
where
    F: FnOnce(&vcw_project::Connection) -> vcw_project::Result<T>,
{
    let path = shell.project_path()?;
    let project = Project::open_read_only(&path)?;
    let read = reader(project.conn());
    project.close()?;
    Ok(read?)
}
