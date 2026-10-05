/*
 *  no_logging_on_the_audio_thread.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §42's rule that routine audio callbacks do not log, checked against the source.
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
//! §42's rule that routine audio callbacks do not log, checked against the
//! source.
//!
//! "Routine audio callbacks shall not log" cannot be tested by running
//! anything. A log line on the audio thread does not fail, it makes a click:
//! `tracing` takes a lock to reach a writer and the writer touches a file
//! descriptor, and a callback that blocks for a millisecond at 192 kHz has
//! already missed its deadline. The defect is silent, intermittent, and shows up
//! as a complaint about a rip weeks later.
//!
//! So this test reads the source. That is an unusual thing for a test to do and
//! it is the honest mechanism here: the property is "this code contains no call
//! to this family of macros", which is a property of the text.
//!
//! Two regions are covered:
//!
//! * The whole of the modules the callback runs *through* - the ring, the chunk
//!   pool, the converter. Nothing in them has any business logging, so the rule
//!   there is total and needs no parsing.
//! * The bodies of the two callback entry points, `Sink::on_data` and
//!   `Render::on_data`, inside modules that legitimately log elsewhere: opening
//!   a stream, the host's error callback, the counters at stop.
//!
//! What this cannot see is a helper called from `on_data` that logs. That gap is
//! the reason the first bullet exists: the modules the callback reaches into are
//! banned wholesale rather than checked function by function.

use std::path::{Path, PathBuf};

/// The macros that would put a log call on the audio thread.
///
/// `println!` and `eprintln!` are in the list because they are logging by
/// another name, and a `dbg!` left behind is worse than either: it writes to
/// stderr on every callback.
const FORBIDDEN: [&str; 10] = [
    "trace!",
    "debug!",
    "info!",
    "warn!",
    "error!",
    "event!",
    "span!",
    "println!",
    "eprintln!",
    "dbg!",
];

/// Modules the capture and playback callbacks run through, which may not log at
/// all.
const CALLBACK_PATH: [&str; 4] = ["buffers.rs", "chunks.rs", "convert.rs", "source.rs"];

/// The callback entry points, as (file, function) pairs.
const ENTRY_POINTS: [(&str, &str); 2] = [
    ("capture.rs", "pub fn on_data(&mut self, bytes: &[u8])"),
    ("playback.rs", "pub fn on_data(&mut self, out: &mut [u8])"),
];

fn source(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name);
    assert!(path.is_file(), "{} is missing", path.display());
    path
}

/// The body of a function, by brace matching from its signature.
///
/// Crude on purpose. A brace inside a string literal or a comment would throw
/// it off, and the answer to that is not a parser in a test: it is that if this
/// ever stops finding the function it fails loudly rather than passing on an
/// empty string, which is the failure mode that would matter.
fn body(text: &str, signature: &str) -> String {
    let at = text
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} is not in this file any more; fix this test"));
    let rest = &text[at..];
    let open = rest
        .find('{')
        .unwrap_or_else(|| panic!("no body after {signature}"));
    let mut depth = 0_i32;
    for (offset, character) in rest[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let found = &rest[open..open + offset + 1];
                    assert!(
                        found.len() > 40,
                        "the body found for {signature} is {} characters, which is not a \
                         function body; the brace matching in this test is broken",
                        found.len()
                    );
                    return found.to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after {signature}");
}

/// Every forbidden macro in a piece of source, with the line it is on.
fn offenses(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let code = line.split_once("//").map_or(*line, |(before, _)| before);
            FORBIDDEN.iter().any(|macro_name| code.contains(macro_name))
        })
        .map(|(number, line)| (number + 1, line.trim().to_owned()))
        .collect()
}

#[test]
fn the_modules_the_callback_runs_through_do_not_log() {
    for name in CALLBACK_PATH {
        let text = std::fs::read_to_string(source(name)).expect("read the module");
        let found = offenses(&text);
        assert!(
            found.is_empty(),
            "{name} is on the audio thread's path and may not log (§42):\n{}",
            found
                .iter()
                .map(|(line, text)| format!("  {name}:{line}  {text}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

#[test]
fn the_callbacks_themselves_do_not_log() {
    for (name, signature) in ENTRY_POINTS {
        let text = std::fs::read_to_string(source(name)).expect("read the module");
        let found = offenses(&body(&text, signature));
        assert!(
            found.is_empty(),
            "{name}'s {signature} runs on the audio thread and may not log (§42):\n{}",
            found
                .iter()
                .map(|(_, text)| format!("  {text}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

#[test]
fn the_rest_of_the_crate_does_log() {
    // The other end of the gate. Two assertions above would both pass on a crate
    // that had no logging in it at all, and a rule that is satisfied by doing
    // nothing is not a rule - so this is the part that proves the check above is
    // measuring a boundary rather than an absence.
    let mut logged = 0;
    for name in ["capture.rs", "playback.rs"] {
        let text = std::fs::read_to_string(source(name)).expect("read the module");
        logged += text.matches("tracing::").count();
    }
    assert!(
        logged >= 4,
        "only {logged} log calls in capture.rs and playback.rs; §42 asks for \
         structured logging, and a crate that logs nowhere passes the callback \
         checks for the wrong reason"
    );
}
