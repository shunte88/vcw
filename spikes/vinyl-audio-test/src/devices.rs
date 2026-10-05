/*
 *  devices.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  REQUIREMENTS §47.1 and §47.2 - what devices exist, and what will they
 *  accept.
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

//! REQUIREMENTS §47.1 and §47.2 - what devices exist, and what will they accept.
//!
//! Enumeration is deliberately host-by-host rather than default-host-only. On
//! Linux the same physical converter appears under ALSA as `hw:`, `plughw:` and
//! whatever PipeWire or PulseAudio publishes, and those are *not* equivalent:
//! only the `hw:` path can be bit-perfect. A user picking a device from a list
//! (§5) is really picking a path through the stack, so the list has to show it.
//!
//! CPAL 0.18 makes that honest. 0.16 enumerated ALSA through `plughw:` only and
//! exposed no stable device identity, so the list was the plug layer's fiction
//! and there was no way to ask for the hardware. 0.18 enumerates `hw:` *and*
//! `plughw:` and gives every device a `DeviceId` - the PCM id itself - which is
//! what we key selection on here. See docs/spikes/S1-cpal-capture.md.

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ConfigRange {
    pub channels: u16,
    pub min_rate: u32,
    pub max_rate: u32,
    pub sample_format: String,
    pub bytes_per_sample: usize,
    pub buffer_frames: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub host: String,
    pub name: String,
    /// The backend's stable identifier - on ALSA the PCM id (`hw:2,0`). This is
    /// what to select on: names collide, ids do not.
    pub id: Option<String>,
    pub is_default_input: bool,
    pub is_default_output: bool,
    /// An ALSA `hw:` PCM - the only route that can be bit-perfect.
    pub direct_hardware: bool,
    /// An ALSA `plughw:` PCM - same hardware, but the plug layer will silently
    /// resample or reformat to satisfy whatever you ask for. Never bit-perfect.
    pub converting: bool,
    pub input_configs: Vec<ConfigRange>,
    pub output_configs: Vec<ConfigRange>,
    pub default_input: Option<ConfigRange>,
    pub default_output: Option<ConfigRange>,
    pub errors: Vec<String>,
}

fn range(r: &cpal::SupportedStreamConfigRange) -> ConfigRange {
    ConfigRange {
        channels: r.channels(),
        min_rate: r.min_sample_rate(),
        max_rate: r.max_sample_rate(),
        sample_format: format!("{:?}", r.sample_format()),
        bytes_per_sample: r.sample_format().sample_size(),
        buffer_frames: match r.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
            cpal::SupportedBufferSize::Unknown => None,
        },
    }
}

fn exact(c: &cpal::SupportedStreamConfig) -> ConfigRange {
    ConfigRange {
        channels: c.channels(),
        min_rate: c.sample_rate(),
        max_rate: c.sample_rate(),
        sample_format: format!("{:?}", c.sample_format()),
        bytes_per_sample: c.sample_format().sample_size(),
        buffer_frames: match c.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
            cpal::SupportedBufferSize::Unknown => None,
        },
    }
}

/// Human-readable name, falling back to the id and then a placeholder. A device
/// that will not describe itself is still worth listing.
fn device_name(d: &cpal::Device) -> Result<String, String> {
    match d.description() {
        Ok(desc) => Ok(desc.name().to_owned()),
        Err(e) => Err(format!("description: {e}")),
    }
}

fn device_id(d: &cpal::Device) -> Option<String> {
    d.id().ok().map(|i| i.id().to_owned())
}

/// Every device on every host CPAL compiled support for.
pub fn enumerate() -> Result<Vec<DeviceInfo>> {
    let mut out = Vec::new();
    for host_id in cpal::available_hosts() {
        let host = match cpal::host_from_id(host_id) {
            Ok(h) => h,
            Err(e) => {
                out.push(DeviceInfo {
                    host: format!("{host_id:?}"),
                    name: "<host unavailable>".into(),
                    id: None,
                    is_default_input: false,
                    is_default_output: false,
                    direct_hardware: false,
                    converting: false,
                    input_configs: vec![],
                    output_configs: vec![],
                    default_input: None,
                    default_output: None,
                    errors: vec![e.to_string()],
                });
                continue;
            }
        };
        // Compare defaults by id, not name: on ALSA several PCMs share a name.
        let def_in = host.default_input_device().and_then(|d| device_id(&d));
        let def_out = host.default_output_device().and_then(|d| device_id(&d));

        for device in host.devices()? {
            let mut errors = Vec::new();
            let id = device_id(&device);
            let name = device_name(&device).unwrap_or_else(|e| {
                errors.push(e);
                id.clone().unwrap_or_else(|| "<unnamed>".into())
            });
            // Querying configs opens the PCM; a device in use by something else
            // fails here rather than at stream build. That is worth reporting,
            // not swallowing - "no formats" and "busy" are different diagnoses.
            let input_configs = match device.supported_input_configs() {
                Ok(it) => it.map(|r| range(&r)).collect(),
                Err(e) => {
                    errors.push(format!("input configs: {e}"));
                    vec![]
                }
            };
            let output_configs = match device.supported_output_configs() {
                Ok(it) => it.map(|r| range(&r)).collect(),
                Err(e) => {
                    errors.push(format!("output configs: {e}"));
                    vec![]
                }
            };
            let idr = id.as_deref().unwrap_or("");
            out.push(DeviceInfo {
                host: format!("{host_id:?}"),
                is_default_input: id.is_some() && id == def_in,
                is_default_output: id.is_some() && id == def_out,
                direct_hardware: idr.starts_with("hw:"),
                converting: idr.starts_with("plughw:"),
                input_configs,
                output_configs,
                default_input: device.default_input_config().ok().map(|c| exact(&c)),
                default_output: device.default_output_config().ok().map(|c| exact(&c)),
                errors,
                name,
                id,
            });
        }
    }
    Ok(out)
}

/// Resolve a device by id, then by exact name, then by substring, across all
/// hosts. Ambiguity is an error: silently taking the first match of "USB" when
/// three devices contain it is how you record from the wrong converter.
///
/// Matching the id first is what lets a caller say `hw:2,0` and be certain they
/// got the hardware rather than the plug wrapper of the same name.
pub fn find(query: &str, want_input: bool) -> Result<(cpal::Device, String)> {
    let mut id_hits = Vec::new();
    let mut exact_hits = Vec::new();
    let mut fuzzy_hits = Vec::new();
    for host_id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(host_id) else {
            continue;
        };
        for device in host.devices()? {
            let usable = if want_input {
                device.supported_input_configs().is_ok()
            } else {
                device.supported_output_configs().is_ok()
            };
            if !usable {
                continue;
            }
            let id = device_id(&device);
            let name = device_name(&device)
                .unwrap_or_else(|_| id.clone().unwrap_or_else(|| "<unnamed>".into()));
            let label = match &id {
                Some(i) if i != &name => format!("{name} [{i}]"),
                _ => name.clone(),
            };
            if id.as_deref() == Some(query) {
                id_hits.push((device, label));
            } else if name == query {
                exact_hits.push((device, label));
            } else if name.contains(query) || id.as_deref().is_some_and(|i| i.contains(query)) {
                fuzzy_hits.push((device, label));
            }
        }
    }
    // An exact id match is unambiguous by construction - take it even if the
    // same string also appears inside other names.
    if id_hits.len() == 1 {
        return Ok(id_hits.pop().unwrap());
    }
    if id_hits.is_empty() && exact_hits.len() == 1 {
        return Ok(exact_hits.pop().unwrap());
    }
    if id_hits.is_empty() && exact_hits.is_empty() && fuzzy_hits.len() == 1 {
        return Ok(fuzzy_hits.pop().unwrap());
    }
    let candidates: Vec<&str> = id_hits
        .iter()
        .chain(exact_hits.iter())
        .chain(fuzzy_hits.iter())
        .map(|(_, n)| n.as_str())
        .collect();
    if candidates.is_empty() {
        anyhow::bail!(
            "no {} device matching {query:?}; run `devices` to list them",
            if want_input { "input" } else { "output" }
        );
    }
    anyhow::bail!("{query:?} is ambiguous, matches: {candidates:?} - select by id");
}

fn labeled(device: cpal::Device) -> (cpal::Device, String) {
    let id = device_id(&device);
    let name = device_name(&device).unwrap_or_else(|_| "<unnamed>".into());
    let label = match &id {
        Some(i) if i != &name => format!("{name} [{i}]"),
        _ => name,
    };
    (device, label)
}

/// The default input, for when the user does not name one.
pub fn default_input() -> Result<(cpal::Device, String)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("no default input device on host {:?}", host.id()))?;
    Ok(labeled(device))
}

/// The default output, for playback.
pub fn default_output() -> Result<(cpal::Device, String)> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no default output device on host {:?}", host.id()))?;
    Ok(labeled(device))
}
