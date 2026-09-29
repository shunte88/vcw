# S5 - Audacity project format probe

**Status:** complete. AUP3 and AUP4 both decoded and verified against a real corpus.
**Date:** 2026-09-22, AUP4 delta and rate correction 2026-09-24 · **Requirement:** REQUIREMENTS §12, §13 · **Plan:** PROJECT_PLAN §4 (S5), decision D1
**Artefact:** [`spikes/aup-format-probe/probe.py`](../../spikes/aup-format-probe/probe.py)

## Question

D1 commits us to a native `.vcw` that is an AUP4 *superset*, with Audacity
**import only** - we never write a file Audacity has to read. The spike had to
establish whether that import is actually buildable, because the project
document is an undocumented binary blob, not XML on disk.

## Method

The corpus is 25 real vinyl rips from `/data2/vinyl_rips`, 10 MB–774 MB,
produced by Audacity 3.7.x over the life of the existing workflow - not
synthetic fixtures. On 2026-09-24 five of them were opened in Audacity 4.0.0,
converted by Audacity itself and saved as `.aup4`, giving **matched AUP3/AUP4
pairs of the same albums** - a far stronger test than unrelated AUP4 files,
because every difference is attributable to the conversion. The five span the
corpus deliberately: two at 48 kHz and three at 192 kHz, 451 MB to 4.7 GB, both
sample formats (float32 and int24), both page sizes (65536 and 4096), and one
heavily clip-split project alongside four single-clip ones. The decoder was derived from the file bytes alone; no
Audacity source was consulted, so nothing here inherits Audacity's GPL.

The acceptance test is deliberately unforgiving: **every byte of every document
must be consumed.** A tag-length grammar that is even slightly wrong
desynchronises within a few records and throws. Partial credit is not
available, so 30/30 clean parses is strong evidence the grammar is right rather
than merely plausible.

## Result

```
30 parsed, 0 failed          (25 AUP3 + 5 AUP4)
elements: project 30, effects 60, tags 30, tag 58, wavetrack 60,
          waveclip 132, sequence 132, waveblock 40438, envelope 132,
          labeltrack 29, label 250, thumbnail 5, controlpoint 16
dangling block references: 0        orphan sample blocks: 0
```

Documents reconstruct to readable XML, and every `waveblock/@blockid` in every
project resolves to a row in `sampleblocks` with nothing left over.

**The import is tractable, for both versions.** D1 stands; no re-plan needed.

## The format

A `.aup3` is an SQLite database, `application_id` `0x41554459` (`"AUDY"` - note
it is *not* `AUD3`, so the magic does not encode the version). **AUP4 keeps the
same `application_id`**, so the magic identifies neither the version nor the
generation; only `user_version` does, and it is a packed dotted quad:
`0x03070000` = 3.7.0.0 for AUP3, `0x04000001` = 4.0.0.1 for AUP4. A reader must
switch on `user_version`, never on the magic or the file extension.

Page size is 65536 on recent files and 4096 on older ones; both occur in the
corpus, so a reader must not assume either.

Tables: `project`, `autosave`, `sampleblocks`, `sqlite_sequence`, plus
`project_history` in AUP4 only. `autosave` is empty in a cleanly closed project
and carries the same blob shape for a session that was not saved - that is where
a crashed Audacity's work lives.

### Document encoding

`project.dict` maps small integer ids to element and attribute names; `project.doc`
is a flat record stream referencing them, so no name is ever repeated. All strings
are UTF-32LE and **all lengths are in bytes, not characters**.

```
dict := 00 04                                  two-byte prologue, invariant
        ( 0F id:u16 nbytes:u16 utf32le[nbytes] )*

doc  := record*
   01 id:u16                                   start element
   02 id:u16                                   end element
   03 id:u16 nbytes:u32 utf32le[nbytes]        attribute, string
   04 id:u16 value:i32                         attribute, 32-bit signed
   05 id:u16 value:u8                          attribute, byte / bool
   06 id:u16 value:u32                         attribute, 32-bit
   07 id:u16 value:u64                         attribute, 64-bit
   08 id:u16 value:u32                         attribute, 32-bit
   0A id:u16 value:f64 digits:i32              attribute, double + precision hint
   0C nbytes:u32 utf32le[nbytes]               character data
   10 id:u16 nbytes:u32 bytes[nbytes]          attribute, binary blob  (AUP4)
```

