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

import { useEffect, useRef, useState } from "react";

import * as api from "../api";
import type { Credential, Device, Settings as Values } from "../bindings/vcw";
import { effortOf } from "../effort";
import {
  type ButtonStyle,
  type MeterStyle,
  SCALES,
  applyButtonStyle,
  applyMeterStyle,
  applyScale,
  buttonStyleOf,
  meterStyleOf,
  scaleOf,
} from "../scale";
import type { Store } from "../store";
import { TOKENS, unknownTokens } from "../template";
import { Switch } from "./Switch";

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
 * The language VCW is written in, and the one every menu has at least.
 *
 * Spelled here as well as in `vcw-i18n` because the select needs a value
 * before the shell has answered, and a `null` language means this one.
 */
const SOURCE_LOCALE = "en-US";

/**
 * A language tag as the people who speak it write it.
 *
 * `Intl.DisplayNames` and no table of our own: the browser already ships
 * every endonym, and a list in this file would mean a translator submitting
 * `pt-BR.toml` also has to get a line into a TypeScript constant before their
 * own language has a name. Falls back to the tag, which is still a thing a
 * person can recognize.
 */
function nameOf(tag: string): string {
  try {
    return new Intl.DisplayNames([tag], { type: "language" }).of(tag) ?? tag;
  } catch {
    return tag;
  }
}

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
  const [buttons, setButtons] = useState(buttonStyleOf);
  // Asked for once, when the panel opens. The list is a directory listing in
  // the shell and the directory does not change while a person reads a menu.
  const [languages, setLanguages] = useState<readonly string[]>([
    SOURCE_LOCALE,
  ]);

  useEffect(() => setDraft(settings), [settings]);

  // # Where a first run lands
  //
  // On Library, with the field focused, and only on a first run. A piCorePlayer
  // user got the window open and then had nowhere to go: the browser is empty
  // because no library is set, and nothing on screen says that setting one is
  // the first move. Appearance is the right default for the second run and the
  // wrong one for the first.
  //
  // `stored === null` is what makes it a first run - a person who has ever
  // chosen a section has said where they want to be, and an empty library
  // after that is a state they put the application in deliberately. An effect
  // rather than the initializer above because `settings` is still null while
  // the shell reads the file, so the initializer cannot see the library.
  const [landed, setLanded] = useState(false);
  useEffect(() => {
    if (landed || settings === null) {
      return;
    }
    setLanded(true);
    if (localStorage.getItem(CHOSEN) === null && settings.recording.library === null) {
      setSection("Library");
    }
  }, [settings, landed]);
  useEffect(() => {
    void api.languages().then(setLanguages);
  }, []);

  // The draft saves itself, which is why there is no Save button. Three things
  // this has to get right.
  //
  // Debounced, because the naming template and the contact address are text
  // fields, and committing on change would write once a keystroke.
  //
  // Compared by value and not by reference, because saving refetches the
  // settings and the effect above then hands this one a fresh object holding
  // exactly what was just written. A reference test would save that straight
  // back and never stop.
  //
  // And the timer restarts for the data and for nothing else. The first
  // version listed `store` and `onSaved` in its dependencies, which looks
  // correct and is fatal: `useStore` returns a new object literal every render
  // and the window re-renders at meter rate, so the effect re-ran sixty times a
  // second and its own cleanup cancelled the timer every time. The panel said
  // "Saving..." and nothing was ever written, and no test saw it because a test
  // renders when something changes and the window renders always. The write
  // lives in a ref instead, refreshed each render, so the dependency list can
  // be the two values that are actually a reason to save again.
  const commit = useRef<(values: Values) => void>(() => undefined);
  useEffect(() => {
    commit.current = (saving: Values) => {
      void store
        .run(() => api.saveSettings(saving), "save settings")
        .then(onSaved);
    };
  });
  useEffect(() => {
    if (draft === null || JSON.stringify(draft) === JSON.stringify(settings)) {
      return;
    }
    const timer = setTimeout(() => commit.current(draft), 600);
    return () => clearTimeout(timer);
  }, [draft, settings]);

  if (draft === null) {
    return (
      <section className="panel settings empty">
        <p>Reading settings...</p>
      </section>
    );
  }

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

  // The Discogs token, reported where it is asked for rather than only in the
  // Credentials group: "Discogs needs a token" and "your token did not arrive"
  // are the same question, and they were two sections apart.
  const discogsToken = credentials.find(
    (credential) => credential.variable === "VCW_DISCOGS_TOKEN",
  );

  const unsaved = JSON.stringify(draft) !== JSON.stringify(settings);

  const text = (value: string) => (value.trim() === "" ? null : value.trim());
  const number = (value: string) =>
    value.trim() === "" ? null : Number(value);

  return (
    <section className="panel settings">
      <header className="panel-head">
        <h2>Settings</h2>
        {/* What replaced the Save button. Derived rather than remembered,
            because `store.run` swallows a refusal to put it in the status bar
            and resolves either way, so a flag set in `.then` would read
            "Saved" over a write that was refused. This cannot: the word goes
            when the stored settings catch up with the draft, and if the write
            never lands it stays, beside the reason. */}
        {unsaved && <span className="dim">Saving...</span>}
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
              Language
              {/* In `Settings` and not in local storage, unlike the three
                  below: those are facts about the monitor in front of this
                  window, and this one is a fact about the person. The CLI
                  prints the same sentences and reads the same file. */}
              <select
                value={draft.language ?? SOURCE_LOCALE}
                onChange={(event) =>
                  setDraft({ ...draft, language: event.target.value })
                }
              >
                {languages.map((tag) => (
                  <option key={tag} value={tag}>
                    {nameOf(tag)}
                  </option>
                ))}
              </select>
            </label>
            <p className="hint">
              VCW ships in US English and is translated by the people who use
              it. To add a language, copy <code>i18n/en-US.toml</code> out of
              the repository, replace each <code>text</code> with yours, and
              put it beside your settings file as{" "}
              <code>i18n/&lt;language tag&gt;.toml</code> - it appears in this
              menu as soon as you reopen the panel. Anything you have not
              translated stays in English rather than going blank.
            </p>
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
            <label>
              Buttons
              <select
                value={buttons}
                onChange={(event) => {
                  const chosen = event.target.value as ButtonStyle;
                  applyButtonStyle(chosen);
                  setButtons(chosen);
                }}
              >
                <option value="text">Words</option>
                <option value="icons">Icons</option>
              </select>
            </label>
            <p className="hint">
              Both rows at once: the panel tabs along the top and the transport
              along the bottom. Icons give the rows back about a third of their
              width, and every one of them keeps its name - the tooltip still
              reads <em>Record (r)</em>, and a screen reader still hears the
              word. Remembered on this machine only, like the two above.
            </p>
          </fieldset>
        )}

        {section === "Library" && (
        <fieldset>
          <legend>Library</legend>
          <label>
            Where projects are kept
            <input
              // The one field a fresh install has to be given. See "Where a
              // first run lands" above: this is the end of that journey.
              autoFocus={draft.recording.library === null}
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
        <fieldset className="stacked">
          <legend>Metadata</legend>

          {/* The master switch, and it stays a switch of its own. The two below
              it choose which *catalogs a search consults*; this one chooses the
              transport, and it is the only thing in the product that stops an
              AcoustID lookup or a cover-art download - neither of which goes
              through MusicBrainz or Discogs. Derived from "either provider is
              on", a both-off configuration would still reach the network. */}
          <div className="setting-switch">
            <span>Allow network lookups</span>
            <Switch
              checked={draft.metadata.online}
              onChange={(online) => metadata({ online })}
              off="Offline"
              on="Online"
              title="The master switch: offline, VCW opens no socket at all"
            />
          </div>
          <p className="hint">
            Offline, nothing here reaches the network: no catalog search, and
            also no AcoustID fingerprint lookup and no cover art, neither of
            which the two switches below cover. Online, those two choose which
            catalogs a search asks.
          </p>

          <div className="setting-switch">
            <span>MusicBrainz</span>
            <Switch
              checked={draft.metadata.musicbrainz}
              onChange={(musicbrainz) => metadata({ musicbrainz })}
              off="Skip"
              on="Search"
              title="Consult MusicBrainz when searching for a release"
            />
          </div>
          <label>
            Contact address for the user agent
            <input
              value={draft.metadata.contact ?? ""}
              placeholder="unset - VCW_CONTACT, or nothing"
              onChange={(event) => metadata({ contact: text(event.target.value) })}
            />
          </label>
          <p className="hint">
            MusicBrainz needs no account, but it does ask who is calling: an
            address here is sent in the user agent on every request, and without
            one the rate limit is harder and a lookup can be refused outright. An
            email address or a URL, and public by design - it is not a secret, so
            unlike the token below it can live in settings. <code>VCW_CONTACT</code>{" "}
            in the environment is used when this is empty.
          </p>

          <div className="setting-switch">
            <span>Discogs</span>
            <Switch
              checked={draft.metadata.discogs}
              onChange={(discogs) => metadata({ discogs })}
              off="Skip"
              on="Search"
              title="Consult Discogs when searching for a release"
            />
          </div>
          <p className="hint">
            <code>VCW_DISCOGS_TOKEN</code>{" "}
            {discogsToken?.present === true ? (
              <span className="ok">
                set, {discogsToken.characters} characters
              </span>
            ) : (
              <span className="dim">not set in this process</span>
            )}
          </p>
          <p className="hint">
            Discogs needs a personal access token, which is a secret: §39 keeps
            it out of every project and settings file, so there is no field for
            it here. Export it before starting VCW and the line above says
            whether it arrived - a desktop launcher starts VCW with its own
            environment rather than your shell&apos;s, which is the usual reason
            a token you have exported reads as not set. Generating one takes a
            Discogs account and no application review.
          </p>
          <p className="hint">
            <button
              type="button"
              className="as-link globe-link"
              onClick={() => {
                void store.run(() => api.support("discogs-token"));
              }}
            >
              <span className="icon globe-mark" aria-hidden="true" />
              Get Discogs API Token
            </button>
          </p>

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
            What a provider calls a genre is not what your catalog calls one:
            Discogs answers <code>Electronic</code> with a style of{" "}
            <code>Dub Techno</code>, and a 1994 pressing carries whatever its
            cataloger typed. The genre map folds those into your own names
            before they are stored or written to a tag. Leave it empty for the
            built-in table - 639 mappings ported from VRipr - or give the path
            to a file of <code>key|Genre; Another</code> lines, one per row,{" "}
            <code>#</code> for a comment. Matching is exact first and
            case-insensitive second, and a genre the table does not mention
            passes through unchanged.
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
              <option value="aiff">AIFF</option>
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

            WAV and AIFF have neither and get no field. Both values stay in the draft
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
          {/*
            Two controls over one stored word, because the two questions a
            person actually asks - what the number looks like, and what it
            counts within - have three answers between them and not four:
            `A6` is not a thing a record label prints. So the second control
            is disabled while the first says Label, and reads "Per side",
            which is what the label's own numbering is.
          */}
          <h3>Track numbering</h3>
          <p className="hint near">
            What <code>{"{tracknum}"}</code> and <code>{"{side}"}</code> expand
            to in the template above, and what goes in the track number tag.
            An export setting rather than a project one: it does not change
            what the record is numbered, only what this export writes.
          </p>
          <label>
            Track numbers
            <select
              value={draft.export.numbering === "alpha" ? "alpha" : "numeric"}
              onChange={(event) =>
                exporting({
                  numbering:
                    event.target.value === "alpha"
                      ? "alpha"
                      : draft.export.numbering === "sequence"
                        ? "sequence"
                        : "numeric",
                })
              }
            >
              <option value="alpha">Label (A1, B2)</option>
              <option value="numeric">Number (01, 02)</option>
            </select>
          </label>
          <label>
            Counted
            <select
              value={draft.export.numbering === "sequence" ? "sequence" : "numeric"}
              disabled={draft.export.numbering === "alpha"}
              onChange={(event) => exporting({ numbering: event.target.value })}
            >
              <option value="numeric">Per side (01 again on side B)</option>
              <option value="sequence">Across the disc (01..0n)</option>
            </select>
          </label>
          <p className="hint near">
            Sides A and B are disc 1, C and D are disc 2, so a sequence
            restarts at the next record rather than running to the end of a box
            set - which is also what a track number tag means. <em>Label</em>{" "}
            leaves <code>{"{side}"}</code> as a letter; either number form
            makes it a number too, so <code>{"{side}-{tracknum}"}</code> reads{" "}
            <code>01-01</code> rather than <code>A-01</code>.
          </p>
          {/*
            Three controls and not one, because narrowing a float master is
            three decisions: how many bits, what noise, and how much room to
            leave. All three have answers out of the box - 24-bit, triangular,
            no attenuation - so an installation nobody configures produces a
            file. "Refuse" is still the first option in the list, because
            somebody who would rather be asked than have VCW choose should not
            have to go looking for that.

            Always visible rather than shown only while the default format is
            FLAC: the default format is a default, and the Export panel can be
            pointed at FLAC on the day without coming back through here. The
            other two are disabled while the first says refuse, which is the
            honest way to say they are read but not yet.
          */}
          <h3>32-bit float captures</h3>
          <p className="hint near">
            FLAC is an integer codec, so a <code>f32</code> capture has to be
            brought down to whole numbers before it has a FLAC path. VCW does
            that at 24 bits with a triangular dither unless you say otherwise,
            which is the answer below; choose <em>Refuse</em> and it will ask
            instead, by refusing the export and naming the containers that would
            have taken the capture as it is. Nothing here touches the capture:
            it is read as it was recorded every time.
          </p>
          <label>
            Narrow to
            <select
              value={draft.export.narrowing}
              onChange={(event) => exporting({ narrowing: event.target.value })}
            >
              <option value="refuse">Refuse (export as WAV instead)</option>
              <option value="24">24-bit integer</option>
              <option value="32">32-bit integer</option>
            </select>
          </label>
          <label>
            Dither
            <select
              value={draft.export.dither}
              disabled={draft.export.narrowing === "refuse"}
              onChange={(event) => exporting({ dither: event.target.value })}
            >
              <option value="tpdf">Triangular</option>
              <option value="none">None (round only)</option>
            </select>
          </label>
          <label>
            Headroom (dB)
            <input
              value={draft.export.headroom}
              disabled={draft.export.narrowing === "refuse"}
              onChange={(event) => exporting({ headroom: event.target.value })}
            />
          </label>
          <p className="hint near">
            24-bit is the archival answer: an <code>f32</code> sample carries a
            24-bit significand, so that is the width at which a sample at full
            scale arrives intact, and it is also the width at which FLAC can
            still code one channel against the other. 32-bit adds no resolution
            to any single sample and does keep the float's scale, which matters
            on a capture with very quiet passages.
          </p>
          <p className="hint near">
            Triangular dither trades about 5 dB of noise floor - at 24 bits,
            some 120 dB below full scale and well under the surface noise of any
            record - for a rounding error that is noise rather than distortion.
            Headroom is attenuation applied before rounding: nothing clips in
            floating point, so a capture peaking above 0 dBFS is rounded to the
            ceiling unless you leave it room.
          </p>
        </fieldset>
        )}

        {section === "Credentials" && (
        <fieldset>
          <legend>Credentials</legend>
          <p className="hint">
            Read from the environment and never written to a project file. This
            list is everything the application can tell you about one.
          </p>
          <p className="hint">
            Read once, when VCW starts, from the environment of whatever started
            it - which is why a credential you have exported can read as not set
            here. A desktop launcher, a dock icon or a .desktop entry starts VCW
            from the session&apos;s environment and not from your shell&apos;s,
            so an <code>export</code> in <code>.bashrc</code> or typed into a
            terminal is invisible to it. Either start VCW from that same
            terminal, or put the variable somewhere the session reads -{" "}
            <code>~/.profile</code> on Linux, <code>launchctl setenv</code> on
            macOS, the user environment variables on Windows - and log in again.
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
