/*
 *  waveform.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Multi-resolution waveform pyramid, built progressively during capture
 *  (§19).
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

//! Multi-resolution waveform pyramid, read at whatever zoom is being drawn (§19).
//!
//! §37 requires waveform rendering to be independent of total recording length.
//! This module is how: a request names a span and a pixel width, and what comes
//! back is exactly that many columns, whether the span is three seconds or three
//! hours. Nothing here ever holds the audio - the reduction is done against
//! summaries the writer already stored, which is the whole point of storing them.
//!
//! S3 added the other half of the answer, and it is worth repeating because it
//! decides where effort goes: the cost that actually bites is main-thread drawing
//! in the webview, not producing these summaries or shipping them across the IPC
//! boundary. A fixed column count is therefore not a nicety, it is the constraint
//! the renderer downstream is built around.
//!
//! # Pure, and therefore testable without a project
//!
//! `vcw-signal` touches no database, so this module does not do the reading. It
//! knows how to *choose* a resolution and how to *fold* what comes back; the SQL
//! lives in `vcw_project::waveform`, which is a thin shim over
//! [`Painter`]. That split is what lets every rule below be tested against
//! hand-written triplets with no SQLite anywhere near it.
//!
//! # The ladder
//!
//! Four resolutions exist in a `.vcw` file, and three of them are read:
//!
//! | level | stride | where it lives | cost |
//! |---|---|---|---|
//! | [`Level::Samples`] | 1 | the `samples` blob | the audio itself |
//! | [`Level::Summary256`] | 256 | the `summary256` blob | a blob per block |
//! | [`Level::Block`] | one block | `summin`/`summax`/`sumrms` | three scalars, no blob |
//! | [`Level::Summary64k`] | 65536 | the `summary64k` blob | a blob per block |
//!
//! **`Summary64k` is never chosen for audio VCW recorded**, and that is a finding
//! rather than an oversight. Both it and the block level come from the same row,
//! so they cost the same number of rows to fetch - but the block level needs no
//! blob parsed and, at D3's 250 ms blocks, is *finer*: 12,000 frames at 48 kHz
//! and 48,000 at 192 kHz, against 65,536. It is written because §49 makes the
//! schema an AUP4 superset and Audacity expects the column, and it is read only
//! when blocks are long enough for it to win - which means blocks imported from
//! Audacity, where a block can be far longer than a quarter of a second.

use vcw_types::{StorageFormat, Summary};

/// Which stored resolution a request should be answered from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// The audio itself. Only at zoom levels where a pixel is under 256 frames.
    Samples,
    /// One triplet per 256 samples.
    Summary256,
    /// One triplet per block, from the row rather than from a blob.
    Block,
    /// One triplet per 65536 samples. See the module doc: imported blocks only.
    Summary64k,
}

impl Level {
    /// The name used in diagnostics and in the CLI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Samples => "samples",
            Self::Summary256 => "summary256",
            Self::Block => "block",
            Self::Summary64k => "summary64k",
        }
    }
}

/// The strides available in one capture.
///
/// Three are fixed by the format; [`Levels::block`] is not, because a block is a
/// duration and a duration is a different number of frames at every rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Levels {
    /// Frames per triplet in `summary256`. 256 for anything VCW wrote.
    pub summary256: u32,
    /// Frames in one block. Rate-dependent: D3's 250 ms.
    pub block: u32,
    /// Frames per triplet in `summary64k`. 65536 for anything VCW wrote.
    pub summary64k: u32,
}

impl Levels {
    /// The strides of a capture written by VCW at this block length.
    #[must_use]
    pub const fn new(block_frames: u32) -> Self {
        Self {
            summary256: 256,
            block: block_frames,
            summary64k: 65_536,
        }
    }

    /// The coarsest level that still puts at least one triplet in every pixel,
    /// breaking ties towards the cheapest read.
    ///
    /// Coarsest, because a pixel drawn from one triplet is drawn correctly and
    /// reading more of them only costs time. Never *coarser* than a pixel,
    /// because a triplet spread over several pixels draws the same min and max
    /// in all of them, which is a plateau where the audio had detail.
    ///
    /// The order below is the tie-break: [`Level::Block`] is tried first because
    /// it comes from the same rows as [`Level::Summary64k`] and needs no blob,
    /// so whenever both would serve it is strictly better.
    #[must_use]
    pub fn choose(self, frames_per_pixel: f64) -> Level {
        let fits = |stride: u32| stride > 0 && f64::from(stride) <= frames_per_pixel;
        if fits(self.block) {
            Level::Block
        } else if fits(self.summary64k) {
            Level::Summary64k
        } else if fits(self.summary256) {
            Level::Summary256
        } else {
            Level::Samples
        }
    }

    /// The stride of one level, in frames.
    #[must_use]
    pub const fn stride(self, level: Level) -> u32 {
        match level {
            Level::Samples => 1,
            Level::Summary256 => self.summary256,
            Level::Block => self.block,
            Level::Summary64k => self.summary64k,
        }
    }
}

/// What to draw: a span of one channel, at a pixel width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// First frame of the span, inclusive.
    pub start: u64,
    /// Last frame of the span, exclusive.
    pub end: u64,
    /// Columns wanted. The answer has exactly this many.
    pub pixels: u32,
}

impl Request {
    /// A request for a whole span at a width.
    #[must_use]
    pub const fn new(start: u64, end: u64, pixels: u32) -> Self {
        Self { start, end, pixels }
    }

    /// Frames in the span.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Frames each column covers. Fractional, so columns stay even across a
    /// span that does not divide by the width.
    #[must_use]
    pub fn frames_per_pixel(&self) -> f64 {
        if self.pixels == 0 {
            return 0.0;
        }
        self.frames() as f64 / f64::from(self.pixels)
    }

    /// The level this request should be answered from.
    #[must_use]
    pub fn level(&self, levels: Levels) -> Level {
        levels.choose(self.frames_per_pixel())
    }
}

/// One drawn column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    /// Least sample value in the column.
    pub min: f32,
    /// Greatest sample value in the column.
    pub max: f32,
    /// Root mean square across the column.
    pub rms: f32,
    /// Frames the column was drawn from. Zero where there is no audio yet,
    /// which is what a capture still in progress looks like at its right edge.
    pub frames: u64,
}

impl Column {
    /// A column with nothing in it.
    pub const EMPTY: Self = Self {
        min: 0.0,
        max: 0.0,
        rms: 0.0,
        frames: 0,
    };

    /// Whether this column covers no audio.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.frames == 0
    }
}

/// A span of one channel, reduced to columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    /// First frame of the span, inclusive.
    pub start: u64,
    /// Last frame of the span, exclusive.
    pub end: u64,
    /// Frames each column covers.
    pub frames_per_pixel: f64,
    /// Where the data came from, for a caller that wants to report it.
    pub level: Level,
    /// Exactly [`Request::pixels`] of them, empty ones included.
    pub columns: Vec<Column>,
    /// Frames actually found. Short of [`Request::frames`] when the capture
    /// has not reached the end of the span yet.
    pub covered: u64,
}

impl Waveform {
    /// The last column that has any audio in it, if any does.
    #[must_use]
    pub fn drawn(&self) -> Option<usize> {
        self.columns.iter().rposition(|c| !c.is_empty())
    }

    /// The greatest magnitude anywhere in the span.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.columns
            .iter()
            .filter(|c| !c.is_empty())
            .fold(0.0f32, |peak, c| peak.max(c.min.abs()).max(c.max.abs()))
    }
}

/// Accumulator for one column, kept as sums so a column costs no allocation.
#[derive(Clone, Copy)]
struct Bucket {
    min: f32,
    max: f32,
    squares: f64,
    frames: u64,
}

impl Bucket {
    const EMPTY: Self = Self {
        min: f32::INFINITY,
        max: f32::NEG_INFINITY,
        squares: 0.0,
        frames: 0,
    };

    fn add(&mut self, summary: Summary, frames: u64) {
        if frames == 0 {
            return;
        }
        self.min = self.min.min(summary.min);
        self.max = self.max.max(summary.max);
        self.squares += f64::from(summary.rms) * f64::from(summary.rms) * frames as f64;
        self.frames += frames;
    }

    fn finish(self) -> Column {
        if self.frames == 0 {
            return Column::EMPTY;
        }
        Column {
            min: self.min,
            max: self.max,
            rms: (self.squares / self.frames as f64).sqrt() as f32,
            frames: self.frames,
        }
    }
}

/// Folds stored summaries into the columns a request asked for.
///
/// The reduction is exact wherever the source runs line up with the columns, and
/// where they do not it is exact per column anyway: a run straddling a boundary
/// contributes its overlap to each side, weighted by the frames in that overlap.
/// See [`Summary::merge`] for why weighting by frame count makes the RMS the same
/// number the samples would have given.
///
/// Runs may arrive in any order and may overlap the span partially or not at all;
/// anything outside is clipped. Feeding the same run twice would double-count it,
/// which is the caller's business - the reader hands over each stored triplet
/// once.
pub struct Painter {
    start: u64,
    end: u64,
    frames_per_pixel: f64,
    buckets: Vec<Bucket>,
    level: Level,
    covered: u64,
}

impl Painter {
    /// A painter for this request, reading from this level.
    #[must_use]
    pub fn new(request: &Request, level: Level) -> Self {
        Self {
            start: request.start,
            end: request.end,
            frames_per_pixel: request.frames_per_pixel(),
            buckets: vec![Bucket::EMPTY; request.pixels as usize],
            level,
            covered: 0,
        }
    }

    /// Which column a frame falls in, or `None` if it is outside the span.
    fn column_of(&self, frame: u64) -> Option<usize> {
        if frame < self.start || frame >= self.end || self.frames_per_pixel <= 0.0 {
            return None;
        }
        let at = ((frame - self.start) as f64 / self.frames_per_pixel) as usize;
        Some(at.min(self.buckets.len().saturating_sub(1)))
    }

    /// First frame of a column.
    fn frame_of(&self, column: usize) -> u64 {
        self.start + (column as f64 * self.frames_per_pixel) as u64
    }

    /// Adds one stored triplet covering `frames` frames from `start`.
    pub fn add(&mut self, start: u64, frames: u64, summary: Summary) {
        if frames == 0 || self.buckets.is_empty() {
            return;
        }
        let lo = start.max(self.start);
        let hi = (start + frames).min(self.end);
        if lo >= hi {
            return;
        }
        self.covered += hi - lo;

        let first = match self.column_of(lo) {
            Some(first) => first,
            None => return,
        };
        let last = self.column_of(hi - 1).unwrap_or(first);
        if first == last {
            self.buckets[first].add(summary, hi - lo);
            return;
        }
        // The run straddles a boundary. Each column takes the part of it that
        // falls inside that column, and takes the whole triplet's extremes with
        // it - which is the best that can be said without the samples, and is
        // why a level coarser than a pixel is never chosen.
        for column in first..=last {
            let bucket_lo = self.frame_of(column).max(lo);
            let bucket_hi = if column + 1 < self.buckets.len() {
                self.frame_of(column + 1).min(hi)
            } else {
                hi
            };
            if bucket_hi > bucket_lo {
                self.buckets[column].add(summary, bucket_hi - bucket_lo);
            }
        }
    }

    /// Adds raw audio, one sample per frame, starting at `start`.
    ///
    /// The [`Level::Samples`] path. Nothing is summarized first: each sample
    /// goes straight into the column it belongs to, so a column drawn this way
    /// is drawn from the audio and not from a summary of it.
    pub fn add_samples(&mut self, start: u64, format: StorageFormat, samples: &[u8]) {
        let n = format.samples_in(samples.len());
        for i in 0..n {
            let frame = start + i as u64;
            let Some(column) = self.column_of(frame) else {
                continue;
            };
            let Some(value) = format.decode_sample(samples, i) else {
                break;
            };
            self.covered += 1;
            self.buckets[column].add(
                Summary {
                    min: value,
                    max: value,
                    rms: value.abs(),
                },
                1,
            );
        }
    }

    /// The finished waveform.
    #[must_use]
    pub fn finish(self) -> Waveform {
        Waveform {
            start: self.start,
            end: self.end,
            frames_per_pixel: self.frames_per_pixel,
            level: self.level,
            columns: self.buckets.into_iter().map(Bucket::finish).collect(),
            covered: self.covered,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 250 ms at 48 kHz, which is what D3 gives a capture at that rate.
    const BLOCK: u32 = 12_000;

    fn levels() -> Levels {
        Levels::new(BLOCK)
    }

    fn summary(min: f32, max: f32, rms: f32) -> Summary {
        Summary { min, max, rms }
    }

    #[test]
    fn the_answer_has_exactly_as_many_columns_as_pixels_were_asked_for() {
        // §37 in one assertion: the shape of the answer does not depend on how
        // much audio it describes.
        for frames in [1_000u64, 48_000, 48_000 * 60, 48_000 * 60 * 60 * 3] {
            let request = Request::new(0, frames, 800);
            let mut painter = Painter::new(&request, request.level(levels()));
            painter.add(0, frames, summary(-0.5, 0.5, 0.25));
            let waveform = painter.finish();
            assert_eq!(waveform.columns.len(), 800, "{frames} frames");
        }
    }

    #[test]
    fn the_level_chosen_is_the_coarsest_that_still_fills_every_pixel() {
        let levels = levels();
        // A pixel narrower than 256 frames has to come from the audio.
        assert_eq!(levels.choose(1.0), Level::Samples);
        assert_eq!(levels.choose(255.9), Level::Samples);
        // Wide enough for a 256 triplet, not for a block.
        assert_eq!(levels.choose(256.0), Level::Summary256);
        assert_eq!(levels.choose(11_999.0), Level::Summary256);
        // Wide enough for a block, which is where the reading gets cheap.
        assert_eq!(levels.choose(12_000.0), Level::Block);
        assert_eq!(levels.choose(1_000_000.0), Level::Block);
    }

    /// The finding in the module doc, as an assertion rather than a claim.
    #[test]
    fn the_64k_level_is_never_read_from_a_capture_we_wrote() {
        let ours = levels();
        for fpp in [1.0, 256.0, 12_000.0, 65_536.0, 1e9] {
            assert_ne!(
                ours.choose(fpp),
                Level::Summary64k,
                "chosen at {fpp} frames per pixel"
            );
        }
        // It earns its place on blocks long enough for it to beat them, which
        // is what an imported Audacity block can be.
        let imported = Levels::new(1_048_576);
        assert_eq!(imported.choose(65_536.0), Level::Summary64k);
        assert_eq!(imported.choose(1_048_576.0), Level::Block);
    }

    #[test]
    fn a_column_is_the_summary_of_what_falls_in_it() {
        // Four triplets of 250 frames into two columns of 500.
        let request = Request::new(0, 1_000, 2);
        let mut painter = Painter::new(&request, Level::Summary256);
        painter.add(0, 250, summary(-0.1, 0.1, 0.1));
        painter.add(250, 250, summary(-0.9, 0.2, 0.5));
        painter.add(500, 250, summary(-0.3, 0.3, 0.3));
        painter.add(750, 250, summary(0.0, 0.0, 0.0));
        let waveform = painter.finish();

        assert_eq!(waveform.columns[0].min, -0.9);
        assert_eq!(waveform.columns[0].max, 0.2);
        // Equal weights, so the rms is the quadratic mean of 0.1 and 0.5.
        let expect = ((0.01f64 + 0.25) / 2.0).sqrt() as f32;
        assert!((waveform.columns[0].rms - expect).abs() < 1e-6);
        assert_eq!(waveform.columns[0].frames, 500);

        assert_eq!(waveform.columns[1].min, -0.3);
        assert_eq!(waveform.columns[1].max, 0.3);
        assert_eq!(waveform.covered, 1_000);
        assert_eq!(waveform.peak(), 0.9);
    }

    #[test]
    fn a_run_that_straddles_a_boundary_is_split_by_how_much_falls_each_side() {
        // One run of 100 frames across a boundary at 75: three quarters into
        // the first column, one into the second.
        let request = Request::new(0, 150, 2);
        let mut painter = Painter::new(&request, Level::Summary256);
        painter.add(0, 100, summary(-1.0, 1.0, 0.5));
        painter.add(100, 50, summary(-0.1, 0.1, 0.1));
        let waveform = painter.finish();

        assert_eq!(waveform.columns[0].frames, 75);
        assert_eq!(waveform.columns[1].frames, 75);
        // Column 1 is 25 frames of the loud run and 50 of the quiet one.
        let expect = ((0.25f64 * 25.0 + 0.01 * 50.0) / 75.0).sqrt() as f32;
        assert!(
            (waveform.columns[1].rms - expect).abs() < 1e-6,
            "{} is not {expect}",
            waveform.columns[1].rms
        );
        assert_eq!(
            waveform.columns[1].min, -1.0,
            "a straddling run lends its extremes to both sides, because \
             without the samples there is nothing finer to say"
        );
    }

    #[test]
    fn audio_outside_the_span_is_clipped_rather_than_folded_in() {
        let request = Request::new(1_000, 2_000, 4);
        let mut painter = Painter::new(&request, Level::Summary256);
        painter.add(0, 1_000, summary(-1.0, 1.0, 1.0)); // entirely before
        painter.add(2_000, 500, summary(-1.0, 1.0, 1.0)); // entirely after
        painter.add(900, 200, summary(-0.5, 0.5, 0.5)); // overlapping the start
        let waveform = painter.finish();

        assert_eq!(waveform.covered, 100, "only the overlap counts");
        assert_eq!(waveform.columns[0].frames, 100);
        assert_eq!(waveform.columns[0].max, 0.5);
        for column in &waveform.columns[1..] {
            assert!(column.is_empty());
        }
    }

    /// What a capture in progress looks like: audio on the left, nothing yet
    /// on the right, and the join between them findable.
    #[test]
    fn a_span_longer_than_the_audio_leaves_the_rest_of_the_columns_empty() {
        let request = Request::new(0, 1_000, 10);
        let mut painter = Painter::new(&request, Level::Block);
        painter.add(0, 350, summary(-0.4, 0.4, 0.2));
        let waveform = painter.finish();

        assert_eq!(waveform.drawn(), Some(3));
        assert_eq!(waveform.covered, 350);
        assert!(waveform.columns[4..].iter().all(Column::is_empty));
        assert!(!waveform.columns[0].is_empty());
    }

    #[test]
    fn samples_are_drawn_from_the_audio_and_not_from_a_summary_of_it() {
        let format = StorageFormat::Int16;
        let pcm: Vec<u8> = [0i16, 16_384, -32_768, 8_192]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();

        // One column per sample: each must report that sample exactly.
        let request = Request::new(0, 4, 4);
        assert_eq!(request.level(levels()), Level::Samples);
        let mut painter = Painter::new(&request, Level::Samples);
        painter.add_samples(0, format, &pcm);
        let waveform = painter.finish();

        assert_eq!(waveform.columns[0].max, 0.0);
        assert_eq!(waveform.columns[1].max, 0.5);
        assert_eq!(waveform.columns[2].min, -1.0);
        assert_eq!(waveform.columns[3].max, 0.25);
        assert!(waveform.columns.iter().all(|c| c.frames == 1));

        // And the same audio in one column is its true min, max and rms.
        let request = Request::new(0, 4, 1);
        let mut painter = Painter::new(&request, Level::Samples);
        painter.add_samples(0, format, &pcm);
        let column = painter.finish().columns[0];
        assert_eq!(column.min, -1.0);
        assert_eq!(column.max, 0.5);
        let expect = ((0.0f64 + 0.25 + 1.0 + 0.0625) / 4.0).sqrt() as f32;
        assert!((column.rms - expect).abs() < 1e-6);
    }

    #[test]
    fn a_span_can_be_drawn_at_any_width_including_awkward_ones() {
        // 1000 frames into 3 columns: 333.33 each, and no frame lost or
        // counted twice at the joins.
        let request = Request::new(0, 1_000, 3);
        let mut painter = Painter::new(&request, Level::Samples);
        for frame in 0..1_000u64 {
            painter.add(frame, 1, summary(0.0, 0.0, 0.0));
        }
        let waveform = painter.finish();
        let total: u64 = waveform.columns.iter().map(|c| c.frames).sum();
        assert_eq!(total, 1_000);
        assert!(waveform.columns.iter().all(|c| c.frames > 330));
    }

    #[test]
    fn a_request_for_nothing_is_answered_rather_than_panicked_over() {
        let empty = Request::new(0, 0, 10);
        assert_eq!(empty.frames(), 0);
        assert_eq!(empty.frames_per_pixel(), 0.0);
        let mut painter = Painter::new(&empty, Level::Block);
        painter.add(0, 100, summary(-1.0, 1.0, 1.0));
        let waveform = painter.finish();
        assert_eq!(waveform.columns.len(), 10);
        assert!(waveform.columns.iter().all(Column::is_empty));
        assert_eq!(waveform.drawn(), None);
        assert_eq!(waveform.peak(), 0.0);

        // Zero pixels is a degenerate ask, not a crash.
        let none = Request::new(0, 1_000, 0);
        let mut painter = Painter::new(&none, Level::Block);
        painter.add(0, 1_000, summary(-1.0, 1.0, 1.0));
        assert!(painter.finish().columns.is_empty());
    }

    /// An end-to-start span, which a UI can produce by dragging backwards.
    #[test]
    fn a_backwards_span_is_empty_and_not_enormous() {
        let request = Request::new(5_000, 1_000, 8);
        assert_eq!(request.frames(), 0, "saturating, not wrapping");
        let mut painter = Painter::new(&request, Level::Block);
        painter.add(1_000, 4_000, summary(-1.0, 1.0, 1.0));
        assert!(painter.finish().columns.iter().all(Column::is_empty));
    }
}
