# WP-30 - the non-destructive processing chain

**Status:** DESIGN ONLY, 2026-10-16. Nothing is built. This is the precursor the
click/pop work asked for: the shape processing has to have before the first repair
algorithm is written, so that the algorithm lands in a place that already exists.
**Date:** 2026-10-16
**Requirements:** §4.1 (non-destructive), §9 (bit-perfect), §23/§24 (observations and
their provenance), §31 (editing), §33 (export from immutable blocks plus edit
instructions), §46 Phase 3 (non-destructive processing, click detection and removal,
optional normalization), §51 (playback equalization, the one stage that exists)
**Plan:** PROJECT_PLAN Phase 3, "non-destructive processing chain"
**Measured against:** nothing yet. There is no number in this note that was taken off
a machine, and the sections that would need one say so.

## Question

§4.1 is one of the four design principles and it is written as an absolute:

> Original captured PCM is immutable. Track markers, metadata, edits, fades, gain
> changes, and future processing instructions are project data and never modify the
> original capture.

"Future processing instructions" is the whole of WP-30. §46 names what they will be:
click detection and removal first, then optional normalization, with the playback
curves already done. The question is not how to remove a click. It is where the
instruction to remove one lives, who may veto it, what order the instructions run in,
and how the answer reaches an exported file without ever reaching a sample block.

The reason to answer it before writing a declicker is that the declicker is the second
stage, and one stage is not a chain. §51 shipped as `Option<Filter>` threaded through
two call sites. A second stage threaded the same way is three fields and six edits; a
third is a mess. The cost of deciding now is this note. The cost of deciding later is
a refactor of the playback path, which is the one path §4.2 says nothing may
compromise.

## 1. What is already non-destructive, and what §51 proved

Three things in VCW are already edit instructions rather than edits:

- **Track boundaries.** `track_boundaries` holds a frame position with its
  `confidence`, its `provenance`, the `sources` that agreed, the `evidence` behind it
  and a `locked` flag. Nothing about a boundary touches audio. §24 required this and
  it is the most complete precedent in the project.
- **The side and track model.** A track *is* two boundaries. `splitter::cut` reads the
  capture's blocks between them at export time.
- **The playback curve.** §51's characteristic is stored as a catalog id on the
  release, the side or the track, resolved outward, and applied by `Filter` at
  audition, render and export. It is never applied at capture.

The curve is the useful precedent because it is the only one that is *processing*
rather than *addressing*, and three things it got right are general:

1. **Two insertion points cover three places.** `Pump::fill` is shared by the live
   playback feeder and the synchronous render, so audition and render take the same
   path; `splitter::cut` is the third. Any stage that goes in at those two points is
   heard exactly as it will be written.
2. **The decision is resolved by the caller, not by the processor.** `filter_for`
   reads the project and hands `Pump::open` a built `Filter`. The pump does not know
   what a release is. That is what lets an audition pass `None` and hear the raw
   transfer with no special case anywhere below.
3. **An unstated decision means silence.** A project nobody has stated anything for is
   left exactly as it was. The default is not "sensible processing", it is none.

All three carry forward unchanged. What does not carry forward is the shape of
`Option<Filter>` itself, and the reason is in the next section.

## 2. Two kinds of stage, and the split is the design

A playback curve is defined entirely by its parameters. Three time constants and a
sample rate produce the same output from the same input, forever, on any machine. It
needs no analysis, stores nothing per sample, and can be re-derived from a sixteen-byte
row.

A declicker is not like that at all. It is two operations that happen at different
times, for different reasons, with different costs:

- **Detection.** Expensive, heuristic, run once over a side, and *wrong sometimes*. A
  snare hit and a click look alike to a threshold. This is the part a person has to be
  able to see and argue with.
- **Repair.** Cheap, deterministic, and only as good as the detection it was handed.
  Interpolating across forty samples is arithmetic.

Treating those as one streaming stage is the mistake available here. It would make the
detection run again on every audition, make the result depend on where playback
started, and give a person no way to say "that one is a snare, leave it". So:

> **A parametric stage is its parameters. A repair stage is a stored list of defects
> plus a cheap deterministic repair. Detection is analysis and writes observations; it
> is not part of the chain.**

