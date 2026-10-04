/*
 *  lossy.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  MP3 and Ogg Vorbis encoding, behind cargo features (D5).
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

//! MP3 and Ogg Vorbis encoding, behind cargo features (D5).
//!
//! §33 requires both as initial export formats, and neither has a credible
//! pure-Rust encoder: MP3 goes through `mp3lame-encoder` over a vendored
//! libmp3lame, Vorbis through `vorbis_rs` over a vendored aoTuV-patched
//! libvorbis. Both compile their C from source, so there is no system library
//! to find on any of the four targets.
//!
//! ## Why there is a feature at all
//!
//! Not size, and not build time. `mp3lame-encoder` declares **LGPL-3.0**,
//! which extends the relink obligation `THIRD-PARTY-NOTICES.md` already
//! records, and anyone redistributing VCW who cannot carry that obligation can
//! build with `--no-default-features --features ogg` and still have three of
//! the four formats. `vorbis_rs` is BSD-3-Clause and costs nothing; it is a
//! feature only so that the pair can be turned off together.
//!
//! D5 recorded both as LGPL. That was right about MP3 - though it said
//! LGPL-2.1 and the crate says LGPL-3.0 - and wrong about Ogg Vorbis, which is
//! BSD-3-Clause like every other piece of Xiph's work.
//!
//! [`crate::encoder::Container`] carries all four variants whatever the
//! features say, so a build without `mp3` still parses `--format mp3` and
//! refuses it with [`crate::Error::NoEncoder`]. The refusals in
//! `mp3_limits` are checked *before* that one, because MPEG's rate table is
//! true in every build and the feature flag is true only in this one.
//!
//! ## Everything here goes through `f32`
//!
//! The lossless writers copy stored bytes; these two cannot. libmp3lame and
//! libvorbis both want normalised floating point, so `fan_out` converts
//! every stored format to `f32` in ±1.0 and splits the interleaved frames into
//! one buffer per channel - planar is what `vorbis_analysis_buffer` hands back
//! and what LAME's `DualPcm` wants, so there is one conversion and two
//! consumers rather than two of each.
//!
//! Converting to float loses nothing a lossy codec would have kept, which is
//! why this is not the dither decision FLAC refuses to make: a `Float32`
//! capture has no FLAC path and does have an MP3 and an Ogg one, because here
//! the signal is being thrown away on purpose and with a published
//! psychoacoustic model rather than silently by a cast.
//!
//! ## What cannot be tested the way WAV is
//!
//! There is no bit-exactness claim to make. `tests/third_party.rs` checks
//! these two the only way a lossy file can be checked: the container decodes,
//! its rate and channel count and duration are the ones that went in, and a
//! tone put into the left channel comes back out of the left channel at the
//! frequency it went in at. That last one is the test that matters, because a
//! planar fan-out is exactly the kind of code that swaps two channels and
//! still produces a file every player will happily play.

use std::path::Path;

// Only the sample conversion needs this, and that is gated: a build with
// neither encoder has nothing to convert to.
#[cfg(any(feature = "mp3", feature = "ogg", test))]
use vcw_types::StorageFormat;

use crate::encoder::{Container, Quality, Spec, Writer};
use crate::error::{Error, Result};

/// Turns stored interleaved bytes into one `f32` buffer per channel.
///
/// `out` is reused across calls: a side is a few thousand of these, and
/// allocating two vectors per 64 KiB chunk would be the most expensive thing in
/// the loop. The per-channel vectors are cleared rather than dropped, so after
/// the first chunk this allocates nothing.
#[cfg(any(feature = "mp3", feature = "ogg", test))]
fn fan_out(spec: &Spec, stored: &[u8], out: &mut Vec<Vec<f32>>) {
    let channels = spec.channels as usize;
    out.resize_with(channels, Vec::new);
    let frames = stored.len() / spec.stored_frame_bytes().max(1);
    for channel in out.iter_mut() {
        channel.clear();
        channel.reserve(frames);
    }
    let width = spec.format.bytes_per_sample();
    for (index, sample) in stored.chunks_exact(width).enumerate() {
        out[index % channels].push(to_f32(spec.format, sample));
    }
}

/// One stored sample as a float in ±1.0.
///
/// Divided by the format's full negative scale and not by its largest positive
/// value, which is the convention every codec uses: a stored `-32768` becomes
/// exactly `-1.0` and the positive peak lands one step short of it. Dividing by
/// 32767 instead would put a full-scale negative sample past -1.0, where
/// libvorbis clips it.
///
/// `bytes` is exactly `format.bytes_per_sample()` long, which is what
/// [`fan_out`]'s `chunks_exact` guarantees, so the indices below cannot be out
/// of range.
#[cfg(any(feature = "mp3", feature = "ogg", test))]
fn to_f32(format: StorageFormat, bytes: &[u8]) -> f32 {
    match format {
        StorageFormat::Int16 => f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0,
        StorageFormat::Int24Packed => {
            // Three bytes sign-extended into four. The stored value is a
            // little-endian 24-bit two's-complement sample, so the top bit of
            // the third byte is the sign and the fourth byte is whatever that
            // bit says.
            let sign = if bytes[2] & 0x80 == 0 { 0x00 } else { 0xFF };
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], sign]) as f32 / 8_388_608.0
        }
        // Already sign-extended in the fourth byte, which is what makes the
        // padded form cheaper to read than the packed one.
        StorageFormat::Int24Padded => {
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 8_388_608.0
        }
        StorageFormat::Int32 => {
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 2_147_483_648.0
        }
        // Already normalised. A sample past ±1.0 is a capture that clipped
        // before VCW saw it, and both libraries clamp rather than wrap.
        StorageFormat::Float32 => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    }
}

/// Wraps a library's complaint with the container it came from.
#[cfg(any(feature = "mp3", feature = "ogg"))]
fn refused(container: Container, why: impl std::fmt::Display) -> Error {
    Error::Lossy {
        container: container.name(),
        why: why.to_string(),
    }
}

/// The audio MP3 can carry, whatever this build was compiled with.
///
/// Both limits are the format's and not the library's, which is why they are
/// checked even in a build that cannot write an MP3 at all: MPEG-1 Layer III
/// defines three sample rates, MPEG-2 three lower ones and MPEG-2.5 three lower
/// still, and nothing in the family carries more than two channels.
///
/// Resampling is the way past the rate limit and VCW does not do it, for the
/// reason FLAC does not dither: an anti-alias filter is a choice with an
/// audible result, and an exporter that picked one silently would be making
/// that choice on someone else's record. Where to go instead is not written
/// here - `alternatives` works it out from the capture, because the answer at
/// 192 kHz is not the answer at 96.
///
/// Returns the reason only, so `carries` can ask without building a message.
pub(crate) fn mp3_why(spec: &Spec) -> Option<String> {
    /// Every sample rate MPEG-1, MPEG-2 and MPEG-2.5 Layer III define.
    const RATES: [u32; 9] = [
        8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000,
    ];

    if spec.channels == 0 || spec.channels > 2 {
        return Some(format!(
            "this capture has {} channels, and MP3 carries one or two.",
            spec.channels
        ));
    }
    if !RATES.contains(&spec.rate) {
        return Some(format!(
            "this capture is at {} Hz, and MP3 carries 32, 44.1 or 48 kHz and the \
             lower MPEG-2 rates, nothing above 48 kHz. Resampling a high-rate \
             capture means choosing an anti-alias filter, which is a decision about \
             the sound that belongs to a person.",
            spec.rate
        ));
    }
    None
}

/// The audio Ogg Vorbis can carry, whatever this build was compiled with.
///
/// Almost everything: libvorbis takes any rate VCW records and the Ogg channel
/// mapping goes to 255, so this refuses only the impossible. It is the lossy
/// container for a 192 kHz capture, and - with WAV - one of only two that will
/// take a `Float32` one.
///
/// Returns the reason only, so `carries` can ask without building a message.
pub(crate) fn ogg_why(spec: &Spec) -> Option<String> {
    if spec.channels == 0 || spec.channels > 255 {
        return Some(format!(
            "this capture has {} channels, and an Ogg Vorbis stream carries between \
             one and 255.",
            spec.channels
        ));
    }
    if spec.rate == 0 || spec.rate > 200_000 {
        return Some(
            "libvorbis encodes rates up to 200 kHz, which is above everything §8 \
             allows VCW to capture."
                .to_owned(),
        );
    }
    None
}

#[cfg(feature = "mp3")]
mod lame {
    //! The MP3 writer. Needs the `mp3` feature.

    use std::fs::File;
    use std::io::{Seek, SeekFrom, Write};
    use std::path::Path;

    use mp3lame_encoder::{
        Builder, DualPcm, FlushGap, MonoPcm, Quality as Effort, VbrMode, max_required_buffer_size,
    };

    use super::{Container, Error, Quality, Result, Spec, fan_out, refused};

    /// What `refused` is given for every failure in here.
    const MP3: Container = Container::Mp3(Quality::High);

    /// A streaming MP3 writer.
    ///
    /// Variable bitrate throughout, in LAME's `mtrh` mode, which is what
    /// `--vbr-new` has meant since 3.98 and is both faster and better than the
    /// old one. The algorithmic effort is pinned at `-q 2`: LAME's own
    /// documentation says 0 and 1 are not worth their time, and a vinyl side is
    /// forty minutes of audio rather than four.
    pub struct Mp3 {
        file: File,
        spec: Spec,
        encoder: mp3lame_encoder::Encoder,
        /// One `f32` buffer per channel, reused for every chunk.
        planar: Vec<Vec<f32>>,
        /// Encoded bytes from one call, reused for every chunk.
        encoded: Vec<u8>,
        bytes: u64,
    }

    impl std::fmt::Debug for Mp3 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // `mp3lame_encoder::Encoder` is a pointer into LAME's own state and
            // is not `Debug`; a dump of it would help nobody anyway.
            f.debug_struct("Mp3")
                .field("spec", &self.spec)
                .field("bytes", &self.bytes)
                .finish_non_exhaustive()
        }
    }

    impl Mp3 {
        /// Builds the encoder and creates the file.
        ///
        /// Nothing is written here, unlike the lossless writers: LAME emits the
        /// Xing/LAME header as a placeholder frame inside the output of the
        /// *first* encode call, which is the same patch-it-at-the-end shape the
        /// other two writers use and one the library rather than we arrange.
        pub(super) fn create(path: &Path, spec: Spec, quality: Quality) -> Result<Self> {
            super::mp3_limits(&spec)?;

            let mut builder =
                Builder::new().ok_or_else(|| refused(MP3, "LAME would not allocate an encoder"))?;
            let build = |why: mp3lame_encoder::BuildError| refused(MP3, why);
            builder.set_sample_rate(spec.rate).map_err(build)?;
            #[allow(clippy::cast_possible_truncation)] // 1 or 2, per `mp3_limits`
            builder
                .set_num_channels(spec.channels as u8)
                .map_err(build)?;
            builder.set_quality(Effort::NearBest).map_err(build)?;
            builder.set_vbr_mode(VbrMode::Mtrh).map_err(build)?;
            builder.set_vbr_quality(effort(quality)).map_err(build)?;
            // The Xing/LAME header. Without it a VBR file's length and seek
            // positions are extrapolated from the first frame's bitrate, which
            // for VBR is wrong: a forty-minute side shows up as twenty-six and
            // dragging the scrubber lands somewhere else.
            builder.set_to_write_vbr_tag(true).map_err(build)?;

            let encoder = builder.build().map_err(build)?;
            Ok(Self {
                file: File::create(path)?,
                spec,
                encoder,
                planar: Vec::new(),
                encoded: Vec::new(),
                bytes: 0,
            })
        }

        /// Encodes interleaved frames and appends whatever came out.
        pub fn write(&mut self, stored: &[u8]) -> Result<()> {
            let frame = self.spec.stored_frame_bytes();
            if frame == 0 || !stored.len().is_multiple_of(frame) {
                return Err(Error::Partial {
                    bytes: stored.len(),
                    frame_bytes: frame,
                });
            }
            if stored.is_empty() {
                return Ok(());
            }

            fan_out(&self.spec, stored, &mut self.planar);
            self.encoded.clear();
            self.encoded
                .reserve(max_required_buffer_size(stored.len() / frame));

            // `InterleavedPcm` divides its slice by two whatever the channel
            // count says, so it is unusable for mono. Planar costs nothing here
            // because `fan_out` is already planar for Ogg's sake.
            let written = match self.planar.as_slice() {
                [mono] => self
                    .encoder
                    .encode_to_vec(MonoPcm(mono.as_slice()), &mut self.encoded),
                [left, right] => self.encoder.encode_to_vec(
                    DualPcm {
                        left: left.as_slice(),
                        right: right.as_slice(),
                    },
                    &mut self.encoded,
                ),
                // `mp3_limits` refused anything else before the file was made.
                channels => {
                    return Err(refused(
                        MP3,
                        format!("{} channels reached the encoder", channels.len()),
                    ));
                }
            }
            .map_err(|why| refused(MP3, why))?;

            self.file.write_all(&self.encoded)?;
            self.bytes += written as u64;
            Ok(())
        }

        /// Flushes the last frame, then fills in the header LAME reserved.
        pub fn finish(mut self) -> Result<u64> {
            self.encoded.clear();
            self.encoded.reserve(max_required_buffer_size(0));
            let written = self
                .encoder
                .flush_to_vec::<FlushGap>(&mut self.encoded)
                .map_err(|why| refused(MP3, why))?;
            self.file.write_all(&self.encoded)?;
            self.bytes += written as u64;

            // Over the placeholder, not appended: `lame_get_lametag_frame`
            // returns exactly the number of bytes LAME reserved at the start of
            // the stream, which is what `lame_mp3_tags_fid` relies on when it
            // seeks to zero and writes. The file does not change length, so
            // `self.bytes` is still its size.
            //
            // lofty prepends an ID3v2 tag afterwards and that is fine: the Xing
            // table is read relative to the first audio frame, which is how
            // every tagger in existence has always treated it.
            self.encoded.clear();
            self.encoded.reserve(self.encoder.lame_tag_size());
            if self
                .encoder
                .lame_tag_encode_to_vec(&mut self.encoded)
                .is_some()
            {
                self.file.seek(SeekFrom::Start(0))?;
                self.file.write_all(&self.encoded)?;
            }
            self.file.flush()?;
            Ok(self.bytes)
        }
    }

    /// LAME's `-V` number for a quality level.
    ///
    /// Exhaustive on purpose. `Quality` is `#[non_exhaustive]` to the outside
    /// world but not in here, so adding a level without deciding what it sounds
    /// like breaks this build rather than defaulting quietly - and a level with
    /// no considered bitrate is not a level.
    const fn effort(quality: Quality) -> Effort {
        match quality {
            Quality::Transparent => Effort::Best,
            Quality::High => Effort::NearBest,
            Quality::Compact => Effort::Good,
        }
    }
}

#[cfg(feature = "ogg")]
mod xiph {
    //! The Ogg Vorbis writer. Needs the `ogg` feature.

    use std::fs::File;
    use std::io::Write;
    use std::num::{NonZeroU8, NonZeroU32};
    use std::path::Path;

    use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoder, VorbisEncoderBuilder};

    use super::{Container, Error, Quality, Result, Spec, fan_out, refused};

    /// What `refused` is given for every failure in here.
    const OGG: Container = Container::OggVorbis(Quality::High);

    /// A streaming Ogg Vorbis writer.
    ///
    /// Quality-managed VBR, which is `oggenc -q n` and the mode Xiph recommends:
    /// the target is a perceptual quality rather than a bitrate, so a quiet
    /// passage costs less and a loud one costs more without anyone having to
    /// predict which is which.
    pub struct Ogg {
        encoder: VorbisEncoder<File>,
        spec: Spec,
        /// One `f32` buffer per channel, reused for every chunk. Planar is what
        /// `vorbis_analysis_buffer` hands back, so this is the native shape.
        planar: Vec<Vec<f32>>,
    }

    impl std::fmt::Debug for Ogg {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // `VorbisEncoder` holds libvorbis state and is not `Debug`.
            f.debug_struct("Ogg")
                .field("spec", &self.spec)
                .finish_non_exhaustive()
        }
    }

    impl Ogg {
        /// Creates the file and writes the three Vorbis header packets.
        ///
        /// The comment header goes out empty and lofty rewrites it afterwards,
        /// which costs a rewrite of the whole file. `vorbis_rs` can take
        /// comments at build time, and using that would mean a second tag
        /// implementation living next to `tagging::mapped` with its own idea of
        /// which fields exist - one tagger for all four containers is worth
        /// more than one file rewrite per track.
        pub(super) fn create(path: &Path, spec: Spec, quality: Quality) -> Result<Self> {
            super::ogg_limits(&spec)?;

            let rate = NonZeroU32::new(spec.rate)
                .ok_or_else(|| refused(OGG, "a stream needs a sample rate"))?;
            #[allow(clippy::cast_possible_truncation)] // 1..=255, per `ogg_why`
            let channels = NonZeroU8::new(spec.channels as u8)
                .ok_or_else(|| refused(OGG, "a stream needs at least one channel"))?;

            let file = File::create(path)?;
            let mut builder =
                VorbisEncoderBuilder::new(rate, channels, file).map_err(|why| refused(OGG, why))?;
            builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
                target_quality: target(quality),
            });
            let encoder = builder.build().map_err(|why| refused(OGG, why))?;

            Ok(Self {
                encoder,
                spec,
                planar: Vec::new(),
            })
        }

        /// Encodes interleaved frames.
        pub fn write(&mut self, stored: &[u8]) -> Result<()> {
            let frame = self.spec.stored_frame_bytes();
            if frame == 0 || !stored.len().is_multiple_of(frame) {
                return Err(Error::Partial {
                    bytes: stored.len(),
                    frame_bytes: frame,
                });
            }
            // Not a no-op and not harmless: `encode_audio_block` passes the
            // sample count straight to `vorbis_analysis_wrote`, and zero there
            // is libvorbis's end-of-stream signal. An empty write would close
            // the stream and every write after it would fail.
            if stored.is_empty() {
                return Ok(());
            }

            fan_out(&self.spec, stored, &mut self.planar);
            self.encoder
                .encode_audio_block(&self.planar)
                .map_err(|why| refused(OGG, why))
        }

        /// Closes the stream and returns the file's size.
        pub fn finish(self) -> Result<u64> {
            let mut file = self.encoder.finish().map_err(|why| refused(OGG, why))?;
            file.flush()?;
            Ok(file.metadata()?.len())
        }
    }

    /// libvorbis's perceptual quality target for a level, which is `oggenc -q n`
    /// divided by ten.
    const fn target(quality: Quality) -> f32 {
        match quality {
            Quality::Transparent => 0.8,
            Quality::High => 0.6,
            Quality::Compact => 0.3,
        }
    }
}

#[cfg(feature = "mp3")]
pub use lame::Mp3;
#[cfg(feature = "ogg")]
pub use xiph::Ogg;

/// Opens an MP3 writer, or says why this build cannot.
///
/// # Errors
///
/// [`Error::Unencodable`] for audio MP3 cannot carry, [`Error::NoEncoder`]
/// without the `mp3` feature, [`Error::Lossy`] if LAME refuses the
/// configuration, or [`Error::Io`] if the file cannot be made.
#[cfg(feature = "mp3")]
pub fn mp3(path: &Path, spec: Spec, quality: Quality) -> Result<Writer> {
    Mp3::create(path, spec, quality).map(|writer| Writer::Mp3(Box::new(writer)))
}

/// Says that this build has no MP3 encoder.
///
/// # Errors
///
/// Always. [`Error::Unencodable`] where MP3 could not have carried the audio
/// anyway, and [`Error::NoEncoder`] otherwise.
#[cfg(not(feature = "mp3"))]
pub fn mp3(_path: &Path, spec: Spec, _quality: Quality) -> Result<Writer> {
    vet_mp3(&spec).and_then(|()| Err(no_mp3()))
}

/// Opens an Ogg Vorbis writer, or says why this build cannot.
///
/// # Errors
///
/// [`Error::Unencodable`] for audio Vorbis cannot carry, [`Error::NoEncoder`]
/// without the `ogg` feature, [`Error::Lossy`] if libvorbis refuses the
/// configuration, or [`Error::Io`] if the file cannot be made.
#[cfg(feature = "ogg")]
pub fn ogg(path: &Path, spec: Spec, quality: Quality) -> Result<Writer> {
    Ogg::create(path, spec, quality).map(|writer| Writer::OggVorbis(Box::new(writer)))
}

/// Says that this build has no Ogg Vorbis encoder.
///
/// # Errors
///
/// Always, as [`mp3`]'s counterpart does.
#[cfg(not(feature = "ogg"))]
pub fn ogg(_path: &Path, spec: Spec, _quality: Quality) -> Result<Writer> {
    vet_ogg(&spec).and_then(|()| Err(no_ogg()))
}

/// Whether MP3 can carry this audio, as a refusal.
///
/// `mp3_why` plus the advice. Kept as its own name because it is what the
/// writer and the tests ask; `mp3_why` exists for `carries`, which must not
/// build a message.
fn mp3_limits(spec: &Spec) -> Result<()> {
    refuse(Container::Mp3(Quality::High), spec, mp3_why(spec))
}

/// Whether Ogg Vorbis can carry this audio, as a refusal.
fn ogg_limits(spec: &Spec) -> Result<()> {
    refuse(Container::OggVorbis(Quality::High), spec, ogg_why(spec))
}

/// A reason plus the advice that goes with it, or `Ok(())` for no reason.
fn refuse(container: Container, spec: &Spec, why: Option<String>) -> Result<()> {
    match why {
        None => Ok(()),
        Some(reason) => Err(Error::Unencodable {
            format: spec.format,
            container: container.name(),
            why: format!("{reason} {}", crate::encoder::alternatives(container, spec)).into(),
        }),
    }
}

/// Whether an MP3 of this spec could be written here.
///
/// # Errors
///
/// [`Error::Unencodable`] for audio MP3 cannot carry, and [`Error::NoEncoder`]
/// in a build without the `mp3` feature.
pub fn vet_mp3(spec: &Spec) -> Result<()> {
    mp3_limits(spec)?;
    #[cfg(not(feature = "mp3"))]
    return Err(no_mp3());
    #[cfg(feature = "mp3")]
    Ok(())
}

/// Whether an Ogg Vorbis file of this spec could be written here.
///
/// # Errors
///
/// [`Error::Unencodable`] for audio Vorbis cannot carry, and
/// [`Error::NoEncoder`] in a build without the `ogg` feature.
pub fn vet_ogg(spec: &Spec) -> Result<()> {
    ogg_limits(spec)?;
    #[cfg(not(feature = "ogg"))]
    return Err(no_ogg());
    #[cfg(feature = "ogg")]
    Ok(())
}

/// The refusal a build without the `mp3` feature gives.
#[cfg(not(feature = "mp3"))]
fn no_mp3() -> Error {
    Error::NoEncoder {
        container: Container::Mp3(Quality::High).name(),
        feature: "mp3",
    }
}

/// The refusal a build without the `ogg` feature gives.
#[cfg(not(feature = "ogg"))]
fn no_ogg() -> Error {
    Error::NoEncoder {
        container: Container::OggVorbis(Quality::High).name(),
        feature: "ogg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spec with the numbers a vinyl rip actually uses.
    fn spec(format: StorageFormat, rate: u32, channels: u16) -> Spec {
        Spec {
            rate,
            channels,
            format,
            frames: 0,
        }
    }

    #[test]
    fn full_scale_negative_is_exactly_minus_one_in_every_format() {
        // The convention every codec uses, and the reason the divisors are
        // powers of two rather than the largest positive value: dividing by
        // 32767 would put this sample past -1.0, where libvorbis clips it.
        assert_eq!(to_f32(StorageFormat::Int16, &i16::MIN.to_le_bytes()), -1.0);
        assert_eq!(
            to_f32(StorageFormat::Int24Packed, &[0x00, 0x00, 0x80]),
            -1.0
        );
        assert_eq!(
            to_f32(StorageFormat::Int24Padded, &[0x00, 0x00, 0x80, 0xFF]),
            -1.0
        );
        assert_eq!(to_f32(StorageFormat::Int32, &i32::MIN.to_le_bytes()), -1.0);
        assert_eq!(
            to_f32(StorageFormat::Float32, &(-1.0f32).to_le_bytes()),
            -1.0
        );
    }

    #[test]
    fn the_positive_peak_lands_one_step_short_of_one() {
        // Not a rounding error to be fixed. A two's-complement format has one
        // more negative value than positive, so a full-scale positive sample is
        // genuinely a hair under unity, and a conversion that returned 1.0 here
        // would have scaled the negative side wrong to get it.
        let peak = to_f32(StorageFormat::Int16, &0x7FFFi16.to_le_bytes());
        assert!(peak < 1.0 && peak > 0.9999, "{peak}");
    }

    #[test]
    fn a_packed_24_bit_sample_is_sign_extended_and_not_zero_filled() {
        // The defect this guards is a one-line one: taking the three stored
        // bytes and leaving the fourth at zero turns every negative sample into
        // a large positive one, which is full-scale noise rather than audio.
        let quiet_negative = to_f32(StorageFormat::Int24Packed, &[0x00, 0x00, 0xFF]);
        assert!(
            quiet_negative < 0.0 && quiet_negative > -0.02,
            "a sample just below zero came out at {quiet_negative}"
        );
    }

    #[test]
    fn the_channels_do_not_swap() {
        // The test that matters for the fan-out, because a swap produces a file
        // every player will happily play. Left is positive, right is negative,
        // and nothing else in the file tells them apart.
        let layout = spec(StorageFormat::Int16, 44_100, 2);
        let mut stored = Vec::new();
        for frame in 0..4i16 {
            stored.extend_from_slice(&(100 * (frame + 1)).to_le_bytes());
            stored.extend_from_slice(&(-100 * (frame + 1)).to_le_bytes());
        }
        let mut out = Vec::new();
        fan_out(&layout, &stored, &mut out);

        assert_eq!(out.len(), 2, "one buffer per channel");
        assert_eq!(out[0].len(), 4);
        assert!(out[0].iter().all(|sample| *sample > 0.0), "{:?}", out[0]);
        assert!(out[1].iter().all(|sample| *sample < 0.0), "{:?}", out[1]);
    }

    #[test]
    fn a_reused_buffer_keeps_nothing_from_the_chunk_before() {
        // `fan_out` is called a few thousand times a side with one set of
        // vectors, so a missing `clear` would append every chunk to the last and
        // the file would be a quarter of an hour of audio growing quadratically.
        let layout = spec(StorageFormat::Int16, 44_100, 2);
        let mut out = Vec::new();
        fan_out(&layout, &[0u8; 4 * 100], &mut out);
        assert_eq!(out[0].len(), 100);
        fan_out(&layout, &[0u8; 4 * 7], &mut out);
        assert_eq!(out[0].len(), 7, "the previous chunk is still in there");
    }

    #[test]
    fn mp3_refuses_a_rate_mpeg_does_not_define() {
        // 96 kHz is a real capture - §8 allows 192 - and MPEG stops at 48.
        for rate in [88_200, 96_000, 176_400, 192_000] {
            let error = mp3_limits(&spec(StorageFormat::Int32, rate, 2))
                .expect_err(&format!("{rate} Hz was accepted"));
            assert!(
                matches!(error, Error::Unencodable { .. }),
                "{rate} Hz: {error}"
            );
            // Not a literal spelling of the advice. The old assertion pinned
            // the words "FLAC or WAV", which stayed green for a whole work
            // package after Ogg Vorbis arrived and became the right answer -
            // and a real 192 kHz rip found that out, not this test. So assert
            // the property instead: the refusal names Ogg, and Ogg genuinely
            // takes the very spec being refused.
            let said = error.to_string();
            assert!(
                said.contains("Ogg"),
                "the refusal has to name a container that works: {said}"
            );
            ogg_limits(&spec(StorageFormat::Int32, rate, 2))
                .unwrap_or_else(|why| panic!("MP3 sent them to Ogg and Ogg refuses it too: {why}"));
        }
        // And the three that work, which is what the corpus is mostly made of.
        for rate in [32_000, 44_100, 48_000] {
            mp3_limits(&spec(StorageFormat::Int32, rate, 2))
                .unwrap_or_else(|why| panic!("{rate} Hz was refused: {why}"));
        }
    }

    #[test]
    fn mp3_refuses_more_than_two_channels() {
        let error = mp3_limits(&spec(StorageFormat::Int16, 44_100, 4)).expect_err("4 channels");
        assert!(matches!(error, Error::Unencodable { .. }), "{error}");
        assert!(
            mp3_limits(&spec(StorageFormat::Int16, 44_100, 1)).is_ok(),
            "mono"
        );
    }

    #[test]
    fn ogg_takes_what_mp3_and_flac_both_refuse() {
        // The reason there are two lossy containers rather than one. A 192 kHz
        // float32 capture has no FLAC path (integer codec), no MP3 path (the
        // rate table) and a perfectly good Ogg one.
        ogg_limits(&spec(StorageFormat::Float32, 192_000, 2)).expect("192 kHz float32");
        ogg_limits(&spec(StorageFormat::Int16, 44_100, 8)).expect("eight channels");
    }

    #[test]
    fn neither_lossy_container_takes_a_capture_with_no_channels() {
        assert!(mp3_limits(&spec(StorageFormat::Int16, 44_100, 0)).is_err());
        assert!(ogg_limits(&spec(StorageFormat::Int16, 44_100, 0)).is_err());
    }

    /// What a build without the feature says, and what one with it says.
    ///
    /// Written as one test over both configurations rather than two cfg'd
    /// tests, because the interesting claim is that the limits are checked
    /// first either way: a 192 kHz capture gets the message about MPEG's rate
    /// table in every build, and the one about cargo features only in a build
    /// where that is the actual obstacle.
    #[test]
    fn the_limits_are_checked_before_the_feature_is() {
        let error = vet_mp3(&spec(StorageFormat::Int32, 192_000, 2)).expect_err("192 kHz");
        assert!(matches!(error, Error::Unencodable { .. }), "{error}");

        let answer = vet_mp3(&spec(StorageFormat::Int32, 44_100, 2));
        if cfg!(feature = "mp3") {
            answer.expect("44.1 kHz stereo, in a build with the encoder");
        } else {
            let error = answer.expect_err("a build without the encoder");
            assert!(matches!(error, Error::NoEncoder { .. }), "{error}");
        }
    }
}
