/*
 *  vcw.d.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  GENERATED FILE - do not edit.
 *
 *  Produced by `vcw_contract::bindings::typescript` from the Rust types in
 *  `crates/contract/src/`, which are the authority for anything this file does
 *  not say. Regenerate with:
 *
 *      VCW_BLESS=1 cargo test -p vcw-contract --test bindings
 *
 *  The types are §35's three groups: `Wire` is every event the core publishes,
 *  the request payloads are what a UI may ask for, and the rest are view
 *  models - what a UI is given to draw, with the units already resolved.
 *
 * MIT License
 *
 * Copyright (c) 2026 Stue Hunter
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */

// ----------------------------------------------------------------------
// Events (§35)
//
// Everything the core publishes, as one discriminated union tagged on
// `kind`. The tag values are the names `vcw_core::Event::name` declares,
// which is what makes a `switch` over this union exhaustive and stable.
// ----------------------------------------------------------------------

export type Wire = { "kind": "phase-change",
/**
 * Where it was.
 */
from: PhaseName,
/**
 * Where it is now.
 */
to: PhaseName, } | { "kind": "armed",
/**
 * The project being recorded into.
 */
project: string,
/**
 * How the device is actually running, as a line of text.
 */
negotiated: string,
/**
 * Every field asked for and not granted.
 */
divergences: Array<string>,
/**
 * Whether the operating system confirmed the format independently.
 */
verified: boolean, } | { "kind": "recording-position",
/**
 * Frames committed, per channel.
 */
frames: number,
/**
 * The same thing in seconds.
 */
seconds: number, } | { "kind": "meter-update",
/**
 * Peak, RMS, hold and the clip latch, per channel, in dBFS.
 */
meter: Meter, } | { "kind": "output-meter-update",
/**
 * Peak, RMS, hold and the clip latch, per channel, in dBFS.
 */
meter: Meter, } | { "kind": "track-detected",
/**
 * Where it is, in frames.
 */
frame: number,
/**
 * The same thing in seconds.
 */
seconds: number,
/**
 * Whether a track starts or ends here.
 */
edge: EdgeName,
/**
 * How much to trust it, 0 to 1.
 */
confidence: number,
/**
 * Which analysis said so.
 */
provenance: ProvenanceName, } | { "kind": "capture-warning",
/**
 * A short stable slug, for a consumer that wants to branch.
 */
code: string,
/**
 * A sentence, for a consumer that wants to show it.
 */
detail: string, } | { "kind": "capture-finished",
/**
 * The capture's row id in the project.
 */
captureId: number,
/**
 * Frames committed, per channel.
 */
frames: number,
/**
 * Finalised, or interrupted if anything was lost.
 */
state: CaptureStateName,
/**
 * The ring's four counters as of the end.
 */
diagnostics: Diagnostics,
/**
 * Whether the capture can honestly be called bit-perfect.
 */
bitPerfect: boolean, } | { "kind": "auditioning",
/**
 * The capture being played.
 */
captureId: number,
/**
 * What is being auditioned, with its extent.
 */
scope: string,
/**
 * How the output stream is actually running.
 */
opened: string,
/**
 * What happens to the stored samples on the way out, or
 * `"straight through"`.
 */
conversion: string,
/**
 * Every field asked for and not granted.
 */
divergences: Array<string>, } | { "kind": "playback-position",
/**
 * The frame now playing, absolute within the capture.
 */
frame: number,
/**
 * The same thing in seconds.
 */
seconds: number, } | { "kind": "playback-refused",
/**
 * The capture that was asked for.
 */
captureId: number,
/**
 * What was asked for, in frames rather than seconds: the case that
 * matters is the one where the project could not be read.
 */
scope: string,
/**
 * Why it was refused.
 */
reason: string, } | { "kind": "playback-finished",
/**
 * The capture that was playing.
 */
captureId: number,
/**
 * Frames delivered to the device.
 */
frames: number,
/**
 * Gaps the listener heard.
 */
underruns: number,
/**
 * Whether what reached the converter was what is in the project.
 */
fidelity: string,
/**
 * Whether that was a confirmation rather than an absence of evidence.
 */
bitPerfect: boolean, } | { "kind": "command-refused",
/**
 * The command that was refused.
 */
command: string,
/**
 * The phase the transport is still in.
 */
phase: PhaseName,
/**
 * Why.
 */
reason: string, } | { "kind": "command-rejected",
/**
 * The command that does not apply.
 */
command: string,
/**
 * The phase it does not apply in.
 */
phase: PhaseName, } | { "kind": "status",
/**
 * Where the transport is.
 */
phase: PhaseName,
/**
 * Frames committed, per channel.
 */
frames: number, } | { "kind": "export-progress",
/**
 * Which file, 1-based.
 */
index: number,
/**
 * How many there are.
 */
of: number,
/**
 * Where it is being written.
 */
path: string,
/**
 * Frames written across the whole export so far.
 */
frames: number,
/**
 * Frames the plan expects in total.
 */
total: number, } | { "kind": "export-finished",
/**
 * Audio files written.
 */
files: number,
/**
 * Cover files written beside them.
 */
covers: number,
/**
 * Frames written, summed across the files.
 */
frames: number,
/**
 * Bytes written, which is the number a progress dialogue should show
 * against the free space it had.
 *
 * Named for what it counts rather than just `bytes`: the contract's
 * own test bans a field called `bytes`, because that is what a payload
 * of audio would be called and §35 forbids one crossing.
 */
bytesWritten: number, } | { "kind": "export-failed",
/**
 * What went wrong, as a sentence.
 */
reason: string,
/**
 * Files written before it stopped.
 */
written: number, } | { "kind": "detection-finished",
/**
 * The sides examined, as letters.
 */
sides: Array<string>,
/**
 * Boundary rows written or updated.
 */
boundaries: number,
/**
 * Track rows created.
 */
tracks: number,
/**
 * Decisions the adoption policy turned down.
 */
rejected: number,
/**
 * Decisions a boundary the operator had already settled accounted for.
 */
alreadySettled: number,
/**
 * How long the pass took, in seconds.
 */
seconds: number, } | { "kind": "detection-failed",
/**
 * What went wrong, as a sentence.
 */
reason: string,
/**
 * The side it was on when it stopped, if it had got that far.
 */
side: string | null,
/**
 * Sides finished before it stopped.
 */
completed: number, } | { "kind": "closed" };

