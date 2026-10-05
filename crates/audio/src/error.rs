/*
 *  error.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What can go wrong between "list the devices" and "open that one".
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

//! What can go wrong between "list the devices" and "open that one".
//!
//! Every variant here names the device, the direction or the path involved.
//! A capture rig has several converters attached and a message that says only
//! "device not found" sends the user to the wrong one.

use std::path::PathBuf;

use crate::devices::{DeviceKey, Direction};

/// An error from device enumeration, probing or selection.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The remembered or requested device is not present.
    ///
    /// This is the ordinary hot-unplug case (§7), not a failure of the program.
    #[error(
        "no {direction} device with id `{key}` is present; it may be unplugged or in \
         use - run `vcw devices` to see what is there"
    )]
    NoSuchDevice {
        /// Which direction was being resolved.
        direction: Direction,
        /// The id that did not resolve.
        key: DeviceKey,
    },

    /// A name or substring matched more than one device.
    ///
    /// Never resolved by taking the first: on ALSA the same converter appears as
    /// both a `hw:` and a `plughw:` PCM, and only one of them can be bit-perfect.
    #[error("`{query}` is ambiguous - it matches {}; select by id instead", .candidates.join(", "))]
    Ambiguous {
        /// What the caller asked for.
        query: String,
        /// The labels of everything it matched.
        candidates: Vec<String>,
    },

    /// A name or substring matched nothing.
    #[error("nothing matches `{query}` among the {considered} {direction} device(s) present")]
    NoMatch {
        /// What the caller asked for.
        query: String,
        /// Which direction was searched.
        direction: Direction,
        /// How many devices were searched, so "nothing at all is plugged in" reads
        /// differently from "your spelling is wrong".
        considered: usize,
    },

    /// A persisted device id names a host this build cannot use.
    ///
    /// Happens when a project moves between machines, or between a build with ASIO
    /// or JACK compiled in and one without.
    #[error("host `{host}` is not available in this build on this platform")]
    UnknownHost {
        /// The host named by the id.
        host: String,
    },

    /// The platform offers no default for this direction.
    #[error("no default {direction} device on host `{host}`")]
    NoDefault {
        /// Which direction was requested.
        direction: Direction,
        /// The host that was asked.
        host: String,
    },

    /// A preferences file exists but cannot be read as preferences.
    #[error("{path} is not readable as device preferences")]
    Preferences {
        /// The offending file.
        path: PathBuf,
        /// What the parser objected to.
        #[source]
        source: serde_json::Error,
    },

    /// A filesystem operation failed, with the path that failed.
    #[error("{path}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// The audio backend refused.
    /// The device offers no §8 configuration matching what was asked for.
    ///
    /// Distinct from a backend refusal: the device is present and working, and
    /// what it can do simply does not include this. §9 makes it an error rather
    /// than a silent downgrade - a capture quietly made at half the requested
    /// rate is the exact failure the honesty rules exist to prevent.
    #[error("{device} cannot record {wanted}; it offers {offered}")]
    NoConfiguration {
        /// The device, as a user would recognize it.
        device: String,
        /// What was asked for.
        wanted: String,
        /// What is actually on offer.
        offered: String,
    },

    /// The device cannot play audio at the rate it was recorded at.
    ///
    /// Its own variant, and not [`Error::NoConfiguration`], because it is the
    /// one playback failure with a concrete answer: there is no resampler in
    /// VCW (§21), so a 192 kHz side needs a device that does 192 kHz. Saying so
    /// is more useful than an unannounced conversion, and far more useful than
    /// a generic refusal.
    #[error(
        "{device} cannot play {wanted} Hz audio; it offers {offered}. VCW does not \
         resample, so this capture needs a device that can play its own rate"
    )]
    RateUnavailable {
        /// The device, as a user would recognize it.
        device: String,
        /// The rate the capture was recorded at.
        wanted: u32,
        /// What the device does offer.
        offered: String,
    },

    /// Whatever the backend said, passed through unedited. Its own wording is
    /// usually more specific than anything we could put in front of it.
    #[error(transparent)]
    Cpal(#[from] cpal::Error),
}

/// The result type used throughout `vcw-audio`.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Attaches a path to an IO error, because a bare `NotFound` is not a
    /// diagnosis.
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