`0x06` and `0x08` carry identical payloads and are used interchangeably for the
same attribute - `sampleformat` appears under both within the corpus. They are
presumably distinct C++ overloads upstream; a reader can treat them alike.
The `digits` field on `0x0A` is a formatting hint; `0xFFFFFFFF` means default.

`0x10` is **new in AUP4** and is the only new record in the whole format: a
length-prefixed opaque byte string, carrying binary payloads the old
string/number tags could not express. Note the payload length is a genuine byte
count here and needs no ×4 correction - unlike `0x03`, this is not UTF-32.

Tags `0x00`, `0x09`, `0x0B`, `0x0D`, `0x0E` never occur in either version.
**The importer must reject them, not guess a width** - a wrong guess silently
desynchronises the stream and yields plausible garbage rather than an error.
That property is what made the AUP4 delta cheap to find: the AUP3 grammar did
not mis-parse the new files, it stopped dead at `tag 0x10 at 1237 (name
'data')`, naming the offset and the attribute.

### Audio storage

```sql
CREATE TABLE sampleblocks(
  blockid INTEGER PRIMARY KEY AUTOINCREMENT, sampleformat INTEGER,
  summin REAL, summax REAL, sumrms REAL,
  summary256 BLOB, summary64k BLOB, samples BLOB);
```

Blocks are **mono**: one channel, `maxsamples` = 262144 samples = 1 MiB at 4
bytes per sample. A stereo track's two channels interleave their block ids
(1,3,5,… and 2,4,6,…) in a single autoincrement sequence. This vindicates the
per-channel block layout S2 measured - the two decisions agree.

Summaries are `(min, max, rms)` f32 triplets: `summary256` covers 256 samples
per triplet (12288 bytes for a full block), `summary64k` covers 65536 (48
bytes). Overhead measured on a 774 MB project: 8 MiB of summaries against 708
MiB of samples, **1.1%**.

`sampleformat` is `(bytes_per_sample << 16) | type_code`:

| value | | meaning |
|---|---|---|
| `0x00020001` | 131073 | int16 |
| `0x00040001` | 262145 | int24, stored in 4 bytes |
| `0x0004000F` | 262159 | float32 |

**There is no 32-bit integer format.** REQUIREMENTS §8 asks for one, so this is
a genuine conflict and the reason D1 chose an AUP4 *superset* rather than
literal AUP4. We add a format code in unused numeric space; Audacity never sees
our files, so nothing breaks.

### Envelopes

The envelope point encoding was unexercised until the fifth conversion produced
one, so it is recorded here rather than left open:

```
envelope  numpoints:u32
          ( controlpoint  t:f64  val:f64 )*
```

`t` is seconds from the clip start and `val` is the gain multiplier, `1.0` being
unity. `numpoints` is authoritative for the child count. Nothing new in the
grammar is needed - `controlpoint` is an ordinary element with two `0x0A`
attributes - but a reader that skipped `envelope` because every AUP3 project in
the corpus had `numpoints="0"` will now meet a populated one.

### Stereo

Stereo is **two sibling `wavetrack` elements**, the left carrying
`channel="0" linked="3"` and the right `channel="1" linked="0"`. It is not one
element with two channels. A clip-split project carries many `waveclip` /
`sequence` pairs per track - one corpus file has 19 clips per channel.

**Confirmed at WP-20 across all 30 files, both generations**: always exactly two
`wavetrack` elements, always that pair of attribute values, always in that
order. The importer still orders channels by `@channel` where the values are a
permutation of `0..n` and falls back to document order otherwise, because
"always" over 30 files is not a guarantee about the 31st.

## AUP4 - the delta, measured

