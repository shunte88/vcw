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
//
// The rows are one line each and clipped, because a table whose rows are as tall
// as their longest sentence is not scannable - and a refusal carrying generated
// advice is several lines long. Double-click opens one row in full, wrapped,
// which is the thing a person is copying into a bug report.
//
// The filter does not take focus when the overlay opens. It did, and that made
// the status bar's standing offer of "? for the keyboard map" a lie for as long
// as this panel was up: a field with focus is correct to take a literal `?`, so
// the only fix is to not be in one before a person has asked to be.

import { Fragment, useState } from "react";

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
  /** The row opened in full, by its key, or null when none is. */
  const [opened, setOpened] = useState<string | null>(null);
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
          <>
            <p className="hint">Double-click a line to read it in full.</p>
            <table className="rows log">
              <tbody>
                {shown.map((line, index) => {
                  const key = `${line.at}-${index}`;
                  const said = describe(line.event);
                  return (
                    <Fragment key={key}>
                      <tr
                        className={opened === key ? "current" : ""}
                        title="Double-click to read in full"
                        onDoubleClick={() => {
                          setOpened(opened === key ? null : key);
                        }}
                      >
                        <td className="dim n">
                          {new Date(line.at).toLocaleTimeString()}
                        </td>
                        <td className="kind">{line.event.kind}</td>
                        <td>{said}</td>
                      </tr>
                      {opened === key && (
                        <tr className="log-full">
                          <td colSpan={3}>
                            <span className="dim">
                              {new Date(line.at).toLocaleString()}
                            </span>{" "}
                            {said}
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          </>
        )}
      </div>
    </div>
  );
}
