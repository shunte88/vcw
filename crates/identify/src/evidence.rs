/*
 *  evidence.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Everything known about the record being captured, collected without judgement.
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

//! Everything known about the record being captured, collected without judgement.
//!
//! Requirements: §23 (the evidence model), §26 (identification is evidence-based
//! rather than a single match).
//!
//! The reason this is a flat list of `(source, fact)` pairs and not a struct of
//! `Option`s is that the same fact can arrive from several places and disagree with
//! itself. A person types `CHRH 1296` off the label, Discogs says the pressing is
//! `CHRH1296`, MusicBrainz has no catalogue number at all: that is three states, not
//! one field, and the resolver's job is to say which release accounts for them best.
//! Collapsing them on the way in would throw away the only information that can
//! explain a wrong answer afterwards.
//!
//! Nothing here scores anything. See [`crate::confidence`] for the weights and
//! [`crate::candidate`] for what a release does or does not agree with.

use std::time::Duration;

/// Where a fact came from.
///
/// The ordering is deliberate and is load-bearing in [`Source::weight`]: a person
/// holding the record outranks a provider's guess about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// Typed by a person at project setup, reading the object in their hands.
    ///
    /// §26 requires that automatic identification never silently replaces what a
    /// person confirmed. Making this a *source* rather than a later override is how
    /// that becomes structural: a stated fact is evidence from the first comparison,
    /// not a veto bolted on after one.
    Stated,
    /// Measured from the audio or from the project: a side's length, a track count a
    /// detector agreed on.
    Signal,
    /// Discogs, the pressing-level database.
    Discogs,
    /// MusicBrainz, the release-level database.
    MusicBrainz,
    /// AcoustID, which answers fingerprints rather than text.
    AcoustId,
}

impl Source {
    /// How much a fact from here counts, as a multiplier on its own weight.
    ///
    /// A stated fact and a measured one are both first-hand, so both count fully. A
    /// provider's fact is second-hand: it is being used to judge *that provider's own
    /// candidate*, so letting it count fully would let a release vouch for itself.
    #[must_use]
    pub const fn weight(self) -> f32 {
        match self {
            Self::Stated | Self::Signal => 1.0,
            Self::Discogs | Self::MusicBrainz | Self::AcoustId => 0.5,
        }
    }

    /// The lowercase token used in JSON and in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stated => "stated",
            Self::Signal => "signal",
            Self::Discogs => "discogs",
            Self::MusicBrainz => "musicbrainz",
            Self::AcoustId => "acoustid",
        }
    }
}

/// One thing known about the record.
#[derive(Debug, Clone, PartialEq)]
pub enum Fact {
    /// The credited artist.
    Artist(String),
    /// The release title.
    Album(String),
    /// The catalogue number off the label, however it was spelled.
    ///
    /// The one identifier a vinyl pressing reliably carries, which is why
    /// [`crate::confidence`] weighs it above everything else.
    Catalogue(String),
    /// The record label.
    Label(String),
    /// Year of the pressing.
    Year(u32),
    /// How many tracks the record has.
    Count(usize),
    /// How long a captured side is.
    Side(Duration),
    /// A recording AcoustID identified in the audio, and how well it scored.
    Recording {
        /// The MusicBrainz recording id. AcoustID's recordings are MusicBrainz's.
        id: String,
        /// AcoustID's score, 0.0 to 1.0.
        score: f32,
        /// The releases AcoustID said this recording appears on.
        ///
        /// Carried with the fact rather than looked up later, because it is what
        /// lets a candidate release agree with the recording at all. It is also
        /// why a recording can never *contradict* a candidate: AcoustID's release
        /// lists come from digital submissions and are badly incomplete for
        /// vinyl, so a pressing missing from the list means nothing.
        releases: Vec<String>,
    },
}

impl Fact {
    /// The kind of fact this is, for weighing without matching on the payload.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        match self {
            Self::Artist(_) => Kind::Artist,
            Self::Album(_) => Kind::Album,
            Self::Catalogue(_) => Kind::Catalogue,
            Self::Label(_) => Kind::Label,
            Self::Year(_) => Kind::Year,
            Self::Count(_) => Kind::Count,
            Self::Side(_) => Kind::Side,
            Self::Recording { .. } => Kind::Recording,
        }
    }
}

/// A fact's kind with its payload removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// [`Fact::Artist`].
    Artist,
    /// [`Fact::Album`].
    Album,
    /// [`Fact::Catalogue`].
    Catalogue,
    /// [`Fact::Label`].
    Label,
    /// [`Fact::Year`].
    Year,
    /// [`Fact::Count`].
    Count,
    /// [`Fact::Side`].
    Side,
    /// [`Fact::Recording`].
    Recording,
}

/// A fact and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// What is known.
    pub fact: Fact,
    /// Who says so.
    pub source: Source,
}

/// Everything known so far, in the order it arrived.
#[derive(Debug, Clone, Default)]
pub struct Observed {
    items: Vec<Item>,
}

impl Observed {
    /// Nothing known yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Records a fact. Duplicates are kept: two providers agreeing is evidence.
    pub fn add(&mut self, source: Source, fact: Fact) -> &mut Self {
        self.items.push(Item { fact, source });
        self
    }

    /// Everything known, in arrival order.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Whether anything is known at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The first thing a person stated of this kind, if they stated one.
    ///
    /// For the places that need the typed value itself rather than a comparison
    /// against it: a catalogue lookup has to send the number somebody typed.
    #[must_use]
    pub fn stated(&self, kind: Kind) -> Option<&Fact> {
        self.items
            .iter()
            .find(|item| item.source == Source::Stated && item.fact.kind() == kind)
            .map(|item| &item.fact)
    }
}

/// Folds a catalogue number to the form two spellings of it share.
///
/// Catalogue numbers are printed with whatever spacing fits the label: `CHRH 1296`,
/// `CHRH1296` and `CHRH-1296` are one pressing, and a person copying one off a sleeve
/// will not match a provider's spelling by accident. Keeping only alphanumerics,
/// uppercased, is the fold both ends can agree on.
#[must_use]
pub fn fold_catalogue(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Folds a title or a name to the form two spellings of it share.
///
/// Looser than [`fold_catalogue`] on purpose: punctuation and case vary between
/// providers and sleeves (`Mr X` against `Mr. X`), but a word that is there in one and
/// missing in the other is a real difference, so the words survive as words.
#[must_use]
pub fn fold_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.extend(c.to_lowercase());
        } else {
            space = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalogue_number_folds_across_the_ways_a_label_prints_it() {
        let folded = fold_catalogue("CHRH 1296");
        assert_eq!(folded, "CHRH1296");
        assert_eq!(fold_catalogue("chrh-1296"), folded);
        assert_eq!(fold_catalogue("CHRH1296"), folded);
        assert_eq!(fold_catalogue(" CHRH/1296 "), folded);
        // A different number must not fold onto the same string, which is the half
        // of this that a looser fold would break.
        assert_ne!(fold_catalogue("CHRH 1297"), folded);
    }

    #[test]
    fn a_title_folds_punctuation_but_not_words() {
        assert_eq!(fold_text("Mr. X"), fold_text("Mr X"));
        assert_eq!(fold_text("All Stood Still"), "all stood still");
        assert_ne!(fold_text("Vienna"), fold_text("Vienna (Single Version)"));
    }

    #[test]
    fn a_stated_fact_outweighs_a_providers_own() {
        assert!(Source::Stated.weight() > Source::Discogs.weight());
        assert_eq!(Source::Signal.weight(), Source::Stated.weight());
    }

    #[test]
    fn what_a_person_typed_can_be_read_back_without_searching_the_list() {
        let mut observed = Observed::new();
        assert!(observed.is_empty());
        observed
            .add(Source::Discogs, Fact::Catalogue("OTHER 1".into()))
            .add(Source::Stated, Fact::Catalogue("CHRH 1296".into()))
            .add(Source::Stated, Fact::Album("Vienna".into()));
        assert_eq!(
            observed.stated(Kind::Catalogue),
            Some(&Fact::Catalogue("CHRH 1296".into())),
            "the provider's catalogue number is not what the person said"
        );
        assert_eq!(observed.stated(Kind::Year), None);
        assert_eq!(observed.items().len(), 3);
    }
}
