/*
 *  store.run.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A refusal returned from a command reaches the event log, not just the bar.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// There are two ways the engine says no, and only one of them used to be
// written down. A refusal published over the bus arrives as `command-refused`
// and the subscription folds it into the log. A refusal *returned* from a Tauri
// command rejects the promise instead, and `run` caught it into `engine.refusal`
// - the status bar - and nowhere else. So the panel a bug report is copied out
// of was silent about precisely the failures a bug report is about, and a
// refusal a person dismissed was gone.
//
// This renders the hook rather than reading a reducer, because `run` is the
// thing under test and it only exists inside one.

import { act } from "react";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { describe as group, expect, it, vi } from "vitest";

import type { Store } from "./store";
import { useEngine } from "./store";

vi.mock("./api", () => ({
  // No project, so the five reads never run.
  openPath: async () => null,
  onEvent: async () => () => {},
  asFailure: (error: unknown) => error,
}));

/** Mounts the hook and hands back the store it produced. */
async function mounted(): Promise<() => Store> {
  let latest: Store | null = null;
  function Probe(): null {
    latest = useEngine();
    return null;
  }
  const container = document.createElement("div");
  document.body.append(container);
  await act(async () => {
    createRoot(container).render(createElement(Probe));
  });
  return () => {
    if (latest === null) {
      throw new Error("the hook never rendered");
    }
    return latest;
  };
}

group("a command that refuses", () => {
  it("writes a line in the log as well as a line in the status bar", async () => {
    const store = await mounted();
    expect(store().engine.log).toHaveLength(0);

    await act(async () => {
      await store().run(async () => {
        throw { code: "invalid-argument", message: "no device is armed" };
      }, "start");
    });

    expect(store().engine.refusal?.message).toBe("no device is armed");
    expect(store().engine.log).toHaveLength(1);
    const line = store().engine.log[0];
    expect(line?.event.kind).toBe("command-refused");
    // The reason, whole: this line is what gets pasted into the report.
    expect(JSON.stringify(line?.event)).toContain("no device is armed");
    expect(JSON.stringify(line?.event)).toContain("start");
  });

  it("leaves the log alone when the command succeeds", async () => {
    const store = await mounted();
    await act(async () => {
      await store().run(async () => undefined, "start");
    });
    expect(store().engine.log).toHaveLength(0);
    expect(store().engine.refusal).toBeNull();
  });
});
