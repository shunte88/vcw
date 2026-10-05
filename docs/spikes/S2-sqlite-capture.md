# S2 - SQLite capture benchmark: findings

**Spike:** `spikes/sqlite-capture-bench`
**Requirement:** REQUIREMENTS.md §48 (and §10, §13, §14, §37)
**Date:** 2026-09-22
**Status:** 90-minute soak passed on x86_64/SSD; Tier 1 hardware runs outstanding

---

## Question

Can the architecture's load-bearing path hold?

```
synthetic RT source → bounded lock-free ring → batched SQLite BLOB writes
                                             → simultaneous analysis reads
```

Specifically at 24-bit/192 kHz stereo, under deliberate abuse and crash injection.

## Method

`sqlite-capture-bench` replaces the CPAL callback with a synthetic source that emits
frames on a real-time deadline and never blocks - if the ring is full it drops the chunk
and counts it, as a real overrun would. The payload is a **deterministic pattern**, so
verification proves not merely that bytes survived (a checksum shows that) but that they
are the *right* bytes, in the right order, at the right offsets. Two reader threads run
against the same database throughout, imitating the waveform worker (summary reads) and
the fingerprint worker (whole-block reads with checksum verification).

The schema is already the D1 shape: `sampleblocks` mirroring AUP4 column for column,
plus `capture_blocks` carrying the provenance Audacity has nowhere to put.

## Headline results

All runs 24-bit/192 kHz stereo, 20 s, 2 concurrent readers, on **SSD** (see caveat 1).

| block | batch | sync | commit p50 | commit p99 | commit max | budget | dropped |
|-------|-------|------|-----------|-----------|-----------|--------|---------|
| 1000 ms | 4 | NORMAL | 105 ms | 197 ms | 197 ms | 4000 ms | **0** |
| 250 ms | 1 | NORMAL | 4.0 ms | 71 ms | 78 ms | 250 ms | **0** |
| 250 ms | 1 | FULL | 7.7 ms | 50 ms | 55 ms | 250 ms | **0** |
| 50 ms | 1 | NORMAL | 1.0 ms | 48 ms | **80 ms** | 50 ms | 0 |

Plus:

- **4× real-time abuse:** 5.8 MiB/s sustained, zero drops.
- **Write amplification:** 1.01–1.02× across every configuration.
- **Peak WAL:** 4 MiB at 250 ms/batch 1; 12–24 MiB at 1 s/batch 4. Bounded, not growing.
- **RSS:** 6–16 MiB, flat.
- **Reader checksum failures:** 0, across thousands of concurrent reads during writing.

### Crash injection

`SIGKILL` mid-capture, three cycles, on SSD, at 250 ms/batch 1:

```
killed at 7.00s → recovered 6.75s, integrity ok,
                  0 checksum failures, 0 pattern failures, 0 sequence gaps
```

Identical on all three cycles, and at 1 s/batch 4 the loss was correspondingly 4 s.

### 90-minute sustained run

The §48 soak, run to completion on SSD at 24-bit/192 kHz stereo with both readers active
(90 min wall clock, finished 16:45):

| Metric | Result | Acceptance |
|---|---|---|
| Wall clock | 5400.196 s, real-time factor 0.99996 | - |
| Frames produced | 1,036,800,512 | - |
| Frames dropped | **0** (0.0 ppm), 0 overrun events | zero ✓ |
| Blocks committed | 21,601 | - |
| Commit p50 / p95 / p99 / max | 2.1 / 48.9 / 63.2 / 102.3 ms | p99 within the 250 ms block budget ✓ |
| Peak WAL | 4.57 MiB, against an 8.41 GB database | bounded ✓ |
| Write amplification | 1.0135× | - |
| Checkpoint stalls | 1 event, 21.6 ms | - |
| Summary build p99 / max | 11.1 / 21.7 ms | - |
| Reader work | 207,707 queries, 4,236,810 blocks, **0 checksum failures** | zero ✓ |
| Reader p99, worse of the two | 9.5 ms | - |
| RSS at exit | 8.1 MiB | flat ✓ |
| Worst producer lateness | 9.9 ms | - |

Verdict `PASS`: 8.29 GB of audio into an 8.41 GB database, nothing dropped, nothing
corrupted, memory flat.

**The tail held up.** Against the 20-second run in the same configuration, p50 and p99
both *improved* slightly (4.0 → 2.1 ms, 71 → 63 ms) while the maximum grew 78 → 102 ms.
That is the expected shape: the soak sampled 21,601 commits against the short run's 80,
so the extreme has 270× more chances to be unlucky and the percentiles are simply better
estimated. The worst commit observed anywhere is still 2.4× inside the block budget.
No drift, no creep - the distribution is stationary over 90 minutes.

**Three things to be honest about.**

1. **The soak ran the harness defaults, not the firmed config.** `synchronous=NORMAL` and
   **interleaved** layout, where D3 firms to `FULL` and per-channel. The short matrix says
   FULL is free or better at the tail and per-channel is kinder to the WAL, so the firmed
   config should be no worse - but *should be* is not *measured*. A soak in the firmed
   configuration must run before D3 is closed.
2. **Recovery was not exercised by this run.** The soak exited cleanly. Crash recovery is
   evidenced by the separate three-cycle `SIGKILL` test above (and S1's live-capture
   crash test), not by the soak.
