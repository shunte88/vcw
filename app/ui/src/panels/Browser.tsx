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
// Two views of the same rows, and the choice is remembered. The list is the
// one that was here first: a table indexed by catalog number and artist,
// with the cover riding beside the text at the height of one row, because a
// vinyl library is looked up by what is printed on the label and a column of
// pictures is not an index.
//
// The argument against a grid was that most rips have no cover until a release
// is assigned, so a wall of tiles would be a wall of identical placeholders -
// a worse list than a list. What answers it is `Project::preview`: every
// recorded project already has a picture of itself, because a side of a record
// has a shape and two rips never look alike. So a tile shows the cover once
// there is one and the waveform until then, and neither is a placeholder.
//
// The covers are fetched one row at a time, lazily, and only for rows that say
// they have one. See `api.artwork`. The waveform needs no fetch at all - it
// arrives on the row, 96 peaks of it, out of the `sampleblocks_levels` index.
//
// The create form is the helper the requirement asked for and nothing more.
// Artist, title and catalog number, none of them required, because the point
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
import type { About, Project } from "../bindings/vcw";
import { useKeys } from "../keys";
import { step } from "../select";
import { Switch } from "./Switch";
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
  fallback,
}: {
  project: Project;
  /** Drawn in the cover's place when there is no cover. */
  fallback?: React.JSX.Element;
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
      fallback ?? (
        <span className="sleeve" aria-hidden="true">
          &#9834;
        </span>
      )
    );
  }
  return <img className="cover" src={url} alt="" />;
}

/**
 * A project's own waveform, as a tile-sized picture of it.
 *
 * An inline SVG rather than a canvas: there is no interaction, no redraw and
 * no device-pixel arithmetic to get right, and 96 points is a string short
 * enough that React re-rendering the whole thing costs less than keeping a
 * ref to a canvas would. `preserveAspectRatio="none"` because the tile decides
 * the shape - the vertical axis is a magnitude, not a length, so stretching it
 * is the correct thing to do rather than a distortion.
 *
 * Falls back to the sleeve mark when there are no peaks, which is a project
 * that has nothing recorded in it yet.
 */
function Preview({ peaks }: { peaks: readonly number[] }): React.JSX.Element {
  if (peaks.length < 2) {
    return (
      <span className="sleeve" aria-hidden="true">
        &#9834;
      </span>
    );
  }
  // Normalized to its own loudest column, which is the one place in VCW that
  // scales a waveform without saying so. It is right here because this is an
  // icon and not a meter: nobody reads a level off a 140-pixel thumbnail, and
  // a record cut with headroom - the demo side peaks around a third of full
  // scale - drew a flat blue smear through the middle of its tile that told
  // you nothing about which record it was. Scaled, the shape is the thing you
  // recognize. The real waveform, where the number matters, is untouched.
  const loudest = Math.max(...peaks);
  const scale = loudest > 0 ? 1 / loudest : 0;
  const height = (column: number) => peaks[column]! * scale;

  // One filled shape, mirrored about the center line: out along the top, back
  // along the bottom, closed. Drawn as an area and not as a polyline because a
  // 1-pixel stroke at this scale disappears into the background on the quiet
  // passages, and the quiet passages are where the sides are.
  const last = peaks.length - 1;
  const top = peaks.map((_, column) => `L${column},${1 - height(column)}`).join("");
  const bottom = peaks
    .map((_, column) => `L${last - column},${1 + height(last - column)}`)
    .join("");
  return (
    <svg
      className="preview"
      viewBox={`0 0 ${last} 2`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <path d={`M0,${1 - height(0)}${top}${bottom}Z`} />
    </svg>
  );
}

/**
 * Where the list-or-tiles choice lives between sittings.
 *
 * `localStorage` and not the project config: this is a property of the person
 * at the window, not of the library on the disk, and putting it in the config
 * file would mean a schema migration and a round trip through the shell to
 * record which of two buttons is pressed.
 */
const VIEW = "vcw.library.view";

/** The two icons on the toggle, drawn rather than named so they do not need a font. */
function ViewIcon({ tiles }: { tiles: boolean }): React.JSX.Element {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className="view-icon">
      {tiles ? (
        [0, 1].map((row) =>
          [0, 1].map((column) => (
            <rect
              key={`${row}.${column}`}
              x={1 + column * 8}
              y={1 + row * 8}
              width="6"
              height="6"
              rx="1"
            />
          )),
        )
      ) : (
        <>
          {[1, 6.5, 12].map((y) => (
            <rect key={y} x="1" y={y} width="3" height="3" rx="1" />
          ))}
          {[1.5, 7, 12.5].map((y) => (
            <rect key={y} x="6" y={y} width="9" height="2" rx="1" />
          ))}
        </>
      )}
    </svg>
  );
}

