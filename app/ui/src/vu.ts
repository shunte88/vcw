/*
 *  vu.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A VU needle's ballistics: a mass on a spring, in a damper (§17).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// A real VU meter is an instrument, not a readout. The needle has mass, the
// spring pulls it back, and the oil damps it, and the reason an engineer can
// read one across a room is that those three things turn a signal into a
// motion with a shape - a loud transient throws the needle and it comes back,
// and the eye reads the throw as well as the level. A bar graph that simply
// shows the last number has none of that, which is why this exists beside it
// rather than instead of it.
//
// The standard the movement is copied from is ANSI C16.5: a step to full
// deflection reaches 99% in 300 ms, and overshoots by 1 to 1.5%. That is a
// second-order system - position, velocity, and a restoring force - and this
// module is that system and nothing else, so it can be stepped in a test
// against a clock rather than watched.
//
//   x'' = w^2 (target - x) - 2 z w x'
//
// `z` below 1 is underdamped, which is what gives the overshoot; `w` sets how
// fast. The pair here were solved for the standard, and the test asserts the
// standard rather than the constants, so a change to either has to still
// produce a VU meter.

/**
 * Damping ratio. Below 1, so the needle overshoots and settles back - the
 * 1 to 1.5% of ANSI C16.5, and the thing that makes a needle look alive.
 */
export const DAMPING = 0.8;

/**
 * Natural frequency, in radians per second.
 *
 * Solved from the settling time: a second-order system is within 1% of its
 * target after about `4.6 / (z * w)` seconds, and the standard says 300 ms.
 */
export const SPEED = 4.6 / (DAMPING * 0.3);

/**
 * The longest step the integrator will take, in seconds.
 *
 * A backgrounded window hands `requestAnimationFrame` a gap of whole seconds
 * when it comes back. Integrating that in one step is how an explicit solver
 * throws the needle off the dial; clamping it means the needle catches up over
 * the next few frames instead, which is both stable and what a real one would
 * do after the lights came back on.
 */
export const LONGEST = 0.05;

/** Where the needle is and how fast it is going. */
export type Swing = {
  /** Deflection, in the same units as the target. */
  readonly at: number;
  /** Rate of change, per second. */
  readonly speed: number;
};

/** A needle at rest at the bottom of the scale. */
export const RESTING: Swing = { at: 0, speed: 0 };

/**
 * Advances the needle by `dt` seconds towards `target`.
 *
 * Semi-implicit Euler: the velocity is updated first and the position with the
 * new velocity. It costs the same as the explicit form and does not pump
 * energy into an oscillator, which the explicit form does - a needle that
 * gained a little amplitude on every bounce would eventually be a needle
 * rattling against the end stops on a steady tone.
 */
export function swing(state: Swing, target: number, dt: number): Swing {
  const step = Math.min(Math.max(dt, 0), LONGEST);
  const force = SPEED * SPEED * (target - state.at) - 2 * DAMPING * SPEED * state.speed;
  const speed = state.speed + force * step;
  return { at: state.at + speed * step, speed };
}
