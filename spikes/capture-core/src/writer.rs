/*
 *  writer.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The capture writer: drains the ring, assembles immutable blocks, and
 *  commits
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

//! The capture writer: drains the ring, assembles immutable blocks, and commits
//! them in batched transactions. This is the thread whose tail latency decides
//! whether the whole architecture holds (§14).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use rtrb::Consumer;
use rusqlite::params;

use crate::config::{Checkpoint, Layout, Params, Summaries};
use crate::db;
use crate::metrics::Latencies;

pub struct WriterOutcome {
    pub blocks: u64,
    pub rows: u64,
    pub bytes: u64,
    pub commit_latency: Latencies,
    pub checkpoint_latency: Latencies,
    pub summary_latency: Latencies,
    pub peak_wal_bytes: u64,
}

pub fn run(
    p: Params,
    db_path: String,
    mut ring: Consumer<u8>,
    stop: Arc<AtomicBool>,
    blocks_committed: Arc<AtomicU64>,
) -> Result<WriterOutcome> {
    let conn = db::open_writer(&db_path, &p)?;
    let wal_path = std::path::PathBuf::from(format!("{db_path}-wal"));

    let frame_bytes = p.frame_bytes();
    let block_frames = p.block_frames();
    let block_bytes = block_frames as usize * frame_bytes;
    let fmt = db::format_tag(p.bytes_per_sample);

    conn.execute(
        "INSERT INTO captures (capture_id, sample_rate, channels, bytes_per_sample, layout, started_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        params![
            p.rate,
            p.channels,
            p.bytes_per_sample as i64,
            format!("{:?}", p.layout),
            now_millis()
        ],
    )?;

    let mut pending = Vec::<u8>::with_capacity(block_bytes);
    let mut outcome = WriterOutcome {
        blocks: 0,
        rows: 0,
        bytes: 0,
        commit_latency: Latencies::default(),
        checkpoint_latency: Latencies::default(),
        summary_latency: Latencies::default(),
        peak_wal_bytes: 0,
    };

    let mut batch: Vec<Block> = Vec::with_capacity(p.batch_blocks);
    let mut sequence: u64 = 0;
    let mut start_frame: u64 = 0;

    loop {
        let finished = stop.load(Ordering::Relaxed);
        let available = ring.slots();

        if available > 0 {
            let want = available.min(block_bytes - pending.len());
            if let Ok(chunk) = ring.read_chunk(want) {
                let (a, b) = chunk.as_slices();
                pending.extend_from_slice(a);
                pending.extend_from_slice(b);
                chunk.commit_all();
            }
        }

        if pending.len() >= block_bytes || (finished && !pending.is_empty() && available == 0) {
            let take = pending.len().min(block_bytes);
            let raw: Vec<u8> = pending.drain(..take).collect();
            let frames = (raw.len() / frame_bytes) as u64;

            let t = Instant::now();
            let block = Block::build(&p, sequence, start_frame, frames, raw);
            outcome
                .summary_latency
                .record(t.elapsed().as_micros() as u64);

            start_frame += frames;
            sequence += 1;
            batch.push(block);
        }

        let flush = batch.len() >= p.batch_blocks
            || (finished && !batch.is_empty() && available == 0 && pending.is_empty());
        if flush {
            let t = Instant::now();
            commit_batch(&conn, &p, fmt, &batch)?;
            outcome
                .commit_latency
                .record(t.elapsed().as_micros() as u64);

            for b in &batch {
                outcome.blocks += 1;
                outcome.rows += b.rows.len() as u64;
                outcome.bytes += b.rows.iter().map(|r| r.samples.len() as u64).sum::<u64>();
            }
            blocks_committed.store(outcome.blocks, Ordering::Relaxed);
            batch.clear();

            let wal = crate::metrics::file_len(&wal_path);
            outcome.peak_wal_bytes = outcome.peak_wal_bytes.max(wal);

            if matches!(p.checkpoint, Checkpoint::Passive | Checkpoint::Truncate)
                && outcome.blocks.is_multiple_of(p.checkpoint_blocks)
            {
                let mode = if p.checkpoint == Checkpoint::Passive {
                    "PASSIVE"
                } else {
                    "TRUNCATE"
                };
                let t = Instant::now();
                let _ = conn.execute_batch(&format!("PRAGMA wal_checkpoint({mode});"));
                outcome
                    .checkpoint_latency
                    .record(t.elapsed().as_micros() as u64);
            }
        }

        if finished && pending.is_empty() && batch.is_empty() && ring.slots() == 0 {
            break;
        }
        if available == 0 && !finished {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    conn.execute(
        "UPDATE captures SET finished_at = ?1 WHERE capture_id = 1",
        params![now_millis()],
    )?;
    let t = Instant::now();
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    outcome
        .checkpoint_latency
        .record(t.elapsed().as_micros() as u64);

    Ok(outcome)
}

pub struct Row {
    pub channel: i64,
    pub samples: Vec<u8>,
    pub checksum: u32,
    pub summin: f64,
    pub summax: f64,
    pub sumrms: f64,
    pub summary256: Option<Vec<u8>>,
    pub summary64k: Option<Vec<u8>>,
}

pub struct Block {
    pub sequence: u64,
    pub start_frame: u64,
    pub frames: u64,
    pub rows: Vec<Row>,
}

impl Block {
    fn build(p: &Params, sequence: u64, start_frame: u64, frames: u64, raw: Vec<u8>) -> Block {
        let rows = match p.layout {
            Layout::Interleaved => vec![make_row(p, -1, raw)],
            Layout::PerChannel => {
                let ch = p.channels as usize;
                let bps = p.bytes_per_sample;
                let mut bufs: Vec<Vec<u8>> = (0..ch)
                    .map(|_| Vec::with_capacity(frames as usize * bps))
                    .collect();
                for f in 0..frames as usize {
                    let base = f * ch * bps;
                    for (c, buf) in bufs.iter_mut().enumerate() {
                        let o = base + c * bps;
                        buf.extend_from_slice(&raw[o..o + bps]);
                    }
                }
                bufs.into_iter()
                    .enumerate()
                    .map(|(c, b)| make_row(p, c as i64, b))
                    .collect()
            }
        };
        Block {
            sequence,
            start_frame,
            frames,
            rows,
        }
    }
}

fn make_row(p: &Params, channel: i64, samples: Vec<u8>) -> Row {
    let checksum = crc32fast::hash(&samples);
    let (summin, summax, sumrms, s256, s64k) = match p.summaries {
        Summaries::None => (0.0, 0.0, 0.0, None, None),
        Summaries::Aup4 => {
            let (mn, mx, rms) = block_stats(&samples, p.bytes_per_sample);
            (
                mn,
                mx,
                rms,
                Some(summary(&samples, p.bytes_per_sample, 256)),
                Some(summary(&samples, p.bytes_per_sample, 65_536)),
            )
        }
    };
    Row {
        channel,
        samples,
        checksum,
        summin,
        summax,
        sumrms,
        summary256: s256,
        summary64k: s64k,
    }
}

/// Decode one sample to a normalized f32, treating the low bytes as little-endian
/// two's complement at the given width.
pub fn sample_at(bytes: &[u8], idx: usize, bps: usize) -> f32 {
    let o = idx * bps;
    let mut v: i32 = 0;
    for i in 0..bps {
        v |= (bytes[o + i] as i32) << (8 * i);
    }
    let shift = 32 - (bps * 8) as i32;
    let signed = (v << shift) >> shift;
    let scale = (1i64 << ((bps * 8) - 1)) as f32;
    signed as f32 / scale
}

fn block_stats(bytes: &[u8], bps: usize) -> (f64, f64, f64) {
    let n = bytes.len() / bps;
    if n == 0 {
        return (0.0, 0.0, 0.0);
    }
    let mut mn = f32::MAX;
    let mut mx = f32::MIN;
    let mut acc = 0f64;
    for i in 0..n {
        let s = sample_at(bytes, i, bps);
        mn = mn.min(s);
        mx = mx.max(s);
        acc += (s as f64) * (s as f64);
    }
    (mn as f64, mx as f64, (acc / n as f64).sqrt())
}

/// AUP-style summary: (min, max, rms) f32 triplets per group of `group` samples.
fn summary(bytes: &[u8], bps: usize, group: usize) -> Vec<u8> {
    let n = bytes.len() / bps;
    let groups = n.div_ceil(group).max(1);
    let mut out = Vec::with_capacity(groups * 12);
    for g in 0..groups {
        let lo = g * group;
        let hi = ((g + 1) * group).min(n);
        let mut mn = f32::MAX;
        let mut mx = f32::MIN;
        let mut acc = 0f64;
        for i in lo..hi {
            let s = sample_at(bytes, i, bps);
            mn = mn.min(s);
            mx = mx.max(s);
            acc += (s as f64) * (s as f64);
        }
        if hi <= lo {
            mn = 0.0;
            mx = 0.0;
        }
        let rms = if hi > lo {
            (acc / (hi - lo) as f64).sqrt() as f32
        } else {
            0.0
        };
        out.extend_from_slice(&mn.to_le_bytes());
        out.extend_from_slice(&mx.to_le_bytes());
        out.extend_from_slice(&rms.to_le_bytes());
    }
    out
}

fn commit_batch(conn: &rusqlite::Connection, p: &Params, fmt: i64, batch: &[Block]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins_block = tx.prepare_cached(
            "INSERT INTO sampleblocks (sampleformat, summin, summax, sumrms, summary256, summary64k, samples)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        let mut ins_meta = tx.prepare_cached(
            "INSERT INTO capture_blocks
               (blockid, capture_id, channel, sequence, start_frame, frame_count, sample_rate, checksum, committed_at)
             VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        let ts = now_millis();
        for b in batch {
            for row in &b.rows {
                ins_block.execute(params![
                    fmt,
                    row.summin,
                    row.summax,
                    row.sumrms,
                    row.summary256.as_deref(),
                    row.summary64k.as_deref(),
                    row.samples.as_slice(),
                ])?;
                let blockid = tx.last_insert_rowid();
                ins_meta.execute(params![
                    blockid,
                    row.channel,
                    b.sequence as i64,
                    b.start_frame as i64,
                    b.frames as i64,
                    p.rate,
                    row.checksum as i64,
                    ts,
                ])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
