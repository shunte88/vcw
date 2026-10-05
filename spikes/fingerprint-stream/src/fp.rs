/*
 *  fp.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Fingerprinting helpers: the two ways to produce a fingerprint (one shot
 *  vs.
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

//! Fingerprinting helpers: the two ways to produce a fingerprint (one shot vs.
//! chunked stream) and the two ways to compare them (exact, and bit error rate
//! with and without a shift allowance).

use anyhow::{Result, anyhow};
use chromaprint::{Algorithm, Fingerprinter};

/// Sub-fingerprints per second for the default (Test2) configuration:
/// 11025 Hz / (4096 - 4096*2/3) frame step. Used only to turn a sample offset
/// into an expected sub-fingerprint shift when reporting.
pub const ITEMS_PER_SEC: f64 = 11025.0 / 1365.0;

pub fn offline(samples: &[i16], rate: u32, channels: u16, algo: Algorithm) -> Result<Vec<u32>> {
    let mut fp = Fingerprinter::new(algo);
    fp.start(rate, channels)
        .map_err(|e| anyhow!("start: {e}"))?;
    fp.feed(samples).map_err(|e| anyhow!("feed: {e}"))?;
    fp.finish().map_err(|e| anyhow!("finish: {e}"))?;
    Ok(fp.fingerprint().to_vec())
}

/// Feed the same samples as a sequence of frame-aligned chunks, the way a live
/// capture worker would drain a ring buffer.
///
/// `chunk_frames` is an iterator so the caller can supply a ragged sequence.
/// Chunks are always whole frames: `AudioProcessor::consume` only `debug_assert!`s
/// frame alignment, so a partial frame corrupts the channel interleave silently in
/// a release build. That contract is the reason this helper takes frames, not samples.
pub fn streamed<I: Iterator<Item = usize>>(
    samples: &[i16],
    rate: u32,
    channels: u16,
    algo: Algorithm,
    mut chunk_frames: I,
) -> Result<Vec<u32>> {
    let ch = channels as usize;
    assert_eq!(
        samples.len() % ch,
        0,
        "sample buffer is not a whole number of frames"
    );

    let mut fp = Fingerprinter::new(algo);
    fp.start(rate, channels)
        .map_err(|e| anyhow!("start: {e}"))?;

    let mut pos = 0usize; // in samples
    while pos < samples.len() {
        let want = chunk_frames.next().unwrap_or(4096).max(1) * ch;
        let end = (pos + want).min(samples.len());
        fp.feed(&samples[pos..end])
            .map_err(|e| anyhow!("feed: {e}"))?;
        pos = end;
    }

    fp.finish().map_err(|e| anyhow!("finish: {e}"))?;
    Ok(fp.fingerprint().to_vec())
}

/// Bit error rate over the common prefix. 0.0 means identical, 0.5 means unrelated.
pub fn ber(a: &[u32], b: &[u32]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return f64::NAN;
    }
    let bits: u32 = a[..n]
        .iter()
        .zip(&b[..n])
        .map(|(x, y)| (x ^ y).count_ones())
        .sum();
    bits as f64 / (n as f64 * 32.0)
}

/// Best bit error rate over all shifts of `b` against `a` within +/- `max_shift`
/// sub-fingerprints, requiring at least `min_overlap` items to compare.
/// Returns (best_ber, shift).
pub fn ber_best_shift(a: &[u32], b: &[u32], max_shift: i64, min_overlap: usize) -> (f64, i64) {
    let mut best = (f64::INFINITY, 0i64);
    for s in -max_shift..=max_shift {
        let (x, y): (&[u32], &[u32]) = if s >= 0 {
            let s = s as usize;
            if s >= b.len() {
                continue;
            }
            (a, &b[s..])
        } else {
            let s = (-s) as usize;
            if s >= a.len() {
                continue;
            }
            (&a[s..], b)
        };
        if x.len().min(y.len()) < min_overlap {
            continue;
        }
        let e = ber(x, y);
        if e < best.0 {
            best = (e, s);
        }
    }
    if best.0.is_finite() {
        best
    } else {
        (f64::NAN, 0)
    }
}

/// A deterministic ragged chunk sequence, standing in for the uneven drains a
/// real ring buffer produces. xorshift so the spike has no rand dependency and
/// every run is reproducible.
pub struct Ragged {
    state: u64,
    max_frames: usize,
}

impl Ragged {
    pub fn new(seed: u64, max_frames: usize) -> Self {
        Self {
            state: seed | 1,
            max_frames,
        }
    }
}

impl Iterator for Ragged {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        Some(1 + (x % self.max_frames as u64) as usize)
    }
}

/// A deterministic, spectrally rich test signal: a handful of inharmonic
/// partials plus shaped noise, so the chroma stages have something to chew on
/// without needing the corpus. Lets the chunk-invariance property be asserted
/// in CI, where /data2/source_rips does not exist. Test-only: the binary always
/// has the corpus, and `cargo test` is the corpus-free reproduction path.
#[cfg(test)]
pub fn synthetic(frames: usize, rate: u32, channels: u16) -> Vec<i16> {
    let mut out = Vec::with_capacity(frames * channels as usize);
    let mut noise = Ragged::new(0xC0FFEE, u16::MAX as usize);
    for n in 0..frames {
        let t = n as f64 / rate as f64;
        let mut v = 0.0f64;
        for (f, a) in [
            (220.0, 0.30),
            (329.6, 0.22),
            (523.3, 0.16),
            (1318.5, 0.08),
            (61.7, 0.12),
        ] {
            v += a * (std::f64::consts::TAU * f * t).sin();
        }
        // Slow amplitude sweep, so the classifiers see change rather than a drone.
        v *= 0.55 + 0.45 * (std::f64::consts::TAU * 0.37 * t).sin();
        let n0 = noise.next().unwrap() as f64 / u16::MAX as f64 - 0.5;
        let l = ((v + 0.02 * n0) * 12000.0).clamp(-32768.0, 32767.0) as i16;
        for ch in 0..channels {
            // Decorrelate the channels slightly, as a real stereo source would be.
            out.push(if ch == 0 {
                l
            } else {
                l.saturating_sub((l as i32 / 17) as i16)
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chromaprint::Algorithm;

    /// The property S4 rests on: how the caller cuts the stream must not change
    /// the fingerprint. Cheap enough to keep in CI and exactly the kind of thing
    /// an upstream optimization could break silently.
    #[test]
    fn chunk_shape_does_not_change_the_fingerprint() {
        let algo = Algorithm::default();
        for &(rate, channels) in &[(48_000u32, 2u16), (192_000, 2), (44_100, 1)] {
            let samples = synthetic(rate as usize * 12, rate, channels);
            let offline = offline(&samples, rate, channels, algo).unwrap();
            assert!(
                offline.len() > 50,
                "{rate} Hz produced only {} items",
                offline.len()
            );

            for n in [1usize, 63, 256, 997, 4096, 32768, 32769] {
                let got = streamed(&samples, rate, channels, algo, std::iter::repeat(n)).unwrap();
                assert_eq!(
                    got, offline,
                    "{rate} Hz {channels} ch, {n}-frame chunks diverged"
                );
            }
            let got = streamed(&samples, rate, channels, algo, Ragged::new(0x5EED, 8192)).unwrap();
            assert_eq!(
                got, offline,
                "{rate} Hz {channels} ch, ragged chunks diverged"
            );
        }
    }

    /// Independent Fingerprinters must not share state.
    #[test]
    fn instances_are_independent() {
        use chromaprint::Fingerprinter;
        let algo = Algorithm::default();
        let samples = synthetic(48_000 * 8, 48_000, 2);
        let reference = offline(&samples, 48_000, 2, algo).unwrap();

        let mut fps: Vec<Fingerprinter> = (0..4).map(|_| Fingerprinter::new(algo)).collect();
        for fp in &mut fps {
            fp.start(48_000, 2).unwrap();
        }
        for block in samples.chunks(12_000 * 2) {
            for fp in &mut fps {
                fp.feed(block).unwrap();
            }
        }
        for fp in &mut fps {
            fp.finish().unwrap();
            assert_eq!(fp.fingerprint(), &reference[..]);
        }
    }

    #[test]
    fn ber_of_unrelated_signals_is_near_a_coin_flip() {
        let algo = Algorithm::default();
        let a = offline(&synthetic(48_000 * 12, 48_000, 2), 48_000, 2, algo).unwrap();
        // A different signal: same generator, resampled duration, so the partials
        // land elsewhere in time.
        let b = offline(&synthetic(31_000 * 12, 31_000, 2), 48_000, 2, algo).unwrap();
        let e = ber(&a, &b);
        assert!(
            e > 0.15,
            "unrelated signals scored {e}, too similar to be a useful scale"
        );
    }
}
