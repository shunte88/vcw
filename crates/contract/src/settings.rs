/*
 *  settings.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §39's settings, as one typed document with the defaults taken from the core.
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

//! §39's settings, as one typed document with the defaults taken from the core.
//!
//! Five groups, named as §39 names them: audio, recording, detection, metadata
//! and export. One struct per group rather than forty fields on one, because a
//! settings *panel* is drawn per group and a group that can be handed to a
//! component whole is a component that cannot read a field belonging to another
//! tab.
//!
//! # Where the defaults come from
//!
//! Not from here. Every default in [`Settings::default`] is either `None` -
//! meaning *let the engine decide, and report what it negotiated* - or a
//! constant read out of the crate that owns the behaviour:
//! [`vcw_core::adopt::Policy`] for the promotion floor,
//! [`vcw_export::naming::DEFAULT_TEMPLATE`] for the naming template. A number
//! typed into this file would be a second opinion about a default, and §2 is
//! the rule that settling those on the Rust side is not enough - they have to
//! be settled in *one place* on the Rust side.
//!
//! `None` is used heavily and deliberately. A rate of `None` is not "44100
//! unless told otherwise": it is "open the device at whatever it offers and
//! publish the negotiated format in the `armed` event", which is the only
//! honest default given CPAL's format report cannot be trusted. A settings
//! panel showing a blank rate and an `armed` event showing 48000 is the system
//! working.
//!
//!
//! # The group types are renamed on the way across
//!
//! `settings::Export` and `command::Export` are both `Export` in Rust, in
//! different modules, and both land in one generated `.d.ts` - where the second
//! one silently shadows the first. `#[ts(rename = "ExportSettings")]` on each
//! group is what keeps them apart, and `no_declaration_is_declared_twice` in
//! `tests/bindings.rs` is what caught it: the collision was already in the
//! committed file and typechecked in Rust, because the clash exists only in the
//! flattened namespace TypeScript has. The suffix reads better on that side
//! anyway - `Settings.audio` is an `AudioSettings`, not an `Audio`.
//! # Credentials are not in here
//!
//! §39: credentials shall not be stored in project files. They are not stored
//! in *this* file either, which is a stronger rule than the requirement and the
//! right one - a plaintext JSON in a config directory is a file that gets
//! copied, synced and backed up without anybody deciding to. A token reaches
//! the application through the environment
//! ([`vcw_metadata::credentials::Credentials::from_env`]), and what crosses to
//! a UI is [`Credential`]: a name, whether one is present, and how many
//! characters it has. Never the value, not even truncated - a prefix is enough
//! to identify which account a leaked log belongs to.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Everything §39 covers.
///
/// `#[serde(default)]` on every group, so a settings file written by an older
/// version deserialises: a field added here appears with its default rather
/// than making the whole document unreadable, which would lose a person's
/// entire configuration over one new checkbox.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Input, output, backend, rate, format, buffer size, capture mode.
    pub audio: Audio,
    /// Default location, transaction and block size, recovery behaviour.
    pub recording: Recording,
    /// Algorithm, thresholds, minimum silence, minimum track length.
    pub detection: Detection,
    /// Which providers to ask, and how to identify ourselves to them.
    pub metadata: Metadata,
    /// Format, codec options, output path, naming template.
    pub export: Export,
}

/// §39's audio group.
///
/// Every field is optional, and that is the design rather than laziness: the
/// negotiated format is whatever the device agreed to, and pinning a value here
/// is a request that can be refused. What was actually opened arrives in the
/// `armed` event, which is the number a person should believe.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(rename = "AudioSettings")]
pub struct Audio {
    /// Capture device id, or `null` for the host default. The simulated source
    /// is what `null` gets on a machine with nothing attached.
    pub input: Option<String>,
    /// Playback device id, or `null` for the host default.
    pub output: Option<String>,
    /// Host API to prefer: `alsa`, `jack`, `coreaudio`, `wasapi`, `asio`.
    pub backend: Option<String>,
    /// Sample rate to pin, in Hz.
    pub rate: Option<u32>,
    /// Sample format to pin, spelled as [`crate::command::parse_format`] takes
    /// it.
    pub format: Option<String>,
    /// Ring capacity in milliseconds. `None` takes §10's default.
    ///
    /// Milliseconds and not frames, because the thing being sized is a duration
    /// of tolerance to a stalled writer and the frame count that buys it
    /// changes with the rate.
    pub ring_millis: Option<u32>,
    /// `shared`, `native` or `exclusive` (§8).
    pub mode: Option<String>,
    /// Equalisation the signal carries when it arrives: `flat`, `riaa` or
    /// `unknown` (§51). `None` means the same as `unknown`.
    ///
    /// A setting rather than a per-capture field because it describes the
    /// operator's preamp, which does not change between records. It cannot be
    /// recovered from the audio afterwards, which is why it is asked for here
    /// instead of when the curves ship.
    pub eq: Option<String>,
}

/// §39's recording group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(rename = "RecordingSettings")]
pub struct Recording {
    /// Where projects are kept, and what the browser lists (§34).
    ///
    /// `null` until a person picks one, and an unset library is an empty
    /// browser rather than a guess: creating a directory in somebody's home
    /// because they opened the application once is not a decision this
    /// application gets to make.
    pub library: Option<String>,
    /// Frames per stored block. `None` takes the writer's own default.
    pub block_frames: Option<u64>,
    /// How often to commit, in seconds.
    ///
    /// This is the crash-loss floor and nothing else is: a crash loses the
    /// uncommitted frames plus whatever the driver was holding, and the ring
    /// size does not enter into it.
    pub checkpoint_seconds: Option<f64>,
    /// What to do with a project that was not closed cleanly: `ask`, `recover`
    /// or `leave`.
    pub recovery: Option<String>,
}

/// §39's detection group.
///
/// The defaults are WP-11's findings, and they live in
/// [`vcw_core::adopt::Policy`]. `min_sources` is the blunt one: 2 turned 270
/// candidate boundaries into 6 on a real side, which is over-segmentation
/// honoured and also a rule that will lose a quiet fade on a worn pressing.
/// Exposing it here is the remedy, and so is the track editor being able to
/// show the boundaries it dropped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(rename = "DetectionSettings")]
pub struct Detection {
    /// Which detectors to run: `silence`, `spectral`, `hmm`, or `all`.
    pub algorithm: String,
    /// How many detectors must agree before a boundary becomes a track (§24).
    pub min_sources: u32,
    /// The lowest confidence worth writing, in `0.0..=1.0`.
    pub min_confidence: f32,
    /// The shortest gap that counts as a gap, in seconds.
    pub min_silence_seconds: f64,
    /// The shortest run of audio worth calling a track, in seconds.
    ///
    /// Seconds here and frames in the policy, because a person thinks in
    /// seconds and the detector needs frames - and the rate needed to convert
    /// belongs to the capture, which is why the conversion is not in this file.
    pub min_track_seconds: f64,
}

/// §39's metadata group.
///
/// Which providers to ask and how to identify ourselves, and *not* the tokens:
/// see this module's header. `contact` is here because §40 requires provider
/// identification - MusicBrainz asks for a contact in the user agent and rate
/// limits harder without one - and a contact address is a setting, not a
/// secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(rename = "MetadataSettings")]
pub struct Metadata {
    /// Whether to ask Discogs (§26).
    pub discogs: bool,
    /// Whether to ask MusicBrainz (§27).
    pub musicbrainz: bool,
    /// Whether to ask AcoustID. Phase 2 (§45), off here.
    pub acoustid: bool,
    /// Contact address sent in the user agent, per §40.
    pub contact: Option<String>,
    /// A `genre.dat` to fold provider genres through, or `null` for the
    /// built-in mapping.
    pub genre_map: Option<String>,
    /// Whether a lookup may go out at all. §40: the application stays fully
    /// usable offline, and this is how a person says so.
    pub online: bool,
}

/// §39's export group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(rename = "ExportSettings")]
pub struct Export {
    /// `flac`, `wav`, `mp3` or `ogg`.
    pub format: String,
    /// What a lossy container is written at: `transparent`, `high` or
    /// `compact`.
    ///
    /// Kept even while the format is lossless, deliberately: a person who sets
    /// a quality, switches to FLAC for an archival copy and switches back
    /// should find their choice still there.
    pub quality: String,
    /// Where to write, or `null` to be asked each time.
    pub output: Option<String>,
    /// The naming template (§33).
    pub template: String,
    /// `none`, `embed`, `folder` or `both`.
    pub artwork: String,
}

/// Whether a credential is configured, and nothing about what it is.
///
/// §39 forbids storing credentials in project files, and this crate goes
/// further: a token never crosses to a UI in any form. `characters` is the one
/// fact a settings panel genuinely needs, because "I pasted it and it did not
/// work" is almost always a truncated paste or a trailing newline, and a length
/// answers that without showing a single character of the secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    /// Which credential: `discogs`, `acoustid`.
    pub name: String,
    /// Whether one was found.
    pub present: bool,
    /// How many characters it has, or zero when there is none.
    pub characters: u32,
    /// The environment variable it is read from, so a panel can tell a person
    /// where to put it rather than offering a field that writes it to disk.
    pub variable: String,
}

impl Default for Recording {
    fn default() -> Self {
        Self {
            library: None,
            block_frames: None,
            checkpoint_seconds: None,
            recovery: Some("ask".to_owned()),
        }
    }
}

impl Detection {
    /// The detector configuration these settings ask for (§22).
    ///
    /// Two of §39's five detection settings land here and three land on the
    /// policy, which looks arbitrary until you ask *when* each one applies:
    /// a minimum gap changes what the signal pass reports, and a minimum
    /// agreement changes what the project believes. They are different
    /// decisions taken at different moments, and merging them into one struct
    /// would mean re-running the FFTs to change a threshold on a number that
    /// has already been computed.
    #[must_use]
    pub fn config(&self) -> vcw_signal::regions::Config {
        vcw_signal::regions::Config {
            min_silence_secs: self.min_silence_seconds,
            min_sound_secs: self.min_track_seconds,
            ..vcw_signal::regions::Config::new()
        }
    }

    /// The adoption policy these settings ask for (§24).
    ///
    /// The rate is the capture's, because `min_track_seconds` has to become
    /// frames and the wrong rate would move the floor by ten percent.
    ///
    /// # Errors
    ///
    /// [`crate::command::Invalid`] naming the `algorithm` field when it is not
    /// one of the four words.
    pub fn policy(
        &self,
        rate: vcw_types::SampleRate,
    ) -> Result<vcw_core::adopt::Policy, crate::command::Invalid> {
        use vcw_types::observation::Provenance;

        let require_source = match self.algorithm.trim().to_ascii_lowercase().as_str() {
            "all" | "" => None,
            "silence" | "rms" => Some(Provenance::Silence),
            "spectral" | "spectral-change" => Some(Provenance::SpectralChange),
            "hmm" => Some(Provenance::Hmm),
            other => {
                return Err(crate::command::Invalid {
                    field: "algorithm",
                    why: format!(
                        "{other:?} is not a detector - silence, spectral, hmm, or all of them"
                    ),
                });
            }
        };

        Ok(vcw_core::adopt::Policy {
            min_sources: self.min_sources as usize,
            min_confidence: self.min_confidence,
            min_track_frames: frames(self.min_track_seconds, rate),
            require_source,
            ..vcw_core::adopt::Policy::at(rate)
        })
    }
}

/// Seconds to frames, rounded, floored at zero.
fn frames(seconds: f64, rate: vcw_types::SampleRate) -> u64 {
    if seconds <= 0.0 {
        return 0;
    }
    (seconds * f64::from(rate.hz())).round() as u64
}

impl Default for Detection {
    fn default() -> Self {
        // Read from the policy rather than repeated, so tuning the default in
        // WP-11's own crate moves this too. `min_track_frames` is deliberately
        // not mirrored: the policy leaves it zero because it has no rate, and
        // the seconds below are what a person sets instead.
        let policy = vcw_core::adopt::Policy::default();
        Self {
            algorithm: "all".to_owned(),
            min_sources: u32::try_from(policy.min_sources).unwrap_or(2),
            min_confidence: policy.min_confidence,
            min_silence_seconds: 1.0,
            min_track_seconds: 20.0,
        }
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            discogs: true,
            musicbrainz: true,
            // Phase 2 (§45). Declared so the panel can show it greyed out
            // rather than have the field appear later and surprise a
            // configuration that was already saved.
            acoustid: false,
            contact: None,
            genre_map: None,
            online: true,
        }
    }
}

impl Default for Export {
    fn default() -> Self {
        Self {
            // FLAC, because §33's point is archival and a 2.33 GiB side is
            // worth halving. A default capture is S32 and `flacenc` stops at
            // 24 bits, so this default and that limitation meet on the first
            // export a person tries - which is the argument for the refusal
            // naming WAV rather than narrowing the samples quietly.
            format: "flac".to_owned(),
            // Ignored while the format is FLAC, and the value a person finds
            // already filled in the moment they switch to MP3 or Ogg. Around
            // 190 kbit/s either way, which is the level above which most
            // listeners on most equipment stop being able to tell.
            quality: vcw_export::encoder::Quality::default().name().to_owned(),
            output: None,
            template: vcw_export::naming::DEFAULT_TEMPLATE.to_owned(),
            artwork: "both".to_owned(),
        }
    }
}

impl Credential {
    /// What is configured, as a panel lists it.
    ///
    /// Takes the credentials rather than reading the environment itself, so a
    /// caller that already loaded them does not read twice and get two answers.
    #[must_use]
    pub fn survey(credentials: &vcw_metadata::credentials::Credentials) -> Vec<Self> {
        use vcw_metadata::credentials::{ACOUSTID_KEY_VAR, DISCOGS_TOKEN_VAR};
        vec![
            Self::of("discogs", DISCOGS_TOKEN_VAR, credentials.discogs()),
            Self::of("acoustid", ACOUSTID_KEY_VAR, credentials.acoustid()),
        ]
    }

    fn of(name: &str, variable: &str, token: Option<&vcw_metadata::credentials::Token>) -> Self {
        Self {
            name: name.to_owned(),
            present: token.is_some(),
            characters: token.map_or(0, |t| u32::try_from(t.characters()).unwrap_or(u32::MAX)),
            variable: variable.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The promotion floor is WP-11's, not this file's.
    #[test]
    fn the_detection_default_tracks_the_policy() {
        let policy = vcw_core::adopt::Policy::default();
        let settings = Detection::default();
        assert_eq!(
            u32::try_from(policy.min_sources).unwrap(),
            settings.min_sources,
            "the settings default drifted from the policy it is supposed to mirror"
        );
        assert!(
            (policy.min_confidence - settings.min_confidence).abs() < f32::EPSILON,
            "the confidence floor drifted from the policy"
        );
    }

    /// The naming template is WP-14's.
    #[test]
    fn the_export_default_tracks_the_template() {
        assert_eq!(
            Export::default().template,
            vcw_export::naming::DEFAULT_TEMPLATE
        );
    }

    /// A document missing a group still loads, because that is what makes
    /// adding a field safe.
    #[test]
    fn a_partial_document_fills_in_the_rest() {
        let loaded: Settings = serde_json::from_str(r#"{"export":{"format":"wav"}}"#)
            .expect("a partial settings document should load");
        assert_eq!(loaded.export.format, "wav");
        assert_eq!(
            loaded.export.template,
            vcw_export::naming::DEFAULT_TEMPLATE,
            "a field the document did not mention should take its default"
        );
        assert_eq!(loaded.detection, Detection::default());
        assert_eq!(loaded.audio, Audio::default());
    }

    /// An unknown field is ignored rather than fatal, so a file written by a
    /// newer build still opens in an older one.
    #[test]
    fn an_unknown_field_does_not_break_the_document() {
        let loaded: Settings =
            serde_json::from_str(r#"{"audio":{"rate":96000,"quantumFlux":true}}"#)
                .expect("an unknown field should be ignored");
        assert_eq!(loaded.audio.rate, Some(96_000));
    }

    /// A round trip is the property a settings panel depends on: what it saves
    /// is what it reads back.
    #[test]
    fn settings_round_trip() {
        let mut settings = Settings::default();
        settings.recording.library = Some("/data2/vinyl_rips".to_owned());
        settings.audio.rate = Some(192_000);
        let text = serde_json::to_string(&settings).expect("serialising");
        let back: Settings = serde_json::from_str(&text).expect("deserialising");
        assert_eq!(settings, back);
    }

    /// No credential survey ever carries a value.
    #[test]
    fn a_survey_carries_no_secret() {
        let token = vcw_metadata::credentials::Token::new("abcdefghijklmnop")
            .expect("a token of sixteen characters");
        let credentials = vcw_metadata::credentials::Credentials::none().with_discogs(token);
        let survey = Credential::survey(&credentials);

        let discogs = survey
            .iter()
            .find(|c| c.name == "discogs")
            .expect("discogs should be surveyed");
        assert!(discogs.present);
        assert_eq!(discogs.characters, 16);

        let json = serde_json::to_string(&survey).expect("serialising the survey");
        assert!(
            !json.contains("abcdef"),
            "the survey leaked the token: {json}"
        );
    }

    /// The two detection settings that reach the signal pass do reach it.
    #[test]
    fn the_detector_configuration_carries_the_two_settings_that_belong_to_it() {
        let detection = Detection {
            min_silence_seconds: 2.5,
            min_track_seconds: 30.0,
            ..Detection::default()
        };
        let cfg = detection.config();
        assert_eq!(cfg.min_silence_secs, 2.5);
        assert_eq!(cfg.min_sound_secs, 30.0);
        // And everything else is still VRipr's default rather than zero, which
        // is what a `Config { .. }` built field by field would have produced.
        assert_eq!(
            cfg.threshold_db,
            vcw_signal::regions::Config::new().threshold_db
        );
    }

    /// `algorithm` becomes a source filter, and the four words are the four.
    #[test]
    fn every_detector_name_resolves_and_nothing_else_does() {
        use vcw_types::observation::Provenance;
        let rate = vcw_types::SampleRate(44_100);
        let of = |name: &str| {
            Detection {
                algorithm: name.to_owned(),
                ..Detection::default()
            }
            .policy(rate)
        };

        assert_eq!(of("all").expect("all").require_source, None);
        assert_eq!(
            of("silence").expect("silence").require_source,
            Some(Provenance::Silence)
        );
        assert_eq!(
            of("spectral").expect("spectral").require_source,
            Some(Provenance::SpectralChange)
        );
        assert_eq!(
            of("hmm").expect("hmm").require_source,
            Some(Provenance::Hmm)
        );

        let refused = of("fingerprint").expect_err("not a detector");
        assert_eq!(refused.field, "algorithm");
        assert!(
            refused.why.contains("fingerprint"),
            "the message should quote what was sent: {refused}"
        );
    }

    /// The seconds a person sets become frames at the capture's rate.
    #[test]
    fn the_track_floor_is_converted_at_the_captures_rate() {
        let detection = Detection {
            min_track_seconds: 20.0,
            ..Detection::default()
        };
        let at_44 = detection
            .policy(vcw_types::SampleRate(44_100))
            .expect("policy");
        let at_96 = detection
            .policy(vcw_types::SampleRate(96_000))
            .expect("policy");
        assert_eq!(at_44.min_track_frames, 882_000);
        assert_eq!(at_96.min_track_frames, 1_920_000);
        assert_ne!(
            at_44.tolerance, at_96.tolerance,
            "the tolerance is rate-derived too, so these must differ"
        );
    }

    /// Nothing present is reported as nothing present, not as an error.
    #[test]
    fn an_empty_survey_is_still_a_survey() {
        let survey = Credential::survey(&vcw_metadata::credentials::Credentials::none());
        assert_eq!(survey.len(), 2);
        assert!(survey.iter().all(|c| !c.present));
        assert!(survey.iter().all(|c| c.characters == 0));
    }
}
