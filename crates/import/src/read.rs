/*
 *  read.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Pulling the document blobs out of the project database.
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
//! Pulling the document blobs out of the project database.
//!
//! Four tables carry a document between them, and which one a reader picks is a
//! correctness question rather than a convenience:
//!
//! ```sql
//! project(id, dict, doc)                            -- the live document
//! autosave(id, dict, doc)                           -- work Audacity did not save
//! project_history(generation, saved_at, dict, doc)   -- AUP4 only, one row per save
//! ```
//!
//! - **`project` is the document.** AUP4's `project_history` generation 1 is
//!   byte-identical to the live row on a freshly converted file, so a careless
//!   query against the history table works by accident and then silently
//!   returns a stale document once the user saves again.
//! - **`autosave` is not empty on a project Audacity did not close cleanly**,
//!   and it holds that session's work. Importing `project` alone discards it, so
//!   [`survey`] reports it and the caller has to decide; there is no reading
//!   path here that can lose it quietly.

use rusqlite::{Connection, OptionalExtension as _};

use crate::doc::{Dict, Event, parse_dict, parse_doc};
use crate::error::{Error, Result};

/// One parsed document: its own name table, and its records in file order.
#[derive(Clone, Debug)]
pub struct Document {
    /// The file's name table. Per-file, so never cache an id from it.
    pub dict: Dict,
    /// Every record, in the order the file stored them.
    ///
    /// Order is not stable across a save or a conversion - an AUP3 to AUP4
    /// conversion reorders `tag` elements with no change of content - so this is
    /// the order to *read*, never the order to assert on.
    pub events: Vec<Event>,
    /// How many bytes the document blob was, which is what the parser consumed.
    pub doc_bytes: usize,
}

/// What a project holds before any of it is interpreted.
#[derive(Clone, Debug)]
pub struct Survey {
    /// The live document, from `project`.
    pub document: Document,
    /// An unsaved session, from `autosave`, when the project has one.
    ///
    /// `Some` means Audacity did not close this project cleanly and there is
    /// work here that the live document does not contain. Report it; do not
    /// silently prefer either one.
    pub autosave: Option<Document>,
    /// How many saves `project_history` holds, or `None` when the table does not
    /// exist, which is every AUP3.
    pub history_generations: Option<u32>,
}

/// Reads and parses the live document, plus enough about the rest of the file to
/// know whether reading only the live document would lose anything.
///
/// # Errors
///
/// [`Error::NoDocument`] if `project` has no usable row, any grammar error from
/// [`parse_doc`], or [`Error::Sqlite`].
pub fn survey(conn: &Connection, path: &std::path::Path) -> Result<Survey> {
    let document = document_from(conn, "project")?.ok_or_else(|| Error::NoDocument {
        path: path.to_path_buf(),
    })?;
    Ok(Survey {
        document,
        autosave: document_from(conn, "autosave")?,
        history_generations: history_generations(conn)?,
    })
}

/// One `(dict, doc)` row, either column of which SQLite may hand back as null.
type Row = (Option<Vec<u8>>, Option<Vec<u8>>);

/// Reads one `(dict, doc)` row from a table that has that shape.
///
/// `None` covers both an absent row and a null or empty `doc`, because an
/// `autosave` table with a row whose `doc` is empty means the same thing as no
/// row at all: nothing was left behind.
fn document_from(conn: &Connection, table: &str) -> Result<Option<Document>> {
    // The table name is one of this module's own two literals, never caller
    // input, which is why it can be formatted into the SQL at all. Any new call
    // site must keep that true - SQLite will not parameterise an identifier.
    debug_assert!(matches!(table, "project" | "autosave"));
    let sql = format!("SELECT dict, doc FROM {table} LIMIT 1");
    let row: Option<Row> = conn
        .query_row(&sql, [], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;

    let Some((dict_blob, Some(doc_blob))) = row else {
        return Ok(None);
    };
    if doc_blob.is_empty() {
        return Ok(None);
    }

    // A missing dictionary is not an error: a document that refers to no name
    // needs none, and it is the reference that fails, by id, if it does.
    let dict = match dict_blob {
        Some(blob) if !blob.is_empty() => parse_dict(&blob)?,
        _ => Dict::default(),
    };
    let doc_bytes = doc_blob.len();
    let (events, dict) = parse_doc(&doc_blob, dict)?;
    Ok(Some(Document {
        dict,
        events,
        doc_bytes,
    }))
}

/// How many saves `project_history` holds, or `None` on a file without the table.
///
/// Counted rather than read: the count is the fact worth having, because an
/// unpruned history is an import-time surprise in file size, and because it
/// distinguishes a converted file from a converted-and-then-edited one.
fn history_generations(conn: &Connection) -> Result<Option<u32>> {
    let present: Option<u32> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'project_history'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if present.is_none() {
        return Ok(None);
    }
    let generations: u32 =
        conn.query_row("SELECT COUNT(*) FROM project_history", [], |row| row.get(0))?;
    Ok(Some(generations))
}
