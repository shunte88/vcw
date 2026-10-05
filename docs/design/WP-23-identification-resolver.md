# WP-23 - the identification resolver

**Status:** DESIGN. The crate is a four-module skeleton; this document is WP-23's
first deliverable.
**Date:** 2026-10-04
**Requirements:** §23 (evidence), §24 (boundary evidence), §26 (progressive
identification), §27 (identification engine), §28 (providers, pressings), §32
(metadata fields), §40 (offline, caching, rate limits) · **Plan:** PROJECT_PLAN WP-23,
"the intellectually hardest piece in the whole project and deserves a design document
before code"
**Depends on:** WP-21 (fingerprint worker), WP-22 (AcoustID, MusicBrainz, Discogs)
**Measured against:** `/data2/source_rips` - 62 real vinyl rips - the live AcoustID
service, and one real library album

## Question

§27 states it, and states it as the thing fingerprinting does *not* answer:

> Fingerprinting answers: what recording does this audio resemble?
> Identification answers: given all current evidence, what release, side and track are
> being recorded?

## What a new project already knows

The design turns on a fact about the workflow rather than about audio. A VCW project
starts from a record in somebody's hand, so project setup asks for what is printed on
it:

- the **artist** and the **release title**
- the **catalog number** off the label
- **mono or stereo**
- whether **RIAA equalization** is to be applied on playback and on export

Four of those five are release identity, stated by a person who is looking at the
object. That is the strongest evidence in the system and it arrives before the first
sample does. The resolver's job is therefore *not* to discover what the record is from
the audio. It is to turn a stated identity into a specific pressing, confirm the audio
is consistent with it, and lay the side out.

The catalog number is the lever. The schema already says why, in a comment written
before any of this was measured:

> the catalog number off the label: the one identifier a vinyl pressing reliably
> carries, and the one a person searches by

Artist and title identify a *work*; a catalog number identifies a *pressing*, which
is what §28 asks VCW to tell apart and what decides the track list, the side topology
and the durations.

## The resolution order

Cheapest and most reliable first. Each step is skipped if the step before it answered.

1. **Discogs by catalog number.** An exact `catno` match on a vinyl release is
   near-conclusive: it names the pressing, and with it the track list, the per-side
   topology and the durations. Discogs is first because its vinyl coverage, pressing
   granularity and catalog-number indexing are all better than MusicBrainz's.
2. **Discogs by artist and title**, where no catalog number was given or none
   matched. Resolves the work, leaves the pressing open, and several candidates is a
   normal and useful outcome at this stage.
3. **MusicBrainz by the same two fields.** Not a duplicate of step 2: MusicBrainz is
   where recording ids live, and a recording id is what makes a later AcoustID answer
   comparable to this release rather than merely plausible.
4. **AcoustID, from the audio, only if the release was not found at all.**

The ordering is the point. **AcoustID is the fallback, not the entry point.** Three
reasons, all measured:

- About **a third of `/data2/source_rips` does not resolve at MusicBrainz from artist
  and album at all** - small labels, non-Latin titles, self-releases. That population
  is exactly who needs a fallback, and it is a minority.
- A text lookup is one request. Identification from audio cost **63 to 138 requests**
  per record in the prototypes, because it has to search for its own alignment.
- A stated identity cannot be contradicted by a weak fingerprint. §26 requires that
  automatic identification never silently replaces what a person confirmed, and making
  the typed facts the *first* evidence rather than a late tie-breaker is how that
  becomes structural instead of a check somebody has to remember.

## What AcoustID is for, once a release is known

Not discovery. Two jobs, both of which need a track list to already exist:

**Confirmation.** Does this audio actually contain the record the person said it is? One
probe against the first track's cataloged duration answers it, and a release that was
chosen from three Discogs candidates can be confirmed or eliminated by one request.

**Layout.** A confirmed track start is a boundary, and §24 already lists **metadata
duration** and **release topology** among its seven boundary sources. This matters
because the detectors cannot supply a layout. Measured over the corpus with
`vcw fingerprint --tracks` at its default `--min-sources 1`:

