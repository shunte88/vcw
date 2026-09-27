/*
 *  Meters.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Peak, RMS and hold per channel, with a clip latch (§14).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The only component in the application that redraws at the event rate, which
// makes it the one worth being careful about. S3's finding was that the IPC
// boundary is free and the main thread is not, so the cost here is entirely in
// what React is asked to do per meter tick.
//
// Three decisions follow from that:
//
// - The bar is a `div` with a `width` in percent, not a canvas. A canvas would
//   mean a draw call per channel per tick and a ref per canvas; a width is one
//   style property React can diff, and the compositor scales it without a
//   layout pass.
// - The numbers are `tabular-nums` and fixed width, so a level changing from
//   `-9.4` to `-10.1` does not reflow the row.
// - Nothing here holds state. The peak hold and the clip latch are computed in
//   `vcw-signal` and arrive in the event, because a hold needle implemented in
//   the UI would decay at the browser's frame rate rather than at §14's rate,
//   and `vcw --json` could not print the same number.

import type { Levels, Meter } from "../bindings/vcw";
import { db } from "../format";

/**
 * Where a level sits on the bar, as a percentage.
 *
 * §14's scale, taken literally: -60 dBFS at the left, 0 at the right, linear in
 * decibels. Linear in decibels rather than in amplitude because that is what a
 * person setting a level is judging - an amplitude scale spends four fifths of
 * its width on the top 6 dB and is useless for the quiet end of a lead-in.
 */
function position(dbfs: number): number {
  if (!Number.isFinite(dbfs)) {
    return 0;
  }
  return Math.max(0, Math.min(100, ((dbfs + 60) / 60) * 100));
}

/** One channel's row. */
function Bar({ levels, name }: { levels: Levels; name: string }) {
  return (
    <div className="meter-row">
      <span className="meter-name">{name}</span>
      <div className="meter-track">
        <div
          className="meter-rms"
          style={{ width: `${position(levels.rmsDb)}%` }}
        />
        <div
          className="meter-peak"
          style={{ width: `${position(levels.peakDb)}%` }}
        />
        <div
          className="meter-hold"
          style={{ left: `${position(levels.holdDb)}%` }}
        />
      </div>
      <span className="meter-db" title="Peak since the last snapshot">
        {db(levels.peakDb)}
      </span>
      <span className="meter-db dim" title="RMS over the meter window">
        {db(levels.rmsDb)}
      </span>
      {levels.clipped && (
        <span
          className="clip"
          title={`${levels.clippedSamples} sample(s) at or beyond full scale`}
        >
          CLIP
        </span>
      )}
    </div>
  );
}

/** The meter bridge. */
export function Meters({ meter }: { meter: Meter | null }): React.JSX.Element {
  if (meter === null) {
    return (
      <section className="meters empty">
        <p>No levels yet. Arm a device to see the input.</p>
      </section>
    );
  }
  // Two channels are Left and Right; anything else is numbered, because a
  // four-channel interface has no conventional names and "Left" on channel 3
  // would be a lie.
  const names =
    meter.channels.length === 2 ? ["L", "R"] : meter.channels.map((_, i) => `${i + 1}`);
  return (
    <section className="meters">
      {meter.channels.map((levels, index) => (
        <Bar
          key={index}
          levels={levels}
          name={names[index] ?? `${index + 1}`}
        />
      ))}
      <div className="meter-foot">
        <span>{meter.frames} frame(s) in this window</span>
        {meter.clipped && <span className="clip">clipped</span>}
      </div>
    </section>
  );
}
