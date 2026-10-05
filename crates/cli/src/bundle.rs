/*
 *  bundle.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `bundle` verb: a diagnostic report with no audio in it (§42).
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
//! The `bundle` verb: a diagnostic report with no audio in it (§42).
//!
//! §42 asks for a bundle that reports the application version, the OS, the
//! backend, the device configuration, project and database integrity, and
//! capture errors, *without including recorded audio*. That last clause is the
//! design constraint, and it is why this is a report rather than an archive: a
//! bundle that copied the project would be a bundle that copied the record, and
//! a 400 MB attachment nobody can email.
//!
//! So what comes out is one JSON document of facts about a project, not a
//! extract of it. The rule this module holds to is that everything it emits is
//! a **shape or a count**, never content:
//!
//! * The four capture counters, the validator's findings and SQLite's own
//!   integrity check - those are the diagnosis.
//! * Row counts for sides, tracks and blocks - enough to tell an empty project
//!   from a full one.
//! * The file name, never the path, because a path carries a home directory and
//!   the directory a person keeps their music in is nobody's business.
//! * Nothing from `tracks.title`, nothing from `release`, and none of the
//!   `import.tag.*` keys an Audacity import leaves in `meta`. An album title
//!   cannot help anybody debug a dropout, and a bundle is something a person
//!   sends to a stranger.
//! * Credentials by name and length only (§39). The value never appears, and
//!   `VCW_CONTACT` not even by length, since it is an email address.
//!
//! # It has to survive a broken project
//!
//! The project most worth a bundle is the one that will not open. Every failure
//! in here is therefore recorded into the document and the run continues: a
//! project that cannot be opened produces a bundle saying so, with the machine
//! and backend sections intact. The verb exits non-zero only if it cannot write
//! the document at all.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use vcw_metadata::credentials::{ACOUSTID_KEY_VAR, CONTACT_VAR, Credentials, DISCOGS_TOKEN_VAR};
use vcw_project::{Options, Project, meta, session, validate};

/// What `vcw bundle` was asked for.
pub(crate) struct Args {
    /// The project to report on. Optional: a machine report is still useful to
    /// somebody whose trouble is that no device appears.
    pub(crate) project: Option<PathBuf>,
    /// Where to write. Default is stdout, so the verb pipes.
    pub(crate) out: Option<PathBuf>,
    /// Skip the device survey.
    ///
    /// Worth having because the survey opens every device, which takes a second
    /// or two and can be blocked by whatever else is using the sound card - and
    /// "the survey hangs" is itself a thing somebody might need to report.
    pub(crate) no_devices: bool,
    /// Report every advertised configuration family, not just a summary.
    pub(crate) all_devices: bool,
    /// Recompute every block's checksum. Minutes on a full side (§49).
    pub(crate) checksums: bool,
}

/// Builds the bundle and writes it.
pub(crate) fn run(args: Args) -> Result<()> {
    let bundle = json!({
        "bundle_version": 1,
        "vcw": program(),
        "host": host(),
        "audio": audio(args.no_devices, args.all_devices),
        "credentials": credentials(),
        "project": args.project.as_deref().map(|path| project(path, args.checksums)),
    });

    let text = serde_json::to_string_pretty(&bundle).context("render the bundle")?;
    match args.out.as_deref() {
        Some(path) => {
            std::fs::write(path, text.as_bytes())
                .with_context(|| format!("write {}", path.display()))?;
            println!("wrote {} ({} bytes)", path.display(), text.len());
        }
        None => println!("{text}"),
    }
    Ok(())
}

/// Which build of what.
fn program() -> Value {
    json!({
        "name": "vcw",
        "version": env!("CARGO_PKG_VERSION"),
        // Which build, because a timing complaint against a debug build is a
        // different conversation, and this is the field that ends it early.
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "sqlite": vcw_project::sqlite::runtime_version(),
        "schema_version": vcw_project::SCHEMA_VERSION,
        "format_version": vcw_project::FORMAT_VERSION,
    })
}

/// The machine, as portably as it can be had without a dependency.
fn host() -> Value {
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        // Both of these are Linux-only, and null elsewhere rather than absent:
        // "we did not look" and "there was nothing to see" are different, and a
        // reader of the bundle should be able to tell which happened.
        "kernel": first_line("/proc/sys/kernel/osrelease"),
        "distribution": os_release_pretty_name(),
    })
}

/// The first line of a file, trimmed, or `None` if it cannot be read.
fn first_line(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .next()
        .map(|line| line.trim().to_owned())
}

/// `PRETTY_NAME` out of `/etc/os-release`.
fn os_release_pretty_name() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"').to_owned())
}

/// The backend and what it can see.
///
/// Summarized, not dumped. The full `Snapshot` of this development machine is
/// 7.7 MB of JSON - 71 devices, most of them ALSA plugin nodes, each with every
/// advertised configuration family - and a 7.7 MB bundle is a bundle nobody
/// sends. §42 asks for the device configuration, and what diagnoses a capture
/// problem is which channel counts, rates and formats a device offers, what the
/// backend picked as its default, and what went wrong while asking. That fits in
/// a few hundred bytes a device. `--all-devices` still gets the lot, for the
/// rare occasion when the answer is hiding in a config range.
fn audio(no_devices: bool, all_devices: bool) -> Value {
    let hosts = vcw_audio::devices::available_hosts();
    if no_devices {
        return json!({ "hosts": hosts, "surveyed": false, "devices": Value::Null });
    }
    let snapshot = vcw_audio::devices::enumerate();
    let devices = if all_devices {
        serde_json::to_value(&snapshot.devices).unwrap_or(Value::Null)
    } else {
        snapshot.devices.iter().map(summarize).collect()
    };
    json!({
        "hosts": hosts,
        "surveyed": true,
        "detail": if all_devices { "full" } else { "summary" },
        "count": snapshot.devices.len(),
        "problems": snapshot.problems,
        "devices": devices,
    })
}

/// One device, in the few hundred bytes that matter.
fn summarize(device: &vcw_audio::devices::DeviceReport) -> Value {
    json!({
        "host": device.key.host(),
        "id": device.key.id(),
        "name": device.name,
        "manufacturer": device.manufacturer,
        "driver": device.driver,
        "device_type": device.device_type,
        "interface": device.interface,
        "transport": format!("{:?}", device.transport),
        "direct_hardware": device.key.is_direct_hardware(),
        "default_input": device.is_default_input,
        "default_output": device.is_default_output,
        "input": direction(&device.input),
        "output": direction(&device.output),
        // Digested, not quoted. The fingerprint itself is kilobytes for an ALSA
        // plugin node that advertises 1 to 128 channels in five formats, and all
        // it is needed for here is equality: "the device changed under us" is a
        // real capture failure, and two bundles with different digests say so in
        // sixteen characters.
        "capability_digest": digest(&device.capability_fingerprint()),
        "problems": device.problems,
    })
}

/// The channel counts a direction advertises, as a range when it is a long one.
///
/// An ALSA plugin node advertises 1 to 64 channels, and sixty-four numbers is
/// several screens of a document somebody is reading to find out why their
/// turntable clicks. The list survives while it is short enough to read, which
/// is the case that matters: a real interface offers two or four.
fn channels(counts: &[u16]) -> Value {
    json!({
        "min": counts.first(),
        "max": counts.last(),
        "distinct": counts.len(),
        "list": if counts.len() <= 16 { Some(counts) } else { None },
    })
}

/// A short, stable digest of a string.
///
/// FNV-1a, written out rather than pulled in. This is not a checksum of
/// anything that matters - the stored block checksums are CRC-32 and come from
/// `crc32fast` - it is a way to compare two device surveys at a glance, and a
/// dependency for that would be a poor trade.
fn digest(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// What a device offers in one direction, condensed.
fn direction(report: &vcw_audio::devices::DirectionReport) -> Value {
    if !report.supported {
        return json!({ "supported": false });
    }
    json!({
        "supported": true,
        "config_families": report.configs.len(),
        "channels": channels(&report.channel_counts()),
        "standard_rates": report
            .standard_rates()
            .into_iter()
            .map(vcw_types::SampleRate::hz)
            .collect::<Vec<_>>(),
        "standard_formats": report
            .standard_formats()
            .into_iter()
            .map(|format| format!("{format:?}"))
            .collect::<Vec<_>>(),
        // The backend's own default, in full: S1 found the default input on this
        // host to be the one configuration guaranteed not to be bit-perfect, so
        // it is worth every byte it costs.
        "backend_default": report.default,
    })
}

/// Which credentials are configured, and nothing about what they are.
///
/// The length is in here because "my token is not working" is very often a
/// truncated paste, and a character count settles that without the bundle
/// carrying the token. `VCW_CONTACT` gets no length: it is an email address, and
/// a length is one more thing about a person than a dropout report needs.
fn credentials() -> Value {
    let credentials = Credentials::from_env();
    json!({
        "note": "values are never included (§39); credentials come from the environment",
        DISCOGS_TOKEN_VAR: token(credentials.discogs()),
        ACOUSTID_KEY_VAR: token(credentials.acoustid()),
        CONTACT_VAR: json!({ "configured": credentials.contact().is_some() }),
    })
}

/// One credential, as a presence and a length.
fn token(token: Option<&vcw_metadata::credentials::Token>) -> Value {
    json!({
        "configured": token.is_some(),
        "characters": token.map(vcw_metadata::credentials::Token::characters),
    })
}

/// Everything the project will say about itself.
///
/// Takes the path but reports only the file name - see the module note.
fn project(path: &Path, checksums: bool) -> Value {
    let mut report = Map::new();
    report.insert("file".to_owned(), json!(file_name(path)));
    report.insert("exists".to_owned(), json!(path.exists()));
    report.insert("bytes".to_owned(), json!(byte_size(path)));
    // The sidecars, which are the first thing to look at after a crash: a `-wal`
    // still on disk means either a live writer or an unfolded log.
    report.insert(
        "wal_bytes".to_owned(),
        json!(byte_size(&sidecar(path, "wal"))),
    );
    report.insert(
        "shm_bytes".to_owned(),
        json!(byte_size(&sidecar(path, "shm"))),
    );

    // Read-only, so a bundle cannot be the thing that damages the project it was
    // called to diagnose. It does still fold in a hot log - see `recover`'s note
    // - which is why this says so rather than pretending to be inert.
    match Project::open_read_only(path) {
        Ok(project) => {
            inspect(&project, checksums, &mut report);
            if let Err(error) = project.close() {
                report.insert("close_error".to_owned(), json!(error.to_string()));
            }
        }
        Err(error) => {
            // Not a failure of the verb. This is the case the verb exists for.
            report.insert("open_error".to_owned(), json!(error.to_string()));
        }
    }
    Value::Object(report)
}

/// The file name alone, or `"?"` for a path that has none.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || "?".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A file's size, or `None` if it is not there.
fn byte_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

/// A SQLite sidecar's path: `side-a.vcw-wal`, not `side-a-wal.vcw`.
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push("-");
    name.push(suffix);
    PathBuf::from(name)
}

/// Fills in everything that needs the project open.
fn inspect(project: &Project, checksums: bool, report: &mut Map<String, Value>) {
    let conn = project.conn();

    report.insert(
        "identity".to_owned(),
        json!({
            "application_id": pragma_i64(project, "application_id").map(|id| format!("{id:#010x}")),
            "user_version": pragma_i64(project, "user_version"),
            "page_size": pragma_i64(project, "page_size"),
            "journal_mode": pragma_text(project, "journal_mode"),
            "schema_version": project.schema_version().ok(),
            "format_version": project.format_version().ok().flatten(),
        }),
    );

    // The required keys only. `meta` is also where an import parks the source's
    // tags, and those are the record's metadata, not ours to forward.
    let present: Vec<&str> = meta::REQUIRED_KEYS
        .into_iter()
        .filter(|key| matches!(meta::get(conn, key), Ok(Some(_))))
        .collect();
    let total = meta::all(conn).map(|rows| rows.len());
    report.insert(
        "meta".to_owned(),
        json!({
            "created_by": meta::get(conn, meta::CREATED_BY).ok().flatten(),
            "created_at": meta::get(conn, meta::CREATED_AT).ok().flatten(),
            "last_written_by": meta::get(conn, meta::LAST_WRITTEN_BY).ok().flatten(),
            "required_keys_present": present.len(),
            "required_keys_expected": meta::REQUIRED_KEYS.len(),
            "missing": meta::REQUIRED_KEYS
                .into_iter()
                .filter(|key| !present.contains(key))
                .collect::<Vec<_>>(),
            // Count only. What the other keys hold is the record, not the fault.
            "other_keys": total.ok().map(|n| n.saturating_sub(present.len())),
        }),
    );

    report.insert("integrity".to_owned(), integrity(project));
    report.insert("counts".to_owned(), counts(project));
    report.insert("captures".to_owned(), captures(project));

    let validated = validate(
        project,
        Options {
            verify_checksums: checksums,
        },
    );
    report.insert(
        "validate".to_owned(),
        match validated {
            Ok(found) => json!({
                "clean": found.is_clean(),
                "captures": found.captures,
                "blocks": found.blocks,
                "checksums_verified": found.checksums_verified,
                "findings": found
                    .findings
                    .iter()
                    .map(|finding| json!({ "code": finding.code, "detail": finding.detail }))
                    .collect::<Vec<_>>(),
            }),
            Err(error) => json!({ "error": error.to_string() }),
        },
    );
}

/// What SQLite itself makes of the file.
///
/// `integrity_check` sees a torn page and cannot see a corrupted sample blob -
/// a wrong byte inside a BLOB is still a valid BLOB - which is exactly the
/// division of labor between this and `--checksums`.
fn integrity(project: &Project) -> Value {
    let conn = project.conn();
    let check = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        .unwrap_or_else(|error| format!("error: {error}"));
    let foreign_keys = conn
        .prepare("PRAGMA foreign_key_check")
        .and_then(|mut stmt| {
            stmt.query_map([], |_| Ok(()))?
                .count()
                .try_into()
                .map_or_else(|_| Ok(u64::MAX), Ok)
        });
    json!({
        "integrity_check": check,
        "foreign_key_violations": foreign_keys.ok(),
    })
}

/// Row counts: enough to tell an empty project from a full one, and no content.
///
/// The table list comes out of `sqlite_master` rather than being written down
/// here. A hardcoded list drifts - the first draft of this asked for a
/// `waveform_blocks` that does not exist and reported it as `null`, which reads
/// like an empty table rather than like a bug in the bundle - and asking the
/// file also means a bundle of a project from a *newer* schema still counts the
/// tables it has never heard of.
fn counts(project: &Project) -> Value {
    let mut out = Map::new();
    let tables = project.conn().prepare(
        "SELECT name FROM sqlite_master
          WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
          ORDER BY name",
    );
    let names: Vec<String> = match tables {
        Ok(mut stmt) => stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default(),
        Err(error) => {
            return json!({ "error": error.to_string() });
        }
    };
    for table in names {
        // Interpolated, which would normally be the wrong way to build SQL. A
        // table name cannot be bound as a parameter, and these names came from
        // `sqlite_master` a moment ago rather than from anything a caller said.
        let count = project
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
                row.get::<_, i64>(0)
            })
            .ok();
        out.insert(table, json!(count));
    }
    Value::Object(out)
}

/// Every capture, with the four counters and whatever the host said went wrong.
///
/// This is §42's "capture errors", and the reason `os_report` is forwarded
/// verbatim while nothing else in the project is: it is the driver talking, not
/// the person.
fn captures(project: &Project) -> Value {
    match session::all(project.conn()) {
        Ok(records) => records
            .iter()
            .map(|record| {
                json!({
                    "capture_id": record.id,
                    "sample_rate": record.info.rate.hz(),
                    "channels": record.info.channels,
                    "storage_format": format!("{:?}", record.info.storage_format),
                    "capture_mode": record.info.capture_mode.as_str(),
                    "capture_eq": record.info.eq.as_str(),
                    "host_api": record.info.host_api,
                    "device_id": record.info.device_id,
                    "device_name": record.info.device_name,
                    "os_verified": record.info.os_verified,
                    "os_report": record.info.os_report,
                    "state": record.state.as_str(),
                    "started_at": record.started_at,
                    "finished_at": record.finished_at,
                    "frames": record.frames,
                    "seconds": record.duration_secs(),
                    "clean": record.diagnostics.is_clean(),
                    "diagnostics": {
                        "overruns": record.diagnostics.overruns,
                        "underruns": record.diagnostics.underruns,
                        "dropped_frames": record.diagnostics.dropped_frames,
                        "stream_errors": record.diagnostics.stream_errors,
                    },
                })
            })
            .collect(),
        Err(error) => json!({ "error": error.to_string() }),
    }
}

/// A pragma that answers with an integer.
fn pragma_i64(project: &Project, name: &str) -> Option<i64> {
    project
        .conn()
        .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .ok()
}

/// A pragma that answers with a word.
fn pragma_text(project: &Project, name: &str) -> Option<String> {
    project
        .conn()
        .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .ok()
}
