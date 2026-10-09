/*
 *  adopt.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Turning a detection pass into project rows, under a promotion policy.
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

//! Turning a detection pass into project rows, under a promotion policy.
//!
//! This is the only place that sees both a [`Decision`] and a database, which is
//! why it is here and not in `vcw-project`: ADR-0003 keeps the project layer
//! ignorant of `vcw-signal`, so the project layer offers the primitive boundary
//! writes and this one decides what deserves to be written.
//!
//! # The policy is the point
//!
//! §24's resolver already merges every detector's opinion into one decision per
//! boundary, and records how many detectors contributed. What it does not do is
//! decide which of them the project should believe - deliberately, because that is
//! a judgment about false positives rather than about signals. [`Policy`] is that
//! judgment, and its default is the conservative one: **two detectors, or it does
//! not go in**. A boundary only the silence detector saw is a level dip, and a
//! level dip in the middle of a quiet passage is a track split in the wrong place,
//! which costs an operator more to undo than a missed split costs to add.
//!
//! # Re-analysis is the normal case
//!
//! Analysis runs again every time the configuration changes, and §24 says a
//! boundary a person placed survives it. Two things make that work here:
//! [`locked_observations`] hands the project's confirmed boundaries back to
//! [`crate::detection::refine`] as prior observations, so the resolver knows about them and can be
//! held to them; and [`adopt`] refuses to write within [`Tolerance`] of a locked
//! row, so a near-miss from a detector cannot appear beside a boundary the
//! operator already placed.

use vcw_project::error::Result;
use vcw_project::track::{self, NewBoundary};
use vcw_project::{Project, side};
use vcw_signal::resolve::{Decision, Tolerance};
use vcw_types::observation::{BoundaryObservation, Edge, Evidence, Provenance};
use vcw_types::vinyl::Side;

use crate::detection::Refined;

/// What a detection pass has to establish before the project believes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Policy {
    /// How many distinct detectors must have reported a boundary.
    ///
    /// Two by default. One means every level dip becomes a track split; three is
    /// defensible on a clean pressing and loses quiet fade-outs on a worn one.
    pub min_sources: usize,
    /// The lowest confidence worth writing, in `0.0..=1.0`.
    pub min_confidence: f32,
    /// How close to a locked boundary counts as the same boundary.
    ///
    /// A detector landing inside this of a confirmed boundary is reporting the
    /// same event, and the confirmed position is the one that stands (§24).
    pub tolerance: Tolerance,
    /// Whether to pair the adopted boundaries into track rows.
    ///
    /// On by default, because a side of boundaries with no tracks is not something
    /// anything downstream can export. Off is for the caller who wants to show the
    /// operator what was found before committing to a track list.
    pub pair_tracks: bool,
    /// The shortest run of audio worth calling a track, in frames.
    ///
    /// Zero disables the check. A default is not set here because it depends on
    /// the rate, which the caller has and this does not.
    pub min_track_frames: u64,
    /// A detector that must be among the sources, or `None` for any of them.
    ///
    /// §22 runs all three passes over one extraction, which is what makes their
    /// agreement meaningful, so this does not turn a detector off - it narrows
    /// what gets *written*. Set to [`Provenance::Silence`] and a boundary the
    /// spectral pass found alone is not adopted, while one both found still is.
    ///
    /// That distinction is the reason this is a source filter rather than a
    /// choice of algorithm: the extraction is where the cost is, and skipping a
    /// pass would save nothing while making `min_sources` unsatisfiable.
    pub require_source: Option<Provenance>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            min_sources: 2,
            min_confidence: 0.0,
            tolerance: Tolerance::of_frames(0),
            pair_tracks: true,
            min_track_frames: 0,
            require_source: None,
        }
    }
}

impl Policy {
    /// The default policy, with the tolerance and minimum track length a rate
    /// implies.
    ///
    /// Half a second of tolerance, matching the resolver's own default, and two
    /// seconds as the shortest thing worth calling a track - shorter than any
    /// track on a record and longer than any lead-in click.
    #[must_use]
    pub fn at(rate: vcw_types::SampleRate) -> Self {
        Self {
            tolerance: Tolerance::default_at(rate),
            min_track_frames: u64::from(rate.hz()) * 2,
            ..Self::default()
        }
    }

    /// Whether a decision clears the bar.
    #[must_use]
    pub fn accepts(&self, decision: &Decision) -> bool {
        if decision.locked {
            // A locked decision came from the project in the first place, via
            // `locked_observations`, and is not something the policy gets a
            // vote on - including the source filter: an operator's boundary
            // has no detector among its sources and must not be dropped for
            // it.
            return true;
        }
        if let Some(required) = self.require_source
            && !decision.sources.contains(&required)
        {
            return false;
        }
        decision.agreement() >= self.min_sources && decision.confidence >= self.min_confidence
    }
}

/// What adoption wrote, and what it did not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Adopted {
    /// The boundary rows written or updated, in timeline order.
    pub boundaries: Vec<i64>,
    /// The track rows created, in timeline order.
    pub tracks: Vec<i64>,
    /// Decisions the policy turned down.
    pub rejected: usize,
    /// Decisions a locked boundary already accounted for.
    ///
    /// Not rejections: these are boundaries the operator has already settled, and
    /// a detector agreeing with them is the system working.
    pub already_locked: usize,
    /// Track pairs too short to be tracks.
    pub too_short: usize,
}

impl Adopted {
    /// How many boundary rows adoption touched.
    #[must_use]
    pub fn written(&self) -> usize {
        self.boundaries.len()
    }
}

/// Writes a refinement pass into a side, under a policy.
///
/// The side is created if it is absent and is *not* cleared first: adoption adds
/// to what a side knows rather than replacing it, which is what makes running
/// analysis twice safe. A detector that changes its mind about a boundary's
/// confidence updates the row; one that no longer sees a boundary leaves the old
/// row in place, because a detector falling silent is not evidence that a boundary
/// is wrong, and the operator deleting it is.
///
/// # Errors
///
/// If the side cannot be read or a write fails.
pub fn adopt(
    project: &mut Project,
    side: Side,
    refined: &Refined,
    policy: &Policy,
) -> Result<Adopted> {
    adopt_decisions(project, side, &refined.decisions, policy)
}

/// Writes a set of decisions into a side, under a policy.
///
/// The half of [`adopt`] that does not need a whole [`Refined`], for a caller that
/// has filtered the decisions itself or is replaying them from a test.
///
/// # Errors
///
/// If the side cannot be read or a write fails.
pub fn adopt_decisions(
    project: &mut Project,
    side: Side,
    decisions: &[Decision],
    policy: &Policy,
) -> Result<Adopted> {
    let record = side::ensure(project, side)?;
    let locked: Vec<(u64, Edge)> = track::boundaries_of(project.conn(), record.id)?
        .into_iter()
        .filter(|b| b.locked)
        .map(|b| (b.at_frame, b.edge))
        .collect();

    let mut ordered: Vec<&Decision> = decisions.iter().collect();
    ordered.sort_by_key(|d| (d.at, d.edge));

    let mut adopted = Adopted::default();
    for decision in ordered {
        if !policy.accepts(decision) {
            adopted.rejected += 1;
            continue;
        }
        if !decision.locked && settled_by(&locked, decision, policy.tolerance) {
            adopted.already_locked += 1;
            continue;
        }
        let id = track::add_boundary_to(project, record.id, &as_new(decision))?;
        adopted.boundaries.push(id);
    }

    if policy.pair_tracks {
        pair(project, record.id, policy, &mut adopted)?;
    }
    Ok(adopted)
}

/// Reads a side's locked boundaries back as prior observations.
///
/// What [`crate::detection::refine`]'s `already` argument was waiting for: passing
/// these in means the resolver sees the operator's boundaries alongside the
/// detectors' and, since `Provenance::User` outranks every other, keeps them
/// where they are. Without this the second pass would simply not know they exist.
///
/// # Errors
///
/// If the side cannot be read.
///
/// # Why an absent side is not an error
///
/// A side with no row has no boundaries - that is simply true, and it is the
/// answer every caller wants. Raising [`vcw_project::Error::NoSuchSide`]
/// instead was the second half of the piCorePlayer detect refusal: nothing in
/// the capture path creates a side, so the shell's detector got past its own
/// resolver, asked what the operator had already locked on a face that did not
/// exist yet, and failed with `side A is not in this project` on a project
/// holding a perfectly good capture. Adoption creates the row a moment later.
pub fn locked_observations(project: &Project, side: Side) -> Result<Vec<BoundaryObservation>> {
    let conn = project.conn();
    let Some(record) = side::load(conn, side)? else {
        return Ok(Vec::new());
    };
    let observations = track::boundaries_of(conn, record.id)?
        .into_iter()
        .filter(|b| b.locked)
        .map(as_observation)
        .collect();
    Ok(observations)
}

/// Reads every boundary of a side back as prior observations, locked or not.
///
/// The form a full re-analysis wants when it should not lose unconfirmed work
/// either - an operator halfway through reviewing a side, who has locked three
/// boundaries and is still looking at the rest.
///
/// # Errors
///
/// If the side cannot be read. An absent side is not one: see
/// [`locked_observations`].
pub fn observations(project: &Project, side: Side) -> Result<Vec<BoundaryObservation>> {
    let conn = project.conn();
    let Some(record) = side::load(conn, side)? else {
        return Ok(Vec::new());
    };
    let observations = track::boundaries_of(conn, record.id)?
        .into_iter()
        .map(as_observation)
        .collect();
    Ok(observations)
}

/// Reads a stored boundary back as an observation the resolver can weigh.
///
/// The evidence needs unwinding on the way, and that is the whole reason this is a
/// function rather than a struct literal. What is in the row is a *decision's*
/// case, so every measurement in it already carries the name of the detector that
/// took it, while `resolve` prefixes an observation's evidence with its provenance
/// as it absorbs it. Handing a row straight back therefore renames
/// `hmm.posterior` to `hmm.hmm.posterior` on the next pass and again on the pass
/// after that: a side that is re-analyzed repeatedly grows this column without
/// learning anything, which was visible on the real side as 40-odd measurements
/// per boundary, most of them the same number under a longer name. Stripping the
/// row's own provenance back off makes the round trip idempotent, and dropping
/// exact duplicates stops a re-run storing a second copy of a reading that has not
/// moved. A reading that *has* moved is still kept, because `resolve::attach` is
/// right that the pair of them is the only record the boundary shifted between
/// passes.
fn as_observation(boundary: track::Boundary) -> BoundaryObservation {
    let mut observation = BoundaryObservation::new(
        boundary.at_frame,
        boundary.edge,
        boundary.confidence,
        boundary.provenance,
    );
    let own = format!("{}.", boundary.provenance.as_str());
    let mut evidence: Vec<Evidence> = Vec::with_capacity(boundary.evidence.len());
    for item in boundary.evidence {
        // Every leading copy, not just one. Stripping a single prefix is a no-op
        // on a name that was already doubled before this function existed, so a
        // project analyzed by the old code would keep `hmm.hmm.at` forever at a
        // fixed depth. Peeling the run of them heals it on the next pass.
        let mut name = item.name.as_str();
        while let Some(rest) = name.strip_prefix(&own) {
            name = rest;
        }
        // `at` and `confidence` are fields on the observation, not evidence about
        // it: `attach` writes them from the struct, so keeping them here as well
        // would store each of them twice.
        if name == "at" || name == "confidence" {
            continue;
        }
        if !keep(&evidence, name, item.value) {
            continue;
        }
        evidence.push(Evidence::new(name, item.value));
    }
    observation.evidence = evidence;
    observation
}

/// Whether a measurement adds anything to what is already held.
///
/// Exact duplicates only. Compared on the bits rather than with `==` so that the
/// answer does not depend on how the number was arrived at.
fn keep(held: &[Evidence], name: &str, value: f64) -> bool {
    !held
        .iter()
        .any(|kept| kept.name == name && kept.value.to_bits() == value.to_bits())
}

/// A decision as a row to write.
fn as_new(decision: &Decision) -> NewBoundary {
    NewBoundary {
        at: decision.at,
        edge: decision.edge,
        confidence: decision.confidence,
        provenance: decision.provenance,
        sources: decision.sources.clone(),
        // Deduplicated, not copied: the same detector is handed in twice on a
        // re-analysis - once as the stored row, once fresh - and both readings
        // agree whenever the pass is deterministic. See [`as_observation`].
        evidence: decision.evidence.iter().fold(
            Vec::with_capacity(decision.evidence.len()),
            |mut held, item| {
                if keep(&held, &item.name, item.value) {
                    held.push(item.clone());
                }
                held
            },
        ),
        locked: decision.locked,
    }
}

/// Whether a locked boundary already speaks for this decision.
fn settled_by(locked: &[(u64, Edge)], decision: &Decision, tolerance: Tolerance) -> bool {
    locked
        .iter()
        .any(|&(at, edge)| edge == decision.edge && at.abs_diff(decision.at) <= tolerance.frames())
}

/// Pairs a side's unpaired boundaries into tracks, start to end.
///
/// One pass in timeline order: each start is held until an end arrives, and the
/// two become a track. Boundaries the existing tracks already use are left out, so
/// re-running adoption over a side that has tracks adds only what is new.
///
/// Three things it deliberately does not do. A start with no end after it is left
/// unpaired rather than run to the end of the side, because a side whose last
/// track has no end is usually a side still being recorded and inventing an end
/// would be inventing the length of a track. A pair shorter than
/// [`Policy::min_track_frames`] is counted and skipped - but its **boundaries stay
/// in the project**, because they are still the best evidence about where that
/// audio changes and an operator may want to see why the pass declined. And two
/// starts in a row pair the *later* one, which is the shorter and more cautious
/// reading of a detector that lost track of itself.
fn pair(project: &mut Project, side_id: i64, policy: &Policy, adopted: &mut Adopted) -> Result<()> {
    let existing = track::tracks_of(project.conn(), side_id)?;
    let bounded: Vec<i64> = existing
        .iter()
        .flat_map(|t| [t.start_boundary, t.end_boundary])
        .collect();
    // In timeline order already, and end-before-start at a shared frame, which is
    // what makes a split point close one track before opening the next.
    //
    // Boundaries *inside* an existing track are left out as well as the ones that
    // bound it. A merge leaves a locked boundary behind where the join was, and
    // pairing that with the next free end would make a second track overlapping
    // the merged one - re-splitting what the operator just joined.
    let free: Vec<(i64, u64, Edge)> = track::boundaries_of(project.conn(), side_id)?
        .into_iter()
        .filter(|b| !bounded.contains(&b.id))
        .filter(|b| !existing.iter().any(|t| t.contains(b.at_frame)))
        .map(|b| (b.id, b.at_frame, b.edge))
        .collect();

    let mut open: Option<(i64, u64)> = None;
    for (id, at, edge) in free {
        match edge {
            Edge::Start => open = Some((id, at)),
            Edge::End => {
                let Some((start_id, start_at)) = open.take() else {
                    continue;
                };
                if at.saturating_sub(start_at) < policy.min_track_frames {
                    adopted.too_short += 1;
                    continue;
                }
                adopted
                    .tracks
                    .push(track::add_track_between(project, start_id, id)?);
            }
        }
    }
    Ok(())
}