export type PhaseName = "idle" | "armed" | "recording" | "paused" | "stopped";

export type EdgeName = "start" | "end";

export type ProvenanceName = "silence" | "spectral-change" | "hmm" | "fingerprint" | "metadata-duration" | "release-topology" | "user" | "unknown";

export type CaptureStateName = "recording" | "finalised" | "interrupted" | "recovered" | "unknown";

// ----------------------------------------------------------------------
// Commands (§35)
//
// Request payloads. `Request` is the whole set as one union; the Tauri
// shell exposes one command per verb and takes the payload types
// directly, which is the idiom and gives a better error.
// ----------------------------------------------------------------------

export type Request = { "command": "arm" } & Arm | { "command": "transport",
/**
 * Which verb.
 */
verb: Transport, } | { "command": "play" } & Audition | { "command": "seek" } & Seek | { "command": "move_marker" } & Marker | { "command": "place_marker" } & Placement | { "command": "delete_marker" } & Removal | { "command": "lock_marker" } & Lock | { "command": "edit_track" } & TrackEdit | { "command": "split_track" } & Split | { "command": "merge_tracks" } & Merge | { "command": "detect_tracks" } & Detect | { "command": "search_metadata" } & Search | { "command": "select_release" } & Selection | { "command": "export" } & Export | { "command": "new_project" } & NewProject | { "command": "save_settings" } & Settings;

export type Failure = {
/**
 * A stable slug: `not-armed`, `invalid-argument`, `not-wired`.
 */
code: string,
/**
 * One sentence, fit to show a person.
 */
message: string,
/**
 * The field at fault, where one command argument is to blame.
 */
field: string | null, };

export type Arm = {
/**
 * Device id as [`crate::view::Device::id`] gave it, or `null` for the
 * simulated source - which is how the transport is driven with nothing
 * plugged in.
 */
device: string | null,
/**
 * Project to record into, created if it does not exist.
 */
project: string,
/**
 * Sample rate to pin, in Hz.
 */
rate: number | null,
/**
 * Channel count to pin.
 */
channels: number | null,
/**
 * Sample format to pin: `s16`, `s24`, `s32` or `f32`.
 */
format: string | null,
/**
 * `exclusive` or `shared`. Defaults to exclusive, which is what §9 wants.
 */
mode: string | null,
/**
 * Ring capacity in milliseconds. `null` takes §10's default.
 */
ringMillis: number | null,
/**
 * What equalization the hardware upstream already applied (§51): `flat`,
 * `riaa` or `unknown`. `null` means unknown, which is what it stays until
 * somebody says otherwise.
 */
eq: string | null, };

