/*
 *  api.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The typed frontend half of the contract: one function per shell command.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The only file in the frontend that calls `invoke`. Everything else calls
// these, which means the argument shapes are checked once, here, against types
// generated from the Rust declarations rather than written twice.
//
// §2: nothing in this file decides anything. No unit conversion, no defaulting,
// no "if the user did not pick a rate use 44100" - all of that is behind the
// boundary, because a second frontend would have to make the same choices and
// two implementations of a policy is one too many.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  About,
  Accepted,
  Arm,
  Audition,
  Boundary,
  Candidate,
  Capture,
  Credential,
  Detect,
  Device,
  Export,
  ExportPlan,
  Failure,
  Lock,
  Marker,
  Merge,
  NewProject,
  Placement,
  Playback,
  Project,
  Release,
  Removal,
  Search,
  Selection,
  Settings,
  Side,
  Split,
  Track,
  TrackEdit,
  Transport,
  Waveform,
  Wire,
  Zoom,
} from "./bindings/vcw";

/** The single event channel every core event arrives on. See `pump.rs`. */
export const EVENT = "vcw://event";

/** Subscribes to the event stream. Returns the function that unsubscribes. */
export function onEvent(handler: (event: Wire) => void): Promise<UnlistenFn> {
  return listen<Wire>(EVENT, (message) => handler(message.payload));
}

/**
 * Narrows a rejected `invoke` to the shell's own failure shape.
 *
 * Tauri rejects with whatever the command's error serialized to, which for
 * every command here is a `Failure`. A thrown string means something failed
 * before the command ran - a name that is not registered, usually.
 */
export function asFailure(error: unknown): Failure {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error
  ) {
    return error as Failure;
  }
  return { code: "unknown", message: String(error), field: null };
}

// --- Commands that change something (§35) ---------------------------------

/** Opens a device and a project. The answer arrives as an `armed` event. */
export function arm(request: Arm): Promise<void> {
  return invoke("arm", { arm: request });
}

/** Sends a capture transport verb. */
export function transport(verb: Transport): Promise<void> {
  return invoke("transport", { verb });
}

/** Asks the engine to publish where it is, as a `status` event. */
export function poll(): Promise<void> {
  return invoke("poll");
}

/** Opens a project without arming a device. */
export function openProject(path: string): Promise<void> {
  return invoke("open_project", { path });
}

/** Starts an audition. */
export function play(
  captureId: number,
  scope: Audition,
  device?: string,
): Promise<void> {
  return invoke("play", { captureId, scope, device: device ?? null });
}

/** Sends a playback verb to the running audition. */
export function playback(verb: Playback): Promise<void> {
  return invoke("playback", { verb });
}

/** Moves a boundary. */
export function moveMarker(marker: Marker): Promise<void> {
  return invoke("move_marker", { marker });
}

/** Places a boundary, and returns the row id it was written as. */
export function placeMarker(placement: Placement): Promise<number> {
  return invoke("place_marker", { placement });
}

/** Deletes a boundary. */
export function deleteMarker(removal: Removal): Promise<void> {
  return invoke("delete_marker", { removal });
}

/**
 * Locks or unlocks a boundary (§31).
 *
 * A locked boundary survives re-analysis, which is the only way a person can
 * overrule a detector and have it stick.
 */
export function lockMarker(lock: Lock): Promise<void> {
  return invoke("lock_marker", { lock });
}

/** Edits a track's metadata. Every field is optional; null leaves it alone. */
export function editTrack(edit: TrackEdit): Promise<void> {
  return invoke("edit_track", { edit });
}

/** Splits a track in two, and returns the id of the new second half. */
export function splitTrack(split: Split): Promise<number> {
  return invoke("split_track", { split });
}

/** Joins two adjacent tracks into one. */
export function mergeTracks(merge: Merge): Promise<void> {
  return invoke("merge_tracks", { merge });
}

/**
 * Runs detection over a side, or the whole project.
 *
 * Returns as soon as the pass has started: a `track-detected` event arrives per
 * boundary, then one `detection-finished` or `detection-failed`.
 */
