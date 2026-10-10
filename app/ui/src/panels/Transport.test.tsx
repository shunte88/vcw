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
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Capture } from "../bindings/vcw";
import type { Engine, Store } from "../store";
import { Transport } from "./Transport";

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

function store(): Store {
  return {
    engine: {
      phase: "idle",
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

async function render(): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <Transport
        store={store()}
        capture={{ id: 1 } as unknown as Capture}
        side={null}
        selection={null}
      />,
    );
  });
  return container;
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
});
