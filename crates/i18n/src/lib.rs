/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The message catalog: one string, in whatever language the settings ask for.
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

//! The message catalog: one string, in whatever language the settings ask for.
//!
//! Every user-facing sentence in VCW is a key here and a line of text in
//! `i18n/en-US.toml`, which is the source language and the file a translator
//! copies. A locale file is the same keys with the text replaced, plus the one
//! field that makes the whole thing maintainable: the hash of the English the
//! translation was made from.
//!
//! **The hash is the point.** A flat `key = "string"` map can report a missing
//! translation, because the key is simply absent. What it cannot report is a
//! translation that is *stale* - where the English underneath changed and the
//! other language still says the old thing, confidently and in full sentences.
//! That is the worse failure of the two, and it is invisible. Recording
//! `source = <hash of the English>` beside every entry makes it visible, and
//! [`Catalog::stale_against`] is the whole mechanism.
//!
//! The catalog is read from both ends. Rust asks through [`t`]; the window asks
//! the shell for the active catalog as a map and looks strings up in
//! JavaScript. Neither side holds a second copy of any English, which is what
//! keeps the source language from drifting away from itself.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::RwLock;

use serde::Deserialize;

/// The source language, compiled in.
///
/// Compiled rather than read from disk because a VCW that cannot find its own
/// English is a VCW that cannot print the error saying so.
pub const EN_US: &str = include_str!("../../../i18n/en-US.toml");

/// The locale VCW falls back to, and the one a translation is measured against.
pub const SOURCE_LOCALE: &str = "en-US";

/// A catalog that could not be read.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file is not the TOML this format is written in.
    #[error("{locale}: {source}")]
    Malformed {
        /// Which file.
        locale: String,
        /// What the parser objected to.
        source: toml::de::Error,
    },

    /// An entry is missing its text or its source hash.
    ///
    /// Separate from [`Malformed`](Self::Malformed) because the two mean
    /// different things to whoever submitted the file: one is a typo in the
    /// syntax, the other is a complete and valid TOML document that is not a
    /// catalog.
    #[error("{locale}: the entry '{key}' has no {missing}")]
    Incomplete {
        /// Which file.
        locale: String,
        /// Which entry.
        key: String,
        /// Which field.
        missing: &'static str,
    },

    /// The file is there and will not come off the disk.
    ///
    /// Not folded into [`Malformed`](Self::Malformed): a person who chose a
    /// language and got nothing needs to know whether their translation is
    /// wrong or simply unreadable, and the two have different fixes.
    #[error("{locale}: {path} could not be read: {source}")]
    Unreadable {
        /// Which file.
        locale: String,
        /// Where it was looked for.
        path: String,
        /// What the filesystem said.
        source: std::io::Error,
    },
}

/// One string, and the English it was made from.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Entry {
    /// The text, in this catalog's language.
    pub text: String,

    /// [`digest`] of the source-language text this was translated from.
    ///
    /// Written by whoever made the translation, by copying it from the
    /// `en-US.toml` they worked against. It is not a checksum of `text` - that
    /// would only prove the file had not been corrupted, which nothing was
    /// worried about. It is a fingerprint of the *English*, and it goes stale
    /// precisely when the English changes.
    ///
    /// Absent in the source language itself, where it is computed rather than
    /// recorded: `en-US.toml` cannot be stale against itself.
    #[serde(default)]
    pub source: Option<String>,
}

/// Every string VCW can say, in one language.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    /// The IETF tag this catalog is written in, as a file name without `.toml`.
    pub locale: String,
    entries: BTreeMap<String, Entry>,
}

impl Catalog {
    /// Reads a catalog from the text of a `.toml` file.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] if it is not TOML or not this shape, and
    /// [`Error::Incomplete`] if an entry is missing its text.
    pub fn parse(locale: &str, text: &str) -> Result<Self, Error> {
        let entries: BTreeMap<String, Entry> =
            toml::from_str(text).map_err(|source| Error::Malformed {
                locale: locale.to_owned(),
                source,
            })?;
        for (key, entry) in &entries {
            if entry.text.is_empty() {
                return Err(Error::Incomplete {
                    locale: locale.to_owned(),
                    key: key.clone(),
                    missing: "text",
                });
            }
        }
        Ok(Self {
            locale: locale.to_owned(),
            entries,
        })
    }