export type Transport = "disarm" | "record" | "pause" | "resume" | "stop" | "reset" | "poll" | "shutdown";

export type Audition = { "scope": "whole" } | { "scope": "region",
/**
 * Where to start, in seconds.
 */
start: number,
/**
 * Where to stop, in seconds.
 */
end: number, } | { "scope": "track",
/**
 * The track's row id.
 */
trackId: number, } | { "scope": "boundary",
/**
 * The boundary's row id.
 */
boundaryId: number, };

export type Playback = { "verb": "play" } | { "verb": "pause" } | { "verb": "stop" } | { "verb": "seek" } & Seek | { "verb": "skip-forward" } | { "verb": "skip-back" };

export type Seek = {
/**
 * Seconds from the start of the capture.
 */
to: number, };

export type Marker = {
/**
 * Which boundary.
 */
boundaryId: number,
/**
 * Where to put it, in seconds.
 */
to: number,
/**
 * Whether to override §24's lock.
 *
 * Defaults to false, and a locked boundary is refused rather than moved.
 * Overriding claims the boundary as the operator's on the way past, since
 * whoever overrides a lock is the new author of that position.
 */
force: boolean, };

export type Placement = {
/**
 * Which side to place it on, as [`crate::view::Side::id`] gave it.
 */
sideId: number,
/**
 * Where, in seconds.
 */
at: number,
/**
 * Which way the audio crosses it.
 */
edge: EdgeName, };

export type Removal = {
/**
 * Which boundary.
 */
boundaryId: number, };

export type Lock = {
/**
 * Which boundary.
 */
boundaryId: number,
/**
 * What to set it to.
 */
locked: boolean, };

export type TrackEdit = {
/**
 * Which track, as [`crate::view::Track::id`] gave it.
 */
trackId: number,
/**
 * The title. Empty means untitled.
 */
title: string | null,
/**
 * The performer, where it differs from the release's.
 */
artist: string | null,
/**
 * The composer.
 */
composer: string | null,
/**
 * Free text.
 */
comments: string | null,
/**
 * The recording it was identified as.
 */
musicbrainzId: string | null,
/**
 * Whether a person has accepted this metadata (§26).
 */
confirmed: boolean | null, };

export type Split = {
/**
 * Which track to cut.
 */
trackId: number,
/**
 * Where to cut it, in seconds.
 */
at: number, };

export type Merge = {
/**
 * The earlier track, which survives.
 */
leftId: number,
/**
 * The later track, which is absorbed into it.
 */
rightId: number, };

export type Detect = {
/**
 * A side letter, or `null` for all of them.
 */
side: string | null,
/**
 * Whether to promote what it finds into tracks, or only record boundaries.
 *
 * Defaults to promoting, because §22's live analysis exists to give a
 * person tracks to correct rather than a list of candidates to approve.
 * A detection pass that only wrote boundaries would leave the track
 * editor empty on a side nobody had touched.
 */
promote: boolean, };

export type Region = {
/**
 * Where it starts, in seconds.
 */
start: number,
/**
 * Where it ends, in seconds.
 */
end: number, };

export type Zoom = {
/**
 * Which capture.
 */
captureId: number,
/**
 * Which channel, zero-based.
 */
channel: number,
/**
 * The first frame to draw.
 */
startFrame: number,
/**
 * One past the last frame, or `null` for the end of the capture.
 */
endFrame: number | null,
/**
 * How many columns to return, which is the canvas width in pixels.
 */
pixels: number, };

export type Search = {
/**
 * Artist.
 */
artist: string | null,
/**
 * Album title.
 */
album: string | null,
/**
 * Catalog number, which is what identifies a pressing.
 */
catalog: string | null,
/**
 * Barcode.
 */
barcode: string | null,
/**
 * `discogs` or `musicbrainz`. `null` asks every configured provider.
 */
provider: string | null, };

export type Selection = {
/**
 * `discogs` or `musicbrainz`, as [`crate::view::Candidate::provider`] gave it.
 */
provider: string,
/**
 * The provider's own id.
 */
id: string, };

export type Export = {
/**
 * Directory to write into. Created if it does not exist.
 */
into: string,
/**
 * `flac`, `wav`, `mp3` or `ogg`.
 */
format: string,
/**
 * `transparent`, `high` or `compact`, or `null` for `high`.
 *
 * Ignored by the lossless containers rather than refused by them, so a
 * panel can keep one value while the format changes under it.
 */
quality: string | null,
/**
 * `0` to `8`, or `null` for `5`.
 *
 * Ignored by everything but FLAC, for `quality`'s reason exactly.
 */
compression: string | null,
/**
 * A naming template, or `null` for the default.
 */
template: string | null,
/**
 * Side letters to export, or empty for every side that has tracks.
 */
sides: Array<string>,
/**
 * `none`, `embed`, `folder` or `both`.
 */
artwork: string | null,
/**
 * Whether to replace files that are already there.
 */
overwrite: boolean, };

