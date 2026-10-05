/*
 *  migrations.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The migration runner, tested against a synthetic migration set.
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

//! The migration runner, tested against a synthetic migration set.
//!
//! Synthetic on purpose. There is exactly one real migration today, so testing
//! only against `MIGRATIONS` would test that a single step works and prove nothing
//! about the machinery §16 depends on. These fixtures exercise multi-step upgrades,
//! resumption from an intermediate version, and rollback.

use rusqlite::Connection;
use vcw_project::migrate::{self, Migration};

/// Migration 1 has to create `schema_migrations` itself, because the runner
/// records itself there - exactly as the real schema does.
const V1: &str = "
CREATE TABLE schema_migrations (
    version     INTEGER PRIMARY KEY,
    description TEXT    NOT NULL,
    applied_at  INTEGER NOT NULL,
    applied_by  TEXT    NOT NULL
);
CREATE TABLE alpha (id INTEGER PRIMARY KEY, a TEXT);
";

const V2: &str = "CREATE TABLE beta (id INTEGER PRIMARY KEY, b TEXT);";
const V3: &str = "ALTER TABLE alpha ADD COLUMN c INTEGER NOT NULL DEFAULT 0;";

const SET: &[Migration] = &[
    Migration {
        version: 1,
        description: "alpha",
        sql: V1,
    },
    Migration {
        version: 2,
        description: "beta",
        sql: V2,
    },
    Migration {
        version: 3,
        description: "alpha.c",
        sql: V3,
    },
];

/// A migration whose second statement fails, after the first has succeeded.
const BROKEN: &[Migration] = &[
    Migration {
        version: 1,
        description: "alpha",
        sql: V1,
    },
    Migration {
        version: 2,
        description: "half-good",
        sql: "CREATE TABLE gamma (id INTEGER PRIMARY KEY); CREATE TABLE alpha (oops);",
    },
    Migration {
        version: 3,
        description: "never reached",
        sql: "CREATE TABLE delta (id INTEGER);",
    },
];

fn db() -> Connection {
    Connection::open_in_memory().unwrap()
}

/// Everything in `sqlite_master`, as a comparable fingerprint of the schema.
fn fingerprint(conn: &Connection) -> Vec<(String, String, String)> {
    let mut stmt = conn
        .prepare("SELECT type, name, COALESCE(sql, '') FROM sqlite_master ORDER BY type, name")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn version(conn: &Connection) -> u32 {
    migrate::current_version(conn).unwrap()
}

fn tables(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[test]
fn an_empty_database_gets_every_migration_in_order() {
    let mut conn = db();
    assert_eq!(migrate::apply(&mut conn, SET).unwrap(), vec![1, 2, 3]);
    assert_eq!(version(&conn), 3);
    assert_eq!(tables(&conn), ["alpha", "beta", "schema_migrations"]);

    let recorded: Vec<(u32, String)> = {
        let mut stmt = conn
            .prepare("SELECT version, description FROM schema_migrations ORDER BY version")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(
        recorded,
        [
            (1, "alpha".into()),
            (2, "beta".into()),
            (3, "alpha.c".into())
        ]
    );
}

#[test]
fn applying_again_changes_nothing() {
    let mut conn = db();
    migrate::apply(&mut conn, SET).unwrap();
    let before = fingerprint(&conn);

    assert!(migrate::apply(&mut conn, SET).unwrap().is_empty());
    assert_eq!(fingerprint(&conn), before);
    assert_eq!(version(&conn), 3);
}

/// The property §16 actually needs: however far a project got, finishing the
/// migration lands it on the same schema as a fresh one.
#[test]
fn resuming_from_any_version_reaches_the_same_schema() {
    let mut fresh = db();
    migrate::apply(&mut fresh, SET).unwrap();
    let target = fingerprint(&fresh);

    for stop in 1..=SET.len() {
        let mut conn = db();
        migrate::apply(&mut conn, &SET[..stop]).unwrap();
        assert_eq!(version(&conn), stop as u32);

        let applied = migrate::apply(&mut conn, SET).unwrap();
        assert_eq!(applied, ((stop as u32 + 1)..=3).collect::<Vec<_>>());
        assert_eq!(
            fingerprint(&conn),
            target,
            "resuming from version {stop} diverged"
        );
        assert_eq!(version(&conn), 3);
    }
}

#[test]
fn pending_reports_what_is_left() {
    let mut conn = db();
    assert_eq!(migrate::pending(&conn, SET).unwrap().len(), 3);
    migrate::apply(&mut conn, &SET[..1]).unwrap();
    let left: Vec<u32> = migrate::pending(&conn, SET)
        .unwrap()
        .iter()
        .map(|m| m.version)
        .collect();
    assert_eq!(left, [2, 3]);
}

#[test]
fn a_failing_migration_leaves_no_trace() {
    let mut conn = db();
    let err = migrate::apply(&mut conn, BROKEN).unwrap_err();
    match err {
        vcw_project::Error::Migration {
            version,
            ref description,
            ..
        } => {
            assert_eq!(version, 2);
            assert_eq!(description, "half-good");
        }
        other => panic!("expected Migration, got {other:?}"),
    }

    // Migration 1 stands. Migration 2 is entirely gone, including the table its
    // first statement created. Migration 3 was never attempted.
    assert_eq!(version(&conn), 1);
    assert_eq!(tables(&conn), ["alpha", "schema_migrations"]);
    let recorded: i64 = conn
        .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(recorded, 1);
}

#[test]
fn a_failure_can_be_retried_once_the_migration_is_fixed() {
    let mut conn = db();
    migrate::apply(&mut conn, BROKEN).unwrap_err();
    assert_eq!(migrate::apply(&mut conn, SET).unwrap(), vec![2, 3]);
    assert_eq!(version(&conn), 3);
}

/// A fresh project is a migration run against an empty database, so the create
/// path and the upgrade path cannot drift apart. This is what proves it: the
/// migrations' own bookkeeping aside, running them must leave the same objects as
/// executing their DDL straight through.
#[test]
fn the_real_migration_set_builds_the_real_schema() {
    let mut migrated = db();
    migrate::apply(&mut migrated, vcw_project::MIGRATIONS).unwrap();

    let direct = db();
    for m in vcw_project::MIGRATIONS {
        direct.execute_batch(m.sql).unwrap();
    }

    let mut from_migration = fingerprint(&migrated);
    // The migration run inserts its own audit row; the fingerprint is structural,
    // so the only difference should be none at all.
    from_migration.retain(|(kind, name, _)| !(kind == "table" && name == "sqlite_sequence"));
    let mut from_ddl = fingerprint(&direct);
    from_ddl.retain(|(kind, name, _)| !(kind == "table" && name == "sqlite_sequence"));

    assert_eq!(from_migration, from_ddl);
    assert_eq!(version(&migrated), vcw_project::SCHEMA_VERSION);
}

/// A project captured before v3 keeps its audio and gains `capture_eq` as 'unknown'.
///
/// §16's promise is that a newer build opens an older project, and §51's is that
/// the equalisation is never guessed. A row written at v2 by definition says
/// nothing about the curve, so the only honest value for it is 'unknown' - and the
/// column arriving must not touch a byte of what was already there.
#[test]
fn an_older_project_gains_the_equalisation_column_as_unknown() {
    let mut conn = db();
    let upto_v2 = &vcw_project::MIGRATIONS
        .iter()
        .filter(|m| m.version <= 2)
        .copied()
        .collect::<Vec<_>>()[..];
    assert_eq!(migrate::apply(&mut conn, upto_v2).unwrap(), vec![1, 2]);
    assert_eq!(version(&conn), 2);

    // The insert a v2 build would have written: no capture_eq, because there is no
    // such column to write.
    conn.execute(
        "INSERT INTO captures
            (capture_id, sample_rate, channels, storage_format, capture_mode,
             started_at, finished_at, frames, state)
         VALUES (1, 96000, 2, 262148, 'Exclusive', 1700000000, 1700003600, 345600000,
                 'finalised')",
        [],
    )
    .unwrap();
    assert!(
        conn.prepare("SELECT capture_eq FROM captures").is_err(),
        "the column cannot exist before the migration that adds it"
    );

    // Every migration from 3 up, derived rather than written out: this test is
    // about what happens to the v2 *row*, and it should not need editing every
    // time a later migration is added.
    assert_eq!(
        migrate::apply(&mut conn, vcw_project::MIGRATIONS).unwrap(),
        (3..=vcw_project::SCHEMA_VERSION).collect::<Vec<_>>()
    );
    assert_eq!(version(&conn), vcw_project::SCHEMA_VERSION);

    let (eq, frames, state): (String, i64, String) = conn
        .query_row(
            "SELECT capture_eq, frames, state FROM captures WHERE capture_id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        eq, "unknown",
        "a v2 row said nothing, so it still says nothing"
    );
    assert_eq!(
        frames, 345_600_000,
        "the migration rewrote a row it should not have"
    );
    assert_eq!(state, "finalised");
}

/// A release written before v4 keeps what it said and gains two unstated intents.
#[test]
fn an_older_release_gains_the_setup_intents_as_unstated() {
    let mut conn = db();
    let upto_v3 = &vcw_project::MIGRATIONS
        .iter()
        .filter(|m| m.version <= 3)
        .copied()
        .collect::<Vec<_>>()[..];
    migrate::apply(&mut conn, upto_v3).unwrap();

    conn.execute(
        "INSERT INTO releases (release_id, album, album_artist, updated_at)
         VALUES (1, 'Vienna', 'Ultravox', 1700000000)",
        [],
    )
    .unwrap();
    assert!(
        conn.prepare("SELECT is_mono FROM releases").is_err(),
        "the column cannot exist before the migration that adds it"
    );

    migrate::apply(&mut conn, vcw_project::MIGRATIONS).unwrap();
    assert_eq!(version(&conn), vcw_project::SCHEMA_VERSION);

    let (album, mono, riaa): (String, i64, i64) = conn
        .query_row(
            "SELECT album, is_mono, riaa_eq FROM releases WHERE release_id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        album, "Vienna",
        "the migration rewrote a row it should not have"
    );
    assert_eq!(mono, 0, "nobody was asked, so nothing was stated");
    assert_eq!(riaa, 0);
}
