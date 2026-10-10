/*
 *  host.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What a command needs from whatever is hosting it.
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

//! What a command needs from whatever is hosting it.
//!
//! Two hosts now: the Tauri window, and §52's HTTP listener. A command body
//! needs four things from either of them and nothing else - somewhere to put
//! an event, the two per-user directories whose location is the host's
//! question rather than VCW's, and the directory a path browser may show, if
//! this host has one at all. Everything else a command touches is a core
//! crate or the [`crate::state::Shell`] it was handed.
//!
//! Four methods rather than a handle, because the alternative is the one this
//! crate exists to undo: a command that takes an `AppHandle` is a command only
//! a window can call, and §52 needs the same bodies answering a socket.

use std::path::PathBuf;
use std::sync::Arc;

use vcw_contract::event::Wire;

/// Whatever is running these commands.
pub trait Host: Send + Sync {
    /// Puts an event in front of the person, however this host does that.
    ///
    /// Returns whether this host can still deliver anything *at all*. A window
    /// that has closed cannot, and [`crate::pump`] stops when it says so -
    /// otherwise a pump outlives its webview and reads a bus forever on
    /// nobody's behalf. Having nobody watching *right now* is not that: §52's
    /// listener has no browser attached between page loads and the capture
    /// still has to run, so it answers `true` and drops the event.
    ///
    /// Not a `Result`, because there is exactly one thing a caller can do
    /// about it and only one caller does it. A command body ignores the answer
    /// for the reason §36 gives: a webview that has gone must never take a
    /// capture down with it.
    #[must_use]
    fn emit(&self, wire: &Wire) -> bool;

    /// Where this user's settings live, if the host can say.
    ///
    /// `None` rather than a refusal, because the one caller
    /// ([`crate::config`]) already has to answer for a machine with no home
    /// directory and does it in one place.
    fn config_dir(&self) -> Option<PathBuf>;

    /// Where this user's cache lives, if the host can say.
    fn cache_dir(&self) -> Option<PathBuf>;

    /// The directory §52's path browser is confined to, if this host has one.
    ///
    /// `None` is the window, and means there is no path browser here at all:
    /// the desktop shell opens the platform's own file chooser, which is not
    /// VCW's to root. §52's listener answers `Some`, because the chooser it
    /// would open belongs to the wrong machine, so it draws one and this is
    /// the fence around it.
    ///
    /// Defaulted, so that a host which has no such browser says nothing about
    /// it rather than writing `None` out longhand.
    fn browse_root(&self) -> Option<PathBuf> {
        None
    }
}

/// A host, shared.
///
/// `Arc` rather than a borrow because three commands spawn a worker thread
/// that outlives them - detection, export and the event pump - and a thread
/// needs an owned handle. One alias so that every signature in this crate
/// names the same thing.
pub type Hosted = Arc<dyn Host>;
