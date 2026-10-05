/*
 *  tokens.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  That the interface and the expander agree on what a token is.
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

//! The naming template's token list exists twice: once in
//! [`vcw_export::naming::TOKENS`], which expands it, and once in
//! `app/ui/src/template.ts`, which tells a person typing a template that they
//! have misspelled one. The second copy is there so the settings field can
//! answer without a round trip to the shell.
//!
//! Two lists that must agree and nothing making them agree is a bug waiting for
//! the next token to be added, so this is the thing that makes them agree. It
//! reads the TypeScript rather than generating it because the generated file
//! would still need checking in, and a test that fails in the gate is cheaper
//! than a build step in a workspace that has no build step.

use std::path::PathBuf;

/// The tokens as the interface lists them.
fn from_the_interface() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/ui/src/template.ts");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|why| panic!("cannot read {}: {why}", path.display()));
    let (_, rest) = source
        .split_once("export const TOKENS")
        .expect("template.ts no longer declares TOKENS");
    let (list, _) = rest
        .split_once("];")
        .expect("TOKENS is no longer a bracketed list");
    list.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_interface_lists_the_tokens_the_expander_knows() {
    let theirs = from_the_interface();
    assert!(
        !theirs.is_empty(),
        "read no tokens at all out of template.ts, so this test is vacuous"
    );
    let ours: Vec<String> = vcw_export::naming::TOKENS
        .iter()
        .map(|token| (*token).to_owned())
        .collect();
    assert_eq!(
        theirs, ours,
        "app/ui/src/template.ts and naming::TOKENS disagree. A token only one \
         of them has is either rejected by a field that should accept it or \
         accepted by a field and then refused by the export"
    );
}