    /// The source-language catalog, compiled in.
    ///
    /// # Panics
    ///
    /// If `i18n/en-US.toml` is not a catalog, which is a build-time mistake
    /// rather than a runtime one: the file is `include_str!`'d, so the only way
    /// here is to have shipped a broken one, and every test in this crate would
    /// have caught it.
    #[must_use]
    pub fn source() -> &'static Self {
        // Parsed once. `t` reaches for this on every miss, and re-reading a
        // few hundred lines of TOML to print one error message would make the
        // fallback path the expensive one.
        static SOURCE: std::sync::OnceLock<Catalog> = std::sync::OnceLock::new();
        SOURCE.get_or_init(|| {
            Catalog::parse(SOURCE_LOCALE, EN_US)
                .expect("the compiled-in en-US catalog is a catalog")
        })
    }

    /// The text for a key, or `None` if this catalog has no such entry.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(|entry| entry.text.as_str())
    }

    /// Every key, in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// How many strings this catalog holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The catalog as a plain map, which is what the window gets.
    #[must_use]
    pub fn texts(&self) -> BTreeMap<String, String> {
        self.entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.text.clone()))
            .collect()
    }

    /// The keys this catalog is missing, against the source language.
    #[must_use]
    pub fn missing_against(&self, source: &Self) -> Vec<String> {
        source
            .keys()
            .filter(|key| !self.entries.contains_key(*key))
            .map(ToOwned::to_owned)
            .collect()
    }

    /// The keys this catalog translates that the source language no longer has.
    ///
    /// Harmless at runtime - nothing asks for them - and worth reporting
    /// anyway, because they are the translator's work sitting on a sentence
    /// that was deleted, and nobody finds that out otherwise.
    #[must_use]
    pub fn orphans_against(&self, source: &Self) -> Vec<String> {
        self.entries
            .keys()
            .filter(|key| source.get(key).is_none())
            .cloned()
            .collect()
    }

    /// The keys whose English has changed since this translation was made.
    ///
    /// The failure this format exists to catch. An entry with no `source` at
    /// all is counted stale as well: it is a translation that cannot say what
    /// it was translated from, which is the same problem one step earlier.
    ///
    /// Empty for the source language, which carries no hashes and does not
    /// need to - asking whether English has drifted from English is the
    /// question this answers "no" to rather than the bug it reports.
    #[must_use]
    pub fn stale_against(&self, source: &Self) -> Vec<String> {
        if self.locale == source.locale {
            return Vec::new();
        }
        self.entries
            .iter()
            .filter_map(|(key, entry)| {
                let english = source.get(key)?;
                match &entry.source {
                    Some(recorded) if *recorded == digest(english) => None,
                    _ => Some(key.clone()),
                }
            })
            .collect()
    }
}

/// A fingerprint of a source string, as eight hex digits.
///
/// FNV-1a, which is not a cryptographic hash and is not meant to be: nothing
/// here is defending against a translator who wants to forge a stale entry.
/// What it has to be is stable across versions of VCW and across machines -
/// `DefaultHasher` is neither, by documented design - and short enough that a
/// person editing a TOML file by hand can copy it without losing their place.
#[must_use]
pub fn digest(text: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

/// The reverse-DNS name VCW's config directory is called, on every platform.
///
/// The same string as `identifier` in `app/src-tauri/tauri.conf.json`, and it
/// has to stay the same string: that is what puts the catalogs beside the
/// settings file rather than in a directory of their own that nobody finds.
pub const IDENTIFIER: &str = "dev.vcw.workstation";

/// Where submitted catalogs live, for whoever is asking.
///
/// The window and the command line both read this, which is the point: a
/// language is chosen once and the two halves of VCW print the same sentences
/// in it. The shell could ask Tauri for its own config directory instead, and
/// that is the version that was written first - but then there were two rules
/// for one path, and a platform where they disagreed would put a person's
/// catalog somewhere their settings file is not.
///
/// The rule is the platform's own, and the same one Tauri's `dirs` applies:
/// `$XDG_CONFIG_HOME` or `~/.config` on Linux and the BSDs,
/// `~/Library/Application Support` on macOS, `%APPDATA%` on Windows. `None`
/// when none of those are set, which is a process with no home rather than a
/// failure worth reporting - [`installed`] answers `en-US` and VCW runs.
///
/// Printed by `vcw doctor --i18n` and logged by the shell at startup, so the
/// answer on a given machine is a fact a person can read rather than a path
/// they have to reconstruct from this comment.
#[must_use]
pub fn user_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(std::path::PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home.map(|home| home.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| home.map(|home| home.join(".config")))
    };
    base.map(|base| base.join(IDENTIFIER).join("i18n"))
}