export function detectTracks(detect: Detect): Promise<void> {
  return invoke("detect_tracks", { detect });
}

/**
 * Asks the metadata providers about this record (§28).
 *
 * Starting a search cancels one already in flight, so a caller that searches
 * on every keystroke gets the last answer rather than a race.
 */
export function searchMetadata(search: Search): Promise<Candidate[]> {
  return invoke("search_metadata", { search });
}

/**
 * Accepts a candidate and writes it into the project (§26).
 *
 * The answer says what was named and what was not: a tracklist that does not
 * line up with the captured sides is reported rather than forced.
 */
export function selectRelease(selection: Selection): Promise<Accepted> {
  return invoke("select_release", { selection });
}

/** Resolves an export and returns the plan, writing nothing. */
export function exportPlan(request: Export): Promise<ExportPlan> {
  return invoke("export_plan", { export: request });
}

/** Writes an export. Progress and the outcome arrive as events. */
export function exportRun(request: Export): Promise<void> {
  return invoke("export_run", { export: request });
}

/** Creates a project in the library, seeded from the sleeve. */
export function newProject(seed: NewProject): Promise<Project> {
  return invoke("new_project", { seed });
}

/** Writes §39's settings, whole. */
export function saveSettings(settings: Settings): Promise<void> {
  return invoke("save_settings", { settings });
}

// --- Commands that read (not in §35's list, because they change nothing) ---

/** Which build this is, and what it links. Constant for the life of the app. */
export function about(): Promise<About> {
  return invoke("about");
}

/**
 * Opens one of the author's pages in the operator's own browser.
 *
 * A name and not a URL: the addresses are a table in the shell, so this side
 * can ask for a page and cannot ask for an address.
 *
 * A `string` rather than a union of the names, because the shell *generates*
 * its table - the repository from the manifest, one entry per third-party
 * notice from the cargo features - so the set is a property of the build and
 * not something this file could spell. An unknown name is refused, with the
 * refusal shown rather than swallowed.
 */
export function support(page: string): Promise<void> {
  return invoke("support", { page });
}

/** Every audio device the host offers. */
export function devices(): Promise<Device[]> {
  return invoke("devices");
}

/** The release, or null in a project that has not had one filled in. */
export function release(): Promise<Release | null> {
  return invoke("release");
}

/** Every side, in playing order. */
export function sides(): Promise<Side[]> {
  return invoke("sides");
}

/** Every track, with §29's positions already rendered. */
export function tracks(): Promise<Track[]> {
  return invoke("tracks");
}

/** Every capture in the project. */
export function captures(): Promise<Capture[]> {
  return invoke("captures");
}

/** One channel of one capture, drawn to a given width. */
export function waveform(zoom: Zoom): Promise<Waveform> {
  return invoke("waveform", { zoom });
}

/** Every boundary, promoted into a track or not. */
export function boundaries(): Promise<Boundary[]> {
  return invoke("boundaries");
}

/** Every project in the library, newest first. */
export function projects(): Promise<Project[]> {
  return invoke("projects");
}

/**
 * One project's front cover as a `data:` URL, or null if it has none.
 *
 * By path, and asked for a row at a time. `projects()` reports `hasArtwork`
 * and not the image: a cover is a megabyte or two and a library is a hundred
 * rows, so a listing that carried them would cost more to open than a project.
 */
export function artwork(path: string): Promise<string | null> {
  return invoke("artwork", { path });
}

/** §39's settings, or the defaults on first run. */
export function settings(): Promise<Settings> {
  return invoke("settings");
}

/** Where the library is, or null if nobody has chosen one. */
export function libraryRoot(): Promise<string | null> {
  return invoke("library_root");
}

/** The project the shell has open, which survives a window reload. */
export function openPath(): Promise<string | null> {
  return invoke("open_path");
}

/**
 * Which credentials are configured (§39).
 *
 * Never their values. The shell has no command that would return one, which is
 * the only way to be sure a settings panel cannot show a token by accident.
 */
export function credentials(): Promise<Credential[]> {
  return invoke("credentials");
}
