/*
 *  format.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Numbers as a person reads them: a clock, a size, a level.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Formatting, not policy. Every function here takes a number Rust already
// decided and returns the same number with separators in it - nothing rounds a
// value that a later calculation depends on, because there is no later
// calculation on this side of the boundary.
//
// §2 is the reason this file is three functions and not thirty. The temptation
// in a UI layer is to grow a `helpers.ts` that quietly starts deciding things:
// a `trackLength(track)` that subtracts two fields, a `isUsable(device)` that
// reads `problems`. Both belong in a view model, where `vcw --json` can print
// the same answer.

/**
 * Seconds as `hh:mm:ss.fff`, always, whatever the magnitude.
 *
 * One format and only one, which is the whole point: the hour field is there
 * at four seconds and the millisecond field is there at four hours, so the
 * digits never move under the eye and a reading can be compared to the one
 * above it in a column. It is also why the widget needs no format picker -
 * there is nothing to pick between.
 *
 * Milliseconds rather than the centiseconds this used to show. A live readout
 * does change faster than it can be read at that column, but the column a
 * person *acts* on is the one they are about to type into or nudge to, and
 * rounding it away in the display meant the display and the frame count
 * disagreed about where the playhead was.
 */
export function clock(seconds: number): string {
  if (!Number.isFinite(seconds)) {
    return "--:--:--.---";
  }
  const negative = seconds < 0;
  // Rounded to the millisecond *before* the fields are split out, so that
  // 59.9999 s reads 00:01:00.000 rather than 00:00:60.000.
  const millis = Math.round(Math.abs(seconds) * 1000);
  const hours = Math.floor(millis / 3_600_000);
  const minutes = Math.floor(millis / 60_000) % 60;
  const secs = Math.floor(millis / 1000) % 60;
  const two = (value: number): string => String(value).padStart(2, "0");
  const body = `${two(hours)}:${two(minutes)}:${two(secs)}.${String(millis % 1000).padStart(3, "0")}`;
  return negative ? `-${body}` : body;
}

/** A byte count in binary units, which is what a filesystem reports. */
export function bytes(count: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = count;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const places = unit === 0 ? 0 : value < 10 ? 2 : 1;
  return `${value.toFixed(places)} ${units[unit] ?? "B"}`;
}

/**
 * A unix timestamp as a date a person can scan down a column.
 *
 * Date only, not a time: the column exists so somebody can find the rip they
 * made last Tuesday, and a minute field makes the column wide for precision
 * nobody is looking for. Zero is spelled rather than printed, because
 * `summarize` uses it for a file whose metadata would not read, and 1970 in a
 * library of vinyl rips reads as a bug.
 */
export function when(unix: number): string {
  if (unix <= 0) {
    return "-";
  }
  return new Date(unix * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/**
 * A level in dBFS, with silence spelled rather than printed.
 *
 * The meter sends `-inf` for a channel with nothing in it, and `-Infinity dB`
 * in a column of numbers reads as a fault rather than as quiet.
 */
export function db(value: number): string {
  return Number.isFinite(value) ? `${value.toFixed(1)}` : "-∞";
}
