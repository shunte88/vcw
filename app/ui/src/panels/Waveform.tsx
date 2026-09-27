/*
 *  Waveform.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The capture drawn to the width of the panel, with the boundaries on it (§30).
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

import { useEffect, useRef, useState } from "react";

import * as api from "../api";
import type { Boundary, Capture, Waveform as Peaks } from "../bindings/vcw";
import { clock } from "../format";

/** How the waveform is drawn. Kept here because it is appearance, not policy. */
const COLOURS = {
  background: "#101216",
  rms: "#4f8cc9",
  peak: "#2b5f8f",
  centre: "#2a2f39",
  playhead: "#f2c14e",
  promoted: "#7ec46f",
  rejected: "#8a8f99",
  locked: "#f2874e",
} as const;

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
 * Draws one channel of one capture.
 *
 * `onSeek` is given seconds, not a frame: the caller is the audition and the
 * audition takes seconds. Converting here would mean this component knowing the
 * rate for a reason other than drawing, which is the beginning of it knowing
 * things.
 */
export function Waveform({
  capture,
  view,
  boundaries,
  playhead,
  playing,
  onSeek,
  generation,
}: {
  capture: Capture | undefined;
  view: View;
  boundaries: readonly Boundary[];
  playhead: number;
  playing: boolean;
  onSeek: (seconds: number) => void;
  generation: number;
}): React.JSX.Element {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const box = useRef<HTMLDivElement | null>(null);
  const [peaks, setPeaks] = useState<Peaks | null>(null);
  const [width, setWidth] = useState(0);
  const [failed, setFailed] = useState<string | null>(null);

  // The width the panel actually has. Measured rather than assumed, because
  // `pixels` is what the shell reduces to and a wrong number is either a
  // stretched picture or peaks computed for columns nobody can see.
  useEffect(() => {
    const element = box.current;
    if (element === null) {
      return;
    }
    const observer = new ResizeObserver((entries) => {
      const first = entries[0];
      if (first !== undefined) {
        setWidth(Math.max(1, Math.floor(first.contentRect.width)));
      }
    });
    observer.observe(element);
    setWidth(Math.max(1, Math.floor(element.clientWidth)));
    return () => observer.disconnect();
  }, []);

  // The fetch. Four dependencies and the generation, as argued above.
  useEffect(() => {
    if (capture === undefined || width < 8) {
      setPeaks(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const drawn = await api.waveform({
          captureId: view.captureId,
          channel: view.channel,
          startFrame: view.startFrame,
          endFrame: view.endFrame,
          pixels: width,
        });
        if (!cancelled) {
          setPeaks(drawn);
          setFailed(null);
        }
      } catch (error) {
        if (!cancelled) {
          setPeaks(null);
          setFailed(api.asFailure(error).message);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    capture,
    view.captureId,
    view.channel,
    view.startFrame,
    view.endFrame,
    width,
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
    const height = element.height;
    const middle = height / 2;
    context.fillStyle = COLOURS.background;
    context.fillRect(0, 0, element.width, height);
    context.fillStyle = COLOURS.centre;
    context.fillRect(0, Math.floor(middle), element.width, 1);

    const columns = Math.min(
      peaks.min.length,
      peaks.max.length,
      peaks.rms.length,
    );
    for (let x = 0; x < columns; x += 1) {
      const low = peaks.min[x] ?? 0;
      const high = peaks.max[x] ?? 0;
      const rms = peaks.rms[x] ?? 0;
      context.fillStyle = COLOURS.peak;
      const top = middle - high * middle;
      context.fillRect(x, top, 1, Math.max(1, (high - low) * middle));
      context.fillStyle = COLOURS.rms;
      context.fillRect(x, middle - rms * middle, 1, Math.max(1, rms * height));
    }

    // The boundaries, on top. A rejected one is drawn dimmer rather than left
    // out: WP-11 asked whether it should be visible at all, and it should -
    // §31 lets a person promote one by hand, and they cannot promote what the
    // picture does not show.
    const span = peaks.endFrame - peaks.startFrame;
    if (span > 0) {
      for (const boundary of boundaries) {
        const x =
          ((boundary.atFrame - peaks.startFrame) / span) * element.width;
        if (x < 0 || x > element.width) {
          continue;
        }
        context.fillStyle = boundary.locked
          ? COLOURS.locked
          : boundary.promoted
            ? COLOURS.promoted
            : COLOURS.rejected;
        // A rejected boundary gets a thin line and a promoted one a thick one,
        // so the difference survives a greyscale screenshot and a person who
        // does not distinguish the two colours.
        context.fillRect(Math.floor(x), 0, boundary.promoted ? 2 : 1, height);
      }
    }

    // The playhead comes in seconds and the window is also given in seconds,
    // so nothing here needs the rate. That is the reason `startSeconds` and
    // `endSeconds` are on the view model at all: multiplying a playhead by a
    // rate in this file would be a unit conversion on the wrong side of the
    // boundary, and it would be wrong the first time a project held two
    // captures at different rates.
    const window = peaks.endSeconds - peaks.startSeconds;
    if (playing && window > 0) {
      const at = (playhead - peaks.startSeconds) / window;
      const x = at * element.width;
      if (x >= 0 && x <= element.width) {
        context.fillStyle = COLOURS.playhead;
        context.fillRect(Math.floor(x), 0, 1, height);
      }
    }
  }, [peaks, boundaries, playhead, playing]);

  if (capture === undefined) {
    return (
      <section className="waveform empty">
        <p>Nothing captured yet.</p>
      </section>
    );
  }

  return (
    <section className="waveform">
      <div className="waveform-box" ref={box}>
        <canvas
          ref={canvas}
          width={width}
          height={160}
          onClick={(event) => {
            if (peaks === null) {
              return;
            }
            const bounds = event.currentTarget.getBoundingClientRect();
            const fraction = (event.clientX - bounds.left) / bounds.width;
            onSeek(
              peaks.startSeconds +
                fraction * (peaks.endSeconds - peaks.startSeconds),
            );
          }}
        />
      </div>
      <div className="waveform-foot">
        <span>
          channel {view.channel + 1} of {capture.channels}
        </span>
        {peaks !== null && (
          <span>
            {clock(peaks.startSeconds)} to {clock(peaks.endSeconds)}
          </span>
        )}
        <span>{width} column(s)</span>
        {failed !== null && <span className="clip">{failed}</span>}
      </div>
    </section>
  );
}
