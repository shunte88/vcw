/*
 *  resolve.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The evidence resolver: many observations in, one decision per boundary out.
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

//! The evidence resolver: many observations in, one decision per boundary out.
//!
//! §23 draws the line this module sits on: "analysis subsystems shall publish
//! observations rather than directly modifying tracks", and "an evidence resolver
//! combines observations into project decisions". Three detectors, a live pass and a
//! refine pass, and eventually a fingerprint and a release's track listing, all report
//! what they saw. None of them edits anything. This is where what they saw becomes what
//! the project believes.
//!
//! The design consequence worth noticing is that the resolver has no idea how many
//! kinds of detector exist. A boundary from [`crate::hmm`] and a boundary derived from a
//! release's published durations are the same shape of thing with a different
//! [`Provenance`], so the day §22's "expected track count, release durations,
//! fingerprints, side topology" arrive, they arrive through this same function with no
//! change to it. The one provenance it treats specially is [`Provenance::User`], and §24
//! requires exactly that: "user-confirmed/locked boundaries shall not be moved by
//! automatic analysis".
//!
//! Requirements: §23 (observations and the resolver), §24 (provenance, confidence,
//! evidence, locking).

use vcw_types::{BoundaryObservation, Edge, Evidence, Provenance, SampleRate};

/// How far apart two observations may be and still be about the same boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tolerance(u64);

impl Tolerance {
    /// Half a second, which is five analysis windows at the default length.
    ///
    /// Wide enough to cover the padding and the smoothing lag the detectors disagree
    /// by - measured at two or three windows in [`crate::spectral`]'s tests - and
    /// narrow enough that two real boundaries never collapse into one, the shortest
    /// gap the pipeline will accept being 0.8 s.
    pub const DEFAULT_SECONDS: f64 = 0.5;

    /// A tolerance in frames.
    #[must_use]
    pub const fn frames(self) -> u64 {
        self.0
    }

    /// A tolerance of so many frames.
    #[must_use]
    pub const fn of_frames(frames: u64) -> Self {
        Self(frames)
    }

    /// A tolerance in seconds at a given rate.
    #[must_use]
    pub fn of_seconds(rate: SampleRate, seconds: f64) -> Self {
        Self((seconds.max(0.0) * f64::from(rate.hz())) as u64)
    }

    /// [`Tolerance::DEFAULT_SECONDS`] at a given rate.
    #[must_use]
    pub fn default_at(rate: SampleRate) -> Self {
        Self::of_seconds(rate, Self::DEFAULT_SECONDS)
    }
}

/// What the project should believe about one boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// Where the boundary is, in frames.
    pub at: u64,
    /// Whether a track starts or ends here.
    pub edge: Edge,
    /// How much to trust it, in 0..=1.
    pub confidence: f32,
    /// The provenance that decided the position.
    pub provenance: Provenance,
    /// Every provenance that reported this boundary, in order, without repeats.
    ///
    /// The number a user interface wants when it says "three detectors agree".
    pub sources: Vec<Provenance>,
    /// Every measurement every contributor made, each prefixed with the provenance it
    /// came from, so `silence.contrast_db` and `hmm.posterior` sit side by side and
    /// neither is lost.
    pub evidence: Vec<Evidence>,
    /// Whether automatic analysis may move this boundary. Set for, and only for, a
    /// boundary a person placed or confirmed.
    pub locked: bool,
}

impl Decision {
    /// Looks a measurement up by its prefixed name.
    #[must_use]
    pub fn measurement(&self, name: &str) -> Option<f64> {
        self.evidence
            .iter()
            .find(|item| item.name == name)
            .map(|item| item.value)
    }

    /// How many distinct detectors reported this boundary.
    #[must_use]
    pub fn agreement(&self) -> usize {
        self.sources.len()
    }
}

/// Combines observations into one decision per boundary.
///
/// Observations may arrive from any number of passes in any order, including duplicates
/// of the same boundary from the live pass and the refine pass. The output is sorted by
/// position, with a track's start before its end where both land on the same frame.
///
/// Four rules, each of which is a judgment rather than a derivation, so each is stated
/// here and tested by name:
///
/// 1. **Observations of the same edge within [`Tolerance`] of each other are one
///    boundary.** Chained, so three detectors each a tolerance apart form one cluster;
///    that is a boundary the detectors disagree about, not three boundaries.
/// 2. **A user boundary fixes the position and cannot be merged away.** Everything else
///    in its cluster becomes evidence attached to it. Two user boundaries close together
///    stay two decisions, because moving either to accommodate the other is exactly what
///    §24 forbids.
/// 3. **Confidence is the highest contributor's, not a combination of all of them.**
///    Corroboration is recorded in [`Decision::sources`] and deliberately does not
///    inflate the number. The three detectors read the same level series, so treating
///    their agreement as independent evidence - noisy-OR, or anything like it - would
///    manufacture certainty out of one measurement counted three times. Two detectors
///    at 0.5 do not make a boundary as good as one detector at 0.75.
/// 4. **Where contributors disagree about position, the safe direction wins**: the
///    earliest start and the latest end. A start a tenth of a second early begins the
///    track in groove noise nobody can hear; a start a tenth late clips the attack. The
///    error is not symmetric, so the choice should not be either.
#[must_use]
pub fn resolve(observations: &[BoundaryObservation], tolerance: Tolerance) -> Vec<Decision> {
    let mut decisions = Vec::new();
    for edge in [Edge::Start, Edge::End] {
        let mut of_edge: Vec<&BoundaryObservation> = observations
            .iter()
            .filter(|item| item.edge == edge)
            .collect();
        of_edge.sort_by_key(|item| (item.at, item.provenance));

        let mut cluster: Vec<&BoundaryObservation> = Vec::new();
        for observation in of_edge {
            let split = cluster
                .last()
                .is_some_and(|last| observation.at.saturating_sub(last.at) > tolerance.frames());
            if split {
                decisions.extend(decide(&cluster, edge));
                cluster.clear();
            }
            cluster.push(observation);
        }
        decisions.extend(decide(&cluster, edge));
    }
    decisions.sort_by_key(|decision| (decision.at, decision.edge == Edge::End));
    decisions
}

/// Turns one cluster into one decision, or into one per user boundary.
fn decide(cluster: &[&BoundaryObservation], edge: Edge) -> Vec<Decision> {
    if cluster.is_empty() {
        return Vec::new();
    }
    let locked: Vec<&BoundaryObservation> = cluster
        .iter()
        .copied()
        .filter(|item| item.provenance.is_locked())
        .collect();
    if locked.is_empty() {
        return vec![automatic(cluster, edge)];
    }

    // Every non-user observation joins the user boundary nearest to it, which is the
    // only reading of §24 that both keeps the evidence and moves nothing.
    let mut out: Vec<Decision> = locked
        .iter()
        .map(|anchor| {
            let mut decision = automatic(&[*anchor], edge);
            decision.at = anchor.at;
            decision.confidence = 1.0;
            decision.provenance = Provenance::User;
            decision.locked = true;
            decision
        })
        .collect();
    for observation in cluster.iter().filter(|item| !item.provenance.is_locked()) {
        let nearest = locked
            .iter()
            .enumerate()
            .min_by_key(|(_, anchor)| anchor.at.abs_diff(observation.at))
            .map_or(0, |(index, _)| index);
        let decision = &mut out[nearest];
        if !decision.sources.contains(&observation.provenance) {
            decision.sources.push(observation.provenance);
        }
        attach(decision, observation);
    }
    for decision in &mut out {
        decision.sources.sort_unstable();
        decision.sources.dedup();
    }
    out
}

/// Builds the decision a cluster with no user boundary in it comes to.
fn automatic(cluster: &[&BoundaryObservation], edge: Edge) -> Decision {
    let at = match edge {
        Edge::Start => cluster.iter().map(|item| item.at).min(),
        Edge::End => cluster.iter().map(|item| item.at).max(),
    }
    .unwrap_or(0);

    // The provenance of record is the one that was most sure, with `Provenance`'s own
    // order breaking a tie - which puts `User` last deliberately, and is why that order
    // is part of the type rather than a local convention here.
    let deciding = cluster
        .iter()
        .max_by(|a, b| {
            a.confidence
                .total_cmp(&b.confidence)
                .then_with(|| a.provenance.cmp(&b.provenance))
        })
        .expect("a cluster is never empty");

    let mut decision = Decision {
        at,
        edge,
        confidence: cluster
            .iter()
            .map(|item| item.confidence)
            .fold(0.0, f32::max),
        provenance: deciding.provenance,
        sources: Vec::new(),
        evidence: Vec::new(),
        locked: false,
    };
    for observation in cluster {
        if !decision.sources.contains(&observation.provenance) {
            decision.sources.push(observation.provenance);
        }
        attach(&mut decision, observation);
    }
    decision.sources.sort_unstable();
    decision.sources.dedup();
    decision
}

/// Copies one observation's case into a decision, prefixed and never merged.
///
/// Two passes of the same detector - the live one and the refine one - both report
/// `silence.contrast_db`, and both are kept. Which is right: the pair of them is the
/// record of a boundary that moved between passes, and a resolver that silently kept
/// one would destroy the only evidence that it did.
fn attach(decision: &mut Decision, observation: &BoundaryObservation) {
    let prefix = observation.provenance.as_str();
    decision
        .evidence
        .push(Evidence::new(format!("{prefix}.at"), observation.at as f64));
    decision.evidence.push(Evidence::new(
        format!("{prefix}.confidence"),
        f64::from(observation.confidence),
    ));
    for item in &observation.evidence {
        decision
            .evidence
            .push(Evidence::new(format!("{prefix}.{}", item.name), item.value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: SampleRate = SampleRate(48_000);
    /// One analysis window at the default length, which is what the detectors disagree
    /// in multiples of.
    const W: u64 = 4_800;

    fn seen(at: u64, edge: Edge, confidence: f32, provenance: Provenance) -> BoundaryObservation {
        BoundaryObservation::new(at, edge, confidence, provenance)
    }

    fn tolerance() -> Tolerance {
        Tolerance::default_at(RATE)
    }

    #[test]
    fn three_detectors_looking_at_one_boundary_produce_one_decision() {
        let observations = vec![
            seen(100 * W, Edge::Start, 0.80, Provenance::Silence).with("contrast_db", 40.0),
            seen(
                100 * W + 2 * W,
                Edge::Start,
                0.70,
                Provenance::SpectralChange,
            ),
            seen(100 * W + W, Edge::Start, 0.60, Provenance::Hmm).with("posterior", 0.99),
        ];
        let decisions = resolve(&observations, tolerance());
        assert_eq!(decisions.len(), 1, "got {decisions:?}");

        let decision = &decisions[0];
        assert_eq!(decision.agreement(), 3);
        assert_eq!(
            decision.sources,
            vec![
                Provenance::Silence,
                Provenance::SpectralChange,
                Provenance::Hmm
            ]
        );
        // The earliest of the three, because this is a start.
        assert_eq!(decision.at, 100 * W);
        // The most confident contributor's number, not a sum of all of them.
        assert!((decision.confidence - 0.80).abs() < 1e-6);
        assert_eq!(decision.provenance, Provenance::Silence);
        assert!(!decision.locked);

        // Every contributor's case survives, under its own name.
        assert_eq!(decision.measurement("silence.contrast_db"), Some(40.0));
        assert_eq!(decision.measurement("hmm.posterior"), Some(0.99));
        assert_eq!(
            decision.measurement("spectral-change.at"),
            Some((100 * W + 2 * W) as f64)
        );
        // Widened from the f32 a confidence is stored as, so compared as one.
        let recorded = decision.measurement("hmm.confidence").unwrap();
        assert!((recorded - 0.6).abs() < 1e-6, "recorded {recorded}");
    }

    #[test]
    fn agreement_is_counted_and_never_added_up() {
        // The rule this module would be wrong without. Three detectors reading the same
        // level series at 0.5 are one measurement counted three times, and a resolver
        // that returned 0.875 for it would have invented the difference.
        let observations = vec![
            seen(100 * W, Edge::End, 0.5, Provenance::Silence),
            seen(100 * W, Edge::End, 0.5, Provenance::SpectralChange),
            seen(100 * W, Edge::End, 0.5, Provenance::Hmm),
        ];
        let decisions = resolve(&observations, tolerance());
        assert_eq!(decisions.len(), 1);
        assert!((decisions[0].confidence - 0.5).abs() < 1e-6);
        assert_eq!(
            decisions[0].agreement(),
            3,
            "the corroboration was lost instead"
        );
    }

    #[test]
    fn a_start_takes_the_earliest_reading_and_an_end_the_latest() {
        let observations = vec![
            seen(200 * W, Edge::Start, 0.9, Provenance::Silence),
            seen(198 * W, Edge::Start, 0.4, Provenance::Hmm),
            seen(300 * W, Edge::End, 0.9, Provenance::Silence),
            seen(302 * W, Edge::End, 0.4, Provenance::Hmm),
        ];
        let decisions = resolve(&observations, tolerance());
        assert_eq!(decisions.len(), 2);
        // Both go outwards, and in both cases the less confident detector won on
        // position while the more confident one set the number. Those are separate
        // questions and answering them separately is deliberate.
        assert_eq!(decisions[0].at, 198 * W);
        assert_eq!(decisions[0].provenance, Provenance::Silence);
        assert_eq!(decisions[1].at, 302 * W);
    }

    #[test]
    fn a_boundary_a_person_placed_is_not_moved_by_anything() {
        // §24, in one test. The detectors are more confident, more numerous, and agree
        // with each other. They still do not get to move it.
        let observations = vec![
            seen(500 * W, Edge::Start, 1.0, Provenance::Silence).with("contrast_db", 60.0),
            seen(501 * W, Edge::Start, 1.0, Provenance::Hmm),
            seen(497 * W, Edge::Start, 0.3, Provenance::User),
        ];
        let decisions = resolve(&observations, tolerance());
        assert_eq!(decisions.len(), 1);

        let decision = &decisions[0];
        assert_eq!(decision.at, 497 * W, "a user boundary moved");
        assert!(decision.locked);
        assert_eq!(decision.provenance, Provenance::User);
        assert!(
            (decision.confidence - 1.0).abs() < 1e-6,
            "a person's own boundary was doubted"
        );
        // The detectors are not discarded, they are demoted to evidence.
        assert_eq!(decision.agreement(), 3);
        assert_eq!(decision.measurement("silence.contrast_db"), Some(60.0));
        assert_eq!(decision.measurement("silence.at"), Some((500 * W) as f64));
    }

    #[test]
    fn two_user_boundaries_close_together_stay_two_boundaries() {
        // The case merging would quietly destroy. Someone has split a segue by hand,
        // twice, four tenths of a second apart. Neither may move, so neither may be
        // merged into the other, whatever the tolerance says.
        let observations = vec![
            seen(100 * W, Edge::Start, 0.9, Provenance::User),
            seen(104 * W, Edge::Start, 0.9, Provenance::User),
            seen(101 * W, Edge::Start, 0.8, Provenance::Silence),
            seen(105 * W, Edge::Start, 0.7, Provenance::Hmm),
        ];
        let decisions = resolve(&observations, tolerance());
        assert_eq!(decisions.len(), 2, "got {decisions:?}");
        assert_eq!(decisions[0].at, 100 * W);
        assert_eq!(decisions[1].at, 104 * W);
        assert!(decisions.iter().all(|decision| decision.locked));

        // And each detector attached to the user boundary it was nearest to.
        assert_eq!(
            decisions[0].sources,
            vec![Provenance::Silence, Provenance::User]
        );
        assert_eq!(
            decisions[1].sources,
            vec![Provenance::Hmm, Provenance::User]
        );
    }

    #[test]
    fn two_boundaries_further_apart_than_the_tolerance_stay_apart() {
        let observations = vec![
            seen(100 * W, Edge::Start, 0.9, Provenance::Silence),
            seen(106 * W, Edge::Start, 0.9, Provenance::Silence),
        ];
        // Six windows is 0.6 s, past the half-second tolerance.
        assert_eq!(resolve(&observations, tolerance()).len(), 2);
        // And a wider tolerance makes them one, which is the knob working.
        assert_eq!(
            resolve(&observations, Tolerance::of_seconds(RATE, 1.0)).len(),
            1
        );
    }

    #[test]
    fn a_ladder_of_observations_is_one_boundary_the_detectors_disagree_about() {
        // Single-link chaining, and the consequence stated out loud: five observations
        // each four windows from the last span two seconds in total, further than the
        // tolerance, and still resolve to one decision. That is the right answer - it is
        // one boundary nobody can place - but it is a choice, so it is pinned.
        let observations: Vec<BoundaryObservation> = (0..5)
            .map(|step| seen((100 + step * 4) * W, Edge::End, 0.5, Provenance::Hmm))
            .collect();
        let decisions = resolve(&observations, tolerance());
        assert_eq!(decisions.len(), 1);
        assert_eq!(
            decisions[0].at,
            116 * W,
            "the latest reading, this being an end"
        );
        assert_eq!(
            decisions[0].evidence.len(),
            10,
            "five observations, two entries each"
        );
    }

    #[test]
    fn a_start_and_an_end_on_the_same_frame_are_two_different_things() {
        // A track ending where the next begins, which is what the de-overlap in
        // `regions::shape` produces on purpose. Clustering across edges would turn the
        // seam between two tracks into one unusable marker.
        let observations = vec![
            seen(400 * W, Edge::End, 0.9, Provenance::Silence),
            seen(400 * W, Edge::Start, 0.9, Provenance::Silence),
        ];
        let decisions = resolve(&observations, tolerance());
        assert_eq!(decisions.len(), 2);
        assert_eq!(
            decisions[0].edge,
            Edge::Start,
            "the start of a track sorts first"
        );
        assert_eq!(decisions[1].edge, Edge::End);
    }

    #[test]
    fn the_live_pass_and_the_refine_pass_disagreeing_is_recorded_not_hidden() {
        // The same detector, twice, one window apart: the provisional marker and the
        // one the whole side produced. Both readings are kept, because the pair of them
        // is the only record that the boundary moved when the rest of the side arrived.
        let observations = vec![
            seen(100 * W, Edge::Start, 0.6, Provenance::Silence).with("contrast_db", 20.0),
            seen(101 * W, Edge::Start, 0.9, Provenance::Silence).with("contrast_db", 44.0),
        ];
        let decisions = resolve(&observations, tolerance());
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].agreement(), 1, "one detector, two passes");
        let readings: Vec<f64> = decisions[0]
            .evidence
            .iter()
            .filter(|item| item.name == "silence.contrast_db")
            .map(|item| item.value)
            .collect();
        assert_eq!(readings, vec![20.0, 44.0]);
    }

    #[test]
    fn a_boundary_from_a_release_listing_needs_no_new_code_to_be_heard() {
        // The claim in the module documentation, tested rather than asserted: the
        // resolver does not know what kinds of detector exist. A boundary derived from
        // published durations and one from a fingerprint transition go through the same
        // path as the three signal detectors, and corroborate them.
        let observations = vec![
            seen(600 * W, Edge::Start, 0.5, Provenance::Hmm),
            seen(601 * W, Edge::Start, 0.7, Provenance::MetadataDuration).with("track", 4.0),
            seen(600 * W, Edge::Start, 0.9, Provenance::Fingerprint).with("score", 0.93),
        ];
        let decisions = resolve(&observations, tolerance());
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].agreement(), 3);
        assert_eq!(decisions[0].provenance, Provenance::Fingerprint);
        assert!((decisions[0].confidence - 0.9).abs() < 1e-6);
        assert_eq!(
            decisions[0].measurement("metadata-duration.track"),
            Some(4.0)
        );
    }

    #[test]
    fn nothing_in_is_nothing_out() {
        assert!(resolve(&[], tolerance()).is_empty());
        assert_eq!(Tolerance::default_at(RATE).frames(), 24_000);
        assert_eq!(
            Tolerance::of_seconds(RATE, -1.0).frames(),
            0,
            "a negative tolerance"
        );
        assert_eq!(Tolerance::of_frames(17).frames(), 17);
    }
}
