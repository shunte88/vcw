# `vcw-alac` is vendored third-party code

MIT License - see the header in any Rust source file for the full text. **That
sentence does not apply to this crate.** Everything under `crates/alac/src/` is
`Apache-2.0 OR MIT`, which is what the upstream author released it under, and
vendoring does not change the terms. `LICENSE-APACHE` and `LICENSE-MIT` beside
this file are upstream's own copies, carried verbatim as both licenses require.

## What it is

| | |
|---|---|
| Upstream | [`alac-encoder`](https://crates.io/crates/alac-encoder) 0.3.0 |
| Published | 2022-01-10 |
| Author | Linus Unnebäck `<linus@folkdatorn.se>` |
| Repository | <https://github.com/LinusU/rust-alac-encoder> |
| License | `Apache-2.0 OR MIT` |
| crates.io checksum | `0a32a4e6eec2ede679898711c25120685f4cd472be57be0b98642312232b9f29` |
| Vendored | 2026-10-11 |

It is itself a port of [Apple's open source ALAC
library](https://macosforge.github.io/alac/), which Apple released under
Apache-2.0 in 2011.

## Why it is vendored rather than depended on

`alac-encoder` could only emit **16-bit** ALAC. `FormatDescription::alac` set
`bits_per_channel: 16` as a literal, the field is private, there is no setter,
and `AlacEncoder::new` refuses any format description that did not come from
that function. The encoder underneath has complete, working arms for 20, 24 and
32 bits - Apple's format has carried all four since 2011 - and no caller could
reach any of them.

VCW cannot live with that. Every file in the project's own rip corpus is 24 or
32 bit, so a 16-bit-only writer would refuse all of them, and REQUIREMENTS §33
asks for ALAC so that a **lossless** archive can leave for the Apple side of a
chain. A format that can only be offered by first narrowing the master is not
the format that was asked for.

Upstream cannot supply the fix on any useful timescale: the repository's last
push was 2022-01-15 and its one open pull request has sat there since. The
patch below was offered upstream anyway, as a courtesy, on 2026-10-11:
<https://github.com/LinusU/rust-alac-encoder/pull/11>. Nothing here waits on
it. If it is ever merged and released, this crate goes away and
`crates/export` depends on the published `alac-encoder` again.

It is `publish = false`. Putting a forked ALAC encoder on crates.io under a VCW
name would leave two for people to choose wrongly between, and the fix belongs
upstream, not in a competing release.

## Every change made to upstream

Five files came across. `ag.rs`, `bit_buffer.rs` and `dp.rs` are **byte for
byte upstream** apart from `cargo fmt`. `lib.rs` and `matrix.rs` carry the
changes below and each one is commented at the point it was made.

### 1. The bit depth is a parameter (`lib.rs`) - the reason the fork exists

`FormatDescription::alac` and `FormatDescription::pcm` both take a width now,
and both check it against the new `SUPPORTED_BIT_DEPTHS`. `AlacEncoder::encode`
gained an assertion that the input width matches the output width, because it
strides its input by the *output* width: a mismatch does not fail, it reads
every sample at the wrong offset and losslessly encodes noise.

### 2. The 20- and 24-bit input reads were big-endian (`matrix.rs`)

All fourteen three-byte reads in `mix20`, `mix24`, `copy20_to_predictor` and
`copy24_to_predictor` were `i32::from_be_bytes`, while `mix16` and `mix32` in
the same file read the same input with `from_le_bytes`. One encoder, two
endiannesses, chosen by sample width.

This was unreachable code in the published crate - the only constructible width
was 16 - so nothing ever ran it and nothing noticed. Widening the encoder made
it reachable and the first 24-bit round trip came back as noise. All fourteen
are now `from_le_bytes`, which gives the crate a single input contract:
**interleaved little-endian PCM, packed at the declared width, no padding
byte**. The sign extension after each read is upstream's and unchanged.

### 3. Six unaligned, native-endian pointer casts (`lib.rs`)

`slice::from_raw_parts(input.as_ptr() as *const i16, ..)` and its `i32` twin
appeared six times. The input is a `&[u8]` and carries byte alignment only, so
the cast is undefined behavior however reliably x86 executes the load it
compiles to; and it is native-endian in a crate whose other reads of the same
bytes are explicitly little-endian. Both are fixed by the `read_i16` and
`read_i32` helpers, which also leave `bit_buffer.rs` holding the only `unsafe`
in the crate.

`bit_buffer.rs` is deliberately untouched. Its `read_unaligned` and
`write_unaligned` are correct about alignment by construction, but a
`write_lte25` near the end of a packet can put down four bytes when fewer
remain, so **callers must give `encode` an output buffer with a few bytes of
slack beyond `max_packet_size()`** - `crates/export/src/encoder.rs` does, and
says so.

### 4. Removals

- The `log` dependency. Its two `debug!` calls reported a frame that did not
  compress, which is ordinary operation - the encoder falls back to escape mode
  - and not worth a dependency. The crate now has none.
- The `PcmFormat` trait and `FormatDescription::pcm::<T>`. The trait existed to
  supply two numbers, was implemented for exactly one type, and that is where
  the 16-bit ceiling began. A plain `bits` argument reaches every width.
- The `#[cfg(test)] mod tests` block. It compares against `bincode` fixtures
  that upstream excludes from the published crate, and needs `serde` and
  `bincode` to do it. `tests/round_trip.rs` replaces it with a decode: every
  supported width and channel count goes out through this encoder and back
  through Symphonia's independent ALAC decoder, and has to come back bit for
  bit. That is a stronger claim than a golden file, and it is the claim the
  fork is making.

One thing that is not a change to the code: `src/` has been through
`cargo fmt`, because the gate's `fmt` leg covers every crate in the workspace
and rustfmt has no stable way to skip one. That makes a line-by-line diff
against an upstream unpack noisy, so the list above is the record rather than
the diff. The `clippy` allows at the top of `lib.rs` are still there, and still
for upstream's idiom rather than its layout.

## Updating it

Upstream is dormant, so there will probably be nothing to take. If there ever
is: diff `src/` against a fresh 0.3.0 unpack **run through `cargo fmt` first**,
or the formatting swamps the signal. Apply the new release, then re-apply each
numbered change above. `tests/round_trip.rs`
is what tells you whether you got it right - in particular, change 2 fails
*silently* in a way that still produces a playable file.
