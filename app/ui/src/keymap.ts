/*
 *  keymap.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §43's keyboard map, as data, and §44's workflows as the thing it has to cover.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// WP-16's exit criterion has two halves. *No business logic in TS* is a review
// gate, and most of it was paid for by the contract resolving every unit on the
// Rust side. *Every §44 workflow completable by keyboard alone* is the half
// with nothing behind it, and a claim in a document is not how it gets met.
//
// So the map is data, and §44's list is a type. `BINDINGS` is declared as a
// value that must cover every `Workflow`, which makes a missing binding a
// compile error in `pnpm check` rather than something a person notices while
// demonstrating the application. The duplicate and reachability checks that
// cannot be expressed in the type system are in `keymap.test.ts`.
//
// What this file is *not*: it holds no handlers. A binding names an action and
// the panel that owns the action decides what it means, because "what does
// SKIP FORWARD do" is a question about the transport and not about a keyboard.

import { key } from "./i18n";

/**
 * §44's MVP list, as the things a person has to be able to do.
 *
 * One entry per required capability, named for the *workflow* rather than the
 * feature: §44 says "marker editing", and what has to be reachable is placing
 * one, moving it and deleting it. Splitting them is what makes the coverage
 * check mean something - a single binding called `markers` would satisfy a
 * naive test and leave a person unable to delete anything.
 *
 * `device-enumeration`, `sqlite-block-storage` and `project-recovery` are in
 * §44 and are deliberately absent here: they are not things a person does, they
 * are things that happen. Enumeration is what the capture workspace shows on
 * open, storage is the writer, and recovery is a prompt the shell raises. A
 * keyboard map that claimed to cover them would be describing the wrong thing.
 */
export type Workflow =
  | "choose-device"
  | "choose-rate-format"
  | "arm"
  | "record"
  | "pause"
  | "stop"
  | "play"
  | "seek"
  | "skip"
  | "place-marker"
  | "move-marker"
  | "delete-marker"
  | "detect-tracks"
  | "edit-track-metadata"
  | "search-metadata"
  | "choose-release"
  | "export"
  | "checkpoint"
  | "navigate"
  | "help";

/** Where a binding applies. */
export type Scope =
  | "global"
  | "browser"
  | "capture"
  | "tracks"
  | "metadata"
  | "export"
  | "settings";

/** One binding. */
export type Binding = {
  /**
   * The chord, in the spelling [`chord`](./keys) produces from a
   * `KeyboardEvent`: modifiers in the order Ctrl, Alt, Shift, then the key, all
   * joined with `+`. A single printable key is itself, lower case.
   */
  readonly chord: string;
  /** A second chord for the same action, where §43 or habit wants one. */
  readonly also?: string;
  /** The §44 workflow it serves. */
  readonly workflow: Workflow;
  /** Where it applies. `global` means everywhere. */
  readonly scope: Scope;
  /**
   * The catalog key for what the help overlay shows.
   *
   * A key rather than the English, because the overlay is the one place a
   * person reads the whole map and it has to read in their language. The key
   * is written out per entry rather than built from the action name, so
   * `crates/i18n/tests/callers.rs` can see it.
   */
  readonly label: string;
  /**
   * Whether §43 names this chord specifically.
   *
   * §43 lists eight "suggested defaults" and the rest of the map is ours.
   * Marking them is what lets a test assert we did not quietly rebind one:
   * moving `R` off record would be a decision worth arguing about, not a tidy-up.
   */
  readonly suggested?: true;
};

/**
 * The map.
 *
 * `satisfies` rather than a type annotation, so the literal's own keys stay
 * visible to the type checker while still being checked against
 * `Record<Workflow, ...>`. That is what turns a missing §44 workflow into an
 * error here rather than a gap discovered later.
 */
