# ADR-0009: What leaving alpha means

**Status:** Accepted 2026-10-22.
**Requirements:** §16 · **Work package:** 0.4.

## Context

Every release this project has ever cut carries `-alpha`, from `v0.1.1-alpha` on
2026-10-06 to `v0.4.0-alpha` on 2026-10-20. Nothing anywhere says what would make one
not. The suffix has been copied forward release after release because the previous one
had it, which is a habit rather than a decision, and a habit cannot be met.

It is not a free habit. GitHub's `releases/latest` excludes pre-releases, so the whole
0.x line answers 404 to the obvious endpoint and the update check had to be written
against the list endpoint instead. More to the point, `-alpha` is a sentence spoken to
somebody standing over a turntable with a record that was pressed once: it says *do not
trust this with anything you cannot replace*. VCW either means that or it does not, and
right now nobody has decided which.

**What is missing is not product.** All five milestones are met, M1 *it records* through
M5 *it has a face*, and so is G3. Phase 3 is part built and the two largest pieces left
in it, the plugin architecture and Android, are already deferred to 0.5+ on their own
merits. A reasonable person could read the feature list and call it finished.

What is thin is **evidence**, and every thin place is already written down in
`PROJECT_PLAN.md` where it has quietly stopped being read. Tier 1 is declared as
"verified on hardware: Linux x86_64, Linux aarch64 (Pi 5), Windows x86_64" and the soak
evidence the repository actually holds is x86_64 only, with the other two still listed
as outstanding at WP-05 from September. R14 carries likelihood **H** and its own
mitigation line reads "Recruit a macOS tester from the OSS community before 0.1, or mark
macOS experimental". §16 requires a backward compatibility policy before the first
stable release and there is not one.

That is the whole gap, and it is a short list.

## Decision

**The suffix comes off at 1.0.0, when five conditions are met, and not before.**

There is no beta. A third label would need criteria of its own, and the absence of
criteria is the thing this record exists to fix. `0.5.0` with no suffix is not available
either: a version that makes no stability claim out loud still makes one by dropping the
word, and §16's precondition is about the first *stable* release, not about the first
release that stops apologizing. The compatibility promise and the version number that
carries it are one event.

The five conditions, three of which block and two of which are judgments made in the
open:

**C1. The §16 compatibility policy is written, and `vcw migrate` agrees with it.**
What a `.vcw` written by 1.0 is guaranteed to do in 1.x; which schema versions the
reader will keep accepting and for how long; what `FORMAT_VERSION` promises as distinct
from `schema_version`. The policy is a description of machinery that already exists, so
this is writing rather than engineering. It is checkable at both ends: the real v1, v2
and v4 projects already on disk are the fixtures, and a test can require `migrate` to
accept every version the policy names and refuse every version it does not.

**C2. A real-time soak on each platform Tier 1 claims.** The harness exists and the
x86_64 run is the reference: 90 minutes at 24/192, four counters at zero, WAL bounded,
and every byte recomputed from its own stored frame index. The Pi 5 on aarch64 and the
Windows rig each need one. Not a CI run: `VCW_SHARED=1` exists precisely because a
hosted runner cannot make a timing claim.

**C3. macOS is honest from inside the product, not only in the README.** Nobody reads a
README out of a `.dmg`. Either a macOS device verification exists, or the macOS build
says what it is: `vcw doctor` and the window's About report that this platform is built
and unit tested but has never recorded from a converter here. R14's second branch,
taken visibly.

**C4. Every known test failure has a cause.** One is open: a single undiagnosed failure
of `the_four_audition_targets_play_what_they_name` on `test (windows-x86_64)` at
`3df2507`, which passed on the next commit. A green run does not clear a flake. The
condition is a root cause and a fix, or a written argument for why it is the weather and
not the code, to the standard the project already applies to a red CI leg.

**C5. One complete transfer by somebody who is not the author.** A record recorded,
split, tagged and exported by another person, with the project file surviving the trip.
This is the only condition that cannot be met by working harder, and it is the most
valuable: the first outside user produced four real defects in one CLI session, and no
recording made by anybody else has ever been through the whole chain. The exhibit that
would have proved it was deleted before it was sent.

## What this rules out

- **A beta.** See above. A label with no criteria is the problem, not a step toward
  solving it.
- **Gating on Phase 3.** Click repair, the archivist transfer log and the ONNX revival
  are additions. A 1.0 that waits for every good idea never ships, and nothing in that
  list is a hole in what 0.4 already claims to do.
- **Counting CI as hardware verification.** It builds four platforms and runs the tests
  that need no device. That is worth a great deal and it is not a converter.
- **Dropping the suffix because the feature list looks finished.** It looks finished
  now. That is what prompted the question.
- **Dropping it on a date.** Every condition above is a thing that happens or does not.

## Where each condition stands, 2026-10-22

Recorded so that a year from now the distance is visible rather than remembered.

| | Condition | State |
|---|---|---|
| **C1** | Compatibility policy | **Written**, same day: [docs/COMPATIBILITY.md](../COMPATIBILITY.md), with `crates/project/tests/compatibility.rs` holding the code to it - every schema version from 1 to the current one built out of a prefix of `MIGRATIONS`, opened read-only unchanged, upgraded, and checked for drift against a new file; a version above this build's refused for reading as well as writing; no migration touching captured audio; and the two numbers the document prints read back out of it. All four proven able to fail |
| **C2** | Tier 1 soaks | **One of three.** x86_64 is the reference run, 90 min at 24/192, rtf 1.00001, worst commit 17.0 ms against a 250 ms budget, 6.2 GB verified byte for byte. Pi 5 and Windows unrun |
| **C3** | macOS honesty | **README only.** Tier 2 is stated there and nowhere the user of a `.dmg` will meet it |
| **C4** | Known failures | **One open**, undiagnosed, now instrumented to print both streams so the next occurrence says why |
| **C5** | An outside transfer | **None.** One outside user, CLI only, four defects found and fixed; their project was deleted before it could be sent |

## What is deliberately not a gate test

C1, C2 and C4 can each fail on purpose and should be wired that way when they are met.
**C3 and C5 cannot**, and no test will be written that pretends otherwise. A check that
a sentence appears in `doctor`'s output tests a string, not the honesty it stands for,
and nothing in a repository can assert that a stranger recorded a record. This project's
rule is that a gate must refuse what it cannot measure; the corollary is that it must
not claim to measure what it cannot, and two of five conditions here are judgments the
author makes in public instead.
