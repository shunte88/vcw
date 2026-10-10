/*
 *  Tracks.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The track editor: boundaries, splits, merges, locks and per-track metadata (§31).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Two lists that are two views of one thing. A track is the span between two
// boundaries, so the boundary table is where a person adjusts *where* a track
// is and the track table is where they say *what* it is. Keeping them apart is
// what makes the keyboard map legible: `Ctrl+Left` nudges a boundary and
// `Enter` edits a title, and a single merged table would have to decide which
// of those the selection means.
//
// # What re-analysis does to a hand edit
//
// Nothing, if it is locked, and that is the whole of §31's contract with the
// operator. The lock column is therefore not a detail: it is the only control
// in the application whose effect is to survive the next detection pass, and it
// is shown on every row rather than hidden behind a selection so that a person
// can see at a glance what a re-run will and will not touch.
//
// A rejected boundary - one the adoption policy did not promote into a track -
// is listed here with its confidence and the detectors that agreed, which
// answers WP-11's deferral: it is visible, at whatever confidence it has, with
// no threshold applied on this side. Hiding it would mean a person could never
// overrule a policy that was set too strictly, and the number they need in
// order to judge it is the number that was already computed.

import { useCallback, useEffect, useState } from "react";

import * as api from "../api";
import type { Boundary, Track } from "../bindings/vcw";
import { useKeys } from "../keys";
import { step } from "../select";
import { clock } from "../format";
import type { Store } from "../store";

/** What the editor has selected. Ephemeral view state, so it lives here. */
export type Chosen = {
  /** The selected track's row id, or null. */
  readonly track: number | null;
  /** The selected boundary's row id, or null. */
  readonly boundary: number | null;
};

/** How far a nudge key moves a boundary, in seconds. */
const NUDGE = 0.05;

/** The side letters a release of this many discs has: two faces per disc. */
function faces(discs: number): string[] {
  const count = Math.min(Math.max(discs, 1), 13) * 2;
  return Array.from({ length: count }, (_, i) => String.fromCharCode(65 + i));
}

