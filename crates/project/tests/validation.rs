/*
 *  validation.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Validation has to find damage, not just pass on healthy files.
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

//! Validation has to find damage, not just pass on healthy files.
//!
//! Each test breaks one specific thing and asserts the specific finding. A
//! validator tested only against sound projects is a function that returns
//! "clean".

use vcw_project::track::{NewBoundary, Update};
use vcw_project::{Options, Project, side, track, validate};
use vcw_types::StorageFormat;
use vcw_types::observation::{Edge, Provenance};
use vcw_types::vinyl::Side;

mod common;
use common::{Blocks, insert_capture};

/// A project with one clean stereo capture, ready to be damaged.
fn project(dir: &tempfile::TempDir) -> Project {
    let path = dir.path().join("side-a.vcw");
    let mut project = Project::create(&path).unwrap();
    insert_capture(
        &mut project,
        Blocks::new(StorageFormat::Int24Packed, 2, 4, 12_000),
    );
    project
}

fn findings(project: &Project) -> Vec<&'static str> {
    validate(
        project,
        Options {
            verify_checksums: true,
        },
    )
    .unwrap()
    .findings
    .iter()
    .map(|f| f.code)
    .collect()
}

#[test]
fn the_undamaged_project_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(findings(&project(&dir)), Vec::<&str>::new());
}

#[test]
fn a_timeline_entry_with_no_samples_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    // Foreign keys would normally stop this; a project touched by a tool that did
    // not enable them can carry it, and it is the worst shape of damage there is -
    // the timeline claiming audio that does not exist.
    p.conn().pragma_update(None, "foreign_keys", false).unwrap();
    p.conn()
        .execute("DELETE FROM sampleblocks WHERE blockid = 3", [])
        .unwrap();
    assert!(findings(&p).contains(&"dangling-block"));
}

#[test]
fn samples_nothing_refers_to_are_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    p.conn()
        .execute(
            "INSERT INTO sampleblocks (sampleformat, samples) VALUES (196610, x'0011')",
            [],
        )
        .unwrap();
    assert!(findings(&p).contains(&"orphan-block"));
}

#[test]
fn a_declared_frame_count_that_does_not_match_the_bytes_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    p.conn()
        .execute(
            "UPDATE capture_blocks SET frame_count = 11999 WHERE blockid = 2",
            [],
        )
        .unwrap();
    let codes = findings(&p);
    assert!(codes.contains(&"size-mismatch"), "{codes:?}");
    // The shortened block also breaks contiguity, which is a separate fault and
    // should be reported as one.
    assert!(codes.contains(&"timeline-gap"), "{codes:?}");
}

#[test]
fn a_gap_in_the_timeline_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    p.conn()
        .execute(
            "UPDATE capture_blocks SET start_frame = start_frame + 1 WHERE sequence >= 2",
            [],
        )
        .unwrap();
    assert!(findings(&p).contains(&"timeline-gap"));
}

#[test]
fn a_format_code_we_do_not_know_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    p.conn()
        .execute(
            "UPDATE sampleblocks SET sampleformat = 123456 WHERE blockid = 1",
            [],
        )
        .unwrap();
    assert!(findings(&p).contains(&"unknown-format"));
}

#[test]
fn corrupted_samples_are_found_only_when_checksums_are_verified() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    // Same length, different bytes: the cheap checks cannot see this, which is
    // exactly why the expensive one exists.
    p.conn()
        .execute(
            "UPDATE sampleblocks SET samples = zeroblob(length(samples)) WHERE blockid = 4",
            [],
        )
        .unwrap();

    let cheap = validate(&p, Options::default()).unwrap();
    assert!(
        cheap.is_clean(),
        "structure is intact: {:?}",
        cheap.findings
    );
    assert!(!cheap.checksums_verified);

    let thorough = validate(
        &p,
        Options {
            verify_checksums: true,
        },
    )
    .unwrap();
    assert!(thorough.has("checksum-mismatch"), "{:?}", thorough.findings);
}

#[test]
fn a_capture_that_finished_before_it_started_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    p.conn()
        .execute("UPDATE captures SET finished_at = started_at - 1", [])
        .unwrap();
    assert!(findings(&p).contains(&"time-travel"));
}

#[test]
fn a_capture_in_an_unknown_state_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    p.conn()
        .execute("UPDATE captures SET state = 'confused'", [])
        .unwrap();
    assert!(findings(&p).contains(&"unknown-state"));
}

#[test]
fn missing_version_metadata_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    p.conn()
        .execute("DELETE FROM meta WHERE key = 'created.at'", [])
        .unwrap();
    assert!(findings(&p).contains(&"missing-meta"));
}

#[test]
fn a_missing_table_stops_the_run_rather_than_cascading() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    p.conn()
        .execute_batch("DROP TABLE capture_diagnostics")
        .unwrap();
    let report = validate(&p, Options::default()).unwrap();
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert!(report.has("missing-table"));
}

#[test]
fn every_finding_names_the_rows_involved() {
    let dir = tempfile::tempdir().unwrap();
    let p = project(&dir);
    p.conn().pragma_update(None, "foreign_keys", false).unwrap();
    p.conn()
        .execute("DELETE FROM sampleblocks WHERE blockid = 3", [])
        .unwrap();
    p.conn()
        .execute("UPDATE captures SET state = 'confused'", [])
        .unwrap();

    let report = validate(&p, Options::default()).unwrap();
    assert!(!report.is_clean());
    for finding in &report.findings {
        assert!(
            finding.detail.chars().any(|c| c.is_ascii_digit()),
            "finding {:?} names no row: {}",
            finding.code,
            finding.detail
        );
    }
}

/// A side attached to the project's one capture, with two tracks on it.
///
/// The topology checks need a side to look at, and the capture matters: a side
/// with boundaries and no audio is itself a finding.
fn a_side_with_two_tracks(project: &mut Project) -> (i64, i64) {
    let capture: i64 = project
        .conn()
        .query_row("SELECT MIN(capture_id) FROM captures", [], |r| r.get(0))
        .unwrap();
    side::attach(project, Side::A, capture).unwrap();
    (
        track::add_track(project, Side::A, 0, 4_000).unwrap(),
        track::add_track(project, Side::A, 4_000, 8_000).unwrap(),
    )
}

#[test]
fn a_side_with_tracks_on_it_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = project(&dir);
    let (first, _) = a_side_with_two_tracks(&mut p);
    track::update(&mut p, first, &Update::title("Opener")).unwrap();
    assert_eq!(findings(&p), Vec::<&str>::new());
}

#[test]
fn boundaries_nothing_uses_are_not_a_finding() {
    // The normal state of an analyzed, unedited side.
    let dir = tempfile::tempdir().unwrap();
    let mut p = project(&dir);
    let capture: i64 = p
        .conn()
        .query_row("SELECT MIN(capture_id) FROM captures", [], |r| r.get(0))
        .unwrap();
    side::attach(&mut p, Side::A, capture).unwrap();
    for at in [0, 1_000, 2_000] {
        track::add_boundary(
            &mut p,
            Side::A,
            &NewBoundary::detected(at, Edge::Start, 0.8, Provenance::Silence),
        )
        .unwrap();
    }
    assert_eq!(findings(&p), Vec::<&str>::new());
}

#[test]
fn a_track_that_runs_backwards_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = project(&dir);
    let (first, _) = a_side_with_two_tracks(&mut p);
    let boundary = track::track(p.conn(), first).unwrap().unwrap().end_boundary;
    // The verbs refuse this, so reach past them: a project a third-party tool or
    // a bad migration touched is exactly what validation exists for.
    p.conn()
        .execute(
            "UPDATE track_boundaries SET at_frame = 0 WHERE boundary_id = ?1",
            [boundary],
        )
        .unwrap();
    assert!(findings(&p).contains(&"empty-track"));
}

#[test]
fn overlapping_tracks_are_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = project(&dir);
    let (_, second) = a_side_with_two_tracks(&mut p);
    let boundary = track::track(p.conn(), second)
        .unwrap()
        .unwrap()
        .start_boundary;
    p.conn()
        .execute(
            "UPDATE track_boundaries SET at_frame = 1_000 WHERE boundary_id = ?1",
            [boundary],
        )
        .unwrap();
    assert!(findings(&p).contains(&"overlapping-tracks"));
}

#[test]
fn a_gap_in_the_numbering_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = project(&dir);
    let (_, second) = a_side_with_two_tracks(&mut p);
    p.conn()
        .execute("UPDATE tracks SET number = 7 WHERE track_id = ?1", [second])
        .unwrap();
    assert!(findings(&p).contains(&"track-numbering"));
}

#[test]
fn boundaries_on_a_side_with_no_audio_are_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = project(&dir);
    a_side_with_two_tracks(&mut p);
    side::detach(&mut p, Side::A).unwrap();
    assert!(findings(&p).contains(&"boundaries-without-audio"));
}
