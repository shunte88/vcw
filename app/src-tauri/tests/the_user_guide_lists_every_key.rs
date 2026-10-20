/*
 *  app/src-tauri/tests/the_user_guide_lists_every_key.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Every shortcut the user guide prints is one the window binds (§43, §44).
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

//! Every shortcut the user guide prints is one the window binds (§43, §44).
//!
//! `docs/VCW-User-Guide/VCW-User-Guide.md` prints an "everyday shortcuts"
//! table. It is authored by hand, in three formats kept in step outside this
//! repository, so nothing here may rewrite it - but a key printed in it that
//! the window ignores is still a defect, and this is the check that finds one:
//! parse `app/ui/src/keymap.ts`, parse that table, and require every chord in
//! the table to be a chord the keymap binds.
//!
//! It is a Rust test reading a TypeScript file, which looks like the wrong
//! language for the job. The UI's own test runner cannot do it: vite's
//! `server.fs.allow` denies reading `docs/`, and loosening a desktop
//! application's dev-server allowlist to make a documentation check possible
//! would be a poor trade. This crate's tests run in the same gate leg and can
//! read both files, so the check lives here.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// One row of the map, as both files have to describe it.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Row {
    scope: String,
    keys: String,
    label: String,
    workflow: String,
    suggested: bool,
}

/// One entry of `BINDINGS`, before it is rendered as a row.
#[derive(Default)]
struct Binding {
    name: String,
    chord: String,
    also: String,
    workflow: String,
    scope: String,
    label: String,
    suggested: bool,
}

impl Binding {
    fn row(self) -> Row {
        assert!(
            !self.chord.is_empty()
                && !self.workflow.is_empty()
                && !self.scope.is_empty()
                && !self.label.is_empty(),
            "binding `{}` is missing a field",
            self.name,
        );
        let keys = if self.also.is_empty() {
            format!("`{}`", self.chord)
        } else {
            format!("`{}` or `{}`", self.chord, self.also)
        };
        Row {
            scope: self.scope,
            keys,
            label: self.label,
            workflow: self.workflow,
            suggested: self.suggested,
        }
    }
}

fn repo() -> PathBuf {
    // `app/src-tauri` -> `app` -> the repository.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|app| app.parent())
        .expect("the crate is two levels below the repository root")
        .to_owned()
}

fn read(relative: &str) -> String {
    let path = repo().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The `key` and `value` of a `key: "value",` or `key: true,` line.
fn field(line: &str) -> Option<(&str, &str)> {
    let (name, rest) = line.split_once(':')?;
    let value = rest.trim().trim_end_matches(',').trim_matches('"');
    Some((name.trim(), value))
}

/// The English a `label:` holds, which is written as a catalog key.
///
/// `keymap.ts` gives every binding `key("keys.play")` rather than the sentence
/// itself, so that the coverage test in `crates/i18n/tests/callers.rs` can see
/// the key is asked for. The guide prints what a person reads, so the key is
/// resolved here - and one the catalog has never heard of fails this test
/// rather than putting a dotted path in the documentation.
fn label(value: &str) -> String {
    let Some(key) = value
        .strip_prefix("key(\"")
        .and_then(|rest| rest.strip_suffix("\")"))
    else {
        return value.to_owned();
    };
    vcw_i18n::Catalog::source()
        .get(key)
        .unwrap_or_else(|| {
            panic!("keymap.ts asks for `{key}`, which i18n/en-US.toml does not have")
        })
        .to_owned()
}

/// Every binding in `keymap.ts`, read as data.
///
/// Hand-parsed rather than pattern-matched. A regular expression over this file
/// was the first attempt and it silently read 38 of the 40 bindings, because the
/// two that carry an `also:` field did not match the shape it expected. A drift
/// check that quietly ignores two rows is worse than no drift check, so this
/// walks the braces instead and panics on any line it cannot place.
fn bindings() -> BTreeSet<Row> {
    let text = read("app/ui/src/keymap.ts");
    let body = text
        .split_once("export const BINDINGS = {")
        .expect("keymap.ts declares BINDINGS")
        .1;

    let mut rows = BTreeSet::new();
    let mut open: Option<Binding> = None;
    for line in body.lines() {
        let plain = line.trim();
        if plain.is_empty() || plain.starts_with("//") {
            continue;
        }
        let Some(entry) = open.as_mut() else {
            // `}` on its own closes the declaration; `name: {` opens an entry.
            if plain.starts_with('}') {
                break;
            }
            if let Some(name) = plain.strip_suffix(": {") {
                open = Some(Binding {
                    name: name.to_owned(),
                    ..Binding::default()
                });
            }
            continue;
        };

        if plain == "}," {
            let done = open.take().expect("an entry is open");
            let row = done.row();
            assert!(rows.insert(row), "the keymap holds the same row twice");
            continue;
        }

        let (name, value) = field(plain).unwrap_or_else(|| panic!("unreadable line: {plain}"));
        match name {
            "chord" => entry.chord = value.to_owned(),
            "also" => entry.also = value.to_owned(),
            "workflow" => entry.workflow = value.to_owned(),
            "scope" => entry.scope = value.to_owned(),
            "label" => entry.label = label(value),
            "suggested" => entry.suggested = value == "true",
            other => panic!("unknown field `{other}` in binding `{}`", entry.name),
        }
    }

    assert!(open.is_none(), "an entry in the keymap was never closed");
    assert!(rows.len() > 30, "only {} bindings parsed", rows.len());
    rows
}

/// Every key the guide's shortcut table names, as the keymap would spell it.
///
/// The guide writes what is printed on the key cap and the keymap writes what
/// the DOM calls it, so `Ctrl+Left` here and `Ctrl+ArrowLeft` there are the
/// same chord. Folding the case and expanding the four arrows is the whole
/// bridge between them; anything else that differs is a real disagreement.
fn named_in_the_guide() -> BTreeSet<String> {
    let text = read("docs/VCW-User-Guide/VCW-User-Guide.md");
    let table = text
        .split_once("### Everyday shortcuts")
        .expect("the guide has an everyday shortcuts table")
        .1;

    let mut keys = BTreeSet::new();
    for line in table.lines() {
        if line.starts_with("## ") || line.starts_with("### ") {
            break; // the next heading; the table is over.
        }
        if !line.starts_with("| ") || line.starts_with("| --- ") || line.starts_with("| Key ") {
            continue;
        }
        let cell = line
            .trim()
            .trim_matches('|')
            .split('|')
            .next()
            .expect("a first cell");
        // The cell is one or more backticked chords and some connecting
        // English: "`r` / `s`", "`a` in Capture", "`?` or `F1`". Every odd
        // piece of a split on the backtick is a chord.
        for chord in cell.split('`').skip(1).step_by(2) {
            keys.insert(normalize(chord));
        }
    }
    assert!(keys.len() >= 10, "only {} shortcuts parsed", keys.len());
    keys
}

/// One spelling for a chord, whichever of the two files it came from.
fn normalize(chord: &str) -> String {
    let lower = chord.trim().to_lowercase();
    if lower.contains("arrow") {
        return lower;
    }
    for arrow in ["left", "right", "up", "down"] {
        if let Some(modifiers) = lower.strip_suffix(arrow) {
            return format!("{modifiers}arrow{arrow}");
        }
    }
    lower
}

#[test]
fn every_shortcut_the_guide_names_is_one_the_window_binds() {
    // The guide is authored by hand and kept in three formats outside this
    // repository, so this check cannot regenerate anything - it can only say
    // that a chord printed in it is a chord the window answers to. That is the
    // failure that matters to a reader: a shortcut table is read while the
    // application is open, and a key that does nothing is worse than a key
    // that is missing from the table.
    //
    // Deliberately one-directional. The old guide's table was generated from
    // `BINDINGS` and this test required the two to match exactly; the guide
    // that replaced it offers "everyday" shortcuts and says the in-app map
    // (`?`) is the reference for the full set, so a binding absent from the
    // table is an editorial choice rather than drift.
    let bound: BTreeSet<String> = bindings()
        .iter()
        .flat_map(|row| {
            row.keys
                .split('`')
                .skip(1)
                .step_by(2)
                .map(normalize)
                .collect::<Vec<_>>()
        })
        .collect();

    let named = named_in_the_guide();
    let unbound: Vec<&String> = named.difference(&bound).collect();
    assert!(
        unbound.is_empty(),
        "the guide's shortcut table names keys app/ui/src/keymap.ts does not bind: {unbound:?}\n\
         bound: {bound:?}",
    );
}

#[test]
fn the_keymap_says_which_keys_section_43_suggested() {
    // §43 offers a set of defaults rather than requiring them, and the map
    // departs from it. The column recording that is only useful if it is not
    // uniform, which is a property of the data and worth asserting: a table of
    // all-yes or all-dash would mean the column had stopped being read.
    let bound = bindings();
    let suggested = bound.iter().filter(|row| row.suggested).count();
    assert!(
        suggested > 0 && suggested < bound.len(),
        "{suggested} of {} bindings are §43 defaults",
        bound.len(),
    );
}
