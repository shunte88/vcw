/*
 *  mp4.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  An ISO base media container for one Apple Lossless track (WP-26).
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

//! An ISO base media container for one Apple Lossless track (WP-26).
//!
//! `vcw-alac` emits raw ALAC packets and nothing else - no framing, no lengths,
//! no index. Everything a decoder needs to make sense of them lives here: how
//! long each packet is, how many frames it holds, where it sits in the file,
//! and the `ALACSpecificConfig` that says what the samples inside it mean.
//!
//! ## Why this is hand-written rather than a dependency
//!
//! Every MP4 crate on crates.io is built for the general case - several tracks,
//! several codecs, edit lists, fragments - and would be a large dependency used
//! for one audio track with no edits. What that track needs is about a dozen
//! boxes, each one a fixed template around three or four numbers, and
//! [`Boxes`] makes writing one a two-line affair. The whole muxer is shorter
//! than the glue a general library would need.
//!
//! ## The layout, and why `moov` is last
//!
//! ```text
//! ftyp        brands
//! mdat        every ALAC packet, back to back, one chunk
//! moov        the index
//! ```
//!
//! An index cannot be written first. ALAC packets are variable length, so the
//! sample size table is not known until the last one is encoded, and two fields
//! of the magic cookie are running maxima the encoder only finishes filling in
//! at the same moment. Writing `moov` first would mean either buffering a whole
//! side of audio in memory or seeking back over a table whose size we guessed.
//! Writing it last means one streaming pass and no guesses.
//!
//! The cost is that the file is not progressively streamable - a player reading
//! it over a slow pipe has to reach the end before it can start. That is
//! `ffmpeg -movflags +faststart`'s problem to solve, it costs a second pass
//! over the file to solve it, and nothing about an archival copy on a local
//! disk cares. If that ever changes, the fix is to write `moov` into a buffer
//! exactly as [`Mp4::finish`] already does and copy the two parts in the other
//! order.
//!
//! ## One chunk
//!
//! `stsc` says every sample is in chunk one and `stco` gives that chunk's
//! offset, which is a fixed number known before any audio is written. The
//! alternative - grouping packets into chunks - buys an offset table nothing
//! reads and an off-by-one nobody needs.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};

use crate::error::Result;

/// Frames in every ALAC packet but the last.
///
/// Apple's default, which is also the only value `vcw-alac` is tested at and
/// the one every decoder has a fast path for.
pub(crate) const FRAMES_PER_PACKET: u32 = 4096;

/// A buffer that knows how to open and close a box.
///
/// An ISO box is a big-endian length, a four-character kind, then its payload -
/// and the length counts itself, so it can only be written once the payload is
/// finished. [`open`](Boxes::open) leaves four zero bytes behind and
/// [`close`](Boxes::close) comes back and fills them in, which is what makes
/// nesting read like the structure it builds.
#[derive(Default)]
struct Boxes(Vec<u8>);

impl Boxes {
    /// Starts a box and returns the handle [`close`](Boxes::close) wants.
    fn open(&mut self, kind: &[u8; 4]) -> usize {
        let at = self.0.len();
        self.0.extend_from_slice(&[0; 4]);
        self.0.extend_from_slice(kind);
        at
    }

    /// Starts a *full* box - one carrying a version and three flag bytes.
    fn full(&mut self, kind: &[u8; 4], version: u8, flags: u32) -> usize {
        let at = self.open(kind);
        self.0.push(version);
        self.0.extend_from_slice(&flags.to_be_bytes()[1..]);
        at
    }

    /// Backfills the length of the box opened at `at`.
    ///
    /// # Panics
    ///
    /// If the box grew past 4 GiB. Only `mdat` can, and `mdat` is not written
    /// through this - see [`Mp4::create`].
    fn close(&mut self, at: usize) {
        let len = u32::try_from(self.0.len() - at).expect("a moov box under 4 GiB");
        self.0[at..at + 4].copy_from_slice(&len.to_be_bytes());
    }

    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.0.extend_from_slice(value);
    }

    /// `count` zero bytes, for the reserved runs these boxes are full of.
    fn zeros(&mut self, count: usize) {
        self.0.resize(self.0.len() + count, 0);
    }

    /// The identity transform, as `tkhd` and `mvhd` both have to carry it.
    ///
    /// Nine 16.16 fixed-point values except the last three, which are 2.30.
    /// Meaningless for audio and not optional.
    fn unity_matrix(&mut self) {
        for value in [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
            self.u32(value);
        }
    }
}

/// The `ftyp` box, which is the same every time.
///
/// `M4A ` as the major brand is what makes a player treat this as music rather
/// than as a movie with no pictures, and it is why the extension is `.m4a`.
/// `isom` and `mp42` are there because a reader that has never heard of `M4A `
/// will have heard of those.
fn ftyp() -> Vec<u8> {
    let mut boxes = Boxes::default();
    let at = boxes.open(b"ftyp");
    boxes.bytes(b"M4A ");
    boxes.u32(0);
    for brand in [b"M4A ", b"mp42", b"isom"] {
        boxes.bytes(brand);
    }
    boxes.close(at);
    boxes.0
}

/// Everything a `moov` box needs that the caller has to supply.
pub(crate) struct Track<'a> {
    /// Samples per second, used as both the movie and the media timescale.
    pub(crate) rate: u32,
    /// Channels, as the sample entry declares them.
    pub(crate) channels: u16,
    /// Bits per sample, as the sample entry declares them.
    pub(crate) bits: u16,
    /// The `ALACSpecificConfig` from the encoder, after the last packet.
    pub(crate) cookie: &'a [u8],
    /// The length in bytes of every packet, in order.
    pub(crate) sizes: &'a [u32],
    /// Frames in the last packet, which is the only one that can be short.
    pub(crate) last_packet_frames: u32,
    /// Where the first packet starts in the finished file.
    pub(crate) audio_at: u32,
}

impl Track<'_> {
    /// Total frames in the track, which is also its duration in media time.
    ///
    /// Derived rather than passed in: the duration a container declares and the
    /// audio it actually holds have to be the same number, and the only way to
    /// be sure of that is to count the packets that were written.
    fn frames(&self) -> u64 {
        match self.sizes.len() {
            0 => 0,
            n => (n as u64 - 1) * u64::from(FRAMES_PER_PACKET) + u64::from(self.last_packet_frames),
        }
    }
}

/// Builds the whole `moov` box.
///
/// One function rather than one per box. The nesting is the structure, the
/// numbers are all in [`Track`], and splitting it into twelve three-line
/// functions would hide the one thing worth seeing: which box closes where.
#[allow(clippy::too_many_lines, reason = "the box tree reads better unbroken")]
fn moov(track: &Track<'_>) -> Vec<u8> {
    let frames = track.frames();
    // `mvhd` and `tkhd` are version 0, so their durations are 32-bit. The
    // movie timescale is the sample rate - see `mvhd` below - so this saturates
    // at 2^32 frames, which is 6.3 hours at 192 kHz and 27 at 44.1. A side that
    // long is not a side, and a wrong duration is a cosmetic fault in a file
    // whose samples are all still there.
    let short_duration = u32::try_from(frames).unwrap_or(u32::MAX);
    let mut b = Boxes::default();

    let moov = b.open(b"moov");

    // The movie timescale is the sample rate rather than the conventional 1000.
    // It costs nothing and it means the movie duration, the media duration and
    // the frame count are one number that cannot be rounded into disagreement.
    let at = b.full(b"mvhd", 0, 0);
    b.u32(0); // creation time - see `the file says nothing about when it was made`
    b.u32(0); // modification time
    b.u32(track.rate); // timescale
    b.u32(short_duration);
    b.u32(0x0001_0000); // rate, 1.0
    b.u16(0x0100); // volume, 1.0
    b.zeros(2 + 8); // reserved
    b.unity_matrix();
    b.zeros(24); // pre_defined
    b.u32(2); // next track id
    b.close(at);

    let trak = b.open(b"trak");

    // Flags 7: enabled, in the movie, in the preview.
    let at = b.full(b"tkhd", 0, 7);
    b.u32(0); // creation time
    b.u32(0); // modification time
    b.u32(1); // track id
    b.zeros(4); // reserved
    b.u32(short_duration);
    b.zeros(8); // reserved
    b.u16(0); // layer
    b.u16(0); // alternate group
    b.u16(0x0100); // volume, 1.0 - an audio track, so this one matters
    b.zeros(2); // reserved
    b.unity_matrix();
    b.u32(0); // width
    b.u32(0); // height
    b.close(at);

    let mdia = b.open(b"mdia");

    let at = b.full(b"mdhd", 0, 0);
    b.u32(0); // creation time
    b.u32(0); // modification time
    b.u32(track.rate); // timescale
    b.u32(short_duration);
    // Language, as three five-bit letters offset from 0x60: `und`, undetermined.
    // The honest answer for a record with no spoken content, and the one that
    // stops a player captioning the track with a language it guessed.
    b.u16(0x55C4);
    b.u16(0); // pre_defined
    b.close(at);

    let at = b.full(b"hdlr", 0, 0);
    b.u32(0); // pre_defined
    b.bytes(b"soun");
    b.zeros(12); // reserved
    // The handler name. A counted-then-null string in QuickTime and a plain
    // null-terminated one in ISO; every reader that cares accepts an empty
    // string, and an empty string cannot be wrong in either dialect.
    b.u8(0);
    b.close(at);

    let minf = b.open(b"minf");

    let at = b.full(b"smhd", 0, 0);
    b.u16(0); // balance, centre
    b.u16(0); // reserved
    b.close(at);

    // The data is in this file. `dref` says so with one `url ` entry whose
    // self-contained flag is set, and whose payload is therefore empty.
    let dinf = b.open(b"dinf");
    let at = b.full(b"dref", 0, 0);
    b.u32(1); // entry count
    let url = b.full(b"url ", 0, 1);
    b.close(url);
    b.close(at);
    b.close(dinf);

    let stbl = b.open(b"stbl");

    let at = b.full(b"stsd", 0, 0);
    b.u32(1); // entry count
    let entry = b.open(b"alac");
    b.zeros(6); // reserved
    b.u16(1); // data reference index
    b.zeros(8); // version, revision, vendor - QuickTime sound, all zero
    b.u16(track.channels);
    b.u16(track.bits);
    b.u16(0); // pre_defined / compression id
    b.u16(0); // reserved / packet size
    // The sample rate as 16.16 fixed point, which cannot hold 96 kHz or
    // 192 kHz - both of which this project records at. ffmpeg writes zero
    // rather than a truncated rate in exactly this case, and so does this: a
    // reader takes the real rate from the `mdhd` timescale and from the magic
    // cookie below, and a plausible-looking wrong number here would be worse
    // than an obviously absent one.
    b.u32(u16::try_from(track.rate).map_or(0, |rate| u32::from(rate) << 16));
    // The `ALACSpecificConfig`, in the full box the format wants it in. Without
    // this the packets are undecodable: it carries the frame length, the bit
    // depth, the three Golomb parameters and the real sample rate.
    let config = b.full(b"alac", 0, 0);
    b.bytes(track.cookie);
    b.close(config);
    b.close(entry);
    b.close(at);

    // Sample durations. Every packet holds `FRAMES_PER_PACKET` frames except
    // possibly the last, so this is one entry, or two when the side does not
    // divide evenly - which is almost always.
    let at = b.full(b"stts", 0, 0);
    let full_packets = track.sizes.len().saturating_sub(1) as u32;
    let short_last = !track.sizes.is_empty() && track.last_packet_frames != FRAMES_PER_PACKET;
    let runs: &[(u32, u32)] = if track.sizes.is_empty() {
        &[]
    } else if short_last {
        &[
            (full_packets, FRAMES_PER_PACKET),
            (1, track.last_packet_frames),
        ]
    } else {
        &[(full_packets + 1, FRAMES_PER_PACKET)]
    };
    b.u32(runs.iter().filter(|(count, _)| *count > 0).count() as u32);
    for &(count, delta) in runs {
        if count > 0 {
            b.u32(count);
            b.u32(delta);
        }
    }
    b.close(at);

    // Every sample is in chunk one. See the module docs.
    let at = b.full(b"stsc", 0, 0);
    if track.sizes.is_empty() {
        b.u32(0);
    } else {
        b.u32(1);
        b.u32(1); // first chunk
        b.u32(track.sizes.len() as u32); // samples per chunk
        b.u32(1); // sample description index
    }
    b.close(at);

    // Sizes. Zero in the common-size field means "they vary, read the table",
    // which for a lossless codec they always do.
    let at = b.full(b"stsz", 0, 0);
    b.u32(0); // sample size: varies
    b.u32(track.sizes.len() as u32);
    for &size in track.sizes {
        b.u32(size);
    }
    b.close(at);

    // One chunk, so one offset, and it is known before a byte of audio is
    // written - `ftyp` and the `mdat` header are both fixed length.
    let at = b.full(b"stco", 0, 0);
    if track.sizes.is_empty() {
        b.u32(0);
    } else {
        b.u32(1);
        b.u32(track.audio_at);
    }
    b.close(at);

    b.close(stbl);
    b.close(minf);
    b.close(mdia);
    b.close(trak);
    b.close(moov);
    b.0
}

/// Bytes reserved before the first packet for the `mdat` header.
///
/// Sixteen either way, which is what lets the audio start at a known offset
/// before its length is known. Which sixteen is decided by
/// [`finish`](Mp4::finish): under 4 GiB it is an eight byte `free` box followed
/// by an ordinary 32-bit `mdat` header, and over it a 64-bit one - a 32-bit `1`,
/// the kind, then the real length. Reserving the wide form unconditionally
/// means there is no size at which we have to refuse, the way WAV does with
/// [`crate::Error::TooLarge`].
///
/// Writing the narrow form when it fits is not cosmetic. lofty 0.25 skips a
/// 64-bit atom by seeking `len - 8` from *after* the sixteen byte header rather
/// than from after the first eight, which lands eight bytes inside whatever
/// follows; `moov` then parses as rubbish and tagging fails with "failed to
/// fill whole buffer". Every file VCW actually produces takes the narrow path,
/// so the bug is only reachable by a side over 4 GiB, and a `free` box costs
/// nothing to anyone.
const MDAT_HEADER: usize = 16;

/// An open `.m4a` file, mid-write.
pub(crate) struct Mp4 {
    file: File,
    /// Where the first packet starts, which `stco` has to point at.
    audio_at: u32,
    /// Bytes of audio written so far, for the `mdat` length.
    mdat_bytes: u64,
    /// Every packet's length, in order. The `stsz` table.
    sizes: Vec<u32>,
    /// Frames in the packet written most recently.
    last_packet_frames: u32,
}

impl Mp4 {
    /// Creates the file and writes everything that comes before the audio.
    ///
    /// # Errors
    ///
    /// If the file cannot be made or the header cannot be written.
    pub(crate) fn create(file: File) -> Result<Self> {
        let mut file = file;
        let header = ftyp();
        file.write_all(&header)?;
        // The `mdat` length is backfilled by `finish`, the same way the RIFF
        // size is - there is no way to know it before the last packet.
        file.write_all(&1u32.to_be_bytes())?;
        file.write_all(b"mdat")?;
        file.write_all(&0u64.to_be_bytes())?;
        Ok(Self {
            // `ftyp` is 32 bytes and the `mdat` header 16, so this is 48 and
            // could be a constant. It is computed because a brand added to
            // `ftyp` would silently move every sample otherwise.
            audio_at: u32::try_from(header.len() + MDAT_HEADER).expect("a 48 byte preamble"),
            file,
            mdat_bytes: 0,
            sizes: Vec::new(),
            last_packet_frames: 0,
        })
    }

    /// Appends one encoded ALAC packet and records its length and frame count.
    ///
    /// # Errors
    ///
    /// If the write fails.
    pub(crate) fn packet(&mut self, bytes: &[u8], frames: u32) -> Result<()> {
        self.file.write_all(bytes)?;
        self.mdat_bytes += bytes.len() as u64;
        self.sizes.push(bytes.len() as u32);
        self.last_packet_frames = frames;
        Ok(())
    }

    /// Backfills the `mdat` length, writes `moov` and returns the file size.
    ///
    /// `cookie` has to be the encoder's *final* magic cookie: two of its fields
    /// are maxima that are only correct once the last packet has been encoded,
    /// which is why this takes it as an argument rather than having been handed
    /// it at [`create`](Self::create).
    ///
    /// # Errors
    ///
    /// If the length cannot be corrected or the index cannot be written.
    pub(crate) fn finish(
        mut self,
        rate: u32,
        channels: u16,
        bits: u16,
        cookie: &[u8],
    ) -> Result<u64> {
        let index = moov(&Track {
            rate,
            channels,
            bits,
            cookie,
            sizes: &self.sizes,
            last_packet_frames: self.last_packet_frames,
            audio_at: self.audio_at,
        });
        self.file.write_all(&index)?;

        // The length counts the header as well as the audio, and which header
        // that is depends on whether the narrow one can hold it.
        self.file.seek(SeekFrom::Start(
            u64::from(self.audio_at) - MDAT_HEADER as u64,
        ))?;
        match u32::try_from(self.mdat_bytes + 8) {
            Ok(narrow) => {
                self.file.write_all(&8u32.to_be_bytes())?;
                self.file.write_all(b"free")?;
                self.file.write_all(&narrow.to_be_bytes())?;
                self.file.write_all(b"mdat")?;
            }
            Err(_) => {
                self.file.write_all(&1u32.to_be_bytes())?;
                self.file.write_all(b"mdat")?;
                self.file
                    .write_all(&(self.mdat_bytes + MDAT_HEADER as u64).to_be_bytes())?;
            }
        }
        self.file.flush()?;
        Ok(u64::from(self.audio_at) + self.mdat_bytes + index.len() as u64)
    }
}
