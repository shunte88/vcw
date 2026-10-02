/*
 *  store.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  One event subscription, one reducer: the only place engine state lives on this side.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// §34: React owns only ephemeral view state. This file is the line, and it is
// worth being exact about where it falls.
//
// Everything here is a *copy of the last thing Rust said*. Nothing is derived,
// nothing is accumulated into a new fact, and no field is computed from two
// others. `frames` is the number the last `recording-position` carried;
// `peakDb` is the number the last `meter-update` carried. If a panel wants
// something that is not in this shape, the answer is a field on a view model or
// an event, not a calculation here - because `vcw --json` has to print the same
// answer and two implementations of one rule is one too many.
//
// The single exception, stated so it can be argued with: the event log keeps a
// bounded history, which is an accumulation. §42 asks for diagnostics a person
// can read, a log of one line is not that, and the alternative is a Rust-side
// ring buffer exposed by a polling command - more machinery for the same list.
// It is bounded at LOG_LIMIT and it is never read by anything but the
// diagnostics panel.
//
// # Why one subscription
//
// S3 measured the boundary and found it free; what it found expensive was the
// main thread. A component that listened for itself would be a second listener
// on the same 60 Hz meter stream, and ten components would be ten. There is one
// `listen` call in the application, in `useEngine`, mounted once at the root.

import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "./api";
import type {
  Boundary,
  Capture,
  Diagnostics,
  Failure,
  Meter,
  Release,
  Side,
  Track,
  Wire,
} from "./bindings/vcw";

/** How many events the diagnostics log keeps. Older ones are dropped. */
export const LOG_LIMIT = 500;

/**
 * Event kinds the log does not keep, because they are a stream and not events.
 *
 * `meter-update` arrives at 60 Hz and `playback-position` at about 12.5, so
 * between them they wrote the log's whole 500-line capacity in roughly eight
 * seconds of playback. Everything that mattered - a refusal, a warning, the
 * end of a capture - was off the end of the buffer before a person could open
 * the panel, which made the one tool for seeing what the engine did useless at
 * exactly the moment it was needed.
 *
 * Dropping them loses nothing. A meter tick's content is a level that is
 * already drawn on the meters and a position is already on the transport
 * clock, and neither carries anything the next one does not. What they are is
 * telemetry sampled continuously, and a log is for things that happened once.
 *
 * `logged` is still folded into the engine state first, so the meters and the
 * clock see every one of them: this is about what is *kept*, not about what is
 * delivered.
 */
export const UNLOGGED: ReadonlySet<string> = new Set([
  "meter-update",
  "playback-position",
  "recording-position",
]);

/** Whether an event is worth a line in the log. */
export function logged(event: Wire): boolean {
  return !UNLOGGED.has(event.kind);
}

/** One line in the event log. */
export type Line = {
  /** Wall-clock arrival, for the log's own timestamp column. */
  readonly at: number;
  /** The event, whole, so the panel can render it however it likes. */
  readonly event: Wire;
};

/**
 * The summary a finished detection pass reported.
 *
 * Narrowed out of the generated union rather than written again, so a field
 * added to the event appears here without an edit - and a field removed is a
 * compile error at the panel that read it.
 */
export type Detection = Extract<Wire, { kind: "detection-finished" }>;

/** What the last thing Rust said adds up to. */
export type Engine = {
  /** The capture phase: `idle`, `armed`, `recording`, `paused`, `stopped`. */
  readonly phase: string;
  /** Frames committed to the project, from `recording-position`. */
  readonly frames: number;
  /** The same position in seconds, as the event carried it. */
  readonly seconds: number;
  /** The last meter snapshot, or null before the first one. */
  readonly meter: Meter | null;
  /** The negotiated format, as the `armed` event reported it. */
  readonly negotiated: string | null;
  /** Whether the OS confirmed the negotiated format. */
  readonly verified: boolean;
  /** Where the negotiation differed from what was asked for. */
  readonly divergences: readonly string[];
  /** Capture diagnostics, from the last `status`. */
  readonly diagnostics: Diagnostics | null;
  /** Whether something is playing. */
  readonly playing: boolean;
  /** The playhead, in seconds, while something is playing. */
  readonly playhead: number;
  /** The capture the audition is reading, if one is playing. */
  readonly auditioning: number | null;
  /** Export progress as `[done, total]`, or null when nothing is exporting. */
  readonly exporting: readonly [number, number] | null;
  /** What the last detection pass found, or null if none has finished. */
  readonly detection: Detection | null;
  /** The last refusal, whether it came back from a command or over the bus. */
  readonly refusal: Failure | null;
  /** The bounded event log (§42). */
  readonly log: readonly Line[];
};

/** Nothing has happened yet. */
/**
 * The state before anything has happened.
 *
 * Exported alongside [`fold`] for the same reason: a reducer test needs a
 * starting point, and inventing one in the test would let the two drift.
 */
