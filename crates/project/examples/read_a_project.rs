/*
 *  read_a_project.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §49's supported API, used from outside: read, validate, recover, migrate.
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
//! §49's supported API, used from outside: read, validate, recover, migrate.
//!
//! §49 says the project crate shall expose supported APIs for reading,
//! validating, recovering and migrating project files. This program uses all
//! four and nothing else, which is what makes it worth having in the tree: it
//! compiles in the gate's `clippy --all-targets` leg, so the day the supported
//! surface changes shape, this stops building and `docs/PROJECT-API.md` gets
//! corrected instead of quietly going stale.
//!
//! Deliberately read-only. Every call here either opens with `mode=ro` or asks
//! a question, so it is safe to point at a project somebody cares about - and
//! a first example that could damage the file it was demonstrating on would be
//! a poor introduction.
//!
//! ```text
//! cargo run -p vcw-project --example read_a_project -- side-a.vcw
//! ```

use std::process::ExitCode;

use vcw_project::{
    Options, Plan, Project, disc, meta, migrate, pcm, recovery, release, side, track, validate,
};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: read_a_project <project.vcw>");
        return ExitCode::FAILURE;
    };
    match report(&path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{path}: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Everything the four supported surfaces will say about one project.
fn report(path: &str) -> vcw_project::Result<()> {
    // READING. `open_read_only` identifies the file from its `application_id`
    // before it trusts a single table, so a `.aup3` handed to it by mistake is
    // refused by name rather than half-read.
    let project = Project::open_read_only(path)?;
    println!("{path}");
    println!("  access          {:?}", project.access());
    println!("  schema version  {}", project.schema_version()?);
    println!(
        "  format version  {}",
        project
            .format_version()?
            .map_or_else(|| "absent".to_owned(), |version| version.to_string())
    );
    println!(
        "  created by      {}",
        meta::get(project.conn(), meta::CREATED_BY)?.unwrap_or_default()
    );

    // MIGRATING. Read-only never migrates, so this reports rather than acts:
    // `Project::open` is what applies them, and it does so on every open.
    let current = migrate::current_version(project.conn())?;
    let target = migrate::target_version(vcw_project::MIGRATIONS);
    if current < target {
        println!("  migrations      {current} -> {target} would be applied by Project::open");
    } else {
        println!("  migrations      up to date at {current}");
    }

    // READING, the part that is the point: the audio and the record over it.
    for record in vcw_project::session::all(project.conn())? {
        let layout = pcm::Layout::of(project.conn(), record.id)?;
        println!(
            "  capture {:<3}     {} Hz, {} ch, {:?}, {} frames ({:.3} s), {}",
            record.id,
            layout.rate.hz(),
            layout.channels,
            layout.format,
            layout.frames,
            layout.seconds(),
            record.state.as_str()
        );

        // One chunk, to show what a reader actually gets: interleaved samples in
        // the stored format, which `Layout` is what describes.
        let mut reader = pcm::Reader::open(project.conn(), record.id, layout.span())?;
        let mut frames = vec![0_u8; layout.frame_bytes() * 1024];
        let filled = reader.fill(&mut frames)?;
        println!(
            "                  first read: {filled} bytes, {} frames",
            filled / layout.frame_bytes().max(1)
        );
    }

    let found = release::load(project.conn())?;
    if let Some(found) = found.filter(|found| !found.is_empty()) {
        println!("  release         {} - {}", found.album_artist, found.album);
    }
    for disc in disc::list(project.conn())? {
        for face in disc.sides() {
            let tracks = track::tracks_of(project.conn(), face.id)?;
            println!(
                "  side {}          {} track(s), capture {}",
                face.side,
                tracks.len(),
                face.capture
                    .map_or_else(|| "none".to_owned(), |id| id.to_string())
            );
            for one in tracks {
                // A span is half-open: `[start, end)`, in frames from the start
                // of the side's capture.
                println!(
                    "    {:>2}. {:<28} {}..{}",
                    one.number, one.title, one.start, one.end
                );
            }
        }
    }
    let _ = side::list(project.conn())?;

    // VALIDATING. Structure only by default; `verify_checksums` reads every
    // sample byte and takes minutes on a full side.
    let checked = validate(
        &project,
        Options {
            verify_checksums: false,
        },
    )?;
    println!(
        "  validate        {} capture(s), {} block(s), {}",
        checked.captures,
        checked.blocks,
        if checked.is_clean() {
            "clean".to_owned()
        } else {
            format!("{} finding(s)", checked.findings.len())
        }
    );
    for finding in &checked.findings {
        println!("    {}  {}", finding.code, finding.detail);
    }

    // RECOVERING. A survey is a question, and `Plan::DryRun` keeps it one: it
    // reports what recovery would do and writes nothing to the database.
    let unfinished = recovery::survey(project.conn())?;
    if unfinished.is_empty() {
        println!("  recovery        nothing unfinished");
    }
    for assessment in &unfinished {
        println!(
            "  recovery        capture {} is unfinished: {} usable frame(s), {} stranded block(s)",
            assessment.capture_id,
            assessment.usable_frames,
            assessment.surplus.len()
        );
        // Needs a writable project to apply, which this program does not have -
        // and that is the right default for an example.
        let _ = Plan::DryRun;
    }

    project.close()
}
