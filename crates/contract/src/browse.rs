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
//! by a newer schema, one written by an older one that has not been upgraded
//! yet, a file being captured into right now, a name that happens to end in
//! `.vcw` and is not a project at all. [`summarise`] answers with a
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

use vcw_project::{Project, release, schema, session, side, track};

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

    // Newest first. `Reverse` rather than a flipped comparator: same stable
    // order, and clippy asks for it from 1.98 on.
    out.sort_by_key(|entry| std::cmp::Reverse(entry.modified));
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
        has_artwork: false,
        preview: Vec::new(),
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

    // Asked before the reads rather than after one of them fails, because the
    // §29 tables are what this function is for and a v1 file has none of them.
    // Without it the row's reason is SQLite's `no such table: releases`, which
    // is a sentence about a schema in front of somebody looking for a record.
    project.require_current_schema()?;

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

    // `artwork_bytes` and not `artwork`: the former is `length(bytes)`, which
    // reads the row, and the latter is the blob. The browser wants to know
    // whether to draw a cover, not to be handed one per row.
    let has_artwork = release::artwork_bytes(conn, release::Artwork::FRONT)?.is_some();

    // The tile view's picture of a project that has no cover yet, which is most
    // of them. Read at the end and allowed to fail quietly: a row that cannot
    // draw a thumbnail is still a row, and a capture whose blocks are missing -
    // a kill mid-write, an import still landing - must not take the whole
    // listing down over a decoration. `problem` is for a file that will not
    // open; this is not that.
    let preview = recorded
        .first()
        .and_then(|record| preview_of(conn, record.id))
        .unwrap_or_default();

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
        row.has_artwork = has_artwork;
        row.preview = preview;
    })
}

/// One capture's whole length as [`view::PREVIEW_COLUMNS`] peak magnitudes.
///
/// `None` rather than an error for every failure here, for the reason given at
/// the call site. The channel is zero because a thumbnail of one channel and a
/// thumbnail of both are the same picture at this size.
fn preview_of(conn: &rusqlite::Connection, capture_id: i64) -> Option<Vec<f32>> {
    let shape = vcw_project::waveform::Shape::of(conn, capture_id).ok()?;
    let request = shape.whole(view::PREVIEW_COLUMNS);
    let drawn = vcw_project::waveform::read(conn, capture_id, 0, &request).ok()?;
    Some(
        drawn
            .columns
            .iter()
            .map(|column| column.min.abs().max(column.max.abs()).clamp(0.0, 1.0))
            .collect(),
    )
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

    /// Turns a project back into the v1 it would have been before WP-13.
    ///
    /// Drops the §29 tables and winds `user_version` back, which is what a real
    /// project captured by an earlier build looks like on disk. Cheaper and more
    /// honest than keeping a binary fixture in the tree, because it is built by
    /// the same migration set it is testing against.
    fn wind_back_to_v1(path: &Path) {
        let project = Project::open(path).expect("opening the project");
        project
            .conn()
            .execute_batch(
                // Everything migration 2 creates, dropped in the order the
                // foreign keys point, then the bookkeeping that says it ran.
                "DROP TABLE IF EXISTS tracks;
                 DROP TABLE IF EXISTS track_boundaries;
                 DROP TABLE IF EXISTS sides;
                 DROP TABLE IF EXISTS release_artwork;
                 DROP TABLE IF EXISTS releases;
                 DELETE FROM schema_migrations WHERE version >= 2;
                 PRAGMA user_version = 1;",
            )
            .expect("winding the schema back");
        project.close().expect("closing it again");
    }

    /// First light found this one: a v1 project listed as zeros with no reason.
    ///
    /// The counts staying at zero is right - there is nothing to count - but the
    /// row has to say why, and it has to say it in a sentence about the project
    /// rather than about SQLite. A browser draws this one amber with the reason
    /// beside it, and opening it upgrades it (§16).
    #[test]
    fn a_project_from_before_the_vinyl_tables_says_it_needs_upgrading() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("older.vcw");
        Project::create(&path)
            .expect("creating the project")
            .close()
            .expect("closing it");
        wind_back_to_v1(&path);

        let listed = library(dir.path());
        assert_eq!(listed.len(), 1);
        let row = &listed[0];
        let problem = row.problem.as_deref().expect("a reason");
        assert!(
            problem.contains("older VCW"),
            "the reason should name the cause, got {problem:?}"
        );
        assert!(
            problem.contains("Open it to upgrade it"),
            "and say what to do about it, got {problem:?}"
        );
        assert!(
            !problem.contains("no such table"),
            "and not leak SQLite at a person, got {problem:?}"
        );
        assert_eq!(row.sides, 0);
        assert_eq!(row.tracks, 0);
        assert_eq!(row.name, "older", "it is still listed, and still named");
        assert!(row.file_bytes > 0, "and still sized from the filesystem");
    }

    /// A real project reports what is in it.
    #[test]
    fn a_real_project_reports_its_counts() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("kraftwerk.vcw");
        {
            let mut project = Project::create(&path).expect("creating the project");
            let mut release = release::ensure(&mut project).expect("the release row");
            release.album = "Trans-Europe Express".to_owned();
            release.album_artist = "Kraftwerk".to_owned();
            release.catalog = "1C 064-82 306".to_owned();
            release::store(&mut project, &release).expect("storing the release");
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
        assert!(
            row.preview.is_empty(),
            "a project with no capture has nothing to draw"
        );
    }

    /// The tile view's picture, which is the only thing most rips have.
    ///
    /// A square wave at half scale, so the answer is a number rather than
    /// "something non-zero": every column covers the same signal, so every
    /// column must read the same, and that catches a fold that took `min` or
    /// `max` alone as readily as it catches one that read nothing.
    #[test]
    fn a_recorded_project_previews_its_waveform() {
        use vcw_project::persistence::{Config, Writer};
        use vcw_types::{CaptureInfo, CaptureMode, CaptureState, SampleRate, StorageFormat};

        let dir = tempfile::tempdir().expect("a temporary directory");
        let info = CaptureInfo {
            rate: SampleRate(48_000),
            channels: 1,
            storage_format: StorageFormat::Int16,
            capture_mode: CaptureMode::Shared,
            host_api: None,
            device_id: None,
            device_name: None,
            os_verified: false,
            os_report: None,
            eq: vcw_types::CaptureEq::Unknown,
        };
        let project = Project::create(dir.path().join("recorded.vcw")).expect("create");
        let mut writer = Writer::begin(project, &info, Config::default()).expect("begin");
        // Ten seconds of a square wave alternating every 480 frames, at half
        // of full scale. i16::MAX / 2 is 16383.
        let mut pcm = Vec::with_capacity(480_000 * 2);
        for frame in 0..480_000u32 {
            let value: i16 = if (frame / 480) % 2 == 0 {
                16_383
            } else {
                -16_383
            };
            pcm.extend_from_slice(&value.to_le_bytes());
        }
        writer.push(&pcm).expect("push");
        let (_, project, _) = writer
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");
        project.close().expect("close");

        let listed = library(dir.path());
        assert_eq!(listed.len(), 1);
        let preview = &listed[0].preview;
        assert_eq!(
            preview.len(),
            view::PREVIEW_COLUMNS as usize,
            "a preview is a fixed width whatever the capture's length"
        );
        for (column, peak) in preview.iter().enumerate() {
            assert!(
                (peak - 0.5).abs() < 0.01,
                "column {column} reads {peak}, and every column covers the same half-scale square wave"
            );
        }
    }
}
