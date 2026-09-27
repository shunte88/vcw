/*
 *  read.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Project rows assembled into the view models a UI binds to (§35).
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

//! Project rows assembled into the view models a UI binds to (§35).
//!
//! Five readers, and they are here rather than in the shell for the reason the
//! crate exists: none of them is a single row. A track's position is §29's
//! numbering rule applied to the side letter and the running sequence, its
//! seconds are frames divided by the rate the side's *capture* ran at, and its
//! side letter lives on a different table. That is a decision about what a
//! track looks like to a person, which §2 puts behind the boundary, and `vcw
//! tracks --json` has to reach the same answer as the webview.
//!
//! What is not here: anything a shell can do in one call and one conversion.
//! Enumerating devices is `vcw_audio` plus [`crate::view::Device::from`], and a
//! waveform is [`vcw_project::waveform::read`] plus
//! [`crate::view::Waveform::of`]. Wrapping those would add a layer with no
//! decision in it.
//!
//! Every reader takes a `&Connection` and not a `&Project`, so a read-only
//! handle works: a UI painting a view must not be able to write to the project
//! that is being captured into.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use vcw_project::{Result, release, session, side, track};
use vcw_types::SampleRate;

use crate::view;

/// The release, or `None` in a project that has never had one written.
///
/// A new project has no release row until something fills one in, and a UI
/// asking for it before then is the normal case rather than an error.
pub fn release(conn: &Connection) -> Result<Option<view::Release>> {
    Ok(release::load(conn)?.as_ref().map(view::Release::from))
}

/// Every side, in playing order.
pub fn sides(conn: &Connection) -> Result<Vec<view::Side>> {
    Ok(side::list(conn)?.iter().map(view::Side::from).collect())
}

/// Every capture in the project, newest last.
pub fn captures(conn: &Connection) -> Result<Vec<view::Capture>> {
    Ok(session::all(conn)?
        .iter()
        .map(view::Capture::from)
        .collect())
}

/// Every track in the project, in playing order, with §29's positions rendered.
///
/// The positions come from [`vcw_project::track::positions`] so that the running
/// sequence a `Numeric` release needs is counted once, in the place that owns
/// the rule. The side letter and the rate are then joined on by row id, which is
/// why this reads the sides as well: a track row knows its `side_id` and nothing
/// else about the side.
pub fn tracks(conn: &Connection) -> Result<Vec<view::Track>> {
    let numbering = release::load(conn)?
        .map(|r| r.numbering)
        .unwrap_or_default();
    let mut letters = HashMap::new();
    let mut rates: HashMap<i64, SampleRate> = HashMap::new();
    for record in side::list(conn)? {
        letters.insert(record.id, record.side);
        // A side with no capture yet has no rate to divide by. `seconds` is then
        // the frame count at the fallback, which is wrong by a factor - but the
        // frames are also zero, because a track cannot exist before the audio
        // it was detected in.
        if let Some(capture) = record.capture
            && let Some(loaded) = session::load(conn, capture)?
        {
            rates.insert(record.id, loaded.info.rate);
        }
    }

    let mut out = Vec::new();
    for (record, position) in track::positions(conn, numbering)? {
        let Some(&side) = letters.get(&record.side_id) else {
            // The foreign key makes this unreachable; skipping rather than
            // unwrapping means a UI still paints the rest of the project if a
            // future migration ever loosens it.
            continue;
        };
        let rate = rates
            .get(&record.side_id)
            .copied()
            .unwrap_or(SampleRate(44_100));
        out.push(view::Track::of(side, &record, position, rate));
    }
    Ok(out)
}

/// Every boundary in the project, in timeline order, promoted or not.
///
/// The track editor's list, and not the same thing as [`tracks`]: a track is a
/// pair of boundaries that survived §24's promotion policy, and on the real
/// side that policy kept 6 of 270. The 264 it dropped are still rows, and an
/// editor that cannot see them cannot promote one by hand - which is the whole
/// remedy for a policy that is deliberately blunt.
///
/// `promoted` is resolved from the track rows in one pass rather than a query
/// per boundary: every track names its two, so collecting those ids first
/// answers the question for the whole project at the cost of one read.
pub fn boundaries(conn: &Connection) -> Result<Vec<view::Boundary>> {
    let mut used = HashSet::new();
    for record in track::listing(conn)? {
        used.insert(record.1.start_boundary);
        used.insert(record.1.end_boundary);
    }

    let mut out = Vec::new();
    for record in side::list(conn)? {
        // Same fallback as `tracks`, and for the same reason: a side with no
        // capture has no rate to divide by, and no boundaries either, because a
        // boundary is something found in audio.
        let rate = match record.capture {
            Some(capture) => {
                session::load(conn, capture)?.map_or(SampleRate(44_100), |c| c.info.rate)
            }
            None => SampleRate(44_100),
        };
        for boundary in track::boundaries_of(conn, record.id)? {
            let promoted = used.contains(&boundary.id);
            out.push(view::Boundary::of(record.side, &boundary, rate, promoted));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use vcw_project::Project;
    use vcw_types::vinyl::{Numbering, Side};

    use super::*;

    /// A project with one side, two tracks and no capture.
    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut project = Project::create(dir.path().join("demo.vcw")).expect("create");
        side::ensure(&mut project, Side::A).expect("side A");
        track::add_track(&mut project, Side::A, 0, 44_100).expect("track one");
        track::add_track(&mut project, Side::A, 88_200, 132_300).expect("track two");
        (dir, project)
    }

    #[test]
    fn a_track_carries_its_side_letter_and_its_position() {
        let (_dir, project) = project();
        let tracks = tracks(project.conn()).expect("tracks");
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].side, "A");
        assert_eq!(tracks[0].position, "A1");
        assert_eq!(tracks[1].position, "A2");
    }

    #[test]
    fn numeric_numbering_runs_across_the_release() {
        let (_dir, mut project) = project();
        let mut record = release::ensure(&mut project).expect("release");
        record.numbering = Numbering::Numeric;
        release::store(&mut project, &record).expect("store");
        let tracks = tracks(project.conn()).expect("tracks");
        assert_eq!(
            tracks
                .iter()
                .map(|t| t.position.as_str())
                .collect::<Vec<_>>(),
            ["1", "2"]
        );
    }

    #[test]
    fn seconds_come_from_the_rate_and_not_a_guess() {
        let (_dir, project) = project();
        let tracks = tracks(project.conn()).expect("tracks");
        // No capture is attached, so the fallback rate applies - and the test is
        // here to say that out loud rather than to bless it.
        assert!((tracks[0].end - 1.0).abs() < 1e-9, "one second at 44.1 kHz");
        assert_eq!(tracks[0].start_frame, 0);
        assert_eq!(tracks[0].end_frame, 44_100);
    }

    /// The distinction this reader exists for: a detected boundary no track
    /// uses is still listed, and says so.
    #[test]
    fn an_unpromoted_boundary_is_listed_and_flagged() {
        let (_dir, mut project) = project();
        let stray = track::add_boundary(
            &mut project,
            Side::A,
            &track::NewBoundary::detected(
                66_150,
                vcw_types::Edge::Start,
                0.4,
                vcw_types::Provenance::Silence,
            ),
        )
        .expect("a boundary between the two tracks");

        let listed = boundaries(project.conn()).expect("boundaries");
        // Two tracks contribute four boundaries, and the stray is the fifth.
        assert_eq!(listed.len(), 5, "{listed:#?}");

        let odd = listed
            .iter()
            .find(|b| b.id == stray)
            .expect("the stray should be listed");
        assert!(
            !odd.promoted,
            "a boundary no track names should not read as promoted"
        );
        assert_eq!(odd.side, "A");
        assert_eq!(odd.at_frame, 66_150);
        assert!((odd.seconds - 1.5).abs() < 1e-9, "1.5 s at 44.1 kHz");
        assert_eq!(odd.provenance, crate::event::ProvenanceName::Silence);

        assert!(
            listed.iter().filter(|b| b.promoted).count() == 4,
            "the four boundaries the two tracks are made of should read as promoted"
        );
    }

    /// Agreement is its own field, and a hand-placed boundary reports none
    /// without being weaker for it.
    #[test]
    fn a_hand_placed_boundary_reports_no_sources() {
        let (_dir, mut project) = project();
        track::add_boundary(
            &mut project,
            Side::A,
            &track::NewBoundary::by_user(50_000, vcw_types::Edge::End),
        )
        .expect("a boundary a person placed");

        let placed = boundaries(project.conn())
            .expect("boundaries")
            .into_iter()
            .find(|b| b.at_frame == 50_000)
            .expect("the placed boundary");
        assert_eq!(placed.agreement, 0);
        assert!(placed.sources.is_empty());
        assert_eq!(placed.provenance, crate::event::ProvenanceName::User);
    }

    #[test]
    fn a_project_with_no_release_reads_as_none() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = Project::create(dir.path().join("empty.vcw")).expect("create");
        assert!(release(project.conn()).expect("release").is_none());
        assert!(sides(project.conn()).expect("sides").is_empty());
        assert!(tracks(project.conn()).expect("tracks").is_empty());
        assert!(boundaries(project.conn()).expect("boundaries").is_empty());
    }
}
