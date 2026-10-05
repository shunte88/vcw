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
// The leading cell is the cover, and it is a table with an icon column rather
// than a grid of tiles. A tile grid makes the artwork the index, which is
// Audacity 4.0's choice and the wrong one here: a vinyl library is indexed by
// catalogue number and artist, most rips have no cover until a release is
// assigned, and a wall of identical placeholders is a worse list than a list.
// So the image rides beside the text at the height of one row, and a project
// without one gets a sleeve mark in the same space - which keeps every row the
// same height whether the cover has arrived or not.
//
// The covers are fetched one row at a time, lazily, and only for rows that say
// they have one. See `api.artwork`.
//
// The create form is the helper the requirement asked for and nothing more.
// Artist, title and catalogue number, none of them required, because the point
// is to save typing them again later and a required field would make it a form
// to fill in rather than a hint to leave. Everything it does not ask for -
// sides, discs, a numbering scheme - is what identification and detection fill
// in, and asking would be asking a person to guess at the answer the
// application is about to find.
//
// The two boxes below them are there on the opposite argument. Mono and the
// RIAA curve are the only things on this form that nothing downstream can work
// out: a mono groove played with a stereo cartridge is two nearly-identical
// channels and never exactly identical ones, and a cutting curve leaves no
// trace in the audio it was applied to. Unticked is an answer rather than a
// gap, which is why they are checkboxes and not a third state, and both are
// changeable afterwards - neither touches what is captured.

import { Fragment, useCallback, useEffect, useState } from "react";

import * as api from "../api";
import type { Project } from "../bindings/vcw";
import { useKeys } from "../keys";
import { step } from "../select";
import { bytes, clock, when } from "../format";
import type { Store } from "../store";

/**
 * Covers already fetched, keyed by path *and* modification time.
 *
 * Module-level rather than state, because the panel unmounts every time a
 * person switches to another tab and a cache that died with it would refetch
 * the whole library on every Ctrl+1. Keyed by `modified` as well as `path` so
 * that assigning a release - which rewrites the file, which moves its
 * timestamp - invalidates the row's entry without anything having to remember
 * to clear it.
 *
 * Unbounded, deliberately. The entries are the covers of projects a person has
 * actually looked at in one sitting, and a library large enough for that to
 * matter is one where the images are the small part of the problem.
 */
const COVERS = new Map<string, string | null>();

/**
 * One row's cover, fetched when the row first draws.
 *
 * A component rather than a prefetch pass over the list: a library of a hundred
 * projects would otherwise fire a hundred `invoke`s on mount, each reading a
 * megabyte blob, to draw maybe fifteen visible rows.
 */
function Cover({
  project,
}: {
  project: Project;
}): React.JSX.Element {
  const key = `${project.path}\u0000${project.modified}`;
  const [url, setUrl] = useState<string | null>(COVERS.get(key) ?? null);

  useEffect(() => {
    if (!project.hasArtwork || COVERS.has(key)) {
      setUrl(COVERS.get(key) ?? null);
      return;
    }
    // `gone` rather than an AbortController: `invoke` has nothing to abort, and
    // the thing that actually matters is not calling `setUrl` on a row that has
    // scrolled away or a panel that has closed.
    let gone = false;
    void api
      .artwork(project.path)
      .then((found) => {
        COVERS.set(key, found);
        if (!gone) {
          setUrl(found);
        }
      })
      .catch(() => {
        // A cover that will not read is a placeholder, not an error banner.
        // The row itself is fine; `problem` is where a broken file is reported.
        COVERS.set(key, null);
      });
    return () => {
      gone = true;
    };
  }, [key, project.hasArtwork, project.path]);

  if (url === null) {
    return (
      <span className="sleeve" aria-hidden="true">
        &#9834;
      </span>
    );
  }
  return <img className="cover" src={url} alt="" />;
}

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
    isMono: false,
    riaaEq: false,
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
          isMono: seed.isMono,
          riaaEq: seed.riaaEq,
        });
        onSelect(made.path);
        await api.openProject(made.path);
      })
      .then(() => {
        setCreating(false);
        setSeed({
          artist: "",
          album: "",
          catalog: "",
          isMono: false,
          riaaEq: false,
        });
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
        <h2>Library</h2>
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
          <label className="tick">
            <input
              type="checkbox"
              checked={seed.isMono}
              onChange={(event) =>
                setSeed({ ...seed, isMono: event.target.checked })
              }
            />
            Mono pressing
          </label>
          <label className="tick">
            <input
              type="checkbox"
              checked={seed.riaaEq}
              onChange={(event) =>
                setSeed({ ...seed, riaaEq: event.target.checked })
              }
            />
            Apply RIAA equalisation
          </label>
          <p className="hint">
            These two are not. Mono folds the channels together in the exported
            files and leaves the capture stereo; RIAA applies the curve on
            playback and on export, so leave it off if your phono stage already
            did. Both can be changed later and neither touches what is recorded.
          </p>
          <button type="submit">Create</button>
        </form>
      )}

      {projects.length === 0 ? (
        // The one screen with room for the logo, and the one that needs it:
        // an empty library is the first thing a new installation shows, and
        // the alternative is a sentence floating in a black rectangle. It is
        // decoration, so it is `alt=""` and the sentence below it carries the
        // meaning on its own.
        <div className="empty start">
          <img className="logo" src="/vcw-logo.webp" alt="" width={260} />
          <p>No projects. Set a library directory in Settings, or create one.</p>
        </div>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th className="art" />
              <th>Album</th>
              <th>Artist</th>
              <th>Catalogue</th>
              <th className="n">Sides</th>
              <th className="n">Tracks</th>
              <th className="n">Length</th>
              <th className="n">Size</th>
              <th className="n">Modified</th>
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
                  <td className="art">
                    <Cover project={project} />
                  </td>
                  <td>{project.album === "" ? project.name : project.album}</td>
                  <td>{project.albumArtist}</td>
                  <td>{project.catalog}</td>
                  <td className="n">{project.sides}</td>
                  <td className="n">{project.tracks}</td>
                  <td className="n">{clock(project.seconds)}</td>
                  <td className="n">{bytes(project.fileBytes)}</td>
                  <td className="n">{when(project.modified)}</td>
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
                    <td colSpan={9}>{project.problem}</td>
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
