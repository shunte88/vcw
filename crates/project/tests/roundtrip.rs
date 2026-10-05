/*
 *  roundtrip.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Create, open, validate, recover: the project lifecycle end to end.
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

//! Create, open, validate, recover: the project lifecycle end to end.
//!
//! The round-trip property that matters is not "the file opens again" but "the
//! bytes come back". §9 forbids conversion on the capture path, and a project
//! store that quietly reshaped a block would break the bit-perfect claim after
//! the callback had already got it right.

use rusqlite::Connection;
use vcw_project::{Access, Error, Options, Project, validate};
use vcw_types::StorageFormat;

mod common;
use common::{Blocks, insert_capture, sample_bytes};

#[test]
fn a_new_project_is_identifiable_versioned_and_clean() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("side-a.vcw");
    let project = Project::create(&path).unwrap();

    assert_eq!(
        project.schema_version().unwrap(),
        vcw_project::SCHEMA_VERSION
    );
    assert_eq!(
        project.format_version().unwrap(),
        Some(vcw_project::FORMAT_VERSION)
    );
    assert_eq!(project.access(), Access::ReadWrite);

    let app_id: i64 = project
        .conn()
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    assert_eq!(app_id as u32, vcw_project::APPLICATION_ID);

    // §16 wants creation and last-written recorded, not just the schema number.
    for key in vcw_project::meta::REQUIRED_KEYS {
        assert!(
            vcw_project::meta::get(project.conn(), key)
                .unwrap()
                .is_some(),
            "meta key {key} is not set"
        );
    }

    assert!(validate(&project, Options::default()).unwrap().is_clean());
    assert!(vcw_project::integrity_check(&project).unwrap().is_clean());
    project.close().unwrap();
}

#[test]
fn creating_over_an_existing_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("side-a.vcw");
    Project::create(&path).unwrap().close().unwrap();

    let err = Project::create(&path).unwrap_err();
    assert!(matches!(err, Error::Io(ref e) if e.kind() == std::io::ErrorKind::AlreadyExists));
}

#[test]
fn reopening_does_not_migrate_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("side-a.vcw");
    Project::create(&path).unwrap().close().unwrap();

    let project = Project::open(&path).unwrap();
    let applied: i64 = project
        .conn()
        .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(applied, vcw_project::MIGRATIONS.len() as i64);
    assert_eq!(
        project.schema_version().unwrap(),
        vcw_project::SCHEMA_VERSION
    );
    project.close().unwrap();
}

#[test]
fn a_plain_sqlite_file_is_not_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE t (a)")
        .unwrap();

    match Project::open(&path).unwrap_err() {
        Error::NotAProject { found, .. } => {
            assert_eq!(found, 0);
            // The message has to say what it is, not just what it is not.
            let text = Project::open(&path).unwrap_err().to_string();
            assert!(text.contains("plain SQLite"), "unhelpful message: {text}");
        }
        other => panic!("expected NotAProject, got {other:?}"),
    }
}

#[test]
fn an_audacity_project_is_refused_with_a_pointer_to_the_importer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rip.aup3");
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "application_id", 0x4155_4459u32)
        .unwrap();
    drop(conn);

    let err = Project::open(&path).unwrap_err();
    let text = err.to_string();
    assert!(matches!(
        err,
        Error::NotAProject {
            found: 0x4155_4459,
            ..
        }
    ));
    assert!(text.contains("Audacity"), "unhelpful message: {text}");
    assert!(text.contains("Import it"), "unhelpful message: {text}");
}

#[test]
fn a_project_from_the_future_is_refused_rather_than_half_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("side-a.vcw");
    Project::create(&path).unwrap().close().unwrap();

    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 99u32).unwrap();
    drop(conn);

    match Project::open(&path).unwrap_err() {
        Error::SchemaTooNew {
            found, supported, ..
        } => {
            assert_eq!(found, 99);
            assert_eq!(supported, vcw_project::SCHEMA_VERSION);
        }
        other => panic!("expected SchemaTooNew, got {other:?}"),
    }
}

#[test]
fn read_only_opens_honor_the_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("side-a.vcw");

    // Write and deliberately leave the writer open, so the new rows live only in
    // the -wal sidecar - the state a crashed or still-running capture leaves
    // behind. S5 trap 18: immutable=1 would ignore the sidecar and serve a stale
    // database. mode=ro does not.
    let mut writer = Project::create(&path).unwrap();
    insert_capture(
        &mut writer,
        Blocks::new(StorageFormat::Int24Packed, 2, 4, 12_000),
    );

    let reader = Project::open_read_only(&path).unwrap();
    assert_eq!(reader.access(), Access::ReadOnly);
    let blocks: i64 = reader
        .conn()
        .query_row("SELECT count(*) FROM capture_blocks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(blocks, 8, "the -wal contents were not visible");

    drop(writer);
}

#[test]
fn read_only_projects_cannot_be_written() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("side-a.vcw");
    Project::create(&path).unwrap().close().unwrap();

    let reader = Project::open_read_only(&path).unwrap();
    let err = reader
        .conn()
        .execute("INSERT INTO meta (key, value) VALUES ('x', 'y')", []);
    assert!(err.is_err(), "a read-only project accepted a write");
}

/// The round-trip property, over every storage format and a spread of shapes.
///
/// Not a single happy path: for each combination the project is created, written,
/// closed, reopened, validated with checksums on, and every block's bytes compared
/// with what went in.
#[test]
fn blocks_survive_a_round_trip_byte_for_byte() {
    let cases = [
        (StorageFormat::Int16, 1u16, 1u32, 4_800u32),
        (StorageFormat::Int16, 2, 7, 12_000),
        (StorageFormat::Int24Packed, 2, 4, 12_000),
        (StorageFormat::Int24Padded, 2, 3, 48_000),
        (StorageFormat::Int32, 2, 5, 24_000),
        (StorageFormat::Float32, 2, 9, 1_000),
        (StorageFormat::Float32, 4, 2, 65_536),
    ];

    for (format, channels, blocks, frames) in cases {
        let label = format!("{format:?} x{channels}ch x{blocks}blk x{frames}f");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("side-a.vcw");

        let mut project = Project::create(&path).unwrap();
        let written = insert_capture(&mut project, Blocks::new(format, channels, blocks, frames));
        project.close().unwrap();

        let project = Project::open(&path).unwrap();
        let report = validate(
            &project,
            Options {
                verify_checksums: true,
            },
        )
        .unwrap();
        assert!(report.is_clean(), "{label}: {:?}", report.findings);
        assert_eq!(
            report.blocks,
            u64::from(channels) * u64::from(blocks),
            "{label}"
        );
        assert_eq!(report.captures, 1, "{label}");
        assert!(
            vcw_project::integrity_check(&project).unwrap().is_clean(),
            "{label}"
        );

        for (blockid, expected) in &written {
            let actual: Vec<u8> = project
                .conn()
                .query_row(
                    "SELECT samples FROM sampleblocks WHERE blockid = ?1",
                    [blockid],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                &actual, expected,
                "{label}: block {blockid} came back different"
            );
        }
        project.close().unwrap();
    }
}

#[test]
fn a_block_of_every_format_is_the_width_it_claims() {
    for format in StorageFormat::ALL {
        let bytes = sample_bytes(format, 1_000, 0);
        assert_eq!(bytes.len(), 1_000 * format.bytes_per_sample(), "{format:?}");
    }
}
