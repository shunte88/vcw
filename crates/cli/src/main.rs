/*
 *  main.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  `vcw` - the headless driver.
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

//! `vcw` - the headless driver.
//!
//! §4.5 and WP-07 make this the primary interface to the core: the whole capture
//! workflow has to be drivable from here, with no UI present. That is not a
//! convenience, it is how the architectural rule in §2 gets tested - a core that
//! cannot be driven headlessly has leaked into its shell.
//!
//! Today it carries `doctor`, `devices` and `formats`, which answer "what can
//! this machine record, and through which path" (WP-03), `capture`, which
//! answers "and what did it actually do" (WP-04), and `soak`, which is how
//! WP-05's writer is measured on a machine before it is trusted with a side.
//! `session` arrives with the engine at WP-07 and is the one that matters: a
//! whole capture, driven by transport commands, with no UI present. `play` is
//! its opposite number from WP-10, and carries the device-free `--render` path
//! that makes §21's gapless seek testable without a sound card. The editing and
//! export verbs follow at WP-13 onwards.

mod capture;
mod contend;
mod detect;
mod devices;
mod export;
mod import;
mod metadata;
mod play;
mod recover;
mod release;
mod session;
mod soak;
mod tracks;
mod waveform;
mod wavfile;

use anyhow::Context;
use clap::{Parser, Subcommand};
use vcw_types::STANDARD_RATES;

use crate::capture::{Format, Mode};
use crate::devices::Which;
use crate::soak::Wal;

#[derive(Parser)]
#[command(name = "vcw", version, about = "VCW - The Vinyl Capture Workstation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report what this build can see: host APIs, SQLite, supported rates.
    Doctor,

    /// List audio devices, their ids, and what each one will record (§7).
    Devices {
        /// Restrict to capture or playback devices.
        #[arg(long, value_enum, default_value_t = Which::Both)]
        which: Which,
        /// Only direct-hardware paths: the ones that can be bit-perfect.
        #[arg(long)]
        hardware: bool,
        /// Machine-readable output, for comparing across machines.
        #[arg(long)]
        json: bool,
        /// Include drivers, interfaces and every advertised range.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show the §8 configurations one device accepts. Select it by id.
    Formats {
        /// Device id, as printed by `vcw devices`. A name works only if unique.
        device: String,
        /// Restrict to capture or playback.
        #[arg(long, value_enum, default_value_t = Which::Both)]
        which: Which,
        /// Open the device once per configuration to find out which it really
        /// accepts. Intrusive: it will fail while another application holds it.
        #[arg(long)]
        confirm: bool,
        /// Channel ceiling for --confirm. A plug PCM advertises 64 counts at
        /// every rate and format; confirming all of them opens the device
        /// thousands of times.
        #[arg(long, default_value_t = 8)]
        max_channels: u16,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Record from a device and report what was really negotiated (§9).
    ///
    /// With --project the samples are written; without one there is nowhere to
    /// put them, so they are drained, counted and discarded.
    Capture {
        /// Device id, as printed by `vcw devices`. A name works only if unique.
        device: String,
        /// Sample rate in Hz. Omit to take the best the device offers.
        #[arg(long)]
        rate: Option<u32>,
        /// Channel count. Omit to take the best on offer.
        #[arg(long)]
        channels: Option<u16>,
        /// Sample format. Omit to take the widest integer format available.
        #[arg(long, value_enum)]
        format: Option<Format>,
        /// How to open the device. Only exclusive can be bit-perfect (§9).
        #[arg(long, value_enum, default_value_t = Mode::Exclusive)]
        mode: Mode,
        /// How long to record.
        #[arg(long, default_value_t = 5.0)]
        seconds: f64,
        /// Ring capacity in milliseconds. Raised to the 500 ms floor if lower.
        #[arg(long, default_value_t = 1000)]
        ring_millis: u32,
        /// Project file to record the session in. Created if absent.
        #[arg(long)]
        project: Option<std::path::PathBuf>,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Drive a whole capture session from the transport commands (§11, §35).
    ///
    /// Reads one verb per line from stdin, or from --script. The verbs are
    /// arm, record, pause, resume, stop, reset, disarm, poll and quit, plus
    /// `sleep <seconds>` for a script that wants to record for a while. Every
    /// event the core publishes is printed as it happens.
    ///
    /// This is WP-07's exit criterion: a full capture, with no UI present.
    Session {
        /// Project to record into. Created if it does not exist.
        project: std::path::PathBuf,
        /// Device id, as printed by `vcw devices`. Omit for the simulated
        /// source, which needs nothing plugged in.
        #[arg(long)]
        device: Option<String>,
        /// Sample rate in Hz. Omit to take the best the device offers.
        #[arg(long)]
        rate: Option<u32>,
        /// Channel count. Omit to take the best on offer.
        #[arg(long)]
        channels: Option<u16>,
        /// Sample format. Omit to take the widest integer format available.
        #[arg(long, value_enum)]
        format: Option<Format>,
        /// How to open the device. Only exclusive can be bit-perfect (§9).
        #[arg(long, value_enum, default_value_t = Mode::Exclusive)]
        mode: Mode,
        /// Ring capacity in milliseconds. Raised to the 500 ms floor if lower.
        #[arg(long)]
        ring_millis: Option<u32>,
        /// A whole session on one line: --script "arm,record,sleep 2,stop".
        #[arg(long)]
        script: Option<String>,
        /// Machine-readable output: one JSON object per line.
        #[arg(long)]
        json: bool,
        /// Print the 50 Hz level meters too. Loud, and off by default.
        #[arg(long)]
        meters: bool,
    },

    /// Play a capture, a region, a track or a boundary (§21).
    ///
    /// VCW does not resample: a capture plays at its own rate or the device is
    /// refused, which is why this can be honest about bit-perfect playback.
    /// With `--render` it needs no device at all and writes the bytes the
    /// converter would have been handed.
    Play {
        /// Project to play from.
        project: std::path::PathBuf,
        /// Capture to play. Omit for the most recent.
        #[arg(long)]
        capture: Option<i64>,
        /// Where to start, in seconds. Omit for the beginning.
        #[arg(long)]
        start: Option<f64>,
        /// Where to end, in seconds. Omit for the end of the capture.
        #[arg(long)]
        end: Option<f64>,
        /// Audition the boundary at this many seconds, with three seconds of
        /// context either side.
        #[arg(long, conflicts_with_all = ["start", "end"])]
        boundary: Option<f64>,
        /// Call the region a track. Needs --start and --end until WP-13
        /// records boundaries in the project.
        #[arg(long)]
        track: Option<u32>,
        /// Output device id, as printed by `vcw devices --which output`. Omit
        /// for the system default.
        #[arg(long)]
        device: Option<String>,
        /// Stream format to insist on. Omit to let playback pick the one that
        /// converts least, which is usually none at all.
        #[arg(long, value_enum)]
        format: Option<Format>,
        /// How to open the device. Only exclusive can be bit-perfect (§9).
        #[arg(long, value_enum, default_value_t = Mode::Exclusive)]
        mode: Mode,
        /// A whole audition on one line: --script "play,sleep 2,seek 30,stop".
        #[arg(long)]
        script: Option<String>,
        /// Write raw interleaved audio here instead of playing it. Needs no
        /// device, and is what makes a gapless seek something a test can check.
        #[arg(long)]
        render: Option<std::path::PathBuf>,
        /// Machine-readable output: one JSON object per line.
        #[arg(long)]
        json: bool,
    },

    /// Find the track boundaries in a stored capture (§22, §24).
    ///
    /// Runs the post-capture pass: one spectral extraction, all three
    /// detectors, one resolver. Writes nothing - a boundary becomes a track in
    /// the editor, and this is how to see what the editor would be handed.
    Detect {
        /// Project to analyse.
        project: std::path::PathBuf,
        /// Capture to analyse. Omit for the most recent.
        #[arg(long)]
        capture: Option<i64>,
        /// Level a window must reach to count as music, in dBFS.
        #[arg(long)]
        threshold_db: Option<f64>,
        /// Derive the threshold from the side's own noise floor (§22).
        #[arg(long)]
        adaptive: bool,
        /// Shortest gap that can separate two tracks, in seconds.
        #[arg(long)]
        min_silence: Option<f64>,
        /// Shortest span that can be a track, in seconds.
        #[arg(long)]
        min_sound: Option<f64>,
        /// Report only boundaries this many detectors reported. The HMM finds
        /// one at every quiet bar of a real side, so 2 is how to see the
        /// boundaries a second detector seconded.
        #[arg(long, default_value_t = 1)]
        min_sources: usize,
        /// Print every measurement behind every boundary (§24).
        #[arg(long)]
        evidence: bool,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Draw a capture's waveform at the terminal (§19).
    ///
    /// The pyramid picks its own resolution from the span and the width, so
    /// this reads the same rows a UI would and reports which level it used.
    Waveform {
        /// Project to draw from.
        project: std::path::PathBuf,
        /// Capture to draw. Omit for the most recent.
        #[arg(long)]
        capture: Option<i64>,
        /// Channel to draw. Omit for all of them.
        #[arg(long)]
        channel: Option<u16>,
        /// Where to start, in seconds.
        #[arg(long)]
        start: Option<f64>,
        /// Where to end, in seconds. Omit for the end of the capture.
        #[arg(long)]
        end: Option<f64>,
        /// Columns to draw.
        #[arg(long, default_value_t = 100)]
        pixels: u32,
        /// Rows per channel.
        #[arg(long, default_value_t = 12)]
        rows: u32,
        /// Recompute the pyramid from the stored audio before drawing (§19).
        #[arg(long)]
        rebuild: bool,
        /// Machine-readable output: the columns as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Find unfinished captures left by a crash and close them honestly (§15).
    ///
    /// Reports by default and writes nothing. A recording that survived a
    /// crash is worth more than the convenience of not typing --apply.
    Recover {
        /// Project to examine.
        project: std::path::PathBuf,
        /// Write the reconstructed frame count, state and end time.
        #[arg(long)]
        apply: bool,
        /// Also delete blocks stranded past the recoverable end. Implies
        /// --apply, and is the only way to make recovery discard audio.
        #[arg(long)]
        repair: bool,
        /// Recompute every block's checksum afterwards. Reads the whole
        /// project, which is minutes for a full side.
        #[arg(long)]
        verify: bool,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Import an Audacity `.aup3` or `.aup4` project as a `.vcw` (§12).
    ///
    /// Writes a new project and never touches the source, which is opened
    /// read-only. The audio is re-blocked through the capture writer, so what
    /// lands is a capture like any other: playback, export, detection and
    /// tagging all work on it unchanged.
    Import {
        /// The Audacity project to read.
        source: std::path::PathBuf,
        /// Where to write the project. Defaults to the source with a .vcw
        /// extension. Refused if it already exists.
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
        /// Which side the capture is. An Audacity project does not say.
        #[arg(long, default_value_t = 'A')]
        side: char,
        /// Report what would land and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Do not turn labels into tracks.
        #[arg(long)]
        no_labels: bool,
        /// Do not copy the tags onto the release.
        #[arg(long)]
        no_tags: bool,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Search a metadata provider, or read one release (§28, §40).
    ///
    /// Offline is a first-class mode, not a simulation: `--offline` hands the
    /// providers the same refusing transport the application defaults to, so
    /// every path here can be exercised with no network and no credential.
    Metadata {
        #[command(subcommand)]
        what: MetadataCommand,
    },

    /// Lay out a record: sides, tracks, boundaries, and the edits of §31.
    ///
    /// Nothing here writes a sample. §4.1 means an edit is a statement about
    /// where the music is, so every one of these subcommands touches only the
    /// side, boundary and track rows - which `vcw validate` will confirm.
    Tracks {
        /// Project to edit.
        project: std::path::PathBuf,
        #[command(subcommand)]
        what: TracksCommand,
        /// Machine-readable output.
        #[arg(long, global = true)]
        json: bool,
    },

    /// Read or write the release the project is of (§28, §32).
    Release {
        /// Project to read.
        project: std::path::PathBuf,
        #[command(subcommand)]
        what: ReleaseCommand,
        /// Machine-readable output.
        #[arg(long, global = true)]
        json: bool,
    },

    /// Turn a finished project into files (§33).
    ///
    /// Reads immutable blocks plus the edit instructions and writes nothing
    /// back, so the project is opened read-only. `--dry-run` resolves the whole
    /// plan - every path, every tag, every frame count - and stops there.
    Export {
        /// Project to export from.
        project: std::path::PathBuf,
        /// Directory the files go under. Created if it is not there.
        #[arg(long)]
        into: std::path::PathBuf,
        /// Container to write: wav or flac.
        #[arg(long, default_value = "flac")]
        format: String,
        /// Naming template. Omit for the default.
        #[arg(long)]
        template: Option<String>,
        /// A side letter. Repeat for several. Omit for every side.
        #[arg(long = "side")]
        sides: Vec<char>,
        /// What to do with the front cover: none, embed, folder or both.
        #[arg(long, default_value = "both")]
        artwork: String,
        /// Replace files that are already there.
        #[arg(long)]
        overwrite: bool,
        /// Resolve and print the plan, and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Drive the writer from a simulated source for a long time and check
    /// every byte that lands (WP-05, D3).
    Soak {
        /// Project to write. Must not already exist.
        project: std::path::PathBuf,
        /// Sample rate in Hz.
        #[arg(long, default_value_t = 192_000)]
        rate: u32,
        /// Channel count.
        #[arg(long, default_value_t = 2)]
        channels: u16,
        /// Sample format.
        #[arg(long, value_enum, default_value_t = Format::S24)]
        format: Format,
        /// How long to run, in minutes.
        #[arg(long, default_value_t = 90.0)]
        minutes: f64,
        /// Block duration in milliseconds. D3 says 250.
        #[arg(long, default_value_t = 250)]
        block_millis: u32,
        /// Blocks per transaction. D3 says 1.
        #[arg(long, default_value_t = 1)]
        batch_blocks: usize,
        /// WAL policy. D3 says automatic.
        #[arg(long, value_enum, default_value_t = Wal::Automatic)]
        wal: Wal,
        /// Blocks between writer-issued checkpoints, for the non-automatic
        /// policies.
        #[arg(long, default_value_t = 64)]
        checkpoint_blocks: u64,
        /// WAL ceiling in MiB, for the automatic policy. SQLite counts pages;
        /// VCW's are 64 KiB, so the stock threshold would be a 64 MiB log.
        #[arg(long, default_value_t = 4)]
        wal_mib: u64,
        /// Ring capacity in milliseconds. Raised to the 500 ms floor if lower.
        #[arg(long, default_value_t = 1000)]
        ring_millis: u32,
        /// Run flat out. Fast, and worthless as a timing measurement.
        #[arg(long)]
        fast: bool,
        /// Skip the byte-for-byte readback.
        #[arg(long)]
        no_verify: bool,
        /// Feed a real WAV rip through the capture path instead of the
        /// generated pattern. Its rate, channels and format win over the flags.
        /// §41's file-backed capture.
        #[arg(long, value_name = "WAV")]
        from_file: Option<std::path::PathBuf>,
        /// Go silent after this many seconds of audio, saying nothing, the way
        /// an unplugged device actually presents. R9.
        #[arg(long)]
        vanish_after: Option<f64>,
        /// The same, with a stream error reported first.
        #[arg(long)]
        unplug_after: Option<f64>,
        /// Report a stream error after this many seconds and keep going.
        #[arg(long)]
        error_after: Option<f64>,
        /// Deliver one empty callback after this many seconds.
        #[arg(long)]
        starve_after: Option<f64>,
        /// Fail if resident memory grows by more than this many MiB over the
        /// run. Zero to measure without gating. §41.
        #[arg(long, default_value_t = 32)]
        max_growth_mib: u64,
        /// Run this many threads drawing the waveform while the capture writes,
        /// which is what the window does during a recording. §41.
        #[arg(long, default_value_t = 0)]
        readers: usize,
        /// Redraws a second per reader. 60 is a window; 0 is flat out, which is
        /// a fuzzer and starves the WAL checkpoint on purpose.
        #[arg(long, default_value_t = 60)]
        reader_hz: u32,
        /// Fail if the WAL peaks above this many times its budget. Zero to
        /// measure without gating. §41.
        #[arg(long, default_value_t = 4)]
        wal_slack: u32,
        /// Report the commit latency without failing the run on it. For shared
        /// machines - a CI runner's commit tail measures the runner, not the
        /// writer. Every other check still applies.
        #[arg(long)]
        ignore_commit_budget: bool,
        /// Seconds between progress lines. Zero for silence.
        #[arg(long, default_value_t = 60)]
        every: u64,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
}

/// What `vcw metadata` was asked to do.
#[derive(Subcommand)]
enum MetadataCommand {
    /// Search for a release by any of §28's criteria.
    Search {
        /// The performing artist.
        #[arg(long)]
        artist: Option<String>,
        /// The release title.
        #[arg(long)]
        album: Option<String>,
        /// The label's catalogue number. Usually identifies one pressing, which
        /// is the difference between finding a record and finding the record.
        #[arg(long)]
        catalog: Option<String>,
        /// The barcode on the sleeve. Discogs indexes it; MusicBrainz does not.
        #[arg(long)]
        barcode: Option<String>,
        /// The record label.
        #[arg(long)]
        label: Option<String>,
        /// Year of release.
        #[arg(long)]
        year: Option<u32>,
        /// Country of release.
        #[arg(long)]
        country: Option<String>,
        /// Include CDs, files and cassettes. Vinyl only is the default.
        #[arg(long)]
        all_formats: bool,
        /// How many results to ask each provider for.
        #[arg(long, default_value_t = 25)]
        limit: usize,
        /// Which provider to ask.
        #[arg(long = "provider", value_enum, default_value_t = metadata::Which::Both)]
        which: metadata::Which,
        /// Refuse to use the network (§40).
        #[arg(long)]
        offline: bool,
        /// Directory to cache provider responses in. Omit for no cache.
        #[arg(long)]
        cache: Option<std::path::PathBuf>,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Read one release in full, with its tracklist.
    Fetch {
        /// The provider's identifier: a MusicBrainz id, or a Discogs number.
        id: String,
        /// Which provider it belongs to. Inferred from the shape when it can be.
        #[arg(long = "provider", value_enum, default_value_t = metadata::Which::Both)]
        which: metadata::Which,
        /// Refuse to use the network (§40).
        #[arg(long)]
        offline: bool,
        /// Directory to cache provider responses in.
        #[arg(long)]
        cache: Option<std::path::PathBuf>,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Normalise genre names through the §32 mapping table. Needs no network.
    Genres {
        /// Names to normalise, semicolon-delimited or one per argument.
        names: Vec<String>,
        /// A replacement mapping table, in the shipped file's format.
        #[arg(long)]
        table: Option<std::path::PathBuf>,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Say which credentials are configured, and never what they are (§39).
    Credentials,
}

/// What `vcw tracks` was asked to do.
#[derive(Subcommand)]
enum TracksCommand {
    /// Print the project's sides, tracks and boundaries.
    List {
        /// One side. Omit for every side.
        #[arg(long)]
        side: Option<char>,
        /// Print boundaries too, with the evidence behind each one (§24).
        #[arg(long)]
        boundaries: bool,
    },
    /// Point a side at the capture that recorded it.
    Attach {
        /// The side letter. A is the first face of the first disc.
        #[arg(long)]
        side: char,
        /// The capture. Omit for the most recent.
        #[arg(long)]
        capture: Option<i64>,
    },
    /// Add a track between two times.
    Add {
        /// The side letter.
        #[arg(long)]
        side: char,
        /// Where it starts, in seconds.
        #[arg(long)]
        start: f64,
        /// Where it ends, in seconds.
        #[arg(long)]
        end: f64,
    },
    /// Split a track in two.
    Split {
        /// The track.
        track: i64,
        /// Where to cut, in seconds from the start of the side.
        at: f64,
    },
    /// Merge two adjacent tracks, keeping the first one's metadata.
    ///
    /// The boundary between them has to be unlocked first if a person placed
    /// it: undoing your own split is a decision only you can make (§24).
    Merge {
        /// The track that survives.
        left: i64,
        /// The track folded into it.
        right: i64,
    },
    /// Delete a track. Keeps every sample it covered (§4.1).
    Delete {
        /// The track.
        track: i64,
    },
    /// Move a boundary, and with it whichever tracks it bounds.
    Move {
        /// The boundary. `vcw tracks list --boundaries` prints the ids.
        boundary: i64,
        /// Where to, in seconds.
        to: f64,
        /// Move it even if it is locked. Also claims it as yours (§24).
        #[arg(long)]
        force: bool,
    },
    /// Lock a boundary against analysis, or hand it back (§24).
    Lock {
        /// The boundary.
        boundary: i64,
        /// Unlock instead.
        #[arg(long)]
        unlock: bool,
    },
    /// Set a track's metadata (§32).
    ///
    /// An empty value clears a field back to the release's, which is what NULL
    /// means in those columns. The title is the exception: empty is untitled.
    Set {
        /// The track.
        track: i64,
        /// The title.
        #[arg(long)]
        title: Option<String>,
        /// The track artist, for a compilation.
        #[arg(long)]
        artist: Option<String>,
        /// The composer.
        #[arg(long)]
        composer: Option<String>,
        /// Free text.
        #[arg(long)]
        comments: Option<String>,
        /// The MusicBrainz recording id.
        #[arg(long)]
        recording: Option<String>,
        /// Mark the metadata as accepted (§26).
        #[arg(long)]
        confirm: bool,
    },
    /// Move a track to another side of the same capture.
    Reassign {
        /// The track.
        track: i64,
        /// The side letter.
        #[arg(long)]
        to: char,
    },
    /// Run a detection pass and write what the policy accepts (§24).
    ///
    /// The one subcommand that can place a boundary without a person naming a
    /// frame, so it is conservative by default: two detectors have to agree, or
    /// it does not go in. A boundary a person locked is left exactly where it is.
    Adopt {
        /// The side letter.
        #[arg(long)]
        side: char,
        /// How many detectors must have reported a boundary.
        #[arg(long, default_value_t = 2)]
        min_sources: usize,
        /// The shortest span worth calling a track, in seconds.
        #[arg(long)]
        min_track: Option<f64>,
        /// Report what would be written, and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Level a window must reach to count as music, in dBFS.
        #[arg(long)]
        threshold_db: Option<f64>,
        /// Derive the threshold from the side's own noise floor (§22).
        #[arg(long)]
        adaptive: bool,
    },
}

/// What `vcw release` was asked to do.
#[derive(Subcommand)]
enum ReleaseCommand {
    /// Print what the project knows about the release.
    Show,
    /// Set one or more of the release's fields (§32).
    ///
    /// An empty value clears a field. Identification fills most of these in
    /// (§28); this is the operator's own hand, and what §5.3's new-project
    /// prompt will write.
    Set(Box<ReleaseFields>),
    /// Store a cover image for the release (§32).
    Artwork {
        /// The image file.
        file: std::path::PathBuf,
        /// Which image it is: front, back, label or other.
        #[arg(long, default_value = "front")]
        role: String,
    },
    /// Print which artwork the project holds, without the bytes.
    Covers,
}

/// The fields `vcw release set` takes.
///
/// A boxed struct rather than a variant's worth of fields: thirteen of them make
/// `ReleaseCommand` several hundred bytes wide, and every copy of the enum would
/// carry that width around for the sake of the two one-word variants beside it.
#[derive(Debug, clap::Args)]
struct ReleaseFields {
    /// The album title.
    #[arg(long)]
    album: Option<String>,
    /// The album artist.
    #[arg(long)]
    artist: Option<String>,
    /// The year of this pressing.
    #[arg(long)]
    year: Option<u32>,
    /// Genres, separated by semicolons, in order.
    #[arg(long)]
    genres: Option<String>,
    /// The label.
    #[arg(long)]
    label: Option<String>,
    /// The catalogue number, which is what identifies a pressing.
    #[arg(long)]
    catalog: Option<String>,
    /// The country of pressing.
    #[arg(long)]
    country: Option<String>,
    /// The barcode.
    #[arg(long)]
    barcode: Option<String>,
    /// The composer.
    #[arg(long)]
    composer: Option<String>,
    /// Free text.
    #[arg(long)]
    comments: Option<String>,
    /// How many discs the record is.
    #[arg(long)]
    discs: Option<u32>,
    /// Track numbering: alpha for A1, numeric for a running count.
    #[arg(long)]
    numbering: Option<String>,
    /// Mark the release's metadata as accepted (§26).
    #[arg(long)]
    confirm: bool,
}
/// Stack for [`run`]. 8 MiB is what Linux and macOS hand a main thread by
/// default, so this gives every platform the size the two we develop on already
/// had.
const RUN_STACK_BYTES: usize = 8 * 1024 * 1024;

/// Runs the whole CLI on a thread whose stack size we chose.
///
/// # Why this is not just a call to `run`
///
/// Windows gives a process's main thread **1 MiB**, against 8 MiB on Linux and
/// macOS, and it is fixed in the executable header rather than asked for at run
/// time. `Cli::parse()` does not fit in 1 MiB in a debug build: clap's derive
/// expands an `augment_subcommands` function per subcommand enum that builds
/// every `Command` and every `Arg` as a local, and unoptimised they are all live
/// at once. Measured on this tree, a debug `vcw --version` needs between 1.0 and
/// 1.5 MiB and aborts with `thread 'main' has overflowed its stack`, while a
/// release build of the same commit runs in 256 KiB.
///
/// So this was never a defect in a shipped binary, because releases are what
/// ship. It was that **no integration test that spawns this binary could pass on
/// Windows**, since `cargo test` builds it in debug: three `detect_from_cli`
/// tests failed with the child's overflow quoted in the assertion, and the rest
/// of the CLI suite never ran because cargo stops at the first failing target.
///
/// # Why a thread and not `/STACK:`
///
/// A thread stack is mmapped, so it is governed by neither the executable header
/// nor `RLIMIT_STACK`. That makes the fix portable and, more importantly,
/// **testable from the dev box**: `ulimit -s 1024` reproduces Windows' ceiling on
/// Linux, which is how this was found without a Windows machine, and
/// `tests/startup.rs` keeps it that way. Raising the header with the MSVC
/// linker's `/STACK:` would have fixed Windows and left nothing that fails here.
fn main() -> anyhow::Result<()> {
    std::thread::Builder::new()
        .name("vcw".to_owned())
        .stack_size(RUN_STACK_BYTES)
        .spawn(run)
        .context("spawning the main thread")?
        .join()
        // Re-raised rather than reported, so a panic in `run` still looks
        // exactly like a panic: same message, same backtrace, same exit code.
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

fn run() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Doctor => doctor(),
        Command::Devices {
            which,
            hardware,
            json,
            verbose,
        } => devices::list(which, hardware, json, verbose),
        Command::Formats {
            device,
            which,
            confirm,
            max_channels,
            json,
        } => devices::formats(&device, which, confirm, max_channels, json),
        Command::Capture {
            device,
            rate,
            channels,
            format,
            mode,
            seconds,
            ring_millis,
            project,
            json,
        } => capture::run(&capture::Options {
            device,
            rate,
            channels,
            format,
            mode,
            seconds,
            ring_millis,
            project,
            json,
        }),
        Command::Session {
            project,
            device,
            rate,
            channels,
            format,
            mode,
            ring_millis,
            script,
            json,
            meters,
        } => session::run(&session::Args {
            project,
            device,
            rate,
            channels,
            format,
            mode,
            ring_millis,
            script,
            json,
            meters,
        }),
        Command::Play {
            project,
            capture,
            start,
            end,
            boundary,
            track,
            device,
            format,
            mode,
            script,
            render,
            json,
        } => play::run(&play::Args {
            project,
            capture,
            start,
            end,
            boundary,
            track,
            device,
            format,
            mode,
            script,
            render,
            json,
        }),
        Command::Detect {
            project,
            capture,
            threshold_db,
            adaptive,
            min_silence,
            min_sound,
            min_sources,
            evidence,
            json,
        } => detect::run(&detect::Args {
            project,
            capture,
            threshold_db,
            adaptive,
            min_silence,
            min_sound,
            min_sources,
            evidence,
            json,
        }),
        Command::Waveform {
            project,
            capture,
            channel,
            start,
            end,
            pixels,
            rows,
            rebuild,
            json,
        } => waveform::run(&waveform::Args {
            project,
            capture,
            channel,
            start,
            end,
            pixels,
            rows,
            rebuild,
            json,
        }),
        Command::Recover {
            project,
            apply,
            repair,
            verify,
            json,
        } => recover::run(&recover::Args {
            project,
            apply,
            repair,
            verify,
            json,
        }),
        Command::Import {
            source,
            output,
            side,
            dry_run,
            no_labels,
            no_tags,
            json,
        } => import::run(&import::Args {
            source,
            output,
            side,
            dry_run,
            no_labels,
            no_tags,
            json,
        }),
        Command::Export {
            project,
            into,
            format,
            template,
            sides,
            artwork,
            overwrite,
            dry_run,
            json,
        } => export::run(&export::Args {
            project,
            into,
            format,
            template,
            sides,
            artwork,
            overwrite,
            dry_run,
            json,
        }),

        Command::Tracks {
            project,
            what,
            json,
        } => tracks::run(&tracks::Args {
            project,
            json,
            task: match what {
                TracksCommand::List { side, boundaries } => tracks::Task::List { side, boundaries },
                TracksCommand::Attach { side, capture } => tracks::Task::Attach { side, capture },
                TracksCommand::Add { side, start, end } => tracks::Task::Add { side, start, end },
                TracksCommand::Split { track, at } => tracks::Task::Split { track, at },
                TracksCommand::Merge { left, right } => tracks::Task::Merge { left, right },
                TracksCommand::Delete { track } => tracks::Task::Delete { track },
                TracksCommand::Move {
                    boundary,
                    to,
                    force,
                } => tracks::Task::Move {
                    boundary,
                    to,
                    force,
                },
                TracksCommand::Lock { boundary, unlock } => tracks::Task::Lock { boundary, unlock },
                TracksCommand::Set {
                    track,
                    title,
                    artist,
                    composer,
                    comments,
                    recording,
                    confirm,
                } => tracks::Task::Set {
                    track,
                    update: vcw_project::track::Update {
                        title,
                        artist,
                        composer,
                        comments,
                        musicbrainz_id: recording,
                        confirmed: confirm.then_some(true),
                    },
                },
                TracksCommand::Reassign { track, to } => tracks::Task::Reassign { track, to },
                TracksCommand::Adopt {
                    side,
                    min_sources,
                    min_track,
                    dry_run,
                    threshold_db,
                    adaptive,
                } => tracks::Task::Adopt {
                    side,
                    min_sources,
                    min_track,
                    dry_run,
                    threshold_db,
                    adaptive,
                },
            },
        }),

        Command::Release {
            project,
            what,
            json,
        } => release::run(&release::Args {
            project,
            json,
            task: match what {
                ReleaseCommand::Show => release::Task::Show,
                ReleaseCommand::Covers => release::Task::Covers,
                ReleaseCommand::Artwork { file, role } => release::Task::Artwork { file, role },
                ReleaseCommand::Set(fields) => release::Task::Set(Box::new(release::Change {
                    album: fields.album,
                    artist: fields.artist,
                    year: fields.year,
                    genres: fields.genres,
                    label: fields.label,
                    catalog: fields.catalog,
                    country: fields.country,
                    barcode: fields.barcode,
                    composer: fields.composer,
                    comments: fields.comments,
                    discs: fields.discs,
                    numbering: fields.numbering,
                    confirm: fields.confirm,
                })),
            },
        }),

        Command::Metadata { what } => metadata::run(match what {
            MetadataCommand::Search {
                artist,
                album,
                catalog,
                barcode,
                label,
                year,
                country,
                all_formats,
                limit,
                which,
                offline,
                cache,
                json,
            } => metadata::Args::Search(Box::new(metadata::SearchArgs {
                artist,
                album,
                catalog,
                barcode,
                label,
                year,
                country,
                all_formats,
                limit,
                which,
                offline,
                cache,
                json,
            })),
            MetadataCommand::Fetch {
                id,
                which,
                offline,
                cache,
                json,
            } => metadata::Args::Fetch(metadata::FetchArgs {
                id,
                which,
                offline,
                cache,
                json,
            }),
            MetadataCommand::Genres { names, table, json } => {
                metadata::Args::Genres { names, table, json }
            }
            MetadataCommand::Credentials => metadata::Args::Credentials,
        }),
        Command::Soak {
            project,
            rate,
            channels,
            format,
            minutes,
            block_millis,
            batch_blocks,
            wal,
            checkpoint_blocks,
            wal_mib,
            ring_millis,
            fast,
            no_verify,
            from_file,
            vanish_after,
            unplug_after,
            error_after,
            starve_after,
            max_growth_mib,
            readers,
            reader_hz,
            wal_slack,
            ignore_commit_budget,
            every,
            json,
        } => soak::run(&soak::Options {
            project,
            rate,
            channels,
            format,
            minutes,
            block_millis,
            batch_blocks,
            wal,
            checkpoint_blocks,
            wal_mib,
            ring_millis,
            fast,
            no_verify,
            from_file,
            vanish_after,
            unplug_after,
            error_after,
            starve_after,
            max_growth_mib,
            readers,
            reader_hz,
            wal_slack,
            ignore_commit_budget,
            every,
            json,
        }),
    }
}

fn doctor() -> anyhow::Result<()> {
    println!("vcw {}", env!("CARGO_PKG_VERSION"));
    println!("target      {}", std::env::consts::ARCH);
    println!("os          {}", std::env::consts::OS);
    println!(
        "sqlite      {} (bundled)",
        vcw_project::sqlite::runtime_version()
    );

    let hosts = vcw_audio::devices::available_hosts();
    println!("audio hosts {}", hosts.join(", "));

    let rates: Vec<String> = STANDARD_RATES.iter().map(|r| r.hz().to_string()).collect();
    println!("rates (§8)  {}", rates.join(", "));

    Ok(())
}
