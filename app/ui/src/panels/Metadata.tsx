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
// one and the provider decides the rest, because "a catalogue number alone
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
// label and the catalogue number are right even when the tracklist is not.

import { useState } from "react";

import * as api from "../api";
import type { Accepted, Candidate } from "../bindings/vcw";
import { useKeys } from "../keys";
import type { Store } from "../store";

/** The metadata browser. */
export function Metadata({ store }: { store: Store }): React.JSX.Element {
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
  const [accepted, setAccepted] = useState<Accepted | null>(null);

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

  useKeys("metadata", { lookup: search, accept });

  return (
    <section className="panel metadata">
      <header className="panel-head">
        <h2>Metadata</h2>
        <button type="button" disabled={searching} onClick={search}>
          {searching ? "Searching..." : "Look up (l)"}
        </button>
        <button type="button" disabled={chosen === null} onClick={accept}>
          Accept (Enter)
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
          Artist
          <input
            value={criteria.artist}
            onChange={(event) =>
              setCriteria({ ...criteria, artist: event.target.value })
            }
          />
        </label>
        <label>
          Album
          <input
            value={criteria.album}
            onChange={(event) =>
              setCriteria({ ...criteria, album: event.target.value })
            }
          />
        </label>
        <label>
          Catalogue number
          <input
            value={criteria.catalog}
            onChange={(event) =>
              setCriteria({ ...criteria, catalog: event.target.value })
            }
          />
        </label>
        <label>
          Barcode
          <input
            value={criteria.barcode}
            onChange={(event) =>
              setCriteria({ ...criteria, barcode: event.target.value })
            }
          />
        </label>
        <label>
          Provider
          <select
            value={provider}
            onChange={(event) => setProvider(event.target.value)}
          >
            <option value="">Every provider that is turned on</option>
            <option value="musicbrainz">MusicBrainz</option>
            <option value="discogs">Discogs</option>
          </select>
        </label>
      </form>

      {candidates.length === 0 ? (
        <p className="empty">
          No candidates. Fill in at least one field and press <kbd>l</kbd>.
        </p>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th>Album</th>
              <th>Artist</th>
              <th className="n">Year</th>
              <th>Label</th>
              <th>Catalogue</th>
              <th>Format</th>
              <th className="n">Tracks</th>
              <th>From</th>
            </tr>
          </thead>
          <tbody>
            {candidates.map((row) => (
              <tr
                key={key(row)}
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
                <td className="n">{row.tracks}</td>
                <td>{row.provider}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {accepted !== null && (
        <div className="accepted">
          <p>
            {accepted.album} by {accepted.albumArtist}: {accepted.named}{" "}
            track(s) named, {accepted.kept} left as they were.
          </p>
          {accepted.unmatched.length > 0 && (
            <p className="hint">
              The provider listed {accepted.unmatched.length} position(s) this
              project has no track at: {accepted.unmatched.join(", ")}. Nothing
              was guessed.
            </p>
          )}
          {accepted.unnamed.length > 0 && (
            <p className="hint">
              {accepted.unnamed.length} track(s) are not in the provider's
              tracklist: {accepted.unnamed.join(", ")}. Name them by hand in the
              track editor.
            </p>
          )}
        </div>
      )}
    </section>
  );
}

/** A candidate's identity, which is the provider *and* the id. */
function key(candidate: Candidate): string {
  return `${candidate.provider}:${candidate.id}`;
}
