/*
 *  Settings.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §39's settings, and which credentials are present without showing one.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Every field here is a field of `Settings`, written whole by `save_settings`.
// Whole rather than per-field, because a settings file written a field at a
// time is a settings file that can be half of two versions, and because the
// shell validates the set together - a detector name and a source count that
// contradict each other are one refusal, not two.
//
// # Credentials
//
// The panel shows a name, whether it is present, how many characters it has and
// which environment variable it comes from. It does not show a token, and it
// cannot: there is no command in the shell that returns one, which is a
// stronger guarantee than a masked input. §39 keeps credentials in the
// environment and out of every project file, so the only thing this panel can
// usefully say is "yes, that one is set", and that is what it says.
//
// A person changing a token does it where tokens belong - in the environment -
// and the panel tells them what to set. Offering an input here would mean the
// application accepting a secret it has nowhere safe to put.

import { useEffect, useState } from "react";

import * as api from "../api";
import type { Credential, Device, Settings as Values } from "../bindings/vcw";
import type { Store } from "../store";

/**
 * The sample formats the contract accepts, which are `parse_format`'s.
 *
 * Four of them, fixed, and not read from a device: this is the §39 default
 * applied before a device is chosen, so there is nothing to ask. It was a text
 * input, which meant a person could save `float32` - an alias the CLI takes
 * and this field does not - and find out at arming time.
 */
const FORMATS: readonly { value: string; label: string }[] = [
  { value: "s16", label: "s16 (16-bit integer)" },
  { value: "s24", label: "s24 (24-bit integer)" },
  { value: "s32", label: "s32 (32-bit integer)" },
  { value: "f32", label: "f32 (32-bit float)" },
];

