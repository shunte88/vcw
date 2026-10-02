/*
 *  ruler.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Where the time marks go on a waveform ruler, and what they say.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Choosing tick marks is arithmetic, so it lives outside the component that
// draws them and is tested on its own. The drawing code then has no decisions
// left in it, which is the only way a canvas is testable at all: a `<canvas>`
// in jsdom has no 2D context, so anything worth asserting has to be a value
// before it is a pixel.
//
// The rule is a spacing rule, not a count rule. A ruler that always shows ten
// marks shows them at 2.6-second intervals on one side and 1.4 on another, and
// a person reading a time off it has to do division. So the step is chosen from
// a fixed list of intervals a person already thinks in - a second, five, ten,
// half a minute, a minute, five - and it is the smallest one that still leaves
// room for its label.

/**
 * The intervals a ruler is allowed to use, in seconds.
 *
 * Nothing between 2 and 5, or 60 and 120, on purpose: a 3-second or 90-second
 * grid is arithmetic every time it is read. The list ends at an hour because a
 * side of vinyl does not reach two.
 */
const STEPS: readonly number[] = [
  0.05, 0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600,
];

/**
 * The narrowest a labelled mark may be spaced, in pixels.
 *
 * `0:00.00` is seven characters at 10px monospace, which is a little over 42
 * pixels, and two labels that touch are worse than half as many labels.
 */
const LABEL_PIXELS = 78;

/**
 * How many intervals the gap between two labels is divided into.
 *
 * Five, unconditionally, and there is no "only if there is room" test because
 * there cannot be one: the step above is chosen so that a label has at least
 * `LABEL_PIXELS` to itself, so a fifth of it is at least fifteen pixels, at
 * every zoom. A guard here would be a branch that can never be taken.
 */
const MINORS = 5;

/**
 * A guard, not a policy. `ticks` returns one entry per mark and a caller that
 * hands it a degenerate range should get an empty ruler rather than a loop.
 */
const MOST_TICKS = 4096;

/** One mark on the ruler. */
export type Tick = {
  /** Where it falls, in seconds from the start of the capture. */
  readonly seconds: number;
  /** What it says, or the empty string for an unlabelled mark. */
  readonly label: string;
  /** Whether it is one of the labelled marks. */
  readonly major: boolean;
};

/**
 * A time as a ruler label: `m:ss`, or `h:mm:ss` past the hour.
 *
 * `step` decides the precision rather than the value does, so a ruler is never
 * a column of `0:05.00` where `0:05` would do, and a ruler zoomed in far enough
 * to step in tenths does not print five marks all saying `0:05`.
 *
 * This is deliberately not `clock` from `format.ts`. `clock` always prints
 * centiseconds because the transport and the track list want exactness; a
 * ruler wants the shortest label that is still unambiguous at its own spacing.
 */
export function mark(seconds: number, step: number): string {
  const places = step < 1 ? (step < 0.1 ? 2 : 1) : 0;
  // Rounded before the split when there are no decimals, or a mark at 59.999
  // prints `0:60`. The ticks are exact multiples of an integer step so this
  // should not arise, but a ruler is not the place to find out.
  const total = Math.max(0, places === 0 ? Math.round(seconds) : seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  const body = `${rest < 10 ? "0" : ""}${rest.toFixed(places)}`;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${body}`
    : `${minutes}:${body}`;
}

/**
 * The marks for a ruler spanning `startSeconds` to `endSeconds` over `pixels`.
 *
 * The marks are on absolute time, not on the start of the window: a ruler over
 * a zoomed range from 8:03 to 9:41 puts its labels on 8:10, 8:20 and so on, so
 * that panning does not slide the numbers about. That is the whole reason
 * `Math.ceil` appears below rather than a loop from `startSeconds`.
 *
 * Returns an empty list for a range with no width or a strip too narrow to
 * label, which is what a panel one pixel wide during a layout pass asks for.
 */
export function ticks(
  startSeconds: number,
  endSeconds: number,
  pixels: number,
): Tick[] {
  const span = endSeconds - startSeconds;
  if (!Number.isFinite(span) || span <= 0 || pixels < LABEL_PIXELS) {
    return [];
  }
  const perPixel = span / pixels;
  const last = STEPS[STEPS.length - 1] ?? 3600;
  const step =
    STEPS.find((candidate) => candidate / perPixel >= LABEL_PIXELS) ?? last;

  const grid = step / MINORS;

  const marks: Tick[] = [];
  const first = Math.ceil(startSeconds / grid) * grid;
  for (let n = 0; marks.length < MOST_TICKS; n += 1) {
    // `first + n * grid` rather than a running sum: the error in the sum grows
    // with the number of marks, and a ruler over a long side has hundreds.
    const at = first + n * grid;
    if (at > endSeconds + grid / 1000) {
      break;
    }
    const major = Math.abs(at / step - Math.round(at / step)) < 1e-6;
    marks.push({
      seconds: at,
      label: major ? mark(at, step) : "",
      major,
    });
  }
  return marks;
}
