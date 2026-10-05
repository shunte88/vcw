/*
 *  App.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The window: one panel at a time, the transport under it, one status line (§34).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// §34's layout, and the reasoning behind the shape rather than the shape itself.
//
// One panel at a time, not a workspace of docked views. A vinyl rip is a
// sequence - capture, detect, name, export - and at each step there is one thing
// a person is doing and a handful of facts they need beside it. Six panels in
// one window would mean six of everything competing for the same 1400 pixels
// and a keyboard map where every chord needs a focus rule. The transport and
// the meters are the exception and stay on screen always, because they are what
// a person watches while a record plays and they are the two things that are
// true regardless of which panel is in front.
//
// # Two shapes, not one
//
// The library is a different kind of screen from the rest and gets a different
// shape. It answers "which record" and it is a table of them, so it takes the
// whole height. The other five answer "what next for this record", and every
// one of them is read against the picture of the audio - so on those the page
// splits in half: the waveform above with a ruler on each edge, the panel in
// focus below.
//
// The first draft had the waveform in the always-on strip at a fixed 160
// pixels, under every panel including the library. Measured on the real
// project at 1600x1000 that left between 350 and 500 pixels of the panel
// region empty on four of the six panels, while the one thing a person looks
// at got 160. A half is not a guess at the right number; it is the number that
// makes the two things a person is comparing - the audio and the decision
// about it - the same size.
//
// # Where state lives, exactly
//
// - `useEngine` holds what Rust last said. It is the only subscription.
// - This component holds which panel is in front and which overlay is open.
// - Each panel holds its own form fields and its own selection.
//
// Nothing holds a second copy of a row. The device list, the settings and the
// credential list are read here rather than in the panels that show them,
// because three panels want the settings and three reads of the same file is
// three chances to render two versions of it.
//
// # Why the scope is derived and not stored
//
// `useKeys` needs a scope, and the scope *is* the panel in front - there is no
// third thing it could be. Storing it separately would create the possibility
// of the two disagreeing, which is a whole class of bug ("why does `t` not
// work") that cannot exist if the value is computed.

import { useCallback, useEffect, useState } from "react";

import * as api from "./api";
import type {
  Credential,
  Device,
  Project,
  Settings as Values,
} from "./bindings/vcw";
import { describe } from "./describe";
import { useKeys } from "./keys";
import type { Scope } from "./keymap";
import { Mark } from "./Mark";
import { useEngine } from "./store";
import type { ProjectState } from "./store";
import { Browser } from "./panels/Browser";
import { Capture } from "./panels/Capture";
import { Diagnostics } from "./panels/Diagnostics";
import { Export } from "./panels/Export";
import { About } from "./panels/About";
import { Help } from "./panels/Help";
import { Meters } from "./panels/Meters";
import { SelectionBar } from "./panels/SelectionBar";
import { Metadata } from "./panels/Metadata";
import { Settings } from "./panels/Settings";
import { Tracks } from "./panels/Tracks";
import { Transport } from "./panels/Transport";
import { Waveform, type Region, type View } from "./panels/Waveform";
import type { Chosen } from "./panels/Tracks";
import { STEP, type Span, fit, toRange, toSelection, zoom } from "./zoom";

/**
 * The panels a person can be in, in the order the work happens.
 *
 * `browser` is labeled *Library* rather than *Projects*: what it lists are
 * records, and "project" is the file they happen to be kept in. The scope is
 * unchanged, so `keymap.ts`, the bindings and `wiring.test.ts` do not move.
 */
const PANELS: readonly { scope: Scope; label: string; chord: string }[] = [
  { scope: "browser", label: "Library", chord: "Ctrl+1" },
  { scope: "capture", label: "Capture", chord: "Ctrl+2" },
  { scope: "tracks", label: "Tracks", chord: "Ctrl+3" },
  { scope: "metadata", label: "Metadata", chord: "Ctrl+4" },
  { scope: "export", label: "Export", chord: "Ctrl+5" },
  { scope: "settings", label: "Settings", chord: "Ctrl+6" },
];

/**
 * The panels the waveform belongs above.
 *
 * Not "every panel but the library", which is what it was. Settings and
 * Metadata are forms about the project as a whole, and a 420 px picture of the
 * audio above them is not context - it is two fifths of the window spent on
 * something neither panel can act on, pushing the thing a person came to read
 * below the fold. The three that are left all point at positions in the
 * recording: Capture is watching it arrive, Tracks is cutting it up, and Export
 * is choosing which of those cuts go out.
 */
