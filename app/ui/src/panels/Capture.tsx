/*
 *  Capture.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Choosing a device and a format, arming it, and what the capture reported (§8, §9).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The panel where §9 becomes visible. A device row shows the rates, formats and
// channel counts the host *enumerated*, and the `armed` event shows what was
// actually negotiated, and the two are displayed separately on purpose: the
// CPAL finding behind that is that a host's own report of its supported formats
// cannot be trusted, and a UI that showed only the enumeration would be
// repeating the lie. `osVerified` on the capture row is the third opinion, taken
// from the driver rather than from the library.
//
// `problems` on a device row is the shell's warning list and is shown as
// written. Nothing here decides whether a device is usable - a device with a
// problem is still armable, because "this interface reports one channel" is
// information a person may already know and have a reason to accept.
//
// # The unfinished capture
//
// WP-07 left open what the transport does with a recovered project. It shows
// it, and it does nothing automatically. A capture whose `state` is `recovered`
// or `interrupted` gets a line saying so and how much of it survived; it is not
// resumed, not deleted and not silently continued into. Appending to a capture
// that stopped for an unknown reason would be the one operation that could lose
// a rip, and "here is what we found, play it and decide" is the honest offer.

import { useEffect, useRef, useState } from "react";

import * as api from "../api";
import type { Capture as CaptureRow, Device, Settings } from "../bindings/vcw";
import { useKeys } from "../keys";
import { clock } from "../format";
import type { Store } from "../store";

/**
 * The device the host itself would pick, if it offers one that can record.
 *
 * Not the same as "no device": `null` arms the simulated source, which is how
 * the transport is driven with nothing plugged in.
 */
function defaultInput(devices: readonly Device[]): string | null {
  return devices.find((row) => row.canCapture && row.isDefaultInput)?.id ?? null;
}

