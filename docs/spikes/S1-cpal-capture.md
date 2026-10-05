# S1 - CPAL bit-perfect capture and playback

**Status:** Linux/x86_64 complete. Windows, Android and macOS outstanding.
**Date:** 2026-09-22, **revised 2026-09-23** (CPAL 0.16.0 → 0.18.2)
**Requirement:** REQUIREMENTS §5, §8, §47 · **Plan:** PROJECT_PLAN §4 (S1), risk R2
**Artifact:** [`spikes/vinyl-audio-test`](../../spikes/vinyl-audio-test)

## Question

Can CPAL capture and play back bit-perfect audio on every target platform, with
device choice being - as the working assumption had it - "simply a matter of
selection from a device list"?

## Answer

**Yes on capture quality. The device list is trustworthy on 0.18, and was not
on 0.16.** On Linux, CPAL reaches genuine bit-perfect 24/192 capture and the
full §47 path works end to end. The original spike ran against CPAL 0.16.0 and
found two blocking defects; both are fixed in the released 0.18.2, which also
adds the exact API this spike had recommended upstreaming. The durable output
is not the bug list - it is the **kernel cross-check**, which is what let us
tell a real success from a convincing lie, and which must survive into the
product regardless of CPAL version.

Verified on this host (Linux 7.0.0-31, ALSA 1.2.15.3, **CPAL 0.18.2, stock from
crates.io, no patches**, HDA Intel PCH line input, `hw:CARD=0,DEV=0`):

```
negotiated        192000 Hz  2 ch  I32  (4 B/sample)   request honored exactly
kernel hw_params  /proc/asound/card0/pcm0c/sub0/hw_params : S32_LE 192000 Hz 2 ch
                  period=16384 buffer=32768
elapsed 15.04 s · callbacks 176 · frames captured 2883584 · frames dropped 0
blocks 61 · commit p50 4.9 ms / p99 30.4 ms / max 37.2 ms · peak WAL 4.0 MiB
verify: integrity ok · checksum failures 0 · sequence gaps 0
PASS (bit-perfect: kernel confirms the negotiated format)
```

Playback returned the capture in its stored format with no conversion and no
short callbacks beyond the expected one at end-of-file. Crash recovery, live
device, `SIGKILL`, three cycles at the D3 defaults (250 ms blocks, batch 1):

```
cycle 1: killed at 7s, recovered 6.50s (lost 0.50s, budget 0.50s) integrity=ok checksums=0 gaps=0
cycle 2: killed at 7s, recovered 6.50s (lost 0.50s, budget 0.50s) integrity=ok checksums=0 gaps=0
cycle 3: killed at 7s, recovered 6.50s (lost 0.50s, budget 0.50s) integrity=ok checksums=0 gaps=0
```

Recovery is exact and repeatable, but the loss is **two blocks, not one** - see
Finding 4, which is the most useful thing this revision turned up.

## Finding 1 - the 0.16 device list described the plug layer *(fixed in 0.18)*

`cpal-0.16.0/src/host/alsa/enumerate.rs` opened every sound card through
`plughw:`, not `hw:`, under a hardcoded `const USE_PLUGHW: bool = true`, and
exposed no way to open a device by PCM id. Consequences, all observed:

1. **Advertised configs were fiction** - 128–256 configurations per device,
   defaulting to 44100 Hz F32, because that is what the plug layer accepts.
2. **Some advertised rates failed at stream build**, e.g.
   `snd_pcm_hw_params_set_rate ... Invalid argument (22)`.
3. **Conversion was invisible.** The serious one.

### The failure mode, caught on the first run

Capturing from the default (PipeWire) device, CPAL reported the request
*honored exactly*: 48000 Hz, 2 channels, I32. The kernel disagreed:

```
negotiated        48000 Hz  2 ch  I32
/proc/asound/card2/pcm0c/sub0/hw_params : S16_LE 8000 Hz 1 ch
```

The hardware was running **8 kHz mono 16-bit**. PipeWire upsampled to 48 kHz
stereo 32-bit and CPAL reported clean success. We would have stored telephone
audio as "48 kHz 24-bit stereo", checksummed it, and called it bit-perfect.
**This is exactly the defect §8 exists to prevent.**

### Fixed upstream - and better than the fix we proposed

The original write-up recommended upstreaming an API to open a device by PCM id
(route B of three). **0.18 already has it.** Verified in the released crate:

- `enumerate.rs:56` iterates `[HW_PREFIX, PLUGHW_PREFIX]`, so `hw:` devices are
  listed alongside `plughw:` ones, the latter labeled *"Hardware device with
  all software conversions"*.
- `traits.rs:67` adds `HostTrait::device_by_id(&DeviceId)`, and every device now
  carries a stable `DeviceId` - on ALSA, the PCM id itself.

The effect is immediate and visible. The same USB device that used to advertise
the plug layer's fiction now tells the truth:

