/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Audio device management, bit-perfect capture and playback.
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

//! Audio device management, bit-perfect capture and playback.
//!
//! This is the only crate in the workspace that links CPAL. Everything downstream
//! of the callback deals in [`vcw_types`] vocabulary and plain PCM, so the rest of
//! the tree neither knows nor cares which host API delivered the bytes.
//!
//! Requirements: §7 (devices), §8 (capture configuration), §9 (bit-perfect capture),
//! §10 (the real-time pipeline), §21 (playback).
//!
//! # The shape of device handling
//!
//! Three steps, deliberately separate, because S1 showed that collapsing them is
//! how a capture ends up silently resampled:
//!
//! 1. [`devices::enumerate`] lists what is present, keyed by a stable id and
//!    labeled with what sits between the application and the converter.
//! 2. [`probe::Matrix`] reduces a device's advertisements to the configurations
//!    §8 cares about, and [`probe::confirm`] turns an advertisement into a fact by
//!    opening the device.
//! 3. [`selection`] remembers the choice across sessions and reports honestly when
//!    the device is no longer there, rather than substituting another.
//!
//! None of that amounts to a bit-perfection claim. §9 forbids making one on the
//! strength of the API's own report, and WP-04's per-platform verifier is what
//! settles the question against the operating system.
//!
//! ```no_run
//! use vcw_audio::{devices, probe, selection};
//!
//! let snapshot = devices::enumerate();
//! let device = snapshot.find("hw:CARD=0,DEV=0", devices::Direction::Input)?;
//! let matrix = probe::Matrix::from_report(&device.input, devices::Direction::Input);
//! if let Some(suggested) = matrix.suggest() {
//!     println!("{} at {} {:?}", device.label(), suggested.rate, suggested.format);
//! }
//! # Ok::<(), vcw_audio::Error>(())
//! ```

pub mod buffers;
pub mod capture;
pub mod chunks;
pub mod convert;
pub mod devices;
pub mod error;
pub mod playback;
pub mod probe;
pub mod selection;
pub mod source;
pub mod verify;

pub use devices::{Change, DeviceKey, DeviceReport, Direction, Snapshot, Transport};
pub use error::{Error, Result};
pub use probe::{Capability, Matrix, Support};
pub use selection::{Preferences, Remembered, Resolution};
