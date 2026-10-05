/*
 *  Vu.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A VU meter: one dial, one needle, one peak lamp (§17).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The dial is an SVG and the needle is a `transform` on one line in it. That
// is the whole of it, and it is deliberate: a canvas would be a draw call per
// channel per frame on the one thread S3 found to be the expensive one, and a
// rotation is a property the compositor animates without a layout pass.
//
// The scale is `position` from `Meters` - the same -60 dBFS to 0 the bars use.
// A dial with a scale of its own would mean the needle and the bar disagreeing
// about the same number, which is the one thing a second meter must not do.
//
// What is on the dial, and why it is not the number:
//
// - The needle follows RMS, through `vu.ts`'s movement. RMS is what a VU reads
//   and the movement is what makes it readable: loudness, not samples.
// - The hold mark is the peak hold, which already falls on the Rust side at
//   §14's rate. A second set of ballistics on top of it would be two decays
//   fighting.
// - The needle keeps moving between events. The meter arrives at 50 Hz and the
//   screen draws at 60 or 120, so without this the needle would step rather
//   than sweep, and a stepping needle has no ballistics at all - it is a bar
//   graph drawn as an arc.

import { useEffect, useRef } from "react";

import type { Levels } from "../bindings/vcw";
import { RESTING, type Swing, swing } from "../vu";
import { position } from "./Meters";

/** Half the needle's travel, in degrees either side of vertical. */
const SWEEP = 52;

/** Where the needle is pinned, in the view box - below it, out of sight. */
const PIVOT = { x: 60, y: 64 } as const;

/** How far out the arc and its ticks are drawn. */
const ARC = 46;

/**
 * The dBFS values that get a mark on the dial.
 *
 * Crowded at the top and sparse at the bottom, which is where the interesting
 * part of the scale is: the difference between -40 and -50 does not change
 * what anyone does, and the difference between -6 and 0 does.
 */
const MARKS: readonly { db: number; label?: string }[] = [
  { db: -60 },
  { db: -50 },
  { db: -40, label: "-40" },
  { db: -30 },
  { db: -20, label: "-20" },
  { db: -12 },
  { db: -6 },
  { db: 0, label: "0" },
];

/** Where the red zone starts: §14's headroom, in dBFS. */
const HOT = -6;

/** A point on the dial for a deflection of `at` per cent, `r` from the pivot. */
function on(at: number, r: number): { x: number; y: number } {
  const radians = ((at / 100) * 2 * SWEEP - SWEEP) * (Math.PI / 180);
  return {
    x: PIVOT.x + r * Math.sin(radians),
    y: PIVOT.y - r * Math.cos(radians),
  };
}

/** The arc from one deflection to another, as a path. */
function arc(from: number, to: number, r: number): string {
  const a = on(from, r);
  const b = on(to, r);
  return `M ${a.x.toFixed(2)} ${a.y.toFixed(2)} A ${r} ${r} 0 0 1 ${b.x.toFixed(2)} ${b.y.toFixed(2)}`;
}

/** Where the needle points, in degrees, for a deflection of `at` per cent. */
function tilt(at: number): string {
  return `rotate(${((at / 100) * 2 * SWEEP - SWEEP).toFixed(2)} ${PIVOT.x} ${PIVOT.y})`;
}

/**
 * The needle, swinging.
 *
 * Everything that changes per frame is in a ref: the target, the integrator's
 * state, and one attribute on one line. Nothing here sets React state. The
 * meter arrives at 50 Hz, and routing a needle's position through the
 * reconciler at that rate would put a render in the path of every frame for a
 * number the compositor can animate on its own.
 *
 * The loop parks itself when the needle stops, so an idle window is not waking
 * the machine sixty times a second to redraw a needle that is not moving. The
 * effect below has no dependency array on purpose: it runs after every render,
 * which is exactly when the target can have changed, and it starts the loop
 * again if it had parked. The integrator survives in a ref, so a needle that
 * is restarted mid-swing keeps its velocity rather than jumping.
 */
function Needle({ target }: { target: number }) {
  const hand = useRef<SVGLineElement>(null);
  const want = useRef(target);
  const state = useRef<Swing>(RESTING);
  const frame = useRef(0);
  const last = useRef(0);
  want.current = target;

  useEffect(() => {
    if (frame.current !== 0) {
      return;
    }
    const tick = (now: number) => {
      state.current = swing(state.current, want.current, (now - last.current) / 1000);
      last.current = now;
      hand.current?.setAttribute("transform", tilt(state.current.at));
      const still =
        Math.abs(state.current.at - want.current) < 0.02 &&
        Math.abs(state.current.speed) < 0.02;
      frame.current = still ? 0 : requestAnimationFrame(tick);
    };
    last.current = performance.now();
    frame.current = requestAnimationFrame(tick);
  });

  useEffect(
    () => () => {
      cancelAnimationFrame(frame.current);
      frame.current = 0;
    },
    [],
  );

  return (
    <line
      ref={hand}
      className="vu-needle"
      x1={PIVOT.x}
      y1={PIVOT.y}
      x2={PIVOT.x}
      y2={PIVOT.y - 44}
      transform={tilt(0)}
    />
  );
}

/** One channel's dial. */
export function Vu({
  levels,
  name,
}: {
  levels: Levels;
  name: string;
}): React.JSX.Element {
  const hold = position(levels.holdDb);
  const root = on(hold, ARC);
  const tip = on(hold, ARC + 3);
  return (
    <figure className="vu">
      {/*
        Cropped to the band the dial actually uses, which is the arc at y 18,
        the scale text at y 45, and nothing else: the pivot at y 64 and the
        lower two thirds of the needle fall outside it. That is both how a
        real compact meter is built - the needle appears from behind the bezel
        - and the whole of the height saving, because the empty wedge below
        the arc is most of a full circle's bounding box. The remit was that
        two dials fit where two bars fit, and the bars are two 10 px tracks.

        The channel name is inside the SVG for the same reason. A `figcaption`
        is a line box, and a line box is 1 rem the dial does not have; in the
        top-left corner it costs nothing, because the arc sweeps up from the
        bottom left and has already left that corner empty.
      */}
      <svg viewBox="0 13 120 36" className="vu-face" aria-hidden="true">
        <path className="vu-arc" d={arc(0, position(HOT), ARC)} />
        <path className="vu-arc hot" d={arc(position(HOT), 100, ARC)} />
        {MARKS.map(({ db, label }) => {
          const at = position(db);
          const outer = on(at, ARC);
          const inner = on(at, ARC - (label === undefined ? 5 : 8));
          const text = on(at, ARC - 15);
          return (
            <g key={db} className={db >= HOT ? "vu-mark hot" : "vu-mark"}>
              <line x1={outer.x} y1={outer.y} x2={inner.x} y2={inner.y} />
              {label !== undefined && (
                <text x={text.x} y={text.y} className="vu-label">
                  {label}
                </text>
              )}
            </g>
          );
        })}
        {/* The hold, as a mark riding the outside of the arc. It was a lamp
            sitting on the face, which at this height put it on top of the
            tick marks; outside the arc it is in the one ring of the dial
            nothing else uses. */}
        <line
          className={levels.holdDb >= HOT ? "vu-hold hot" : "vu-hold"}
          x1={root.x}
          y1={root.y}
          x2={tip.x}
          y2={tip.y}
        />
        {levels.clipped && <circle className="vu-clip" cx={113} cy={18} r={3.5} />}
        <text className="vu-name" x={3} y={21}>
          {name}
        </text>
        <Needle target={position(levels.rmsDb)} />
      </svg>
    </figure>
  );
}