```
USB Device 0x46d:0x8ad, USB Audio  [DIRECT HARDWARE - bit-perfect capable]
  id: hw:CARD=2,DEV=0
  default in : 1 ch  8000 Hz  I16
  input configs: 1
```

One configuration, 8 kHz mono I16 - which is what the hardware is. Under 0.16
this device claimed 48 kHz stereo I32 and silently resampled.

`devices.rs` now keys selection on the id rather than the name, distinguishes
`direct_hardware` (`hw:`) from `converting` (`plughw:`), and reports both. The
three-route A/B/C decision the original spike raised is **moot**: upstream took
the good option.

**The cross-check stays.** `vinyl-audio-test` still verifies every capture
against `/proc/asound/card*/pcm*c/sub*/hw_params` and refuses to call a capture
bit-perfect unless the kernel agrees. A better device list makes the lie less
likely; it does not make it detectable. This is a **WP-04 obligation**, and it
needs a per-platform equivalent - WASAPI exclusive mode reports its negotiated
format directly, and that path is untested here.

## Finding 2 - a CPAL bug that returned zero frames *(fixed in 0.18)*

On 0.16, every ALSA capture on this host produced **zero callbacks**, with the
error callback firing continuously:

```
A backend-specific error has occurred: get_htstamp `0.0` was earlier than
get_trigger_htstamp `110272.984807580`
```

Cause: CPAL probed for usable driver timestamps *before* starting the stream.
Here `get_htstamp()` returns non-zero before start and `0.0` once running, so
the heuristic picked the htstamp path and every callback then failed its sanity
check. Format negotiation had already succeeded perfectly - the stream was
correctly configured and simply produced no audio, which is the worst way for
this to fail.

We root-caused it and carried a 32-line vendored patch. **0.18 fixes it
upstream, differently and better:** `htstamp_elapsed` now clamps with
`nanos.max(0)` instead of erroring, and a negative trigger delta is handled
explicitly as a driver bug with a monotone fallback. The comparison that used to
kill every callback can no longer produce an error at all.

The vendored copy and the `[patch.crates-io]` entry have been **deleted**. The
capture quoted at the top of this document is stock 0.18.2 from crates.io.

## Finding 3 - the default input is still the wrong default *(partly addressed)*

CPAL's default input on this host was the PipeWire path at 44100 Hz F32 - for a
vinyl capture application, the one configuration guaranteed not to be
bit-perfect. 0.18 improves the defaults (its release notes confirm `I32` and
`I24` now rank above `I16`, and `hw:` devices are enumerated), but "the default
device" is still a desktop-audio default, not an archival one.

`vinyl-audio-test` biases deliberately: given no explicit request it ranks
**I32 > I24 > I16 > F32**. That ranking is a policy decision that belongs in
WP-04, with the user able to override it, and the UI should steer toward a
`hw:` device rather than whatever the desktop calls default.

## Finding 4 - recovery loss is commit granularity **plus the driver buffer**

New in this revision, and it corrects something the earlier write-up stated too
confidently. S2 concluded that worst-case loss on a power cut is exactly
`block_ms × batch_blocks`, and the 0.16 run appeared to confirm it: kill at
7.00 s, recover 6.75 s, loss exactly one 250 ms block. On 0.18 against the `hw:`
device the loss is consistently **0.50 s - two blocks**.

That is not a regression. Measuring it properly:

| block_ms | loss | blocks lost |
|---|---|---|
| 250 | 0.50 s | 2 |
| 500 | 0.50 s | 1 |
| 1000 | 1.00 s | 1 |

The loss has a **floor of about 0.5 s that does not scale with block size**. The
obvious suspect was the ring, so we made it adjustable and swept it:

| ring_ms | 100 | 250 | 500 | 1000 |
|---|---|---|---|---|
| loss | 0.50 s | 0.50 s | 0.50 s | 0.50 s |

**Ring capacity makes no difference at all** - the writer keeps the ring
near-empty, so it is a throughput cushion, not a durability exposure. The floor
is audio the *driver* has buffered but never handed to a callback: ALSA reports
`buffer=32768` frames here, which at 192 kHz is 170 ms, and recovery is then
quantised up to a block boundary. 170 ms of driver buffer plus up to 250 ms of
uncommitted partial block rounds to two 250 ms blocks.

Three consequences:

1. **The crash-test budget formula was wrong** and was failing correct runs. It
   now budgets `commit granularity + driver buffer`, rounded up to a block
   boundary, with the driver allowance exposed as `--inflight-ms` (default
   250 ms, against the 170 ms measured here). Ring capacity is deliberately
   absent from the formula because it was measured not to belong there.
