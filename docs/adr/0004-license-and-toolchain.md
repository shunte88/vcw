# ADR-0004: License posture and toolchain floor

**Status:** Accepted 2026-09-25.
**Decisions:** D7, D10 · **Requirements:** §39, §49 · **Work package:** WP-01

## Context

VCW ships statically linked Rust binaries, so every dependency's license terms travel
into the artifact. Two of the planned Phase 2 dependencies - `chromaprint-next` for
fingerprinting (§25) and the MP3/Ogg encoders (D5) - carry LGPL relink obligations.
VRipr already solved this once and its `THIRD-PARTY-NOTICES.md` was correct; the work
here is porting the structure while stating VCW's *actual* position rather than
inheriting VRipr's dependency list.

Separately, D10 fixes the toolchain floor, and a floor nobody tests is not a floor.

## Decision

### License posture

- **VCW's own code is MIT.** The `LICENSE` file, and `license = "MIT"` in the
  workspace manifest.
- **`deny.toml` is the enforcement point**, not a review habit. It carries an allowlist
  of permissive licenses; anything outside it fails CI. `cargo deny check` runs on
  every push.
- **`THIRD-PARTY-NOTICES.md` is written ahead of the obligation**, describing what is
  linked today (permissive only) and what arrives at Phase 2, with the relink
  instructions already in place. `LICENSE-LGPL-2.1` is in the repository now.
- **The LGPL exception in `deny.toml` stays commented out until the dependency
  actually lands.** An allowance carried ahead of its dependency is an allowance
  nobody reviews.
- **MP3 and Ogg are candidates for optional cargo features**, so a default build can
  carry no LGPL obligation at all. Decided at WP-14; whichever way it goes, the
  notices record the outcome.

### Dependency of record

Depend on the **published crate**, never on a local checkout, and never via a
`[patch.crates-io]` path entry.

This is a scar, not a preference. VCW carried a patched CPAL fork for two defects; both
were fixed upstream in 0.18.2, and the patch entry had quietly hidden how stale the pin
had become. The same rule was then applied to `chromaprint-next`, whose local checkout
sits two SIMD commits ahead of the release - both were verified fingerprint-neutral
*out of tree* rather than patched in.

Fork only for a defect that is not already fixed upstream and that we cannot get
upstreamed in time. `shunte88/cpal` remains the fork of record for CPAL and is
currently, and preferably, unused.

### Toolchain floor

- Rust **edition 2024**, MSRV **1.90**, declared in `[workspace.package]` and enforced
  by a CI job pinned to 1.90. `rust-toolchain.toml` stays on `stable` so day-to-day
  work gets current diagnostics; the pinned job is what makes the promise real.
- **Node 22 LTS** and **pnpm** for the frontend, pinned in `.nvmrc` at the repository
  root. It governs the S3 bench frontend today and `app/ui` when it lands at WP-16.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` gate every push.
  Workspace lints add `missing_docs`, `unreachable_pub` and `unused_qualifications`.

### Credentials

§39 forbids credentials in project files, and this extends to the repository.
Discogs tokens and the AcoustID API key come from the OS keychain or the environment.
Nothing in `crates/`, `app/` or the fixtures may contain one, and no fixture generated
from a real session may carry one either.

## Consequences

- A dependency bump that changes the license picture fails CI rather than reaching a
  release.
- Adding an LGPL dependency is a deliberate two-step: uncomment the exception, update
  the notices. Neither happens by accident.
- The MSRV job will fail the first time a dependency raises its own floor past 1.90.
  That is the job working: the floor moves as a decision, recorded here.
- `cargo about` can regenerate a full per-crate inventory when a release needs one.
  `cargo deny` is the cheap daily check.
