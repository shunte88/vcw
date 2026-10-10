/*
 *  config.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §39's settings on disk, and the library of projects they point at (§34).
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

//! §39's settings on disk, and the library of projects they point at (§34).
//!
//! # Read through, never cached
//!
//! [`crate::state`] says the shell caches nothing, because a cache here would
//! be a second copy of the truth. Settings are no exception: every command
//! below reads the file, and `save` writes it. A person editing the JSON by
//! hand and a person using the panel therefore cannot disagree, and the
//! alternative - an in-memory copy written back on quit - is how a crash loses
//! a configuration that was visibly applied.
//!
//! The cost is a file read per settings-reading command, which happens when a
//! panel opens and not in any loop.
//!
//! # A missing file is the default settings, not an error
//!
//! First run has no file, and that is the normal case rather than something to
//! report. An *unreadable* file is different and does report: a JSON syntax
//! error after hand-editing should say so, because silently reverting to
//! defaults would look like the settings were thrown away.
//!
//! # No credential ever reaches this file
//!
//! §39 forbids credentials in project files, and this is stricter: they are not
//! in the settings file either. A config directory gets copied, synced and
//! backed up without anybody deciding to, which makes a plaintext token there a
//! credential with no custody. Tokens come from the environment and what
//! crosses to a UI is [`vcw_contract::settings::Credential`] - present, and how
//! many characters. `save` cannot write one because the type it takes has
//! nowhere to put one.

use std::fs;
use std::path::{Path, PathBuf};

use vcw_contract::browse;
use vcw_contract::command::NewProject;
use vcw_contract::settings::{Credential, Settings};
use vcw_contract::view::{Listing, Project as ProjectRow};
use vcw_project::Project;
use vcw_project::release::{self, Artwork};

use crate::host::Hosted;
use crate::state::{Error, Shell};

/// The settings file's name inside the app config directory.
const FILE: &str = "settings.json";

/// §39's settings, or the defaults on first run.
///
/// # Errors
///
/// [`Error::Invalid`] naming the file when it exists and will not parse. A file
/// that is not there is not an error.
pub fn settings(host: &Hosted) -> Result<Settings, Error> {
    load(host)
}

/// Writes §39's settings.
///
/// Written whole rather than per field, because a settings panel with a save
/// button is one decision by a person and half-applied settings are a state
/// nothing can reason about. The config directory is created if it is not
/// there, the one directory this application makes without being asked, and it
/// makes it only when a person has pressed save.
///
/// # Errors
///
/// [`Error::Invalid`] if the file cannot be written, naming the path.
pub fn save_settings(host: &Hosted, settings: Settings) -> Result<(), Error> {
    save(host, &settings)?;
    // After the write, not before: a language that will not load is worth
    // reporting, but it is not a reason to refuse to save the rest of the
    // panel a person just filled in.
    speak(&settings)
}

/// Whether a credential is configured, and nothing about what it is (§39).
///
/// Read from the environment on every call rather than at startup, so a person
/// who exports a token and restarts nothing still sees it appear - and so that
/// this process never holds a token it was not about to use.
///
/// # Errors
///
/// Never. Nothing configured is an answer, not a failure.
pub fn credentials() -> Vec<Credential> {
    Credential::survey(&vcw_metadata::credentials::Credentials::from_env())
}

/// Every project in the library, newest first (§34).
///
/// An unset library root answers with an empty list rather than refusing.
/// That reads as a deliberate choice and it is: the browser's empty state is
/// where a person is told to pick a library, so a refusal here would put the
/// same message in an error banner instead of in the one place it helps.
///
/// # Errors
///
/// [`Error::Invalid`] only if the settings file will not parse.
pub fn projects(host: &Hosted) -> Result<Vec<ProjectRow>, Error> {
    let settings = load(host)?;
    let Some(root) = settings.recording.library else {
        return Ok(Vec::new());
    };
    Ok(browse::library(Path::new(&root)))
}

/// Creates a project in the library, seeded from what is on the sleeve (§34).
///
/// Every field of [`NewProject`] is optional and so is the whole prompt: this
/// is a helper, and identification fills the gaps. What it will not do is guess
/// a *location* - an unset library root refuses, naming the field, because
/// writing somebody's first project into a directory this application chose is
/// how a file goes missing.
///
/// Returns the browser row for the file it made, so the caller does not have to
/// re-list the library to find it.
///
/// # Errors
///
/// [`Error::Invalid`] naming `library` when no library root is set, `name` when
/// the derived file already exists, and [`Error::Project`] if the file cannot
/// be created.
pub fn new_project(host: &Hosted, seed: NewProject) -> Result<ProjectRow, Error> {
    let settings = load(host)?;
    let Some(root) = settings.recording.library else {
        return Err(Error::Invalid {
            field: "library".to_owned(),
            why: "no library is set - choose where projects are kept in settings first".to_owned(),
        });
    };

    let root = PathBuf::from(root);
    fs::create_dir_all(&root).map_err(|why| Error::Invalid {
        field: "library".to_owned(),
        why: format!("{} could not be created: {why}", root.display()),
    })?;

    let path = root
        .join(file_name(&seed))
        .with_extension(vcw_project::EXTENSION);
    if path.exists() {
        return Err(Error::Invalid {
            field: "name".to_owned(),
            why: format!("{} is already there", path.display()),
        });
    }

    {
        let mut project = Project::create(&path)?;
        // Only written when the prompt gave something. `release::ensure` creates
        // the row, and storing an all-empty record would make
        // `read::release` answer with a release rather than `null` - which is
        // the difference a metadata panel uses to decide whether to offer a
        // lookup or show what is already known.
        // The two flags count as "something given": a person who ticked mono and
        // typed nothing else has still told the project the one thing nothing
        // can work out later, and leaving them out of this test would drop it.
        if seed.artist.is_some()
            || seed.album.is_some()
            || seed.catalog.is_some()
            || seed.is_mono
            || seed.riaa_eq
        {
            let mut release = release::ensure(&mut project)?;
            release.album = seed.album.clone().unwrap_or_default();
            release.album_artist = seed.artist.clone().unwrap_or_default();
            release.catalog = seed.catalog.clone().unwrap_or_default();
            release.is_mono = seed.is_mono;
            release.riaa_eq = seed.riaa_eq;
            release::store(&mut project, &release)?;
        }
        project.close()?;
    }

    Ok(browse::summarize(&path))
}

/// Opens the library's own directory, or nothing when none is set.
///
/// Separate from [`projects`] so a settings panel can show the path it is about
/// to list without listing it.
///
/// # Errors
///
/// As [`settings`].
pub fn library_root(host: &Hosted) -> Result<Option<String>, Error> {
    Ok(load(host)?.recording.library)
}

/// One directory of the host's filesystem, for §52's path browser.
///
/// The desktop shell never calls this: it opens the platform's own chooser,
/// which draws better and knows about bookmarks and removable volumes. A
/// browser pointed at `vcw serve` cannot, because that chooser would open on
/// the wrong machine, so the listener draws a list instead and this is where
/// the list comes from.
///
/// The command exists on both hosts even so, because §52 forbids a command
/// `serve` has and the window does not. What differs is the fence:
/// [`crate::Host::browse_root`] is `None` in the window, and a host with no root has
/// nothing to browse.
///
/// # Errors
///
/// [`Error::Invalid`] when the host has no path browser. That is a refusal
/// rather than an empty listing because it is a deployment fact and not a
/// directory that happens to be empty, and the frontend hides the button it
/// belongs to either way.
pub fn browse(host: &Hosted, at: Option<String>) -> Result<Listing, Error> {
    let root = host.browse_root().ok_or_else(|| Error::Invalid {
        // The argument was fine; what is missing is the fence. Naming the
        // flag as the field puts it in front of the sentence without the
        // sentence having to say it twice - `Error::Invalid` renders as
        // `{field}: {why}`.
        field: "--files".to_owned(),
        why: "this copy of VCW has no directory browser - type the export \
              path as the machine running VCW names it, or restart it with \
              a directory to browse under"
            .to_owned(),
    })?;
    Ok(browse::directories(&root, at.as_deref().map(Path::new)))
}

/// The front cover of one project in the library, as a `data:` URL (§34).
///
/// By path and not from the open project, because the browser draws a hundred
/// rows and has none of them open. Each call opens that file read-only, reads
/// one blob and closes, which is the same bargain every read command in
/// [`crate::library`] makes and for the same reason.
///
/// # Why this is not on the listing
///
/// [`projects`] reports `has_artwork`, a flag, and this returns the image.
/// Splitting them is the whole design: a cover is a megabyte or two, a library
/// is a hundred rows, and a listing that carried the images would cost more to
/// open than a project does. The table asks for the covers it is about to
/// draw, one row at a time, and a row with `has_artwork: false` never asks.
///
/// Base64 and a `data:` URL rather than a second asset protocol, because the
/// alternative is a URL scheme, a handler and a cache-invalidation question for
/// an image that changes when a person assigns a release. The encoding costs
/// about a third again in size on a payload that is already in memory.
///
/// # Errors
///
/// [`Error::Project`] if the file will not open or will not read. A project
/// with no front cover is `None`, which is an answer and not a failure.
pub fn artwork(path: String) -> Result<Option<String>, Error> {
    use base64::Engine as _;

    let project = Project::open_read_only(Path::new(&path))?;
    let Some(image) = release::artwork(project.conn(), Artwork::FRONT)? else {
        return Ok(None);
    };
    // The stored MIME and not a guess: `put_artwork` sniffed it when the image
    // arrived, so the one thing that knows is the row.
    let encoded = base64::engine::general_purpose::STANDARD.encode(&image.bytes);
    Ok(Some(format!("data:{};base64,{encoded}", image.mime)))
}

/// The project the shell currently has open, if any.
///
/// The frontend needs this after a reload: the window can be refreshed while
/// the engine stays armed, and a UI that assumed nothing was open would offer
/// to arm a device that is already recording.
///
/// # Errors
///
/// Never.
pub fn open_path(shell: &Shell) -> Option<String> {
    shell
        .project
        .lock()
        .expect("the project mutex")
        .as_ref()
        .map(|path| path.display().to_string())
}

/// Every language this copy of VCW can be set to (§39).
///
/// The source language plus whatever catalogs are in `i18n/` beside the
/// settings file, which is where a submission goes to be tried out before it
/// is sent in. Read on every call rather than at startup for the reason the
/// settings are: a person who has just dropped a file in that directory should
/// find it in the menu, not after a restart.
///
/// # Errors
///
/// Never. A missing directory is a machine with no translations on it.
pub fn languages() -> Vec<String> {
    vcw_i18n::user_dir()
        .map(|dir| vcw_i18n::installed(&dir))
        .unwrap_or_else(|| vec![vcw_i18n::SOURCE_LOCALE.to_owned()])
}

/// Switches the process to the language the settings name.
///
/// Called at startup and again whenever settings are saved, which is what
/// makes the menu take effect without a restart. A catalog that will not read
/// is reported rather than swallowed - a person who chose a language and got
/// English back is owed the parser's complaint - but the language still falls
/// back, so a broken submission cannot stop the application.
///
/// # Errors
///
/// [`Error::Invalid`] naming `language` when the chosen catalog will not read.
pub fn speak(settings: &Settings) -> Result<(), Error> {
    let locale = settings
        .language
        .clone()
        .unwrap_or_else(|| vcw_i18n::SOURCE_LOCALE.to_owned());
    let Some(dir) = vcw_i18n::user_dir() else {
        vcw_i18n::activate(None);
        return Ok(());
    };
    match vcw_i18n::load(&dir, &locale) {
        Ok(catalog) => {
            vcw_i18n::activate(catalog);
            Ok(())
        }
        Err(why) => {
            vcw_i18n::activate(None);
            Err(Error::Invalid {
                field: "language".to_owned(),
                why: format!(
                    "{why} - VCW is speaking {} instead",
                    vcw_i18n::SOURCE_LOCALE
                ),
            })
        }
    }
}

/// Where the settings file lives.
fn path_of(host: &Hosted) -> Result<PathBuf, Error> {
    host.config_dir()
        .map(|dir| dir.join(FILE))
        .ok_or_else(|| Error::Invalid {
            field: "settings".to_owned(),
            why: "this platform has no config directory".to_owned(),
        })
}

/// Reads the settings file, or the defaults.
///
/// `pub` because a command that acts on a setting has to read it: §39's
/// detection group is what [`crate::detect`] runs under, and the alternative
/// is the frontend passing thresholds back down with every request, which
/// would put the authoritative copy of a setting in the webview.
pub fn load(host: &Hosted) -> Result<Settings, Error> {
    let path = path_of(host)?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        // Not there is first run. Anything else - a permission problem, a
        // directory where the file should be - is worth saying, because
        // defaults that look like a wiped configuration are worse than an
        // error.
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(why) => {
            return Err(Error::Invalid {
                field: "settings".to_owned(),
                why: format!("{} could not be read: {why}", path.display()),
            });
        }
    };
    serde_json::from_str(&text).map_err(|why| Error::Invalid {
        field: "settings".to_owned(),
        why: format!("{} is not valid settings: {why}", path.display()),
    })
}

/// Writes the settings file.
fn save(host: &Hosted, settings: &Settings) -> Result<(), Error> {
    let path = path_of(host)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|why| Error::Invalid {
            field: "settings".to_owned(),
            why: format!("{} could not be created: {why}", dir.display()),
        })?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|why| Error::Invalid {
        field: "settings".to_owned(),
        why: why.to_string(),
    })?;
    fs::write(&path, text + "\n").map_err(|why| Error::Invalid {
        field: "settings".to_owned(),
        why: format!("{} could not be written: {why}", path.display()),
    })
}

/// The file name a new project gets.
///
/// In order of preference: what the person typed, then the sleeve, then the
/// date. A name is derived here rather than in the frontend because it is a
/// decision - and because the sanitizer it has to go through is
/// [`vcw_export::naming::sanitize`], which already knows about the nine
/// characters Windows forbids, control characters in a badly encoded tag, and
/// that `AUX` is a file nobody can create. A second sanitizer in TypeScript
/// would be a second answer.
fn file_name(seed: &NewProject) -> String {
    let typed = seed
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(name) = typed {
        return vcw_export::naming::sanitize(name);
    }

    let parts: Vec<&str> = [
        seed.artist.as_deref(),
        seed.album.as_deref(),
        seed.catalog.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .collect();

    if parts.is_empty() {
        // Nothing to go on. A date sorts and does not collide, which is all a
        // placeholder has to do - and the browser shows the row by release
        // anyway once one is filled in.
        return format!("Untitled {}", stamp());
    }
    vcw_export::naming::sanitize(&parts.join(" - "))
}

/// Today, as `YYYY-MM-DD-HHMMSS`.
///
/// Hand-rolled from a unix timestamp rather than pulled in with a date crate:
/// this is the only place in the shell that needs a calendar, and the value only
/// has to sort and not collide.
fn stamp() -> String {
    stamp_at(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
    )
}

/// The calendar itself, from a timestamp.
///
/// Split from [`stamp`] so a test can drive it at a leap day rather than at
/// whatever today is. The split is the whole reason this is two functions: the
/// first draft took `now` internally, and the only test possible then was one
/// that reimplemented the arithmetic and compared it to itself.
fn stamp_at(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);

    // Civil-from-days, the standard algorithm, shifted to a March-based year so
    // that the leap day lands at the end.
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = era * 400 + yoe + i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}-{hour:02}{minute:02}{second:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> NewProject {
        NewProject {
            name: None,
            artist: None,
            album: None,
            catalog: None,
            is_mono: false,
            riaa_eq: false,
        }
    }

    #[test]
    fn a_typed_name_wins() {
        let given = NewProject {
            name: Some("  Trans Europe Express  ".to_owned()),
            artist: Some("Kraftwerk".to_owned()),
            ..seed()
        };
        assert_eq!(file_name(&given), "Trans Europe Express");
    }

    #[test]
    fn the_sleeve_is_the_fallback() {
        let given = NewProject {
            artist: Some("Kraftwerk".to_owned()),
            album: Some("Trans-Europe Express".to_owned()),
            catalog: Some("1C 064-82 306".to_owned()),
            ..seed()
        };
        assert_eq!(
            file_name(&given),
            "Kraftwerk - Trans-Europe Express - 1C 064-82 306"
        );
    }

    #[test]
    fn a_partial_sleeve_leaves_no_empty_joins() {
        let given = NewProject {
            artist: Some("Kraftwerk".to_owned()),
            catalog: Some("   ".to_owned()),
            ..seed()
        };
        assert_eq!(
            file_name(&given),
            "Kraftwerk",
            "a blank field should not become a ' - ' in the file name"
        );
    }

    /// The name goes through the export sanitizer, so a slash off a sleeve is
    /// one file and not two directories.
    #[test]
    fn a_slash_does_not_become_a_directory() {
        let given = NewProject {
            artist: Some("AC/DC".to_owned()),
            ..seed()
        };
        let name = file_name(&given);
        assert!(!name.contains('/'), "{name} still has a separator in it");
        assert_eq!(name, "AC_DC");
    }

    #[test]
    fn nothing_at_all_still_names_a_file() {
        let name = file_name(&seed());
        assert!(name.starts_with("Untitled 2"), "{name}");
        assert!(!name.contains('/'));
    }

    /// The calendar is hand-rolled, so it gets checked against dates whose
    /// answers are known: the epoch, a leap day, the day after one, and the
    /// midnight this was written across.
    #[test]
    fn the_stamp_is_a_real_calendar() {
        assert_eq!(stamp_at(0), "1970-01-01-000000");
        assert_eq!(stamp_at(951_782_400), "2000-02-29-000000", "a leap day");
        assert_eq!(
            stamp_at(1_583_020_800),
            "2020-03-01-000000",
            "the day after one"
        );
        assert_eq!(stamp_at(1_790_467_199), "2026-09-26-235959");
        assert_eq!(stamp_at(1_790_467_200), "2026-09-27-000000");
        // 2100 is not a leap year, which is the case a naive `% 4` gets wrong.
        assert_eq!(stamp_at(4_107_542_400), "2100-03-01-000000");
    }
}
