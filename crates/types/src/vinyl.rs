/*
 *  vinyl.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Sides, discs and alpha track numbering (§29).
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

//! Sides, discs and alpha track numbering (§29).
//!
//! §29 makes vinyl topology first-class and asks that VRipr's alpha numbering be
//! retained. Both come down to one observation: **a side letter is an index**, and
//! everything else is arithmetic on it. Side A is 0, side D is 3, and the disc a
//! side belongs to is that index divided by two. Nothing here is stored in the
//! project as a string, because a string cannot say that C follows B.
//!
//! The types are here rather than in `vcw-project` because two unrelated crates
//! need them and neither should depend on the other: `vcw-metadata` parses side
//! letters out of Discogs and MusicBrainz track positions, and the data model
//! writes them. `vcw-export` will name files from them.
//!
//! # What is *not* here
//!
//! The Discogs and MusicBrainz position grammars. A position like `AA` meaning the
//! second track on side A, or a bare `3` on a release whose sides were never
//! labeled, is a provider convention rather than a property of vinyl, so it is
//! parsed in `vcw-metadata` and arrives here already resolved. [`Position`]'s own
//! [`FromStr`](std::str::FromStr::from_str) takes the unambiguous form only.

use std::fmt;
use std::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How many sides one disc has. Not a configuration knob: it is what a disc is.
pub const SIDES_PER_DISC: u8 = 2;

/// The number of sides VCW can name, `A` through `Z`.
///
/// Thirteen discs. A cap exists because the letter has to come from somewhere, and
/// a box set past it would need a different convention rather than a bigger number.
pub const MAX_SIDES: u8 = 26;

/// Which face of a disc a side is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    /// The first side of the disc: A, C, E.
    First,
    /// The second side of the disc: B, D, F.
    Second,
}

/// One side of one disc, identified by its letter.
///
/// Held as a zero-based index, so `A` is 0. Ordering is the order a listener plays
/// them in, which is what makes `#[derive(PartialOrd)]` meaningful here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Side(u8);

impl Side {
    /// Side A, the first side of the first disc.
    pub const A: Self = Self(0);

    /// A side from its zero-based index, or `None` past [`MAX_SIDES`].
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        if index < MAX_SIDES {
            Some(Self(index))
        } else {
            None
        }
    }

    /// A side from its letter, upper or lower case.
    #[must_use]
    pub const fn from_letter(letter: char) -> Option<Self> {
        let upper = letter.to_ascii_uppercase();
        if upper.is_ascii_uppercase() {
            Self::from_index(upper as u8 - b'A')
        } else {
            None
        }
    }

    /// The zero-based index: A is 0.
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0
    }

    /// The letter: A is `'A'`.
    #[must_use]
    pub const fn letter(self) -> char {
        (b'A' + self.0) as char
    }

    /// The one-based disc this side is on. A and B are disc 1.
    #[must_use]
    pub const fn disc(self) -> u32 {
        self.0 as u32 / SIDES_PER_DISC as u32 + 1
    }

    /// Which face of that disc it is.
    #[must_use]
    pub const fn face(self) -> Face {
        if self.0.is_multiple_of(SIDES_PER_DISC) {
            Face::First
        } else {
            Face::Second
        }
    }

    /// The side on a given one-based disc and face, or `None` past [`MAX_SIDES`].
    ///
    /// Disc 0 is `None` rather than disc 1: a caller that has lost track of its
    /// numbering base should hear about it here, not silently press a record.
    #[must_use]
    pub const fn on_disc(disc: u32, face: Face) -> Option<Self> {
        if disc == 0 {
            return None;
        }
        let offset = match face {
            Face::First => 0,
            Face::Second => 1,
        };
        let index = (disc - 1) * SIDES_PER_DISC as u32 + offset;
        if index < MAX_SIDES as u32 {
            Some(Self(index as u8))
        } else {
            None
        }
    }

    /// The next side to play, or `None` at Z.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        Self::from_index(self.0 + 1)
    }

    /// Every side up to and including this one, in playing order.
    #[must_use]
    pub fn up_to(self) -> Vec<Self> {
        (0..=self.0).map(Self).collect()
    }

    /// The sides of a release with `discs` discs, in playing order.
    ///
    /// Empty for zero discs, and truncated at [`MAX_SIDES`] rather than panicking.
    #[must_use]
    pub fn for_discs(discs: u32) -> Vec<Self> {
        let count = discs
            .saturating_mul(u32::from(SIDES_PER_DISC))
            .min(u32::from(MAX_SIDES)) as u8;
        (0..count).map(Self).collect()
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.letter())
    }
}

impl FromStr for Side {
    type Err = SideError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.trim().chars();
        match (chars.next(), chars.next()) {
            (Some(letter), None) => Self::from_letter(letter).ok_or(SideError(s.to_owned())),
            _ => Err(SideError(s.to_owned())),
        }
    }
}

/// A string that is not a side letter.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a side letter: {0:?}")]
pub struct SideError(pub String);

// Sides cross the IPC boundary and land in JSON that a person reads, so they
// serialize as their letter. An index would be shorter and unreadable, and would
// also silently survive the day someone changes the base.
impl Serialize for Side {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.letter().to_string())
    }
}

impl<'de> Deserialize<'de> for Side {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(D::Error::custom)
    }
}

/// Where a track sits on a record: a side and a one-based number within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    /// The side the track is on.
    pub side: Side,
    /// Its one-based position within that side.
    pub number: u32,
}

impl Position {
    /// A position from a side and a one-based number.
    #[must_use]
    pub const fn new(side: Side, number: u32) -> Self {
        Self { side, number }
    }

    /// The alpha form VRipr used and §29 retains: `A1`, `B12`.
    #[must_use]
    pub fn alpha(&self) -> String {
        format!("{}{}", self.side.letter(), self.number)
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.side.letter(), self.number)
    }
}

impl FromStr for Position {
    type Err = PositionError;

    /// Parses `A1`, `B12`, or a bare `A` meaning the first track on side A.
    ///
    /// Anything else is refused, including the provider conventions described in
    /// the module docs: they are resolved before they get here.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let mut chars = trimmed.chars();
        let side = chars
            .next()
            .and_then(Side::from_letter)
            .ok_or_else(|| PositionError(s.to_owned()))?;
        let rest = chars.as_str();
        if rest.is_empty() {
            return Ok(Self::new(side, 1));
        }
        let number: u32 = rest.parse().map_err(|_| PositionError(s.to_owned()))?;
        if number == 0 {
            return Err(PositionError(s.to_owned()));
        }
        Ok(Self::new(side, number))
    }
}

/// A string that is not a track position.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a track position: {0:?}")]
pub struct PositionError(pub String);

/// How a track number is presented (§29, and §39's export naming).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Numbering {
    /// VRipr's form: the side letter and the number within the side, `B2`.
    ///
    /// The default, because it is the number printed on the label and the one a
    /// person reads off the sleeve.
    #[default]
    Alpha,
    /// A running number across the whole release, `6`.
    Numeric,
}

impl Numbering {
    /// Renders a track number.
    ///
    /// `sequence` is the one-based position across the whole release, which
    /// [`Numbering::Alpha`] ignores and [`Numbering::Numeric`] is.
    #[must_use]
    pub fn render(self, position: Position, sequence: u32) -> String {
        match self {
            Self::Alpha => position.alpha(),
            Self::Numeric => sequence.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_side_letter_is_an_index() {
        assert_eq!(Side::A.index(), 0);
        assert_eq!(Side::A.letter(), 'A');
        assert_eq!(Side::from_letter('d').map(Side::index), Some(3));
        assert_eq!(Side::from_letter('D').map(Side::letter), Some('D'));
        assert_eq!(Side::from_letter('1'), None);
        assert_eq!(Side::from_index(26), None, "past Z");
    }

    #[test]
    fn two_sides_make_a_disc() {
        let sides = Side::for_discs(2);
        let letters: String = sides.iter().map(|s| s.letter()).collect();
        assert_eq!(letters, "ABCD");
        assert_eq!(
            sides.iter().map(|s| s.disc()).collect::<Vec<_>>(),
            [1, 1, 2, 2]
        );
        assert_eq!(sides[2].face(), Face::First, "C opens disc 2");
        assert_eq!(sides[3].face(), Face::Second);
        assert_eq!(Side::on_disc(2, Face::Second), Some(sides[3]));
        assert_eq!(Side::on_disc(0, Face::First), None, "there is no disc zero");
    }

    #[test]
    fn a_box_set_stops_at_z_instead_of_wrapping() {
        assert_eq!(Side::for_discs(20).len(), usize::from(MAX_SIDES));
        assert_eq!(Side::from_index(25).and_then(Side::next), None);
        assert_eq!(
            Side::on_disc(14, Face::First),
            None,
            "side AA does not exist"
        );
        assert_eq!(
            Side::for_discs(0),
            [],
            "a release with no discs has no sides"
        );
    }

    #[test]
    fn a_position_reads_and_prints_the_way_the_label_does() {
        let b2: Position = "B2".parse().expect("B2 parses");
        assert_eq!(b2, Position::new(Side::from_letter('B').unwrap(), 2));
        assert_eq!(b2.to_string(), "B2");
        assert_eq!(b2.alpha(), "B2");
        assert_eq!(
            "C".parse::<Position>().expect("a bare letter"),
            Position::new(Side::from_letter('C').unwrap(), 1),
            "a side with one track is often just its letter"
        );
        assert_eq!("A12".parse::<Position>().map(|p| p.number), Ok(12));
    }

    #[test]
    fn the_provider_conventions_are_refused_here() {
        for odd in ["AA", "3", "", "A0", "A1B", "-1", "A-1"] {
            assert!(
                odd.parse::<Position>().is_err(),
                "{odd:?} is a provider convention or nonsense, not a position"
            );
        }
    }

    #[test]
    fn positions_sort_into_playing_order() {
        let mut positions: Vec<Position> = ["B1", "A2", "C1", "A1", "B10", "B2"]
            .iter()
            .map(|s| s.parse().expect("a position"))
            .collect();
        positions.sort();
        let order: Vec<String> = positions.iter().map(Position::alpha).collect();
        assert_eq!(order, ["A1", "A2", "B1", "B2", "B10", "C1"]);
    }

    #[test]
    fn numbering_renders_both_forms_of_the_same_track() {
        let position: Position = "B2".parse().expect("B2");
        assert_eq!(Numbering::Alpha.render(position, 6), "B2");
        assert_eq!(Numbering::Numeric.render(position, 6), "6");
        assert_eq!(Numbering::default(), Numbering::Alpha);
    }

    #[test]
    fn a_side_travels_as_its_letter() {
        let side = Side::from_letter('C').expect("C");
        let json = serde_json::to_string(&side).expect("serializes");
        assert_eq!(json, "\"C\"");
        assert_eq!(
            serde_json::from_str::<Side>(&json).expect("round trips"),
            side
        );
        assert_eq!(
            serde_json::to_string(&Position::new(side, 2)).expect("serializes"),
            r#"{"side":"C","number":2}"#
        );
        assert!(
            serde_json::from_str::<Side>("\"AA\"").is_err(),
            "a two-letter side is not a side"
        );
    }
}
