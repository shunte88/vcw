# WP-31 - the archivist transfer log

**Status:** BUILT, 2026-10-19, in commit 840d68e - the same day the note was
written, because the open questions turned out to be smaller than they looked. The
user raised it on 2026-10-18 as a future task and the reason it was a note first is
the tag names: they are written into other people's files, and a tag name is the one
thing in an export that cannot be taken back. All five steps of the plan in §8 are
in, and the eleven keys have a three-backend test. What is not in: no CLI path sets
a capture's operator or transfer date - the window has the two boxes and
`vcw serve` has the route, and a CLI-only workflow has neither.
**Date:** 2026-10-19
**Requirements:** §23/§24 (evidence and its provenance), §29 (the vinyl data model),
§32 (metadata), §33 (export), §39 (settings), §42 (diagnostics, and what a bundle may
contain), §51 (playback equalization, which already records one link in this chain)
**Plan:** PROJECT_PLAN Phase 3, alongside the rest of the archival work
**Measured against:** nothing. There is no number in this note.

## Question

VCW already records what the *software* did to a transfer. §51 writes the curve, WP-30
step 7 writes the chain, and `captures` holds the host API, the device name and what
the OS said about the format. None of that says what the audio came out of.

An archival transfer is only citable if the chain that made it is on the record. A
catalog receiving a file wants to know which copy of the record was played, on what,
through what, by whom and when, because two transfers of the same pressing on two
cartridges are two different documents. That is the second audience the §51 note
identified and the user has concurred with: the sixteen curves make VCW an archival
tool, and an archival tool that cannot state its own signal chain is a tool an archive
cannot cite.

So: which facts, where do they live, and what are they called in a file.

## 1. The fields, and why there are eleven and not ten

The user's list is ten: physical item identifier, turntable, arm, cartridge and
stylus, phono stage, interface, operator, date, known condition, and any physical
signal path modification.

**Cartridge and stylus split.** A stylus is replaced on a schedule the cartridge body
outlives, and the hours on it are the single most common reason two transfers of one
record on one rig do not match. A field that holds "Ortofon 2M Blue" cannot also hold
"Shibata, 120 hours". Eleven fields:

| field | what it is |
|---|---|
| item | The physical item identifier: shelf mark, accession number, or whatever the holder calls this copy. |
| turntable | The deck. |
| tonearm | The arm, where it is not the deck's own. |
| cartridge | The cartridge body. |
| stylus | The stylus profile, and its hours where the operator tracks them. |
| phono_stage | The preamp that applied, or did not apply, a curve. |
| interface | The converter, as a person names the box. |
| operator | Who made the transfer. |
| date | When. |
| condition | The known condition of the copy, in the holder's own words. |
| modification | Any physical signal path modification: a rewired arm, a bypassed stage. |

`modification` is free text by necessity. There is no vocabulary for "the phono stage
is bypassed and the cartridge runs straight into a head amp", and inventing one would
produce a field nobody fills in correctly.

## 2. Three homes, chosen by what the fact is about

The temptation is to put all eleven in Settings, because a rig does not change between
sides and nobody should retype it per capture. That is right for seven of them and
wrong for the other four, and the test is simple: a fact about the rig belongs to the
rig, a fact about the copy belongs to the release, and a fact about the session
belongs to the capture.

**The rig, in Settings (§39), a new `transfer` section.** turntable, tonearm,
cartridge, stylus, phono_stage, interface, modification, and operator as the default.
Eight strings, all optional, all empty by default. Nothing in VCW reads them except
export.

**The copy, on `releases`.** item and condition. §29 makes a project one release, and
the copy being transferred is that release's physical instance; `releases.comments`
already holds "free text a person added about this copy: the pressing, the condition,
the shop", which is the same thought arrived at informally. Two new columns, and
`comments` stays what it is.

**The session, on `captures`.** operator, as an override of the Settings default.
That is one new column.

**The date is not a field anybody types.** `captures.started_at` is already Unix
seconds at stream start, which is the transfer date, more accurately than a person
retyping it. There is one exception and it is the reason the field is in the table at
all: a capture with `capture_mode = 'imported'` has a `started_at` of the import, not
of the transfer, and an Audacity project carried over from 2019 should not be dated
2026. So `captures` gains a nullable `transferred_on` that is NULL for a recorded
capture and set on an import or by hand.

That is three new columns and a Settings section, which is schema v7.

## 3. What is already recorded, and must not be recorded twice

Five facts about the chain are already in the project and none of them moves:

- `releases.playback_eq` is the characteristic the record was *cut* with (§51). It is
  a fact about the pressing, not about the rig, and it already exports as
  `VCW_PLAYBACK_EQ`.
- `captures.capture_eq` is what the signal carried when it reached the sound card. It
  is the phono stage's *effect*; `transfer.phono_stage` is the phono stage's *name*.
  Both are wanted and neither implies the other: a named preamp does not say whether
  it applied a curve, and "riaa" does not say which box applied it.
- `captures.host_api`, `captures.device_name` and `captures.os_report` are what the
  operating system said. `transfer.interface` is what the operator calls the hardware.
  "Scarlett 2i2 USB" and "Focusrite Scarlett 2i2 4th Gen" are the same box described
  by two parties, and an archivist cites the second while a diagnostician reads the
  first. The OS strings stay in the project and out of the tags.

## 4. What the tags are called

The open question was "which tag names, per container (Vorbis comments, ID3, MP4
freeform, AIFF)". It has a smaller answer than it looks: **one name per field,
written identically everywhere**, because `Tags::extra` already routes a freeform key
to all three tag formats. `crates/export/src/tagging.rs` turns each pair into a Vorbis
comment, an ID3v2 `TXXX` user text frame, or an MP4 `----:com.apple.iTunes:NAME`
atom, and a WAV and an AIFF both carry the ID3 tag in an `ID3 ` chunk. `VINYL_POSITION`
and `VCW_PLAYBACK_EQ` already go out this way and come back out of ffmpeg, Picard and
Music under the name they went in with.

