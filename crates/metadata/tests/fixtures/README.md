# Provider fixtures

Recorded provider responses, replayed through `vcw_metadata::fixtures::Recorded` so
that every provider test runs with no network (§40) and gives the same answer on a
machine that has never had one.

## MusicBrainz - real, captured 2026-09-26

Fetched from the public API with the user agent VCW itself sends. No credential is
involved; MusicBrainz asks only that a client identify itself.

| File | Request |
| --- | --- |
| `musicbrainz_search_amber_vinyl.json` | `GET /ws/2/release?query=artist:"Autechre" AND release:"Amber" AND (format:"Vinyl" OR format:"12\" Vinyl" OR format:"7\" Vinyl" OR format:"10\" Vinyl")&fmt=json&limit=5` |
| `musicbrainz_release_amber_1994.json` | `GET /ws/2/release/bd5b1270-7468-47f0-9c9a-928199f9e4ad?inc=recordings+artist-credits+labels+release-groups+genres&fmt=json` |

Chosen because they exercise the things that are easy to get wrong and hard to
invent: the search returns **two pressings of the same record** (1994 `WARPLP25`
and the 2016 reissue `WARPLP25R`, which is what §28's "distinguish vinyl pressings"
means in practice), and the release is a **2xLP whose second disc carries sides C
and D**, with the side letters living in each track's `number` field and the
release's own `genres` array *empty* so that the release-group fallback is
exercised.

## Discogs - hand-built from the shape its client parses

Discogs requires a token (§39), so these are constructed rather than captured: the
field names and nesting are those VRipr's client reads in
`/data2/vripr/src/metadata/discogs.rs`, which was written against the live API. The
tracklist deliberately includes the cases that parser handles - a heading row, a
letter-run position (`AA`), and an unlabelled numeric tracklist - because those are
the ones a naive reader gets wrong.

## AcoustID - real, captured 2026-10-04

| File | Request |
| --- | --- |
| `acoustid_lookup_silverside.json` | `POST /v2/lookup` with `trackid=<mbid>`, `meta=recordings releases tracks` |
| `acoustid_lookup_no_match.json` | `POST /v2/lookup` with a real 198 s fingerprint off `boc.vcw` |
| `acoustid_fingerprint_400s.txt` | not a response: a real 400 s fingerprint, 9,856 base64 characters |
| `musicbrainz_recording_silverside.json` | `GET /ws/2/recording/<mbid>?inc=artists+releases+media&fmt=json` |

**The populated one was captured by `trackid` rather than by fingerprint, and that
is the finding, not a shortcut.** AcoustID's index is submitted overwhelmingly from
digital releases. Every fingerprint VCW can make on this machine comes off a record
- a different master, cut at a slightly different speed, through a different
cartridge - and the service matches none of them: `acoustid_lookup_no_match.json`
is what a real side actually gets back, and it is a successful request. So the
populated path was recorded by asking the same endpoint the same question a
different way, by MusicBrainz recording id, which returns the identical
`results[].recordings[]` shape a matching fingerprint would.

It is a 12" single, which is why it is worth keeping: 13 releases under one
recording, two of them records, and the vinyl medium's track `A3` is where §28's
side letter lives. `acoustid_fingerprint_400s.txt` exists to hold a claim the
module doc makes - a real side does not fit in a request line - and the test on it
fails if the file is ever replaced with something shorter than 8 KB.

Re-record any of these by hand; nothing in the build fetches them.
