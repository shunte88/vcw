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
import { key, useText } from "../i18n";
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
  { value: "s16", label: key("settings.format.s16") },
  { value: "s24", label: key("settings.format.s24") },
  { value: "s32", label: key("settings.format.s32") },
  { value: "f32", label: key("settings.format.f32") },
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
  "Transfer",
  "Credentials",
] as const;

/** One of [`SECTIONS`]. */
type Section = (typeof SECTIONS)[number];

/**
 * What each group is called on screen.
 *
 * The name down the side and the name in local storage are two different
 * things: one is read by a person and the other is written to
 * `vcw.settings.section` and compared against [`SECTIONS`] on the next run.
 * Translating the first would have translated the second, and a panel
 * remembered as "Apariencia" opens on Appearance for somebody who has since
 * switched back to English.
 */
const NAMES: Record<Section, string> = {
  Appearance: key("settings.section.appearance"),
  Library: key("settings.section.library"),
  Audio: key("settings.section.audio"),
  Detection: key("settings.section.detection"),
  Metadata: key("settings.section.metadata"),
  Export: key("settings.section.export"),
  Transfer: key("settings.section.transfer"),
  Credentials: key("settings.section.credentials"),
};

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
  const text = useText();
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

  /**
   * The JSON of what the shell is holding, as far as this panel knows.
   *
   * One string answers two questions that used to be asked of `settings`
   * directly: whether the draft is dirty, and whether an arriving `settings`
   * is the echo of this panel's own write or news from somewhere else.
   */
  const stored = useRef<string | null>(null);

  /**
   * The draft as of the last render.
   *
   * Both effects below need it and neither may depend on it: one must run
   * only when the shell's copy changes, and the other only when a person has
   * finished with a field. The window renders at meter rate, so a dependency
   * on the draft would be a dependency on nothing in particular.
   */
  const latest = useRef<Values | null>(draft);
  latest.current = draft;

  /** Whether the draft holds edits the shell has not been told about. */
  const dirty = () =>
    stored.current !== null &&
    latest.current !== null &&
    JSON.stringify(latest.current) !== stored.current;

  // Adopt the shell's copy, *unless* this panel is holding unsaved edits.
  //
  // This line used to be `setDraft(settings)` with no condition, and that is
  // the keystroke eater a piCorePlayer user hit over `vcw serve`: a save
  // refetches, the refetch lands here, and the draft is replaced by whatever
  // was last written - so a word typed while the round trip was in the air
  // came back truncated. Invisible through the Tauri shell, where the trip is
  // a millisecond; plainly visible over HTTP to a Pi in another room.
  useEffect(() => {
    if (settings === null) {
      return;
    }
    if (dirty()) {
      return;
    }
    stored.current = JSON.stringify(settings);
    setDraft(settings);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings]);

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
  // Written when a person has *finished* with a field, not 600 ms after they
  // last touched one. The timer version wrote once a word and refetched once a
  // word, which over a local shell is free and over `vcw serve` is a settings
  // panel fighting the hand that is typing into it.
  //
  // `flush` is what "finished" means, and it is bumped in exactly three
  // places on the pane below: a blur, an Enter, and a change to a control
  // that is one click rather than a sentence. The effect depends on it alone
  // so that a render caused by anything else - and the window renders at meter
  // rate - cannot write.
  const [flush, setFlush] = useState(0);
  useEffect(() => {
    const held = latest.current;
    if (held === null || !dirty()) {
      return;
    }
    stored.current = JSON.stringify(held);
    commit.current(held);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flush]);

  // A panel can be left with a field still focused: `Ctrl+1` takes the whole
  // tree away and `focusout` does not fire for an element that was removed, so
  // without this the last thing typed before switching panels is the one thing
  // never written.
  useEffect(
    () => () => {
      if (dirty()) {
        commit.current(latest.current!);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  if (draft === null) {
    return (
      <section className="panel settings empty">
        <p>{text("settings.reading")}</p>
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
  const transfer = (patch: Partial<Values["transfer"]>) =>
    setDraft({ ...draft, transfer: { ...draft.transfer, ...patch } });

  // Which of the two encoder knobs the chosen default format has, if either.
  const effort = effortOf(draft.export.format);

  // The Discogs token, reported where it is asked for rather than only in the
  // Credentials group: "Discogs needs a token" and "your token did not arrive"
  // are the same question, and they were two sections apart.
  const discogsToken = credentials.find(
    (credential) => credential.variable === "VCW_DISCOGS_TOKEN",
  );

  const unsaved = dirty();

  const blank = (value: string) => (value.trim() === "" ? null : value.trim());
  const number = (value: string) =>
    value.trim() === "" ? null : Number(value);

  return (
    <section className="panel settings">
      <header className="panel-head">
        <h2>{text("settings.title")}</h2>
        {/* What replaced the Save button. Derived rather than remembered,
            because `store.run` swallows a refusal to put it in the status bar
            and resolves either way, so a flag set in `.then` would read
            "Saved" over a write that was refused. This cannot: the word goes
            when the stored settings catch up with the draft, and if the write
            never lands it stays, beside the reason. */}
        {unsaved && <span className="dim">{text("settings.unsaved")}</span>}
      </header>

      <div className="prefs">
        <nav className="prefs-nav" aria-label={text("settings.nav")}>
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
              {text(NAMES[name])}
            </button>
          ))}
        </nav>

        {/* Where "finished with that field" is decided, once, for every
            control in the pane. A delegated listener rather than forty
            handlers: `onBlur` and `onKeyDown` bubble, React's synthetic
            `onChange` bubbles too, and the alternative is the same three
            lines repeated at every input in the file with one of them
            eventually missing.

            A select, a checkbox or a radio writes on the spot, because one
            discrete choice making one save is not chatter. Anything a person
            *types* waits for the blur or the Enter. */}
        <div
          className="prefs-pane"
          onBlur={() => setFlush((n) => n + 1)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              setFlush((n) => n + 1);
            }
          }}
          onChange={(event) => {
            if (!typed(event.target)) {
              setFlush((n) => n + 1);
            }
          }}
        >
        {section === "Appearance" && (
          <fieldset>
            <legend>{text("settings.section.appearance")}</legend>
            <label>
              {text("settings.language")}
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
            <Hint text={text("settings.language.hint")} />
            <label>
              {text("settings.scale")}
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
            <Hint text={text("settings.scale.hint")} />
            <label>
              {text("settings.meters")}
              <select
                value={meters}
                onChange={(event) => {
                  const chosen = event.target.value as MeterStyle;
                  applyMeterStyle(chosen);
                  setMeters(chosen);
                }}
              >
                <option value="bars">{text("settings.meters.bars")}</option>
                <option value="vu">{text("settings.meters.vu")}</option>
              </select>
            </label>
            <Hint text={text("settings.meters.hint")} />
            <label>
              {text("settings.buttons")}
              <select
                value={buttons}
                onChange={(event) => {
                  const chosen = event.target.value as ButtonStyle;
                  applyButtonStyle(chosen);
                  setButtons(chosen);
                }}
              >
                <option value="text">{text("settings.buttons.words")}</option>
                <option value="icons">{text("settings.buttons.icons")}</option>
              </select>
            </label>
            <Hint text={text("settings.buttons.hint")} />
          </fieldset>
        )}

        {section === "Library" && (
        <fieldset>
          <legend>{text("settings.section.library")}</legend>
          <label>
            {text("settings.library.path")}
            <input
              // The one field a fresh install has to be given. See "Where a
              // first run lands" above: this is the end of that journey.
              autoFocus={draft.recording.library === null}
              value={draft.recording.library ?? ""}
              placeholder={text("settings.library.unset")}
              onChange={(event) =>
                recording({ library: blank(event.target.value) })
              }
            />
          </label>
          <label>
            {text("settings.commit")}
            <input
              type="number"
              min="1"
              value={draft.recording.checkpointSeconds ?? ""}
              placeholder={text("settings.unset.builtin")}
              onChange={(event) =>
                recording({ checkpointSeconds: number(event.target.value) })
              }
            />
          </label>
          <Hint text={text("settings.commit.hint")} />
          <label>
            {text("settings.recovery")}
            <select
              value={draft.recording.recovery ?? "ask"}
              onChange={(event) => recording({ recovery: event.target.value })}
            >
              <option value="ask">{text("settings.recovery.ask")}</option>
              <option value="recover">
                {text("settings.recovery.recover")}
              </option>
              <option value="leave">{text("settings.recovery.leave")}</option>
            </select>
          </label>
        </fieldset>
        )}

        {section === "Audio" && (
        <fieldset>
          <legend>{text("settings.section.audio")}</legend>
          <label>
            {text("settings.host")}
            {/* The hosts this build enumerated, not the five that exist. A text
                input here accepted "pulse" on a machine with no such host and
                said nothing until something tried to open a device. */}
            <select
              value={draft.audio.backend ?? ""}
              onChange={(event) => audio({ backend: blank(event.target.value) })}
            >
              <option value="">{text("settings.host.any")}</option>
              {hosts(devices).map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
              {draft.audio.backend !== null &&
                !hosts(devices).includes(draft.audio.backend) && (
                  <option value={draft.audio.backend}>
                    {text("settings.host.absent", { name: draft.audio.backend })}
                  </option>
                )}
            </select>
          </label>
          <label>
            {text("settings.rate")}
            <input
              type="number"
              value={draft.audio.rate ?? ""}
              placeholder={text("settings.unset.device")}
              onChange={(event) => audio({ rate: number(event.target.value) })}
            />
          </label>
          <label>
            {text("settings.format")}
            <select
              value={draft.audio.format ?? ""}
              onChange={(event) => audio({ format: blank(event.target.value) })}
            >
              <option value="">{text("settings.format.any")}</option>
              {FORMATS.map((row) => (
                <option key={row.value} value={row.value}>
                  {text(row.label)}
                </option>
              ))}
            </select>
          </label>
          <label>
            {text("settings.mode")}
            <select
              value={draft.audio.mode ?? "shared"}
              onChange={(event) => audio({ mode: event.target.value })}
            >
              <option value="shared">{text("settings.mode.shared")}</option>
              <option value="native">{text("settings.mode.native")}</option>
              <option value="exclusive">
                {text("settings.mode.exclusive")}
              </option>
            </select>
          </label>
          <label>
            {text("settings.eq")}
            <select
              value={draft.audio.eq ?? "unknown"}
              onChange={(event) => audio({ eq: event.target.value })}
            >
              <option value="unknown">{text("settings.eq.unknown")}</option>
              <option value="riaa">{text("settings.eq.riaa")}</option>
              <option value="flat">{text("settings.eq.flat")}</option>
            </select>
          </label>
          <label>
            {text("settings.ring")}
            <input
              type="number"
              value={draft.audio.ringMillis ?? ""}
              placeholder={text("settings.unset.builtin")}
              onChange={(event) =>
                audio({ ringMillis: number(event.target.value) })
              }
            />
          </label>
        </fieldset>
        )}

        {section === "Detection" && (
        <fieldset>
          <legend>{text("settings.section.detection")}</legend>
          <label>
            {text("settings.detectors")}
            <select
              value={draft.detection.algorithm}
              onChange={(event) => detection({ algorithm: event.target.value })}
            >
              <option value="all">{text("settings.detectors.all")}</option>
              <option value="silence">
                {text("settings.detectors.silence")}
              </option>
              <option value="spectral">
                {text("settings.detectors.spectral")}
              </option>
              <option value="hmm">{text("settings.detectors.hmm")}</option>
            </select>
          </label>
          <label>
            {text("settings.sources")}
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
            {text("settings.confidence")}
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
            {text("settings.silence")}
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
            {text("settings.shortest")}
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
          <Hint text={text("settings.detection.hint")} />
        </fieldset>
        )}

        {section === "Metadata" && (
        <fieldset className="stacked">
          <legend>{text("settings.section.metadata")}</legend>

          {/* The master switch, and it stays a switch of its own. The two below
              it choose which *catalogs a search consults*; this one chooses the
              transport, and it is the only thing in the product that stops an
              AcoustID lookup or a cover-art download - neither of which goes
              through MusicBrainz or Discogs. Derived from "either provider is
              on", a both-off configuration would still reach the network. */}
          <div className="setting-switch">
            <span>{text("settings.online")}</span>
            <Switch
              checked={draft.metadata.online}
              onChange={(online) => metadata({ online })}
              off={text("settings.online.off")}
              on={text("settings.online.on")}
              title={text("settings.online.title")}
            />
          </div>
          <Hint text={text("settings.online.hint")} />

          <div className="setting-switch">
            <span>MusicBrainz</span>
            <Switch
              checked={draft.metadata.musicbrainz}
              onChange={(musicbrainz) => metadata({ musicbrainz })}
              off={text("settings.provider.skip")}
              on={text("settings.provider.search")}
              title={text("settings.musicbrainz.title")}
            />
          </div>
          <label>
            {text("settings.contact")}
            <input
              value={draft.metadata.contact ?? ""}
              placeholder={text("settings.contact.unset")}
              onChange={(event) => metadata({ contact: blank(event.target.value) })}
            />
          </label>
          <Hint text={text("settings.contact.hint")} />

          <div className="setting-switch">
            <span>Discogs</span>
            <Switch
              checked={draft.metadata.discogs}
              onChange={(discogs) => metadata({ discogs })}
              off={text("settings.provider.skip")}
              on={text("settings.provider.search")}
              title={text("settings.discogs.title")}
            />
          </div>
          <p className="hint">
            <code>VCW_DISCOGS_TOKEN</code>{" "}
            {discogsToken?.present === true ? (
              <span className="ok">
                {text("settings.token.set", { n: discogsToken.characters })}
              </span>
            ) : (
              <span className="dim">{text("settings.token.unset")}</span>
            )}
          </p>
          <Hint text={text("settings.discogs.hint")} />
          <p className="hint">
            <button
              type="button"
              className="as-link globe-link"
              onClick={() => {
                void store.run(() => api.support("discogs-token"));
              }}
            >
              <span className="icon globe-mark" aria-hidden="true" />
              {text("settings.discogs.link")}
            </button>
          </p>

          <label>
            {text("settings.genre")}
            <input
              value={draft.metadata.genreMap ?? ""}
              placeholder={text("settings.unset.builtin")}
              onChange={(event) =>
                metadata({ genreMap: blank(event.target.value) })
              }
            />
          </label>
          <Hint text={text("settings.genre.hint")} />
        </fieldset>
        )}

        {section === "Export" && (
        <fieldset>
          <legend>{text("settings.export.legend")}</legend>
          <label>
            {text("settings.format")}
            <select
              value={draft.export.format}
              onChange={(event) => exporting({ format: event.target.value })}
            >
              <option value="flac">FLAC</option>
              <option value="wav">WAV</option>
              <option value="aiff">AIFF</option>
              <option value="alac">ALAC</option>
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

            WAV, AIFF and ALAC have neither and get no field. Both values stay in
            the draft whichever is on screen - the backend ignores the one that
            does not apply - so switching the default format and back does not
            lose a choice, which was the thing the old always-visible field was
            for.
          */}
          {effort !== null && (
            <label>
              {text(effort.caption)}
              <select
                value={draft.export[effort.field]}
                onChange={(event) =>
                  exporting({ [effort.field]: event.target.value })
                }
              >
                {effort.options.map(([value, label]) => (
                  <option key={value} value={value}>
                    {text(label)}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label>
            {text("settings.output")}
            <input
              value={draft.export.output ?? ""}
              onChange={(event) => exporting({ output: blank(event.target.value) })}
            />
          </label>
          <label>
            {text("settings.template")}
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
            <Hint
              className="hint warn near"
              text={text("settings.template.unknown", {
                tokens: unknownTokens(draft.export.template)
                  .map((token) => `{${token}}`)
                  .join(", "),
              })}
            />
          )}
          <Hint className="hint near" text={text("settings.template.hint")} />
          <ul className="tokens">
            {TOKENS.map((token) => (
              <li key={token}>
                <code>{`{${token}}`}</code>
              </li>
            ))}
          </ul>
          <label>
            {text("settings.artwork")}
            <select
              value={draft.export.artwork}
              onChange={(event) => exporting({ artwork: event.target.value })}
            >
              <option value="none">{text("settings.artwork.none")}</option>
              <option value="embed">{text("settings.artwork.embed")}</option>
              <option value="folder">{text("settings.artwork.folder")}</option>
              <option value="both">{text("settings.artwork.both")}</option>
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
          <h3>{text("settings.numbering")}</h3>
          <Hint className="hint near" text={text("settings.numbering.hint")} />
          <label>
            {text("settings.numbering.scheme")}
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
              <option value="alpha">{text("settings.numbering.alpha")}</option>
              <option value="numeric">
                {text("settings.numbering.numeric")}
              </option>
            </select>
          </label>
          <label>
            {text("settings.counted")}
            <select
              value={draft.export.numbering === "sequence" ? "sequence" : "numeric"}
              disabled={draft.export.numbering === "alpha"}
              onChange={(event) => exporting({ numbering: event.target.value })}
            >
              <option value="numeric">{text("settings.counted.side")}</option>
              <option value="sequence">{text("settings.counted.disc")}</option>
            </select>
          </label>
          <Hint className="hint near" text={text("settings.counted.hint")} />
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
          <h3>{text("settings.narrowing")}</h3>
          <Hint className="hint near" text={text("settings.narrowing.hint")} />
          <label>
            {text("settings.narrow.to")}
            <select
              value={draft.export.narrowing}
              onChange={(event) => exporting({ narrowing: event.target.value })}
            >
              <option value="refuse">{text("settings.narrow.refuse")}</option>
              <option value="24">{text("settings.narrow.24")}</option>
              <option value="32">{text("settings.narrow.32")}</option>
            </select>
          </label>
          <label>
            {text("settings.dither")}
            <select
              value={draft.export.dither}
              disabled={draft.export.narrowing === "refuse"}
              onChange={(event) => exporting({ dither: event.target.value })}
            >
              <option value="tpdf">{text("settings.dither.tpdf")}</option>
              <option value="none">{text("settings.dither.none")}</option>
            </select>
          </label>
          <label>
            {text("settings.headroom")}
            <input
              value={draft.export.headroom}
              disabled={draft.export.narrowing === "refuse"}
              onChange={(event) => exporting({ headroom: event.target.value })}
            />
          </label>
          <Hint className="hint near" text={text("settings.narrow.width.hint")} />
          <Hint className="hint near" text={text("settings.dither.hint")} />
        </fieldset>
        )}

        {section === "Transfer" && (
        <fieldset className="stacked">
          <legend>{text("settings.section.transfer")}</legend>
          <Hint text={text("settings.transfer.hint")} />

          <label>
            {text("settings.transfer.turntable")}
            <input
              value={draft.transfer.turntable}
              onChange={(event) => transfer({ turntable: event.target.value })}
            />
          </label>
          <label>
            {text("settings.transfer.tonearm")}
            <input
              value={draft.transfer.tonearm}
              onChange={(event) => transfer({ tonearm: event.target.value })}
            />
          </label>
          <label>
            {text("settings.transfer.cartridge")}
            <input
              value={draft.transfer.cartridge}
              onChange={(event) => transfer({ cartridge: event.target.value })}
            />
          </label>
          <label>
            {text("settings.transfer.stylus")}
            <input
              value={draft.transfer.stylus}
              onChange={(event) => transfer({ stylus: event.target.value })}
            />
          </label>
          <Hint text={text("settings.transfer.stylus.hint")} />
          <label>
            {text("settings.transfer.phono")}
            <input
              value={draft.transfer.phonoStage}
              onChange={(event) => transfer({ phonoStage: event.target.value })}
            />
          </label>
          <label>
            {text("settings.transfer.interface")}
            <input
              value={draft.transfer.interface}
              onChange={(event) => transfer({ interface: event.target.value })}
            />
          </label>
          <label>
            {text("settings.transfer.operator")}
            <input
              value={draft.transfer.operator}
              onChange={(event) => transfer({ operator: event.target.value })}
            />
          </label>
          <Hint text={text("settings.transfer.operator.hint")} />
          <label>
            {text("settings.transfer.modification")}
            <input
              value={draft.transfer.modification}
              onChange={(event) =>
                transfer({ modification: event.target.value })
              }
            />
          </label>
          <Hint text={text("settings.transfer.modification.hint")} />
        </fieldset>
        )}

        {section === "Credentials" && (
        <fieldset>
          <legend>{text("settings.section.credentials")}</legend>
          <Hint text={text("settings.credentials.hint")} />
          <Hint text={text("settings.credentials.environment")} />
          <table className="rows">
            <thead>
              <tr>
                <th>{text("settings.column.credential")}</th>
                <th>{text("settings.column.set")}</th>
                <th className="n">{text("settings.column.characters")}</th>
                <th>{text("settings.column.variable")}</th>
              </tr>
            </thead>
            <tbody>
              {credentials.map((credential) => (
                <tr key={credential.variable}>
                  <td>{credential.name}</td>
                  <td>
                    {credential.present ? (
                      <span className="ok">{text("settings.yes")}</span>
                    ) : (
                      <span className="dim">{text("settings.no")}</span>
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
 * Whether a control is one a person types a sentence into, rather than one
 * they click once.
 *
 * The question the settings pane asks of every change: a select, a checkbox
 * or a radio is a decision already made, and is written on the spot; a text
 * box, a number box or a textarea is a half-finished word until the person
 * leaves it. Exported for its test, because the whole of the auto-save
 * timing now turns on this one answer.
 *
 * Unknown input types read as "typed" on purpose. A control this does not
 * recognize saving a beat late is a delay; one saving mid-word is the bug
 * this exists to fix.
 */
export function typed(target: EventTarget | null): boolean {
  if (target instanceof HTMLTextAreaElement) {
    return true;
  }
  if (!(target instanceof HTMLInputElement)) {
    return false;
  }
  return !["checkbox", "radio", "range", "color", "file"].includes(target.type);
}

/**
 * A paragraph of catalog prose, with its literals and its emphasis kept.
 *
 * This panel is the only one that marks anything up inside a sentence: a file
 * name, an environment variable, the name of an option being referred to.
 * Cutting each paragraph into a key per fragment would hand a translator
 * three half sentences and no way to reorder them, and dropping the markup
 * would make `i18n/en-US.toml` read as prose in the middle of a paragraph
 * about file names. So the catalog keeps one entry per paragraph and marks a
 * literal with backticks and an emphasis with asterisks, which is what a
 * translator editing a TOML file is already looking at.
 */
function Hint({
  text,
  className = "hint",
}: {
  text: string;
  className?: string;
}): React.JSX.Element {
  return (
    <p className={className}>
      {marked(text).map(([kind, value], index) =>
        kind === "code" ? (
          <code key={index}>{value}</code>
        ) : kind === "em" ? (
          <em key={index}>{value}</em>
        ) : (
          value
        ),
      )}
    </p>
  );
}

/** A marked paragraph, split into its runs. Exported for its test. */
export function marked(
  source: string,
): readonly (readonly ["text" | "code" | "em", string])[] {
  return source
    .split(/(`[^`]+`|\*[^*]+\*)/)
    .filter((part) => part !== "")
    .map((part) =>
      part.startsWith("`")
        ? (["code", part.slice(1, -1)] as const)
        : part.startsWith("*")
          ? (["em", part.slice(1, -1)] as const)
          : (["text", part] as const),
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