const STAGE: readonly Scope[] = ["capture", "tracks", "export"];

/**
 * What the title bar calls the open project.
 *
 * An identified release is "Artist - Album", except that `albumArtist` is a
 * string and an unidentified release has it empty, so the imported Audacity
 * projects - which carry an album tag and no artist tag - were titling the
 * window " - Tomorrow's Harvest (Vinyl)", leading dash and all. Either half
 * may be missing, so the join is over the halves that are there.
 *
 * A project with no release at all falls back to its path, and no project to a
 * sentence. The file name is not used as a third fallback: two sides of the
 * same record are two files in one directory, and the name alone would not say
 * which was open.
 */
export function titled(project: ProjectState): string {
  if (project.release === null) {
    return project.path ?? "no project open";
  }
  const parts = [project.release.albumArtist, project.release.album].filter(
    (part) => part !== "",
  );
  return parts.length === 0 ? (project.path ?? "untitled") : parts.join(" - ");
}

/**
 * What to do next, for the status line before anything has happened.
 *
 * It said "Ready. Press ? for the keyboard map." whatever the state was, which
 * is true and is not help: a person who has just opened the application has no
 * project, and a person who has just opened a project is one keystroke from
 * recording and is not told which. The keyboard map is still offered, at the
 * end, because that is where a general offer belongs.
 *
 * Only shown until the first event arrives, after which the line is the log
 * and this would be overwriting a fact with a suggestion.
 */
export function next(project: ProjectState, phase: string): string {
  if (project.path === null) {
    return "No project open. Ctrl+1 for the library, n for a new one, ? for the keyboard map.";
  }
  if (phase === "armed") {
    return "Armed. Press r to record, or ? for the keyboard map.";
  }
  if (project.captures.length === 0) {
    return "Nothing captured yet. Ctrl+2 to pick a device, a to arm it, ? for the keyboard map.";
  }
  if (project.tracks.length === 0) {
    return "Captured. Ctrl+3 and t to find the track boundaries, ? for the keyboard map.";
  }
  return "Ready. Ctrl+4 to identify the record, Ctrl+5 to export, ? for the keyboard map.";
}

