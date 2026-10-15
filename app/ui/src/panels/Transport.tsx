/*
 *  Transport.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The capture transport and the audition transport, in one strip (§43).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Always mounted, whichever panel is in front, because §43's chords are global
// and a person nudging a boundary in the track editor still wants `Space` to
// audition it. That is also why this component owns those chords rather than
// `App`: the handler and the button that runs it should be the same line of
// code, and a transport action dispatched from two places is how a keyboard and
// a mouse end up doing subtly different things.
//
// One rule decides what every button here does: `record`, `pause` and `stop`
// are *capture* verbs and go to the engine; `Space`, the seeks and the skips are
// *audition* verbs and go to the player. `Space` is the only chord that has to
// choose, and it chooses by what is playing - which is a fact the store copied
// from an event, not a guess.
//
// Which capture and which side are *given* to this component, not worked out in
// it. There is no "current side" in the project - a side is a row and a capture
// is a row and §21 lets two faces share one capture - so somebody has to decide
// what `m` means, and doing it here as well as in the waveform would be two
// answers to one question. `App` decides once; this file reads it, and when the
// answer is `null` the marker key does nothing rather than picking a side.

import * as api from "../api";
import type { Audition, Capture, Side } from "../bindings/vcw";
import { Face } from "../Face";
import { key, useText } from "../i18n";
import { inTheShell } from "../host";
import type { Region } from "./Waveform";
import { useKeys } from "../keys";
import type { Store } from "../store";
import { clock } from "../format";

/**
 * The catalog key for what each phase is called in front of a person.
 *
 * Keyed by the wire word, and falling back to it: the store holds the phase
 * as a string, so a phase this build does not know shows as the shell named
 * it rather than as a dotted key.
 */
const PHASES: Record<string, string> = {
  idle: key("transport.phase.idle"),
  armed: key("transport.phase.armed"),
  recording: key("transport.phase.recording"),
  paused: key("transport.phase.paused"),
  stopped: key("transport.phase.stopped"),
};

