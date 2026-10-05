/*
 *  preferences.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Persisting the chosen devices, and resolving them on the next run (§7,
 *  §39).
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

//! Persisting the chosen devices, and resolving them on the next run (§7, §39).
//!
//! Two obligations meet here. Preferences must survive between sessions, and a
//! device that has gone must be *reported* gone rather than quietly replaced -
//! see the module documentation on [`vcw_audio::selection`] for why substitution
//! is the dangerous behavior.

mod common;

use common::{device, input, range, snapshot, three_paths_to_one_card};
use vcw_audio::devices::{DeviceKey, Direction, Transport};
use vcw_audio::probe::Matrix;
use vcw_audio::selection::{self, Preferences, Remembered, Resolution};
use vcw_types::{CaptureMode, SampleFormat, SampleRate};

fn chosen() -> (Preferences, vcw_audio::Snapshot) {
    let snapshot = three_paths_to_one_card();
    let direct = snapshot.find("hw:CARD=0,DEV=0", Direction::Input).unwrap();
    let matrix = Matrix::from_report(&direct.input, Direction::Input);
    let mut preferences = Preferences::default();
    preferences.set(
        Direction::Input,
        Remembered::new(direct, matrix.suggest(), Some(CaptureMode::Exclusive)),
    );
    (preferences, snapshot)
}

#[test]
fn a_choice_survives_a_round_trip_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.json");
    let (preferences, _) = chosen();

    preferences.save(&path).unwrap();
    let loaded = Preferences::load(&path)
        .unwrap()
        .expect("the file we just wrote");

    assert_eq!(loaded, preferences);
    let remembered = loaded.get(Direction::Input).unwrap();
    assert_eq!(remembered.key, DeviceKey::new("alsa", "hw:CARD=0,DEV=0"));
    assert_eq!(remembered.rate, Some(SampleRate(96_000)));
    assert_eq!(remembered.format, Some(SampleFormat::S24));
    assert_eq!(remembered.capture_mode, Some(CaptureMode::Exclusive));
    assert_eq!(remembered.transport, Transport::DirectHardware);
}

/// The id is the whole point of persisting anything, so it has to survive JSON
/// intact - colons and all.
#[test]
fn the_persisted_form_carries_the_id_not_the_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.json");
    let (preferences, _) = chosen();
    preferences.save(&path).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("alsa:hw:CARD=0,DEV=0"), "{text}");
    assert!(text.ends_with('\n'));
}

#[test]
fn the_first_run_has_no_file_and_that_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("never-written.json");

    assert_eq!(Preferences::load(&path).unwrap(), None);
    let (preferences, reset) = Preferences::load_or_reset(&path);
    assert_eq!(preferences, Preferences::default());
    assert!(reset.is_none());
    assert!(preferences.get(Direction::Input).is_none());
}

#[test]
fn saving_creates_the_settings_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/deeper/audio.json");
    Preferences::default().save(&path).unwrap();
    assert!(path.exists());
    assert!(
        !path.with_extension("tmp").exists(),
        "the temporary file should be renamed away"
    );
}

/// Settings must never stop the application starting. A truncated file - a power
/// cut mid-write, which is the whole reason the save is atomic - is moved aside.
#[test]
fn an_unreadable_file_is_moved_aside_rather_than_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.json");
    std::fs::write(&path, "{\"version\": 1, \"input\": {\"key\"").unwrap();

    assert!(Preferences::load(&path).is_err());

    let (preferences, reset) = Preferences::load_or_reset(&path);
    assert_eq!(preferences, Preferences::default());
    let reset = reset.expect("a reset");
    assert_eq!(reset.moved_to, Some(path.with_extension("corrupt")));
    assert!(path.with_extension("corrupt").exists());
    assert!(!path.exists());
    assert!(
        reset.to_string().contains("device preferences reset"),
        "{reset}"
    );
}

#[test]
fn a_file_from_a_newer_build_is_refused_rather_than_half_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.json");
    std::fs::write(&path, r#"{"version": 99, "input": null, "output": null}"#).unwrap();

    let err = Preferences::load(&path).unwrap_err().to_string();
    assert!(err.contains("audio.json"), "{err}");

    let (preferences, reset) = Preferences::load_or_reset(&path);
    assert_eq!(preferences, Preferences::default());
    assert!(reset.is_some());
}

#[test]
fn input_and_output_are_remembered_independently() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.json");

    let mut preferences = Preferences::default();
    let capture = device("hw:CARD=0,DEV=0", "HDA Intel PCH");
    let mut monitor = device("hw:CARD=1,DEV=0", "Studio Monitors");
    monitor.output = input(vec![range(2, 44_100, 192_000, SampleFormat::S32)]);
    monitor.input = Default::default();

    preferences.set(Direction::Input, Remembered::new(&capture, None, None));
    preferences.set(Direction::Output, Remembered::new(&monitor, None, None));
    preferences.save(&path).unwrap();

    let loaded = Preferences::load(&path).unwrap().unwrap();
    assert_eq!(
        loaded.get(Direction::Input).unwrap().key.id(),
        "hw:CARD=0,DEV=0"
    );
    assert_eq!(
        loaded.get(Direction::Output).unwrap().key.id(),
        "hw:CARD=1,DEV=0"
    );

    let mut loaded = loaded;
    loaded.clear(Direction::Input);
    assert!(loaded.get(Direction::Input).is_none());
    assert!(loaded.get(Direction::Output).is_some());
}

#[test]
fn a_remembered_device_that_is_still_there_resolves_ready() {
    let (preferences, snapshot) = chosen();
    let resolution = selection::resolve(&preferences, &snapshot, Direction::Input);

    assert!(resolution.is_ready(), "{resolution:?}");
    assert_eq!(resolution.device().unwrap().key.id(), "hw:CARD=0,DEV=0");
}

#[test]
fn nothing_remembered_resolves_to_unset_not_to_the_default() {
    let snapshot = three_paths_to_one_card();
    let nothing = Preferences::default();
    let resolution = selection::resolve(&nothing, &snapshot, Direction::Input);

    assert!(matches!(resolution, Resolution::Unset));
    assert!(resolution.device().is_none());
    // A default exists and is deliberately not offered.
    assert!(snapshot.default_for(Direction::Input).is_some());
}

/// The §7 hot-unplug case, and the heart of this module. The converter is gone,
/// a perfectly good default is present, and resolution must still refuse to
/// substitute it.
#[test]
fn an_unplugged_device_resolves_to_missing_and_never_to_a_substitute() {
    let (preferences, _) = chosen();
    let mut unplugged = three_paths_to_one_card();
    unplugged
        .devices
        .retain(|d| d.key.id() != "hw:CARD=0,DEV=0");

    let resolution = selection::resolve(&preferences, &unplugged, Direction::Input);
    match &resolution {
        Resolution::Missing { remembered } => {
            assert_eq!(remembered.key.id(), "hw:CARD=0,DEV=0");
        }
        other => panic!("expected Missing, got {other:?}"),
    }
    assert!(resolution.device().is_none());
    assert!(!resolution.is_ready());
    assert!(
        resolution.to_string().contains("not connected"),
        "{resolution}"
    );

    // The plug path to the same card, and the platform default, are both still
    // there. Neither is offered.
    assert!(
        unplugged
            .get(&DeviceKey::new("alsa", "plughw:CARD=0,DEV=0"))
            .is_some()
    );
    assert!(unplugged.default_for(Direction::Input).is_some());
}

/// The other half of §7's clause: *configuration* change. The device is present
/// but no longer offers what was chosen, which a capture armed from the
/// preferences alone would discover only at stream build.
#[test]
fn a_device_that_no_longer_offers_the_chosen_rate_resolves_to_changed() {
    let (preferences, _) = chosen();
    let mut moved = three_paths_to_one_card();
    moved.devices[0].input = input(vec![range(2, 44_100, 48_000, SampleFormat::S24)]);

    let resolution = selection::resolve(&preferences, &moved, Direction::Input);
    match &resolution {
        Resolution::Changed {
            concerns, device, ..
        } => {
            assert_eq!(device.key.id(), "hw:CARD=0,DEV=0");
            assert!(concerns.iter().any(|c| c.contains("96000")), "{concerns:?}");
        }
        other => panic!("expected Changed, got {other:?}"),
    }
    // Usable, but not without saying so.
    assert!(resolution.device().is_some());
    assert!(!resolution.is_ready());
}

/// The quietest and worst failure: the same id now reaches the hardware through
/// the resampling plug layer. Nothing is missing, nothing errors, and the capture
/// would no longer be bit-perfect.
#[test]
fn a_device_that_became_a_converting_path_is_flagged() {
    let (preferences, _) = chosen();
    let mut degraded = three_paths_to_one_card();
    degraded.devices[0].transport = Transport::Converting;

    let resolution = selection::resolve(&preferences, &degraded, Direction::Input);
    let Resolution::Changed { concerns, .. } = &resolution else {
        panic!("expected Changed, got {resolution:?}")
    };
    assert!(
        concerns.iter().any(|c| c.contains("converting")),
        "{concerns:?}"
    );
}

#[test]
fn a_device_that_stopped_offering_input_is_flagged() {
    let (preferences, _) = chosen();
    let mut output_only = three_paths_to_one_card();
    output_only.devices[0].input = Default::default();

    let resolution = selection::resolve(&preferences, &output_only, Direction::Input);
    let Resolution::Changed { concerns, .. } = &resolution else {
        panic!("expected Changed, got {resolution:?}")
    };
    assert!(concerns.iter().any(|c| c.contains("input")), "{concerns:?}");
}

#[test]
fn resolution_matches_on_the_id_alone_even_when_a_namesake_is_present() {
    let (preferences, _) = chosen();
    // The card is gone; a different card has taken its name.
    let mut impostor = snapshot(vec![device("hw:CARD=7,DEV=0", "HDA Intel PCH")]);
    impostor.devices[0].is_default_input = true;

    assert!(matches!(
        selection::resolve(&preferences, &impostor, Direction::Input),
        Resolution::Missing { .. }
    ));
}
