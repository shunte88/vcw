/*
 *  Accepting a provider release into a project, end to end (§26, §32).
 *
 *  Accepting a provider release into a project, end to end (§26, §32).
 *  Accepting a provider release into a project, end to end (§26, §32).
 *
 *  Accepting a provider release into a project, end to end (§26, §32).
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

//! Accepting a provider release into a project, end to end (§26, §32).
//!
//! [`vcw_core::identity`] is the only place `vcw-metadata` and `vcw-project`
//! meet, and the interesting behaviour is all at the seam: a position string a
//! provider printed has to name the same track §29 numbered, a tracklist that
//! does not line up has to be reported rather than forced, and a title a person
//! typed has to survive.
//!
//! No audio and no detection here. Tracks are added directly, because what is
//! under test is the mapping and a recorded side would only make the fixture
//! slower.

use vcw_core::identity::{self, Applied};
use vcw_metadata::release::{Medium, Release, TrackEntry};
use vcw_project::{Project, release, side, track};
use vcw_types::vinyl::{Position, Side};

/// Side B. `Side` names only `A` as a constant, because every other side is
/// derived from a letter or an index rather than written down.
fn b() -> Side {
    Side::from_letter('B').expect("B is a side")
}

/// A project with two sides of three tracks each, all untitled.
fn a_record(dir: &tempfile::TempDir) -> Project {
    let mut project = Project::create(dir.path().join("accept.vcw")).expect("create");
    for side in [Side::A, b()] {
        side::ensure(&mut project, side).expect("side");
        for n in 0..3u64 {
            let start = n * 200_000;
            track::add_track(&mut project, side, start, start + 150_000).expect("track");
        }
    }
    project
}

/// One provider track, positioned.
fn entry(position: &str, side: Side, number: u32, title: &str) -> TrackEntry {
    TrackEntry {
        position: position.to_owned(),
        resolved: Some(Position { side, number }),
        title: title.to_owned(),
        artist: None,
        duration: None,
    }
}

/// A release whose tracklist covers the whole of `a_record`.
fn a_matching_release() -> Release {
    Release {
        id: "9999".to_owned(),
        album: "Amber".to_owned(),
        album_artist: "Autechre".to_owned(),
        year: Some(1994),
        genres: vec!["Electronic".to_owned()],
        label: "Warp".to_owned(),
        catalog: "WARP LP 25".to_owned(),
        country: "UK".to_owned(),
        barcode: Some("5021603025127".to_owned()),
        musicbrainz_id: Some("d1d0f0b4-0000-0000-0000-000000000000".to_owned()),
        discogs_id: Some("9999".to_owned()),
        media: vec![Medium {
            position: 1,
            format: "Vinyl".to_owned(),
            tracks: vec![
                entry("A1", Side::A, 1, "Foil"),
                entry("A2", Side::A, 2, "Montreal"),
                entry("A3", Side::A, 3, "Silverside"),
                entry("B1", b(), 1, "Slip"),
                entry("B2", b(), 2, "Glitch"),
                entry("B3", b(), 3, "Piezo"),
            ],
        }],
        ..Release::default()
    }
}

#[test]
fn accepting_a_release_fills_the_row_and_names_every_track() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = a_record(&dir);

    let applied = identity::accept(&mut project, &a_matching_release()).expect("accept");
    assert!(
        applied.is_exact(),
        "the tracklist lines up, so nothing should be left over: {applied:?}"
    );
    assert_eq!(applied.tracks.len(), 6);

    let record = release::load(project.conn())
        .expect("load")
        .expect("a release row");
    assert_eq!(record.album, "Amber");
    assert_eq!(record.album_artist, "Autechre");
    assert_eq!(record.year, Some(1994));
    assert_eq!(record.catalog, "WARP LP 25");
    assert_eq!(record.genres, ["Electronic"]);
    assert_eq!(record.discs, 1);
    assert!(
        record.confirmed,
        "choosing a candidate from a list is §26's confirmation"
    );

    // And the titles landed on the right side, which is the whole point of
    // matching on the resolved position rather than on order.
    let titles: Vec<String> = track::tracks(project.conn(), b())
        .expect("tracks")
        .into_iter()
        .map(|row| row.title)
        .collect();
    assert_eq!(titles, ["Slip", "Glitch", "Piezo"]);
}

#[test]
fn a_title_a_person_confirmed_is_not_overwritten() {
    // The rule that makes accepting a release safe to do twice, and safe to do
    // after an hour of typing: a confirmed track is a person's answer, and a
    // provider does not get to replace it.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = a_record(&dir);
    let mine = track::tracks(project.conn(), Side::A).expect("tracks")[1].id;
    track::update(
        &mut project,
        mine,
        &track::Update {
            title: Some("What The Label Actually Says".to_owned()),
            confirmed: Some(true),
            ..track::Update::default()
        },
    )
    .expect("update");

    let applied = identity::accept(&mut project, &a_matching_release()).expect("accept");
    assert_eq!(applied.confirmed, [mine], "{applied:?}");
    assert_eq!(applied.tracks.len(), 5);
    assert!(
        applied.is_exact(),
        "a skipped track is still a covered track: {applied:?}"
    );

    let row = track::track(project.conn(), mine)
        .expect("track")
        .expect("still there");
    assert_eq!(row.title, "What The Label Actually Says");
}

#[test]
fn a_tracklist_that_does_not_line_up_is_reported_rather_than_forced() {
    // The case that matters in the middle of a rip: side B has not been
    // recorded, and the provider lists it. Nothing about that is an error, and
    // nothing about it should silently retitle side A's tracks with side B's
    // titles - which is exactly what matching on order would do.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = Project::create(dir.path().join("half.vcw")).expect("create");
    side::ensure(&mut project, Side::A).expect("side");
    for n in 0..2u64 {
        track::add_track(&mut project, Side::A, n * 200_000, n * 200_000 + 150_000).expect("track");
    }

    let applied = identity::accept(&mut project, &a_matching_release()).expect("accept");
    assert!(!applied.is_exact(), "{applied:?}");
    assert_eq!(applied.tracks.len(), 2);
    assert_eq!(
        applied.unmatched,
        ["A3", "B1", "B2", "B3"],
        "every provider position with no track should be listed: {applied:?}"
    );
    assert!(applied.unnamed.is_empty(), "{applied:?}");

    let titles: Vec<String> = track::tracks(project.conn(), Side::A)
        .expect("tracks")
        .into_iter()
        .map(|row| row.title)
        .collect();
    assert_eq!(titles, ["Foil", "Montreal"]);
}

#[test]
fn a_track_the_provider_does_not_list_is_named_as_unnamed() {
    // The other direction: the record has a lead-out groove detection picked up
    // as a fourth track, or the pressing has a track the release listing does
    // not. Either way it keeps its empty title and is reported.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = a_record(&dir);
    track::add_track(&mut project, Side::A, 800_000, 900_000).expect("fourth");

    let applied = identity::accept(&mut project, &a_matching_release()).expect("accept");
    assert_eq!(applied.unnamed, ["A4"], "{applied:?}");
    assert!(applied.unmatched.is_empty(), "{applied:?}");
}

#[test]
fn a_position_the_provider_could_not_resolve_is_not_guessed_at() {
    // `resolved: None` is `positions::read` saying the string made no sense as
    // vinyl. Falling back to order here would be inventing a side.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = a_record(&dir);
    let mut found = a_matching_release();
    found.media[0].tracks = vec![TrackEntry {
        position: "untitled".to_owned(),
        resolved: None,
        title: "Nobody Knows".to_owned(),
        ..TrackEntry::default()
    }];

    let applied: Applied = identity::accept(&mut project, &found).expect("accept");
    assert!(applied.tracks.is_empty(), "{applied:?}");
    assert_eq!(applied.unmatched, ["untitled"]);
    assert_eq!(applied.unnamed.len(), 6, "{applied:?}");
    assert!(
        track::tracks(project.conn(), Side::A)
            .expect("tracks")
            .iter()
            .all(|row| row.title.is_empty()),
        "a title was written from a position nobody could read"
    );
}

#[test]
fn what_no_provider_reports_is_left_alone() {
    // `composer` and `comments` are a person's own. Accepting a release must
    // not blank them, which is the failure mode of building the row from the
    // provider's fields and defaulting the rest.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = a_record(&dir);
    let existing = release::ensure(&mut project).expect("ensure");
    release::store(
        &mut project,
        &release::Record {
            composer: "Booth / Brown".to_owned(),
            comments: "warped near the label".to_owned(),
            ..existing
        },
    )
    .expect("store");

    identity::accept(&mut project, &a_matching_release()).expect("accept");
    let record = release::load(project.conn())
        .expect("load")
        .expect("a release row");
    assert_eq!(record.composer, "Booth / Brown");
    assert_eq!(record.comments, "warped near the label");
    assert_eq!(record.album, "Amber", "the provider's fields did land");
}
