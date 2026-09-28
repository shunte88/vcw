/*
 *  encoder.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WAV and FLAC encoding (D5).
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

//! WAV and FLAC encoding (D5).
//!
//! Our own WAV writer - trivial to write, and it avoids `hound`'s format limits
//! at 24/192. FLAC through `flacenc`, pure Rust and Apache-2.0. MP3 and Ogg
//! Vorbis are Phase 2 and both carry LGPL relink obligations, which is why they
//! are candidates for optional cargo features rather than defaults.
//!
//! Both writers are **streaming**: [`Writer::write`] takes whatever
//! `vcw_project::pcm::Reader::fill` produced and hands nothing back, so a
//! twenty-minute side costs one block of memory and not a gigabyte of it. That
//! rules out `flacenc`'s one-call `encode_with_fixed_block_size`, which builds
//! the entire encoded stream in RAM before anything reaches the disk; the
//! per-frame `encode_fixed_size_frame` is used instead and the `STREAMINFO`
//! block is patched at the end, which is what the reference encoder does too.
//!
//! ## What a container will not take
//!
//! Refusals are loud and early - [`Writer::create`] fails before a file is
//! made - because the alternative is a plausible-looking file that is wrong:
//!
//! - **WAV is capped at 4 GiB** by RIFF's 32-bit sizes. Reachable by a long
//!   unsplit side at 192 kHz, not by a track.
//! - **FLAC is an integer codec**, so a `Float32` capture has no FLAC path
//!   until a person decides how it should be dithered.
//! - **`flacenc` 0.5.1 stops at 24 bits and 96 kHz.** Both are the library's
//!   limits and not the format's: FLAC itself allows 32 bits and rates to
//!   655350 Hz. §8 requires capture at 192 kHz, so this is a real gap in the
//!   requirement rather than a theoretical one, and it is asserted against the
//!   library in `tests::the_flac_library_really_does_stop_where_we_say_it_does`,
//!   spelled out rather than linked because a `cfg(test)` item is not there to
//!   link to in a doc build, so the day either cap is lifted that test fails
//!   and tells us.
//!
//! ## Why the fmt chunk is extensible above 16 bits
//!
//! Microsoft's guidance is to use `WAVE_FORMAT_EXTENSIBLE` above 16 bits or
//! beyond two channels. The files people actually have do not: all 46 WAVs in
//! `/data2/source_rips` (2026-09-26) are 32-bit stereo with a 16-byte `fmt `
//! chunk and a format tag of 1, which is what libsndfile writes and therefore
//! what Audacity, `sox` and most rippers produce. That looked like the better
//! precedent until a reader was asked: `flac 1.5.0` reading a 24-bit file of
//! ours in that shape says
//!
//! ```text
//! WARNING: legacy WAVE file has format type 1 but bits-per-sample=24
//! ```
//!
//! A warning from the reference encoder on a file we just wrote is not
//! something to ship, so the guidance wins and the corpus is recorded as
//! evidence that readers tolerate the other shape rather than as a reason to
//! produce it. Sixteen-bit stereo stays a plain 44-byte header, because there
//! nothing is ambiguous and everything reads it.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use flacenc::component::BitRepr;
use flacenc::error::Verify;
use flacenc::source::Fill;
use vcw_types::StorageFormat;

use crate::error::{Error, Result};

/// A deliverable container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Container {
    /// Uncompressed PCM in a RIFF wrapper.
    Wav,
    /// Lossless compression.
    Flac,
}

impl Container {
    /// The file extension, without the dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
        }
    }

    /// The name used in messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
        }
    }

    /// Reads a container from an extension, with or without its dot.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "wav" | "wave" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            _ => None,
        }
    }
}

impl std::fmt::Display for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What an encoder has to know before the first sample.
///
/// `frames` is the length of the piece being written, which both writers need
/// up front: WAV to refuse a file RIFF cannot describe before making it, and
/// FLAC to declare `total_samples` in a header that is written first and
/// corrected last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// Sample rate, in Hz.
    pub rate: u32,
    /// Channels, and therefore samples per frame.
    pub channels: u16,
    /// How the samples are laid out in the project's blocks.
    pub format: StorageFormat,
    /// Frames to be written, per channel.
    pub frames: u64,
}

impl Spec {
    /// Bits that carry signal, which is what a header declares.
    #[must_use]
    pub const fn bits(&self) -> u16 {
        match self.format {
            StorageFormat::Int16 => 16,
            StorageFormat::Int24Packed | StorageFormat::Int24Padded => 24,
            StorageFormat::Int32 | StorageFormat::Float32 => 32,
        }
    }

    /// Whether the samples are floating point.
    #[must_use]
    pub const fn is_float(&self) -> bool {
        matches!(self.format, StorageFormat::Float32)
    }

    /// Bytes one sample occupies in the written file.
    ///
    /// The same as the stored width except for [`StorageFormat::Int24Padded`],
    /// where the stored form is 24 bits of signal in four bytes and every
    /// container wants the three that carry it.
    #[must_use]
    pub const fn wire_sample_bytes(&self) -> usize {
        self.bits() as usize / 8
    }

    /// Bytes one interleaved frame occupies in the written file.
    #[must_use]
    pub const fn wire_frame_bytes(&self) -> usize {
        self.wire_sample_bytes() * self.channels as usize
    }

    /// Bytes one interleaved frame occupies in the project's blocks.
    #[must_use]
    pub const fn stored_frame_bytes(&self) -> usize {
        self.format.bytes_per_sample() * self.channels as usize
    }

    /// Whether the stored bytes have to be rewritten on the way out.
    #[must_use]
    pub const fn repacks(&self) -> bool {
        matches!(self.format, StorageFormat::Int24Padded)
    }

    /// How many audio bytes the finished file will hold.
    #[must_use]
    pub const fn wire_bytes(&self) -> u64 {
        self.frames * self.wire_frame_bytes() as u64
    }
}

/// Rewrites stored interleaved bytes into the shape the container wants.
///
/// A verbatim copy for four of the five formats, which is what makes the
/// bit-exactness claim checkable: the WAV data chunk is the block bytes.
/// `Int24Padded` drops the pad byte, losing nothing - the stored sample is a
/// little-endian value in +/-2^23, so the fourth byte is sign extension.
fn to_wire(spec: &Spec, stored: &[u8], out: &mut Vec<u8>) {
    out.clear();
    if spec.repacks() {
        out.reserve(stored.len() / 4 * 3);
        for sample in stored.as_chunks::<4>().0 {
            out.extend_from_slice(&sample[..3]);
        }
    } else {
        out.extend_from_slice(stored);
    }
}

/// An open output file.
///
/// An enum rather than a boxed trait: there are two containers, `finish`
/// consumes the writer, and neither of those wants `dyn`.
#[derive(Debug)]
#[non_exhaustive]
pub enum Writer {
    /// A RIFF/WAVE file.
    Wav(Wav),
    /// A FLAC file.
    ///
    /// Boxed because a [`Flac`] carries the encoder's block buffers and is an
    /// order of magnitude bigger than a [`Wav`], and every `Writer` moved by
    /// value would otherwise carry the larger of the two. One allocation per
    /// exported file is not a cost worth measuring.
    Flac(Box<Flac>),
}

impl Writer {
    /// Creates a file and writes its header.
    ///
    /// # Errors
    ///
    /// If the container cannot carry the format, if the length will not fit, or
    /// if the file cannot be made.
    pub fn create(path: &Path, container: Container, spec: Spec) -> Result<Self> {
        match container {
            Container::Wav => Wav::create(path, spec).map(Self::Wav),
            Container::Flac => Flac::create(path, spec).map(|flac| Self::Flac(Box::new(flac))),
        }
    }

    /// Writes interleaved frames, in the project's stored format.
    ///
    /// # Errors
    ///
    /// If the bytes are not a whole number of frames, if the file grows past
    /// what the container can describe, or if the write fails.
    pub fn write(&mut self, stored: &[u8]) -> Result<()> {
        match self {
            Self::Wav(wav) => wav.write(stored),
            Self::Flac(flac) => flac.write(stored),
        }
    }

    /// Completes the file and returns its size in bytes.
    ///
    /// # Errors
    ///
    /// If the header cannot be corrected, or a final flush fails.
    pub fn finish(self) -> Result<u64> {
        match self {
            Self::Wav(wav) => wav.finish(),
            Self::Flac(flac) => flac.finish(),
        }
    }
}

/// A RIFF/WAVE writer.
#[derive(Debug)]
pub struct Wav {
    file: File,
    spec: Spec,
    /// Bytes before the first sample, which is also where the data size lives
    /// minus four.
    header_bytes: u64,
    data_bytes: u64,
    scratch: Vec<u8>,
}

/// `WAVE_FORMAT_PCM`.
const TAG_PCM: u16 = 1;
/// `WAVE_FORMAT_IEEE_FLOAT`.
const TAG_FLOAT: u16 = 3;
/// `WAVE_FORMAT_EXTENSIBLE`.
const TAG_EXTENSIBLE: u16 = 0xFFFE;

/// The `KSDATAFORMAT_SUBTYPE_PCM` GUID, little-endian as it goes on disk.
const GUID_PCM: [u8; 16] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

/// The `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT` GUID.
const GUID_FLOAT: [u8; 16] = [
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

impl Wav {
    /// Creates the file and writes a header with placeholder sizes.
    fn create(path: &Path, spec: Spec) -> Result<Self> {
        let header = Self::header(&spec);
        let ceiling = u32::MAX as u64 - (header.len() as u64 - 8);
        if spec.wire_bytes() > ceiling {
            return Err(Error::TooLargeForWav {
                bytes: spec.wire_bytes(),
                ceiling,
            });
        }
        let mut file = File::create(path)?;
        file.write_all(&header)?;
        Ok(Self {
            file,
            spec,
            header_bytes: header.len() as u64,
            data_bytes: 0,
            scratch: Vec::new(),
        })
    }

    /// The header, with both size fields left as zero for `finish` to correct.
    ///
    /// Written with the sizes wrong rather than computed from `spec.frames`,
    /// because a short read leaves a file whose header describes audio that is
    /// not there, and that is indistinguishable from corruption to the person
    /// who opens it.
    fn header(spec: &Spec) -> Vec<u8> {
        // See the module docs: above 16 bits or beyond stereo, the plain
        // header is ambiguous enough that the reference FLAC encoder complains
        // about it.
        let extensible = spec.channels > 2 || spec.bits() > 16;
        let fmt_bytes: u32 = if extensible { 40 } else { 16 };
        let block_align = spec.wire_frame_bytes() as u16;
        let byte_rate = spec.rate * u32::from(block_align);
        let tag = match (extensible, spec.is_float()) {
            (true, _) => TAG_EXTENSIBLE,
            (false, true) => TAG_FLOAT,
            (false, false) => TAG_PCM,
        };

        let mut out = Vec::with_capacity(68);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&0u32.to_le_bytes()); // patched by `finish`
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&fmt_bytes.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&spec.channels.to_le_bytes());
        out.extend_from_slice(&spec.rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&spec.bits().to_le_bytes());
        if extensible {
            out.extend_from_slice(&22u16.to_le_bytes()); // cbSize
            out.extend_from_slice(&spec.bits().to_le_bytes()); // valid bits
            // No channel mask. Zero means "not assigned to speakers", which is
            // the truth: VCW knows it captured four channels and nothing about
            // where they were meant to point.
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(if spec.is_float() {
                &GUID_FLOAT
            } else {
                &GUID_PCM
            });
        }
        out.extend_from_slice(b"data");
        out.extend_from_slice(&0u32.to_le_bytes()); // patched by `finish`
        out
    }

    /// Appends interleaved frames.
    fn write(&mut self, stored: &[u8]) -> Result<()> {
        let frame = self.spec.stored_frame_bytes();
        if frame == 0 || !stored.len().is_multiple_of(frame) {
            return Err(Error::Partial {
                bytes: stored.len(),
                frame_bytes: frame,
            });
        }
        to_wire(&self.spec, stored, &mut self.scratch);
        let ceiling = u32::MAX as u64 - (self.header_bytes - 8);
        if self.data_bytes + self.scratch.len() as u64 > ceiling {
            return Err(Error::TooLargeForWav {
                bytes: self.data_bytes + self.scratch.len() as u64,
                ceiling,
            });
        }
        self.file.write_all(&self.scratch)?;
        self.data_bytes += self.scratch.len() as u64;
        Ok(())
    }

    /// Pads the data chunk, corrects both sizes and closes the file.
    fn finish(mut self) -> Result<u64> {
        // RIFF chunks are even-length. Three-byte samples in mono make an odd
        // data chunk, and a reader that trusts the spec reads the next chunk
        // header one byte late.
        let pad = u64::from(!self.data_bytes.is_multiple_of(2));
        if pad == 1 {
            self.file.write_all(&[0u8])?;
        }
        let total = self.header_bytes + self.data_bytes + pad;

        #[allow(clippy::cast_possible_truncation)] // checked in `write`
        let riff = (total - 8) as u32;
        #[allow(clippy::cast_possible_truncation)]
        let data = self.data_bytes as u32;

        self.file.seek(SeekFrom::Start(4))?;
        self.file.write_all(&riff.to_le_bytes())?;
        self.file.seek(SeekFrom::Start(self.header_bytes - 4))?;
        self.file.write_all(&data.to_le_bytes())?;
        self.file.flush()?;
        Ok(total)
    }
}

/// Bytes of padding written after `STREAMINFO`.
///
/// The reference encoder's default, and it exists for the tagger: a
/// `VORBIS_COMMENT` block that fits in the padding is written in place, and one
/// that does not means rewriting every byte of audio to make room.
const FLAC_PADDING: u32 = 8192;

/// Where the `STREAMINFO` metadata block starts: straight after `fLaC`.
///
/// Its four-byte block header lives here and the 34 bytes of fields at
/// `STREAMINFO_AT + 4`, which is where a decoder reads the sample rate from.
/// `finish` seeks back to *this* offset, not to the fields, because
/// [`Flac::write_stream_info`] writes the header and the fields together.
const STREAMINFO_AT: u64 = 4;

/// `STREAMINFO` is 34 bytes, always.
const STREAMINFO_BYTES: usize = 34;

/// A FLAC writer.
pub struct Flac {
    file: File,
    spec: Spec,
    config: flacenc::error::Verified<flacenc::config::Encoder>,
    info: flacenc::component::StreamInfo,
    buffer: flacenc::source::FrameBuf,
    context: flacenc::source::Context,
    sink: flacenc::bitsink::ByteSink,
    /// Wire bytes read but not yet a whole block.
    pending: Vec<u8>,
    scratch: Vec<u8>,
    block_frames: usize,
    frame_number: usize,
    bytes: u64,
}

impl std::fmt::Debug for Flac {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Verified<Encoder>` and `ByteSink` are not `Debug`, and a dump of an
        // encoder's internals would not help anyone anyway.
        f.debug_struct("Flac")
            .field("spec", &self.spec)
            .field("block_frames", &self.block_frames)
            .field("frame_number", &self.frame_number)
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl Flac {
    /// Checks the format against what FLAC and `flacenc` will take, then writes
    /// the metadata blocks.
    fn create(path: &Path, spec: Spec) -> Result<Self> {
        Self::vet(&spec)?;

        let config = flacenc::config::Encoder::default()
            .into_verified()
            .map_err(|why| Error::Flac {
                why: format!("the encoder configuration was rejected: {why:?}"),
            })?;
        let block_frames = config.block_size;
        let channels = spec.channels as usize;
        let bits = spec.bits() as usize;

        let mut info = flacenc::component::StreamInfo::new(spec.rate as usize, channels, bits)
            .map_err(|why| Error::Flac {
                why: format!("{} Hz, {channels} channels, {bits}-bit: {why:?}", spec.rate),
            })?;
        // Declared now and corrected in `finish`: a decoder that reads the
        // header before the stream ends can still show a duration.
        info.set_total_samples(usize::try_from(spec.frames).unwrap_or(usize::MAX));

        let buffer =
            flacenc::source::FrameBuf::with_size(channels, block_frames).map_err(|why| {
                Error::Flac {
                    why: format!("a {block_frames}-frame buffer was rejected: {why:?}"),
                }
            })?;

        let mut sink = flacenc::bitsink::ByteSink::new();
        let mut file = File::create(path)?;
        file.write_all(b"fLaC")?;
        Self::write_stream_info(&mut file, &info, &mut sink)?;
        // Padding block: type 1, last block, `FLAC_PADDING` bytes of zero.
        let length = FLAC_PADDING.to_be_bytes();
        file.write_all(&[0x81, length[1], length[2], length[3]])?;
        file.write_all(&vec![0u8; FLAC_PADDING as usize])?;

        Ok(Self {
            file,
            spec,
            config,
            info,
            buffer,
            context: flacenc::source::Context::new(bits, channels),
            sink,
            pending: Vec::with_capacity(block_frames * spec.wire_frame_bytes()),
            scratch: Vec::new(),
            block_frames,
            frame_number: 0,
            bytes: 8 + STREAMINFO_BYTES as u64 + 4 + u64::from(FLAC_PADDING),
        })
    }

    /// Everything that has to be true before a FLAC file is created.
    ///
    /// Separated from `create` so the refusals can be tested without a
    /// filesystem, and so each one can say which limit it hit.
    fn vet(spec: &Spec) -> Result<()> {
        if spec.is_float() {
            return Err(Error::Unencodable {
                format: spec.format,
                container: Container::Flac.name(),
                why: "FLAC is an integer codec, and choosing how to dither 32-bit \
                      float down to integers is a decision about headroom that belongs \
                      to a person. Export this one as WAV.",
            });
        }
        if spec.bits() > 24 {
            return Err(Error::Unencodable {
                format: spec.format,
                container: Container::Flac.name(),
                why: "the FLAC format allows 32-bit samples but flacenc 0.5.1 stops at \
                      24, and narrowing 32 bits to 24 loses signal. Export this one as WAV.",
            });
        }
        if spec.rate > 96_000 {
            return Err(Error::Unencodable {
                format: spec.format,
                container: Container::Flac.name(),
                why: "flacenc 0.5.1 refuses rates above 96 kHz, though the FLAC format \
                      allows up to 655350 Hz. Export this one as WAV.",
            });
        }
        if spec.channels == 0 || spec.channels > 8 {
            return Err(Error::Unencodable {
                format: spec.format,
                container: Container::Flac.name(),
                why: "FLAC carries between one and eight channels.",
            });
        }
        Ok(())
    }

    /// Writes the `STREAMINFO` metadata block, header and all, at the cursor.
    fn write_stream_info(
        file: &mut File,
        info: &flacenc::component::StreamInfo,
        sink: &mut flacenc::bitsink::ByteSink,
    ) -> Result<()> {
        sink.clear();
        info.write(sink).map_err(|why| Error::Flac {
            why: format!("the stream header would not serialise: {why:?}"),
        })?;
        let bytes = sink.as_slice();
        if bytes.len() != STREAMINFO_BYTES {
            return Err(Error::Flac {
                why: format!(
                    "the stream header came out {} bytes and STREAMINFO is {STREAMINFO_BYTES}",
                    bytes.len()
                ),
            });
        }
        // Block header: not the last block, type 0, length 34.
        file.write_all(&[0x00, 0x00, 0x00, STREAMINFO_BYTES as u8])?;
        file.write_all(bytes)?;
        sink.clear();
        Ok(())
    }

    /// Appends interleaved frames, encoding whole blocks as they complete.
    fn write(&mut self, stored: &[u8]) -> Result<()> {
        let frame = self.spec.stored_frame_bytes();
        if frame == 0 || !stored.len().is_multiple_of(frame) {
            return Err(Error::Partial {
                bytes: stored.len(),
                frame_bytes: frame,
            });
        }
        to_wire(&self.spec, stored, &mut self.scratch);
        self.pending.extend_from_slice(&self.scratch);

        let block_bytes = self.block_frames * self.spec.wire_frame_bytes();
        let mut consumed = 0;
        while consumed + block_bytes <= self.pending.len() {
            self.encode_block(consumed..consumed + block_bytes)?;
            consumed += block_bytes;
        }
        if consumed > 0 {
            self.pending.drain(..consumed);
        }
        Ok(())
    }

    /// Encodes one block out of `pending` and appends it to the file.
    fn encode_block(&mut self, range: std::ops::Range<usize>) -> Result<()> {
        let width = self.spec.wire_sample_bytes();
        (&mut self.buffer, &mut self.context)
            .fill_le_bytes(&self.pending[range], width)
            .map_err(|why| Error::Flac {
                why: format!("block {} would not load: {why:?}", self.frame_number),
            })?;

        let frame = flacenc::encode_fixed_size_frame(
            &self.config,
            &self.buffer,
            self.frame_number,
            &self.info,
        )
        .map_err(|why| Error::Flac {
            why: format!("block {} would not encode: {why:?}", self.frame_number),
        })?;
        self.info.update_frame_info(&frame);

        self.sink.clear();
        frame.write(&mut self.sink).map_err(|why| Error::Flac {
            why: format!("block {} would not serialise: {why:?}", self.frame_number),
        })?;
        let bytes = self.sink.as_slice();
        self.file.write_all(bytes)?;
        self.bytes += bytes.len() as u64;
        self.frame_number += 1;
        Ok(())
    }

    /// Encodes the last short block, corrects `STREAMINFO` and closes the file.
    fn finish(mut self) -> Result<u64> {
        if !self.pending.is_empty() {
            let end = self.pending.len();
            self.encode_block(0..end)?;
            self.pending.clear();
        }

        // The digest and the sample count are whatever actually went through,
        // not whatever `spec` promised. `flac -t` checks the digest, which is
        // the only reason it is worth carrying.
        self.info.set_md5_digest(&self.context.md5_digest());
        self.info.set_total_samples(self.context.total_samples());

        // The *nominal* block size, both ends, which is not what
        // `update_frame_info` accumulated: it had honestly recorded the short
        // final frame as the minimum. A `STREAMINFO` with min != max tells
        // libFLAC the stream is variably blocked, and a variably blocked stream
        // carries sample numbers in its frame headers where ours carry frame
        // numbers - so `flac -t` warned once per frame that the numbering did
        // not increase and that the file might not be seekable, on a file whose
        // audio frames are byte-identical to the reference encoder's.
        //
        // Measured 2026-09-26: `flac 1.5.0` encoding the same 48000 frames
        // declares min = max = 4096 and so does a 300-frame file, where the
        // only frame there is runs short. The nominal size is the answer in
        // both cases.
        self.info
            .set_block_sizes(self.block_frames, self.block_frames)
            .map_err(|why| Error::Flac {
                why: format!("a {}-frame block was rejected: {why:?}", self.block_frames),
            })?;

        let mut sink = std::mem::take(&mut self.sink);
        self.file.seek(SeekFrom::Start(STREAMINFO_AT))?;
        Self::write_stream_info(&mut self.file, &self.info, &mut sink)?;
        self.file.flush()?;
        Ok(self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spec with the numbers a vinyl rip actually uses.
    fn spec(format: StorageFormat, rate: u32, frames: u64) -> Spec {
        Spec {
            rate,
            channels: 2,
            format,
            frames,
        }
    }

    /// Interleaved stored bytes for `frames` frames, each byte distinct enough
    /// that a repack or an off-by-one shows up as a mismatch and not as silence.
    fn ramp(spec: &Spec, frames: usize) -> Vec<u8> {
        (0..frames * spec.stored_frame_bytes())
            .map(|i| (i % 251) as u8)
            .collect()
    }

    #[test]
    fn a_stereo_header_is_the_44_bytes_everything_reads() {
        let header = Wav::header(&spec(StorageFormat::Int16, 44_100, 0));
        assert_eq!(header.len(), 44, "the canonical WAV header length");
        assert_eq!(&header[0..4], b"RIFF");
        assert_eq!(&header[8..12], b"WAVE");
        assert_eq!(&header[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(header[16..20].try_into().unwrap()), 16);
        assert_eq!(
            u16::from_le_bytes(header[20..22].try_into().unwrap()),
            TAG_PCM
        );
        assert_eq!(u16::from_le_bytes(header[22..24].try_into().unwrap()), 2);
        assert_eq!(
            u32::from_le_bytes(header[24..28].try_into().unwrap()),
            44_100
        );
        assert_eq!(
            u32::from_le_bytes(header[28..32].try_into().unwrap()),
            44_100 * 4,
            "byte rate"
        );
        assert_eq!(u16::from_le_bytes(header[32..34].try_into().unwrap()), 4);
        assert_eq!(u16::from_le_bytes(header[34..36].try_into().unwrap()), 16);
        assert_eq!(&header[36..40], b"data");
    }

    #[test]
    fn above_sixteen_bits_the_header_goes_extensible() {
        // The reference FLAC encoder warns about a 24-bit file with a plain
        // header, so every width above 16 gets the 68-byte one.
        for format in [
            StorageFormat::Int24Packed,
            StorageFormat::Int24Padded,
            StorageFormat::Int32,
            StorageFormat::Float32,
        ] {
            let header = Wav::header(&spec(format, 96_000, 0));
            assert_eq!(header.len(), 68, "{format:?}");
            assert_eq!(
                u16::from_le_bytes(header[20..22].try_into().unwrap()),
                TAG_EXTENSIBLE,
                "{format:?}"
            );
            assert_eq!(
                u32::from_le_bytes(header[40..44].try_into().unwrap()),
                0,
                "{format:?}: two channels, and no claim about where they point"
            );
        }

        let padded = Wav::header(&spec(StorageFormat::Int24Padded, 96_000, 0));
        assert_eq!(
            u16::from_le_bytes(padded[34..36].try_into().unwrap()),
            24,
            "a padded sample is declared as the 24 bits it carries"
        );
        assert_eq!(
            u16::from_le_bytes(padded[32..34].try_into().unwrap()),
            6,
            "and three bytes per sample on the wire, not four"
        );
        assert_eq!(&padded[44..60], &GUID_PCM);
    }

    #[test]
    fn sixteen_bit_stereo_keeps_the_header_everything_reads() {
        // The one case nothing is ambiguous about, and the one shape every
        // reader written since 1991 handles.
        let header = Wav::header(&spec(StorageFormat::Int16, 44_100, 0));
        assert_eq!(header.len(), 44);
        assert_eq!(
            u16::from_le_bytes(header[20..22].try_into().unwrap()),
            TAG_PCM
        );
    }

    #[test]
    fn a_float_capture_says_so_in_its_subformat() {
        // 32 bits, so the header is extensible and the "this is float" claim
        // lives in the GUID rather than in the format tag. `TAG_FLOAT` is still
        // right for a hypothetical narrow float and is kept for that reason.
        let header = Wav::header(&spec(StorageFormat::Float32, 48_000, 0));
        assert_eq!(
            u16::from_le_bytes(header[20..22].try_into().unwrap()),
            TAG_EXTENSIBLE
        );
        assert_eq!(u16::from_le_bytes(header[34..36].try_into().unwrap()), 32);
        assert_eq!(&header[44..60], &GUID_FLOAT);
    }

    #[test]
    fn more_than_two_channels_needs_the_extensible_header() {
        let mut four = spec(StorageFormat::Int24Packed, 96_000, 0);
        four.channels = 4;
        let header = Wav::header(&four);
        assert_eq!(header.len(), 68);
        assert_eq!(u32::from_le_bytes(header[16..20].try_into().unwrap()), 40);
        assert_eq!(
            u16::from_le_bytes(header[20..22].try_into().unwrap()),
            TAG_EXTENSIBLE
        );
        assert_eq!(
            u16::from_le_bytes(header[36..38].try_into().unwrap()),
            22,
            "cbSize"
        );
        assert_eq!(
            u16::from_le_bytes(header[38..40].try_into().unwrap()),
            24,
            "valid bits"
        );
        assert_eq!(&header[44..60], &GUID_PCM);
        assert_eq!(&header[60..64], b"data");
    }

    #[test]
    fn the_data_chunk_is_the_block_bytes() {
        // The bit-exactness claim at the smallest scale it can be stated: for
        // every format the device hands us verbatim, the bytes in the file are
        // the bytes in the blocks.
        for format in [
            StorageFormat::Int16,
            StorageFormat::Int24Packed,
            StorageFormat::Int32,
            StorageFormat::Float32,
        ] {
            let spec = spec(format, 48_000, 100);
            let stored = ramp(&spec, 100);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("t.wav");
            let mut writer = Writer::create(&path, Container::Wav, spec).unwrap();
            writer.write(&stored).unwrap();
            let total = writer.finish().unwrap();

            let file = std::fs::read(&path).unwrap();
            let head = Wav::header(&spec).len();
            assert_eq!(file.len() as u64, total, "{format:?}");
            assert_eq!(&file[head..], &stored[..], "{format:?}");
            assert_eq!(
                u32::from_le_bytes(file[head - 4..head].try_into().unwrap()) as usize,
                stored.len(),
                "{format:?} data size"
            );
            assert_eq!(
                u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize,
                file.len() - 8,
                "{format:?} riff size"
            );
        }
    }

    #[test]
    fn a_padded_sample_loses_its_pad_and_nothing_else() {
        let spec = spec(StorageFormat::Int24Padded, 96_000, 2);
        // Two frames, two channels: four samples, low three bytes then a sign
        // byte that has to disappear.
        let stored = vec![1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0xFF, 10, 11, 12, 0xFF];
        let mut wire = Vec::new();
        to_wire(&spec, &stored, &mut wire);
        assert_eq!(wire, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn an_odd_data_chunk_is_padded_to_even() {
        // Mono 24-bit: three bytes a frame, so an odd frame count makes an odd
        // chunk, and RIFF chunks are even.
        let mut spec = spec(StorageFormat::Int24Packed, 48_000, 5);
        spec.channels = 1;
        let stored = ramp(&spec, 5);
        assert_eq!(stored.len(), 15);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mono.wav");
        let mut writer = Writer::create(&path, Container::Wav, spec).unwrap();
        writer.write(&stored).unwrap();
        writer.finish().unwrap();

        let file = std::fs::read(&path).unwrap();
        let head = Wav::header(&spec).len();
        assert_eq!(file.len(), head + 16, "one pad byte");
        assert_eq!(
            u32::from_le_bytes(file[head - 4..head].try_into().unwrap()),
            15,
            "the declared size excludes the pad"
        );
        assert_eq!(*file.last().unwrap(), 0);
    }

    #[test]
    fn a_wav_too_big_for_riff_is_refused_before_the_file_is_made() {
        // A 32-bit stereo capture needs eight bytes a frame, so 2^29 frames is
        // over the ceiling.
        let spec = spec(StorageFormat::Int32, 192_000, 1 << 29);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.wav");
        let err = Writer::create(&path, Container::Wav, spec).unwrap_err();
        assert!(matches!(err, Error::TooLargeForWav { .. }), "got {err:?}");
        assert!(!path.exists(), "nothing was created");
        assert!(
            err.to_string().contains("FLAC"),
            "the message names the way out: {err}"
        );
    }

    #[test]
    fn a_partial_frame_is_refused() {
        let spec = spec(StorageFormat::Int16, 48_000, 10);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let mut writer = Writer::create(&path, Container::Wav, spec).unwrap();
        let err = writer.write(&[0, 1, 2]).unwrap_err();
        assert!(matches!(err, Error::Partial { .. }), "got {err:?}");
    }

    #[test]
    fn flac_refuses_what_it_cannot_carry_and_says_why() {
        let float = Flac::vet(&spec(StorageFormat::Float32, 48_000, 10)).unwrap_err();
        assert!(float.to_string().contains("integer codec"), "{float}");

        let wide = Flac::vet(&spec(StorageFormat::Int32, 48_000, 10)).unwrap_err();
        assert!(wide.to_string().contains("flacenc 0.5.1"), "{wide}");

        let fast = Flac::vet(&spec(StorageFormat::Int24Packed, 192_000, 10)).unwrap_err();
        assert!(fast.to_string().contains("96 kHz"), "{fast}");

        let mut many = spec(StorageFormat::Int16, 48_000, 10);
        many.channels = 9;
        assert!(Flac::vet(&many).is_err());

        // And the case that has to work: the archival default.
        Flac::vet(&spec(StorageFormat::Int24Packed, 96_000, 10)).unwrap();
        Flac::vet(&spec(StorageFormat::Int24Padded, 96_000, 10)).unwrap();
        Flac::vet(&spec(StorageFormat::Int16, 44_100, 10)).unwrap();
    }

    #[test]
    fn the_flac_library_really_does_stop_where_we_say_it_does() {
        // `vet` refuses 32 bits and 192 kHz on the library's behalf. This is the
        // evidence that the refusal is the library's limit and not ours, and the
        // test that will fail - loudly, and in the right place - on the day
        // flacenc lifts either one.
        use flacenc::component::StreamInfo;
        assert!(
            StreamInfo::new(192_000, 2, 24).is_err(),
            "flacenc now takes 192 kHz: drop the rate check in Flac::vet"
        );
        assert!(
            StreamInfo::new(96_000, 2, 32).is_err(),
            "flacenc now takes 32-bit: drop the width check in Flac::vet"
        );
        StreamInfo::new(96_000, 2, 24).expect("24/96 is the archival default");
        StreamInfo::new(44_100, 2, 16).expect("16/44.1 is the CD case");
    }

    #[test]
    fn a_flac_file_starts_with_the_blocks_a_decoder_expects() {
        let spec = spec(StorageFormat::Int24Packed, 96_000, 300);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.flac");
        let mut writer = Writer::create(&path, Container::Flac, spec).unwrap();
        writer.write(&ramp(&spec, 300)).unwrap();
        let total = writer.finish().unwrap();

        let file = std::fs::read(&path).unwrap();
        assert_eq!(file.len() as u64, total);
        assert_eq!(&file[0..4], b"fLaC");
        assert_eq!(file[4], 0x00, "STREAMINFO, and not the last block");
        assert_eq!(
            u32::from_be_bytes([0, file[5], file[6], file[7]]),
            STREAMINFO_BYTES as u32
        );
        let padding_at = 8 + STREAMINFO_BYTES;
        assert_eq!(file[padding_at], 0x81, "PADDING, and the last block");
        assert_eq!(
            u32::from_be_bytes([
                0,
                file[padding_at + 1],
                file[padding_at + 2],
                file[padding_at + 3]
            ]),
            FLAC_PADDING
        );

        // total_samples, the low 36 bits of the packed field at offset 8+18.
        let packed = u64::from_be_bytes(file[8 + 10..8 + 18].try_into().unwrap());
        assert_eq!(
            packed & 0xF_FFFF_FFFF,
            300,
            "total samples, corrected on finish"
        );
        let md5 = &file[8 + 18..8 + 34];
        assert_ne!(md5, [0u8; 16], "the digest was filled in");
    }

    #[test]
    fn a_flac_file_survives_more_than_one_block() {
        // The default block is 4096 frames, so this is three whole blocks and a
        // short one - the case where `pending`, the frame numbering and the
        // short final block all have to agree.
        let spec = spec(StorageFormat::Int16, 48_000, 4096 * 3 + 17);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.flac");
        let mut writer = Writer::create(&path, Container::Flac, spec).unwrap();
        // Fed in awkward pieces on purpose: a reader's fill size has nothing to
        // do with the encoder's block size.
        let stored = ramp(&spec, 4096 * 3 + 17);
        for piece in stored.chunks(1000 * spec.stored_frame_bytes()) {
            writer.write(piece).unwrap();
        }
        writer.finish().unwrap();

        let file = std::fs::read(&path).unwrap();
        let packed = u64::from_be_bytes(file[18..26].try_into().unwrap());
        assert_eq!(packed & 0xF_FFFF_FFFF, 4096 * 3 + 17);
    }

    #[test]
    fn a_container_knows_its_extension_both_ways() {
        assert_eq!(Container::Wav.extension(), "wav");
        assert_eq!(Container::from_extension(".FLAC"), Some(Container::Flac));
        assert_eq!(Container::from_extension("wave"), Some(Container::Wav));
        assert_eq!(Container::from_extension("mp3"), None, "Phase 2");
    }
}
