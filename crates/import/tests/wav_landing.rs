/*
 *  wav_landing.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Landing a WAV file as a capture: the bytes that go in are the bytes that come out.
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

//! Landing a WAV as a capture (§12).
//!
//! One test, because the WAV path is one read loop and the claim it has to
//! support is one claim: the audio that lands is the audio in the file, in that
//! order, unconverted. Everything else about an imported capture is already
//! covered by `landing.rs` - the writer, the blocks, the summaries and the
//! reader are the same ones.
//!
//! The source is built here rather than taken from `/data2/source_rips`,
//! because CI has no corpus and because a fixture of our own can carry the
//! three awkward shapes a real rip has in ones and twos: a chunk before the
//! audio, a `data` size that overstates what is on disk, and a trailing partial
//! frame. A file with none of them would pass whether the walk and the clamps
//! worked or not.

use std::path::Path;

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
///
/// A ramp, not a tone: a capture written in the wrong order, or one frame out,
/// or with the channels swapped, still looks like a tone.
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

/// A WAV carrying `audio`, with the three awkward shapes in it.
fn wav(audio: &[u8]) -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // WAVE_FORMAT_EXTENSIBLE, which anything above 16 bits has to use, and a
    // 40-byte fmt chunk rather than the canonical 16.
    let mut fmt: Vec<u8> = Vec::new();
    fmt.extend_from_slice(&0xFFFE_u16.to_le_bytes());
    fmt.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
    fmt.extend_from_slice(&RATE.to_le_bytes());
    fmt.extend_from_slice(&(RATE * FRAME_BYTES as u32).to_le_bytes());
    fmt.extend_from_slice(&(FRAME_BYTES as u16).to_le_bytes());
    fmt.extend_from_slice(&24_u16.to_le_bytes());
    fmt.extend(std::iter::repeat_n(0_u8, 24));
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    out.extend_from_slice(&fmt);

    // A tagger's chunk, before the audio, of odd length so the pad byte counts.
    let list = b"INFOINAMtest".to_vec();
    out.extend_from_slice(b"LIST");
    out.extend_from_slice(&(list.len() as u32 + 1).to_le_bytes());
    out.extend_from_slice(&list);
    out.push(b'x');
    out.push(0);

    // A `data` size that overstates what follows, as a recorder killed
    // mid-write leaves behind.
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(audio.len() as u32 + 500).to_le_bytes());
    out.extend_from_slice(audio);
    // ...and a trailing partial frame, which is not a frame.
    out.extend_from_slice(&[0_u8; FRAME_BYTES - 2]);

    let total = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&total.to_le_bytes());
    out
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
    let source = directory.path().join("side-a.wav");
    let destination = directory.path().join("side-a.vcw");
    let expected = audio();
    std::fs::write(&source, wav(&expected)).expect("write the fixture");

    let landed = vcw_import::wav::land(
        &source,
        &destination,
        Side::from_index(1).expect("side B"),
        Config::default(),
    )
    .expect("landing a plain WAV");

    assert_eq!(landed.rate, SampleRate(RATE));
    assert_eq!(landed.channels, CHANNELS as u16);
    // Packed, not padded: a WAV writes 24-bit in three bytes and so does VCW,
    // which is why the copy below can be a copy.
    assert_eq!(landed.storage_format, StorageFormat::Int24Packed);
    assert_eq!(landed.frames, FRAMES as u64);
    assert_eq!(landed.side, Side::from_index(1).expect("side B"));

    // The declared length was a lie and the tail was not a whole frame. Both
    // are reported rather than repaired, and neither landed.
    assert_eq!(landed.declared_bytes, expected.len() as u64 + 500);
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
        vcw_project::meta::get(project.conn(), vcw_import::wav::FORMAT_KEY).expect("meta"),
        Some("wav".to_owned())
    );
}
