/*
 *  convert.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Turning a project's stored samples into what a particular output device will accept (§21).
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

//! Turning a project's stored samples into what a particular output device will
//! accept (§21).
//!
//! §9 is about capture, and it is absolute: the bytes the converter produced are
//! the bytes that get stored. Playback is the other direction and cannot be
//! absolute, because the device that plays a side is not always the device that
//! recorded it - a 24-bit capture on a card that only takes float has to be
//! converted by somebody. So this module exists to do it in *one* place, exactly,
//! and to describe what it did.
//!
//! Three rules:
//!
//! 1. **Identical is a memcpy.** When the stored format is the device's format
//!    and the channel counts agree, [`Conversion::apply`] copies. There is no
//!    "normalize to float and back" step to lose anything in, which is the usual
//!    way a playback path quietly stops being bit-perfect.
//! 2. **Widening is exact, narrowing is reported.** 16- and 24-bit samples widen
//!    into anything without losing a bit. Going the other way - a 32-bit capture
//!    on a 24-bit device - drops the low bits, and that is named as a loss rather
//!    than done silently.
//! 3. **It happens on the feeder thread.** The audio callback does not convert,
//!    because it does not do arithmetic at all. Chunks are filled with device
//!    bytes before they are queued.
//!
//! # Rate is not converted here, or anywhere
//!
//! There is no resampler in VCW, and playback does not pretend otherwise: a
//! capture is played at the rate it was recorded at or not at all
//! (`vcw_audio::playback::Error::RateUnavailable`). A resampler is a quality
//! decision with a corpus behind it, not a detail to slip into a playback path,
//! and the honest failure - "this device cannot do 192 kHz" - is more useful
//! than an unannounced conversion. Recorded here so nobody has to guess whether
//! it was forgotten, and as `docs/adr/0006-playback-rate-policy.md` so the
//! reasoning outlives the code.

use vcw_types::{SampleFormat, StorageFormat};

/// What has to happen to the stored samples, and what it costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    from: StorageFormat,
    to: SampleFormat,
    from_channels: u16,
    to_channels: u16,
    losses: Vec<String>,
}

impl Conversion {
    /// Works out the conversion between a capture and a stream.
    #[must_use]
    pub fn new(
        from: StorageFormat,
        from_channels: u16,
        to: SampleFormat,
        to_channels: u16,
    ) -> Self {
        let mut losses = Vec::new();

        let stored = bits(from);
        let played = match to {
            // A float's mantissa is 24 bits, so a 32-bit integer sample does not
            // survive the trip. Anything narrower does, exactly.
            SampleFormat::F32 => 24,
            other => other.bytes_per_sample() as u32 * 8,
        };
        if from == StorageFormat::Float32 && to != SampleFormat::F32 {
            losses.push(format!(
                "float samples are converted to {played}-bit integer, and anything \
                 above full scale clips"
            ));
        } else if stored > played {
            losses.push(format!(
                "{stored}-bit samples are truncated to {played} bits for this device"
            ));
        }

        if from_channels > to_channels {
            losses.push(format!(
                "the capture has {from_channels} channels and the device takes \
                 {to_channels}, so the rest are not played"
            ));
        }

        Self {
            from,
            to,
            from_channels,
            to_channels,
            losses,
        }
    }

    /// Whether this is a straight copy: nothing to convert, nothing to remap.
    ///
    /// The condition a playback path has to meet before it may be described as
    /// bit-perfect, and the reason the check lives beside the conversion rather
    /// than being re-derived by the caller.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.from_channels == self.to_channels && identical(self.from, self.to)
    }

    /// Whether a mono capture is being spread across the device's channels.
    ///
    /// Not a loss, and not silent either, so it is reported separately: playing
    /// a mono side down one speaker would look like a fault.
    #[must_use]
    pub const fn fans_out_mono(&self) -> bool {
        self.from_channels == 1 && self.to_channels > 1
    }

    /// Whether the device has channels the capture cannot fill.
    #[must_use]
    pub const fn pads_channels(&self) -> bool {
        self.to_channels > self.from_channels && self.from_channels > 1
    }

    /// Everything this conversion costs, one per line. Empty is the good case.
    #[must_use]
    pub fn losses(&self) -> &[String] {
        &self.losses
    }

    /// Bytes in one frame as stored.
    #[must_use]
    pub const fn from_frame_bytes(&self) -> usize {
        self.from.bytes_per_sample() * self.from_channels as usize
    }

    /// Bytes in one frame as the device wants it.
    #[must_use]
    pub const fn to_frame_bytes(&self) -> usize {
        self.to.bytes_per_sample() * self.to_channels as usize
    }

    /// How many whole frames of output `dst` could hold from `src`.
    #[must_use]
    pub const fn frames_for(&self, src: usize, dst: usize) -> usize {
        let from = self.from_frame_bytes();
        let to = self.to_frame_bytes();
        if from == 0 || to == 0 {
            return 0;
        }
        let by_src = src / from;
        let by_dst = dst / to;
        if by_src < by_dst { by_src } else { by_dst }
    }

    /// Converts whole frames from `src` into `dst` and returns how many.
    ///
    /// Stops at whole frames on both sides. A partial frame is never produced,
    /// for the reason a partial frame is never stored: it shears everything
    /// after it.
    pub fn apply(&self, src: &[u8], dst: &mut [u8]) -> usize {
        let frames = self.frames_for(src.len(), dst.len());
        if frames == 0 {
            return 0;
        }
        let from_frame = self.from_frame_bytes();
        let to_frame = self.to_frame_bytes();

        if self.is_identity() {
            let bytes = frames * to_frame;
            dst[..bytes].copy_from_slice(&src[..bytes]);
            return frames;
        }

        let from_width = self.from.bytes_per_sample();
        let to_width = self.to.bytes_per_sample();
        let mono = self.fans_out_mono();

        for frame in 0..frames {
            let in_at = frame * from_frame;
            let out_at = frame * to_frame;
            for channel in 0..self.to_channels as usize {
                let out = out_at + channel * to_width;
                let source = if mono { 0 } else { channel };
                if source >= self.from_channels as usize {
                    // A channel the capture cannot fill plays silence, which is
                    // all-zero bytes in every one of the four formats.
                    dst[out..out + to_width].fill(0);
                    continue;
                }
                let sample = &src[in_at + source * from_width..][..from_width];
                write(
                    self.to,
                    decode(self.from, sample),
                    &mut dst[out..out + to_width],
                );
            }
        }
        frames
    }
}

impl std::fmt::Display for Conversion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_identity() {
            return write!(
                f,
                "{:?} straight through, {} channel(s)",
                self.from, self.from_channels
            );
        }
        write!(
            f,
            "{:?} x{} to {:?} x{}",
            self.from, self.from_channels, self.to, self.to_channels
        )
    }
}

/// The device format a stored format would rather be played in.
///
/// Not a claim about any particular device - it is what playback's
/// configuration choice is aiming at, and what the device-free render path uses,
/// so that a render of an untouched capture is the stored bytes rearranged and
/// nothing more.
///
/// Every stored format has an exact answer. [`StorageFormat::Int24Padded`] maps
/// to [`SampleFormat::S24`] rather than [`SampleFormat::S32`]: both are
/// lossless, but the packed form is the same 24 bits in a narrower container,
/// where the 32-bit form would be the same bits shifted up. Neither loses a
/// sample; the packed one moves them less.
#[must_use]
pub const fn natural(from: StorageFormat) -> SampleFormat {
    match from {
        StorageFormat::Int16 => SampleFormat::S16,
        StorageFormat::Int24Packed | StorageFormat::Int24Padded => SampleFormat::S24,
        StorageFormat::Int32 => SampleFormat::S32,
        StorageFormat::Float32 => SampleFormat::F32,
    }
}

/// Whether a stored format and a stream format are the same bytes.
///
/// [`StorageFormat::Int24Padded`] is *not* the same bytes as
/// [`SampleFormat::S24`], which is three bytes wide: the padded form is what
/// CPAL hands us on this host and what Audacity writes, and turning it into
/// packed 24-bit is a real conversion even though no sample value changes.
const fn identical(from: StorageFormat, to: SampleFormat) -> bool {
    matches!(
        (from, to),
        (StorageFormat::Int16, SampleFormat::S16)
            | (StorageFormat::Int24Packed, SampleFormat::S24)
            | (StorageFormat::Int32, SampleFormat::S32)
            | (StorageFormat::Float32, SampleFormat::F32)
    )
}

/// How many bits of a sample a format actually carries.
const fn bits(format: StorageFormat) -> u32 {
    match format {
        StorageFormat::Int16 => 16,
        StorageFormat::Int24Packed | StorageFormat::Int24Padded => 24,
        StorageFormat::Int32 => 32,
        // A float carries more range than any of them and 24 bits of precision.
        StorageFormat::Float32 => 24,
    }
}

/// Full scale for an integer sample, as a float.
const FULL_SCALE: f32 = 2_147_483_648.0;

/// Decodes one stored sample, left-justified into an `i32`.
///
/// Left-justified rather than normalized to `f32`, because that is what makes
/// the integer paths exact: a 24-bit sample in the top 24 bits of an `i32` can
/// be widened or narrowed by a shift, with no rounding and no scaling factor.
/// [`StorageFormat::decode_sample`] is the other way round and is for display,
/// where a float is what the caller wants anyway.
fn decode(format: StorageFormat, bytes: &[u8]) -> i32 {
    match format {
        StorageFormat::Int16 => i32::from(i16::from_le_bytes([bytes[0], bytes[1]])) << 16,
        // Sign-extend by landing the three bytes in the top of the word, which
        // is also exactly where they need to end up.
        StorageFormat::Int24Packed => i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]),
        // Measured, not assumed: a padded 24-bit sample is an i32 holding a
        // value in +/-2^23, not a left-justified one. See
        // `StorageFormat::decode_sample`, which was checked against the AUP3
        // corpus - get it wrong and playback is 48 dB quiet.
        StorageFormat::Int24Padded => {
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]).wrapping_mul(256)
        }
        StorageFormat::Int32 => i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        StorageFormat::Float32 => {
            let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            // Clamped, because a float project can legitimately hold samples
            // past full scale and an integer stream cannot. Clipping is what a
            // DAC would do with them anyway; silently wrapping is not.
            let scaled = value * FULL_SCALE;
            if scaled >= FULL_SCALE {
                i32::MAX
            } else if scaled <= -FULL_SCALE {
                i32::MIN
            } else {
                scaled as i32
            }
        }
    }
}

/// Writes a left-justified sample out in the stream's format.
///
/// Narrowing truncates rather than rounds, and there is no dither: playback is
/// monitoring, not mastering, and §33's export path is where a width reduction
/// gets to be a considered decision rather than a shift.
fn write(to: SampleFormat, sample: i32, dst: &mut [u8]) {
    match to {
        SampleFormat::S16 => dst.copy_from_slice(&((sample >> 16) as i16).to_le_bytes()),
        SampleFormat::S24 => dst.copy_from_slice(&sample.to_le_bytes()[1..4]),
        SampleFormat::S32 => dst.copy_from_slice(&sample.to_le_bytes()),
        SampleFormat::F32 => dst.copy_from_slice(&(sample as f32 / FULL_SCALE).to_le_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample of one channel, as stored.
    fn stored(format: StorageFormat, value: i32) -> Vec<u8> {
        match format {
            StorageFormat::Int16 => ((value >> 16) as i16).to_le_bytes().to_vec(),
            StorageFormat::Int24Packed => value.to_le_bytes()[1..4].to_vec(),
            StorageFormat::Int24Padded => (value / 256).to_le_bytes().to_vec(),
            StorageFormat::Int32 => value.to_le_bytes().to_vec(),
            StorageFormat::Float32 => (value as f32 / FULL_SCALE).to_le_bytes().to_vec(),
        }
    }

    #[test]
    fn the_matching_formats_are_a_memcpy_and_the_padded_one_is_not() {
        // The distinction the honest-reporting rule turns on. Padded 24-bit is
        // the *same sample* as packed 24-bit and not the same bytes, so a device
        // that wants S24 is being converted even though nothing is lost.
        for (from, to) in [
            (StorageFormat::Int16, SampleFormat::S16),
            (StorageFormat::Int24Packed, SampleFormat::S24),
            (StorageFormat::Int32, SampleFormat::S32),
            (StorageFormat::Float32, SampleFormat::F32),
        ] {
            let c = Conversion::new(from, 2, to, 2);
            assert!(c.is_identity(), "{from:?} to {to:?} should be a copy");
            assert!(c.losses().is_empty());
        }
        let padded = Conversion::new(StorageFormat::Int24Padded, 2, SampleFormat::S24, 2);
        assert!(!padded.is_identity());
        assert!(
            padded.losses().is_empty(),
            "repacking 24-bit loses nothing, so it is not a loss"
        );
    }

    #[test]
    fn an_identity_conversion_moves_the_bytes_unaltered() {
        let c = Conversion::new(StorageFormat::Int32, 2, SampleFormat::S32, 2);
        let src: Vec<u8> = (0..64u8).collect();
        let mut dst = vec![0u8; 64];
        assert_eq!(c.apply(&src, &mut dst), 8);
        assert_eq!(dst, src);
    }

    #[test]
    fn widening_a_sample_loses_nothing_and_is_not_called_a_loss() {
        // 16- and 24-bit samples fit inside everything wider, exactly. The test
        // is on the value, not on the bytes: full scale has to stay full scale.
        for from in [
            StorageFormat::Int16,
            StorageFormat::Int24Packed,
            StorageFormat::Int24Padded,
        ] {
            for to in [SampleFormat::S32, SampleFormat::F32] {
                let c = Conversion::new(from, 1, to, 1);
                assert!(
                    c.losses().is_empty(),
                    "{from:?} to {to:?} was reported as lossy"
                );
                for value in [0, 1 << 24, -(1 << 24), i32::MIN, 0x7FFF_FF00] {
                    let src = stored(from, value);
                    let mut dst = vec![0u8; to.bytes_per_sample()];
                    assert_eq!(c.apply(&src, &mut dst), 1);
                    let back = match to {
                        SampleFormat::S32 => i32::from_le_bytes(dst.clone().try_into().unwrap()),
                        SampleFormat::F32 => {
                            let f = f32::from_le_bytes(dst.clone().try_into().unwrap());
                            (f * FULL_SCALE) as i32
                        }
                        _ => unreachable!(),
                    };
                    let expected =
                        i32::from_le_bytes(stored(StorageFormat::Int32, value).try_into().unwrap());
                    let width = bits(from);
                    let mask = !0i32 << (32 - width);
                    assert_eq!(
                        back & mask,
                        expected & mask,
                        "{from:?} to {to:?} altered {value}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_padded_sample_plays_at_the_level_it_was_recorded_at() {
        // The 48 dB bug. A padded 24-bit sample is an i32 in +/-2^23; read as a
        // left-justified 32-bit one it is 256 times too quiet, which is quiet
        // enough to look like a broken cartridge rather than a broken decoder.
        let c = Conversion::new(StorageFormat::Int24Padded, 1, SampleFormat::F32, 1);
        let full = 0x007F_FFFFi32.to_le_bytes();
        let mut dst = [0u8; 4];
        assert_eq!(c.apply(&full, &mut dst), 1);
        let played = f32::from_le_bytes(dst);
        assert!(
            played > 0.999,
            "full scale played back at {played}, not near 1.0"
        );
    }

    #[test]
    fn narrowing_truncates_and_says_so() {
        let c = Conversion::new(StorageFormat::Int32, 1, SampleFormat::S16, 1);
        assert_eq!(c.losses().len(), 1);
        assert!(c.losses()[0].contains("32-bit samples are truncated to 16 bits"));

        // The low bits go, and the high ones do not move.
        let src = 0x1234_5678i32.to_le_bytes();
        let mut dst = [0u8; 2];
        c.apply(&src, &mut dst);
        assert_eq!(i16::from_le_bytes(dst), 0x1234);
    }

    #[test]
    fn a_float_project_on_an_integer_device_clips_rather_than_wrapping() {
        // Float projects can hold samples past full scale; integers cannot. A
        // wrap turns the loudest moment of a side into the quietest, in the
        // opposite phase.
        let c = Conversion::new(StorageFormat::Float32, 1, SampleFormat::S32, 1);
        assert_eq!(c.losses().len(), 1);
        assert!(c.losses()[0].contains("clips"));

        for (value, expected) in [(1.5f32, i32::MAX), (-1.5, i32::MIN), (1.0, i32::MAX)] {
            let mut dst = [0u8; 4];
            c.apply(&value.to_le_bytes(), &mut dst);
            assert_eq!(i32::from_le_bytes(dst), expected, "{value} did not clip");
        }
        // And an ordinary sample is not clipped.
        let mut dst = [0u8; 4];
        c.apply(&0.5f32.to_le_bytes(), &mut dst);
        assert!((i32::from_le_bytes(dst) - (i32::MAX / 2)).abs() < 256);
    }

    #[test]
    fn a_mono_capture_is_played_on_every_channel() {
        // Down one speaker it would look like a fault, so mono fans out - and
        // that is reported as a change, not as a loss.
        let c = Conversion::new(StorageFormat::Int16, 1, SampleFormat::S16, 2);
        assert!(c.fans_out_mono());
        assert!(c.losses().is_empty());

        let src = [0x34u8, 0x12, 0x78, 0x56];
        let mut dst = vec![0u8; 8];
        assert_eq!(c.apply(&src, &mut dst), 2);
        assert_eq!(dst, [0x34, 0x12, 0x34, 0x12, 0x78, 0x56, 0x78, 0x56]);
    }

    #[test]
    fn a_device_with_channels_to_spare_plays_silence_in_them() {
        let c = Conversion::new(StorageFormat::Int16, 2, SampleFormat::S16, 4);
        assert!(c.pads_channels());
        assert!(!c.fans_out_mono());
        assert!(c.losses().is_empty());

        let src = [1u8, 2, 3, 4];
        let mut dst = vec![0xFFu8; 8];
        assert_eq!(c.apply(&src, &mut dst), 1);
        assert_eq!(dst, [1, 2, 3, 4, 0, 0, 0, 0]);
    }

    #[test]
    fn channels_the_device_cannot_take_are_dropped_and_reported() {
        // No summing. Mixing two channels into one is a decision about the
        // music; playback is not the place to make it quietly.
        let c = Conversion::new(StorageFormat::Int16, 2, SampleFormat::S16, 1);
        assert_eq!(c.losses().len(), 1);
        assert!(c.losses()[0].contains("not played"));

        let src = [1u8, 2, 3, 4];
        let mut dst = vec![0u8; 2];
        assert_eq!(c.apply(&src, &mut dst), 1);
        assert_eq!(dst, [1, 2]);
    }

    #[test]
    fn a_partial_frame_is_never_produced() {
        let c = Conversion::new(StorageFormat::Int24Padded, 2, SampleFormat::S24, 2);
        assert_eq!(c.from_frame_bytes(), 8);
        assert_eq!(c.to_frame_bytes(), 6);

        // Source short of a whole frame, and output short of a whole frame.
        let src = vec![0u8; 8 * 3 + 5];
        let mut dst = vec![0u8; 6 * 10];
        assert_eq!(c.apply(&src, &mut dst), 3);
        let mut tight = vec![0u8; 6 * 2 + 4];
        assert_eq!(c.apply(&src, &mut tight), 2);
        let mut none = vec![0u8; 5];
        assert_eq!(c.apply(&src, &mut none), 0);
    }

    #[test]
    fn every_pair_of_formats_converts_without_panicking() {
        // A conversion path is reachable from any project and any device, and a
        // panic on the feeder thread stops playback dead. Exhaustive because
        // there are only twenty of them.
        for from in StorageFormat::ALL {
            for to in [
                SampleFormat::S16,
                SampleFormat::S24,
                SampleFormat::S32,
                SampleFormat::F32,
            ] {
                for (in_ch, out_ch) in [(1u16, 1u16), (1, 2), (2, 1), (2, 2), (2, 4), (4, 2)] {
                    let c = Conversion::new(from, in_ch, to, out_ch);
                    let src = vec![0x5Au8; c.from_frame_bytes() * 7];
                    let mut dst = vec![0u8; c.to_frame_bytes() * 7];
                    assert_eq!(c.apply(&src, &mut dst), 7, "{c}");
                }
            }
        }
    }

    #[test]
    fn the_natural_format_never_loses_a_sample() {
        // The render path depends on this: if the format it picked for itself
        // were lossy, a rendered file would prove nothing about the capture.
        for from in [
            StorageFormat::Int16,
            StorageFormat::Int24Packed,
            StorageFormat::Int24Padded,
            StorageFormat::Int32,
            StorageFormat::Float32,
        ] {
            let conversion = Conversion::new(from, 2, natural(from), 2);
            assert!(
                conversion.losses().is_empty(),
                "{from:?} to {:?} loses: {:?}",
                natural(from),
                conversion.losses()
            );
        }
    }
}
