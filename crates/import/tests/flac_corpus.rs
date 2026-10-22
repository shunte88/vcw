/*
 *  flac_corpus.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Landing real FLAC rips, and agreeing with the stream's own MD5.
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
//! Landing real FLAC rips, and agreeing with the stream's own MD5.
//!
//! `flac_landing.rs` writes a stream with this crate's dependency and reads it
//! back with the same one, which proves the plumbing and nothing about real
//! files. These are the user's own rips: 13 of the 62 in `/data2/source_rips`
//! are FLAC, the largest is 1.7 GB, and none of them is in the repository or
//! ever will be. So they are `#[ignore]`d and pointed at by an environment
//! variable:
//!
//! ```text
//! VCW_FLAC_CORPUS=/data2/source_rips VCW_SCRATCH=/data2/vcw-scratch \
//!     cargo test -p vcw-import --test flac_corpus -- --ignored --nocapture
//! ```
//!
//! **The variable is required, not optional.** A corpus test that quietly
//! becomes a no-op when the corpus is absent is a test that passes by doing
//! nothing, and this project has been bitten by exactly that.
//!
//! `VCW_SCRATCH` is not optional either in practice: a landed 192 kHz side is
//! 3.5 GB, the system temporary directory on these machines is tmpfs, and
//! 3.5 GB of RAM spent to avoid typing a path is not a trade.
//!
//! The landing half runs on a reduced set: the smallest file of each distinct
//! shape the corpus holds, where a shape is a rate, a width and a channel
//! count. That is what varies between these files and the only thing the
//! importer can be wrong about per file; the other ten are the same three
//! shapes again at ten times the wall clock. Every file is still read for its
//! layout, which is cheap and is where an unsupported width would show up.
//!
//! The oracle is in two halves, neither of them ours:
//!
//! 1. `flac_codec::decode::verify` decodes the file and compares the result
//!    against the MD5 the encoder wrote into `STREAMINFO`. That is the ripping
//!    software's own claim about the audio, so a mismatch means the file on
//!    disk is damaged and the import is not the thing under suspicion.
//! 2. The landed capture is streamed back through the reader playback uses and
//!    compared byte for byte against a fresh decode of the source. Combined
//!    with the first half, a pass means the bytes in the project are the bytes
//!    the rip's own checksum describes.

use std::io::Read;
use std::path::{Path, PathBuf};

use flac_codec::decode::{Verified, verify};
use vcw_project::persistence::Config;
use vcw_types::vinyl::Side;

/// Where the real rips are. Required; see the module documentation.
const CORPUS_ENV: &str = "VCW_FLAC_CORPUS";
/// Where a landed project goes. Required for the landing test, for the reason
/// the module documentation gives.
const SCRATCH_ENV: &str = "VCW_SCRATCH";
/// What the corpus held when this was written. A floor rather than an equality
/// because the user adds rips, and the point of the number is that a run over
/// an empty or half-copied directory cannot look like a pass.
const EXPECTED_FLAC: usize = 13;

fn corpus_dir() -> PathBuf {
    let raw = std::env::var(CORPUS_ENV).unwrap_or_else(|_| {
        panic!(
            "{CORPUS_ENV} is not set. These tests read the real FLAC rips; run them as\n  \
             {CORPUS_ENV}=/data2/source_rips VCW_SCRATCH=/data2/vcw-scratch \
             cargo test -p vcw-import --test flac_corpus -- --ignored"
        )
    });
    let dir = PathBuf::from(raw);
    assert!(
        dir.is_dir(),
        "{CORPUS_ENV} points at {dir:?}, which is not a directory"
    );
    dir
}

/// A directory to land projects into, on real storage because the operator said
/// where. Unlike `corpus.rs` this has no temporary-directory fallback: the
/// smallest file in the reduced set lands as 1.1 GB.
fn scratch() -> tempfile::TempDir {
    let dir = std::env::var(SCRATCH_ENV).unwrap_or_else(|_| {
        panic!(
            "{SCRATCH_ENV} is not set. A landed side here is gigabytes and the system \
             temporary directory is tmpfs; point it at real storage"
        )
    });
    tempfile::tempdir_in(dir).expect("a temporary directory in VCW_SCRATCH")
}

/// Every FLAC in the corpus, sorted, so a failure names the same file twice in
/// a row.
fn rips(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read the corpus directory")
        .filter_map(|entry| {
            let path = entry.expect("a corpus directory entry").path();
            // The extension picks the candidates. `layout` decides whether it
            // is really a FLAC, and a file named `.flac` that is not one is a
            // failure rather than a skip.
            (path.extension()?.to_str()? == "flac").then_some(path)
        })
        .collect();
    found.sort();
    assert!(
        found.len() >= EXPECTED_FLAC,
        "{CORPUS_ENV} holds {} FLAC files and the corpus has at least {EXPECTED_FLAC}; \
         a partial copy would otherwise pass by reading less",
        found.len()
    );
    found
}

/// Reads up to a whole buffer, because one `read` is allowed to return less and
/// a short read against a full one is not a difference in the audio.
fn fully(source: &mut impl Read, buffer: &mut [u8]) -> usize {
    let mut got = 0;
    while got < buffer.len() {
        let read = source.read(&mut buffer[got..]).expect("decode the source");
        if read == 0 {
            break;
        }
        got += read;
    }
    got
}

/// The same, for the capture reader, which fills rather than reads.
fn filled(reader: &mut vcw_project::pcm::Reader<'_>, buffer: &mut [u8]) -> usize {
    let mut got = 0;
    while got < buffer.len() {
        let read = reader.fill(&mut buffer[got..]).expect("read the capture");
        if read == 0 {
            break;
        }
        got += read;
    }
    got
}

/// What the importer makes of every file, which is also the inventory. Cheap:
/// it reads a header per file and no audio.
#[test]
#[ignore = "reads the real corpus; set VCW_FLAC_CORPUS and run with --ignored"]
fn every_rip_in_the_corpus_declares_a_shape_a_capture_can_store() {
    let dir = corpus_dir();
    let paths = rips(&dir);

    for path in &paths {
        let name = path.file_name().expect("a file name").to_string_lossy();
        let layout =
            vcw_import::flac::layout(path).unwrap_or_else(|e| panic!("layout {name}: {e}"));
        let declared = match layout.declared_frames {
            Some(frames) => format!("{frames} frames, {:.3} s", layout.seconds()),
            None => "no declared length".to_string(),
        };
        println!(
            "{name}: {} Hz, {} ch, {:?}, {declared}",
            layout.rate.hz(),
            layout.channels,
            layout.format
        );
        assert!(layout.channels > 0, "{name}: no channels");
        assert!(layout.rate.hz() > 0, "{name}: no rate");
        assert_eq!(
            usize::from(layout.frame_bytes),
            usize::from(layout.channels) * layout.format.bytes_per_sample(),
            "{name}: the frame width has to be the packed width times the channels"
        );
    }
    println!(
        "{} rips declare a shape the capture path has a name for",
        paths.len()
    );
}

/// The reduced set: the smallest file of each shape, landed and compared.
#[test]
#[ignore = "lands the real corpus; set VCW_FLAC_CORPUS and VCW_SCRATCH, run with --ignored"]
fn a_real_rip_lands_as_the_bytes_its_own_checksum_describes() {
    let dir = corpus_dir();
    let scratch = scratch();

    // One file per shape, smallest first, so the set covers every rate and
    // width present without landing the same shape eleven times.
    let mut shapes: std::collections::BTreeMap<(u32, usize, u16), (u64, PathBuf)> =
        std::collections::BTreeMap::new();
    for path in rips(&dir) {
        let layout = vcw_import::flac::layout(&path).expect("a layout");
        let size = std::fs::metadata(&path).expect("a size").len();
        let shape = (
            layout.rate.hz(),
            layout.format.bytes_per_sample(),
            layout.channels,
        );
        let entry = shapes.entry(shape).or_insert((size, path.clone()));
        if size < entry.0 {
            *entry = (size, path);
        }
    }
    assert!(!shapes.is_empty(), "no shapes in the corpus");

    for (shape, (size, source)) in &shapes {
        let name = source.file_name().expect("a file name").to_string_lossy();
        println!("{name}: {shape:?}, {size} bytes");

        // Half one: the file agrees with the checksum its ripper wrote.
        match verify(source).unwrap_or_else(|e| panic!("verify {name}: {e}")) {
            Verified::MD5Match => println!("  the source decodes to its own MD5"),
            // Legal, and it means this file can only be compared against a
            // second decode of itself rather than against the ripper's claim.
            Verified::NoMD5 => println!("  the source carries no MD5"),
            Verified::MD5Mismatch => {
                panic!("{name}: the file on disk does not decode to its own MD5")
            }
        }

        let layout = vcw_import::flac::layout(source).expect("a layout");
        let destination = scratch.path().join(format!("{name}.vcw"));
        let landed = vcw_import::flac::land(source, &destination, Side::A, Config::default())
            .unwrap_or_else(|e| panic!("land {name}: {e}"));
        assert_eq!(landed.rate, layout.rate, "{name}: the landed rate");
        assert_eq!(landed.channels, layout.channels, "{name}: the channels");
        if let Some(frames) = layout.declared_frames {
            assert_eq!(
                landed.frames, frames,
                "{name}: the frames STREAMINFO declared"
            );
        }

        // Half two: every byte in the project is the byte the decoder gives,
        // streamed rather than collected because this one is gigabytes.
        let project =
            vcw_project::Project::open_read_only(&destination).expect("open the landed project");
        let stored = vcw_project::Layout::of(project.conn(), landed.capture_id).expect("layout");
        assert_eq!(stored.frames, landed.frames, "{name}: the capture's length");
        let mut reader =
            vcw_project::pcm::Reader::open(project.conn(), landed.capture_id, stored.span())
                .expect("a reader over the whole capture");
        let mut decoder = flac_codec::decode::FlacByteReader::endian(
            std::io::BufReader::new(std::fs::File::open(source).expect("reopen the source")),
            flac_codec::byteorder::LittleEndian,
        )
        .expect("a decoder");

        let frame_bytes = stored.frame_bytes();
        // A whole number of frames, so an offset converts to a frame without a
        // remainder when one differs.
        let chunk = (1 << 20) / frame_bytes * frame_bytes;
        let mut from_project = vec![0_u8; chunk];
        let mut from_source = vec![0_u8; chunk];
        let mut at = 0_u64;
        loop {
            let read = filled(&mut reader, &mut from_project);
            let decoded = fully(&mut decoder, &mut from_source[..read.max(1)]);
            assert_eq!(
                read,
                decoded,
                "{name}: at frame {}, the capture gave {read} bytes and the decoder {decoded}",
                at / frame_bytes as u64
            );
            if read == 0 {
                break;
            }
            if from_project[..read] != from_source[..read] {
                let differs = (0..read)
                    .find(|&i| from_project[i] != from_source[i])
                    .expect("a differing byte");
                panic!(
                    "{name}: frame {} differs: the capture holds {:#04x}, the decoder gave {:#04x}",
                    (at + differs as u64) / frame_bytes as u64,
                    from_project[differs],
                    from_source[differs]
                );
            }
            at += read as u64;
        }
        assert_eq!(
            at,
            landed.frames * frame_bytes as u64,
            "{name}: the comparison has to cover the whole capture"
        );
        println!("  {at} bytes identical over {} frames", landed.frames);

        // The next file in the set is gigabytes too, and the scratch directory
        // only empties when the whole test ends.
        drop(project);
        std::fs::remove_file(&destination).expect("remove the landed project");
    }
    println!("{} shapes landed byte for byte", shapes.len());
}
