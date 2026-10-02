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
 * Seconds as `m:ss.cc`, or `h:mm:ss.cc` past the hour.
 *
 * Centiseconds because that is the resolution a person can act on with a
 * nudge key, and because a millisecond column that changes 60 times a second
 * is unreadable at a glance. The frame count is the exact number and the
 * panels show it where exactness matters.
 */
export function clock(seconds: number): string {
  if (!Number.isFinite(seconds)) {
    return "-:--";
  }
  const negative = seconds < 0;
  const total = Math.abs(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  const pad = rest < 10 ? "0" : "";
  const body =
    hours > 0
      ? `${hours}:${String(minutes).padStart(2, "0")}:${pad}${rest.toFixed(2)}`
      : `${minutes}:${pad}${rest.toFixed(2)}`;
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
 * `summarise` uses it for a file whose metadata would not read, and 1970 in a
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