This is exactly the division §23 and §24 already make for boundaries, and it is worth
being explicit that a click is the same kind of object as a track boundary: a thing an
analyzer proposed, at a position in a capture, with a confidence and a provenance,
which a person may confirm, move, veto or lock. A boundary is a point. A defect is a
span. That is the only difference, and it is why the table below looks like
`track_boundaries` with two frame columns instead of one.

| | parametric | repair |
|---|---|---|
| examples | playback EQ (§51), gain, normalization, the parked IEC rumble filter | declick, decrackle, dropout fill |
| defined by | a few parameters on a project row | a row per defect, in a table |
| cost of applying | per sample, constant | proportional to defects, near zero between them |
| needs analysis | no | yes, once, stored |
| a person can veto | the whole stage | the whole stage, or one defect |
| deterministic on seek | only after `reset` | yes, inherently |

That last row is not a footnote. An IIR filter carries state across fills and rings
when the input jumps, which is why `Filter` has to be reset on a seek. A repair stage
has no such state: its state is the table, which is addressed by frame. Seeking into
the middle of a side lands on the right repairs with no warm-up, and that is what makes
scrubbing over a declicked side honest rather than approximately honest.

## 3. Where a defect lives

A new table, proposed for schema v6. Nothing is built, and the column names below are
the argument, not an implementation.

```
defects
  defect_id    INTEGER PRIMARY KEY AUTOINCREMENT
  capture_id   INTEGER NOT NULL REFERENCES captures(capture_id)
  start_frame  INTEGER NOT NULL      -- frames from the start of the capture
  end_frame    INTEGER NOT NULL      -- exclusive, so an empty span is an error
  channels     INTEGER NOT NULL      -- bitmask: a groove defect is usually one channel
  kind         TEXT NOT NULL         -- 'click', 'crackle', 'dropout'
  confidence   REAL NOT NULL DEFAULT 1.0
  provenance   TEXT NOT NULL         -- which detector, or 'user'
  evidence     TEXT NOT NULL DEFAULT ''
  repair       TEXT NOT NULL         -- 'interpolate', 'mute', 'none'
  locked       INTEGER NOT NULL DEFAULT 0
  created_at   INTEGER NOT NULL
  updated_at   INTEGER NOT NULL
```

Four things about that are deliberate:

**It hangs off the capture, not off the track.** Frames index a capture. A defect found
before the tracks were laid out survives re-detection, re-splitting and a side being
re-adopted, because none of those change what frame 4,812,330 is. Boundaries hang off a
side for a different reason, which is that a side is what a person edits.

**`channels` is a bitmask and not a boolean.** A vinyl click is physical damage to one
groove wall far more often than both. Repairing both channels when one was fine throws
away half of a good stereo image for no reason, and a mono fold would then hear the
repair twice.

**`repair` is a column and not a global setting.** Interpolation is right for a short
click and wrong for a forty-millisecond dropout, where it invents a melody. Storing the
choice per defect is what lets a person override one without changing the others, and
`'none'` is the veto: the row stays, so re-analysis does not propose it again and the
evidence for the decision is still readable.

**`locked` means the same thing it means on a boundary.** §24's rule that automatic
analysis may not move what a person confirmed is a property of the project, not of one
detector, and a second table with a second spelling of it would be the bug.

No `repairs` table and no stored repaired audio. The repair is computed at playback and
at export from the defect row and the immutable blocks, every time. It is tens of
samples; measuring that is in section 8, but the shape of the claim is that a repair
costs nothing next to reading a 64 KiB page.

## 4. Where the chain runs

The same two insertion points §51 uses, because they already cover the three places
audio leaves the project:

```
  sampleblocks  (immutable, §4.1)
        |
        v
    Reader::fill          stored-format bytes
        |
        v
  +-----------------------------------+
  |  Chain::process_stored            |   <-- Pump::fill, and splitter::cut
  |    1. repair stages               |
  |    2. parametric stages           |
  +-----------------------------------+
        |
        +---> Conversion::apply ---> device        (audition)
        |
        +---> fold to mono ---> narrow ---> encoder (render, export)
```

`Chain` replaces `Option<Filter>` in `Pump` and in `splitter::Item`, and carries the
same two methods `Filter` already has: `process_stored(&mut [u8], StorageFormat)` and
`reset()`. A `Chain` with nothing in it is the `None` case and must cost nothing, which
means the empty check happens before the loop and not inside it.

