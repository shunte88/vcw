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
import type { Boundary, Side, Track } from "../bindings/vcw";
import { key, useText } from "../i18n";
import { useKeys } from "../keys";
import { step } from "../select";
import { clock } from "../format";
import type { Store } from "../store";
import type { Region } from "./Waveform";

/** What the editor has selected. Ephemeral view state, so it lives here. */
export type Chosen = {
  /** The selected track's row id, or null. */
  readonly track: number | null;
  /** The selected boundary's row id, or null. */
  readonly boundary: number | null;
};

/**
 * The catalog key for what each edge is called.
 *
 * Keyed by the wire word and falling back to it, for the same reason the
 * capture panel keys its states that way.
 */
const EDGES: Record<string, string> = {
  start: key("tracks.edge.start"),
  end: key("tracks.edge.end"),
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
  side,
  selection,
  onSelect,
}: {
  store: Store;
  chosen: Chosen;
  onChoose: (chosen: Chosen) => void;
  /** The side the capture on screen belongs to, where there is one. */
  side: Side | null;
  /** The stretch chosen on the waveform, in seconds. */
  selection: Region | null;
  /** Put the selection away, which adding a track from it does. */
  onSelect: (selection: Region | null) => void;
}): React.JSX.Element {
  const text = useText();
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
        // A nudge breaks the lock, and must: `NewBoundary::by_user` writes
        // every hand-placed boundary locked, so a nudge that respected the
        // lock refused the marker a person had just put down and was lining
        // up - which is the whole of what the first outside report meant by
        // the markers being problematic. §24's lock is a contract with the
        // detector, not with the hand that set it, and the forced move leaves
        // the row locked and the provenance the operator's afterwards.
        force: true,
      }),
    ).then(store.reload);
  };

  /**
   * Make a track of the stretch chosen on the waveform.
   *
   * The verb that was missing. Everything else in this header adjusts
   * something a detector already found - split one, merge two, nudge a
   * boundary - and none of them makes a track where there is not one. A first
   * user with a quiet opening on side B could correct it from `vcw tracks
   * add` on a terminal and from nowhere in the window: "I can drag across a
   * track to select it, but how do I then say that this highlighted area
   * should be A1 and all of the later ones should be renamed".
   *
   * Both halves of that sentence are the shell's: it places the two
   * boundaries and renumbers the side in timeline order. This clears the band
   * afterwards and selects what it made, because leaving a selection over a
   * track that now exists invites the same drag twice.
   */
  const add = () => {
    if (side === null || selection === null) {
      return;
    }
    const { from, to } = selection;
    void run(
      () =>
        api.addTrack({
          sideId: side.id,
          start: Math.min(from, to),
          end: Math.max(from, to),
        }),
      "add track",
    ).then((id) => {
      onSelect(null);
      if (typeof id === "number") {
        onChoose({ track: id, boundary: null });
      }
      store.reload();
    });
  };

  /**
   * Take a boundary away.
   *
   * `force`, for the reason the nudge above overrides the lock and in the
   * same place: every boundary a person places is written locked, so a delete
   * that honored the lock refused the marker somebody has just put down in
   * the wrong spot - which is exactly the marker they are asking to remove.
   * A boundary a track is using is still refused, and that refusal is the one
   * worth keeping: it means merge the tracks, not delete half of one.
   */
  const remove = (id: number) => {
    void run(() => api.deleteMarker({ boundaryId: id, force: true })).then(() => {
      if (chosen.boundary === id) {
        onChoose({ ...chosen, boundary: null });
      }
      store.reload();
    });
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
        remove(boundary.id);
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
        // Null leaves it alone. This form edits what is printed on the label;
        // the characteristic a track was cut to is set per side or per track
        // from `vcw tracks`, and nothing here should quietly clear it.
        playbackEq: null,
      }),
    ).then(() => {
      setEditing(null);
      store.reload();
    });
  };

  return (
    <section className="panel tracks">
      <header className="panel-head">
        <h2>{text("tracks.title")}</h2>
        <label className="face">
          {text("tracks.side")}
          <select
            value={face}
            onChange={(event) => setFace(event.target.value)}
            title={text("tracks.side.title")}
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
          {text("tracks.detect", { chord: "t" })}
        </button>
        <button
          type="button"
          disabled={side === null || selection === null}
          onClick={add}
          title={
            selection === null
              ? text("tracks.add.needs.selection")
              : text("tracks.add.title")
          }
        >
          {text("tracks.add")}
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
          title={text("tracks.split.title")}
        >
          {text("tracks.split")}
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
          title={text("tracks.merge.title")}
        >
          {text("tracks.merge")}
        </button>
      </header>

      {engine.detection !== null && (
        <p className="hint">
          {text("tracks.last.pass", {
            boundaries: engine.detection.boundaries,
            tracks: engine.detection.tracks,
            rejected: engine.detection.rejected,
            settled: engine.detection.alreadySettled,
            seconds: engine.detection.seconds.toFixed(2),
          })}
        </p>
      )}

      {project.tracks.length === 0 ? (
        /* The advice has to match the state, or it is worse than silence.
           "Press t to run detection" against no project at all sends a person
           to a key that is disabled, and tells them nothing about the step
           they actually have to take first. */
        <p className="empty">
          {project.path === null
            ? text("tracks.no.project", { chord: "Ctrl+1" })
            : project.captures.length === 0
              ? text("tracks.no.capture", { chord: "Ctrl+2" })
              : text("tracks.none", { chord: "t" })}
        </p>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th>{text("tracks.column.position")}</th>
              <th>{text("tracks.column.title")}</th>
              <th>{text("tracks.column.artist")}</th>
              <th className="n">{text("tracks.column.start")}</th>
              <th className="n">{text("tracks.column.length")}</th>
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
                    <span className="ok" title={text("tracks.confirmed")}>
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
            {text("tracks.edit.title")}
            <input
              autoFocus
              value={draft.title}
              onChange={(event) =>
                setDraft({ ...draft, title: event.target.value })
              }
            />
          </label>
          <label>
            {text("tracks.edit.artist")}
            <input
              value={draft.artist}
              onChange={(event) =>
                setDraft({ ...draft, artist: event.target.value })
              }
            />
          </label>
          <label>
            {text("tracks.edit.composer")}
            <input
              value={draft.composer}
              onChange={(event) =>
                setDraft({ ...draft, composer: event.target.value })
              }
            />
          </label>
          <label>
            {text("tracks.edit.comments")}
            <textarea
              value={draft.comments}
              onChange={(event) =>
                setDraft({ ...draft, comments: event.target.value })
              }
            />
          </label>
          <div className="row">
            <button type="submit">
              {text("tracks.edit.save", { chord: "Ctrl+S" })}
            </button>
            <button type="button" onClick={() => setEditing(null)}>
              {text("tracks.edit.cancel")}
            </button>
          </div>
        </form>
      )}

      <h3>{text("tracks.boundaries")}</h3>
      {project.boundaries.length === 0 ? (
        <p className="empty">{text("tracks.boundaries.none")}</p>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th>{text("tracks.column.side")}</th>
              <th>{text("tracks.column.edge")}</th>
              <th className="n">{text("tracks.column.at")}</th>
              <th className="n">{text("tracks.column.confidence")}</th>
              <th>{text("tracks.column.agreed")}</th>
              <th></th>
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
                <td>{text(EDGES[row.edge] ?? row.edge)}</td>
                <td className="n">{clock(row.seconds)}</td>
                <td className="n">{(row.confidence * 100).toFixed(0)}%</td>
                <td title={row.sources.join(", ")}>
                  {text("tracks.agreement", {
                    agreed: row.agreement,
                    of: row.sources.length,
                  })}
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
                        ? text("tracks.locked.title")
                        : text("tracks.unlocked.title")
                    }
                  >
                    {row.locked ? text("tracks.locked") : text("tracks.lock")}
                  </button>
                </td>
                <td>
                  {/* The recovery path that was complete and unclickable.
                      `delete_marker` has been on the contract since WP-11 and
                      reachable only from the `Delete` key, acting on a row
                      that first had to be selected - so a first user who put
                      a marker in the wrong place reported, correctly, that
                      there was no way to take it out again. */}
                  <button
                    type="button"
                    onClick={(event) => {
                      event.stopPropagation();
                      remove(row.id);
                    }}
                    title={text("tracks.delete.title", { chord: "Delete" })}
                  >
                    {text("tracks.delete")}
                  </button>
                </td>
                <td>{row.promoted ? "" : text("tracks.rejected")}</td>
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
  const text = useText();
  return (
    <dl className="evidence">
      <dt>{text("tracks.evidence.decided")}</dt>
      <dd>{boundary.provenance}</dd>
      <dt>{text("tracks.evidence.agreed")}</dt>
      <dd>{boundary.sources.join(", ")}</dd>
      {boundary.evidence.length > 0 && (
        <>
          <dt>{text("tracks.evidence.measurements")}</dt>
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
