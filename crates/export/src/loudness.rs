/*
 *  loudness.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  ReplayGain: the loudness of what was written, as a tag and never as a gain (§33).
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

//! ReplayGain: the loudness of what was written, as a tag and never as a gain (§33).
//!
//! **VCW measures and does not apply.** Every sample in an exported file is the
//! sample the capture held, put through whatever repairs and curve the operator
//! asked for and nothing else; what this module adds is four text tags saying
//! how loud the result came out. A player that honors them turns its own volume
//! down, which is reversible and is the player's decision. Baking the gain into
//! the samples would not be, and §9's bit-perfect claim would not survive it.
//!
//! The numbers are ReplayGain 2.0: gated integrated loudness to EBU R 128,
//! referenced to [`REFERENCE_LUFS`], with a true peak so a player applying the
//! gain can tell in advance that it would clip.
//!
//! An **album gain** is the gated loudness of the whole side played end to end,
//! not the mean of its tracks' gains - gating throws away quiet blocks, and a
//! quiet track contributes fewer surviving blocks than a loud one. [`album`]
//! gets that right by handing every track's block history to one gate, which is
//! what `ebur128`'s `loudness_global_multiple` is for.

use ebur128::{EbuR128, Mode};
use vcw_types::StorageFormat;

/// ReplayGain 2.0's reference level, in LUFS.
///
/// -18 LUFS, which is the 2.0 specification's figure and not ReplayGain 1.0's
/// 89 dB SPL pink-noise calibration. The two land about 5 dB apart on real
/// material, so writing a 1.0 number under a 2.0 name would make a library
/// mixing the two sound uneven in exactly the way the tag exists to prevent.
pub const REFERENCE_LUFS: f64 = -18.0;

/// One measurement: how far off reference it landed, and how close to clipping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gain {
    /// Decibels a player should add to reach [`REFERENCE_LUFS`].
    pub gain_db: f64,
    /// True peak as a linear amplitude, where 1.0 is full scale.
    pub peak: f64,
}

impl Gain {
    /// The gain as ReplayGain spells it: two decimals and a unit.
    #[must_use]
    pub fn gain_text(&self) -> String {
        format!("{:.2} dB", self.gain_db)
    }

    /// The peak as ReplayGain spells it: a bare linear amplitude.
    ///
    /// Six decimals, because a peak is compared against 1.0 and the difference
    /// between 0.999969 and 1.0 is the difference between a file a player can
    /// raise and one it cannot.
    #[must_use]
    pub fn peak_text(&self) -> String {
        format!("{:.6}", self.peak)
    }
}

/// Both halves of what goes in one file's tags.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayGain {
    /// This track on its own.
    pub track: Gain,
    /// The release the track belongs to, played end to end.
    pub album: Gain,
}

/// A meter fed the bytes as they are written.
///
/// Created from the [`crate::encoder::Spec`] the writer was opened with rather
/// than from the capture's layout, so what is measured is what the file holds:
/// after a fold to mono, after a narrowing to 16 bits, dither and all.
pub struct Meter {
    meter: EbuR128,
    format: StorageFormat,
    /// Reused across chunks so a long side does not allocate per read.
    samples: Vec<f32>,
}

impl Meter {
    /// A meter for audio of this shape, or `None` if `ebur128` will not take it.
    ///
    /// `None` rather than an error: a rate or a channel count outside the
    /// meter's range is a file VCW can still write perfectly well, and losing
    /// four tags is not a reason to refuse an export.
    #[must_use]
    pub fn new(channels: u16, rate: u32, format: StorageFormat) -> Option<Self> {
        let meter = EbuR128::new(u32::from(channels), rate, Mode::I | Mode::TRUE_PEAK).ok()?;
        Some(Self {
            meter,
            format,
            samples: Vec::new(),
        })
    }

    /// Measures one chunk of written bytes.
    ///
    /// Reading a stored sample as an `f32` to describe it is not the conversion
    /// D4 forbids: nothing here is written back, and the bytes this was handed
    /// have already gone to the file.
    pub fn add(&mut self, chunk: &[u8]) {
        let width = self.format.bytes_per_sample();
        self.samples.clear();
        self.samples.extend(
            (0..chunk.len() / width).filter_map(|index| self.format.decode_sample(chunk, index)),
        );
        // An error here means the chunk did not divide into whole frames, which
        // would be the writer's bug and not the meter's. Dropping the chunk
        // would quietly understate the loudness, so say so and keep the export.
        if let Err(error) = self.meter.add_frames_f32(&self.samples) {
            tracing::warn!(?error, "a chunk the loudness meter would not take");
        }
    }

    /// This track's gain, or `None` if there was nothing loud enough to gate in.
    ///
    /// Silence measures `-inf` LUFS, which would become an infinite gain. A
    /// player handed that would either ignore it or turn a locked groove into a
    /// full-scale noise, so a silent track gets no tag at all.
    #[must_use]
    pub fn gain(&self) -> Option<Gain> {
        gain(self.meter.loudness_global().ok()?, self.peak())
    }

    /// The loudest true peak on any channel.
    fn peak(&self) -> f64 {
        (0..self.meter.channels())
            .filter_map(|channel| self.meter.true_peak(channel).ok())
            .fold(0.0, f64::max)
    }
}

/// The gain for a set of tracks measured as one release.
///
/// `None` if the set is empty or every track in it was silent. An iterator
/// rather than a slice because the tracks that were measured are a subset of
/// the tracks that were written, and `Clone` because the gate and the peak are
/// two passes over the same set.
pub fn album<'a>(meters: impl Iterator<Item = &'a Meter> + Clone) -> Option<Gain> {
    let loudness = EbuR128::loudness_global_multiple(meters.clone().map(|m| &m.meter)).ok()?;
    let peak = meters.map(Meter::peak).fold(0.0, f64::max);
    gain(loudness, peak)
}

/// Reference minus measured, with the infinities the gate can produce refused.
fn gain(loudness: f64, peak: f64) -> Option<Gain> {
    loudness.is_finite().then_some(Gain {
        gain_db: REFERENCE_LUFS - loudness,
        peak,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1 kHz sine at the asked-for dBFS, as interleaved `Float32` bytes.
    fn sine(rate: u32, channels: u16, seconds: f64, dbfs: f64) -> Vec<u8> {
        let amplitude = 10f64.powf(dbfs / 20.0);
        let frames = (f64::from(rate) * seconds) as usize;
        let mut bytes = Vec::with_capacity(frames * usize::from(channels) * 4);
        for frame in 0..frames {
            let phase = std::f64::consts::TAU * 1000.0 * frame as f64 / f64::from(rate);
            let value = (amplitude * phase.sin()) as f32;
            for _ in 0..channels {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    /// EBU Tech 3341 case 1, which is also the one number that proves the
    /// reference level: a stereo 1 kHz sine at -23 dBFS measures -23.0 LUFS, so
    /// against a -18 LUFS reference it asks for exactly +5.00 dB.
    #[test]
    fn a_sine_at_minus_23_dbfs_asks_for_five_decibels() {
        let mut meter = Meter::new(2, 48_000, StorageFormat::Float32).expect("a stereo meter");
        meter.add(&sine(48_000, 2, 20.0, -23.0));
        let gain = meter.gain().expect("a sine is not silent");
        assert!(
            (gain.gain_db - 5.0).abs() < 0.1,
            "Tech 3341 case 1 wants +5.00 dB, got {}",
            gain.gain_text()
        );
        // -23 dBFS is 0.0708 linear, and a sine reaches its amplitude.
        assert!(
            (gain.peak - 0.0708).abs() < 0.001,
            "peak {}",
            gain.peak_text()
        );
    }

    /// The album gain is the gate over everything, not the mean of the tracks.
    ///
    /// A loud track and a quiet one, both 20 s: their gains differ by 20 dB, so
    /// a mean would land exactly between them. The gate does not, because the
    /// quiet track's blocks fall below the relative threshold the loud one sets
    /// and stop counting - which is the whole reason this is not an average.
    #[test]
    fn an_album_gain_is_gated_and_not_averaged() {
        let mut meters = Vec::new();
        for dbfs in [-23.0, -43.0] {
            let mut meter = Meter::new(2, 48_000, StorageFormat::Float32).expect("a meter");
            meter.add(&sine(48_000, 2, 20.0, dbfs));
            meters.push(meter);
        }
        let track = meters[0].gain().expect("the loud one");
        let album = album(meters.iter()).expect("two sines are not silence");
        let mean = (track.gain_db + meters[1].gain().expect("the quiet one").gain_db) / 2.0;
        assert!(
            (album.gain_db - track.gain_db).abs() < 0.1,
            "the quiet track should have gated out, leaving {}, got {}",
            track.gain_text(),
            album.gain_text()
        );
        assert!(
            (album.gain_db - mean).abs() > 5.0,
            "an average would have been {mean:.2} dB and that is the bug"
        );
        // Album peak is the loudest sample anywhere on the side.
        assert!((album.peak - track.peak).abs() < 1e-9);
    }

    /// Silence has no loudness, so it gets no tag rather than an infinite one.
    #[test]
    fn a_silent_track_is_refused_rather_than_given_an_infinite_gain() {
        let mut meter = Meter::new(2, 48_000, StorageFormat::Float32).expect("a meter");
        meter.add(&vec![0u8; 48_000 * 2 * 4]);
        assert!(meter.gain().is_none());
        assert!(album(std::iter::empty()).is_none());
    }

    /// The meter reads the format it was told it is reading.
    #[test]
    fn an_integer_export_measures_the_same_as_the_float_it_came_from() {
        let float = sine(48_000, 2, 20.0, -23.0);
        let mut narrowed = Vec::with_capacity(float.len() / 2);
        for index in 0..float.len() / 4 {
            let value = StorageFormat::Float32
                .decode_sample(&float, index)
                .expect("a sample");
            let mut bytes = [0u8; 2];
            assert!(StorageFormat::Int16.encode_sample(value, &mut bytes));
            narrowed.extend_from_slice(&bytes);
        }
        let mut meter = Meter::new(2, 48_000, StorageFormat::Int16).expect("a meter");
        meter.add(&narrowed);
        let gain = meter.gain().expect("not silent");
        assert!(
            (gain.gain_db - 5.0).abs() < 0.1,
            "16 bits is plenty to measure -23 dBFS, got {}",
            gain.gain_text()
        );
    }
}