```
2002 regions over 21 rips          median region 5.6 s
     0-10   s   1403   70.1%       a real track is 120-480 s
    10-30   s    347   17.3%
    30-60   s    118    5.9%
    60-120  s     65    3.2%
   120-300  s     57    2.8%
   300+    s      12    0.6%
```

Only 3.4% of regions are even track-shaped, and the agreement threshold does not rescue
it: on one ten-minute three-track side, `--min-sources 1` gives 16 regions with a 5 s
median, `2` gives a single 134 s region spanning all three tracks, and `3` gives
`no track boundaries were agreed`. Quiet passages in real music are indistinguishable
from between-track grooves to a silence detector. That is not a tuning oversight, it is
why §24 lists seven sources.

So the resolver is one of the detectors, and the cataloged durations are what it
detects with.

## The four numbers the audio path turns on

Measured 2026-10-04 against the live service. Three are not documented by AcoustID and
none were obvious.

| fact | measurement |
| --- | --- |
| The declared `duration` is a hard pre-filter, not a hint | One track's fingerprint matched at a declared 170 to 183 s and returned **zero results** at 166 s and at 190 s. The same bytes scored 0 declared as 120 s and 0.974 declared as the track's true 371 s. |
| The fingerprint must begin at the track's start | 0 s of offset scored 0.867, 5 s scored 0.868, 10 s scored 0.833, and **11 s scored nothing at all**. |
| The audio itself can be short | 30 s from the track start scored 0.840 and 60 s scored 0.867, against 0.868 for the whole track. |
| A vinyl transfer does match | 0.64 to 0.88 across the tracks of a real rip. A digital source of the same music scores 0.96 to 1.00 - 37/37 tracks of one library album at 0.956 to 1.000. |

Read together: **the audio is nearly irrelevant and the alignment is everything.** Half
a minute of a worn transfer is enough; being 11 s out is fatal. Which is why a known
track list is worth so much more than a better fingerprint, and why an earlier
conclusion that "vinyl does not match AcoustID" was wrong - every lookup behind it
declared the *window's* length and started mid-side, so every one was guaranteed to
return nothing whatever the audio was.

S4's finding still holds and is not in conflict: boundary error costs at most 0.064 BER
in *fingerprint* terms. AcoustID's index lookup is not a BER comparison.

### Where a track actually starts

The offset of a confirmed start from its cataloged cumulative position is **small and
does not accumulate**. Measured across a 17-track double LP: 2, 4, 6, 8, 10 s, with no
trend. Pressed durations match cataloged ones closely and the gaps are absorbed.

Two consequences, both learned the hard way:

- **Do not carry a cursor.** A running cursor that advances by duration after each
  track never recovers from a miss: 5 of 17 tracks, against 9 of 17 for the same
  algorithm with a luckier cursor.
- **Do not fit a slope.** A least-squares fit of `offset = lead_in + gap * index` over
  five noisy confirmations claimed 2.84 s per track and was **27 s out by track
  thirteen**, outside the 10 s tolerance, so it failed every track on the second disc.
  The median of a constant predicted track C4's start to **the second** - it matched at
  exactly `cumulative + 8 s`, step +0.

So: predict `cumulative(track) + median offset`, re-estimate the median as
confirmations arrive, and search a small window around it. Nothing is extrapolated and
nothing accumulates.

One caveat recorded as a known unknown: a probe at 469.0 s returned nothing where
469.7 s scored 0.851 on the same track. A sub-second shift should not matter at a 10 s
tolerance, and chromaprint's ~0.124 s frame step is the obvious suspect - the index
needs exact subfingerprint hashes, so the phase of the frame grid may matter for a
marginal match. Unverified, and parked.

## The evidence model

Four modules, as scaffolded, and the split is the design: evidence is collected without
being judged, candidates are scored against it, and the resolver commits to one, so a
wrong identification traces back to the evidence that caused it instead of vanishing
into one opaque number.

