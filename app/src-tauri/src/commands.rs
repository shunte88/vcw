/*
 *  commands.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The list of commands the frontend may call, one line each.
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

//! The list of commands the frontend may call, one line each (§35).
//!
//! Every body is in `vcw-shell`, and every function here does the same three
//! things: hand it the managed [`Shell`], hand it this window as a
//! [`Hosted`](vcw_shell::Hosted), and hand back what it returned. There is no
//! logic in this file on purpose - §52 serves the same commands over HTTP
//! from a machine with no window, so a decision taken here would be a decision
//! that only desktop users get.
//!
//! The argument names are part of the contract. Tauri deserializes a command's
//! payload by parameter name, so renaming one here renames it in the frontend;
//! `shell` and `app` are injected by Tauri and are not payload.
//!
//! The two metadata commands are `async` and the rest are not. A search is a
//! network round trip boxed at ten seconds by §40, and a synchronous
//! `#[tauri::command]` runs on the main thread - so those two do their waiting
//! on a blocking worker and the window stays answerable. That is this host's
//! problem rather than the command's, which is why the wrapping is here.

use tauri::{AppHandle, Manager, State};
use vcw_contract::command::{
    Arm, Audition, Detect, Export, Lock, Marker, Merge, NewProject, Placement, Playback, Removal,
    Search, Selection, Split, TrackEdit, Transport, Zoom,
};
use vcw_contract::settings::{Credential, Settings};
use vcw_contract::view::{
    About, Accepted, Boundary, Candidate, Capture, Device, ExportPlan, Listing,
    Project as ProjectRow, Release, Side, Track, Waveform,
};
use vcw_shell::state::{Error, Shell};

use crate::host::hosted;

#[tauri::command]
pub(crate) fn arm(shell: State<'_, Shell>, app: AppHandle, arm: Arm) -> Result<(), Error> {
    vcw_shell::transport::arm(&shell, &hosted(&app), arm)
}

#[tauri::command]
pub(crate) fn transport(shell: State<'_, Shell>, verb: Transport) -> Result<(), Error> {
    vcw_shell::transport::transport(&shell, verb)
}

#[tauri::command]
pub(crate) fn poll(shell: State<'_, Shell>) -> Result<(), Error> {
    vcw_shell::transport::poll(&shell)
}

#[tauri::command]
pub(crate) fn open_project(shell: State<'_, Shell>, path: String) -> Result<(), Error> {
    vcw_shell::transport::open_project(&shell, path)
}

#[tauri::command]
pub(crate) fn about() -> About {
    vcw_shell::library::about()
}

#[tauri::command]
pub(crate) fn support(page: String) -> Result<(), Error> {
    vcw_shell::library::support(page)
}

#[tauri::command]
pub(crate) fn devices() -> Vec<Device> {
    vcw_shell::library::devices()
}

#[tauri::command]
pub(crate) fn release(shell: State<'_, Shell>) -> Result<Option<Release>, Error> {
    vcw_shell::library::release(&shell)
}

#[tauri::command]
pub(crate) fn sides(shell: State<'_, Shell>) -> Result<Vec<Side>, Error> {
    vcw_shell::library::sides(&shell)
}

#[tauri::command]
pub(crate) fn tracks(shell: State<'_, Shell>) -> Result<Vec<Track>, Error> {
    vcw_shell::library::tracks(&shell)
}

#[tauri::command]
pub(crate) fn captures(shell: State<'_, Shell>) -> Result<Vec<Capture>, Error> {
    vcw_shell::library::captures(&shell)
}

#[tauri::command]
pub(crate) fn waveform(shell: State<'_, Shell>, zoom: Zoom) -> Result<Waveform, Error> {
    vcw_shell::library::waveform(&shell, zoom)
}

#[tauri::command]
pub(crate) fn boundaries(shell: State<'_, Shell>) -> Result<Vec<Boundary>, Error> {
    vcw_shell::library::boundaries(&shell)
}

#[tauri::command]
pub(crate) fn settings(app: AppHandle) -> Result<Settings, Error> {
    vcw_shell::config::settings(&hosted(&app))
}

#[tauri::command]
pub(crate) fn save_settings(app: AppHandle, settings: Settings) -> Result<(), Error> {
    vcw_shell::config::save_settings(&hosted(&app), settings)
}

#[tauri::command]
pub(crate) fn languages() -> Vec<String> {
    vcw_shell::config::languages()
}

#[tauri::command]
pub(crate) fn credentials() -> Vec<Credential> {
    vcw_shell::config::credentials()
}

#[tauri::command]
pub(crate) fn projects(app: AppHandle) -> Result<Vec<ProjectRow>, Error> {
    vcw_shell::config::projects(&hosted(&app))
}

#[tauri::command]
pub(crate) fn new_project(app: AppHandle, seed: NewProject) -> Result<ProjectRow, Error> {
    vcw_shell::config::new_project(&hosted(&app), seed)
}

#[tauri::command]
pub(crate) fn library_root(app: AppHandle) -> Result<Option<String>, Error> {
    vcw_shell::config::library_root(&hosted(&app))
}

#[tauri::command]
pub(crate) fn browse(app: AppHandle, at: Option<String>) -> Result<Listing, Error> {
    vcw_shell::config::browse(&hosted(&app), at)
}

#[tauri::command]
pub(crate) fn artwork(path: String) -> Result<Option<String>, Error> {
    vcw_shell::config::artwork(path)
}

#[tauri::command]
pub(crate) fn open_path(shell: State<'_, Shell>) -> Option<String> {
    vcw_shell::config::open_path(&shell)
}

#[tauri::command]
pub(crate) fn move_marker(shell: State<'_, Shell>, marker: Marker) -> Result<(), Error> {
    vcw_shell::edit::move_marker(&shell, marker)
}

#[tauri::command]
pub(crate) fn place_marker(shell: State<'_, Shell>, placement: Placement) -> Result<i64, Error> {
    vcw_shell::edit::place_marker(&shell, placement)
}

#[tauri::command]
pub(crate) fn delete_marker(shell: State<'_, Shell>, removal: Removal) -> Result<(), Error> {
    vcw_shell::edit::delete_marker(&shell, removal)
}

#[tauri::command]
pub(crate) fn lock_marker(shell: State<'_, Shell>, lock: Lock) -> Result<(), Error> {
    vcw_shell::edit::lock_marker(&shell, lock)
}

#[tauri::command]
pub(crate) fn edit_track(shell: State<'_, Shell>, edit: TrackEdit) -> Result<(), Error> {
    vcw_shell::edit::edit_track(&shell, edit)
}

#[tauri::command]
pub(crate) fn split_track(shell: State<'_, Shell>, split: Split) -> Result<i64, Error> {
    vcw_shell::edit::split_track(&shell, split)
}

#[tauri::command]
pub(crate) fn merge_tracks(shell: State<'_, Shell>, merge: Merge) -> Result<(), Error> {
    vcw_shell::edit::merge_tracks(&shell, merge)
}

#[tauri::command]
pub(crate) fn detect_tracks(
    shell: State<'_, Shell>,
    app: AppHandle,
    detect: Detect,
) -> Result<(), Error> {
    vcw_shell::detect::detect_tracks(&shell, &hosted(&app), detect)
}

#[tauri::command]
pub(crate) fn play(
    shell: State<'_, Shell>,
    capture_id: i64,
    scope: Audition,
    device: Option<String>,
) -> Result<(), Error> {
    vcw_shell::audition::play(&shell, capture_id, scope, device)
}

#[tauri::command]
pub(crate) fn playback(shell: State<'_, Shell>, verb: Playback) -> Result<(), Error> {
    vcw_shell::audition::playback(&shell, verb)
}

#[tauri::command]
pub(crate) fn export_plan(shell: State<'_, Shell>, export: Export) -> Result<ExportPlan, Error> {
    vcw_shell::exporter::export_plan(&shell, export)
}

#[tauri::command]
pub(crate) fn export_run(
    shell: State<'_, Shell>,
    app: AppHandle,
    export: Export,
) -> Result<(), Error> {
    vcw_shell::exporter::export_run(&shell, &hosted(&app), export)
}

#[tauri::command]
pub(crate) async fn search_metadata(
    app: AppHandle,
    search: Search,
) -> Result<Vec<Candidate>, Error> {
    off_the_main_thread("provider", app, move |app| {
        vcw_shell::metadata::search_metadata(&app.state::<Shell>(), &hosted(&app), search)
    })
    .await
}

#[tauri::command]
pub(crate) async fn select_release(
    app: AppHandle,
    selection: Selection,
) -> Result<Accepted, Error> {
    off_the_main_thread("id", app, move |app| {
        vcw_shell::metadata::select_release(&app.state::<Shell>(), &hosted(&app), selection)
    })
    .await
}

/// Runs a command body on a blocking worker and waits for it.
///
/// `field` is what a thread that never finished is blamed on. It cannot
/// happen through any path here - the closures below do not panic and nothing
/// cancels the handle - but a `JoinError` is a real outcome of `await` and
/// swallowing it would turn a dead worker into a command that hangs.
async fn off_the_main_thread<T, F>(field: &'static str, app: AppHandle, body: F) -> Result<T, Error>
where
    T: Send + 'static,
    F: FnOnce(AppHandle) -> Result<T, Error> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || body(app))
        .await
        .map_err(|why| Error::Invalid {
            field: field.to_owned(),
            why: format!("the worker thread did not finish: {why}"),
        })?
}