Three constraints on the shape that are not obvious:

**A repair stage needs to know where it is.** `process_stored` on a filter is
position-independent; on a repair stage it is not, because the buffer has to be matched
against frame-addressed rows. So the chain's process call takes the frame position of
the first frame in the buffer. `Pump` already tracks this for the cue; `splitter::cut`
knows the track's start frame. Adding the argument to the trait is the whole of it.

**A defect that straddles the edge of a cut span cannot be repaired from inside it.**
Interpolation needs samples either side of the damage, and at a track's first frame
there are none in the buffer. Two honest options: widen the read span by the longest
repair's context and discard the margin, or leave straddling defects alone and say so.
The first is correct and costs one extra block read per track; the second is one line.
This is the decision the first repair stage has to make, and it should make the first
one.

**Order within the chain is fixed by the code, not configurable.** Repairs first,
parametric second. The reason is physical: a click is damage to the groove, so it is
present in the signal in the same domain the stored samples are in, and VCW's curve is
something VCW adds afterwards. Repairing after the curve means repairing a signal that
an IIR has already smeared the impulse across, which makes the damage longer and
lower-contrast than it is. The same argument says a stage order setting would be a knob
for getting it wrong, so there is not one.

Note the honest form of that claim. If the signal chain already applied RIAA before
capture, which `captures.capture_eq = 'riaa'` records, then "before VCW's curve" is
still after the preamp's. VCW cannot undo what it did not do. The rule is that repairs
run upstream of everything VCW applies, which is the strongest statement the
architecture can make.

## 5. What leaves the building

§9 forbids claiming bit-perfect operation while anything is in force, and §51 already
implements that for one stage: a side played through a curve reports not bit-perfect,
and the exported file carries `VCW_PLAYBACK_EQ` and `VCW_PLAYBACK_EQ_APPLIED`.

Two stages make that a pattern rather than a pair of fields:

- The bit-perfect refutation asks the chain whether it is empty, not whether a filter
  is present. One predicate, and every future stage is covered by it the day it is
  added rather than the day somebody remembers.
- The export tags gain a `VCW_PROCESSING` listing the chain as applied, in order, as
  the stages name themselves. `VCW_PLAYBACK_EQ` stays, because the curve is a fact
  about the record and not only about what VCW did to it, and an archivist reading the
  tag wants to know which characteristic the transfer assumed.

A defect count belongs in that tag too: "declick: 412 repaired, 3 vetoed" is the one
line that tells somebody receiving the file what was done to it. This is the archival
audience the §51 note identified, and it is the same requirement: state what was
applied, in terms a person can check.

## 6. What a person sees

The design is only non-destructive if the person can see and reverse it, so three UI
consequences fall out of section 3 rather than being added later:

- **Defects draw on the waveform**, the way boundaries do. A dense cluster is itself a
  diagnosis: it usually means a scratch rather than dirt.
- **A/B is the empty chain.** Auditioning with processing off is `Chain::default()` at
  the call site, not a bypass flag inside each stage. The same mechanism gives the
  waveform panel a "hear it raw" that cannot drift out of agreement with what export
  will write.
- **Veto is per defect and reversible**, because `repair = 'none'` keeps the row. A
  destructive design would have no place to put that fact.

## 7. What is not decided here

**The detection algorithm.** Nothing in this note has been measured and no literature
has been read for it yet. What the architecture requires of the detector is only:
that it runs over a committed capture rather than the live path (§4.2), that it emits
spans with a confidence and a provenance, that it is re-runnable without disturbing
locked rows, and that it is honest about false positives, because the veto path above
exists for them. The algorithm itself is the click/pop work package and starts with a
literature pass the way §51 did.

**Where a repair stage lives.** `vcw-signal` is analysis and writes nothing, so
detection belongs there beside the boundary detectors. The repair is processing and
does not belong in `vcw-eq`, which is sixteen curves and a biquad. Probably a new
small crate; not worth naming before there is code in it. **Settled 2026-10-18:** it
is `vcw-process`, which step 3 created to hold `Chain`. It sits below both consumers
because `vcw-core` owns the audition path, `vcw-export` owns the render and export
paths, and `vcw-core` depends on `vcw-export` rather than the other way about.

