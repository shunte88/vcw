/*
 *  host.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The window as a host: where an event goes, and where the two directories are.
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
//! The window as a host: where an event goes, and where the two directories are.
//!
//! [`vcw_shell::Host`] is four questions a command body cannot answer for
//! itself, and this is the window's answer to them. §52's listener answers the
//! same three differently, which is the whole reason the trait exists - see
//! `crates/shell/src/host.rs`.
//!
//! One event name, `vcw://event`, for every kind. The alternative - a name per
//! kind, so a meter component subscribes only to meters - was measured in S3
//! and is not worth it: the cost of a 60 Hz meter update is drawing it on the
//! main thread, not delivering it, and the boundary itself was free at every
//! rate tried. One name keeps `Wire`'s discriminated union usable as it was
//! designed, with a single `switch (event.kind)` at the top of the frontend.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use vcw_contract::event::Wire;
use vcw_shell::{Host, Hosted};

/// The event name every core event arrives under.
pub(crate) const EVENT: &str = "vcw://event";

/// The window, as a command body sees it.
///
/// Generic over the runtime only so that the test at the bottom can build one
/// on Tauri's mock runtime and ask it where the config directory is. The
/// binary has exactly one runtime.
pub(crate) struct Window<R: Runtime>(AppHandle<R>);

/// This window, shareable.
///
/// A fresh `Arc` per command rather than one in managed state: an `AppHandle`
/// is itself a cheap clone, the allocation is once per user action, and state
/// that has to be installed before the first command can run is one more thing
/// that can be forgotten.
pub(crate) fn hosted<R: Runtime>(app: &AppHandle<R>) -> Hosted {
    Arc::new(Window(app.clone()))
}

impl<R: Runtime> Host for Window<R> {
    /// Emits on the webview's event channel.
    ///
    /// A failed emit means the window has gone, which is the one case
    /// [`vcw_shell::pump`] stops for: there is nothing left to deliver to and
    /// nothing to report it to either.
    fn emit(&self, wire: &Wire) -> bool {
        self.0.emit(EVENT, wire).is_ok()
    }

    fn config_dir(&self) -> Option<PathBuf> {
        self.0.path().app_config_dir().ok()
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        self.0.path().app_cache_dir().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window and [`vcw_i18n`] name the same two directories.
    ///
    /// There used to be two rules for one path: Tauri's, which the window
    /// asked, and a hand-rolled copy of the platform convention, which
    /// everything without a window asked. They agreed on this machine and
    /// there was no reason to believe they would keep agreeing - a bundle
    /// identifier changes, or `appDirectoriesOverride` gets set, and a
    /// translator's catalog quietly stops being found by the window that told
    /// them where to put it.
    ///
    /// So there is one rule now, in `vcw-i18n`, and this is the only place
    /// that can check it against Tauri's, because this is the only crate that
    /// has a Tauri to ask. The mock runtime opens no window; `app_config_dir`
    /// reads the real `tauri.conf.json` identifier through
    /// `generate_context!`, which is the half of the answer that can drift.
    #[test]
    fn the_window_and_the_rule_agree() {
        let app = tauri::test::mock_builder()
            .build(tauri::generate_context!())
            .expect("the mock runtime builds an app");
        let window = hosted(app.handle());

        assert_eq!(
            window.config_dir(),
            vcw_i18n::config_dir(),
            "Tauri and `vcw_i18n::config_dir` disagree about where this user's \
             settings live. One of them has to move - and it is not Tauri, \
             because `vcw serve` has no Tauri to ask."
        );
        assert_eq!(
            window.cache_dir(),
            vcw_i18n::cache_dir(),
            "Tauri and `vcw_i18n::cache_dir` disagree about where this user's \
             cache lives."
        );
    }
}
