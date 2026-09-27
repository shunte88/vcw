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
  Arm,
  Audition,
  Capture,
  Device,
  Export,
  ExportPlan,
  Failure,
  Marker,
  Playback,
  Release,
  Side,
  Track,
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
 * Tauri rejects with whatever the command's error serialised to, which for
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

/** Resolves an export and returns the plan, writing nothing. */
export function exportPlan(request: Export): Promise<ExportPlan> {
  return invoke("export_plan", { export: request });
}

/** Writes an export. Progress and the outcome arrive as events. */
export function exportRun(request: Export): Promise<void> {
  return invoke("export_run", { export: request });
}

// --- Commands that read (not in §35's list, because they change nothing) ---

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
