/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The application engine - everything but the user interface.
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

//! The application engine - everything but the user interface.
//!
//! Requirements: §11 (recording state), §35 (command and event surface), §36
//! (concurrency), §4.5 (a cross-platform Rust core).
//!
//! §2 is the rule this crate exists to enforce: the core is independent of any UI
//! framework. `vcw-cli` drives the whole application through this surface, and the
//! Tauri shell (WP-15) is one more consumer of the same commands and events, with
//! no logic of its own. CI asserts that no crate below `app/` depends on Tauri.
//!
//! Concurrency follows D8, locked by WP-07 as `docs/adr/0005-concurrency-model.md`:
//! dedicated OS threads on the capture path, `mpsc` between them, and no async
//! runtime anywhere near audio or SQLite. Tokio arrives with network I/O at WP-12
//! and no sooner. The engine thread is not a preference - a `cpal` stream handle
//! is `!Send`, so the thread that opens a device is the thread that keeps it.

pub mod adopt;
pub mod commands;
pub mod detection;
pub mod engine;
pub mod events;
pub mod identity;
pub mod metering;
pub mod playback;
pub mod state;

pub use adopt::{Adopted, Policy};
pub use commands::{Command, Setup};
pub use detection::{Detectors, Refined};
pub use engine::{Engine, Recorded, Recorder};
pub use events::{Bus, Event, Events};
pub use identity::{Applied, accept};
pub use metering::Meters;
pub use playback::{Audition, Cue, Player, Scope, Verb};
pub use state::{Deck, Machine, Phase, Rehearsal};
