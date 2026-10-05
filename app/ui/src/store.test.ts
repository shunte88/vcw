/*
 *  store.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What the window does with a playback refusal (§21, §35).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The reducer is where an event becomes something a person can see, and
// `playback-refused` is the one event whose whole point is what does *not*
// follow it. A refused open publishes nothing else: no `auditioning` came
// before it and no `playback-finished` is coming, so if the fold does not put
// the transport back, the window says "Pause" over a device that never opened
// and there is no second event to correct it.
//
// That is why this reads the reducer directly rather than rendering the panel.
// The assertion is about what is true *after* the last event ever sent, and a
// rendered test would prove the button's label on the way there without proving
// that nothing arrives to fix it.

import { describe as group, expect, it } from "vitest";

import type { Wire } from "./bindings/vcw";
import { describe } from "./describe";
import { NOTHING, fold, logged } from "./store";

/** An audition that opened, so there is a playing state to be put back. */
const OPENED: Wire = {
  kind: "auditioning",
  captureId: 4,
  scope: "the whole capture (1200.000 s)",
  opened: "96000 Hz s32 x2",
  conversion: "straight through",
  divergences: [],
};

/** The refusal, shaped as the shell publishes it. */
const REFUSED: Wire = {
  kind: "playback-refused",
  captureId: 4,
  scope: "the whole capture",
  reason: "the device will not play 96000 Hz",
};

group("a playback refusal", () => {
  // There is deliberately no test that folds a refusal into the initial state
  // and asserts the transport is not playing. It was written, and removing the
  // reducer's `playing: false` did not make it fail: nothing is playing in
  // `NOTHING` either, so it asserted the starting state rather than the fold.
  // The sequence below is the one that can fail.
  it("puts the transport back, because nothing else will", () => {
    // The sequence a person can actually produce: play a side, then ask for a
    // rate the device refuses. The `playing` left by the first audition is what
    // would leave the transport claiming to play a stream that never opened.
    const playing = fold(NOTHING, OPENED);
    expect(playing.playing).toBe(true);

    const state = fold(playing, REFUSED);
    expect(state.playing).toBe(false);
    expect(state.auditioning).toBeNull();
    expect(state.playhead).toBe(0);
  });

  it("says why, in a sentence with the reason in it", () => {
    const state = fold(NOTHING, REFUSED);
    expect(state.refusal?.code).toBe("playback-refused");
    expect(state.refusal?.message).toContain("the device will not play 96000 Hz");
    // The capture is named too. A refusal that does not say what it refused is
    // no use in a library of sides.
    expect(state.refusal?.message).toContain("capture 4");
  });

  it("reads as one line in the log", () => {
    expect(describe(REFUSED)).toContain("the device will not play 96000 Hz");
  });
});

/** An output snapshot, shaped as the feeder publishes it. */
const OUT: Wire = {
  kind: "output-meter-update",
  meter: {
    channels: [
      { peakDb: -3.2, rmsDb: -14.1, holdDb: -3.2, clipped: false, clippedSamples: 0 },
      { peakDb: -3.4, rmsDb: -14.3, holdDb: -3.4, clipped: false, clippedSamples: 0 },
    ],
    frames: 4800,
    clipped: false,
  },
};

/** The end of an audition that ran to the end of its span. */
const FINISHED: Wire = {
  kind: "playback-finished",
  captureId: 4,
  frames: 1_000_000,
  underruns: 0,
  fidelity: "bit-perfect",
  bitPerfect: true,
};

group("the output meter", () => {
  // The needle has to go out when the sound does. The bridge grays a side
  // whose meter is null, so a snapshot left behind here is a lit output meter
  // resting at -3 dB over a device that stopped playing, which is the one
  // reading an instrument must never give.
  it("goes out when playback ends", () => {
    const playing = fold(fold(NOTHING, OPENED), OUT);
    expect(playing.output?.channels).toHaveLength(2);

    const state = fold(playing, FINISHED);
    expect(state.output).toBeNull();
    // The input meter is not touched: its last reading is the level the record
    // was captured at, and that is worth leaving on screen.
    expect(state.meter).toBe(playing.meter);
  });

  // A stream, not an event. 50 Hz of these would fill the log's 500 lines in
  // ten seconds of audition, which is what UNLOGGED exists to stop.
  it("is not kept in the log", () => {
    expect(logged(OUT)).toBe(false);
  });
});