export type NewProject = {
/**
 * File name without the extension, or `null` to derive one.
 */
name: string | null,
/**
 * Release artist, off the sleeve.
 */
artist: string | null,
/**
 * Release title.
 */
album: string | null,
/**
 * Catalog number, which is what actually identifies a pressing (§32).
 */
catalog: string | null,
/**
 * Whether this is a mono pressing. Folded to one channel on export only.
 */
isMono: boolean,
/**
 * Whether the RIAA curve is applied on playback and on export (§51).
 */
riaaEq: boolean, };

// ----------------------------------------------------------------------
// View models (§35)
//
// What a UI is given to draw. Units are resolved on the Rust side: dBFS
// rather than amplitudes, seconds beside frames, a side letter rather
// than a side index. §2 is why - each of those is a calculation, and a
// calculation in the view layer is application logic in the wrong place.
// ----------------------------------------------------------------------

export type Meter = {
/**
 * One entry per channel, in stream order.
 */
channels: Array<Levels>,
/**
 * Frames metered since the meter was made or last reset.
 */
frames: number,
/**
 * Whether any channel has clipped. Computed here so a clip indicator does
 * not need a reduce in the view layer.
 */
clipped: boolean, };

export type Levels = {
/**
 * Highest magnitude since the previous snapshot.
 */
peakDb: number,
/**
 * Root mean square over the meter's window.
 */
rmsDb: number,
/**
 * The hold needle: a recent maximum, falling.
 */
holdDb: number,
/**
 * Whether this channel has clipped since the latch was cleared.
 */
clipped: boolean,
/**
 * Samples at or beyond full scale since the latch was cleared.
 */
clippedSamples: number, };

export type Diagnostics = {
/**
 * Times the ring filled before the writer drained it.
 */
overruns: number,
/**
 * Times the device callback found no data ready.
 */
underruns: number,
/**
 * Frames known to be lost. Non-zero means not bit-perfect.
 */
droppedFrames: number,
/**
 * Stream errors reported by the host.
 */
streamErrors: number, };

export type Device = {
/**
 * The identity to select on and to persist. Never the name: §7's rule is
 * that names are neither unique nor stable.
 */
id: string,
/**
 * Human-readable name.
 */
name: string,
/**
 * `alsa`, `wasapi`, `coreaudio`, as CPAL spells it.
 */
host: string,
/**
 * How the device is attached: USB, PCI, Bluetooth.
 */
interface: string,
/**
 * What sits between us and the converter, as a line of text.
 */
transport: string,
/**
 * Whether it can capture.
 */
canCapture: boolean,
/**
 * Whether it can play back.
 */
canPlay: boolean,
/**
 * This host's default input.
 */
isDefaultInput: boolean,
/**
 * This host's default output.
 */
isDefaultOutput: boolean,
/**
 * Which of §8's rates it advertises for capture, ascending.
 */
captureRates: Array<number>,
/**
 * Which of §8's formats it advertises for capture.
 */
captureFormats: Array<string>,
/**
 * The channel counts it advertises for capture, ascending.
 */
captureChannels: Array<number>,
/**
 * Whatever went wrong while interrogating it.
 */
problems: Array<string>, };

export type Capture = {
/**
 * Row id, which is what every other command refers to it by.
 */
id: number,
/**
 * Frames recorded, per channel.
 */
frames: number,
/**
 * The same thing in seconds, at the rate it was recorded at.
 */
seconds: number,
/**
 * Sample rate in Hz.
 */
rate: number,
/**
 * Channel count.
 */
channels: number,
/**
 * Sample format, spelled as an arm request would spell it.
 */
format: string,
/**
 * How the device was opened: `shared`, `native` or `exclusive`.
 */
mode: string,
/**
 * `recording`, `finalised` or `interrupted`.
 */
state: string,
/**
 * The device it came off, where the row records one.
 */
device: string | null,
/**
 * The host API that opened it.
 */
host: string | null,
/**
 * Whether the operating system confirmed the format independently (§9).
 */
osVerified: boolean,
/**
 * Unix seconds at the start.
 */
startedAt: number,
/**
 * Unix seconds at finalization, or `null` while it is still running.
 */
finishedAt: number | null,
/**
 * Overruns, underruns, dropped frames and stream errors.
 */
diagnostics: Diagnostics, };

