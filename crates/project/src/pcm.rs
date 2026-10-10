/*
 *  pcm.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Reading a capture's stored audio back out as interleaved frames (§21).
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

//! Reading a capture's stored audio back out as interleaved frames (§21).
//!
//! The writer's inverse, and the only other place in the tree that knows blocks
//! are stored per channel. `persistence::prepare` splits an interleaved callback
//! into one blob per channel because that is AUP4's layout and S2 measured it
//! kinder to the WAL; playback has to put the frames back together, and the
//! seam between those two functions is the only thing standing between a
//! bit-perfect capture and a channel-swapped one.
//!
//! **Nothing here converts.** The bytes handed out are the bytes the converter
//! produced, in the capture's own [`StorageFormat`], and turning them into
//! something a particular output device will accept is `vcw-audio`'s problem
//! (and is reported rather than done quietly - see `vcw_audio::playback`).
//!
//! # Why it reads by sequence and not by frame
//!
//! Locating a block by frame number costs an indexed lookup. Locating the *next*
//! block costs nothing, because blocks are numbered: one block per sequence per
//! channel, the same span of time on every channel. So the reader does an indexed
//! lookup once per [`Reader::seek`] and walks `sequence + 1` from there, which is
//! what makes ordinary playback cheap and a seek exact rather than approximate.
//!
//! ```no_run
//! # fn main() -> Result<(), vcw_project::Error> {
//! use vcw_project::{pcm, Project};
//! use vcw_types::Span;
//!
//! let project = Project::open_read_only("album.vcw")?;
//! let layout = pcm::Layout::of(project.conn(), 1)?;
//! let mut reader = pcm::Reader::open(project.conn(), 1, Span::whole(layout.frames))?;
//!
//! let mut frames = vec![0u8; layout.frame_bytes() * 1024];
//! while !reader.is_finished() {
//!     let filled = reader.fill(&mut frames)?;
//!     // frames[..filled] is interleaved audio at layout.format
//!     let _ = filled;
//! }
//! # Ok(()) }
//! ```

use rusqlite::Connection;
use vcw_types::{SampleRate, Span, StorageFormat};

use crate::error::{Error, Result};
use crate::session;

/// What a capture's stored audio looks like to something that wants to play it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// The rate it was recorded at, which is the rate it has to be played at.
    pub rate: SampleRate,
    /// Channels, and therefore samples per frame.
    pub channels: u16,
    /// How a sample is laid out on disk.
    pub format: StorageFormat,
    /// Frames committed, per channel.
    pub frames: u64,
}

impl Layout {
    /// Reads a capture's layout.
    ///
    /// # Errors
    ///
    /// If the capture is not in this project, or cannot be read.
    pub fn of(conn: &Connection, capture_id: i64) -> Result<Self> {
        let record = session::load(conn, capture_id)?.ok_or(Error::NoSuchCapture { capture_id })?;
        Ok(Self {
            rate: record.info.rate,
            channels: record.info.channels,
            format: record.info.storage_format,
            frames: record.frames,
        })
    }

    /// Bytes in one interleaved frame.
    #[must_use]
    pub const fn frame_bytes(&self) -> usize {
        self.format.bytes_per_sample() * self.channels as usize
    }

    /// The whole capture, as a span.
    #[must_use]
    pub const fn span(&self) -> Span {
        Span::whole(self.frames)
    }

    /// How long the capture runs.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.span().seconds(self.rate)
    }
}

/// One block, decoded: the same span of time on every channel.
struct Loaded {
    sequence: u64,
    start_frame: u64,
    frames: u64,
    /// One blob per channel, indexed by channel number.
    channels: Vec<Vec<u8>>,
}

impl Loaded {
    /// Whether this block holds a frame.
    const fn holds(&self, frame: u64) -> bool {
        frame >= self.start_frame && frame < self.start_frame + self.frames
    }
}

/// Hands out a capture's audio, interleaved, from anywhere in it.
///
/// Holds a borrowed connection and no lock: playback opens the project
/// read-only on its own thread, so a capture can be auditioned while another one
/// is being recorded, which is what WAL is for.
pub struct Reader<'a> {
    conn: &'a Connection,
    capture_id: i64,
    layout: Layout,
    span: Span,
    /// The next frame to hand out.
    cursor: u64,
    /// The block the cursor is in, once something has needed it.
    loaded: Option<Loaded>,
}

impl<'a> Reader<'a> {
    /// Opens a reader over part of a capture.
    ///
    /// The span is clamped to what was actually recorded, so a caller asking for
    /// a track whose end was estimated past the end of the side gets the side.
    ///
    /// # Errors
    ///
    /// If the capture is not in this project, or cannot be read.
    pub fn open(conn: &'a Connection, capture_id: i64, span: Span) -> Result<Self> {
        let layout = Layout::of(conn, capture_id)?;
        let span = span.clamp_to(layout.frames);
        Ok(Self {
            conn,
            capture_id,
            layout,
            span,
            cursor: span.start,
            loaded: None,
        })
    }

    /// The capture's layout.
    #[must_use]
    pub const fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The span being played, after clamping.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// The next frame that will be handed out.
    #[must_use]
    pub const fn position(&self) -> u64 {
        self.cursor
    }

    /// Whether the span has been played out.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.cursor >= self.span.end
    }

    /// Moves the cursor, to an absolute frame in the capture.
    ///
    /// Clamped into the span rather than refused: seeking past the end of a
    /// selection means the end of the selection, and the transport above decides
    /// whether that stops playback or loops.
    ///
    /// Keeps the loaded block if the new position is still inside it, which is
    /// what makes a fine seek - a skip of a few frames, or a UI dragging a
    /// playhead - cost nothing at all.
    pub fn seek(&mut self, frame: u64) {
        self.cursor = frame.clamp(self.span.start, self.span.end);
        if !self
            .loaded
            .as_ref()
            .is_some_and(|block| block.holds(self.cursor))
        {
            self.loaded = None;
        }
    }

    /// Fills `dst` with interleaved frames and returns how many bytes it wrote.
    ///
    /// Short fills are ordinary: a fill stops at the end of the span, and stops
    /// at a whole frame, because handing out three bytes of a four-byte sample
    /// would tear every frame after it. Zero means the span is played out.
    ///
    /// # Errors
    ///
    /// If a block cannot be read, or the capture is missing audio the block
    /// index says it has - which is corruption, and `validate` is where it gets
    /// explained properly.
    pub fn fill(&mut self, dst: &mut [u8]) -> Result<usize> {
        let width = self.layout.format.bytes_per_sample();
        let frame_bytes = self.layout.frame_bytes();
        if frame_bytes == 0 {
            return Ok(0);
        }

        let mut written = 0usize;
        while written + frame_bytes <= dst.len() && !self.is_finished() {
            self.load_for_cursor()?;
            let Some(block) = self.loaded.as_ref() else {
                break;
            };

            // How much of this block is wanted, and how much room is left.
            let offset = (self.cursor - block.start_frame) as usize;
            let in_block = block.frames as usize - offset;
            let to_span = (self.span.end - self.cursor) as usize;
            let room = (dst.len() - written) / frame_bytes;
            let frames = in_block.min(to_span).min(room);

            // Interleave: one sample from each channel, in channel order, which
            // is exactly what `persistence::prepare` took apart.
            for frame in 0..frames {
                let at = written + frame * frame_bytes;
                let from = (offset + frame) * width;
                for (channel, samples) in block.channels.iter().enumerate() {
                    let to = at + channel * width;
                    dst[to..to + width].copy_from_slice(&samples[from..from + width]);
                }
            }

            written += frames * frame_bytes;
            self.cursor += frames as u64;
            if self.cursor >= block.start_frame + block.frames {
                self.loaded = None;
            }
        }
        Ok(written)
    }

    /// Makes sure the block holding the cursor is the loaded one.
    fn load_for_cursor(&mut self) -> Result<()> {
        if self
            .loaded
            .as_ref()
            .is_some_and(|block| block.holds(self.cursor))
        {
            return Ok(());
        }
        // The next block is the one after the last, except after a seek, when
        // it has to be looked up. That is the whole reason `Loaded` remembers
        // its sequence.
        let sequence = match self.loaded.as_ref() {
            Some(block) if self.cursor >= block.start_frame + block.frames => block.sequence + 1,
            _ => self.locate(self.cursor)?,
        };
        self.loaded = self.load(sequence)?;
        Ok(())
    }

    /// Finds the sequence number of the block holding a frame.
    ///
    /// Channel 0 answers for all of them, because a block is the same span of
    /// time on every channel. Named against the timeline index, for the reason
    /// given in `waveform::sql_for`: an unindexed answer here would walk every
    /// block in the capture on every seek.
    fn locate(&self, frame: u64) -> Result<u64> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT sequence, start_frame, frame_count \
             FROM capture_blocks INDEXED BY capture_blocks_timeline \
             WHERE capture_id = ?1 AND channel = 0 AND start_frame <= ?2 \
             ORDER BY start_frame DESC LIMIT 1",
        )?;
        let found = stmt
            .query_row(
                (self.capture_id, session::clamp(frame)),
                |row| -> rusqlite::Result<(i64, i64, i64)> {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                },
            )
            .map_err(Error::from);

        let found = found.map(|(sequence, start_frame, frames)| {
            (
                sequence.max(0) as u64,
                start_frame.max(0) as u64,
                frames.max(0) as u64,
            )
        });
        match found {
            Ok((sequence, start_frame, frames)) if frame < start_frame + frames => Ok(sequence),
            // A frame inside the capture's length that no block covers is a hole
            // in the timeline, not an end. `validate` explains those; playback
            // only has to refuse to invent audio for one.
            Ok((sequence, _, _)) => Err(Error::Unplayable {
                capture_id: self.capture_id,
                channel: 0,
                sequence,
                why: format!("no block holds frame {frame}"),
            }),
            Err(Error::Sqlite(rusqlite::Error::QueryReturnedNoRows)) => Err(Error::Unplayable {
                capture_id: self.capture_id,
                channel: 0,
                sequence: 0,
                why: format!("no block holds frame {frame}"),
            }),
            Err(other) => Err(other),
        }
    }

    /// Reads one block, every channel of it.
    ///
    /// `None` means there is no such sequence, which at the end of a capture is
    /// the ordinary case. A sequence that exists on one channel and not another
    /// is corruption, and says so.
    fn load(&self, sequence: u64) -> Result<Option<Loaded>> {
        let width = self.layout.format.bytes_per_sample();
        let mut channels = Vec::with_capacity(self.layout.channels as usize);
        let mut shape: Option<(u64, u64)> = None;

        for channel in 0..self.layout.channels {
            let mut stmt = self.conn.prepare_cached(
                "SELECT cb.start_frame, cb.frame_count, sb.samples \
                 FROM capture_blocks cb JOIN sampleblocks sb ON sb.blockid = cb.blockid \
                 WHERE cb.capture_id = ?1 AND cb.channel = ?2 AND cb.sequence = ?3",
            )?;
            let row = stmt
                .query_row(
                    (
                        self.capture_id,
                        i64::from(channel),
                        session::clamp(sequence),
                    ),
                    |row| -> rusqlite::Result<(i64, i64, Option<Vec<u8>>)> {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                    },
                )
                .map(Some)
                .or_else(|err| match err {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(Error::from(other)),
                })?;

            let row = row.map(|(start_frame, frames, samples)| {
                (start_frame.max(0) as u64, frames.max(0) as u64, samples)
            });
            let Some((start_frame, frames, samples)) = row else {
                if channel == 0 {
                    // Past the last block. Ordinary.
                    return Ok(None);
                }
                return Err(Error::Unplayable {
                    capture_id: self.capture_id,
                    channel,
                    sequence,
                    why: "the block is missing on this channel but present on channel 0".to_owned(),
                });
            };
            let samples = samples.ok_or_else(|| Error::Unplayable {
                capture_id: self.capture_id,
                channel,
                sequence,
                why: "the block holds no audio".to_owned(),
            })?;
            let wanted = frames as usize * width;
            if samples.len() < wanted {
                return Err(Error::Unplayable {
                    capture_id: self.capture_id,
                    channel,
                    sequence,
                    why: format!(
                        "the block claims {frames} frames but holds {} byte(s), not {wanted}",
                        samples.len()
                    ),
                });
            }
            if let Some((first_start, first_frames)) = shape
                && (first_start != start_frame || first_frames != frames)
            {
                return Err(Error::Unplayable {
                    capture_id: self.capture_id,
                    channel,
                    sequence,
                    why: format!(
                        "the block covers frames {start_frame}+{frames} on this channel \
                         but {first_start}+{first_frames} on channel 0"
                    ),
                });
            }
            shape = Some((start_frame, frames));
            channels.push(samples);
        }

        let (start_frame, frames) = shape.unwrap_or((0, 0));
        Ok(Some(Loaded {
            sequence,
            start_frame,
            frames,
            channels,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{Config, Writer};
    use crate::sqlite::Project;
    use vcw_types::{CaptureEq, CaptureInfo, CaptureMode, CaptureState};

    const RATE: SampleRate = SampleRate(96_000);
    const WIDTH: usize = 4;
    const CHANNELS: u16 = 2;
    const FRAME: usize = WIDTH * CHANNELS as usize;

    fn info() -> CaptureInfo {
        CaptureInfo {
            rate: RATE,
            channels: CHANNELS,
            storage_format: StorageFormat::Int24Padded,
            capture_mode: CaptureMode::Exclusive,
            host_api: Some("ALSA".into()),
            device_id: Some("hw:CARD=0,DEV=0".into()),
            device_name: Some("Cirrus Analog".into()),
            os_verified: false,
            os_report: None,
            eq: CaptureEq::Unknown,
        }
    }

    /// Every byte is a function of its own offset, so one wrong byte anywhere
    /// says where it came from - and a channel swap is not a subtle failure.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    /// A capture of `frames` frames of [`pattern`], and the bytes that went in.
    fn recorded(dir: &tempfile::TempDir, frames: usize) -> (Project, i64, Vec<u8>) {
        let info = info();
        let project = Project::create(dir.path().join("pcm.vcw")).expect("create");
        let mut writer = Writer::begin(project, &info, Config::default()).expect("begin");
        let sent = pattern(frames * FRAME);
        writer.push(&sent).expect("push");
        let (outcome, project, _) = writer
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");
        (project, outcome.capture_id, sent)
    }

    /// Reads a whole span through a buffer of `chunk` bytes.
    fn drain(reader: &mut Reader<'_>, chunk: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buffer = vec![0u8; chunk];
        loop {
            let filled = reader.fill(&mut buffer).expect("fill");
            if filled == 0 {
                break;
            }
            out.extend_from_slice(&buffer[..filled]);
        }
        out
    }

    #[test]
    fn the_frames_that_come_back_are_the_frames_that_went_in() {
        // §9 through the whole round trip. The writer split the stream per
        // channel; if the reader reassembles it even slightly wrong, a stereo
        // capture comes back with its channels swapped or its frames sheared,
        // and both of those are inaudible in a summary and obvious here.
        let dir = tempfile::tempdir().expect("tempdir");
        // Two and a bit blocks at 250 ms / 96 kHz, so a short final block is in.
        let (project, capture_id, sent) = recorded(&dir, 24_000 * 2 + 1_234);
        let layout = Layout::of(project.conn(), capture_id).expect("layout");
        assert_eq!(layout.frames, 24_000 * 2 + 1_234);
        assert_eq!(layout.frame_bytes(), FRAME);

        let mut reader = Reader::open(project.conn(), capture_id, layout.span()).expect("open");
        let got = drain(&mut reader, 4_096);
        assert_eq!(got.len(), sent.len());
        assert_eq!(got, sent, "the playback path altered a byte");
        assert!(reader.is_finished());
    }

    #[test]
    fn the_buffer_size_the_caller_happens_to_use_changes_nothing() {
        // A device hands out whatever period size it likes, and one of them is
        // smaller than a frame's worth of anything sensible. Every chunking of
        // the same span has to produce the same bytes, including ones that do
        // not divide a block and ones that do not divide a frame.
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, capture_id, sent) = recorded(&dir, 24_000 + 500);
        let span = Span::whole(24_000 + 500);
        for chunk in [FRAME, FRAME + 3, 999, 4_096, 1 << 20] {
            let mut reader = Reader::open(project.conn(), capture_id, span).expect("open");
            let got = drain(&mut reader, chunk);
            assert_eq!(got, sent, "a {chunk}-byte buffer changed the audio");
        }
    }

    #[test]
    fn a_fill_never_hands_out_part_of_a_frame() {
        // Three bytes of a four-byte sample would tear every frame after it.
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, capture_id, _) = recorded(&dir, 24_000);
        let mut reader =
            Reader::open(project.conn(), capture_id, Span::whole(24_000)).expect("open");

        let mut ragged = vec![0u8; FRAME * 10 + 5];
        let filled = reader.fill(&mut ragged).expect("fill");
        assert_eq!(filled % FRAME, 0);
        assert_eq!(filled, FRAME * 10);

        // And a buffer too small for even one frame writes nothing rather than
        // half of one.
        let mut tiny = vec![0u8; FRAME - 1];
        assert_eq!(reader.fill(&mut tiny).expect("fill"), 0);
    }

    #[test]
    fn a_span_hands_out_exactly_the_frames_it_names() {
        // A region audition, and the case that makes the half-open convention
        // worth having: the frame at `end` must not be played.
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, capture_id, sent) = recorded(&dir, 24_000 * 3);
        let span = Span::new(30_000, 61_111);
        let mut reader = Reader::open(project.conn(), capture_id, span).expect("open");
        assert_eq!(reader.position(), 30_000);

        let got = drain(&mut reader, 8_192);
        assert_eq!(got.len(), span.frames() as usize * FRAME);
        assert_eq!(got, sent[30_000 * FRAME..61_111 * FRAME]);
    }

    #[test]
    fn a_span_past_the_end_plays_what_was_recorded_and_stops() {
        // Track ends are estimated, and the last one is routinely estimated
        // past the end of the side. Playing the side is the only useful answer.
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, capture_id, sent) = recorded(&dir, 10_000);
        let mut reader =
            Reader::open(project.conn(), capture_id, Span::new(5_000, 900_000)).expect("open");
        assert_eq!(reader.span(), Span::new(5_000, 10_000));
        let got = drain(&mut reader, 4_096);
        assert_eq!(got, sent[5_000 * FRAME..]);
    }

    #[test]
    fn seeking_lands_on_the_frame_it_names_from_either_direction() {
        // The exactness the transport's gapless seek is built on: after a seek
        // the next byte handed out is the first byte of that frame, whether the
        // seek went forwards, backwards, or nowhere at all.
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, capture_id, sent) = recorded(&dir, 24_000 * 4);
        let mut reader =
            Reader::open(project.conn(), capture_id, Span::whole(24_000 * 4)).expect("open");

        let mut buffer = vec![0u8; FRAME * 64];
        for target in [0u64, 1, 23_999, 24_000, 24_001, 71_003, 12, 95_936] {
            reader.seek(target);
            assert_eq!(reader.position(), target);
            let filled = reader.fill(&mut buffer).expect("fill");
            let at = target as usize * FRAME;
            assert_eq!(
                &buffer[..filled],
                &sent[at..at + filled],
                "a seek to {target} handed out the wrong frame"
            );
        }
    }

    #[test]
    fn a_seek_inside_the_loaded_block_keeps_it() {
        // Why `Loaded` remembers its sequence, and why dragging a playhead is
        // cheap: a seek within the loaded 250 ms costs no query at all, and one
        // outside it costs exactly one indexed lookup.
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, capture_id, _) = recorded(&dir, 24_000 * 4);
        let mut reader =
            Reader::open(project.conn(), capture_id, Span::whole(24_000 * 4)).expect("open");

        let mut buffer = vec![0u8; FRAME * 8];
        reader.seek(30_000);
        reader.fill(&mut buffer).expect("fill");
        let held = reader.loaded.as_ref().expect("a block is loaded").sequence;
        assert_eq!(held, 1, "frame 30,000 is in the second 24,000-frame block");

        reader.seek(47_999);
        assert!(
            reader.loaded.is_some(),
            "a seek within the block dropped it"
        );
        reader.seek(48_000);
        assert!(
            reader.loaded.is_none(),
            "a seek past the block kept a stale one"
        );
    }

    #[test]
    fn a_hole_in_the_timeline_refuses_rather_than_playing_silence() {
        // Silence in place of a missing block is indistinguishable from silence
        // that was recorded, so the reader will not produce it. Corruption made
        // deliberately here; D4's immutability rule is about the writer.
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, capture_id, _) = recorded(&dir, 24_000 * 3);
        project
            .conn()
            .execute(
                "DELETE FROM capture_blocks WHERE capture_id = ?1 AND sequence = 1",
                [capture_id],
            )
            .expect("delete");

        let mut reader =
            Reader::open(project.conn(), capture_id, Span::whole(24_000 * 3)).expect("open");
        let mut buffer = vec![0u8; FRAME * 1_024];
        // The first block still plays.
        assert!(reader.fill(&mut buffer).expect("first block") > 0);
        reader.seek(30_000);
        match reader.fill(&mut buffer) {
            Err(Error::Unplayable { capture_id: id, .. }) => assert_eq!(id, capture_id),
            Err(other) => panic!("wrong error: {other}"),
            Ok(n) => panic!("invented {n} byte(s) of audio for a missing block"),
        }
    }

    #[test]
    fn a_block_missing_on_one_channel_only_is_named_as_such() {
        // The failure that would otherwise play one channel and silence the
        // other, which sounds like a bad pressing rather than a bad file.
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, capture_id, _) = recorded(&dir, 24_000 * 2);
        project
            .conn()
            .execute(
                "DELETE FROM capture_blocks WHERE capture_id = ?1 AND sequence = 0 AND channel = 1",
                [capture_id],
            )
            .expect("delete");

        let mut reader =
            Reader::open(project.conn(), capture_id, Span::whole(24_000 * 2)).expect("open");
        let mut buffer = vec![0u8; FRAME * 16];
        match reader.fill(&mut buffer) {
            Err(Error::Unplayable { channel: 1, .. }) => {}
            Err(other) => panic!("wrong error: {other}"),
            Ok(_) => panic!("a half-missing block played"),
        }
    }

    #[test]
    fn an_empty_span_and_an_empty_capture_both_play_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (project, capture_id, _) = recorded(&dir, 1_000);
        let mut buffer = vec![0u8; FRAME * 8];

        let mut nothing =
            Reader::open(project.conn(), capture_id, Span::new(500, 500)).expect("open");
        assert!(nothing.is_finished());
        assert_eq!(nothing.fill(&mut buffer).expect("fill"), 0);

        // A capture that exists and holds no audio: the state a project is in
        // between `arm` and the first commit.
        let info = info();
        let mut empty = Project::create(dir.path().join("empty.vcw")).expect("create");
        let session = session::Session::begin(&mut empty, &info).expect("begin");
        let layout = Layout::of(empty.conn(), session.id()).expect("layout");
        assert_eq!(layout.frames, 0);
        let mut reader = Reader::open(empty.conn(), session.id(), layout.span()).expect("open");
        assert!(reader.is_finished());
        assert_eq!(reader.fill(&mut buffer).expect("fill"), 0);
    }

    #[test]
    fn a_capture_that_is_not_there_is_refused_at_the_door() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::create(dir.path().join("none.vcw")).expect("create");
        match Reader::open(project.conn(), 42, Span::whole(100)) {
            Err(Error::NoSuchCapture { capture_id: 42 }) => {}
            Err(other) => panic!("wrong error: {other}"),
            Ok(_) => panic!("a reader opened over a capture that does not exist"),
        }
    }
}
