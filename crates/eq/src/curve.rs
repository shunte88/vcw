/*
 *  curve.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  One equalization characteristic: three time constants and the citation they came from.
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

//! One equalization characteristic.
//!
//! A curve is three time constants - a bass shelf, a bass turnover and a treble
//! rolloff - and the published source they were taken from. Everything else here is
//! derived from those three numbers.

use serde::Deserialize;

/// Microseconds, as the literature states time constants.
const US: f64 = 1e-6;

/// The frequency every curve is referenced to. Not a parameter: a curve is defined as
/// 0 dB at 1 kHz, and a chart that references something else (NAB uses 700 Hz) reads
/// shifted by a constant against every other chart.
pub const REFERENCE_HZ: f64 = 1000.0;

/// The replay bass limit assumed for a characteristic cut without one, in dB.
///
/// Corresponds to the `R` of the old code letters. See [`Curve::shelf_us`] for why a
/// shelfless curve cannot simply be realized as written.
pub const DEFAULT_REPLAY_SHELF_DB: f64 = 20.0;

/// One playback equalization characteristic, as the catalog file states it.
///
/// Fields are optional because the networks are: a curve with no treble rolloff omits
/// `treble_us`, and a curve with none of the three is flat.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Curve {
    /// Stable identifier, used in projects and on the command line. Lower snake case.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The historic code, e.g. `500R-13.7`, where one exists. Carried because it
    /// cross-checks [`Curve::shelf_us`]: the letter states the player's maximum bass
    /// boost, which predicts the shelf to within 0.02 dB on every curve that has one.
    #[serde(default)]
    pub code: Option<String>,
    /// Bass shelf time constant, microseconds. `None` means the characteristic was cut
    /// without one - the `N` of the code letters - and a replay shelf is synthesized
    /// from [`Curve::replay_shelf_db`] instead.
    ///
    /// It has to be synthesized rather than left out. A curve with no shelf has a pole
    /// at DC, so its replay response rises 20 dB per decade without limit: +69 dB at
    /// 0.1 Hz on EMI, and climbing. Against a real capture, which always carries some
    /// DC offset, that filter is an integrator that runs to the clipping point. Period
    /// players imposed a shelf for exactly this reason, which is what the letters are
    /// describing - they are a property of the player, not of the record.
    #[serde(default)]
    pub shelf_us: Option<f64>,
    /// Bass turnover time constant, microseconds.
    #[serde(default)]
    pub turnover_us: Option<f64>,
    /// Treble rolloff time constant, microseconds. `None` for the characteristics cut
    /// with no pre-emphasis, whose codes end in `-0`.
    #[serde(default)]
    pub treble_us: Option<f64>,
    /// The replay bass limit for a characteristic with no shelf of its own, in dB,
    /// stated as the shelf-to-turnover ratio `20*log10(t1/t2)`. Ignored when
    /// `shelf_us` is set.
    ///
    /// That ratio is exactly what the historic code letters state - RIAA is `500R`
    /// and 3180/318 is 10, which is +20 dB - which is why it is expressed this way
    /// rather than as a gain read off the finished curve. The two differ by a few
    /// tenths of a dB, because the 1 kHz reference point is not quite on the
    /// turnover's asymptote: EMI at +20 dB here settles at 19.63 dB below 1 Hz.
    #[serde(default = "default_replay_shelf_db")]
    pub replay_shelf_db: f64,
    /// Where the parameters came from. Required by §51, and required here: a curve
    /// nobody can trace is a curve nobody can check.
    pub citation: String,
    /// When the characteristic was in use.
    #[serde(default)]
    pub era: Option<String>,
    /// Which labels cut to it.
    #[serde(default)]
    pub labels: Option<String>,
    /// Set where published sources disagree about a parameter, naming the
    /// disagreement. The shipped value is the better-evidenced one; this records that
    /// a reader will find another number elsewhere, and what it is.
    #[serde(default)]
    pub disputed: Option<String>,
}

fn default_replay_shelf_db() -> f64 {
    DEFAULT_REPLAY_SHELF_DB
}

impl Curve {
    /// Whether this curve applies no equalization at all.
    pub fn is_flat(&self) -> bool {
        self.turnover_us.is_none() && self.shelf_us.is_none() && self.treble_us.is_none()
    }

    /// The shelf actually used on replay, in seconds, synthesizing one at
    /// [`Curve::replay_shelf_db`] where the characteristic has none.
    ///
    /// `None` only when the curve has no turnover either, which means it is flat.
    pub fn replay_shelf_s(&self) -> Option<f64> {
        match (self.shelf_us, self.turnover_us) {
            (Some(shelf), _) => Some(shelf * US),
            (None, Some(turnover)) => Some(turnover * US * 10f64.powf(self.replay_shelf_db / 20.0)),
            (None, None) => None,
        }
    }

    /// The curve's poles and zeros in the s plane, in rad/s.
    ///
    /// All of them are real and negative, which is what lets every magnitude in this
    /// crate be computed without complex arithmetic. Returned as `(zeros, poles)`.
    pub fn poles_zeros(&self) -> (Vec<f64>, Vec<f64>) {
        let mut zeros = Vec::new();
        let mut poles = Vec::new();
        if let Some(turnover) = self.turnover_us {
            zeros.push(-1.0 / (turnover * US));
        }
        if let Some(shelf) = self.replay_shelf_s() {
            poles.push(-1.0 / shelf);
        }
        if let Some(treble) = self.treble_us {
            poles.push(-1.0 / (treble * US));
        }
        (zeros, poles)
    }

    /// The exact response at `hz`, in dB relative to 1 kHz.
    ///
    /// This is the definition §51's tolerance is measured against - not a published dB
    /// column, which in most sources describes the treble section alone.
    pub fn response_db(&self, hz: f64) -> f64 {
        let (zeros, poles) = self.poles_zeros();
        let at = |f: f64| magnitude(f, &zeros, &poles);
        20.0 * (at(hz) / at(REFERENCE_HZ)).log10()
    }
}

/// Magnitude of a transfer function with real poles and zeros at frequency `hz`.
///
/// `|j*w - r|` is `sqrt(w^2 + r^2)` for real `r`, so no complex arithmetic is needed.
pub(crate) fn magnitude(hz: f64, zeros: &[f64], poles: &[f64]) -> f64 {
    let w = 2.0 * std::f64::consts::PI * hz;
    let prod = |roots: &[f64]| -> f64 {
        roots
            .iter()
            .map(|r| (w * w + r * r).sqrt())
            .product::<f64>()
    };
    let den = prod(poles);
    if den == 0.0 {
        f64::INFINITY
    } else {
        prod(zeros) / den
    }
}
