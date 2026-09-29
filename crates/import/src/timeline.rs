/*
 *  timeline.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Assembling Audacity's clips into one interleaved timeline.
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
//! Assembling Audacity's clips into one interleaved timeline (§12).
//!
//! The import path's one hard question. An Audacity project is not a recording:
//! it is a set of per-channel *sequences*, each cut into clips that have been
//! dragged, trimmed and left with gaps between them. A VCW capture is the
//! opposite - one contiguous interleaved stream per capture, with
//! `capture_blocks.sequence` documented as having no gaps. This module is the
//! bridge, and everything awkward about import lives here rather than being
//! spread through the writer.
//!
//! # Four things it has to get right
//!
//! **A clip's `offset` is where its sequence begins, not where it plays.** The
//! audible span is `[offset + trimLeft, offset + numsamples/rate - trimRight]`,
//! and [`crate::model`] records how the corpus proved it. So the timeline is
//! laid out from [`Clip::start_sample`], never from `offset`.
//!
//! **Gaps are silence, and the silence is load-bearing.** A label at 179.1 s
//! means 179.1 s from the start of the *timeline*. Closing a gap by
//! concatenating clips would shorten everything after it and slide every label
//! off the audio it names, silently. So frame 0 of the capture is second 0 of
//! the Audacity timeline and the gaps are written out.
//!
//! **No sample is converted.** D4's rule holds on the way in as well as on the
//! way out: the bytes go from Audacity's `sampleblocks` blob into VCW's with
//! nothing done to them, and the capture records
//! [`StorageFormat::Int24Padded`] - a format that exists solely so that an
//! imported 24-bit project keeps Audacity's four-byte layout instead of being
//! repacked into three. Silence is written as zero bytes, which is zero in all
//! three formats Audacity can store.
//!
//! **Blocks are shared, so they are read and not moved.** The clip-split corpus
//! project has 532 references to 456 distinct blocks, one of them used three
//! times, and a block may be referenced from two places at two different
//! sequence offsets. Reading through the timeline rather than adopting the rows
//! means sharing costs nothing and needs no reference counting.
//!
//! # Why the audio is re-blocked rather than adopted
//!
//! Adopting Audacity's rows verbatim is tempting - it is a `INSERT INTO ...
//! SELECT` and it would keep its summaries - but it cannot represent a trim. An
//! Audacity block is 262,144 samples and a clip's audible span starts and ends
//! wherever the user dragged it, which is almost never on a block boundary. An
//! adopted timeline would therefore have to carry a per-block sample offset that
//! `capture_blocks` has no column for and `validate` would have no way to check.
//! Re-blocking at VCW's 250 ms granularity costs one copy of the audio at import
//! time and buys a capture that is *structurally identical* to a recorded one:
//! playback, export, the waveform pyramids, boundaries and recovery all work on
//! it with no special case anywhere.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};
use vcw_types::{SampleRate, StorageFormat};

use crate::error::{Error, Result};
use crate::model::{Clip, Project, SampleFormat, Track};

/// A block as the timeline needs it: where it sits in its sequence and how many
/// samples it actually holds.
///
/// `samples` is measured from the stored blob rather than taken from
/// `waveblock/@length`, which only AUP4 writes at all. [`crate::audit`] checks
/// the two against each other where both exist, so by the time a timeline is
/// planned the measurement is the one that has been corroborated.
#[derive(Debug, Clone, Copy)]
struct Block {
    /// First sequence sample this block carries.
    start: u64,
    /// Its `sampleblocks` key.
    id: i64,
    /// Samples stored in it.
    samples: u64,
}

/// One audible stretch of one channel, in timeline frames.
#[derive(Debug, Clone)]
struct Run {
    /// Timeline frame its first sample lands on.
    at: u64,
    /// How many frames it covers.
    frames: u64,
    /// Sequence sample its first frame comes from. Non-zero exactly when the
    /// clip has a left trim.
    from: u64,
    /// The clip's blocks, ascending by sequence start.
    blocks: Vec<Block>,
}

impl Run {
    /// One past the last timeline frame it covers.
    const fn end(&self) -> u64 {
        self.at + self.frames
    }
}

/// One channel's whole timeline, read once, forwards.
///
/// Forward-only on purpose: the writer consumes the timeline in order, so a
/// cursor is enough and a seekable reader would be a second thing to get wrong.
/// The one-block cache is per channel rather than shared because the channels
/// are read alternately, one chunk at a time, and a shared cache would evict the
/// other channel's block on every chunk.
#[derive(Debug)]
struct Channel {
    runs: Vec<Run>,
    /// Run the cursor is in or before.
    run: usize,
    /// Next timeline frame to emit.
    at: u64,
    /// The most recently read block, kept whole.
    cached: Option<(i64, Vec<u8>)>,
}

impl Channel {
    /// One past the last frame this channel has audio for.
    fn end(&self) -> u64 {
        self.runs.last().map_or(0, Run::end)
    }

    /// Fills `out` with this channel's samples for the next `out.len() / width`
    /// frames, writing zeros wherever no clip covers them.
    ///
    /// # Errors
    ///
    /// If a block cannot be read, or a block is shorter than the sequence needs.
    fn fill(&mut self, conn: &Connection, out: &mut [u8], width: usize) -> Result<()> {
        out.fill(0);
        let mut done = 0_usize;
        let frames = out.len() / width;
        while done < frames {
            // Past the last clip: the rest of the chunk is silence, which
            // `out.fill(0)` has already written.
            let Some(run) = self.runs.get(self.run) else {
                break;
            };
            if self.at < run.at {
                // A gap. Skip it rather than write it: the zeros are there.
                let gap = ((run.at - self.at) as usize).min(frames - done);
                self.at += gap as u64;
                done += gap;
                continue;
            }
            let into_run = self.at - run.at;
            if into_run >= run.frames {
                self.run += 1;
                continue;
            }
            let sample = run.from + into_run;
            let block = find_block(&run.blocks, sample).ok_or(Error::SequenceHole {
                sample,
                blocks: run.blocks.len(),
            })?;
            let into_block = sample - block.start;
            let available = block.samples - into_block;
            let take = available
                .min(run.frames - into_run)
                .min((frames - done) as u64) as usize;
            let bytes = self.block_bytes(conn, block)?;
            let from = into_block as usize * width;
            let to = from + take * width;
            if to > bytes.len() {
                return Err(Error::BlockTooShort {
                    blockid: block.id,
                    wanted: to / width,
                    held: bytes.len() / width,
                });
            }
            out[done * width..(done + take) * width].copy_from_slice(&bytes[from..to]);
            self.at += take as u64;
            done += take;
        }
        Ok(())
    }

    /// The block's samples blob, from the cache or from the file.
    fn block_bytes(&mut self, conn: &Connection, block: Block) -> Result<&[u8]> {
        let hit = matches!(self.cached, Some((id, _)) if id == block.id);
        if !hit {
            let bytes: Option<Vec<u8>> = conn
                .query_row(
                    "SELECT samples FROM sampleblocks WHERE blockid = ?1",
                    [block.id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            // `plan` has already resolved every reference against
            // `sampleblocks`, so a miss here is a row whose `samples` column is
            // absent or NULL rather than a reference to a row that never was.
            let bytes = bytes.ok_or(Error::NoSamples { blockid: block.id })?;
            self.cached = Some((block.id, bytes));
        }
        // Set immediately above when it was absent.
        Ok(&self.cached.as_ref().expect("just cached").1)
    }
}

/// The block holding `sample`, by binary search on the sequence starts.
fn find_block(blocks: &[Block], sample: u64) -> Option<Block> {
    let at = match blocks.binary_search_by_key(&sample, |b| b.start) {
        Ok(exact) => exact,
        // `Err(0)` means the sample sits before the first block's start, which
        // cannot happen for a sequence that begins at zero and is a hole if it
        // does. Reported rather than clamped.
        Err(0) => return None,
        Err(after) => after - 1,
    };
    let block = blocks[at];
    (sample < block.start + block.samples).then_some(block)
}

/// Audacity's clips, laid out as one interleaved stream a capture writer can
/// consume.
#[derive(Debug)]
pub struct Timeline<'a> {
    conn: &'a Connection,
    channels: Vec<Channel>,
    rate: SampleRate,
    format: StorageFormat,
    frames: u64,
    at: u64,
    scratch: Vec<u8>,
}

impl<'a> Timeline<'a> {
    /// Plans the timeline: resolves every block's real length, orders the
    /// channels, and works out where each clip lands.
    ///
    /// Nothing is read from `sampleblocks.samples` here - only the lengths - so
    /// planning a 8 GB project is cheap and the refusals all happen before the
    /// first byte of audio moves.
    ///
    /// # Errors
    ///
    /// [`Error::NoAudio`] if the project has no wave tracks,
    /// [`Error::MixedTracks`] if the channels disagree about rate or format,
    /// [`Error::FractionalRate`] if the rate is not a whole number of Hz,
    /// [`Error::TooManyChannels`] past what a capture row can hold, or
    /// [`Error::DanglingBlock`] for a reference `sampleblocks` cannot satisfy.
    pub fn plan(conn: &'a Connection, project: &Project) -> Result<Self> {
        let tracks = order_channels(&project.tracks);
        let first = *tracks.first().ok_or(Error::NoAudio)?;
        let rate = first.rate;
        let format = first.sample_format;
        for track in &tracks {
            if track.rate != rate {
                return Err(Error::MixedTracks {
                    what: "rate",
                    first: format!("{rate}"),
                    found: format!("{}", track.rate),
                });
            }
            if track.sample_format != format {
                return Err(Error::MixedTracks {
                    what: "sample format",
                    first: format!("0x{:08X}", format.code()),
                    found: format!("0x{:08X}", track.sample_format.code()),
                });
            }
        }
        // Checked here so that the cast in `channels()` cannot be wrong later.
        u16::try_from(tracks.len()).map_err(|_| Error::TooManyChannels {
            found: tracks.len(),
        })?;
        let rate = whole_hz(rate)?;
        let lengths = block_lengths(conn, format)?;

        let mut planned = Vec::with_capacity(tracks.len());
        for track in &tracks {
            planned.push(Channel {
                runs: runs_of(track, rate, &lengths)?,
                run: 0,
                at: 0,
                cached: None,
            });
        }
        let frames = planned.iter().map(Channel::end).max().unwrap_or(0);
        let format = match format {
            SampleFormat::Int16 => StorageFormat::Int16,
            // Audacity's 24-bit is padded to four bytes and stays that way: D4
            // forbids the conversion, and `Int24Padded` exists for exactly this.
            SampleFormat::Int24 => StorageFormat::Int24Padded,
            SampleFormat::Float32 => StorageFormat::Float32,
        };
        Ok(Self {
            conn,
            channels: planned,
            rate,
            format,
            frames,
            at: 0,
            scratch: Vec::new(),
        })
    }

    /// Frames in the whole timeline, which is the longest channel's extent.
    ///
    /// The channels are allowed to differ: a trim applied to one and not the
    /// other leaves them a sample apart, and padding the short one with silence
    /// is the only answer that keeps the frames aligned.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// The authoritative rate, from `wavetrack/@rate`.
    #[must_use]
    pub const fn rate(&self) -> SampleRate {
        self.rate
    }

    /// Channels, which is the number of wave tracks.
    #[must_use]
    pub fn channels(&self) -> u16 {
        // Bounded by `plan`, which refuses a project with more tracks than this
        // can hold.
        self.channels.len() as u16
    }

    /// How the samples are laid out, in VCW's coding.
    #[must_use]
    pub const fn storage_format(&self) -> StorageFormat {
        self.format
    }

    /// Whether every frame has been handed out.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.at >= self.frames
    }

    /// The next `frames` interleaved frames, or `None` at the end.
    ///
    /// Short only at the end of the timeline. The returned buffer is freshly
    /// allocated because it is handed straight to
    /// [`vcw_project::persistence::Writer::push`], which takes a slice and
    /// copies it.
    ///
    /// # Errors
    ///
    /// If a block cannot be read, or is shorter than its sequence position needs.
    pub fn next_chunk(&mut self, frames: u64) -> Result<Option<Vec<u8>>> {
        if self.is_done() || frames == 0 {
            return Ok(None);
        }
        let width = self.format.bytes_per_sample();
        let take = frames.min(self.frames - self.at) as usize;
        self.scratch.resize(take * width, 0);
        let channels = self.channels.len();
        let mut out = vec![0_u8; take * width * channels];
        for (index, channel) in self.channels.iter_mut().enumerate() {
            channel.fill(self.conn, &mut self.scratch, width)?;
            for frame in 0..take {
                let from = frame * width;
                let to = (frame * channels + index) * width;
                out[to..to + width].copy_from_slice(&self.scratch[from..from + width]);
            }
        }
        self.at += take as u64;
        Ok(Some(out))
    }
}

/// Orders the wave tracks into channels.
///
/// By the `channel` attribute when every track has one and they are a
/// permutation of `0..n`, which is what all 30 corpus projects look like -
/// `channel="0" linked="3"` then `channel="1" linked="0"` for a stereo pair.
/// Otherwise document order, because a file we have never seen is better read in
/// the order it was written than rejected over an attribute we cannot interpret.
fn order_channels(tracks: &[Track]) -> Vec<&Track> {
    let mut ordered: Vec<&Track> = tracks.iter().collect();
    let mut declared: Vec<u32> = Vec::with_capacity(tracks.len());
    for track in tracks {
        match track.channel {
            Some(channel) => declared.push(channel),
            None => return ordered,
        }
    }
    let mut sorted = declared.clone();
    sorted.sort_unstable();
    if sorted.iter().copied().eq(0..tracks.len() as u32) {
        ordered.sort_by_key(|track| track.channel.unwrap_or(0));
    }
    ordered
}

/// Every stored block's sample count, measured from its blob.
///
/// One query for the whole table. The alternative - a `length(samples)` per
/// reference - is 22,332 queries on the largest corpus project, and the table
/// is the same size either way.
fn block_lengths(conn: &Connection, format: SampleFormat) -> Result<HashMap<i64, u64>> {
    let width = format.bytes_per_sample() as u64;
    let mut stmt = conn.prepare("SELECT blockid, length(samples) FROM sampleblocks")?;
    let mut rows = stmt.query([])?;
    let mut lengths = HashMap::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let bytes: Option<i64> = row.get(1)?;
        let bytes = bytes.unwrap_or(0).max(0) as u64;
        lengths.insert(id, bytes / width.max(1));
    }
    Ok(lengths)
}

/// Where each of a track's clips lands on the timeline, with its blocks resolved.
fn runs_of(track: &Track, rate: SampleRate, lengths: &HashMap<i64, u64>) -> Result<Vec<Run>> {
    let hz = f64::from(rate.hz());
    let mut runs = Vec::with_capacity(track.clips.len());
    for clip in &track.clips {
        let frames = clip.audible_samples(hz);
        if frames == 0 {
            // A clip trimmed to nothing. Audacity keeps it; the timeline has
            // nowhere to put it, and dropping it changes no frame's position.
            continue;
        }
        runs.push(Run {
            at: clip.start_sample(hz),
            frames,
            from: clip.first_audible_sample(hz),
            blocks: blocks_of(clip, lengths)?,
        });
    }
    // Sorted and checked here rather than trusted. `model::order_clips` does
    // both when it parses a document, but a `Project` is an ordinary struct that
    // a caller can build - the landing tests do - and a `Timeline` that assumed
    // an invariant its input type does not carry would read the wrong clip
    // rather than say so.
    runs.sort_unstable_by_key(|run| run.at);
    for pair in runs.windows(2) {
        if pair[0].end() > pair[1].at {
            let hz = f64::from(rate.hz());
            return Err(Error::OverlappingClips {
                track: track.name.clone(),
                start: pair[1].at as f64 / hz,
                previous_end: pair[0].end() as f64 / hz,
            });
        }
    }
    Ok(runs)
}

/// A clip's blocks, ascending, with the stored length of each.
fn blocks_of(clip: &Clip, lengths: &HashMap<i64, u64>) -> Result<Vec<Block>> {
    let mut blocks = Vec::with_capacity(clip.blocks.len());
    for reference in &clip.blocks {
        let samples = *lengths
            .get(&reference.blockid)
            .ok_or_else(|| Error::DanglingBlock {
                clip: clip.name.clone(),
                blockid: reference.blockid,
            })?;
        blocks.push(Block {
            start: reference.start,
            id: reference.blockid,
            samples,
        });
    }
    blocks.sort_unstable_by_key(|block| block.start);
    Ok(blocks)
}

/// The rate as whole Hz, refusing a fractional one rather than rounding it.
///
/// A capture row holds an integer rate, and every rate in the corpus is one.
/// Rounding 44,100.5 to 44,100 would be a 0.001 % pitch error nobody would ever
/// find, so this stops instead.
fn whole_hz(rate: f64) -> Result<SampleRate> {
    if !rate.is_finite() || rate <= 0.0 || rate.fract() != 0.0 || rate > f64::from(u32::MAX) {
        return Err(Error::FractionalRate { found: rate });
    }
    Ok(SampleRate(rate as u32))
}
