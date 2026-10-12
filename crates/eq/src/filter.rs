/*
 *  filter.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Designing a curve into a cascade of first-order sections, and running audio through it.
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

//! Turning a curve into a filter, and running audio through it.
//!
//! The textbook route - bilinear transform each analog section - misses §51's
//! +/-0.5 dB by a factor of twenty-six at 44.1 kHz, because 20 kHz sits at 0.907 of
//! Nyquist and the transform maps it onto the analog prototype's 95.6 kHz while
//! forcing a spurious zero at Nyquist. Matched-z is better and still four times over
//! budget.
//!
//! What clears it is matched-z plus one free pole/zero pair whose positions are fitted
//! to minimize the worst-case error against the exact analog response. That lands
//! every curve in the catalog inside 0.08 dB at every rate §8 requires, and the fit is
//! two parameters, so it costs a few hundred microseconds once per curve and rate.
//!
//! Every pole and zero is real, so magnitudes need no complex arithmetic and the
//! filter is a cascade of first-order sections rather than biquads.

use crate::Error;
use crate::catalog::{Catalog, RIAA};
use crate::curve::{Curve, REFERENCE_HZ, magnitude};
use vcw_types::format::CaptureEq;

/// The band §51 states the tolerance over.
pub const BAND_HZ: (f64, f64) = (20.0, 20_000.0);

/// §51's tolerance, in dB.
pub const TOLERANCE_DB: f64 = 0.5;

/// One first-order section: `y[n] = x[n] + b1*x[n-1] + a1*y[n-1]`.
#[derive(Debug, Clone, Copy)]
struct Section {
    b1: f64,
    a1: f64,
}

/// A designed equalization filter, with state for a fixed number of channels.
///
/// Build one with [`Filter::playback`]. It is specific to a curve, a capture
/// provenance and a sample rate; none of the three can change without redesigning.
#[derive(Debug, Clone)]
pub struct Filter {
    sections: Vec<Section>,
    gain: f64,
    /// `(x[n-1], y[n-1])` per channel per section.
    state: Vec<(f64, f64)>,
    channels: usize,
    rate: u32,
    inverts_riaa: bool,
}

impl Filter {
    /// The filter that plays `curve` back from audio captured with `capture`.
    ///
    /// A `Flat` capture takes the curve directly. A `Riaa` capture has already been
    /// through a phono stage, so the curve is applied *over an inverse RIAA* - which
    /// is a real operation on the noise floor, not a bookkeeping detail, and
    /// [`Filter::inverts_riaa`] reports it so a caller can say so. `Unknown` is
    /// refused: §51 requires an operator to state the provenance rather than have one
    /// assumed, and a wrong assumption here cannot be heard as wrong.
    pub fn playback(
        catalog: &Catalog,
        curve_id: &str,
        capture: CaptureEq,
        rate: u32,
        channels: usize,
    ) -> Result<Self, Error> {
        let curve = catalog.require(curve_id)?;
        let (mut zeros, mut poles) = curve.poles_zeros();
        let inverts_riaa = match capture {
            CaptureEq::Unknown => return Err(Error::ProvenanceUnknown),
            CaptureEq::Flat => false,
            CaptureEq::Riaa => {
                // Dividing by RIAA: its poles become our zeros and the other way round.
                let (riaa_zeros, riaa_poles) = catalog.require(RIAA)?.poles_zeros();
                zeros.extend(riaa_poles);
                poles.extend(riaa_zeros);
                curve.id != RIAA
            }
        };
        let mut filter = Self::design(&zeros, &poles, rate, channels)?;
        filter.inverts_riaa = inverts_riaa;
        Ok(filter)
    }

    /// Design a filter for an arbitrary set of real s-plane roots, in rad/s.
    ///
    /// Public because the conformance test and the curve chooser both want to measure
    /// a response without owning a capture.
    pub fn design(zeros: &[f64], poles: &[f64], rate: u32, channels: usize) -> Result<Self, Error> {
        if rate == 0 || channels == 0 {
            return Err(Error::BadStream { rate, channels });
        }
        let (dz, dp) = fit(zeros, poles, rate);
        let n = dz.len().max(dp.len());
        let sections: Vec<Section> = (0..n)
            .map(|i| Section {
                b1: -dz.get(i).copied().unwrap_or(0.0),
                a1: dp.get(i).copied().unwrap_or(0.0),
            })
            .collect();
        if let Some(bad) = sections.iter().find(|s| s.a1.abs() >= 1.0) {
            return Err(Error::Unstable { pole: bad.a1 });
        }
        let at_reference = digital_magnitude(REFERENCE_HZ, rate, &dz, &dp);
        let gain = if at_reference > 0.0 {
            1.0 / at_reference
        } else {
            1.0
        };
        Ok(Self {
            sections,
            gain,
            state: vec![(0.0, 0.0); channels * n],
            channels,
            rate,
            inverts_riaa: false,
        })
    }

    /// Whether this filter undoes an RIAA phono stage on its way to another curve.
    ///
    /// When it does, it amplifies whatever the phono stage attenuated, noise included.
    /// §51 requires that be stated rather than done quietly. [`Filter::response_db`]
    /// at 10 kHz says by how much.
    pub fn inverts_riaa(&self) -> bool {
        self.inverts_riaa
    }

    /// The sample rate this filter was designed for.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The filter's own response at `hz`, in dB relative to 1 kHz.
    ///
    /// This is what §51's tolerance is measured on: it reads the realized digital
    /// filter, not the curve it was designed from.
    pub fn response_db(&self, hz: f64) -> f64 {
        let at = |f: f64| {
            let theta = 2.0 * std::f64::consts::PI * f / f64::from(self.rate);
            self.sections
                .iter()
                .map(|s| {
                    let num = first_order_magnitude(-s.b1, theta);
                    let den = first_order_magnitude(s.a1, theta);
                    if den == 0.0 { f64::INFINITY } else { num / den }
                })
                .product::<f64>()
        };
        20.0 * (at(hz) / at(REFERENCE_HZ)).log10()
    }

    /// Clear the filter's memory, as at a seek or at the start of a render.
    pub fn reset(&mut self) {
        self.state.fill((0.0, 0.0));
    }

    /// Apply the filter in place to interleaved samples.
    ///
    /// `samples.len()` need not be a whole number of frames; a partial frame at the
    /// end is carried correctly because channel is derived from the running index.
    pub fn process(&mut self, samples: &mut [f32]) {
        if self.sections.is_empty() {
            return;
        }
        let n = self.sections.len();
        for (i, sample) in samples.iter_mut().enumerate() {
            let channel = i % self.channels;
            let mut value = f64::from(*sample);
            for (s, section) in self.sections.iter().enumerate() {
                let slot = &mut self.state[channel * n + s];
                let out = value + section.b1 * slot.0 + section.a1 * slot.1;
                *slot = (value, out);
                value = out;
            }
            *sample = (value * self.gain) as f32;
        }
    }
}

/// `|1 - q * e^(-j*theta)|` for real `q`.
fn first_order_magnitude(q: f64, theta: f64) -> f64 {
    (1.0 - 2.0 * q * theta.cos() + q * q).max(0.0).sqrt()
}

fn digital_magnitude(hz: f64, rate: u32, zeros: &[f64], poles: &[f64]) -> f64 {
    let theta = 2.0 * std::f64::consts::PI * hz / f64::from(rate);
    let prod = |roots: &[f64]| -> f64 {
        roots
            .iter()
            .map(|q| first_order_magnitude(*q, theta))
            .product::<f64>()
    };
    let den = prod(poles);
    if den == 0.0 {
        f64::INFINITY
    } else {
        prod(zeros) / den
    }
}

/// The frequencies the fit and the conformance test both measure on.
fn probe_frequencies(rate: u32) -> Vec<f64> {
    let top = BAND_HZ.1.min(f64::from(rate) * 0.4995);
    let count = 200;
    (0..count)
        .map(|i| {
            let t = f64::from(i) / f64::from(count - 1);
            BAND_HZ.0 * (top / BAND_HZ.0).powf(t)
        })
        .collect()
}

/// Matched-z, then one free pole/zero pair fitted for worst-case error.
///
/// Returns the digital zeros and poles. The free pair is what takes the design from
/// about 3 dB of error at 44.1 kHz to under 0.08 dB; see the module comment.
fn fit(zeros: &[f64], poles: &[f64], rate: u32) -> (Vec<f64>, Vec<f64>) {
    let t = 1.0 / f64::from(rate);
    let matched = |roots: &[f64]| -> Vec<f64> { roots.iter().map(|r| (r * t).exp()).collect() };
    let (dz, dp) = (matched(zeros), matched(poles));
    if zeros.is_empty() && poles.is_empty() {
        return (dz, dp);
    }

    let probes = probe_frequencies(rate);
    let reference = magnitude(REFERENCE_HZ, zeros, poles);
    let target: Vec<f64> = probes
        .iter()
        .map(|f| 20.0 * (magnitude(*f, zeros, poles) / reference).log10())
        .collect();

    let cost = |extra_zero: f64, extra_pole: f64| -> f64 {
        if extra_pole.abs() >= 1.0 || extra_zero.abs() >= 2.0 {
            return f64::INFINITY;
        }
        let mut z = dz.clone();
        let mut p = dp.clone();
        z.push(extra_zero);
        p.push(extra_pole);
        let reference = digital_magnitude(REFERENCE_HZ, rate, &z, &p);
        probes
            .iter()
            .zip(&target)
            .map(|(f, want)| {
                let got = 20.0 * (digital_magnitude(*f, rate, &z, &p) / reference).log10();
                (got - want).abs()
            })
            .fold(0.0, f64::max)
    };

    let (extra_zero, extra_pole) = nelder_mead(cost, (-0.5, -0.4));
    let mut z = dz;
    let mut p = dp;
    z.push(extra_zero);
    p.push(extra_pole);
    (z, p)
}

/// Nelder-Mead over two parameters.
///
/// Two, and not the full set of root positions: fitting every position as well buys
/// 0.001 dB on this problem, and a two-parameter simplex is small enough to be
/// obviously terminating. The objective is a worst-case error, so it is not smooth,
/// which is why this rather than a gradient method.
fn nelder_mead<F: Fn(f64, f64) -> f64>(cost: F, seed: (f64, f64)) -> (f64, f64) {
    let mut simplex = [
        (seed.0, seed.1),
        (seed.0 + 0.2, seed.1),
        (seed.0, seed.1 + 0.2),
    ];
    let mut value = simplex.map(|(a, b)| cost(a, b));

    for _ in 0..500 {
        let mut order = [0usize, 1, 2];
        order.sort_by(|a, b| value[*a].total_cmp(&value[*b]));
        let (best, mid, worst) = (order[0], order[1], order[2]);
        let spread = (value[worst] - value[best]).abs();
        if spread < 1e-12 {
            break;
        }

        let centroid = (
            (simplex[best].0 + simplex[mid].0) / 2.0,
            (simplex[best].1 + simplex[mid].1) / 2.0,
        );
        let along = |k: f64| {
            (
                centroid.0 + k * (centroid.0 - simplex[worst].0),
                centroid.1 + k * (centroid.1 - simplex[worst].1),
            )
        };

        let reflected = along(1.0);
        let reflected_value = cost(reflected.0, reflected.1);
        if reflected_value < value[best] {
            let expanded = along(2.0);
            let expanded_value = cost(expanded.0, expanded.1);
            let (point, point_value) = if expanded_value < reflected_value {
                (expanded, expanded_value)
            } else {
                (reflected, reflected_value)
            };
            simplex[worst] = point;
            value[worst] = point_value;
        } else if reflected_value < value[mid] {
            simplex[worst] = reflected;
            value[worst] = reflected_value;
        } else {
            let contracted = along(-0.5);
            let contracted_value = cost(contracted.0, contracted.1);
            if contracted_value < value[worst] {
                simplex[worst] = contracted;
                value[worst] = contracted_value;
            } else {
                for i in [mid, worst] {
                    simplex[i] = (
                        simplex[best].0 + 0.5 * (simplex[i].0 - simplex[best].0),
                        simplex[best].1 + 0.5 * (simplex[i].1 - simplex[best].1),
                    );
                    value[i] = cost(simplex[i].0, simplex[i].1);
                }
            }
        }
    }

    let best = (0..3)
        .min_by(|a, b| value[*a].total_cmp(&value[*b]))
        .unwrap_or(0);
    simplex[best]
}

/// The worst divergence, in dB, between a designed filter and the curve it came from,
/// over §51's band at `rate`.
///
/// The conformance measurement, factored out because the chooser wants to show it too.
pub fn worst_error_db(curve: &Curve, rate: u32) -> Result<f64, Error> {
    let (zeros, poles) = curve.poles_zeros();
    let filter = Filter::design(&zeros, &poles, rate, 1)?;
    Ok(probe_frequencies(rate)
        .into_iter()
        .map(|f| (filter.response_db(f) - curve.response_db(f)).abs())
        .fold(0.0, f64::max))
}
