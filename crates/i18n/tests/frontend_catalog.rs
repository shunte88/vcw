/*
 *  frontend_catalog.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Generates and drift-checks the source catalog the window is built with.
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

//! Generates and drift-checks the source catalog the window is built with.
//!
//! The window needs the English before it has asked the shell anything. Three
//! ways to give it that, and only one of them is free:
//!
//! - **Fetch it at startup.** Then the first paint is a grid of dotted keys,
//!   and every rendered test in `app/ui` needs a backend to say "Export".
//! - **Read `i18n/en-US.toml` from the frontend.** Vite's `fs.allow` denies
//!   reading above the project root, and widening a desktop app's dev-server
//!   allowlist to reach a sibling directory is the same bad trade WP-18
//!   refused for `docs/`.
//! - **Generate a TypeScript module and commit it.** Which is what D9 already
//!   does for the type bindings, with the same `VCW_BLESS` convention and the
//!   same drift test, so there is one habit to learn rather than two.
//!
//! The generated module is the *source* language only. A translation is a file
//! a user drops beside their settings long after this binary was built, so it
//! can only ever arrive at run time - `strings` serves it, and the window
//! overlays it on this.
//!
//! Regenerate with:
//!
//!     VCW_BLESS=1 cargo test -p vcw-i18n --test frontend_catalog
//!
//! and read the diff before committing it.

use std::path::{Path, PathBuf};

use vcw_i18n::Catalog;

/// Where the generated file is committed, relative to the repository root.
const PATH: &str = "app/ui/src/bindings/strings.ts";

/// The header written above the catalog.
///
/// No timestamp and no count: either one would make a regeneration a diff on
/// its own, and a drift check whose output moves by itself cannot be trusted
/// when it finally does move.
const HEADER: &str = "\
/*
 *  strings.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  GENERATED FILE - do not edit.
 *
 *  The source language, compiled into the window so the first paint is English
 *  rather than a grid of keys. Produced from `i18n/en-US.toml`, which is the
 *  authority. Regenerate with:
 *
 *      VCW_BLESS=1 cargo test -p vcw-i18n --test frontend_catalog
 *
 *  A translation is not here. It is a file a person drops beside their
 *  settings, served by the `strings` command and overlaid on this at run time
 *  - see `app/ui/src/i18n.ts`.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 *
 */

/** Every string VCW can say, in the language it is written in. */
export const SOURCE: Readonly<Record<string, string>> = {
";

/// The repository root, from this crate's manifest.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/i18n is two below the root")
        .to_path_buf()
}

/// The whole module, header and all.
fn typescript() -> String {
    let source = Catalog::source();
    let mut out = String::from(HEADER);
    for key in source.keys() {
        let text = source.get(key).expect("a key the catalog just listed");
        out.push_str("  ");
        out.push_str(&serde_json::to_string(key).expect("a string is JSON"));
        out.push_str(": ");
        out.push_str(&serde_json::to_string(text).expect("a string is JSON"));
        out.push_str(",\n");
    }
    out.push_str("};\n");
    out
}

#[test]
fn the_window_catalog_matches_the_toml() {
    let path = root().join(PATH);
    let generated = typescript();

    if std::env::var_os("VCW_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("making the directory");
        std::fs::write(&path, &generated).expect("writing the catalog");
        eprintln!("blessed {}", path.display());
        return;
    }

    let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} will not read ({error}). Generate it with \
             `VCW_BLESS=1 cargo test -p vcw-i18n --test frontend_catalog`",
            path.display()
        )
    });

    if committed != generated {
        let (line, want, got) = first_difference(&generated, &committed);
        panic!(
            "{} is out of date at line {line}\n  i18n/en-US.toml says: {want}\n  \
             the committed file says: {got}\n\
             Regenerate with `VCW_BLESS=1 cargo test -p vcw-i18n --test frontend_catalog`",
            path.display()
        );
    }
}

/// The first line that differs, one-based, with both sides.
///
/// A whole-file diff of a few hundred strings is unreadable in a test failure,
/// and the first difference is nearly always the only one that matters.
fn first_difference(left: &str, right: &str) -> (usize, String, String) {
    let mut lefts = left.lines();
    let mut rights = right.lines();
    let mut line = 0;
    loop {
        line += 1;
        match (lefts.next(), rights.next()) {
            (None, None) => return (line, "(end of file)".to_owned(), "(end of file)".to_owned()),
            (a, b) if a != b => {
                return (
                    line,
                    a.unwrap_or("(end of file)").to_owned(),
                    b.unwrap_or("(end of file)").to_owned(),
                );
            }
            _ => {}
        }
    }
}

#[test]
fn a_placeholder_is_a_name_the_window_can_substitute() {
    // `t` in `app/ui/src/i18n.ts` substitutes `{name}` and leaves anything it
    // has no argument for alone. A catalog entry whose braces hold something
    // that is not a name - a stray `{` in prose, a `{0}` from a `format!` that
    // was pasted in - would survive to the screen as itself, so it is refused
    // here rather than discovered by a translator.
    let source = Catalog::source();
    for key in source.keys() {
        let text = source.get(key).expect("a key the catalog just listed");
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let close = after.find('}').unwrap_or_else(|| {
                panic!("{key}: a `{{` with no `}}` after it - the window cannot substitute that")
            });
            let name = &after[..close];
            assert!(
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{key}: `{{{name}}}` is not a placeholder name - \
                 a placeholder is letters, digits and underscores"
            );
            rest = &after[close + 1..];
        }
    }
}
