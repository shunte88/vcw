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
//! that out the command has already returned successfully. An open that fails
//! is therefore published as [`Event::Denied`], which reaches the frontend as
//! `playback-refused`.
//!
//! That event exists because of this module and it replaced a wart. Until it
//! did, a refused open arrived as a `capture-warning` coded `playback-failed`,
//! which is the wrong shape in one specific way: a warning can be followed by
//! the thing it warned about, and a refusal cannot be followed by anything. No
//! `auditioning` came before it and no `playback-finished` will come after it,
//! so a frontend holding a transport in a playing state has to learn it here or
//! never.
//!
//! A verb that fails *after* the stream opened is still a warning, and
//! deliberately: audio was playing, the audition ends the ordinary way, and
//! `playback-finished` follows. The two cases are not the same event because
//! they are not the same fact.
//!
//! The thread also ticks. [`Player::tick`] is what publishes the playhead, so a
//! loop that only waited for verbs would play perfectly and report nothing.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

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
pub struct Playing {
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
    pub fn send(&self, verb: Verb) -> Result<(), Error> {
        self.verbs.send(verb).map_err(|_| Error::NotPlaying)
    }

    /// Stops the audition and waits for the device to close.
    ///
    /// Waits on purpose: the next `play` opens the same device, and returning
    /// before this one had let go would refuse it as busy.
    pub fn stop(mut self) {
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
pub fn play(
    shell: &Shell,
    capture_id: i64,
    scope: Scope,
    device: Option<String>,
) -> Result<(), Error> {
    let path = shell.project_path()?;
    let (resolved, marks) = resolve(&path, capture_id, scope)?;

    let mut audition = Audition::new(&path, capture_id)
        .scope(resolved)
        .marks(marks);
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
pub fn playback(shell: &Shell, verb: Playback) -> Result<(), Error> {
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

/// Turns a scope that names a row into one that names a span, and reads the
/// frames a skip should jump between.
///
/// Both in one open, because they come from the same project and the same
/// capture, and a second read-only open to fetch the track edges would be a
/// second chance for them to disagree with the span.
///
/// The marks are the whole capture's edges, not the scope's: [`Player::seek`]
/// clamps into the span, so a skip out of a one-track audition lands at its own
/// end rather than somewhere else, and filtering here would mean the shell
/// deciding twice what playback already decides once.
fn resolve(
    path: &std::path::Path,
    capture_id: i64,
    scope: Scope,
) -> Result<(vcw_core::playback::Scope, Vec<u64>), Error> {
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
    let marks = vcw_project::track::edges_of_capture(project.conn(), capture_id)?;
    project.close()?;
    Ok((resolved, marks))
}

/// The audition thread.
fn run(audition: &Audition, bus: &Bus, orders: &Receiver<Verb>) {
    let player = match Player::open(audition, bus) {
        Ok(player) => player,
        Err(error) => {
            // The command has already returned, so this is the only way to say
            // so, and it is terminal: nothing else will be published about this
            // audition.
            bus.publish(&Event::Denied {
                capture_id: audition.capture_id,
                scope: audition.scope.label(),
                reason: error.to_string(),
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
                    // A warning rather than a refusal: the stream opened, audio
                    // played, and `player.stop()` below still publishes
                    // `playback-finished`. See the module docs.
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

#[cfg(test)]
mod tests {
    use super::*;
    use vcw_contract::event::Wire;

    /// An audition that cannot possibly open, for the cheapest reason: there is
    /// no project at the path.
    ///
    /// A device that refuses a rate is the case this event was written for, and
    /// it is not the case a test can arrange - it needs a converter that says
    /// no. What both cases share is the part worth asserting: the failure
    /// happens on the thread, after the command returned, and the bus is the
    /// only way anybody hears about it.
    fn cannot_open() -> Audition {
        Audition::new(std::path::Path::new("/nonexistent/no.vcw"), 7)
            .scope(vcw_core::playback::Scope::Whole)
    }

    #[test]
    fn a_refused_open_is_a_refusal_and_not_a_warning() {
        let bus = Bus::new();
        let events = bus.subscribe();
        let (_verbs, orders) = channel::<Verb>();

        run(&cannot_open(), &bus, &orders);

        let published = events.drain();
        let Some(Event::Denied {
            capture_id,
            scope,
            reason,
        }) = published.first()
        else {
            panic!("a refused open published {published:?}");
        };
        assert_eq!(*capture_id, 7);
        assert_eq!(scope, "the whole capture");
        assert!(!reason.is_empty(), "a refusal with no reason in it");

        // Terminal, which is the whole argument for the variant: a frontend
        // that turned its transport on when it asked to play is turning it off
        // here or never.
        assert_eq!(
            published.len(),
            1,
            "something followed the refusal: {published:?}"
        );
        assert_eq!(Wire::from(&published[0]).kind(), "playback-refused");
    }
}