/** The track editor. */
export function Tracks({
  store,
  chosen,
  onChoose,
}: {
  store: Store;
  chosen: Chosen;
  onChoose: (chosen: Chosen) => void;
}): React.JSX.Element {
  const { project, engine, run } = store;
  const [editing, setEditing] = useState<number | null>(null);
  // Which face the capture on screen holds. A statement, not a guess: nothing
  // in the capture path records it, so until a person says otherwise the
  // detector attaches what it promotes to side A. A side B only rip named
  // that way gets side B's titles from the release; left on A it gets side
  // A's, which is the wrong words over real audio and nothing flags it.
  const [face, setFace] = useState("A");
  const [draft, setDraft] = useState({
    title: "",
    artist: "",
    composer: "",
    comments: "",
  });

  const track = project.tracks.find((row) => row.id === chosen.track);
  const boundary = project.boundaries.find((row) => row.id === chosen.boundary);

  // The draft follows the selection, so opening the editor on a different row
  // does not show the previous row's words.
  useEffect(() => {
    if (track !== undefined) {
      setDraft({
        title: track.title,
        artist: track.artist ?? "",
        composer: track.composer ?? "",
        comments: track.comments ?? "",
      });
    }
  }, [track?.id]);

  const nudge = (by: number) => {
    if (boundary === undefined) {
      return;
    }
    void run(() =>
      api.moveMarker({
        boundaryId: boundary.id,
        to: Math.max(0, boundary.seconds + by),
        // A nudge does not break a lock. §24 refuses the move and the refusal
        // says why, which is better than a key that silently does the one
        // thing the lock exists to prevent - and the lock button beside it is
        // how a person who meant it says so.
        force: false,
      }),
    ).then(store.reload);
  };

  // Keeps whichever row the arrows just selected on screen. One callback for
  // both lists: `nearest` only scrolls the pane the row is actually in.
  const show = useCallback((row: HTMLTableRowElement | null) => {
    row?.scrollIntoView({ block: "nearest" });
  }, []);

  // Two lists, two pairs of arrows (WP-16a). Every verb above acts on
  // `chosen.track` or `chosen.boundary`, and before first light neither could
  // be set without a click - so `Enter` to edit a track and `Delete` to remove
  // a marker were bound to actions no keyboard could reach the subject of.
  const moveTrack = (delta: -1 | 1) => () =>
    onChoose({
      ...chosen,
      track: step(
        project.tracks.map((row) => row.id),
        chosen.track,
        delta,
      ),
    });
  const moveBoundary = (delta: -1 | 1) => () =>
    onChoose({
      ...chosen,
      boundary: step(
        project.boundaries.map((row) => row.id),
        chosen.boundary,
        delta,
      ),
    });

  useKeys("tracks", {
    previousTrack: moveTrack(-1),
    nextTrack: moveTrack(1),
    previousBoundary: moveBoundary(-1),
    nextBoundary: moveBoundary(1),
    detect: () => void run(
              () => api.detectTracks({ side: face, promote: true }),
              "detect",
            ),
    deleteMarker: () => {
      if (boundary !== undefined) {
        void run(() => api.deleteMarker({ boundaryId: boundary.id }));
        onChoose({ ...chosen, boundary: null });
      }
    },
    nudgeBack: () => nudge(-NUDGE),
    nudgeForward: () => nudge(NUDGE),
    rename: () => {
      if (track !== undefined) {
        setEditing(track.id);
      }
    },
  });

  const commit = () => {
    if (editing === null) {
      return;
    }
    const blank = (value: string) => (value.trim() === "" ? null : value.trim());
    void run(() =>
      api.editTrack({
        trackId: editing,
        title: blank(draft.title),
        artist: blank(draft.artist),
        composer: blank(draft.composer),
        comments: blank(draft.comments),
        musicbrainzId: null,
        confirmed: true,
      }),
    ).then(() => {
      setEditing(null);
      store.reload();
    });
  };

  return (
    <section className="panel tracks">
      <header className="panel-head">
        <h2>Tracks</h2>
        <label className="face">
          Side
          <select
            value={face}
            onChange={(event) => setFace(event.target.value)}
            title="Which face of the record this capture holds"
          >
            {faces(project.release?.discs ?? 1).map((letter) => (
              <option key={letter} value={letter}>
                {letter}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          // A capture as well as a project. Detection reads audio, and with
          // none the button was live, ran, and came back having found nothing
          // - which looks like a detector that failed rather than a panel
          // that had nothing to offer.
          disabled={project.path === null || project.captures.length === 0}
          onClick={() =>
            void run(
              () => api.detectTracks({ side: face, promote: true }),
              "detect",
            )
          }
        >
          Detect (t)
        </button>
        <button
          type="button"
          disabled={track === undefined}
          onClick={() => {
            if (track !== undefined) {
              void run(
                () => api.splitTrack({ trackId: track.id, at: engine.playhead }),
                "split",
              ).then(store.reload);
            }
          }}
          title="Split the selected track at the playhead. Click the waveform to move the playhead; it snaps to a boundary."
        >
          Split at playhead
        </button>
        <button
          type="button"
          disabled={track === undefined || next(project.tracks, track) === undefined}
          onClick={() => {
            const after = track === undefined ? undefined : next(project.tracks, track);
            if (track !== undefined && after !== undefined) {
              void run(
                () => api.mergeTracks({ leftId: track.id, rightId: after.id }),
                "merge",
              ).then(store.reload);
            }
          }}
          title="Join the selected track to the one after it"
        >
          Merge with next
        </button>
      </header>

      {engine.detection !== null && (
        <p className="hint">
          Last pass: {engine.detection.boundaries} boundary(s),{" "}
          {engine.detection.tracks} track(s), {engine.detection.rejected}{" "}
          rejected, {engine.detection.alreadySettled} already settled, in{" "}
          {engine.detection.seconds.toFixed(2)} s.
        </p>
      )}

      {project.tracks.length === 0 ? (
        /* The advice has to match the state, or it is worse than silence.
           "Press t to run detection" against no project at all sends a person
           to a key that is disabled, and tells them nothing about the step
           they actually have to take first. */
        <p className="empty">
          {project.path === null ? (
            <>
              No project open. Open or create one in the Library (
              <kbd>Ctrl+1</kbd>).
            </>
          ) : project.captures.length === 0 ? (
            <>
              Nothing captured yet. Record a side in Capture (
              <kbd>Ctrl+2</kbd>).
            </>
          ) : (
            <>
              No tracks. Press <kbd>t</kbd> to run detection over the capture.
            </>
          )}
        </p>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th>Pos</th>
              <th>Title</th>
              <th>Artist</th>
              <th className="n">Start</th>
              <th className="n">Length</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {project.tracks.map((row) => (
              <tr
                key={row.id}
                ref={row.id === chosen.track ? show : null}
                className={row.id === chosen.track ? "selected" : ""}
                onClick={() => onChoose({ ...chosen, track: row.id })}
                onDoubleClick={() => setEditing(row.id)}
              >
                <td>{row.position}</td>
                <td>{row.title}</td>
                <td>{row.artist ?? ""}</td>
                <td className="n">{clock(row.start)}</td>
                <td className="n">{clock(row.seconds)}</td>
                <td>
                  {row.confirmed && (
                    <span className="ok" title="Confirmed by a person or a provider">
                      &#10003;
                    </span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {editing !== null && (
        <form
          className="seed"
          onSubmit={(event) => {
            event.preventDefault();
            commit();
          }}
        >
          <label>
            Title
            <input
              autoFocus
              value={draft.title}
              onChange={(event) =>
                setDraft({ ...draft, title: event.target.value })
              }
            />
          </label>
          <label>
            Artist
            <input
              value={draft.artist}
              onChange={(event) =>
                setDraft({ ...draft, artist: event.target.value })
              }
            />
          </label>
          <label>
            Composer
            <input
              value={draft.composer}
              onChange={(event) =>
                setDraft({ ...draft, composer: event.target.value })
              }
            />
          </label>
          <label>
            Comments
            <textarea
              value={draft.comments}
              onChange={(event) =>
                setDraft({ ...draft, comments: event.target.value })
              }
            />
          </label>
          <div className="row">
            <button type="submit">Save (Ctrl+S)</button>
            <button type="button" onClick={() => setEditing(null)}>
              Cancel
            </button>
          </div>
        </form>
      )}

      <h3>Boundaries</h3>
      {project.boundaries.length === 0 ? (
        <p className="empty">No boundaries.</p>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th>Side</th>
              <th>Edge</th>
              <th className="n">At</th>
              <th className="n">Confidence</th>
              <th>Agreed</th>
              <th></th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {project.boundaries.map((row) => (
              <tr
                key={row.id}
                ref={row.id === chosen.boundary ? show : null}
                className={[
                  row.id === chosen.boundary ? "selected" : "",
                  row.promoted ? "" : "rejected",
                ]
                  .filter((name) => name !== "")
                  .join(" ")}
                onClick={() => onChoose({ ...chosen, boundary: row.id })}
              >
                <td>{row.side}</td>
                <td>{row.edge}</td>
                <td className="n">{clock(row.seconds)}</td>
                <td className="n">{(row.confidence * 100).toFixed(0)}%</td>
                <td title={row.sources.join(", ")}>
                  {row.agreement} of {row.sources.length}
                </td>
                <td>
                  <button
                    type="button"
                    className={row.locked ? "locked" : ""}
                    onClick={(event) => {
                      event.stopPropagation();
                      void run(() =>
                        api.lockMarker({
                          boundaryId: row.id,
                          locked: !row.locked,
                        }),
                      ).then(store.reload);
                    }}
                    title={
                      row.locked
                        ? "Locked: re-analysis will leave it alone"
                        : "Unlocked: re-analysis may move it"
                    }
                  >
                    {row.locked ? "Locked" : "Lock"}
                  </button>
                </td>
                <td>{row.promoted ? "" : "not a track"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {boundary !== undefined && <Detail boundary={boundary} />}
    </section>
  );
}

/** The evidence behind one boundary (§31's audit trail). */
function Detail({ boundary }: { boundary: Boundary }) {
  return (
    <dl className="evidence">
      <dt>Decided by</dt>
      <dd>{boundary.provenance}</dd>
      <dt>Detectors that agreed</dt>
      <dd>{boundary.sources.join(", ")}</dd>
      {boundary.evidence.length > 0 && (
        <>
          <dt>Measurements</dt>
          <dd>
            <ul>
              {boundary.evidence.map((measurement) => (
                <li key={measurement.name}>
                  {measurement.name}: {measurement.value.toFixed(3)}
                </li>
              ))}
            </ul>
          </dd>
        </>
      )}
    </dl>
  );
}

/** The track after this one on the same side, if there is one. */
function next(tracks: readonly Track[], track: Track): Track | undefined {
  return tracks.find(
    (row) => row.sideId === track.sideId && row.number === track.number + 1,
  );
}
