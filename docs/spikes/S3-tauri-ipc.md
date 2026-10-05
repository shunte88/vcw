# S3 - Tauri 2 IPC throughput

**Status:** complete on Linux/x86_64 (WebKitGTK 2.52.6) - 13-arm matrix plus a
30-minute soak. Windows/WebView2 and Pi 5 outstanding.
**Gates:** G0, via **D6**
**Crate:** `spikes/tauri-ipc-bench`

D6 is currently written as:

> Tauri 2 channels (not the event bus) for meter and waveform deltas;
> pre-serialized compact payloads; coalesce to <=60 Hz in Rust. *Validate in S3.*

This spike exists to accept, amend or reject that sentence, and to answer the
question it does not ask: whether the waveform can be drawn on the main thread
at all.

## What reading the Tauri source changed about the experiment

The obvious design - channels versus the event bus, binary versus JSON, four
cells - would have measured the wrong thing. Two things in `tauri` 2.11.6 make
that matrix misleading before a single number is taken.

**Both transports are `eval` for small payloads.** `src/ipc/channel.rs` sends a
payload under `MAX_JSON_DIRECT_EXECUTE_THRESHOLD` (8192 bytes) by calling
`webview.eval(format_raw_js(callback_id, json))`. `src/event/mod.rs` builds

```js
(function () { const fn = window['{}']; fn && fn({event: '{}', payload: {}}, {ids}) })()
```

and evals that. So "channels versus the event bus" is a question about how many
layers of JavaScript dispatch sit between the eval and the handler - not about
two different pipes. Above the threshold, channels switch to a
`ChannelDataIpcQueue` entry plus a `fetch`-based round trip; none of the
payloads in §35 are anywhere near that size, so that path is not exercised
here.

**Small binary payloads are inflated, not compacted.**
`MAX_RAW_DIRECT_EXECUTE_THRESHOLD` is 1024 bytes, and under it an
`InvokeResponseBody::Raw` is rendered by `serde_json::to_string(&bytes)` - a
*decimal* JSON array - and evaluated as `new Uint8Array([...]).buffer`. A
30-byte meter frame becomes well over 100 characters of JavaScript source. So
the `raw` arm is in this matrix as a hypothesis to be disproved, not as an
assumed win.

A third finding came from the JS side. `@tauri-apps/api`'s `Channel` class
maintains a per-message reorder buffer (`index`, `nextMessageIndex`,
`pendingMessages`) so that out-of-order deliveries are replayed in order. The
event bus has no equivalent. That is per-message main-thread work that channels
pay and events do not, pulling in the opposite direction to D6.

Upstream's own comments say the two thresholds were tuned against WebView2
v135 and macOS. VCW's Linux target is WebKitGTK, which nobody has measured.

## Method

`spikes/tauri-ipc-bench` is a real Tauri 2 app: a dedicated OS producer thread
(D8 - no async on the real-time path) emitting the three payloads §35 names, and
a React frontend that draws them.

The three payloads are the real ones, not stand-ins. §35 forbids high-frequency
PCM crossing the boundary, so a `MeterFrame` is peak/RMS/clip per channel, a
`WaveDelta` is a run of `[min_l, max_l, min_r, max_r]` summary buckets - the
`summary256` shape S5 found in AUP3 and D1 adopted - and a `PositionUpdate` is
a frame counter.

Each is implemented in three encodings: `serde` (tauri's blanket
`impl<T: Serialize> IpcResponse`), `manual` (hand-built compact JSON with short
keys into a reused buffer - D6's "pre-serialized"), and `raw` (fixed-layout
little-endian bytes). Encoding happens outside the timed region for the latter
two, because moving that work off the IPC path is precisely what D6 proposes;
the `serde` arm's serialization is inside the timed region, which is the
difference being measured.

The producer uses absolute scheduling (`next += period`, never
`sleep(period)`), and records its own tick lateness, so that jitter observed in
the webview can be attributed to the correct side of the boundary.

### The arms

