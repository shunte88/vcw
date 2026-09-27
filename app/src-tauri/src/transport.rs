/*
 *  transport.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The commands that drive the capture transport, which is the engine's own vocabulary (§35).
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

//! The commands that drive the capture transport, which is the engine's own
//! vocabulary (§35).
//!
//! Every one of these returns as soon as the engine has been told, and none of
//! them returns what happened. That is not laziness: §35 says commands change
//! state and events expose it, and the engine is a thread that owns a device.
//! `record` cannot know whether recording started until the device's first
//! callback arrives, so a command that returned "started" would be guessing,
//! and a command that waited would block the webview for as long as the driver
//! felt like taking.
//!
//! What a frontend does instead is send the verb and watch for the
//! `phase-change`, `armed` or `command-refused` event that follows. That is the
//! same contract the CLI's interactive session works to.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, State};
use vcw_contract::command::{Arm, Transport};
use vcw_core::{Command, Engine, Setup};

use crate::pump;
use crate::state::{Error, Shell};

/// Opens a device and a project. §35's `arm`.
///
/// Starts the engine on the first call. The engine is not started with the
/// window because a machine with no sound card should still open one, and
/// because the thread has nothing to do until it is armed.
///
/// # Errors
///
/// [`Error::Invalid`] if an argument will not parse, and [`Error::Engine`] if
/// the engine thread has gone. A device that will not open is *not* an error
/// here: the engine reports that as a `command-refused` event, because by then
/// the command has been accepted and the answer is asynchronous.
#[tauri::command]
pub(crate) fn arm(shell: State<'_, Shell>, app: AppHandle, arm: Arm) -> Result<(), Error> {
    let setup = Setup::try_from(arm)?;
    let path = setup.project.clone();

    let mut held = shell.engine.lock().expect("the engine mutex");
    if held.is_none() {
        let engine = Engine::start()?;
        // Subscribed before the first command is sent, so the `armed` event
        // this call produces cannot be missed. The bus replays nothing.
        pump::forward(&app, engine.events(), "engine");
        *held = Some(engine);
    }
    let engine = held.as_ref().ok_or(Error::NotArmed)?;
    engine.send(Command::Arm(Box::new(setup)))?;
    drop(held);

    *shell.project.lock().expect("the project mutex") = Some(path);
    Ok(())
}

/// Sends a transport verb. §35's `disarm`, `record`, `pause`, `resume`,
/// `stop`, `reset`.
///
/// # Errors
///
/// [`Error::NotArmed`] if nothing has been armed, because there is no engine to
/// send to yet - which is a different thing from the engine refusing the verb,
/// and reads differently to a person.
#[tauri::command]
pub(crate) fn transport(shell: State<'_, Shell>, verb: Transport) -> Result<(), Error> {
    if verb == Transport::Shutdown {
        return shutdown(shell);
    }
    let held = shell.engine.lock().expect("the engine mutex");
    let engine = held.as_ref().ok_or(Error::NotArmed)?;
    engine.send(Command::from(verb))?;
    Ok(())
}

/// Asks the engine to publish where it is. §35's `poll`.
///
/// The answer is a `status` event, not a return value. A UI that has just
/// loaded uses this: the bus replays nothing, so a window opened onto a running
/// capture would otherwise have to wait for the next position update to know
/// anything at all.
///
/// # Errors
///
/// [`Error::NotArmed`] when there is no engine. A frontend can treat that as
/// "idle" rather than as a failure.
#[tauri::command]
pub(crate) fn poll(shell: State<'_, Shell>) -> Result<(), Error> {
    let held = shell.engine.lock().expect("the engine mutex");
    let engine = held.as_ref().ok_or(Error::NotArmed)?;
    engine.send(Command::Poll)?;
    Ok(())
}

/// Stops the engine and waits for it.
///
/// Finalises a capture in progress rather than abandoning it - that is
/// [`Engine::shutdown`]'s behaviour and the reason this is not just a dropped
/// handle: the audio is the part that cannot be recorded again.
///
/// # Errors
///
/// [`Error::Engine`] if the engine had already gone.
fn shutdown(shell: State<'_, Shell>) -> Result<(), Error> {
    let taken = shell.engine.lock().expect("the engine mutex").take();
    match taken {
        Some(engine) => Ok(engine.shutdown()?),
        None => Ok(()),
    }
}

/// Opens a project without arming a device, upgrading it if it is older (§16).
///
/// §50 starts at "choose a device", but a person who wants to look at yesterday
/// evening's capture should not have to open one. This sets the path every read
/// command uses and nothing else - no engine is started and no device is
/// touched.
///
/// # The path is only committed once the reads are known to work
///
/// First light found the reason this matters. A pre-WP-13 project opens
/// perfectly well read-only - it is a valid `.vcw` with a valid
/// `application_id` - and then every §29 read against it fails, because the
/// tables are not there. Committing the path on the strength of the open alone
/// left the shell pointing at one project while the window still showed the
/// last one that had read cleanly, and the next edit verb would have gone to
/// the file nobody was looking at.
///
/// So the check here is `require_current_schema`, not merely "does it open", and
/// a project that fails it is upgraded rather than refused: §16 asks a newer
/// build to upgrade an older project without destroying it, and
/// [`vcw_project::Project::open`] does exactly that inside a transaction. The
/// upgrade is v1 to v2, which adds three empty tables and touches no audio.
///
/// # Errors
///
/// [`Error::Project`] if the file will not open, or if the upgrade fails - in
/// which case the path is left as it was, because a shell pointing at a project
/// it cannot read is the bug this exists to prevent.
#[tauri::command]
pub(crate) fn open_project(shell: State<'_, Shell>, path: String) -> Result<(), Error> {
    let path = PathBuf::from(path);
    ensure_readable(&path)?;
    *shell.project.lock().expect("the project mutex") = Some(path);
    Ok(())
}

/// Opens a project, upgrades it if it is older, and closes it again.
///
/// Split out of [`open_project`] so it can be tested without a Tauri `State`:
/// the whole decision this command makes is in here, and what the command adds
/// is one assignment.
///
/// # Errors
///
/// Whatever the open or the upgrade failed with.
fn ensure_readable(path: &Path) -> Result<(), Error> {
    // Read-only first, so a typo is an error rather than a new empty project:
    // `open` would create the file, `open_read_only` cannot.
    let project = vcw_project::Project::open_read_only(path)?;
    let stale = project.require_current_schema().is_err();
    project.close()?;

    if stale {
        // Transactional, and rolled back on failure - so a project that cannot
        // be upgraded is left exactly as it was found.
        vcw_project::Project::open(path)?.close()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Winds a project back to the v1 it would have been before WP-13.
    fn wind_back_to_v1(path: &Path) {
        let project = vcw_project::Project::open(path).expect("opening the project");
        project
            .conn()
            .execute_batch(
                // Everything migration 2 creates, dropped in the order the
                // foreign keys point, then the bookkeeping that says it ran.
                "DROP TABLE IF EXISTS tracks;
                 DROP TABLE IF EXISTS track_boundaries;
                 DROP TABLE IF EXISTS sides;
                 DROP TABLE IF EXISTS release_artwork;
                 DROP TABLE IF EXISTS releases;
                 DELETE FROM schema_migrations WHERE version >= 2;
                 PRAGMA user_version = 1;",
            )
            .expect("winding the schema back");
        project.close().expect("closing it again");
    }

    /// The version in the file, read through a fresh open.
    fn version_of(path: &Path) -> u32 {
        let project = vcw_project::Project::open_read_only(path).expect("opening the project");
        let found = project.schema_version().expect("the schema version");
        project.close().expect("closing it");
        found
    }

    /// First light's finding: an older project used to be committed as the open
    /// path and then fail every read, leaving the window on the last project
    /// that had worked. Opening one upgrades it instead.
    #[test]
    fn opening_an_older_project_upgrades_it() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("older.vcw");
        vcw_project::Project::create(&path)
            .expect("creating the project")
            .close()
            .expect("closing it");
        wind_back_to_v1(&path);
        assert_eq!(version_of(&path), 1, "the fixture should start at v1");

        ensure_readable(&path).expect("an older project should open");

        assert_eq!(
            version_of(&path),
            vcw_project::schema::SCHEMA_VERSION,
            "and be current afterwards"
        );
        let project = vcw_project::Project::open_read_only(&path).expect("reopening it");
        project
            .require_current_schema()
            .expect("so the reads the browser does now work");
        project.close().expect("closing it");
    }

    /// A current project is left alone: no upgrade, no write, no error.
    #[test]
    fn opening_a_current_project_changes_nothing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("current.vcw");
        vcw_project::Project::create(&path)
            .expect("creating the project")
            .close()
            .expect("closing it");

        ensure_readable(&path).expect("a current project should open");
        assert_eq!(version_of(&path), vcw_project::schema::SCHEMA_VERSION);
    }

    /// A name that is not a project is an error rather than a new empty file,
    /// which is why the probe is read-only.
    #[test]
    fn a_path_that_is_not_there_is_refused_and_not_created() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("typo.vcw");

        assert!(ensure_readable(&path).is_err(), "it does not exist");
        assert!(!path.exists(), "and opening it must not have created it");
    }
}
