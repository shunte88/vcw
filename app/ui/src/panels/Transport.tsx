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
import type { Region } from "./Waveform";
import { useKeys } from "../keys";
import type { Store } from "../store";
import { clock } from "../format";

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
  const { engine, run } = store;
  const recording = engine.phase === "recording";
  const armed = engine.phase !== "idle";

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
    marker: () => {
      if (side === null) {
        return;
      }
      void run(() =>
        api.placeMarker({ sideId: side.id, at: engine.playhead, edge: "start" }),
      );
    },
  });

  return (
    <div className="transport">
      <div className="transport-clock">
        <span className="clock" title="Frames committed to the project">
          {clock(engine.seconds)}
        </span>
        <span className="phase">{engine.phase}</span>
        {/* Not only while playing. The playhead is where Split and Place
            marker will land, and a click on the waveform now puts it there
            without playing anything - so the number those two buttons are
            about has to be readable when nothing is running, which is
            exactly when a person is lining one of them up. */}
        {(engine.playing || engine.playhead > 0) && (
          <span
            className="clock playhead"
            title="The playhead: where Split and Place marker will land"
          >
            {clock(engine.playhead)}
          </span>
        )}
      </div>

      <div className="transport-buttons">
        <button
          type="button"
          disabled={!armed}
          onClick={() => void run(() => api.transport("record"))}
          title="Record (r)"
        >
          <Face icon="record">Record</Face>
        </button>
        <button
          type="button"
          disabled={!armed}
          onClick={() =>
            void run(() => api.transport(recording ? "pause" : "resume"))
          }
          title="Pause or resume (p)"
        >
          {/* Resume wears the play glyph, because the transport's second
              button is the same control in two states and a paused recorder
              is resumed by the thing that means "go". */}
          <Face icon={recording ? "pause" : "play"}>
            {recording ? "Pause" : "Resume"}
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
          title="Stop (s)"
        >
          <Face icon="stop">Stop</Face>
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
              ? "Play or pause the audition (space)"
              : "Play the selection (space)"
          }
        >
          <Face icon={engine.playing ? "pause" : "play"}>
            {engine.playing ? "Pause" : "Play"}
          </Face>
        </button>
        <button
          type="button"
          disabled={side === null}
          onClick={() => {
            if (side !== null) {
              void run(() =>
                api.placeMarker({
                  sideId: side.id,
                  at: engine.playhead,
                  edge: "start",
                }),
              );
            }
          }}
          title="Place a marker at the playhead (m)"
        >
          <Face icon="marker">Marker</Face>
        </button>
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
