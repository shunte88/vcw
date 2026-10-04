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
//! dialog that shows it carries no licence prose of its own.
//!
//! That is the rule `encoder::alternatives` arrived at the hard way.
//! Refusal advice written as a string literal drifted four separate ways in one
//! work package, because a literal agrees with whatever it said yesterday. A
//! notice is worse than stale advice when it is wrong: it is either a claim
//! about a licence that does not apply, or silence about one that does.
//!
//! # What is not here
//!
//! Attribution-only dependencies, one by one. There are several hundred,
//! `Cargo.lock` is the inventory, and `cargo deny check licenses` is what keeps
//! the set bounded. What is here is the components a person holding a VCW binary
//! has to be *told* about: the encoders, because they are the ones a feature
//! turns off, and because one of them is weak copyleft.

use crate::encoder::Container;

/// A third-party component this build links, and what its licence asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notice {
    /// The crate, or the library it vendors, as its own authors spell it.
    pub component: &'static str,
    /// The SPDX expression the crate declares.
    pub licence: &'static str,
    /// What VCW can do because it is linked, in a person's words.
    pub provides: &'static str,
    /// Where the complete corresponding source is published.
    pub source: &'static str,
    /// Whether the licence grants a right to modify the component and relink
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
            Self::Flac => Some(Notice {
                component: "flacenc",
                licence: "Apache-2.0",
                provides: "FLAC export",
                source: "https://crates.io/crates/flacenc",
                copyleft: false,
            }),
            // The one that made WP-28 a licence obligation rather than a
            // courtesy: `mp3lame-sys` vendors libmp3lame's C source and
            // compiles it in, so a VCW binary built with this feature contains
            // LGPL-3.0 object code inside an MIT product.
            Self::Mp3(_) => Some(Notice {
                component: "libmp3lame, via mp3lame-encoder and mp3lame-sys",
                licence: "LGPL-3.0",
                provides: "MP3 export",
                source: "https://crates.io/crates/mp3lame-sys",
                copyleft: true,
            }),
            Self::OggVorbis(_) => Some(Notice {
                component: "libvorbis and libogg, via vorbis_rs",
                licence: "BSD-3-Clause",
                provides: "Ogg Vorbis export",
                source: "https://crates.io/crates/vorbis_rs",
                copyleft: false,
            }),
        }
    }
}

/// Every notice this build owes, in the order [`Container::ALL`] is offered in.
///
/// Empty is not a reachable answer: FLAC is unconditional, so a build with no
/// features at all still owes flacenc its attribution.
#[must_use]
pub fn notices() -> Vec<Notice> {
    Container::ALL
        .into_iter()
        .filter(|container| container.compiled_in())
        .filter_map(Container::notice)
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
        let mut expected = 0;
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
    /// has to stay true: the copyleft notice appears exactly when the feature
    /// that brings the copyleft code is on.
    #[test]
    fn only_a_build_with_mp3_declares_a_copyleft_component() {
        let copyleft: Vec<&'static str> = notices()
            .iter()
            .filter(|notice| notice.copyleft)
            .map(|notice| notice.licence)
            .collect();
        if cfg!(feature = "mp3") {
            assert_eq!(copyleft, ["LGPL-3.0"], "{:#?}", notices());
        } else {
            assert!(
                copyleft.is_empty(),
                "a build without `mp3` links no copyleft code and must not claim to: {copyleft:?}"
            );
        }
    }

    /// Flat attribution is still attribution, and FLAC is not optional.
    #[test]
    fn every_build_owes_flacenc_and_names_a_source_for_everything() {
        let owed = notices();
        assert!(
            owed.iter().any(|notice| notice.component == "flacenc"),
            "{owed:#?}"
        );
        for notice in &owed {
            assert!(
                notice.source.starts_with("https://"),
                "{notice:#?} names no source, which is the one thing a relink \
                 right is useless without"
            );
            assert!(!notice.licence.is_empty(), "{notice:#?}");
            assert!(!notice.provides.is_empty(), "{notice:#?}");
        }
    }
}
