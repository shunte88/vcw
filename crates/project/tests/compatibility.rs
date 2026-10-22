/*
 *  compatibility.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Holding the code to the backward compatibility policy in docs/COMPATIBILITY.md.
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

//! `docs/COMPATIBILITY.md` is §16's policy and ADR-0009's C1, and this is what
//! holds the code to it.
//!
//! The document makes four claims that can be checked here. Every schema version
//! from 1 to this build's own still opens and comes out current; a version above
//! this build's is refused for reading as well as for writing; no migration
//! rewrites captured audio; and the two numbers the document prints are the two
//! numbers this build uses.
//!
//! The old projects are built rather than committed. Applying a prefix of
//! `MIGRATIONS` to an empty database is how a build of that day made one, so a
//! v2 fixture here is a v2 project and not an impression of one - which is more
//! than the hand-written wind-back in `crates/cli/tests/migrate_from_cli.rs` can
//! claim, and it costs nothing to have every version rather than one.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use vcw_project::schema::{APPLICATION_ID, PAGE_SIZE};
use vcw_project::{Error, MIGRATIONS, Project, SCHEMA_VERSION, doc, migrate};

fn policy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/COMPATIBILITY.md")
}

/// Builds a project at `version`: the file a build whose newest migration was
/// `version` would have written.
fn a_project_at(version: u32, path: &Path) {
    let set = &MIGRATIONS[..version as usize];
    assert_eq!(
        set.last().expect("a migration").version,
        version,
        "the migration versions are no longer 1, 2, 3 ... in order, so a prefix \
         of MIGRATIONS is not a version any more"
    );

    let mut conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "page_size", PAGE_SIZE).unwrap();
    conn.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    migrate::apply(&mut conn, set).unwrap();
    // No `meta` rows. §16's other half - who wrote the file and when - is
    // `roundtrip.rs`; nothing in the upgrade path reads them.
    assert_eq!(migrate::current_version(&conn).unwrap(), version);
    conn.close().unwrap();
}

#[test]
fn every_schema_version_the_policy_names_opens_and_comes_out_current() {
    let dir = tempfile::tempdir().unwrap();

    for version in 1..=SCHEMA_VERSION {
        let path = dir.path().join(format!("v{version}.vcw"));
        a_project_at(version, &path);

        // Read-only accepts it as it stands, and leaves it where it was. The
        // policy says an old file is incomplete rather than wrong, and this is
        // the half of that nothing else asserts.
        let reader = Project::open_read_only(&path)
            .unwrap_or_else(|e| panic!("a v{version} project should still open: {e}"));
        assert_eq!(
            reader.schema_version().unwrap(),
            version,
            "a read-only open migrated a v{version} project"
        );
        reader.close().unwrap();

        // Writing brings it current.
        let project = Project::open(&path)
            .unwrap_or_else(|e| panic!("a v{version} project should upgrade: {e}"));
        assert_eq!(
            project.schema_version().unwrap(),
            SCHEMA_VERSION,
            "a v{version} project did not reach the current schema"
        );

        // And current means current: the version number records which migrations
        // ran, so the dictionary is asked separately.
        let drift = migrate::drift(project.conn(), MIGRATIONS).unwrap();
        assert!(
            drift.is_empty(),
            "a v{version} project upgraded to a schema that differs from a new \
             one: {drift:?}"
        );
        project.close().unwrap();
    }
}

#[test]
fn a_version_above_this_build_is_refused_for_reading_as_well_as_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("from-the-future.vcw");
    Project::create(&path).unwrap().close().unwrap();

    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    conn.close().unwrap();

    // `roundtrip.rs` has the write half. This is the read half, which matters
    // because a read-only open is the one place that deliberately accepts a
    // version that is not this build's.
    for (how, err) in [
        (
            "read-only",
            Project::open_read_only(&path)
                .expect_err("a read-only open accepted a project from the future"),
        ),
        (
            "read-write",
            Project::open(&path).expect_err("a write open accepted a project from the future"),
        ),
    ] {
        match err {
            Error::SchemaTooNew {
                found, supported, ..
            } => {
                assert_eq!(found, SCHEMA_VERSION + 1, "{how}");
                assert_eq!(supported, SCHEMA_VERSION, "{how}");
            }
            other => panic!("a {how} open of a future project gave {other:?}"),
        }
    }
}

#[test]
fn no_migration_rewrites_captured_audio() {
    // `doc::parse` refuses anything but CREATE, ALTER and UPDATE outright, so an
    // INSERT or a DELETE in a migration fails this test by panicking in it. What
    // is left to check is which table the UPDATEs name.
    let audio = ["sampleblocks", "capture_blocks"];
    for migration in MIGRATIONS {
        for statement in doc::parse(migration.sql) {
            if statement.kind != doc::DATA {
                continue;
            }
            assert!(
                !audio.contains(&statement.name.as_str()),
                "migration {} rewrites {}, and the policy says an upgrade never \
                 touches captured audio:\n{}",
                migration.version,
                statement.name,
                statement.sql
            );
        }
    }
}

#[test]
fn the_policy_names_the_two_versions_this_build_uses() {
    let text = std::fs::read_to_string(policy()).unwrap_or_else(|e| {
        let path = policy();
        panic!("{}: {e}", path.display())
    });

    // The table row, read as a table row: `| `name` | where | number | about |`.
    let stated = |name: &str| -> u32 {
        let row = text
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("| `{name}`")))
            .unwrap_or_else(|| panic!("docs/COMPATIBILITY.md has no row for `{name}`"));
        let cell = row
            .split('|')
            .nth(3)
            .unwrap_or_else(|| panic!("the `{name}` row has no third column: {row}"))
            .trim();
        cell.parse().unwrap_or_else(|e| {
            panic!("the `{name}` row says `{cell}`, which is not a version: {e}")
        })
    };

    assert_eq!(
        stated("schema_version"),
        SCHEMA_VERSION,
        "docs/COMPATIBILITY.md states a schema version this build does not use"
    );
    assert_eq!(
        stated("format_version"),
        vcw_project::FORMAT_VERSION,
        "docs/COMPATIBILITY.md states a format version this build does not use"
    );
}
