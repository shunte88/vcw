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
import { useEngine } from "./store";
import { Browser } from "./panels/Browser";
import { Capture } from "./panels/Capture";
import { Diagnostics } from "./panels/Diagnostics";
import { Export } from "./panels/Export";
import { Help } from "./panels/Help";
import { Meters } from "./panels/Meters";
import { Metadata } from "./panels/Metadata";
import { Settings } from "./panels/Settings";
import { Tracks } from "./panels/Tracks";
import { Transport } from "./panels/Transport";
import { Waveform, type View } from "./panels/Waveform";
import type { Chosen } from "./panels/Tracks";

/** The panels a person can be in, in the order the work happens. */
const PANELS: readonly { scope: Scope; label: string; chord: string }[] = [
  { scope: "browser", label: "Projects", chord: "Ctrl+1" },
  { scope: "capture", label: "Capture", chord: "Ctrl+2" },
  { scope: "tracks", label: "Tracks", chord: "Ctrl+3" },
  { scope: "metadata", label: "Metadata", chord: "Ctrl+4" },
  { scope: "export", label: "Export", chord: "Ctrl+5" },
  { scope: "settings", label: "Settings", chord: "Ctrl+6" },
];

/** The window. */
export function App(): React.JSX.Element {
  const store = useEngine();
  const { engine, project } = store;

  const [panel, setPanel] = useState<Scope>("browser");
  const [help, setHelp] = useState(false);
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

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [found, values, secrets, projects] = await Promise.all([
        api.devices(),
        api.settings(),
        api.credentials(),
        api.projects(),
      ]);
      if (!cancelled) {
        setDevices(found);
        setSettings(values);
        setCredentials(secrets);
        setLibrary(projects);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [generation]);

  useKeys("global", {
    gotoBrowser: () => setPanel("browser"),
    gotoCapture: () => setPanel("capture"),
    gotoTracks: () => setPanel("tracks"),
    gotoMetadata: () => setPanel("metadata"),
    gotoExport: () => setPanel("export"),
    gotoSettings: () => setPanel("settings"),
    help: () => setHelp((open) => !open),
    log: () => setLog((open) => !open),
    dismiss: () => {
      // One key, three things it could mean, in the order a person expects:
      // close what is on top, then clear the refusal, then nothing.
      if (help) {
        setHelp(false);
      } else if (log) {
        setLog(false);
      } else {
        store.dismiss();
      }
    },
  });

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
  const [view, setView] = useState<View | null>(null);
  useEffect(() => {
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

  const last = engine.log[engine.log.length - 1];

  return (
    <div className="app">
      <header className="titlebar">
        <h1>VCW</h1>
        <span className="project">
          {project.release === null
            ? (project.path ?? "no project open")
            : `${project.release.albumArtist} - ${project.release.album}`}
        </span>
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
      </header>

      <main>
        {panel === "browser" && (
          <Browser
            store={store}
            projects={library}
            selected={selected}
            onSelect={setSelected}
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
            onSaved={reread}
          />
        )}
      </main>

      <aside className="always">
        {view !== null && (
          <Waveform
            capture={capture}
            view={view}
            boundaries={project.boundaries}
            playhead={engine.playhead}
            playing={engine.playing}
            generation={generation}
            onSeek={(seconds) => {
              if (engine.playing) {
                void store.run(() => api.playback({ verb: "seek", to: seconds }));
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
        )}
        <Meters meter={engine.meter} />
        <Transport store={store} capture={capture} side={side} />
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
              ? "Ready. Press ? for the keyboard map."
              : `${last.event.kind}: ${describe(last.event)}`}
          </span>
        )}
      </footer>

      {help && <Help scope={panel} onClose={() => setHelp(false)} />}
      {log && (
        <Diagnostics log={engine.log} onClose={() => setLog(false)} />
      )}
    </div>
  );
}
