/*
 *  positions.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Reading the position strings providers actually print on labels.
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

//! Reading the position strings providers actually print on labels.
//!
//! [`Position::from_str`](std::str::FromStr::from_str) accepts the unambiguous form, `A1`, and
//! nothing else, which is right for a project file. Providers are not a project
//! file. Discogs carries whatever the person who cataloged the record typed off
//! the label, and MusicBrainz carries whatever its editors agreed on, so a real
//! tracklist contains all of this:
//!
//! | Written        | Means                  | Why                                      |
//! |----------------|------------------------|------------------------------------------|
//! | `A1`, `B12`    | side A track 1, B 12   | the ordinary case                        |
//! | `A`, `AA`, `AAA` | A1, A2, A3           | the letter-run convention, common on 12"s |
//! | `A-1`, `A.1`, `A 1` | A1                  | separators people add                    |
//! | `1`, `2`, `3`  | unresolved             | a numeric tracklist; see [`split_numeric`] |
//! | `Video`, `-`   | unresolved             | not a position at all                    |
//!
//! Everything here is a *guess about a label*, so nothing here is allowed to
//! fail: an unreadable position yields `None` and the track keeps its title. A
//! tracklist with one odd row is still worth showing.
//!
//! # The letter run
//!
//! `AA` is side A track 2, not side AA. This is a real convention and the reason
//! [`Position::from_str`](std::str::FromStr::from_str) refuses `AA` outright: in a project file it
//! would be ambiguous with a 27-sided release, whereas here the provider has
//! already told us which medium the track is on.
//!
//! # Numeric tracklists
//!
//! A numeric tracklist on a record means the cataloger did not record sides, and
//! the only available guess is that the first half is side A. It is a guess and it
//! is labeled as one: [`split_numeric`] returns the positions, and a caller that
//! shows them should say the sides were inferred. On a multi-disc release the
//! split is per medium, because half of *this* record is the useful unit.

use std::time::Duration;

use vcw_types::vinyl::{Face, Position, Side};

/// A position read from a provider's string, and how confident that reading is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// The string named a side and a number.
    Exact(Position),
    /// The string was a bare number and the side is not known yet.
    Numeric(u32),
    /// The string was not a position.
    Unreadable,
}

/// Reads one position string.
///
/// `medium` is the one-based medium number, used to offset the side: track `A1` of
/// medium 2 of a 2xLP is side C, because the sides of a release are lettered
/// straight through. A `medium` of 0 or 1 means no offset.
#[must_use]
pub fn read(text: &str, medium: u32) -> Reading {
    let text = text.trim();
    if text.is_empty() {
        return Reading::Unreadable;
    }

    let mut chars = text.chars();
    let first = chars.next().unwrap_or(' ').to_ascii_uppercase();
    if first.is_ascii_uppercase() {
        // A letter run: A, AA, AAA.
        if text.chars().all(|c| c.to_ascii_uppercase() == first) {
            let number = u32::try_from(text.chars().count()).unwrap_or(1);
            return match side_for(first, medium) {
                Some(side) => Reading::Exact(Position { side, number }),
                None => Reading::Unreadable,
            };
        }
        // A letter, an optional separator, then digits: A1, B-2, C.3, D 4.
        let rest = text[first.len_utf8()..].trim_start_matches(['-', '.', ' ', '/']);
        if !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_digit())
            && let Ok(number) = rest.parse::<u32>()
            && number > 0
        {
            return match side_for(first, medium) {
                Some(side) => Reading::Exact(Position { side, number }),
                None => Reading::Unreadable,
            };
        }
    }

    if let Ok(number) = text.parse::<u32>()
        && number > 0
    {
        return Reading::Numeric(number);
    }

    Reading::Unreadable
}

/// Which side a provider's letter means, on a given medium.
///
/// A letter is taken at face value when the medium offset would not move it, and
/// offset otherwise - so `A1` on medium 2 is side C while `C1` on medium 2 stays
/// side C. Providers are inconsistent about which they print, and taking a letter
/// that is already past the medium's first side at face value is the reading that
/// cannot invent a side the record does not have.
#[must_use]
pub fn side_for(letter: char, medium: u32) -> Option<Side> {
    let side = Side::from_letter(letter)?;
    let medium = medium.max(1);
    let first = Side::on_disc(medium, Face::First)?;
    if side.index() >= first.index() {
        // Already lettered through the release, as MusicBrainz does.
        Some(side)
    } else {
        // Lettered from A on every disc, as some Discogs entries do.
        Side::from_index(first.index().saturating_add(side.index()))
    }
}

/// Assigns sides to a numeric tracklist by splitting it in half.
///
/// The first half goes to the medium's first side and the rest to its second, with
/// an odd count putting the extra track on the first side - which is what a
/// cataloger does, because the longer side goes first. Returns one position per
/// input, in the order given.
///
/// ```
/// # use vcw_metadata::positions::split_numeric;
/// let positions = split_numeric(5, 1);
/// let written: Vec<String> = positions.iter().map(|p| p.alpha()).collect();
/// assert_eq!(written, ["A1", "A2", "A3", "B1", "B2"]);
/// ```
#[must_use]
pub fn split_numeric(count: usize, medium: u32) -> Vec<Position> {
    let medium = medium.max(1);
    let Some(first) = Side::on_disc(medium, Face::First) else {
        return Vec::new();
    };
    let Some(second) = first.next() else {
        return Vec::new();
    };
    let half = count.div_ceil(2);
    (0..count)
        .map(|index| {
            if index < half {
                Position {
                    side: first,
                    number: u32::try_from(index + 1).unwrap_or(1),
                }
            } else {
                Position {
                    side: second,
                    number: u32::try_from(index - half + 1).unwrap_or(1),
                }
            }
        })
        .collect()
}

/// Reads a duration written `M:SS` or `H:MM:SS`.
///
/// Seconds are whole because that is the precision a provider has. A missing or
/// unreadable duration is `None` and never zero: zero is a length, and a track
/// whose length nobody recorded does not have one.
#[must_use]
pub fn read_duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let parts: Vec<&str> = text.split(':').collect();
    let seconds = match parts.as_slice() {
        [minutes, seconds] => {
            minutes.trim().parse::<u64>().ok()? * 60 + seconds.trim().parse::<u64>().ok()?
        }
        [hours, minutes, seconds] => {
            hours.trim().parse::<u64>().ok()? * 3_600
                + minutes.trim().parse::<u64>().ok()? * 60
                + seconds.trim().parse::<u64>().ok()?
        }
        _ => return None,
    };
    Some(Duration::from_secs(seconds))
}

/// Strips the ` (2)` Discogs appends to disambiguate artists with the same name.
///
/// Only a bare number in the parentheses, so `Nirvana (2)` becomes `Nirvana` and
/// `Underworld (Live)` is left alone.
#[must_use]
pub fn clean_artist(name: &str) -> String {
    let name = name.trim();
    if let Some(open) = name.rfind(" (") {
        let suffix = &name[open + 2..];
        if let Some(inner) = suffix.strip_suffix(')')
            && !inner.is_empty()
            && inner.chars().all(|c| c.is_ascii_digit())
        {
            return name[..open].to_string();
        }
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact(text: &str, medium: u32) -> String {
        match read(text, medium) {
            Reading::Exact(position) => position.alpha(),
            other => panic!("{text:?} on medium {medium} read as {other:?}"),
        }
    }

    #[test]
    fn the_ordinary_case() {
        assert_eq!(exact("A1", 1), "A1");
        assert_eq!(exact("B12", 1), "B12");
        assert_eq!(
            exact("  b2  ", 1),
            "B2",
            "lowercase off a badly typed entry"
        );
    }

    #[test]
    fn a_letter_run_is_a_track_number() {
        assert_eq!(exact("A", 1), "A1");
        assert_eq!(exact("AA", 1), "A2");
        assert_eq!(exact("AAA", 1), "A3");
        assert_eq!(exact("BB", 1), "B2");
    }

    #[test]
    fn separators_people_type_are_ignored() {
        for written in ["A-1", "A.1", "A 1", "A/1"] {
            assert_eq!(exact(written, 1), "A1", "{written}");
        }
    }

    #[test]
    fn a_bare_number_is_not_resolved_here() {
        assert_eq!(read("3", 1), Reading::Numeric(3));
        assert_eq!(read("12", 1), Reading::Numeric(12));
    }

    #[test]
    fn things_that_are_not_positions_read_as_nothing() {
        for written in ["", "   ", "-", "Video", "A0", "0", "CD1-3", "A1B"] {
            assert_eq!(read(written, 1), Reading::Unreadable, "{written}");
        }
    }

    #[test]
    fn the_second_disc_of_a_double_is_sides_c_and_d() {
        // Discogs style: lettered from A on every medium.
        assert_eq!(exact("A1", 2), "C1");
        assert_eq!(exact("B2", 2), "D2");
        // MusicBrainz style: lettered straight through the release already.
        assert_eq!(exact("C1", 2), "C1");
        assert_eq!(exact("D2", 2), "D2");
    }

    #[test]
    fn a_third_disc_carries_on_lettering() {
        assert_eq!(exact("A1", 3), "E1");
        assert_eq!(exact("B1", 3), "F1");
        assert_eq!(exact("F1", 3), "F1", "already lettered through");
    }

    #[test]
    fn a_side_past_the_end_of_the_alphabet_is_not_a_side() {
        assert_eq!(read("Z1", 26), Reading::Unreadable);
    }

    #[test]
    fn an_odd_numeric_tracklist_puts_the_extra_track_on_the_first_side() {
        let written = |count, medium| {
            split_numeric(count, medium)
                .iter()
                .map(Position::alpha)
                .collect::<Vec<_>>()
        };
        assert_eq!(written(5, 1), ["A1", "A2", "A3", "B1", "B2"]);
        assert_eq!(written(4, 1), ["A1", "A2", "B1", "B2"]);
        assert_eq!(written(1, 1), ["A1"]);
        assert!(written(0, 1).is_empty());
    }

    #[test]
    fn a_numeric_tracklist_splits_per_medium() {
        let written: Vec<String> = split_numeric(4, 2).iter().map(Position::alpha).collect();
        assert_eq!(
            written,
            ["C1", "C2", "D1", "D2"],
            "half of this record, not half of the release"
        );
    }

    #[test]
    fn durations_come_out_in_whole_seconds_or_not_at_all() {
        assert_eq!(read_duration("4:33"), Some(Duration::from_secs(273)));
        assert_eq!(read_duration("1:02:03"), Some(Duration::from_secs(3_723)));
        assert_eq!(read_duration("0:07"), Some(Duration::from_secs(7)));
        for written in ["", "  ", "4", "4:33:22:11", "four", "4:ab", "-1:00"] {
            assert_eq!(read_duration(written), None, "{written}");
        }
    }

    #[test]
    fn a_discogs_disambiguator_is_not_part_of_the_name() {
        assert_eq!(clean_artist("Nirvana (2)"), "Nirvana");
        assert_eq!(clean_artist("  Bass (14)  "), "Bass");
        assert_eq!(clean_artist("Underworld"), "Underworld");
        assert_eq!(
            clean_artist("Underworld (Live)"),
            "Underworld (Live)",
            "only a bare number is a disambiguator"
        );
        assert_eq!(clean_artist("Add N to (X)"), "Add N to (X)");
    }
}