Four albums were opened in Audacity 4.0.0, converted automatically and saved,
giving matched AUP3/AUP4 pairs of the same material. Two are 48 kHz, two are 192
kHz; the largest is 4.7 GB. The headline: **the delta is small, and the audio
layer is untouched.**

### The conversion is lossless on the audio layer

Not "looks the same" - byte-identical. Hashing every `sampleblocks` row in
`blockid` order, samples separately from format, per-block min/max/rms and both
summary pyramids:

| album | rate | format | page | blocks | `blockid` range | `samples` hash | meta+summaries hash |
|---|---|---|---|---|---|---|---|
| Atomos | 48000 | float32 | 65536 | 1342 -> 1342 | 1..1342 unchanged | **identical** | **identical** |
| Tomorrow's Harvest | 48000 | float32 | 65536 | 1354 -> 1354 | 1..1354 unchanged | **identical** | **identical** |
| King Buffalo Demo | 192000 | float32 | 65536 | 2010 -> 2010 | 1..2010 unchanged | **identical** | **identical** |
| Ohm | 192000 | float32 | 65536 | 4426 -> 4426 | 1..4426 unchanged | **identical** | **identical** |
| simples_test | 192000 | **int24** | **4096** | 456 -> 456 | **73..2005** unchanged | **identical** | **identical** |

Nothing is resampled, reformatted or repacked. **`sampleformat` 262145 (int24)
survives** and so does a 4096-byte **page size** - Audacity 4 does not normalise
either, so a converted file inherits whatever its AUP3 had. And `blockid` values
are preserved exactly, including simples_test's sparse, non-contiguous 73..2005
range: ids are neither dense nor 1-based, and the conversion does not renumber
them.

So an importer that only wants audio and clip boundaries can treat AUP3 and AUP4
as the same format behind one `user_version` check. **The migration risk R12 was
aimed at the document, and the document is where all the change landed; the part
we actually depend on did not move.**

### What changes in the document, exhaustively

Rather than spot-checking chosen attributes, every attribute in both documents
was keyed by element path plus sibling index and compared. Out of 1,647 to 8,941
shared attributes per pair, **8 to 22 differ**, and every one falls into five
groups:

| group | attributes | audio-bearing? |
|---|---|---|
| version stamps | `project/@version` 1.3.0 -> 2.0.0, `@audacityversion` 3.7.5 -> 4.0.0 | no |
| metadata reordered | `tag/@name`, `tag/@value` swap position, same content | no |
| **track colour assigned** | `wavetrack/@colorindex` 0 -> 1, 3, 4 or 5, per project | no |
| editor state | `project/@sel0`, `@sel1`, `wavetrack/@isSelected` | no |
| **envelope points created** | `envelope/@numpoints` 0 -> 1 on 16 of 38 envelopes | see below |
| **f64 re-rounding** | `waveclip/@trimLeft` on 2 of 38 clips | yes, negligibly |

Two of those deserve care, because they are the only places the conversion is
not purely additive on data we read.

**`trimLeft` re-rounds.** Two clips in simples_test moved from
`7.798630208333336` to `7.798630208333333`: a difference of 2.7e-15 s, which at
192 kHz is **5.1e-10 of a sample**. It cannot change which sample a clip starts
on, and no other attribute in any pair drifted. But it means the correct claim
is narrower than "unchanged": **the audio blocks are byte-identical, while
audio-bearing f64 attributes are preserved to within a ULP.** An importer diff
or a fixture test must compare these with a tolerance, not with `==`.

**Envelope points appear where there were none.** simples_test's AUP3 has 38
envelopes, all `numpoints="0"`. Its AUP4 has 16 of them carrying one
`controlpoint` each - and every single one is `val="1.0"`, unity gain, at a `t`
near a clip boundary. So the conversion materialises no-op envelope anchors
rather than changing any gain. Harmless to play back, but an importer that
renders envelopes must not assume a point means the user drew one, and must not
assume an AUP3 project with no points converts to an AUP4 project with no
points.

### `waveblock/@length`: a free integrity check

AUP4 adds one attribute to every `waveblock`: `length`, the block's sample
count. It is present on all 5,664 waveblocks across the five AUP4 files and it
**matches `length(samples) / bytes_per_sample` from `sampleblocks` in every
single case**, 100% of 5,664.