export type Side = {
/**
 * Row id, for commands that name a side.
 */
id: number,
/**
 * The side letter: `A`, `B`, `C`, `D`.
 */
letter: string,
/**
 * Which disc it is on, one-based.
 */
disc: number,
/**
 * `"a"` or `"b"`: which face of that disc.
 */
face: string,
/**
 * The capture holding its audio, where one has been recorded.
 */
captureId: number | null,
/**
 * A title, where the label prints one for the side.
 */
title: string | null, };

export type Track = {
/**
 * Row id.
 */
id: number,
/**
 * The side it is on.
 */
sideId: number,
/**
 * The side letter, so a flat list can be grouped without a second lookup.
 */
side: string,
/**
 * One-based number within the side.
 */
number: number,
/**
 * §29's position as it is presented: `A1`, or `1` under continuous
 * numbering. Resolved here because the rule is the release's, not the
 * track's.
 */
position: string,
/**
 * Where it starts, in seconds.
 */
start: number,
/**
 * Where it ends, in seconds.
 */
end: number,
/**
 * How long it is, in seconds.
 *
 * `end - start`, and carried rather than left to the caller on purpose: a
 * track length is the number a person reads off the screen and the number
 * `vcw list` prints, and two subtractions in two languages is how those
 * two come to disagree about a rounding.
 */
seconds: number,
/**
 * The frame it starts at.
 */
startFrame: number,
/**
 * The frame it ends at.
 */
endFrame: number,
/**
 * The boundary it starts at, for a command that moves or locks one.
 */
startBoundary: number,
/**
 * The boundary it ends at.
 */
endBoundary: number,
/**
 * Title, empty until something fills it in.
 */
title: string,
/**
 * Track artist, or `null` to take the release's (§32).
 */
artist: string | null,
/**
 * Composer, or `null` to take the release's.
 */
composer: string | null,
/**
 * Free text a person added.
 */
comments: string | null,
/**
 * The recording it was identified as (§25/§28).
 */
musicbrainzId: string | null,
/**
 * Whether a person has confirmed the metadata.
 */
confirmed: boolean, };

export type Boundary = {
/**
 * Row id, for a command that moves, locks or deletes one.
 */
id: number,
/**
 * The side it is on.
 */
sideId: number,
/**
 * The side letter, so a flat list groups without a second lookup.
 */
side: string,
/**
 * The frame it sits at, in that side's capture timeline.
 */
atFrame: number,
/**
 * The same position in seconds, divided by the capture's own rate.
 */
seconds: number,
/**
 * Which way the audio crosses it.
 */
edge: EdgeName,
/**
 * How much to trust it, in `0.0..=1.0`.
 */
confidence: number,
/**
 * What decided its position.
 */
provenance: ProvenanceName,
/**
 * Every provenance that reported it.
 */
sources: Array<ProvenanceName>,
/**
 * How many distinct detectors agreed, which is what §24's policy
 * thresholds on. Sent as its own field rather than left as
 * `sources.length`, because a person who placed the boundary reports zero
 * sources and is not less certain for it.
 */
agreement: number,
/**
 * Whether analysis may move it (§24).
 */
locked: boolean,
/**
 * Whether a track is bounded by it.
 *
 * `false` is the interesting case: a detected boundary that no track uses.
 */
promoted: boolean,
/**
 * The measurements behind it.
 */
evidence: Array<Measurement>, };

export type Measurement = {
/**
 * What was measured.
 */
name: string,
/**
 * The value, in whatever unit the name implies.
 */
value: number, };

