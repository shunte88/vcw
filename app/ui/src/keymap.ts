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
  /** What the help overlay shows. */
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
    label: "Play or pause the audition",
    suggested: true,
  },
  record: {
    chord: "r",
    workflow: "record",
    scope: "global",
    label: "Start recording",
    suggested: true,
  },
  stop: {
    chord: "s",
    workflow: "stop",
    scope: "global",
    label: "Stop",
    suggested: true,
  },
  marker: {
    chord: "m",
    workflow: "place-marker",
    scope: "global",
    label: "Place a marker at the playhead",
    suggested: true,
  },
  deleteMarker: {
    chord: "Delete",
    also: "Backspace",
    workflow: "delete-marker",
    scope: "tracks",
    label: "Delete the selected marker",
    suggested: true,
  },
  seekBack: {
    chord: "ArrowLeft",
    workflow: "seek",
    scope: "global",
    label: "Seek back",
    suggested: true,
  },
  seekForward: {
    chord: "ArrowRight",
    workflow: "seek",
    scope: "global",
    label: "Seek forward",
    suggested: true,
  },
  checkpoint: {
    chord: "Ctrl+s",
    workflow: "checkpoint",
    scope: "global",
    label: "Commit what has been recorded so far",
    suggested: true,
  },
  exportRun: {
    chord: "Ctrl+e",
    workflow: "export",
    scope: "global",
    label: "Export",
    suggested: true,
  },

  // --- The rest, which are ours -------------------------------------------
  pause: {
    chord: "p",
    workflow: "pause",
    scope: "global",
    label: "Pause or resume recording",
  },
  arm: {
    chord: "a",
    workflow: "arm",
    scope: "capture",
    label: "Arm the chosen device on the open project",
  },
  chooseDevice: {
    chord: "d",
    workflow: "choose-device",
    scope: "capture",
    label: "Focus the device list",
  },
  chooseFormat: {
    chord: "f",
    workflow: "choose-rate-format",
    scope: "capture",
    label: "Focus the rate and format fields",
  },
  skipForward: {
    chord: "Shift+ArrowRight",
    workflow: "skip",
    scope: "global",
    label: "Skip to the next track",
  },
  skipBack: {
    chord: "Shift+ArrowLeft",
    workflow: "skip",
    scope: "global",
    label: "Skip to the previous track",
  },
  nudgeBack: {
    chord: "Ctrl+ArrowLeft",
    workflow: "move-marker",
    scope: "tracks",
    label: "Nudge the selected marker earlier",
  },
  nudgeForward: {
    chord: "Ctrl+ArrowRight",
    workflow: "move-marker",
    scope: "tracks",
    label: "Nudge the selected marker later",
  },
  detect: {
    chord: "t",
    workflow: "detect-tracks",
    scope: "tracks",
    label: "Run track detection over the capture",
  },
  rename: {
    chord: "Enter",
    workflow: "edit-track-metadata",
    scope: "tracks",
    label: "Edit the selected track",
  },
  lookup: {
    chord: "l",
    workflow: "search-metadata",
    scope: "metadata",
    label: "Look the record up with a provider",
  },
  accept: {
    chord: "Enter",
    workflow: "choose-release",
    scope: "metadata",
    label: "Accept the selected release",
  },
  openProject: {
    chord: "Enter",
    workflow: "navigate",
    scope: "browser",
    label: "Open the selected project",
  },
  newProject: {
    chord: "n",
    workflow: "navigate",
    scope: "browser",
    label: "Create a project",
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
    label: "Select the project above",
  },
  nextProject: {
    chord: "ArrowDown",
    workflow: "navigate",
    scope: "browser",
    label: "Select the project below",
  },
  previousTrack: {
    chord: "ArrowUp",
    workflow: "edit-track-metadata",
    scope: "tracks",
    label: "Select the track above",
  },
  nextTrack: {
    chord: "ArrowDown",
    workflow: "edit-track-metadata",
    scope: "tracks",
    label: "Select the track below",
  },
  previousBoundary: {
    chord: "Shift+ArrowUp",
    workflow: "move-marker",
    scope: "tracks",
    label: "Select the marker above",
  },
  nextBoundary: {
    chord: "Shift+ArrowDown",
    workflow: "move-marker",
    scope: "tracks",
    label: "Select the marker below",
  },
  previousCandidate: {
    chord: "ArrowUp",
    workflow: "choose-release",
    scope: "metadata",
    label: "Select the candidate above",
  },
  nextCandidate: {
    chord: "ArrowDown",
    workflow: "choose-release",
    scope: "metadata",
    label: "Select the candidate below",
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
    label: "Zoom the waveform in",
  },
  zoomOut: {
    chord: "-",
    workflow: "place-marker",
    scope: "global",
    label: "Zoom the waveform out",
  },
  zoomFit: {
    chord: "0",
    workflow: "place-marker",
    scope: "global",
    label: "Show the whole capture",
  },
  zoomSelection: {
    chord: "z",
    workflow: "move-marker",
    scope: "global",
    label: "Zoom to the selection, the selected marker, or the selected track",
  },
  help: {
    chord: "?",
    also: "F1",
    workflow: "help",
    scope: "global",
    label: "Show the keyboard map",
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
    label: "Library",
  },
  gotoCapture: {
    chord: "Ctrl+2",
    workflow: "navigate",
    scope: "global",
    label: "Capture",
  },
  gotoTracks: {
    chord: "Ctrl+3",
    workflow: "navigate",
    scope: "global",
    label: "Tracks",
  },
  gotoMetadata: {
    chord: "Ctrl+4",
    workflow: "navigate",
    scope: "global",
    label: "Metadata",
  },
  gotoExport: {
    chord: "Ctrl+5",
    workflow: "navigate",
    scope: "global",
    label: "Export",
  },
  gotoSettings: {
    chord: "Ctrl+6",
    workflow: "navigate",
    scope: "global",
    label: "Settings",
  },
  log: {
    chord: "Ctrl+d",
    workflow: "navigate",
    scope: "global",
    label: "Show the event log",
  },
  dismiss: {
    chord: "Escape",
    workflow: "navigate",
    scope: "global",
    label: "Close the overlay, or clear the last refusal",
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
  "search-metadata": ["lookup"],
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
  help: ["help"],
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