/** The capture workspace. */
export function Capture({
  store,
  devices,
  settings,
}: {
  store: Store;
  devices: readonly Device[];
  settings: Settings | null;
}): React.JSX.Element {
  const { engine, project } = store;
  const [device, setDevice] = useState<string | null>(null);
  const [rate, setRate] = useState<number | null>(null);
  const [format, setFormat] = useState<string | null>(null);
  const list = useRef<HTMLSelectElement | null>(null);
  const formats = useRef<HTMLSelectElement | null>(null);

  // The settings are the default and the panel is the override, which is why
  // this runs once per settings load and not on every render: a person who
  // picked a device in this panel should not have it replaced by §39's when
  // something else causes a re-read.
  //
  // The fallback to the host's own default input is WP-19's: `device: null` is
  // the *simulated source* in `contract::command::Arm`, so a first run with
  // nothing pinned in §39 and nothing chosen here armed a synthetic tone and
  // showed full-scale meters. Found by arming the packaged AppImage. The row
  // said "simulated source" and the picker said "Host default", which is two
  // names for one value and only one of them true.
  useEffect(() => {
    if (settings !== null) {
      setDevice((held) => held ?? settings.audio.input ?? defaultInput(devices));
      setRate((held) => held ?? settings.audio.rate);
      setFormat((held) => held ?? settings.audio.format);
    }
  }, [settings, devices]);

  const chosen = devices.find((row) => row.id === device);
  const arm = () => {
    if (project.path === null) {
      return;
    }
    void store.run(() =>
      api.arm({
        device,
        project: project.path ?? "",
        rate,
        channels: null,
        format,
        mode: settings?.audio.mode ?? null,
        ringMillis: settings?.audio.ringMillis ?? null,
      }),
    );
  };

  useKeys("capture", {
    arm,
    chooseDevice: () => list.current?.focus(),
    chooseFormat: () => formats.current?.focus(),
  });

  return (
    <section className="panel capture">
      <header className="panel-head">
        <h2>Capture</h2>
      </header>

      {project.path === null && (
        <p className="empty">Open a project before arming a device.</p>
      )}

      <div className="fields">
        <label>
          Device (d)
          <select
            ref={list}
            value={device ?? ""}
            onChange={(event) =>
              setDevice(event.target.value === "" ? null : event.target.value)
            }
          >
            <option value="">Simulated source (no device)</option>
            {devices
              .filter((row) => row.canCapture)
              .map((row) => (
                <option key={row.id} value={row.id}>
                  {row.name} ({row.host}
                  {row.isDefaultInput ? ", default" : ""})
                </option>
              ))}
          </select>
        </label>

        <label>
          Rate (f)
          <select
            ref={formats}
            value={rate === null ? "" : String(rate)}
            onChange={(event) =>
              setRate(event.target.value === "" ? null : Number(event.target.value))
            }
          >
            <option value="">Device default</option>
            {(chosen?.captureRates ?? []).map((hz) => (
              <option key={hz} value={hz}>
                {hz} Hz
              </option>
            ))}
          </select>
        </label>

        <label>
          Format
          <select
            value={format ?? ""}
            onChange={(event) =>
              setFormat(event.target.value === "" ? null : event.target.value)
            }
          >
            <option value="">Device default</option>
            {(chosen?.captureFormats ?? []).map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>

        {/* With the three fields it acts on, and not in the panel header at
            the far right of the window. Arming is the last thing a person does
            in this row - pick a device, pick a rate, pick a format, arm - and
            the button was a full screen width away from the last of them,
            diagonally opposite Record. `.fields` aligns to the bottom of its
            controls, so it lands on the same line as the selects. */}
        <button type="button" disabled={project.path === null} onClick={arm}>
          Arm (a)
        </button>
      </div>

      {chosen !== undefined && chosen.problems.length > 0 && (
        <ul className="problems">
          {chosen.problems.map((problem) => (
            <li key={problem}>{problem}</li>
          ))}
        </ul>
      )}

      <dl className="negotiated">
        <dt>Negotiated</dt>
        <dd>
          {engine.negotiated ?? "not armed"}
          {engine.verified && <span className="ok"> os-confirmed</span>}
        </dd>
        {engine.divergences.length > 0 && (
          <>
            <dt>Differs from what was asked</dt>
            <dd>
              <ul>
                {engine.divergences.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            </dd>
          </>
        )}
        {engine.diagnostics !== null && (
          <>
            <dt>Last capture</dt>
            <dd>
              {engine.diagnostics.overruns} overrun(s),{" "}
              {engine.diagnostics.droppedFrames} dropped frame(s),{" "}
              {engine.diagnostics.streamErrors} stream error(s)
            </dd>
          </>
        )}
      </dl>

      <h3>Captures</h3>
      {project.captures.length === 0 ? (
        <p className="empty">Nothing captured in this project yet.</p>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th className="n">#</th>
              <th>Format</th>
              <th>Device</th>
              <th className="n">Length</th>
              <th>State</th>
            </tr>
          </thead>
          <tbody>
            {project.captures.map((row) => (
              <tr key={row.id} className={unfinished(row) ? "problem" : ""}>
                <td className="n">{row.id}</td>
                <td>
                  {row.rate} Hz {row.format} &times;{row.channels}
                  {row.osVerified ? "" : " (unverified)"}
                </td>
                <td>{row.device ?? "unknown"}</td>
                <td className="n">{clock(row.seconds)}</td>
                <td>{row.state}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {project.captures.filter(unfinished).map((row) => (
        <p className="recovered" key={row.id}>
          Capture {row.id} was not closed cleanly ({row.state}).{" "}
          {clock(row.seconds)} survived and is playable. Nothing has been
          resumed or removed: play it, and decide.
        </p>
      ))}
    </section>
  );
}

/** Whether a capture row stopped for a reason nobody chose (§37). */
function unfinished(row: CaptureRow): boolean {
  return row.state === "recovered" || row.state === "interrupted";
}