/// Every language this copy of VCW can be set to, source language first.
///
/// `dir` is where submitted catalogs live - `i18n/` beside the settings file.
/// A missing directory is not an error, it is a machine with no translations
/// on it, which is every machine until somebody puts one there. Anything that
/// is not a `.toml` is ignored rather than reported, because a directory a
/// person can open is a directory that will have a `.DS_Store` in it.
///
/// The file is not parsed here. Listing has to work even when one of the
/// catalogs is broken - otherwise a single bad submission makes every other
/// language disappear from the menu, which is the opposite of a useful
/// failure. [`load`] is where a file gets read and where it gets to refuse.
#[must_use]
pub fn installed(dir: &Path) -> Vec<String> {
    let mut found = vec![SOURCE_LOCALE.to_owned()];
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "toml")
            && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
            && stem != SOURCE_LOCALE
        {
            found.push(stem.to_owned());
        }
    }
    found[1..].sort_unstable();
    found.dedup();
    found
}

/// Reads one submitted catalog off the disk.
///
/// `Ok(None)` for the source language, and for a language with no file: both
/// mean "nothing to activate", which [`activate`] already spells `None`. A
/// settings file naming a language whose file has since been deleted therefore
/// falls back to English rather than refusing to start, and the menu - built
/// from [`installed`] - stops offering it.
///
/// # Errors
///
/// [`Error::Unreadable`] if the file is there and will not read, and whatever
/// [`Catalog::parse`] objects to otherwise.
pub fn load(dir: &Path, locale: &str) -> Result<Option<Catalog>, Error> {
    if locale == SOURCE_LOCALE {
        return Ok(None);
    }
    let path = dir.join(format!("{locale}.toml"));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Unreadable {
                locale: locale.to_owned(),
                path: path.display().to_string(),
                source,
            });
        }
    };
    Catalog::parse(locale, &text).map(Some)
}

/// The catalog the next [`t`] will read, or `None` while it is still English.
static ACTIVE: RwLock<Option<Catalog>> = RwLock::new(None);

/// Switches VCW to a catalog, or back to the source language with `None`.
///
/// Global, which is the thing to argue with first. The alternative is passing a
/// `&Catalog` into every function that can produce a sentence, which is most of
/// them, and threading it through `Display` - where there is no argument to
/// thread it through at all. Language is a process-wide fact set once at
/// startup and changed by hand, so a global is what it is.
pub fn activate(catalog: Option<Catalog>) {
    if let Ok(mut active) = ACTIVE.write() {
        *active = catalog;
    }
}

/// Which locale is active, which is the source language until one is set.
#[must_use]
pub fn locale() -> String {
    ACTIVE
        .read()
        .ok()
        .and_then(|active| active.as_ref().map(|catalog| catalog.locale.clone()))
        .unwrap_or_else(|| SOURCE_LOCALE.to_owned())
}

