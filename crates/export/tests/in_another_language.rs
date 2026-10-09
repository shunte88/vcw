/*
 *  in_another_language.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A refusal written in Rust, read in a language VCW was not written in.
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

//! A refusal written in Rust, read in a language VCW was not written in.
//!
//! The end-to-end claim i18n is actually making, and the one worth a test of
//! its own: not that a catalog can be parsed, but that a sentence produced
//! deep inside the exporter - from a `thiserror` variant, through
//! `alternatives`, into a `Display` - comes out in the language the settings
//! asked for, with the English still available underneath for the keys the
//! translation has not reached.
//!
//! Its own test binary because the active locale is process-wide. Sharing a
//! process with the rest of the export tests would make every one of them
//! depend on the order this ran in.

use vcw_export::encoder::{Container, Spec, Writer};
use vcw_i18n::{Catalog, activate, digest, t};
use vcw_types::StorageFormat;

/// A float capture, which is the one both lossless codecs refuse.
fn float() -> Spec {
    Spec {
        rate: 44_100,
        channels: 2,
        format: StorageFormat::Float32,
        frames: 1_000,
    }
}

/// A catalog in a language that does not exist, which is the point: nothing in
/// it can be mistaken for English leaking through.
fn elsewhere() -> Catalog {
    let source = Catalog::source();
    let english = source
        .get("export.aiff.float")
        .expect("the key is in en-US");
    Catalog::parse(
        "zz-ZZ",
        &format!(
            "[\"export.aiff.float\"]\ntext = \"NOPE ZORBLAX QUUX\"\nsource = \"{}\"\n",
            digest(english)
        ),
    )
    .expect("a catalog")
}

#[test]
fn a_refusal_comes_out_in_the_language_the_settings_asked_for() {
    activate(None);
    let before = Writer::vet(Container::Aiff, &float())
        .expect_err("AIFF will not take a float capture")
        .to_string();
    assert!(before.contains("integer container"), "{before}");

    activate(Some(elsewhere()));
    let after = Writer::vet(Container::Aiff, &float())
        .expect_err("AIFF will not take a float capture")
        .to_string();
    assert!(after.contains("NOPE ZORBLAX QUUX"), "{after}");
    assert!(
        !after.contains("integer container"),
        "the English came through as well: {after}"
    );

    // The clause `alternatives` generates is still English, because nobody has
    // translated it yet - and that is the fallback working rather than the
    // translation failing. A reader gets a half-translated message instead of
    // a half-empty one.
    assert!(after.contains("Export this one as"), "{after}");

    activate(None);
}

#[test]
fn an_untranslated_refusal_falls_back_rather_than_disappearing() {
    activate(Some(elsewhere()));
    // FLAC's refusal is in the catalog and not in `zz-ZZ`.
    let said = Writer::vet(
        Container::Flac(vcw_export::encoder::Compression::default()),
        &float(),
    )
    .expect_err("FLAC will not take a float capture either")
    .to_string();
    assert!(said.contains("integer codec"), "{said}");
    assert_eq!(t("export.flac.float"), said_without(&said));
    activate(None);
}

/// The catalog entry out of the assembled message, so the test compares the
/// string the catalog holds rather than a copy of it written here.
fn said_without(message: &str) -> String {
    let start = message
        .find("FLAC is an integer")
        .expect("the catalog text");
    let end = message
        .find(" Export this one as")
        .expect("the generated advice");
    message[start..end].to_owned()
}
