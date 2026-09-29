/*
 *  waveform.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Reading the waveform pyramid back out of a project (§19, §37).
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

//! Reading the waveform pyramid back out of a project (§19, §37).
//!
//! The thin half of WP-09. [`vcw_signal::waveform`] decides *which* resolution
//! answers a request and folds what comes back into columns; this decides how to
//! fetch it, which is one indexed range scan per request and no more.
//!
//! # Why the split
//!
//! `vcw-signal` touches no database, by its own module doc and by the layering
//! in [ADR-0003](../../../docs/adr/0003-workspace-layout.md). Keeping the
//! arithmetic there and the SQL here means every rule about levels, weighting
//! and boundaries is tested against hand-written triplets with no SQLite
//! present, and what is tested here is only that the right rows come back.
//!
//! # One scan, whatever the zoom
//!
//! Every level is served by the same query shape against
//! `capture_blocks_timeline`, which is `(capture_id, channel, start_frame)` -
//! exactly the range this asks for. What changes between levels is which
//! columns come with it: three scalars at [`Level::Block`], a blob at the two
//! summary levels, the audio itself at [`Level::Samples`].
//!
//! The rows touched are proportional to the *span being drawn*, never to the
//! length of the recording. Zooming out does not read more rows, it reads the
//! same rows more coarsely - and past a quarter-second per pixel it stops
//! opening blobs at all. That is what §37's "independent of total sample count"
//! means in practice, and `tests/waveform_reads.rs` measures it
//! rather than assuming it.
//!
//! # Progressive by construction (§19)
//!
//! Nothing here knows or cares whether the capture is still running. The writer
//! commits a block every 250 ms and this reads committed blocks, so a waveform
//! drawn during a recording is the recording so far, and the columns past its
//! end come back empty rather than missing. There is no second code path for
//! the live case, and therefore no way for the live case to disagree with the
//! finished one.

use rusqlite::{Connection, params};
use vcw_signal::waveform::{Level, Levels, Painter, Request, Waveform};
use vcw_types::{StorageFormat, Summary};

use crate::error::{Error, Result};
use crate::schema::BLOCK_MILLIS;
use crate::session;
use crate::sqlite::{Access, Project};

/// What a capture's pyramid looks like: the strides, and how to read a sample.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    /// The strides available.
    pub levels: Levels,
    /// How samples are laid out, for the [`Level::Samples`] path.
    pub format: StorageFormat,
    /// Frames committed, per channel.
    pub frames: u64,
    /// Channels in the capture.
    pub channels: u16,
}

impl Shape {
    /// Reads a capture's shape.
    ///
    /// The nominal block length comes from the rate and [`BLOCK_MILLIS`] rather
    /// than from a block, because the last block of a capture is usually short
    /// and picking the strides off it would choose a finer level than the data
    /// supports.
    ///
    /// # Errors
    ///
    /// If the capture is not in this project, or cannot be read.
    pub fn of(conn: &Connection, capture_id: i64) -> Result<Self> {
        let record = session::load(conn, capture_id)?.ok_or(Error::NoSuchCapture { capture_id })?;
        let block = (u64::from(record.info.rate.hz()) * u64::from(BLOCK_MILLIS) / 1000)
            .clamp(1, u64::from(u32::MAX)) as u32;
        Ok(Self {
            levels: Levels::new(block),
            format: record.info.storage_format,
            frames: record.frames,
            channels: record.info.channels,
        })
    }

    /// A request covering the whole capture at this width.
    #[must_use]
    pub const fn whole(&self, pixels: u32) -> Request {
        Request::new(0, self.frames, pixels)
    }
}

/// Draws one channel of one capture.
///
/// The level is chosen from the request, so a caller asks for a span and a width
/// and never for a resolution. [`read_at`] exists for the cases that want to
/// pin one - a benchmark, or a test proving two levels agree.
///
/// # Errors
///
/// If the capture is not in this project, or a row cannot be read.
pub fn read(
    conn: &Connection,
    capture_id: i64,
    channel: u16,
    request: &Request,
) -> Result<Waveform> {
    let shape = Shape::of(conn, capture_id)?;
    let level = request.level(shape.levels);
    read_at(conn, capture_id, channel, request, &shape, level)
}

/// The one query shape, named for the level it is reading.
///
/// One statement for every rung, so the row shape does not change between
/// levels and neither does the loop that consumes it: the payload column is
/// whichever blob this level needs, or `NULL` where the whole-block triplet is
/// already the answer.
///
/// The `INDEXED BY` clauses are the whole reason a zoomed-out drawing is fast.
/// A `sampleblocks` row sits behind a 192 KB samples blob at 24/192, so it
/// occupies 64 KiB pages of its own, and reading three floats out of 12,528 of
/// them costs 3.77 s cold. The two covering indexes hold the coarse rungs beside
/// the key, away from the audio, and the same read is 16 ms. Left to itself
/// SQLite prefers the integer primary key, so the index has to be named.
///
/// `INDEXED BY` is an assertion, not a hint: if an index ever goes missing the
/// query fails loudly here rather than quietly reverting to the slow plan in
/// front of a user. At the sample level the blob *is* the point, so the row has
/// to be read and there is nothing to name; `summary64k` has no index, for the
/// reason given in the schema.
fn sql_for(level: Level) -> String {
    let payload = match level {
        Level::Samples => "sb.samples",
        Level::Summary256 => "sb.summary256",
        Level::Summary64k => "sb.summary64k",
        Level::Block => "NULL",
    };
    let source = match level {
        Level::Block => "sampleblocks sb INDEXED BY sampleblocks_levels",
        Level::Summary256 => "sampleblocks sb INDEXED BY sampleblocks_summary256",
        Level::Summary64k | Level::Samples => "sampleblocks sb",
    };
    format!(
        "SELECT cb.start_frame, cb.frame_count, sb.summin, sb.summax, sb.sumrms, {payload} \
         FROM capture_blocks cb JOIN {source} ON sb.blockid = cb.blockid \
         WHERE cb.capture_id = ?1 AND cb.channel = ?2 \
           AND cb.start_frame < ?3 AND cb.start_frame + cb.frame_count > ?4 \
         ORDER BY cb.start_frame"
    )
}

/// Draws one channel at a resolution the caller has chosen.
///
/// # Errors
///
/// If a row cannot be read.
pub fn read_at(
    conn: &Connection,
    capture_id: i64,
    channel: u16,
    request: &Request,
    shape: &Shape,
    level: Level,
) -> Result<Waveform> {
    let mut painter = Painter::new(request, level);
    if request.pixels == 0 || request.frames() == 0 {
        return Ok(painter.finish());
    }

    let sql = sql_for(level);
    let mut stmt = conn.prepare_cached(&sql).map_err(Error::from)?;
    let mut rows = stmt
        .query(params![
            capture_id,
            channel,
            request.end as i64,
            request.start as i64,
        ])
        .map_err(Error::from)?;

    let stride = u64::from(shape.levels.stride(level));
    while let Some(row) = rows.next().map_err(Error::from)? {
        let start: i64 = row.get(0)?;
        let frames: i64 = row.get(1)?;
        let (start, frames) = (start.max(0) as u64, frames.max(0) as u64);

        match level {
            Level::Block => {
                // The cheap path, and the one a zoomed-out view lands on: the
                // whole-block triplet is three columns of the row, so nothing
                // is decompressed, parsed or allocated per block.
                painter.add(
                    start,
                    frames,
                    Summary {
                        min: row.get::<_, f64>(2)? as f32,
                        max: row.get::<_, f64>(3)? as f32,
                        rms: row.get::<_, f64>(4)? as f32,
                    },
                );
            }
            Level::Samples => {
                let samples: Option<Vec<u8>> = row.get(5)?;
                // A block with no audio is a corrupt block, and `validate` is
                // where that is reported. Drawing it as a gap is the honest
                // thing for a renderer to do about it.
                if let Some(samples) = samples {
                    painter.add_samples(start, shape.format, &samples);
                }
            }
            Level::Summary256 | Level::Summary64k => {
                let blob: Option<Vec<u8>> = row.get(5)?;
                match blob {
                    Some(blob) => add_triplets(&mut painter, start, frames, stride, &blob),
                    // Summaries can legitimately be absent: `Config::summaries`
                    // turns them off, and an import may not carry them. Fall
                    // back to the whole-block triplet, which is always there,
                    // rather than leaving a hole in the drawing.
                    None => painter.add(
                        start,
                        frames,
                        Summary {
                            min: row.get::<_, f64>(2)? as f32,
                            max: row.get::<_, f64>(3)? as f32,
                            rms: row.get::<_, f64>(4)? as f32,
                        },
                    ),
                }
            }
        }
    }
    Ok(painter.finish())
}

/// Feeds one stored pyramid level into the painter, triplet by triplet.
///
/// The last triplet of a block covers what is left rather than a full stride,
/// which matters: weighting it as a full one is the arithmetic Audacity gets
/// wrong, recorded in [`crate::persistence::pyramid`].
fn add_triplets(painter: &mut Painter, start: u64, frames: u64, stride: u64, blob: &[u8]) {
    if stride == 0 {
        return;
    }
    for (index, summary) in Summary::triplets(blob).enumerate() {
        let at = index as u64 * stride;
        if at >= frames {
            break;
        }
        painter.add(start + at, stride.min(frames - at), summary);
    }
}

/// Draws every channel of a capture over the same span.
///
/// §20 asks for a stereo waveform, which is two of these drawn against one time
/// axis, so they have to be the same span at the same width or the two halves
/// would not line up.
///
/// # Errors
///
/// If the capture is not in this project, or a row cannot be read.
pub fn read_all(conn: &Connection, capture_id: i64, request: &Request) -> Result<Vec<Waveform>> {
    let shape = Shape::of(conn, capture_id)?;
    let level = request.level(shape.levels);
    (0..shape.channels)
        .map(|channel| read_at(conn, capture_id, channel, request, &shape, level))
        .collect()
}

/// Which blocks a rebuild should touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rebuild {
    /// Only blocks with no stored pyramid. The ordinary case: a capture
    /// recorded with [`Config::summaries`](crate::persistence::Config::summaries)
    /// off, or one whose summaries a fault left behind.
    Missing,
    /// Every block of the capture, replacing what is there.
    ///
    /// For the case where the summaries are present but wrong, which nothing
    /// currently produces and a future import might.
    All,
}

/// What a rebuild did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rebuilt {
    /// Blocks considered.
    pub examined: usize,
    /// Blocks whose summaries were written.
    pub rewritten: usize,
    /// Blocks whose stored audio was missing, so nothing could be computed.
    pub silent: usize,
}

/// Recomputes a capture's waveform summaries from the audio it stored (§19).
///
/// §19 allows summaries to be persisted *and* regenerated from source PCM when
/// required, which is the escape hatch this is. It is also the proof that the
/// pyramid is derived data and not a second copy of the truth: throw it away and
/// it comes back identical, which `waveform_reads.rs` checks by doing exactly
/// that.
///
/// # What it will not touch
///
/// The `samples` blob, the format tag, and any block no capture claims - one
/// with no `capture_blocks` row. That last one is D4 as a guard rather than a
/// comment. Nothing writes such a block today: import re-blocks an Audacity
/// timeline through the capture writer (see `capture_blocks` in
/// [`crate::schema`]) and so produces rows like any other capture. The guard is
/// for a block that arrives some other way, where rewriting summaries we did not
/// compute would silently replace someone else's arithmetic with ours.
///
/// # Errors
///
/// [`Error::ReadOnly`] if the project cannot be written, or SQLite's own error.
pub fn rebuild(project: &mut Project, capture_id: i64, plan: Rebuild) -> Result<Rebuilt> {
    if project.access() != Access::ReadWrite {
        return Err(Error::ReadOnly {
            path: project.path().to_path_buf(),
        });
    }
    let shape = Shape::of(project.conn(), capture_id)?;
    let tx = project.conn_mut().transaction().map_err(Error::from)?;

    let filter = match plan {
        Rebuild::Missing => " AND (sb.summary256 IS NULL OR sb.summary64k IS NULL)",
        Rebuild::All => "",
    };
    let todo: Vec<(i64, Option<Vec<u8>>)> = {
        let mut stmt = tx
            .prepare(&format!(
                "SELECT cb.blockid, sb.samples \
                 FROM capture_blocks cb JOIN sampleblocks sb ON sb.blockid = cb.blockid \
                 WHERE cb.capture_id = ?1{filter} ORDER BY cb.blockid"
            ))
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(params![capture_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(Error::from)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(Error::from)?
    };

    let mut done = Rebuilt {
        examined: todo.len(),
        ..Rebuilt::default()
    };
    for (blockid, samples) in todo {
        let Some(samples) = samples else {
            done.silent += 1;
            continue;
        };
        let whole = Summary::of(shape.format, &samples);
        let s256 = crate::persistence::pyramid(shape.format, &samples, shape.levels.summary256);
        let s64k = crate::persistence::pyramid(shape.format, &samples, shape.levels.summary64k);
        tx.execute(
            "UPDATE sampleblocks SET summin = ?2, summax = ?3, sumrms = ?4, \
             summary256 = ?5, summary64k = ?6 WHERE blockid = ?1",
            params![blockid, whole.min, whole.max, whole.rms, s256, s64k],
        )
        .map_err(Error::from)?;
        done.rewritten += 1;
    }
    tx.commit().map_err(Error::from)?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plan SQLite chooses for a statement, one line per step.
    fn plan(conn: &Connection, sql: &str) -> String {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let rows = stmt
            .query_map([0i64, 0, 0, 0], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        rows.join("\n")
    }

    /// A structural test, not a timing one, because the difference it guards is
    /// two hundred fold and a timing test would still be flaky.
    ///
    /// "COVERING INDEX" is the word that matters. `SEARCH sb USING INDEX` means
    /// SQLite found the row through the index and then read the row anyway,
    /// which is the slow plan wearing the fast plan's name - and is exactly what
    /// happened when `sampleblocks_summary256` was first written without the
    /// three whole-block columns in it.
    #[test]
    fn the_coarse_levels_never_touch_a_row_that_holds_audio() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::create(dir.path().join("plan.vcw")).unwrap();
        let conn = project.conn();

        for (level, index) in [
            (Level::Block, "sampleblocks_levels"),
            (Level::Summary256, "sampleblocks_summary256"),
        ] {
            let chosen = plan(conn, &sql_for(level));
            assert!(
                chosen.contains(&format!("COVERING INDEX {index}")),
                "{level:?} should read {index} and nothing else, but the plan is:\n{chosen}"
            );
            assert!(
                !chosen.contains("sb USING INTEGER PRIMARY KEY"),
                "{level:?} fell back to the row:\n{chosen}"
            );
        }
    }

    /// The other two levels want the audio itself, so naming an index would only
    /// add a probe in front of the row fetch they have to do anyway.
    #[test]
    fn the_fine_levels_read_the_row_on_purpose() {
        for level in [Level::Samples, Level::Summary64k] {
            assert!(
                !sql_for(level).contains("INDEXED BY"),
                "{level:?} should not name an index"
            );
        }
    }
}
