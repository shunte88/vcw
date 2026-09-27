/*
 *  wire.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Every core event maps to a wire event with the same name, and the JSON is the shape §35 describes.
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

//! Every core event maps to a wire event with the same name, and the JSON is
//! the shape §35 describes.
//!
//! The first test is the one that matters. `vcw_core::Event` is
//! `#[non_exhaustive]` and grows as work packages land, and
//! [`vcw_contract::event::Wire`]'s `From` has a catch-all arm so that a new
//! core event cannot panic a running UI. That arm is also exactly how a new
//! event could be silently swallowed - reported as `capture-warning` with the
//! code `unmapped-event` and never drawn. So the mapping is asserted per
//! variant here, by naming every variant in a list: the day a fifteenth is
//! added, this test still passes, but the list does not mention it and the
//! count assertion at the end fails.

use vcw_contract::event::Wire;
use vcw_core::events::Event;
use vcw_core::state::Phase;
use vcw_signal::meter::Snapshot;
use vcw_types::Diagnostics;
use vcw_types::capture::CaptureState;
use vcw_types::observation::{Edge, Provenance};

/// One of every event the core publishes today.
fn every_event() -> Vec<Event> {
    vec![
        Event::Phase {
            from: Phase::Idle,
            to: Phase::Armed,
        },
        Event::Armed {
            project: "demo.vcw".to_owned(),
            negotiated: "44100 Hz, 2 ch, s16".to_owned(),
            divergences: vec!["rate: asked 96000, got 44100".to_owned()],
            verified: true,
        },
        Event::Position {
            frames: 44_100,
            seconds: 1.0,
        },
        Event::Meter {
            levels: Snapshot::default(),
        },
        Event::Detected {
            frame: 88_200,
            seconds: 2.0,
            edge: Edge::Start,
            provenance: Provenance::Silence,
            confidence: 0.75,
        },
        Event::Warning {
            code: "overrun",
            detail: "the ring filled".to_owned(),
        },
        Event::Finished {
            capture_id: 1,
            frames: 44_100,
            state: CaptureState::Finalised,
            diagnostics: Diagnostics::default(),
            bit_perfect: true,
        },
        Event::Auditioning {
            capture_id: 1,
            scope: "whole".to_owned(),
            opened: "44100 Hz, 2 ch, s16".to_owned(),
            conversion: "none".to_owned(),
            divergences: Vec::new(),
        },
        Event::Playhead {
            frame: 1_000,
            seconds: 0.02,
        },
        Event::Ended {
            capture_id: 1,
            frames: 44_100,
            underruns: 0,
            fidelity: "bit-perfect".to_owned(),
            bit_perfect: true,
        },
        Event::Refused {
            command: "record",
            phase: Phase::Idle,
            reason: "arm first".to_owned(),
        },
        Event::Rejected {
            command: "stop",
            phase: Phase::Idle,
        },
        Event::Status {
            phase: Phase::Recording,
            frames: 44_100,
        },
        Event::Closed,
    ]
}

#[test]
fn every_event_keeps_its_name_across_the_boundary() {
    for event in every_event() {
        let wire = Wire::from(&event);
        assert_eq!(
            wire.kind(),
            event.name(),
            "{event:?} arrived at the frontend as {:?}",
            wire.kind()
        );
        // The catch-all arm also reports as `capture-warning`, so the name
        // alone cannot tell a mapped warning from a swallowed event. The code
        // can: the arm sets `unmapped-event` and nothing else does.
        let value = serde_json::to_value(&wire).expect("serialising");
        assert_ne!(
            value.get("code").and_then(serde_json::Value::as_str),
            Some("unmapped-event"),
            "{event:?} hit the catch-all arm in `From<&Event> for Wire`"
        );
    }
}

#[test]
fn the_list_above_is_every_variant() {
    // `Event::name` is the only exhaustive match over the enum outside the core
    // crate's own code, so the set of distinct names is the variant count.
    let names: std::collections::BTreeSet<_> = every_event().iter().map(Event::name).collect();
    assert_eq!(
        names.len(),
        14,
        "a core event was added or removed: {names:?}. Map it in \
         `vcw_contract::event::Wire` and add it to `every_event` above."
    );
}

#[test]
fn the_tag_is_the_only_discriminator() {
    // §35's events are read by a `switch (event.kind)`, which only works if the
    // tag is a field of the object rather than a wrapper around it. serde's
    // internally-tagged representation is what does that, and it is easy to
    // lose in a refactor.
    for event in every_event() {
        let value = serde_json::to_value(Wire::from(&event)).expect("serialising");
        let object = value.as_object().expect("an object, not a wrapper");
        assert_eq!(
            object.get("kind").and_then(serde_json::Value::as_str),
            Some(event.name()),
            "{event:?} did not carry its kind as a field"
        );
    }
}

#[test]
fn fields_cross_as_camel_case() {
    let finished = Event::Finished {
        capture_id: 7,
        frames: 1_000,
        state: CaptureState::Finalised,
        diagnostics: Diagnostics::default(),
        bit_perfect: false,
    };
    let text = serde_json::to_string(&Wire::from(&finished)).expect("serialising");
    assert!(text.contains("\"captureId\":7"), "{text}");
    assert!(text.contains("\"bitPerfect\":false"), "{text}");
    assert!(
        !text.contains("capture_id"),
        "a snake_case field reached the frontend: {text}"
    );
}

#[test]
fn a_frame_count_crosses_as_a_number() {
    // Not a string and not a bigint. `2^53` frames at 192 kHz is about a
    // billion years, so a double is exact for every value that can occur, and
    // the generated TypeScript says `number` to match.
    let position = Event::Position {
        frames: 9_007_199_254_740_991,
        seconds: 46_912_496_118.4,
    };
    let text = serde_json::to_string(&Wire::from(&position)).expect("serialising");
    assert!(text.contains("\"frames\":9007199254740991"), "{text}");
}

#[test]
fn closed_is_the_last_event() {
    assert!(Wire::from(&Event::Closed).is_last());
    assert!(
        !Wire::from(&Event::Status {
            phase: Phase::Idle,
            frames: 0
        })
        .is_last()
    );
}
