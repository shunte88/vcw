/*
 *  selection.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Remembering which device the user chose, and coping when it is not there.
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

//! Remembering which device the user chose, and coping when it is not there.
//!
//! §7 asks for two things that look separate and are not: *"Preferred devices
//! shall persist between sessions"* and *"Device removal or configuration change
//! shall be handled without corrupting the project."* Persistence is what makes
//! removal detectable - without a record of what was chosen, an absent converter
//! is indistinguishable from never having chosen one, and the only available
//! behavior is to silently use something else.
//!
//! So [`resolve`] never falls back. If the remembered device is gone it says so
//! and stops. S1 finding 3 is the reason: the platform default on the development
//! host was the PipeWire path at 44.1 kHz float, which for a vinyl capture
//! application is the one configuration guaranteed not to be bit-perfect. Quietly
//! recording an album through it because a USB cable was loose is exactly the
//! failure §9 exists to prevent.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vcw_types::{CaptureMode, SampleFormat, SampleRate};

use crate::devices::{DeviceKey, DeviceReport, Direction, Snapshot, Transport};
use crate::error::{Error, Result};
use crate::probe::Capability;

/// The on-disk format version of the preferences file.
///
/// Bumped when the shape changes incompatibly. A file from the future is refused
/// rather than half-read, on the same principle as the project file (§16).
pub const PREFERENCES_VERSION: u32 = 1;

/// A device the user chose, and the configuration they chose with it.
///
/// The name and transport are stored alongside the key even though only the key
/// is used to re-open. They are there so the program can say *"the Tascam DA-3000
/// you recorded with last time is not connected"* rather than
/// *"alsa:hw:CARD=3,DEV=0 not found"*, and so that a device that has quietly
/// become a converting path can be noticed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remembered {
    /// The identity to re-open with.
    pub key: DeviceKey,
    /// What it was called when it was chosen.
    pub name: String,
    /// What kind of path it was.
    pub transport: Transport,
    /// The chosen sample rate, if one was chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<SampleRate>,
    /// The chosen sample representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<SampleFormat>,
    /// The chosen channel count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u16>,
    /// How the stream should be opened (§9). A request, not an outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_mode: Option<CaptureMode>,
}

impl Remembered {
    /// Records a choice made from the current device list.
    pub fn new(
        device: &DeviceReport,
        capability: Option<&Capability>,
        capture_mode: Option<CaptureMode>,
    ) -> Self {
        Self {
            key: device.key.clone(),
            name: device.name.clone(),
            transport: device.transport,
            rate: capability.map(|c| c.rate),
            format: capability.map(|c| c.format),
            channels: capability.map(|c| c.channels),
            capture_mode,
        }
    }

    /// Name and id together, for messages.
    pub fn label(&self) -> String {
        format!("{} [{}]", self.name, self.key)
    }
}

/// The persisted audio device choices (§39, *Audio*).
///
/// Deliberately **not** in the project file. A project is movable between
/// machines (§12) and a device id is not: carrying `alsa:hw:CARD=2,DEV=0` to
/// another computer would name whatever card happened to be second there. This is
/// machine state, and it lives with the application's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    /// File format version.
    pub version: u32,
    /// The capture device.
    #[serde(default)]
    pub input: Option<Remembered>,
    /// The playback device. Chosen independently of the capture device (§7).
    #[serde(default)]
    pub output: Option<Remembered>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            input: None,
            output: None,
        }
    }
}

impl Preferences {
    /// The choice for one direction.
    pub fn get(&self, direction: Direction) -> Option<&Remembered> {
        match direction {
            Direction::Input => self.input.as_ref(),
            Direction::Output => self.output.as_ref(),
        }
    }

    /// Records a choice.
    pub fn set(&mut self, direction: Direction, remembered: Remembered) {
        match direction {
            Direction::Input => self.input = Some(remembered),
            Direction::Output => self.output = Some(remembered),
        }
    }

    /// Forgets a choice, so the next session asks again rather than guessing.
    pub fn clear(&mut self, direction: Direction) {
        match direction {
            Direction::Input => self.input = None,
            Direction::Output => self.output = None,
        }
    }

    /// Reads the preferences file.
    ///
    /// `Ok(None)` means there is no file yet, which is the first-run case and not
    /// an error.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file exists but cannot be read, [`Error::Preferences`]
    /// if it is not valid preferences or comes from a newer version.
    pub fn load(path: impl AsRef<Path>) -> Result<Option<Self>> {
        let path = path.as_ref();
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::io(path, e)),
        };
        let prefs: Self = serde_json::from_str(&text).map_err(|source| Error::Preferences {
            path: path.to_owned(),
            source,
        })?;
        if prefs.version > PREFERENCES_VERSION {
            return Err(Error::Preferences {
                path: path.to_owned(),
                source: serde::de::Error::custom(format!(
                    "written by a newer version of VCW (format {}, this build reads {PREFERENCES_VERSION})",
                    prefs.version
                )),
            });
        }
        Ok(Some(prefs))
    }

    /// Reads the preferences, and moves an unreadable file aside rather than
    /// failing.
    ///
    /// Settings must never stop the application from starting. A file truncated by
    /// a power cut, or written by a newer build, is renamed to `<name>.corrupt` and
    /// the defaults are returned; the [`Reset`] says what happened so the UI can
    /// mention it once instead of failing silently.
    pub fn load_or_reset(path: impl AsRef<Path>) -> (Self, Option<Reset>) {
        let path = path.as_ref();
        match Self::load(path) {
            Ok(Some(prefs)) => (prefs, None),
            Ok(None) => (Self::default(), None),
            Err(e) => {
                let moved_to = path.with_extension("corrupt");
                let outcome = fs::rename(path, &moved_to);
                (
                    Self::default(),
                    Some(Reset {
                        reason: e.to_string(),
                        moved_to: outcome.is_ok().then_some(moved_to),
                    }),
                )
            }
        }
    }

    /// Writes the preferences file, atomically.
    ///
    /// Written to a sibling temporary file and renamed, so an interrupted write
    /// leaves the previous preferences intact rather than a half-file. The parent
    /// directory is created if it is missing.
    ///
    /// # Errors
    ///
    /// [`Error::Io`], naming the path that failed.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let mut json = serde_json::to_string_pretty(self).expect("preferences always serialize");
        json.push('\n');

        let temporary = path.with_extension("tmp");
        fs::write(&temporary, json).map_err(|e| Error::io(&temporary, e))?;
        fs::rename(&temporary, path).map_err(|e| Error::io(path, e))
    }
}

/// What happened to a preferences file that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reset {
    /// Why it was rejected.
    pub reason: String,
    /// Where the old file went, or `None` if it could not be moved.
    pub moved_to: Option<PathBuf>,
}

impl std::fmt::Display for Reset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "device preferences reset: {}", self.reason)?;
        if let Some(to) = &self.moved_to {
            write!(f, " (old file kept at {})", to.display())?;
        }
        Ok(())
    }
}

/// The outcome of looking for a remembered device in the current device list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution<'a> {
    /// No device has been chosen for this direction yet.
    Unset,
    /// The remembered device is present and still offers what was remembered.
    Ready {
        /// What was remembered.
        remembered: &'a Remembered,
        /// What is there now.
        device: &'a DeviceReport,
    },
    /// The remembered device is present, but something about it has moved.
    ///
    /// Usable, and worth saying out loud before a capture starts. The common case
    /// is a converter whose front panel was switched to a different rate.
    Changed {
        /// What was remembered.
        remembered: &'a Remembered,
        /// What is there now.
        device: &'a DeviceReport,
        /// What differs, in words fit to show a user.
        concerns: Vec<String>,
    },
    /// The remembered device is not present.
    ///
    /// No substitute is offered. See the module documentation.
    Missing {
        /// What was remembered.
        remembered: &'a Remembered,
    },
}

impl<'a> Resolution<'a> {
    /// The device, if one was found.
    pub fn device(&self) -> Option<&'a DeviceReport> {
        match self {
            Self::Ready { device, .. } | Self::Changed { device, .. } => Some(device),
            Self::Unset | Self::Missing { .. } => None,
        }
    }

    /// Whether a capture could proceed on this device without asking the user.
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

impl std::fmt::Display for Resolution<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unset => f.write_str("no device chosen"),
            Self::Ready { device, .. } => write!(f, "{}", device.label()),
            Self::Changed {
                device, concerns, ..
            } => {
                write!(f, "{} - changed: {}", device.label(), concerns.join("; "))
            }
            Self::Missing { remembered } => {
                write!(f, "{} is not connected", remembered.label())
            }
        }
    }
}

/// Looks for the remembered device in a snapshot.
///
/// Matching is on the id alone. A device with the same name at a different id is
/// a different device - on ALSA it is very often the same card reached through the
/// resampling plug layer instead of the driver.
pub fn resolve<'a>(
    preferences: &'a Preferences,
    snapshot: &'a Snapshot,
    direction: Direction,
) -> Resolution<'a> {
    let Some(remembered) = preferences.get(direction) else {
        return Resolution::Unset;
    };
    let Some(device) = snapshot.get(&remembered.key) else {
        return Resolution::Missing { remembered };
    };

    let mut concerns = Vec::new();
    if !device.supports(direction) {
        concerns.push(format!("it no longer offers {direction}"));
    }
    if device.name != remembered.name {
        concerns.push(format!("it is now called {:?}", device.name));
    }
    if device.transport != remembered.transport {
        concerns.push(format!(
            "it is now a {} path rather than {}",
            device.transport, remembered.transport
        ));
    }
    let report = device.direction(direction);
    if let Some(rate) = remembered.rate
        && !report.standard_rates().contains(&rate)
    {
        concerns.push(format!("{rate} is no longer offered"));
    }
    if let Some(format) = remembered.format
        && !report.standard_formats().contains(&format)
    {
        concerns.push(format!("{format:?} is no longer offered"));
    }
    if let Some(channels) = remembered.channels
        && !report.channel_counts().contains(&channels)
    {
        concerns.push(format!("{channels} channels are no longer offered"));
    }

    if concerns.is_empty() {
        Resolution::Ready { remembered, device }
    } else {
        Resolution::Changed {
            remembered,
            device,
            concerns,
        }
    }
}
