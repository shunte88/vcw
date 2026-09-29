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

import { Fragment, useCallback, useState } from "react";

import * as api from "../api";
import type { Project } from "../bindings/vcw";
import { useKeys } from "../keys";
import { step } from "../select";
import { bytes, clock } from "../format";
import type { Store } from "../store";

/** The browser. */
export function Browser({
  store,
  projects,
  selected,
  onSelect,
  onLibraryChanged,
}: {
  store: Store;
  projects: readonly Project[];
  selected: string | null;
  onSelect: (path: string | null) => void;
  /**
   * Re-read the library.
   *
   * The rows come from `config::projects`, which the root reads; `store.reload`
   * only re-reads the *open* project. Opening a project can change a row -
   * upgrading a pre-WP-13 file is the case that made this necessary - so the
   * list has to be asked again, or the row a person just fixed goes on saying
   * it needs fixing.
   */
  onLibraryChanged: () => void;
}): React.JSX.Element {
  const [creating, setCreating] = useState(false);
  const [seed, setSeed] = useState({
    artist: "",
    album: "",
    catalog: "",
  });

  const open = () => {
    if (selected !== null) {
      void store.open(selected).then(onLibraryChanged);
    }
  };

  // Keeps the selected row on screen, so arrowing down a long library does not
  // walk the selection out of the viewport. `nearest` rather than `center`:
  // a list that re-centres on every press is a list that will not sit still.
  const show = useCallback((row: HTMLTableRowElement | null) => {
    row?.scrollIntoView({ block: "nearest" });
  }, []);

  // The rows answer the arrows as well as the mouse, because `openProject`
  // acts on `selected` and nothing else could set it (WP-16a).
  const move = (delta: -1 | 1) => () =>
    onSelect(
      step(
        projects.map((project) => project.path),
        selected,
        delta,
      ),
    );

  useKeys("browser", {
    openProject: open,
    newProject: () => setCreating(true),
    previousProject: move(-1),
    nextProject: move(1),
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
        // Both, and they are not the same thing: `reload` re-reads the project
        // the shell has open, `onLibraryChanged` re-reads the directory it came
        // from. Creating a project changes both, and for WP-19 only the first
        // was called - so a project created in the window was open, on disk,
        // and absent from the list until the application was restarted.
        store.reload();
        onLibraryChanged();
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
              <Fragment key={project.path}>
                <tr
                  ref={project.path === selected ? show : null}
                  className={[
                    project.path === selected ? "selected" : "",
                    project.path === store.project.path ? "open" : "",
                    project.problem !== null ? "problem" : "",
                  ]
                    .filter((name) => name !== "")
                    .join(" ")}
                  onClick={() => onSelect(project.path)}
                  onDoubleClick={() =>
                    void store.open(project.path).then(onLibraryChanged)
                  }
                  title={project.path}
                >
                  <td>{project.album === "" ? project.name : project.album}</td>
                  <td>{project.albumArtist}</td>
                  <td>{project.catalog}</td>
                  <td className="n">{project.sides}</td>
                  <td className="n">{project.tracks}</td>
                  <td className="n">{clock(project.seconds)}</td>
                  <td className="n">{bytes(project.fileBytes)}</td>
                </tr>
                {/*
                  On the row, not in a tooltip. `browse::summarise` promises
                  "the row a browser draws greyed out with a reason beside
                  it", and until WP-16a the reason was a `title=` attribute -
                  so first light showed an amber row of zeros against a file
                  holding twenty seconds of audio and said nothing about why.
                  A hover is also no use to somebody driving this by keyboard.
                */}
                {project.problem !== null && (
                  <tr
                    className="problem-reason"
                    onClick={() => onSelect(project.path)}
                  >
                    <td colSpan={7}>{project.problem}</td>
                  </tr>
                )}
              </Fragment>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
