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

import type { Capture as Row, Device, Settings } from "../bindings/vcw";
import type { Store } from "../store";
import { useArm } from "../arm";
import { Capture, counts, label } from "./Capture";

vi.mock("../api", () => ({
  arm: vi.fn(async () => undefined),
  setProvenance: vi.fn(async () => undefined),
}));

function device(
  id: string,
  isDefaultInput: boolean,
  extra: Partial<Device> = {},
): Device {
  return {
    id,
    name: id,
    host: "alsa",
    interface: "USB",
    transport: "direct hardware",
    canCapture: true,
    isDefaultInput,
    isDefaultOutput: false,
    captureRates: [48000, 96000],
    captureFormats: ["s32"],
    captureChannels: [1, 2],
    problems: [],
    ...extra,
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

function capture(eq: string, operator: string | null = null): Row {
  return {
    id: 7,
    rate: 96000,
    format: "s32",
    channels: 2,
    seconds: 1200,
    device: "alsa:default",
    state: "finished",
    osVerified: true,
    captureEq: eq,
    operator,
    transferredOn: null,
  } as unknown as Row;
}

function store(path: string | null = null, captures: readonly Row[] = []): Store {
  return {
    engine: {
      // The arm button reads it: a panel whose engine has no phase cannot
      // arm, which is right, and was how this fixture first failed.
      phase: "idle",
      negotiated: null,
      verified: false,
      divergences: [],
      diagnostics: null,
    },
    project: { path, captures },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
    reload: () => {},
    open: async () => undefined,
  } as unknown as Store;
}

/**
 * The panel with the arming the root does for it.
 *
 * `useArm` holds the three pickers and builds the request, because the
 * toolbar's Set Levels button issues the same one. Rendering the hook here
 * rather than handing the panel three literal props is what keeps the tests
 * below honest: they are about a settings value reaching `api.arm`, and a
 * fixture that supplied the answer itself would prove only that the fixture
 * works.
 */
function Harness({
  store,
  devices,
  settings,
}: {
  store: Store;
  devices: readonly Device[];
  settings: Settings | null;
}): React.JSX.Element {
  const { choice, setChoice, arm } = useArm(store, devices, settings);
  return (
    <Capture
      store={store}
      devices={devices}
      choice={choice}
      onChoice={setChoice}
      arm={arm}
    />
  );
}

async function render(
  devices: readonly Device[],
  held: Settings = settings(),
  path: string | null = null,
  captures: readonly Row[] = [],
): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <Harness store={store(path, captures)} devices={devices} settings={held} />,
    );
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

describe("what the device picker says about a device", () => {
  // A piCorePlayer user reading `arecord -L` sees one USB card behind eight
  // PCM names, every one of them "USB AUDIO CODEC", and only one of them the
  // direct hardware path a rip wants. A menu row reading just the name picks
  // nothing out, and the identity - the string §39 persists and `vcw devices`
  // prints - was on screen nowhere at all.

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  });

  it("names the attachment and the path, not just the device", () => {
    expect(
      label(device("alsa:hw:CARD=CODEC,DEV=0", false, {
        name: "USB AUDIO CODEC, USB Audio",
      })),
    ).toBe("USB AUDIO CODEC, USB Audio - USB - direct hardware");
  });

  it("leaves out what the host did not say", () => {
    expect(
      label(device("alsa:default", false, {
        name: "pCP_Mixer",
        interface: "",
        transport: "",
      })),
    ).toBe("pCP_Mixer");
    // And "Unknown", which is what the shell writes for a host that reports
    // no interface type - every ALSA plug PCM on the machine this was found
    // on. It reads as a word in the row and carries nothing.
    expect(
      label(device("alsa:default", false, {
        name: "Default ALSA Output",
        interface: "Unknown",
        transport: "virtual",
      })),
    ).toBe("Default ALSA Output - virtual");
  });

  it("collapses a run of channel counts", () => {
    // An ALSA plug PCM advertises every count from 1 to 64.
    expect(counts([1, 2])).toBe("1, 2");
    expect(counts([2])).toBe("2");
    expect(counts(Array.from({ length: 64 }, (_, at) => at + 1))).toBe("1-64");
    expect(counts([1, 2, 6, 8])).toBe("1, 2, 6, 8");
  });

  it("puts the identity on screen for whichever device is chosen", async () => {
    const container = await render([
      device("alsa:hw:CARD=CODEC,DEV=0", true, {
        name: "USB AUDIO CODEC, USB Audio",
      }),
    ]);
    const detail = container.querySelector("dl.device");
    expect(detail?.querySelector("code")?.textContent).toBe(
      "alsa:hw:CARD=CODEC,DEV=0",
    );
    const text = detail?.textContent ?? "";
    expect(text).toContain("direct hardware");
    expect(text).toContain("48000 96000 Hz");
    expect(text).toContain("s32");
  });

  it("says nothing about a device when the simulated source is chosen", async () => {
    expect((await render([])).querySelector("dl.device")).toBeNull();
  });
});

