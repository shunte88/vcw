/*
 *  Waveform.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The capture drawn across the top half of the project page: a time ruler on
 *  both edges, the boundaries on it (§30), a playhead, and the track labels.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// # Pushed or polled: polled, and this is the argument
//
// WP-09 left the question open and WP-16 has to answer it, because the answer
// decides what this component is.
//
// §35 names a `waveform-update` event, and nothing publishes it. That is the
// decision, not an omission. A pushed waveform means the engine deciding how
// wide the panel is and how many pixels a peak column covers, which are facts
// only the browser has, and it means a peak stream arriving during a capture at
// whatever rate the writer happens to commit at. A polled waveform means one
// `waveform` command per (capture, channel, range, width), answered from the
// stored blocks, cached by the browser for as long as those four are unchanged.
//
// What polling costs is freshness during a live capture, and that cost is paid
// by the meters instead: a person watching a record go by is watching the meter
// bridge, and the waveform is what they look at afterwards to place a boundary.
// So the redraw is driven by the four things that change the picture - the
// capture, the zoom, the panel width and the project generation - and by
// nothing else. `capture-finished` bumps the generation, which is how the last
// block of a capture appears without an event that carries pixels.
//
// # Why a canvas and not an SVG
//
// A zoomed-out side is 1800 columns, and 1800 `<path>` elements is 1800 nodes
// React has to diff. The canvas is drawn in one pass in an effect and the DOM
// sees one element. It is also the reason `pixels` is sent to the shell: the
// peaks come back already reduced to one column per pixel, so nothing here
// walks a sample array.
//
// # Why the rulers are inside the canvas
//
// Two strips of `<div>`s with a tick per mark would be the obvious thing and
// would be wrong twice. It would put hundreds of nodes back into the tree that
// the paragraph above took out, and it would align the ticks to CSS pixels
// while the peak columns are aligned to device pixels - so a mark at 8:00 and
// the column at 8:00 would sit a fraction apart, which on a ruler is the one
// thing that must not happen. One canvas, one coordinate system, and
// `ruler.ts` holds the arithmetic so the only thing here is drawing.
//
// # Why device pixels
//
// The canvas is sized in device pixels and `pixels` asks the shell for that
// many columns, because this window runs at a device pixel ratio above two.
// Drawing 1600 columns into 3400 device pixels is a soft picture and 10px
// ruler text in it is unreadable. The peaks come out of stored summaries, so
// asking for twice as many columns is twice as many index rows and no decode.
//
// # Zooming, scrolling and scrubbing
//
// §20 requires horizontal zoom and pan, and the requirement is about the job
// rather than about the feature. A whole side of vinyl across this canvas is
// about half a second to the column, and the gap before a track's first groove
// is not half a second wide. Setting a start point by eye means zooming in
// until that gap is wider than a pixel, listening across it, and nudging - so
// the visible range is something a person drives continuously, not something
// the shell picks once.
//
// The arithmetic for it is in `zoom.ts`, where a window cannot leave the
// capture or close past a sample and every one of those rules is asserted. What
// is left here is three gestures and the one thing each has to get right:
//
//  - the wheel zooms about the pointer, so the peak under the cursor stays
//    under the cursor. Shift, or a sideways wheel, pans instead;
//  - the bar under the picture is the window's own position *and* width, so how
//    far in a person is zoomed is something they can see rather than deduce;
//  - dragging across the picture selects, and the selection *stays*. This is
//    the gesture a person reaches for first and the one the first draft did
//    not have: the thing you want to say is "that bit". What "that bit" then
//    means is play it - the selection is handed up to `App`, the transport
//    plays it as a region and the engine stops at the end of it, which is the
//    whole of auditioning a track before there is a track. Zooming to it is
//    the `z` key, where every other zoom already lives;
//
//  - dragging either ruler scrolls, and the picture follows the hand the way
//    a map does. A ruler is the one strip with no audio drawn on it, so it is
//    the one place a drag can mean "move the paper" without taking a gesture
//    away from the waveform;
//
//    The first draft made a release zoom to the band instead, and the band
//    went away with it. That is one gesture doing two jobs - choosing a
//    stretch and magnifying it - and it made the common case impossible: you
//    cannot hear what you selected, because by the time you let go there is
//    nothing selected.
//
//    Shift turns a drag on the picture into a scrub, with a hand for a cursor.
//    Scrubbing is the narrower act and so it takes the modifier: it only means
//    anything while something is playing, where a selection means something
//    always. Shift on a stopped engine therefore selects, rather than doing
//    nothing - a modifier that silently kills a gesture is worse than one that
//    is ignored. §21's verbs are play, pause, stop, seek and skip; there is no
//    "move the playhead", so a scrub on a stopped engine could only be a run
//    of overlapping auditions, each one a round trip behind the pointer.
//
//    A press that does not travel [`SLOP`] pixels is a click and still seeks,
//    which is what it always did - and it clears the selection, because a
//    click is how a person says "not that bit after all". A press that does
//    travel must not seek when it comes up, or choosing a stretch would move
//    the playhead out of it every time.
//
// # The playhead and the labels
//
// Both are on this canvas and both are there because of the same fact: a mark
// that is a pixel away from the column it describes is worse than no mark. The
// rulers are inside the canvas for that reason (above), and a playhead beside
// the column it is playing, or a track name beside the boundary it starts at,
// would be the same defect wearing a different hat.
//
// The playhead is drawn wherever the engine's position is, playing or stopped.
// §20 requires "playhead and time ruler", and stopped is the state in which it
// matters most: a person stops *because* they have just heard the boundary go
// by, and the next thing they do is read where it fell. The handle in the top
// ruler is Audacity's shape - a flat top narrowing to a point on the line -
// because that is the shape people already read as "the playhead is here", and
// because a line one device pixel wide is not something a pointer can aim at.
//
// It is a grab handle only while something is playing, and the cursor says so:
// the handle is the one place a scrub needs no Shift, because a thing shaped
// like a grip is already the modifier. That is the same limit the scrub has and
// for the same reason, above. A *click* anywhere - picture, labels, either
// ruler - still moves the position, which is what Audacity's ruler click does
// and is the whole of what a stopped engine needs.
//
// The label lane is Audacity's label track: one strip under the picture, each
// track's name across the stretch of time it covers, with a line at the frame
// it starts on. It is the only thing on the page that says *which* track a peak
// belongs to. The lane is only there when there are tracks, so a project with
// nothing detected yet gives the picture the whole of its height.
//
// The bar drag is coalesced to one change a frame: a pointer reports faster than
// the screen draws, each change asks the shell for a fresh set of columns, and
// the positions a drag passed through are not something anybody asked to see.
// The wheel is deliberately *not* coalesced, because a wheel is incremental -
// dropping one report would be dropping a zoom step rather than an intermediate
// position - and a browser already delivers it at about the frame rate. The
// scrub is throttled harder than either, because a `SEEK` reserves a chunk in
// the playback queue and sixty of those a second is an engine that spends the
// drag re-cueing rather than playing.

import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../api";
import type {
  Boundary,
  Capture,
  Track,
  Waveform as Peaks,
} from "../bindings/vcw";
import { Face } from "../Face";
import { clock } from "../format";
import { ticks } from "../ruler";
import {
  STEP,
  type Span,
  fit,
  follow,
  nearestFrame,
  pan,
  region,
  resolve,
  toBand,
  zoom,
} from "../zoom";

/** How the waveform is drawn. Kept here because it is appearance, not policy. */
const COLORS = {
  background: "#101216",
  rms: "#4f8cc9",
  peak: "#2b5f8f",
  center: "#2a2f39",
  playhead: "#f2c14e",
  promoted: "#7ec46f",
  rejected: "#8a8f99",
  locked: "#f2874e",
  ruler: "#191c22",
  rulerLine: "#2a2f39",
  rulerText: "#8a8f99",
  lane: "#15161f",
  laneFill: "#2f2a47",
  laneLine: "#6a5ca8",
  laneText: "#cfc9e8",
} as const;

