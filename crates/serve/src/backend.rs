/*
 *  backend.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Every command name the frontend can send, answered without a window.
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
//! Every command name the frontend can send, answered without a window (§52).
//!
//! The desktop shell's `commands.rs` and this file are the two hosts of
//! `vcw-shell`, and they do the same job by different means. Tauri reads a
//! command's arguments out of the invoke payload by parameter name and
//! serializes what comes back; here that is written out, because an HTTP body
//! is a string and nothing is generating the glue.
//!
//! The names and the argument keys are the frontend's, not this crate's.
//! `app/ui/src/api.ts` is the whole list and `invoke("waveform", { zoom })` is
//! the shape, so a key here is the key the webview sends. Two of them are not
//! what the Rust parameter is called: Tauri converts `captureId` to
//! `capture_id` on the way in and nothing does that here, so `play` reads the
//! name the browser actually sent.
//!
//! A command nobody wrote a line for is a refusal and not a panic. The list is
//! held to the contract by `every_command_the_frontend_can_send_is_answered`
//! below, which reads `api.ts` - the same trick `vcw-app` plays against the
//! generated `Request` union, for the same reason: a command a frontend can
//! send and nothing answers is worse than one that refuses out loud.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde::de::DeserializeOwned;
use vcw_contract::command::Failure;
use vcw_contract::event::Wire;
use vcw_shell::{Host, Hosted, Shell};

use crate::server::Backend;

/// Everything §52 serves: one shell, and whoever is watching it.
pub struct Served {
    shell: Shell,
    fanout: Arc<Fanout>,
    host: Hosted,
}

impl Served {
    /// A shell with nothing open and nobody watching.
    ///
    /// `files` is the directory §52's path browser is confined to - the one
    /// the operator named when `serve` started. `None` turns the browser off
    /// rather than opening the filesystem.
    #[must_use]
    pub fn new(files: Option<PathBuf>) -> Self {
        let fanout = Arc::new(Fanout {
            files,
            ..Fanout::default()
        });
        let host: Hosted = fanout.clone();
        Self {
            shell: Shell::default(),
            fanout,
            host,
        }
    }

    /// Starts forwarding the shell's own bus.
    ///
    /// Separate from [`Self::new`] and called once by whoever is serving, for
    /// the reason the window's `setup` gives: a subscription taken out when
    /// the first audition starts would miss whatever that audition published
    /// before the reader was attached, and a bus with no reader is silent
    /// without complaining.
    pub fn pump(&self) {
        vcw_shell::pump::forward(&self.host, self.shell.bus.subscribe(), "shell");
    }
}

impl Default for Served {
    fn default() -> Self {
        Self::new(None)
    }
}

/// Everyone attached right now.
///
/// A `Vec` of senders behind a mutex, swept on every event. There is no
/// bounded channel and no coalescing here because there is none in the window
/// either - S3 measured both and found sending every frame cheaper than
/// deciding not to.
///
/// ponytail: a 60 Hz meter event locks this once and walks the list. With one
/// or two browsers that is nothing; if §52 ever has to feed a room full of
/// them, a broadcast channel is the upgrade.
#[derive(Default)]
struct Fanout {
    watching: Mutex<Vec<Sender<String>>>,
    /// The fence around the path browser (§52), or `None` for no browser.
    files: Option<PathBuf>,
}

impl Fanout {
    /// One more browser.
    fn attach(&self) -> Receiver<String> {
        let (sender, receiver) = channel();
        self.watching
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(sender);
        receiver
    }
}

impl Host for Fanout {
    /// Hands the event to every attached browser, and forgets the ones that
    /// have gone.
    ///
    /// Always `true`. Nobody watching is an ordinary state here and not a
    /// closed window: a capture started from one browser has to keep running
    /// while its tab is reloading, so the event is dropped and the pump lives.
    fn emit(&self, wire: &Wire) -> bool {
        let Ok(line) = serde_json::to_string(wire) else {
            // A `Wire` that will not serialize is a bug in `vcw-contract`
            // rather than a reason to take the pump down. Nothing can be sent,
            // so nothing is.
            return true;
        };
        let mut watching = self
            .watching
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        watching.retain(|browser| browser.send(line.clone()).is_ok());
        true
    }

    fn config_dir(&self) -> Option<PathBuf> {
        vcw_i18n::config_dir()
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        vcw_i18n::cache_dir()
    }

    fn browse_root(&self) -> Option<PathBuf> {
        self.files.clone()
    }
}

impl Backend for Served {
    fn subscribe(&self) -> Receiver<String> {
        self.fanout.attach()
    }

    fn call(&self, name: &str, arguments: &str) -> Result<String, String> {
        let shell = &self.shell;
        let host = &self.host;
        let args = Arguments(arguments);
        match name {
            // Capture.
            "arm" => done(vcw_shell::transport::arm(shell, host, args.get("arm")?)),
            "transport" => done(vcw_shell::transport::transport(shell, args.get("verb")?)),
            "poll" => done(vcw_shell::transport::poll(shell)),
            "open_project" => done(vcw_shell::transport::open_project(shell, args.get("path")?)),
            // Reading what is open.
            "about" => ok(&vcw_shell::library::about()),
            "support" => done(vcw_shell::library::support(args.get("page")?)),
            "devices" => ok(&vcw_shell::library::devices()),
            "release" => done(vcw_shell::library::release(shell)),
            "sides" => done(vcw_shell::library::sides(shell)),
            "tracks" => done(vcw_shell::library::tracks(shell)),
            "captures" => done(vcw_shell::library::captures(shell)),
            "waveform" => done(vcw_shell::library::waveform(shell, args.get("zoom")?)),
            "boundaries" => done(vcw_shell::library::boundaries(shell)),
            // Settings and the library.
            "settings" => done(vcw_shell::config::settings(host)),
            "save_settings" => done(vcw_shell::config::save_settings(
                host,
                args.get("settings")?,
            )),
            "languages" => ok(&vcw_shell::config::languages()),
            "credentials" => ok(&vcw_shell::config::credentials()),
            "projects" => done(vcw_shell::config::projects(host)),
            "new_project" => done(vcw_shell::config::new_project(host, args.get("seed")?)),
            "library_root" => done(vcw_shell::config::library_root(host)),
            "browse" => done(vcw_shell::config::browse(host, args.get("at")?)),
            "artwork" => done(vcw_shell::config::artwork(args.get("path")?)),
            "open_path" => ok(&vcw_shell::config::open_path(shell)),
            // Editing.
            "move_marker" => done(vcw_shell::edit::move_marker(shell, args.get("marker")?)),
            "place_marker" => done(vcw_shell::edit::place_marker(shell, args.get("placement")?)),
            "delete_marker" => done(vcw_shell::edit::delete_marker(shell, args.get("removal")?)),
            "lock_marker" => done(vcw_shell::edit::lock_marker(shell, args.get("lock")?)),
            "edit_track" => done(vcw_shell::edit::edit_track(shell, args.get("edit")?)),
            "split_track" => done(vcw_shell::edit::split_track(shell, args.get("split")?)),
            "merge_tracks" => done(vcw_shell::edit::merge_tracks(shell, args.get("merge")?)),
            "detect_tracks" => done(vcw_shell::detect::detect_tracks(
                shell,
                host,
                args.get("detect")?,
            )),
            // The network.
            "search_metadata" => done(vcw_shell::metadata::search_metadata(
                shell,
                host,
                args.get("search")?,
            )),
            "select_release" => done(vcw_shell::metadata::select_release(
                shell,
                host,
                args.get("selection")?,
            )),
            // Playback and export.
            "play" => done(vcw_shell::audition::play(
                shell,
                args.get("captureId")?,
                args.get("scope")?,
                args.get("device")?,
            )),
            "playback" => done(vcw_shell::audition::playback(shell, args.get("verb")?)),
            "export_plan" => done(vcw_shell::exporter::export_plan(shell, args.get("export")?)),
            "export_run" => done(vcw_shell::exporter::export_run(
                shell,
                host,
                args.get("export")?,
            )),
            _ => Err(refusal(
                "command",
                None,
                &format!("{name} is not a command this VCW answers"),
            )),
        }
    }
}

/// One command's JSON payload.
struct Arguments<'a>(&'a str);

impl Arguments<'_> {
    /// The argument under `key`, or a refusal naming it.
    ///
    /// An absent key reads as `null`, which is what makes `device: null` and
    /// an omitted `device` the same request - the frontend sends the first and
    /// a `curl` by hand sends the second.
    fn get<T: DeserializeOwned>(&self, key: &str) -> Result<T, String> {
        let whole: serde_json::Value = serde_json::from_str(self.0)
            .map_err(|why| refusal(key, None, &format!("the arguments are not JSON: {why}")))?;
        let value = whole.get(key).cloned().unwrap_or(serde_json::Value::Null);
        serde_json::from_value(value)
            .map_err(|why| refusal(key, Some(key), &format!("{key} is not usable: {why}")))
    }
}

/// A command that returned a value.
fn ok<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|why| {
        refusal(
            "response",
            None,
            &format!("the answer would not serialize: {why}"),
        )
    })
}

/// A command that could also have refused.
///
/// The refusal crosses as a `Failure` because [`vcw_shell::Error`] serializes
/// as one, which is the same shape `invoke` rejects with in the window.
fn done<T: Serialize>(outcome: Result<T, vcw_shell::Error>) -> Result<String, String> {
    match outcome {
        Ok(value) => ok(&value),
        Err(error) => Err(serde_json::to_string(&error)
            .unwrap_or_else(|_| refusal("command", None, &error.to_string()))),
    }
}

/// A refusal this file raised itself, in the shape the frontend expects.
fn refusal(code: &str, field: Option<&str>, message: &str) -> String {
    let failure = Failure {
        code: code.to_owned(),
        message: message.to_owned(),
        field: field.map(str::to_owned),
    };
    serde_json::to_string(&failure).unwrap_or_else(|_| {
        // Three owned strings and three string fields. Unreachable, and the
        // fallback is still JSON so the frontend's `asFailure` has something
        // to read.
        r#"{"code":"command","message":"refused","field":null}"#.to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command name `api.ts` can send.
    ///
    /// Read out of the frontend rather than listed here, for the reason
    /// `vcw-app`'s `WIRED` test gives: a list beside the thing it describes is
    /// a second copy that drifts. The path is relative to this crate because
    /// `CARGO_MANIFEST_DIR` is the only fixed point a test has.
    fn asked_for() -> Vec<String> {
        let source = include_str!("../../../app/ui/src/api.ts");
        let mut found = Vec::new();
        let mut rest = source;
        while let Some(at) = rest.find("invoke(\"") {
            rest = &rest[at + 8..];
            let Some(close) = rest.find('"') else { break };
            found.push(rest[..close].to_owned());
            rest = &rest[close..];
        }
        found
    }

    /// Every command name this file answers.
    fn answered() -> Vec<String> {
        let source = include_str!("backend.rs");
        let body = source
            .split_once("match name {")
            .expect("the dispatch is a match")
            .1;
        let body = body.split_once("\n        }\n").expect("it ends").0;
        let mut found = Vec::new();
        for line in body.lines() {
            let line = line.trim();
            // `"name" =>`, and nothing else. A bare string on its own line
            // is an argument to a refusal, not an arm.
            if let Some(rest) = line.strip_prefix('"')
                && let Some((name, after)) = rest.split_once('"')
                && after.starts_with(" =>")
            {
                found.push(name.to_owned());
            }
        }
        found
    }

    #[test]
    fn there_are_commands_to_read_on_both_sides() {
        // If this fails one of the two parsers above has stopped working and
        // the real tests below would pass by finding nothing.
        assert!(asked_for().len() >= 30, "{:?}", asked_for());
        assert!(answered().len() >= 30, "{:?}", answered());
        assert!(asked_for().contains(&"arm".to_owned()));
        assert!(answered().contains(&"arm".to_owned()));
    }

    #[test]
    fn every_command_the_frontend_can_send_is_answered() {
        for name in asked_for() {
            assert!(
                answered().contains(&name),
                "{name:?} is a command `api.ts` sends and §52's listener has \
                 no arm for it. Over HTTP it would refuse where the window \
                 works, which is the one difference between the two hosts \
                 that is never acceptable."
            );
        }
    }

    #[test]
    fn nothing_is_answered_that_the_frontend_does_not_send() {
        for name in answered() {
            assert!(
                asked_for().contains(&name),
                "{name:?} is answered here and nothing sends it - renamed, or \
                 removed?"
            );
        }
    }

    #[test]
    fn an_unknown_command_refuses_in_the_shape_the_frontend_reads() {
        let served = Served::new(None);
        let refused = served.call("nonesuch", "{}").expect_err("a refusal");
        let failure: Failure = serde_json::from_str(&refused).expect("a Failure");
        assert_eq!(failure.code, "command");
        assert!(failure.message.contains("nonesuch"), "{}", failure.message);
    }

    #[test]
    fn an_argument_that_is_not_what_it_should_be_names_the_field() {
        let served = Served::new(None);
        let refused = served
            .call("open_project", r#"{"path": 7}"#)
            .expect_err("a refusal");
        let failure: Failure = serde_json::from_str(&refused).expect("a Failure");
        assert_eq!(failure.field.as_deref(), Some("path"));
    }

    #[test]
    fn a_command_that_needs_no_project_answers_without_one() {
        let served = Served::new(None);
        let answer = served.call("about", "{}").expect("about always answers");
        assert!(answer.contains("version"), "{answer}");
    }

    #[test]
    fn a_command_that_needs_a_project_refuses_as_the_window_would() {
        let served = Served::new(None);
        let refused = served.call("tracks", "{}").expect_err("nothing is open");
        let failure: Failure = serde_json::from_str(&refused).expect("a Failure");
        assert_eq!(failure.code, "no-project");
    }

    #[test]
    fn an_event_reaches_every_attached_browser_and_no_detached_one() {
        let served = Served::new(None);
        let first = served.subscribe();
        let second = served.subscribe();
        drop(served.subscribe());

        let moved = Wire::PhaseChange {
            from: vcw_contract::event::PhaseName::Idle,
            to: vcw_contract::event::PhaseName::Armed,
        };
        assert!(served.fanout.emit(&moved));
        assert!(
            first
                .try_recv()
                .expect("the first browser")
                .contains("kind")
        );
        assert!(second.try_recv().is_ok(), "the second browser");

        // The dropped one was swept on the way past.
        assert_eq!(served.fanout.watching.lock().expect("the fanout").len(), 2);
    }
}
