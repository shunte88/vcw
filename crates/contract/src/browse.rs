/*
 *  browse.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The library: every project under a root, summarised for the browser (§34).
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

//! The library: every project under a root, summarised for the browser (§34).
//!
//! Separate from [`crate::read`] because it is a different kind of read. Every
//! reader there takes a `&Connection` and answers a question about one open
//! project; this one takes a directory, opens each file it finds read-only, and
//! closes it again. Mixing the two would mean a module whose functions do not
//! share a precondition.
//!
//! # A file that will not open still gets a row
//!
//! The browser's job is to describe a directory a person keeps records in, and
//! such a directory always contains surprises: a partial copy, a `.vcw` written
//! by a newer schema, a file being captured into right now, a name that happens
//! to end in `.vcw` and is not a project at all. [`summarise`] answers with a
//! row for every one of them, carrying `problem` rather than an error, because
//! the alternative is a browser that silently omits the file the person is
//! looking for.
//!
//! The one thing it will not do is repair anything. A project is opened
//! `mode=ro` (never `immutable=1`, so a populated `-wal` is honoured), which
//! means listing a library cannot run a migration, cannot recover a sidecar and
//! cannot touch a capture in progress.
//!
//! # No recursion, and no cache
//!
//! One directory, not a tree: a library root is where projects are kept, and
//! walking a whole filesystem branch is how a browser becomes slow on the day
//! somebody points it at their home directory. And nothing is remembered
//! between calls - §2 leaves one copy of the truth, so a track count shown here
//! is read out of the file each time rather than kept beside it, where the two
//! could disagree.

use std::fs;
use std::path::Path;

use vcw_project::{Project, schema, session, side, track};

use crate::view;

/// Every project directly under `root`, newest first.
///
/// An unreadable root is an empty library rather than an error: a settings
/// default that has not been created yet is the normal first-run case, and
/// `Vec::new()` is what a browser should draw for it.
///
/// The order is by modification time, descending, because the project a person
/// wants is almost always the one they had open last. Ties keep the order the
/// filesystem gave, which is arbitrary but stable within a call.
#[must_use]
pub fn library(root: &Path) -> Vec<view::Project> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };

    let mut out: Vec<view::Project> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(schema::EXTENSION))
        })
        .map(|path| summarise(&path))
        .collect();

    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

/// Summarises one project file.
///
/// Never fails. Everything that can go wrong lands in
/// [`problem`](view::Project::problem) with the counts left at zero, which is
/// the row a browser draws greyed out with a reason beside it.
#[must_use]
pub fn summarise(path: &Path) -> view::Project {
    let (bytes, modified) = fs::metadata(path).map_or((0, 0), |meta| {
        let seconds = meta
            .modified()
            .ok()
            .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            });
        (meta.len(), seconds)
    });

    let mut row = view::Project {
        path: path.display().to_string(),
        name: path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
        album: String::new(),
        album_artist: String::new(),
        catalog: String::new(),
        year: None,
        sides: 0,
        tracks: 0,
        captures: 0,
        seconds: 0.0,
        file_bytes: bytes,
        modified,
        problem: None,
    };

    match contents(path) {
        Ok(filled) => filled(&mut row),
        Err(why) => row.problem = Some(why.to_string()),
    }
    row
}

/// Reads the counts, or says why it could not.
///
/// Returns a closure rather than a tuple so that the field names are written
/// once, next to what fills them, rather than unpacked positionally at the call
/// site where a transposed pair would typecheck.
fn contents(path: &Path) -> vcw_project::Result<impl FnOnce(&mut view::Project)> {
    let project = Project::open_read_only(path)?;
    let conn = project.conn();

    let release = crate::read::release(conn)?;
    let sides = u32::try_from(side::list(conn)?.len()).unwrap_or(u32::MAX);
    let tracks = u32::try_from(track::listing(conn)?.len()).unwrap_or(u32::MAX);

    // Summed from the captures rather than from the sides, because a capture
    // can hold two faces (adoption cannot tell where one ends) and a side
    // without a capture has no audio at all. The total a person wants is how
    // much was recorded, which is the captures'.
    let recorded = session::all(conn)?;
    let captures = u32::try_from(recorded.len()).unwrap_or(u32::MAX);
    let seconds = recorded
        .iter()
        .map(|record| record.frames as f64 / f64::from(record.info.rate.hz()).max(1.0))
        .sum();

    Ok(move |row: &mut view::Project| {
        if let Some(release) = release {
            row.album = release.album;
            row.album_artist = release.album_artist;
            row.catalog = release.catalog;
            row.year = release.year;
        }
        row.sides = sides;
        row.tracks = tracks;
        row.captures = captures;
        row.seconds = seconds;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A root that does not exist is an empty library, not a failure.
    #[test]
    fn a_missing_root_lists_nothing() {
        assert!(library(Path::new("/nonexistent/vcw/library")).is_empty());
    }

    /// The browser's own contract: a file it cannot open is still a row.
    #[test]
    fn a_file_that_is_not_a_project_gets_a_row_with_a_reason() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("not-really.vcw");
        fs::write(&path, b"this is not a database").expect("writing the decoy");

        let listed = library(dir.path());
        assert_eq!(listed.len(), 1, "the decoy should still be listed");
        let row = &listed[0];
        assert_eq!(row.name, "not-really");
        assert!(row.problem.is_some(), "it should say why it would not open");
        assert_eq!(row.tracks, 0);
        assert!(
            row.file_bytes > 0,
            "the size comes from the filesystem, not the db"
        );
    }

    /// Only `.vcw` files, and the extension match is case-insensitive because
    /// two of the four target platforms have case-insensitive filesystems.
    #[test]
    fn only_projects_are_listed() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for name in ["a.vcw", "b.VCW", "notes.txt", "side-a.wav"] {
            fs::write(dir.path().join(name), b"x").expect("writing a file");
        }
        let names: Vec<_> = library(dir.path()).into_iter().map(|p| p.name).collect();
        assert_eq!(names.len(), 2, "got {names:?}");
        assert!(names.contains(&"a".to_owned()));
        assert!(names.contains(&"b".to_owned()));
    }

    /// A real project reports what is in it.
    #[test]
    fn a_real_project_reports_its_counts() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("kraftwerk.vcw");
        {
            let mut project = Project::create(&path).expect("creating the project");
            let mut release = vcw_project::release::ensure(&mut project).expect("the release row");
            release.album = "Trans-Europe Express".to_owned();
            release.album_artist = "Kraftwerk".to_owned();
            release.catalog = "1C 064-82 306".to_owned();
            vcw_project::release::store(&mut project, &release).expect("storing the release");
        }

        let listed = library(dir.path());
        assert_eq!(listed.len(), 1);
        let row = &listed[0];
        assert_eq!(row.problem, None, "a project we just wrote should open");
        assert_eq!(row.album, "Trans-Europe Express");
        assert_eq!(row.album_artist, "Kraftwerk");
        assert_eq!(row.catalog, "1C 064-82 306");
        assert_eq!(row.captures, 0);
        assert_eq!(row.seconds, 0.0);
    }
}
