/*
 *  devices.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  `vcw devices` and `vcw formats` - the §7 device matrix, on a terminal.
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

//! `vcw devices` and `vcw formats` - the §7 device matrix, on a terminal.
//!
//! WP-03's exit criterion is that the device matrix is reported on three
//! operating systems, and this is what reports it. The human form is arranged so
//! the two facts that decide a vinyl capture are the ones you see first: the id
//! to select on, and whether the path can be bit-perfect at all.
//!
//! `--json` exists because the same matrix has to be comparable across machines,
//! and reading three terminals side by side is not comparison.

use anyhow::{Context, Result};
use vcw_audio::devices::{self, DeviceReport, Direction, Snapshot, Transport};
use vcw_audio::probe::{self, Matrix, Support};

/// Which direction the user asked about.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Which {
    /// Capture devices only.
    Input,
    /// Playback devices only.
    Output,
    /// Both.
    Both,
}

impl Which {
    fn directions(self) -> &'static [Direction] {
        match self {
            Self::Input => &[Direction::Input],
            Self::Output => &[Direction::Output],
            Self::Both => &[Direction::Input, Direction::Output],
        }
    }
}

/// Lists every device on every host.
pub(crate) fn list(which: Which, hardware_only: bool, json: bool, verbose: bool) -> Result<()> {
    let snapshot = devices::enumerate();

    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
        return Ok(());
    }

    if snapshot.devices.is_empty() {
        println!(
            "no audio devices on hosts {}",
            devices::available_hosts().join(", ")
        );
    }
    for problem in &snapshot.problems {
        println!("! {problem}");
    }

    // Direct hardware first. On a Linux desktop the plugin and alias PCMs
    // outnumber the real cards several to one, and the card is what the user came
    // for (S1 finding 3).
    let mut shown: Vec<&DeviceReport> = snapshot
        .devices
        .iter()
        .filter(|d| which.directions().iter().any(|dir| d.supports(*dir)))
        .filter(|d| !hardware_only || d.transport == Transport::DirectHardware)
        .collect();
    shown.sort_by_key(|d| (d.key.host().to_owned(), transport_rank(d.transport)));

    let mut host = String::new();
    for device in shown {
        if device.key.host() != host {
            host = device.key.host().to_owned();
            println!("\n{}", host.to_uppercase());
        }
        print_device(device, which, verbose);
    }
    print_legend(&snapshot);
    Ok(())
}

/// Listing order: the paths that can be bit-perfect, then the unclassifiable
/// ones, then the software endpoints, then the ones that definitely convert.
fn transport_rank(transport: Transport) -> u8 {
    match transport {
        Transport::DirectHardware => 0,
        Transport::Unknown => 1,
        Transport::Virtual => 2,
        Transport::Converting => 3,
    }
}

/// Renders a channel-count list, collapsing runs. An ALSA plug PCM advertises
/// every count from 1 to 64, and printing all sixty-four of them buries the line
/// that matters.
fn channels(counts: &[u16]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < counts.len() {
        let start = counts[i];
        let mut end = start;
        while i + 1 < counts.len() && counts[i + 1] == end + 1 {
            i += 1;
            end = counts[i];
        }
        parts.push(if start == end {
            start.to_string()
        } else if end == start + 1 {
            format!("{start}, {end}")
        } else {
            format!("{start}-{end}")
        });
        i += 1;
    }
    parts.join(", ")
}

fn print_device(device: &DeviceReport, which: Which, verbose: bool) {
    let mut flags = Vec::new();
    if device.is_default_input {
        flags.push("default-in");
    }
    if device.is_default_output {
        flags.push("default-out");
    }
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!("  ({})", flags.join(", "))
    };

    println!("\n  {}{}", device.name, flags);
    println!("    id         {}", device.key);
    println!(
        "    path       {}{}",
        device.transport,
        bit_perfect_note(device.transport)
    );
    if let Some(m) = &device.manufacturer {
        println!("    made by    {m}");
    }
    if verbose {
        if let Some(d) = &device.driver {
            println!("    driver     {d}");
        }
        println!("    interface  {}", device.interface);
        println!("    type       {}", device.device_type);
    }

    for direction in which.directions() {
        let report = device.direction(*direction);
        if !report.supported {
            continue;
        }
        let matrix = Matrix::from_report(report, *direction);
        println!("    {:<10} {}", direction.to_string(), summarize(&matrix));
        if let Some(s) = matrix.suggest() {
            println!(
                "               suggested {} {:?} {} ch",
                s.rate, s.format, s.channels
            );
        }
        if let Some(default) = &report.default {
            println!(
                "               backend default {} {} {} ch",
                default.rate, default.sample_format, default.channels
            );
        }
        if verbose {
            for config in &report.configs {
                println!(
                    "               advertised {} ch, {}-{} Hz, {}{}",
                    config.channels,
                    config.min_rate,
                    config.max_rate,
                    config.sample_format,
                    if config.format.is_none() {
                        " (outside §8)"
                    } else {
                        ""
                    }
                );
            }
        }
    }
    for problem in &device.problems {
        println!("    !          {problem}");
    }
}

fn summarize(matrix: &Matrix) -> String {
    if matrix.is_empty() {
        return "nothing §8 can use".to_owned();
    }
    let rates: Vec<String> = matrix.rates().iter().map(|r| r.hz().to_string()).collect();
    let formats: Vec<String> = matrix.formats().iter().map(|f| format!("{f:?}")).collect();
    format!(
        "{} ch, {} Hz, {}",
        channels(&matrix.channel_counts()),
        rates.join(" "),
        formats.join(" ")
    )
}

fn bit_perfect_note(transport: Transport) -> &'static str {
    match transport.can_be_bit_perfect() {
        Some(true) => "  (can be bit-perfect, subject to verification)",
        Some(false) => "  (converts; never bit-perfect)",
        None => "  (bit-perfection unknown on this platform)",
    }
}

fn print_legend(snapshot: &Snapshot) {
    if snapshot.devices.is_empty() {
        return;
    }
    println!(
        "\nSelect by id, never by name: on ALSA one converter appears as both a hw: and a\n\
         plughw: PCM under the same name, and only the hw: path can be bit-perfect.\n\
         No line above is a bit-perfection claim - §9 settles that against the OS, not\n\
         against the backend's own report."
    );
}

/// Reports, and optionally confirms, what one device will accept.
pub(crate) fn formats(
    query: &str,
    which: Which,
    confirm: bool,
    max_channels: u16,
    json: bool,
) -> Result<()> {
    let snapshot = devices::enumerate();
    let directions: Vec<Direction> = which
        .directions()
        .iter()
        .copied()
        .filter(|d| snapshot.find(query, *d).is_ok())
        .collect();
    if directions.is_empty() {
        // Re-run the search so the user gets the real reason, ambiguity included.
        snapshot
            .find(query, which.directions()[0])
            .with_context(|| format!("looking up {query:?}"))?;
    }

    let mut matrices = Vec::new();
    for direction in directions {
        let device = snapshot.find(query, direction)?;
        let mut matrix = Matrix::from_report(device.direction(direction), direction);
        if confirm {
            matrix = matrix.with_channels_at_most(max_channels);
            let opened = devices::open(&device.key, direction)
                .with_context(|| format!("opening {}", device.label()))?;
            probe::confirm_all(&opened, &mut matrix);
        }
        matrices.push((device.label(), matrix));
    }

    if json {
        let payload: Vec<_> = matrices
            .iter()
            .map(|(label, m)| serde_json::json!({ "device": label, "matrix": m }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    for (label, matrix) in &matrices {
        println!("{label}  {}", matrix.direction);
        if matrix.is_empty() {
            println!("  nothing §8 can use");
            continue;
        }
        for entry in &matrix.entries {
            let state = match entry.support {
                Support::Advertised => "advertised",
                Support::Confirmed => "confirmed",
                Support::Rejected => "REJECTED",
            };
            println!(
                "  {:>6} Hz  {:<4}  {:>2} ch  {state}{}",
                entry.rate.hz(),
                format!("{:?}", entry.format),
                entry.channels,
                entry
                    .note
                    .as_deref()
                    .map(|n| format!("  {n}"))
                    .unwrap_or_default()
            );
        }
        if let Some(s) = matrix.suggest() {
            println!("  suggested: {} {:?} {} ch", s.rate, s.format, s.channels);
        }
        if confirm {
            println!("  confirmed by opening the device, up to {max_channels} channels");
        } else {
            println!(
                "  advertised only - pass --confirm to open the device and find out which\n  \
                 of these it will really accept"
            );
        }
        println!();
    }
    Ok(())
}
