/*
 *  flac.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Landing a FLAC file as a VCW capture.
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

//! Landing a FLAC file as a VCW capture (§12).
//!
//! The third sibling of [`mod@crate::land`], and the shortest of the three. The
//! WAV reader has to find the audio in the container before it can copy it; a
//! FLAC reader has to decode, and `flac_codec::decode::FlacByteReader` hands
//! back exactly the bytes a `data` chunk would have held - interleaved,
//! little-endian, and **packed**, three bytes to a 24-bit sample rather than
//! four. That is VCW's own storage layout, so from the writer's side a FLAC
//! import and a WAV import are the same loop over a different reader, and they
//! share a [`Landed`] because they are the same operation.
//!
//! # Nothing is converted
//!
//! FLAC's own range is 1 to 32 bits at any rate up to 2^20, which is wider than
//! the three widths a capture stores. Sixteen, twenty-four and thirty-two bit
//! streams land byte for byte; anything else is refused by width, with the
//! width in the sentence. A 20-bit stream is the one that costs somebody
//! something, and it is still the right refusal: widening it to 24 would make
//! the import a conversion that looked like a copy, and the capture would then
//! claim a precision the record never had.
//!
//! # What the declared length is worth
//!
//! `STREAMINFO` carries a total sample count, and a stream written by an
//! encoder that did not know the length in advance carries zero there instead.
//! It is a claim either way, and it is reported beside the count that was
//! actually decoded rather than checked against it, for the same reason the WAV
//! reader prints both byte figures: an import that landed less audio than the
//! file said it had must not be indistinguishable from one that landed all of
//! it.
//!
//! # No tags
//!
//! A `VORBIS_COMMENT` is not read. An Audacity project's tags land on the
//! release because an Audacity project is somebody's edit of a record and
//! carries their naming of it; a FLAC in this corpus is a raw side with no
//! comment block worth having. If a rip with real tags turns up, this is where
//! they would be read, and it would want the `--no-tags` flag the Audacity path
//! already has.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use flac_codec::byteorder::LittleEndian;
use flac_codec::decode::FlacByteReader;
use flac_codec::metadata::Metadata;

use vcw_project::persistence::{Config, Writer};
use vcw_project::{Project as Destination, meta, side};
use vcw_types::vinyl::Side;
use vcw_types::{
    CaptureEq, CaptureInfo, CaptureMode, CaptureState, SampleFormat, SampleRate, StorageFormat,
};

use crate::error::{Error, Result};
use crate::land::SOURCE_KEY;

/// The landing report, and the `meta` key naming the container, are the WAV
/// reader's: the two land the same thing and a caller that printed one of them
/// can print the other.
pub use crate::wav::{FORMAT_KEY, Landed};

/// What a FLAC stream says it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Frames a second.
    pub rate: SampleRate,
    /// Channels, interleaved once decoded.
    pub channels: u16,
    /// The sample format, as the capture path names it.
    pub format: SampleFormat,
    /// Bytes per frame once decoded, which is the packed width times the
    /// channel count.
    pub frame_bytes: u16,
    /// Frames `STREAMINFO` claims, or `None` when it claims none. Zero in the
    /// field means the encoder did not know, which is a legal stream and not a
    /// damaged one.
    pub declared_frames: Option<u64>,
}

impl Layout {
    /// Frames of audio the stream claims, zero when it does not say.
    #[must_use]
    pub fn frames(&self) -> u64 {
        self.declared_frames.unwrap_or(0)
    }

    /// Seconds of audio the stream claims, zero when it does not say.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.rate.hz() == 0 {
            return 0.0;
        }
        self.frames() as f64 / f64::from(self.rate.hz())
    }

    /// Bytes the decoded audio will occupy, zero when the length is unknown.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.frames() * u64::from(self.frame_bytes)
    }
}

/// A refusal naming the file and the reason in one place.
fn refuse(path: &Path, reason: impl Into<String>) -> Error {
    Error::NotFlac {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

/// The operating system refused, which is not the file's fault.
fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Opens a FLAC file for decoding, with the refusal naming the file.
fn open(path: &Path) -> Result<FlacByteReader<BufReader<File>, LittleEndian>> {
    let file = File::open(path).map_err(|source| io(path, source))?;
    FlacByteReader::endian(BufReader::new(file), LittleEndian)
        .map_err(|source| refuse(path, source.to_string()))
}

/// Reads what a FLAC stream declares about itself.
///
/// # Errors
///
/// If the file cannot be read, is not a FLAC stream, or is in a width, rate or
/// channel count the capture path has no name for.
pub fn layout(path: &Path) -> Result<Layout> {
    let reader = open(path)?;
    describe(path, &reader)
}

/// The layout of a stream already open, so the landing reads the header once.
fn describe(path: &Path, reader: &FlacByteReader<BufReader<File>, LittleEndian>) -> Result<Layout> {
    // From the bit depth, as the WAV reader does, and for the same reason: the
    // capture path names a format by its sample width.
    let bits = reader.bits_per_sample();
    let format = match bits {
        16 => SampleFormat::S16,
        24 => SampleFormat::S24,
        32 => SampleFormat::S32,
        other => {
            return Err(refuse(
                path,
                format!("it is {other}-bit, and 16, 24 and 32 are what a capture stores"),
            ));
        }
    };
    let channels = u16::from(reader.channel_count());
    let rate = reader.sample_rate();
    if channels == 0 || rate == 0 {
        return Err(refuse(
            path,
            format!("it declares {channels} channels at {rate} Hz"),
        ));
    }
    Ok(Layout {
        rate: SampleRate(rate),
        channels,
        format,
        frame_bytes: channels * (bits as u16 / 8),
        declared_frames: reader.total_samples().filter(|&total| total > 0),
    })
}

/// Decodes a FLAC file and writes it as a new `.vcw`.
///
/// The source is opened read-only and never written to. `destination` must not
/// exist; see [`mod@crate::land`] for why an interrupted import is discarded
/// rather than resumed.
///
/// # Errors
///
/// Anything [`layout`] can refuse, plus [`Error::Io`] if the stream cannot be
/// decoded and [`crate::Error::Project`] if the destination cannot be written.
pub fn land(source: &Path, destination: &Path, side: Side, config: Config) -> Result<Landed> {
    tracing::info!(
        source = %source.display(),
        destination = %destination.display(),
        "importing a FLAC file"
    );
    let mut reader = open(source)?;
    let layout = describe(source, &reader)?;
    let frame_bytes = u64::from(layout.frame_bytes);

    let info = CaptureInfo {
        rate: layout.rate,
        channels: layout.channels,
        storage_format: StorageFormat::native_for(layout.format),
        // No stream was opened, exactly as for a WAV or an Audacity import.
        capture_mode: CaptureMode::Imported,
        host_api: None,
        device_id: None,
        device_name: None,
        os_verified: false,
        os_report: None,
        // A FLAC carries no signal chain, so nothing may be assumed about one.
        eq: CaptureEq::Unknown,
    };

    let project = Destination::create(destination)?;
    let mut writer = Writer::begin(project, &info, config)?;
    let chunk = (writer.block_frames() * frame_bytes) as usize;
    let mut buffer = vec![0u8; chunk];
    // Whatever the decoder gives, however much of the buffer it filled: the
    // writer accumulates and drains whole blocks itself, so a read that stops
    // mid-sample costs nothing. A FLAC frame decodes whole or it errors, so the
    // total can only be a whole number of frames.
    let mut read_bytes = 0_u64;
    loop {
        let filled = reader.read(&mut buffer).map_err(|s| io(source, s))?;
        if filled == 0 {
            break;
        }
        writer.push(&buffer[..filled])?;
        read_bytes += filled as u64;
    }
    if read_bytes < frame_bytes {
        return Err(refuse(
            source,
            format!("it decoded {read_bytes} byte(s), which is not one whole frame"),
        ));
    }
    let (outcome, mut project, _session) = writer.finish_with_project(CaptureState::Finalised)?;

    side::attach(&mut project, side, outcome.capture_id)?;
    {
        let conn = project.conn();
        meta::set(conn, SOURCE_KEY, &source.display().to_string())?;
        meta::set(conn, FORMAT_KEY, "flac")?;
    }
    project.close()?;

    let landed = Landed {
        path: destination.to_path_buf(),
        capture_id: outcome.capture_id,
        side,
        rate: layout.rate,
        channels: layout.channels,
        storage_format: info.storage_format,
        frames: outcome.frames,
        blocks: outcome.blocks,
        commits: outcome.commits,
        batch_blocks: config.batch_blocks,
        declared_bytes: layout.bytes(),
        read_bytes,
    };
    tracing::info!(
        destination = %landed.path.display(),
        rate = landed.rate.hz(),
        channels = landed.channels,
        storage = ?landed.storage_format,
        frames = landed.frames,
        blocks = landed.blocks,
        declared_bytes = landed.declared_bytes,
        read_bytes = landed.read_bytes,
        "a FLAC file landed as a capture"
    );
    Ok(landed)
}