export const NOTHING: Engine = {
  phase: "idle",
  frames: 0,
  seconds: 0,
  meter: null,
  negotiated: null,
  verified: false,
  divergences: [],
  diagnostics: null,
  playing: false,
  playhead: 0,
  auditioning: null,
  exporting: null,
  detection: null,
  refusal: null,
  log: [],
};

/**
 * Folds one event into the state.
 *
 * Exhaustive over the union on purpose: a new event kind added to the contract
 * is a compile error here, which is the whole argument for D9. The `default` at
 * the end is unreachable and is there so the function still returns a value if
 * a build ever compiles against an older declaration file.
 *
 * Exported for `store.test.ts`, which is the only caller outside this module: a
 * pure function over one event and one state is the cheapest place to assert
 * what the window does with an event, and the alternative is mounting the
 * application to find out.
 */
export function fold(state: Engine, event: Wire): Engine {
  switch (event.kind) {
    case "phase-change":
      return { ...state, phase: event.to };
    case "armed":
      return {
        ...state,
        negotiated: event.negotiated,
        verified: event.verified,
        divergences: event.divergences,
      };
    case "recording-position":
      return { ...state, frames: event.frames, seconds: event.seconds };
    case "meter-update":
      return { ...state, meter: event.meter };
    case "status":
      // No diagnostics here: `status` answers `poll` with the phase and the
      // frame count and nothing else. The overrun and dropped-frame counts
      // arrive with `capture-finished`, which is the honest place for them -
      // §38's diagnostics are a property of a finished capture, and a live
      // count would need an event the bus does not publish.
      return { ...state, phase: event.phase, frames: event.frames };
    case "capture-finished":
      // The phase is not touched here. `capture-finished` says a capture
      // closed; `phase-change` says what the transport is doing, and the engine
      // sends both. Inferring one from the other is exactly the kind of
      // second-guessing §2 keeps out of this layer.
      return { ...state, frames: event.frames, diagnostics: event.diagnostics };
    case "auditioning":
      return {
        ...state,
        playing: true,
        auditioning: event.captureId,
        playhead: 0,
      };
    case "playback-position":
      return { ...state, playhead: event.seconds };
    case "playback-finished":
      return { ...state, playing: false, auditioning: null };
    case "playback-refused":
      // Both halves matter. The transport goes back to not playing, because
      // this event is the only thing that will ever say so - there is no
      // `playback-finished` behind a refusal - and the reason is shown, because
      // "the device will not play 96 kHz" is a sentence somebody can act on.
      return {
        ...state,
        playing: false,
        auditioning: null,
        playhead: 0,
        refusal: {
          code: "playback-refused",
          message: `${event.scope} of capture ${event.captureId}: ${event.reason}`,
          field: null,
        },
      };
    case "export-progress":
      return { ...state, exporting: [event.index, event.of] };
    case "export-finished":
      return { ...state, exporting: null };
    case "export-failed":
      return {
        ...state,
        exporting: null,
        refusal: { code: "export", message: event.reason, field: null },
      };
    case "detection-finished":
      // The boundaries themselves are not here, for the same reason
      // `track-detected` holds nothing: they are rows, and rows are read with a
      // command. What is kept is the pass's own report - how many were rejected
      // and how many were already settled - which is not in any row and is the
      // only place §31's "what did re-analysis actually do" can be answered from.
      return { ...state, detection: event };
    case "detection-failed":
      return {
        ...state,
        detection: null,
        refusal: {
          code: "detection",
          message:
            event.side === null
              ? event.reason
              : `${event.side}: ${event.reason} (${event.completed} sides done)`,
          field: null,
        },
      };
    case "command-refused":
      return {
        ...state,
        refusal: {
          code: "refused",
          message: `${event.command} in ${event.phase}: ${event.reason}`,
          field: null,
        },
      };
    case "command-rejected":
      return {
        ...state,
        refusal: {
          code: "rejected",
          message: `${event.command} is not available in ${event.phase}`,
          field: null,
        },
      };
    case "capture-warning":
      return {
        ...state,
        refusal: { code: event.code, message: event.detail, field: null },
      };
    case "track-detected":
      // Nothing to hold: this says *the project changed*, and the project is
      // read back with a command. Copying a detected boundary into this state
      // would make it the second place a boundary lives.
      //
      // §35 also names `waveform-update` and `fingerprint-match`, and neither
      // is in the union. `waveform-update` never will be: the pyramid answers
      // a query in about a millisecond and is at most one commit interval
      // behind the device, so a push cannot make the picture newer: the window
      // asks when it draws. `fingerprint-match` is Phase 2's. Neither is
      // handled here because a case for a kind the type does not have will not
      // compile, which is the drift check working in the direction that
      // matters.
      return state;
    case "closed":
      return { ...NOTHING, log: state.log };
    default:
      return state;
  }
}

