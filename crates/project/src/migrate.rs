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
use crate::schema::{SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4};

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
        description: "capture equalisation provenance: captures.capture_eq",
        sql: SCHEMA_V3,
    },
    Migration {
        version: 4,
        description: "release intents: releases.is_mono, releases.riaa_eq",
        sql: SCHEMA_V4,
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
    fn the_equalisation_migration_only_adds_a_column() {
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
