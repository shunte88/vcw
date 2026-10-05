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
//
// The third is a different seam: §51's equalization provenance is settings-wide
// and only this panel can put it on an `arm`. It cannot be recovered from the
// audio afterwards, so a panel that quietly dropped it would make every capture
// taken through the window permanently 'unknown'.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";

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

function settings(audio: Record<string, unknown> = {}): Settings {
  return {
    audio: {
      input: null,
      output: null,
      rate: null,
      format: null,
      mode: null,
      ringMillis: null,
      eq: null,
      ...audio,
    },
  } as unknown as Settings;
}

function store(path: string | null = null): Store {
  return {
    engine: { negotiated: null, verified: false, divergences: [], diagnostics: null },
    project: { path, captures: [] },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
    reload: () => {},
    open: async () => undefined,
  } as unknown as Store;
}

async function render(
  devices: readonly Device[],
  held: Settings = settings(),
  path: string | null = null,
): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<Capture store={store(path)} devices={devices} settings={held} />);
  });
  return container;
}

/** Presses the panel's arm button and hands back the request it sent. */
async function armed(held: Settings): Promise<Record<string, unknown>> {
  const container = await render([], held, "/data2/rips/side-a.vcw");
  const button = [...container.querySelectorAll("button")].find((b) =>
    /^arm\b/i.test((b.textContent ?? "").trim()),
  );
  expect(button, "the panel has an arm button").toBeTruthy();
  await act(async () => {
    button?.click();
  });
  const mock = vi.mocked(api.arm);
  expect(mock).toHaveBeenCalledTimes(1);
  const [request] = mock.mock.calls[0] ?? [];
  expect(request, "the panel sent an arm request").toBeTruthy();
  return request as unknown as Record<string, unknown>;
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

  it("arms with the equalization the operator stated once", async () => {
    vi.mocked(api.arm).mockClear();
    expect((await armed(settings({ eq: "riaa" }))).eq).toBe("riaa");
  });

  it("says nothing about the curve when nothing was stated", async () => {
    vi.mocked(api.arm).mockClear();
    // Not "riaa". The contract reads null as unknown, and unknown is the truth
    // about a preamp nobody described.
    expect((await armed(settings())).eq).toBeNull();
  });

  it("leaves the simulated source selected when there is no device to record from", async () => {
    const container = await render([]);
    const select = container.querySelector("select");
    expect(select?.value).toBe("");
  });
});
