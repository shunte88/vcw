/*
 *  app/src-tauri/tests/the_user_guide_lists_every_key.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The keyboard table in the user guide is the keymap, or the build fails (§43, §44).
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

//! The keyboard table in the user guide is the keymap, or the build fails
//! (§43, §44).
//!
//! `docs/USER-GUIDE.md` claims that its keyboard table is generated from the
//! application's own binding table "rather than transcribed, so it cannot drift
//! from what the window actually does". That claim is worth exactly as much as
//! the check behind it, so here is the check: parse `app/ui/src/keymap.ts`,
//! parse the guide's tables, and require the two to agree on every chord, every
//! label, every scope and every §43 suggestion.
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
            "label" => entry.label = value.to_owned(),
            "suggested" => entry.suggested = value == "true",
            other => panic!("unknown field `{other}` in binding `{}`", entry.name),
        }
    }

    assert!(open.is_none(), "an entry in the keymap was never closed");
    assert!(rows.len() > 30, "only {} bindings parsed", rows.len());
    rows
}

/// Every row of every keyboard table in the user guide.
///
/// The scope comes from the `###` heading above the table, so a table filed
/// under the wrong heading is a failure and not something the reader has to
/// notice.
fn guide() -> BTreeSet<Row> {
    let text = read("docs/USER-GUIDE.md");
    let keyboard = text
        .split_once("## The keyboard")
        .expect("the guide has a keyboard section")
        .1;

    let mut rows = BTreeSet::new();
    let mut scope = String::new();
    for line in keyboard.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            scope = heading.trim().to_lowercase();
            continue;
        }
        if line.starts_with("## ") {
            break; // the next top-level section; the tables are over.
        }
        if !line.starts_with("| ") || line.starts_with("| --- ") || line.starts_with("| Key ") {
            continue;
        }
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
        assert_eq!(cells.len(), 4, "a keyboard row needs four cells: {line}");
        let cell = |at: usize| cells[at].trim().to_owned();
        assert!(
            !scope.is_empty(),
            "a keyboard table with no heading: {line}"
        );
        let suggested = match cells[3].trim() {
            "yes" => true,
            "-" => false,
            other => panic!("the §43 column says `{other}`; it says `yes` or `-`"),
        };
        let row = Row {
            scope: scope.clone(),
            keys: cell(0),
            label: cell(1),
            workflow: cell(2).trim_matches('`').to_owned(),
            suggested,
        };
        assert!(rows.insert(row), "the guide lists the same row twice");
    }
    rows
}

#[test]
fn the_user_guide_lists_every_key_the_window_binds() {
    let bound = bindings();
    let listed = guide();

    let missing: Vec<&Row> = bound.difference(&listed).collect();
    let extra: Vec<&Row> = listed.difference(&bound).collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "the user guide's keyboard table has drifted from app/ui/src/keymap.ts.\n\
         Regenerate the table in docs/USER-GUIDE.md.\n\
         bound but not listed: {missing:#?}\n\
         listed but not bound: {extra:#?}",
    );
}

#[test]
fn the_guide_files_every_scope_under_its_own_heading() {
    // A drift check that compared only chords would pass with every table under
    // one heading, and the scope is the part a person needs: `s` stops, but `t`
    // only detects tracks when the tracks panel has the keyboard.
    let scopes: BTreeSet<String> = bindings().into_iter().map(|row| row.scope).collect();
    let text = read("docs/USER-GUIDE.md");
    for scope in &scopes {
        let heading = format!("### {}{}", scope[..1].to_uppercase(), &scope[1..]);
        assert!(
            text.contains(&heading),
            "the guide has no `{heading}` table, and the keymap binds keys in that scope",
        );
    }
    assert!(scopes.len() >= 5, "only {} scopes parsed", scopes.len());
}

#[test]
fn the_guide_says_which_keys_section_43_suggested() {
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
