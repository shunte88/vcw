/*
 *  SelectionBar.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What is selected, how long it is, and how hot the input is.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The strip under the waveform, and only under the waveform: a selection is a
// fact about a capture, so on the Library and Settings panels there is nothing
// for it to say and a row of zeros would be noise.
//
// Read-only. Audacity's equivalent is three editable spin fields, and they are
// genuinely useful, but typing a selection in is a verb and this is a readout;
// the day one of these becomes an input it will need the whole set - commit on
// blur, clamp to the capture, refuse a backwards range - and none of that
// belongs in a status line added for a number a person wanted to be able to
// read off the screen.
//
// The levels here are the same numbers as the meter bridge above, at a tenth
// of the size, because the bridge is where a level gets *set* and this is where
// it gets glanced at while doing something else. Peak only, no RMS, no hold, no
// dB text: anything more and it stops being a glance.

import type { Meter } from "../bindings/vcw";
import { clock } from "../format";
import { position } from "./Meters";
import type { Region } from "./Waveform";

/** One boxed time, in the one format VCW has. */
function Time({ seconds, label }: { seconds: number; label: string }) {
  return (
    <span className="time-box" title={label}>
      {clock(seconds)}
    </span>
  );
}

/** The strip. */
export function SelectionBar({
  selection,
  meter,
}: {
  selection: Region | null;
  meter: Meter | null;
}): React.JSX.Element {
  // Zeros and not blanks when nothing is selected. The fields keep their width
  // either way, so the strip does not change shape as a drag starts, and a
  // person learning the window can see what the three numbers are going to be.
  const from = selection?.from ?? 0;
  const to = selection?.to ?? 0;

  return (
    <div className="selbar">
      <span className="selbar-label">Selection</span>
      <Time seconds={from} label="Where the selection starts" />
      <Time seconds={to} label="Where the selection ends" />
      <span className="selbar-label">Duration</span>
      <Time seconds={to - from} label="How long the selection is" />

      <span className="selbar-label levels">Levels</span>
      <div className="peaks">
        {meter === null ? (
          <div className="peak-track" title="No input armed" />
        ) : (
          meter.channels.map((levels, index) => (
            <div
              key={index}
              className={levels.clipped ? "peak-track clipped" : "peak-track"}
              title={`Channel ${index + 1} peak`}
            >
              <div
                className="peak-fill"
                style={{ width: `${position(levels.peakDb)}%` }}
              />
            </div>
          ))
        )}
      </div>
    </div>
  );
}
