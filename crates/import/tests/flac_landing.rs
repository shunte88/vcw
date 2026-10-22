/*
 *  flac_landing.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Landing a FLAC file as a capture.
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

//! Landing a FLAC as a capture (§12).
//!
//! The sibling of `wav_landing.rs`, and it has to support the same claim with
//! one thing added. The WAV path copies bytes, so the question is whether the
//! walk found the right ones; the FLAC path *decodes*, so the question is
//! whether a lossless codec came back lossless through our storage - packed
//! three bytes to a 24-bit sample, in frame order, with the channels the way
//! round they went in. A ramp is the fixture because a tone that came back one
//! frame out, or with the channels swapped, is still a tone.
//!
//! Three tests: the round trip, a stream that declares no length, and a width a
//! capture has no name for. The corpus at `/data2/source_rips` is all 24-bit
//! stereo, so the second and third shapes are not in it, and CI has no corpus
//! at all.

use std::io::Write;
use std::path::Path;

use flac_codec::byteorder::LittleEndian;
use flac_codec::encode::{FlacByteWriter, Options};

use vcw_project::persistence::Config;
use vcw_types::vinyl::Side;
use vcw_types::{CaptureMode, SampleRate, StorageFormat};

/// Frames of audio in the fixture, and the shape of them.
const FRAMES: usize = 2_000;
const CHANNELS: usize = 2;
const BYTES_PER_SAMPLE: usize = 3;
const FRAME_BYTES: usize = CHANNELS * BYTES_PER_SAMPLE;
const RATE: u32 = 48_000;

/// 24-bit stereo audio where every sample is distinguishable from every other.
fn audio() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FRAMES * FRAME_BYTES);
    for frame in 0..FRAMES {
        for channel in 0..CHANNELS {
            let value = (frame * CHANNELS + channel) as i32 - (FRAMES * CHANNELS / 2) as i32;
            bytes.extend_from_slice(&value.to_le_bytes()[..BYTES_PER_SAMPLE]);
        }
    }
    bytes
}

/// Encodes `samples` as a FLAC file at `path`, at the given width.
///
/// A real encoder and not a hand-built stream: a FLAC header can be written by
/// hand but a frame cannot, and the thing under test is the decode.
fn encode(path: &Path, samples: &[u8], bits: u32) {
    let file = std::fs::File::create(path).expect("create the fixture");
    let mut writer = FlacByteWriter::endian(
        file,
        LittleEndian,
        Options::default(),
        RATE,
        bits,
        CHANNELS as u8,
        // The length is left for `finalize` to fill in, which is what an
        // encoder writing to a file does.
        None,
    )
    .expect("an encoder at a width FLAC takes");
    writer.write_all(samples).expect("encode the audio");
    writer.finalize().expect("finalize the stream");
}

/// Zeroes `STREAMINFO`'s total sample count, as an encoder that was piped its
/// input and could not seek back leaves it.
///
/// Done to the bytes because there is no way to ask the encoder for it: a
/// `File` is seekable, so `finalize` always goes back and fills the figure in.
/// `STREAMINFO` is the first metadata block and carries no checksum, so the
/// field is 36 bits at a fixed place and nothing else has to change. The MD5
/// below it stays correct, because it is a sum of the audio and the audio has
/// not moved.
fn forget_the_length(path: &Path) {
    let mut bytes = std::fs::read(path).expect("read the fixture back");
    assert_eq!(&bytes[0..4], b"fLaC", "not a FLAC stream");
    // `fLaC` plus a four-byte block header, then 34 bytes of STREAMINFO. The
    // rate, channel count, width and sample count share one 64-bit field at
    // offset 10 within it, and the count is its low 36 bits.
    let at = 8 + 10;
    let field = u64::from_be_bytes(bytes[at..at + 8].try_into().expect("eight bytes"));
    let cleared = field & !((1_u64 << 36) - 1);
    bytes[at..at + 8].copy_from_slice(&cleared.to_be_bytes());
    std::fs::write(path, bytes).expect("rewrite the fixture");
}

/// Reads a landed capture back through the reader playback uses.
fn played_back(path: &Path, capture_id: i64) -> Vec<u8> {
    let project = vcw_project::Project::open_read_only(path).expect("open the landed project");
    let layout = vcw_project::Layout::of(project.conn(), capture_id).expect("layout");
    let mut reader = vcw_project::pcm::Reader::open(project.conn(), capture_id, layout.span())
        .expect("a reader over the whole capture");
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 4_096];
    loop {
        let filled = reader.fill(&mut buffer).expect("fill");
        if filled == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..filled]);
    }
    bytes
}

#[test]
fn the_bytes_that_go_in_are_the_bytes_that_come_out() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = directory.path().join("side-a.flac");
    let destination = directory.path().join("side-a.vcw");
    let expected = audio();
    encode(&source, &expected, 24);

    let landed = vcw_import::flac::land(
        &source,
        &destination,
        Side::from_index(1).expect("side B"),
        Config::default(),
    )
    .expect("landing a FLAC");

    assert_eq!(landed.rate, SampleRate(RATE));
    assert_eq!(landed.channels, CHANNELS as u16);
    // Packed, not padded: the decoder hands back three bytes to a 24-bit
    // sample and so does VCW, which is why the landing can be a copy.
    assert_eq!(landed.storage_format, StorageFormat::Int24Packed);
    assert_eq!(landed.frames, FRAMES as u64);
    assert_eq!(landed.side, Side::from_index(1).expect("side B"));
    // The encoder went back and filled the length in, so the two figures are
    // the same here. They are both reported anyway, because the day they
    // disagree is the day somebody needs to know.
    assert_eq!(landed.declared_bytes, expected.len() as u64);
    assert_eq!(landed.read_bytes, expected.len() as u64);

    let played = played_back(&destination, landed.capture_id);
    assert_eq!(
        played.len(),
        expected.len(),
        "the capture is a different length from the file"
    );
    assert!(
        played == expected,
        "the audio came back changed, or in a different order"
    );

    // An imported capture says so, and says nothing it was not told.
    let project = vcw_project::Project::open_read_only(&destination).expect("open");
    let (mode, device): (String, Option<String>) = project
        .conn()
        .query_row(
            "SELECT capture_mode, device_name FROM captures WHERE capture_id = ?1",
            [landed.capture_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the capture row");
    assert_eq!(mode, CaptureMode::Imported.as_str());
    assert_eq!(device, None);
    assert_eq!(
        vcw_project::meta::get(project.conn(), vcw_import::flac::FORMAT_KEY).expect("meta"),
        Some("flac".to_owned())
    );
}

#[test]
fn a_stream_that_declares_no_length_still_lands() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = directory.path().join("piped.flac");
    let destination = directory.path().join("piped.vcw");
    let expected = audio();
    encode(&source, &expected, 24);
    forget_the_length(&source);

    // A dry run says it does not know, rather than saying nought.
    let layout = vcw_import::flac::layout(&source).expect("a layout without a length");
    assert_eq!(layout.declared_frames, None);
    assert_eq!(layout.bytes(), 0);
    assert_eq!(layout.rate, SampleRate(RATE));

    let landed = vcw_import::flac::land(
        &source,
        &destination,
        Side::from_index(0).expect("side A"),
        Config::default(),
    )
    .expect("landing a FLAC with no declared length");

    assert_eq!(landed.frames, FRAMES as u64, "the audio is all there");
    assert_eq!(landed.declared_bytes, 0, "nothing was claimed");
    assert_eq!(landed.read_bytes, expected.len() as u64);
    assert!(
        played_back(&destination, landed.capture_id) == expected,
        "the audio came back changed"
    );
}

#[test]
fn a_width_a_capture_cannot_store_is_refused_by_width() {
    let directory = tempfile::tempdir().expect("tempdir");
    let source = directory.path().join("narrow.flac");
    let destination = directory.path().join("narrow.vcw");
    // Eight bits is legal FLAC and byte-aligned, so the encoder writes it and
    // the refusal is ours rather than the decoder's.
    let narrow: Vec<u8> = (0..FRAMES * CHANNELS).map(|n| (n % 251) as u8).collect();
    encode(&source, &narrow, 8);

    let error = vcw_import::flac::land(
        &source,
        &destination,
        Side::from_index(0).expect("side A"),
        Config::default(),
    )
    .expect_err("8-bit is not a width a capture stores");
    let said = error.to_string();
    assert!(
        said.contains("8-bit") && said.contains("16, 24 and 32"),
        "the refusal has to name the width and the widths: {said}"
    );
    // Refused before anything was written, so there is no half a project to
    // explain to whoever finds it.
    assert!(
        !destination.exists(),
        "a refused import left a project behind"
    );
}
