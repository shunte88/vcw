/*
 *  wav.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Landing a WAV file as a VCW capture, and finding the audio in the container.
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

//! Landing a WAV file as a VCW capture (§12), and finding the audio in the
//! container to do it (§41).
//!
//! The sibling of [`mod@crate::land`], and much the shorter of the two. An Audacity
//! project is a document plus a block store and needs a timeline assembled out
//! of it; a WAV is one run of interleaved frames, so the landing is a read loop.
//! What the two share is the thing that matters: both write through
//! [`vcw_project::persistence::Writer`], so what lands is a capture like any
//! other and nothing downstream has an import-shaped branch in it.
//!
//! # Why not just skip 44 bytes
//!
//! Because it is right often enough to be dangerous. A canonical WAV is
//! `RIFF....WAVEfmt ` + 16 bytes + `data`, which does put the audio at 44 - but
//! anything above 16 bits gets a `WAVE_FORMAT_EXTENSIBLE` `fmt ` of 40 bytes,
//! and a rip that has been through a tagger carries a `LIST` or `id3 ` chunk
//! that may come *before* `data`. Guessing 44 on one of those offsets every
//! sample in the run by however far the guess was wrong, and the result is a
//! capture that verifies byte for byte against a source that was reading the
//! wrong bytes. So the chunks are walked.
//!
//! # Nothing is converted
//!
//! Sixteen, twenty-four and thirty-two bit integer PCM, at whatever rate and
//! channel count the container declares, and nothing else: no resampling, no
//! width change, no float, no ADPCM. VCW stores 24-bit packed and a WAV writes
//! it packed, so every supported width is a byte-for-byte copy from the `data`
//! chunk into the blocks (D4). A reader that quietly converted would make the
//! import a lossy operation that looked lossless, which is the one thing an
//! import must not be. A file this refuses is a file to convert *on purpose*,
//! with a tool that says what it did.
//!
//! This module began as the WAV reader behind `vcw soak --from-file`, where
//! §41's file-backed capture needed the same three facts - where the audio is,
//! how much of it there is, and what format it is in. Its own documentation
//! said it would move into a product crate when VCW grew a real import verb.
//! This is that move, and the soak reads it from here now.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use vcw_project::persistence::{Config, Writer};
use vcw_project::{Project as Destination, meta, side};
use vcw_types::vinyl::Side;
use vcw_types::{
    CaptureEq, CaptureInfo, CaptureMode, CaptureState, SampleFormat, SampleRate, StorageFormat,
};

use crate::error::{Error, Result};
use crate::land::SOURCE_KEY;

/// Where the audio is in a WAV file, and what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// First byte of the `data` chunk's payload.
    pub offset: u64,
    /// Bytes of audio, as the container declares. A claim and not a
    /// measurement: every reader of this clamps it to the file's real size,
    /// because the length is written before the audio and a recorder that died
    /// mid-take never went back to correct it.
    pub bytes: u64,
    /// Frames a second.
    pub rate: SampleRate,
    /// Channels, interleaved.
    pub channels: u16,
    /// The sample format, as the capture path names it.
    pub format: SampleFormat,
    /// Bytes per frame, from the container's own block align.
    pub frame_bytes: u16,
}

impl Layout {
    /// Frames of audio in the file.
    pub fn frames(&self) -> u64 {
        if self.frame_bytes == 0 {
            return 0;
        }
        self.bytes / u64::from(self.frame_bytes)
    }

    /// Seconds of audio in the file.
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate.hz())
    }
}

/// A refusal naming the file and the reason in one place.
fn refuse(path: &Path, reason: impl Into<String>) -> Error {
    Error::NotWave {
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

/// Reads the layout of a WAV file.
///
/// # Errors
///
/// If the file cannot be read, is not a RIFF/WAVE file, has no `fmt ` or `data`
/// chunk, or is in a format the capture path has no name for.
pub fn layout(path: &Path) -> Result<Layout> {
    let mut file = File::open(path).map_err(|source| io(path, source))?;

    let mut riff = [0u8; 12];
    file.read_exact(&mut riff)
        .map_err(|_| refuse(path, "it is too short to be a WAV file"))?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return Err(refuse(
            path,
            format!("it is not a RIFF/WAVE file: it starts {:02X?}", &riff[0..4]),
        ));
    }

    let mut fmt: Option<Format> = None;
    let mut data: Option<(u64, u64)> = None;
    // Every chunk until both are found, in the order the file puts them. A
    // `data` chunk before `fmt ` is legal and rare; a reader that assumed the
    // usual order would fail on a file the spec allows.
    loop {
        let mut header = [0u8; 8];
        if file.read_exact(&mut header).is_err() {
            break;
        }
        let id: [u8; 4] = header[0..4].try_into().expect("four bytes");
        let size = u64::from(u32::from_le_bytes(
            header[4..8].try_into().expect("four bytes"),
        ));
        let payload = file.stream_position().map_err(|s| io(path, s))?;

        match &id {
            b"fmt " => {
                let mut bytes = vec![0u8; size.min(64) as usize];
                file.read_exact(&mut bytes)
                    .map_err(|_| refuse(path, "its fmt chunk is truncated"))?;
                fmt = Some(parse_fmt(path, &bytes)?);
            }
            // `data` before `fmt ` is legal, so finding it is not the end of
            // the walk: note where the audio is and carry on looking for the
            // format. Noted rather than returned even when the format is
            // already known, so there is one exit and not two.
            b"data" => data = Some((payload, size)),
            _ => {}
        }
        // Chunks are word-aligned and the pad byte is not counted in the size,
        // so a reader that added only the size would land one byte short on any
        // file with an odd-length chunk and read the next header as garbage.
        file.seek(SeekFrom::Start(payload + padded(size)))
            .map_err(|s| io(path, s))?;
    }

    match (fmt, data) {
        (Some(fmt), Some((offset, bytes))) => Ok(Layout {
            offset,
            bytes,
            rate: fmt.rate,
            channels: fmt.channels,
            format: fmt.format,
            frame_bytes: fmt.frame_bytes,
        }),
        (_, None) => Err(refuse(path, "it has no data chunk")),
        (None, _) => Err(refuse(path, "it has no fmt chunk")),
    }
}

/// The size a chunk occupies, pad byte included.
const fn padded(size: u64) -> u64 {
    size + (size & 1)
}

/// What a `fmt ` chunk says.
struct Format {
    rate: SampleRate,
    channels: u16,
    format: SampleFormat,
    frame_bytes: u16,
}

/// Reads a `fmt ` chunk.
fn parse_fmt(path: &Path, bytes: &[u8]) -> Result<Format> {
    if bytes.len() < 16 {
        return Err(refuse(
            path,
            format!("its fmt chunk is {} bytes; 16 is the minimum", bytes.len()),
        ));
    }
    let u16_at = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().expect("two bytes"));
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));

    let tag = u16_at(0);
    let channels = u16_at(2);
    let rate = u32_at(4);
    let frame_bytes = u16_at(12);
    let bits = u16_at(14);

    // 1 is PCM and 0xFFFE is EXTENSIBLE, which is what anything above 16 bits
    // has to use. EXTENSIBLE's real sub-format is a GUID further into the
    // chunk; it is not read, because the only sub-format a vinyl rip uses is
    // PCM and a rip that used another would fail the format match below anyway.
    if tag != 1 && tag != 0xFFFE {
        return Err(refuse(
            path,
            format!("it declares WAVE format tag {tag:#06X}, and only integer PCM is stored"),
        ));
    }
    if channels == 0 || rate == 0 {
        return Err(refuse(
            path,
            format!("it declares {channels} channels at {rate} Hz"),
        ));
    }
    // From the bit depth, not from the block align: a container may pad a frame
    // wider than its samples need, and the capture path names a format by its
    // sample width.
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
    // The one consistency check worth making. A block align that disagrees with
    // the bit depth and the channel count means the file is not laid out the
    // way the rest of this assumes, and the failure would otherwise appear as a
    // byte mismatch thousands of frames in.
    let expected = channels * (bits / 8);
    if frame_bytes != expected {
        return Err(refuse(
            path,
            format!(
                "it declares {frame_bytes} bytes a frame but {channels} channels of \
                 {bits}-bit needs {expected}"
            ),
        ));
    }
    Ok(Format {
        rate: SampleRate(rate),
        channels,
        format,
        frame_bytes,
    })
}

/// `meta` key holding which container the audio was imported from.
pub const FORMAT_KEY: &str = "import.format";

/// What landing a WAV produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landed {
    /// The project written.
    pub path: PathBuf,
    /// The capture the audio landed as.
    pub capture_id: i64,
    /// Which side it was attached to. Nothing in a WAV says, so the caller did.
    pub side: Side,
    /// The rate, from the `fmt ` chunk.
    pub rate: SampleRate,
    /// Channels, interleaved as the file had them.
    pub channels: u16,
    /// How the samples are stored, which is how the file stored them.
    pub storage_format: StorageFormat,
    /// Frames written per channel.
    pub frames: u64,
    /// Blocks written.
    pub blocks: u64,
    /// Transactions they were written in.
    pub commits: u64,
    /// The batch size that was in force.
    pub batch_blocks: usize,
    /// Bytes of audio the `data` chunk declared.
    pub declared_bytes: u64,
    /// Bytes of audio there turned out to be, whole frames only.
    ///
    /// Reported beside the declared figure rather than checked against it,
    /// because the two disagree for two innocent reasons - a recorder killed
    /// mid-write leaves a length it never went back to patch, and a trailing
    /// partial frame is one short sample - and for one guilty one. An import
    /// that silently landed less audio than the file said it had would be
    /// indistinguishable from a correct one, so the numbers are both printed
    /// and the person decides.
    pub read_bytes: u64,
}

impl Landed {
    /// The capture's duration in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.rate.hz() == 0 {
            return 0.0;
        }
        self.frames as f64 / f64::from(self.rate.hz())
    }
}

/// Reads a WAV file and writes it as a new `.vcw`.
///
/// The source is opened read-only and never written to. `destination` must not
/// exist; see [`mod@crate::land`] for why an interrupted import is discarded rather
/// than resumed.
///
/// # Errors
///
/// Anything [`layout`] can refuse, plus [`Error::Io`] if the audio cannot be
/// read and [`crate::Error::Project`] if the destination cannot be written.
pub fn land(source: &Path, destination: &Path, side: Side, config: Config) -> Result<Landed> {
    tracing::info!(
        source = %source.display(),
        destination = %destination.display(),
        "importing a WAV file"
    );
    let layout = layout(source)?;

    // The declared length is a claim, not a measurement: it is written before
    // the audio and a recorder that died mid-take never went back to correct
    // it. Clamped to what is on disk, and then down to a whole frame, because
    // half a frame landed is a channel swap for every sample after it.
    let on_disk = std::fs::metadata(source).map_err(|s| io(source, s))?.len();
    let frame_bytes = u64::from(layout.frame_bytes);
    let available = layout.bytes.min(on_disk.saturating_sub(layout.offset));
    let frames = available / frame_bytes;
    if frames == 0 {
        return Err(refuse(
            source,
            format!(
                "its data chunk holds no whole frames: {available} bytes at {frame_bytes} a frame"
            ),
        ));
    }
    let audio = frames * frame_bytes;

    let info = CaptureInfo {
        rate: layout.rate,
        channels: layout.channels,
        storage_format: StorageFormat::native_for(layout.format),
        // No stream was opened, exactly as for an Audacity import.
        capture_mode: CaptureMode::Imported,
        host_api: None,
        device_id: None,
        device_name: None,
        os_verified: false,
        os_report: None,
        // A WAV carries no signal chain, so nothing may be assumed about one.
        eq: CaptureEq::Unknown,
    };

    let mut file = File::open(source).map_err(|s| io(source, s))?;
    file.seek(SeekFrom::Start(layout.offset))
        .map_err(|s| io(source, s))?;

    let project = Destination::create(destination)?;
    let mut writer = Writer::begin(project, &info, config)?;
    let chunk = (writer.block_frames() * frame_bytes) as usize;
    let mut buffer = vec![0u8; chunk];
    let mut left = audio;
    while left > 0 {
        let want = chunk.min(left as usize);
        file.read_exact(&mut buffer[..want])
            .map_err(|s| io(source, s))?;
        writer.push(&buffer[..want])?;
        left -= want as u64;
    }
    let (outcome, mut project, _session) = writer.finish_with_project(CaptureState::Finalised)?;

    side::attach(&mut project, side, outcome.capture_id)?;
    {
        let conn = project.conn();
        meta::set(conn, SOURCE_KEY, &source.display().to_string())?;
        meta::set(conn, FORMAT_KEY, "wav")?;
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
        declared_bytes: layout.bytes,
        read_bytes: audio,
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
        "a WAV file landed as a capture"
    );
    Ok(landed)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Builds a WAV out of chunks, so a test can put them in any order.
    fn wav(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        for (id, payload) in chunks {
            out.extend_from_slice(*id);
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(payload);
            // The pad byte, which is what makes an odd-length chunk worth a test.
            if payload.len() % 2 == 1 {
                out.push(0);
            }
        }
        let total = (out.len() - 8) as u32;
        out[4..8].copy_from_slice(&total.to_le_bytes());
        out
    }

    /// A `fmt ` payload. `extra` pads it to an EXTENSIBLE length.
    fn fmt(tag: u16, channels: u16, rate: u32, bits: u16, extra: usize) -> Vec<u8> {
        let block = channels * (bits / 8);
        let mut out = Vec::new();
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend(std::iter::repeat_n(0u8, extra));
        out
    }

    fn written(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    }

    #[test]
    fn the_audio_is_found_past_whatever_chunks_come_first() {
        // The case that makes walking the chunks necessary rather than tidy.
        // A 24-bit file has a 40-byte EXTENSIBLE `fmt `, a tagger has left a
        // `LIST` of odd length in front of the audio, and the result is that
        // the audio is nowhere near byte 44. A reader that assumed 44 would
        // offset every sample in the run and still report a clean verify,
        // because it would be comparing the wrong bytes against themselves.
        let dir = tempfile::tempdir().expect("tempdir");
        let audio = vec![0xABu8; 24];
        let path = written(
            dir.path(),
            "tagged.wav",
            &wav(&[
                (b"fmt ", fmt(0xFFFE, 2, 96_000, 24, 24)),
                (b"LIST", b"INFOIART odd".to_vec()),
                (b"data", audio.clone()),
            ]),
        );

        let l = layout(&path).expect("layout");
        assert_eq!(l.rate, SampleRate(96_000));
        assert_eq!(l.channels, 2);
        assert_eq!(l.format, SampleFormat::S24);
        assert_eq!(l.frame_bytes, 6);
        assert_eq!(l.bytes, 24);
        assert_eq!(l.frames(), 4);
        assert_ne!(l.offset, 44, "this file's audio is not at 44");
        // The offset is the one number a mistake in is silent, so it is checked
        // against the file rather than against arithmetic.
        let raw = std::fs::read(&path).expect("read");
        assert_eq!(
            &raw[l.offset as usize..l.offset as usize + 24],
            &audio[..],
            "the offset does not point at the audio"
        );
    }

    #[test]
    fn a_data_chunk_before_the_format_is_still_found() {
        // Legal, rare, and the reason the walk does not stop at `data`.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = written(
            dir.path(),
            "backwards.wav",
            &wav(&[
                (b"data", vec![1u8; 16]),
                (b"fmt ", fmt(1, 2, 44_100, 16, 0)),
            ]),
        );
        let l = layout(&path).expect("a data chunk before fmt should still resolve");
        assert_eq!(l.format, SampleFormat::S16);
        assert_eq!(l.bytes, 16);
        assert_eq!(l.frames(), 4);
    }

    #[test]
    fn a_thirty_two_bit_rip_reads_whichever_tag_it_declares() {
        // Every WAV in /data2/source_rips is 32-bit with format tag 1, which is
        // not what the spec asks for above 16 bits. Refusing them would refuse
        // the corpus, so both tags are read.
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, tag, extra) in [("plain.wav", 1u16, 0usize), ("ext.wav", 0xFFFE, 24)] {
            let path = written(
                dir.path(),
                name,
                &wav(&[
                    (b"fmt ", fmt(tag, 2, 48_000, 32, extra)),
                    (b"data", vec![0u8; 32]),
                ]),
            );
            let l = layout(&path).expect(name);
            assert_eq!(l.format, SampleFormat::S32, "{name}");
            assert_eq!(l.frame_bytes, 8, "{name}");
            assert!(
                (l.seconds() - 4.0 / 48_000.0).abs() < f64::EPSILON,
                "{name}"
            );
        }
    }

    #[test]
    fn what_cannot_be_fed_through_the_capture_path_is_refused_by_name() {
        // Each of these would otherwise fail somewhere deep: a float file as a
        // burst of noise, a header-less file as a format mismatch thousands of
        // frames in. Refusing at the door with the number in the message is the
        // difference between a diagnosis and a mystery.
        let dir = tempfile::tempdir().expect("tempdir");
        let cases: [(&str, Vec<u8>, &str); 4] = [
            (
                "float.wav",
                wav(&[
                    (b"fmt ", fmt(3, 2, 48_000, 32, 0)),
                    (b"data", vec![0u8; 32]),
                ]),
                "0x0003",
            ),
            (
                "eightbit.wav",
                wav(&[(b"fmt ", fmt(1, 1, 48_000, 8, 0)), (b"data", vec![0u8; 32])]),
                "8-bit",
            ),
            (
                "nodata.wav",
                wav(&[(b"fmt ", fmt(1, 2, 48_000, 16, 0))]),
                "no data chunk",
            ),
            ("raw.pcm", vec![0u8; 64], "not a RIFF/WAVE"),
        ];
        for (name, bytes, expected) in cases {
            let path = written(dir.path(), name, &bytes);
            let error = layout(&path)
                .map(|l| format!("{l:?}"))
                .expect_err(&format!("{name} was accepted"));
            let text = format!("{error:#}");
            assert!(
                text.contains(expected),
                "{name}: wanted {expected:?} in {text:?}"
            );
        }
    }

    #[test]
    fn a_block_align_that_contradicts_the_bit_depth_is_refused() {
        // The one internal inconsistency worth checking. If it is wrong, the
        // file is not laid out the way the feeder and the verifier both assume,
        // and the symptom would be a byte mismatch far into the run.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut bad = fmt(1, 2, 48_000, 16, 0);
        bad[12..14].copy_from_slice(&8u16.to_le_bytes());
        let path = written(
            dir.path(),
            "skew.wav",
            &wav(&[(b"fmt ", bad), (b"data", vec![0u8; 32])]),
        );
        let text = format!("{:#}", layout(&path).expect_err("accepted"));
        assert!(text.contains('8') && text.contains('4'), "{text}");
    }
}
