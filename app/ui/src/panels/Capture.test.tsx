/*
 *  Capture.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The device nothing chose is the host's own input, not a synthetic tone.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// `Arm.device: null` means the *simulated source* - the contract says so, and
// that is how the transport is driven on a machine with nothing plugged in.
// The panel then offered `null` as its first choice and called it "Host
// default", so a first run with nothing pinned in §39 armed a tone generator
// and drew full-scale meters. Found by arming the packaged AppImage, which is
// exactly the state a new install is in.
//
// Two things are asserted: the empty choice says what it is, and a panel given
// devices picks the one the host calls its default input.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Device, Settings } from "../bindings/vcw";
import type { Store } from "../store";
import { Capture } from "./Capture";

vi.mock("../api", () => ({
  arm: vi.fn(async () => undefined),
}));

function device(id: string, isDefaultInput: boolean): Device {
  return {
    id,
    name: id,
    host: "alsa",
    canCapture: true,
    isDefaultInput,
    isDefaultOutput: false,
    captureRates: [48000, 96000],
    captureFormats: ["s32"],
    problems: [],
  } as unknown as Device;
}

function settings(): Settings {
  return {
    audio: { input: null, output: null, rate: null, format: null, mode: null, ringMillis: null },
  } as unknown as Settings;
}

function store(): Store {
  return {
    engine: { negotiated: null, verified: false, divergences: [], diagnostics: null },
    project: { path: null, captures: [] },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
    reload: () => {},
    open: async () => undefined,
  } as unknown as Store;
}

async function render(devices: readonly Device[]): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<Capture store={store()} devices={devices} settings={settings()} />);
  });
  return container;
}

describe("the capture panel", () => {
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  });

  it("says what the empty choice actually arms", async () => {
    const container = await render([]);
    const empty = container.querySelector('option[value=""]');
    expect(empty?.textContent ?? "").toMatch(/simulated/i);
  });

  it("chooses the host's default input when nothing is pinned", async () => {
    const container = await render([device("alsa:hw:CARD=0,DEV=0", false), device("alsa:default", true)]);
    const select = container.querySelector("select");
    expect(select?.value).toBe("alsa:default");
  });

  it("leaves the simulated source selected when there is no device to record from", async () => {
    const container = await render([]);
    const select = container.querySelector("select");
    expect(select?.value).toBe("");
  });
});
