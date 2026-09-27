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

use std::path::PathBuf;

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

/// Opens a project without arming a device.
///
/// §50 starts at "choose a device", but a person who wants to look at yesterday
/// evening's capture should not have to open one. This sets the path every read
/// command uses and nothing else - no engine is started and no device is
/// touched.
///
/// # Errors
///
/// [`Error::Project`] if the file will not open, which is checked here rather
/// than left for the first read to discover.
#[tauri::command]
pub(crate) fn open_project(shell: State<'_, Shell>, path: String) -> Result<(), Error> {
    let path = PathBuf::from(path);
    // Opened and closed, purely to fail now rather than on the first read. A
    // read-only open cannot create the file, so a typo is an error instead of a
    // new empty project.
    vcw_project::Project::open_read_only(&path)?.close()?;
    *shell.project.lock().expect("the project mutex") = Some(path);
    Ok(())
}
