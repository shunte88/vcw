/*
 *  doc.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Generating `docs/SCHEMA.md` from the schema, so the two cannot drift.
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

//! Generating `docs/SCHEMA.md` from the schema, so the two cannot drift.
//!
//! §49 promises the project format is openly documented. A schema document
//! maintained by hand keeps that promise for about one release, so this one is
//! generated: [`markdown`] parses every migration's DDL by way of [`objects`] -
//! comments included, since the
//! comments are the explanation - and `tests/schema_doc.rs` fails if the committed
//! file differs.
//!
//! The parser only has to handle DDL we wrote ourselves, so it is deliberately
//! strict and small. `tests/schema_doc.rs` cross-checks every table and column it
//! finds against `PRAGMA table_info` on a real database, so a parser that
//! misreads the schema fails the build rather than quietly documenting a fiction.

use std::fmt::Write as _;

use vcw_types::StorageFormat;

use crate::meta;
use crate::migrate::MIGRATIONS;
use crate::schema::{
    APPLICATION_ID, BLOCK_MILLIS, EXTENSION, FORMAT_VERSION, PAGE_SIZE, SCHEMA_VERSION,
    SUMMARY_64K_STRIDE, SUMMARY_256_STRIDE,
};

/// A column in a parsed `CREATE TABLE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// Column name.
    pub name: String,
    /// Declared type and constraints, as written.
    pub declaration: String,
    /// The `--` comment lines immediately above it, joined.
    pub comment: String,
}

/// A parsed schema object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// `TABLE` or `INDEX`.
    pub kind: String,
    /// Object name.
    pub name: String,
    /// The `--` comment block immediately above the statement, joined.
    pub comment: String,
    /// Columns, for a table. Empty for an index.
    pub columns: Vec<Column>,
    /// The statement, as written.
    pub sql: String,
}

/// Every object the current schema has, in the order the migrations create them.
///
/// The document describes what a project *is*, and what a project is, is the result
/// of running every migration. Parsing one version's DDL would document the file
/// VCW used to write.
#[must_use]
pub fn objects() -> Vec<Object> {
    MIGRATIONS.iter().flat_map(|m| parse(m.sql)).collect()
}

/// Parses DDL into objects, carrying the comments across.
///
/// # Panics
///
/// On DDL shapes it was not written for. That is intentional: the only input is
/// our own schema, and a silent misparse would produce documentation that looks
/// authoritative and is wrong.
pub fn parse(ddl: &str) -> Vec<Object> {
    let mut objects = Vec::new();
    let mut comment = Vec::new();
    let mut statement = String::new();

    for line in ddl.lines() {
        let trimmed = line.trim();
        if statement.is_empty() {
            if let Some(text) = trimmed.strip_prefix("--") {
                comment.push(text.trim().to_owned());
                continue;
            }
            if trimmed.is_empty() {
                comment.clear();
                continue;
            }
        }
        statement.push_str(line);
        statement.push('\n');
        if trimmed.ends_with(';') {
            objects.push(parse_statement(statement.trim_end(), &comment.join(" ")));
            statement.clear();
            comment.clear();
        }
    }
    assert!(
        statement.trim().is_empty(),
        "unterminated statement: {statement}"
    );
    objects
}

fn parse_statement(sql: &str, comment: &str) -> Object {
    let head = sql.split_whitespace().take(3).collect::<Vec<_>>();
    assert_eq!(head.first(), Some(&"CREATE"), "unexpected statement: {sql}");
    let kind = head[1].to_owned();
    let name = head[2].trim_end_matches('(').to_owned();

    let mut columns = Vec::new();
    if kind == "TABLE" {
        let body = sql
            .split_once('(')
            .expect("CREATE TABLE has a body")
            .1
            .rsplit_once(')')
            .expect("CREATE TABLE closes its body")
            .0;
        let mut pending = Vec::new();
        for line in body.lines() {
            let line = line.trim().trim_end_matches(',').trim();
            if let Some(text) = line.strip_prefix("--") {
                pending.push(text.trim().to_owned());
                continue;
            }
            if line.is_empty() {
                continue;
            }
            // Table-level constraints are not columns.
            if line.starts_with("UNIQUE")
                || line.starts_with("PRIMARY KEY")
                || line.starts_with("FOREIGN KEY")
                || line.starts_with("CHECK")
            {
                pending.clear();
                continue;
            }
            let (col, declaration) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            columns.push(Column {
                name: col.to_owned(),
                declaration: declaration.split_whitespace().collect::<Vec<_>>().join(" "),
                comment: pending.join(" "),
            });
            pending.clear();
        }
    }

    Object {
        kind,
        name,
        comment: comment.to_owned(),
        columns,
        sql: sql.to_owned(),
    }
}

/// Escapes a value for a markdown table cell. Schema comments contain shift
/// operators and bitwise ors, and an unescaped `|` silently splits the row.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

/// The keys §16 requires, with a one-line meaning each, for the document.
///
/// Kept here rather than in `meta` because the strings are the specification's
/// prose rather than the module's API, but the keys themselves come from
/// `meta`'s constants so the two cannot drift apart.
const REQUIRED_META: [(&str, &str); 6] = [
    (meta::CREATED_AT, "Unix seconds at creation."),
    (
        meta::CREATED_BY,
        "Crate name and version of the build that created the project.",
    ),
    (
        meta::CREATED_FORMAT_VERSION,
        "The format version at creation, which a migration never rewrites.",
    ),
    (
        meta::FORMAT_VERSION,
        "The format version as last written: the one a reader must understand.",
    ),
    (meta::LAST_WRITTEN_AT, "Unix seconds at the last write."),
    (
        meta::LAST_WRITTEN_BY,
        "Crate name and version of the build that last wrote to it.",
    ),
];

/// How one sample of a format is laid out in the `samples` blob.
///
/// Part of the document rather than a doc comment because §49's promise is that
/// a third party can read a project from the specification alone, and a byte
/// width without a layout is not enough to decode anything.
const fn layout_of(format: StorageFormat) -> &'static str {
    match format {
        StorageFormat::Int16 => "little-endian `i16`",
        StorageFormat::Int24Packed => "3-byte little-endian two's complement",
        StorageFormat::Int24Padded => "little-endian `i32`, value in +/-2^23",
        StorageFormat::Int32 => "little-endian `i32`",
        StorageFormat::Float32 => "little-endian IEEE-754 `f32`",
    }
}

/// What divisor takes a stored sample to roughly -1.0..=1.0.
const fn full_scale_of(format: StorageFormat) -> &'static str {
    match format {
        StorageFormat::Int16 => "32768",
        StorageFormat::Int24Packed | StorageFormat::Int24Padded => "8388608",
        StorageFormat::Int32 => "2147483648",
        StorageFormat::Float32 => "1.0 already",
    }
}

/// Renders the schema document.
pub fn markdown() -> String {
    let mut out = String::new();

    out.push_str(
        "<!-- Generated by `cargo test -p vcw-project`. Do not edit by hand:\n     \
         the source is crates/project/src/schema.rs, and tests/schema_doc.rs fails\n     \
         if this file and that one disagree. Re-generate with VCW_BLESS=1. -->\n\n",
    );
    out.push_str("# The `.vcw` project schema\n\n");
    out.push_str(
        "A VCW project is one SQLite file (§12): self-contained, movable, and\n\
         self-identifying. The schema is a deliberate superset of Audacity's AUP4 -\n\
         `sampleblocks` is reproduced column for column so that imported blocks need\n\
         no rewriting, and everything else is what Audacity has nowhere to put. See\n\
         [ADR-0001](adr/0001-project-format.md).\n\n",
    );

    out.push_str("## Identity\n\n");
    out.push_str("| | |\n|---|---|\n");
    let _ = writeln!(out, "| extension | `.{EXTENSION}` |");
    let _ = writeln!(
        out,
        "| `application_id` | `0x{APPLICATION_ID:08X}` (ASCII `VCW\\0`) - Audacity's is \
         `0x41554459`, `AUDY`, for *both* AUP3 and AUP4 |"
    );
    let _ = writeln!(
        out,
        "| `user_version` | {SCHEMA_VERSION} - the schema version, a plain ascending \
         integer |"
    );
    let _ = writeln!(
        out,
        "| format version | {FORMAT_VERSION} - the *meaning* of the schema, in `meta` |"
    );
    let _ = writeln!(out, "| page size | {PAGE_SIZE} bytes, set at creation |");
    let _ = writeln!(out, "| journal mode | WAL, `synchronous=FULL` (D3) |");
    out.push_str(
        "\nDispatch on `application_id` and `user_version`, never on the extension. \
         AUP3 and AUP4 share an `application_id` and differ only in `user_version`, \
         so a reader that trusts the file name is already guessing.\n\n",
    );

    out.push_str("## Capture parameters\n\n");
    let _ = writeln!(
        out,
        "Blocks are **{BLOCK_MILLIS} ms per channel** (D3, provisional from S2). The \
         figure is a recovery-granularity decision, not a throughput one: crash loss is \
         commit granularity plus the driver buffer, and this is the commit granularity.\n"
    );
    let _ = writeln!(
        out,
        "Summaries match Audacity exactly - `(min, max, rms)` f32 triplets over \
         {SUMMARY_256_STRIDE} and {SUMMARY_64K_STRIDE} samples. Measured overhead on a \
         774 MB project: 1.1%.\n"
    );

    out.push_str("## Sample format codes\n\n");
    out.push_str(
        "Audacity's encoding, `(bytes_per_sample << 16) | type_code`, extended with the \
         two formats it has no code for. Audacity never reads a `.vcw` file, so the \
         extension costs nothing; §8 requires 32-bit integer capture and D4 stores \
         24-bit verbatim rather than padded.\n\n",
    );
    out.push_str("| code | format | bytes | layout | full scale | origin |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for s in StorageFormat::ALL {
        let _ = writeln!(
            out,
            "| `0x{:08X}` | {:?} | {} | {} | {} | {} |",
            s.code(),
            s,
            s.bytes_per_sample(),
            layout_of(s),
            full_scale_of(s),
            if s.is_audacity() {
                "Audacity, identical meaning"
            } else {
                "VCW"
            }
        );
    }
    out.push('\n');
    out.push_str(
        "**Every multi-byte value in a `.vcw` is little-endian**: samples, summary \
         triplets, all of it. The schema comment on `sampleblocks.samples` says \
         \"native-endian, exactly as captured\" because D4 forbids the write path from \
         touching a sample, and that is accurate as far as it goes - but it is not \
         something a third-party reader can act on, so the format *declares* \
         little-endian and `vcw-project` refuses to compile on a big-endian target \
         rather than write a file nobody can read. If VCW is ever wanted on a \
         big-endian machine, the conversion belongs on the write path and this \
         sentence is the contract that says so.\n\n\
         `Int24Padded` is the one layout worth stating twice, because getting it \
         wrong is easy and quiet: it is a four-byte little-endian integer holding a \
         value in +/-2^23, **not** a 32-bit sample left-justified into four bytes. \
         Decoding it as though the low byte were padding makes a waveform 256x too \
         quiet. This was measured against Audacity's own `summin`/`summax`/`sumrms` \
         over the whole corpus rather than assumed.\n\n",
    );

    out.push_str("## Reading a project\n\n");
    out.push_str(
        "§49 requires the format to be usable by a third-party tool without the GUI, \
         and `tools/vcw-read.py` is the proof: a reader written from this document \
         alone, in Python, with no VCW code in it. \
         `crates/project/tests/third_party_spec.rs` runs it against a project the \
         product wrote and compares the audio it extracts frame for frame. \
         Everything a reader needs is below; where this document and the code \
         disagree, the code is the bug.\n\n\
         **Open it read-only, and open it with `mode=ro`.** A project is in WAL mode, \
         so content that has not been checkpointed lives in the `-wal` sidecar. \
         SQLite's `immutable=1` tells the library to ignore that file, which turns an \
         unflushed project into a silently stale one. `mode=ro` honours the sidecar \
         and costs only the creation of a `-shm`.\n\n\
         **Identify it from the header, not from the name.** `application_id` and \
         `user_version` are at offsets 68 and 60 of the first database page, \
         big-endian, which means a reader can refuse a file it should not touch \
         without opening a connection at all. An Audacity project answers \
         `0x41554459` to the same question.\n\n\
         **Then check `format_version` in `meta`.** `user_version` is the shape of \
         the tables; the format version is what they mean. A future VCW may add \
         tables without changing either, but it will not change the meaning of an \
         existing column without changing this.\n\n",
    );

    out.push_str("### Required `meta` keys\n\n");
    out.push_str(
        "Every project carries these (§16). A file missing one is rejected on open \
         rather than repaired, because a project that cannot say what wrote it \
         cannot be reasoned about.\n\n",
    );
    out.push_str("| key | meaning |\n|---|---|\n");
    for (key, meaning) in REQUIRED_META {
        let _ = writeln!(out, "| `{key}` | {meaning} |");
    }
    out.push_str(
        "\nEvery other key is a value some part of VCW stores because a column would \
         be a worse trade, and a reader may ignore all of them. Two families are \
         worth knowing about: `export.*` records what the last export was asked for, \
         and `import.*` records where imported audio came from, including \
         `import.tag.*`, which is every tag the source project carried, verbatim.\n\n",
    );

    out.push_str("### Getting the audio out\n\n");
    out.push_str(
        "Five steps, and none of them need anything outside this document.\n\n\
         1. Pick a capture from `captures`. It gives the rate, the channel count and \
            the `storage_format` code, and those three are fixed for the whole \
            capture - a project never changes format mid-recording.\n\
         2. For each channel `0..channels`, read its blocks: \
            `SELECT start_frame, frame_count, samples FROM capture_blocks JOIN \
            sampleblocks USING (blockid) WHERE capture_id = ? AND channel = ? ORDER \
            BY sequence`. \
            `sequence` is contiguous from zero with no gaps, so the ordering is total \
            and needs no tie-break.\n\
         3. **A block is one channel of audio, never interleaved.** Its `samples` \
            blob holds exactly `frame_count` samples in the capture's format, so it \
            is `frame_count * bytes_per_sample` bytes long. Anything else means a \
            damaged block, and `validate()` says so.\n\
         4. Interleave: frame *f* of the output is channel 0's sample *f* followed \
            by channel 1's, and so on. That is the only transform between a `.vcw` \
            and a WAV data chunk for the four integer formats - the bytes are \
            already what WAV wants.\n\
         5. `frames` on the capture row is the authoritative length per channel. It \
            is updated as blocks land rather than at the end, so it is correct even \
            on an interrupted capture.\n\n\
         A **track** is a span of one capture: `tracks` points at two rows of \
         `track_boundaries`, and `at_frame` on each is frames from the start of the \
         side's capture. **The span is half-open, `[start, end)`** - a track of *n* \
         frames runs from `start` to `start + n`, and the frame at `end` belongs to \
         whatever comes next, or to nothing. A side is not a span: two sides may \
         share one capture, and nothing in the schema says where one face ends.\n\n\
         **`checksum` is a CRC-32** over the `samples` blob: the ordinary reflected \
         IEEE 802.3 polynomial with an initial and final inversion, which is what \
         `zlib.crc32`, `binascii.crc32` and Rust's `crc32fast` all compute. It is \
         stored as a non-negative integer in `0..2^32`, so a reader whose SQLite \
         bindings hand back a signed 64-bit value needs no sign correction. The \
         column detects the bit rot SQLite's own integrity check cannot see: a \
         corrupted blob is still a valid blob.\n\n",
    );

    out.push_str("## Tables\n\n");
    for object in objects() {
        if object.kind != "TABLE" {
            continue;
        }
        let _ = writeln!(out, "### `{}`\n", object.name);
        if !object.comment.is_empty() {
            let _ = writeln!(out, "{}\n", object.comment);
        }
        out.push_str("| column | declaration | notes |\n|---|---|---|\n");
        for c in &object.columns {
            let _ = writeln!(
                out,
                "| `{}` | `{}` | {} |",
                c.name,
                cell(&c.declaration),
                cell(&c.comment)
            );
        }
        out.push('\n');
    }

    out.push_str("## Indices\n\n");
    for object in objects() {
        if object.kind != "INDEX" {
            continue;
        }
        let _ = writeln!(out, "- `{}`", object.name);
        if !object.comment.is_empty() {
            let _ = writeln!(out, "  {}", object.comment);
        }
    }
    out.push('\n');

    out.push_str("## What is not here yet\n\n");
    out.push_str(
        "Schema v1 covers capture - blocks, sessions, diagnostics and versioning, \
         everything milestone M1, *it records*, depends on - and v2 adds the vinyl \
         data model: the release, its artwork, its sides, their track boundaries and \
         the tracks between them. Identification evidence, export settings and \
         imported-project provenance arrive as later migrations, at WP-14, WP-20 and \
         WP-26. That is what the migration machinery is for, and writing those tables \
         now would be guessing at shapes several work packages away.\n\n\
         There is no `discs` table, deliberately. A disc carries no fact a side does \
         not already imply: side index 2 is disc 2's first face, by arithmetic, and a \
         disc row would be a second place to store the same thing.\n",
    );

    out
}