3. **The `reader_latency` field is mislabeled.** It reports `count: 2` because
   `main.rs:327` folds each reader's *p99* in as a single sample, so the object is a
   two-point distribution over p99s and its own `p50`/`p95`/`p99` labels are noise. The
   one figure that does mean something is `max_us: 9501` - i.e. **both** readers held a
   p99 under 9.5 ms while the writer was committing 21,601 blocks, which is the result we
   wanted. The per-query samples are collected (`readers.rs:53`); only the roll-up throws
   them away. Worth reporting the merged distribution properly before the Pi 5 run, where
   reader latency is the number most likely to bite.

## What the numbers mean

**1. Throughput is not the constraint, and is not close to being one.** At the archival
worst case the commit tail sits at 5–30 % of the available budget, and the path absorbs
4× real-time without dropping a frame. The R1 fallback (sidecar block file + SQLite
index) looks unnecessary. §12's single-file project survives.

**2. The real design variable is recovery granularity, not speed.** Worst-case audio loss
on a power cut is driven by commit granularity, not throughput, and the crash test
demonstrates recovery landing exactly on a block boundary every time. Since throughput is
a solved problem, that budget should be spent on *smaller, more frequent commits*, not on
bigger batches. This inverts the usual instinct to batch for throughput.

> **Qualified by S1 Finding 4 (2026-09-23).** This synthetic harness has no audio device,
> so `block_ms × batch_blocks` is the *whole* story here. Against a real converter it is
> not: the driver holds audio that never reached a callback, and that is lost too. On the
> `hw:` device measured in S1 the ALSA buffer is 32768 frames = 170 ms at 192 kHz, which
> puts a **floor** under recovery loss that shrinking `block_ms` cannot cross. Below
> roughly the driver buffer duration, smaller commits stop buying durability. 250 ms is
> still the right default, but it sits *at* that floor rather than inside it. See
> [`S1-cpal-capture.md`](S1-cpal-capture.md) Finding 4 for the measurements.

**3. Recommended default: 250 ms blocks, batch 1.** Worst-case loss a quarter second,
commit tail ~29 % of budget, WAL steady at 4 MiB. 50 ms blocks go too far - the maximum
commit (80 ms) exceeds the block budget (50 ms), so the ring starts absorbing tails
rather than idling. It still didn't drop a frame, but the margin is gone and there is
nothing to gain.

**4. `synchronous=FULL` is essentially free - take it.** At 250 ms/batch 1 it cost 3.7 ms
on the median and was *better* at the tail than NORMAL. For a capture you cannot repeat
without replaying the side, maximum durability at no measurable price is an easy call.

**5. AUP4 compatibility costs nothing measurable.** Per-channel (AUP4-style) and
interleaved layouts both ran clean; per-channel showed *lower* peak WAL (9 vs 18 MiB) and
cheaper checkpoints, at slightly higher summary cost. D1's superset schema stands on
evidence rather than aesthetics.

**6. Summary computation is the largest per-block CPU cost** - 26–31 ms per 1 s block at
192 kHz, scaling linearly (~1.3 ms per 50 ms block, ~3 % of real time). Acceptable on the
writer thread today. If it grows - LUFS, true peak, richer pyramids - it should move to
the waveform worker rather than sit in the commit path.

## Caveats - what is not yet proven

1. **The first round of results was measured against tmpfs.** `/tmp` is RAM on this
   machine, which made SQLite look 3–6× better at the tail than it is. Every number above
   has been re-measured on SSD. Worth stating loudly: benchmark output is only as
   trustworthy as the filesystem underneath it, and a plausible-looking result is the
   easiest kind to accept without checking.
2. **The 90-minute soak passed, but in the unfirmed configuration** (NORMAL/interleaved).
   See the sustained-run section above.
3. **x86_64 SSD only.** The acceptance runs must happen on the Pi 5 (SD/NVMe, the honest
   worst case) and the Windows rig. Expect materially worse tails on the Pi.
4. **Not yet exercised:** disk-full, induced fsync stalls, `VACUUM`/compaction, copying a
   live project, page-size sweep, WAL2, and the full §48 matrix. The harness supports the
   sweep (`sqlite-capture-bench sweep`); it has not been run to completion.
5. **No real-time thread priority.** The synthetic source runs at normal priority with no
   `SCHED_FIFO`. A real CPAL callback gets better scheduling, so this is conservative.

## Provisional recommendations for D3

| Parameter | Recommendation | Confidence |
|-----------|----------------|-----------|
| Block size | **250 ms** | High, pending Pi 5 |
| Transaction batch | **1 block** | High |
| Journal | **WAL** | High |
| `synchronous` | **FULL** | High - but soaked only at NORMAL |
| Layout | **Per-channel** (AUP4-compatible) | Medium - both work; per-channel is kinder to the WAL and keeps D1 mechanical, and S5 confirms AUP3/AUP4 store mono blocks natively. Soaked only at interleaved |
| Ring capacity | **≥ 500 ms** | Medium - it absorbs commit tails; size it at ≥ 5× measured commit max (102 ms over 90 min → ≥ 510 ms). S1 measured that ring size does **not** affect crash loss, so this is a throughput cushion only |
| Page size | 4 KiB default | Low - not yet swept |

## Reproducing

```sh
cargo build --release
./target/release/sqlite-capture-bench run --db ./bench.vcw --rate 192000 \
    --block-ms 250 --batch-blocks 1 --sync full --duration 20
./target/release/sqlite-capture-bench crash-test --db ./crash.vcw --kill-after 7 --cycles 3
./target/release/sqlite-capture-bench sweep --dir ./sweep --duration 20 > sweep.jsonl
./target/release/sqlite-capture-bench verify ./bench.vcw
```

**Do not benchmark against `/tmp` if it is tmpfs.** Use a path on the storage the real
application will use.