/// The active catalog's text for a key.
///
/// Falls back to the source language, and then to the key itself. The last step
/// is deliberate and is not a silent failure: a window showing
/// `export.aiff.float` where a sentence should be is a bug report that writes
/// itself, where an empty string is a layout that merely looks odd.
#[must_use]
pub fn t(key: &str) -> String {
    if let Ok(active) = ACTIVE.read()
        && let Some(text) = active.as_ref().and_then(|catalog| catalog.get(key))
    {
        return text.to_owned();
    }
    Catalog::source().get(key).unwrap_or(key).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_catalog_is_a_catalog() {
        let source = Catalog::source();
        assert!(!source.is_empty(), "en-US.toml holds nothing");
        assert_eq!(source.locale, SOURCE_LOCALE);
        for key in source.keys() {
            assert!(
                !key.contains(' ') && key.to_lowercase() == key,
                "{key}: a key is a dotted lower-case path, not a sentence"
            );
        }
    }

    #[test]
    fn the_source_language_needs_no_source_hash() {
        // The other way round would be a file nobody could edit: every change
        // to an English sentence would mean recomputing a number by hand in
        // the same commit, and getting it wrong would fail the gate for a
        // reason that has nothing to do with translation.
        let source = Catalog::source();
        assert!(source.stale_against(source).is_empty());
        assert!(source.missing_against(source).is_empty());
        assert!(source.orphans_against(source).is_empty());
    }

    /// A catalog written the way a translator would write one.
    fn translated(pairs: &[(&str, &str, &str)]) -> Catalog {
        let text = pairs
            .iter()
            .map(|(key, text, source)| {
                format!("[\"{key}\"]\ntext = \"{text}\"\nsource = \"{source}\"\n")
            })
            .collect::<String>();
        Catalog::parse("xx-XX", &text).expect("a catalog")
    }

    #[test]
    fn a_translation_of_the_current_english_is_not_stale() {
        let source = Catalog::source();
        let english = source.get("export.aiff.channels").expect("the key");
        let them = translated(&[(
            "export.aiff.channels",
            "AIFF tragt mindestens einen Kanal.",
            &digest(english),
        )]);
        assert!(them.stale_against(source).is_empty());
        assert_eq!(
            them.get("export.aiff.channels"),
            Some("AIFF tragt mindestens einen Kanal.")
        );
    }

    #[test]
    fn a_translation_of_english_that_has_since_changed_is_stale() {
        // The whole reason this format has a third field. The translation is
        // present, well-formed and completely plausible; the only thing wrong
        // with it is that it answers a question nobody asks any more.
        let source = Catalog::source();
        let them = translated(&[(
            "export.aiff.channels",
            "AIFF tragt zwischen einem und acht Kanalen.",
            &digest("AIFF carries between one and eight channels."),
        )]);
        assert_eq!(them.stale_against(source), vec!["export.aiff.channels"]);
    }

    #[test]
    fn a_translation_that_does_not_say_what_it_came_from_is_stale_too() {
        let source = Catalog::source();
        let them = Catalog::parse("xx-XX", "[\"export.aiff.channels\"]\ntext = \"etwas\"\n")
            .expect("a catalog");
        assert_eq!(them.stale_against(source), vec!["export.aiff.channels"]);
    }

    #[test]
    fn a_catalog_reports_what_it_has_not_done_and_what_it_did_twice() {
        let source = Catalog::source();
        let them = translated(&[
            (
                "export.aiff.channels",
                "etwas",
                &digest(source.get("export.aiff.channels").unwrap()),
            ),
            ("export.gone.away", "etwas anderes", "00000000"),
        ]);
        assert!(
            them.missing_against(source)
                .contains(&"export.flac.float".to_owned())
        );
        assert_eq!(them.orphans_against(source), vec!["export.gone.away"]);
    }

    #[test]
    fn a_file_that_is_not_a_catalog_is_refused_rather_than_half_read() {
        assert!(matches!(
            Catalog::parse("xx-XX", "this is not toml at all ["),
            Err(Error::Malformed { .. })
        ));
        assert!(matches!(
            Catalog::parse("xx-XX", "[\"a.key\"]\ntext = \"\"\n"),
            Err(Error::Incomplete {
                missing: "text",
                ..
            })
        ));
    }

    #[test]
    fn the_digest_is_the_same_number_on_every_machine() {
        // Written out rather than compared to itself. `DefaultHasher` is
        // documented as free to change between releases, and a catalog format
        // whose staleness check depends on the toolchain is one that goes red
        // on somebody else's machine for no reason.
        assert_eq!(digest(""), "811c9dc5");
        assert_eq!(digest("a"), "e40c292c");
        assert_eq!(digest("foobar"), "bf9cf968");
        assert_ne!(
            digest("AIFF carries one channel."),
            digest("AIFF carries two channels.")
        );
    }

    #[test]
    fn a_missing_string_shows_its_key_rather_than_nothing() {
        activate(None);
        assert_eq!(
            t("export.aiff.channels"),
            "AIFF carries at least one channel."
        );
        assert_eq!(t("nobody.wrote.this"), "nobody.wrote.this");
        assert_eq!(locale(), SOURCE_LOCALE);
    }

    #[test]
    fn a_string_travels_from_the_catalog_to_the_caller() {
        let source = Catalog::source();
        let them = translated(&[(
            "export.aiff.channels",
            "AIFF tragt mindestens einen Kanal.",
            &digest(source.get("export.aiff.channels").unwrap()),
        )]);
        activate(Some(them));
        assert_eq!(locale(), "xx-XX");
        assert_eq!(
            t("export.aiff.channels"),
            "AIFF tragt mindestens einen Kanal."
        );
        // And a key the translation does not carry still says something.
        assert_eq!(
            t("export.flac.channels"),
            "FLAC carries between one and eight channels."
        );
        activate(None);
    }

    #[test]
    fn a_directory_of_submissions_becomes_a_menu() {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::write(dir.path().join("pt-BR.toml"), "").expect("a file");
        std::fs::write(dir.path().join("de-DE.toml"), "").expect("a file");
        // Not a catalog, and not an error either.
        std::fs::write(dir.path().join(".DS_Store"), "junk").expect("a file");
        // A second copy of the source language is not a second menu entry.
        std::fs::write(dir.path().join("en-US.toml"), "").expect("a file");

        assert_eq!(installed(dir.path()), ["en-US", "de-DE", "pt-BR"]);
        // A machine with no translations on it still has one language.
        assert_eq!(installed(&dir.path().join("nothing")), [SOURCE_LOCALE]);
    }

    #[test]
    fn a_language_with_no_file_is_english_rather_than_a_refusal() {
        let dir = tempfile::tempdir().expect("a directory");
        assert!(
            load(dir.path(), SOURCE_LOCALE)
                .expect("the source language")
                .is_none()
        );
        assert!(
            load(dir.path(), "pt-BR")
                .expect("a language nobody wrote")
                .is_none()
        );

        let source = Catalog::source();
        let text = format!(
            "[\"export.aiff.channels\"]\ntext = \"um canal\"\nsource = \"{}\"\n",
            digest(source.get("export.aiff.channels").unwrap())
        );
        std::fs::write(dir.path().join("pt-BR.toml"), text).expect("a file");
        let them = load(dir.path(), "pt-BR")
            .expect("a catalog")
            .expect("one that is there");
        assert_eq!(them.get("export.aiff.channels"), Some("um canal"));

        // And a file that is there but is not a catalog says so rather than
        // vanishing into the same `None`.
        std::fs::write(dir.path().join("de-DE.toml"), "this is not toml at all{").expect("a file");
        assert!(matches!(
            load(dir.path(), "de-DE"),
            Err(Error::Malformed { .. })
        ));
    }

    #[test]
    fn every_language_this_repository_ships_is_current() {
        // The gate for a submitted translation. Today the directory holds one
        // file and this loop runs zero times, which is the point: the check
        // has to be here *before* the first translation arrives, or the first
        // one merges unchecked and the second is measured against it.
        //
        // Stale is the interesting half. Missing keys are obvious in review -
        // the diff is short - and a stale entry is invisible: a complete,
        // well-formed, confidently wrong sentence left behind when the
        // English above it changed.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../i18n");
        let source = Catalog::source();
        for locale in installed(&dir) {
            if locale == SOURCE_LOCALE {
                continue;
            }
            let them = load(&dir, &locale)
                .unwrap_or_else(|why| panic!("{locale} will not read: {why}"))
                .expect("installed() listed it");
            assert_eq!(
                them.missing_against(source),
                Vec::<String>::new(),
                "{locale} is short"
            );
            assert_eq!(
                them.orphans_against(source),
                Vec::<String>::new(),
                "{locale} has leftovers"
            );
            assert_eq!(
                them.stale_against(source),
                Vec::<String>::new(),
                "{locale} is stale"
            );
        }
    }

    #[test]
    fn every_key_the_code_asks_for_is_in_the_source_catalog() {
        // The drift the plan wanted `en-US.toml` generated from the code to
        // prevent. Generating it is the better answer at 272 strings and an
        // over-build at four, so this is the half that matters: a key the
        // code asks for and the catalog has not got falls back to printing
        // the key, which is a sentence-shaped hole in a window. The other
        // direction - an entry nobody asks for - costs a line in a file.
        //
        // A scan of the sources rather than a macro, which is how
        // `formats.test.ts` and `Face.test.tsx` hold their own two lists
        // together. Qualified calls only: a bare `t(` is this crate's own
        // tests.
        const CALL: &str = "i18n::t(\"";
        let source = Catalog::source();
        let mut asked = Vec::new();
        let mut stack = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                if path.is_dir() {
                    // The build output and the checkouts under it dwarf the
                    // sources and hold no call of ours.
                    if !matches!(name.to_str(), Some("target" | ".git" | "node_modules")) {
                        stack.push(path);
                    }
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && let Ok(text) = std::fs::read_to_string(&path)
                {
                    for (at, _) in text.match_indices(CALL) {
                        if let Some(key) = text[at + CALL.len()..].split('"').next() {
                            asked.push((path.clone(), key.to_owned()));
                        }
                    }
                }
            }
        }

        // The scan has to find something, or it passes by looking in the
        // wrong place - which is the failure mode of every test that greps.
        assert!(!asked.is_empty(), "no call sites found at all");
        for (path, key) in asked {
            assert!(
                source.get(&key).is_some(),
                "{} asks for '{key}', which is not in i18n/en-US.toml",
                path.display()
            );
        }
    }
}
