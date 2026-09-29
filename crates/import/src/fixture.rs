/*
 *  fixture.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Shrinking a real Audacity project into a committable test fixture.
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
//! Shrinking a real Audacity project into a committable test fixture.
//!
//! # Why this ships
//!
//! The 30-project corpus cannot be committed - the smallest is 271 MB - and a
//! fixture written by this crate's author would only prove that our encoder
//! agrees with our decoder, which is worth nothing. So the fixtures are made by
//! *deleting* from real projects. Records are self-delimiting, which is what
//! makes that possible: dropping all but the first `waveblock` of a sequence is
//! a byte-range deletion, and every byte that survives is a byte Audacity wrote.
//!
//! # What is not Audacity's
//!
//! Four things, and they are listed because "genuine Audacity bytes" has to mean
//! something:
//!
//! 1. **`numsamples`, `waveblock/@start` and `waveblock/@length` are rewritten**
//!    to describe what is left. Patched in place, in the record's own width, so
//!    the document's shape is untouched.
//! 2. **`trimLeft` and `trimRight` are zeroed.** A trim that described a
//!    sequence of 2.7 M samples is meaningless against one of 256, and left in
//!    place it would trim the fixture's audio away entirely.
//! 3. **Sample data is replaced with zeros** and the summary statistics with
//!    `0.0`. This is deliberate and not only for size: the fixture is a test of
//!    the *document* grammar and the block bookkeeping, and it has no business
//!    carrying 85 ms of somebody's commercial vinyl into a git repository. It
//!    follows that a fixture cannot prove sample decoding - only the corpus
//!    tests can, and that is what they are for.
//! 4. **The thumbnail's PNG payload is truncated** to its first few bytes. It is
//!    a screenshot of the user's editor window, so it does not go in the
//!    repository either; what remains is a genuine `0x10` record with a genuine
//!    byte count, which is the part a reader has to get right.
//!
//! Everything else - the dictionary, the element structure, every other
//! attribute, the page size, `application_id`, `user_version`, the
//! `project_history` table's existence - is the file Audacity wrote.
//!
//! # The source is never opened for writing
//!
//! These are the user's irreplaceable rips. The read is `SQLITE_OPEN_READ_ONLY`
//! and the copy is made by `VACUUM INTO`, which builds a fresh database from a
//! read-only connection: it also folds in any WAL content, so the copy cannot be
//! the stale-database trap that `immutable=1` would have been.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::Path;

use rusqlite::Connection;

use crate::doc::{Dict, Event, Record, parse_dict, parse_doc_spans};
use crate::error::{Error, Result};
use crate::model::SampleFormat;
use crate::sniff;

/// How much of a project to keep.
#[derive(Clone, Copy, Debug)]
pub struct Shrink {
    /// Blocks to keep per sequence.
    ///
    /// One is enough for the grammar and keeps the clip structure: a 38-clip
    /// project stays a 38-clip project.
    pub blocks_per_sequence: usize,
    /// Samples to keep in each surviving block.
    ///
    /// A real block holds 262,144. The blocks in a fixture are the short last
    /// block a sequence is allowed to end on, which is a shape Audacity itself
    /// produces.
    pub samples_per_block: u64,
    /// Bytes of each binary blob to keep.
    ///
    /// Enough to be a blob and not enough to be a picture.
    pub blob_bytes: usize,
}

impl Default for Shrink {
    fn default() -> Self {
        Self {
            blocks_per_sequence: 1,
            samples_per_block: 256,
            blob_bytes: 64,
        }
    }
}

/// What a shrink did, for the operator to check against the file it produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shrunk {
    /// Blocks the fixture still references.
    pub blocks_kept: usize,
    /// Rows deleted from `sampleblocks`.
    pub blocks_removed: usize,
    /// `waveblock` records deleted from the document.
    pub refs_removed: usize,
    /// `waveblock` records the document still carries.
    ///
    /// Not `blocks_kept`: a fixture made from a clip-split project keeps the
    /// sharing, so there are more references than blocks, which is the whole
    /// reason such a fixture is worth having.
    pub refs_kept: usize,
    /// Document size before and after, in bytes.
    pub doc_bytes: (usize, usize),
    /// `project_history` rows deleted. Zero on an AUP3.
    pub history_removed: usize,
    /// The fixture's size on disk.
    pub file_bytes: u64,
}

/// Builds a fixture at `destination` from the project at `source`.
///
/// `source` is opened read-only and is never written to. `destination` must not
/// already exist, because `VACUUM INTO` will not overwrite and this is not the
/// place to be clever about that.
///
/// # Errors
///
/// [`Error::Sqlite`] if the copy or any statement fails, any grammar error from
/// reading the source's document, and [`Error::UnpatchableRecord`] if a number
/// this has to rewrite is in a record whose width cannot hold the new value -
/// which would mean the document's shape had changed under us.
pub fn shrink(source: &Path, destination: &Path, how: &Shrink) -> Result<Shrunk> {
    // Read-only, and `VACUUM INTO` rather than a file copy: it reads through the
    // WAL, so the fixture cannot be built from a stale database, and it leaves
    // no sidecar behind.
    let (read_only, _) = sniff::open(source)?;
    read_only.execute("VACUUM INTO ?1", [destination.to_string_lossy()])?;
    drop(read_only);

    let conn = Connection::open(destination)?;
    let (mut doc, dict) = document(&conn)?;
    let doc_before = doc.len();
    let stored = stored_blocks(&conn)?;

    let plan = plan(&doc, dict, &stored, how)?;
    apply(&mut doc, &plan)?;
    conn.execute("UPDATE project SET doc = ?1", [&doc])?;

    let history_removed = clear_history(&conn)?;
    let blocks_removed = trim_blocks(&conn, &plan.keep, &stored, how)?;
    // Last, so the file on disk is the size the fixture actually needs rather
    // than the size the deletions left behind.
    conn.execute_batch("VACUUM")?;
    drop(conn);

    Ok(Shrunk {
        blocks_kept: plan.keep.len(),
        blocks_removed,
        refs_removed: plan.remove.len(),
        refs_kept: plan.kept_refs,
        doc_bytes: (doc_before, doc.len()),
        history_removed,
        file_bytes: std::fs::metadata(destination).map(|m| m.len()).unwrap_or(0),
    })
}

/// The live document blob and the dictionary that resolves it.
fn document(conn: &Connection) -> Result<(Vec<u8>, Dict)> {
    let (dict_blob, doc): (Option<Vec<u8>>, Vec<u8>) =
        conn.query_row("SELECT dict, doc FROM project LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    let dict = match dict_blob {
        Some(blob) if !blob.is_empty() => parse_dict(&blob)?,
        _ => Dict::default(),
    };
    Ok((doc, dict))
}

/// Every stored block's id and sample count.
fn stored_blocks(conn: &Connection) -> Result<BTreeMap<i64, u64>> {
    let mut statement =
        conn.prepare("SELECT blockid, sampleformat, length(samples) FROM sampleblocks")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut stored = BTreeMap::new();
    for row in rows {
        let (blockid, code, bytes) = row?;
        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "sampleformat is a 32-bit code stored in a SQLite INTEGER, and \
                      length() is never negative"
        )]
        let per_sample = SampleFormat::from_code(code as u32)?.bytes_per_sample() as u64;
        #[expect(clippy::cast_sign_loss, reason = "length() is never negative")]
        stored.insert(blockid, bytes as u64 / per_sample);
    }
    Ok(stored)
}

/// One number to rewrite in place, and what to rewrite it to.
struct Patch {
    span: Range<usize>,
    value: Value,
}

/// What a patch writes.
enum Value {
    /// An integer, in whatever width the record already uses.
    Integer(u64),
    /// A double, in a `0x0A` record, leaving the precision hint alone.
    Double(f64),
    /// A length-prefixed blob, truncated to this many bytes. Changes the
    /// record's length, so it is applied as a deletion of the tail.
    Blob(usize),
}

/// Every edit the fixture needs, computed before any of it is applied.
struct Plan {
    /// Byte ranges to delete, in ascending order and non-overlapping.
    remove: Vec<Range<usize>>,
    /// Numbers to rewrite in place.
    patch: Vec<Patch>,
    /// The blocks the fixture keeps.
    keep: BTreeSet<i64>,
    /// How many `waveblock` records survive.
    kept_refs: usize,
}

/// Works out what to delete and what to rewrite.
///
/// Computed in one pass and applied in another, because a deletion moves every
/// span after it: deciding and editing at the same time is the classic way to
/// corrupt a document with an off-by-one that only shows up in the second clip.
fn plan(doc: &[u8], dict: Dict, stored: &BTreeMap<i64, u64>, how: &Shrink) -> Result<Plan> {
    let (records, _) = parse_doc_spans(doc, dict)?;
    let mut plan = Plan {
        remove: Vec::new(),
        patch: Vec::new(),
        keep: BTreeSet::new(),
        kept_refs: 0,
    };

    let mut sequence: Option<Sequence> = None;
    let mut block: Option<Block> = None;

    for record in &records {
        match &record.event {
            Event::Start(name) if &**name == "sequence" => sequence = Some(Sequence::default()),
            Event::Start(name) if &**name == "waveblock" => {
                block = Some(Block {
                    span: record.span.start..record.span.end,
                    ..Block::default()
                });
            }
            Event::End(name) if &**name == "waveblock" => {
                if let (Some(mut done), Some(open)) = (block.take(), sequence.as_mut()) {
                    done.span.end = record.span.end;
                    open.blocks.push(done);
                }
            }
            Event::End(name) if &**name == "sequence" => {
                if let Some(done) = sequence.take() {
                    done.finish(&mut plan, stored, how)?;
                }
            }
            Event::Attr { name, .. } => {
                note_attribute(record, name, &mut block, &mut sequence, &mut plan, how);
            }
            _ => {}
        }
    }

    plan.remove.sort_by_key(|range| range.start);
    plan.patch.sort_by_key(|patch| patch.span.start);
    Ok(plan)
}

/// Records where an attribute we care about lives.
fn note_attribute(
    record: &Record,
    name: &str,
    block: &mut Option<Block>,
    sequence: &mut Option<Sequence>,
    plan: &mut Plan,
    how: &Shrink,
) {
    let Event::Attr { value, .. } = &record.event else {
        return;
    };
    if let Some(open) = block.as_mut() {
        match name {
            "blockid" => open.blockid = value.as_u64().map(cast_blockid),
            "start" => open.start = Some(record.span.clone()),
            "length" => open.length = Some(record.span.clone()),
            _ => {}
        }
        return;
    }
    if let Some(open) = sequence.as_mut() {
        if name == "numsamples" {
            open.numsamples = Some(record.span.clone());
        }
        return;
    }
    match name {
        // A trim that described 2.7 M samples would trim a 256-sample fixture
        // out of existence.
        "trimLeft" | "trimRight" => plan.patch.push(Patch {
            span: record.span.clone(),
            value: Value::Double(0.0),
        }),
        // The editor screenshot. Truncated rather than removed, so the fixture
        // still carries a real 0x10 record: that tag is AUP4's only new one and
        // the only thing in the format whose length is already a byte count.
        _ => {
            if let crate::doc::Value::Blob(bytes) = value
                && bytes.len() > how.blob_bytes
            {
                plan.patch.push(Patch {
                    span: record.span.clone(),
                    value: Value::Blob(how.blob_bytes),
                });
            }
        }
    }
}

/// `blockid` as SQLite stores it.
#[expect(
    clippy::cast_possible_wrap,
    reason = "blockid is an INTEGER PRIMARY KEY, which is signed in SQLite"
)]
const fn cast_blockid(value: u64) -> i64 {
    value as i64
}

/// One `waveblock` being read.
#[derive(Default)]
struct Block {
    span: Range<usize>,
    blockid: Option<i64>,
    start: Option<Range<usize>>,
    length: Option<Range<usize>>,
}

/// One `sequence` being read.
#[derive(Default)]
struct Sequence {
    numsamples: Option<Range<usize>>,
    blocks: Vec<Block>,
}

impl Sequence {
    /// Decides which of this sequence's blocks survive and what they now say.
    fn finish(self, plan: &mut Plan, stored: &BTreeMap<i64, u64>, how: &Shrink) -> Result<()> {
        let mut at = 0_u64;
        for (index, block) in self.blocks.iter().enumerate() {
            if index >= how.blocks_per_sequence {
                plan.remove.push(block.span.clone());
                continue;
            }
            let Some(blockid) = block.blockid else {
                // A waveblock with no blockid is not something the corpus has,
                // and dropping it would change the document rather than shrink
                // it. Left alone, so the reader refuses the fixture and we find
                // out here rather than in CI.
                continue;
            };
            plan.keep.insert(blockid);
            plan.kept_refs += 1;
            let samples = stored
                .get(&blockid)
                .copied()
                .unwrap_or(0)
                .min(how.samples_per_block);
            if let Some(span) = &block.start {
                plan.patch.push(Patch {
                    span: span.clone(),
                    value: Value::Integer(at),
                });
            }
            if let Some(span) = &block.length {
                plan.patch.push(Patch {
                    span: span.clone(),
                    value: Value::Integer(samples),
                });
            }
            at += samples;
        }
        if let Some(span) = self.numsamples {
            plan.patch.push(Patch {
                span,
                value: Value::Integer(at),
            });
        }
        Ok(())
    }
}

/// Applies the plan: patches first, then deletions from the end.
///
/// That order is the whole reason this is a separate pass. A patch is
/// same-width, so it does not move anything; a deletion moves every span after
/// it, so deletions run last and back to front.
fn apply(doc: &mut Vec<u8>, plan: &Plan) -> Result<()> {
    let mut tails: Vec<Range<usize>> = Vec::new();
    for patch in &plan.patch {
        if let Some(tail) = patch_record(doc, patch)? {
            tails.push(tail);
        }
    }

    let mut removals: Vec<Range<usize>> = plan.remove.iter().cloned().chain(tails).collect();
    removals.sort_by_key(|range| range.start);
    for range in removals.iter().rev() {
        doc.drain(range.clone());
    }
    Ok(())
}

/// Rewrites one record's payload, returning a byte range to delete if the
/// record shrank.
fn patch_record(doc: &mut [u8], patch: &Patch) -> Result<Option<Range<usize>>> {
    let at = patch.span.start;
    let tag = doc[at];
    // Tag, then the two-byte name id. Every record this patches has one.
    let payload = at + 3;
    match (&patch.value, tag) {
        (Value::Integer(value), 0x04) => {
            let narrowed = i32::try_from(*value).map_err(|_| unpatchable(tag, at, *value))?;
            doc[payload..payload + 4].copy_from_slice(&narrowed.to_le_bytes());
        }
        (Value::Integer(value), 0x05) => {
            let narrowed = u8::try_from(*value).map_err(|_| unpatchable(tag, at, *value))?;
            doc[payload] = narrowed;
        }
        (Value::Integer(value), 0x06 | 0x08) => {
            let narrowed = u32::try_from(*value).map_err(|_| unpatchable(tag, at, *value))?;
            doc[payload..payload + 4].copy_from_slice(&narrowed.to_le_bytes());
        }
        (Value::Integer(value), 0x07) => {
            doc[payload..payload + 8].copy_from_slice(&value.to_le_bytes());
        }
        (Value::Double(value), 0x0A) => {
            // Eight bytes of double, leaving the four-byte precision hint.
            doc[payload..payload + 8].copy_from_slice(&value.to_le_bytes());
        }
        (Value::Blob(keep), 0x10) => {
            // The one length in the format that is already a byte count, so
            // there is nothing to divide by four here.
            let bytes_at = payload + 4;
            let was = u32::from_le_bytes([
                doc[payload],
                doc[payload + 1],
                doc[payload + 2],
                doc[payload + 3],
            ]) as usize;
            let kept = u32::try_from(*keep).map_err(|_| unpatchable(tag, at, *keep as u64))?;
            doc[payload..payload + 4].copy_from_slice(&kept.to_le_bytes());
            return Ok(Some(bytes_at + *keep..bytes_at + was));
        }
        (_, _) => {
            return Err(unpatchable(tag, at, 0));
        }
    }
    Ok(None)
}

fn unpatchable(tag: u8, offset: usize, value: u64) -> Error {
    Error::UnpatchableRecord { tag, offset, value }
}

/// Empties `project_history`, if the file has one.
///
/// The table's *existence* is the AUP4 signal a reader keys off, and it survives.
/// Its contents are a second full copy of a document that no longer matches the
/// live one, so keeping them would put a stale document in the fixture -
/// precisely the trap the reader is built to avoid.
fn clear_history(conn: &Connection) -> Result<usize> {
    let present: bool = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'project_history'",
        [],
        |row| row.get::<_, i64>(0).map(|n| n > 0),
    )?;
    if !present {
        return Ok(0);
    }
    Ok(conn.execute("DELETE FROM project_history", [])?)
}

/// Deletes the blocks the fixture no longer references and empties the rest.
fn trim_blocks(
    conn: &Connection,
    keep: &BTreeSet<i64>,
    stored: &BTreeMap<i64, u64>,
    how: &Shrink,
) -> Result<usize> {
    let mut removed = 0_usize;
    for (&blockid, &samples) in stored {
        if !keep.contains(&blockid) {
            removed += conn.execute("DELETE FROM sampleblocks WHERE blockid = ?1", [blockid])?;
            continue;
        }
        let kept = samples.min(how.samples_per_block);
        let code: i64 = conn.query_row(
            "SELECT sampleformat FROM sampleblocks WHERE blockid = ?1",
            [blockid],
            |row| row.get(0),
        )?;
        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "sampleformat is a 32-bit code stored in a SQLite INTEGER"
        )]
        let per_sample = SampleFormat::from_code(code as u32)?.bytes_per_sample() as u64;
        // Summaries are (min, max, rms) f32 triplets at strides of 256 and
        // 65536 samples, so the sizes follow from the sample count and are not
        // free to choose.
        let summary256 = kept.div_ceil(256) * 12;
        let summary64k = kept.div_ceil(65_536) * 12;
        conn.execute(
            "UPDATE sampleblocks
                SET samples = zeroblob(?2), summary256 = zeroblob(?3),
                    summary64k = zeroblob(?4), summin = 0.0, summax = 0.0, sumrms = 0.0
              WHERE blockid = ?1",
            rusqlite::params![
                blockid,
                cast_size(kept * per_sample),
                cast_size(summary256),
                cast_size(summary64k)
            ],
        )?;
    }
    Ok(removed)
}

/// A byte count as SQLite wants it.
#[expect(
    clippy::cast_possible_wrap,
    reason = "a fixture's blob sizes are a few hundred bytes"
)]
const fn cast_size(bytes: u64) -> i64 {
    bytes as i64
}

#[cfg(test)]
mod tests {
    use super::{Patch, Value, patch_record};

    /// A record: tag, name id 1, then the payload.
    fn record(tag: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![tag, 0x01, 0x00];
        bytes.extend(payload);
        bytes
    }

    #[test]
    fn a_number_is_rewritten_in_the_width_the_record_already_uses() {
        // Audacity is not consistent about which numeric record it uses for a
        // given attribute, so a patch that assumed one width would either
        // corrupt the next record or write into the middle of this one.
        let mut i32_record = record(0x04, &99_i32.to_le_bytes());
        let mut u32_record = record(0x06, &99_u32.to_le_bytes());
        let mut u64_record = record(0x07, &99_u64.to_le_bytes());

        for (bytes, width) in [
            (&mut i32_record, 4),
            (&mut u32_record, 4),
            (&mut u64_record, 8),
        ] {
            let before = bytes.len();
            let tail = patch_record(
                bytes,
                &Patch {
                    span: 0..before,
                    value: Value::Integer(256),
                },
            )
            .expect("256 fits every one of these widths");
            assert_eq!(tail, None, "an integer patch does not change the length");
            assert_eq!(bytes.len(), before);
            assert_eq!(bytes.len(), 3 + width);
            assert_eq!(bytes[3], 0x00, "256 is 00 01 little-endian");
            assert_eq!(bytes[4], 0x01);
        }
    }

    #[test]
    fn a_number_too_wide_for_its_record_is_refused_rather_than_truncated() {
        // A `numsamples` that no longer fits would silently become a different
        // number, and the fixture would then describe audio it does not have.
        let mut bytes = record(0x05, &[0]);
        let whole = 0..bytes.len();
        assert!(
            patch_record(
                &mut bytes,
                &Patch {
                    span: whole,
                    value: Value::Integer(256),
                },
            )
            .is_err(),
            "256 does not fit in a one-byte record"
        );
    }

    #[test]
    fn a_double_is_rewritten_and_the_precision_hint_is_left_alone() {
        // A 0x0A record is twelve bytes: the double, then how Audacity would
        // print it. The hint is not ours to change.
        let mut payload = 8.845_239_583_333_333_f64.to_le_bytes().to_vec();
        payload.extend((-1_i32).to_le_bytes());
        let mut bytes = record(0x0A, &payload);
        let whole = 0..bytes.len();
        let tail = patch_record(
            &mut bytes,
            &Patch {
                span: whole,
                value: Value::Double(0.0),
            },
        )
        .expect("a double fits a double");
        assert_eq!(tail, None);
        assert_eq!(bytes.len(), 3 + 12);
        assert_eq!(&bytes[3..11], &0.0_f64.to_le_bytes());
        assert_eq!(
            &bytes[11..15],
            &(-1_i32).to_le_bytes(),
            "the precision hint survived"
        );
    }

    #[test]
    fn truncating_a_blob_shortens_its_declared_length_and_names_the_tail() {
        // The length here is already a byte count, so there is nothing to
        // multiply by four, and the tail to delete starts where the kept bytes
        // end rather than at the end of the record.
        let mut payload = 10_u32.to_le_bytes().to_vec();
        payload.extend([0x89, b'P', b'N', b'G', 5, 6, 7, 8, 9, 10]);
        let mut bytes = record(0x10, &payload);
        assert_eq!(bytes.len(), 3 + 4 + 10);

        let whole = 0..bytes.len();
        let tail = patch_record(
            &mut bytes,
            &Patch {
                span: whole,
                value: Value::Blob(4),
            },
        )
        .expect("a blob can always be shortened")
        .expect("shortening a blob leaves a tail to delete");
        assert_eq!(tail, 11..17, "the four kept bytes end at 11");
        assert_eq!(
            u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]),
            4,
            "the declared length is the kept length"
        );
        bytes.drain(tail);
        assert_eq!(bytes, record(0x10, &[4, 0, 0, 0, 0x89, b'P', b'N', b'G']));
    }
}