/** The transport strip. */
export function Transport({
  store,
  capture,
  side,
  selection,
}: {
  store: Store;
  capture: Capture | undefined;
  side: Side | null;
  selection: Region | null;
}): React.JSX.Element {
  const text = useText();
  const { engine, run } = store;
  const recording = engine.phase === "recording";
  const armed = engine.phase !== "idle";

  /**
   * Put a boundary at the playhead.
   *
   * One function and not two call sites, and the reload is why. The key and
   * the button each issued the command and neither re-read the project, so a
   * marker was written to the file and then not drawn, not listed, and not
   * deletable: pressing `m` looked like pressing nothing. The shell publishes
   * no event for an edit - every other verb in the window reloads for itself
   * - and these two were the pair that forgot.
   *
   * `edge: "start"` from both, which is honest rather than arbitrary: a lone
   * boundary bounds nothing until something adopts it, and the window has no
   * question to ask about an edge that describes no track yet. Making a track
   * out of a stretch is `Add track from selection` in the Tracks panel, and
   * that places both edges correctly without anyone choosing.
   */
  const mark = () => {
    if (side === null) {
      return;
    }
    void run(() =>
      api.placeMarker({ sideId: side.id, at: engine.playhead, edge: "start" }),
    ).then(store.reload);
  };

  /** Start the audition, on whatever [`scopeOf`] says Play means now. */
  const audition = () => {
    if (capture !== undefined) {
      void run(() => api.play(capture.id, scopeOf(selection)));
    }
  };

  // §43 does not say how far a seek key moves, so the panel does: one second,
  // which is a little less than a groove revolution and small enough that
  // holding the key is a sensible way to travel. The shell clamps it.
  const back = Math.max(0, engine.playhead - 1);
  const forward = engine.playhead + 1;

  useKeys("global", {
    play: () => {
      if (engine.playing) {
        void run(() => api.playback({ verb: "pause" }));
      } else if (engine.auditioning !== null) {
        void run(() => api.playback({ verb: "play" }));
      } else {
        audition();
      }
    },
    record: () => void run(() => api.transport("record")),
    pause: () =>
      void run(() => api.transport(recording ? "pause" : "resume")),
    stop: () =>
      void run(() =>
        engine.playing ? api.playback({ verb: "stop" }) : api.transport("stop"),
      ),
    checkpoint: () => void run(() => api.transport("poll")),
    seekBack: () => void run(() => api.playback({ verb: "seek", to: back })),
    seekForward: () =>
      void run(() => api.playback({ verb: "seek", to: forward })),
    skipBack: () => void run(() => api.playback({ verb: "skip-back" })),
    skipForward: () => void run(() => api.playback({ verb: "skip-forward" })),
    marker: mark,
  });

  return (
    <div className="transport">
      <div className="transport-clock">
        <span className="clock" title={text("transport.committed")}>
          {clock(engine.seconds)}
        </span>
        <span className="phase">{text(PHASES[engine.phase] ?? engine.phase)}</span>
        {/* Not only while playing. The playhead is where Split and Place
            marker will land, and a click on the waveform now puts it there
            without playing anything - so the number those two buttons are
            about has to be readable when nothing is running, which is
            exactly when a person is lining one of them up. */}
        {(engine.playing || engine.playhead > 0) && (
          <span
            className="clock playhead"
            title={text("transport.playhead")}
          >
            {clock(engine.playhead)}
          </span>
        )}
        {/* §51: the curve in force is visible wherever audio is auditioned.
            Only while something is playing, because that is the only time
            there is a "in force" to speak of - the stored decision itself
            lives in the project panel. */}
        {engine.playbackEq !== null && (
          <span className="curve" title={text("transport.eq.title")}>
            {text("transport.eq", { curve: engine.playbackEq })}
          </span>
        )}
      </div>

      <div className="transport-buttons">
        <button
          type="button"
          disabled={!armed}
          onClick={() => void run(() => api.transport("record"))}
          title={text("transport.record.title", { chord: "r" })}
        >
          <Face icon="record">{text("transport.record")}</Face>
        </button>
        <button
          type="button"
          disabled={!armed}
          onClick={() =>
            void run(() => api.transport(recording ? "pause" : "resume"))
          }
          title={text("transport.pause.title", { chord: "p" })}
        >
          {/* Resume wears the play glyph, because the transport's second
              button is the same control in two states and a paused recorder
              is resumed by the thing that means "go". */}
          <Face icon={recording ? "pause" : "play"}>
            {recording ? text("transport.pause") : text("transport.resume")}
          </Face>
        </button>
        <button
          type="button"
          disabled={!armed && !engine.playing}
          onClick={() =>
            void run(() =>
              engine.playing
                ? api.playback({ verb: "stop" })
                : api.transport("stop"),
            )
          }
          title={text("transport.stop.title", { chord: "s" })}
        >
          <Face icon="stop">{text("transport.stop")}</Face>
        </button>
        <span className="spacer" />
        <button
          type="button"
          disabled={capture === undefined && !engine.playing}
          onClick={() => {
            if (engine.playing) {
              void run(() => api.playback({ verb: "pause" }));
            } else {
              audition();
            }
          }}
          title={
            selection === null
              ? text("transport.play.title", { chord: "space" })
              : text("transport.play.selection.title", { chord: "space" })
          }
        >
          <Face icon={engine.playing ? "pause" : "play"}>
            {engine.playing ? text("transport.pause") : text("transport.play")}
          </Face>
        </button>
        <button
          type="button"
          disabled={side === null}
          onClick={mark}
          title={text("transport.marker.title", { chord: "m" })}
        >
          <Face icon="marker">{text("transport.marker")}</Face>
        </button>
        {/* §52: "Audio auditioned through `serve` is rendered by the host, not
            by the browser, because the host is the machine the audio hardware
            is attached to. The interface shall say so rather than appear to
            have failed."

            On the bar rather than in a tooltip, and always rather than only
            while playing, because the thing it explains is silence: a person
            whose laptop stays quiet does not hover the button that just did
            what they asked, they decide Play is broken. The meters are the
            same machine's, which is why this says audio and not playback. */}
        {!inTheShell() && (
          <span className="elsewhere" title={text("transport.remote.title")}>
            {text("transport.remote")}
          </span>
        )}
      </div>
    </div>
  );
}

/**
 * What Play plays: the band on the waveform if there is one, and the whole
 * capture if there is not.
 *
 * Its own function, exported, for two reasons. The key and the button must not
 * disagree, which is this file's rule from the top; and this is the sentence
 * the gesture rework turned on, so it is the sentence worth a test. §21's
 * region scope already stops at the end of what it is given, which is why
 * there is nothing here about stopping.
 */
export function scopeOf(selection: Region | null): Audition {
  return selection === null
    ? { scope: "whole" }
    : { scope: "region", start: selection.from, end: selection.to };
}