Thirteen arms, written out as a named list in `src/arms.ts` rather than
generated from a cross product: a 5x5x2 product would be 50 runs, most of which
vary two things at once and therefore answer nothing. The baseline for every arm is
192 kHz capture with 256-frame callbacks - a 750 Hz worker rate, the worst case
§35 has to survive - coalesced to 60 Hz meter, 30 Hz waveform, 10 Hz position.

Three of the thirteen are controls, and they are what make the rest readable -
a 2x2 of traffic against drawing:

- `control-raf` - no traffic, no drawing. The rAF loop and nothing else: the
  floor this rig can produce.
- `control-idle` - no traffic, full incremental drawing. Isolates the canvas
  and compositor cost.
- `control-nodraw` - full traffic, nothing drawn. Separates transport cost from
  canvas cost, which no single real arm can do.

The two idle arms are set to 0.001 Hz rather than "off". An earlier draft used
1 Hz and was not an idle control at all: at 192 kHz with 1024-frame buckets a
one-second tick batches 187 waveform buckets into a single 4.6 kB message, so
the arm was quietly measuring a burst pattern nothing else in the matrix used.
At 0.001 Hz the period exceeds the run length and nothing is ever sent.

### What is and is not measurable here

`performance.now()` is clamped in WebKit for Spectre reasons. The bench measures
that clamp rather than assuming it and reports it per run; on this rig it is
exactly 1.000 ms. Three consequences:

- One-way Rust-to-webview latency is **not** measurable. A cross-clock estimate
  built from `now_us` round trips has an error bar wider than the quantity, and
  the first draft of this bench duly reported every latency as zero after
  clamping. It was replaced with `echo`: the webview hands the frame's Rust
  timestamp straight back through a command, and Rust times the whole round
  trip on its own clock at microsecond resolution. That is an upper bound on
  one-way latency, which is worth more than a precise-looking guess.
- Client-side durations are reported as log-bucket upper edges, not
  interpolated percentiles, and are floored at 1 ms. Alongside them the bench
  counts frames whose draw took >=1 ms, >=4 ms and >=8 ms, which is exact at the
  resolution the clock actually has.
- Frame *intervals* are unaffected in any way that matters: the question is
  16.7 ms versus 33 ms, and a 1 ms floor resolves that comfortably.

The clamp does leave one thing intact, and it is the thing the conclusions rest
on. WebKit clamps by truncating the *timestamps*, not the durations, so a
duration reads as either `floor(d)` or `floor(d) + 1` ms depending on where in
the millisecond the interval started. For a start phase uniform within the
millisecond the expectation of the reading is exactly `d`: a 0.3 ms draw reads
1 ms about three times in ten and 0 ms otherwise. **The mean is unbiased even
though every individual sample is useless.** That is why main-thread occupancy
below is computed from means, and why no client-side percentile is quoted as
though it were precise.

One more thing had to be measured rather than assumed. `requestAnimationFrame`
in WebKitGTK is **not** paced to vsync: on this fixed 60 Hz output the observed
callback rate varied from 56/s to 96/s depending on what the arm was doing. So
"fps" is not a quality measure here and frame counts are not comparable across
arms. Everything below is read in terms of occupancy and of gaps longer than
20 ms, neither of which depends on the callback rate being fixed.

### Rig caveat, recorded up front

This machine mirrors two outputs at 3840x2160: DP-1 at 59.98 Hz and HDMI-1,
the primary, at **29.96 Hz**. Some of the frame-interval outliers below belong
to that display configuration, not to the application. This is why
`control-idle` exists; every jank figure should be read as a difference from it,
not as an absolute.

Runs are unattended (`S3_AUTORUN=matrix S3_SECS=n`), because a run started by a
human clicking a button is a run whose starting conditions are not
reproducible. The window is real and on the real compositor: Xvfb has no
vsync, and frame pacing is the core measurement.

## Results

<!-- generated by spikes/tauri-ipc-bench/analyze.py -->

Thirteen arms, 60 s each, one run per arm, on the real compositor. Before any
of the numbers: **zero send errors, `recv/sent` = 1.000 and zero sequence gaps
in every arm**, including the two uncoalesced arms that pushed 45,000 meter
messages apiece. Nothing below is a reliability finding. The boundary did not
drop, reorder or corrupt anything at any rate tested.

