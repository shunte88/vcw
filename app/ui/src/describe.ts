/*
 *  describe.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  One line of readable text for any event on the bus (§42).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The diagnostics log's renderer, and the only place an event kind is turned
// into prose. Its own module rather than a function in the panel that shows it,
// because two panels want it: §42's log and the status line that shows the last
// thing that happened.
//
// §2 holds: nothing here decides anything, and nothing is computed that Rust
// did not already say. `toFixed` is formatting, not policy - the number is the
// number the event carried, and a reader who wants the full precision has the
// event itself in the log line beside this text.

import type { Wire } from "./bindings/vcw";

/**
 * One line of text for an event.
 *
 * The `switch` is exhaustive over the union, which is the generated types
 * earning their place: a new event kind is a compile error here rather than a
 * silent gap in the log. There is no `default`, on purpose - a fallback would
 * turn that compile error into a line reading "something happened".
 */
export function describe(event: Wire): string {
  switch (event.kind) {
    case "phase-change":
      return `${event.from} -> ${event.to}`;
    case "armed":
      return `${event.negotiated}${event.verified ? ", os-confirmed" : ""}${
        event.divergences.length > 0 ? ` (${event.divergences.join("; ")})` : ""
      }`;
    case "recording-position":
      return `${event.frames} frame(s), ${event.seconds.toFixed(2)} s`;
    case "meter-update":
      return event.meter.channels
        .map((channel) => `${channel.peakDb.toFixed(1)} dBFS`)
        .join(" / ");
    case "track-detected":
      return `${event.edge} at ${event.seconds.toFixed(2)} s, ${(
        event.confidence * 100
      ).toFixed(0)}% (${event.provenance})`;
    case "capture-warning":
      return `${event.code}: ${event.detail}`;
    case "capture-finished":
      return `capture ${event.captureId}, ${event.frames} frame(s), ${event.state}${
        event.bitPerfect ? ", bit-perfect" : ""
      }`;
    case "auditioning":
      return `${event.scope} on ${event.opened} (${event.conversion})`;
    case "playback-position":
      return `${event.seconds.toFixed(2)} s`;
    case "playback-finished":
      return `${event.frames} frame(s), ${event.underruns} underrun(s), ${event.fidelity}`;
    case "command-refused":
      return `${event.command} in ${event.phase}: ${event.reason}`;
    case "command-rejected":
      return `${event.command} in ${event.phase}`;
    case "status":
      return `${event.phase}, ${event.frames} frame(s)`;
    case "export-progress":
      return `${event.index}/${event.of} ${event.path}`;
    case "export-finished":
      return `${event.files} file(s), ${event.covers} cover(s), ${event.bytesWritten} byte(s)`;
    case "export-failed":
      return `after ${event.written} file(s): ${event.reason}`;
    case "detection-finished":
      return `${event.sides.join(", ")}: ${event.boundaries} boundary(s), ${
        event.tracks
      } track(s), ${event.rejected} rejected, ${
        event.alreadySettled
      } already settled, ${event.seconds.toFixed(2)} s`;
    case "detection-failed":
      return `${event.side ?? "the project"} after ${
        event.completed
      } side(s): ${event.reason}`;
    case "closed":
      return "the engine has stopped";
  }
}
