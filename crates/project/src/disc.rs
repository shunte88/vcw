/*
 *  disc.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Discs, as an arithmetic view over sides (§29).
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

//! Discs, as an arithmetic view over sides (§29).
//!
//! There is no `discs` table and there should not be one. A disc is two sides -
//! §29's own definition - and the side index already says which disc a side is on:
//! side 2 (C) is disc 2's first face because `2 / 2 + 1 == 2`. A row would store
//! that same fact a second time, and two copies of a fact are one chance to
//! disagree.
//!
//! So this module computes. [`list`] groups the project's side rows into discs,
//! [`load`] fetches one, and [`Disc`] answers the questions a UI actually asks -
//! how many discs is this, is this one complete, which side do I record next. The
//! `releases.discs` column is the operator's *claim* about the record in their
//! hands (a gatefold double, say, still shrink-wrapped); the sides are what has
//! been recorded. [`expected`] reads the claim, [`list`] reports the reality, and
//! [`missing`] is the gap between them, which is the useful thing to show.

use rusqlite::Connection;
use vcw_types::vinyl::{Face, SIDES_PER_DISC, Side};

use crate::error::Result;
use crate::release;
use crate::side;

/// One disc, with whichever of its two faces the project has rows for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disc {
    /// One-based disc number, as printed on a label.
    pub number: u32,
    /// The first face, where the project has a row for it.
    pub first: Option<side::Record>,
    /// The second face, where the project has a row for it.
    pub second: Option<side::Record>,
}

impl Disc {
    /// An empty disc, with neither face present.
    #[must_use]
    pub const fn empty(number: u32) -> Self {
        Self {
            number,
            first: None,
            second: None,
        }
    }

    /// The face asked for, where the project has a row for it.
    #[must_use]
    pub const fn face(&self, face: Face) -> Option<&side::Record> {
        match face {
            Face::First => self.first.as_ref(),
            Face::Second => self.second.as_ref(),
        }
    }

    /// The faces present, in playing order.
    #[must_use]
    pub fn sides(&self) -> Vec<&side::Record> {
        [self.first.as_ref(), self.second.as_ref()]
            .into_iter()
            .flatten()
            .collect()
    }

    /// Whether the project has rows for both faces.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.first.is_some() && self.second.is_some()
    }

    /// Whether the project has audio for both faces.
    ///
    /// Stronger than [`Self::is_complete`]: a side row with no capture is a side
    /// that has been named and not yet recorded, which is exactly the state a
    /// half-finished session is in.
    #[must_use]
    pub fn is_recorded(&self) -> bool {
        self.sides().len() == usize::from(SIDES_PER_DISC)
            && self.sides().iter().all(|s| s.is_recorded())
    }

    /// The faces of this disc with nothing recorded yet, in playing order.
    #[must_use]
    pub fn unrecorded(&self) -> Vec<Side> {
        [Face::First, Face::Second]
            .into_iter()
            .filter(|&face| self.face(face).is_none_or(|s| !s.is_recorded()))
            .filter_map(|face| Side::on_disc(self.number, face))
            .collect()
    }
}

/// Every disc the project has a side row for, in order.
///
/// Discs with no sides at all are left out, so this is what has been started
/// rather than what is expected - see [`missing`] for the difference. A gap is
/// possible and is not an error: recording disc 2 first, because that is the one
/// on the turntable, is a reasonable thing to do.
///
/// # Errors
///
/// If the query fails.
pub fn list(conn: &Connection) -> Result<Vec<Disc>> {
    let mut discs: Vec<Disc> = Vec::new();
    // `side::list` is already in playing order, so a disc's two faces arrive
    // together and in order, and the last entry is always the one to add to.
    for record in side::list(conn)? {
        let number = record.disc();
        if discs.last().map(|d| d.number) != Some(number) {
            discs.push(Disc::empty(number));
        }
        let Some(disc) = discs.last_mut() else {
            unreachable!("a disc was pushed above if the last one was not this one")
        };
        match record.face() {
            Face::First => disc.first = Some(record),
            Face::Second => disc.second = Some(record),
        }
    }
    Ok(discs)
}

/// One disc, or `None` if the project has no side row on it.
///
/// # Errors
///
/// If the query fails.
pub fn load(conn: &Connection, number: u32) -> Result<Option<Disc>> {
    Ok(list(conn)?.into_iter().find(|d| d.number == number))
}

/// How many discs the release says it has.
///
/// The operator's claim, from `releases.discs`, defaulting to one for a project
/// with no release record yet. Identification fills this in (§28) and the operator
/// can correct it.
///
/// # Errors
///
/// If the query fails.
pub fn expected(conn: &Connection) -> Result<u32> {
    Ok(release::load(conn)?.map_or(1, |r| r.discs))
}

/// How many discs the project has side rows on.
///
/// The highest disc number seen, not the count, so a project holding only disc 2
/// reports 2 - a two-disc set with the first one still to do.
///
/// # Errors
///
/// If the query fails.
pub fn recorded(conn: &Connection) -> Result<u32> {
    Ok(list(conn)?.iter().map(|d| d.number).max().unwrap_or(0))
}

/// The sides the release expects that have no audio yet, in playing order.
///
/// What a session panel shows as "still to record". Sides past the expected disc
/// count are not reported, on the grounds that a side that exists is evidence
/// about the record and the `discs` column is only a claim.
///
/// # Errors
///
/// If the query fails.
pub fn missing(conn: &Connection) -> Result<Vec<Side>> {
    let discs = expected(conn)?.max(recorded(conn)?);
    let present = list(conn)?;
    let mut wanted = Vec::new();
    for number in 1..=discs {
        match present.iter().find(|d| d.number == number) {
            Some(disc) => wanted.extend(disc.unrecorded()),
            None => wanted.extend(Side::for_discs(number).into_iter().filter(|s| {
                // `for_discs` counts from A, so keep only this disc's own faces.
                s.disc() == number
            })),
        }
    }
    Ok(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::Project;
    use vcw_types::{CaptureEq, CaptureInfo, CaptureMode, SampleRate, StorageFormat};

    fn project(dir: &tempfile::TempDir) -> Project {
        Project::create(dir.path().join("discs.vcw")).expect("create")
    }

    fn a_capture(project: &mut Project) -> i64 {
        let info = CaptureInfo {
            rate: SampleRate(48_000),
            channels: 2,
            storage_format: StorageFormat::Int32,
            capture_mode: CaptureMode::Exclusive,
            host_api: Some("ALSA".into()),
            device_id: None,
            device_name: None,
            os_verified: false,
            os_report: None,
            eq: CaptureEq::Unknown,
        };
        crate::session::Session::begin(project, &info)
            .expect("begin")
            .id()
    }

    fn side_of(letter: char) -> Side {
        Side::from_letter(letter).expect("letter")
    }

    fn letters(sides: &[Side]) -> Vec<char> {
        sides.iter().map(|s| s.letter()).collect()
    }

    #[test]
    fn two_sides_make_a_disc() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        side::ensure(&mut p, Side::A).expect("A");
        side::ensure(&mut p, side_of('B')).expect("B");

        let discs = list(p.conn()).expect("list");
        assert_eq!(discs.len(), 1);
        assert_eq!(discs[0].number, 1);
        assert!(discs[0].is_complete());
        assert!(!discs[0].is_recorded(), "named is not recorded");
        assert_eq!(discs[0].sides().len(), 2);
    }

    #[test]
    fn four_sides_make_two_discs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        for letter in ['A', 'B', 'C', 'D'] {
            side::ensure(&mut p, side_of(letter)).expect("ensure");
        }
        let discs = list(p.conn()).expect("list");
        assert_eq!(discs.iter().map(|d| d.number).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(discs[1].first.as_ref().expect("C").letter(), 'C');
        assert_eq!(discs[1].second.as_ref().expect("D").letter(), 'D');
    }

    #[test]
    fn a_half_recorded_disc_reports_the_face_still_to_do() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let capture = a_capture(&mut p);
        side::attach(&mut p, Side::A, capture).expect("A");
        side::ensure(&mut p, side_of('B')).expect("B");

        let disc = load(p.conn(), 1).expect("load").expect("disc 1");
        assert!(disc.is_complete());
        assert!(!disc.is_recorded());
        assert_eq!(letters(&disc.unrecorded()), ['B']);
    }

    #[test]
    fn a_fully_recorded_disc_has_nothing_left() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let capture = a_capture(&mut p);
        side::attach(&mut p, Side::A, capture).expect("A");
        side::attach(&mut p, side_of('B'), capture).expect("B");

        let disc = load(p.conn(), 1).expect("load").expect("disc 1");
        assert!(disc.is_recorded());
        assert!(disc.unrecorded().is_empty());
        assert!(missing(p.conn()).expect("missing").is_empty());
    }

    #[test]
    fn a_double_album_with_nothing_recorded_wants_all_four_sides() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let mut record = release::ensure(&mut p).expect("release");
        record.discs = 2;
        release::store(&mut p, &record).expect("store");

        assert_eq!(expected(p.conn()).expect("expected"), 2);
        assert_eq!(recorded(p.conn()).expect("recorded"), 0);
        assert_eq!(
            letters(&missing(p.conn()).expect("missing")),
            ['A', 'B', 'C', 'D']
        );
    }

    #[test]
    fn recording_disc_two_first_is_not_an_error() {
        // The one on the turntable is the one you record.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let mut record = release::ensure(&mut p).expect("release");
        record.discs = 2;
        release::store(&mut p, &record).expect("store");
        let capture = a_capture(&mut p);
        side::attach(&mut p, side_of('C'), capture).expect("C");
        side::attach(&mut p, side_of('D'), capture).expect("D");

        let discs = list(p.conn()).expect("list");
        assert_eq!(discs.len(), 1, "only the disc with rows is listed");
        assert_eq!(discs[0].number, 2);
        assert!(discs[0].is_recorded());
        assert_eq!(recorded(p.conn()).expect("recorded"), 2);
        assert_eq!(letters(&missing(p.conn()).expect("missing")), ['A', 'B']);
    }

    #[test]
    fn a_side_past_the_claimed_disc_count_still_counts() {
        // The claim was a single album; a third side says otherwise.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let capture = a_capture(&mut p);
        for letter in ['A', 'B', 'C'] {
            side::attach(&mut p, side_of(letter), capture).expect("attach");
        }
        assert_eq!(expected(p.conn()).expect("expected"), 1);
        assert_eq!(recorded(p.conn()).expect("recorded"), 2);
        assert_eq!(
            letters(&missing(p.conn()).expect("missing")),
            ['D'],
            "the fourth side is what is left"
        );
    }

    #[test]
    fn an_empty_project_has_no_discs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = project(&dir);
        assert!(list(p.conn()).expect("list").is_empty());
        assert_eq!(recorded(p.conn()).expect("recorded"), 0);
        assert_eq!(load(p.conn(), 1).expect("load"), None);
        // No release record yet, so the claim is the default single disc.
        assert_eq!(expected(p.conn()).expect("expected"), 1);
        assert_eq!(letters(&missing(p.conn()).expect("missing")), ['A', 'B']);
    }
}
