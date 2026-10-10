# The project API

*VCW - The Vinyl Capture Workstation. (c) 2026 Stue Hunter. MIT License - see
the header in any Rust source file for the full text.*

§49 asks for two things. The first is that the project format be openly
documented and usable by third-party tools without the GUI; that is
[`SCHEMA.md`](SCHEMA.md), and [`tools/vcw-read.py`](../tools/vcw-read.py) is the
proof that the document is enough on its own. The second is this: a Rust crate
that exposes **supported APIs for reading, validating, recovering and migrating**
project files.

That crate is `vcw-project`. This document says what "supported" means, which
calls are in and which are out, and where the working example is.

## The example is the contract

[`crates/project/examples/read_a_project.rs`](../crates/project/examples/read_a_project.rs)
uses all four surfaces and nothing else:

```text
cargo run -p vcw-project --example read_a_project -- album.vcw
```

It is in the tree rather than in this document because the gate compiles it:
`cargo clippy --workspace --all-targets` builds every example, so the day a
supported signature changes, the example stops building and this file gets
corrected instead of quietly going stale. A code block in a markdown file has no
such property.

## Reading

```rust
use vcw_project::{Project, pcm, session, meta, disc, side, track, release};

let project = Project::open_read_only("album.vcw")?;      // mode=ro
let layout = pcm::Layout::of(project.conn(), capture_id)?; // rate, channels, format, frames
let mut reader = pcm::Reader::open(project.conn(), capture_id, layout.span())?;
let filled = reader.fill(&mut buffer)?;                    // interleaved, stored format
```

* **`Project::open_read_only`** is the right entry point for anything that only
  reads. It opens with a `mode=ro` URI rather than `immutable=1`, which matters:
  `immutable` tells SQLite to ignore the `-wal`, and a project whose last blocks
  are still in the log would silently read short. It also identifies the file
  from its `application_id` before trusting a single table, so an `.aup3` handed
  to it by mistake is refused by name.
* **`Project::open`** is read-write and **migrates on open** (see below). Do not
  reach for it to read.
* **`pcm::Layout`** is what a reader needs before it can interpret a byte:
  `rate`, `channels`, `format` and `frames`, plus `frame_bytes()` and `span()`.
* **`pcm::Reader`** hands back interleaved frames in the capture's stored format,
  block by block, without ever holding a side in memory. It seeks by frame.
* **`session::all`** and **`session::load`** give the capture rows: the
  configuration, the state, the frame count and the four diagnostic counters.
* **`meta`**, **`disc`**, **`side`**, **`track`** and **`release`** read the
  record laid over the audio. A track span is **half-open**, `[start, end)`, in
  frames from the start of the side's capture.
* **`waveform`** reads the stored summary pyramid, for drawing.

The one rule that is not obvious from the signatures: **a side has no frame
extent**. Two faces of a record can share one capture, and nothing in the file
says where one face ends. Ask a track for its span; do not ask a side for its
length.

## Validating

```rust
use vcw_project::{validate, Options, integrity_check};

let report = validate(&project, Options { verify_checksums: false })?;
if !report.is_clean() {
    for finding in &report.findings { eprintln!("{} {}", finding.code, finding.detail); }
}
```

* **`validate`** runs the structural checks: tables present, required `meta`
  keys, blocks that reference a capture that is not there, blocks nothing
  references, block sizes against their declared frame counts, the boundary
  timeline, capture states, and whether a capture's blocks cover its frames.
* **`Options::verify_checksums`** additionally recomputes every block's CRC-32
  and compares it with the stored value. Off by default because it reads every
  sample byte in the project - minutes for a full-length rip - and worth it when
  recovering or when something is wrong.
* **`integrity_check`** is SQLite's own. Run both: they see different faults.
  `integrity_check` finds a torn page and **cannot** find a corrupted sample
  blob, because a wrong byte inside a BLOB is still a valid BLOB. That division
  of labor is the entire reason the `checksum` column exists.
* **`Finding::code`** is stable and meant to be matched on. `Finding::detail` is
  for a person and is not.

## Recovering

```rust
use vcw_project::{recovery, Plan};

for assessment in recovery::survey(project.conn())? {
    let would = recovery::recover(&mut project, &assessment, Plan::DryRun)?;
    println!("{} usable frames", would.frames);
}
```

* **`recovery::survey`** finds captures that never finished: the process died,
  or the device vanished, or the power went.
* **`recovery::assess`** produces the same `Assessment` for one capture: how many
  frames are usable, which blocks are stranded past them, and how far the
  persisted counters lag behind the audio.
* **`Plan::DryRun`** reports and writes nothing to the database. `Plan::Commit`
  refuses if there are stranded blocks; `Plan::Repair` removes them. The default
  in the CLI is a dry run, deliberately: a recording that survived a crash is
  worth more than the convenience of not typing `--apply`.
* **`Sidecars::inspect`** reads the `-wal` and `-shm` **before** the project is
  opened, because opening it is what makes that evidence disappear.
* A dry run is a report and not a snapshot. Opening a SQLite database replays a
  hot log and closing it folds the log in, so the first open after a crash
  consumes the log whatever flags were passed. Anyone who wants the crashed state
  kept has to copy the file and both sidecars together first.

## Migrating

```rust
use vcw_project::{migrate, MIGRATIONS};

let current = migrate::current_version(project.conn())?;
let target = migrate::target_version(MIGRATIONS);
```

* **`Project::open` applies pending migrations**, in one transaction each, and
  records every one in `schema_migrations`. There is no separate "please
  migrate" call in the normal path, because a project that is open for writing
  and half-migrated is the state nobody wants to have to reason about.
* **`migrate::current_version`** and **`migrate::target_version`** let a reader
  ask without acting. `migrate::pending` lists what would run.
* **`migrate::apply`** is public for the recovery and tooling paths. It is
  ascending, idempotent, and rolls back the failing step rather than leaving it
  half-applied.
* There is no downgrade. A project written by a newer VCW is refused by an older
  one rather than read optimistically: `Project::open_read_only` accepts a
  **v1** project, so a reader that needs v2 has to ask the version rather than
  assume the open succeeding means the schema is current.

## What is not supported

Public because the workspace needs it, not because a third party should build on
it:

* **`persistence::Writer`** and **`Session`** are the capture write path. They
  assume a single writer, a specific commit cadence and the WAL settings
  `Project::create` applies. Use the CLI or the application to make a capture.
* **`schema`** exposes the DDL strings so `doc.rs` can generate `SCHEMA.md` from
  them. Read the generated document, not the constants.
* **`doc`** is the generator itself.
* **`Connection`** is re-exported because the surface above already speaks it -
  `pcm::Reader::open` takes one - not as an invitation to run statements of your
  own against a project. If you are writing SQL, write it against a copy, or use
  `SCHEMA.md` and your own connection as `tools/vcw-read.py` does.

## Stability

Pre-1.0, so the Rust surface can change with a release note. Two things are held
steadier than the code:

* **The file format.** `format_version` in `meta` is 1. A reader should check it,
  and `SCHEMA.md` documents what changes would raise it.
* **`Finding::code`** values, which exist to be matched on by tests and by a UI.

Anything else may move. Pin a version if that matters, and run the example
against a new one: it is the cheapest check that the four surfaces still work
the way this document says they do.
