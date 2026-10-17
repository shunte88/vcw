/*
 *  the_window_can_call_everything_it_calls.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Every command the frontend invokes is one the shell registered.
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

//! A command the frontend sends and nothing answers.
//!
//! `main.rs` already asserts that in one direction: `WIRED` is held against the
//! tags of the contract's `Request` union, both ways. But a *read* command has
//! no `Request` variant - `about`, `devices`, `update` and the rest change
//! nothing, so §35 keeps them out of it - and nothing was checking those at all.
//!
//! The failure is silent, which is what makes it worth a test. `invoke` rejects
//! with "Command x not found", the caller that does not want a toast swallows
//! it, and the feature is simply absent: the release check was added this way
//! and would have shown no button, no error and no log line.
//!
//! Read out of the two files rather than from a list here, because a third copy
//! of the command names is a third thing to forget.

use std::collections::BTreeSet;
use std::path::Path;

/// Every name `api.ts` hands to `invoke`.
fn invoked(api: &str) -> BTreeSet<String> {
    api.match_indices("invoke(\"")
        .filter_map(|(at, _)| {
            let rest = &api[at + "invoke(\"".len()..];
            rest.find('"').map(|end| rest[..end].to_owned())
        })
        .collect()
}

/// Every name in `generate_handler!`.
fn registered(main: &str) -> BTreeSet<String> {
    let at = main
        .find("generate_handler![")
        .expect("main.rs registers its commands with generate_handler!");
    let block = &main[at..];
    let block = &block[..block.find("])").expect("the macro's arguments end")];
    block
        .lines()
        .filter_map(|line| line.trim().strip_prefix("commands::"))
        .map(|name| name.trim_end_matches(',').to_owned())
        .collect()
}

#[test]
fn every_command_the_frontend_sends_is_one_the_shell_registered() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let api =
        std::fs::read_to_string(here.join("../ui/src/api.ts")).expect("api.ts is the boundary");
    let main = std::fs::read_to_string(here.join("src/main.rs")).expect("main.rs is this crate");

    let sent = invoked(&api);
    let answered = registered(&main);
    assert!(
        sent.len() > 30,
        "only {} invoke calls found - the scanner has stopped matching api.ts",
        sent.len()
    );

    let unanswered: Vec<&String> = sent.difference(&answered).collect();
    assert!(
        unanswered.is_empty(),
        "the window calls {unanswered:?} and `generate_handler!` does not list them, \
         which fails as a rejected promise the caller may never show"
    );
}
