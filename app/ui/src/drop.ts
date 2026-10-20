/*
 *  drop.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Releasing a paused recorder when the stylus lands.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// §50's sequence is set the level, cue the record, drop the needle, start
// recording - and the last two are in the wrong order for one person working
// alone. This inverts them: the transport is armed, started and held paused
// before anybody walks to the deck, and the needle itself releases it.
//
// It watches the meter stream rather than the audio. The window already
// receives `meter-update` at §17's refresh rate from the moment the transport
// is armed, so the detector is two comparisons on an event the UI is drawing
// anyway - no second reader of the device, no worker, and nothing in the
// capture path. The cost is the boundary's latency, which means the first
// tenth of a second of stylus noise is dropped rather than committed. That is
// lead-in groove, seconds before the first track, and losing it is closer to
// what an archivist wants than keeping it.
//
// The two numbers came out of the Boards of Canada rip the author pointed at,
// which carries four drops - one per side. Measured at 5.3 ms resolution from
// the summaries Audacity had already computed:
//
//   the electrical floor before the stylus lands   -55 to -65 dBFS
//   the landing transient, one 5.3 ms window       -4.5 to -7.9 dBFS
//   the groove noise it settles into               -18 to -28 dBFS
//
// Fifty decibels of daylight, so the threshold is absolute and there is
// nothing to learn or calibrate. The sustain is what tells a drop from the
// clicks the same side is full of: a click is about a millisecond and the
// level is back in the floor on the next tick, while after a drop it never
// returns there.

import { useEffect, useRef, useState } from "react";

import * as api from "./api";
import type { Meter } from "./bindings/vcw";
import type { Store } from "./store";

/** Above this, something is in the groove. Measured; see the note above. */
export const DROP_DB = -35;

/** How long it has to stay there to be a stylus rather than a click. */
export const SUSTAIN_MS = 50;

/** The loudest channel in a snapshot, in dBFS. */
export function peakOf(meter: Meter): number {
  return meter.channels.reduce(
    (loudest, channel) => Math.max(loudest, channel.peakDb),
    Number.NEGATIVE_INFINITY,
  );
}

/**
 * When the level first rose above the threshold, carried from tick to tick.
 *
 * `null` while it is below, which is also how a click is forgotten: one tick
 * over and the next one under leaves nothing behind.
 */
export function rose(
  peakDb: number,
  since: number | null,
  now: number,
): number | null {
  return peakDb < DROP_DB ? null : (since ?? now);
}

/** Whether a rise that began at `since` has lasted long enough to be a drop. */
export function dropped(since: number | null, now: number): boolean {
  return since !== null && now - since >= SUSTAIN_MS;
}

/** The needle-drop trigger: whether it is listening, and the switch for it. */
export function useNeedleDrop(
  store: Store,
  arm: () => Promise<void>,
): { waiting: boolean; toggle: () => void } {
  const [waiting, setWaiting] = useState(false);
  const since = useRef<number | null>(null);
  const { engine, run } = store;
  const { meter, phase } = engine;

  // Keyed on the snapshot, not on the store: every tick has to be seen, and
  // an effect that watched the whole store would also run on the position
  // events arriving between them.
  useEffect(() => {
    if (!waiting) {
      since.current = null;
      return;
    }
    // The transport left on its own - stopped by hand, or ended by a writer
    // that died. Nothing to release, so stop listening rather than wait for a
    // level that will never be metered.
    if (phase === "idle" || phase === "stopped") {
      setWaiting(false);
      return;
    }
    // `armed` and `recording` are the moments between the click and the
    // queued `pause` landing, and there is nothing to decide in them.
    if (phase !== "paused" || meter === null) {
      return;
    }
    const now = performance.now();
    since.current = rose(peakOf(meter), since.current, now);
    if (dropped(since.current, now)) {
      since.current = null;
      setWaiting(false);
      void run(() => api.transport("resume"));
    }
  }, [waiting, phase, meter]);

  const toggle = () => {
    if (waiting) {
      setWaiting(false);
      return;
    }
    setWaiting(true);
    void (async () => {
      if (phase === "idle") {
        await arm();
      }
      if (phase !== "paused") {
        await run(() => api.transport("record"));
        await run(() => api.transport("pause"));
      }
    })();
  };

  return { waiting, toggle };
}