export const BINDINGS = {
  // --- §43's suggested defaults -------------------------------------------
  play: {
    chord: "space",
    workflow: "play",
    scope: "global",
    label: key("keys.play"),
    suggested: true,
  },
  record: {
    chord: "r",
    workflow: "record",
    scope: "global",
    label: key("keys.record"),
    suggested: true,
  },
  stop: {
    chord: "s",
    workflow: "stop",
    scope: "global",
    label: key("keys.stop"),
    suggested: true,
  },
  marker: {
    chord: "m",
    workflow: "place-marker",
    scope: "global",
    label: key("keys.marker"),
    suggested: true,
  },
  deleteMarker: {
    chord: "Delete",
    also: "Backspace",
    workflow: "delete-marker",
    scope: "tracks",
    label: key("keys.delete.marker"),
    suggested: true,
  },
  seekBack: {
    chord: "ArrowLeft",
    workflow: "seek",
    scope: "global",
    label: key("keys.seek.back"),
    suggested: true,
  },
  seekForward: {
    chord: "ArrowRight",
    workflow: "seek",
    scope: "global",
    label: key("keys.seek.forward"),
    suggested: true,
  },
  checkpoint: {
    chord: "Ctrl+s",
    workflow: "checkpoint",
    scope: "global",
    label: key("keys.checkpoint"),
    suggested: true,
  },
  exportRun: {
    chord: "Ctrl+e",
    workflow: "export",
    scope: "global",
    label: key("keys.export.run"),
    suggested: true,
  },

  // --- The rest, which are ours -------------------------------------------
  pause: {
    chord: "p",
    workflow: "pause",
    scope: "global",
    label: key("keys.pause"),
  },
  arm: {
    chord: "a",
    workflow: "arm",
    scope: "capture",
    label: key("keys.arm"),
  },
  chooseDevice: {
    chord: "d",
    workflow: "choose-device",
    scope: "capture",
    label: key("keys.choose.device"),
  },
  chooseFormat: {
    chord: "f",
    workflow: "choose-rate-format",
    scope: "capture",
    label: key("keys.choose.format"),
  },
  skipForward: {
    chord: "Shift+ArrowRight",
    workflow: "skip",
    scope: "global",
    label: key("keys.skip.forward"),
  },
  skipBack: {
    chord: "Shift+ArrowLeft",
    workflow: "skip",
    scope: "global",
    label: key("keys.skip.back"),
  },
  nudgeBack: {
    chord: "Ctrl+ArrowLeft",
    workflow: "move-marker",
    scope: "tracks",
    label: key("keys.nudge.back"),
  },
  nudgeForward: {
    chord: "Ctrl+ArrowRight",
    workflow: "move-marker",
    scope: "tracks",
    label: key("keys.nudge.forward"),
  },
  detect: {
    chord: "t",
    workflow: "detect-tracks",
    scope: "tracks",
    label: key("keys.detect"),
  },
  rename: {
    chord: "Enter",
    workflow: "edit-track-metadata",
    scope: "tracks",
    label: key("keys.rename"),
  },
  lookup: {
    chord: "l",
    workflow: "search-metadata",
    scope: "metadata",
    label: key("keys.lookup"),
  },
  identify: {
    chord: "i",
    workflow: "search-metadata",
    scope: "metadata",
    label: key("keys.identify"),
  },
  accept: {
    chord: "Enter",
    workflow: "choose-release",
    scope: "metadata",
    label: key("keys.accept"),
  },
  openProject: {
    chord: "Enter",
    workflow: "navigate",
    scope: "browser",
    label: key("keys.open.project"),
  },
  newProject: {
    chord: "n",
    workflow: "navigate",
    scope: "browser",
    label: key("keys.new.project"),
  },
  // --- Moving a selection ---------------------------------------------------
  //
  // WP-16a added these, and first light is why. Every binding above that acts
  // on "the selected" something - open the selected project, edit the selected
  // track, delete the selected marker, accept the selected release - had a
  // handler, and `wiring.test.ts` proved it. What none of them had was a way to
  // *make* the selection: the rows answered `onClick` and nothing else, so
  // five of §44's workflows needed a mouse to reach their own first step.
  //
  // `ArrowUp` and `ArrowDown`, unmodified, which are free because the global
  // seek took the horizontal pair. They are scoped per panel rather than
  // global: a list is the only thing a vertical arrow could mean, and what the
  // list holds differs, so the label can say what moves.
  //
  // The tracks panel has two lists and needs both. `Shift` picks the boundary
  // list, on the same argument §21's skip uses Shift for the coarser move.
  previousProject: {
    chord: "ArrowUp",
    workflow: "navigate",
    scope: "browser",
    label: key("keys.previous.project"),
  },
  nextProject: {
    chord: "ArrowDown",
    workflow: "navigate",
    scope: "browser",
    label: key("keys.next.project"),
  },
  previousTrack: {
    chord: "ArrowUp",
    workflow: "edit-track-metadata",
    scope: "tracks",
    label: key("keys.previous.track"),
  },
  nextTrack: {
    chord: "ArrowDown",
    workflow: "edit-track-metadata",
    scope: "tracks",
    label: key("keys.next.track"),
  },
  previousBoundary: {
    chord: "Shift+ArrowUp",
    workflow: "move-marker",
    scope: "tracks",
    label: key("keys.previous.boundary"),
  },
  nextBoundary: {
    chord: "Shift+ArrowDown",
    workflow: "move-marker",
    scope: "tracks",
    label: key("keys.next.boundary"),
  },
  previousCandidate: {
    chord: "ArrowUp",
    workflow: "choose-release",
    scope: "metadata",
    label: key("keys.previous.candidate"),
  },
  nextCandidate: {
    chord: "ArrowDown",
    workflow: "choose-release",
    scope: "metadata",
    label: key("keys.next.candidate"),
  },
  // --- Seeing what you are about to mark -----------------------------------
  //
  // §20 requires horizontal zoom and pan and "zoom to selection/track", and
  // the workflow they serve is §44's *marker editing*. A whole side of vinyl
  // drawn across this canvas is about half a second to the column, and nobody
  // places the start of a track to half a second - so zooming is not a
  // convenience beside marker editing, it is what makes marker editing
  // possible by eye. That is why these declare `place-marker` and
  // `move-marker` rather than a `zoom` workflow of their own: §44 does not ask
  // for zooming, and a `Workflow` member that no requirement names would make
  // the coverage record describe something other than the requirement.
  //
  // Global scope, because the waveform is on five of the six panels and the
  // visible range is one piece of state across all of them. In the library it
  // does nothing, which is already true of every transport binding there.
  //
  // `+` and `-` as every viewer spells them, and `=` as well - `+` is a shifted
  // key and `=` is the unshifted twin under the same finger, so a person who
  // does not reach for Shift still zooms in. `+` is also the one chord the
  // map's own `+`-joined spelling cannot be taken apart by splitting; see
  // `spelled` in `keymap.test.ts`.
  zoomIn: {
    chord: "+",
    also: "=",
    workflow: "place-marker",
    scope: "global",
    label: key("keys.zoom.in"),
  },
  zoomOut: {
    chord: "-",
    workflow: "place-marker",
    scope: "global",
    label: key("keys.zoom.out"),
  },
  zoomFit: {
    chord: "0",
    workflow: "place-marker",
    scope: "global",
    label: key("keys.zoom.fit"),
  },
  zoomSelection: {
    chord: "z",
    workflow: "move-marker",
    scope: "global",
    label: key("keys.zoom.selection"),
  },
  help: {
    chord: "?",
    also: "F1",
    workflow: "help",
    scope: "global",
    label: key("keys.help"),
  },

  // --- Getting between the panels ------------------------------------------
  //
  // WP-16 added these, and they are the reason the rest of the map works. A
  // binding scoped to `tracks` is unreachable unless something can put the
  // tracks panel in front of the person, so §44's "completable by keyboard
  // alone" is a claim about these six as much as about the verbs they lead to.
  //
  // `Ctrl` and a digit, in the order the panels sit in: a bare digit would be
  // taken away the moment somebody types a catalog number, and `typing`
  // cannot tell a digit meant for a field from one meant for us until it is
  // already too late.
  gotoBrowser: {
    chord: "Ctrl+1",
    workflow: "navigate",
    scope: "global",
    label: key("keys.goto.browser"),
  },
  gotoCapture: {
    chord: "Ctrl+2",
    workflow: "navigate",
    scope: "global",
    label: key("keys.goto.capture"),
  },
  gotoTracks: {
    chord: "Ctrl+3",
    workflow: "navigate",
    scope: "global",
    label: key("keys.goto.tracks"),
  },
  gotoMetadata: {
    chord: "Ctrl+4",
    workflow: "navigate",
    scope: "global",
    label: key("keys.goto.metadata"),
  },
  gotoExport: {
    chord: "Ctrl+5",
    workflow: "navigate",
    scope: "global",
    label: key("keys.goto.export"),
  },
  gotoSettings: {
    chord: "Ctrl+6",
    workflow: "navigate",
    scope: "global",
    label: key("keys.goto.settings"),
  },
  log: {
    chord: "Ctrl+d",
    workflow: "navigate",
    scope: "global",
    label: key("keys.log"),
  },
  // The only overlay that had no chord at all, which made §44's "no workflow
  // needs the mouse" false for the one panel a person opens to answer "what
  // version is this" - the first question of every bug report. `Ctrl+I` for
  // information: `Ctrl+A` is select-all inside every text field in the product
  // and `chord()` cannot tell `Ctrl+Shift+A` from it, because Shift is carried
  // by the character for a single-character key.
  about: {
    chord: "Ctrl+i",
    workflow: "help",
    scope: "global",
    label: key("keys.about"),
  },
  dismiss: {
    chord: "Escape",
    workflow: "navigate",
    scope: "global",
    label: key("keys.dismiss"),
  },
} satisfies Record<string, Binding> & { [K in string]: Binding };

