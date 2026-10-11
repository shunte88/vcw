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

import { useSyncExternalStore } from "react";

import type { Levels, Meter } from "../bindings/vcw";
import { db } from "../format";
import { count, useText } from "../i18n";
import { type MeterStyle, meterStyleOf, onMeterStyle } from "../scale";
import { Vu } from "./Vu";

/**
 * Where a level sits on the bar, as a percentage.
 *
 * Exported because the compact strip under the waveform draws the same scale
 * at a tenth of the size, and two scales for one number would mean a bar that
 * reads half full beside a bar that reads two thirds.
 *
 * §14's scale, taken literally: -60 dBFS at the left, 0 at the right, linear in
 * decibels. Linear in decibels rather than in amplitude because that is what a
 * person setting a level is judging - an amplitude scale spends four fifths of
 * its width on the top 6 dB and is useless for the quiet end of a lead-in.
 */
export function position(dbfs: number): number {
  if (!Number.isFinite(dbfs)) {
    return 0;
  }
  return Math.max(0, Math.min(100, ((dbfs + 60) / 60) * 100));
}

/** One channel's row. */
function Bar({ levels, name }: { levels: Levels; name: string }) {
  const text = useText();
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
      <span className="meter-db" title={text("meters.peak")}>
        {db(levels.peakDb)}
      </span>
      <span className="meter-db dim" title={text("meters.rms")}>
        {db(levels.rmsDb)}
      </span>
      {levels.clipped && (
        <span
          className="clip"
          title={count(levels.clippedSamples, "meters.clipped.samples")}
        >
          {text("meters.clip")}
        </span>
      )}
    </div>
  );
}

/** A silent channel: the floor, no hold, no clip. */
const SILENT: Levels = {
  peakDb: Number.NEGATIVE_INFINITY,
  rmsDb: Number.NEGATIVE_INFINITY,
  holdDb: Number.NEGATIVE_INFINITY,
  clipped: false,
  clippedSamples: 0,
};

/**
 * One side of the bridge: a named group of channel rows.
 *
 * Drawn whether or not it has a meter behind it. A meter that is not running
 * is a disabled control - grayed, at the floor, with its channels still there
 * - rather than a sentence where the meter should be. The sentence was worse
 * than useless: it told a person who could see the transport was idle
 * something they already knew, and it did it by making the instrument vanish,
 * so the window's whole bottom edge changed height the moment a device was
 * armed.
 *
 * Two channels when there is nothing to count, because a turntable has two
 * and a grayed stereo pair is the right guess at what is about to appear.
 */
function Bridge({
  name,
  title,
  meter,
  live,
  style,
}: {
  /** What the group is called, already in the reader's language. */
  name: string;
  /** What hovering it says, which differs by whether it is running. */
  title: (running: boolean) => string;
  meter: Meter | null;
  live: boolean;
  style: MeterStyle;
}) {
  const text = useText();
  const running = live && meter !== null;
  const channels = running && meter !== null ? meter.channels : [SILENT, SILENT];
  const names =
    channels.length === 2
      ? style === "vu"
        ? [text("meters.left"), text("meters.right")]
        : [text("meters.left.short"), text("meters.right.short")]
      : channels.map((_, i) => `${i + 1}`);
  return (
    <div
      className={running ? "bridge" : "bridge off"}
      aria-disabled={!running}
      title={title(running)}
    >
      <span className="bridge-name">{name}</span>
      <div className={style === "vu" ? "bridge-dials" : "bridge-bars"}>
        {channels.map((levels, index) =>
          style === "vu" ? (
            <Vu key={index} levels={levels} name={names[index] ?? `${index + 1}`} />
          ) : (
            <Bar key={index} levels={levels} name={names[index] ?? `${index + 1}`} />
          ),
        )}
      </div>
      {running && meter !== null && (
        <div className="meter-foot">
          <span>{count(meter.frames, "meters.window")}</span>
          {meter.clipped && (
            <span className="clip">{text("meters.clipped")}</span>
          )}
        </div>
      )}
    </div>
  );
}

/** The meter bridge: what is coming in, and what is going out. */
export function Meters({
  meter,
  output,
  capturing,
  playing,
}: {
  meter: Meter | null;
  output: Meter | null;
  capturing: boolean;
  playing: boolean;
}): React.JSX.Element {
  // Subscribed rather than passed down from the root. The choice belongs to
  // the window and is read in two places that are nowhere near each other in
  // the tree - here and the settings panel - and threading a prop through
  // both would be more wiring than the thing being wired.
  const style = useSyncExternalStore(onMeterStyle, meterStyleOf);
  const text = useText();
  return (
    <section className={style === "vu" ? "meters dials" : "meters"}>
      <Bridge
        name={text("meters.input")}
        title={(running) =>
          running ? text("meters.input.live") : text("meters.input.idle")
        }
        meter={meter}
        live={capturing}
        style={style}
      />
      <Bridge
        name={text("meters.output")}
        title={(running) =>
          running ? text("meters.output.live") : text("meters.output.idle")
        }
        meter={output}
        live={playing}
        style={style}
      />
    </section>
  );
}
