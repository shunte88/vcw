/*
 *  Export.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Planning an export, then running it (§33).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Plan, look, run. §33 resolves the whole export before it writes a byte, and
// the plan is a value the shell will hand over - so the panel shows it, with
// every path exactly as it will be on disk. A naming template with a field the
// project has not filled in produces a visible `Unknown Artist` in the plan
// rather than a surprise in the output directory, and an overwrite is refused
// by the planner rather than discovered afterwards.
//
// Nothing here renders a filename. The template is substituted in `vcw-export`,
// where the sanitising rules live, because a path this panel composed and a
// path the writer composed would differ on the first track whose title has a
// slash in it.
//
// The one native dialog in the application is here. Typing an absolute output
// path from memory was the roughest edge in the panel: a person at the
// turntable with a finished side had to leave the window, find the directory in
// a file manager and paste it back. `Browse` is `dialog:allow-open` in
// `capabilities/default.json` and nothing else - open a directory, never save a
// file, because every filename VCW writes comes from the template.

import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import * as api from "../api";
import type { ExportPlan, Settings } from "../bindings/vcw";
import { effortOf } from "../effort";
import { useKeys } from "../keys";
import type { Exported, Store } from "../store";

/** The export panel. */
export function Export({
  store,
  settings,
}: {
  store: Store;
  settings: Settings | null;
}): React.JSX.Element {
  const { project, engine, run } = store;
  const [into, setInto] = useState("");
  const [format, setFormat] = useState("flac");
  const [quality, setQuality] = useState("high");
  const [compression, setCompression] = useState("5");
  const [artwork, setArtwork] = useState("embed");
  const [overwrite, setOverwrite] = useState(false);
  const [sides, setSides] = useState<readonly string[]>([]);
  const [plan, setPlan] = useState<ExportPlan | null>(null);

  // §39's defaults, taken once. The panel is the override; the settings panel
  // is where a default is changed.
  const [seeded, setSeeded] = useState(false);
  if (!seeded && settings !== null) {
    setSeeded(true);
    setInto(settings.export.output ?? "");
    setFormat(settings.export.format);
    setQuality(settings.export.quality);
    setCompression(settings.export.compression);
    setArtwork(settings.export.artwork);
  }

  // Which encoder knob this format has, if either.
  const effort = effortOf(format);

  const request = () => ({
    into,
    format,
    // Both sent whatever the format is. The backend's `with_quality` and
    // `with_compression` are no-ops on the containers that have no such
    // setting, so there is no branch here and no way for the two to disagree
    // about when a field matters.
    quality,
    compression,
    template: settings?.export.template ?? null,
    sides: [...sides],
    artwork,
    overwrite,
  });

  // Cancelling returns null and a cancelled picker must not clear the field: a
  // person who opens the dialog to look and changes their mind still has the
  // path they typed. The plan is dropped either way, because a plan resolved
  // against the old directory describes files nobody asked for.
  const browse = () => {
    void run(async () => {
      // Somewhere useful to start. Left to itself the chooser opens on the
      // process working directory, which for a dev run is `app/src-tauri` and
      // for a packaged one is wherever the launcher happened to be: the source
      // tree is the one place an export definitely does not belong. The library
      // root is where this person already keeps records, so it is the guess
      // worth making, and `null` there means they have not picked one and the
      // host should decide.
      //
      // Spread rather than set to undefined, because `exactOptionalPropertyTypes`
      // draws the distinction the dialog's own types do: an absent key means
      // "the host decides", and an explicit undefined is not the same thing.
      const start = into.trim() === "" ? settings?.recording.library : into;
      const chosen = await open({
        directory: true,
        multiple: false,
        title: "Where the exported files go",
        ...(start === null || start === undefined ? {} : { defaultPath: start }),
      });
      if (typeof chosen === "string") {
        setInto(chosen);
        setPlan(null);
      }
    });
  };

  const makePlan = () => {
    void run(async () => {
      setPlan(await api.exportPlan(request()));
    });
  };

  const write = () => {
    if (into.trim() === "") {
      return;
    }
    void run(() => api.exportRun(request()));
  };

  useKeys("export", { exportRun: write });

  return (
    <section className="panel export">
      <header className="panel-head">
        <h2>Export</h2>
        <button
          type="button"
          disabled={project.path === null || into.trim() === ""}
          onClick={makePlan}
        >
          Plan
        </button>
        <button
          type="button"
          disabled={project.path === null || into.trim() === ""}
          onClick={write}
        >
          Export (Ctrl+E)
        </button>
      </header>

      <div className="fields">
        <label>
          Into
          {/*
            The input and its button are a row inside the label, because a
            `label` is a flex column: a button added as a sibling of the input
            lands underneath it and full width, which is what a Browse button
            that looked like a second text field did on the first run.
          */}
          <span className="row">
            <input
              className="wide"
              value={into}
              placeholder="a directory that will be created if it is not there"
              onChange={(event) => {
                setInto(event.target.value);
                setPlan(null);
              }}
            />
            <button type="button" onClick={browse}>
              Browse...
            </button>
          </span>
        </label>
        <label>
          Format
          {/* Lossless first: the archival copy is the one that matters, and a
              list opening with MP3 would be a list suggesting otherwise. The
              plan is dropped on a change because the paths in it end in the
              old extension. */}
          <select
            value={format}
            onChange={(event) => {
              setFormat(event.target.value);
              setPlan(null);
            }}
          >
            <option value="flac">FLAC</option>
            <option value="wav">WAV</option>
            <option value="mp3">MP3</option>
            <option value="ogg">Ogg Vorbis</option>
          </select>
        </label>
        {effort !== null && (
          <label>
            {effort.caption}
            {/* Only for the containers it means anything to, and under the
                name that container's setting actually has: FLAC's number
                changes the size and the time and nothing else, so calling it
                a quality would be calling FLAC lossy. Both values are kept in
                state whichever is on screen, so switching format and back
                does not lose one - the field disappears, the choice does not. */}
            <select
              value={effort.field === "quality" ? quality : compression}
              onChange={(event) => {
                if (effort.field === "quality") {
                  setQuality(event.target.value);
                } else {
                  setCompression(event.target.value);
                }
                setPlan(null);
              }}
            >
              {effort.options.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          Artwork
          <select
            value={artwork}
            onChange={(event) => {
              setArtwork(event.target.value);
              setPlan(null);
            }}
          >
            <option value="none">None</option>
            <option value="embed">Embedded</option>
            <option value="folder">Folder image</option>
            <option value="both">Both</option>
          </select>
        </label>
        <label className="tick">
          <input
            type="checkbox"
            checked={overwrite}
            onChange={(event) => setOverwrite(event.target.checked)}
          />
          Overwrite files that are already there
        </label>
      </div>

      <fieldset className="sides">
        <legend>Sides</legend>
        <p className="hint">
          Nothing ticked exports every side that has tracks.
        </p>
        {project.sides.map((side) => (
          <label className="tick" key={side.id}>
            <input
              type="checkbox"
              checked={sides.includes(side.letter)}
              onChange={(event) =>
                setSides(
                  event.target.checked
                    ? [...sides, side.letter]
                    : sides.filter((letter) => letter !== side.letter),
                )
              }
            />
            {side.letter}
            {side.title === null ? "" : ` - ${side.title}`}
          </label>
        ))}
      </fieldset>

      {engine.exporting !== null && (
        <p className="hint">
          Writing {engine.exporting[0]} of {engine.exporting[1]}...
        </p>
      )}

      {engine.exporting === null && engine.exported !== null && (
        <p className="hint">{wrote(engine.exported)}</p>
      )}

      {plan !== null && (
        <>
          {/*
            Frames, not a duration. Turning one into the other needs the rate,
            and the plan does not carry it - the rate belongs to the capture,
            and a two-rate project would make any single divisor here wrong.
            §2 says the answer comes from a view model or not at all.
          */}
          <h3>
            Plan: {plan.files.length} file(s) in {plan.container},{" "}
            {plan.covers.length} cover(s), {plan.frames} frame(s)
          </h3>
          {/*
            The fold is invisible once the files exist - they are simply mono -
            so the only place it can be noticed and refused is here, before the
            export runs. The release said so at setup; this is the reminder.
          */}
          {plan.foldToMono && (
            <p className="hint">
              This release is marked mono, so every file is written with one
              channel: the captured channels are summed at -6 dB each, which
              cannot clip. The capture itself is unchanged.
            </p>
          )}
          <table className="rows">
            <thead>
              <tr>
                <th>Pos</th>
                <th>Title</th>
                <th>Path</th>
                <th className="n">Frames</th>
              </tr>
            </thead>
            <tbody>
              {plan.files.map((file) => (
                <tr key={file.path}>
                  <td>{file.position}</td>
                  <td>{file.title}</td>
                  <td className="path">{file.path}</td>
                  <td className="n">{file.frames}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </section>
  );
}

/**
 * What a finished export wrote, as one sentence.
 *
 * Exported for `Export.test.tsx`: the plural rules and the size scaling are the
 * only logic in this panel, and a reducer-free pure function is the cheapest
 * place to pin them. Frames are deliberately not turned into a duration for the
 * same reason the plan does not - the rate belongs to the capture, and this
 * report spans every side in the export.
 */
export function wrote(report: Exported): string {
  const files = `${report.files} file${report.files === 1 ? "" : "s"}`;
  const covers =
    report.covers === 0
      ? ""
      : `, ${report.covers} cover image${report.covers === 1 ? "" : "s"}`;
  return `Wrote ${files}${covers}, ${size(report.bytesWritten)}.`;
}

/**
 * A byte count at a scale a person can read.
 *
 * Binary units, because that is what every tool reporting on these files uses
 * and a side that `du` calls 274 MiB should not be called 287 MB here. One
 * decimal from a mebibyte up: `273.6 MiB` is a number you can compare with the
 * next side, and `273.63` is not.
 */
export function size(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} bytes`;
  }
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KiB`;
  }
  if (bytes < 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GiB`;
}