### Frame pacing, draw cost and main-thread occupancy

`rAF/s` is not a quality measure here: WebKitGTK does not pace `requestAnimationFrame` to vsync, so it varies by arm on a fixed 60 Hz output (see `control-raf`). Occupancy is the comparable figure: mean draw duration x callbacks / wall clock, i.e. the share of the main thread the UI spends painting. Mean is used rather than a percentile because a millisecond-clamped clock makes each *sample* useless but leaves the *mean* unbiased: a 0.3 ms draw reads as 1 ms about 30% of the time and 0 ms otherwise.

| arm | render | rAF/s | mean draw | occupancy | >20ms gaps | >33ms | draw >=1ms | draw >=4ms |
|---|---|--:|--:|--:|--:|--:|--:|--:|
| `control-raf` | none | 61.9 | 0.00 ms | 0.0% | 0 (0.0%) | 0 | 6 | 0 |
| `control-idle` | incremental | 61.9 | 0.11 ms | 0.7% | 0 (0.0%) | 0 | 373 | 0 |
| `control-nodraw` | none | 61.9 | 0.00 ms | 0.0% | 1 (0.0%) | 0 | 11 | 0 |
| `channel-serde` | incremental | 61.9 | 0.30 ms | 1.8% | 104 (2.8%) | 0 | 950 | 0 |
| `channel-manual` | incremental | 61.9 | 0.28 ms | 1.7% | 140 (3.8%) | 0 | 891 | 0 |
| `channel-raw` | incremental | 61.9 | 0.27 ms | 1.7% | 104 (2.8%) | 0 | 925 | 0 |
| `event-serde` | incremental | 62.0 | 0.28 ms | 1.7% | 80 (2.1%) | 0 | 925 | 0 |
| `event-manual` | incremental | 62.0 | 0.30 ms | 1.8% | 117 (3.1%) | 0 | 935 | 0 |
| `render-naive` | naive | 55.7 | 5.21 ms | 29.0% | 298 (8.9%) | 0 | 3324 | 3020 |
| `render-worker` | worker | 91.1 | 0.05 ms | 0.5% | 6 (0.1%) | 0 | 277 | 0 |
| `render-react-dom` | react-dom | 97.4 | 0.18 ms | 1.7% | 10 (0.2%) | 0 | 939 | 0 |
| `uncoalesced` | incremental | 62.1 | 0.26 ms | 1.6% | 71 (1.9%) | 0 | 869 | 0 |
| `uncoalesced-raw` | incremental | 62.1 | 0.27 ms | 1.6% | 86 (2.3%) | 0 | 857 | 0 |

### Send cost on the producer thread

| arm | transport | enc | meter/s | meter send p99 | wave send p99 | tick late p99 | producer CPU |
|---|---|---|--:|--:|--:|--:|--:|
| `control-raf` | channel | manual | 0 | 0 us | 0 us | 2 us | 0.00% |
| `control-idle` | channel | manual | 0 | 0 us | 0 us | 2 us | 0.00% |
| `control-nodraw` | channel | manual | 60 | 64 us | 16 us | 256 us | 0.16% |
| `channel-serde` | channel | serde | 60 | 64 us | 32 us | 256 us | 0.17% |
| `channel-manual` | channel | manual | 60 | 64 us | 16 us | 256 us | 0.16% |
| `channel-raw` | channel | raw | 60 | 64 us | 32 us | 256 us | 0.19% |
| `event-serde` | event | serde | 60 | 64 us | 32 us | 256 us | 0.18% |
| `event-manual` | event | manual | 60 | 64 us | 32 us | 256 us | 0.19% |
| `render-naive` | channel | manual | 60 | 64 us | 16 us | 256 us | 0.14% |
| `render-worker` | channel | manual | 60 | 64 us | 32 us | 256 us | 0.15% |
| `render-react-dom` | channel | manual | 60 | 64 us | 32 us | 256 us | 0.16% |
| `uncoalesced` | channel | manual | 750 | 32 us | 16 us | 256 us | 0.75% |
| `uncoalesced-raw` | channel | raw | 750 | 32 us | 32 us | 256 us | 0.79% |

