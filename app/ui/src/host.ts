/*
 *  host.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  How a command reaches the backend: the Tauri shell, or HTTP under `vcw serve`.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// §52's whole frontend cost. `api.ts` is the only file that issues a command
// and the only file that subscribes to the event stream; this is the only file
// that knows how either one travels. Nothing above here can tell the
// difference, which is the property §2 was written to buy and the reason a
// remote interface is a transport rather than a second application.
//
// The choice is made at run time rather than at build time, deliberately:
// `app/ui/dist` is one artefact, baked into the desktop binary by the
// `custom-protocol` feature and served as files by `vcw serve`. Two builds of
// the frontend would be two things to keep in step, and the whole point is
// that there is one.

import { invoke as invokeTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { Wire } from "./bindings/vcw";

/** The single event channel every core event arrives on. See `pump.rs`. */
export const EVENT = "vcw://event";

/**
 * Are we inside the desktop shell?
 *
 * Tauri puts this on `window` before any of our code runs. Asking the question
 * this way rather than from a build flag is what keeps one `dist` serving both
 * deployments.
 */
export function inTheShell(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Send one command and wait for its answer.
 *
 * Rejects the way Tauri rejects - with the `Failure` the command serialized -
 * so `asFailure` reads both transports the same way.
 */
export async function invoke<T>(
  command: string,
  argument?: Record<string, unknown>,
): Promise<T> {
  if (inTheShell()) {
    return invokeTauri<T>(command, argument);
  }
  const answer = await fetch(`/command/${command}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    // Same origin, so the cookie holding the §52 token rides along without
    // this file ever seeing it. It is `HttpOnly` precisely so that it cannot.
    credentials: "same-origin",
    body: JSON.stringify(argument ?? {}),
  });
  const body = await answer.text();
  if (!answer.ok) {
    // A refusal is an answer: the body is the same `Failure` the shell would
    // have rejected with. Only when it is not - a proxy's error page, say -
    // does the status line become the message.
    throw parse(body) ?? {
      code: "transport",
      message: `the VCW host answered ${answer.status}`,
      field: null,
    };
  }
  // An empty body is a command that returned nothing, which is what `invoke`
  // gives for a `Result<(), _>`.
  //
  // Checked here rather than folded into `parse` with a `??`, which is what
  // this did first and which cost an hour of first light. `null` is a real
  // answer from three commands - `open_path`, `library_root` and `release` -
  // and `null ?? undefined` is `undefined`, so `openPath()` stopped matching
  // the `=== null` the store branches on. The window then asked for the
  // tracks of a project nobody had opened and put `no project is open` in the
  // status bar before a person had touched anything. Over Tauri the same code
  // was fine, because `invoke` hands back the `null` itself.
  if (body.trim() === "") {
    return undefined as T;
  }
  return parse(body) as T;
}

/** JSON, or nothing, without throwing on a body that is not JSON. */
function parse(body: string): unknown {
  if (body.trim() === "") {
    return undefined;
  }
  try {
    return JSON.parse(body);
  } catch {
    return undefined;
  }
}

/** Unsubscribes from the event stream. */
export type Unsubscribe = () => void;

/**
 * Subscribe to the one event stream.
 *
 * Under `serve` this is an `EventSource`, which reconnects by itself when the
 * host restarts or a socket is reaped - the behavior the desktop shell gets
 * for free by being in the same process.
 */
export async function subscribe(
  handler: (event: Wire) => void,
): Promise<Unsubscribe> {
  if (inTheShell()) {
    return listen<Wire>(EVENT, (message) => handler(message.payload));
  }
  const source = new EventSource("/events", { withCredentials: true });
  source.onmessage = (message: MessageEvent<string>) => {
    const wire = parse(message.data);
    if (wire !== undefined) {
      handler(wire as Wire);
    }
  };
  return () => source.close();
}
