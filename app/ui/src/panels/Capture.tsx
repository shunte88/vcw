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

import { useRef, useState } from "react";

import * as api from "../api";
import type { Capture as CaptureRow, Device } from "../bindings/vcw";
import { key, useText } from "../i18n";
import { useKeys } from "../keys";
import { type Choice, canArm } from "../arm";
import { clock } from "../format";
import type { Store } from "../store";

/**
 * The catalog key for what each capture state is called.
 *
 * Keyed by the wire word rather than by `CaptureStateName`, and falling back
 * to that word, because the row carries a string: a state this build has
 * never heard of shows as the shell wrote it rather than as a dotted key.
 */
/**
 * What each §51 provenance reads as.
 *
 * Keyed and defaulted like `STATES`, and for the same reason: the row carries
 * a string, and a value this build has not heard of shows as it was written.
 */
const PROVENANCE: Record<string, string> = {
  flat: key("capture.eq.flat"),
  riaa: key("capture.eq.riaa"),
  unknown: key("capture.eq.unknown"),
};

const STATES: Record<string, string> = {
  recording: key("capture.state.recording"),
  finalised: key("capture.state.finalised"),
  interrupted: key("capture.state.interrupted"),
  recovered: key("capture.state.recovered"),
  unknown: key("capture.state.unknown"),
};

