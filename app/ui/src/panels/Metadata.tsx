/*
 *  Metadata.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Searching the providers, and accepting what one of them said (§26, §28).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Four fields and a list. The fields are the criteria §28 names, and the panel
// does not judge which combination is a good query: the shell refuses an empty
// one and the provider decides the rest, because "a catalog number alone
// beats an artist alone" is knowledge about Discogs and MusicBrainz rather than
// about forms.
//
// # Why accepting a release can report a disappointment
//
// `select_release` comes back with what it named and what it could not, and
// both are shown. The matching rule behind it is exact - side letter and track
// number, from the provider's own position - so a release whose tracklist does
// not line up with the captured sides names nothing and says which tracks it
// could not place. That is the honest outcome for a 2-LP reissue matched
// against a single-disc pressing, and the alternative, guessing by duration,
// is §26's confidence-weighted matching, which is Phase 2 work with its own
// gate.
//
// The panel therefore shows `unmatched` and `unnamed` as a result rather than
// as an error. The release row is written either way, because the album, the
// label and the catalog number are right even when the tracklist is not.
//
// # The two facts a provider cannot supply
//
// Mono and the characteristic the lacquer was cut with. Discogs will not tell
// you either one, and no analysis recovers them, so both are asked at project
// setup - and until this panel grew them, that was the only place they could
// be answered.
// They sit in this panel rather than in Capture or Export because this is the
// panel about what the record *is*: the fold to one channel and the curve to
// undo are consequences of those facts, not separate settings.

import { useCallback, useState } from "react";

import * as api from "../api";
import type { Accepted, Candidate, ReleaseEdit } from "../bindings/vcw";
import { label, useCurves } from "../curves";
import { type Translate, t, useText } from "../i18n";
import { useKeys } from "../keys";
import { step } from "../select";
import type { Store } from "../store";
import { Switch } from "./Switch";

/** The metadata browser. */
export function Metadata({ store }: { store: Store }): React.JSX.Element {
  const text = useText();
  const { project, run } = store;
  const [criteria, setCriteria] = useState({
    artist: "",
    album: "",
    catalog: "",
    barcode: "",
  });
  const [provider, setProvider] = useState("");
  const [candidates, setCandidates] = useState<readonly Candidate[]>([]);
  const [chosen, setChosen] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  // Whether a lookup has been run and come back, which is not the same as
  // having no candidates. Without it the panel said "No candidates. Fill in at
  // least one field" over a form with the album already in it, which is two
  // wrong things at once: nothing had been looked up, and the field it asked
  // for was filled.
  const [searched, setSearched] = useState(false);
  const [accepted, setAccepted] = useState<Accepted | null>(null);
  const curves = useCurves();

  // A project nobody has identified yet has no release row at all, and both
  // answers still have a sensible shape: stereo, and nobody has said. The row
  // is created by the write, not by reading it.
  const isMono = project.release?.isMono ?? false;
  const playbackEq = project.release?.playbackEq ?? "";

  const stateRelease = (edit: ReleaseEdit) => {
    void run(() => api.editRelease(edit)).then(store.reload);
  };

  // Seeded from the release row the first time the panel is opened, which is
  // what the four-field helper on project creation was for: what a person typed
  // then is the query now, and typing it twice is the thing being avoided.
  const [seeded, setSeeded] = useState(false);
  if (!seeded && project.release !== null) {
    setSeeded(true);
    setCriteria({
      artist: project.release.albumArtist,
      album: project.release.album,
      catalog: project.release.catalog,
      barcode: project.release.barcode ?? "",
    });
  }

  const search = () => {
    const blank = (value: string) => (value.trim() === "" ? null : value.trim());
    setSearching(true);
    setSearched(false);
    setAccepted(null);
    void run(async () => {
      const found = await api.searchMetadata({
        artist: blank(criteria.artist),
        album: blank(criteria.album),
        catalog: blank(criteria.catalog),
        barcode: blank(criteria.barcode),
        provider: provider === "" ? null : provider,
      });
      setCandidates(found);
      setChosen(found[0] === undefined ? null : key(found[0]));
      setSearched(true);
    }).then(() => setSearching(false));
  };

  const accept = () => {
    const candidate = candidates.find((row) => key(row) === chosen);
    if (candidate === undefined) {
      return;
    }
    void run(async () => {
      const result = await api.selectRelease({
        provider: candidate.provider,
        id: candidate.id,
      });
      setAccepted(result);
    }).then(store.reload);
  };

  // `accept` acts on `chosen`, which before WP-16a only a click could set - so
  // §44's choose-release needed a mouse for its middle step. The arrows are
  // that step. A lookup lands on nothing selected, and the first `ArrowDown`
  // takes the top candidate, which is the one the providers ranked first.
  const move = (delta: -1 | 1) => () =>
    setChosen(step(candidates.map(key), chosen, delta));

  const show = useCallback((row: HTMLTableRowElement | null) => {
    row?.scrollIntoView({ block: "nearest" });
  }, []);

  useKeys("metadata", {
    lookup: search,
    accept,
    previousCandidate: move(-1),
    nextCandidate: move(1),
  });

  return (
    <section className="panel metadata">
      <header className="panel-head">
        <h2>{text("metadata.title")}</h2>
        <button type="button" disabled={searching} onClick={search}>
          {searching
            ? text("metadata.searching")
            : text("metadata.lookup", { chord: "l" })}
        </button>
        <button type="button" disabled={chosen === null} onClick={accept}>
          {text("metadata.accept", { chord: "Enter" })}
        </button>
      </header>

      <form
        className="fields"
        onSubmit={(event) => {
          event.preventDefault();
          search();
        }}
      >
        <label>
          {text("metadata.artist")}
          <input
            value={criteria.artist}
            onChange={(event) =>
              setCriteria({ ...criteria, artist: event.target.value })
            }
          />
        </label>
        <label>
          {text("metadata.album")}
          <input
            value={criteria.album}
            onChange={(event) =>
              setCriteria({ ...criteria, album: event.target.value })
            }
          />
        </label>
        <label>
          {text("metadata.catalog")}
          <input
            value={criteria.catalog}
            onChange={(event) =>
              setCriteria({ ...criteria, catalog: event.target.value })
            }
          />
        </label>
        <label>
          {text("metadata.barcode")}
          <input
            value={criteria.barcode}
            onChange={(event) =>
              setCriteria({ ...criteria, barcode: event.target.value })
            }
          />
        </label>
        <label>
          {text("metadata.provider")}
          <select
            value={provider}
            onChange={(event) => setProvider(event.target.value)}
          >
            <option value="">{text("metadata.provider.any")}</option>
            <option value="musicbrainz">MusicBrainz</option>
            <option value="discogs">Discogs</option>
          </select>
        </label>
      </form>

      {/*
        The two the providers will not answer. Written straight through rather
        than held and saved: a switch and a sixteen-answer picker are discrete
        choices, so there is no half-typed state to protect and no debounce to
        get wrong.
      */}
      <fieldset className="pressing stacked">
        <legend>{text("metadata.pressing")}</legend>
        <div className="seed-switch">
          <span>{text("metadata.pressing.channels")}</span>
          <Switch
            checked={!isMono}
            onChange={(stereo) =>
              stateRelease({ isMono: !stereo, playbackEq: null })
            }
            off={text("metadata.mono")}
            on={text("metadata.stereo")}
            title={text("metadata.mono.title")}
          />
        </div>
        <label className="seed-field">
          {text("metadata.eq")}
          {/* The span clips the option box; see Browser.tsx for why. */}
          <span className="picker">
            <select
              value={playbackEq}
              title={text("metadata.eq.title")}
              onChange={(event) =>
                stateRelease({ isMono: null, playbackEq: event.target.value })
              }
            >
              {/*
                Empty is a real answer and not a placeholder: a release that
                states nothing is played back as RIAA by default, which is not
                the same as an operator saying it was cut to RIAA. Clearing the
                statement is how somebody takes back a guess.
              */}
              <option value="">{text("metadata.eq.unstated")}</option>
              {curves.map((curve) => (
                <option key={curve.id} value={curve.id}>
                  {label(curve)}
                </option>
              ))}
            </select>
          </span>
        </label>
      </fieldset>

      {candidates.length === 0 ? (
        <p className="empty">{nothing(criteria, searching, searched, text)}</p>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th>{text("metadata.column.album")}</th>
              <th>{text("metadata.column.artist")}</th>
              <th className="n">{text("metadata.column.year")}</th>
              <th>{text("metadata.column.label")}</th>
              <th>{text("metadata.column.catalog")}</th>
              <th>{text("metadata.column.format")}</th>
              <th className="n">{text("metadata.column.tracks")}</th>
              <th>{text("metadata.column.from")}</th>
            </tr>
          </thead>
          <tbody>
            {candidates.map((row) => (
              <tr
                key={key(row)}
                ref={key(row) === chosen ? show : null}
                className={key(row) === chosen ? "selected" : ""}
                onClick={() => setChosen(key(row))}
                onDoubleClick={accept}
              >
                <td>{row.album}</td>
                <td>{row.artist}</td>
                <td className="n">{row.year ?? ""}</td>
                <td>{row.label}</td>
                <td>{row.catalog}</td>
                <td>{row.format}</td>
                <td className="n">{row.tracks ?? "-"}</td>
                <td>{row.provider}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {accepted !== null && (
        <div className="accepted">
          <p>
            {text("metadata.accepted", {
              album: accepted.album,
              artist: accepted.albumArtist,
              named: accepted.named,
              kept: accepted.kept,
            })}
          </p>
          {accepted.unmatched.length > 0 && (
            <p className="hint">
              {text("metadata.unmatched", {
                count: accepted.unmatched.length,
                positions: accepted.unmatched.join(", "),
              })}
            </p>
          )}
          {accepted.unnamed.length > 0 && (
            <p className="hint">
              {text("metadata.unnamed", {
                count: accepted.unnamed.length,
                positions: accepted.unnamed.join(", "),
              })}
            </p>
          )}
          {accepted.relaid.length > 0 && (
            <p className="hint">
              {text("metadata.relaid", {
                sides: sides(accepted.relaid, text),
                count: accepted.relaid.length,
              })}
            </p>
          )}
          <p className="hint">
            {accepted.artwork > 0
              ? text("metadata.artwork", { size: kb(accepted.artwork, text) })
              : text("metadata.artwork.none")}
          </p>
        </div>
      )}
    </section>
  );
}

/**
 * The side letters a list of positions touches, as English.
 *
 * `["B1", "B2", "C1"]` is "sides B and C". Read off the positions rather than
 * taken from the release, because what matters to a person reading the line is
 * where their tracks went.
 */
export function sides(
  positions: readonly string[],
  text: Translate = t,
): string {
  const letters = [...new Set(positions.map((at) => at.slice(0, 1)))].sort();
  if (letters.length === 0) {
    return text("metadata.sides.none");
  }
  if (letters.length === 1) {
    return text("metadata.side", { letters: letters[0] ?? "" });
  }
  // The list and the sentence are separate entries, because "B, C and D" is
  // not how every language joins a list and "sides {letters}" is not how
  // every language wraps one.
  return text("metadata.sides", {
    letters: text("metadata.sides.last", {
      rest: letters.slice(0, -1).join(", "),
      last: letters[letters.length - 1] ?? "",
    }),
  });
}

/** A byte count as kilobytes, for a line about a cover. */
export function kb(bytes: number, text: Translate = t): string {
  return text("metadata.kb", { n: Math.round(bytes / 1024) });
}

/** A candidate's identity, which is the provider *and* the id. */
function key(candidate: Candidate): string {
  return `${candidate.provider}:${candidate.id}`;
}

/**
 * What to say when the candidate list is empty, which is three situations.
 *
 * Nothing has been looked up and the form is blank; nothing has been looked up
 * and the form is not blank, which is the usual case because the panel seeds
 * itself from the release; and a lookup ran and matched nothing. The old text
 * said the first of those regardless, so the common path read as a complaint
 * about a field that was already filled.
 */
export function nothing(
  criteria: { artist: string; album: string; catalog: string; barcode: string },
  searching: boolean,
  searched: boolean,
  text: Translate = t,
): string {
  if (searching) {
    return text("metadata.looking");
  }
  if (searched) {
    return text("metadata.nothing.matched");
  }
  const filled = Object.values(criteria).some((value) => value.trim() !== "");
  return filled
    ? text("metadata.nothing.ready", { chord: "l" })
    : text("metadata.nothing.blank", { chord: "l" });
}
