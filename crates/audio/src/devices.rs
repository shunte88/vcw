/*
 *  devices.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Enumeration, identity and capability reporting for audio devices (§7).
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

//! Enumeration, identity and capability reporting for audio devices (§7).
//!
//! The one rule that shapes everything here: **a device is identified by its id,
//! never by its name**. On ALSA the same physical converter appears as `hw:`,
//! `plughw:` and whatever PipeWire publishes, all under the same human-readable
//! name, and only the `hw:` path can be bit-perfect. S1 caught CPAL 0.16 reporting
//! an honored 48 kHz stereo I32 request while the hardware ran 8 kHz mono I16,
//! because the list described the plug layer rather than the card. CPAL 0.18
//! enumerates both and gives every device a stable `DeviceId`; this module keys
//! everything on that id and reports which path each entry is.
//!
//! Enumeration never fails as a whole. A host that will not load, or a device
//! already held open by something else, is recorded as a problem on the entry and
//! the rest of the list is still returned - "the card is busy" and "the card has no
//! formats" are different diagnoses and the user needs to see which one it is.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};
use vcw_types::{SampleFormat, SampleRate};

use crate::error::{Error, Result};
use crate::probe;

/// Which way the audio flows. Capture and playback devices are chosen
/// independently (§7), so almost everything here is parameterised by direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Into the machine: the turntable's converter.
    Input,
    /// Out of it: monitoring and audition.
    Output,
}

impl Direction {
    /// The other one.
    pub const fn opposite(self) -> Self {
        match self {
            Self::Input => Self::Output,
            Self::Output => Self::Input,
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Input => "input",
            Self::Output => "output",
        })
    }
}

/// What sits between the application and the converter.
///
/// This is the §9 question in disguise. A path that resamples cannot be
/// bit-perfect however the request is reported, so the classification travels with
/// the device and the UI can steer away from the converting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    /// An ALSA `hw:` PCM - the driver, with nothing in between.
    DirectHardware,
    /// An ALSA `plughw:` PCM. Same hardware, but the plug layer will silently
    /// resample or reformat to satisfy whatever is asked of it.
    Converting,
    /// A software endpoint: PipeWire, PulseAudio, a loopback, a virtual cable.
    /// It may be transparent today and resampling tomorrow.
    Virtual,
    /// Not classifiable on this platform. WASAPI, CoreAudio and AAudio do not
    /// expose the distinction in the id, so their devices land here and the
    /// question is settled by WP-04's verifier instead.
    Unknown,
}

impl Transport {
    /// Whether this path *can* carry bit-perfect audio, where that is knowable.
    ///
    /// `None` means unknown, and unknown is not a yes. §9 forbids claiming
    /// bit-perfect operation on the strength of the API's own report, so even
    /// [`Transport::DirectHardware`] is only a candidate until WP-04's verifier
    /// confirms it against the operating system.
    pub const fn can_be_bit_perfect(self) -> Option<bool> {
        match self {
            Self::DirectHardware => Some(true),
            Self::Converting => Some(false),
            Self::Virtual | Self::Unknown => None,
        }
    }

    /// Classifies a device from its host and its id.
    ///
    /// Derived from the id rather than the name, because the name is shared by all
    /// three ALSA paths to one card. `backend_says_virtual` is the backend's own
    /// opinion where it has one, which overrides the id: a loopback PCM can be
    /// spelled `hw:` and is still not a converter.
    pub fn classify(host: &str, id: &str, backend_says_virtual: bool) -> Self {
        if backend_says_virtual {
            return Self::Virtual;
        }
        if host.eq_ignore_ascii_case("alsa") {
            if id.starts_with("hw:") {
                return Self::DirectHardware;
            }
            if id.starts_with("plughw:") {
                return Self::Converting;
            }
            // `default`, `pipewire`, `pulse`, `sysdefault`, `dmix` and friends.
            return Self::Virtual;
        }
        Self::Unknown
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DirectHardware => "direct hardware",
            Self::Converting => "converting",
            Self::Virtual => "virtual",
            Self::Unknown => "unknown",
        })
    }
}

/// A device's stable identity: the host that owns it and the backend's own id.
///
/// The string form is CPAL's, `host:id` - for example `alsa:hw:CARD=0,DEV=0`.
/// Note that the id half contains colons of its own, so the split is on the
/// *first* one only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceKey {
    host: String,
    id: String,
}

impl DeviceKey {
    /// Builds a key from its two halves.
    pub fn new(host: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            id: id.into(),
        }
    }

    /// The host name, lowercase, as CPAL spells it: `alsa`, `wasapi`, `coreaudio`.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The backend's device id. On ALSA this is the PCM id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Whether this id names an ALSA `hw:` PCM.
    pub fn is_direct_hardware(&self) -> bool {
        self.host.eq_ignore_ascii_case("alsa") && self.id.starts_with("hw:")
    }

    /// Converts to CPAL's own id, for `HostTrait::device_by_id`.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownHost`] if this build has no such host on this platform,
    /// which is what a preferences file carried between machines looks like.
    pub fn to_cpal(&self) -> Result<cpal::DeviceId> {
        let host = cpal::HostId::from_str(&self.host).map_err(|_| Error::UnknownHost {
            host: self.host.clone(),
        })?;
        Ok(cpal::DeviceId::new(host, &self.id))
    }
}

impl fmt::Display for DeviceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.id)
    }
}

impl From<&cpal::DeviceId> for DeviceKey {
    fn from(id: &cpal::DeviceId) -> Self {
        Self::new(id.host().to_string(), id.id())
    }
}

impl From<DeviceKey> for String {
    fn from(key: DeviceKey) -> Self {
        key.to_string()
    }
}

impl FromStr for DeviceKey {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let (host, id) = s
            .split_once(':')
            .ok_or_else(|| format!("`{s}` is not a device id; expected `host:id`"))?;
        if host.is_empty() || id.is_empty() {
            return Err(format!("`{s}` is not a device id; expected `host:id`"));
        }
        Ok(Self::new(host, id))
    }
}

impl TryFrom<String> for DeviceKey {
    type Error = String;

    fn try_from(s: String) -> std::result::Result<Self, Self::Error> {
        s.parse()
    }
}

/// One advertised family of configurations, as the backend describes it.
///
/// A range is a claim, not a guarantee: S1 saw advertised rates fail at stream
/// build. [`probe`] is what turns claims into confirmed capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigRange {
    /// Channel count this family applies to.
    pub channels: u16,
    /// Lowest rate in the family, Hz.
    pub min_rate: u32,
    /// Highest rate in the family, Hz.
    pub max_rate: u32,
    /// CPAL's name for the sample format, kept verbatim so formats outside §8
    /// are still visible rather than dropped.
    pub sample_format: String,
    /// The §8 format this corresponds to, or `None` for one §8 does not cover
    /// (`U8`, `I64`, `F64` and the like).
    pub format: Option<SampleFormat>,
    /// Bytes one sample of one channel occupies as the device delivers it.
    pub bytes_per_sample: usize,
    /// Buffer sizes the backend will accept, in frames, where it says.
    pub buffer_frames: Option<(u32, u32)>,
}

impl ConfigRange {
    /// Whether this family covers a rate.
    pub fn covers(&self, rate: SampleRate) -> bool {
        (self.min_rate..=self.max_rate).contains(&rate.hz())
    }
}

/// A single configuration, exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactConfig {
    /// Channel count.
    pub channels: u16,
    /// Sample rate.
    pub rate: SampleRate,
    /// CPAL's name for the sample format.
    pub sample_format: String,
    /// The §8 format, if it is one.
    pub format: Option<SampleFormat>,
    /// Buffer sizes the backend will accept, in frames, where it says.
    pub buffer_frames: Option<(u32, u32)>,
}

/// What a device offers in one direction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectionReport {
    /// Whether the device works in this direction at all.
    pub supported: bool,
    /// Every advertised configuration family.
    pub configs: Vec<ConfigRange>,
    /// The backend's own default. Worth showing and worth distrusting: S1 found
    /// the default input on this host to be the one configuration guaranteed not
    /// to be bit-perfect.
    pub default: Option<ExactConfig>,
}

impl DirectionReport {
    /// The distinct channel counts advertised, ascending.
    pub fn channel_counts(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self.configs.iter().map(|c| c.channels).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Which of §8's required rates are advertised, ascending.
    pub fn standard_rates(&self) -> Vec<SampleRate> {
        vcw_types::STANDARD_RATES
            .into_iter()
            .filter(|r| self.configs.iter().any(|c| c.covers(*r)))
            .collect()
    }

    /// Which of §8's required formats are advertised.
    pub fn standard_formats(&self) -> Vec<SampleFormat> {
        let mut v: Vec<SampleFormat> = self.configs.iter().filter_map(|c| c.format).collect();
        v.dedup_by(|a, b| a == b);
        let mut seen = Vec::new();
        for f in v {
            if !seen.contains(&f) {
                seen.push(f);
            }
        }
        seen
    }
}

/// Everything §7 asks to be exposed about one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceReport {
    /// The identity to select on and to persist.
    pub key: DeviceKey,
    /// Human-readable name. Not unique, and not to be selected on.
    pub name: String,
    /// Vendor, where the backend knows.
    pub manufacturer: Option<String>,
    /// Driver, where the backend knows.
    pub driver: Option<String>,
    /// The backend's device-type categorization, as a string.
    pub device_type: String,
    /// How the device is attached: USB, PCI, Bluetooth and so on.
    pub interface: String,
    /// What sits between us and the converter.
    pub transport: Transport,
    /// This host's default input.
    pub is_default_input: bool,
    /// This host's default output.
    pub is_default_output: bool,
    /// Capture capability.
    pub input: DirectionReport,
    /// Playback capability.
    pub output: DirectionReport,
    /// Anything that went wrong while interrogating this device. Non-empty does
    /// not mean unusable: "busy" is the common one, and it clears.
    pub problems: Vec<String>,
}

impl DeviceReport {
    /// The report for one direction.
    pub fn direction(&self, direction: Direction) -> &DirectionReport {
        match direction {
            Direction::Input => &self.input,
            Direction::Output => &self.output,
        }
    }

    /// Whether the device works in this direction.
    pub fn supports(&self, direction: Direction) -> bool {
        self.direction(direction).supported
    }

    /// Whether this is the host's default for the direction.
    pub fn is_default(&self, direction: Direction) -> bool {
        match direction {
            Direction::Input => self.is_default_input,
            Direction::Output => self.is_default_output,
        }
    }

    /// Name and id together, for messages. The id alone is unreadable and the
    /// name alone is ambiguous.
    pub fn label(&self) -> String {
        format!("{} [{}]", self.name, self.key)
    }

    /// A stable summary of what this device offers, for change detection.
    ///
    /// Deliberately excludes `problems`, which is transient - a device that was
    /// busy on the last sweep has not *changed*, it was just unavailable.
    pub fn capability_fingerprint(&self) -> String {
        let mut s = String::new();
        for (tag, d) in [("in", &self.input), ("out", &self.output)] {
            s.push_str(tag);
            s.push('=');
            let mut lines: Vec<String> = d
                .configs
                .iter()
                .map(|c| {
                    format!(
                        "{}/{}-{}/{}",
                        c.channels, c.min_rate, c.max_rate, c.sample_format
                    )
                })
                .collect();
            lines.sort();
            s.push_str(&lines.join(","));
            s.push(';');
        }
        s
    }
}

/// How a device differs between two sweeps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "kebab-case")]
pub enum Change {
    /// A device that was not there before.
    Appeared {
        /// Its identity.
        key: DeviceKey,
        /// Its name, for the message.
        name: String,
    },
    /// A device that has gone. If it is the selected capture device, §7's
    /// "without corrupting the project" clause is now in force.
    Disappeared {
        /// Its identity.
        key: DeviceKey,
        /// Its name as last seen.
        name: String,
    },
    /// Same device, different capabilities, name or default status. A converter
    /// switched from 44.1 to 96 kHz by its own front panel looks like this.
    Reconfigured {
        /// Its identity.
        key: DeviceKey,
        /// Its name.
        name: String,
        /// Which aspects moved.
        details: Vec<String>,
    },
}

impl Change {
    /// The device this change is about.
    pub fn key(&self) -> &DeviceKey {
        match self {
            Self::Appeared { key, .. }
            | Self::Disappeared { key, .. }
            | Self::Reconfigured { key, .. } => key,
        }
    }
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Appeared { key, name } => write!(f, "appeared: {name} [{key}]"),
            Self::Disappeared { key, name } => write!(f, "disappeared: {name} [{key}]"),
            Self::Reconfigured { key, name, details } => {
                write!(f, "reconfigured: {name} [{key}] ({})", details.join(", "))
            }
        }
    }
}

/// Everything visible at one moment.
///
/// A snapshot is a value, not a handle: it can be compared with an earlier one to
/// detect hot-plug and hot-unplug, which is the only portable way to do it, since
/// CPAL exposes no device-change notification.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The devices, in enumeration order.
    pub devices: Vec<DeviceReport>,
    /// Hosts that could not be opened at all, with the reason.
    pub problems: Vec<String>,
}

impl Snapshot {
    /// The devices usable in one direction.
    pub fn in_direction(&self, direction: Direction) -> impl Iterator<Item = &DeviceReport> {
        self.devices.iter().filter(move |d| d.supports(direction))
    }

    /// Looks a device up by its exact identity.
    pub fn get(&self, key: &DeviceKey) -> Option<&DeviceReport> {
        self.devices.iter().find(|d| &d.key == key)
    }

    /// This host's default for a direction, if it has one.
    pub fn default_for(&self, direction: Direction) -> Option<&DeviceReport> {
        self.devices.iter().find(|d| d.is_default(direction))
    }

    /// Finds one device by id, then exact name, then substring.
    ///
    /// An exact id match wins outright: it is unambiguous by construction, even
    /// when the same string also appears inside other names. Everything else must
    /// match exactly one device. Taking the first of several matches is how a
    /// session gets recorded from the wrong converter.
    ///
    /// # Errors
    ///
    /// [`Error::NoMatch`] if nothing matched, [`Error::Ambiguous`] if more than
    /// one did.
    pub fn find(&self, query: &str, direction: Direction) -> Result<&DeviceReport> {
        let usable: Vec<&DeviceReport> = self.in_direction(direction).collect();

        if let Some(hit) = usable.iter().find(|d| d.key.to_string() == query) {
            return Ok(hit);
        }
        // Bare backend ids are what users actually read off `aplay -l`, so accept
        // `hw:CARD=0,DEV=0` as well as `alsa:hw:CARD=0,DEV=0` - but only when it
        // picks out exactly one device across the hosts.
        let bare: Vec<&&DeviceReport> = usable.iter().filter(|d| d.key.id() == query).collect();
        if bare.len() == 1 {
            return Ok(bare[0]);
        }
        if bare.len() > 1 {
            return Err(Error::Ambiguous {
                query: query.to_owned(),
                candidates: bare.iter().map(|d| d.label()).collect(),
            });
        }

        let exact: Vec<&&DeviceReport> = usable.iter().filter(|d| d.name == query).collect();
        let hits = if exact.is_empty() {
            usable
                .iter()
                .filter(|d| d.name.contains(query) || d.key.id().contains(query))
                .collect::<Vec<_>>()
        } else {
            exact
        };

        match hits.len() {
            1 => Ok(hits[0]),
            0 => Err(Error::NoMatch {
                query: query.to_owned(),
                direction,
                considered: usable.len(),
            }),
            _ => Err(Error::Ambiguous {
                query: query.to_owned(),
                candidates: hits.iter().map(|d| d.label()).collect(),
            }),
        }
    }

    /// What changed since an earlier snapshot.
    ///
    /// Pure, and deliberately so: hot-plug handling is the part of §7 hardest to
    /// test with real hardware, and keeping the comparison free of CPAL means it
    /// can be tested without any.
    pub fn diff(&self, previous: &Snapshot) -> Vec<Change> {
        let before: BTreeMap<&DeviceKey, &DeviceReport> =
            previous.devices.iter().map(|d| (&d.key, d)).collect();
        let now: BTreeMap<&DeviceKey, &DeviceReport> =
            self.devices.iter().map(|d| (&d.key, d)).collect();

        let mut changes = Vec::new();
        for (key, gone) in &before {
            if !now.contains_key(key) {
                changes.push(Change::Disappeared {
                    key: (*key).clone(),
                    name: gone.name.clone(),
                });
            }
        }
        for (key, fresh) in &now {
            let Some(old) = before.get(key) else {
                changes.push(Change::Appeared {
                    key: (*key).clone(),
                    name: fresh.name.clone(),
                });
                continue;
            };
            let mut details = Vec::new();
            if old.name != fresh.name {
                details.push(format!("name {:?} -> {:?}", old.name, fresh.name));
            }
            if old.transport != fresh.transport {
                details.push(format!(
                    "transport {} -> {}",
                    old.transport, fresh.transport
                ));
            }
            if old.capability_fingerprint() != fresh.capability_fingerprint() {
                details.push("supported configurations".to_owned());
            }
            if old.is_default_input != fresh.is_default_input {
                details.push(format!("default input {}", yes_no(fresh.is_default_input)));
            }
            if old.is_default_output != fresh.is_default_output {
                details.push(format!(
                    "default output {}",
                    yes_no(fresh.is_default_output)
                ));
            }
            if !details.is_empty() {
                changes.push(Change::Reconfigured {
                    key: (*key).clone(),
                    name: fresh.name.clone(),
                    details,
                });
            }
        }
        changes
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// Every device on every host this build of CPAL can reach.
///
/// Host-by-host rather than default-host-only, because on Linux the default host
/// is the one least likely to be bit-perfect (S1 finding 3) and the user needs to
/// see the alternative.
pub fn enumerate() -> Snapshot {
    let mut snapshot = Snapshot::default();

    for host_id in cpal::available_hosts() {
        let host = match cpal::host_from_id(host_id) {
            Ok(h) => h,
            Err(e) => {
                snapshot.problems.push(format!("host {host_id}: {e}"));
                continue;
            }
        };
        // Compare defaults by id, not name: several ALSA PCMs share a name.
        let default_in = host.default_input_device().and_then(|d| d.id().ok());
        let default_out = host.default_output_device().and_then(|d| d.id().ok());

        let devices = match host.devices() {
            Ok(d) => d,
            Err(e) => {
                snapshot.problems.push(format!("host {host_id}: {e}"));
                continue;
            }
        };

        for device in devices {
            let Ok(id) = device.id() else {
                // Without an id there is nothing to select on later, and offering
                // a device that cannot be re-found is worse than omitting it.
                snapshot
                    .problems
                    .push(format!("host {host_id}: a device would not report an id"));
                continue;
            };
            snapshot.devices.push(describe(
                &device,
                &id,
                default_in.as_ref() == Some(&id),
                default_out.as_ref() == Some(&id),
            ));
        }
    }
    snapshot
}

fn describe(
    device: &cpal::Device,
    id: &cpal::DeviceId,
    is_default_input: bool,
    is_default_output: bool,
) -> DeviceReport {
    let key = DeviceKey::from(id);
    let mut problems = Vec::new();

    let description = match device.description() {
        Ok(d) => Some(d),
        Err(e) => {
            problems.push(format!("description: {e}"));
            None
        }
    };
    let name = description
        .as_ref()
        .map(|d| d.name().to_owned())
        .unwrap_or_else(|| key.id().to_owned());
    let device_type = description
        .as_ref()
        .map(|d| d.device_type())
        .unwrap_or_default();

    // Querying configurations opens the PCM. A device held by another application
    // fails here rather than at stream build, and that is worth reporting.
    let (input, input_failure) = direction_report(
        device
            .supported_input_configs()
            .map(|it| it.collect::<Vec<_>>()),
        device.default_input_config(),
    );
    let (output, output_failure) = direction_report(
        device
            .supported_output_configs()
            .map(|it| it.collect::<Vec<_>>()),
        device.default_output_config(),
    );
    // A capture-only PCM reports its *output* configs as unavailable rather than
    // unsupported, and a genuinely absent device reports both. So a failure is
    // only worth showing when the device did not work in the other direction
    // either - otherwise every microphone acquires a spurious fault.
    note_failure(
        Direction::Input,
        input_failure,
        output.supported,
        &mut problems,
    );
    note_failure(
        Direction::Output,
        output_failure,
        input.supported,
        &mut problems,
    );

    DeviceReport {
        transport: Transport::classify(
            key.host(),
            key.id(),
            device_type == cpal::DeviceType::Virtual,
        ),
        manufacturer: description
            .as_ref()
            .and_then(|d| d.manufacturer().map(str::to_owned)),
        driver: description
            .as_ref()
            .and_then(|d| d.driver().map(str::to_owned)),
        device_type: format!("{device_type:?}"),
        interface: description
            .as_ref()
            .map(|d| format!("{:?}", d.interface_type()))
            .unwrap_or_else(|| "Unknown".to_owned()),
        is_default_input,
        is_default_output,
        input,
        output,
        problems,
        name,
        key,
    }
}

fn direction_report(
    configs: std::result::Result<Vec<cpal::SupportedStreamConfigRange>, cpal::Error>,
    default: std::result::Result<cpal::SupportedStreamConfig, cpal::Error>,
) -> (DirectionReport, Option<cpal::Error>) {
    let configs = match configs {
        Ok(v) => v,
        Err(e) => return (DirectionReport::default(), Some(e)),
    };
    let configs: Vec<ConfigRange> = configs.iter().map(range).collect();
    (
        DirectionReport {
            supported: !configs.is_empty(),
            default: default.ok().as_ref().map(exact),
            configs,
        },
        None,
    )
}

/// Records a probe failure, unless it is just "this device does not go that way".
fn note_failure(
    direction: Direction,
    failure: Option<cpal::Error>,
    other_direction_worked: bool,
    problems: &mut Vec<String>,
) {
    let Some(e) = failure else { return };
    let one_way_device = matches!(
        e.kind(),
        cpal::ErrorKind::UnsupportedOperation | cpal::ErrorKind::DeviceNotAvailable
    ) && other_direction_worked;
    if !one_way_device {
        problems.push(format!("{direction} configs: {e}"));
    }
}

fn range(r: &cpal::SupportedStreamConfigRange) -> ConfigRange {
    ConfigRange {
        channels: r.channels(),
        min_rate: r.min_sample_rate(),
        max_rate: r.max_sample_rate(),
        sample_format: format!("{:?}", r.sample_format()),
        format: probe::sample_format(r.sample_format()),
        bytes_per_sample: r.sample_format().sample_size(),
        buffer_frames: buffer_frames(r.buffer_size()),
    }
}

fn exact(c: &cpal::SupportedStreamConfig) -> ExactConfig {
    ExactConfig {
        channels: c.channels(),
        rate: SampleRate(c.sample_rate()),
        sample_format: format!("{:?}", c.sample_format()),
        format: probe::sample_format(c.sample_format()),
        buffer_frames: buffer_frames(c.buffer_size()),
    }
}

fn buffer_frames(size: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match size {
        cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
        cpal::SupportedBufferSize::Unknown => None,
    }
}

/// Re-opens a device by its persisted identity.
///
/// This is the only supported way back to a device across runs. Names are not
/// stable and, on ALSA, are shared by paths with different conversion behavior.
///
/// # Errors
///
/// [`Error::UnknownHost`] if the host is not in this build, [`Error::NoSuchDevice`]
/// if nothing on that host answers to the id - the ordinary unplugged case.
pub fn open(key: &DeviceKey, direction: Direction) -> Result<cpal::Device> {
    let cpal_id = key.to_cpal()?;
    let host = cpal::host_from_id(cpal_id.host())?;
    host.device_by_id(&cpal_id)
        .ok_or_else(|| Error::NoSuchDevice {
            direction,
            key: key.clone(),
        })
}

/// The host's default device for a direction.
///
/// Offered for completeness and used reluctantly. S1 finding 3: on Linux the
/// default input was the PipeWire path at 44.1 kHz F32, which for a vinyl capture
/// application is the one configuration guaranteed not to be bit-perfect.
///
/// # Errors
///
/// [`Error::NoDefault`] if the platform offers none.
pub fn default_device(direction: Direction) -> Result<cpal::Device> {
    let host = cpal::default_host();
    let device = match direction {
        Direction::Input => host.default_input_device(),
        Direction::Output => host.default_output_device(),
    };
    device.ok_or_else(|| Error::NoDefault {
        direction,
        host: host.id().to_string(),
    })
}

/// The host APIs this build of CPAL can actually use on this platform.
///
/// A smoke check rather than the §7 matrix - `vcw doctor` prints it so a support
/// question starts from what the binary can see.
pub fn available_hosts() -> Vec<&'static str> {
    cpal::available_hosts()
        .into_iter()
        .map(|h| h.name())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_least_one_host_is_compiled_in() {
        assert!(!available_hosts().is_empty());
    }

    #[test]
    fn a_key_round_trips_through_its_string_form() {
        let key = DeviceKey::new("alsa", "hw:CARD=0,DEV=0");
        assert_eq!(key.to_string(), "alsa:hw:CARD=0,DEV=0");
        // The id half contains colons; only the first one separates.
        assert_eq!(key.to_string().parse::<DeviceKey>().unwrap(), key);
        assert_eq!(key.id(), "hw:CARD=0,DEV=0");
        assert!(key.is_direct_hardware());
    }

    #[test]
    fn a_key_without_a_host_is_rejected() {
        assert!("hw:CARD=0,DEV=0".parse::<DeviceKey>().is_ok()); // host "hw", id "CARD=0,DEV=0"
        assert!("alsa".parse::<DeviceKey>().is_err());
        assert!(":x".parse::<DeviceKey>().is_err());
        assert!("x:".parse::<DeviceKey>().is_err());
    }

    #[test]
    fn transport_is_read_from_the_id_not_the_name() {
        let c = |id| Transport::classify("alsa", id, false);
        assert_eq!(c("hw:CARD=2,DEV=0"), Transport::DirectHardware);
        assert_eq!(c("plughw:CARD=2,DEV=0"), Transport::Converting);
        assert_eq!(c("pipewire"), Transport::Virtual);
        assert_eq!(c("default"), Transport::Virtual);
        assert_eq!(
            Transport::classify("wasapi", "{0.0.1.0}", false),
            Transport::Unknown
        );
        // The backend's own word beats the prefix: a loopback can be spelled `hw:`.
        assert_eq!(
            Transport::classify("alsa", "hw:CARD=9,DEV=0", true),
            Transport::Virtual
        );
    }

    #[test]
    fn unknown_transport_is_not_a_yes() {
        assert_eq!(Transport::DirectHardware.can_be_bit_perfect(), Some(true));
        assert_eq!(Transport::Converting.can_be_bit_perfect(), Some(false));
        assert_eq!(Transport::Unknown.can_be_bit_perfect(), None);
        assert_eq!(Transport::Virtual.can_be_bit_perfect(), None);
    }
}
