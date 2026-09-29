# Audacity fixtures

Three Audacity projects, small enough to commit, whose document bytes Audacity
wrote. They are not files anyone typed: each one was made by deleting from one
of the user's real rips with

```sh
cargo run -p vcw-import --example make-fixture -- <source> <destination>
```

| fixture | source | what it is for |
|---|---|---|
| `clips.aup3` | `simples_test.aup3` | 19 clips a channel, shared blocks, int24, 4 KiB pages |
| `clips.aup4` | `simples_test.aup4` | the AUP4 delta: `waveblock/@length`, a `0x10` blob, `project_history` |
| `rate-trap.aup3` | `Gasp_Stardonas (Transverse).aup3` | `project/@rate` 192000 against a 48000 track, float32, 64 KiB pages |

The sources live in `/data2/vinyl_rips` and are the user's own irreplaceable
recordings. They are 271 MB and up, so they are not here and never will be; the
tests that read them are the `#[ignore]`d ones in `../corpus.rs`.

## What is Audacity's and what is not

`crates/import/src/fixture.rs` is the authority and says it in full. In short,
the dictionary, the element structure, the page size, `application_id`,
`user_version` and every attribute but four are the bytes Audacity produced.
Rewritten: `numsamples`, `waveblock/@start`, `waveblock/@length` (to describe
what survived) and `trimLeft` / `trimRight` (zeroed, because a trim measured
against 2.7 M samples would trim a 256-sample fixture out of existence).

**The audio is zeros.** Deliberately: a fixture tests the document grammar and
the block bookkeeping, and a git repository is not the place for 85 ms of
somebody's commercial vinyl. It follows that no fixture can prove sample
decoding - that is what `../corpus.rs` is for. The `0x10` thumbnail keeps its
first 64 bytes, which is a PNG signature and not a picture of anyone's desktop.

## Why not a fixture we wrote ourselves

Because it would prove that our encoder agrees with our decoder, and nothing
else. The Audacity grammar was derived clean-room from observed bytes (risk R13,
spike S5); the only evidence that the derivation is right is bytes we did not
produce.
