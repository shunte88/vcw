/*
 *  Browser.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The project browser, and the four-field helper that creates one (§34).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// §34's list, read from `projects`, which walks the library directory and opens
// each file read-only. Nothing is computed here: the size, the duration, the
// track count and `problem` all arrive on the row, because the CLI's `vcw list`
// prints the same columns from the same view model.
//
// The create form is the helper the requirement asked for and nothing more.
// Artist, title and catalogue number, none of them required, because the point
// is to save typing them again later and a required field would make it a form
// to fill in rather than a hint to leave. Everything it does not ask for -
// sides, discs, a numbering scheme - is what identification and detection fill
// in, and asking would be asking a person to guess at the answer the
// application is about to find.

import { useState } from "react";

import * as api from "../api";
import type { Project } from "../bindings/vcw";
import { useKeys } from "../keys";
import { bytes, clock } from "../format";
import type { Store } from "../store";

/** The browser. */
export function Browser({
  store,
  projects,
  selected,
  onSelect,
}: {
  store: Store;
  projects: readonly Project[];
  selected: string | null;
  onSelect: (path: string | null) => void;
}): React.JSX.Element {
  const [creating, setCreating] = useState(false);
  const [seed, setSeed] = useState({
    artist: "",
    album: "",
    catalog: "",
  });

  const open = () => {
    if (selected !== null) {
      void store.open(selected);
    }
  };

  useKeys("browser", {
    openProject: open,
    newProject: () => setCreating(true),
  });

  const create = () => {
    const blank = (value: string) => (value.trim() === "" ? null : value.trim());
    void store
      .run(async () => {
        const made = await api.newProject({
          name: null,
          artist: blank(seed.artist),
          album: blank(seed.album),
          catalog: blank(seed.catalog),
        });
        onSelect(made.path);
        await api.openProject(made.path);
      })
      .then(() => {
        setCreating(false);
        setSeed({ artist: "", album: "", catalog: "" });
        store.reload();
      });
  };

  return (
    <section className="panel browser">
      <header className="panel-head">
        <h2>Projects</h2>
        <button type="button" onClick={() => setCreating(!creating)}>
          {creating ? "Cancel" : "New project (n)"}
        </button>
        <button type="button" disabled={selected === null} onClick={open}>
          Open (Enter)
        </button>
      </header>

      {creating && (
        <form
          className="seed"
          onSubmit={(event) => {
            event.preventDefault();
            create();
          }}
        >
          <p className="hint">
            A helper, not a requirement. Leave a field blank and identification
            will fill it in.
          </p>
          <label>
            Artist
            <input
              autoFocus
              value={seed.artist}
              onChange={(event) =>
                setSeed({ ...seed, artist: event.target.value })
              }
            />
          </label>
          <label>
            Recording title
            <input
              value={seed.album}
              onChange={(event) =>
                setSeed({ ...seed, album: event.target.value })
              }
            />
          </label>
          <label>
            Catalogue number
            <input
              value={seed.catalog}
              onChange={(event) =>
                setSeed({ ...seed, catalog: event.target.value })
              }
            />
          </label>
          <button type="submit">Create</button>
        </form>
      )}

      {projects.length === 0 ? (
        <p className="empty">
          No projects. Set a library directory in Settings, or create one.
        </p>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th>Album</th>
              <th>Artist</th>
              <th>Catalogue</th>
              <th className="n">Sides</th>
              <th className="n">Tracks</th>
              <th className="n">Length</th>
              <th className="n">Size</th>
            </tr>
          </thead>
          <tbody>
            {projects.map((project) => (
              <tr
                key={project.path}
                className={[
                  project.path === selected ? "selected" : "",
                  project.path === store.project.path ? "open" : "",
                  project.problem !== null ? "problem" : "",
                ]
                  .filter((name) => name !== "")
                  .join(" ")}
                onClick={() => onSelect(project.path)}
                onDoubleClick={() => void store.open(project.path)}
                title={project.problem ?? project.path}
              >
                <td>{project.album === "" ? project.name : project.album}</td>
                <td>{project.albumArtist}</td>
                <td>{project.catalog}</td>
                <td className="n">{project.sides}</td>
                <td className="n">{project.tracks}</td>
                <td className="n">{clock(project.seconds)}</td>
                <td className="n">{bytes(project.fileBytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