export type Project = {
/**
 * Absolute path to the `.vcw` file.
 */
path: string,
/**
 * The file stem, which is what a person named it.
 */
name: string,
/**
 * Release title, empty until something fills one in.
 */
album: string,
/**
 * Release artist.
 */
albumArtist: string,
/**
 * Catalog number (§32), which is how a vinyl library is actually
 * indexed.
 */
catalog: string,
/**
 * Release year.
 */
year: number | null,
/**
 * How many sides the project has rows for.
 */
sides: number,
/**
 * How many tracks.
 */
tracks: number,
/**
 * How many captures.
 */
captures: number,
/**
 * Total recorded audio, in seconds.
 */
seconds: number,
/**
 * The size of the `.vcw` file on disk, which for a vinyl project is most
 * of what a person wants to know before opening it.
 *
 * `file_bytes` rather than `bytes`, and the longer name is load-bearing:
 * `no_pcm_crosses_the_boundary` bans a field called `bytes` outright,
 * because in this contract that word has only ever meant sample data. The
 * guard fired on the first draft of this type, which is the guard working
 * - the honest fix is to say which bytes, not to rename around the check.
 */
fileBytes: number,
/**
 * Last modification, in unix seconds.
 */
modified: number,
/**
 * Whether the release has a front cover stored.
 *
 * A flag and not the image. A cover is a megabyte or two and a library is
 * a hundred rows, so shipping the bytes with the listing would make
 * opening the browser cost more than opening a project. The flag is what a
 * table needs to decide between an image and a placeholder, and it is free
 * here because the walk already has the file open: `length(bytes)` reads
 * the row, not the blob.
 */
hasArtwork: boolean,
/**
 * A waveform small enough to be an icon: peak magnitude per column, in
 * `0..=1`, over the whole of the first capture. Empty when there is none.
 *
 * The tile view needs a picture of every project, and until a release is
 * assigned there is no cover to be one. The waveform is what the project
 * already is - a side of a record has a shape, and two rips of different
 * records never look alike - so it is the honest placeholder rather than a
 * repeated sleeve glyph.
 *
 * This is the same reader the real waveform uses at a tiny width, not a
 * second summary: `Shape::whole` over [`PREVIEW_COLUMNS`], served out of
 * the `sampleblocks_levels` covering index, which is 14 ms for a
 * 26-minute side and the reason this can ride on the listing at all.
 * One array and not three, because at this size min and max are mirror
 * images and rms is invisible.
 */
preview: Array<number>,
/**
 * Why the file could not be read, where it could not.
 */
problem: string | null, };

export type Release = {
/**
 * Release title.
 */
album: string,
/**
 * The artist credited for the release as a whole.
 */
albumArtist: string,
/**
 * Release year, where it is known.
 */
year: number | null,
/**
 * Normalized genres, in order (§32).
 */
genres: Array<string>,
/**
 * Record label.
 */
label: string,
/**
 * Catalog number off the label, which is what identifies a pressing.
 */
catalog: string,
/**
 * Country of the pressing.
 */
country: string,
/**
 * Barcode, where the sleeve carries one.
 */
barcode: string | null,
/**
 * Composer.
 */
composer: string,
/**
 * Whatever a person wrote about this copy.
 */
comments: string,
/**
 * How many discs the release has.
 */
discs: number,
/**
 * `"alpha"` or `"continuous"`: how track numbers are presented (§29).
 */
numbering: string,
/**
 * MusicBrainz release id.
 */
musicbrainzId: string | null,
/**
 * Discogs release id.
 */
discogsId: string | null,
/**
 * Whether a person has accepted this metadata (§26).
 */
confirmed: boolean, };

export type Candidate = {
/**
 * `discogs` or `musicbrainz`.
 */
provider: string,
/**
 * The provider's own id, which is what `select_release` takes back.
 */
id: string,
/**
 * Release title.
 */
album: string,
/**
 * Credited artist.
 */
artist: string,
/**
 * Year, where the provider knows.
 */
year: number | null,
/**
 * Label.
 */
label: string,
/**
 * Catalog number.
 */
catalog: string,
/**
 * Country.
 */
country: string,
/**
 * Format, as the provider describes the medium.
 */
format: string,
/**
 * How many tracks it lists, which is the first thing to compare against
 * what was detected, or `None` where the provider did not say.
 *
 * Discogs is the reason this is optional. Its search endpoint returns no
 * tracklist at all - only a release fetch has one - so a count of zero
 * would be the panel saying "this pressing has no tracks" when what it
 * means is "ask again and I will know".
 */
tracks: number | null, };

export type Accepted = {
/**
 * The release title that was written.
 */
album: string,
/**
 * The release artist that was written.
 */
albumArtist: string,
/**
 * How many tracks were retitled.
 */
named: number,
/**
 * How many were left alone because a person had confirmed them.
 */
kept: number,
/**
 * Provider positions that named no track in this project.
 */
unmatched: Array<string>,
/**
 * Project tracks the tracklist did not cover, as §29 positions.
 */
unnamed: Array<string>,
/**
 * Tracks the release's layout moved to another side, as their new
 * positions, when the project's own layout was not the record's.
 */
relaid: Array<string>,
/**
 * Bytes of cover art stored with the release, or zero if none arrived.
 */
artwork: number, };