That is worth more to us than it looks. It lets the importer validate the
document against the audio *before* reading a byte of sample data, and their sum
cross-checks `sequence/@numsamples` (351,478,980 = 2 x 175,739,490 for Atomos,
i.e. two channels). `probe.py` now performs this check and reports `len_ok=N`.
AUP3 has no equivalent - its `waveblock` carries only `blockid` and `start`.

### What the conversion costs on disk

| album | AUP3 | AUP4 | delta | doc blob |
|---|---|---|---|---|
| Atomos | 1,423,900,672 | 1,424,097,280 | +196,608 (3 x 64 KiB) | 41,137 -> 101,873 |
| Tomorrow's Harvest | 1,435,959,296 | 1,436,286,976 | +327,680 (5 x 64 KiB) | 42,111 -> 150,809 |
| King Buffalo Demo | 2,132,606,976 | 2,132,934,656 | +327,680 (5 x 64 KiB) | 59,249 -> 132,362 |
| Ohm | 4,697,686,016 | 4,698,013,696 | +327,680 (5 x 64 KiB) | 126,708 -> 246,472 |
| simples_test | 451,219,456 | 451,411,968 | +192,512 (47 x 4 KiB) | 24,070 -> 106,812 |

The AUP4 grows by a handful of pages over its AUP3 original and nothing more,
independent of project size. The document blob doubles to quadruples, which is
the thumbnail, plus the new view state, plus `waveblock/@length` on every block,
plus the `project_history` copy.

Conversion is **non-destructive**: Audacity writes a new `.aup4` and leaves the
`.aup3` intact beside it. That matters for us twice over - it is why matched
pairs exist to compare at all, and it means a user who converts has not lost the
file our importer already handles. It also means a converted album costs a
second full copy on disk, which for Ohm is another 4.7 GB.

### One new table: `project_history`

```sql
project_history(generation, saved_at, dict, doc)
```

Generation 1 is **byte-identical to the live `project` row**, both `dict` and
`doc`, in all five files. So it is a per-save undo/version log holding a
*complete* document each time, not a delta. Only one generation exists in each,
because each file has been saved exactly once since conversion. `saved_at` is a
Unix timestamp in seconds.

That has a cost implication worth flagging even though we never write these
files: the document carries a screenshot (below), so each save plausibly appends
another ~45–95 KiB *plus* a full document copy. Whether Audacity prunes the log
is not measurable from a single generation. For us it means one thing only: **an
importer must read `project`, not `project_history`**, and should say so
explicitly rather than relying on row order.

### One new record type, and it is a screenshot

Tag `0x10` appears exactly once per file, as `project/thumbnail/@data`: a PNG
between 45,551 and 93,357 bytes across the five. Every one is 3138×1628 - the
editor window size, not a fixed thumbnail size - and each is a literal
screenshot of that window: title bar, waveform, label track, ruler.

**Confirmed in the UI:** these render as the preview tiles in Audacity 4's
recent-projects list. That is what the blob is for, and it is stored at full
window resolution rather than downscaled to tile size.

For the *importer* this is pure noise: it must **skip it by length** and must
not try to interpret it. The write-up records it so nobody mistakes a 90 KB
opaque blob for audio data or a corrupt stream.

It is worth noting as a UX idea rather than a format finding, though. A
recent-projects list with real waveform previews is a cheap, high-value touch,
and VCW would get it more cheaply than Audacity does: S3 already renders
waveforms in a worker to an `OffscreenCanvas`, so a preview is a `toBlob` away
and needs no window screenshot. Not in scope for any current work package;
recorded here so it is not lost.

### Dictionary: 20 more names, and nearly all of them are UI

**The dictionary is per-project, not a format constant**: it holds only the
names a given document actually uses. The four single-clip albums go 61 -> 81
names, adding 25 and dropping 5. simples_test goes **59 -> 81**, adding 27,
because its converted envelopes bring in `controlpoint` and `val` which the
others never need. Compare dictionaries as sets of names, and never hard-code a
count or an id: **ids are assigned per file** and the same name can have a
different id in two projects.

