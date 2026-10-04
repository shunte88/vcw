/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Shared vocabulary for VCW.
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

//! Shared vocabulary for VCW.
//!
//! Every other crate in the workspace speaks these types, and this crate depends on
//! nothing but `serde` and `thiserror`. That is deliberate: it keeps CPAL, SQLite and
//! the network stack out of the dependency closure of crates that have no business
//! linking them. `vcw-signal` analysing samples should not pull in ALSA.
//!
//! The types here are the ones the specification already fixes - sample formats (§8),
//! capture modes (§9) and the hardware sample rates (§8). Nothing speculative lives
//! here; a type earns its place once a requirement or a decision pins it down.

pub mod capture;
pub mod format;
pub mod observation;
pub mod rate;
pub mod span;
pub mod summary;
pub mod vinyl;

pub use capture::{CaptureInfo, CaptureState, Diagnostics, PcmSource};
pub use format::{CaptureEq, CaptureMode, SampleFormat, StorageFormat};
pub use observation::{AudioObservation, BoundaryObservation, Edge, Evidence, Provenance};
pub use rate::{STANDARD_RATES, SampleRate};
pub use span::Span;
pub use summary::{Summary, TRIPLET_BYTES};
pub use vinyl::{Face, Numbering, Position, Side};
