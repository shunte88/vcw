/*
 *  conformance.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §51's tolerance, proved on every curve at every rate, and on the audio itself.
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

//! §51's claim, checked.
//!
//! > The implemented response shall be measured against that source and shall be
//! > within +/-0.5 dB from 20 Hz to 20 kHz, at every sample rate §8 requires.
//!
//! "Measured against that source" means against the analytic response of the time
//! constants the citation gives, and not against a published dB column: most of those
//! describe the treble section alone, NAB's are referenced to 700 Hz, and a trailing
//! `-0` in a code means there is no treble network rather than 0 dB at 10 kHz. See
//! `docs/design/WP-29-playback-equalization.md`.

use vcw_eq::{
    Catalog, Filter, IEC_RUMBLE, IEC_RUMBLE_US, RUMBLE_IDS, TOLERANCE_DB, worst_error_db,
    worst_rumble_error_db,
};
use vcw_types::format::CaptureEq;

/// Every rate §8 requires.
const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

#[test]
fn the_builtin_catalog_parses_and_every_curve_cites_a_source() {
    let catalog = Catalog::builtin();
    assert!(catalog.len() >= 16, "catalog shrank to {}", catalog.len());
    for curve in catalog.iter() {
        assert!(
            !curve.citation.trim().is_empty(),
            "{} has no citation",
            curve.id
        );
    }
    // §51 names these by name. Flat, RIAA, Columbia LP, Decca FFRR, EMI, HMV, AES,
    // NAB/NARTB, Teldec - with EMI and HMV being one characteristic, which is the
    // finding that let a single entry answer for both.
    for id in [
        "flat",
        "riaa",
        "columbia_lp",
        "decca_ffrr_78",
        "emi",
        "aes",
        "nab",
        "teldec",
    ] {
        assert!(catalog.get(id).is_some(), "§51 requires a '{id}' curve");
    }
}

#[test]
fn every_curve_meets_the_tolerance_at_every_rate() {
    let catalog = Catalog::builtin();
    let mut worst = (0.0f64, String::new(), 0u32);
    for curve in catalog.iter() {
        for rate in RATES {
            let error = worst_error_db(curve, rate).expect("design");
            assert!(
                error < TOLERANCE_DB,
                "{} at {} Hz is {:.4} dB out, and §51 allows {}",
                curve.id,
                rate,
                error,
                TOLERANCE_DB
            );
            if error > worst.0 {
                worst = (error, curve.id.clone(), rate);
            }
        }
    }
    // Not just inside the budget but comfortably inside it: if a change to the design
    // eats the margin, that is worth knowing before it eats the tolerance.
    assert!(
        worst.0 < TOLERANCE_DB / 2.0,
        "worst error {:.4} dB ({} at {} Hz) has used over half the budget",
        worst.0,
        worst.1,
        worst.2
    );
}

#[test]
fn riaa_reproduces_its_published_figures() {
    let catalog = Catalog::builtin();
    let riaa = catalog.require("riaa").expect("riaa");
    // The one curve whose published dB figures are whole-curve values referenced to
    // 1 kHz, which is what makes it the only one usable as an external check.
    assert!((riaa.response_db(20.0) - 19.274).abs() < 0.01);
    assert!((riaa.response_db(10_000.0) + 13.7).abs() < 0.05);
}

#[test]
fn the_code_letter_predicts_the_published_shelf() {
    // The letter in a code like 500C-16 states the player's maximum bass boost, so it
    // predicts the shelf: t1 = t2 * 10^(dB/20). This is an independent check on the
    // one parameter the sources fight over - Columbia LP's shelf, where Galo's 1590 us
    // is the letter's value to 0.02 dB and the competing 1950 us is no letter at all.
    let catalog = Catalog::builtin();
    for (id, boost_db) in [
        ("riaa", 20.0),
        ("nab", 20.0),
        ("columbia_lp", 14.0),
        ("teldec", 20.0),
        ("decca_lp", 14.0),
        ("london_lp", 14.0),
    ] {
        let curve = catalog.require(id).expect(id);
        let (shelf, turnover) = (
            curve.shelf_us.expect("shelf"),
            curve.turnover_us.expect("turnover"),
        );
        let implied = 20.0 * (shelf / turnover).log10();
        assert!(
            (implied - boost_db).abs() < 0.1,
            "{id}: shelf implies {implied:.2} dB where its code says {boost_db:.0} dB"
        );
    }
}

#[test]
fn a_shelfless_curve_is_given_a_replay_limit_rather_than_a_dc_pole() {
    let catalog = Catalog::builtin();
    let emi = catalog.require("emi").expect("emi");
    assert!(emi.shelf_us.is_none(), "EMI is cut with no shelf");
    // Left as written it would rise without bound. With the replay shelf it flattens,
    // and the gain at 0.1 Hz is the gain at 1 Hz.
    let low = emi.response_db(0.1);
    assert!(
        (low - emi.response_db(1.0)).abs() < 0.5,
        "the replay shelf has not flattened the bottom end: {low:.1} dB at 0.1 Hz"
    );
    // `replay_shelf_db` states the shelf-to-turnover ratio, as the code letters do,
    // so the realized boost sits a few tenths under it: 1 kHz is not quite on the
    // turnover's asymptote. What matters is that it settles at all.
    let shelf = emi.replay_shelf_s().expect("a synthesized shelf");
    let turnover = emi.turnover_us.expect("turnover") * 1e-6;
    assert!((20.0 * (shelf / turnover).log10() - emi.replay_shelf_db).abs() < 1e-9);
    assert!(
        (low - emi.replay_shelf_db).abs() < 0.5,
        "the shelf settled at {low:.2} dB, nowhere near the stated {}",
        emi.replay_shelf_db
    );
    for rate in RATES {
        Filter::playback(&catalog, "emi", None, CaptureEq::Flat, rate, 2).expect("stable design");
    }
}

#[test]
fn the_audio_leaving_the_filter_matches_the_curve() {
    // The tests above read coefficients. This one reads samples: it runs sine waves
    // through `process` and measures what comes out, which is the only check that the
    // running filter is the filter that was designed.
    let catalog = Catalog::builtin();
    let rate = 44_100u32;
    for id in ["riaa", "columbia_lp", "emi", "decca_ffrr_78"] {
        let curve = catalog.require(id).expect(id);
        let reference = measure(&catalog, id, rate, 1000.0);
        for hz in [30.0, 100.0, 500.0, 3000.0, 10_000.0, 18_000.0] {
            let got = 20.0 * (measure(&catalog, id, rate, hz) / reference).log10();
            let want = curve.response_db(hz);
            assert!(
                (got - want).abs() < TOLERANCE_DB,
                "{id} at {hz} Hz: the audio measures {got:.3} dB, the curve says {want:.3} dB"
            );
        }
    }
}

/// Steady-state amplitude of a sine at `hz` after passing through the filter.
fn measure(catalog: &Catalog, id: &str, rate: u32, hz: f64) -> f64 {
    let mut filter = Filter::playback(catalog, id, None, CaptureEq::Flat, rate, 1).expect("design");
    let cycles = 400.0;
    let total = ((f64::from(rate) / hz) * cycles) as usize;
    let mut audio: Vec<f32> = (0..total)
        .map(|n| (2.0 * std::f64::consts::PI * hz * n as f64 / f64::from(rate)).sin() as f32)
        .collect();
    filter.process(&mut audio);
    // Skip the first half so the filter has settled, then take the peak.
    audio[total / 2..]
        .iter()
        .fold(0.0f64, |peak, s| peak.max(f64::from(s.abs())))
}

#[test]
fn riaa_played_back_from_an_riaa_capture_changes_nothing() {
    let catalog = Catalog::builtin();
    let mut filter =
        Filter::playback(&catalog, "riaa", None, CaptureEq::Riaa, 48_000, 2).expect("design");
    assert!(!filter.inverts_riaa(), "nothing is being inverted here");
    for hz in [20.0, 1000.0, 20_000.0] {
        assert!(
            filter.response_db(hz).abs() < 0.01,
            "a no-op moved {hz} Hz by {:.3} dB",
            filter.response_db(hz)
        );
    }
    let mut audio = [0.25f32; 256];
    filter.process(&mut audio);
    assert!(audio.iter().all(|s| (s - 0.25).abs() < 1e-3));
}

#[test]
fn another_curve_over_an_riaa_capture_inverts_riaa_and_says_so() {
    let catalog = Catalog::builtin();
    let filter =
        Filter::playback(&catalog, "teldec", None, CaptureEq::Riaa, 96_000, 2).expect("design");
    assert!(filter.inverts_riaa());
    // §51 wants the noise cost stated, not implied. Teldec differs from RIAA only in
    // the treble constant, so the net is a treble lift: about +2.8 dB at 10 kHz.
    let lift = filter.response_db(10_000.0);
    assert!(
        (lift - 2.83).abs() < 0.1,
        "expected about +2.83 dB at 10 kHz, got {lift:.2}"
    );
}

#[test]
fn a_capture_whose_provenance_was_never_stated_is_refused() {
    let catalog = Catalog::builtin();
    let error = Filter::playback(&catalog, "riaa", None, CaptureEq::Unknown, 48_000, 2)
        .expect_err("§51 requires the operator to state provenance");
    let message = error.to_string();
    assert!(message.contains("never recorded"), "{message}");
    // The refusal has to name the way out, or it is just a wall.
    assert!(
        message.contains("flat") && message.contains("RIAA"),
        "{message}"
    );
}

#[test]
fn the_flat_entry_passes_audio_through_untouched() {
    let catalog = Catalog::builtin();
    assert!(catalog.require("flat").expect("flat").is_flat());
    let mut filter =
        Filter::playback(&catalog, "flat", None, CaptureEq::Flat, 44_100, 2).expect("design");
    let original: Vec<f32> = (0..512).map(|n| (n as f32 / 97.0).sin()).collect();
    let mut audio = original.clone();
    filter.process(&mut audio);
    assert_eq!(audio, original);
}

#[test]
fn an_unknown_curve_names_the_ones_that_exist() {
    let catalog = Catalog::builtin();
    let message = catalog
        .require("decca")
        .expect_err("no such curve")
        .to_string();
    assert!(message.contains("decca_ffrr_78"), "{message}");
}

#[test]
fn a_user_file_adds_curves_and_replaces_built_in_ones() {
    let user = Catalog::parse(
        r#"
        [[curve]]
        id = "my_test_disc"
        name = "My test disc"
        turnover_us = 500
        citation = "measured from a 1 kHz reference tone, 2026-10-12"

        [[curve]]
        id = "riaa"
        name = "RIAA, as my phono stage actually builds it"
        shelf_us = 3200
        turnover_us = 318
        treble_us = 75
        citation = "measured on the bench"
        "#,
    )
    .expect("a well formed user file");

    let catalog = Catalog::builtin().with_overrides(user);
    assert!(
        catalog.get("my_test_disc").is_some(),
        "the new curve is there"
    );
    assert_eq!(
        catalog.require("riaa").expect("riaa").shelf_us,
        Some(3200.0),
        "the user's RIAA replaced the shipped one"
    );
    // Replacing rather than appending: still one riaa.
    assert_eq!(catalog.ids().filter(|id| *id == "riaa").count(), 1);
}

#[test]
fn a_curve_without_a_citation_is_refused() {
    let error = Catalog::parse(
        "[[curve]]\nid = \"x\"\nname = \"X\"\nturnover_us = 500\ncitation = \"  \"\n",
    )
    .expect_err("§51 requires a citation");
    assert!(error.to_string().contains("citation"), "{error}");
}

#[test]
fn a_shelf_below_its_turnover_is_refused() {
    // The commonest way to mistype a curve: the shelf is the *larger* time constant.
    let error = Catalog::parse(
        "[[curve]]\nid = \"x\"\nname = \"X\"\nshelf_us = 100\nturnover_us = 318\n\
         treble_us = 75\ncitation = \"a typo\"\n",
    )
    .expect_err("an inverted curve");
    assert!(error.to_string().contains("cut bass"), "{error}");
}

#[test]
fn two_curves_with_one_id_are_refused() {
    let error = Catalog::parse(
        "[[curve]]\nid = \"x\"\nname = \"X\"\nturnover_us = 500\ncitation = \"a\"\n\
         [[curve]]\nid = \"x\"\nname = \"Y\"\nturnover_us = 600\ncitation = \"b\"\n",
    )
    .expect_err("a duplicate id");
    assert!(error.to_string().contains("both called"), "{error}");
}

/// §51's rule for an unstated curve over an unknown capture, which is every
/// project made before the catalog existed. Nothing is claimed, so nothing is
/// done - and the moment somebody does claim a curve, the unknown provenance is
/// refused rather than guessed at.
#[test]
fn a_decision_nobody_made_changes_nothing() {
    let catalog = Catalog::builtin();
    let wanted = |stated, capture| Filter::wanted(&catalog, stated, None, capture, 48_000, 2);

    assert!(
        wanted(None, CaptureEq::Unknown)
            .expect("silence is not an error")
            .is_none(),
        "an untouched project must play exactly as it did before"
    );
    assert!(
        wanted(None, CaptureEq::Riaa)
            .expect("the common case")
            .is_none(),
        "RIAA cut, RIAA phono stage: already flat"
    );
    assert!(
        wanted(Some("flat"), CaptureEq::Flat)
            .expect("nothing either way")
            .is_none(),
        "no curve cut, no curve applied"
    );
    assert!(
        wanted(None, CaptureEq::Flat).expect("a real job").is_some(),
        "a flat capture of an ordinary record still needs RIAA playing back"
    );
    assert!(
        wanted(Some("columbia_lp"), CaptureEq::Riaa)
            .expect("a real job")
            .is_some(),
        "a pre-RIAA record through a RIAA phono stage needs both"
    );

    let error = wanted(Some("columbia_lp"), CaptureEq::Unknown)
        .expect_err("a stated curve over an unstated chain");
    // The refusal has to tell an operator what to do about it, not just that it
    // happened: the two answers it wants are the whole fix.
    let text = error.to_string();
    assert!(text.contains("never recorded"), "{text}");
    assert!(text.contains("flat") && text.contains("RIAA"), "{text}");
}

/// The two entry points are the same filter. A curve applied to stored bytes has
/// to land where the same curve applied to floats lands, or the audio that is
/// exported is not the audio that was auditioned.
#[test]
fn stored_bytes_and_floats_take_the_same_curve() {
    let catalog = Catalog::builtin();
    // Int32 so the comparison is about the filter and not about quantizing: one
    // LSB here is -186 dBFS, far below anything the arithmetic does.
    let format = vcw_types::format::StorageFormat::Int32;
    let samples: Vec<f32> = (0..2_000)
        .map(|i| (f64::from(i) * 0.05).sin() as f32 * 0.4)
        .collect();

    let mut floats = samples.clone();
    Filter::playback(&catalog, "riaa", None, CaptureEq::Flat, 48_000, 2)
        .expect("design")
        .process(&mut floats);

    let mut bytes = vec![0u8; samples.len() * format.bytes_per_sample()];
    for (sample, slot) in samples
        .iter()
        .zip(bytes.chunks_exact_mut(format.bytes_per_sample()))
    {
        assert!(format.encode_sample(*sample, slot));
    }
    Filter::playback(&catalog, "riaa", None, CaptureEq::Flat, 48_000, 2)
        .expect("design")
        .process_stored(&mut bytes, format);

    for (i, want) in floats.iter().enumerate() {
        let got = format.decode_sample(&bytes, i).expect("written above");
        assert!(
            (got - want).abs() < 1e-5,
            "sample {i}: stored path gave {got}, float path gave {want}"
        );
    }
}

/// A filter that does nothing must cost nothing, including not quantizing. A
/// `flat` curve over a `Flat` capture has no sections, and `process_stored` has
/// to return the bytes it was handed rather than a requantized copy of them.
#[test]
fn an_empty_filter_leaves_the_bytes_alone() {
    let catalog = Catalog::builtin();
    let format = vcw_types::format::StorageFormat::Int16;
    let original: Vec<u8> = (0..64u8).collect();
    let mut bytes = original.clone();
    Filter::playback(&catalog, "flat", None, CaptureEq::Flat, 48_000, 2)
        .expect("design")
        .process_stored(&mut bytes, format);
    assert_eq!(bytes, original, "a flat curve rewrote the audio");
}

/// §51's other clause: the amendment's rumble filter, held to the same tolerance.
///
/// The number it is measured against is not read off a chart. A single real pole
/// at `-1/t4` is down 3.01 dB where `w = 1/t4`, exactly, so 7950 us predicts
/// -3.0103 dB at 20.019 Hz and the published "-3 dB at 20 Hz" is the rounded
/// version of that. The test asserts the arithmetic, which is what the filter has
/// to realize.
#[test]
fn the_rumble_filter_meets_the_tolerance_at_every_rate() {
    let corner = 1e6 / (2.0 * std::f64::consts::PI * IEC_RUMBLE_US);
    assert!(
        (corner - 20.019).abs() < 0.001,
        "7950 us is a corner at {corner:.4} Hz, not 20.019"
    );

    for id in RUMBLE_IDS {
        for rate in RATES {
            let error = worst_rumble_error_db(id, rate).expect("design");
            assert!(
                error < TOLERANCE_DB,
                "the {id} rumble filter at {rate} Hz is {error:.4} dB out, \
                 and §51 allows {TOLERANCE_DB}"
            );
        }
    }

    // At the corner itself, where the published figure is quoted.
    for rate in RATES {
        let filter = Filter::rumble_only(IEC_RUMBLE, rate, 2).expect("design");
        let at_corner = filter.response_db(corner);
        assert!(
            (at_corner + 3.0103).abs() < 0.01,
            "{rate} Hz: the corner reads {at_corner:+.4} dB, not -3.0103"
        );
        assert!(
            filter.response_db(1000.0).abs() < 0.001,
            "{rate} Hz: a rumble filter must not touch 1 kHz"
        );
    }
}

/// The free pole/zero pair `fit` places must not spoil a design it is not needed
/// for.
///
/// Matched-z alone is already exact for one zero at the origin against one real
/// pole, so the fit's only job here is to find the diagonal where its extra pair
/// cancels itself. It is seeded at `(-0.5, -0.4)`, which is **1.89 dB out** at
/// 44.1 kHz - four times §51's budget - so a fit that failed to converge would
/// ship a filter that is audibly wrong, and nothing else in this file would
/// notice. This is the assertion that it converges rather than the assumption.
#[test]
fn the_fit_converges_on_a_design_that_needs_no_fitting() {
    for rate in RATES {
        let error = worst_rumble_error_db(IEC_RUMBLE, rate).expect("design");
        assert!(
            error < 0.001,
            "{rate} Hz: the fit left {error:.5} dB on a design matched-z gets exactly \
             right, so it did not reach the cancelling diagonal"
        );
    }
}

/// The two halves of §51 compose, and the rumble filter is the independent one.
///
/// Folding its roots in before `fit` rather than cascading a second filter after
/// one has to produce the curve's response *plus* the high pass, with nothing
/// else moved: the same 1 kHz reference, the same curve id reported, and exactly
/// the amendment's attenuation at 20 Hz on top of whatever the curve does there.
#[test]
fn the_rumble_filter_is_independent_of_the_curve() {
    let catalog = Catalog::builtin();
    let alone = Filter::rumble_only(IEC_RUMBLE, 44_100, 2).expect("design");

    for id in ["riaa", "columbia_lp", "emi", "flat"] {
        let bare = Filter::playback(&catalog, id, None, CaptureEq::Flat, 44_100, 2).expect("bare");
        let both = Filter::playback(&catalog, id, Some(IEC_RUMBLE), CaptureEq::Flat, 44_100, 2)
            .expect("with rumble");

        assert_eq!(
            both.curve(),
            id,
            "the curve lost its name to the rumble filter"
        );
        assert_eq!(both.rumble(), Some(IEC_RUMBLE));
        assert_eq!(bare.rumble(), None);

        for hz in [20.0, 50.0, 100.0, 1_000.0, 10_000.0] {
            let added = both.response_db(hz) - bare.response_db(hz);
            assert!(
                (added - alone.response_db(hz)).abs() < 0.01,
                "{id} at {hz} Hz: the rumble filter added {added:+.4} dB over the curve \
                 but {:+.4} dB on its own",
                alone.response_db(hz)
            );
        }
    }
}

/// A high pass needs no provenance, where a curve does.
///
/// §51 refuses to apply a *characteristic* to a capture nobody recorded anything
/// about, because the wrong characteristic cannot be heard as wrong. That argument
/// does not reach a 20 Hz high pass: no cutting characteristic put anything there,
/// so removing it is correct over a flat transfer, an RIAA transfer and an
/// unrecorded one alike. The asymmetry is deliberate and this is where it is
/// stated.
#[test]
fn rumble_alone_is_allowed_where_a_curve_is_refused() {
    let catalog = Catalog::builtin();
    let wanted =
        |stated, rumble, capture| Filter::wanted(&catalog, stated, rumble, capture, 48_000, 2);

    let unknown = wanted(None, Some(IEC_RUMBLE), CaptureEq::Unknown)
        .expect("a high pass asks nothing of the provenance")
        .expect("and it is not a no-op");
    assert_eq!(unknown.rumble(), Some(IEC_RUMBLE));
    assert_eq!(
        unknown.curve(),
        "",
        "no curve was stated, so none may be claimed"
    );

    // The curve is still refused over the same capture, rumble or no rumble.
    for rumble in [None, Some(IEC_RUMBLE)] {
        let refusal = wanted(Some("riaa"), rumble, CaptureEq::Unknown)
            .expect_err("a stated curve over an unknown capture is refused");
        assert!(refusal.to_string().contains("never recorded"), "{refusal}");
    }

    // The two no-ops stop being no-ops once a rumble filter is asked for.
    for (stated, capture) in [(None, CaptureEq::Riaa), (Some("flat"), CaptureEq::Flat)] {
        assert!(
            wanted(stated, None, capture).expect("a no-op").is_none(),
            "{stated:?} over {capture:?} was a no-op before the amendment arrived"
        );
        let only = wanted(stated, Some(IEC_RUMBLE), capture)
            .expect("the rumble filter is still wanted")
            .expect("so the chain is not empty");
        assert_eq!(only.rumble(), Some(IEC_RUMBLE));
    }
}

/// An unknown rumble filter is refused, and the refusal names what is on offer
/// from the same list the offer comes from.
#[test]
fn an_unknown_rumble_filter_is_refused_by_name() {
    let error = Filter::rumble_only("subsonic", 48_000, 2).expect_err("no such filter");
    let message = error.to_string();
    assert!(message.contains("subsonic"), "{message}");
    for id in RUMBLE_IDS {
        assert!(
            message.contains(id),
            "the refusal offers {RUMBLE_IDS:?} but did not name {id}: {message}"
        );
    }
    // And through every entry point, because a refusal that only guards one is a
    // refusal the other two call sites walk past.
    let catalog = Catalog::builtin();
    assert!(
        Filter::playback(
            &catalog,
            "riaa",
            Some("subsonic"),
            CaptureEq::Flat,
            48_000,
            2
        )
        .is_err()
    );
    assert!(Filter::wanted(&catalog, None, Some("subsonic"), CaptureEq::Flat, 48_000, 2).is_err());
    assert!(worst_rumble_error_db("subsonic", 48_000).is_err());
}