describe("the chain a capture was taken through", () => {
  // §51 refuses a curve over an `unknown` capture, and every imported project
  // is unknown. Without a control here the window can state a characteristic
  // in the project panel and then never be allowed to hear it - the CLI's
  // `vcw eq provenance` would be the only way out of a project the window
  // opened. The verb exists; this asserts there is a noun to reach it with.

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.mocked(api.setProvenance).mockClear();
  });

  it("shows what each capture states, per row", async () => {
    const container = await render([], settings(), "/data2/rips/side-a.vcw", [
      capture("unknown"),
    ]);
    const select = container.querySelector("table.rows select") as HTMLSelectElement | null;
    expect(select, "the capture row has a chain picker").toBeTruthy();
    expect(select?.value).toBe("unknown");
    // All three of §51's answers are reachable, including the one that says
    // nobody knows - a person who guessed wrong has to be able to take it back.
    expect([...(select?.options ?? [])].map((o) => o.value).sort()).toEqual([
      "flat",
      "riaa",
      "unknown",
    ]);
  });

  it("states it against the capture it belongs to", async () => {
    const container = await render([], settings(), "/data2/rips/side-a.vcw", [
      capture("unknown"),
    ]);
    const select = container.querySelector("table.rows select") as HTMLSelectElement;
    await act(async () => {
      select.value = "flat";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    // The capture's own id, not the first row's: a project holds a capture per
    // side and the two can have been taken through different preamps.
    expect(vi.mocked(api.setProvenance)).toHaveBeenCalledWith({
      captureId: 7,
      eq: "flat",
      operator: null,
      transferredOn: null,
    });
  });

  // WP-31's override. Settings carries the operator a session is normally
  // run by; this box is for the side somebody else cut, and it is empty when
  // it agrees - an override that starts life pre-filled is one nobody notices
  // is wrong, and it would also write the default back into every row that
  // was only tabbed through.
  it("asks who made the transfer, empty when nobody has said", async () => {
    const container = await render([], settings(), "/data2/rips/side-a.vcw", [
      capture("riaa"),
    ]);
    const box = container.querySelector<HTMLInputElement>("table.rows input");
    expect(box, "the capture row has an operator box").toBeTruthy();
    expect(box?.value).toBe("");
    await act(async () => {
      box?.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    expect(
      vi.mocked(api.setProvenance),
      "leaving an untouched box states nothing",
    ).not.toHaveBeenCalled();
  });

  it("states the operator against the capture, keeping its chain", async () => {
    const container = await render([], settings(), "/data2/rips/side-a.vcw", [
      capture("riaa", "A. Librarian"),
    ]);
    const box = container.querySelector<HTMLInputElement>("table.rows input");
    expect(box?.value, "what the capture already states is in the box").toBe(
      "A. Librarian",
    );
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(box, "B. Archivist");
      box?.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => {
      box?.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    // The chain rides along unchanged: the command states both, so sending
    // the row's own `riaa` is what stops an operator edit clearing it.
    expect(vi.mocked(api.setProvenance)).toHaveBeenCalledWith({
      captureId: 7,
      eq: "riaa",
      operator: "B. Archivist",
      transferredOn: null,
    });
  });
});
