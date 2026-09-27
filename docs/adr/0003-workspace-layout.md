# ADR-0003: Cargo workspace layout and crate naming

**Status:** Accepted 2026-09-25. Amended 2026-09-27 by
[ADR-0007](0007-rust-typescript-contract.md), which adds an eleventh crate and a third
workspace. The layering rule here is unchanged.
**Requirements:** §2, §6, §4.5 · **Work package:** WP-01

## Context

§6 proposes a workspace as a two-level tree - `audio/{capture, playback, devices,
buffers}`, `signal/{meter, waveform, silence, spectral, hmm}`, and so on, about
thirty-five leaves in total. It is labelled "proposed", and it is a description of
*structure*, not a statement about crate count. WP-01 has to turn it into actual
manifests and settle the naming, which `docs/STATUS.md` had recorded as open.

## Decision

**One crate per §6 top-level group. §6's leaves become modules inside it.**

```
crates/
    types/        vcw-types        format, rate
    audio/        vcw-audio        capture, playback, devices, buffers
    signal/       vcw-signal       meter, waveform, silence, spectral, hmm
    fingerprint/  vcw-fingerprint  chromaprint, acoustid
    identify/     vcw-identify     evidence, resolver, candidate, confidence
    metadata/     vcw-metadata     discogs, musicbrainz, genres, artwork
    project/      vcw-project      sqlite, session, disc, side, track,
                                   persistence, recovery
    export/       vcw-export       splitter, encoder, tagging
    core/         vcw-core         engine, commands, events, state
    cli/          vcw-cli          the `vcw` binary
app/                               WP-15: src-tauri/ and ui/
```

Crates are named `vcw-*`; the binary is `vcw`. Directory names drop the prefix, so the
tree reads as §6 writes it.

## Why not one crate per leaf

Thirty-five crates buys thirty-five manifests, thirty-five version bumps and a much
wider compile graph, in exchange for boundaries that are not the ones we need
enforced. The boundaries that actually matter are between the *groups* - the core must
not know about the UI (§2), analysis must not depend on device access, and only one
crate may open the project database. Those are all group-level, and the module system
enforces cohesion within a group perfectly well.

Splitting further is cheap to do later and expensive to undo. A leaf that grows its own
dependency set - `signal/hmm` if the ONNX path returns - can be promoted to a crate
when there is a reason, and the module path barely changes.

## `vcw-types`, which §6 does not list

Added deliberately. Sample formats (§8), capture modes (§9) and sample rates are
spoken by nearly every crate. Without a shared leaf they would have to live in
`vcw-audio`, which links CPAL - and then `vcw-signal` analysing a buffer of samples, or
`vcw-export` writing a WAV header, would pull ALSA into its dependency closure for the
sake of an enum.

`vcw-types` depends on `serde` and `thiserror` and nothing else. The test that proves
the point: the detector port's A/B harness (WP-11) has to run against the labelled
corpus on a machine with no audio stack at all.

## Dependency direction

```
types <- audio, signal, project, fingerprint, metadata
                                  identify <- fingerprint, metadata
                                  export   <- project
core  <- everything above
cli   <- core
app/src-tauri <- core
```

Acyclic, and pointed the one way that matters: nothing below `core` knows what is above
it. CI asserts the specific case §2 cares about - no crate under `crates/` may have
`tauri`, `wry`, `tao` or `webkit2gtk` anywhere in its dependency graph.

## The spikes are a separate workspace

`spikes/` has its own virtual workspace and is `exclude`d from the root. The Phase 0
spikes are finished evidence, not living code: they should not appear in the product's
four-target CI matrix, and `tauri-ipc-bench` alone would drag WebKitGTK into every
platform's build for nothing. A Linux-only CI job still compiles them, so the numbers
in `docs/spikes/` stay reproducible rather than rotting quietly.

## Consequences

- Module files exist ahead of their implementations, each documenting its scope, its
  requirement sections and the work package that fills it. `missing_docs` is a warning
  the CI treats as an error, so a module cannot be added without saying what it is for.
- `vcw-cli` declares only what it links. `vcw-core` joins its dependencies at WP-07,
  when there is an engine to drive, rather than sitting unused in the manifest.
- `app/` does not exist yet. It arrives with the Tauri shell at WP-15, and the CI rule
  above is what keeps the boundary from eroding once it does. **It did arrive, on
  2026-09-27, as a workspace of its own** rather than as a member of this one - which is
  what keeps the CI rule a statement about structure instead of an allow-list. See
  [ADR-0007](0007-rust-typescript-contract.md).