/** The action names, as the panels refer to them. */
export type Action = keyof typeof BINDINGS;

/**
 * Every §44 workflow the map covers.
 *
 * Declared as the exhaustive record, so a `Workflow` added above without a
 * binding fails to compile. This is the coverage check, and it runs in
 * `pnpm check` with no test runner involved.
 */
export const COVERAGE: Record<Workflow, readonly Action[]> = {
  "choose-device": ["chooseDevice"],
  "choose-rate-format": ["chooseFormat"],
  arm: ["arm"],
  record: ["record"],
  pause: ["pause"],
  stop: ["stop"],
  play: ["play"],
  seek: ["seekBack", "seekForward"],
  skip: ["skipBack", "skipForward"],
  "place-marker": ["marker", "zoomIn", "zoomOut", "zoomFit"],
  "move-marker": [
    "nudgeBack",
    "nudgeForward",
    "previousBoundary",
    "nextBoundary",
    "zoomSelection",
  ],
  "delete-marker": ["deleteMarker"],
  "detect-tracks": ["detect"],
  "edit-track-metadata": ["rename", "previousTrack", "nextTrack"],
  "search-metadata": ["lookup", "identify"],
  "choose-release": ["accept", "previousCandidate", "nextCandidate"],
  export: ["exportRun"],
  checkpoint: ["checkpoint"],
  navigate: [
    "openProject",
    "newProject",
    "previousProject",
    "nextProject",
    "gotoBrowser",
    "gotoCapture",
    "gotoTracks",
    "gotoMetadata",
    "gotoExport",
    "gotoSettings",
    "log",
    "dismiss",
  ],
  help: ["help", "about"],
};

/**
 * The whole map, as pairs typed against [`Binding`].
 *
 * `BINDINGS` is declared with `satisfies`, which keeps each entry's narrow
 * literal type - useful at a call site that names one action, and awkward
 * everywhere that walks the map, because an entry with no `also` does not have
 * the property at all. Widening once here is better than a cast at each of the
 * four places that iterate.
 */
export function entries(): [Action, Binding][] {
  return Object.entries(BINDINGS) as [Action, Binding][];
}

/** Every chord a binding answers to. */
export function spellings(binding: Binding): string[] {
  return binding.also === undefined
    ? [binding.chord]
    : [binding.chord, binding.also];
}

/** The bindings that apply in a scope, including the global ones. */
export function inScope(scope: Scope): [Action, Binding][] {
  return entries().filter(
    ([, binding]) => binding.scope === scope || binding.scope === "global",
  );
}
