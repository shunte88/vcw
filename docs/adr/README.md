# Architecture decision records

§49 promises the project format and its design are openly documented. These records
are how that promise stays honest after the fact: each one states what was decided,
what it rules out, and - the part that matters a year later - what evidence it rests
on.

The table in [`PROJECT_PLAN.md`](../../PROJECT_PLAN.md) §3 is the index of decisions
and their deadlines. An ADR is written when a decision *locks*, not when it is first
proposed, so the plan may name decisions that have no record here yet.

| ADR | Decision | Status |
|-----|----------|--------|
| [0001](0001-project-format.md) | Native project format, extension, and the degree of Audacity compatibility (D1) | Accepted 2026-09-22, validated 2026-09-24 |
| [0002](0002-sqlite-binding.md) | SQLite binding (D2) | Accepted 2026-09-25 |
| [0003](0003-workspace-layout.md) | Cargo workspace layout and crate naming | Accepted 2026-09-25 |
| [0004](0004-licence-and-toolchain.md) | Licence posture and toolchain floor (D7, D10) | Accepted 2026-09-25 |
| [0005](0005-concurrency-model.md) | Concurrency model: threads, message passing, and where Tokio belongs (D8) | Accepted 2026-09-25 |
| [0006](0006-playback-rate-policy.md) | Playback rate and format policy: no resampler, lossless conversion only | Accepted 2026-09-26 |
| [0007](0007-rust-typescript-contract.md) | The Rust/TypeScript contract, and where the shell lives (D9) | Accepted 2026-09-27, amends 0003 |

A superseded record is never deleted or rewritten. It is marked superseded and links
forward, because the reasoning that turned out to be wrong is the most useful thing in
the file - D6 and the S5 sample-rate reversal are both cases where our own earlier
conclusion was the thing that needed correcting.