**`evidence`** - a flat list of facts, each with its source. A fact is a statement
about the record (`Artist`, `Album`, `Catalog`, `Label`, `Year`, `TrackCount`,
`SideSeconds`, `Channels`) or about the audio (`Recording { id, score, at }`). A source
is where it came from: stated by a person, a provider, or the signal. Nothing here
scores anything.

**`candidate`** - a release from a provider, plus how it accounts for each fact:
agrees, disagrees, or is silent. The three-way verdict is load-bearing. A release that
is *silent* on the catalog number is merely unsupported; one that *disagrees* about
the track count is contradicted, and must be able to lose to a candidate with less
support rather than more.

**`confidence`** - the weights, and the thresholds below which the application asks
rather than asserts. Stated facts outweigh provider agreement; an exact catalog match
outweighs an artist-and-title match; identifications accumulate, because §26 says
"multiple identified tracks may constrain the likely release".

**`resolver`** - picks one, or declines to. Three outcomes and no fourth: resolved with
its reasons, ask the person with a ranked shortlist, or nothing found. Declining is a
first-class result; a wrong release silently written over a typed catalog number is
the one failure mode worth designing against.

## What it writes

Nothing directly. It emits observations (§23) which existing plumbing consumes:

- `IdentificationObservation` per confirmed track: recording, release, score, and the
  fingerprint it came from.
- `BoundaryObservation` per confirmed start, with provenance naming **fingerprint
  transition** or **release topology** (§24). A confirmed start with a known duration
  implies the next boundary too.

The `releases` row already holds everything a resolved release needs - `catalog`,
`label`, `country`, `musicbrainz_id`, `discogs_id`, `discs`, `numbering` - including the
`confirmed` flag that §26's rule hangs on. Two setup facts have nowhere to live yet:
**mono/stereo** and the **playback and export equalization intent**. `captures.capture_eq`
(schema v3) records what a capture *arrived* with, which is the input to that decision
and not the decision; a schema addition is needed, and is called out here rather than
smuggled in.

## Cost, and what bounds it

The main line is one or two HTTP requests for a whole record: a catalog lookup, and a
release fetch for the track list. Confirmation adds one probe per track at most, and
one probe is one fingerprint plus one request. AcoustID publishes 3 requests per second
and VCW's limiter runs at 1, so a side is seconds of background work on the main line
and a minute or two on the audio fallback.

- **Progressive and interruptible** (§26). Naming the record is most of the
  user-visible value and it happens first; confirmations publish as they land.
- **Every probe is cacheable** on `(fingerprint, declared duration)`, which is already
  the shape of §40's cache key, and the fallback re-probes the same audio at different
  durations often enough for that to matter.
- **Offline it does nothing and says so** (§40). `Error::Offline` is an ordinary
  answer, and a side identified only by what a person typed is an ordinary side.

## What this does not solve

- **Records that are not in any index.** A Winged Victory For The Sullen's *Atomos*
  returned nothing on every track at every position, and `/v2/track/list_by_mbid`
  confirms its recordings have no fingerprints submitted. The resolver must say "not in
  the index" and not "not identified".
- **Pressing ambiguity without a catalog number.** Artist and title cannot choose
  between a 1980 UK pressing and a 2020 reissue with different mastering. Nothing in
  the audio reliably can either. This is a case for asking.
- **Boundary precision.** A confirmed start is accurate to the probe step, not to the
  groove. Good evidence for WP-24 to refine, not a replacement for it.
- **Side extent.** Two faces can share one capture and nothing here says where one
  face ends. The release topology constrains it; WP-24 gets better input than it had.

## Prototypes

Outside the repo, because they probe live services and are measurements rather than
tests:

- `/data2/vcw-scratch/album-probe.py` - the control. Already-split per-track files,
  where both parameters are free. 37/37 at 0.956 to 1.000.
- `/data2/vcw-bench/align-probe.py` - the baseline. Takes the track list as given and
  walks a cumulative cursor.
- `/data2/vcw-bench/anchor-probe.py` - identification from audio alone, end to end.
- `/data2/vcw-scratch/dense-probe.py` - the offset model: constant against fitted
  slope, which is how the slope was caught being 27 s wrong.
