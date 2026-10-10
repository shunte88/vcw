/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The commands, with no window around them.
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

//! The commands, with no window around them (§5, §35, §52).
//!
//! Every verb §35 declares, as an ordinary function. This crate holds no audio
//! logic, no detection, no persistence and no decisions about what a track
//! looks like - all of that is in the core crates and the typed surface is
//! `vcw-contract`. What it does hold is the one piece that used to live in the
//! Tauri shell and could not: the glue that turns a frontend request into core
//! calls, and the single mutable [`state::Shell`] those calls share.
//!
//! It is here rather than in `app/src-tauri` because of §52. A command body
//! that takes a `tauri::AppHandle` is a command only a window can serve, and
//! §52 serves the same frontend over HTTP to a machine that has no window and
//! no WebKit to draw one with. Writing the bodies twice was the alternative and
//! §2 forbids it: one authoritative answer to each question, in one place.
//! What varies between the two hosts is four things, and [`host::Host`] names
//! them.
//!
//! `app/src-tauri` is now a window, a plugin, a list of `#[tauri::command]`
//! wrappers one line long each, and the pump that turns this crate's events
//! into webview events. The `core-is-ui-free` CI job walks this workspace, so
//! the compiler is what keeps that true.

pub mod audition;
pub mod config;
pub mod detect;
pub mod edit;
pub mod exporter;
pub mod host;
pub mod library;
pub mod metadata;
pub mod pump;
pub mod state;
pub mod transport;

pub use host::{Host, Hosted};
pub use state::{Error, Shell};