**Added**, common to all five (25 names). All but three are view, selection or
spectrogram state:

- *view and selection:* `trackViewType`, `viewstate_hpos`, `viewstate_vpos`,
  `viewstate_zoom`, `isFocused`, `colorScheme`, `rulerType`, `snap_enabled`,
  `snap_type`, `snap_triplets`, `syncWithGlobalSettings`
- *spectrogram:* `scaleType`, `algorithm`, `windowSize`, `windowType`,
  `zeroPaddingFactor`, `frequencyGain`, `minFreq`, `maxFreq`, `range`
- *clip model:* `groupId`, `clipStretchToMatchTempo`
- **not UI state:** `length` (the per-`waveblock` sample count, see above) and
  `thumbnail` / `data` (the PNG). These three are the only additions that carry
  anything an importer cares about, and only `length` is useful.

Two more appear in simples_test alone, from its converted envelopes:
`controlpoint` and `val`.

**Removed:** `height1`, `minimized`, `minimized1`, `snapto`,
`preferred_export_rate` - the old per-track layout and snapping state, replaced
by the `viewstate_*` / `snap_*` set.

Two of the additions look alarming and are not:

- `groupId="4294967295"` is `0xFFFFFFFF`, a no-group sentinel. Track grouping is
  a new Audacity 4 feature and these projects use none of it.
- `clipStretchToMatchTempo="1"` reads like time-stretching is enabled, but
  `clipStretchRatio="1.0"` and `numsamples` is unchanged, so no stretch is
  applied. It is a mode flag, not a transform. An importer must read the ratio,
  never the flag.

### `labeltrack` now carries the full track attribute set

In AUP3 a `labeltrack` has 5 attributes. In AUP4 it has 20 - including `gain`,
`colorindex`, and the whole spectrogram parameter block, none of which mean
anything for a label track. Upstream has evidently unified `Track`
serialization. Harmless, but a parser that switches on "does this element have a
`gain` attribute" to decide what kind of track it is will now get it wrong.

Labels themselves gained one attribute, `isSelected`, which is UI state.

### `project/@version` moved, and metadata did not go anywhere

`project/@version` 1.3.0 → **2.0.0** and `audacityversion` 3.7.5 → **4.0.0**.
Both are strings in the document and are a second, independent version signal to
the `user_version` pragma.

One prior expectation is corrected: PROJECT_PLAN §3.2 guessed that AUP3's `tags`
table was gone in AUP4 and metadata had "presumably moved into the document".
There never was a `tags` *table* - metadata has always lived in the document as
`tags`/`tag` elements, and it still does, unchanged, in AUP4. Both albums carry
the same two tags (`ALBUM`, `GENRE`) with the same values.

**But the order changed**, in all four pairs that have tags at all (simples_test
has none) - AUP3 wrote `GENRE` then `ALBUM`,
AUP4 writes `ALBUM` then `GENRE`. Element order shifts elsewhere too: the
`thumbnail` element lands in a different position relative to its siblings from
one file to the next. Order is not stable across a conversion, so any
comparison, test or fixture diff must treat these as **sets, not sequences**.
That is the one AUP4 finding with a direct bearing on how we write the WP-20
regression test.

## The rate trap, and a correction to this document

The 2026-09-22 version of this write-up got this backwards, and it is worth
recording why, because the wrong version had already reached the plan's
acceptance criteria for WP-20.

The original claim was that `wavetrack/@rate` "lies" and `project/@rate`
"correctly says 192000". The rate disagreement was real and the 22-of-25 count
was right; the conclusion about which side to trust was inverted, on the
assumption that these vinyl rips were 192 kHz captures.

Three independent checks say otherwise, none of them relying on that assumption:

