# ADR-0007: The Rust/TypeScript contract, and where the shell lives (D9)

**Status:** Accepted 2026-09-27.
**Requirements:** §2, §5, §35 · **Work package:** WP-15 · **Decision:** D9.

## Context

§35 asks for a command and event surface between the core and the interface. §2 says the
application's behaviour belongs to the application and not to the view. WP-16's exit
criterion turns that into a review gate - *no business logic in TS* - and a review gate is
the weakest kind of rule there is, because it fails slowly. Nobody adds a business rule to
a React component in the commit that introduces the component. It arrives in the third
one, as a unit conversion that was easier to do where the number was needed.

There are also two consumers of that surface, not one. The shell is the obvious one; §4.5
requires the whole workflow to be drivable without a UI, and `vcw --json` already prints
the same facts. Two consumers means any decision left to the caller gets made twice, and
the second answer is the one nobody tests.

The third pressure is drift. A hand-written TypeScript interface is correct the day it is
written. The failure mode is not a wrong interface; it is a *stale* one that still
compiles, describing a field that no longer arrives.

And the fourth is the exit criterion for WP-15 itself: *core crates have zero Tauri
dependency, enforced in CI*. That job existed before the shell did - `core-is-ui-free`
walks `cargo tree --workspace` at the repository root - and it had to survive the arrival
of a crate that does depend on Tauri.

## Decision

**Four parts, and the fourth is the one that makes the other three enforceable.**

1. **The surface is a core crate.** `vcw-contract` holds §35's events, commands and view
   models, depends on `vcw-core`, `vcw-project`, `vcw-audio`, `vcw-types` and serde, and
   on nothing from Tauri. Every unit is resolved on the Rust side: dBFS rather than
   amplitudes, seconds beside frames, a side letter rather than a side index, `A3` rather
   than a position index. The shell is left holding glue, and `vcw --json` can answer with
   the same types.

2. **The TypeScript is generated, committed, and checked.** One file,
   `app/ui/src/bindings/vcw.d.ts`, assembled by `vcw_contract::bindings::typescript()`
   from `ts-rs` declarations in a fixed order. A test compares it against the committed
   copy and reports the first differing line; `VCW_BLESS=1` rewrites it. CI runs that test
   and then `git diff --exit-code` over the file, so a regenerated-but-uncommitted
   contract fails the build.

3. **64-bit integers cross as `number`, not `bigint`.** `serde_json` writes a frame count
   as a JSON number and `JSON.parse` returns a double, so nothing on this boundary is ever
   a `bigint` at run time, and a frontend typed that way cannot add two frame counts
   without a cast that lies in the other direction. The accepted ceiling is 2^53 frames,
   which at 192 kHz is about a billion years.

4. **The shell is a cargo workspace of its own.** `app/` is excluded from the root
   workspace and has its own `Cargo.toml` with `crates/*` as path dependencies, which is
   what keeps `core-is-ui-free` a statement about structure rather than about discipline.

## Why `ts-rs` rather than `specta`

Both were named in the plan. `ts-rs` won on the fourth decision above: `specta`'s value is
its ecosystem, and the Tauri half of that ecosystem is `tauri-specta`, which generates the
bindings *from the command definitions*. That is a better developer experience and it
requires the crate declaring the commands to depend on Tauri - which is precisely what the
exit criterion forbids of a core crate. Taking it would have meant either moving the
contract into the shell, where `vcw --json` cannot reach it, or accepting a Tauri
dependency in `crates/`.

`ts-rs` reads serde's own attributes, so `#[serde(rename_all)]` and `#[serde(tag)]` are
honoured by the thing that generates the types rather than mirrored by hand, and it has no
run-time component at all: the declarations are produced by a test.

## What this rules out

- **Hand-written TypeScript for anything on the boundary.** The generated file is a
  `.d.ts` and the drift check is a gate leg.
- **A second unit conversion in the view.** If a view needs a number VCW has not already
  computed, the fix is a field on a view model, not a helper in `src/`.
- **Samples over the IPC.** `no_pcm_crosses_the_boundary` walks the generated field names
  and fails on typed-array types, which is S3's measurement turned into a rule: the
  boundary is free, the main-thread draw is the cost, and the frontend is handed the
  summary rows WP-09 already writes.
- **A convenient allow-list in `core-is-ui-free`.** With the shell outside the workspace
  there is no legitimate reason for `tauri`, `wry`, `tao` or `webkit2gtk` to appear in the
  root tree, so the job needs no exceptions and a new one would be a red flag rather than
  a maintenance chore.
- **Adding the shell to the four-target matrix by accident.** WebKitGTK is a Linux system
  library; the product's own jobs do not install it.

## What it costs

Root `fmt`, `clippy`, `test` and `doc` do not reach `app/`. That is paid for explicitly:
the gate has four more legs (`appfmt`, `appclippy`, `apptest`, and the frontend's
`uicheck`) and CI has two more jobs (`shell` and `bindings-are-current`). `app/Cargo.toml`
repeats the root's `[workspace.lints]` table verbatim, because a rule that fired in one
workspace and not the other would make moving code between them an argument about lints.
The duplication is deliberate and is the kind that should be noticed when it diverges.

## Evidence

- **The drift check works in the direction that matters.** Regenerating after a clippy fix
  in `bindings.rs` produced a byte-identical file, and adding a view model produced a diff
  confined to one block.
- **`pnpm check` passed first time** against the generated types, with an event-log
  function switching exhaustively over all seventeen event kinds. That is the payoff
  stated as a measurement rather than as an intention: a new event kind is a compile error
  in the frontend, not a silent gap in the log.
- **`bigint` was not a hypothetical.** It is `ts-rs`'s default, and the first generated
  file typed every frame count that way.
- **`custom-protocol` is on by default** because Tauri decides whether it is a development
  build from that feature and not from the cargo profile. S3 lost an afternoon to a
  release binary that loaded `devUrl` and opened blank.

## Amends ADR-0003

[ADR-0003](0003-workspace-layout.md) describes one product workspace of ten crates beside
the spikes' own. This record adds the eleventh crate, `vcw-contract`, and a third
workspace at `app/`. The layering rule in ADR-0003 is unchanged and `vcw-contract` obeys
it: it sits above `vcw-core` and below nothing.
