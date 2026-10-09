/*
 *  main.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The window, and the list of commands the frontend may call (§5, §6, §35).
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

//! The window, and the list of commands the frontend may call (§5, §6, §35).
//!
//! This crate is glue and nothing else. It holds no audio logic, no detection,
//! no persistence and no decisions about what a track looks like - all of that
//! is in the core crates, and the typed surface between the two is
//! `vcw-contract`. §2 requires it, and the layout enforces it: `app/` is a
//! workspace of its own, so `cargo tree --workspace` at the repository root
//! cannot reach Tauri and the `core-is-ui-free` CI job is a statement about
//! structure rather than a promise about discipline.
//!
//! What is here:
//!
//! - [`state`], the only mutable state in the shell;
//! - [`pump`], one thread per bus turning core events into webview events;
//! - [`transport`], [`library`], [`audition`], [`edit`], [`detect`],
//!   [`metadata`], [`config`] and [`exporter`], which are §35's commands
//!   grouped by what they act on.
//!
//! Every verb §35 declares is now wired, and the tests at the bottom of this
//! file are what say so: [`WIRED`] is checked against the tags in the generated
//! `Request` union, in both directions. Until WP-16 there was a `NOT_WIRED`
//! list beside it and a `refused` command to answer for it, because a command a
//! frontend can send and nothing answers is worse than one that refuses out
//! loud. There is nothing left for it to hold, so it is gone - which makes the
//! test a requirement rather than a reminder: a command added to the contract
//! now fails the suite until something here honors it.

// A release build should not open a console window behind the app on Windows. In
// a debug build it should, because that is where a panic is printed.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audition;
mod config;
mod detect;
mod edit;
mod exporter;
mod library;
mod metadata;
mod pump;
mod state;
mod transport;

use tauri::Manager;

use state::Shell;

/// The commands in [`vcw_contract::Request`] that this shell honors.
///
/// These are the contract's *tags*, not the names in `generate_handler!`, and
/// three of them differ from the command that serves them:
///
/// - `seek` is served by `playback`, because a seek is one of six playback
///   verbs and a command per verb would be five commands that all do
///   `player.apply`;
/// - `export` is served by `export_plan` and `export_run`, because §33 plans
///   before it writes and a UI shows the plan;
/// - `transport` carries the capture verbs, which is one command with a verb
///   argument for the same reason `playback` is.
///
/// The read commands - `devices`, `tracks`, `waveform` and the rest - are not
/// in [`vcw_contract::Request`] at all. §35's list is the commands that change
/// something; reading is not one of them.
///
/// `#[cfg(test)]`, because `generate_handler!` below is the actual wiring and
/// this list is the assertion about it. A copy compiled into the binary would
/// be a second answer to the same question.
#[cfg(test)]
pub(crate) const WIRED: [&str; 17] = [
    "arm",
    "transport",
    "play",
    "seek",
    "move_marker",
    "place_marker",
    "delete_marker",
    "lock_marker",
    "edit_track",
    "split_track",
    "merge_tracks",
    "detect_tracks",
    "search_metadata",
    "select_release",
    "export",
    "new_project",
    "save_settings",
];

/// Installs the log subscriber for the window process (§42).
///
/// The same policy as the CLI's `logging` module, for the same reasons: stderr,
/// `warn` by default so a packaged application is quiet, and `VCW_LOG` to turn
/// it up per target. Kept as its own function rather than inlined so that "the
/// binaries decide logging, the libraries do not" is visible in both binaries.
///
/// A failed install is ignored: something else having arranged logging is not a
/// reason to refuse to open a window.
fn install_logging() {
    let filter = match std::env::var("VCW_LOG") {
        Ok(text) if !text.trim().is_empty() => {
            tracing_subscriber::EnvFilter::builder().parse_lossy(text)
        }
        _ => tracing_subscriber::EnvFilter::builder().parse_lossy("warn"),
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .try_init();
}

fn main() {
    install_logging();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "the shell is starting");
    tauri::Builder::default()
        // The one plugin VCW installs. Every other capability is a command in
        // this crate, which is why `capabilities/default.json` asked for
        // nothing until now: a native directory picker is the exception,
        // because no Rust command can draw the host's own file chooser.
        .plugin(tauri_plugin_dialog::init())
        .manage(Shell::default())
        .setup(|app| {
            // The shell's own bus, forwarded for the life of the window.
            //
            // Nothing did this, and nothing said so. `Bus::publish` counts its
            // subscribers and reports zero without complaining, which is what
            // it has to do - §36's isolation means a webview that has gone
            // must never take a capture down with it - so every event an
            // audition published went into an empty room. The window really
            // did play the record, with a `cpal` output stream open and audio
            // coming out of it, and the transport read `0:00.00 IDLE` the
            // whole way through: no `auditioning`, so nothing was playing as
            // far as the frontend knew, and no `playback-position`, so the
            // playhead never left zero. A device that refused a rate published
            // `playback-refused` into the same empty room, which is the half
            // of this that was a silent failure rather than a missing picture.
            //
            // Here rather than in `play`, and before the first command can
            // arrive, for the reason `arm` gives about the engine's bus: a
            // subscription taken out when an audition starts would miss
            // whatever that audition published before the reader was attached.
            // One pump, from startup, and it also carries the refusals.
            pump::forward(
                &app.handle().clone(),
                app.state::<Shell>().bus.subscribe(),
                "shell",
            );

            // The language, before the first command can produce a sentence.
            // Not fatal either way: a settings file that will not parse, or a
            // catalog that will not read, leaves the process speaking English
            // - which is the state it starts in - and says so in the log
            // rather than refusing to open a window over a translation.
            let handle = app.handle().clone();
            match config::load(&handle).and_then(|settings| config::speak(&settings)) {
                // The directory as well as the language: where a submitted
                // catalog goes is the first question a translator asks, and
                // the answer differs on three platforms.
                Ok(()) => tracing::info!(
                    locale = vcw_i18n::locale(),
                    catalogs = ?vcw_i18n::user_dir(),
                    "the language is set"
                ),
                Err(why) => tracing::warn!(%why, "staying in en-US"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            transport::arm,
            transport::transport,
            transport::poll,
            transport::open_project,
            library::about,
            library::support,
            library::devices,
            library::release,
            library::sides,
            library::tracks,
            library::captures,
            library::waveform,
            library::boundaries,
            config::settings,
            config::save_settings,
            config::languages,
            config::credentials,
            config::projects,
            config::new_project,
            config::library_root,
            config::artwork,
            config::open_path,
            edit::move_marker,
            edit::place_marker,
            edit::delete_marker,
            edit::lock_marker,
            edit::edit_track,
            edit::split_track,
            edit::merge_tracks,
            detect::detect_tracks,
            metadata::search_metadata,
            metadata::select_release,
            audition::play,
            audition::playback,
            exporter::export_plan,
            exporter::export_run,
        ])
        .run(tauri::generate_context!())
        .expect("the VCW window could not be created");
}

#[cfg(test)]
mod tests {
    use super::WIRED;

    /// Every `command` tag in the generated `Request` union.
    ///
    /// Read out of the TypeScript rather than listed here, because the
    /// TypeScript is generated from the type: a variant added to the contract
    /// appears in this set without anyone remembering to add it, which is the
    /// whole point of the test below.
    fn tags() -> Vec<String> {
        let generated = vcw_contract::bindings::typescript();
        let start = generated
            .find("export type Request =")
            .expect("the contract declares a Request union");
        let decl = &generated[start..];
        let end = decl.find(";").expect("a declaration ends");
        let mut found = Vec::new();
        let mut rest = &decl[..end];
        while let Some(at) = rest.find("\"command\": \"") {
            rest = &rest[at + 12..];
            let Some(close) = rest.find('"') else { break };
            found.push(rest[..close].to_owned());
            rest = &rest[close..];
        }
        found
    }

    #[test]
    fn the_union_has_tags_to_read() {
        // If this fails the parser above has stopped working and the real test
        // below would pass by finding nothing.
        let tags = tags();
        assert!(tags.len() >= 8, "only found {tags:?}");
        assert!(tags.contains(&"arm".to_owned()));
    }

    #[test]
    fn every_command_in_the_contract_is_wired() {
        for tag in tags() {
            assert!(
                WIRED.contains(&tag.as_str()),
                "{tag:?} is in the contract and nothing in the shell answers \
                 it. Wire it into `generate_handler!` and name it in `WIRED` - \
                 a command a frontend can send and nothing answers is the one \
                 outcome worth a failing test."
            );
        }
    }

    #[test]
    fn nothing_claims_to_be_wired_that_the_contract_does_not_declare() {
        let tags = tags();
        for name in &WIRED {
            assert!(
                tags.contains(&(*name).to_owned()),
                "{name:?} is not a command in the contract - renamed, or removed?"
            );
        }
    }
}
