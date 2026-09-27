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
import type { Capture, Side } from "../bindings/vcw";
import { useKeys } from "../keys";
import type { Store } from "../store";
import { clock } from "../format";

/** The transport strip. */
export function Transport({
  store,
  capture,
  side,
}: {
  store: Store;
  capture: Capture | undefined;
  side: Side | null;
}): React.JSX.Element {
  const { engine, run } = store;
  const recording = engine.phase === "recording";
  const armed = engine.phase !== "idle";

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
      } else if (capture !== undefined) {
        void run(() => api.play(capture.id, { scope: "whole" }));
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
        {engine.playing && (
          <span className="clock playhead" title="The audition playhead">
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
          Record
        </button>
        <button
          type="button"
          disabled={!armed}
          onClick={() =>
            void run(() => api.transport(recording ? "pause" : "resume"))
          }
          title="Pause or resume (p)"
        >
          {recording ? "Pause" : "Resume"}
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
          Stop
        </button>
        <span className="spacer" />
        <button
          type="button"
          disabled={capture === undefined && !engine.playing}
          onClick={() => {
            if (engine.playing) {
              void run(() => api.playback({ verb: "pause" }));
            } else if (capture !== undefined) {
              void run(() => api.play(capture.id, { scope: "whole" }));
            }
          }}
          title="Play or pause the audition (space)"
        >
          {engine.playing ? "Pause" : "Play"}
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
          Marker
        </button>
      </div>
    </div>
  );
}
