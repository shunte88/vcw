/*
 *  Diagnostics.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The event log, and the counters a capture reported (§38, §42).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// §42 asks for diagnostics a person can read, and this is the literal answer:
// every event that crossed the boundary, newest first, with the time it
// arrived and the one line `describe` makes of it. It is the panel a bug report
// is copied out of.
//
// Newest first because the interesting event is the last one. The log is bounded
// in the store at `LOG_LIMIT`, which is the one accumulation this side of the
// boundary is allowed and is argued for where it happens rather than here.

import { useState } from "react";

import type { Line } from "../store";
import { describe } from "../describe";

/** How many lines are rendered at once. */
const PAGE = 120;

/** The diagnostics overlay. */
export function Diagnostics({
  log,
  onClose,
}: {
  log: readonly Line[];
  onClose: () => void;
}): React.JSX.Element {
  const [filter, setFilter] = useState("");
  const matching =
    filter.trim() === ""
      ? log
      : log.filter((line) => line.event.kind.includes(filter.trim()));
  // Sliced rather than virtualised. 500 lines is the whole log and 120 is more
  // than a screen, so the scrollback a person actually reads is one page and
  // the rest is there when they scroll. A windowing library for a bounded list
  // this short would be machinery for nothing.
  const shown = [...matching].reverse().slice(0, PAGE);

  return (
    <div className="overlay" role="dialog" aria-label="Diagnostics">
      <div className="overlay-box wide">
        <header className="panel-head">
          <h2>Event log</h2>
          <input
            autoFocus
            value={filter}
            placeholder="filter by event kind"
            onChange={(event) => setFilter(event.target.value)}
          />
          <span className="dim">
            {matching.length} of {log.length}
          </span>
          <button type="button" onClick={onClose}>
            Close (Esc)
          </button>
        </header>
        {shown.length === 0 ? (
          <p className="empty">
            Nothing yet. Meter and position ticks are not kept - see UNLOGGED
            in the store.
          </p>
        ) : (
          <table className="rows log">
            <tbody>
              {shown.map((line, index) => (
                <tr key={`${line.at}-${index}`}>
                  <td className="dim n">
                    {new Date(line.at).toLocaleTimeString()}
                  </td>
                  <td className="kind">{line.event.kind}</td>
                  <td>{describe(line.event)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
