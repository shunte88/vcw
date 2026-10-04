/*
 *  commands.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The command surface the core is driven through (§35).
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

//! The command surface the core is driven through (§35).
//!
//! §35 splits the boundary in two: commands change state, events report it.
//! This is the first half. A command is a *request* - it can be refused, and it
//! can be illegal - so nothing here returns a value. What happened comes back
//! as an [`Event`](crate::events::Event), through the same channel every other
//! observer is watching, so the CLI, the UI and a log all learn it the same way
//! and cannot disagree.
//!
//! # Why the transport commands do not carry a deck
//!
//! [`state::Step`](crate::state::Step) carries the deck to arm with, because
//! the state machine has to be handed the thing it will drive. A `Command` must
//! not: it crosses a process boundary in the Tauri shell (WP-15), and a live
//! audio device is not something a UI can serialise and send. So [`Command`]
//! carries a *description* of what to open - a [`Setup`] - and the engine turns
//! it into a deck on its own thread. That is not only a serialisation
//! convenience: a `vcw_audio::capture::Capture` is `!Send`, so the thread that
//! opens it must be the thread that keeps it, and no other arrangement is
//! possible.
//!
//! # What is here and what is not
//!
//! §35's examples run from `start_recording` to `export`. WP-07 owns the
//! transport; `play`, `seek`, `move_marker`, `search_metadata`,
//! `select_release` and `export` arrive with WP-10, WP-11, WP-12 and WP-14.
//! [`Command`] is `#[non_exhaustive]` so that adding them is not a breaking
//! change, and so that a `match` written today keeps compiling when they do.

use std::path::PathBuf;

use vcw_types::{CaptureEq, CaptureMode, SampleFormat};

/// What to open, described rather than held.
///
/// Everything the engine needs to build a deck, in a form a UI can send. The
/// optional fields are §9's rule in a struct: what the caller pins is a hard
/// requirement and what it leaves out is the device's choice. A pinned rate the
/// device cannot do is an error, never a quiet substitution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// Device id, or a name if it is unambiguous. `None` means the simulated
    /// source, which is how the transport is driven with nothing plugged in.
    pub device: Option<String>,
    /// Project to record into, created if it does not exist.
    pub project: PathBuf,
    /// Sample rate to pin.
    pub rate: Option<u32>,
    /// Channel count to pin.
    pub channels: Option<u16>,
    /// Sample format to pin.
    pub format: Option<SampleFormat>,
    /// Capture mode to request.
    pub mode: CaptureMode,
    /// Ring capacity in milliseconds. `None` takes the default from §10.
    pub ring_millis: Option<u32>,
    /// What equalisation the hardware upstream has already applied (§51).
    ///
    /// Not a request, unlike every field above it: nothing is asked of the
    /// device and nothing can be negotiated. It is the operator telling VCW what
    /// their signal chain is doing, and [`CaptureEq::Unknown`] when they have not.
    pub eq: CaptureEq,
}

impl Setup {
    /// A setup that records the simulated source into this project.
    ///
    /// The shortest path to a working transport, and the one the tests and the
    /// UI-with-nothing-attached both take.
    #[must_use]
    pub fn simulated(project: impl Into<PathBuf>) -> Self {
        Self {
            device: None,
            project: project.into(),
            rate: None,
            channels: None,
            format: None,
            mode: CaptureMode::Exclusive,
            ring_millis: None,
            eq: CaptureEq::Unknown,
        }
    }

    /// A setup that records this device into this project.
    #[must_use]
    pub fn device(device: impl Into<String>, project: impl Into<PathBuf>) -> Self {
        Self {
            device: Some(device.into()),
            ..Self::simulated(project)
        }
    }

    /// Pins the sample rate.
    #[must_use]
    pub const fn at(mut self, hz: u32) -> Self {
        self.rate = Some(hz);
        self
    }

    /// Pins the channel count.
    #[must_use]
    pub const fn channels(mut self, channels: u16) -> Self {
        self.channels = Some(channels);
        self
    }

    /// Pins the sample format.
    #[must_use]
    pub const fn format(mut self, format: SampleFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Requests a capture mode.
    #[must_use]
    pub const fn mode(mut self, mode: CaptureMode) -> Self {
        self.mode = mode;
        self
    }

    /// States the capture's equalisation provenance (§51).
    #[must_use]
    pub const fn eq(mut self, eq: CaptureEq) -> Self {
        self.eq = eq;
        self
    }

    /// Sets the ring capacity.
    #[must_use]
    pub const fn ring_millis(mut self, millis: u32) -> Self {
        self.ring_millis = Some(millis);
        self
    }
}

/// Something the core is asked to do (§35).
///
/// Named for the transition rather than for the button, because the transition
/// is what §11 defines and buttons change.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Open the device and the project. `Idle -> Armed`.
    Arm(Box<Setup>),
    /// Close them again without recording. `Armed -> Idle`.
    Disarm,
    /// Begin committing audio. `Armed -> Recording`.
    Record,
    /// Stop committing without giving up the device. `Recording -> Paused`.
    Pause,
    /// Begin committing again. `Paused -> Recording`.
    Resume,
    /// Finalise the capture. Keeps the project open, per §11.
    Stop,
    /// Release the finished capture and return to rest. `Stopped -> Idle`.
    Reset,
    /// Report the current phase and position without changing anything.
    ///
    /// Not a state change, which makes it the one command that bends §35's
    /// definition. It earns its place by being the only way a UI that has just
    /// connected - or reconnected after a webview reload, which S3's R8
    /// mitigation explicitly plans for - can find out where the transport is
    /// without waiting for something to happen.
    Poll,
    /// Finish the current capture if there is one, then shut the engine down.
    Shutdown,
}

impl Command {
    /// The lower-case name used in events, JSON and the CLI.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Arm(_) => "arm",
            Self::Disarm => "disarm",
            Self::Record => "record",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Stop => "stop",
            Self::Reset => "reset",
            Self::Poll => "poll",
            Self::Shutdown => "shutdown",
        }
    }

    /// Parses a command from the word an operator typed.
    ///
    /// `arm` needs a [`Setup`] and so cannot be built from a bare word; the CLI
    /// assembles that one from its own arguments. Everything else is a verb and
    /// nothing else.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word.trim().to_ascii_lowercase().as_str() {
            "disarm" => Some(Self::Disarm),
            "record" | "rec" => Some(Self::Record),
            "pause" => Some(Self::Pause),
            "resume" => Some(Self::Resume),
            "stop" => Some(Self::Stop),
            "reset" => Some(Self::Reset),
            "poll" | "status" => Some(Self::Poll),
            "shutdown" | "quit" | "exit" => Some(Self::Shutdown),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verb_parses_to_the_command_that_prints_as_it() {
        for command in [
            Command::Disarm,
            Command::Record,
            Command::Pause,
            Command::Resume,
            Command::Stop,
            Command::Reset,
            Command::Poll,
            Command::Shutdown,
        ] {
            assert_eq!(
                Command::parse(command.as_str()),
                Some(command.clone()),
                "{} does not survive the round trip",
                command.as_str()
            );
        }
    }

    #[test]
    fn the_operators_shorthand_works_and_nonsense_does_not() {
        assert_eq!(Command::parse("REC"), Some(Command::Record));
        assert_eq!(Command::parse("  quit  "), Some(Command::Shutdown));
        assert_eq!(Command::parse("status"), Some(Command::Poll));
        assert_eq!(Command::parse("arm"), None, "arm needs a setup");
        assert_eq!(Command::parse("eject"), None);
        assert_eq!(Command::parse(""), None);
    }

    #[test]
    fn a_setup_pins_only_what_it_was_told_to() {
        let setup = Setup::simulated("/tmp/x.vcw").at(192_000).channels(2);
        assert_eq!(setup.rate, Some(192_000));
        assert_eq!(setup.channels, Some(2));
        assert_eq!(
            setup.format, None,
            "an unpinned format is the device's call"
        );
        assert_eq!(setup.device, None);
    }
}
