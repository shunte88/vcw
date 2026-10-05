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
import { effortOf } from "../effort";
import {
  type MeterStyle,
  SCALES,
  applyMeterStyle,
  applyScale,
  meterStyleOf,
  scaleOf,
} from "../scale";
import type { Store } from "../store";
import { TOKENS, unknownTokens } from "../template";

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

/**
 * The groups, in the order they are listed down the side.
 *
 * A single scrolling column of six fieldsets was the panel until a 4K window
 * made it a column of twenty-eight controls that all looked alike, and the
 * thing people actually do here - change one setting - meant scrolling past
 * five groups that were not it. So: pick a group, see that group. The nav is
 * `button`s and nothing else, which is why there is no key that moves it; Tab
 * reaches each one and Space chooses it, the same as every other button in
 * the window.
 *
 * Appearance is first because it is the one a person needs before they can
 * comfortably read the rest, and Credentials is last because it is the only
 * group that is a report rather than a setting.
 */
const SECTIONS = [
  "Appearance",
  "Library",
  "Audio",
  "Detection",
  "Metadata",
  "Export",
  "Credentials",
] as const;

/** One of [`SECTIONS`]. */
type Section = (typeof SECTIONS)[number];

/** Which group is on screen, remembered across a remount of the panel. */
const CHOSEN = "vcw.settings.section";

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
  const [section, setSection] = useState<Section>(() => {
    const stored = localStorage.getItem(CHOSEN) as Section | null;
    return stored !== null && SECTIONS.includes(stored) ? stored : "Appearance";
  });
  const [scale, setScale] = useState(scaleOf);
  const [meters, setMeters] = useState(meterStyleOf);

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

  // Which of the two encoder knobs the chosen default format has, if either.
  const effort = effortOf(draft.export.format);

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

      <div className="prefs">
        <nav className="prefs-nav" aria-label="Settings sections">
          {SECTIONS.map((name) => (
            <button
              key={name}
              type="button"
              className={name === section ? "current" : ""}
              aria-current={name === section}
              onClick={() => {
                localStorage.setItem(CHOSEN, name);
                setSection(name);
              }}
            >
              {name}
            </button>
          ))}
        </nav>

        <div className="prefs-pane">
        {section === "Appearance" && (
          <fieldset>
            <legend>Appearance</legend>
            <label>
              Interface scale
              {/* Not part of `Settings`: this is a fact about the monitor in
                  front of this window, not about the library, and the settings
                  file is read by the CLI and copied between machines. It is
                  applied to the root font size, which every measurement in the
                  stylesheet is written against, so the whole window grows
                  together rather than the text outgrowing its boxes. */}
              <select
                value={scale}
                onChange={(event) => {
                  const chosen = Number(event.target.value);
                  applyScale(chosen);
                  setScale(chosen);
                }}
              >
                {SCALES.map((percent) => (
                  <option key={percent} value={percent}>
                    {percent}%
                  </option>
                ))}
              </select>
            </label>
            <p className="hint">
              The default is sized for a 1080p screen or a desktop that is
              already scaling for you. On a 4K monitor at no scaling, 175% or
              200% is about the same physical size of letter. It takes effect
              as you choose it and is remembered on this machine only.
            </p>
            <label>
              Level meters
              <select
                value={meters}
                onChange={(event) => {
                  const chosen = event.target.value as MeterStyle;
                  applyMeterStyle(chosen);
                  setMeters(chosen);
                }}
              >
                <option value="bars">Bars</option>
                <option value="vu">VU dials</option>
              </select>
            </label>
            <p className="hint">
              The bars give the peak, the RMS and the clip count to a tenth of
              a decibel, which is what setting a level wants. The dials have
              the movement of the real thing - 300 ms to full deflection, and
              a needle with mass - which is what watching a side from across
              the room wants. Both read the same scale.
            </p>
          </fieldset>
        )}

        {section === "Library" && (
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
        )}

        {section === "Audio" && (
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
            Equalization on input
            <select
              value={draft.audio.eq ?? "unknown"}
              onChange={(event) => audio({ eq: event.target.value })}
            >
              <option value="unknown">Not stated</option>
              <option value="riaa">RIAA, from a phono stage</option>
              <option value="flat">Flat, no curve applied</option>
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
        )}

        {section === "Detection" && (
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
        )}

        {section === "Metadata" && (
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
              placeholder="unset - VCW_CONTACT, or nothing"
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
          <p className="hint">
            MusicBrainz needs no account, but it does ask who is calling: an
            address here is sent in the user agent on every request, and without
            one the rate limit is harder and a lookup can be refused outright. An
            email address or a URL, and public by design. Discogs needs a token,
            which is a secret and so is never kept here - export
            VCW_DISCOGS_TOKEN before starting VCW, and the Credentials table
            below says whether it arrived.
          </p>
        </fieldset>
        )}

        {section === "Export" && (
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
              <option value="mp3">MP3</option>
              <option value="ogg">Ogg Vorbis</option>
            </select>
          </label>
          {/*
            The caption and the options follow the format, because the two
            settings behind this one control are not the same setting: FLAC has
            a compression level, which changes only the size and the time, and
            MP3 and Ogg have a quality, which changes what survives. This used
            to read "Lossy quality" whatever the format was, so a person picking
            FLAC for an archival copy was told their archival copy was lossy.

            WAV has neither and gets no field. Both values stay in the draft
            whichever is on screen - the backend ignores the one that does not
            apply - so switching the default format and back does not lose a
            choice, which was the thing the old always-visible field was for.
          */}
          {effort !== null && (
            <label>
              {effort.caption}
              <select
                value={draft.export[effort.field]}
                onChange={(event) =>
                  exporting({ [effort.field]: event.target.value })
                }
              >
                {effort.options.map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label>
            Output directory
            <input
              value={draft.export.output ?? ""}
              onChange={(event) => exporting({ output: text(event.target.value) })}
            />
          </label>
          <label>
            Naming template
            {/* Twice the width of the other fields, because it is the only one
                whose value is longer than a word: the default is 44 characters
                and a template with a catalog number and a year in it runs to
                seventy, which an 18ch box showed a third of. */}
            <input
              className="template"
              value={draft.export.template}
              onChange={(event) => exporting({ template: event.target.value })}
            />
          </label>
          {/* Validated as it is typed, by the same reading of the template the
              expander does - `template.ts` says why there are two copies and
              what stops them drifting. An unknown token is not refused here:
              the field keeps whatever is typed into it, and the export refuses
              it with a "did you mean" if it is still wrong by then. This is
              the help, not the gate. */}
          {unknownTokens(draft.export.template).length > 0 && (
            <p className="hint warn near">
              No such token:{" "}
              {unknownTokens(draft.export.template)
                .map((token) => `{${token}}`)
                .join(", ")}
              . An export with this template would be refused.
            </p>
          )}
          <p className="hint near">
            Anything in <code>{"{braces}"}</code> is substituted; anything in{" "}
            <code>[square brackets]</code> disappears if every token inside it
            is empty. The extension comes from the format, not the template.
          </p>
          <ul className="tokens">
            {TOKENS.map((token) => (
              <li key={token}>
                <code>{`{${token}}`}</code>
              </li>
            ))}
          </ul>
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
        )}

        {section === "Credentials" && (
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
        )}
        </div>
      </div>
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
