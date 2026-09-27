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
//! - [`transport`], [`library`], [`audition`], [`edit`] and [`exporter`], which
//!   are §35's commands grouped by what they act on.
//!
//! Two of §35's verbs are declared in the contract and not wired here:
//! `search_metadata` and `select_release`. They need a provider client, a disk
//! cache, credentials and a cancellation path, which is WP-17's work, and a
//! shell command that quietly did nothing would be worse than one that refuses.
//! `refused` is that refusal, and the tests at the bottom of this file keep the
//! list of what is missing honest.

// A release build should not open a console window behind the app on Windows. In
// a debug build it should, because that is where a panic is printed.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audition;
mod edit;
mod exporter;
mod library;
mod pump;
mod state;
mod transport;

use state::{Error, Shell};

/// Commands the contract declares and the shell does not honour yet.
///
/// Named here rather than left out, so that `every_command_is_wired_or_named`
/// can assert the list against [`vcw_contract::Request`] and a new command
/// cannot be forgotten into silence.
pub(crate) const NOT_WIRED: [&str; 2] = ["search_metadata", "select_release"];

/// The commands in [`vcw_contract::Request`] that this shell honours.
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
pub(crate) const WIRED: [&str; 6] = ["arm", "transport", "play", "seek", "move_marker", "export"];

/// Refuses a command that is declared but not wired.
///
/// # Errors
///
/// Always, with the code `not-wired`.
#[tauri::command]
fn refused(command: String) -> Result<(), Error> {
    let known = NOT_WIRED.iter().find(|name| **name == command);
    Err(Error::NotWired {
        command: known.copied().unwrap_or("that command"),
    })
}

fn main() {
    tauri::Builder::default()
        .manage(Shell::default())
        .invoke_handler(tauri::generate_handler![
            transport::arm,
            transport::transport,
            transport::poll,
            transport::open_project,
            library::devices,
            library::release,
            library::sides,
            library::tracks,
            library::captures,
            library::waveform,
            edit::move_marker,
            audition::play,
            audition::playback,
            exporter::export_plan,
            exporter::export_run,
            refused,
        ])
        .run(tauri::generate_context!())
        .expect("the VCW window could not be created");
}

#[cfg(test)]
mod tests {
    use super::{NOT_WIRED, WIRED};

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
    fn every_command_is_wired_or_named() {
        for tag in tags() {
            let wired = WIRED.contains(&tag.as_str());
            let named = NOT_WIRED.contains(&tag.as_str());
            assert!(
                wired || named,
                "{tag:?} is in the contract and neither wired into \
                 `generate_handler!` nor listed in `NOT_WIRED`. Wire it, or say \
                 out loud that it is not wired - a command a frontend can send \
                 and nothing answers is the one outcome worth a failing test."
            );
            assert!(
                !(wired && named),
                "{tag:?} is listed as both wired and not wired"
            );
        }
    }

    #[test]
    fn nothing_claims_to_be_wired_that_the_contract_does_not_declare() {
        let tags = tags();
        for name in WIRED.iter().chain(NOT_WIRED.iter()) {
            assert!(
                tags.contains(&(*name).to_owned()),
                "{name:?} is not a command in the contract - renamed, or removed?"
            );
        }
    }

    #[test]
    fn a_refusal_says_which_command_and_why() {
        let error = super::refused("search_metadata".to_owned()).expect_err("not wired yet");
        assert_eq!(error.code(), "not-wired");
        assert!(
            error.to_string().contains("search_metadata"),
            "{error} does not name the command"
        );
    }
}
