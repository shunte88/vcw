/*
 *  audition.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Playback lives on a thread of its own, because a cpal stream cannot leave the thread that opened it (§21).
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

//! Playback lives on a thread of its own, because a cpal stream cannot leave
//! the thread that opened it (§21).
//!
//! [`vcw_core::playback::Player`] is documented as not `Send`, which decides
//! the shape of this module: the thread that opens a player is the thread that
//! keeps it, and every verb reaches it over a channel. That is the same shape
//! the engine has, for the same reason.
//!
//! It also decides where errors go. A device that cannot play the capture's
//! rate is refused rather than resampled (§9), and by the time the thread finds
//! that out the command has already returned. So an open that fails is
//! published as a warning with the code `playback-failed` and arrives at the
//! frontend as `capture-warning`. That is the honest answer given the bus has
//! no playback-refused event, and it is a wart: the code is there so a UI can
//! tell this apart from an overrun, and a later work package should give
//! playback a refusal event of its own.
//!
//! The thread also ticks. [`Player::tick`] is what publishes the playhead, so a
//! loop that only waited for verbs would play perfectly and report nothing.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use tauri::State;
use vcw_contract::command::{Audition as Scope, Playback};
use vcw_core::playback::{Audition, Player, Verb};
use vcw_core::{Bus, Event};

use crate::state::{Error, Shell};

/// How often the playhead is published while nothing else is happening.
///
/// 16 ms, which is a frame at 60 Hz and §17's meter rate. The playhead moves
/// continuously and a UI redraws at the display's rate, so anything faster is
/// work nobody sees.
const TICK: Duration = Duration::from_millis(16);

/// A running audition.
///
/// Holds the channel and the thread, and stops both when dropped, so a window
/// closing cannot leave a device open.
#[derive(Debug)]
pub(crate) struct Playing {
    /// Verbs on their way to the player.
    verbs: Sender<Verb>,
    /// The thread that owns the player.
    thread: Option<JoinHandle<()>>,
}

impl Playing {
    /// Sends a verb.
    ///
    /// # Errors
    ///
    /// [`Error::NotPlaying`] if the thread has already finished, which is the
    /// normal state a moment after an audition reaches its end.
    pub(crate) fn send(&self, verb: Verb) -> Result<(), Error> {
        self.verbs.send(verb).map_err(|_| Error::NotPlaying)
    }

    /// Stops the audition and waits for the device to close.
    ///
    /// Waits on purpose: the next `play` opens the same device, and returning
    /// before this one had let go would refuse it as busy.
    pub(crate) fn stop(mut self) {
        let _ = self.verbs.send(Verb::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Playing {
    fn drop(&mut self) {
        let _ = self.verbs.send(Verb::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts an audition. §35's `play`.
///
/// Any audition already running is stopped first, and waited for: one output
/// device, one thing playing.
///
/// A scope naming a row rather than a span - a track, a boundary - is resolved
/// here, from the project, which is why this takes the connection at all.
/// [`vcw_contract::command::Audition::scope`] returns `None` for exactly those
/// two cases rather than guessing a span.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Project`] if the capture is
/// not in the project, and [`Error::Invalid`] if the scope names a row that is
/// not there. A device that will not open is *not* an error here - see the
/// module docs.
#[tauri::command]
pub(crate) fn play(
    shell: State<'_, Shell>,
    capture_id: i64,
    scope: Scope,
    device: Option<String>,
) -> Result<(), Error> {
    let path = shell.project_path()?;
    let resolved = resolve(&path, capture_id, scope)?;

    let mut audition = Audition::new(&path, capture_id).scope(resolved);
    if let Some(device) = device {
        audition = audition.device(device);
    }

    // Stopped before the new one is opened, and outside the lock on the slot so
    // the join cannot deadlock against a thread that is publishing.
    let previous = shell.playing.lock().expect("the playing mutex").take();
    if let Some(previous) = previous {
        previous.stop();
    }

    let bus = shell.bus.clone();
    let (verbs, orders) = channel();
    let thread = std::thread::Builder::new()
        .name("vcw-audition".to_owned())
        .spawn(move || run(&audition, &bus, &orders))
        .map_err(|error| Error::Project(vcw_project::Error::Io(error)))?;

    *shell.playing.lock().expect("the playing mutex") = Some(Playing {
        verbs,
        thread: Some(thread),
    });
    Ok(())
}

/// Sends a verb to the running audition. §35's `pause`, `resume`, `seek`.
///
/// # Errors
///
/// [`Error::NotPlaying`] if nothing is playing, which includes an audition that
/// has just reached its end.
#[tauri::command]
pub(crate) fn playback(shell: State<'_, Shell>, verb: Playback) -> Result<(), Error> {
    if verb == Playback::Stop {
        let taken = shell.playing.lock().expect("the playing mutex").take();
        return match taken {
            Some(playing) => {
                playing.stop();
                Ok(())
            }
            None => Err(Error::NotPlaying),
        };
    }
    let held = shell.playing.lock().expect("the playing mutex");
    let playing = held.as_ref().ok_or(Error::NotPlaying)?;
    playing.send(Verb::from(verb))
}

/// Turns a scope that names a row into one that names a span.
fn resolve(
    path: &std::path::Path,
    capture_id: i64,
    scope: Scope,
) -> Result<vcw_core::playback::Scope, Error> {
    let project = vcw_project::Project::open_read_only(path)?;
    let layout = vcw_project::pcm::Layout::of(project.conn(), capture_id)?;
    let rate = layout.rate.hz();

    // The two spans a frontend cannot work out for itself.
    let resolved = match scope {
        Scope::Track { track_id } => {
            let record = vcw_project::track::track(project.conn(), track_id)?.ok_or_else(|| {
                Error::Invalid {
                    field: "trackId".to_owned(),
                    why: format!("there is no track {track_id} in this project"),
                }
            })?;
            let side = vcw_project::side::by_id(project.conn(), record.side_id)?;
            vcw_core::playback::Scope::Track {
                number: side.map_or(record.number, |side| record.position(side.side).number),
                span: vcw_types::Span::new(record.start, record.end),
            }
        }
        Scope::Boundary { boundary_id } => {
            let boundary =
                vcw_project::track::boundary(project.conn(), boundary_id)?.ok_or_else(|| {
                    Error::Invalid {
                        field: "boundaryId".to_owned(),
                        why: format!("there is no boundary {boundary_id} in this project"),
                    }
                })?;
            vcw_core::playback::Scope::boundary(layout.rate, boundary.at_frame)
        }
        // Whole and Region need no lookup, and `scope` says so by answering.
        other => other.scope(rate).ok_or_else(|| Error::Invalid {
            field: "scope".to_owned(),
            why: "a scope that names a row has to be resolved against the project".to_owned(),
        })?,
    };
    project.close()?;
    Ok(resolved)
}

/// The audition thread.
fn run(audition: &Audition, bus: &Bus, orders: &Receiver<Verb>) {
    let player = match Player::open(audition, bus) {
        Ok(player) => player,
        Err(error) => {
            // The command has already returned, so this is the only way to
            // say so. See the module docs: it is a wart, not a preference.
            bus.publish(&Event::Warning {
                code: "playback-failed",
                detail: error.to_string(),
            });
            return;
        }
    };

    // Opened paused, because §50 has the operator pick a region and then press
    // play. A `play` command means play, so this is the play.
    let _ = player.play();

    loop {
        player.tick();
        match orders.recv_timeout(TICK) {
            Ok(Verb::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(verb) => {
                if let Err(error) = player.apply(verb) {
                    bus.publish(&Event::Warning {
                        code: "playback-failed",
                        detail: error.to_string(),
                    });
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if player.has_ended() {
            break;
        }
    }

    // `stop` publishes `playback-finished`, which is how the frontend learns
    // that the audition is over whether it asked or the end arrived.
    let _ = player.stop();
}
