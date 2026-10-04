/*
 *  schema_doc.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The schema document is generated, and this is what keeps it honest.
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

//! The schema document is generated, and this is what keeps it honest.
//!
//! Two obligations, and they are separate. §49 promises the project format is
//! openly documented, so `docs/SCHEMA.md` must match the schema it claims to
//! describe. But a generator is only as good as its parser, so the parse is also
//! cross-checked against `PRAGMA table_info` on a real database - otherwise a
//! misreading would produce a document that is self-consistent and false.

use std::collections::BTreeSet;
use std::path::PathBuf;

use vcw_project::doc;
use vcw_project::schema::REQUIRED_TABLES;
use vcw_project::{Access, Project};

fn schema_md() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/SCHEMA.md")
}

/// A project on disk, so the cross-check runs against SQLite's own reading of
/// the DDL rather than against the DDL string a second time.
fn a_real_project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(dir.path().join("crosscheck.vcw")).unwrap();
    assert_eq!(project.access(), Access::ReadWrite);
    (dir, project)
}

#[test]
fn the_committed_document_matches_the_schema() {
    let generated = doc::markdown();
    let path = schema_md();

    if std::env::var_os("VCW_BLESS").is_some() {
        std::fs::write(&path, &generated).unwrap();
        return;
    }

    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}. Generate it with `VCW_BLESS=1 cargo test -p vcw-project`",
            path.display()
        )
    });

    if committed != generated {
        // Point at the first line that differs; a whole-file diff in a panic
        // message is unreadable.
        let at = committed
            .lines()
            .zip(generated.lines())
            .position(|(a, b)| a != b)
            .map(|i| format!("line {}", i + 1))
            .unwrap_or_else(|| "end of file (length differs)".to_owned());
        panic!(
            "docs/SCHEMA.md is stale, first difference at {at}.\n\
             Re-generate with `VCW_BLESS=1 cargo test -p vcw-project --test schema_doc`."
        );
    }
}

#[test]
fn the_parser_finds_every_table_the_database_has() {
    let (_dir, project) = a_real_project();

    let mut in_db: BTreeSet<String> = {
        let mut stmt = project
            .conn()
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    // `sqlite_sequence` is filtered above; AUTOINCREMENT creates it, and we do not
    // declare it, so it is correctly absent from the document.
    in_db.remove("sqlite_sequence");

    let parsed: BTreeSet<String> = doc::objects()
        .into_iter()
        .filter(|o| o.kind == "TABLE")
        .map(|o| o.name)
        .collect();

    assert_eq!(
        parsed, in_db,
        "the parser and the database disagree about which tables exist"
    );
    for &required in REQUIRED_TABLES {
        assert!(
            parsed.contains(required),
            "{required} is required but not documented"
        );
    }
}

#[test]
fn every_documented_column_exists_with_the_type_it_claims() {
    let (_dir, project) = a_real_project();

    for object in doc::objects() {
        if object.kind != "TABLE" {
            continue;
        }
        let actual: Vec<(String, String)> = {
            let mut stmt = project
                .conn()
                .prepare(&format!("PRAGMA table_info({})", object.name))
                .unwrap();
            stmt.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };

        let documented: Vec<String> = object.columns.iter().map(|c| c.name.clone()).collect();
        let real: Vec<String> = actual.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(
            documented, real,
            "column list for `{}` differs, and order matters",
            object.name
        );

        // The declaration the document prints must start with the type SQLite
        // actually resolved, so nobody reads `TEXT` where the column is INTEGER.
        for ((_, ty), column) in actual.iter().zip(&object.columns) {
            assert!(
                column
                    .declaration
                    .to_uppercase()
                    .starts_with(&ty.to_uppercase()),
                "`{}.{}` is declared `{}` but SQLite reports type `{ty}`",
                object.name,
                column.name,
                column.declaration
            );
        }
    }
}

#[test]
fn every_index_the_document_lists_is_really_there() {
    let (_dir, project) = a_real_project();

    let in_db: BTreeSet<String> = {
        let mut stmt = project
            .conn()
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'index' AND sql IS NOT NULL",
            )
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };

    let parsed: BTreeSet<String> = doc::objects()
        .into_iter()
        .filter(|o| o.kind == "INDEX")
        .map(|o| o.name)
        .collect();

    assert_eq!(
        parsed, in_db,
        "the parser and the database disagree about which indices exist"
    );
}

/// The comments in the DDL *are* the documentation, so an undocumented table is
/// a hole in §49's promise, not a style preference.
#[test]
fn nothing_is_documented_by_its_name_alone() {
    for object in doc::objects() {
        assert!(
            !object.comment.trim().is_empty(),
            "`{}` has no comment above it, so the schema document would explain nothing",
            object.name
        );
        for column in &object.columns {
            assert!(
                !column.comment.trim().is_empty(),
                "`{}.{}` has no comment",
                object.name,
                column.name
            );
        }
    }
}

/// An added column is documented on the table it was added to, last, and not as
/// an object of its own.
///
/// The parser reads the whole migration history, so a column added at v3 has to
/// find its way onto a table created at v1. Getting that wrong would be quiet: a
/// stray `ADD COLUMN` section, or a column documented nowhere.
#[test]
fn a_column_added_by_a_migration_joins_the_table_that_has_it() {
    let parsed = doc::parse(
        "-- Why the column is here.\n\
         ALTER TABLE captures ADD COLUMN capture_eq TEXT NOT NULL DEFAULT 'unknown';\n",
    );
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].kind, doc::ADDED_COLUMN);
    assert_eq!(parsed[0].name, "captures");
    assert_eq!(parsed[0].columns.len(), 1);
    assert_eq!(parsed[0].columns[0].name, "capture_eq");
    assert_eq!(
        parsed[0].columns[0].declaration, "TEXT NOT NULL DEFAULT 'unknown'",
        "the declaration is what the document prints, semicolon excluded"
    );
    assert_eq!(
        parsed[0].columns[0].comment, "Why the column is here.",
        "the block above an ALTER describes the column, there being nothing else"
    );

    let objects = doc::objects();
    assert!(
        objects.iter().all(|o| o.kind != doc::ADDED_COLUMN),
        "an added column is folded in, so nothing of this kind survives objects()"
    );
    let captures = objects
        .iter()
        .find(|o| o.kind == "TABLE" && o.name == "captures")
        .expect("the captures table is documented");
    assert_eq!(
        captures.columns.last().map(|c| c.name.as_str()),
        Some("capture_eq"),
        "SQLite appends an added column, so the document has to as well"
    );
    assert_eq!(
        captures
            .columns
            .iter()
            .filter(|c| c.name == "capture_eq")
            .count(),
        1,
        "folded once, not once per migration parsed"
    );
}
