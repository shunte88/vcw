/*
 *  migrate.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Transactional, forward-only schema migrations (§16).
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

//! Transactional, forward-only schema migrations (§16).
//!
//! Every migration runs inside its own transaction together with the bookkeeping
//! that records it, so a migration either happened or did not. There is no state in
//! which the DDL landed but `user_version` and `schema_migrations` disagree with it.
//!
//! Forward-only is a deliberate limitation. §16 asks that a newer application
//! upgrade an older project; it does not ask for downgrade, and a downgrade path
//! would have to decide what to do with data the older schema cannot hold. Opening
//! a project from the future is refused instead - see [`crate::Error::SchemaTooNew`].

use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::schema::{SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6};

/// One step from schema version `version - 1` to `version`.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// The version this step produces.
    pub version: u32,
    /// What it does, recorded in `schema_migrations` and shown if it fails.
    pub description: &'static str,
    /// The DDL, executed as a batch.
    pub sql: &'static str,
}

/// The migrations that build the current schema, in ascending order.
///
/// Migration 1 *is* the schema: a fresh project is a migration run against an empty
/// database, so the create path and the upgrade path cannot drift apart.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "initial capture schema",
        sql: SCHEMA_V1,
    },
    Migration {
        version: 2,
        description: "vinyl data model: releases, artwork, sides, boundaries, tracks",
        sql: SCHEMA_V2,
    },
    Migration {
        version: 3,
        description: "capture equalization provenance: captures.capture_eq",
        sql: SCHEMA_V3,
    },
    Migration {
        version: 4,
        description: "release intents: releases.is_mono, releases.riaa_eq",
        sql: SCHEMA_V4,
    },
    Migration {
        version: 5,
        description: "named playback characteristic: playback_eq on releases, sides and tracks",
        sql: SCHEMA_V5,
    },
    Migration {
        version: 6,
        description: "the defect list: spans of damage, and the repair proposed for each",
        sql: SCHEMA_V6,
    },
];

/// Identifies the build that applied a migration, for the audit row.
pub const APPLIED_BY: &str = concat!("vcw-project ", env!("CARGO_PKG_VERSION"));

/// The schema version recorded in the file.
pub fn current_version(conn: &Connection) -> rusqlite::Result<u32> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map(|v| v as u32)
}

/// Migrations in `set` that have not been applied to `conn`.
pub fn pending<'a>(
    conn: &Connection,
    set: &'a [Migration],
) -> rusqlite::Result<Vec<&'a Migration>> {
    let at = current_version(conn)?;
    Ok(set.iter().filter(|m| m.version > at).collect())
}

/// Applies every pending migration in `set`, in order. Returns the versions applied.
///
/// Each step is one transaction. A failure rolls that step back and stops: later
/// migrations are not attempted, because they were written against a schema that
/// does not exist.
pub fn apply(conn: &mut Connection, set: &[Migration]) -> Result<Vec<u32>> {
    debug_assert!(
        set.windows(2).all(|w| w[0].version < w[1].version),
        "migrations must be ascending and unique"
    );

    let mut applied = Vec::new();
    for m in pending(conn, set)? {
        let tx = conn.transaction()?;
        let step = (|| -> rusqlite::Result<()> {
            tx.execute_batch(m.sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, description, applied_at, applied_by)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![m.version, m.description, crate::now(), APPLIED_BY],
            )?;
            // Not a bindable parameter: PRAGMA takes a literal.
            tx.pragma_update(None, "user_version", m.version)?;
            Ok(())
        })();

        match step {
            Ok(()) => {
                tx.commit()?;
                // At `info`, because a schema change is the one thing that
                // happens to somebody's project without them asking for it.
                tracing::info!(
                    version = m.version,
                    description = m.description,
                    "applied a schema migration"
                );
                applied.push(m.version);
            }
            Err(source) => {
                // Explicit, though the Drop impl would also roll back.
                let _ = tx.rollback();
                tracing::error!(
                    version = m.version,
                    description = m.description,
                    error = %source,
                    "a schema migration failed and was rolled back"
                );
                return Err(Error::Migration {
                    version: m.version,
                    description: m.description.to_owned(),
                    source,
                });
            }
        }
    }
    Ok(applied)
}

/// How one schema object in a project differs from the one this build defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum How {
    /// This build defines it and the project has not got it. Carries the DDL
    /// that would create it, taken from the dictionary rather than written down.
    Missing(String),
    /// The project has it and this build does not define it.
    Unexpected,
    /// Both have it, under different definitions.
    Differs {
        /// What this build's migrations produce.
        expected: String,
        /// What the project's dictionary says.
        found: String,
    },
}

/// One object a project and this build disagree about. See [`drift`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    /// `sqlite_master.type`: `table`, `index`, `trigger` or `view`.
    pub kind: String,
    /// The object's name.
    pub name: String,
    /// Which way it differs.
    pub how: How,
}

/// A stored `CREATE` statement reduced to the part that is the schema.
///
/// The DDL in `schema.rs` is commented line by line and SQLite stores the
/// statement verbatim, comments and indentation included. Two projects can
/// therefore hold the same table under different text because somebody reworded
/// a comment - and both of `captures` and `capture_blocks` have been reworded
/// since the schema was first written, so a raw comparison reports two false
/// differences on every project old enough to matter. Comments and whitespace
/// come out; what is left is the columns, the constraints and their order.
///
/// Not an SQL parser and not trying to be: a `--` inside a string literal would
/// be mangled, and no statement in `schema.rs` has one. The alternative is
/// `PRAGMA table_info` per object, which cannot see a `CHECK`, a `UNIQUE` or the
/// expression an index is on.
#[must_use]
pub fn canonical(sql: &str) -> String {
    sql.lines()
        .map(|line| match line.find("--") {
            Some(at) => &line[..at],
            None => line,
        })
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every schema object the migrations produce, as `(type, name, sql)`, sorted.
///
/// Built by running them, not by writing them down a second time. Objects SQLite
/// manages itself - `sqlite_sequence`, the autoindexes, `sqlite_stat1` - are left
/// out: they follow from the DDL that is compared, and `sqlite_sequence` in
/// particular only materializes once a row is inserted, so a project with data
/// would otherwise always look different from a freshly built one.
pub fn expected(set: &[Migration]) -> Result<Vec<(String, String, String)>> {
    let mut conn = Connection::open_in_memory()?;
    apply(&mut conn, set)?;
    Ok(dictionary(&conn)?)
}

/// The project's own schema objects, in the same shape as [`expected`].
fn dictionary(conn: &Connection) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT type, name, COALESCE(sql, '') FROM sqlite_master
         WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect()
}

/// What a project's data dictionary says that this build's schema does not, or
/// the other way round.
///
/// `user_version` records which migrations *ran*. It does not record what they
/// produced, and the two can part company: `MIGRATIONS[0]` **is** `SCHEMA_V1`,
/// which is what stops the create path and the upgrade path drifting - but
/// migration 1 only ever runs against an empty database, so an edit to
/// `SCHEMA_V1` reaches every project made afterwards and no project made before.
/// One such edit has happened: two covering indexes were added to the waveform
/// tables in September 2026. A project written the week before migrates to the
/// current version, is current by every version number in it, and still answers
/// `no such index` when the pyramid is read. Nothing in `schema_migrations` can
/// say so, because every migration it names did run.
///
/// So this asks the dictionary instead. That is also why it reports object by
/// object rather than returning a yes or a no: a missing covering index is a
/// slow query, a missing table is a verb that cannot run, and an object nobody
/// here defines is somebody else's and not ours to touch. Only the caller can
/// weigh those, so each one is named, and [`How::Missing`] carries the statement
/// that would create it.
///
/// Comparison is on [`canonical`] text: the statement with its comments and
/// indentation taken out. It can be that literal because the migrated path and
/// the create path execute the same statements in the same order, so a project
/// built by migration and one built fresh hold the same columns in the same
/// order. What it must not be is *exact*, because the comments in `schema.rs`
/// get reworded and a difference in prose is not a difference in schema.
pub fn drift(conn: &Connection, set: &[Migration]) -> Result<Vec<Drift>> {
    let expected = expected(set)?;
    let found = dictionary(conn)?;
    let mut out = Vec::new();

    for (kind, name, sql) in &expected {
        match found.iter().find(|(k, n, _)| k == kind && n == name) {
            None => out.push(Drift {
                kind: kind.clone(),
                name: name.clone(),
                how: How::Missing(sql.clone()),
            }),
            Some((_, _, theirs)) if canonical(theirs) != canonical(sql) => out.push(Drift {
                kind: kind.clone(),
                name: name.clone(),
                how: How::Differs {
                    expected: sql.clone(),
                    found: theirs.clone(),
                },
            }),
            Some(_) => {}
        }
    }
    for (kind, name, _) in &found {
        if !expected.iter().any(|(k, n, _)| k == kind && n == name) {
            out.push(Drift {
                kind: kind.clone(),
                name: name.clone(),
                how: How::Unexpected,
            });
        }
    }
    Ok(out)
}

/// The highest version `set` can produce.
pub fn target_version(set: &[Migration]) -> u32 {
    set.iter().map(|m| m.version).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_migration_set_reaches_the_declared_schema_version() {
        assert_eq!(target_version(MIGRATIONS), crate::schema::SCHEMA_VERSION);
    }

    #[test]
    fn a_project_the_migrations_built_has_no_drift() {
        let mut conn = Connection::open_in_memory().expect("in memory");
        apply(&mut conn, MIGRATIONS).expect("apply");
        assert_eq!(drift(&conn, MIGRATIONS).expect("drift"), []);
    }

    #[test]
    fn drift_names_the_object_and_carries_the_statement_that_restores_it() {
        // The shape of the real case: `SCHEMA_V1` grew two covering indexes in
        // September 2026, and migration 1 only ever runs against an empty
        // database, so a project written the week before is current by every
        // version number in it and has neither index. Simulated here by dropping
        // one, because the file that proves it is a pre-release soak fixture.
        let mut conn = Connection::open_in_memory().expect("in memory");
        apply(&mut conn, MIGRATIONS).expect("apply");
        conn.execute_batch("DROP INDEX sampleblocks_summary256;")
            .expect("drop the index");

        let found = drift(&conn, MIGRATIONS).expect("drift");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, "index");
        assert_eq!(found[0].name, "sampleblocks_summary256");
        let How::Missing(sql) = &found[0].how else {
            panic!("expected a missing index, got {:?}", found[0].how)
        };

        // The statement is the remedy, not a description of one: run it and the
        // drift is gone. A hand-written list of expected objects could not make
        // this claim, which is the argument for asking the dictionary.
        conn.execute_batch(sql).expect("the carried statement runs");
        assert_eq!(drift(&conn, MIGRATIONS).expect("drift"), []);
    }

    #[test]
    fn rewording_a_comment_is_not_a_schema_difference() {
        // Both `captures` and `capture_blocks` have had their comments reworded
        // since the schema was first written, and SQLite stores a CREATE statement
        // verbatim. A raw text comparison therefore reports two differences on
        // every project written before those edits - which is every project this
        // check exists for.
        let then = "CREATE TABLE t (\n  -- the old wording\n  a INTEGER NOT NULL\n)";
        let now = "CREATE TABLE t (\n    -- a much longer and better wording, -- even\n    a INTEGER NOT NULL\n)";
        assert_eq!(canonical(then), canonical(now));
        assert_eq!(canonical(then), "CREATE TABLE t ( a INTEGER NOT NULL )");

        // And it still sees a column.
        let later = "CREATE TABLE t (a INTEGER NOT NULL, b TEXT)";
        assert_ne!(canonical(then), canonical(later));
    }

    #[test]
    fn an_object_nobody_here_defines_is_reported_rather_than_ignored() {
        // Somebody else's index on our table. It is not ours to remove - another
        // tool may depend on it - but a dictionary comparison that stayed quiet
        // about it would be a comparison in one direction only.
        let mut conn = Connection::open_in_memory().expect("in memory");
        apply(&mut conn, MIGRATIONS).expect("apply");
        conn.execute_batch("CREATE INDEX theirs ON sampleblocks (blockid);")
            .expect("their index");

        let found = drift(&conn, MIGRATIONS).expect("drift");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "theirs");
        assert_eq!(found[0].how, How::Unexpected);
    }

    #[test]
    fn the_vinyl_migration_only_adds_tables() {
        // §16 allows a newer build to upgrade an older project, and the thing that
        // makes that safe is that migration 2 creates rows nobody had rather than
        // rewriting rows somebody did. A project captured at v1 keeps every byte of
        // its audio, so the check is on the DDL itself: no ALTER, no UPDATE, no DROP.
        let sql = SCHEMA_V2.to_ascii_uppercase();
        for forbidden in ["ALTER ", "UPDATE ", "DROP ", "DELETE ", "INSERT "] {
            assert!(
                !sql.contains(forbidden),
                "migration 2 contains {forbidden}, so it is not purely additive"
            );
        }
    }

    #[test]
    fn the_equalization_migration_only_adds_a_column() {
        // Same promise as migration 2, kept a different way: this one has to ALTER,
        // so the check is that the ALTER is the additive kind. ADD COLUMN with a
        // NOT NULL DEFAULT is metadata-only in SQLite - it rewrites no rows, which
        // is what makes it safe to run on open against a 2 GiB project. DROP COLUMN
        // and RENAME COLUMN rewrite the table and would not be.
        let sql = SCHEMA_V3
            .lines()
            .filter(|l| !l.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_uppercase();
        for forbidden in [
            "UPDATE ", "DROP ", "DELETE ", "INSERT ", "RENAME ", "CREATE ",
        ] {
            assert!(
                !sql.contains(forbidden),
                "migration 3 contains {forbidden}, so it is not purely additive"
            );
        }
        assert!(sql.contains("ALTER TABLE CAPTURES ADD COLUMN"));
    }

    #[test]
    fn the_release_intent_migration_only_adds_columns() {
        // Migration 3's promise, kept for the two columns the setup prompt writes.
        // Both default to 0, so an existing project reads as "nobody was asked",
        // and no row is rewritten on open.
        let sql = SCHEMA_V4
            .lines()
            .filter(|l| !l.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_uppercase();
        for forbidden in [
            "UPDATE ", "DROP ", "DELETE ", "INSERT ", "RENAME ", "CREATE ",
        ] {
            assert!(
                !sql.contains(forbidden),
                "migration 4 contains {forbidden}, so it is not purely additive"
            );
        }
        assert_eq!(
            sql.matches("ALTER TABLE RELEASES ADD COLUMN").count(),
            2,
            "migration 4 is two added columns and nothing else"
        );
    }

    #[test]
    fn migrations_are_ascending_and_unique() {
        assert!(MIGRATIONS.windows(2).all(|w| w[0].version < w[1].version));
        assert_eq!(MIGRATIONS.first().map(|m| m.version), Some(1));
    }
}
