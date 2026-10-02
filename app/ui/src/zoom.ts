/*
 *  zoom.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Which frames the waveform is showing: zooming, panning and staying in range.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Setting a track's start and end by eye means zooming in until the gap
// between two sides of a boundary is wide enough to click in, scrubbing across
// it to hear where it falls, and nudging. Zoomed out to a whole side of vinyl
// the picture is about a second per column, so every one of those steps needs a
// window narrower than the capture - which makes the visible range something a
// person drives rather than something the shell decides once.
//
// The arithmetic lives here rather than in the component for the usual reason:
// a `<canvas>` has no 2D context in jsdom, so nothing inside the draw effect
// can be asserted, and the interesting part of zooming is not the drawing. It
// is the four ways a range can go wrong - narrower than a sample, wider than
// the capture, off the left end, off the right end - and every one of those is
// a value before it is a picture.
//
// # Why `endFrame` stays nullable
//
// `null` means "to the end of the capture", which is not the same fact as the
// frame count the capture happens to have right now: a recording grows, and a
// window pinned to a number would stop at wherever the capture was when the
// zoom happened. So a range that reaches the end is stored as `null` and the
// shell resolves it against the capture it is reading. That is one
// representation of "the whole thing", not two.

/** The visible window of a capture, in frames. */
export type Span = {
  /** First frame shown. */
  readonly startFrame: number;
  /** Last frame shown, or null for the end of the capture. */
  readonly endFrame: number | null;
};

/**
 * The narrowest window, in frames.
 *
 * Four thousand and ninety-six, and the number comes from the panel rather
 * than from the audio. The waveform asks the shell for one column per device
 * pixel and this window is over three thousand of them wide, so 4096 frames is
 * already about one frame to the column - zooming closer would ask for columns
 * that cannot differ from their neighbours. At 96 kHz it is 43 milliseconds
 * across the full width, which is four times finer than the centisecond a
 * boundary is placed to.
 */
export const CLOSEST = 4096;

/** How much of the window a zoom step keeps. Halve in, double out. */
export const STEP = 2;

/**
 * The window as the shell would read it: inside the capture, both ends.
 *
 * This is the lossy read, and deliberately so - it answers "which frames are
 * on screen", and a window that starts past the end of a capture has none. It
 * is therefore not what the movement functions below start from; see `asked`.
 */
export function resolve(
  span: Span,
  frames: number,
): { start: number; end: number } {
  const total = Math.max(0, frames);
  const start = Math.min(Math.max(0, span.startFrame), total);
  const end = span.endFrame === null ? total : Math.min(span.endFrame, total);
  return { start, end: Math.max(start, end) };
}

/**
 * The window as it was asked for, with its width intact.
 *
 * The difference from [`resolve`] is the whole reason both exist. A panel
 * zoomed into the end of a long capture, shown a shorter one - which is a
 * project re-read after a capture was deleted - has a window entirely past the
 * end. `resolve` says "no frames", correctly. But the person did not ask for
 * no frames, they asked for ten million of them, and `clamp` has to slide that
 * width back inside rather than collapse it to the floor. The first draft used
 * `resolve` here and did exactly that.
 */
function asked(span: Span, frames: number): { start: number; width: number } {
  const start = Math.max(0, span.startFrame);
  const end = span.endFrame === null ? frames : Math.max(start, span.endFrame);
  return { start, width: end - start };
}

/** The whole capture. */
export function fit(): Span {
  return { startFrame: 0, endFrame: null };
}

/**
 * A span from two frames, with a range that reaches the end stored as `null`.
 *
 * Every function here ends in this one, which is why none of them has to
 * remember the nullable end: there is exactly one place that decides it.
 */
function span(start: number, end: number, frames: number): Span {
  const startFrame = Math.round(Math.max(0, Math.min(start, frames)));
  const endFrame = Math.round(Math.max(startFrame, Math.min(end, frames)));
  return {
    startFrame,
    endFrame: endFrame >= frames ? null : endFrame,
  };
}

/** A width brought inside what the capture and the panel can show. */
function width(wanted: number, frames: number): number {
  return Math.max(Math.min(CLOSEST, frames), Math.min(wanted, frames));
}