/**
 * The classes a project's row or tile carries, in whichever view is showing.
 *
 * Shared rather than written out twice, and not only to save the six lines:
 * `wiring.test.ts` counts the `"selected"` literals in a panel and expects a
 * `step(` for each, because a selectable list with no way to move it from the
 * keyboard is §44 failing quietly. Two *views* of one list are still one
 * list, and spelling the class twice would have claimed otherwise.
 */
function marks(
  project: Project,
  selected: string | null,
  open: string | null,
): string {
  return [
    project.path === selected ? "selected" : "",
    project.path === open ? "open" : "",
    project.problem !== null ? "problem" : "",
  ]
    .filter((name) => name !== "")
    .join(" ");
}

/**
 * Which build this is, under the logo on the splash.
 *
 * The same three facts the About dialog leads with, in the one place a person
 * is already looking at a logo and has nothing else to read. It is derived,
 * not typed: `product` is the shell's name for itself, `version` is Cargo's,
 * and `built` is stamped by `crates/contract/build.rs` - so a screenshot of
 * this screen is evidence of what was running, which is the entire reason to
 * put a date on a build.
 *
 * Nothing is drawn until the answer arrives. A version that appears as "0.0.0"
 * and then corrects itself is worse than one that appears a frame late, and on
 * a splash there is no layout to hold open.
 */
function Stamp(): React.JSX.Element | null {
  const [build, setBuild] = useState<About | null>(null);

  useEffect(() => {
    let live = true;
    void api.about().then((answer) => {
      if (live) {
        setBuild(answer);
      }
    });
    return () => {
      live = false;
    };
  }, []);

  if (build === null) {
    return null;
  }
  return (
    <p className="stamp">
      {build.product} <span>v{build.version}</span>
      <br />
      <span>Built {build.built}</span>
    </p>
  );
}