### Payload size, and the size tauri actually evals

| arm | enc | meter payload | meter in JS | wave payload | wave in JS | JS source kB/s |
|---|---|--:|--:|--:|--:|--:|
| `control-nodraw` | manual | 76 B | 76 B | 197 B | 197 B | 10.9 |
| `channel-serde` | serde | 98 B | 98 B | 217 B | 217 B | 12.9 |
| `channel-manual` | manual | 76 B | 76 B | 197 B | 197 B | 10.9 |
| `channel-raw` | raw | 30 B | 116 B (3.9x) | 74 B | 257 B | 15.5 |
| `event-serde` | serde | 98 B | 98 B | 217 B | 217 B | 12.9 |
| `event-manual` | manual | 76 B | 76 B | 197 B | 197 B | 10.9 |
| `render-naive` | manual | 76 B | 76 B | 197 B | 197 B | 10.9 |
| `render-worker` | manual | 76 B | 76 B | 197 B | 197 B | 10.9 |
| `render-react-dom` | manual | 76 B | 76 B | 197 B | 197 B | 10.9 |
| `uncoalesced` | manual | 77 B | 77 B | 197 B | 197 B | 63.9 |
| `uncoalesced-raw` | raw | 30 B | 117 B (3.9x) | 74 B | 258 B | 96.2 |

### Delivery latency (Rust clock, round trip) and receive-side cost

| arm | echo RTT mean | echo RTT p99 | echo RTT max | meter recv | recv/sent | seq gaps | handler p99 | RSS total |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| `control-raf` | n/a | n/a | n/a | 0 | 0.000 | 0 | - | 476 MiB |
| `control-idle` | n/a | n/a | n/a | 0 | 0.000 | 0 | - | 481 MiB |
| `control-nodraw` | 1.4 ms | 2.0 ms | 3.1 ms | 3600 | 1.000 | 0 | <=1 ms (floor) | 482 MiB |
| `channel-serde` | 10.2 ms | 32.8 ms | 23.0 ms | 3600 | 1.000 | 0 | <=1 ms (floor) | 487 MiB |
| `channel-manual` | 10.5 ms | 32.8 ms | 23.0 ms | 3600 | 1.000 | 0 | <=1 ms (floor) | 489 MiB |
| `channel-raw` | 10.7 ms | 32.8 ms | 23.4 ms | 3600 | 1.000 | 0 | <=1 ms (floor) | 494 MiB |
| `event-serde` | 10.0 ms | 32.8 ms | 23.8 ms | 3600 | 1.000 | 0 | <=1 ms (floor) | 496 MiB |
| `event-manual` | 10.2 ms | 32.8 ms | 22.8 ms | 3600 | 1.000 | 0 | <=1 ms (floor) | 493 MiB |
| `render-naive` | 13.0 ms | 32.8 ms | 25.4 ms | 3600 | 1.000 | 0 | <=1 ms (floor) | 494 MiB |
| `render-worker` | 9.7 ms | 32.8 ms | 21.7 ms | 3600 | 1.000 | 0 | <=1 ms (floor) | 504 MiB |
| `render-react-dom` | 8.7 ms | 32.8 ms | 18.3 ms | 3600 | 1.000 | 0 | <=1 ms (floor) | 509 MiB |
| `uncoalesced` | 2.8 ms | 32.8 ms | 23.1 ms | 45000 | 1.000 | 0 | <1 ms (floor) | 509 MiB |
| `uncoalesced-raw` | 3.0 ms | 32.8 ms | 21.6 ms | 45000 | 1.000 | 0 | <1 ms (floor) | 509 MiB |

Two notes on reading that table. The `p99` column is a log-bucket *upper edge*
while `max` is exact, so p99 reading higher than max is expected arithmetic
rather than a contradiction: every traffic-carrying arm's p99 lands in the
16.4-32.8 ms bucket. And the two idle controls send nothing, so they echo
nothing - `n/a`, not a fast round trip.

### The finding is about rendering, not about IPC

