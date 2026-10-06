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

use std::collections::BTreeMap;
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
            // Both ran out, so the files differ somewhere `lines()` cannot
            // see: a line ending, or a trailing newline. Saying "out of date
            // at line 1630: committed <end of file>, generated <end of file>"
            // describes nothing, and that is the message a Windows runner
            // printed when its checkout arrived as CRLF.
            (None, None) => {
                return (
                    line,
                    format!(
                        "<{} bytes, every line equal - check line endings>",
                        left.len()
                    ),
                    format!(
                        "<{} bytes, every line equal - check line endings>",
                        right.len()
                    ),
                );
            }
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
    // The rule is a banned *name* on a field whose *type is a list of numbers*,
    // and it took both halves to be right. Names alone rejected §39's
    // `settings.audio` group and the browser's `bytes` file size, neither of
    // which is a sample; types alone would allow `Array<number>` under any
    // name at all. PCM is a list of numbers, so the conjunction is the thing
    // being described - and the prose above a declaration stays free to say
    // "samples", which it needs to, because `clippedSamples` is a count.
    let generated = bindings::typescript();
    let banned = [
        "samples", "pcm", "audio", "buffer", "bytes", "blob", "frames",
    ];
    for line in generated.lines() {
        let line = line.trim();
        if line.starts_with('*') || line.starts_with('/') {
            continue;
        }
        let Some((name, declared)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let numeric_list = declared.contains("Array<number>") || declared.contains("number[]");
        assert!(
            !(banned.contains(&name) && numeric_list),
            "the field {name:?} crosses as a list of numbers, which \u{a7}35 forbids"
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
fn no_declaration_is_declared_twice() {
    // One file is one flat namespace, and two modules with the same type name
    // are perfectly legal Rust. `settings::Export` and `command::Export` both
    // rendered as `export type Export` and the second shadowed the first, in a
    // file that had already been blessed and committed - `cargo test` was
    // green, because the clash does not exist on the Rust side at all.
    //
    // This is the cost of assembling one file rather than taking ts-rs's
    // file-per-type export, which would have put them in separate modules and
    // let both stand. It is still the right trade - a deleted type is the case
    // a directory comparison gets wrong - but it needs this test to be paid
    // for, and the remedy is `#[ts(rename = "...")]`, not a rename in Rust.
    let generated = bindings::typescript();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for line in generated.lines() {
        let Some(rest) = line.strip_prefix("export type ") else {
            continue;
        };
        let Some(name) = rest.split_whitespace().next() else {
            continue;
        };
        *seen.entry(name).or_default() += 1;
    }

    let twice: Vec<_> = seen
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(name, count)| format!("{name} x{count}"))
        .collect();
    assert!(
        twice.is_empty(),
        "declared more than once, so the later one shadows the earlier: {}",
        twice.join(", ")
    );
    assert!(seen.len() >= 30, "only found {} declarations", seen.len());
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
