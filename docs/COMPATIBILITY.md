# Backward compatibility

*VCW - The Vinyl Capture Workstation. (c) 2026 Stue Hunter. MIT License - see
the header in any Rust source file for the full text.*

§16 asks for this document, and [ADR-0009](adr/0009-leaving-alpha.md) makes it a
condition of dropping the `-alpha` suffix: a version number that claims stability
has to say what it is promising about the files already on somebody's disk. This
is that promise, and it is about the `.vcw` file alone. The Rust crate's own
stability is a separate and weaker thing, in
[PROJECT-API.md](PROJECT-API.md#stability).

None of it is new machinery. Every rule below is something the code already does,
and `crates/project/tests/compatibility.rs` holds the code to this document
rather than to a second copy of it.

## The two version numbers

|                  | Where it lives                      | Now | What it is about                            |
|------------------|-------------------------------------|-----|---------------------------------------------|
| `schema_version` | SQLite's `user_version`             | 7   | the shape: which tables and columns exist   |
| `format_version` | the `format_version` row in `meta`  | 1   | the meaning: what the values in them are    |

They move independently, which is the whole of §16's point in asking for both.

**`schema_version` answers "does this build know every table and column this file
has?"** It goes up by one per migration. A file below the current number is
incomplete rather than wrong: every column it does have means today what it meant
when it was written, which is why a read-only open accepts it as it stands and why
opening it for writing upgrades it in place. [SCHEMA.md](SCHEMA.md) is generated
from the migrations and describes the schema a reader will meet; the
`schema_migrations` table in any project names the versions that built that
particular file, one row each.

**`format_version` answers "does this build read those columns the way the build
that wrote them meant them?"** It goes up when an existing column changes unit,
encoding or invariant - the change that makes an older reader *wrong* rather than
merely incomplete. It has never moved. A third-party reader checks this one first,
because a file whose `schema_version` is familiar can still be one it should not
interpret.

## What a 1.0 project will do in 1.x

* **Every 1.x build opens every project any 1.x build wrote**, at or below its own
  schema version, and brings it current on the first write.
* **`format_version` stays 1 for the whole of the 1.x line.** No minor release will
  change what a column a 1.0 build wrote means. A change that would raise the
  number waits for 2.0, and this document moves with it.
* **No migration rewrites captured audio.** Migrations add tables and columns and,
  once so far, carry one release-level setting into its replacement. The
  `sampleblocks` rows a capture wrote are never updated by an upgrade, in keeping
  with the never-update-a-block rule the format inherited from AUP4.
* **An upgrade is recorded.** Each migration is one transaction, and
  `schema_migrations` keeps the version, the description, the time and the build
  that applied it. A half-applied upgrade is therefore detectable, and a rolled
  back one leaves nothing.

## Which schema versions stay readable

Every version from 1 to this build's own, and that does not shrink within a major
line. Support for schema version 1 - a capture-only project from before there was
a vinyl data model - is as current as support for 7.

Dropping one would need a major version, a release note naming the version, and
the last build of the previous line still being available as the route through:
old project, old build, current file. Nothing plans to drop any.

## A project from the future is refused

A file whose `schema_version` is above this build's is refused outright, for
reading as well as for writing, with a message naming both numbers and telling you
to upgrade VCW. §16 allows a newer VCW to upgrade an older project; it says nothing
about the other direction, and a partial read of a schema we do not know is how
data gets lost quietly.

The same door checks `application_id`, which is `"VCW\0"`, before it looks at any
version at all. An Audacity project is identified as one and sent to the importer
rather than opened.

## Upgrading is one way

There is no downgrade and there will not be one. `vcw migrate` says so in the two
lines before it writes:

```text
This is one way. An older VCW will refuse the upgraded file.
Copy it first if you need to go back.
```

A copy is the whole of the mechanism. VCW does not make one for you, because a side
is well over a gigabyte and the box most likely to be short of the room is the
headless one this verb exists for.

## What this document does not promise

* **The Rust API.** Pre-1.0 it can change with a release note; see
  [PROJECT-API.md](PROJECT-API.md#stability).
* **The CLI's `--json` shapes and the command and event contract.** They are
  versioned by the release notes, not by this file.
* **That a current `schema_version` means a complete file.** The number records
  which migrations ran, not what they produced; something outside VCW can drop an
  index. `vcw migrate` ends by asking the project's own dictionary and reporting
  what it finds, and `vcw recover` runs the fuller validation.

## Checking it yourself

```text
vcw migrate album.vcw --dry-run     # what would run, and nothing written
vcw migrate album.vcw --json        # the same, machine readable
```

Both report the version found and the version this build targets.
[`tools/vcw-read.py`](../tools/vcw-read.py) reads `meta` and the schema version
out of a project with no VCW code at all, which is the form a third-party reader's
own check takes.
