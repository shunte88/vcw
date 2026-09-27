/*
 *  bindings.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The drift check: the committed TypeScript must match what the Rust types generate (D9).
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

//! The drift check: the committed TypeScript must match what the Rust types
//! generate (D9).
//!
//! The generated file is committed rather than built, because a frontend has to
//! typecheck without a Rust toolchain having run first and because a change to
//! the wire contract should be visible in a diff. That only works if the two can
//! never silently disagree, which is what this test is: it regenerates and
//! compares.
//!
//! When it fails, it is usually right. Rewrite the file with
//!
//! ```sh
//! VCW_BLESS=1 cargo test -p vcw-contract --test bindings
//! ```
//!
//! and read the diff before committing it. `VCW_BLESS` is the same convention
//! `docs/SCHEMA.md` uses, for the same reason: blessing is a decision, so it
//! takes a deliberate word.

use std::path::{Path, PathBuf};

use vcw_contract::bindings;

/// The repository root, from this crate's manifest.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/contract has two ancestors")
        .to_path_buf()
}

#[test]
fn the_typescript_matches_the_rust() {
    let path = root().join(bindings::PATH);
    let generated = bindings::typescript();

    if std::env::var_os("VCW_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("making the directory");
        std::fs::write(&path, &generated).expect("writing the bindings");
        eprintln!("blessed {}", path.display());
        return;
    }

    let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} could not be read ({error}). Generate it with \
             `VCW_BLESS=1 cargo test -p vcw-contract --test bindings`",
            path.display()
        )
    });

    if committed == generated {
        return;
    }

    // The first differing line rather than the whole file: the message goes to a
    // terminal, and "line 214 changed" is the useful part of a 400-line diff.
    let (line, was, now) = first_difference(&committed, &generated);
    panic!(
        "{} is out of date at line {line}:\n  committed: {was}\n  generated: {now}\n\
         Regenerate with `VCW_BLESS=1 cargo test -p vcw-contract --test bindings`",
        bindings::PATH
    );
}

/// The one-based line where two files first differ, and both sides of it.
fn first_difference(left: &str, right: &str) -> (usize, String, String) {
    let mut lefts = left.lines();
    let mut rights = right.lines();
    let mut line = 0;
    loop {
        line += 1;
        match (lefts.next(), rights.next()) {
            (None, None) => return (line, "<end of file>".into(), "<end of file>".into()),
            (a, b) if a == b => {}
            (a, b) => {
                return (
                    line,
                    a.unwrap_or("<end of file>").to_owned(),
                    b.unwrap_or("<end of file>").to_owned(),
                );
            }
        }
    }
}

#[test]
fn every_declaration_is_exported() {
    let generated = bindings::typescript();
    for line in generated.lines() {
        if line.starts_with("type ") || line.starts_with("interface ") {
            panic!("{line:?} is not exported, so a frontend cannot import it");
        }
    }
    assert!(
        generated.contains("export type Wire ="),
        "the event union is the one type a frontend cannot do without"
    );
}

#[test]
fn nothing_generated_is_a_bigint() {
    // A frame count crosses as a JSON number, because that is what `serde_json`
    // writes and what `JSON.parse` reads. `bigint` would typecheck and then fail
    // at run time on the first event, so the config that prevents it is worth a
    // test of its own.
    let generated = bindings::typescript();
    assert!(
        !generated.contains("bigint"),
        "a u64 rendered as bigint, so `with_large_int` was lost"
    );
}

#[test]
fn no_pcm_crosses_the_boundary() {
    // §35: high-frequency PCM shall never cross the Rust/JavaScript boundary. A
    // waveform's three arrays are the drawn summary - one float per drawn
    // column, not per frame - and this test fails if something ever adds a
    // field of raw audio beside them.
    //
    // Field names rather than a substring search, because the prose above a
    // declaration is allowed to say "samples" and `clippedSamples` is a count.
    let generated = bindings::typescript();
    let banned = ["samples", "pcm", "audio", "buffer", "bytes", "blob"];
    for line in generated.lines() {
        let line = line.trim();
        if line.starts_with('*') || line.starts_with('/') {
            continue;
        }
        let Some(name) = line.split(':').next().map(str::trim) else {
            continue;
        };
        assert!(
            !banned.contains(&name),
            "the field {name:?} appears in the contract, which §35 forbids"
        );
        for array in ["Int16Array", "Float32Array", "Uint8Array", "ArrayBuffer"] {
            assert!(
                !line.contains(array),
                "{array} appears in the contract: a typed array is PCM by another name"
            );
        }
    }
}

#[test]
fn the_file_has_no_trailing_whitespace() {
    // Not cosmetic: the file is committed, and an editor that strips trailing
    // space on save would turn every future diff into a whitespace diff and
    // fail the drift check on a file nobody edited.
    for (number, line) in bindings::typescript().lines().enumerate() {
        assert_eq!(
            line.trim_end(),
            line,
            "line {} ends in whitespace",
            number + 1
        );
    }
}
