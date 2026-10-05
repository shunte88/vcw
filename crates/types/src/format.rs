/*
 *  format.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  PCM sample representation and the honesty rules that attach to it.
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

//! PCM sample representation and the honesty rules that attach to it.

use serde::{Deserialize, Serialize};

/// A PCM sample representation, stored verbatim as the device supplies it (D4).
///
/// §9 forbids format conversion on the capture path, so this is a tag traveling
/// beside the bytes rather than a conversion target. §8 requires all four variants
/// where the hardware permits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SampleFormat {
    /// 16-bit signed integer, little-endian.
    S16,
    /// 24-bit signed integer packed into 3 bytes, little-endian.
    S24,
    /// 32-bit signed integer, little-endian. Has no Audacity equivalent - see
    /// [`SampleFormat::audacity_code`].
    S32,
    /// 32-bit IEEE 754 float, little-endian.
    F32,
}

impl SampleFormat {
    /// Bytes occupied by one sample of one channel at rest.
    ///
    /// Note that [`SampleFormat::S24`] is 3 bytes here but 4 in an Audacity
    /// `sampleblocks` row, which pads it - see [`SampleFormat::audacity_bytes`].
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::S16 => 2,
            Self::S24 => 3,
            Self::S32 | Self::F32 => 4,
        }
    }

    /// The `sampleformat` code Audacity writes into an AUP3/AUP4 `sequence` element,
    /// or `None` for a format Audacity cannot represent.
    ///
    /// Measured from the corpus rather than taken from Audacity's source (S5). The
    /// `None` arm is exactly why D1 makes the `.vcw` schema a *superset* of AUP4's:
    /// §8 requires 32-bit integer capture and Audacity has no code for it.
    pub const fn audacity_code(self) -> Option<u32> {
        match self {
            Self::S16 => Some(0x0002_0001),
            Self::S24 => Some(0x0004_0001),
            Self::F32 => Some(0x0004_000F),
            Self::S32 => None,
        }
    }

    /// Bytes per sample in an Audacity `sampleblocks` payload, or `None` for a format
    /// Audacity cannot represent. Audacity pads 24-bit to 4 bytes; we do not.
    pub const fn audacity_bytes(self) -> Option<usize> {
        match self {
            Self::S16 => Some(2),
            Self::S24 | Self::F32 => Some(4),
            Self::S32 => None,
        }
    }

    /// Reads an Audacity `sampleformat` code, for the AUP3/AUP4 importer (WP-20).
    pub const fn from_audacity_code(code: u32) -> Option<Self> {
        match code {
            0x0002_0001 => Some(Self::S16),
            0x0004_0001 => Some(Self::S24),
            0x0004_000F => Some(Self::F32),
            _ => None,
        }
    }
}

/// How the capture stream asks the host to open the device (§9).
///
/// The mode is a *request*. What the hardware actually did is a separate,
/// OS-confirmed fact: §9 forbids claiming bit-perfect operation on the strength of
/// the API's own report, and S1 found CPAL 0.16 reporting a silent 8 kHz -> 48 kHz
/// upsample as an honored request. WP-04 carries the per-platform verifier that
/// settles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureMode {
    /// The OS mixer owns the device; conversion is likely and must be reported.
    Shared,
    /// The device's own default configuration, taken unmodified.
    Native,
    /// Exclusive hardware access where the platform offers it (WASAPI exclusive,
    /// ALSA `hw:`); the only mode in which bit-perfection is plausible.
    Exclusive,
    /// No stream was opened at all: the audio came out of another application's
    /// project file (§12).
    ///
    /// Here because the other three are all answers to "how did VCW ask for this
    /// device", and an import has no device to have asked about. Recording it as
    /// `Shared` or `Native` would be the sort of plausible-looking default that
    /// later gets read as a measurement, and the capture row is the one place a
    /// reader looks to find out where the audio came from.
    Imported,
}

impl CaptureMode {
    /// The `capture_mode` column's spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Native => "native",
            Self::Exclusive => "exclusive",
            Self::Imported => "imported",
        }
    }

    /// Reads the `capture_mode` column. `None` for a value no version of VCW wrote.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "shared" => Some(Self::Shared),
            "native" => Some(Self::Native),
            "exclusive" => Some(Self::Exclusive),
            "imported" => Some(Self::Imported),
            _ => None,
        }
    }

    /// Whether this mode *could* deliver untouched samples, on a platform that
    /// honors it.
    ///
    /// [`CaptureMode::Shared`] never can: the OS mixer owns the device and
    /// conversion is the mixer's job. The other two might, which is a long way
    /// from saying they did - §9 settles that against the operating system, not
    /// against the mode that was asked for.
    ///
    /// [`CaptureMode::Imported`] cannot either, and for a different reason: the
    /// samples may well be untouched, but whoever opened the device was not us
    /// and left no record of how. Unknown provenance is reported as "no" rather
    /// than inherited.
    pub const fn could_be_bit_perfect(self) -> bool {
        matches!(self, Self::Native | Self::Exclusive)
    }

    /// Every mode a device can be asked for, widest access first, for
    /// negotiation that falls back.
    ///
    /// [`CaptureMode::Imported`] is deliberately absent: it is not a request
    /// anyone can make of a device, so a negotiator that met it in this list
    /// would try to open a stream in a mode that does not exist.
    pub const ALL: [Self; 3] = [Self::Exclusive, Self::Native, Self::Shared];
}

/// What playback equalization the hardware upstream of the capture had already
/// applied (§51).
///
/// Here because applying a curve later is only meaningful if what was captured is
/// known. A flat transfer wants a curve applied; a capture that has already been
/// through a RIAA phono stage wants RIAA *undone* before another curve goes on,
/// which amplifies noise and is not a thing to do on a guess.
///
/// The field cannot be recovered afterwards, which is why §51 requires it from the
/// first capture-capable release rather than from the Phase 3 processing chain that
/// will consume it: a rip whose provenance was never recorded is one nobody can
/// correctly re-equalize, however good the filter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum CaptureEq {
    /// No equalization was applied upstream: a flat transfer, from a flat preamp
    /// or a phono stage with its curve defeated. The case a curve can simply be
    /// applied to.
    Flat,
    /// The RIAA curve was applied upstream, by an ordinary phono stage. Playing it
    /// back as-is is correct for anything cut to RIAA, and anything else requires
    /// inverting this first.
    Riaa,
    /// Nobody said, and VCW did not get to see. **The default**, and deliberately
    /// not `Riaa`: a RIAA phono stage is overwhelmingly the common case, and that
    /// is exactly what makes guessing it dangerous - a plausible default here would
    /// be read later as a measurement by a processing chain that cannot tell the
    /// difference. §51 requires an operator to state the provenance rather than
    /// have one assumed.
    ///
    /// What every imported project gets (§12): the audio came out of another
    /// application, which recorded nothing about the signal chain ahead of it.
    #[default]
    Unknown,
}

impl CaptureEq {
    /// The `capture_eq` column's spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Riaa => "riaa",
            Self::Unknown => "unknown",
        }
    }

    /// Reads the `capture_eq` column. `None` for a value no version of VCW wrote.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "flat" => Some(Self::Flat),
            "riaa" => Some(Self::Riaa),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    /// Whether a curve can be applied to this capture without first undoing one.
    ///
    /// False for [`CaptureEq::Unknown`] as well as for [`CaptureEq::Riaa`], for the
    /// same reason [`CaptureMode::could_be_bit_perfect`] is false for an import:
    /// unknown provenance is reported as "no" rather than inherited.
    pub const fn is_flat(self) -> bool {
        matches!(self, Self::Flat)
    }

    /// Every value, for a UI that offers the choice and a test that covers it.
    pub const ALL: [Self; 3] = [Self::Flat, Self::Riaa, Self::Unknown];
}

/// How a block of samples is laid out on disk, and the `sampleformat` code that
/// records it.
///
/// Distinct from [`SampleFormat`], which says what the samples *are*. The same
/// logical format can be stored two ways: Audacity pads 24-bit to four bytes, and
/// we do not. A block therefore carries a storage code, not a sample format, and
/// the codes are a superset of Audacity's three.
///
/// The encoding is Audacity's - `(bytes_per_sample << 16) | type_code` - kept so
/// that imported blocks need no rewriting and stay byte-identical to the source
/// project. Type code 1 is integer and 15 is float in Audacity's space; the two
/// formats it has no code for take type code 2, which Audacity never emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StorageFormat {
    /// 16-bit integer, 2 bytes. Audacity's code, identical meaning.
    Int16,
    /// 24-bit integer packed into 3 bytes. Ours: this is what the device hands us
    /// and D4 stores it verbatim, where Audacity would pad it.
    Int24Packed,
    /// 24-bit integer padded into 4 bytes. Audacity's code and layout, produced
    /// only by import.
    Int24Padded,
    /// 32-bit integer, 4 bytes. Ours; §8 requires it and Audacity has no code for
    /// it, which is the concrete reason D1 is a superset rather than a clone.
    Int32,
    /// 32-bit float, 4 bytes. Audacity's code, identical meaning.
    Float32,
}

impl StorageFormat {
    /// The `sampleformat` code written into a block row.
    pub const fn code(self) -> u32 {
        match self {
            Self::Int16 => 0x0002_0001,
            Self::Int24Packed => 0x0003_0002,
            Self::Int24Padded => 0x0004_0001,
            Self::Int32 => 0x0004_0002,
            Self::Float32 => 0x0004_000F,
        }
    }

    /// Reads a `sampleformat` code from a `.vcw` or an imported Audacity project.
    pub const fn from_code(code: u32) -> Option<Self> {
        match code {
            0x0002_0001 => Some(Self::Int16),
            0x0003_0002 => Some(Self::Int24Packed),
            0x0004_0001 => Some(Self::Int24Padded),
            0x0004_0002 => Some(Self::Int32),
            0x0004_000F => Some(Self::Float32),
            _ => None,
        }
    }

    /// Bytes one sample of one channel occupies in the stored blob.
    pub const fn bytes_per_sample(self) -> usize {
        ((self.code() >> 16) & 0xFFFF) as usize
    }

    /// What the samples are, independent of how they are laid out.
    pub const fn sample_format(self) -> SampleFormat {
        match self {
            Self::Int16 => SampleFormat::S16,
            Self::Int24Packed | Self::Int24Padded => SampleFormat::S24,
            Self::Int32 => SampleFormat::S32,
            Self::Float32 => SampleFormat::F32,
        }
    }

    /// Whether Audacity writes this code, i.e. whether a block carrying it could
    /// have arrived by import.
    pub const fn is_audacity(self) -> bool {
        matches!(self, Self::Int16 | Self::Int24Padded | Self::Float32)
    }

    /// How VCW stores a freshly captured sample format: verbatim, no padding (D4).
    pub const fn native_for(format: SampleFormat) -> Self {
        match format {
            SampleFormat::S16 => Self::Int16,
            SampleFormat::S24 => Self::Int24Packed,
            SampleFormat::S32 => Self::Int32,
            SampleFormat::F32 => Self::Float32,
        }
    }

    /// Decodes one stored sample to a normalized `f32` in roughly -1.0..=1.0.
    ///
    /// `index` counts samples, not bytes. Returns `None` if the slice is too
    /// short, because a summary computed over a truncated block would be a
    /// plausible-looking lie about a damaged one.
    ///
    /// The scaling is **measured, not assumed** (2026-09-25, against
    /// `/data2/vinyl_rips/simples_test.aup3` and `OWS20.aup3`): a padded 24-bit
    /// sample is a little-endian `i32` holding a value in +/-2^23, *not* a
    /// left-justified 32-bit one, and decoding it this way reproduces Audacity's
    /// own `summin`, `summax` and `sumrms` for every block in the corpus. Get
    /// this wrong and a waveform drawn from an imported project is 256x too
    /// quiet, which is the kind of bug that is easy to ship and hard to see.
    ///
    /// Only summaries and display use this. The capture and export paths never
    /// touch it: §9 stores and returns the bytes the converter produced, and a
    /// round trip through `f32` is exactly the conversion D4 forbids.
    pub fn decode_sample(self, bytes: &[u8], index: usize) -> Option<f32> {
        let width = self.bytes_per_sample();
        let at = index.checked_mul(width)?;
        let raw = bytes.get(at..at.checked_add(width)?)?;
        Some(match self {
            Self::Int16 => f32::from(i16::from_le_bytes([raw[0], raw[1]])) / 32_768.0,
            Self::Int24Packed => {
                // Sign-extend 24 bits by putting them in the high three bytes of
                // an i32 and shifting back down.
                let v = i32::from_le_bytes([0, raw[0], raw[1], raw[2]]) >> 8;
                v as f32 / 8_388_608.0
            }
            Self::Int24Padded => {
                i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f32 / 8_388_608.0
            }
            Self::Int32 => {
                i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f32 / 2_147_483_648.0
            }
            Self::Float32 => f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
        })
    }

    /// How many whole samples a blob of this format holds.
    pub const fn samples_in(self, bytes: usize) -> usize {
        bytes / self.bytes_per_sample()
    }

    /// Every storage format, for exhaustive tests and schema documentation.
    pub const ALL: [Self; 5] = [
        Self::Int16,
        Self::Int24Packed,
        Self::Int24Padded,
        Self::Int32,
        Self::Float32,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audacity_codes_round_trip() {
        for f in [SampleFormat::S16, SampleFormat::S24, SampleFormat::F32] {
            let code = f.audacity_code().expect("representable in Audacity");
            assert_eq!(SampleFormat::from_audacity_code(code), Some(f));
        }
    }

    #[test]
    fn s32_is_the_superset_case() {
        assert_eq!(SampleFormat::S32.audacity_code(), None);
        assert_eq!(SampleFormat::S32.audacity_bytes(), None);
        assert_eq!(SampleFormat::S32.bytes_per_sample(), 4);
    }

    #[test]
    fn audacity_pads_24_bit_and_we_do_not() {
        assert_eq!(SampleFormat::S24.bytes_per_sample(), 3);
        assert_eq!(SampleFormat::S24.audacity_bytes(), Some(4));
    }

    #[test]
    fn capture_modes_round_trip_and_reject_nonsense() {
        for m in CaptureMode::ALL {
            assert_eq!(CaptureMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(CaptureMode::parse("Exclusive"), None);
        assert_eq!(CaptureMode::parse("unknown"), None);
    }

    #[test]
    fn equalization_round_trips_and_rejects_nonsense() {
        for eq in CaptureEq::ALL {
            assert_eq!(CaptureEq::parse(eq.as_str()), Some(eq));
        }
        // The spellings are the stored ones, lower case. A reader that accepted
        // both cases would let two spellings of one fact into the column.
        assert_eq!(CaptureEq::parse("RIAA"), None);
        assert_eq!(CaptureEq::parse("Flat"), None);
        assert_eq!(CaptureEq::parse(""), None);
        assert_eq!(CaptureEq::parse("riaa-inverse"), None);
    }

    #[test]
    fn an_unstated_curve_is_unknown_and_never_riaa() {
        // §51: RIAA is overwhelmingly the common case, which is exactly why it
        // must not be the default. Assuming it would silently claim a fact about
        // somebody's preamp, and the claim is unrecoverable from the audio.
        assert_eq!(CaptureEq::default(), CaptureEq::Unknown);
        assert!(!CaptureEq::default().is_flat());
        assert!(CaptureEq::Flat.is_flat());
        assert!(!CaptureEq::Riaa.is_flat());
    }

    #[test]
    fn shared_is_the_one_mode_that_can_never_be_bit_perfect() {
        assert!(!CaptureMode::Shared.could_be_bit_perfect());
        assert!(CaptureMode::Native.could_be_bit_perfect());
        assert!(CaptureMode::Exclusive.could_be_bit_perfect());
    }

    #[test]
    fn storage_codes_round_trip_and_are_distinct() {
        let mut seen = std::collections::HashSet::new();
        for s in StorageFormat::ALL {
            assert!(seen.insert(s.code()), "duplicate code for {s:?}");
            assert_eq!(StorageFormat::from_code(s.code()), Some(s));
        }
    }

    #[test]
    fn audacitys_three_codes_are_ours_verbatim() {
        for f in [SampleFormat::S16, SampleFormat::S24, SampleFormat::F32] {
            let code = f.audacity_code().expect("representable");
            let stored = StorageFormat::from_code(code).expect("we read it too");
            assert!(stored.is_audacity());
            assert_eq!(stored.sample_format(), f);
            assert_eq!(Some(stored.bytes_per_sample()), f.audacity_bytes());
        }
    }

    #[test]
    fn our_two_extra_codes_are_outside_audacitys() {
        for s in [StorageFormat::Int24Packed, StorageFormat::Int32] {
            assert!(!s.is_audacity());
            assert_eq!(SampleFormat::from_audacity_code(s.code()), None);
        }
    }

    #[test]
    fn capture_stores_verbatim_and_never_pads() {
        for f in [
            SampleFormat::S16,
            SampleFormat::S24,
            SampleFormat::S32,
            SampleFormat::F32,
        ] {
            let stored = StorageFormat::native_for(f);
            assert_eq!(stored.sample_format(), f);
            assert_eq!(stored.bytes_per_sample(), f.bytes_per_sample());
        }
        assert_eq!(
            StorageFormat::native_for(SampleFormat::S24).bytes_per_sample(),
            3
        );
    }

    #[test]
    fn full_scale_decodes_to_full_scale_in_every_format() {
        let cases: [(StorageFormat, &[u8]); 5] = [
            (StorageFormat::Int16, &[0x00, 0x80]),
            (StorageFormat::Int24Packed, &[0x00, 0x00, 0x80]),
            (StorageFormat::Int24Padded, &[0x00, 0x00, 0x80, 0xFF]),
            (StorageFormat::Int32, &[0x00, 0x00, 0x00, 0x80]),
            (StorageFormat::Float32, &(-1.0f32).to_le_bytes()),
        ];
        for (format, bytes) in cases {
            assert_eq!(
                format.decode_sample(bytes, 0),
                Some(-1.0),
                "negative full scale in {format:?}"
            );
        }
        for format in StorageFormat::ALL {
            let zero = vec![0u8; format.bytes_per_sample()];
            assert_eq!(format.decode_sample(&zero, 0), Some(0.0));
        }
    }

    #[test]
    fn a_padded_24_bit_sample_is_not_left_justified() {
        // Measured against the corpus: the stored i32 holds the sample value
        // itself, so 0x00400000 is a half-scale positive, not 1/512th of one.
        // Decoding it as a 32-bit sample would draw every imported waveform
        // 256x too quiet.
        let half: [u8; 4] = 0x0040_0000i32.to_le_bytes();
        assert_eq!(
            StorageFormat::Int24Padded.decode_sample(&half, 0),
            Some(0.5)
        );
        assert_eq!(
            StorageFormat::Int32.decode_sample(&half, 0),
            Some(0.001_953_125)
        );
    }

    #[test]
    fn a_packed_24_bit_sample_is_sign_extended() {
        // 0xFFFFFF is -1 in 24-bit two's complement, not 16777215.
        let minus_one: [u8; 3] = [0xFF, 0xFF, 0xFF];
        let decoded = StorageFormat::Int24Packed
            .decode_sample(&minus_one, 0)
            .expect("in range");
        assert!(decoded < 0.0 && decoded > -0.000_001, "got {decoded}");
    }

    #[test]
    fn a_short_blob_decodes_to_none_rather_than_to_a_plausible_number() {
        // A truncated block is damage. Returning silence for it would let a
        // summary describe audio that is not there.
        assert_eq!(StorageFormat::Int32.decode_sample(&[0, 0, 0], 0), None);
        assert_eq!(StorageFormat::Int16.decode_sample(&[0, 0], 1), None);
        assert_eq!(StorageFormat::Int16.samples_in(5), 2);
    }

    #[test]
    fn the_width_lives_in_the_high_half_of_the_code() {
        for s in StorageFormat::ALL {
            assert_eq!(s.bytes_per_sample(), (s.code() >> 16) as usize);
        }
    }
}