export type Waveform = {
/**
 * The capture this describes.
 */
captureId: number,
/**
 * First frame covered.
 */
startFrame: number,
/**
 * One past the last frame covered.
 */
endFrame: number,
/**
 * The first frame covered, in seconds.
 */
startSeconds: number,
/**
 * One past the last frame covered, in seconds.
 */
endSeconds: number,
/**
 * Minimum, per column, in -1..=1.
 */
min: Array<number>,
/**
 * Maximum, per column.
 */
max: Array<number>,
/**
 * RMS, per column.
 */
rms: Array<number>, };

export type ExportPlan = {
/**
 * `wav` or `flac`.
 */
container: string,
/**
 * One entry per file, in the order they will be written.
 */
files: Array<ExportFile>,
/**
 * Artwork files to be written beside the tracks.
 */
covers: Array<string>,
/**
 * Frames across the whole export, which is what a progress bar divides by.
 */
frames: number,
/**
 * Whether every file's channels are summed to one on the way out.
 *
 * The release's `is_mono`. A confirmation dialog has to say it: the fold
 * is invisible once the files exist - they are simply mono - so the only
 * place to notice it was not wanted is before the export runs.
 */
foldToMono: boolean, };

export type ExportFile = {
/**
 * Where it will be written.
 */
path: string,
/**
 * The track it comes from.
 */
trackId: number,
/**
 * §29's position, for a list that wants to show A1 rather than a row id.
 */
position: string,
/**
 * The title it will be tagged with.
 */
title: string,
/**
 * How many frames it will contain.
 */
frames: number, };

export type About = {
/**
 * The product's name.
 */
product: string,
/**
 * The release this is.
 */
version: string,
/**
 * `debug` or `release`. A timing complaint against a debug build is a
 * different conversation, and this is the field that ends it early.
 */
profile: string,
/**
 * The day this was compiled, as `YYYY-MM-DD`.
 *
 * Stamped by `build.rs`, because a version answers "which release" and
 * this answers the question people holding a build actually ask, which is
 * "is this the one from Tuesday". `SOURCE_DATE_EPOCH` wins where it is
 * set, so a reproducible build stays reproducible.
 */
built: string,
/**
 * Where the source is.
 */
repository: string,
/**
 * Whoever holds the copyright on VCW's own code.
 */
authors: Array<string>,
/**
 * The SPDX expression VCW's own code is under.
 */
license: string,
/**
 * The operating system the binary was built for.
 */
os: string,
/**
 * The processor architecture.
 */
arch: string,
/**
 * The bundled SQLite, as the library reports itself at run time.
 */
sqlite: string,
/**
 * The project schema this build writes.
 */
schemaVersion: number,
/**
 * The audio format version this build writes.
 */
formatVersion: number,
/**
 * Every third-party component this build owes a notice for.
 */
notices: Array<Notice>, };

export type Notice = {
/**
 * The crate, or the library it vendors.
 */
component: string,
/**
 * The SPDX expression the crate declares.
 */
license: string,
/**
 * What VCW can do because it is linked.
 */
provides: string,
/**
 * Where the complete corresponding source is published.
 */
source: string,
/**
 * Whether the license grants the right to modify the component and relink
 * it into VCW, which is the sentence LGPL-3.0 §4 requires be offered.
 */
copyleft: boolean, };

// ----------------------------------------------------------------------
// Settings (§39)
//
// Five groups, named as §39 names them. Every default is either `null`
// - let the engine negotiate and report what it got - or a constant read
// out of the crate that owns the behavior, which is why none of them
// appears in this declaration. `Credential` is the one type here that
// carries nothing: whether a token is configured and how long it is,
// never the value.
// ----------------------------------------------------------------------

export type Settings = {
/**
 * Input, output, backend, rate, format, buffer size, capture mode.
 */
audio: AudioSettings,
/**
 * Default location, transaction and block size, recovery behavior.
 */
recording: RecordingSettings,
/**
 * Algorithm, thresholds, minimum silence, minimum track length.
 */
detection: DetectionSettings,
/**
 * Which providers to ask, and how to identify ourselves to them.
 */
metadata: MetadataSettings,
/**
 * Format, codec options, output path, naming template.
 */
export: ExportSettings, };