D6 is a sentence about transport and encoding. Neither turned out to matter.
The one thing that moved the main thread by an order of magnitude is the thing
D6 does not mention: **how the waveform is drawn.**

| | `render-naive` | `render-worker` |
|---|--:|--:|
| main-thread occupancy | **29.0%** | **0.5%** |
| mean draw per frame | 5.21 ms | 0.05 ms |
| frames costing >=4 ms | 3020 of ~3340 | 0 |
| gaps > 20 ms | 298 (8.9%) | 6 (0.1%) |
| rAF callbacks/s | 55.7 | 91.1 |
| echo RTT mean | 13.0 ms (worst of 13) | 9.7 ms |

A full-canvas redraw of a 1400x220 waveform at 60 Hz costs **29% of the main
thread** and makes nearly every frame a >=4 ms frame. The same waveform in an
`OffscreenCanvas` worker costs **0.5%**, and the in-between - incremental
self-blit redraw on the main thread, the `IncrementalWave` used by every other
drawing arm - costs 1.7%.

**Read that as jank risk, not as CPU.** Occupancy is the share of the *main
thread* spent inside the draw callback, which is the right metric for
responsiveness and the wrong one for a power or thermal budget. Moving the draw
into a worker stops it blocking the main thread; it does not make the work
cheaper. Whole-application CPU, sampled externally during the 30-minute soak
(`cpu-sample.sh`, because the bench cannot see this), is **~89% of one core**
for the recommended configuration: 68% in `WebKitWebProcess`, 18% on the tauri
process's GTK main thread dispatching evals, 1.8% in `ReceiveQueue`, 0.5% in
the producer.

The worker arm is also the only configuration in the matrix that hit its frame
budget every single time. Its own report, measured on the worker's clock:
3988 frames in 60 s, mean interval 15.11 ms, **maximum interval 16.0 ms**. Not
one frame over budget in a minute. Every main-thread arm has a worst-case
interval of 18.3-25.4 ms. The worker's rAF is paced tighter than the main
thread's, which is worth knowing independently of the occupancy figure.

Two caveats keep this from being a rout. No arm produced a single gap over
33 ms, so even the naive renderer never actually fell to 30 fps on this rig -
it ate the main thread without missing a visible frame. And `render-naive` is
deliberately the dumb implementation; the interesting comparison for VCW is
worker versus incremental, which is 0.5% against 1.7% and much less dramatic.

### Transport: channels versus the event bus is a wash

Reading the source predicted this and the measurement confirms it. Across the
four transport/encoding arms plus `control-nodraw`, occupancy is 1.7-1.8% and
echo RTT is 10.0-10.7 ms with **no ordering by transport**:

| | occupancy | >20 ms gaps | echo RTT mean |
|---|--:|--:|--:|
| `channel-serde` | 1.8% | 104 | 10.2 ms |
| `channel-manual` | 1.7% | 140 | 10.5 ms |
| `channel-raw` | 1.7% | 104 | 10.7 ms |
| `event-serde` | 1.7% | 80 | 10.0 ms |
| `event-manual` | 1.8% | 117 | 10.2 ms |

The gap counts span 80-140 in no pattern - `event-serde` is lowest and
`channel-manual` highest, which is the opposite of D6's prediction, and the
spread is single-run noise rather than signal. At §35 payload sizes both
transports are the same `webview.eval()` call with a different number of JS
dispatch layers after it, and that difference is below this rig's noise floor.
The channel's per-message reorder buffer did not show up as a cost either.

### Encoding: `manual` is marginal, `raw` is actively wrong

`raw` was in the matrix as a hypothesis to disprove, and it is disproved:

| enc | meter payload | meter as evaluated JS | JS source kB/s |
|---|--:|--:|--:|
| `manual` | 76 B | 76 B | **10.9** |
| `serde` | 98 B | 98 B | 12.9 |
| `raw` | **30 B** | **116 B (3.9x)** | **15.5** |