2. **S2's "spend the budget on smaller, more frequent commits" has a floor.**
   Below roughly the driver buffer duration, shrinking `block_ms` stops buying
   durability - at 250 ms blocks the driver buffer already dominates. This
   refines D3 rather than overturning it: 250 ms remains a fine choice, but it
   should be understood as *at* the floor, not comfortably inside it.
3. **Worst-case loss is a property of the device, not just our config.** A
   converter with a deeper buffer loses more on a power cut, and we cannot
   configure that away. The honest figure to show a user is
   `block_ms × batch + device buffer`, read from the device.

Storage recovery itself remains exact in every run: integrity ok, zero checksum
failures, zero sequence gaps, recovery always landing on a block boundary.

## What this means for the plan

- **Capture quality: CPAL delivers.** Bytes arrive unconverted via
  `build_input_stream_raw` and reach SQLite untouched.
- **Device selection is sound on 0.18** - `hw:` is enumerable and selectable by
  id - **but must still be verified against the OS**, not assumed from the API.
- R2 should read: *"CPAL's device abstraction can hide the hardware's real
  capabilities; bit-perfection must be verified against the OS, not assumed
  from the API."* The likelihood drops back toward Low on 0.18, but the
  mitigation (the verifier) is mandatory and is what keeps it there.
- **Stay current on CPAL.** Two blocking defects and the device-id API all
  arrived within two minor releases. Pinning 0.16 would have cost us a fork.

Nothing here changes the architecture. It adds one component - a per-platform
format verifier - and removes one decision.

## Upgrading 0.16 → 0.18

Mechanical, about 30 call sites, no design impact:

- `SampleRate` is now a plain `u32` alias: drop `.0`, and `SampleRate(x)` → `x`.
- `build_*_stream_raw` takes `StreamConfig` **by value** (it is `Copy`).
- `device.name()` → `device.description()?.name()`; `device.id()?` gives the id.
- All per-operation error types collapse into one `cpal::Error` with `.kind()`.
- **Behavioral, and silent if missed:** ALSA, CoreAudio and JACK no longer
  auto-start streams - an explicit `stream.play()` is required. We already had
  one on both paths.

## Coverage of §47

| § | behavior | state |
|---|---|---|
| 1 | list devices | done - `devices`, now with ids and a hw/plug distinction |
| 2 | list supported formats | done - `devices -v`, `formats` (selectable by id) |
| 3 | open the requested stream | done - `capture` |
| 4 | create a SQLite project | done - via `capture-core`, shared with S2 |
| 5 | capture PCM into SQLite blocks | done - 24/192, zero drops |
| 6 | live peak/RMS | done - computed in the RT callback, published by atomics |
| 7 | capture diagnostics | done - full report, JSON or human |
| 8 | play captured audio from SQLite | done - no conversion, all frames |
| 9 | verify block checksums | done - `verify`, automatic after capture |
| 10 | simulate interruption and recovery | done - `crash-test`, SIGKILL, 3/3 exact |
| 11 | requested vs negotiated format | done - **and cross-checked against the kernel** |
| 12 | overruns, underruns, dropped frames | done |
| - | run on Linux, Windows, macOS | **Linux only.** See below. |

## Outstanding

- **Windows.** WASAPI exclusive mode is the bit-perfect path and is untested.
  The kernel cross-check has no direct equivalent; WASAPI reports the negotiated
  format itself, which needs its own verifier.
- **Android/AAudio** and **macOS/CoreAudio** - macOS has no hardware here at all,
  so it stays unverified until that changes.
- **A real converter.** Everything above used the motherboard's line input. The
  HiFiBerry DAC+ADC Pro on the Pi 5 and the Tascam DA-3000 are the devices that
  matter, and neither has been through this yet. Finding 4 in particular will
  read differently on hardware with a different buffer depth.
- **`snd-aloop` bit-exactness.** Loading the loopback module needs root and has
  not been done. It is the only way to prove capture output equals playback
  input sample-for-sample rather than merely proving the format was not converted.
- **Exclusive/hog modes** per platform - the capture-mode matrix G0 asks for is
  not yet published.

## Running it

```sh
cargo run -p vinyl-audio-test -- devices --input-only     # ids and hw/plug flags
cargo run -p vinyl-audio-test -- formats "hw:CARD=0,DEV=0"
cargo run -p vinyl-audio-test -- capture --device "hw:CARD=0,DEV=0" \
    --rate 192000 --channels 2 --format i32 --db ./capture.vcw --duration 30
cargo run -p vinyl-audio-test -- play --db ./capture.vcw
cargo run -p vinyl-audio-test -- verify ./capture.vcw
cargo run -p vinyl-audio-test -- crash-test --device "hw:CARD=0,DEV=0" \
    --rate 192000 --channels 2 --format i32 --kill-after 7 --cycles 3
```

Select devices **by id**, not name: on ALSA the same name appears as both a
`hw:` and a `plughw:` PCM, and only one of them can be bit-perfect.
Add `--json` to any of them for machine-readable output.