/** The settings panel. */
export function Settings({
  store,
  settings,
  credentials,
  devices,
  onSaved,
}: {
  store: Store;
  settings: Values | null;
  credentials: readonly Credential[];
  devices: readonly Device[];
  onSaved: () => void;
}): React.JSX.Element {
  const [draft, setDraft] = useState<Values | null>(settings);

  useEffect(() => setDraft(settings), [settings]);

  if (draft === null) {
    return (
      <section className="panel settings empty">
        <p>Reading settings...</p>
      </section>
    );
  }

  const save = () => {
    void store.run(() => api.saveSettings(draft)).then(onSaved);
  };

  // One helper per group rather than a generic path setter, because a generic
  // one would need a string path and lose the type that makes this file safe.
  const audio = (patch: Partial<Values["audio"]>) =>
    setDraft({ ...draft, audio: { ...draft.audio, ...patch } });
  const recording = (patch: Partial<Values["recording"]>) =>
    setDraft({ ...draft, recording: { ...draft.recording, ...patch } });
  const detection = (patch: Partial<Values["detection"]>) =>
    setDraft({ ...draft, detection: { ...draft.detection, ...patch } });
  const metadata = (patch: Partial<Values["metadata"]>) =>
    setDraft({ ...draft, metadata: { ...draft.metadata, ...patch } });
  const exporting = (patch: Partial<Values["export"]>) =>
    setDraft({ ...draft, export: { ...draft.export, ...patch } });

  const text = (value: string) => (value.trim() === "" ? null : value.trim());
  const number = (value: string) =>
    value.trim() === "" ? null : Number(value);

  return (
    <section className="panel settings">
      <header className="panel-head">
        <h2>Settings</h2>
        <button type="button" onClick={save}>
          Save
        </button>
      </header>

      <fieldset>
        <legend>Library</legend>
        <label>
          Where projects are kept
          <input
            value={draft.recording.library ?? ""}
            placeholder="unset - no library"
            onChange={(event) =>
              recording({ library: text(event.target.value) })
            }
          />
        </label>
        <label>
          Commit every (seconds)
          <input
            type="number"
            min="1"
            value={draft.recording.checkpointSeconds ?? ""}
            placeholder="unset - built-in"
            onChange={(event) =>
              recording({ checkpointSeconds: number(event.target.value) })
            }
          />
        </label>
        <p className="hint">
          The commit interval is the crash-loss floor: a crash loses the
          uncommitted frames plus whatever the driver was holding, and the ring
          size does not enter into it.
        </p>
        <label>
          A project that was not closed cleanly
          <select
            value={draft.recording.recovery ?? "ask"}
            onChange={(event) => recording({ recovery: event.target.value })}
          >
            <option value="ask">Ask</option>
            <option value="recover">Recover it</option>
            <option value="leave">Leave it alone</option>
          </select>
        </label>
      </fieldset>

      <fieldset>
        <legend>Audio</legend>
        <label>
          Host API
          {/* The hosts this build enumerated, not the five that exist. A text
              input here accepted "pulse" on a machine with no such host and
              said nothing until something tried to open a device. */}
          <select
            value={draft.audio.backend ?? ""}
            onChange={(event) => audio({ backend: text(event.target.value) })}
          >
            <option value="">Whatever the build picks</option>
            {hosts(devices).map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
            {draft.audio.backend !== null &&
              !hosts(devices).includes(draft.audio.backend) && (
                <option value={draft.audio.backend}>
                  {draft.audio.backend} (not on this machine)
                </option>
              )}
          </select>
        </label>
        <label>
          Rate (Hz)
          <input
            type="number"
            value={draft.audio.rate ?? ""}
            placeholder="unset - device"
            onChange={(event) => audio({ rate: number(event.target.value) })}
          />
        </label>
        <label>
          Format
          <select
            value={draft.audio.format ?? ""}
            onChange={(event) => audio({ format: text(event.target.value) })}
          >
            <option value="">Whatever the device offers</option>
            {FORMATS.map((row) => (
              <option key={row.value} value={row.value}>
                {row.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Exclusivity
          <select
            value={draft.audio.mode ?? "shared"}
            onChange={(event) => audio({ mode: event.target.value })}
          >
            <option value="shared">Shared</option>
            <option value="native">Native</option>
            <option value="exclusive">Exclusive</option>
          </select>
        </label>
        <label>
          Ring (milliseconds)
          <input
            type="number"
            value={draft.audio.ringMillis ?? ""}
            placeholder="unset - built-in"
            onChange={(event) =>
              audio({ ringMillis: number(event.target.value) })
            }
          />
        </label>
      </fieldset>

      <fieldset>
        <legend>Detection</legend>
        <label>
          Detectors
          <select
            value={draft.detection.algorithm}
            onChange={(event) => detection({ algorithm: event.target.value })}
          >
            <option value="all">All of them</option>
            <option value="silence">Silence only</option>
            <option value="spectral">Spectral change only</option>
            <option value="hmm">HMM only</option>
          </select>
        </label>
        <label>
          Detectors that must agree
          <input
            type="number"
            min="1"
            value={draft.detection.minSources}
            onChange={(event) =>
              detection({ minSources: Number(event.target.value) })
            }
          />
        </label>
        <label>
          Least confidence to promote
          <input
            type="number"
            min="0"
            max="1"
            step="0.05"
            value={draft.detection.minConfidence}
            onChange={(event) =>
              detection({ minConfidence: Number(event.target.value) })
            }
          />
        </label>
        <label>
          Shortest gap that counts as silence (seconds)
          <input
            type="number"
            min="0"
            step="0.1"
            value={draft.detection.minSilenceSeconds}
            onChange={(event) =>
              detection({ minSilenceSeconds: Number(event.target.value) })
            }
          />
        </label>
        <label>
          Shortest track (seconds)
          <input
            type="number"
            min="0"
            step="1"
            value={draft.detection.minTrackSeconds}
            onChange={(event) =>
              detection({ minTrackSeconds: Number(event.target.value) })
            }
          />
        </label>
        <p className="hint">
          A boundary a person has locked is promoted whatever these say. That is
          the point of the lock.
        </p>
      </fieldset>

      <fieldset>
        <legend>Metadata</legend>
        <label className="tick">
          <input
            type="checkbox"
            checked={draft.metadata.online}
            onChange={(event) => metadata({ online: event.target.checked })}
          />
          Allow network lookups
        </label>
        <label className="tick">
          <input
            type="checkbox"
            checked={draft.metadata.musicbrainz}
            onChange={(event) =>
              metadata({ musicbrainz: event.target.checked })
            }
          />
          MusicBrainz
        </label>
        <label className="tick">
          <input
            type="checkbox"
            checked={draft.metadata.discogs}
            onChange={(event) => metadata({ discogs: event.target.checked })}
          />
          Discogs
        </label>
        <label>
          Contact address for the user agent
          <input
            value={draft.metadata.contact ?? ""}
            onChange={(event) => metadata({ contact: text(event.target.value) })}
          />
        </label>
        <label>
          Genre map
          <input
            value={draft.metadata.genreMap ?? ""}
            placeholder="unset - built-in"
            onChange={(event) =>
              metadata({ genreMap: text(event.target.value) })
            }
          />
        </label>
      </fieldset>

      <fieldset>
        <legend>Export defaults</legend>
        <label>
          Format
          <select
            value={draft.export.format}
            onChange={(event) => exporting({ format: event.target.value })}
          >
            <option value="flac">FLAC</option>
            <option value="wav">WAV</option>
          </select>
        </label>
        <label>
          Output directory
          <input
            value={draft.export.output ?? ""}
            onChange={(event) => exporting({ output: text(event.target.value) })}
          />
        </label>
        <label>
          Naming template
          <input
            value={draft.export.template}
            onChange={(event) => exporting({ template: event.target.value })}
          />
        </label>
        <label>
          Artwork
          <select
            value={draft.export.artwork}
            onChange={(event) => exporting({ artwork: event.target.value })}
          >
            <option value="none">None</option>
            <option value="embed">Embedded</option>
            <option value="folder">Folder image</option>
            <option value="both">Both</option>
          </select>
        </label>
      </fieldset>

      <fieldset>
        <legend>Credentials</legend>
        <p className="hint">
          Read from the environment and never written to a project file. This
          list is everything the application can tell you about one.
        </p>
        <table className="rows">
          <thead>
            <tr>
              <th>Credential</th>
              <th>Set</th>
              <th className="n">Characters</th>
              <th>Environment variable</th>
            </tr>
          </thead>
          <tbody>
            {credentials.map((credential) => (
              <tr key={credential.variable}>
                <td>{credential.name}</td>
                <td>
                  {credential.present ? (
                    <span className="ok">yes</span>
                  ) : (
                    <span className="dim">no</span>
                  )}
                </td>
                <td className="n">
                  {credential.present ? credential.characters : ""}
                </td>
                <td>
                  <code>{credential.variable}</code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </fieldset>
    </section>
  );
}

/**
 * The host APIs this build enumerated, in the order they were reported.
 *
 * Taken from the device list rather than from a fixed list of CPAL's hosts,
 * because §39's `backend` is a filter applied to what the build can see and a
 * name it cannot see is a setting that does nothing. A machine with no devices
 * at all offers only "whatever the build picks", which is the honest answer.
 */
function hosts(devices: readonly Device[]): readonly string[] {
  return [...new Set(devices.map((row) => row.host))];
}