export type AudioSettings = {
/**
 * Capture device id, or `null` for the host default. The simulated source
 * is what `null` gets on a machine with nothing attached.
 */
input: string | null,
/**
 * Playback device id, or `null` for the host default.
 */
output: string | null,
/**
 * Host API to prefer: `alsa`, `jack`, `coreaudio`, `wasapi`, `asio`.
 */
backend: string | null,
/**
 * Sample rate to pin, in Hz.
 */
rate: number | null,
/**
 * Sample format to pin, spelled as [`crate::command::parse_format`] takes
 * it.
 */
format: string | null,
/**
 * Ring capacity in milliseconds. `None` takes §10's default.
 *
 * Milliseconds and not frames, because the thing being sized is a duration
 * of tolerance to a stalled writer and the frame count that buys it
 * changes with the rate.
 */
ringMillis: number | null,
/**
 * `shared`, `native` or `exclusive` (§8).
 */
mode: string | null,
/**
 * Equalization the signal carries when it arrives: `flat`, `riaa` or
 * `unknown` (§51). `None` means the same as `unknown`.
 *
 * A setting rather than a per-capture field because it describes the
 * operator's preamp, which does not change between records. It cannot be
 * recovered from the audio afterwards, which is why it is asked for here
 * instead of when the curves ship.
 */
eq: string | null, };

export type RecordingSettings = {
/**
 * Where projects are kept, and what the browser lists (§34).
 *
 * `null` until a person picks one, and an unset library is an empty
 * browser rather than a guess: creating a directory in somebody's home
 * because they opened the application once is not a decision this
 * application gets to make.
 */
library: string | null,
/**
 * Frames per stored block. `None` takes the writer's own default.
 */
blockFrames: number | null,
/**
 * How often to commit, in seconds.
 *
 * This is the crash-loss floor and nothing else is: a crash loses the
 * uncommitted frames plus whatever the driver was holding, and the ring
 * size does not enter into it.
 */
checkpointSeconds: number | null,
/**
 * What to do with a project that was not closed cleanly: `ask`, `recover`
 * or `leave`.
 */
recovery: string | null, };

export type DetectionSettings = {
/**
 * Which detectors to run: `silence`, `spectral`, `hmm`, or `all`.
 */
algorithm: string,
/**
 * How many detectors must agree before a boundary becomes a track (§24).
 */
minSources: number,
/**
 * The lowest confidence worth writing, in `0.0..=1.0`.
 */
minConfidence: number,
/**
 * The shortest gap that counts as a gap, in seconds.
 */
minSilenceSeconds: number,
/**
 * The shortest run of audio worth calling a track, in seconds.
 *
 * Seconds here and frames in the policy, because a person thinks in
 * seconds and the detector needs frames - and the rate needed to convert
 * belongs to the capture, which is why the conversion is not in this file.
 */
minTrackSeconds: number, };

export type MetadataSettings = {
/**
 * Whether to ask Discogs (§26).
 */
discogs: boolean,
/**
 * Whether to ask MusicBrainz (§27).
 */
musicbrainz: boolean,
/**
 * Whether to ask AcoustID. Phase 2 (§45), off here.
 */
acoustid: boolean,
/**
 * Contact address sent in the user agent, per §40.
 */
contact: string | null,
/**
 * A `genre.dat` to fold provider genres through, or `null` for the
 * built-in mapping.
 */
genreMap: string | null,
/**
 * Whether a lookup may go out at all. §40: the application stays fully
 * usable offline, and this is how a person says so.
 */
online: boolean, };

export type ExportSettings = {
/**
 * `flac`, `wav`, `mp3` or `ogg`.
 */
format: string,
/**
 * What a lossy container is written at: `transparent`, `high` or
 * `compact`.
 *
 * Kept even while the format is lossless, deliberately: a person who sets
 * a quality, switches to FLAC for an archival copy and switches back
 * should find their choice still there.
 */
quality: string,
/**
 * What FLAC is written at: `0` to `8`.
 *
 * A separate field from `quality` and not a reuse of it, because the two
 * answer different questions. `quality` decides what gets thrown away and
 * FLAC throws nothing away; this decides how long the encoder spends
 * finding a smaller way to say the same samples. Kept across a format
 * change for `quality`'s reason.
 */
compression: string,
/**
 * Where to write, or `null` to be asked each time.
 */
output: string | null,
/**
 * The naming template (§33).
 */
template: string,
/**
 * `none`, `embed`, `folder` or `both`.
 */
artwork: string, };

export type Credential = {
/**
 * Which credential: `discogs`, `acoustid`.
 */
name: string,
/**
 * Whether one was found.
 */
present: boolean,
/**
 * How many characters it has, or zero when there is none.
 */
characters: number,
/**
 * The environment variable it is read from, so a panel can tell a person
 * where to put it rather than offering a field that writes it to disk.
 */
variable: string, };