**Normalization.** It is in §46 and it is parametric, so it fits the left-hand column
with nothing new required. It is also the one stage that needs a measurement over the
whole side before it can set its parameter, which makes it a third shape: parametric
to apply, analytic to decide. Worth noticing now, not worth designing now.

## 8. Not done, and in what order

1. Read the declick literature and pick a detector. Nothing here is blocked on the rest
   of this list; the rest of this list is blocked on it, because a chain with one stage
   in it is still not a chain. **Done, 2026-10-17:**
   [WP-30 - the declick detector survey](WP-30-declick-detector-survey.md). It picks an
   AR residual detector in `vcw-signal`, which answers section 7's question for the
   *detector* and not for the repair stage, and it adds a requirement this note does
   not have: a detection run reports what
   fraction of the side it flagged, because the published defaults flag 5 to 7 percent
   of a 78 rpm transfer and an LP is not that material.
2. Schema v6: the `defects` table, and the five places a migration touches.
3. `Chain`, replacing `Option<Filter>` at both insertion points, with the frame
   position argument. This is a refactor with no behavior change and should be a
   commit of its own, provable by the §51 conformance tests passing unchanged.
   **Done, 2026-10-18:** `vcw-process`, the small crate section 7 said would probably
   be needed and was not worth naming before there was code in it. It holds one
   `Option<Filter>` and a `process_stored` that reads the frame position it is given
   with nothing, which is the whole of the step: the plumbing a frame-addressed repair
   needs, threaded through `Pump` and `splitter::Item` on its own so that the thing
   that reads it arrives alone. `Pump::equalizing` went with it - it had no callers,
   and once a second stage exists an "is a curve in force" question answered from the
   chain's emptiness is answered wrongly.
4. The detector, writing defect rows, re-runnable, respecting `locked`.
   **Done, 2026-10-18:** `vcw_signal::declick` is the AR residual detector the survey
   recommended, written from the papers; `vcw_project::defect` stores what it finds;
   `vcw_core::declick` streams a capture through the one into the other; and `vcw
   declick <project>` drives it with the five parameters on the command line.
   Re-runnability is one function - `defect::replace` - because the three rules only
   hold together: a span this pass did not find goes, a locked row stays, and a span
   that lands on a locked row keeps the locked row's verdict. A dry run is the same
   code with the project opened read-only.

   Two of the five published defaults did not survive contact. `K=2` marks one sample
   in twenty-two of a Gaussian residual by construction, which is the survey's own
   "five to seven percent" and is unusable; `K=4` flags nothing on clean synthetic
   material and still finds every injected click. Oudre's model order of 12.5% of the
   frame cost six times what FFmpeg's 2% cost and found one more span than there was
   damage, so the cheap one won.

   **The re-derivation on real records, 2026-10-18.** WAV import made it possible the
   same day. Eight sides from `/data2/source_rips` - 4.5 hours, 1.11e9 frames, solo
   piano to dense noise, one of them at 192 kHz - were swept over `K` from 3 to 40,
   and 300 clicks of a known four-sample shape were injected into one of them at
   known frames, fifty each at +0, +3, +6, +9, +12 and +18 dB relative to the local
   RMS, with the uninjected side as the control. Four results:

   - **The residual is heavy-tailed on music, not Gaussian.** Span counts fall 1.1x
     to 2.4x per unit `K`, where a Gaussian tail falls about tenfold. There is no
     knee and no natural boundary; the threshold is a policy choice. The synthetic
     material that produced `K=4` had a Gaussian floor and so produced a cliff that
     does not exist on records. That is the methodological lesson: a detector
     thresholding a residual cannot be tuned on synthesized audio at all.
   - **Recall is not the binding constraint.** 100% of injected clicks at every
     amplitude up to `K=12`, 98% at `K=16`, 94% at 20, 79% at 24, 31% at 32, zero by
     40. False alarms fall far faster than recall over that whole range.
   - **`K=16` is the shipped value.** It is the lowest threshold that flags none of
     300 randomly chosen music positions, and it flags 0.0024% to 0.0116% of a side
     against `K=4`'s 0.31% to 5.6%. `K=12` keeps the last 2% of recall at twice the
     flagged fraction; 16 wins because the repair is destructive and unattended.
   - **`burst` and `order_percent` are inert.** Swept at `K=16` across five sides:
     `burst` from 1 to 50 does not change recall at all, and `order_percent` from 2
     to 12.5 changes it by one click in 300 for six times the work. Both keep their
     values. `burst` trades span count against flagged samples and should be
     revisited in step 5, when there is a repair to listen to across a fused gap.

   One performance finding fell out of it: the 192 kHz side analyzed at 0.9x real
   time against 14.4x for every 48 kHz side. `window_millis` is fixed in time, so the
   model order scales with the sample rate and the cost per second of audio scales as
   the square of it. A 192 kHz side takes longer to analyze than to play.

   Two things came out of writing it that were not in the plan. `Provenance` gained a
   variant and the kebab-case *reader* lived in `vcw-project` rather than beside
   `as_str`, so a declick row came back out of the database claiming to be a gap in
   the audio; `Provenance::parse` and `Provenance::ALL` now sit next to the names,
   with an exhaustive match in the test so that the next variant cannot repeat it. And
   the first version of the re-run test passed with the `locked = 0` clause deleted,
   because vetoing a span also moves its provenance to `user` and the delete is scoped
   by provenance - so the test now also pins a row the detector still owns, which is
   the only case that clause is for.