/** The window. */
export function App(): React.JSX.Element {
  const store = useEngine();
  const { engine, project } = store;

  const [panel, setPanel] = useState<Scope>("browser");
  const [help, setHelp] = useState(false);
  const [about, setAbout] = useState(false);
  const [log, setLog] = useState(false);

  const [devices, setDevices] = useState<readonly Device[]>([]);
  const [settings, setSettings] = useState<Values | null>(null);
  const [credentials, setCredentials] = useState<readonly Credential[]>([]);
  const [library, setLibrary] = useState<readonly Project[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [chosen, setChosen] = useState<Chosen>({ track: null, boundary: null });

  // Read once at start, and again when the settings change - the library list
  // depends on `recording.library`, so a person who sets it should see the
  // browser fill in rather than be told to restart.
  const [generation, setGeneration] = useState(0);
  const reread = useCallback(() => setGeneration((n) => n + 1), []);

  // Through `store.run`, which is the same reason `store.ts` grew a `catch`:
  // an unhandled `Promise.all` here meant one failing read left the devices,
  // the settings, the credentials *and* the library on their previous values
  // with nothing said about it. Four panels describing a state that has gone.
  // `run` puts the reason in the status line instead. `store.run` is stable,
  // so the dependency stays `generation` alone.
  useEffect(() => {
    let canceled = false;
    void store.run(async () => {
      const [found, values, secrets, projects] = await Promise.all([
        api.devices(),
        api.settings(),
        api.credentials(),
        api.projects(),
      ]);
      if (!canceled) {
        setDevices(found);
        setSettings(values);
        setCredentials(secrets);
        setLibrary(projects);
      }
    });
    return () => {
      canceled = true;
    };
  }, [generation]);

  // # Which capture and which side are "current"
  //
  // Decided here, once, and handed to both the waveform and the transport,
  // because the project has no such notion and two components inventing one
  // would invent two. The rules, in order:
  //
  //  - the capture the audition is reading, if something is playing: a person
  //    watching a playhead move expects the picture under it to be the thing
  //    they are hearing;
  //  - otherwise the last capture in the project, which is the one just made;
  //  - and the side is whichever side names that capture. Not "the first side
  //    with a capture" - §21 allows two faces on one capture, so that guess
  //    would put a marker on side A of a record cued to side B. When the
  //    answer is genuinely ambiguous it is `null`, and the marker key does
  //    nothing rather than something arbitrary.
  const capture =
    project.captures.find((row) => row.id === engine.auditioning) ??
    project.captures[project.captures.length - 1];
  const naming = project.sides.filter((row) => row.captureId === capture?.id);
  const side = naming.length === 1 ? (naming[0] ?? null) : null;

  // What the picture is allowed to draw on itself, which is a different
  // question from which side is current. `naming` rather than `side`: §21
  // allows one capture to hold two faces, and in that case there is no single
  // current side but both faces' marks are genuinely in this capture's
  // timeline - so the picture shows them and the marker key still does nothing.
  //
  // Filtered rather than passed whole, and this is not tidiness. A boundary's
  // `atFrame` is a position "in that side's capture timeline", so a boundary
  // from a different side is a number about a different recording: the fixture
  // project has a side A and a side B of the same lengths, and drawing both
  // gave a picture of side B with side A's lines on it. They agreed by
  // coincidence, which is exactly the kind of agreement that stops as soon as
  // two sides differ.
  const held = new Set(naming.map((row) => row.id));
  const labels = project.tracks.filter((row) => held.has(row.sideId));
  const marks = project.boundaries.filter((row) => held.has(row.sideId));
  const [view, setView] = useState<View | null>(null);
  // # The selection
  //
  // Here rather than in the waveform, because the waveform is where it is
  // drawn and the transport is where it is used: a band means "play this",
  // and §21's region audition is a transport verb. Two panels, one fact, so
  // it belongs to the thing that holds both.
  //
  // In seconds, because that is what `api.play` takes and what the waveform
  // is given; and cleared with the capture, because a stretch of one
  // recording means nothing in another.
  const [selection, setSelection] = useState<Region | null>(null);
  useEffect(() => {
    setSelection(null);
    if (capture !== undefined) {
      setView({
        captureId: capture.id,
        channel: 0,
        startFrame: 0,
        endFrame: null,
      });
    } else {
      setView(null);
    }
  }, [capture?.id]);


  // # The visible range
  //
  // Held here rather than in the waveform, because three things move it and
  // only one of them is the waveform: the wheel and the bar are its own, the
  // zoom keys are the keyboard map's, and opening a different capture resets
  // it. `zoom.ts` holds every rule about what a range may be; this holds which
  // range it currently is.
  const frames = capture?.frames ?? 0;
  const visible: Span =
    view === null
      ? fit()
      : { startFrame: view.startFrame, endFrame: view.endFrame };

  /** Show a different range of the current capture. */
  const reframe = (next: Span) => {
    if (view !== null) {
      setView({ ...view, ...next });
    }
  };

  useKeys("global", {
    gotoBrowser: () => setPanel("browser"),
    gotoCapture: () => setPanel("capture"),
    gotoTracks: () => setPanel("tracks"),
    gotoMetadata: () => setPanel("metadata"),
    gotoExport: () => setPanel("export"),
    gotoSettings: () => setPanel("settings"),
    zoomIn: () => reframe(zoom(visible, frames, 1 / STEP, 0.5)),
    zoomOut: () => reframe(zoom(visible, frames, STEP, 0.5)),
    zoomFit: () => reframe(fit()),
    zoomSelection: () => {
      if (capture === undefined) {
        return;
      }
      // A band on the waveform before either of the others. It is the only
      // one of the three a person drew by hand, and they drew it a moment
      // ago: "zoom to the selection" can hardly mean anything else while
      // there is one. Seconds to frames here, like the track below.
      if (selection !== null) {
        reframe(
          toSelection(
            Math.round(selection.from * capture.rate),
            Math.round(selection.to * capture.rate),
            capture.frames,
          ),
        );
        return;
      }
      // The boundary before the track, because choosing one is the finer act:
      // a person with a marker selected is working on that marker, and the
      // track it happens to fall inside is not what they asked to see.
      const marker = project.boundaries.find(
        (row) => row.id === chosen.boundary,
      );
      if (marker !== undefined) {
        reframe(toRange(marker.atFrame, marker.atFrame, capture.frames));
        return;
      }
      // Seconds to frames here rather than in the waveform, and this is the
      // one place in the frontend that should do it: the rule for *which*
      // capture is current is decided above, so the rate to convert by is
      // already known here. The waveform is given a range and never a rate.
      const track = project.tracks.find((row) => row.id === chosen.track);
      if (track !== undefined) {
        reframe(
          toRange(
            Math.round(track.start * capture.rate),
            Math.round(track.end * capture.rate),
            capture.frames,
          ),
        );
      }
    },
    help: () => setHelp((open) => !open),
    log: () => setLog((open) => !open),
    dismiss: () => {
      // One key, three things it could mean, in the order a person expects:
      // close what is on top, then clear the refusal, then nothing.
      if (about) {
        setAbout(false);
      } else if (help) {
        setHelp(false);
      } else if (log) {
        setLog(false);
      } else {
        store.dismiss();
      }
    },
  });

  const last = engine.log[engine.log.length - 1];

  return (
    <div className="app">
      <header className="titlebar">
        <h1>
          <Mark />
          VCW
        </h1>
        <span className="project">{titled(project)}</span>
        <nav className="tabs">
          {PANELS.map((tab) => (
            <button
              key={tab.scope}
              type="button"
              className={tab.scope === panel ? "current" : ""}
              onClick={() => setPanel(tab.scope)}
              title={tab.chord}
            >
              {tab.label}
            </button>
          ))}
        </nav>
        <button type="button" onClick={() => setLog(true)} title="Ctrl+D">
          Log
        </button>
        <button type="button" onClick={() => setHelp(true)} title="?">
          Keys
        </button>
        <button type="button" onClick={() => setAbout(true)}>
          About
        </button>
      </header>

      <main className={STAGE.includes(panel) ? "project" : "library"}>
        {STAGE.includes(panel) && (
          <div className="stage-top">
            <Waveform
              capture={capture}
              view={view}
              onView={setView}
              boundaries={marks}
              tracks={labels}
              playhead={engine.playhead}
              playing={engine.playing}
              selection={selection}
              onSelect={setSelection}
              generation={generation}
              onSeek={(seconds) => {
                if (engine.playing) {
                  void store.run(() =>
                    api.playback({ verb: "seek", to: seconds }),
                  );
                } else if (capture !== undefined) {
                  void store.run(() =>
                    api.play(capture.id, {
                      scope: "region",
                      start: seconds,
                      end: capture.seconds,
                    }),
                  );
                }
              }}
            />
          </div>
        )}
        <div className="stage-pane">
          {panel === "browser" && (
            <Browser
              store={store}
              projects={library}
              selected={selected}
              onSelect={setSelected}
              onLibraryChanged={reread}
            />
          )}
          {panel === "capture" && (
            <Capture store={store} devices={devices} settings={settings} />
          )}
          {panel === "tracks" && (
            <Tracks store={store} chosen={chosen} onChoose={setChosen} />
          )}
          {panel === "metadata" && <Metadata store={store} />}
          {panel === "export" && <Export store={store} settings={settings} />}
          {panel === "settings" && (
            <Settings
              store={store}
              settings={settings}
              credentials={credentials}
              devices={devices}
              onSaved={reread}
            />
          )}
        </div>
      </main>

      <aside className="always">
        {/* Only where there is a waveform to have selected something in. */}
        {STAGE.includes(panel) && (
          <SelectionBar selection={selection} meter={engine.meter} />
        )}
        {/* Live when the device is open, which is everything but `idle`:
            §50 sets the level while armed, so the input is metering long
            before anything is recorded. The output is live only while
            something is auditioning. */}
        <Meters
          meter={engine.meter}
          output={engine.output}
          capturing={engine.phase !== "idle"}
          playing={engine.playing}
        />
        <Transport
          store={store}
          capture={capture}
          side={side}
          selection={selection}
        />
      </aside>

      <footer className="status">
        {engine.refusal !== null ? (
          <span className="refusal">
            <strong>{engine.refusal.code}</strong> {engine.refusal.message}
            {engine.refusal.field !== null && ` (${engine.refusal.field})`}
            <button type="button" onClick={store.dismiss}>
              Dismiss (Esc)
            </button>
          </span>
        ) : (
          <span className="dim">
            {last === undefined
              ? next(project, engine.phase)
              : `${last.event.kind}: ${describe(last.event)}`}
          </span>
        )}
      </footer>

      {help && <Help scope={panel} onClose={() => setHelp(false)} />}
      {about && <About onClose={() => setAbout(false)} />}
      {log && (
        <Diagnostics log={engine.log} onClose={() => setLog(false)} />
      )}
    </div>
  );
}
