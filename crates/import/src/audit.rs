/*
 *  audit.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Cross-checking the document against the audio the file actually holds.
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
//! Cross-checking the document against the audio the file actually holds.
//!
//! The document and `sampleblocks` are two descriptions of one thing, and the
//! places they overlap are free integrity tests. Three of them:
//!
//! - **Every `waveblock` must resolve.** A reference to a row that is not there
//!   means the file is damaged, and it means it before any audio has been read,
//!   which is when the user still has the option of going back to Audacity.
//! - **`waveblock/@length` restates a size the audio table already knows.** It
//!   is AUP4 only and it matched in all 5,664 cases in the corpus, so a
//!   disagreement is evidence rather than noise. Its *absence* means AUP3.
//! - **Blocks are shared, so the two counts differ.** 532 references to 456
//!   distinct blocks in one corpus project, one block used three times. The gap
//!   between [`Audit::refs`] and [`Audit::distinct`] is the sharing, and
//!   anything that copies or frees blocks has to respect it.
//!
//! Orphans are counted and not treated as an error: Audacity leaves unreferenced
//! blocks behind after an edit, so they are a disk-space observation, not
//! damage.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::model::{Project, SampleFormat};

/// What the document and the audio table agree and disagree about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Audit {
    /// How many `waveblock` references the document makes.
    pub refs: usize,
    /// How many distinct blocks those references reach.
    pub distinct: usize,
    /// The most-shared block and how many references reach it, when any block
    /// is shared at all.
    pub most_shared: Option<(i64, usize)>,
    /// How many rows `sampleblocks` holds.
    pub rows: usize,
    /// Blocks present in the table that no clip references.
    ///
    /// Not an error. Audacity leaves these behind, and the corpus has them.
    pub orphans: Vec<i64>,
    /// How many `waveblock/@length` attributes were checked. Zero on an AUP3.
    pub lengths_checked: usize,
}

/// Checks a project's block references against `sampleblocks`.
///
/// # Errors
///
/// [`Error::DanglingBlock`] if a clip references a row that is not there,
/// [`Error::BlockLengthMismatch`] if a declared length disagrees with the stored
/// block, [`Error::UnknownSampleFormat`] if a row's format code is not one of
/// the three, and [`Error::Sqlite`].
pub fn audit(conn: &Connection, project: &Project) -> Result<Audit> {
    // `length(samples)` rather than `samples`: SQLite answers it from the blob's
    // header, so this walks a 774 MB project's whole block table without reading
    // a single sample.
    let mut statement =
        conn.prepare("SELECT blockid, sampleformat, length(samples) FROM sampleblocks")?;
    let mut stored: HashMap<i64, u64> = HashMap::new();
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (blockid, format_code, bytes) = row?;
        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "sampleformat is stored as a SQLite INTEGER but is a 32-bit code"
        )]
        let format = SampleFormat::from_code(format_code as u32)?;
        #[expect(
            clippy::cast_sign_loss,
            reason = "length() of a blob is never negative"
        )]
        let samples = bytes as u64 / format.bytes_per_sample() as u64;
        stored.insert(blockid, samples);
    }

    let mut uses: HashMap<i64, usize> = HashMap::new();
    let mut refs = 0_usize;
    let mut lengths_checked = 0_usize;
    for track in &project.tracks {
        for clip in &track.clips {
            for block in &clip.blocks {
                refs += 1;
                let actual = *stored
                    .get(&block.blockid)
                    .ok_or_else(|| Error::DanglingBlock {
                        clip: clip.name.clone(),
                        blockid: block.blockid,
                    })?;
                if let Some(declared) = block.length {
                    lengths_checked += 1;
                    if declared != actual {
                        return Err(Error::BlockLengthMismatch {
                            blockid: block.blockid,
                            declared,
                            actual,
                        });
                    }
                }
                *uses.entry(block.blockid).or_default() += 1;
            }
        }
    }

    let mut orphans: Vec<i64> = stored
        .keys()
        .copied()
        .filter(|blockid| !uses.contains_key(blockid))
        .collect();
    orphans.sort_unstable();

    Ok(Audit {
        refs,
        distinct: uses.len(),
        most_shared: uses
            .iter()
            .max_by_key(|&(blockid, count)| (*count, *blockid))
            .filter(|&(_, count)| *count > 1)
            .map(|(blockid, count)| (*blockid, *count)),
        rows: stored.len(),
        orphans,
        lengths_checked,
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::audit;
    use crate::error::Error;
    use crate::model::{BlockRef, Clip, Project, SampleFormat, Track};

    /// A `sampleblocks` table with one row per `(blockid, samples)` pair.
    ///
    /// The schema is Audacity's, and `samples` is a blob of the right *size*
    /// rather than real audio: the audit reads `length(samples)` and never the
    /// samples, which is what lets it walk a 774 MB project without touching the
    /// audio.
    fn table(rows: &[(i64, u64)], format: SampleFormat) -> Connection {
        let conn = Connection::open_in_memory().expect("an in-memory database");
        conn.execute_batch(
            "CREATE TABLE sampleblocks(
               blockid INTEGER PRIMARY KEY AUTOINCREMENT, sampleformat INTEGER,
               summin REAL, summax REAL, sumrms REAL,
               summary256 BLOB, summary64k BLOB, samples BLOB);",
        )
        .expect("the Audacity schema");
        for &(blockid, samples) in rows {
            let bytes = samples * format.bytes_per_sample() as u64;
            conn.execute(
                "INSERT INTO sampleblocks(blockid, sampleformat, samples) VALUES (?1, ?2, zeroblob(?3))",
                rusqlite::params![blockid, i64::from(format.code()), bytes as i64],
            )
            .expect("insert a block");
        }
        conn
    }

    /// A one-track, one-clip project referencing exactly these blocks.
    fn project(blocks: Vec<BlockRef>) -> Project {
        Project {
            tracks: vec![Track {
                name: "Audio 1".to_owned(),
                rate: 192_000.0,
                channel: Some(0),
                linked: None,
                sample_format: SampleFormat::Int24,
                gain: 1.0,
                pan: 0.0,
                muted: false,
                solo: false,
                clips: vec![Clip {
                    name: "Audio 1.1".to_owned(),
                    offset: 0.0,
                    trim_left: 0.0,
                    trim_right: 0.0,
                    num_samples: 262_144,
                    max_samples: 262_144,
                    sample_format: SampleFormat::Int24,
                    stretch_ratio: 1.0,
                    blocks,
                    envelope: Vec::new(),
                }],
            }],
            ..Project::default()
        }
    }

    #[test]
    fn a_shared_block_is_one_row_and_several_references() {
        // The corpus proportions in miniature: three references reaching two
        // blocks, one of them twice. A reader that assumed one block per
        // reference would report a block missing, and one that freed a block
        // when a clip stopped referencing it would corrupt the other clip.
        let conn = table(&[(73, 262_144), (75, 262_144)], SampleFormat::Int24);
        let audited = audit(
            &conn,
            &project(vec![
                BlockRef {
                    start: 0,
                    blockid: 73,
                    length: None,
                },
                BlockRef {
                    start: 262_144,
                    blockid: 75,
                    length: None,
                },
                BlockRef {
                    start: 524_288,
                    blockid: 73,
                    length: None,
                },
            ]),
        )
        .expect("a shared block is not an error");
        assert_eq!(audited.refs, 3);
        assert_eq!(audited.distinct, 2);
        assert_eq!(audited.rows, 2);
        assert_eq!(audited.most_shared, Some((73, 2)));
        assert!(audited.orphans.is_empty());
    }

    #[test]
    fn an_unshared_project_reports_no_most_shared_block() {
        let conn = table(&[(1, 262_144)], SampleFormat::Float32);
        let audited = audit(
            &conn,
            &project(vec![BlockRef {
                start: 0,
                blockid: 1,
                length: None,
            }]),
        )
        .expect("audit");
        assert_eq!(
            audited.most_shared, None,
            "one reference each is not sharing"
        );
        assert_eq!(audited.refs, audited.distinct);
    }

    #[test]
    fn a_reference_to_a_block_that_is_not_there_is_refused() {
        // Damage, and worth reporting before any audio is read: the user still
        // has the option of going back to Audacity.
        let conn = table(&[(73, 262_144)], SampleFormat::Int24);
        let error = audit(
            &conn,
            &project(vec![BlockRef {
                start: 0,
                blockid: 9_999,
                length: None,
            }]),
        )
        .expect_err("a dangling reference must be refused");
        assert!(
            matches!(
                error,
                Error::DanglingBlock {
                    blockid: 9_999,
                    ref clip
                } if clip == "Audio 1.1"
            ),
            "got {error}"
        );
    }

    #[test]
    fn a_declared_length_is_checked_against_the_stored_block() {
        // AUP4's waveblock/@length restates a size the audio table already
        // knows, so a disagreement means one of the two is wrong. It matched in
        // all 5,664 corpus cases, which is what makes a mismatch evidence.
        let conn = table(&[(73, 262_144)], SampleFormat::Int24);
        let agreeing = audit(
            &conn,
            &project(vec![BlockRef {
                start: 0,
                blockid: 73,
                length: Some(262_144),
            }]),
        )
        .expect("a matching length");
        assert_eq!(agreeing.lengths_checked, 1);

        let error = audit(
            &conn,
            &project(vec![BlockRef {
                start: 0,
                blockid: 73,
                length: Some(262_143),
            }]),
        )
        .expect_err("a mismatched length must be refused");
        assert!(
            matches!(
                error,
                Error::BlockLengthMismatch {
                    blockid: 73,
                    declared: 262_143,
                    actual: 262_144
                }
            ),
            "got {error}"
        );
    }

    #[test]
    fn the_sample_count_comes_from_the_rows_own_format_and_not_the_clips() {
        // int16 is two bytes a sample and the other two formats are four, so a
        // count taken with the wrong format is out by a factor of two. The row
        // carries its own `sampleformat`, and that is the one that describes its
        // blob.
        let conn = table(&[(1, 100)], SampleFormat::Int16);
        let audited = audit(
            &conn,
            &project(vec![BlockRef {
                start: 0,
                blockid: 1,
                length: Some(100),
            }]),
        )
        .expect("the row's own format gives 100 samples");
        assert_eq!(audited.lengths_checked, 1);
    }

    #[test]
    fn an_unreferenced_block_is_counted_and_not_treated_as_damage() {
        // Audacity leaves these behind after an edit. They are disk space, not
        // corruption, so they are reported rather than refused.
        let conn = table(
            &[(1, 262_144), (2, 262_144), (3, 262_144)],
            SampleFormat::Float32,
        );
        let audited = audit(
            &conn,
            &project(vec![BlockRef {
                start: 0,
                blockid: 2,
                length: None,
            }]),
        )
        .expect("orphans are not an error");
        assert_eq!(audited.orphans, vec![1, 3]);
        assert_eq!(audited.rows, 3);
        assert_eq!(audited.distinct, 1);
    }

    #[test]
    fn a_row_in_a_format_this_reader_does_not_know_is_refused() {
        let conn = Connection::open_in_memory().expect("database");
        conn.execute_batch(
            "CREATE TABLE sampleblocks(blockid INTEGER PRIMARY KEY, sampleformat INTEGER,
               summin REAL, summax REAL, sumrms REAL,
               summary256 BLOB, summary64k BLOB, samples BLOB);
             INSERT INTO sampleblocks(blockid, sampleformat, samples)
               VALUES (1, 262146, zeroblob(400));",
        )
        .expect("a block claiming a fourth format");
        let error = audit(
            &conn,
            &project(vec![BlockRef {
                start: 0,
                blockid: 1,
                length: None,
            }]),
        )
        .expect_err("a format we do not know must be refused");
        assert!(
            matches!(error, Error::UnknownSampleFormat { found: 262_146 }),
            "got {error}"
        );
    }
}