A 30-byte binary meter frame is the smallest payload in the matrix and the
largest thing actually handed to the JavaScript engine. Under Tauri's
1024-byte `MAX_RAW_DIRECT_EXECUTE_THRESHOLD` an `InvokeResponseBody::Raw` is
rendered as a decimal JSON array and eval'd as `new Uint8Array([...]).buffer`,
so `raw` puts **42% more source through the parser than compact JSON** while
looking 2.5x smaller in the payload column. At 750 Hz the effect scales:
96.2 kB/s against 63.9.

`manual` beats `serde` by 16% on JS source (10.9 against 12.9 kB/s) and that is
the entire measurable benefit of "pre-serialized compact payloads" - occupancy
is identical to within noise and every handler cost is under the 1 ms clock
floor. It is worth having because it is nearly free to implement, not because
anything in this matrix was struggling.

### Coalescing to 60 Hz: rejected as stated, and it costs latency

This is the clause that measured worst. The uncoalesced arms run the producer
at the full 750 Hz worker rate - 192 kHz capture with 256-frame callbacks, the
worst case §35 has to survive - with no coalescing anywhere:

| | meter/s | occupancy | >20 ms gaps | echo RTT mean | producer CPU |
|---|--:|--:|--:|--:|--:|
| `channel-manual` (60 Hz) | 60 | 1.7% | 140 (3.8%) | **10.5 ms** | 0.16% |
| `uncoalesced` (750 Hz) | 750 | **1.6%** | **71 (1.9%)** | **2.8 ms** | 0.75% |
| `uncoalesced-raw` (750 Hz) | 750 | 1.6% | 86 (2.3%) | 3.0 ms | 0.79% |

Sending 12.5x more messages produced **no additional main-thread occupancy,
fewer long frames, and a quarter of the delivery latency**, with zero loss
across 45,000 messages. The receive side simply was not the bottleneck that
coalescing was invented to protect.

The latency direction is the part worth explaining, because it is
counter-intuitive and it is consistent. `control-nodraw` is coalesced at 60 Hz
and draws nothing: 1.4 ms. The coalesced arms that draw: 8.7-13.0 ms. The
uncoalesced arms, which also draw: 2.8-3.0 ms. So neither the message rate nor
the drawing explains it alone - it is the interaction. The plausible mechanism
is that an eval'd message waits for a main-thread turn whose cadence is set by
painting, so at 60 Hz arrivals each message waits a large fraction of a frame,
while at 750 Hz there is always work pending when a turn comes. **I did not
isolate this with a controlled experiment**, so it is recorded as a
reproducible observation across five arms and a hypothesis, not as a mechanism.
Either way the practical conclusion does not depend on the explanation:
coalescing sends bought nothing and cost roughly 7 ms of delivery latency.

What coalescing *should* mean is one **paint** per animation frame, which every
drawing arm already does by reading the latest state in its rAF callback
rather than painting per message. That is a receive-side policy and it is free.

### The producer thread is free either way

D8's dedicated OS thread costs **0.14-0.19% of a core** coalesced and
**0.75-0.79% at 750 Hz**. Send p99 is <=64 us and tick lateness p99 is 256 us
with absolute scheduling. There is no throughput argument for coalescing on
the send side either; the producer is not the constraint at any rate tested.

That figure is time *inside* `send()`, and it is not the whole cost of sending.
`webview.eval()` queues work that the tauri process's **GTK main thread** then
dispatches, and external sampling puts that thread at **18% of a core** during
the soak. The producer thread is genuinely cheap; the send is not free, it is
just paid somewhere this bench was not looking. Anything needing a real budget
should be measured with `cpu-sample.sh`, not from the producer's own figures.

### The 30-minute soak on the recommended configuration

One arm, `soak-recommended` - channel transport, `manual` encoding, coalesced,
`OffscreenCanvas` worker - for 1800.3 s. 180,000 messages (108,000 meter,
54,000 waveform, 18,000 position). **Zero send errors, 108,000 of 108,000
received, zero sequence gaps.**

Everything the matrix measured held for half an hour, and some of it improved:

| | 60 s matrix | 30 min soak |
|---|--:|--:|
| main-thread occupancy | 0.5% | 0.7% |
| mean draw | 0.05 ms | 0.08 ms |
| rAF/s (main) | 91.1 | 89.4 |
| gaps > 20 ms | 0.1% | 1.3% |
| gaps > 33 ms | 0 | **3** (in 160,856 frames) |
| echo RTT mean | 9.7 ms | 10.6 ms |
| worker max frame interval | 16.0 ms | **18.0 ms** |
| whole-app CPU | not measured | 88.7% → 85.6% of a core |

The worker's own report is the headline: **119,133 frames over 30 minutes,
mean interval 15.11 ms, maximum 18.0 ms.** Half an hour of continuous
rendering with a worst case 1.3 ms outside a 16.7 ms budget. Mean draw was
268 µs, max 3.0 ms. Main-thread draw cost *fell* over the run (92 µs → 79 µs,
JIT warm-up), and whole-app CPU was flat to slightly falling, so nothing is
accumulating work.

**The one thing that did not hold is memory.** Total RSS across the three
processes went **491 MiB → 538 MiB, +47 MiB, a steady +1.46 MiB/min** with no
plateau: the first-half slope is +1.43 MiB/min and the second-half slope
+1.23 MiB/min, so it decelerates slightly but does not stop. This answers the
question the soak was run to answer - the 476 → 509 MiB drift across the
13-arm matrix was **time-based, not per-arm** - and replaces it with a better
one. Extrapolated, that is ~88 MiB/hour. A vinyl side is ~22 minutes (~32 MiB,
harmless); an afternoon of capturing and editing is not.

Two honest qualifications on that figure. It is the sum over the tauri process
and WebKit's network and web processes, and one sample caught a sixth process
transiently (543 MiB at 17.6 min, back to 520 MiB at 18.1 min), so the series
is noisy and not strictly monotonic even smoothed. And this bench's UI is two
canvases and a table - it allocates almost nothing per frame by design, with
preallocated decode targets, a fixed-size `Int16Array` ring and
allocation-free histograms - which makes application code the *least* likely
explanation and WebKit or tauri's own per-message bookkeeping the most likely.
**It is unexplained, and it is not diagnosed by this spike.**

Alongside it, one mild degradation: >20 ms frame gaps drift up from a mean of
33.5 per 30 s bucket in the first third of the run to 41.8 in the last third,
about 25%. Small, possibly the same underlying cause, and invisible to a user
at these magnitudes - but it is a drift rather than a stationary process, and
S2's soak taught the habit of checking for exactly that.

## Conclusions

**D6 as written is wrong in two of its three clauses, and silent on the one
thing that matters.** Proposed replacement:

> **D6 (revised).** Use Tauri 2 channels for meter, waveform and position -
> chosen for API shape (typed, per-invocation, no global event namespace), not
> for throughput, which is indistinguishable from the event bus at §35 payload
> sizes. Encode as hand-built compact JSON; **never** `InvokeResponseBody::Raw`
> for small frames, which Tauri eval's as a decimal JSON array and which is 42%
> larger on the wire than the JSON it replaces. **Do not coalesce sends** - the
> webview absorbed 750 Hz with zero loss, no added main-thread cost and a
> quarter of the latency. Coalesce *paints* instead: one read of latest state
> per `requestAnimationFrame`. **Draw the waveform incrementally and prefer an
> `OffscreenCanvas` worker**: a full-canvas main-thread redraw costs 29% of the
> main thread against 0.5% in a worker, and the worker is the only
> configuration measured here that never exceeded a 16 ms frame.
>
> The acceptance metric for UI work is **main-thread occupancy**, not fps:
> WebKitGTK does not pace `requestAnimationFrame` to vsync, so fps is not
> comparable across configurations.

Three consequences beyond the decision itself:

1. **The waveform renderer is a real work package, the IPC layer is not.**
   Transport and encoding can be chosen on ergonomics and revisited never.
   Budget the engineering into `OffscreenCanvas`, bucket stores and
   incremental redraw.
2. **Latency budgeting should assume ~10 ms of delivery**, not the sub-
   millisecond figure a "60 Hz push" suggests. For meters and playhead that is
   invisible; for anything where the user acts on what they see - punch-in,
   click-to-seek during capture - the round trip is the number to design
   against, and the authoritative timeline must stay in Rust.
