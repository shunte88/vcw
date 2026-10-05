/*
 *  probe.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Turning a backend's claims into a §8 capability matrix.
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

//! Turning a backend's claims into a §8 capability matrix.
//!
//! §8 says the UI shall expose only configurations the selected device supports,
//! which sounds like reading a list and is not. A `SupportedStreamConfigRange` is
//! an advertisement: S1 saw advertised rates fail at stream build with
//! `snd_pcm_hw_params_set_rate ... Invalid argument`. So capability here has three
//! states, not two - advertised, confirmed by actually opening the device, and
//! rejected - and nothing in the matrix claims to be more than it is.
//!
//! The matrix is built from a [`DirectionReport`], not from a live device, which
//! keeps the whole of the §8 filtering logic testable on a machine with no sound
//! card at all. Only [`confirm`] needs hardware.

use cpal::traits::DeviceTrait;
use serde::{Deserialize, Serialize};
use vcw_types::{STANDARD_RATES, SampleFormat, SampleRate, StorageFormat};

use crate::devices::{Direction, DirectionReport};

/// The §8 format a CPAL format corresponds to, or `None` for one §8 does not
/// cover.
///
/// `U8`, `I8`, `I64`, `U16`, `U24`, `U32`, `U64` and `F64` all return `None`. They
/// are real formats that real devices advertise, and a vinyl capture application
/// has no business recording in any of them.
pub const fn sample_format(f: cpal::SampleFormat) -> Option<SampleFormat> {
    match f {
        cpal::SampleFormat::I16 => Some(SampleFormat::S16),
        cpal::SampleFormat::I24 => Some(SampleFormat::S24),
        cpal::SampleFormat::I32 => Some(SampleFormat::S32),
        cpal::SampleFormat::F32 => Some(SampleFormat::F32),
        _ => None,
    }
}

/// The CPAL format to ask for, given a §8 format.
pub const fn to_cpal(f: SampleFormat) -> cpal::SampleFormat {
    match f {
        SampleFormat::S16 => cpal::SampleFormat::I16,
        SampleFormat::S24 => cpal::SampleFormat::I24,
        SampleFormat::S32 => cpal::SampleFormat::I32,
        SampleFormat::F32 => cpal::SampleFormat::F32,
    }
}

/// How bytes from this CPAL format are stored at rest.
///
/// The interesting case is 24-bit. `StorageFormat::native_for(SampleFormat::S24)`
/// is [`StorageFormat::Int24Packed`], three bytes to a sample - but **CPAL's `I24`
/// occupies four**, with 24 significant bits, which is exactly Audacity's
/// `0x00040001`. D4 stores the device's bytes verbatim, so a CPAL 24-bit capture
/// is [`StorageFormat::Int24Padded`] and re-uses Audacity's own code. Packing to
/// three bytes would be a conversion, and §9 forbids conversions on the capture
/// path however harmless they look.
pub const fn storage_format(f: cpal::SampleFormat) -> Option<StorageFormat> {
    match f {
        cpal::SampleFormat::I16 => Some(StorageFormat::Int16),
        cpal::SampleFormat::I24 => Some(StorageFormat::Int24Padded),
        cpal::SampleFormat::I32 => Some(StorageFormat::Int32),
        cpal::SampleFormat::F32 => Some(StorageFormat::Float32),
        _ => None,
    }
}

/// How much is known about one configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    /// The backend lists it. Nothing has been opened, so nothing is proven.
    Advertised,
    /// A stream was built with exactly this configuration and the backend
    /// accepted it. Still not a bit-perfection claim - that is WP-04's verifier.
    Confirmed,
    /// A stream was attempted and refused. The reason is on the entry.
    Rejected,
}

/// One cell of the §8 matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    /// Sample rate.
    pub rate: SampleRate,
    /// Sample representation.
    pub format: SampleFormat,
    /// Channel count.
    pub channels: u16,
    /// What is known about it.
    pub support: Support,
    /// Why it was rejected, or anything else worth carrying to the UI.
    pub note: Option<String>,
}

impl Capability {
    /// Whether this configuration is worth offering: not yet disproven.
    pub fn is_usable(&self) -> bool {
        self.support != Support::Rejected
    }

    /// The CPAL stream configuration this describes.
    pub fn stream_config(&self) -> cpal::StreamConfig {
        cpal::StreamConfig {
            channels: self.channels,
            sample_rate: self.rate.hz(),
            buffer_size: cpal::BufferSize::Default,
        }
    }
}

/// What a device will accept in one direction, restricted to §8.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Matrix {
    /// Which direction this describes.
    pub direction: Direction,
    /// Every §8 configuration the device advertises, rate-major.
    pub entries: Vec<Capability>,
}

impl Matrix {
    /// Builds the matrix from what the backend advertised.
    ///
    /// Restricted to §8's six rates and four formats. A device offering 8 kHz
    /// mono I16 - S1 met one - produces an **empty** matrix, which is the honest
    /// answer: there is no configuration here this application should record in.
    pub fn from_report(report: &DirectionReport, direction: Direction) -> Self {
        let mut entries = Vec::new();
        for rate in STANDARD_RATES {
            for config in &report.configs {
                let Some(format) = config.format else {
                    continue;
                };
                if !config.covers(rate) {
                    continue;
                }
                if entries.iter().any(|c: &Capability| {
                    c.rate == rate && c.format == format && c.channels == config.channels
                }) {
                    continue;
                }
                entries.push(Capability {
                    rate,
                    format,
                    channels: config.channels,
                    support: Support::Advertised,
                    note: None,
                });
            }
        }
        Self { direction, entries }
    }

    /// Drops every entry above a channel count.
    ///
    /// Confirmation opens the device once per entry, and an ALSA plug PCM
    /// advertises every channel count from 1 to 64 at every rate in every
    /// format. That is 1536 entries for a mono webcam, and measurement on the
    /// development host says the plug layer accepts one channel of them.
    /// Bounding the channel count before confirming turns thousands of device
    /// opens into a handful.
    ///
    /// The advertisement itself is left alone: what the backend claimed is a
    /// fact about the backend, and [`Matrix::from_report`] reports it faithfully.
    #[must_use]
    pub fn with_channels_at_most(mut self, max: u16) -> Self {
        self.entries.retain(|c| c.channels <= max);
        self
    }

    /// Whether the device advertises nothing this application can use.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry for an exact configuration.
    pub fn get(
        &self,
        rate: SampleRate,
        format: SampleFormat,
        channels: u16,
    ) -> Option<&Capability> {
        self.entries
            .iter()
            .find(|c| c.rate == rate && c.format == format && c.channels == channels)
    }

    /// Everything not yet disproven.
    pub fn usable(&self) -> impl Iterator<Item = &Capability> {
        self.entries.iter().filter(|c| c.is_usable())
    }

    /// The distinct usable rates, ascending.
    pub fn rates(&self) -> Vec<SampleRate> {
        let mut v: Vec<SampleRate> = self.usable().map(|c| c.rate).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The distinct usable channel counts, ascending.
    pub fn channel_counts(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self.usable().map(|c| c.channels).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Every usable format, in the preference order [`Matrix::suggest`] uses.
    pub fn formats(&self) -> Vec<SampleFormat> {
        let mut v: Vec<SampleFormat> = self.usable().map(|c| c.format).collect();
        v.sort_by_key(|f| format_rank(*f));
        v.dedup();
        v
    }

    /// The usable formats at a rate, in the preference order [`Matrix::suggest`]
    /// uses.
    pub fn formats_at(&self, rate: SampleRate) -> Vec<SampleFormat> {
        let mut v: Vec<SampleFormat> = self
            .usable()
            .filter(|c| c.rate == rate)
            .map(|c| c.format)
            .collect();
        v.sort_by_key(|f| format_rank(*f));
        v.dedup();
        v
    }

    /// A starting configuration, which the user may always override.
    ///
    /// §8: *"A sensible archival default is 24-bit / 96 kHz but shall not be
    /// imposed."* So 96 kHz if the device offers it, otherwise the highest rate it
    /// does; 24-bit if it offers that, otherwise 32-bit integer, then 16-bit, and
    /// float last. Float ranks last on purpose - it is the one representation that
    /// is a *rendering* of the converter's integer word rather than the word
    /// itself, and D4 stores the word.
    ///
    /// Stereo is preferred where offered; this is a vinyl application.
    ///
    /// Returns `None` for a device with no §8 configuration at all. The suggestion
    /// is [`Support::Advertised`] unless [`confirm_all`] has been run, so it is a
    /// starting point and not a promise.
    pub fn suggest(&self) -> Option<&Capability> {
        let rate = if self.rates().contains(&SampleRate(96_000)) {
            SampleRate(96_000)
        } else {
            *self.rates().last()?
        };
        self.usable()
            .filter(|c| c.rate == rate)
            .min_by_key(|c| (format_rank(c.format), channel_rank(c.channels)))
    }

    /// Replaces the support state of one entry.
    fn record(
        &mut self,
        rate: SampleRate,
        format: SampleFormat,
        channels: u16,
        support: Support,
        note: Option<String>,
    ) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|c| c.rate == rate && c.format == format && c.channels == channels)
        {
            entry.support = support;
            entry.note = note;
        }
    }
}

/// §8's archival preference, lowest is best. See [`Matrix::suggest`].
fn format_rank(f: SampleFormat) -> u8 {
    match f {
        SampleFormat::S24 => 0,
        SampleFormat::S32 => 1,
        SampleFormat::S16 => 2,
        SampleFormat::F32 => 3,
    }
}

/// Stereo first, then fewest channels. A 32-channel interface should not default
/// to recording 32 channels of a stereo turntable.
fn channel_rank(channels: u16) -> u16 {
    if channels == 2 { 0 } else { channels }
}

/// Opens the device with exactly this configuration and closes it again.
///
/// This is the only way to tell an advertisement from a capability, and it is
/// intrusive: it opens the device, so it will fail while something else holds it,
/// and on some backends it is audible as a click. Call it when arming, not while
/// drawing a settings dialogue.
///
/// Confirms that the backend **accepts** the configuration. It says nothing about
/// whether the hardware is really running it - that is the §9 question, and S1
/// found a backend reporting an honored request while the card ran something else
/// entirely. WP-04's per-platform verifier is what settles that.
///
/// # Errors
///
/// Whatever the backend said when it refused.
pub fn confirm(
    device: &cpal::Device,
    direction: Direction,
    capability: &Capability,
) -> Result<(), cpal::Error> {
    let config = capability.stream_config();
    let format = to_cpal(capability.format);
    let timeout = Some(std::time::Duration::from_secs(2));
    match direction {
        // Built and immediately dropped. `play()` is never called, so on every
        // backend this opens the device, negotiates, and closes it again without
        // a single callback.
        Direction::Input => {
            drop(device.build_input_stream_raw(config, format, |_, _| {}, |_| {}, timeout)?)
        }
        Direction::Output => {
            drop(device.build_output_stream_raw(config, format, |_, _| {}, |_| {}, timeout)?)
        }
    }
    Ok(())
}

/// Confirms every advertised entry, in place.
///
/// Opens the device once per entry, so a device advertising 24 combinations is
/// opened 24 times. Worth it before a long capture; not worth it on every UI
/// refresh.
pub fn confirm_all(device: &cpal::Device, matrix: &mut Matrix) {
    let direction = matrix.direction;
    let pending: Vec<Capability> = matrix.entries.clone();
    for capability in pending {
        let (support, note) = match confirm(device, direction, &capability) {
            Ok(()) => (Support::Confirmed, None),
            Err(e) => (Support::Rejected, Some(e.to_string())),
        };
        matrix.record(
            capability.rate,
            capability.format,
            capability.channels,
            support,
            note,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::ConfigRange;

    fn range(channels: u16, min: u32, max: u32, format: SampleFormat) -> ConfigRange {
        ConfigRange {
            channels,
            min_rate: min,
            max_rate: max,
            sample_format: format!("{format:?}"),
            format: Some(format),
            bytes_per_sample: format.bytes_per_sample(),
            buffer_frames: Some((64, 8192)),
        }
    }

    fn report(configs: Vec<ConfigRange>) -> DirectionReport {
        DirectionReport {
            supported: !configs.is_empty(),
            configs,
            default: None,
        }
    }

    #[test]
    fn cpal_24_bit_is_four_bytes_wide_and_stores_as_audacitys_padded_code() {
        // The trap this mapping exists to avoid: our own S24 is three bytes, CPAL's
        // I24 is four. Getting this wrong misreads every 24-bit block by a byte.
        assert_eq!(SampleFormat::S24.bytes_per_sample(), 3);
        assert_eq!(cpal::SampleFormat::I24.sample_size(), 4);
        assert_eq!(
            storage_format(cpal::SampleFormat::I24),
            Some(StorageFormat::Int24Padded)
        );
        assert_eq!(StorageFormat::Int24Padded.bytes_per_sample(), 4);
        assert!(StorageFormat::Int24Padded.is_audacity());
    }

    #[test]
    fn formats_outside_section_8_are_not_silently_promoted() {
        assert_eq!(sample_format(cpal::SampleFormat::U8), None);
        assert_eq!(sample_format(cpal::SampleFormat::F64), None);
        assert_eq!(storage_format(cpal::SampleFormat::I64), None);
        for f in [
            SampleFormat::S16,
            SampleFormat::S24,
            SampleFormat::S32,
            SampleFormat::F32,
        ] {
            assert_eq!(sample_format(to_cpal(f)), Some(f));
        }
    }

    #[test]
    fn a_range_expands_to_the_standard_rates_it_covers() {
        let m = Matrix::from_report(
            &report(vec![range(2, 44_100, 192_000, SampleFormat::S32)]),
            Direction::Input,
        );
        assert_eq!(m.rates(), STANDARD_RATES.to_vec());
        assert_eq!(m.entries.len(), 6);
        assert!(m.entries.iter().all(|c| c.support == Support::Advertised));
    }

    #[test]
    fn a_device_with_nothing_section_8_wants_produces_an_empty_matrix() {
        // The 8 kHz mono I16 webcam S1 met. Offering the user "8000 Hz" here would
        // be worse than offering nothing.
        let m = Matrix::from_report(
            &report(vec![range(1, 8_000, 8_000, SampleFormat::S16)]),
            Direction::Input,
        );
        assert!(m.is_empty());
        assert!(m.suggest().is_none());
    }

    #[test]
    fn the_suggestion_is_section_8s_archival_default_where_the_device_allows() {
        let m = Matrix::from_report(
            &report(vec![
                range(2, 44_100, 192_000, SampleFormat::S24),
                range(2, 44_100, 192_000, SampleFormat::S32),
                range(2, 44_100, 48_000, SampleFormat::F32),
            ]),
            Direction::Input,
        );
        let s = m.suggest().unwrap();
        assert_eq!(s.rate, SampleRate(96_000));
        assert_eq!(s.format, SampleFormat::S24);
        assert_eq!(s.channels, 2);
    }

    #[test]
    fn the_suggestion_falls_back_rather_than_imposing_96k() {
        let m = Matrix::from_report(
            &report(vec![range(2, 44_100, 48_000, SampleFormat::F32)]),
            Direction::Input,
        );
        let s = m.suggest().unwrap();
        assert_eq!(s.rate, SampleRate(48_000));
        // Float is all this device offers, so float it is - last in preference is
        // not the same as forbidden.
        assert_eq!(s.format, SampleFormat::F32);
    }

    #[test]
    fn stereo_wins_over_a_higher_channel_count() {
        let m = Matrix::from_report(
            &report(vec![
                range(2, 96_000, 96_000, SampleFormat::S24),
                range(8, 96_000, 96_000, SampleFormat::S24),
                range(1, 96_000, 96_000, SampleFormat::S24),
            ]),
            Direction::Input,
        );
        assert_eq!(m.suggest().unwrap().channels, 2);
        assert_eq!(m.channel_counts(), [1, 2, 8]);
    }

    /// The plug layer's advertisement, measured on the development host: a mono
    /// 8 kHz webcam reached through `plughw:` claims 6 rates x 4 formats x 64
    /// channels, and confirmation rejects all but one of them.
    #[test]
    fn a_plug_layers_channel_claim_is_bounded_before_confirming() {
        let m = Matrix::from_report(
            &report(
                (1..=64u16)
                    .flat_map(|ch| {
                        [
                            SampleFormat::S16,
                            SampleFormat::S24,
                            SampleFormat::S32,
                            SampleFormat::F32,
                        ]
                        .into_iter()
                        .map(move |f| range(ch, 44_100, 192_000, f))
                    })
                    .collect(),
            ),
            Direction::Input,
        );
        assert_eq!(m.entries.len(), 6 * 4 * 64);

        let bounded = m.with_channels_at_most(8);
        assert_eq!(bounded.entries.len(), 6 * 4 * 8);
        assert_eq!(bounded.channel_counts(), (1..=8).collect::<Vec<u16>>());
        // Bounding must not change what is recommended for a stereo turntable.
        assert_eq!(bounded.suggest().unwrap().channels, 2);
    }

    #[test]
    fn a_rejected_entry_stops_being_offered() {
        let mut m = Matrix::from_report(
            &report(vec![range(2, 44_100, 192_000, SampleFormat::S24)]),
            Direction::Input,
        );
        m.record(
            SampleRate(96_000),
            SampleFormat::S24,
            2,
            Support::Rejected,
            Some("Invalid argument (22)".to_owned()),
        );
        assert!(!m.rates().contains(&SampleRate(96_000)));
        assert_eq!(m.suggest().unwrap().rate, SampleRate(192_000));
        assert_eq!(
            m.get(SampleRate(96_000), SampleFormat::S24, 2)
                .unwrap()
                .note
                .as_deref(),
            Some("Invalid argument (22)")
        );
    }
}
