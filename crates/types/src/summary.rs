/*
 *  summary.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The (min, max, rms) triplet the waveform pyramid is built from (§19).
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

//! The (min, max, rms) triplet the waveform pyramid is built from (§19).
//!
//! Shared vocabulary, for the same reason [`PcmSource`](crate::PcmSource) is: the
//! writer in `vcw-project` *produces* these and the renderer in `vcw-signal`
//! *consumes* them, and neither crate should have to depend on the other to agree
//! on what one is. The shape is Audacity's, verified against the corpus rather
//! than assumed, so a block VCW writes and a block Audacity wrote summarize
//! identically.
//!
//! # RMS composes exactly
//!
//! The property the whole pyramid rests on. Given the RMS of two runs and how
//! many samples each covers, the RMS of the pair is
//! `sqrt((n1*r1^2 + n2*r2^2) / (n1 + n2))` - not an approximation of it, but the
//! same number the samples themselves would have given. So a column drawn from
//! 190 stored triplets is exactly as correct as one computed from the 48,640
//! samples underneath them, and a waveform can be drawn without reading audio.
//!
//! That is only true if the weights are the true sample counts. Audacity's own
//! 64k level weights every group as though it held a full 256 samples, which is
//! why its RMS runs slightly high wherever a block does not divide evenly - a
//! divergence measured against `/data2/vinyl_rips/simples_test.aup3` and recorded
//! in `vcw_project::persistence::pyramid`. [`Summary::merge`] takes the counts.

use crate::StorageFormat;

/// The bytes one triplet occupies on disk: three little-endian `f32`s.
pub const TRIPLET_BYTES: usize = 12;

/// A (min, max, rms) triplet over a run of samples, normalized to -1.0..=1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    /// Least sample value in the run.
    pub min: f32,
    /// Greatest sample value in the run.
    pub max: f32,
    /// Root mean square across the run.
    pub rms: f32,
}

impl Summary {
    /// The summary of no samples at all. Zeroes rather than Audacity's
    /// `(FLT_MAX, -FLT_MAX, 0)` sentinel: we never emit a triplet for a run that
    /// does not exist, so the sentinel has nothing to mean here.
    pub const EMPTY: Self = Self {
        min: 0.0,
        max: 0.0,
        rms: 0.0,
    };

    /// Summarizes `samples` of one channel, stored in `format`.
    #[must_use]
    pub fn of(format: StorageFormat, samples: &[u8]) -> Self {
        let n = format.samples_in(samples.len());
        if n == 0 {
            return Self::EMPTY;
        }
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut squares = 0f64;
        for i in 0..n {
            let Some(v) = format.decode_sample(samples, i) else {
                break;
            };
            min = min.min(v);
            max = max.max(v);
            squares += f64::from(v) * f64::from(v);
        }
        Self {
            min,
            max,
            rms: (squares / n as f64).sqrt() as f32,
        }
    }

    /// The 12 bytes Audacity stores: three little-endian `f32`s, min then max
    /// then rms.
    #[must_use]
    pub fn to_le_bytes(self) -> [u8; TRIPLET_BYTES] {
        let mut out = [0u8; TRIPLET_BYTES];
        out[0..4].copy_from_slice(&self.min.to_le_bytes());
        out[4..8].copy_from_slice(&self.max.to_le_bytes());
        out[8..12].copy_from_slice(&self.rms.to_le_bytes());
        out
    }

    /// Reads one triplet back.
    #[must_use]
    pub fn from_le_bytes(bytes: &[u8; TRIPLET_BYTES]) -> Self {
        let f = |at: usize| {
            f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        Self {
            min: f(0),
            max: f(4),
            rms: f(8),
        }
    }

    /// Every triplet in a stored pyramid level, in order.
    ///
    /// A trailing partial triplet is ignored rather than guessed at. A blob of
    /// the wrong length is a corrupt blob, and `validate` is where that gets
    /// reported; a renderer's job is to draw what is there.
    pub fn triplets(blob: &[u8]) -> impl Iterator<Item = Self> + '_ {
        // `as_chunks` rather than `chunks_exact` because the chunk size is a
        // constant, so the compiler can hand `from_le_bytes` the array it wants
        // without the intermediate copy this used to make. Clippy asks for it
        // from 1.98 on; it has been available since well before our MSRV.
        blob.as_chunks::<TRIPLET_BYTES>()
            .0
            .iter()
            .map(Self::from_le_bytes)
    }

    /// Combines summaries of adjacent runs, each weighted by its sample count.
    ///
    /// Exact, not approximate - see the module doc. Runs of zero samples are
    /// skipped, and merging nothing gives [`Summary::EMPTY`].
    #[must_use]
    pub fn merge(parts: impl IntoIterator<Item = (Self, u64)>) -> Self {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut squares = 0f64;
        let mut total = 0u64;
        for (part, count) in parts {
            if count == 0 {
                continue;
            }
            min = min.min(part.min);
            max = max.max(part.max);
            squares += f64::from(part.rms) * f64::from(part.rms) * count as f64;
            total += count;
        }
        if total == 0 {
            return Self::EMPTY;
        }
        Self {
            min,
            max,
            rms: (squares / total as f64).sqrt() as f32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One channel of 16-bit, as bytes.
    fn pcm(values: &[i16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn a_summary_of_nothing_is_empty_rather_than_a_sentinel() {
        assert_eq!(Summary::of(StorageFormat::Int16, &[]), Summary::EMPTY);
        assert_eq!(Summary::merge(std::iter::empty()), Summary::EMPTY);
        assert_eq!(
            Summary::merge([(Summary::of(StorageFormat::Int16, &pcm(&[1])), 0)]),
            Summary::EMPTY,
            "a run of no samples contributes nothing"
        );
    }

    #[test]
    fn a_triplet_survives_the_round_trip() {
        let summary = Summary {
            min: -0.75,
            max: 0.5,
            rms: 0.25,
        };
        let bytes = summary.to_le_bytes();
        assert_eq!(Summary::from_le_bytes(&bytes), summary);

        let mut blob = Vec::new();
        blob.extend_from_slice(&bytes);
        blob.extend_from_slice(&Summary::EMPTY.to_le_bytes());
        let read: Vec<_> = Summary::triplets(&blob).collect();
        assert_eq!(read, vec![summary, Summary::EMPTY]);

        // A blob with a ragged tail yields the whole triplets and stops.
        blob.push(0);
        assert_eq!(Summary::triplets(&blob).count(), 2);
    }

    /// The property the pyramid rests on, checked rather than assumed.
    #[test]
    fn merging_summaries_gives_the_answer_the_samples_would_have() {
        let format = StorageFormat::Int16;
        // Three runs of deliberately different lengths, so a merge that
        // weighted them equally would be visibly wrong.
        let a = pcm(&[16_384, -8_192, 4_096]);
        let b = pcm(&[32_767; 11]);
        let c = pcm(&[-32_768, 0]);

        let whole = {
            let mut all = a.clone();
            all.extend_from_slice(&b);
            all.extend_from_slice(&c);
            Summary::of(format, &all)
        };
        let merged = Summary::merge([
            (Summary::of(format, &a), 3),
            (Summary::of(format, &b), 11),
            (Summary::of(format, &c), 2),
        ]);

        assert_eq!(merged.min, whole.min);
        assert_eq!(merged.max, whole.max);
        assert!(
            (merged.rms - whole.rms).abs() < 1e-6,
            "merged rms {} is not the rms of the samples {}",
            merged.rms,
            whole.rms
        );
    }

    /// Audacity's arithmetic, reproduced so the divergence stays visible.
    #[test]
    fn weighting_by_capacity_instead_of_count_is_what_makes_audacity_high() {
        let format = StorageFormat::Int16;
        let full = pcm(&[32_767; 8]);
        let short = pcm(&[0; 2]);

        let honest = Summary::merge([
            (Summary::of(format, &full), 8),
            (Summary::of(format, &short), 2),
        ]);
        // The same two runs, but with the short one weighted as though it held
        // a full eight samples: more silence in the average, so a lower answer
        // here - and correspondingly a higher one wherever the short run is the
        // loud one. Either way it is not the RMS of the audio.
        let audacity = Summary::merge([
            (Summary::of(format, &full), 8),
            (Summary::of(format, &short), 8),
        ]);
        assert!(
            (honest.rms - audacity.rms).abs() > 0.1,
            "the two weightings must not agree, or this test proves nothing"
        );

        let mut all = full.clone();
        all.extend_from_slice(&short);
        assert!((honest.rms - Summary::of(format, &all).rms).abs() < 1e-6);
    }
}
