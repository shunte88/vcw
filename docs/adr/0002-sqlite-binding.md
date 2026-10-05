# ADR-0002: SQLite binding

**Status:** Accepted 2026-09-25.
**Decision:** D2 · **Requirements:** §13, §14, §15, §36 · **Evidence:** S2

## Context

The project file is SQLite (ADR-0001) and audio is written into it *during capture*,
from a dedicated writer thread fed by a lock-free ring from the CPAL callback (§10,
§13). The binding choice therefore decides more than ergonomics: it decides whether an
async runtime exists anywhere near the capture path.

The realistic options in Rust are `rusqlite` (synchronous, a thin safe wrapper over
the C API) and `sqlx` (async-first, compile-time-checked queries, connection pooling).

## Decision

**`rusqlite` with the `bundled` feature.**

Synchronous, because the writer thread is a dedicated OS thread with a deadline, not a
task competing for an executor. D8 confines tokio to network and export I/O for the
same reason, and a binding that requires a runtime to issue a statement would smuggle
one in through the back door.

Bundled, because the SQLite build becomes a property of the binary rather than of the
machine. §15 makes recovery a correctness requirement, and recovery behavior depends
on WAL semantics, `synchronous` handling and journal-mode details that vary across the
SQLite versions Linux distributions ship. A recovery test that passes on the CI runner
and fails on a Raspberry Pi because the OS shipped an older SQLite is not a test.

## Why not `sqlx`

Its central features are the ones we cannot use. Async is wrong on the writer thread.
Connection pooling is wrong for a single-file project opened by a single process.
Compile-time query verification needs a live database at build time, which a schema we
create and migrate at runtime does not provide cleanly. What remains after removing
those is a slower `rusqlite` with a runtime dependency.

## Evidence this rests on

S2 ran the capture path against `rusqlite`/bundled for 90 minutes at 24/192 with
`synchronous=FULL`, and the headline result was that **throughput is a non-issue**:
the transaction cost at the rates §8 demands is far below the budget, which is why D3
spends the budget on recovery granularity (250 ms blocks, batch 1) instead of on
batching for speed.

S2 also established the recovery floor - loss is commit granularity plus the driver
buffer, rounded to a block boundary, and ring size does not affect it. That floor is a
property of how the database commits, so pinning the SQLite build pins the floor.

## Consequences

- The `bundled` feature compiles SQLite from source, so every CI target needs a C
  toolchain. All four Tier 1 runners have one; the aarch64 Linux job runs natively
  rather than cross-compiling partly for this reason.
- SQLite's own license is public domain and `rusqlite` is MIT, so the choice adds no
  obligation to `THIRD-PARTY-NOTICES.md`.
- `vcw-project` is the only crate that links it. Nothing else in the tree may open the
  project file directly - the schema is an invariant, not a shared resource.
- Open read paths with `mode=ro`, never `immutable=1`. S5 hit this: `immutable=1` makes
  SQLite ignore the `-wal` sidecar and silently serve a stale database, which for a
  project being recovered is exactly the wrong answer.
