/*
 *  callers.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Holds the catalog and the code that reads it to the same list of keys.
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

//! Holds the catalog and the code that reads it to the same list of keys.
//!
//! Two failures, and each is invisible from one side alone:
//!
//! - **A key that is asked for and is not there.** It renders as itself, so
//!   the window says `export.title` where it meant "Export". Nothing refuses
//!   it at compile time in either language - a key is a string.
//! - **A key that is there and nothing asks for.** Dead weight, and dead
//!   weight a *translator* pays for: every orphan is a sentence somebody
//!   renders into their own language for a screen that will never show it.
//!
//! This has to be one test and it has to be here. VCW says things in two
//! languages - `vcw_i18n::t` in Rust, `t`/`text`/`count` in TypeScript - and
//! a check that reads only `app/ui` calls the seven export refusals orphans,
//! while a check that reads only `crates/` calls every panel string an
//! orphan. The catalog is shared, so its coverage test is too.
//!
//! Only literal keys are seen. A key built at run time cannot be checked by
//! anything, which is the argument for never building one: put the whole key
//! in the call, and branch on which call to make.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use vcw_i18n::Catalog;

/// The repository root, from this crate's manifest.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/i18n is two below the root")
        .to_path_buf()
}

/// Every source file under a directory with one of these extensions.
fn sources(dir: &Path, extensions: &[&str], into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `target` and `node_modules` are build output, and both are big
            // enough that walking them would make this test the slow one.
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name != "target" && name != "node_modules" {
                sources(&path, extensions, into);
            }
        } else if path
            .extension()
            .is_some_and(|ext| extensions.iter().any(|want| ext == *want))
        {
            into.push(path);
        }
    }
}

/// Every key named in a call, and one file that names it.
///
/// `t("key")` and `text("key")` in either language, `key("key")` for one held
/// as data and resolved later, plus `count(n, "key")`, which asks for
/// `key.one` and `key.other` rather than `key`.
fn asked() -> BTreeMap<String, String> {
    let root = root();
    let mut files = Vec::new();
    sources(&root.join("crates"), &["rs"], &mut files);
    sources(&root.join("app/src-tauri/src"), &["rs"], &mut files);
    sources(&root.join("app/ui/src"), &["ts", "tsx"], &mut files);

    let mut found = BTreeMap::new();
    for path in files {
        // This file names keys that do not exist, to prove the reader works.
        if path.ends_with("callers.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let where_ = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        for (call, suffixes) in [
            ("t(\"", &[""][..]),
            ("text(\"", &[""][..]),
            ("key(\"", &[""][..]),
            ("count(", &[".one", ".other"][..]),
        ] {
            for key in keys_after(&text, call) {
                for suffix in suffixes {
                    found
                        .entry(format!("{key}{suffix}"))
                        .or_insert_with(|| where_.clone());
                }
            }
        }
    }
    found
}

/// Marks a key on the same line as one the catalog is not meant to hold.
const EXEMPT: &str = "i18n-exempt";

/// Every key literal named by an occurrence of `call`.
///
/// `call` either ends in the opening quote - `t("`, `text("` - in which case
/// the key starts right there, or it does not - `count(` - in which case the
/// key is the first quoted thing on the same line.
///
/// Deliberately crude, and the cost of being wrong is one-sided. A string
/// that is not a key has to be all lower case, contain a dot and sit in an
/// argument position to be mistaken for one, and the result of mistaking it
/// is a test failure naming a key nobody can find - which somebody reads.
/// The other direction, missing a real call, would make both tests above
/// pass by seeing nothing, which is what the third one is for.
fn keys_after(text: &str, call: &str) -> Vec<String> {
    let quoted = call.ends_with('"');
    let mut keys = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(call) {
        let at = from + at;
        from = at + call.len();

        // A word boundary, so `text(` and `format!(` are not read as `t(`.
        // `text("` has its own marker; the duplicate key it then yields is
        // the same key, into the same map.
        if text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            continue;
        }

        let line = text[from..].split('\n').next().unwrap_or_default();
        // A key that is deliberately not in the catalog - the two tests that
        // prove a miss renders as itself. Marked rather than guessed at by
        // file name, because the Rust one is an inline `#[cfg(test)]` module
        // in a file that is otherwise all production code.
        if line.contains(EXEMPT) {
            continue;
        }
        let body = if quoted {
            line
        } else {
            // `count(n, "key")`: only an argument and a comma may sit before
            // the quote, which is what stops the next string on the line
            // being read as the key of a call that spans lines.
            let Some(quote) = line.find('"') else {
                continue;
            };
            if !line[..quote]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || " ,_.".contains(c))
            {
                continue;
            }
            &line[quote + 1..]
        };
        let Some(end) = body.find('"') else {
            continue;
        };
        let key = &body[..end];
        if key.contains('.')
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        {
            keys.push(key.to_owned());
        }
    }
    keys
}

#[test]
fn every_key_that_is_asked_for_is_in_the_catalog() {
    let source = Catalog::source();
    let missing: Vec<String> = asked()
        .iter()
        .filter(|(key, _)| source.get(key).is_none())
        .map(|(key, where_)| format!("{key}, asked for in {where_}"))
        .collect();
    assert!(
        missing.is_empty(),
        "these keys would render as themselves. Add them to i18n/en-US.toml, \
         then `VCW_BLESS=1 cargo test -p vcw-i18n --test frontend_catalog`:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn every_key_in_the_catalog_is_asked_for() {
    let named: BTreeSet<String> = asked().into_keys().collect();
    let orphans: Vec<&str> = Catalog::source()
        .keys()
        .filter(|key| !named.contains(*key))
        .collect();
    assert!(
        orphans.is_empty(),
        "nothing says these, and a translator would translate them anyway. \
         Remove them from i18n/en-US.toml, or use them:\n  {}",
        orphans.join("\n  ")
    );
}

#[test]
fn the_reader_finds_a_key_in_either_language() {
    // The guard on the test above: a reader that found nothing would report
    // every key an orphan and no key missing, and both tests would pass. The
    // shapes here are the five real ones, written out rather than sampled
    // from the tree, so this still fails if the tree stops using one.
    let rust = r#"return Some(vcw_i18n::t("export.flac.float"));"#;
    let tsx = r#"<h2>{text("export.title")}</h2>"#;
    let plain = r#"const label = t("export.plan");"#;
    let plural = r#"count(report.files, "export.wrote.files")"#;
    let held = r#"  global: key("help.scope.global"),"#;

    assert_eq!(keys_after(rust, "t(\""), vec!["export.flac.float"]);
    assert_eq!(keys_after(tsx, "text(\""), vec!["export.title"]);
    assert_eq!(keys_after(plain, "t(\""), vec!["export.plan"]);
    assert_eq!(keys_after(plural, "count("), vec!["export.wrote.files"]);
    assert_eq!(keys_after(held, "key(\""), vec!["help.scope.global"]);

    // And does not mistake an ordinary string for one. No dot, so no key.
    assert!(keys_after(r#"panic!("writing the bindings")"#, "t(\"").is_empty());
}