1. **Internal consistency.** `sequence/@numsamples` is corroborated by the
   stored audio: `sum(length(samples)) / 4` is exactly `2 × numsamples` in every
   stereo project, so the sample count is trustworthy. Divide it by each
   candidate rate and compare against the label extents, which are recorded in
   *seconds* and therefore carry the answer with no outside knowledge. For
   Atomos, `numsamples/48000` = 3661 s against a last label boundary at 3655.1 s
   - just inside, as it must be. `numsamples/192000` = 915 s, which the labels
   overrun by four times. Across the corpus **29 of 30 projects** agree with
   `wavetrack/@rate`; the one exception has no labels, so it cannot vote.

2. **External ground truth.** `/data2/source_rips` holds the same albums as WAV,
   whose headers carry the capture rate and whose frame counts are exact:

   | project | WAV header | WAV frames | `numsamples` | `wavetrack/@rate` |
   |---|---|---|---|---|
   | Atomos | 48000 | 175,739,490 | 175,739,490 | 48000 ✓ |
   | Tomorrow's Harvest | 48000 | 177,222,530 | 177,222,530 | 48000 ✓ |
   | King Buffalo | **192000** | 263,232,536 | 263,232,536 | **192000** ✓ |

   Frame counts match to the sample, and the header rate matches the *track*
   attribute in every case - including King Buffalo, one of the three projects
   where both attributes happen to agree.

3. **The attribute does not vary.** `project/@rate` is `192000.0` in all 30
   files, across five years of rips at two different capture rates. A field that
   never changes is not describing the content. `wavetrack/@rate` does vary, and
   varies correctly. The tell was in the original data and I read past it.

**So `wavetrack/@rate` is authoritative and `project/@rate` is an editor
preference** - most likely the default rate for new projects, sitting alongside
the `preferred_export_rate` that AUP4 dropped.

The consequence is the reverse of the original warning: an importer that trusts
`project/@rate` plays 22 of these 25 rips at **four times speed**, silently. The
corrected rule is in trap 1 above, and `probe.py` now reports `project_rate` and
`track_rates` as separate fields with a `rate_disagrees` flag, so the two can
never again be collapsed into one number by a caller.

## Traps the importer must handle

