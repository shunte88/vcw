/*
 *  commands.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What a frontend sends, parsed: the JSON a UI posts becomes a core command or a named refusal.
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

//! What a frontend sends, parsed: the JSON a UI posts becomes a core command or
//! a named refusal.
//!
//! These tests are written from the outside in - a string of JSON, as it would
//! arrive - because that is the only form the contract is actually used in. A
//! test that built the struct in Rust would pass while the tag name, the
//! casing or an optional field was wrong.

use vcw_contract::Request;
use vcw_contract::command::{Arm, Audition, Invalid, Region, Transport};
use vcw_core::Command;
use vcw_core::playback::Scope;
use vcw_types::{CaptureEq, SampleFormat};

/// Parses a request the way the shell will.
fn request(json: &str) -> Request {
    serde_json::from_str(json).unwrap_or_else(|error| panic!("{json}: {error}"))
}

#[test]
fn a_transport_verb_is_a_tag_and_a_verb() {
    let parsed = request(r#"{"command":"transport","verb":"record"}"#);
    let Request::Transport { verb } = parsed else {
        panic!("{parsed:?} is not a transport request");
    };
    assert_eq!(verb, Transport::Record);
    assert!(matches!(Command::from(verb), Command::Record));
}

#[test]
fn every_transport_verb_is_spelled_in_kebab_case() {
    // The nine §35 verbs, as a frontend types them. `poll` and `shutdown` are
    // here because the engine has them, not because a button does.
    for (text, expected) in [
        ("disarm", Transport::Disarm),
        ("record", Transport::Record),
        ("pause", Transport::Pause),
        ("resume", Transport::Resume),
        ("stop", Transport::Stop),
        ("reset", Transport::Reset),
        ("poll", Transport::Poll),
        ("shutdown", Transport::Shutdown),
    ] {
        let json = format!(r#""{text}""#);
        let parsed: Transport = serde_json::from_str(&json).expect(text);
        assert_eq!(parsed, expected);
    }
}

#[test]
fn arm_takes_a_project_and_nothing_else_is_required() {
    let parsed = request(r#"{"command":"arm","project":"/tmp/demo.vcw"}"#);
    let Request::Arm(arm) = parsed else {
        panic!("not an arm request");
    };
    let setup = vcw_core::Setup::try_from(arm).expect("a bare arm is valid");
    assert_eq!(setup.project.to_string_lossy(), "/tmp/demo.vcw");
    assert_eq!(setup.rate, None, "unasked-for is not the same as default");
    assert_eq!(setup.format, None);
}

#[test]
fn a_format_a_frontend_might_send_is_accepted_under_any_of_its_names() {
    for (text, expected) in [
        ("s16", SampleFormat::S16),
        ("i16", SampleFormat::S16),
        ("s24", SampleFormat::S24),
        ("s32", SampleFormat::S32),
        ("f32", SampleFormat::F32),
        ("float", SampleFormat::F32),
        ("FLOAT32", SampleFormat::F32),
    ] {
        let arm = Arm {
            project: "demo.vcw".to_owned(),
            format: Some(text.to_owned()),
            ..Arm::default()
        };
        let setup = vcw_core::Setup::try_from(arm).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(setup.format, Some(expected), "{text}");
    }
}

#[test]
fn a_refusal_names_the_field_and_says_what_was_wrong() {
    let arm = Arm {
        project: "demo.vcw".to_owned(),
        format: Some("s20".to_owned()),
        ..Arm::default()
    };
    let error = vcw_core::Setup::try_from(arm).expect_err("s20 is not a format");
    let Invalid { field, why } = &error;
    assert_eq!(*field, "format");
    assert!(why.contains("s16"), "{why} does not say what is allowed");
    // The message is what a UI puts in front of a person, so it has to read as
    // a sentence rather than as a debug dump.
    assert!(
        error.to_string().contains("format"),
        "{error} does not name the field"
    );
}

#[test]
fn an_equalisation_a_frontend_sends_is_taken_or_refused_by_name() {
    for (text, expected) in [
        ("flat", CaptureEq::Flat),
        ("riaa", CaptureEq::Riaa),
        ("unknown", CaptureEq::Unknown),
    ] {
        let arm = Arm {
            project: "demo.vcw".to_owned(),
            eq: Some(text.to_owned()),
            ..Arm::default()
        };
        let setup = vcw_core::Setup::try_from(arm).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(setup.eq, expected, "{text}");
    }

    // Saying nothing is Unknown, which is the honest reading of saying nothing -
    // but a *typo* is refused rather than quietly becoming Unknown, because the
    // operator who typed `raia` believes they have recorded RIAA and §51 says the
    // curve cannot be recovered from the audio later.
    let silent = Arm {
        project: "demo.vcw".to_owned(),
        ..Arm::default()
    };
    assert_eq!(
        vcw_core::Setup::try_from(silent)
            .expect("saying nothing is allowed")
            .eq,
        CaptureEq::Unknown
    );

    let arm = Arm {
        project: "demo.vcw".to_owned(),
        eq: Some("raia".to_owned()),
        ..Arm::default()
    };
    let error = vcw_core::Setup::try_from(arm).expect_err("raia is not a curve");
    assert_eq!(error.field, "eq");
    assert!(
        error.why.contains("riaa"),
        "{} does not say what is allowed",
        error.why
    );
}

#[test]
fn an_empty_project_path_is_refused_rather_than_opened() {
    let arm = Arm {
        project: String::new(),
        ..Arm::default()
    };
    let error = vcw_core::Setup::try_from(arm).expect_err("an empty path is not a project");
    assert_eq!(error.field, "project");
}

#[test]
fn a_whole_capture_audition_needs_no_project_lookup() {
    let parsed = request(r#"{"command":"play","scope":"whole"}"#);
    let Request::Play(audition) = parsed else {
        panic!("not a play request");
    };
    assert!(matches!(audition.scope(44_100), Some(Scope::Whole)));
}

#[test]
fn a_region_audition_resolves_seconds_to_frames_here() {
    // Seconds are what a UI has, frames are what the core takes, and 44100 is
    // the rate the capture ran at. §35 puts the conversion behind the boundary
    // so two frontends cannot round it differently.
    let parsed = request(r#"{"command":"play","scope":"region","start":1.0,"end":3.5}"#);
    let Request::Play(audition) = parsed else {
        panic!("not a play request");
    };
    let Some(Scope::Region(span)) = audition.scope(44_100) else {
        panic!("a region did not resolve");
    };
    assert_eq!(span.start, 44_100);
    assert_eq!(span.end, 154_350);
}

#[test]
fn a_track_audition_defers_to_the_project() {
    // `None` is not a failure: a track's extent is two boundary rows, and the
    // shell has to read them. Returning a guessed span would play the wrong
    // audio silently.
    let parsed = request(r#"{"command":"play","scope":"track","trackId":4}"#);
    let Request::Play(audition) = parsed else {
        panic!("not a play request");
    };
    assert!(audition.scope(44_100).is_none());
    assert!(matches!(audition, Audition::Track { track_id: 4 }));
}

#[test]
fn a_dragged_selection_is_ordered_rather_than_refused() {
    // A person dragging right to left has made a selection, not a mistake, and
    // `Span::new` swaps the pair. Asserted here because the alternative - an
    // empty span, or an error - is what a naive conversion would produce, and
    // the frontend would then have to sort the pair itself.
    let backwards = Region {
        start: 3.0,
        end: 1.0,
    };
    let forwards = Region {
        start: 1.0,
        end: 3.0,
    };
    assert_eq!(backwards.span(44_100), forwards.span(44_100));
    let span = forwards.span(44_100);
    assert_eq!((span.start, span.end), (44_100, 132_300));
}

#[test]
fn a_search_carries_only_what_was_typed() {
    let parsed = request(
        r#"{"command":"search_metadata","artist":"Kraftwerk","album":"Trans-Europe Express"}"#,
    );
    let Request::SearchMetadata(search) = parsed else {
        panic!("not a search request");
    };
    assert_eq!(search.artist.as_deref(), Some("Kraftwerk"));
    assert_eq!(search.catalog, None);
    assert_eq!(search.provider, None, "a provider is the shell's default");
}

#[test]
fn an_export_request_is_the_cli_verb_with_the_same_defaults() {
    let parsed = request(
        r#"{"command":"export","into":"/tmp/out","format":"flac","artwork":"both","sides":["A"]}"#,
    );
    let Request::Export(export) = parsed else {
        panic!("not an export request");
    };
    assert_eq!(export.format, "flac");
    assert_eq!(export.sides, vec!["A".to_owned()]);
    assert!(!export.overwrite, "overwriting is never the default");
}

#[test]
fn an_unknown_command_is_a_parse_error_and_not_a_silent_no_op() {
    let error = serde_json::from_str::<Request>(r#"{"command":"delete_everything"}"#)
        .expect_err("there is no such command");
    assert!(
        error.to_string().contains("delete_everything"),
        "{error} does not say what was rejected"
    );
}

#[test]
fn an_export_request_becomes_the_exporters_own() {
    let parsed = request(
        r#"{"command":"export","into":"/tmp/out","format":"FLAC","artwork":"embed","sides":["A","B"]}"#,
    );
    let Request::Export(export) = parsed else {
        panic!("not an export request");
    };
    let request = export.request().expect("a valid export");
    assert_eq!(request.container.extension(), "flac", "the case is ignored");
    assert_eq!(request.sides.len(), 2);
    assert_eq!(request.into.to_string_lossy(), "/tmp/out");
    assert_eq!(
        request.template,
        vcw_export::naming::DEFAULT_TEMPLATE,
        "no template means the default, not an empty one"
    );
}

#[test]
fn an_artwork_policy_defaults_to_both_rather_than_none() {
    // A person who exports without saying anything about artwork wants the
    // cover: §33 embeds it and writes it beside the files, and the CLI's
    // default is the same word.
    let export = vcw_contract::command::Export {
        into: "/tmp/out".to_owned(),
        format: "wav".to_owned(),
        template: None,
        sides: Vec::new(),
        artwork: None,
        overwrite: false,
        quality: None,
    };
    let request = export.request().expect("a valid export");
    assert_eq!(request.artwork, vcw_export::splitter::Artwork::Both);
}

#[test]
fn a_container_vcw_cannot_write_is_refused_by_name() {
    // This used to be `mp3`, which VCW now writes. The refusal still has to
    // list what it does take, because a format field is free text coming off a
    // JSON command line and "no" on its own leaves the caller guessing.
    let export = vcw_contract::command::Export {
        into: "/tmp/out".to_owned(),
        format: "opus".to_owned(),
        template: None,
        sides: Vec::new(),
        artwork: None,
        overwrite: false,
        quality: None,
    };
    let error = export.request().expect_err("VCW does not write Opus");
    assert_eq!(error.field, "format");
    for spelling in ["flac", "wav", "mp3", "ogg"] {
        assert!(
            error.why.contains(spelling),
            "the refusal does not offer {spelling:?}: {}",
            error.why
        );
    }
}

#[test]
fn a_quality_is_taken_by_the_lossy_containers_and_ignored_by_the_others() {
    // The panel keeps one quality while the format changes under it, so the
    // pair arrives at the contract in every combination. A lossless container
    // with a quality is not an error - it is a setting that does not apply -
    // and a lossy container without one gets the default rather than a refusal.
    let export = |format: &str, quality: Option<&str>| vcw_contract::command::Export {
        into: "/tmp/out".to_owned(),
        format: format.to_owned(),
        template: None,
        sides: Vec::new(),
        artwork: None,
        overwrite: false,
        quality: quality.map(str::to_owned),
    };
    use vcw_export::encoder::{Container, Quality};

    let request = export("mp3", Some("compact"))
        .request()
        .expect("mp3 at compact");
    assert_eq!(request.container, Container::Mp3(Quality::Compact));

    let request = export("ogg", None).request().expect("ogg with no quality");
    assert_eq!(request.container, Container::OggVorbis(Quality::High));

    let request = export("flac", Some("transparent"))
        .request()
        .expect("flac does not mind");
    assert_eq!(request.container, Container::Flac);

    // A word that is not a quality is still refused, and against the right
    // field: a typo in the quality is not a problem with the format.
    let error = export("mp3", Some("lossless"))
        .request()
        .expect_err("lossless is not a quality");
    assert_eq!(error.field, "quality");
    assert!(error.why.contains("transparent"), "{}", error.why);
}

#[test]
fn a_side_has_to_be_one_letter() {
    for given in ["AB", "", "1"] {
        let export = vcw_contract::command::Export {
            into: "/tmp/out".to_owned(),
            format: "wav".to_owned(),
            template: None,
            sides: vec![given.to_owned()],
            artwork: None,
            overwrite: false,
            quality: None,
        };
        let error = export
            .request()
            .expect_err(&format!("{given:?} is not a side"));
        assert_eq!(error.field, "sides");
    }
}
