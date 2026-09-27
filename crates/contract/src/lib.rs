/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The typed command, event and view-model surface between the core and any UI.
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

//! The typed command, event and view-model surface between the core and any UI.
//!
//! Requirements: §35 (commands, events and view models), §2 (the frontend is a
//! view layer and nothing else), D9 (a generated TypeScript contract with a
//! drift check).
//!
//! # Why this is a core crate and not part of the shell
//!
//! The obvious place for wire types is next to the thing that puts them on the
//! wire, which would be `app/src-tauri`. They are here instead, and the reason
//! is §2. The moment a type exists only inside the shell, the shell is the
//! thing that decides what a track looks like to a user - and that decision is
//! application behaviour, not presentation. Two consumers already need the same
//! answer: the Tauri shell and `vcw --json`, which is the CLI's machine-readable
//! output and was hand-building its own JSON object by object before this crate
//! existed.
//!
//! The practical consequence is the one WP-15's exit criterion asks for. The
//! shell is left holding nothing but glue: a `#[tauri::command]` per verb, each
//! one a call into a core crate and a `From` conversion, with no type of its own
//! to be authoritative about.
//!
//! # What a DTO is for
//!
//! Every type here is a *projection*, not a re-export. The core's own types are
//! built for the core's own work - [`vcw_core::Event`] carries a
//! [`vcw_signal::meter::Snapshot`] of linear amplitudes because that is what a
//! meter produces, and a `Phase` is `Copy` because the state machine compares
//! phases - and serialising them directly would publish those choices as the
//! contract. Then every internal rename is a breaking change to the frontend.
//!
//! So the rules for this crate are:
//!
//! - **Names are the ones §35 declares**, not the ones the variants happen to
//!   have. [`event::Wire`]'s tag is [`vcw_core::Event::name`] for every variant,
//!   and a test asserts it rather than a comment asking for it.
//! - **Fields are `camelCase`**, because the consumer is TypeScript and a
//!   contract that needs a translation layer to be idiomatic will get one.
//! - **Units are resolved here.** Amplitudes become dBFS, frames come with the
//!   seconds beside them, and a scope becomes a start and an end. §2 forbids
//!   signal processing in the frontend, and `20 * log10(x)` is signal
//!   processing.
//! - **No PCM, ever.** §35 is explicit, and nothing in this crate can carry a
//!   sample: the largest array on the wire is a waveform *summary*, three floats
//!   per pixel column.
//!
//! # The generated TypeScript
//!
//! [`bindings::typescript`] renders every type in this crate as one `.d.ts`
//! file, and `the_committed_typescript_matches_the_rust` compares it against the
//! copy checked in at [`bindings::PATH`]. Regenerate with
//! `VCW_BLESS=1 cargo test -p vcw-contract --test bindings`, which is the same
//! convention `docs/SCHEMA.md` uses.

pub mod bindings;
pub mod browse;
pub mod command;
pub mod event;
pub mod read;
pub mod settings;
pub mod view;

pub use command::{Audition, Failure, Marker, Playback, Region, Request, Search, Transport, Zoom};
pub use event::Wire;
pub use view::{Capture, Device, Levels, Meter, Release, Side, Track};