/** Everything the root component needs, and the only event subscription. */
export type Store = {
  /** The engine's last word. */
  readonly engine: Engine;
  /** The project rows, reloaded when something says they changed. */
  readonly project: ProjectState;
  /** Runs a command, catching a refusal into `engine.refusal`. */
  readonly run: (what: () => Promise<unknown>) => Promise<void>;
  /** Clears the last refusal. */
  readonly dismiss: () => void;
  /** Re-reads the project rows. */
  readonly reload: () => void;
  /** Opens a project and re-reads everything. */
  readonly open: (path: string) => Promise<void>;
};

/**
 * What has been read out of the open project.
 *
 * Five commands, read together, because a panel that asked for its own rows
 * would be a fifth place that decides when they are stale. They are read as a
 * batch and replaced as a batch, so nothing ever renders a track list from one
 * generation beside a boundary list from another.
 */
export type ProjectState = {
  /** The path the shell has open, or null. */
  readonly path: string | null;
  /** Every capture. */
  readonly captures: readonly Capture[];
  /** Every side, in playing order. */
  readonly sides: readonly Side[];
  /** Every track, with §29's positions rendered. */
  readonly tracks: readonly Track[];
  /** Every boundary, promoted into a track or not. */
  readonly boundaries: readonly Boundary[];
  /** The release, or null in a project nobody has identified yet. */
  readonly release: Release | null;
};

const NO_PROJECT: ProjectState = {
  path: null,
  captures: [],
  sides: [],
  tracks: [],
  boundaries: [],
  release: null,
};

/**
 * Subscribes to the engine and reads the project.
 *
 * Call once, at the root. Two calls would be two subscriptions.
 */
export function useEngine(): Store {
  const [engine, setEngine] = useState<Engine>(NOTHING);
  const [project, setProject] = useState<ProjectState>(NO_PROJECT);

  // A counter rather than a promise, so several events arriving together cause
  // one re-read instead of three: each bumps the same number and the effect
  // below runs once for the batch React renders.
  const [generation, setGeneration] = useState(0);
  const bump = useCallback(() => setGeneration((n) => n + 1), []);

  // Held in a ref so the subscription effect never re-runs. An effect that
  // depended on `bump` would unsubscribe and resubscribe on every render that
  // changed its identity, and a gap between the two is a dropped event.
  const onChange = useRef(bump);
  onChange.current = bump;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void api
      .onEvent((event) => {
        if (cancelled) {
          return;
        }
        setEngine((previous) => ({
          ...fold(previous, event),
          log: logged(event)
            ? [...previous.log, { at: Date.now(), event }].slice(-LOG_LIMIT)
            : previous.log,
        }));
        // Which events mean "the rows on disk moved". Listed rather than
        // reloading on everything, because `meter-update` arrives at 60 Hz and
        // a re-read per meter tick is the one mistake S3 warned about.
        if (
          event.kind === "capture-finished" ||
          event.kind === "track-detected" ||
          event.kind === "detection-finished" ||
          event.kind === "export-finished"
        ) {
          onChange.current();
        }
      })
      .then((off) => {
        if (cancelled) {
          off();
        } else {
          unlisten = off;
        }
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // The project rows. Read on mount so a reloaded window finds an engine that
  // is already armed, which is what `open_path` is for.
  //
  // The catch is not defensive tidiness, it is the fix for what first light
  // found. These five reads used to be an unhandled `Promise.all`: if one
  // rejected, `setProject` was never reached and the window carried on
  // displaying the *previous* project - its title, its tracks, its waveform -
  // while the shell pointed at the new one. Every edit verb resolves against
  // the shell's path, so the next marker or split would have gone to the
  // project nobody was looking at.
  //
  // So a failed read clears to `NO_PROJECT` and says why. An empty window with
  // a reason on it is a bad outcome; a full window describing the wrong record
  // is a worse one.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const path = await api.openPath();
        if (cancelled) {
          return;
        }
        if (path === null) {
          setProject(NO_PROJECT);
          return;
        }
        const [captures, sides, tracks, boundaries, release] =
          await Promise.all([
            api.captures(),
            api.sides(),
            api.tracks(),
            api.boundaries(),
            api.release(),
          ]);
        if (!cancelled) {
          setProject({ path, captures, sides, tracks, boundaries, release });
        }
      } catch (error) {
        if (cancelled) {
          return;
        }
        setProject(NO_PROJECT);
        setEngine((previous) => ({
          ...previous,
          refusal: api.asFailure(error),
        }));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [generation]);

  const run = useCallback(async (what: () => Promise<unknown>) => {
    setEngine((previous) => ({ ...previous, refusal: null }));
    try {
      await what();
    } catch (error) {
      const refusal = api.asFailure(error);
      setEngine((previous) => ({ ...previous, refusal }));
    }
  }, []);

  const dismiss = useCallback(
    () => setEngine((previous) => ({ ...previous, refusal: null })),
    [],
  );

  const open = useCallback(
    async (path: string) => {
      await run(() => api.openProject(path));
      bump();
    },
    [run, bump],
  );

  return { engine, project, run, dismiss, reload: bump, open };
}