5. The repair stage, and the straddling-defect decision from section 4.
   **Done, 2026-10-18:** `vcw_process::repair` applies the stored rows on the way out,
   and section 4's decision went the way section 4 said it should - widen the read span
   by the longest repair's context and discard the margin.

   The margin is a `Context { before, after }` of slices borrowed from the caller's own
   read, one `split_at_mut` apart from the buffer between them, so nothing is copied and
   the stage stays stateless but for a forward cursor through the span list. The
   alternative shapes were both worse. A delay line inside the stage is uniform and
   correct and makes every caller prime it, discard twice the context and flush it with
   zeros, for a shifted output stream. Splitting the chain into a `repair()` call and a
   `parametric()` call moves the stage-order decision out to the call sites, which
   section 4 already calls a knob for getting it wrong.

   What the chosen shape buys is that **a track's first frame and a chunk seam are the
   same question**. There is no separate code path for the edge of a cut; `splitter::cut`
   and `Pump::fill` both over-read `Chain::context_frames` at each end of every buffer,
   rewind to it, and write only the middle. The over-read is about 6% of a 64 KiB chunk
   at 48 kHz, and the reader keeps its decoded block across the rewind, so re-reading the
   margin usually costs an index lookup.

   The margin has to be bounded or one pathological row makes every chunk in the file
   read the whole span around it. `MAX_GAP_MILLIS = 10` is what bounds it, and it is the
   same number as the musical answer: a curve drawn across more than 10 ms of missing
   groove is invention, and silence is the more honest failure. A span past the cap
   degrades to a mute keyed on the span's own length and never on the buffer, so the
   same damage cannot be interpolated in one chunk and muted in the next.

   The interpolator is cubic Hermite with Catmull-Rom tangents, and the first version of
   it was wrong in a way worth recording. Uniform Catmull-Rom places its four control
   points one *parameter step* apart, which here is one gap length, not one frame; reading
   p0 at `start - 2` and p3 at `end + 1` gives tangents scaled to the wrong interval, and
   the curve does not reproduce a straight line. Measuring the tangents across the whole
   gap instead - `m1 = (y2 - y0) / (gap + 1)` - is exact on a line and stays bounded on a
   long one.

   The test for it is a sawtooth, because a line is the one signal a cubic reproduces
   exactly: every repaired sample has a right answer that can be named, so "it sounds
   smoother" is not something the test can settle for. Damage goes on the track's first
   frame, mid-track, and across the seam between two export chunks, and the assertion is
   byte equality against the undamaged line. Forcing `context_frames` to zero makes the
   first-frame defect export as silence and fails it, which is the half of the check that
   says the margin is load-bearing rather than decorative.

   `burst` was to be revisited here, per step 4. It is not, yet: it changes which spans
   are fused and therefore how long a gap the repair draws across, and that is a question
   for a listener and the step 6 overlay, not for an assertion.
6. The waveform overlay and the per-defect veto.
7. `VCW_PROCESSING`, and moving the bit-perfect refutation onto the chain predicate.

## What this does not build

Anything. This is a note. The one thing it asks of the work before it is that §51's
two insertion points are not quietly made into three by whoever gets there first.