3. **G0 can close on D6.** The decision is answerable on the measurements in
   hand for the Linux target, with the platform caveats below.
4. **One open item, carried forward: the webview's memory grows ~1.5 MiB/min
   and does not plateau in 30 minutes.** It does not block D6 - it is
   orthogonal to transport, encoding and rendering strategy, and it appears in
   a bench that allocates almost nothing per frame. But VCW is an application
   expected to stay open across a multi-hour session, so this needs isolating
   before G2: run the soak with the producer stopped to separate WebKit's
   baseline from per-message cost, then with `echo` disabled to test tauri's
   per-invocation bookkeeping.

## Honest limits

- **One platform.** Linux/x86_64/WebKitGTK 2.52.6 only. Tauri's own comments
  say both direct-execute thresholds were tuned against WebView2 v135 and
  macOS, and the clock and rAF behavior that shapes every conclusion here is
  WebKit-specific. **Windows/WebView2 and Pi 5 are unmeasured**, and the `raw`
  inflation finding in particular is threshold-dependent, so it should be
  re-checked rather than assumed portable.
- **The rig contaminates the jank baseline.** Two mirrored 3840x2160 outputs at
  59.98 Hz and 29.96 Hz. Absolute gap counts are not meaningful; only
  differences from `control-raf` and `control-idle` are.
- **One run per arm, 60 s each.** No repetition, so differences of the size
  seen between the transport arms (80 vs 140 gaps) carry no weight. The
  order-of-magnitude findings - 29.0% vs 0.5%, 3.9x inflation, 2.8 vs 10.5 ms -
  are far outside that noise; nothing else here should be read as ranked.
- **A 1 ms clock floor.** No client-side percentile is precise. Occupancy is
  computed from means, which the clamp leaves unbiased, and is corroborated by
  exact counts of frames over 1/4/8 ms. Handler costs are reported as
  "<=1 ms (floor)" because that is all the clock can say.
- **One-way latency was not measured and cannot be.** `echo` RTT is an upper
  bound that includes the JS-to-Rust return command.
- **The coalescing/latency interaction is unexplained.** Consistent across five
  arms, but not isolated by an experiment that varies rate with drawing held
  constant. Worth one follow-up arm if delivery latency ever becomes a design
  constraint.
- **`render-react-dom` understates React.** `setState` per frame schedules the
  reconcile *after* the rAF callback returns, so React's commit work falls
  outside the timed draw region. Its 1.7% occupancy is a floor. It also still
  drew the incremental canvas, so it is not a React-only waveform - it measures
  "canvas plus a DOM meter", which is the shape VCW would actually ship, not
  the cost of reconciling a waveform.
- **The bench does not measure its own CPU cost.** Every figure it reports is
  time inside a callback it controls - the rAF draw, `send()`, a message
  handler. Whole-application CPU had to be sampled from `/proc` by a separate
  script, and it is an order of magnitude larger than the occupancy figures
  suggest: **~89% of one core** for the recommended configuration on this
  desktop. The arms were never compared on that basis, so **whether the worker
  actually reduces total CPU relative to a naive redraw is unmeasured** - it
  demonstrably reduces main-thread *blocking*, which is a different claim. Any
  Pi 5 conclusion should rest on whole-app CPU, not on occupancy. This is the
  largest gap in the spike.
- **No audio device in the loop.** The producer is a synthetic xorshift source,
  so this measures the IPC boundary and the renderer, not capture. S1 covers
  the device side; the two have not been run together.
- **The UI is a bench, not an editor.** A real VCW window with a track list,
  transport, region overlays and a menu bar will not behave like two canvases
  and a table. These figures are a floor for the eventual application.
- **The memory growth is measured but not diagnosed.** The soak settled that
  it is time-based rather than per-arm (+1.46 MiB/min, no plateau in 30
  minutes), which is as far as this spike goes. What it does *not* establish
  is where it comes from, whether it plateaus eventually, or whether it scales
  with message rate - the obvious next experiments are a soak with the
  producer stopped and one with `echo` disabled. Until then, treat
  "~88 MiB/hour" as a measurement of this bench, not a property of Tauri.