/**
 * The look of a ruler, in CSS pixels before the device ratio is applied.
 *
 * `HEIGHT` is what the strip costs the picture, twice: at 18 each a ruler pair
 * takes 36 of the top half's height, which on a 1000-pixel window is 36 of
 * about 460. Worth it for being able to read a time off either edge without
 * tracking a line across the whole width.
 */
const RULER = {
  HEIGHT: 18,
  MAJOR: 6,
  MINOR: 3,
  TEXT: 10,
  /** How far a label sits to the right of its own mark. */
  GAP: 3,
} as const;

/**
 * The label lane, in CSS pixels before the device ratio is applied.
 *
 * Twenty is a line of 10px text with a pixel of air above and below it, which
 * is as little as a name can be given and still be read. With the two rulers
 * that is 56 pixels of furniture around the picture, and only when a project
 * has tracks to put in it.
 */
const LANE = {
  HEIGHT: 20,
  TEXT: 10,
  /** How far a name sits inside its own block. */
  PAD: 4,
} as const;

/**
 * The playhead's grab handle, in CSS pixels.
 *
 * `WIDTH` is what a pointer has to hit: eleven pixels is about a mouse's worth
 * of aim, against the one device pixel the line itself is. `HEIGHT` is capped
 * at the ruler's own height when it is drawn, so the handle can never overhang
 * the picture whatever the ratio.
 */
const HANDLE = { WIDTH: 11, HEIGHT: 14 } as const;

/**
 * The shortest gap between two scrub seeks, in milliseconds.
 *
 * A pointer drag reports several times faster than this, and every report would
 * otherwise be a `SEEK` that reserves a chunk in the playback queue. Eleven a
 * second is enough to hear where a drag has got to and few enough that the
 * engine spends the drag playing.
 */
const SCRUB = 90;

/**
 * How far a pointer must travel before a press becomes a drag, in CSS pixels.
 *
 * Without it there is no such thing as a click: a mouse moves a pixel or two
 * under the force of the button going down, and every attempt to put the
 * playhead somewhere would instead select two pixels of audio and zoom to
 * them. Three is the figure a scrollbar uses for the same reason.
 */
const SLOP = 3;

/**
 * How near a boundary a click must land to snap to it, in CSS pixels.
 *
 * Wider than `SLOP`, because this is a target being aimed at rather than a
 * tremor being tolerated. A boundary is drawn one or two pixels wide, so a
 * tolerance the width of the line would mean hitting it exactly.
 */
const SNAP = 6;

/**
 * How much of the window a wheel notch pans.
 *
 * A tenth, so crossing a whole side takes ten flicks at any zoom - the same
 * gesture whether the window is the side or a second of it. A notch is a
 * hundred pixels of delta, hence the thousand.
 */
const PAN_PER_DELTA = 1 / 1000;

/** The widest delta one wheel event is allowed to mean. */
const MOST_DELTA = 300;

/** What the panel is showing, which is view state and lives here. */
export type View = {
  /** Which capture. */
  readonly captureId: number;
  /** Which channel. */
  readonly channel: number;
  /** First frame shown. */
  readonly startFrame: number;
  /** Last frame shown, or null for the end of the capture. */
  readonly endFrame: number | null;
};

/**
 * A stretch of the capture a person has chosen, in seconds.
 *
 * Seconds, and held by the caller rather than here, for one reason each. In
 * seconds because the only thing that happens to a selection is that it gets
 * played, and §35's audition takes a region in seconds; in the caller because
 * the transport plays it and the transport is not inside this panel. This
 * draws the band and reports the drag.
 */
export type Region = { readonly from: number; readonly to: number };

/**
 * Draws one channel of one capture.
 *
 * `onSeek` is given seconds, not a frame: the caller is the audition and the
 * audition takes seconds. Converting here would mean this component knowing the
 * rate for a reason other than drawing, which is the beginning of it knowing
 * things.
 *
 * `onCue` is the other half of that, and the difference is whether anything is
 * meant to be heard. A scrub seeks, because a scrub is a person listening for
 * where they are. A click cues: it puts the playhead down and plays nothing,
 * which is what somebody lining up a split is doing.
 *
 * `tracks` are the ones on a side this capture holds, which the caller decides
 * for the same reason it decides which capture is current. This draws what it
 * is given, in the seconds it is given them in.
 *
 * `view` is nullable so that the top half of the page has exactly one empty
 * state. A project with nothing recorded has no capture and therefore no range
 * to show, and the caller holds those as two pieces of state which are set one
 * after the other - so either of them can be the missing one for a render. Both
 * land here, on one branch, rather than the caller inventing a range for a
 * capture that is not there.
 */
