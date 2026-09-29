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
import { NOTHING, fold } from "./store";

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
