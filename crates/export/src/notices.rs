/*
 *  notices.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The third-party notices this build owes, derived from its cargo features.
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

//! The third-party notices this build owes, derived from its cargo features.
//!
//! `THIRD-PARTY-NOTICES.md` is the long form and the authority; this is the
//! short form a running binary can show, and it is here rather than written out
//! in the shell for one reason: **a build compiled without `mp3` has no LGPL
//! component to declare.** The only place that question can be answered is the
//! crate the features belong to, so the answer is generated here and the
//! dialog that shows it carries no license prose of its own.
//!
//! That is the rule `encoder::alternatives` arrived at the hard way.
//! Refusal advice written as a string literal drifted four separate ways in one
//! work package, because a literal agrees with whatever it said yesterday. A
//! notice is worse than stale advice when it is wrong: it is either a claim
//! about a license that does not apply, or silence about one that does.
//!
//! # What is not here
//!
//! Attribution-only dependencies, one by one. There are several hundred,
//! `Cargo.lock` is the inventory, and `cargo deny check licenses` is what keeps
//! the set bounded. What is here is the components a person holding a VCW binary
//! has to be *told* about: the encoders, because they are the ones a feature
//! turns off, and because one of them is weak copyleft.

use crate::encoder::Container;

/// A third-party component this build links, and what its license asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notice {
    /// The crate, or the library it vendors, as its own authors spell it.
    pub component: &'static str,
    /// The SPDX expression the crate declares.
    pub license: &'static str,
    /// What VCW can do because it is linked, in a person's words.
    pub provides: &'static str,
    /// Where the complete corresponding source is published.
    pub source: &'static str,
    /// Whether the license grants a right to modify the component and relink
    /// it into VCW.
    ///
    /// A field rather than a sentence, because the sentence is the same for
    /// every component that has the right and absent for every one that does
    /// not. LGPL-3.0 section 4 and LGPL-2.1 section 6 both require that the
    /// right be *offered*, which is a thing a dialog has to say out loud rather
    /// than leave to be inferred from an SPDX string.
    pub copyleft: bool,
}

impl Container {
    /// The notice linking this container's encoder incurs, where there is one.
    ///
    /// `None` for WAV: RIFF is written by this crate and nothing else.
    ///
    /// Note what this does *not* depend on - whether the feature is enabled.
    /// The obligation a container would bring is a fact about the container;
    /// which of them this build actually brings is [`notices`]'s question, and
    /// keeping the two apart is what lets the test below check the filter
    /// rather than check itself.
    #[must_use]
    pub const fn notice(self) -> Option<Notice> {
        match self {
            Self::Wav => None,
            Self::Flac(_) => Some(Notice {
                component: "flac-codec",
                license: "MIT OR Apache-2.0",
                provides: "FLAC export",
                source: "https://crates.io/crates/flac-codec",
                copyleft: false,
            }),
            // The one that made WP-28 a license obligation rather than a
            // courtesy: `mp3lame-sys` vendors libmp3lame's C source and
            // compiles it in, so a VCW binary built with this feature contains
            // LGPL-3.0 object code inside an MIT product.
            Self::Mp3(_) => Some(Notice {
                component: "libmp3lame, via mp3lame-encoder and mp3lame-sys",
                license: "LGPL-3.0",
                provides: "MP3 export",
                source: "https://crates.io/crates/mp3lame-sys",
                copyleft: true,
            }),
            Self::OggVorbis(_) => Some(Notice {
                component: "libvorbis and libogg, via vorbis_rs",
                license: "BSD-3-Clause",
                provides: "Ogg Vorbis export",
                source: "https://crates.io/crates/vorbis_rs",
                copyleft: false,
            }),
        }
    }
}

/// The notices no feature can switch off.
///
/// Chromaprint is not a container and has no feature behind it: `vcw-core`
/// depends on `vcw-fingerprint`, which depends on `chromaprint-next`, so every
/// VCW binary - the shell included, which does not name either crate in its own
/// manifest - links it. A dialog that derives its list from the encoders alone
/// therefore said nothing about the one component here whose license asks to be
/// spoken about: LGPL-2.1 section 6 wants the relink offer *made*, not inferred.
const ALWAYS: [Notice; 1] = [Notice {
    component: "libchromaprint, via chromaprint-next",
    license: "LGPL-2.1-or-later",
    provides: "acoustic fingerprinting",
    source: "https://crates.io/crates/chromaprint-next",
    copyleft: true,
}];

/// Every notice this build owes: `ALWAYS`, then the containers it was
/// compiled with in the order [`Container::ALL`] offers them.
///
/// Empty is not a reachable answer, and not only because FLAC is unconditional:
/// `ALWAYS` is owed by a build with no features at all.
#[must_use]
pub fn notices() -> Vec<Notice> {
    ALWAYS
        .into_iter()
        .chain(
            Container::ALL
                .into_iter()
                .filter(|container| container.compiled_in())
                .filter_map(Container::notice),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WP-28's exit criterion: what a binary says it links agrees with what it
    /// was compiled with.
    ///
    /// Run in each of the four feature combinations by the gate's `features`
    /// leg, which is the whole reason it can fail: in the default build every
    /// container is compiled in, so a filter that did nothing would pass.
    #[test]
    fn the_notices_agree_with_the_features_this_build_was_compiled_with() {
        let owed = notices();
        let mut expected = ALWAYS.len();
        for container in Container::ALL {
            let Some(notice) = container.notice() else {
                continue;
            };
            let listed = owed.iter().filter(|n| **n == notice).count();
            assert_eq!(
                listed,
                usize::from(container.compiled_in()),
                "{container}: compiled_in is {}, and the notices name it {listed} time(s)",
                container.compiled_in()
            );
            expected += usize::from(container.compiled_in());
        }
        assert_eq!(
            owed.len(),
            expected,
            "the notices carry something no container accounts for: {owed:#?}"
        );
    }

    /// The obligation the dialog exists for, stated as the one sentence that
    /// has to stay true: a copyleft notice appears exactly when the code it
    /// speaks for is linked.
    ///
    /// This test used to say `copyleft.is_empty()` for a build without `mp3`,
    /// and it passed for as long as it was wrong. Chromaprint came in through
    /// `vcw-core` with no feature in front of it, so *every* build has owed an
    /// LGPL-2.1 relink offer since WP-21 and the dialog made none.
    #[test]
    fn a_copyleft_notice_appears_exactly_when_its_code_is_linked() {
        let copyleft: Vec<&'static str> = notices()
            .iter()
            .filter(|notice| notice.copyleft)
            .map(|notice| notice.license)
            .collect();
        let expected: &[&str] = if cfg!(feature = "mp3") {
            &["LGPL-2.1-or-later", "LGPL-3.0"]
        } else {
            &["LGPL-2.1-or-later"]
        };
        assert_eq!(copyleft, expected, "{:#?}", notices());
    }

    /// Fingerprinting has no feature gate, so no build may be silent about it.
    #[test]
    fn every_build_declares_chromaprint() {
        assert!(
            notices()
                .iter()
                .any(|notice| notice.component.contains("chromaprint") && notice.copyleft),
            "{:#?}",
            notices()
        );
    }

    /// Flat attribution is still attribution, and FLAC is not optional.
    #[test]
    fn every_build_owes_the_flac_encoder_and_names_a_source_for_everything() {
        let owed = notices();
        assert!(
            owed.iter().any(|notice| notice.component == "flac-codec"),
            "{owed:#?}"
        );
        for notice in &owed {
            assert!(
                notice.source.starts_with("https://"),
                "{notice:#?} names no source, which is the one thing a relink \
                 right is useless without"
            );
            assert!(!notice.license.is_empty(), "{notice:#?}");
            assert!(!notice.provides.is_empty(), "{notice:#?}");
        }
    }
}
