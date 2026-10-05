/*
 *  chromaprint.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Streaming fingerprinter over `chromaprint-next` (§25).
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

//! Streaming fingerprinter over `chromaprint-next` (§25).
//!
//! One [`Builder`] is one region's fingerprint, built as the audio arrives. §25 asks
//! for exactly that - "PCM shall be fed asynchronously from the capture pipeline to
//! the fingerprint worker", and candidate regions fingerprinted progressively rather
//! than the whole side over and over - and S4 established that it costs nothing to do
//! it live: 0.79 MiB and 0.6% of a core per stream, at the capture rate, with no
//! staging file and no pre-decimation.
//!
//! The dependency of record is the published `chromaprint-next` 0.1.0 from crates.io,
//! not the local checkout - see ADR-0004. Linking it adds an LGPL-2.1-or-later relink
//! obligation to released binaries, which is why `THIRD-PARTY-NOTICES.md` and
//! `LICENSE-LGPL-2.1` were in the repository before this module was, and why
//! `deny.toml` now names the exception.
//!
//! # Why this type exists at all
//!
//! `Fingerprinter::feed` takes `&[i16]`, and a capture tap hands out bytes in whatever
//! quantity happened to be waiting - which is not a whole number of frames. S4 found
//! that `AudioProcessor::consume` only `debug_assert!`s frame alignment, so in a
//! release build a half-frame silently rotates the channel interleave and every sample
//! after it is attributed to the wrong channel. The fingerprint still comes out, and it
//! is wrong.
//!
//! So the alignment is guaranteed here rather than asked for: [`Builder::push`] takes
//! any byte count, holds back the bytes of an incomplete frame, and is the only way
//! audio can get in. Nothing a caller can do makes a partial frame reach `feed`.

use chromaprint::{Algorithm, Fingerprinter};

use vcw_types::{CaptureInfo, SampleRate, StorageFormat};

/// The algorithm AcoustID's index is built with.
///
/// Not a choice: a fingerprint computed with any other one looks like unrelated audio
/// to the service, and the encoded form carries the algorithm in its first byte so a
/// mismatch is a silent miss rather than an error.
pub const ALGORITHM: Algorithm = Algorithm::Test2;

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The library refused the format or the samples.
    #[error("chromaprint refused this capture: {0}")]
    Chromaprint(#[from] chromaprint::Error),
    /// Less audio than one sub-fingerprint needs.
    ///
    /// Roughly 0.4 s at the algorithm's internal 11,025 Hz. An empty fingerprint
    /// encodes to a string a service will accept and never match, so it is refused
    /// here instead of traveling.
    #[error("{frames} frames is too little audio to fingerprint")]
    TooShort {
        /// What was fed.
        frames: u64,
    },
}

/// One region's finished fingerprint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    /// The sub-fingerprints, one per 1/8.3 s of audio.
    ///
    /// Kept as well as [`Self::encoded`] because comparing two of these is how §24's
    /// `fingerprint-transition` evidence gets made, and the encoded form has to be
    /// decoded to do it.
    pub raw: Vec<u32>,
    /// The compressed base64 form, which is what a lookup sends (§26).
    pub encoded: String,
    /// Frames fed, which is the length of the audio this describes.
    pub frames: u64,
    /// The rate they were fed at, so [`Self::seconds`] needs nothing else.
    pub rate: SampleRate,
}

impl Fingerprint {
    /// How much audio this fingerprint covers.
    ///
    /// AcoustID wants the duration alongside the fingerprint, and it is the capture's
    /// own frame count rather than anything the algorithm reports.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.rate.hz()).max(1.0)
    }

    /// The fingerprint in 32 bits: chromaprint's own simhash of the raw vector.
    ///
    /// Not an identifier and not a lookup key - two pressings of the same recording
    /// agree on most of their *bits* rather than on this number, which is what
    /// [`raw`](Self::raw) is compared for. It is the short handle a person or a corpus
    /// run needs to tell two regions apart without reading five kilobytes of base64.
    #[must_use]
    pub fn hash(&self) -> u32 {
        chromaprint::hash_fingerprint(&self.raw)
    }
}

/// A fingerprint being built from a stream of capture bytes.
///
/// Open one per region, [`push`](Self::push) the region's audio at it in any sized
/// pieces, and [`finish`](Self::finish) it. A `Builder` cannot be re-opened: the
/// algorithm's state is consumed by finishing, which is also why a region that needs
/// re-fingerprinting after its boundary moves needs a new one.
pub struct Builder {
    fp: Fingerprinter,
    format: StorageFormat,
    channels: usize,
    frame_bytes: usize,
    rate: SampleRate,
    /// Bytes of an incomplete frame, always shorter than one frame.
    carry: Vec<u8>,
    /// Narrowed samples, reused between pushes so a drain costs no allocation.
    samples: Vec<i16>,
    frames: u64,
}

impl std::fmt::Debug for Builder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Builder")
            .field("rate", &self.rate)
            .field("channels", &self.channels)
            .field("format", &self.format)
            .field("frames", &self.frames)
            .field("carried", &self.carry.len())
            .finish()
    }
}

impl Builder {
    /// Opens a fingerprinter for audio in this capture's format.
    ///
    /// The capture's own rate is fed in unchanged. S4 measured 44.1k, 48k, 96k and
    /// 192k and found the fingerprint identical and the cost flat, so there is nothing
    /// to decide here and nothing to configure wrongly.
    ///
    /// # Errors
    ///
    /// If chromaprint will not accept the rate or the channel count.
    pub fn open(info: &CaptureInfo) -> Result<Self, Error> {
        Self::of(info.rate, info.channels, info.storage_format)
    }

    /// Opens a fingerprinter for audio of a given shape.
    ///
    /// The three facts [`Builder::open`] takes out of a [`CaptureInfo`], for a caller
    /// that has them from somewhere else: a capture read back out of a project
    /// describes its committed audio with the same three and has no `CaptureInfo` to
    /// hand, and fabricating one to get at this constructor would mean inventing seven
    /// other fields the algorithm has no interest in.
    ///
    /// # Errors
    ///
    /// If chromaprint will not accept the rate or the channel count.
    pub fn of(rate: SampleRate, channels: u16, format: StorageFormat) -> Result<Self, Error> {
        let mut fp = Fingerprinter::new(ALGORITHM);
        fp.start(rate.hz(), channels)?;
        let channels = channels as usize;
        Ok(Self {
            fp,
            format,
            channels,
            frame_bytes: format.bytes_per_sample() * channels,
            rate,
            carry: Vec::new(),
            samples: Vec::new(),
            frames: 0,
        })
    }

    /// Feeds interleaved capture bytes, in any quantity.
    ///
    /// Whole frames only reach the algorithm; a trailing part-frame is carried over to
    /// the next call. See the module note on why that is a guarantee and not a
    /// courtesy.
    ///
    /// # Errors
    ///
    /// If chromaprint rejects the samples.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Error> {
        // The carried part-frame and the new bytes are not contiguous, so the join
        // happens here rather than in a slice. It runs at most once per push and only
        // when the previous push ended mid-frame.
        if !self.carry.is_empty() {
            let want = self.frame_bytes - self.carry.len();
            let take = want.min(bytes.len());
            self.carry.extend_from_slice(&bytes[..take]);
            if self.carry.len() < self.frame_bytes {
                return Ok(());
            }
            let frame = std::mem::take(&mut self.carry);
            self.feed(&frame)?;
            return self.push(&bytes[take..]);
        }
        let whole = bytes.len() - bytes.len() % self.frame_bytes;
        self.carry.extend_from_slice(&bytes[whole..]);
        self.feed(&bytes[..whole])
    }

    /// Narrows and feeds a whole number of frames.
    fn feed(&mut self, frames: &[u8]) -> Result<(), Error> {
        debug_assert_eq!(frames.len() % self.frame_bytes, 0, "partial frame");
        if frames.is_empty() {
            return Ok(());
        }
        self.samples.clear();
        self.samples.reserve(self.format.samples_in(frames.len()));
        for index in 0..self.format.samples_in(frames.len()) {
            // `decode_sample` is the one decoder in the product and it normalizes to
            // f32, so this is a float round trip - which D4 forbids on the capture and
            // export paths and this is neither. Narrowing to 16 bits is what the
            // algorithm does to its input anyway: S4 fed `>> 16` of an i32 capture and
            // got a bit-identical fingerprint to the full-width offline run.
            let sample = self.format.decode_sample(frames, index).unwrap_or(0.0);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "clamped into i16's range on the line above the cast"
            )]
            self.samples
                .push((sample * 32_767.0).clamp(-32_768.0, 32_767.0) as i16);
        }
        self.frames += (frames.len() / self.frame_bytes) as u64;
        self.fp.feed(&self.samples)?;
        Ok(())
    }

    /// Frames fed so far.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Audio fed so far, in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.rate.hz()).max(1.0)
    }

    /// Finishes the fingerprint.
    ///
    /// The part-frame still carried is dropped rather than padded: a frame's worth of
    /// audio cannot change a sub-fingerprint, and padding it with silence could.
    ///
    /// # Errors
    ///
    /// [`Error::TooShort`] if the region held too little audio to produce a single
    /// sub-fingerprint, or whatever chromaprint says about finishing.
    pub fn finish(mut self) -> Result<Fingerprint, Error> {
        self.fp.finish()?;
        let raw = self.fp.fingerprint().to_vec();
        if raw.is_empty() {
            return Err(Error::TooShort {
                frames: self.frames,
            });
        }
        Ok(Fingerprint {
            encoded: self.fp.encode(),
            raw,
            frames: self.frames,
            rate: self.rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Builder, Error};
    use vcw_types::{CaptureEq, CaptureInfo, CaptureMode, SampleRate, StorageFormat};

    /// Eight seconds. The algorithm emits 8.08 sub-fingerprints a second once it has
    /// warmed up, and the warm-up is the first 2.65 s - measured, by asking it: five
    /// seconds produced 19 items rather than 40.
    const SECONDS: u32 = 8;
    const RATE: u32 = 48_000;

    fn info(format: StorageFormat) -> CaptureInfo {
        CaptureInfo {
            rate: SampleRate(RATE),
            channels: 2,
            storage_format: format,
            capture_mode: CaptureMode::Shared,
            host_api: None,
            device_id: None,
            device_name: None,
            os_verified: false,
            os_report: None,
            eq: CaptureEq::Unknown,
        }
    }

    /// Deterministic music-shaped audio: two tones a fifth apart, beating slowly, with
    /// the channels at different levels so a rotated interleave is audible to the
    /// algorithm rather than merely different.
    fn audio(format: StorageFormat) -> Vec<u8> {
        let frames = RATE * SECONDS;
        let mut bytes = Vec::with_capacity(frames as usize * 8);
        for frame in 0..frames {
            let t = f64::from(frame) / f64::from(RATE);
            let left = 0.4 * (t * 440.0 * std::f64::consts::TAU).sin()
                + 0.2 * (t * 660.0 * std::f64::consts::TAU).sin();
            let right = 0.1 * (t * 220.0 * std::f64::consts::TAU).sin();
            for sample in [left * (1.0 + 0.3 * (t * 0.7).sin()), right] {
                push_sample(&mut bytes, format, sample);
            }
        }
        bytes
    }

    fn push_sample(out: &mut Vec<u8>, format: StorageFormat, value: f64) {
        let clamped = value.clamp(-1.0, 1.0);
        match format {
            StorageFormat::Int16 => {
                out.extend_from_slice(&((clamped * 32_767.0) as i16).to_le_bytes());
            }
            StorageFormat::Int24Packed => {
                let raw = (clamped * 8_388_607.0) as i32;
                out.extend_from_slice(&raw.to_le_bytes()[..3]);
            }
            StorageFormat::Int24Padded => {
                out.extend_from_slice(&((clamped * 8_388_607.0) as i32).to_le_bytes());
            }
            StorageFormat::Int32 => {
                out.extend_from_slice(&((clamped * 2_147_483_000.0) as i32).to_le_bytes());
            }
            StorageFormat::Float32 => {
                out.extend_from_slice(&(clamped as f32).to_le_bytes());
            }
        }
    }

    /// Feeds `bytes` in pieces of `chunk` bytes, whatever that does to frame alignment.
    fn fingerprint(format: StorageFormat, bytes: &[u8], chunk: usize) -> Vec<u32> {
        let mut builder = Builder::open(&info(format)).expect("open");
        for piece in bytes.chunks(chunk) {
            builder.push(piece).expect("push");
        }
        builder.finish().expect("finish").raw
    }

    #[test]
    fn the_chunk_a_tap_happened_to_hand_over_does_not_change_the_fingerprint() {
        let bytes = audio(StorageFormat::Int32);
        let whole = fingerprint(StorageFormat::Int32, &bytes, bytes.len());
        assert!(
            whole.len() > 40,
            "eight seconds should be ~43 items: {}",
            whole.len()
        );
        // 8 bytes is one frame here. 777 and 4,099 are not multiples of it, which is
        // the case `carry` exists for and the case a ring buffer actually produces -
        // and 3 is smaller than a single frame, which exercises a carry that stays
        // incomplete across several pushes.
        for chunk in [3, 777, 4_099, 65_536] {
            assert_eq!(
                fingerprint(StorageFormat::Int32, &bytes, chunk),
                whole,
                "{chunk}-byte pushes produced a different fingerprint"
            );
        }
    }

    #[test]
    fn every_storage_format_fingerprints_the_same_music_the_same_way() {
        // Not bit-identical across formats - 16-bit audio genuinely differs from 32-bit
        // audio - but the same piece of music, which is what identification needs. The
        // bar is a bit error rate under 10%, against the 47-49% S4 measured for
        // unrelated audio.
        let reference = fingerprint(StorageFormat::Int32, &audio(StorageFormat::Int32), 4_099);
        for format in StorageFormat::ALL {
            let theirs = fingerprint(format, &audio(format), 4_099);
            let common = reference.len().min(theirs.len());
            assert!(common > 30, "{format:?} produced {common} items");
            let bits: u32 = reference[..common]
                .iter()
                .zip(&theirs[..common])
                .map(|(a, b)| (a ^ b).count_ones())
                .sum();
            let ber = f64::from(bits) / (common as f64 * 32.0);
            assert!(
                ber < 0.10,
                "{format:?} is {ber:.3} off the 32-bit fingerprint"
            );
        }
    }

    #[test]
    fn a_region_too_short_to_fingerprint_is_refused_rather_than_encoded() {
        let mut builder = Builder::open(&info(StorageFormat::Int32)).expect("open");
        builder
            .push(&audio(StorageFormat::Int32)[..8_000])
            .expect("push");
        assert_eq!(builder.frames(), 1_000);
        match builder.finish() {
            Err(Error::TooShort { frames }) => assert_eq!(frames, 1_000),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_eq_a_capture_arrived_with_is_none_of_the_algorithms_business() {
        // A smoke check on `open` across the formats, and a note: §51's equalization
        // provenance does not reach the fingerprint. Two copies of a record captured
        // flat and RIAA-corrected are different audio and will fingerprint
        // differently, which is a fact about vinyl and not something to compensate for
        // here.
        let mut info = info(StorageFormat::Int16);
        info.eq = CaptureEq::Riaa;
        assert!(Builder::open(&info).is_ok());
    }
}
