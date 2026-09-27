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

import { useState } from "react";

import * as api from "../api";
import type { ExportPlan, Settings } from "../bindings/vcw";
import { useKeys } from "../keys";
import type { Store } from "../store";

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
    setArtwork(settings.export.artwork);
  }

  const request = () => ({
    into,
    format,
    template: settings?.export.template ?? null,
    sides: [...sides],
    artwork,
    overwrite,
  });

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
          <input
            value={into}
            placeholder="a directory that will be created if it is not there"
            onChange={(event) => setInto(event.target.value)}
          />
        </label>
        <label>
          Format
          <select
            value={format}
            onChange={(event) => setFormat(event.target.value)}
          >
            <option value="flac">FLAC</option>
            <option value="wav">WAV</option>
          </select>
        </label>
        <label>
          Artwork
          <select
            value={artwork}
            onChange={(event) => setArtwork(event.target.value)}
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
