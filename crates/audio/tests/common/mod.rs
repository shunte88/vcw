/*
 *  mod.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Device fixtures.
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

//! Device fixtures.
//!
//! §7's hot-plug and persistence behavior is the part of the audio stack hardest
//! to test with real hardware: it needs two converters, a spare hand and a USB
//! cable. Everything except opening a stream is pure over [`DeviceReport`], so
//! these fixtures stand in for the cable.
//!
//! The shapes are taken from devices S1 actually met on the development host: an
//! HDA Intel line input reached three ways, and a webcam that advertises 8 kHz
//! mono.

#![allow(dead_code)]

use vcw_audio::devices::{
    ConfigRange, DeviceKey, DeviceReport, DirectionReport, ExactConfig, Snapshot, Transport,
};
use vcw_types::{SampleFormat, SampleRate};

pub(crate) fn range(
    channels: u16,
    min_rate: u32,
    max_rate: u32,
    format: SampleFormat,
) -> ConfigRange {
    ConfigRange {
        channels,
        min_rate,
        max_rate,
        sample_format: format!("{format:?}"),
        format: Some(format),
        bytes_per_sample: format.bytes_per_sample(),
        buffer_frames: Some((64, 8192)),
    }
}

pub(crate) fn input(configs: Vec<ConfigRange>) -> DirectionReport {
    let default = configs.first().map(|c| ExactConfig {
        channels: c.channels,
        rate: SampleRate(c.min_rate),
        sample_format: c.sample_format.clone(),
        format: c.format,
        buffer_frames: c.buffer_frames,
    });
    DirectionReport {
        supported: !configs.is_empty(),
        configs,
        default,
    }
}

/// A capture device, by default the `hw:` path S1 recorded 24/192 through.
pub(crate) fn device(id: &str, name: &str) -> DeviceReport {
    let key = DeviceKey::new("alsa", id);
    let transport = Transport::classify("alsa", id, false);
    DeviceReport {
        name: name.to_owned(),
        manufacturer: None,
        driver: Some("snd_hda_intel".to_owned()),
        device_type: "Unknown".to_owned(),
        interface: "BuiltIn".to_owned(),
        transport,
        is_default_input: false,
        is_default_output: false,
        input: input(vec![
            range(2, 44_100, 192_000, SampleFormat::S32),
            range(2, 44_100, 192_000, SampleFormat::S24),
        ]),
        output: DirectionReport::default(),
        problems: Vec::new(),
        key,
    }
}

pub(crate) fn snapshot(devices: Vec<DeviceReport>) -> Snapshot {
    Snapshot {
        devices,
        problems: Vec::new(),
    }
}

/// The three ALSA paths to one card, which is the case that makes selecting by
/// name unsafe: same converter, same name, different conversion behavior.
pub(crate) fn three_paths_to_one_card() -> Snapshot {
    let mut direct = device("hw:CARD=0,DEV=0", "HDA Intel PCH");
    direct.is_default_input = false;
    let plug = device("plughw:CARD=0,DEV=0", "HDA Intel PCH");
    let mut pipewire = device("pipewire", "PipeWire");
    pipewire.is_default_input = true;
    pipewire.input = input(vec![range(2, 44_100, 48_000, SampleFormat::F32)]);
    snapshot(vec![direct, plug, pipewire])
}
