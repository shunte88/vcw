/*
 *  defect.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What a defect is and what may be done about it - the vocabulary of WP-30's defect list.
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

//! The defect vocabulary (§46 Phase 3, WP-30).
//!
//! A defect row names three things a reader has to agree with a writer about: the
//! kind of damage, what to do about it, and who said so. The third is
//! [`crate::Provenance`], which boundaries already use and which gains nothing by
//! being spelled a second way. The second is [`Repair`] and is here, below both
//! `vcw-project` which stores it and `vcw-process` which has to act on it.
//!
//! The first - the kind - is deliberately *not* here. The schema comment on
//! `defects.kind` says free text, so that a fourth kind of damage does not need a
//! migration, and an enum would quietly undo that.

use serde::{Deserialize, Serialize};

/// What to do about a span of damage on the way out.
///
/// No default. A repair nobody chose is a repair nobody can be held to, which is
/// the same rule `defects.repair` is declared `NOT NULL` without a default for:
/// the writer states it. [`Repair::None`] is the veto, and it is a stored value
/// rather than a deleted row so that re-analysis does not propose the same span
/// again and the person's reason stays readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Repair {
    /// Replace the damaged samples with a prediction from the audio around them.
    Interpolate,
    /// Silence them. Cruder, and audible on a long span, but it never invents a
    /// transient where the interpolator would.
    Mute,
    /// Leave them alone. The veto.
    None,
}

impl Repair {
    /// The kebab-case name, which is what the column stores.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interpolate => "interpolate",
            Self::Mute => "mute",
            Self::None => "none",
        }
    }

    /// Reads one back, refusing anything else.
    ///
    /// `Option` and not a lenient default: a misspelled repair must be refused
    /// where a misspelled curve is (§51), because the two failures are the same
    /// one - audio leaving the project having had something done to it that
    /// nobody asked for, or nothing done to it that somebody did.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "interpolate" => Some(Self::Interpolate),
            "mute" => Some(Self::Mute),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// Whether this repair changes the audio.
    ///
    /// What the chain predicate asks, rather than asking whether a defect list is
    /// empty: a side whose every defect is vetoed is a side nothing is done to.
    #[must_use]
    pub const fn is_audible(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[cfg(test)]
mod tests {
    use super::Repair;

    /// Every repair round-trips through its name, and nothing else is a repair.
    #[test]
    fn a_repair_survives_the_column() {
        for repair in [Repair::Interpolate, Repair::Mute, Repair::None] {
            assert_eq!(Repair::parse(repair.as_str()), Some(repair));
        }
        assert_eq!(Repair::parse("Interpolate"), None);
        assert_eq!(Repair::parse(""), None);
        assert_eq!(Repair::parse("interpolate "), None);
        assert!(Repair::Interpolate.is_audible());
        assert!(!Repair::None.is_audible());
    }
}