/**
 * A span brought back inside a capture that may have changed length.
 *
 * Every function below starts here, so none of them has to cope with a window
 * that is already impossible: one normalising step, in one place, and the
 * arithmetic after it can assume a window that fits.
 */
export function clamp(current: Span, frames: number): Span {
  if (frames <= 0) {
    return fit();
  }
  const { start, width: was } = asked(current, frames);
  const wide = width(was, frames);
  const from = Math.min(start, frames - wide);
  return span(from, from + wide, frames);
}

/**
 * Zoom by `factor`, holding still the frame at `anchor` across the window.
 *
 * `anchor` is a fraction of the *visible* window, not of the capture, because
 * it comes from a pointer over a canvas: wheeling over a peak should keep that
 * peak under the pointer, which is the only zoom that does not feel like the
 * picture running away.
 *
 * `factor` under one zooms in. Zooming in past [`CLOSEST`] or out past the
 * capture is not refused, it is clamped - a wheel does not know it has reached
 * the end and a refusal would be a dead scroll rather than a limit.
 */
export function zoom(
  current: Span,
  frames: number,
  factor: number,
  anchor: number,
): Span {
  if (frames <= 0 || !Number.isFinite(factor) || factor <= 0) {
    return clamp(current, frames);
  }
  const { start, end } = resolve(clamp(current, frames), frames);
  const was = end - start;
  const wide = width(was * factor, frames);
  const held = Math.min(Math.max(anchor, 0), 1);
  const at = start + held * was;
  const from = Math.min(Math.max(0, at - held * wide), frames - wide);
  return span(from, from + wide, frames);
}

/** Slide the window by `byFrames`, keeping its width. */
export function pan(current: Span, frames: number, byFrames: number): Span {
  if (frames <= 0 || !Number.isFinite(byFrames)) {
    return clamp(current, frames);
  }
  const { start, end } = resolve(clamp(current, frames), frames);
  const wide = end - start;
  const from = Math.min(Math.max(0, start + byFrames), frames - wide);
  return span(from, from + wide, frames);
}

/** Keep the current width and put `atFrame` in the middle. */
export function centre(current: Span, frames: number, atFrame: number): Span {
  if (frames <= 0 || !Number.isFinite(atFrame)) {
    return clamp(current, frames);
  }
  const { start, end } = resolve(clamp(current, frames), frames);
  return pan(current, frames, atFrame - (start + (end - start) / 2));
}

/**
 * The window a range of frames wants, with a margin either side.
 *
 * What "zoom to the selected track" means: the track's own frames plus a tenth
 * of its length at each end, because a boundary you cannot see the outside of
 * is a boundary you cannot judge.
 */
export function toRange(
  fromFrame: number,
  toFrame: number,
  frames: number,
): Span {
  if (frames <= 0) {
    return fit();
  }
  const low = Math.min(fromFrame, toFrame);
  const high = Math.max(fromFrame, toFrame);
  const margin = Math.max(CLOSEST / 8, (high - low) / 10);
  const wide = width(high - low + margin * 2, frames);
  const from = Math.min(Math.max(0, low - margin), frames - wide);
  return span(from, from + wide, frames);
}

/**
 * Page the window along so a moving playhead stays in it.
 *
 * Paged, not centred: a window that recentres on every tick is a picture that
 * never sits still, and a person reading a waveform is reading the shape of it.
 * So nothing happens while the playhead is inside the window, and when it
 * leaves the right edge the window jumps so that it sits a tenth of the way in
 * - which leaves nine tenths of a window before the next jump.
 *
 * Returns the span it was given, by identity, when nothing needs to move. The
 * caller compares by reference to decide whether to re-render, so this is part
 * of the contract and not an optimisation.
 */
export function follow(current: Span, frames: number, atFrame: number): Span {
  if (frames <= 0 || !Number.isFinite(atFrame)) {
    return current;
  }
  const { start, end } = resolve(current, frames);
  if (end - start >= frames) {
    return current;
  }
  if (atFrame >= start && atFrame <= end) {
    return current;
  }
  const wide = end - start;
  const from = Math.min(Math.max(0, atFrame - wide / 10), frames - wide);
  if (Math.round(from) === start) {
    return current;
  }
  return span(from, from + wide, frames);
}
