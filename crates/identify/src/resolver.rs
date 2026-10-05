/*
 *  resolver.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Choosing between candidates, deciding what to ask next, and recording why.
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

//! Choosing between candidates, deciding what to ask next, and recording why.
//!
//! Requirements: §26 (progressive identification, never silently replacing what a
//! person confirmed), §27 (what release, side and track are being recorded), §28
//! (providers), §40 (a network request is something a person asks for).
//!
//! # Two decisions, not one
//!
//! [`next_lookup`] decides what to ask next and [`resolve`] decides what the answers
//! mean. Both are pure functions over [`Observed`], which is the whole reason the
//! escalation order is testable at all: the order in which VCW consults Discogs,
//! MusicBrainz and AcoustID is a design decision with consequences for cost and for
//! correctness, and it is asserted here rather than emerging from the order somebody
//! wrote the calls in.
//!
//! # Declining is a result
//!
//! Three outcomes and no fourth. The failure mode worth designing against is not
//! "could not identify the record", which is ordinary and recoverable; it is a
//! plausible wrong pressing written silently over a catalog number somebody read
//! off the label. So a tie asks, a contradiction asks, and asking is not a fallback.

use crate::candidate::{Assessed, Verdict};
use crate::confidence::{Confidence, MARGIN, Stance, weigh};
use crate::evidence::{Kind, Observed, Source};

/// How many candidates a question offers.
///
/// A person choosing between pressings is reading sleeve details off a screen; past
/// about five the list stops being a question and becomes a search result.
pub const SHORTLIST: usize = 5;

/// One lookup VCW can make.
///
/// In escalation order, cheapest and most reliable first. See [`Lookup::ORDER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// Discogs, by the catalog number off the label.
    ///
    /// First because it is the only lookup that identifies a *pressing* rather than
    /// a work, and because Discogs indexes catalog numbers and has the better
    /// vinyl coverage of the two databases.
    CatalogAtDiscogs,
    /// Discogs, by artist and title.
    SearchDiscogs,
    /// MusicBrainz, by artist and title.
    ///
    /// Not a duplicate of the Discogs search: MusicBrainz is where recording ids
    /// live, and a recording id is what makes a later AcoustID answer comparable to
    /// this release rather than merely plausible.
    SearchMusicBrainz,
    /// AcoustID, from the audio.
    ///
    /// Last, and only when the release was not found by name at all. Measured on
    /// `/data2/source_rips`, about a third of real rips do not resolve at
    /// MusicBrainz from artist and album, and that minority is who this is for. It
    /// is also by far the most expensive: a text lookup is one request, while
    /// identifying a record from audio alone cost 63 to 138 in the prototypes,
    /// because it has to search for its own alignment.
    IdentifyAudio,
}

impl Lookup {
    /// Every lookup, in escalation order.
    pub const ORDER: [Self; 4] = [
        Self::CatalogAtDiscogs,
        Self::SearchDiscogs,
        Self::SearchMusicBrainz,
        Self::IdentifyAudio,
    ];

    /// Whether this lookup has the evidence it needs to be worth making.
    ///
    /// A catalog lookup with no catalog number is not a cheap failure, it is a
    /// request that cannot succeed, and §40 says a request is something a person
    /// asked for.
    #[must_use]
    pub fn is_possible(self, observed: &Observed) -> bool {
        match self {
            Self::CatalogAtDiscogs => observed.stated(Kind::Catalog).is_some(),
            Self::SearchDiscogs | Self::SearchMusicBrainz => {
                observed.stated(Kind::Artist).is_some() || observed.stated(Kind::Album).is_some()
            }
            // The audio is always there. Whether it is worth fingerprinting is the
            // caller's call, not this function's.
            Self::IdentifyAudio => true,
        }
    }

    /// The lowercase token used in JSON and in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CatalogAtDiscogs => "discogs-catalog",
            Self::SearchDiscogs => "discogs-search",
            Self::SearchMusicBrainz => "musicbrainz-search",
            Self::IdentifyAudio => "acoustid",
        }
    }
}

/// The next lookup worth making, given what is known and what has been tried.
///
/// `None` means there is nothing left to ask, which is a finished state and not an
/// error: a record that is in no database is still a record.
#[must_use]
pub fn next_lookup(observed: &Observed, tried: &[Lookup]) -> Option<Lookup> {
    Lookup::ORDER
        .into_iter()
        .find(|step| !tried.contains(step) && step.is_possible(observed))
}

/// A candidate with its score.
#[derive(Debug, Clone)]
pub struct Ranked {
    /// The candidate and its account of the evidence.
    pub assessed: Assessed,
    /// How well it accounts for it.
    pub confidence: Confidence,
}

/// What the evidence supports.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// One release, well enough supported to state.
    Resolved {
        /// The release, and the agreements that chose it.
        chosen: Box<Ranked>,
    },
    /// Several releases worth a person's attention, best first.
    Ask {
        /// At most [`SHORTLIST`] candidates, ranked.
        shortlist: Vec<Ranked>,
        /// Why it is a question rather than an answer.
        because: Doubt,
    },
    /// Nothing the evidence supports at all.
    Nothing,
}

/// Why the resolver declined to choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doubt {
    /// The best candidate was not well enough supported on its own.
    NotEnoughEvidence,
    /// Two candidates were too close to separate, which is the ordinary shape of
    /// two pressings of one record.
    TooClose,
    /// The best candidate disputed something a person stated (§26).
    ContradictsAPerson,
}

/// Picks a release, or declines to.
#[must_use]
pub fn resolve(assessed: Vec<Assessed>) -> Outcome {
    let mut ranked: Vec<Ranked> = assessed
        .into_iter()
        .map(|assessed| Ranked {
            confidence: weigh(&assessed),
            assessed,
        })
        .collect();
    // `total_cmp` rather than `partial_cmp`: a NaN would otherwise make the sort
    // order depend on the input order, and a resolver whose answer depends on the
    // order the providers happened to answer in is not diagnosable.
    ranked.sort_by(|a, b| {
        b.confidence
            .value()
            .total_cmp(&a.confidence.value())
            .then_with(|| a.assessed.claim.album.cmp(&b.assessed.claim.album))
    });
    ranked.retain(|r| r.confidence.stance() != Stance::Ask);
    let Some(best) = ranked.first() else {
        return Outcome::Nothing;
    };

    let runner_up = ranked.get(1).map_or(0.0, |r| r.confidence.value());
    // Most specific reason first. Two candidates a hair apart and both short of
    // certainty are both "too close" and "not enough", and the first of those is the
    // one that tells a person what to do about it.
    let because = if best.assessed.contradicts_a_person() {
        Some(Doubt::ContradictsAPerson)
    } else if best.confidence.value() - runner_up < MARGIN {
        Some(Doubt::TooClose)
    } else if best.confidence.stance() == Stance::Certain {
        None
    } else {
        Some(Doubt::NotEnoughEvidence)
    };

    match because {
        None => Outcome::Resolved {
            chosen: Box::new(ranked.swap_remove(0)),
        },
        Some(because) => {
            ranked.truncate(SHORTLIST);
            Outcome::Ask {
                shortlist: ranked,
                because,
            }
        }
    }
}

/// A one-line account of why a candidate scored what it did, for a report or a log.
///
/// Reads the agreements rather than the score, because the score is the thing being
/// explained. §26's "evidence-based rather than a single-match decision" is only true
/// if the evidence can be printed.
#[must_use]
pub fn because(ranked: &Ranked) -> String {
    let mut agreed: Vec<&str> = Vec::new();
    let mut against: Vec<&str> = Vec::new();
    for agreement in &ranked.assessed.agreements {
        let name = name_of(agreement.fact.kind());
        let into = match agreement.verdict {
            Verdict::Agrees => &mut agreed,
            Verdict::Disagrees => &mut against,
            Verdict::Silent => continue,
        };
        if !into.contains(&name) {
            into.push(name);
        }
    }
    let stated = ranked
        .assessed
        .agreements
        .iter()
        .any(|a| a.source == Source::Stated);
    let mut out = format!("{:.2}", ranked.confidence.value());
    if !agreed.is_empty() {
        out.push_str(" agrees on ");
        out.push_str(&agreed.join(", "));
    }
    if !against.is_empty() {
        out.push_str(" but disputes ");
        out.push_str(&against.join(", "));
    }
    if agreed.is_empty() && against.is_empty() {
        out.push_str(if stated {
            " accounts for nothing stated"
        } else {
            " with nothing to go on"
        });
    }
    out
}

/// The word a report uses for a kind of fact.
const fn name_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Artist => "artist",
        Kind::Album => "title",
        Kind::Catalog => "catalog number",
        Kind::Label => "label",
        Kind::Year => "year",
        Kind::Count => "track count",
        Kind::Side => "side length",
        Kind::Recording => "identified audio",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::{Claim, assess};
    use crate::evidence::{Fact, Observed, Source};

    fn sleeve() -> Observed {
        let mut observed = Observed::new();
        observed
            .add(Source::Stated, Fact::Artist("Ultravox".into()))
            .add(Source::Stated, Fact::Album("Vienna".into()))
            .add(Source::Stated, Fact::Catalog("CHRH 1296".into()))
            .add(Source::Signal, Fact::Count(9));
        observed
    }

    fn pressing(id: &str, catalog: &str) -> Claim {
        Claim {
            ids: vec![id.into()],
            artist: "Ultravox".into(),
            album: "Vienna".into(),
            catalog: catalog.into(),
            label: "Chrysalis".into(),
            year: Some(1980),
            tracks: Some(9),
            sides: Vec::new(),
        }
    }

    #[test]
    fn the_escalation_order_is_catalog_then_discogs_then_musicbrainz_then_audio() {
        let observed = sleeve();
        let mut tried: Vec<Lookup> = Vec::new();
        let mut order: Vec<Lookup> = Vec::new();
        while let Some(step) = next_lookup(&observed, &tried) {
            order.push(step);
            tried.push(step);
        }
        assert_eq!(order, Lookup::ORDER.to_vec());
        assert_eq!(next_lookup(&observed, &tried), None, "nothing left to ask");
    }

    #[test]
    fn a_lookup_with_nothing_to_look_up_is_never_made() {
        let mut observed = Observed::new();
        observed.add(Source::Stated, Fact::Artist("Ultravox".into()));
        assert_eq!(
            next_lookup(&observed, &[]),
            Some(Lookup::SearchDiscogs),
            "no catalog number was typed, so there is nothing to search by number"
        );

        // A record with nothing typed at all: the audio is the only evidence there
        // is, which is the population the fallback exists for.
        let nothing = Observed::new();
        assert_eq!(next_lookup(&nothing, &[]), Some(Lookup::IdentifyAudio));
    }

    #[test]
    fn a_whole_sleeve_agreeing_resolves() {
        let outcome = resolve(vec![assess(pressing("1", "CHRH 1296"), &sleeve())]);
        let Outcome::Resolved { chosen } = outcome else {
            panic!("expected a resolution, got {outcome:?}");
        };
        assert_eq!(chosen.assessed.claim.ids, vec!["1".to_owned()]);
        assert!(
            because(&chosen).contains("catalog number"),
            "{}",
            because(&chosen)
        );
    }

    #[test]
    fn two_pressings_that_cannot_be_separated_are_a_question() {
        // Neither provider recorded a catalog number, which is the ordinary case
        // for an old pressing and the reason this is not a coin toss.
        let mut observed = Observed::new();
        observed
            .add(Source::Stated, Fact::Artist("Ultravox".into()))
            .add(Source::Stated, Fact::Album("Vienna".into()))
            .add(Source::Signal, Fact::Count(9));
        let outcome = resolve(vec![
            assess(pressing("original", ""), &observed),
            assess(pressing("reissue", ""), &observed),
        ]);
        let Outcome::Ask { shortlist, because } = outcome else {
            panic!("expected a question, got {outcome:?}");
        };
        assert_eq!(because, Doubt::TooClose);
        assert_eq!(shortlist.len(), 2);
    }

    #[test]
    fn a_candidate_that_disputes_the_sleeve_is_asked_about_and_not_asserted() {
        let outcome = resolve(vec![assess(pressing("wrong", "XYZ 999"), &sleeve())]);
        match outcome {
            Outcome::Ask { because, .. } => assert_eq!(because, Doubt::ContradictsAPerson),
            Outcome::Nothing => {}
            other => panic!("a disputed catalog number must never resolve: {other:?}"),
        }
    }

    #[test]
    fn a_clear_winner_beats_a_weaker_candidate_without_asking() {
        let outcome = resolve(vec![
            assess(pressing("right", "CHRH 1296"), &sleeve()),
            assess(pressing("other", ""), &sleeve()),
        ]);
        let Outcome::Resolved { chosen } = outcome else {
            panic!("expected a resolution, got {outcome:?}");
        };
        assert_eq!(chosen.assessed.claim.ids, vec!["right".to_owned()]);
    }

    #[test]
    fn nothing_worth_showing_is_nothing_rather_than_a_bad_question() {
        assert!(matches!(resolve(Vec::new()), Outcome::Nothing));
        let mut observed = Observed::new();
        observed.add(Source::Stated, Fact::Artist("Somebody Else".into()));
        assert!(
            matches!(
                resolve(vec![assess(pressing("1", ""), &observed)]),
                Outcome::Nothing
            ),
            "a candidate nobody would accept is not a shortlist of one"
        );
    }

    #[test]
    fn a_shortlist_does_not_grow_past_what_a_person_can_read() {
        let mut observed = Observed::new();
        observed
            .add(Source::Stated, Fact::Artist("Ultravox".into()))
            .add(Source::Stated, Fact::Album("Vienna".into()))
            .add(Source::Signal, Fact::Count(9));
        let many: Vec<Assessed> = (0..12)
            .map(|n| assess(pressing(&format!("p{n}"), ""), &observed))
            .collect();
        let Outcome::Ask { shortlist, .. } = resolve(many) else {
            panic!("twelve identical pressings must be a question");
        };
        assert_eq!(shortlist.len(), SHORTLIST);
    }
}