/** The capture workspace. */
export function Capture({
  store,
  devices,
  choice,
  onChoice,
  arm,
}: {
  store: Store;
  devices: readonly Device[];
  /** What is pinned for the next arm. Held at the root - see `arm.ts`. */
  choice: Choice;
  /** Change one of the three. */
  onChoice: (choice: Choice) => void;
  /** Open the device. The toolbar's Set Levels button calls this same one. */
  arm: () => void;
}): React.JSX.Element {
  const text = useText();
  const { engine, project } = store;
  // What is being typed into an operator box, by capture id. The row is the
  // truth once it is written; this exists only so a half-typed name survives
  // a reload landing mid-word.
  const [operators, setOperators] = useState<Record<number, string>>({});
  const { device, rate, format } = choice;
  const list = useRef<HTMLSelectElement | null>(null);
  const formats = useRef<HTMLSelectElement | null>(null);

  const chosen = devices.find((row) => row.id === device);

  useKeys("capture", {
    arm,
    chooseDevice: () => list.current?.focus(),
    chooseFormat: () => formats.current?.focus(),
  });

  return (
    <section className="panel capture">
      <header className="panel-head">
        <h2>{text("capture.title")}</h2>
      </header>

      {project.path === null && (
        <p className="empty">{text("capture.no.project")}</p>
      )}

      <div className="fields">
        <label>
          {text("capture.device", { chord: "d" })}
          <select
            ref={list}
            value={device ?? ""}
            onChange={(event) =>
              onChoice({
                ...choice,
                device: event.target.value === "" ? null : event.target.value,
              })
            }
          >
            <option value="">{text("capture.simulated")}</option>
            {devices
              .filter((row) => row.canCapture)
              .map((row) => (
                <option key={row.id} value={row.id} title={row.id}>
                  {label(row)}
                  {row.isDefaultInput ? text("capture.default.suffix") : ""}
                </option>
              ))}
          </select>
        </label>

        <label>
          {text("capture.rate", { chord: "f" })}
          <select
            ref={formats}
            value={rate === null ? "" : String(rate)}
            onChange={(event) =>
              onChoice({
                ...choice,
                rate:
                  event.target.value === ""
                    ? null
                    : Number(event.target.value),
              })
            }
          >
            <option value="">{text("capture.device.default")}</option>
            {(chosen?.captureRates ?? []).map((hz) => (
              <option key={hz} value={hz}>
                {hz} Hz
              </option>
            ))}
          </select>
        </label>

        <label>
          {text("capture.format")}
          <select
            value={format ?? ""}
            onChange={(event) =>
              onChoice({
                ...choice,
                format: event.target.value === "" ? null : event.target.value,
              })
            }
          >
            <option value="">{text("capture.device.default")}</option>
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
        <button
          type="button"
          disabled={project.path === null || !canArm(engine.phase)}
          onClick={arm}
        >
          {text("capture.arm", { chord: "a" })}
        </button>
      </div>

      {chosen !== undefined && <Detail device={chosen} />}

      {chosen !== undefined && chosen.problems.length > 0 && (
        <ul className="problems">
          {chosen.problems.map((problem) => (
            <li key={problem}>{problem}</li>
          ))}
        </ul>
      )}

      <dl className="negotiated">
        <dt>{text("capture.negotiated")}</dt>
        <dd>
          {engine.negotiated ?? text("capture.not.armed")}
          {engine.verified && (
            <span className="ok"> {text("capture.os.confirmed")}</span>
          )}
        </dd>
        {engine.divergences.length > 0 && (
          <>
            <dt>{text("capture.divergences")}</dt>
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
            <dt>{text("capture.last")}</dt>
            <dd>
              {text("capture.counters", {
                overruns: engine.diagnostics.overruns,
                dropped: engine.diagnostics.droppedFrames,
                errors: engine.diagnostics.streamErrors,
              })}
            </dd>
          </>
        )}
      </dl>

      <h3>{text("capture.list")}</h3>
      {project.captures.length === 0 ? (
        <p className="empty">{text("capture.list.empty")}</p>
      ) : (
        <table className="rows">
          <thead>
            <tr>
              <th className="n">#</th>
              <th>{text("capture.column.format")}</th>
              <th>{text("capture.column.device")}</th>
              <th className="n">{text("capture.column.length")}</th>
              <th>{text("capture.column.state")}</th>
              <th>{text("capture.column.eq")}</th>
              <th>{text("capture.column.operator")}</th>
            </tr>
          </thead>
          <tbody>
            {project.captures.map((row) => (
              <tr key={row.id} className={unfinished(row) ? "problem" : ""}>
                <td className="n">{row.id}</td>
                <td>
                  {row.rate} Hz {row.format} &times;{row.channels}
                  {row.osVerified ? "" : ` ${text("capture.unverified")}`}
                </td>
                <td>{row.device ?? text("capture.device.unknown")}</td>
                <td className="n">{clock(row.seconds)}</td>
                <td>{text(STATES[row.state] ?? row.state)}</td>
                {/* §51: the one fact about a capture that cannot be read back
                    out of its samples, and the one an import never has -
                    Audacity does not store it. A curve over an `unknown`
                    capture is refused, so without this the window could name a
                    characteristic in the project panel and never be allowed to
                    hear it. */}
                <td>
                  <span className="picker">
                    <select
                      value={row.captureEq}
                      title={text("capture.eq.title")}
                      onChange={(event) =>
                        void store
                          .run(() =>
                            api.setProvenance({
                              captureId: row.id,
                              eq: event.target.value,
                              operator: null,
                              transferredOn: null,
                            }),
                          )
                          // Nothing publishes an event for this, so the row is
                          // re-read rather than left showing what it said
                          // before - a picker that snaps back to "Not stated"
                          // after a successful write reads as a refusal.
                          .then(store.reload)
                      }
                    >
                      {Object.entries(PROVENANCE).map(([value, label]) => (
                        <option key={value} value={value}>
                          {text(label)}
                        </option>
                      ))}
                    </select>
                  </span>
                </td>
                {/* WP-31: who made this transfer, where that is not the
                    operator in Settings. Empty rather than pre-filled with the
                    settings answer - an override that starts life filled in is
                    one nobody notices is wrong. */}
                <td>
                  <input
                    value={operators[row.id] ?? row.operator ?? ""}
                    title={text("capture.operator.title")}
                    onChange={(event) =>
                      setOperators({
                        ...operators,
                        [row.id]: event.target.value,
                      })
                    }
                    onBlur={(event) => {
                      if (event.target.value === (row.operator ?? "")) return;
                      void store
                        .run(() =>
                          api.setProvenance({
                            captureId: row.id,
                            eq: row.captureEq,
                            operator: event.target.value,
                            transferredOn: null,
                          }),
                        )
                        .then(store.reload);
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") event.currentTarget.blur();
                    }}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {project.captures.filter(unfinished).map((row) => (
        <p className="recovered" key={row.id}>
          {text("capture.recovered", {
            id: row.id,
            state: text(STATES[row.state] ?? row.state),
            length: clock(row.seconds),
          })}
        </p>
      ))}
    </section>
  );
}

/**
 * How a device reads in the menu.
 *
 * Name, how it is attached, and what sits between it and the converter -
 * because on ALSA the name alone does not pick anything out. A piCorePlayer
 * user looking at `arecord -L` sees one USB card behind eight entries, all of
 * them saying "USB AUDIO CODEC", and only one of them the direct hardware
 * path a rip wants. The menu has to say which that is.
 *
 * Exported for its test. The host is left off: it is the same word on every
 * row of a list a person is choosing within, and it is in the detail below.
 */
export function label(device: Device): string {
  return [device.name, device.interface, device.transport]
    .map((part) => part.trim())
    // "Unknown" as well as empty, and it is not a translated word: the shell
    // writes the literal when the host offers no interface type at all, which
    // on ALSA is every plug PCM. A row reading "- Unknown -" in the middle is
    // a word of noise in the one line a person is scanning, and the detail
    // below says everything the row left out.
    .filter((part) => part !== "" && part !== "Unknown")
    .join(" - ");
}

/**
 * A run of channel counts, collapsed.
 *
 * An ALSA plug PCM advertises every count from 1 to 64, and sixty-four
 * numbers across the panel bury the line they are in. The CLI's `devices`
 * printer collapses the same list for the same reason.
 */
export function counts(channels: readonly number[]): string {
  const runs: string[] = [];
  for (let at = 0; at < channels.length; at += 1) {
    const first = channels[at]!;
    let last = first;
    while (at + 1 < channels.length && channels[at + 1] === last + 1) {
      at += 1;
      last = channels[at]!;
    }
    runs.push(
      first === last
        ? String(first)
        : last === first + 1
          ? `${first}, ${last}`
          : `${first}-${last}`,
    );
  }
  return runs.join(", ");
}

/**
 * Everything the picker could not fit on one line.
 *
 * The identity first, because that is the string §39 persists and the string
 * `vcw devices` prints, and a person comparing the two needs to see the same
 * characters in both places. `arecord -L` is the standard a Linux user will
 * hold this against, and it names the PCM; so does this.
 */
function Detail({ device }: { device: Device }) {
  const text = useText();
  return (
    <dl className="device">
      <dt>{text("capture.device.id")}</dt>
      <dd>
        <code>{device.id}</code>
      </dd>
      <dt>{text("capture.device.host")}</dt>
      <dd>{device.host}</dd>
      <dt>{text("capture.device.path")}</dt>
      <dd>{device.transport}</dd>
      {device.captureChannels.length > 0 && (
        <>
          <dt>{text("capture.device.channels")}</dt>
          <dd>{counts(device.captureChannels)}</dd>
        </>
      )}
      {device.captureRates.length > 0 && (
        <>
          <dt>{text("capture.device.rates")}</dt>
          <dd>{device.captureRates.join(" ")} Hz</dd>
        </>
      )}
      {device.captureFormats.length > 0 && (
        <>
          <dt>{text("capture.device.formats")}</dt>
          <dd>{device.captureFormats.join(" ")}</dd>
        </>
      )}
    </dl>
  );
}

/** Whether a capture row stopped for a reason nobody chose (§37). */
function unfinished(row: CaptureRow): boolean {
  return row.state === "recovered" || row.state === "interrupted";
}
