/*
 *  encoder.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Container selection and the two lossless writers (D5).
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

//! Container selection and the two lossless writers (D5).
//!
//! Our own WAV writer - trivial to write, and it avoids `hound`'s format limits
//! at 24/192. FLAC through `flacenc`, pure Rust and Apache-2.0. The two lossy
//! containers live in [`crate::lossy`], behind the `mp3` and `ogg` cargo
//! features: they are the only code in this crate that links a C library, the
//! only code that converts samples rather than copying them, and the only code
//! whose output cannot be compared byte for byte with its input, so they are
//! kept where those three facts can be stated once.
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
//! - **MP3 carries nine sample rates and at most two channels.** MPEG-1 stops
//!   at 48 kHz, so a 96 or 192 kHz capture has no MP3 path without resampling,
//!   and choosing a resampling filter is the same kind of decision as choosing
//!   a dither. Measured against the corpus on 2026-10-04: of the 59 rips in
//!   `/data2/source_rips`, 54 are at 44.1 or 48 kHz and go to MP3 untouched,
//!   and the 5 at 192 kHz are refused.
//! - **Ogg Vorbis refuses almost nothing.** libvorbis takes any rate VCW
//!   records and up to 255 channels, which makes it the lossy format for a
//!   high-rate capture and the only container besides WAV that will take a
//!   `Float32` one.
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

/// How hard a lossy encoder should work.
///
/// Three choices rather than a number, because a number invites one nobody can
/// justify. The useful span is V0 to V5 for MP3 and q3 to q8 for Vorbis;
/// outside it a setting is either indistinguishable from the one above it or
/// audibly worse than the record, and offering `V7` to someone archiving vinyl
/// is offering them a way to waste an afternoon.
///
/// Every level is variable-bitrate. Constant bitrate exists for streaming to
/// something that has to budget bandwidth, and a file on a disc is not that.
///
/// The lossless containers ignore this entirely -
/// [`with_quality`](Container::with_quality) is a no-op on WAV and FLAC - which
/// is what lets a settings panel keep one value across all four formats instead
/// of a field that appears and disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Quality {
    /// As close to the record as the codec gets: MP3 V0, Vorbis q8.
    Transparent,
    /// The default: MP3 V2, Vorbis q6, both around 190 kbit/s.
    #[default]
    High,
    /// Small enough to stop thinking about: MP3 V5, Vorbis q3.
    Compact,
}

impl Quality {
    /// Every level, in the order a UI should offer them.
    ///
    /// Here rather than in the UI so that a list of options and the parser that
    /// reads them back cannot drift apart.
    pub const ALL: [Self; 3] = [Self::Transparent, Self::High, Self::Compact];

    /// The word a person types, and the one a settings file stores.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Transparent => "transparent",
            Self::High => "high",
            Self::Compact => "compact",
        }
    }

    /// Reads a quality from what was typed, in any case.
    #[must_use]
    pub fn parse(given: &str) -> Option<Self> {
        let given = given.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|level| level.name() == given)
    }

    /// LAME's own spelling of this level, which is what a bitrate table shows.
    #[must_use]
    pub const fn mp3(self) -> &'static str {
        match self {
            Self::Transparent => "V0",
            Self::High => "V2",
            Self::Compact => "V5",
        }
    }

    /// `oggenc`'s own spelling of this level.
    #[must_use]
    pub const fn vorbis(self) -> &'static str {
        match self {
            Self::Transparent => "q8",
            Self::High => "q6",
            Self::Compact => "q3",
        }
    }
}

impl std::fmt::Display for Quality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A deliverable container.
///
/// The two lossy variants carry their quality, so that a resolved
/// [`crate::splitter::Plan`] records what it is going to write rather than
/// merely what kind of thing. A plan that said `MP3` and left the bitrate to
/// whatever the writer felt like would be a plan that could not be checked
/// against the file it produced.
///
/// All four exist whatever the cargo features say. A build without the `mp3`
/// feature still understands `--format mp3` and still refuses it with a
/// sentence explaining that this binary cannot write one, which is a better
/// answer than not knowing the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Container {
    /// Uncompressed PCM in a RIFF wrapper.
    Wav,
    /// Lossless compression.
    Flac,
    /// MPEG-1 Audio Layer III, variable bitrate.
    Mp3(Quality),
    /// Vorbis in an Ogg stream.
    OggVorbis(Quality),
}

impl Container {
    /// Every container, in the order a UI should offer them.
    ///
    /// Lossless first, because the archival copy is the one that matters and a
    /// list opening with MP3 would be a list suggesting otherwise.
    pub const ALL: [Self; 4] = [
        Self::Flac,
        Self::Wav,
        Self::Mp3(Quality::High),
        Self::OggVorbis(Quality::High),
    ];

    /// The file extension, without the dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3(_) => "mp3",
            // Xiph's own guidance is `.oga` for Ogg audio generally and `.ogg`
            // for Ogg Vorbis specifically. `.ogg` is also what every player,
            // phone and car stereo expects, so it is what gets written; `.oga`
            // is accepted on the way in.
            Self::OggVorbis(_) => "ogg",
        }
    }

    /// The name used in messages, without the quality.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::Mp3(_) => "MP3",
            Self::OggVorbis(_) => "Ogg Vorbis",
        }
    }

    /// The cargo feature a build needs to write this, where it needs one.
    #[must_use]
    pub const fn feature(self) -> Option<&'static str> {
        match self {
            Self::Wav | Self::Flac => None,
            Self::Mp3(_) => Some("mp3"),
            Self::OggVorbis(_) => Some("ogg"),
        }
    }

    /// Whether this build can write it.
    ///
    /// The feature question, answered in the crate the features belong to.
    /// `cfg!(feature = "mp3")` written anywhere else asks about *that* crate's
    /// features, and in `app/src-tauri` the answer would be about the shell.
    ///
    /// Matched on the variant rather than on [`feature`](Self::feature)'s
    /// string, so a container added later cannot compile until it says which
    /// way it goes.
    #[must_use]
    pub const fn compiled_in(self) -> bool {
        match self {
            Self::Wav | Self::Flac => true,
            Self::Mp3(_) => cfg!(feature = "mp3"),
            Self::OggVorbis(_) => cfg!(feature = "ogg"),
        }
    }

    /// The quality this will be written at, where the container has one.
    #[must_use]
    pub const fn quality(self) -> Option<Quality> {
        match self {
            Self::Wav | Self::Flac => None,
            Self::Mp3(quality) | Self::OggVorbis(quality) => Some(quality),
        }
    }

    /// Whether audio will be thrown away on the way out.
    #[must_use]
    pub const fn is_lossy(self) -> bool {
        self.quality().is_some()
    }

    /// The same container at a different quality.
    ///
    /// A no-op on WAV and FLAC, deliberately rather than by oversight: someone
    /// who has set a quality and then switches the format to FLAC has not asked
    /// for an error, and a settings file that kept a quality only while the
    /// format happened to be lossy would lose it on every round trip.
    #[must_use]
    pub const fn with_quality(self, quality: Quality) -> Self {
        match self {
            Self::Wav | Self::Flac => self,
            Self::Mp3(_) => Self::Mp3(quality),
            Self::OggVorbis(_) => Self::OggVorbis(quality),
        }
    }

    /// Reads a container from an extension, with or without its dot.
    ///
    /// Lossy containers come back at [`Quality::High`];
    /// [`with_quality`](Self::with_quality) is how a caller that was given a
    /// quality as well applies it.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "wav" | "wave" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "mp3" => Some(Self::Mp3(Quality::default())),
            "ogg" | "oga" | "vorbis" => Some(Self::OggVorbis(Quality::default())),
            _ => None,
        }
    }

    /// Every spelling a person may type, for a message that lists them.
    #[must_use]
    pub fn spellings() -> String {
        Self::ALL
            .iter()
            .map(|container| container.extension())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl std::fmt::Display for Container {
    /// The container and, for a lossy one, the codec's own spelling of the
    /// quality - `MP3 V2` - so that what VCW says it wrote can be checked
    /// against what a decoder says it read.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wav | Self::Flac => f.write_str(self.name()),
            Self::Mp3(quality) => write!(f, "MP3 {}", quality.mp3()),
            Self::OggVorbis(quality) => write!(f, "Ogg Vorbis {}", quality.vorbis()),
        }
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
/// An enum rather than a boxed trait: `finish` consumes the writer and there
/// are four containers, neither of which wants `dyn`.
///
/// The lossy variants are absent from a build without their cargo feature.
/// [`Container`] is not - see its own docs for why the refusal is better than
/// the gap.
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
    /// An MP3 file. Needs the `mp3` feature.
    #[cfg(feature = "mp3")]
    Mp3(Box<crate::lossy::Mp3>),
    /// An Ogg Vorbis file. Needs the `ogg` feature.
    #[cfg(feature = "ogg")]
    OggVorbis(Box<crate::lossy::Ogg>),
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
            // Both arms go through `crate::lossy`, which has a stub for each
            // whose only job is to raise [`Error::NoEncoder`]. The alternative
            // was `#[cfg]` on the arms themselves, and a match arm that exists
            // in one build and not another is a match that has to be read twice
            // to be trusted.
            Container::Mp3(quality) => crate::lossy::mp3(path, spec, quality),
            Container::OggVorbis(quality) => crate::lossy::ogg(path, spec, quality),
        }
    }

    /// Whether [`create`](Self::create) would accept this spec, without
    /// touching the filesystem.
    ///
    /// Exists so a plan can be refused before a byte is written. A dry run that
    /// says `3 file(s) in FLAC` for a float32 capture is worse than no dry run:
    /// the whole point of planning first is that a container can be argued with
    /// for free, and a plan that only fails once the first file is open has
    /// moved the argument to after the directories were made.
    ///
    /// Both writers check again in `create`, which is not duplication. This is
    /// a question about a spec; that is a precondition on a file, and a writer
    /// reached any other way still owes it.
    ///
    /// # Errors
    ///
    /// [`Error::Unencodable`] for a format the container cannot carry,
    /// [`Error::TooLargeForWav`] for a piece too long for RIFF to describe, and
    /// [`Error::NoEncoder`] for a container this build was not compiled with.
    pub fn vet(container: Container, spec: &Spec) -> Result<()> {
        match container {
            Container::Wav => Wav::vet(spec),
            Container::Flac => Flac::vet(spec),
            Container::Mp3(_) => crate::lossy::vet_mp3(spec),
            Container::OggVorbis(_) => crate::lossy::vet_ogg(spec),
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
            #[cfg(feature = "mp3")]
            Self::Mp3(mp3) => mp3.write(stored),
            #[cfg(feature = "ogg")]
            Self::OggVorbis(ogg) => ogg.write(stored),
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
            #[cfg(feature = "mp3")]
            Self::Mp3(mp3) => mp3.finish(),
            #[cfg(feature = "ogg")]
            Self::OggVorbis(ogg) => ogg.finish(),
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

/// Whether a container could carry this audio at all.
///
/// About the recording, not about this binary: a build compiled without the
/// `mp3` feature still answers yes for an MP3 of 44.1 kHz stereo, because
/// [`Error::NoEncoder`] is a different sentence and explains itself. Judging
/// otherwise would make the advice in a refusal depend on the cargo features,
/// and the `features` gate leg would then disagree with the default build about
/// what the prose should say.
pub(crate) fn carries(container: Container, spec: &Spec) -> bool {
    match container {
        Container::Wav => Wav::carries(spec),
        Container::Flac => Flac::why(spec).is_none(),
        Container::Mp3(_) => crate::lossy::mp3_why(spec).is_none(),
        Container::OggVorbis(_) => crate::lossy::ogg_why(spec).is_none(),
    }
}

/// The "export it as this instead" clause for a capture one container refuses.
///
/// Every refusal in this crate ends with this rather than with a sentence
/// naming containers from memory, and that is the whole point. The hand-written
/// version drifted four separate ways in one work package: all three FLAC
/// refusals said "Export this one as WAV", written before Ogg Vorbis existed
/// and never updated; the MP3 rate refusal offered FLAC, which stops at 96 kHz
/// just as MPEG stops at 48; [`Error::TooLargeForWav`] offered FLAC for a
/// capture FLAC would refuse on bit depth; and the MP3 refusal offered WAV for
/// a side too long for a 32-bit RIFF size. None of them was caught by a test,
/// because advice written as a string literal agrees with whatever it said
/// yesterday. A real 192 kHz rip found the first one.
///
/// Asking every container makes all four impossible by construction, and makes
/// a fifth container appear in every message that should mention it on the day
/// it is added.
///
/// Both halves of the answer where both exist. Someone refused a format wants
/// to know how to keep the capture lossless *and* how to get a small file, and
/// which of those they wanted is not knowable from the format they asked for.
pub(crate) fn alternatives(refused: Container, spec: &Spec) -> String {
    let mut lossless = Vec::new();
    let mut lossy = Vec::new();
    for other in Container::ALL {
        if other.name() == refused.name() || !carries(other, spec) {
            continue;
        }
        if other.is_lossy() {
            lossy.push(other.name());
        } else {
            lossless.push(other.name());
        }
    }
    match (lossless.is_empty(), lossy.is_empty()) {
        (false, false) => format!(
            "Export this one as {} to keep it lossless, or as {} for a smaller file.",
            join(&lossless),
            join(&lossy)
        ),
        (false, true) => format!("Export this one as {}.", join(&lossless)),
        // Reachable, and the reason this arm says so out loud: a 192 kHz side
        // long enough to overflow RIFF has no lossless container left, because
        // `flacenc` stops at 96 kHz and WAV stops at four gibibytes.
        (true, false) => format!(
            "Export this one as {} - nothing VCW writes will take this capture \
             losslessly.",
            join(&lossy)
        ),
        (true, true) => "Nothing VCW writes will take this capture.".to_owned(),
    }
}

/// `a`, `a or b`, `a, b or c` - the way a person would read a list aloud.
fn join(names: &[&'static str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [most @ .., last] => format!("{} or {last}", most.join(", ")),
    }
}

impl Wav {
    /// The most audio this header can describe.
    ///
    /// RIFF counts everything after `RIFF<size>` in a 32-bit field, so the
    /// ceiling is four gibibytes less the header. An extensible header is 24
    /// bytes longer than a plain one, which is why this takes the header's own
    /// length rather than a constant.
    const fn ceiling(header_bytes: u64) -> u64 {
        u32::MAX as u64 - (header_bytes - 8)
    }

    /// Everything that has to be true before a WAV file is created.
    ///
    /// RIFF carries every format VCW records, so length is the only refusal.
    /// It is a real one: a 32-bit stereo side at 96 kHz passes four gibibytes
    /// in about an hour and a half, which is one long side.
    fn vet(spec: &Spec) -> Result<()> {
        if Self::carries(spec) {
            return Ok(());
        }
        Err(Error::TooLargeForWav {
            bytes: spec.wire_bytes(),
            ceiling: Self::ceiling(Self::header(spec).len() as u64),
            instead: alternatives(Container::Wav, spec),
        })
    }

    /// Whether RIFF can describe a file this long.
    ///
    /// Split out from `vet` so [`carries`] can ask without building a message,
    /// which is what keeps [`alternatives`] from recursing into the refusals it
    /// is writing the advice for.
    fn carries(spec: &Spec) -> bool {
        spec.wire_bytes() <= Self::ceiling(Self::header(spec).len() as u64)
    }

    /// Creates the file and writes a header with placeholder sizes.
    fn create(path: &Path, spec: Spec) -> Result<Self> {
        Self::vet(&spec)?;
        let header = Self::header(&spec);
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
        let ceiling = Self::ceiling(self.header_bytes);
        if self.data_bytes + self.scratch.len() as u64 > ceiling {
            return Err(Error::TooLargeForWav {
                bytes: self.data_bytes + self.scratch.len() as u64,
                ceiling,
                instead: alternatives(Container::Wav, &self.spec),
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
        match Self::why(spec) {
            None => Ok(()),
            Some(reason) => Err(Error::Unencodable {
                format: spec.format,
                container: Container::Flac.name(),
                why: format!("{reason} {}", alternatives(Container::Flac, spec)).into(),
            }),
        }
    }

    /// Why FLAC cannot carry this audio, or `None` if it can.
    ///
    /// The reason only. Where to go instead is [`alternatives`]'s job, and
    /// keeping the two apart is what stops a message naming a container that
    /// will refuse the same capture a moment later.
    fn why(spec: &Spec) -> Option<&'static str> {
        if spec.is_float() {
            return Some(
                "FLAC is an integer codec, and choosing how to dither 32-bit float \
                 down to integers is a decision about headroom that belongs to a \
                 person.",
            );
        }
        if spec.bits() > 24 {
            return Some(
                "the FLAC format allows 32-bit samples but flacenc 0.5.1 stops at 24, \
                 and narrowing 32 bits to 24 loses signal.",
            );
        }
        if spec.rate > 96_000 {
            return Some(
                "flacenc 0.5.1 refuses rates above 96 kHz, though the FLAC format \
                 allows up to 655350 Hz.",
            );
        }
        if spec.channels == 0 || spec.channels > 8 {
            return Some("FLAC carries between one and eight channels.");
        }
        None
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

    /// Every container whose own name appears in a refusal's text.
    ///
    /// Matched on the words a person reads, not on a structured field, because
    /// the words are what they will act on. "Ogg Vorbis" is matched before
    /// "Ogg" would be, and both spellings resolve to the same container.
    fn named_in(said: &str) -> Vec<Container> {
        let mut found = Vec::new();
        for (word, container) in [
            ("WAV", Container::Wav),
            ("FLAC", Container::Flac),
            ("MP3", Container::Mp3(Quality::High)),
            ("Ogg Vorbis", Container::OggVorbis(Quality::High)),
            ("Ogg", Container::OggVorbis(Quality::High)),
        ] {
            if said.contains(word) && !found.contains(&container) {
                found.push(container);
            }
        }
        found
    }

    /// `carries` and `vet` must never disagree.
    ///
    /// [`carries`] is the cheap predicate [`alternatives`] is built on, and it
    /// has to stay the same question `vet` asks or every refusal in the crate
    /// starts recommending containers that then refuse. They cannot be the same
    /// function - `vet` builds the message, and building the message calls
    /// `alternatives`, which calls `carries` - so this is the seam, and a seam
    /// gets a test.
    #[test]
    fn the_cheap_predicate_agrees_with_the_real_refusal() {
        for format in [
            StorageFormat::Int16,
            StorageFormat::Int24Packed,
            StorageFormat::Int24Padded,
            StorageFormat::Int32,
            StorageFormat::Float32,
        ] {
            for rate in [0, 44_100, 48_000, 96_000, 192_000, 400_000] {
                for frames in [0, 48_000, 2_000_000_000] {
                    let spec = spec(format, rate, frames);
                    for container in Container::ALL {
                        // `NoEncoder` is not an answer about the audio, so it
                        // counts as carrying - the same rule `carries` uses.
                        let vetted = matches!(
                            Writer::vet(container, &spec),
                            Ok(()) | Err(Error::NoEncoder { .. })
                        );
                        assert_eq!(
                            carries(container, &spec),
                            vetted,
                            "{container} at {rate} Hz, {format:?}, {frames} frame(s)"
                        );
                    }
                }
            }
        }
    }

    /// A refusal must send a person somewhere that works.
    ///
    /// The one test in this file that is about prose. It exists because the
    /// FLAC refusals said "Export this one as WAV" for a whole work package
    /// after Ogg Vorbis arrived and became an equally true answer, and nothing
    /// here noticed: the advice was a string literal, and a string literal
    /// agrees with whatever it said yesterday. A real 192 kHz rip found it.
    ///
    /// So the property, rather than the wording. For every spec some container
    /// refuses: each container the refusal names must itself accept that spec,
    /// and the refusal must name at least one. Add a fifth container and this
    /// starts failing on every message that should have mentioned it, which is
    /// the direction the failure wants to point.
    #[test]
    fn a_refusal_only_sends_a_person_to_a_container_that_takes_the_capture() {
        // Int32 and Float32 are what the FLAC refusals are about; the rates
        // span MPEG's ceiling, flacenc's and §8's. Not a matrix for its own
        // sake - every row here is a capture VCW can really produce.
        let mut checked = 0usize;
        for format in [
            StorageFormat::Int16,
            StorageFormat::Int24Packed,
            StorageFormat::Int24Padded,
            StorageFormat::Int32,
            StorageFormat::Float32,
        ] {
            for rate in [44_100, 48_000, 88_200, 96_000, 176_400, 192_000] {
                // A second or so, and then a side long enough to overflow a
                // 32-bit RIFF size. Both are real: the second is a 90-minute
                // unsplit side, which is what `--format wav` on a whole capture
                // asks for, and it is the only way to reach TooLargeForWav -
                // whose advice was wrong in exactly the way FLAC's was.
                for frames in [48_000, 2_000_000_000] {
                    let spec = spec(format, rate, frames);
                    for container in Container::ALL {
                        let Err(why) = Writer::vet(container, &spec) else {
                            continue;
                        };
                        // A build compiled without an encoder is not giving advice
                        // about the audio, so it is not this test's business.
                        if matches!(why, Error::NoEncoder { .. }) {
                            continue;
                        }
                        assert!(!carries(container, &spec), "{container} contradicts itself");
                        let said = why.to_string();
                        let offered = named_in(&said);

                        // Named and wrong is worse than not named: it costs a
                        // person a second refusal to find out.
                        for &other in &offered {
                            if other == container {
                                continue;
                            }
                            assert!(
                                carries(other, &spec),
                                "{format:?} at {rate} Hz: {container} says to use \
                             {other}, and {other} will not carry it either"
                            );
                        }

                        // And the half that the stale FLAC advice got past. "Export
                        // this one as WAV" was never *wrong* - WAV really does take
                        // a 192 kHz Int32 capture - it was incomplete, and an
                        // assertion that only checked the named container worked
                        // stayed green through it. A person refused a format wants
                        // both halves of the answer: how to keep it lossless, and
                        // how to make it small. So each group that has a working
                        // member has to be represented.
                        for lossy in [false, true] {
                            let group: Vec<_> = Container::ALL
                                .into_iter()
                                .filter(|&other| {
                                    other != container
                                        && other.is_lossy() == lossy
                                        && carries(other, &spec)
                                })
                                .collect();
                            if group.is_empty() {
                                continue;
                            }
                            assert!(
                                group.iter().any(|other| offered.contains(other)),
                                "{format:?} at {rate} Hz: {container} refuses it and \
                             names {offered:?}, but says nothing about {group:?}, \
                             which would carry it: {said}"
                            );
                        }
                        checked += 1;
                    }
                }
            }
        }
        // The loop above is only evidence if it found refusals to read. A
        // change that made every container accept everything would otherwise
        // pass this test by having nothing to say.
        assert!(checked >= 12, "only {checked} refusal(s) were examined");
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
        // Ogg Vorbis, and only Ogg Vorbis. This assertion used to require the
        // word "FLAC", which for this very spec - 192 kHz, 32-bit - is a
        // container that refuses it on both counts, so the test was holding the
        // wrong advice in place. The right answer here is that nothing lossless
        // will take it: WAV is out on length and FLAC on rate and depth.
        let said = err.to_string();
        assert!(said.contains("Ogg Vorbis"), "{said}");
        assert!(
            !said.contains("FLAC"),
            "FLAC refuses this spec too, so naming it costs a second attempt: {said}"
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
        assert_eq!(
            Container::from_extension("mp3"),
            Some(Container::Mp3(Quality::High)),
            "a bare extension carries no quality, so it gets the default"
        );
        assert_eq!(
            Container::from_extension(".OGA"),
            Some(Container::OggVorbis(Quality::High))
        );
        assert_eq!(Container::from_extension("opus"), None);
    }

    #[test]
    fn a_quality_is_only_attached_to_the_containers_it_means_something_to() {
        // `with_quality` is deliberately a no-op rather than a refusal, so that
        // a panel can hold one quality while the format changes under it and
        // nothing has to be cleared. The thing that must not happen is a
        // lossless container quietly acquiring a setting it does not have.
        assert_eq!(
            Container::Flac.with_quality(Quality::Compact),
            Container::Flac
        );
        assert_eq!(
            Container::Wav.with_quality(Quality::Compact),
            Container::Wav
        );
        assert_eq!(Container::Flac.quality(), None);
        assert!(!Container::Flac.is_lossy());

        assert_eq!(
            Container::Mp3(Quality::High).with_quality(Quality::Transparent),
            Container::Mp3(Quality::Transparent)
        );
        assert_eq!(
            Container::OggVorbis(Quality::High).quality(),
            Some(Quality::High)
        );
        assert!(Container::Mp3(Quality::High).is_lossy());
    }

    #[test]
    fn a_quality_round_trips_through_the_name_a_person_types() {
        // The name is what the CLI flag, the settings file and the panel's
        // select all carry, so a parse that disagreed with `name` would make a
        // saved setting unreadable by the thing that saved it.
        for quality in Quality::ALL {
            assert_eq!(Quality::parse(quality.name()), Some(quality));
        }
        assert_eq!(Quality::parse("  HIGH  "), Some(Quality::High));
        assert_eq!(Quality::parse("medium"), None);
        assert_eq!(Quality::default(), Quality::High);
    }

    #[test]
    fn what_a_container_calls_itself_includes_the_quality() {
        // What the CLI prints in a plan and what the event log records. A line
        // saying "MP3" alone cannot be checked against the file afterwards.
        assert_eq!(Container::Mp3(Quality::High).to_string(), "MP3 V2");
        assert_eq!(
            Container::OggVorbis(Quality::Transparent).to_string(),
            "Ogg Vorbis q8"
        );
        assert_eq!(Container::Flac.to_string(), "FLAC");
    }

    #[test]
    fn the_spellings_a_refusal_offers_are_the_ones_that_parse() {
        // `Container::spellings()` is pasted straight into the message a bad
        // `--format` gets, so every word in it has to come back out of
        // `from_extension`. Spelling a format a person cannot then type is a
        // worse failure than the original typo.
        let offered = Container::spellings();
        for word in offered.split(", ") {
            assert!(
                Container::from_extension(word).is_some(),
                "the refusal offers {word:?}, which does not parse: {offered}"
            );
        }
        assert_eq!(offered.split(", ").count(), Container::ALL.len());
    }

    #[test]
    fn a_build_without_an_encoder_refuses_at_plan_time_and_not_at_write_time() {
        // The claim the whole `vet` path exists for: whatever this build was
        // compiled with, asking for a container it cannot write is answered
        // before a file is created. With both features on there is nothing to
        // refuse, which is the case worth asserting the other way round.
        let dir = tempfile::tempdir().expect("tempdir");
        let layout = spec(StorageFormat::Int16, 44_100, 0);
        for container in Container::ALL {
            // What this build can actually write, named feature by feature -
            // which also asserts that `feature()` returns a name that exists.
            // A build with one of the two is a real configuration: the licence
            // reason to drop MP3 says nothing about Ogg.
            let available = match container.feature() {
                None => true,
                Some("mp3") => cfg!(feature = "mp3"),
                Some("ogg") => cfg!(feature = "ogg"),
                Some(other) => panic!("{container} names a feature nothing knows: {other:?}"),
            };
            // The same answer `compiled_in` gives, derived independently - it
            // is the accessor the About dialog's notices are filtered by, and
            // a cfg typo there would be invisible to anything else.
            assert_eq!(
                container.compiled_in(),
                available,
                "{container}: compiled_in disagrees with the cargo features"
            );
            let answer = Writer::vet(container, &layout);
            assert_eq!(answer.is_ok(), available, "{container}: {answer:?}");
            if let Err(why) = answer {
                assert!(matches!(why, Error::NoEncoder { .. }), "{why}");
            }
        }
        assert_eq!(
            std::fs::read_dir(dir.path()).expect("read_dir").count(),
            0,
            "vetting wrote something"
        );
    }
}