Eleven keys, `TRANSFER_`-prefixed:

```
TRANSFER_ITEM          TRANSFER_PHONO_STAGE   TRANSFER_CONDITION
TRANSFER_TURNTABLE     TRANSFER_INTERFACE     TRANSFER_MODIFICATION
TRANSFER_TONEARM       TRANSFER_OPERATOR
TRANSFER_CARTRIDGE     TRANSFER_DATE
TRANSFER_STYLUS
```

The prefix follows a rule the codebase already keeps without having written it down:
**`VCW_*` is what VCW did to the audio** (`VCW_PLAYBACK_EQ`, `VCW_PROCESSING`), and a
domain-prefixed key is a fact about the record (`VINYL_POSITION`). The transfer chain
is a fact about the record, not a claim about the software, and naming it `VCW_` would
suggest VCW knows what cartridge was fitted. It does not. A person typed it.

**A readable name is the whole specification.** Confirmed by the user on 2026-10-19:
these are spelled the way `DISCOGS_RELEASEID` is, a word a person can read in a tag
editor, and not a reverse-DNS name or a registered term. The MP4 writer wraps it in
`com.apple.iTunes` because that container has no other way to carry a custom field,
and that is the one place the name is not literally what was written.

`TRANSFER_DATE` is ISO 8601, date only. A transfer has a date and not a timestamp, and
the minute a stylus was lowered is not a fact anybody cites.

**An empty field is not written.** `freeform()` already skips an empty value, which is
the behavior this needs rather than a new rule: a catalog reading `TRANSFER_STYLUS=`
has been told something false, where a catalog finding no such key has been told
nothing, which is the truth.

## 5. What is not written, and why

**No `bext` chunk.** The archival convention for a signal chain in a WAV file is the
`CodingHistory` field of the Broadcast Wave `bext` chunk, formatted per EBU R98, and
FADGI's embedding guidelines are built on it. VCW does not write one, for three
reasons that are about this application and not about the standard:

- It is WAV-only. VCW exports six containers and five of them have no `bext`. Writing
  the chain there and in `TRANSFER_*` keys puts the same fact in two places in one
  container and one place in the others, and the two will disagree the first time
  somebody edits a file.
- lofty does not write `bext`, so it is a chunk writer of our own against a format
  where the WAV header rules already have teeth.
- `CodingHistory` is a free-text log of coding *processes*, with `A=`, `F=`, `W=` and
  `T=` tokens. The eleven fields above are structured, and flattening structure into
  a text log to satisfy a reader that may not exist is the wrong direction.

If an ingest workflow turns up that needs it, it is a WAV-only addition generated from
the same eleven fields, and nothing in this design has to change to allow it. That is
the point of deciding the fields rather than the serialization.

**Not in `vcw bundle`.** The question was open; the existing design closes it. §42's
bundle is a diagnostic report whose stated rule is that everything it emits is a shape
or a count and never content, down to reporting a file name but never a path "because
a path carries a home directory". `TRANSFER_OPERATOR` is a person's name and
`TRANSFER_ITEM` is a shelf mark that identifies an institution's holdings. A bundle is
something a person sends to a stranger. Neither goes in it.

## 6. What a person sees

A `Transfer` panel in Settings holding the eight rig fields, which is where a person
fills them in once. Item, condition and the operator override belong on the screens
that already own those objects: item and condition with the release metadata, the
operator override with the capture. Nothing new is invented to hold them.

The per-capture override is a text field that is empty when it agrees with Settings,
not a copy of the default. An override that starts life pre-filled is an override
nobody notices is wrong.

## 7. What is not decided here

- **Whether the rig is one rig.** Settings holds one `transfer` section, so an
  operator with two decks edits it between transfers. A named list of rigs is the
  obvious next thing and is deliberately not built: it adds a selector, a default and
  a migration, and nobody has yet said they have two decks.
- **Stylus hours as a number.** `stylus` is one string today. Counting hours means
  knowing when a stylus was fitted and summing captures, which is a feature, not a
  field.
- **Stylus hours as a number**, above, and nothing else. The third thing that was
  open here - whether `TRANSFER_*` needs registering with anybody or spelling as a
  reverse-DNS name - was closed by the user on 2026-10-19: a readable tag name is
  sufficient, as `DISCOGS_RELEASEID` already is. Vorbis comment keys are not
  registered with anybody, and if a convention ever emerges these are eleven strings
  in one function.

## 8. Not done, and in what order

1. Schema v7: `releases.item`, `releases.condition`, `captures.transferred_on`,
   `captures.operator`. A migration touches five places and seven once a column is
   removed: the DDL, the migration, the DDL parser's expectations, the generated
   `docs/SCHEMA.md`, the Python reader's column allowlist, and the wind-back fixture,
   which has three copies.
2. `Settings.transfer`, with its ts-rs bindings and its entries in the i18n catalog.
3. The Settings panel, and the two existing screens gaining their fields.
4. `tags()` in the splitter grows the eleven keys, reading Settings for the rig and
   the project for the rest, with the capture's override winning over the default and
   `started_at` filling the date where `transferred_on` is NULL.
5. One export test asserting that a filled chain comes out of all three tag backends
   under the same names, and that an empty field writes no key at all.

## What this does not build

Anything. This is a note. The one thing it asks of the work before it is that nobody
writes a transfer field into `releases.comments` in the meantime, because that is a
free text field a person is already using for the same thought and separating them
afterwards is guesswork.
