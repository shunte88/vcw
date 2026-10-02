/*
 *  views.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The view models are projections, and these tests pin what each one resolves.
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

//! The view models are projections, and these tests pin what each one resolves.
//!
//! A view model exists to answer a question a UI should not have to: which
//! medium of a five-disc release is the vinyl one, what a waveform column looks
//! like once it is on the wire, which file an export is about to write. The
//! conversions are small, and each of them makes a choice that is invisible
//! from the type alone.
//!
//! `Device` is not here, because a [`vcw_audio::devices::DeviceReport`] comes
//! from enumerating real hardware and this crate has no device. It is exercised
//! by the shell's own tests and by `vcw devices` instead.

use vcw_contract::view::{Candidate, Waveform};
use vcw_metadata::release::{Medium, Release, TrackEntry};
use vcw_signal::waveform::{Column, Level};
use vcw_types::SampleRate;

/// A two-disc release: one vinyl medium and one CD.
fn release() -> Release {
    Release {
        id: "r-1".to_owned(),
        album: "Trans-Europe Express".to_owned(),
        album_artist: "Kraftwerk".to_owned(),
        year: Some(1977),
        label: "Kling Klang".to_owned(),
        catalog: "1C 064-82 306".to_owned(),
        country: "DE".to_owned(),
        media: vec![
            Medium {
                position: 1,
                format: "CD".to_owned(),
                tracks: vec![TrackEntry::default(), TrackEntry::default()],
            },
            Medium {
                position: 2,
                format: "12\" Vinyl".to_owned(),
                tracks: vec![TrackEntry::default()],
            },
        ],
        ..Release::default()
    }
}

#[test]
fn a_candidate_shows_the_vinyl_format_even_when_it_is_not_the_first_medium() {
    // The point of §28's search is a pressing, and a provider that returns a CD
    // reissue alongside the LP lists them in its own order. Showing "CD" beside
    // a record a person is holding is the mistake this prevents.
    let candidate = Candidate::of("musicbrainz", &release());
    assert_eq!(candidate.format, "12\" Vinyl");
    assert_eq!(candidate.provider, "musicbrainz");
    assert_eq!(candidate.id, "r-1");
}

#[test]
fn a_candidates_track_count_is_the_whole_release() {
    // Every medium, not the vinyl one: the count is there to tell two pressings
    // apart at a glance, and a release with a bonus disc has more tracks than
    // one without.
    assert_eq!(Candidate::of("discogs", &release()).tracks, Some(3));
}

#[test]
fn a_release_with_no_media_still_makes_a_candidate() {
    let bare = Release {
        album: "Untitled".to_owned(),
        ..Release::default()
    };
    let candidate = Candidate::of("discogs", &bare);
    assert_eq!(
        candidate.format, "",
        "an unknown format is empty, not a guess"
    );
    assert_eq!(
        candidate.tracks,
        Some(0),
        "a fetched release with no media really does list no tracks, which is \
         not the same as a search hit that was never asked"
    );
}

#[test]
fn a_waveform_crosses_as_three_arrays_and_not_a_list_of_objects() {
    let drawn = vcw_signal::waveform::Waveform {
        start: 0,
        end: 300,
        frames_per_pixel: 100.0,
        level: Level::Samples,
        covered: 300,
        columns: vec![
            Column {
                min: -0.5,
                max: 0.5,
                rms: 0.25,
                frames: 100,
            },
            Column {
                min: -1.0,
                max: 1.0,
                rms: 0.5,
                frames: 100,
            },
            Column {
                min: 0.0,
                max: 0.0,
                rms: 0.0,
                frames: 100,
            },
        ],
    };
    let view = Waveform::of(9, &drawn, SampleRate(100));
    assert_eq!(view.capture_id, 9);
    assert_eq!((view.start_frame, view.end_frame), (0, 300));
    // The same window in seconds, so a renderer that draws in frames and seeks
    // in seconds needs neither the rate nor a division of its own.
    assert_eq!((view.start_seconds, view.end_seconds), (0.0, 3.0));
    assert_eq!(view.min, vec![-0.5, -1.0, 0.0]);
    assert_eq!(view.max, vec![0.5, 1.0, 0.0]);
    assert_eq!(view.rms, vec![0.25, 0.5, 0.0]);

    // The three arrays must stay the same length, because a renderer walks them
    // by index and a short one would draw a truncated waveform rather than fail.
    assert_eq!(view.min.len(), view.max.len());
    assert_eq!(view.min.len(), view.rms.len());

    // And the JSON is arrays of numbers - the thing that makes this worth doing.
    let text = serde_json::to_string(&view).expect("serialising");
    assert!(text.contains("\"min\":[-0.5,-1.0,0.0]"), "{text}");
}

#[test]
fn an_empty_waveform_is_empty_and_not_an_error() {
    let drawn = vcw_signal::waveform::Waveform {
        start: 0,
        end: 0,
        frames_per_pixel: 1.0,
        level: Level::Samples,
        covered: 0,
        columns: Vec::new(),
    };
    let view = Waveform::of(1, &drawn, SampleRate(44_100));
    assert!(view.min.is_empty() && view.max.is_empty() && view.rms.is_empty());
}