export function Waveform({
  capture,
  view,
  onView,
  boundaries,
  tracks,
  playhead,
  playing,
  onSeek,
  onCue,
  selection,
  onSelect,
  generation,
}: {
  capture: Capture | undefined;
  view: View | null;
  onView: (view: View) => void;
  boundaries: readonly Boundary[];
  tracks: readonly Track[];
  playhead: number;
  playing: boolean;
  onSeek: (seconds: number) => void;
  onCue: (seconds: number) => void;
  selection: Region | null;
  onSelect: (selection: Region | null) => void;
  generation: number;
}): React.JSX.Element {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const box = useRef<HTMLDivElement | null>(null);
  const bar = useRef<HTMLDivElement | null>(null);
  const [peaks, setPeaks] = useState<Peaks | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0, scale: 1 });
  const [failed, setFailed] = useState<string | null>(null);

  /** Where on the thumb a bar drag took hold, as a fraction of the bar. */
  const grabbed = useRef<number | null>(null);
  /** Whether a drag across the picture is currently scrubbing. */
  const scrubbing = useRef(false);
  /** When the last scrub seek went out, on the monotonic clock. */
  const scrubbed = useRef(0);
  /**
   * Where a selection drag took hold: the second, and the client x it started
   * at so that [`SLOP`] can be measured from somewhere.
   */
  const selecting = useRef<{ x: number; from: number } | null>(null);
  /**
   * Where a scroll drag on a ruler took hold: the client x, and the first
   * frame of the window at the moment of the press.
   *
   * The window start is remembered rather than the drag being applied one
   * report at a time, for the reason the scrollbar drag works the same way: a
   * relative pan accumulates the rounding of every intermediate position, and
   * a drag out to the end of the capture and back would not come home.
   */
  const scrolling = useRef<{ x: number; start: number } | null>(null);
  /**
   * Whether the press in progress has traveled far enough to be a drag.
   *
   * Read by the click handler, which fires after the pointer is up and must
   * not also seek: the end of a selection is a zoom, not a position.
   */
  const dragged = useRef(false);
  /**
   * The last place the pointer was over the picture, in client coordinates.
   *
   * Kept so that pressing or releasing Shift can change the cursor without
   * waiting for the pointer to move. It has to: the whole sequence is hover,
   * press Shift, press the button, drag - so a cursor that only updated on
   * movement would show the scrub hand for the first time once the scrub was
   * already under way, which is the one moment it is no longer a hint.
   */
  const hover = useRef<{ x: number; y: number } | null>(null);
  /**
   * The band a drag in progress is painting, or null when none is.
   *
   * Local, where the finished selection is the caller's. A pointer reports
   * several times a frame and every report would otherwise be a state change
   * in `App` and a render of every panel, to move one edge of one `div`. The
   * caller hears once, when the pointer comes up.
   */
  const [drafting, setDrafting] = useState<Region | null>(null);
  /** The work a drag has asked for and the next frame has not yet done. */
  const queued = useRef<(() => void) | null>(null);

  /**
   * Do `work` on the next frame, dropping any earlier work still waiting.
   *
   * Dropping rather than queueing, which is safe only because the one caller
   * asks for an *absolute* position: the last position of a drag is the only
   * one still true when the frame is drawn, and each of the others would cost a
   * set of columns from the shell. The wheel does not come through here for the
   * opposite reason - see the header.
   */
  const soon = useCallback((work: () => void) => {
    const idle = queued.current === null;
    queued.current = work;
    if (idle) {
      requestAnimationFrame(() => {
        const run = queued.current;
        queued.current = null;
        run?.();
      });
    }
  }, []);

  const frames = capture?.frames ?? 0;
  const span: Span =
    view === null
      ? fit()
      : { startFrame: view.startFrame, endFrame: view.endFrame };
  const shown = resolve(span, frames);

  /**
   * Show a different range of the same capture.
   *
   * Guarded by value, not by identity: every gesture here ends in a `zoom.ts`
   * function that returns a fresh object, and a pan already against the end of
   * the capture returns the same two numbers in a new one. Without the guard
   * that is a state change, a re-render and a summary read for every pointer
   * report, drawing a picture that cannot move.
   */
  const show = useCallback(
    (next: Span) => {
      if (view === null) {
        return;
      }
      if (
        next.startFrame !== view.startFrame ||
        next.endFrame !== view.endFrame
      ) {
        onView({ ...view, ...next });
      }
    },
    [onView, view],
  );

  // The size the box actually has, in device pixels. Measured rather than
  // assumed, because `pixels` is what the shell reduces to and a wrong number
  // is either a stretched picture or peaks computed for columns nobody can
  // see. The height is measured too, now that the box is half the page rather
  // than a fixed strip.
  useEffect(() => {
    const element = box.current;
    if (element === null) {
      return;
    }
    const measure = (css: { width: number; height: number }) => {
      const scale = window.devicePixelRatio || 1;
      setSize({
        width: Math.max(1, Math.floor(css.width * scale)),
        height: Math.max(1, Math.floor(css.height * scale)),
        scale,
      });
    };
    const observer = new ResizeObserver((entries) => {
      const first = entries[0];
      if (first !== undefined) {
        measure(first.contentRect);
      }
    });
    observer.observe(element);
    measure({
      width: element.clientWidth,
      height: element.clientHeight,
    });
    return () => observer.disconnect();
  }, []);

  // The fetch. Four dependencies and the generation, as argued above. The
  // height is not one of them: a taller picture is the same peaks drawn
  // further, and refetching on a vertical resize would ask the shell for
  // columns it has already answered.
  useEffect(() => {
    if (capture === undefined || view === null || size.width < 8) {
      setPeaks(null);
      return;
    }
    let canceled = false;
    void (async () => {
      try {
        const drawn = await api.waveform({
          captureId: view.captureId,
          channel: view.channel,
          startFrame: view.startFrame,
          endFrame: view.endFrame,
          pixels: size.width,
        });
        if (!canceled) {
          setPeaks(drawn);
          setFailed(null);
        }
      } catch (error) {
        if (!canceled) {
          setPeaks(null);
          setFailed(api.asFailure(error).message);
        }
      }
    })();
    return () => {
      canceled = true;
    };
  }, [
    capture,
    view?.captureId,
    view?.channel,
    view?.startFrame,
    view?.endFrame,
    size.width,
    generation,
  ]);

  // The draw. Separate from the fetch so a playhead moving at 20 Hz repaints
  // the canvas without asking the shell for peaks it already has.
  useEffect(() => {
    const element = canvas.current;
    if (element === null || peaks === null) {
      return;
    }
    const context = element.getContext("2d");
    if (context === null) {
      return;
    }
    const width = element.width;
    const height = element.height;
    const scale = size.scale;
    const rule = Math.round(RULER.HEIGHT * scale);

    // One lane row per side, in the order the sides appear, and none at all
    // when there are no tracks - an undetected project spends none of the
    // picture's height on an empty strip.
    //
    // Per side rather than one row for everything, because §21 allows two
    // faces on one capture and the first light project is exactly that: both
    // its sides name capture 1. Stacked in a single row, side A's labels were
    // drawn and then painted over by side B's, which is a lane that quietly
    // shows half of what it was given. The positions already carry the letter
    // (`A1`, `B1`), so a row needs no name of its own.
    const rows = new Map<number, number>();
    for (const track of tracks) {
      if (!rows.has(track.sideId)) {
        rows.set(track.sideId, rows.size);
      }
    }
    const row = Math.round(LANE.HEIGHT * scale);
    const lane = rows.size * row;

    // The band the signal gets: what is left between the two rulers and the
    // label lane. Floored at a pixel so a box briefly shorter than its own
    // furniture during a layout pass draws something rather than dividing by
    // zero.
    const bandTop = rule;
    const bandHeight = Math.max(1, height - rule * 2 - lane);
    const middle = bandTop + bandHeight / 2;
    const reach = bandHeight / 2;
    const laneTop = bandTop + bandHeight;

    // The visible window in seconds, which is what the labels, the playhead and
    // the rulers are all placed by. The playhead arrives in seconds and the
    // window carries both ends in seconds, so nothing in this file needs the
    // rate - which is why `startSeconds` and `endSeconds` are on the view model
    // at all. Multiplying a playhead by a rate here would be a unit conversion
    // on the wrong side of the boundary, and it would be wrong the first time a
    // project held two captures at different rates.
    const visible = peaks.endSeconds - peaks.startSeconds;

    context.fillStyle = COLORS.background;
    context.fillRect(0, 0, width, height);
    context.fillStyle = COLORS.center;
    context.fillRect(0, Math.floor(middle), width, 1);

    const columns = Math.min(
      peaks.min.length,
      peaks.max.length,
      peaks.rms.length,
    );
    for (let x = 0; x < columns; x += 1) {
      const low = peaks.min[x] ?? 0;
      const high = peaks.max[x] ?? 0;
      const rms = peaks.rms[x] ?? 0;
      context.fillStyle = COLORS.peak;
      context.fillRect(
        x,
        middle - high * reach,
        1,
        Math.max(1, (high - low) * reach),
      );
      context.fillStyle = COLORS.rms;
      context.fillRect(
        x,
        middle - rms * reach,
        1,
        Math.max(1, rms * reach * 2),
      );
    }

    // The boundaries, on top. A rejected one is drawn dimmer rather than left
    // out: WP-11 asked whether it should be visible at all, and it should -
    // §31 lets a person promote one by hand, and they cannot promote what the
    // picture does not show.
    //
    // Across the band only, not the whole canvas: a line through a ruler label
    // makes the label unreadable, and the ruler is the reason a person can say
    // where the boundary *is*. Nor down through the lane - a boundary belongs
    // to one side, and a line across every row of a two-faced capture would
    // claim it belongs to both. Each row draws its own starts instead.
    const span = peaks.endFrame - peaks.startFrame;
    if (span > 0) {
      for (const boundary of boundaries) {
        const x = ((boundary.atFrame - peaks.startFrame) / span) * width;
        if (x < 0 || x > width) {
          continue;
        }
        context.fillStyle = boundary.locked
          ? COLORS.locked
          : boundary.promoted
            ? COLORS.promoted
            : COLORS.rejected;
        // A rejected boundary gets a thin line and a promoted one a thick one,
        // so the difference survives a grayscale screenshot and a person who
        // does not distinguish the two colors.
        context.fillRect(
          Math.floor(x),
          bandTop,
          Math.max(1, Math.round((boundary.promoted ? 2 : 1) * scale)),
          bandHeight,
        );
      }
    }

    // The label lane. Drawn after the boundaries so a boundary line and the
    // edge of a block can be seen to be the same instant, and before the
    // rulers, which own the two strips it cannot reach.
    //
    // Seconds throughout, because a track is stored in seconds and the window
    // carries seconds - the same reason the playhead needs no rate.
    if (lane > 0 && visible > 0) {
      context.fillStyle = COLORS.lane;
      context.fillRect(0, laneTop, width, lane);
      context.fillStyle = COLORS.rulerLine;
      context.fillRect(0, laneTop, width, 1);
      const size = Math.round(LANE.TEXT * scale);
      context.font = `${size}px system-ui, -apple-system, "Segoe UI", sans-serif`;
      context.textBaseline = "middle";
      const pad = Math.round(LANE.PAD * scale);
      for (const track of tracks) {
        const from = ((track.start - peaks.startSeconds) / visible) * width;
        const to = ((track.end - peaks.startSeconds) / visible) * width;
        if (to < 0 || from > width) {
          continue;
        }
        const top = laneTop + (rows.get(track.sideId) ?? 0) * row;
        // Clipped to the canvas rather than dropped, and that is the case that
        // matters: zooming in far enough to judge a boundary puts both ends of
        // the track off screen, and it is exactly then that a person wants to
        // be told which track they are inside.
        const left = Math.max(0, Math.floor(from));
        const right = Math.min(width, Math.ceil(to));
        context.fillStyle = COLORS.laneFill;
        context.fillRect(left, top + 1, Math.max(1, right - left - 1), row - 1);
        // The start edge, drawn only where the start is really on screen. A
        // block clipped at the left edge with a line on it would claim the
        // track starts where the window does.
        if (from >= 0) {
          context.fillStyle = COLORS.laneLine;
          context.fillRect(
            Math.floor(from),
            top + 1,
            Math.max(1, Math.round(scale)),
            row - 1,
          );
        }
        const name =
          track.title === ""
            ? track.position
            : `${track.position}  ${track.title}`;
        context.save();
        context.beginPath();
        context.rect(left, top, Math.max(0, right - left), row);
        context.clip();
        context.fillStyle = COLORS.laneText;
        context.fillText(name, left + pad, top + row / 2);
        context.restore();
      }
    }

    // The playhead, in two parts because the rulers are drawn over everything
    // above them: the line now, across the picture and the labels, and the
    // handle after the rulers, since sitting in the top ruler is the whole
    // point of it.
    const stem = Math.max(1, Math.round(scale));
    const playAt =
      visible > 0 ? ((playhead - peaks.startSeconds) / visible) * width : -1;
    const onScreen = playAt >= 0 && playAt <= width;
    if (onScreen) {
      context.fillStyle = COLORS.playhead;
      // Across the labels as well as the picture, because the playhead is a
      // fact about the capture and every row of the lane is in that capture.
      context.fillRect(Math.floor(playAt), bandTop, stem, bandHeight + lane);
    }

    // The rulers, last, so nothing drawn above can run into them.
    context.fillStyle = COLORS.ruler;
    context.fillRect(0, 0, width, rule);
    context.fillRect(0, height - rule, width, rule);
    context.fillStyle = COLORS.rulerLine;
    context.fillRect(0, rule - 1, width, 1);
    context.fillRect(0, height - rule, width, 1);

    const text = Math.round(RULER.TEXT * scale);
    context.font = `${text}px ui-monospace, "SF Mono", Menlo, Consolas, monospace`;
    context.textBaseline = "top";
    const gap = Math.round(RULER.GAP * scale);
    const major = Math.round(RULER.MAJOR * scale);
    const minor = Math.round(RULER.MINOR * scale);

    // Both rulers, one pass, same marks: the answer to "which range is this"
    // must be the same at both edges or they are two rulers and not one.
    for (const tick of ticks(peaks.startSeconds, peaks.endSeconds, width)) {
      const x = Math.floor(
        ((tick.seconds - peaks.startSeconds) / visible) * width,
      );
      const length = tick.major ? major : minor;
      context.fillStyle = COLORS.rulerLine;
      // Growing inwards from the picture on both sides, so a mark always
      // points at the column it belongs to.
      context.fillRect(x, rule - length, 1, length);
      context.fillRect(x, height - rule, 1, length);
      if (tick.label === "") {
        continue;
      }
      // Dropped rather than clipped or pushed left: a label hanging off the
      // right edge reads as a different number, and one shoved back inside
      // would no longer sit beside its own mark.
      if (x + gap + context.measureText(tick.label).width > width) {
        continue;
      }
      context.fillStyle = COLORS.rulerText;
      context.fillText(tick.label, x + gap, Math.round(scale));
      context.fillText(tick.label, x + gap, height - rule + major + Math.round(scale));
    }

    // The handle: a flat top in the ruler narrowing to a point on the line, so
    // the thing a pointer aims at and the thing it points at are one object.
    // Last of all, because the ruler strip above was just painted over it.
    if (onScreen) {
      const half = Math.round(HANDLE.WIDTH * scale) / 2;
      const deep = Math.min(rule, Math.round(HANDLE.HEIGHT * scale));
      const tip = Math.floor(playAt) + stem / 2;
      const top = rule - deep;
      context.fillStyle = COLORS.playhead;
      context.beginPath();
      context.moveTo(tip - half, top);
      context.lineTo(tip + half, top);
      context.lineTo(tip + half, top + deep * 0.55);
      context.lineTo(tip, top + deep);
      context.lineTo(tip - half, top + deep * 0.55);
      context.closePath();
      context.fill();
    }
  }, [peaks, boundaries, tracks, playhead, size.height, size.scale]);

  // The wheel, bound natively rather than as an `onWheel` prop. React attaches
  // `wheel` at the root container as a *passive* listener, so `preventDefault`
  // inside a JSX handler is ignored with a console warning and the gesture
  // reaches the window as well as the canvas.
  useEffect(() => {
    const element = canvas.current;
    if (element === null || frames <= 0) {
      return;
    }
    const wheeled = (event: WheelEvent) => {
      event.preventDefault();
      const bounds = element.getBoundingClientRect();
      if (bounds.width <= 0) {
        return;
      }
      const at = (event.clientX - bounds.left) / bounds.width;
      // A notch is a hundred pixels of delta in a browser and one *line* under
      // GTK, and the two have to feel the same. Clamped, because one flick of a
      // free-spinning wheel can report a thousand and that is a gesture nobody
      // can aim.
      const lines = event.deltaMode === 1;
      const grip = (delta: number) =>
        Math.max(-MOST_DELTA, Math.min(MOST_DELTA, lines ? delta * 16 : delta));
      const up = grip(event.deltaY);
      const sideways = grip(event.deltaX);
      if (event.shiftKey || Math.abs(sideways) > Math.abs(up)) {
        const by = (sideways !== 0 ? sideways : up) * PAN_PER_DELTA;
        const width = shown.end - shown.start;
        show(pan(span, frames, Math.round(width * by)));
      } else {
        // `STEP` per notch exactly, and the matching fraction of it for the
        // fraction of a notch a trackpad reports - so a mouse click halves the
        // window and a two-finger drag is smooth rather than stepped.
        show(zoom(span, frames, STEP ** (up / 100), at));
      }
    };
    element.addEventListener("wheel", wheeled, { passive: false });
    return () => element.removeEventListener("wheel", wheeled);
  }, [frames, shown.start, shown.end, span.startFrame, span.endFrame, show]);

  // Page the window along while the audition plays, so a playhead that runs off
  // the right edge takes the picture with it. `follow` decides whether anything
  // moves at all; this only has to say where the playhead is.
  //
  // In frames, read off the window rather than off a sample rate. The peaks
  // carry both ends in frames *and* in seconds, so the map between them is the
  // window's own and this file still does not know what rate the capture was
  // made at - which is what keeps it right for a project holding two captures
  // at different rates. Linear, so it is exact for a playhead outside the
  // window too, which is the case that matters here.
  useEffect(() => {
    if (!playing || peaks === null || frames <= 0) {
      return;
    }
    const seconds = peaks.endSeconds - peaks.startSeconds;
    if (seconds <= 0) {
      return;
    }
    const perSecond = (peaks.endFrame - peaks.startFrame) / seconds;
    const at = peaks.startFrame + (playhead - peaks.startSeconds) * perSecond;
    show(follow(span, frames, Math.round(at)));
  }, [playing, playhead, peaks, frames, span.startFrame, span.endFrame, show]);

  /** Where a pointer is over the picture, in seconds of the capture. */
  const secondsAt = (clientX: number, element: HTMLElement): number | null => {
    if (peaks === null) {
      return null;
    }
    const bounds = element.getBoundingClientRect();
    if (bounds.width <= 0) {
      return null;
    }
    const at = (clientX - bounds.left) / bounds.width;
    return peaks.startSeconds + at * (peaks.endSeconds - peaks.startSeconds);
  };

  /**
   * The boundary nearest a pointer, in seconds, or `null` if none is in reach.
   *
   * The search is [`nearestFrame`], which is where the frame-to-pixel
   * arithmetic lives and where it is tested. This half is only the plumbing:
   * the element's box, the frames at its two ends, and the conversion back to
   * the seconds the callbacks speak.
   */
  const boundaryNear = (clientX: number, element: HTMLElement): number | null => {
    if (peaks === null) {
      return null;
    }
    const bounds = element.getBoundingClientRect();
    const at = nearestFrame(
      boundaries.map((boundary) => boundary.atFrame),
      clientX - bounds.left,
      bounds.width,
      peaks.startFrame,
      peaks.endFrame,
      SNAP,
    );
    if (at === null) {
      return null;
    }
    const across = peaks.endFrame - peaks.startFrame;
    return (
      peaks.startSeconds +
      ((at - peaks.startFrame) / across) * (peaks.endSeconds - peaks.startSeconds)
    );
  };

  /**
   * Whether a pointer is on one of the two rulers, and so whether a drag from
   * here would scroll.
   *
   * Either ruler, because there are two and a gesture that worked on one of
   * them would be a gesture a person had to remember the location of. In CSS
   * pixels, like [`onHandle`] and for the same reason.
   */
  const onRuler = (clientY: number, element: HTMLElement): boolean => {
    const bounds = element.getBoundingClientRect();
    if (bounds.height <= RULER.HEIGHT * 2) {
      return false;
    }
    const at = clientY - bounds.top;
    return at <= RULER.HEIGHT || at >= bounds.height - RULER.HEIGHT;
  };

  /** Put the window where a drag on a ruler has carried it. */
  const scroll = (clientX: number, element: HTMLElement) => {
    const held = scrolling.current;
    if (held === null || peaks === null) {
      return;
    }
    const bounds = element.getBoundingClientRect();
    if (bounds.width <= 0) {
      return;
    }
    // The picture follows the hand, which is why the travel is subtracted:
    // dragging a ruler to the right pulls earlier audio into view, the way
    // dragging a map does. Scrolling the window the same way as the pointer
    // instead would be correct for a scrollbar and wrong for the thing being
    // scrolled - and the scrollbar is six pixels below, doing exactly that.
    const perPixel = (peaks.endFrame - peaks.startFrame) / bounds.width;
    const want = Math.round(held.start - (clientX - held.x) * perPixel);
    const start = shown.start;
    soon(() => show(pan(span, frames, want - start)));
  };

  /**
   * Whether a pointer is on the playhead's handle, and so whether a drag from
   * here would move it.
   *
   * In CSS pixels throughout, because a pointer reports in CSS pixels and the
   * handle's size is declared in them - the device ratio belongs to the canvas
   * and nowhere else. `playing`, because a handle is only a handle while there
   * is something to drag; see the header.
   */
  const onHandle = (
    clientX: number,
    clientY: number,
    element: HTMLElement,
  ): boolean => {
    if (!playing || peaks === null) {
      return false;
    }
    const bounds = element.getBoundingClientRect();
    const seconds = peaks.endSeconds - peaks.startSeconds;
    if (bounds.width <= 0 || seconds <= 0) {
      return false;
    }
    if (clientY - bounds.top > RULER.HEIGHT) {
      return false;
    }
    const at = ((playhead - peaks.startSeconds) / seconds) * bounds.width;
    return Math.abs(clientX - bounds.left - at) <= HANDLE.WIDTH;
  };

  /**
   * Say what a drag from here would do, which the stylesheet turns into a
   * cursor.
   *
   * Written straight onto the element rather than held in state, which is the
   * same choice the first draft made and for the same reason: a hint that
   * re-rendered would cost a React pass per pointer report to change one
   * string.
   *
   * Three answers for four gestures, and that is the point rather than a
   * shortfall: a scrub and a ruler scroll are both "take hold of this and
   * move it", so both get the hand, and what a hand means in each place is
   * obvious from where it is. Only selecting looks different, because it is
   * the one that draws rather than drags.
   *
   * An attribute rather than `style.cursor`, which is what it used to set. The
   * select cursor is a drawing in `public/`, and a component that had to name
   * that file would be the one place in the UI where an asset path lives in
   * TypeScript - and it would have to name it twice, once per pixel ratio.
   */
  const dress = (shift: boolean) => {
    const element = canvas.current;
    const at = hover.current;
    if (element === null || at === null) {
      return;
    }
    element.dataset.gesture =
      scrubbing.current || scrolling.current !== null
        ? "dragging"
        : (shift && playing) || onHandle(at.x, at.y, element) || onRuler(at.y, element)
          ? "grab"
          : boundaryNear(at.x, element) !== null
            ? "snap"
            : "select";
  };

  // Through a ref so the listener below can be attached once. An effect that
  // depended on `dress` would detach and reattach on every render, and `dress`
  // closes over `playing` and `peaks`, so that is every render there is.
  const dresser = useRef(dress);
  dresser.current = dress;

  // Shift changes what a press means, so it has to change the cursor, and a
  // key press is not a pointer event - nothing else here would notice it.
  useEffect(() => {
    const onShift = (event: KeyboardEvent) => {
      if (event.key === "Shift") {
        dresser.current(event.type === "keydown");
      }
    };
    window.addEventListener("keydown", onShift);
    window.addEventListener("keyup", onShift);
    return () => {
      window.removeEventListener("keydown", onShift);
      window.removeEventListener("keyup", onShift);
    };
  }, []);

  /** Put the window's left edge where a drag on the bar has taken it. */
  const slide = (clientX: number, offset: number) => {
    const track = bar.current;
    if (track === null || frames <= 0) {
      return;
    }
    const bounds = track.getBoundingClientRect();
    if (bounds.width <= 0) {
      return;
    }
    const at = (clientX - bounds.left) / bounds.width - offset;
    const start = shown.start;
    soon(() => show(pan(span, frames, Math.round(at * frames) - start)));
  };

  // The bar, as two fractions of the capture. A floor on the thumb's width so
  // that a window four thousand frames wide inside a whole side is still
  // something a pointer can find, and the left edge pulled back off the end so
  // the floor cannot push the thumb out of the bar.
  const portion = frames > 0 ? (shown.end - shown.start) / frames : 1;
  const leading = frames > 0 ? shown.start / frames : 0;
  const thumbWide = Math.min(100, Math.max(portion * 100, 2));
  const thumbLeft = Math.min(leading * 100, 100 - thumbWide);

  // The selection band, in percentages of the picture: the drag in progress
  // if there is one, and otherwise what was chosen last. Computed against the
  // peaks rather than against the span, because the peaks are what is actually
  // drawn: a window that has just changed has a render where the two disagree,
  // and a band a few pixels off the audio it covers is the one thing it must
  // never be.
  //
  // Which also makes a selection survive zooming and scrolling for free, and
  // it has to: choosing a stretch and then looking more closely at it is one
  // act, and a band that vanished at the first wheel notch would make the
  // selection something a person had to redo rather than keep.
  const chosen = drafting ?? selection;
  const band =
    chosen === null || peaks === null
      ? null
      : toBand(chosen.from, chosen.to, peaks.startSeconds, peaks.endSeconds);

  // No early return for "nothing captured yet", and that is not tidiness. The
  // size of the picture is measured by a `ResizeObserver` set up once on mount,
  // so a render that leaves the box out of the tree gives that effect nothing
  // to observe and it never runs again - the panel then draws zero columns
  // forever. It did exactly that the first time, because this component used to
  // live in the always-on strip where it was mounted once for the life of the
  // window, and it now mounts and unmounts as a person moves between panels.
  //
  // So the box is always there and the empty state is a line in the foot. That
  // is the better shape anyway: the top half of the page keeps its size whether
  // or not a record has been played into it.
  return (
    <section className="waveform">
      <div className="waveform-box" ref={box}>
        <canvas
          ref={canvas}
          width={size.width}
          height={size.height}
          data-gesture="select"
          onPointerDown={(event) => {
            if (peaks === null || event.button !== 0) {
              return;
            }
            dragged.current = false;
            // Shift, or the playhead's own handle, and only while something is
            // playing - see the header. Everything else is a selection, so a
            // held Shift on a stopped engine selects rather than doing
            // nothing: a modifier that silently disables a gesture is worse
            // than one that is ignored.
            if (
              playing &&
              (event.shiftKey ||
                onHandle(event.clientX, event.clientY, event.currentTarget))
            ) {
              scrubbing.current = true;
              scrubbed.current = 0;
              event.currentTarget.setPointerCapture(event.pointerId);
              dress(event.shiftKey);
              return;
            }
            // A ruler scrolls. Checked after the handle, which lives in the
            // top ruler: the handle is the smaller target and the more
            // specific thing to be pointing at, so it wins where they overlap.
            if (onRuler(event.clientY, event.currentTarget)) {
              scrolling.current = { x: event.clientX, start: shown.start };
              event.currentTarget.setPointerCapture(event.pointerId);
              dress(event.shiftKey);
              return;
            }
            const from = secondsAt(event.clientX, event.currentTarget);
            if (from === null) {
              return;
            }
            selecting.current = { x: event.clientX, from };
            event.currentTarget.setPointerCapture(event.pointerId);
          }}
          onPointerMove={(event) => {
            hover.current = { x: event.clientX, y: event.clientY };
            dress(event.shiftKey);
            if (scrubbing.current) {
              const now = performance.now();
              if (now - scrubbed.current < SCRUB) {
                return;
              }
              const seconds = secondsAt(event.clientX, event.currentTarget);
              if (seconds !== null) {
                scrubbed.current = now;
                onSeek(seconds);
              }
              return;
            }
            if (scrolling.current !== null) {
              if (
                !dragged.current &&
                Math.abs(event.clientX - scrolling.current.x) < SLOP
              ) {
                return;
              }
              dragged.current = true;
              scroll(event.clientX, event.currentTarget);
              return;
            }
            const held = selecting.current;
            if (held === null) {
              return;
            }
            if (!dragged.current && Math.abs(event.clientX - held.x) < SLOP) {
              return;
            }
            dragged.current = true;
            const to = secondsAt(event.clientX, event.currentTarget);
            if (to !== null) {
              // Through `soon` for the reason the bar drag is: a pointer
              // reports faster than the screen draws, and every report but the
              // last describes a band nobody will ever see.
              soon(() => setDrafting({ from: held.from, to }));
            }
          }}
          onPointerUp={(event) => {
            scrubbing.current = false;
            scrolling.current = null;
            const held = selecting.current;
            selecting.current = null;
            setDrafting(null);
            dress(event.shiftKey);
            if (held === null || !dragged.current) {
              return;
            }
            // The one place a selection is published, and the reason the
            // panel keeps a draft of its own. `region` clamps it: pointer
            // capture reports past both ends of the capture, and "to the end"
            // is what running off the right edge of the picture means.
            const to = secondsAt(event.clientX, event.currentTarget);
            if (to !== null && capture !== undefined) {
              onSelect(region(held.from, to, capture.seconds));
            }
          }}
          onPointerCancel={() => {
            // Canceled, so nothing is chosen and nothing has moved: the draft
            // band goes away and the selection that was there before - if any
            // - is left alone. A gesture the system took away is not a gesture
            // a person finished.
            scrubbing.current = false;
            scrolling.current = null;
            selecting.current = null;
            dragged.current = false;
            setDrafting(null);
          }}
          onPointerLeave={() => {
            hover.current = null;
          }}
          onClick={(event) => {
            // Kept as a click rather than folded into the pointer handlers
            // above: a press with no drag is where a person wants to *start*,
            // and that is the same act whether the engine is running or not.
            // A scrub that ends in a click seeks once more to where it was
            // released, which is where it already is.
            //
            // A drag must not seek, which is the whole job of `dragged`: a
            // person who has just drawn a band around a track has said where
            // the interesting part is, and moving the playhead off it would
            // undo that in the same gesture.
            if (dragged.current) {
              dragged.current = false;
              return;
            }
            // Snapped to a boundary in reach first, because the frame a person
            // is aiming at is almost always one a detector has already found
            // and drawn, and never the pixel they managed to hit. Splitting a
            // track exactly where it was split before is the move this makes
            // possible, and it was the one nothing here could do.
            const seconds =
              boundaryNear(event.clientX, event.currentTarget) ??
              secondsAt(event.clientX, event.currentTarget);
            if (seconds !== null) {
              onCue(seconds);
            }
            // And a click puts the selection away. Nothing else can: the band
            // outlives the gesture that drew it, so there has to be a way to
            // say "not that bit" that is as cheap as saying "that bit" was,
            // and a click is already the gesture for "here, not there".
            if (selection !== null) {
              onSelect(null);
            }
          }}
        />
        {band !== null && (
          // A div over the canvas, not a rectangle drawn into it. The canvas
          // is redrawn from a fetch of peaks, so painting the band there would
          // put a pointer-rate redraw on the same path as a shell round trip;
          // and the band is wanted over the rulers and the label lane as well,
          // which are the same canvas but not the same drawing.
          <div
            className="waveform-selection"
            style={{ left: `${band.left}%`, width: `${band.width}%` }}
          />
        )}
      </div>
      <div
        className="waveform-bar"
        ref={bar}
        onPointerDown={(event) => {
          const bounds = event.currentTarget.getBoundingClientRect();
          if (bounds.width <= 0 || frames <= 0) {
            return;
          }
          const held = (event.clientX - bounds.left) / bounds.width;
          // Taking hold of the thumb keeps the point under the pointer;
          // pressing the bar either side of it puts the window's middle there,
          // which is what a scrollbar does everywhere else.
          const offset =
            held >= leading && held <= leading + portion
              ? held - leading
              : portion / 2;
          grabbed.current = offset;
          event.currentTarget.setPointerCapture(event.pointerId);
          slide(event.clientX, offset);
        }}
        onPointerMove={(event) => {
          if (grabbed.current !== null) {
            slide(event.clientX, grabbed.current);
          }
        }}
        onPointerUp={() => {
          grabbed.current = null;
        }}
        onPointerCancel={() => {
          grabbed.current = null;
        }}
      >
        <div
          className="waveform-thumb"
          style={{ left: `${thumbLeft}%`, width: `${thumbWide}%` }}
        />
      </div>
      <div className="waveform-foot">
        {capture === undefined || view === null ? (
          <span>Nothing captured yet.</span>
        ) : capture.channels < 2 ? (
          <span>1 channel</span>
        ) : (
          // `view.channel` has existed since WP-09 and nothing ever wrote to
          // it, so every stereo capture was drawn as its left channel with a
          // footer saying "channel 1 of 2" and no way to see the other one.
          // A select rather than a left/right pair of buttons because a
          // four-channel interface is allowed by §8 and two buttons are not.
          <label className="waveform-channel">
            Channel
            <select
              value={view.channel}
              onChange={(event) =>
                onView({ ...view, channel: Number(event.target.value) })
              }
            >
              {Array.from({ length: capture.channels }, (_, index) => (
                <option key={index} value={index}>
                  {capture.channels === 2
                    ? ["Left", "Right"][index]
                    : `${index + 1}`}
                </option>
              ))}
            </select>
          </label>
        )}
        {peaks !== null && (
          <span>
            {clock(peaks.startSeconds)} to {clock(peaks.endSeconds)}
          </span>
        )}
        <span>{size.width} column(s)</span>
        {failed !== null && <span className="clip">{failed}</span>}
        {/* The third row that follows Appearance > Buttons, after the panel
            tabs and the transport. It was text when those two switched and
            stayed text, which made the setting look like it meant "the two
            rows I happened to do first" rather than "the buttons". */}
        <span className="waveform-zoom">
          <button
            type="button"
            title="Zoom out (-)"
            disabled={frames <= 0}
            onClick={() => show(zoom(span, frames, STEP, 0.5))}
          >
            <Face icon="zoom-out">Out</Face>
          </button>
          <button
            type="button"
            title="Zoom in (+)"
            disabled={frames <= 0}
            onClick={() => show(zoom(span, frames, 1 / STEP, 0.5))}
          >
            <Face icon="zoom-in">In</Face>
          </button>
          <button
            type="button"
            title="Show the whole capture (0)"
            disabled={frames <= 0}
            onClick={() => show(fit())}
          >
            <Face icon="zoom-fit">Fit</Face>
          </button>
        </span>
      </div>
    </section>
  );
}
