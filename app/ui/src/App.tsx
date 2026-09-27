/*
 *  App.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-15's smoke page: proof that the command and event surface works end to end.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// This is not the UI. WP-16 builds that, against the layout in §12 through §21.
//
// What this page is for is the claim WP-15 has to make: that a real webview can
// enumerate devices, arm a real device, record, watch the meters move, stop, and
// see the capture appear in the project - through the generated types and with
// no audio logic on this side of the boundary. Every number shown here arrived
// as an event or a view model; nothing is computed in this file, and that is the
// property worth keeping as it grows.

import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "./api";
import type { Capture, Device, Wire } from "./bindings/vcw";

/** How many events the log keeps. Old ones are dropped, not stored. */
const LOG_LIMIT = 200;

/** One line in the event log. */
type Line = { at: number; text: string; kind: string };

export function App() {
  const [devices, setDevices] = useState<Device[]>([]);
  const [chosen, setChosen] = useState<string | null>(null);
  const [project, setProject] = useState("/data2/vcw-scratch/wp15/demo.vcw");
  const [phase, setPhase] = useState("idle");
  const [frames, setFrames] = useState(0);
  const [peaks, setPeaks] = useState<number[]>([]);
  const [captures, setCaptures] = useState<Capture[]>([]);
  const [failure, setFailure] = useState<string | null>(null);
  const [lines, setLines] = useState<Line[]>([]);
  const log = useRef<HTMLDivElement>(null);

  // One subscription for the whole page, and one `switch` over the union. The
  // handlers below are the only place an event is read: a component that
  // listened for itself would be a second subscription to the same 60 Hz
  // stream, which S3 measured as the expensive mistake.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void api.onEvent((event: Wire) => {
      if (cancelled) {
        return;
      }
      setLines((previous) =>
        [...previous, { at: Date.now(), text: describe(event), kind: event.kind }].slice(
          -LOG_LIMIT,
        ),
      );
      switch (event.kind) {
        case "phase-change":
          setPhase(event.to);
          break;
        case "status":
          setPhase(event.phase);
          setFrames(event.frames);
          break;
        case "recording-position":
          setFrames(event.frames);
          break;
        case "meter-update":
          setPeaks(event.meter.channels.map((channel) => channel.peakDb));
          break;
        case "capture-finished":
          void api.captures().then(setCaptures);
          break;
        default:
          break;
      }
    }).then((off) => {
      if (cancelled) {
        off();
      } else {
        unlisten = off;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // The log scrolls itself, because a log that needs scrolling is a log nobody
  // reads.
  useEffect(() => {
    log.current?.scrollTo({ top: log.current.scrollHeight });
  }, [lines]);

  useEffect(() => {
    void api.devices().then(setDevices);
  }, []);

  const attempt = useCallback(async (what: () => Promise<unknown>) => {
    setFailure(null);
    try {
      await what();
    } catch (error) {
      const refusal = api.asFailure(error);
      setFailure(
        refusal.field ? `${refusal.field}: ${refusal.message}` : refusal.message,
      );
    }
  }, []);

  const inputs = devices.filter((device) => device.canCapture);
  const armed = phase !== "idle";

  return (
    <main>
      <h1>VCW - WP-15 shell smoke test</h1>

      {failure ? <p className="failure">refused: {failure}</p> : null}

      <h2>Devices ({inputs.length} can capture)</h2>
      <table>
        <thead>
          <tr>
            <th>Name</th>
            <th>Host</th>
            <th>Transport</th>
            <th>Rates</th>
            <th>Formats</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {inputs.map((device) => (
            <tr key={device.id}>
              <td>{device.name}</td>
              <td>{device.host}</td>
              <td>{device.transport}</td>
              <td>{device.captureRates.join(", ")}</td>
              <td>{device.captureFormats.join(", ")}</td>
              <td>
                <button
                  type="button"
                  disabled={chosen === device.id}
                  onClick={() => setChosen(device.id)}
                >
                  {chosen === device.id ? "chosen" : "choose"}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <h2>Transport - {phase}</h2>
      <p>
        <label>
          project{" "}
          <input
            size={48}
            value={project}
            onChange={(event) => setProject(event.target.value)}
          />
        </label>
      </p>
      <p>
        <button
          type="button"
          disabled={!chosen}
          onClick={() =>
            void attempt(() =>
              api.arm({
                device: chosen,
                project,
                rate: null,
                channels: null,
                format: null,
                mode: null,
                ringMillis: null,
              }),
            )
          }
        >
          arm
        </button>
        {(["record", "pause", "resume", "stop", "reset", "disarm"] as const).map(
          (verb) => (
            <button
              key={verb}
              type="button"
              disabled={!armed}
              onClick={() => void attempt(() => api.transport(verb))}
            >
              {verb}
            </button>
          ),
        )}
        <button type="button" onClick={() => void attempt(() => api.poll())}>
          poll
        </button>
      </p>
      <p>
        {frames.toLocaleString()} frame(s) committed
        {peaks.length > 0
          ? ` - peak ${peaks.map((db) => db.toFixed(1)).join(" / ")} dBFS`
          : ""}
      </p>

      <h2>Captures ({captures.length})</h2>
      <table>
        <thead>
          <tr>
            <th>#</th>
            <th>Seconds</th>
            <th>Rate</th>
            <th>Format</th>
            <th>State</th>
            <th>Verified</th>
          </tr>
        </thead>
        <tbody>
          {captures.map((capture) => (
            <tr key={capture.id}>
              <td>{capture.id}</td>
              <td>{capture.seconds.toFixed(2)}</td>
              <td>{capture.rate}</td>
              <td>{capture.format}</td>
              <td>{capture.state}</td>
              <td>{capture.osVerified ? "yes" : "no"}</td>
            </tr>
          ))}
        </tbody>
      </table>

      <h2>Events ({lines.length})</h2>
      <div className="log" ref={log}>
        {lines.map((line, index) => (
          <div key={`${line.at}-${index}`}>
            <span className="kind">{line.kind.padEnd(20)}</span>
            {line.text}
          </div>
        ))}
      </div>
    </main>
  );
}

/**
 * One line of text for an event.
 *
 * The `switch` is exhaustive over the union, which is the generated types
 * earning their place: a new event kind is a compile error here rather than a
 * silent gap in the log.
 */
function describe(event: Wire): string {
  switch (event.kind) {
    case "phase-change":
      return `${event.from} -> ${event.to}`;
    case "armed":
      return `${event.negotiated}${event.verified ? ", os-confirmed" : ""}${
        event.divergences.length > 0 ? ` (${event.divergences.join("; ")})` : ""
      }`;
    case "recording-position":
      return `${event.frames} frame(s), ${event.seconds.toFixed(2)} s`;
    case "meter-update":
      return event.meter.channels
        .map((channel) => `${channel.peakDb.toFixed(1)} dBFS`)
        .join(" / ");
    case "track-detected":
      return `${event.edge} at ${event.seconds.toFixed(2)} s, ${(
        event.confidence * 100
      ).toFixed(0)}% (${event.provenance})`;
    case "capture-warning":
      return `${event.code}: ${event.detail}`;
    case "capture-finished":
      return `capture ${event.captureId}, ${event.frames} frame(s), ${event.state}${
        event.bitPerfect ? ", bit-perfect" : ""
      }`;
    case "auditioning":
      return `${event.scope} on ${event.opened} (${event.conversion})`;
    case "playback-position":
      return `${event.seconds.toFixed(2)} s`;
    case "playback-finished":
      return `${event.frames} frame(s), ${event.underruns} underrun(s), ${event.fidelity}`;
    case "command-refused":
      return `${event.command} in ${event.phase}: ${event.reason}`;
    case "command-rejected":
      return `${event.command} in ${event.phase}`;
    case "status":
      return `${event.phase}, ${event.frames} frame(s)`;
    case "export-progress":
      return `${event.index}/${event.of} ${event.path}`;
    case "export-finished":
      return `${event.files} file(s), ${event.covers} cover(s), ${event.bytesWritten} byte(s)`;
    case "export-failed":
      return `after ${event.written} file(s): ${event.reason}`;
    case "closed":
      return "the engine has stopped";
  }
}