1. **`project/@rate` is not the sample rate.** In 22 of the 25 AUP3 projects
   `project/@rate` says `192000.0` while `wavetrack/@rate` says `48000.0`.
   **`wavetrack/@rate` is the correct one** - see
   [the rate trap](#the-rate-trap-and-a-correction-to-this-document) for the
   evidence. `project/@rate` is a stored *editor preference*, not a property of
   the audio; it reads exactly `192000.0` in all 30 files regardless of what
   they contain. **Authoritative source is `wavetrack/@rate`**; treat
   `project/@rate` as UI state and ignore it.
2. **Two tags for one type.** Accept `0x06` and `0x08` identically.
3. **Byte lengths, not character counts.** A UTF-32 length read as characters
   under-reads by 4× and desynchronises.
4. **Page size varies** - 4096 and 65536 both present.
5. **`autosave` may be populated** on a project Audacity did not close cleanly.
   Importing `project` alone silently discards that session's work; detect the
   case and tell the user rather than quietly losing it.
6. **The magic does not carry the version.** `application_id` is `"AUDY"` for
   both AUP3 and AUP4. Switch on `user_version` (`0x03070000` vs `0x04000001`),
   not on the magic and not on the file extension.
7. **`0x10` lengths are real byte counts**, not UTF-32 byte counts. It is the
   one length field in the format that needs no reasoning about characters -
   which makes it the one place a reader who has internalised trap 3 will
   over-correct.
8. **Read `project`, not `project_history`.** AUP4's history table holds a
   complete document per save and its generation 1 is byte-identical to the
   live row, so a careless `SELECT ... LIMIT 1` may work by accident on a
   freshly converted file and silently return a stale document later.
9. **Order is not stable.** An AUP3 → AUP4 conversion reorders `tag` elements
   with no change in content. Compare metadata as sets; never assert on
   document order in a test or fixture diff.
10. **`labeltrack` carries `wavetrack`'s attributes in AUP4** - `gain`,
    `colorindex`, spectrogram parameters and all. Dispatch on the element name,
    never on which attributes are present.
11. **`clipStretchToMatchTempo="1"` does not mean stretched.** Read
    `clipStretchRatio`; the flag is a mode, the ratio is the transform.
12. **Sample blocks are shared, so reference counts matter.** simples_test has
    **532 `waveblock` elements referencing 456 distinct blocks**, one of them
    three times: clip splitting and copy/paste share storage rather than
    duplicating it. A reader that assumes one block per reference will
    mis-count, and anything that *frees* a block because one clip stopped
    referencing it will corrupt another clip. Only a clip-split project reveals
    this; the four single-clip albums are all 1:1.
13. **`blockid` is sparse and not 1-based.** simples_test runs 73..2005 for 456
    blocks, and the conversion preserves the gaps rather than renumbering.
    Never derive a count from a range, and never assume ids start at 1.
14. **Check `waveblock/@length` when it is there, and do not require it.** It is
    AUP4-only and matched `sampleblocks` exactly in all 5,664 cases, so it is
    worth validating against; its absence means AUP3, not corruption.
15. **The dictionary is per-file.** Names present and the ids assigned to them
    both vary between projects - **four sizes across the 30 files: 55, 59, 61
    and 81** (55 is `OWS20.aup3`, 59 is `simples_test.aup3`, 61 is the other 23
    AUP3s, 81 is all five AUP4s). Resolve names through the file's own dict
    every time; never cache an id across files or hard-code one.
16. **f64 attributes may re-round across a conversion.** `trimLeft` shifted by
    2.7e-15 s in two clips. Compare timing attributes with a tolerance, or in
    samples after multiplying by the rate, never with `==`.
17. **`envelope` can gain unity control points on conversion.** An AUP3 project
    with `numpoints="0"` throughout became an AUP4 project with 16 envelopes
    carrying one `val="1.0"` point each. A point does not imply the user drew
    one, and an all-unity envelope should be treated as absent.
18. **Open with `mode=ro`, not `immutable=1`.** Every project in this corpus is
    in WAL journal mode. `immutable=1` tells SQLite to ignore the `-wal`
    sidecar, so a project with uncheckpointed content would silently read as a
    stale database. The corpus's `-wal` files are all zero-length, so nothing
    was missed here, but that is luck rather than a guarantee. (`immutable=1`
    remains the right choice only for read-only tooling over files already
    known to be checkpointed, because it avoids creating a `-shm`.)
19. **`waveclip/@offset` is the sequence origin, not the audible start.** This
    is the trap the importer spent the most care on. The audible span of a clip
    is
    `[offset + trimLeft, offset + numsamples / rate - trimRight]`, so
    `offset` locates sample 0 of the *stored sequence* on the timeline and the
    first audible sample sits `trimLeft` seconds later. A reader that treats
    `offset` as the start of the audio places every trimmed clip early by its
    own trim. Two consequences: a trim must be converted to samples by
    rounding, not truncating, and **a clip's audible start almost never falls
    on a 262,144-sample block boundary** - which is why WP-20 re-blocks the
    timeline rather than adopting `sampleblocks` rows.
20. **Document order is not offset order.** Clips must be sorted by offset
    before they are read, and the sort cannot be assumed to have happened
    upstream.
21. **Most projects carry almost no metadata.** Across all 30 files: **GENRE in
    28, ALBUM in 28, YEAR in 1, ARTIST in 1**, and two files (`simples_test` in
    both generations) have no `tags` element content at all. An importer that
    needs an artist to name a file will be without one in 29 cases out of 30,
    so the naming template has to cope and the metadata lookup of §28 is the
    real source of that field rather than the project file.

## Incidental finding, and it matters

**24 of 25 existing rips are stored as float32, not integer.** Only
`simples_test` is int24. The current Audacity workflow has been converting the
ADC's integer sample words to float and storing those - every one of these rips
is one lossy conversion away from what the converter actually produced.

That is a direct, evidenced argument for REQUIREMENTS §8's bit-perfect capture
path, and it reframes the existing library: these files are good masters but
they are not bit-perfect captures, and re-ripping is the only way to make them
so. Worth deciding deliberately rather than discovering later.

## Open questions

- **All five AUP4 files were converted from AUP3 rather than created natively.**
  Rate is covered (48 and 192 kHz), format is covered (float32 and int24), page
  size is covered (65536 and 4096), size is covered (451 MB to 4.7 GB), and both
  single-clip and 19-clips-per-channel layouts are covered. Two gaps remain:
  1. **A second save of an existing `.aup4`**, which is the only way to see
     `project_history` generation 2 and settle whether each save appends a full
     document plus a fresh thumbnail (i.e. whether these files grow per save)
     or whether the log is pruned.
  2. **A project created natively in Audacity 4**, to separate "what AUP4 is"
     from "what an AUP3 conversion produces". The converted files may be
     carrying AUP3-shaped defaults that a native project would not - and the
     unity control points the conversion invented are a concrete hint that
     conversion output is not identical to native output.
- **Resolved.** The `envelope` point encoding was unexercised until the
  simples_test conversion invented 16 points; it is documented above. Still
  unseen: an envelope with more than one point, or with a `val` other than
  `1.0`, so the interpolation between points is still inferred rather than
  measured.
- The `0x00 0x04` dict prologue is invariant across all 30 files **including
  the five AUP4**, which weakens the "format-version marker" guess considerably - the
  format version did change and the prologue did not. Its meaning stays
  unconfirmed; the decoder asserts it rather than interpreting it.
- **Does Audacity 4 prune `project_history`?** If not, a long-lived project
  accumulates a full document and a ~90 KiB screenshot per save. Only affects
  us as an import-time surprise (a very large `project_history` in a file we
  read), but worth knowing before WP-20 sets any size expectations.
- **Why does the conversion invent unity envelope points on 16 of 38 clips but
  not the other 22?** The `t` values sit near clip boundaries, which suggests
  the new clip model anchors envelopes at edges, but the selection rule is not
  established. It costs us nothing (all-unity envelopes are no-ops) so it is
  recorded rather than chased.

## Test fixtures - a problem with a clean answer

The corpus cannot be committed: the smallest project is 271 MB. CI still needs
genuine Audacity bytes, not something we wrote ourselves, or the import test
only proves our encoder agrees with our decoder.

The grammar makes a surgical shrink possible without re-encoding anything.
Records are self-delimiting, so dropping all but the first few `waveblock`
records per channel is a byte-slice deletion, and correcting `numsamples` is a
fixed-width `u64` patched in place. Delete the corresponding `sampleblocks`
rows, `VACUUM`, and the result is a few-MB project whose every surviving byte
was written by Audacity. That belongs in WP-20, not here.

## Next - done at WP-20, 2026-09-28

Port the grammar to Rust in the importer work package with the corpus as the
regression test: parse every file, require full byte consumption, require zero
dangling block references. The Python probe stays as the oracle the Rust
implementation is diffed against.

**All of that happened.** `crates/import` reads all 30 projects byte-complete
with zero dangling references and is diffed against `probe.py --json` file by
file, the shrink described above is `vcw_import::fixture` and produced the three
committed fixtures, and the landing turns a project into a capture. The one
thing the port added to the format's documentation is trap 19, which is written
up above because it changed the design: `offset` is the sequence origin, so an
adopted sample block would need a sample offset the schema has no room for, and
the audio is re-blocked instead. See `docs/STATUS.md` for the work package.

The corpus is now the regression set for both versions, and the rate rule the
Rust importer must implement is `wavetrack/@rate`, with `project/@rate` ignored.

Concrete follow-ups:
1. Save one existing `.aup4` a second time, for `project_history` generation 2,
   and create one project natively in Audacity 4. Both are cheap and neither
   blocks WP-20.
2. Build the fixture shrinker described above (WP-20). The `0x10` record is
   self-delimiting like everything else, so dropping the thumbnail is a clean
   byte-slice deletion - which makes AUP4 fixtures *smaller* than AUP3 ones.
3. Find or make a project with a non-empty `envelope`, and one with a populated
   `autosave`, so both paths are exercised.
4. Create a project natively in Audacity 4 rather than by conversion.
