/*
 *  Transport.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Where the sound comes out, when it is not out of this machine.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// §52: "Audio auditioned through `serve` is rendered by the host, not by the
// browser ... The interface shall say so rather than appear to have failed."
//
// A requirement about silence needs a test, because silence is what a broken
// Play button also produces, and nothing in a screenshot tells the two apart.
// The window must not carry the note - it would be saying "elsewhere" about
// the speakers in the same room.

import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Capture } from "../bindings/vcw";
import type { Engine, Store } from "../store";
import { Transport } from "./Transport";
import { mount } from "../mount";

const inTheShell = vi.fn(() => true);

vi.mock("../host", () => ({
  inTheShell: () => inTheShell(),
}));

vi.mock("../api", () => ({
  play: vi.fn(async () => undefined),
  playback: vi.fn(async () => undefined),
  transport: vi.fn(async () => undefined),
  placeMarker: vi.fn(async () => undefined),
}));

function store(phase = "idle", path: string | null = "/library/a.vcw"): Store {
  return {
    project: { path },
    engine: {
      phase,
      playing: false,
      playhead: 0,
      seconds: 0,
      auditioning: null,
    } as unknown as Engine,
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
    reload: () => {},
  } as unknown as Store;
}

async function render(
  onSetLevels: (() => void) | null = null,
  phase = "idle",
  path: string | null = "/library/a.vcw",
  drop: { waiting: boolean; toggle: () => void } = {
    waiting: false,
    toggle: () => {},
  },
): Promise<HTMLElement> {
  return mount(
      <Transport
        store={store(phase, path)}
        capture={{ id: 1 } as unknown as Capture}
        side={null}
        selection={null}
        onSetLevels={onSetLevels}
        drop={drop}
      />,
    );
}

/** The toolbar's Set Levels button, if the strip is showing one. */
function levels(container: HTMLElement): HTMLButtonElement | undefined {
  return [...container.querySelectorAll("button")].find((button) =>
    (button.textContent ?? "").includes("Set Levels"),
  );
}

/** The toolbar's needle-drop trigger, whichever face it is wearing. */
function trigger(container: HTMLElement): HTMLButtonElement | undefined {
  return [...container.querySelectorAll("button")].find((button) =>
    /Needle Drop|Listening/.test(button.textContent ?? ""),
  );
}

describe("the transport strip", () => {
  beforeEach(() => {
    (
      globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    inTheShell.mockReturnValue(true);
  });

  it("says where the audio comes out when VCW is somewhere else", async () => {
    inTheShell.mockReturnValue(false);
    const container = await render();
    expect(container.textContent).toMatch(/Audio plays on the VCW host/);
  });

  it("says nothing of the sort in the window", async () => {
    const container = await render();
    expect(container.textContent).not.toMatch(/VCW host/);
  });

  // Opening the device without recording is how a level gets set, and until
  // now it could only be reached from the Capture panel - a tab away from
  // Record, which is the button pressed next. The root hands the strip the
  // same function that panel's own Arm button calls, and hands it over only
  // where it answers something: on Tracks there is no level to set.
  it("offers Set Levels only on the panel that asked for it", async () => {
    expect(levels(await render(null)), "not on the other panels").toBeUndefined();

    const arm = vi.fn();
    const container = await render(arm);
    const button = levels(container);
    expect(button, "on the capture panel").toBeDefined();
    await act(async () => {
      button?.click();
    });
    expect(arm).toHaveBeenCalledTimes(1);
  });

  // Nothing to offer once the device is open: the meters are already live.
  it("takes Set Levels away once something is armed", async () => {
    const container = await render(vi.fn(), "armed");
    expect(levels(container)).toBeUndefined();
  });

  // And nothing to arm without a project to record into.
  it("cannot set levels with no project open", async () => {
    const container = await render(vi.fn(), "idle", null);
    expect(levels(container)?.disabled).toBe(true);
  });

  // The trigger is a transport verb and lives on the bar beside Record, not
  // on the panel that happens to be in front. These three are the states a
  // person can see it in.
  it("offers the needle-drop trigger beside Record", async () => {
    const toggle = vi.fn();
    const container = await render(null, "idle", "/library/a.vcw", {
      waiting: false,
      toggle,
    });
    const button = trigger(container);
    expect(button?.textContent).toBe("Needle Drop");
    await act(async () => {
      button?.click();
    });
    expect(toggle).toHaveBeenCalledTimes(1);
  });

  it("says it is listening while it waits for the stylus", async () => {
    const container = await render(null, "paused", "/library/a.vcw", {
      waiting: true,
      toggle: () => {},
    });
    expect(trigger(container)?.textContent).toBe("Listening");
    expect(trigger(container)?.className).toBe("listening");
  });

  // Nothing left to trigger once the record is turning, and nothing to
  // record into without a project.
  it("has nothing to trigger while recording or without a project", async () => {
    expect(trigger(await render(null, "recording"))?.disabled).toBe(true);
    expect(trigger(await render(null, "idle", null))?.disabled).toBe(true);
  });
});