/** The browser. */
export function Browser({
  store,
  projects,
  selected,
  onSelect,
  onLibraryChanged,
  onOpened,
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
  /** A project was opened, so the window should move on from the library. */
  onOpened: () => void;
}): React.JSX.Element {
  const [creating, setCreating] = useState(false);
  const [tiles, setTiles] = useState(
    () => localStorage.getItem(VIEW) === "tiles",
  );
  const [seed, setSeed] = useState({
    artist: "",
    album: "",
    catalog: "",
    isMono: false,
    riaaEq: false,
  });

  // Every way into a project goes through here, so the panel change that
  // follows one cannot be wired to two of the three. Opening lands on Tracks
  // because that is what somebody who just picked a record off the shelf came
  // to look at: the library answers "which record", and the next question is
  // always "what is on it". Creating a project deliberately does not go here -
  // a new project has nothing to show on Tracks.
  const openProject = (path: string) => {
    void store.open(path).then(() => {
      onLibraryChanged();
      onOpened();
    });
  };

  const open = () => {
    if (selected !== null) {
      openProject(selected);
    }
  };

  // A list with no cursor is a list where Enter does nothing and the header's
  // own "Open (Enter)" is a lie. A library of one was the case that made it
  // obvious: the single row is plainly the one meant, and a person still had
  // to click it before the keyboard would act on it. First row rather than
  // none, which is what every other list in the product does and what the
  // arrow keys already assume when they start from nothing.
  useEffect(() => {
    if (selected === null && projects.length > 0) {
      onSelect(projects[0]?.path ?? null);
    }
  }, [projects, selected, onSelect]);

  // Keeps the selected row on screen, so arrowing down a long library does not
  // walk the selection out of the viewport. `nearest` rather than `center`:
  // a list that re-centers on every press is a list that will not sit still.
  const show = useCallback((row: HTMLElement | null) => {
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
        <div className="view-toggle" role="group" aria-label="Library view">
          {[false, true].map((wanted) => (
            <button
              key={String(wanted)}
              type="button"
              className={tiles === wanted ? "on" : ""}
              aria-pressed={tiles === wanted}
              title={wanted ? "Tiles" : "List"}
              onClick={() => {
                localStorage.setItem(VIEW, wanted ? "tiles" : "list");
                setTiles(wanted);
              }}
            >
              <ViewIcon tiles={wanted} />
            </button>
          ))}
        </div>
        <button type="button" onClick={() => setCreating(true)}>
          New project (n)
        </button>
        <button type="button" disabled={selected === null} onClick={open}>
          Open (Enter)
        </button>
      </header>

      {creating && (
        /*
          A dialog rather than a band pushed in above the library, which is
          what this was. The band moved every row down the moment it opened,
          so the project a person had just selected jumped out from under the
          pointer, and in the tile view it reflowed the whole grid. A create
          form is also a modal act in fact - there is nothing useful to do to
          the list while one is half filled in - so it may as well say so.

          `onKeyDown` and not the `dismiss` binding: `creating` is this
          panel's state, `useKeys` listens on the window, and routing Escape
          through `App` would mean lifting the state up there to close it.
          Stopping the event here keeps the global handler from also clearing
          a refusal on the same press.
        */
        <div
          className="overlay"
          role="dialog"
          aria-label="New project"
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.stopPropagation();
              setCreating(false);
            }
          }}
        >
        <form
          className="seed overlay-box narrow"
          onSubmit={(event) => {
            event.preventDefault();
            create();
          }}
        >
          <header className="panel-head">
            <h2>New project</h2>
            <button type="button" onClick={() => setCreating(false)}>
              Cancel (Esc)
            </button>
          </header>
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
            Catalog number
            <input
              value={seed.catalog}
              onChange={(event) =>
                setSeed({ ...seed, catalog: event.target.value })
              }
            />
          </label>
          {/*
            A group of its own, because these two are not fields of the
            project the way the artist and the catalog number are: they are
            what VCW will do to the audio afterwards, and the note that says
            so belongs above them rather than after the fact. A person reading
            downwards should know the switches are harmless before they touch
            one, not after.
          */}
          <fieldset className="seed-group">
            <legend>Post-Processing</legend>
            <p className="hint">
              Neither of these two touches what is recorded, and both can be
              changed later. Mono folds the channels together in the exported
              files; RIAA applies the curve on audition and on export.
            </p>
            <div className="seed-switch">
              <span>Pressing</span>
              <Switch
                checked={!seed.isMono}
                onChange={(stereo) => setSeed({ ...seed, isMono: !stereo })}
                off="Mono"
                on="Stereo"
                title="Mono folds the channels together in the exported files and leaves the capture stereo"
              />
            </div>
            <div className="seed-switch">
              <span>Equalization</span>
              <Switch
                checked={seed.riaaEq}
                onChange={(riaa) => setSeed({ ...seed, riaaEq: riaa })}
                off="None"
                on="RIAA"
                title="Applies the RIAA playback curve on audition and on export"
              />
            </div>
            {seed.riaaEq && (
              /*
                Shown only when it is switched on, because it is advice about a
                decision just taken and not a standing caveat. There is no safe
                default here and VCW cannot work the answer out: a phono stage
                and a head amp both normally apply the curve themselves, and a
                record equalized twice sounds wrong in a way that is hard to
                name and impossible to undo after the fact.
              */
              <p className="hint warn">
                Most phono stages and head amps apply the RIAA curve themselves.
                If yours did, applying it again here equalizes the record twice.
                Nothing in the signal reaching VCW says which happened, so this
                one is your call: switch it on only if the capture arrives flat,
                and leave it off if anything upstream has already curved it.
              </p>
            )}
          </fieldset>
          <button type="submit">Create</button>
        </form>
        </div>
      )}

      {projects.length === 0 ? (
        // The one screen with room for the logo, and the one that needs it:
        // an empty library is the first thing a new installation shows, and
        // the alternative is a sentence floating in a black rectangle. It is
        // decoration, so it is `alt=""` and the sentence below it carries the
        // meaning on its own.
        <div className="empty start">
          <img className="logo" src="/vcw-logo.webp" alt="" width={260} />
          <Stamp />
          <p>No projects. Set a library directory in Settings, or create one.</p>
        </div>
      ) : tiles ? (
        <div className="tiles">
          {/*
            The first cell, and a cell rather than a second place to find the
            "New project" button that is already in the header: in a grid the
            empty slot at the start is where a person looks to add one, and
            leaving it out would mean the grid began with whichever project
            sorted first and the way to add one was off in a corner.
          */}
          <button
            type="button"
            className="tile new"
            onClick={() => setCreating(true)}
          >
            <span className="tile-art" aria-hidden="true">
              +
            </span>
            <span className="tile-name">New project</span>
          </button>
          {projects.map((project) => (
            <button
              key={project.path}
              type="button"
              ref={project.path === selected ? show : null}
              className={`tile ${marks(project, selected, store.project.path)}`}
              onClick={() => onSelect(project.path)}
              onDoubleClick={() => openProject(project.path)}
              title={project.path}
            >
              <span className="tile-art">
                <Cover
                  project={project}
                  fallback={<Preview peaks={project.preview} />}
                />
              </span>
              <span className="tile-name">
                {project.album === "" ? project.name : project.album}
              </span>
              <span className="tile-when">
                {project.problem ?? when(project.modified)}
              </span>
            </button>
          ))}
        </div>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th className="art" />
              <th>Album</th>
              <th>Artist</th>
              <th>Catalog</th>
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
                  className={marks(project, selected, store.project.path)}
                  onClick={() => onSelect(project.path)}
                  onDoubleClick={() => openProject(project.path)}
                  title={project.path}
                >
                  <td className="art">
                    <Cover project={project} />
                  </td>
                  <td>{project.album === "" ? project.name : project.album}</td>
                  <td>{project.albumArtist}</td>
                  <td>{project.catalog}</td>
                  {/*
                    A dash and not a zero for a project that could not be
                    read. `summarize` returns zeros because it got no rows,
                    and "Sides 0, Tracks 0, 0:00.00" is a *claim* - it reads
                    exactly like an empty project, and the one thing known
                    about this file is that nobody knows what is in it. The
                    size and the date are different: they come from the
                    filesystem and are true whatever the schema says.
                  */}
                  <td className="n">
                    {project.problem === null ? project.sides : "-"}
                  </td>
                  <td className="n">
                    {project.problem === null ? project.tracks : "-"}
                  </td>
                  <td className="n">
                    {project.problem === null ? clock(project.seconds) : "-"}
                  </td>
                  <td className="n">{bytes(project.fileBytes)}</td>
                  <td className="n">{when(project.modified)}</td>
                </tr>
                {/*
                  On the row, not in a tooltip. `browse::summarize` promises
                  "the row a browser draws grayed out with a reason beside
                  it", and until WP-16a the reason was a `title=` attribute -
                  so first light showed an amber row of zeros against a file
                  holding twenty seconds of audio and said nothing about why.
                  A hover is also no use to somebody driving this by keyboard.

                  On the *selected* row only. A library holding seven projects
                  of an older schema got seven copies of the same sentence,
                  each one doubling a row's height, and the album names it
                  pushed apart were what a person had come to the panel to
                  read. Selection is how this list is driven - a click, or an
                  arrow key - so the reason still arrives without a mouse and
                  without being asked for twice.
                */}
                {project.problem !== null && project.path === selected && (
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
