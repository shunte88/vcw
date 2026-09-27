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
seconds: number, } | { "kind": "playback-finished",
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
written: number, } | { "kind": "closed" };

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
verb: Transport, } | { "command": "play" } & Audition | { "command": "seek" } & Seek | { "command": "move_marker" } & Marker | { "command": "search_metadata" } & Search | { "command": "select_release" } & Selection | { "command": "export" } & Export;

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
ringMillis: number | null, };

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
 * Catalogue number, which is what identifies a pressing.
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
 * `wav` or `flac`.
 */
format: string,
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
 * Unix seconds at finalisation, or `null` while it is still running.
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
 * Normalised genres, in order (§32).
 */
genres: Array<string>,
/**
 * Record label.
 */
label: string,
/**
 * Catalogue number off the label, which is what identifies a pressing.
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
 * Catalogue number.
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
 * what was detected.
 */
tracks: number, };

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
frames: number, };

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
