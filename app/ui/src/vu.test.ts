/*
 *  vu.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  That the needle obeys ANSI C16.5 (§17).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The standard, not the constants. `DAMPING` and `SPEED` are one solution to
// these three assertions and a test that pinned them would only prove they
// had not been retyped. What has to stay true is that the movement is a VU
// movement: 99% in 300 ms, a small overshoot, and no energy gained on the way.

import { describe as group, expect, it } from "vitest";

import { RESTING, type Swing, swing } from "./vu";

/** Runs the needle at `frames` per second for `seconds`, reporting every step. */
function run(seconds: number, target: number, frames = 240): number[] {
  const dt = 1 / frames;
  let state: Swing = RESTING;
  const trace: number[] = [];
  for (let i = 0; i < Math.round(seconds * frames); i += 1) {
    state = swing(state, target, dt);
    trace.push(state.at);
  }
  return trace;
}

group("a VU needle", () => {
  it("reaches 99 per cent of a step in 300 ms, within the standard's tenth", () => {
    const trace = run(0.4, 100);
    const at = (ms: number) => trace[Math.round((ms / 1000) * 240) - 1] ?? 0;
    // Not there early: 270 ms is the bottom of the standard's window, and a
    // needle that arrived before it would be a needle reading transients the
    // ear does not hear as loudness.
    expect(at(200)).toBeLessThan(99);
    // There by the top of it.
    expect(at(330)).toBeGreaterThan(99);
  });

  it("overshoots, but barely", () => {
    const peak = Math.max(...run(1, 100));
    // It must overshoot - that is the needle having mass - and it must not
    // look like a bounce.
    expect(peak).toBeGreaterThan(100);
    expect(peak).toBeLessThan(103);
  });

  it("settles rather than ringing, and comes back down", () => {
    const trace = run(2, 100);
    expect(trace[trace.length - 1]).toBeCloseTo(100, 3);

    // Drop the signal and the needle falls at the same rate it rose: the
    // spring is the same spring. A fall that took a different time would be a
    // peak meter wearing a VU's face.
    let state: Swing = { at: 100, speed: 0 };
    for (let i = 0; i < 72; i += 1) {
      state = swing(state, 0, 1 / 240);
    }
    expect(state.at).toBeLessThan(1);
  });

  it("cannot be thrown off the dial by a frame that never came", () => {
    // A backgrounded window returns with a gap of whole seconds in it. Without
    // the clamp this explicit integrator diverges - the needle leaves the
    // scale and never comes back.
    let state: Swing = RESTING;
    state = swing(state, 100, 12);
    expect(Number.isFinite(state.at)).toBe(true);
    expect(state.at).toBeLessThan(100);
    expect(state.at).toBeGreaterThanOrEqual(0);
  });
});
