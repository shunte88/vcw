/*
 *  arm.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A finished capture is not the end of the session: arming again clears it.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// §35's machine makes `stopped` terminal - a capture ends there and only
// `reset` leaves it - and nothing in the window ever sent one. So the second
// side of a record was impossible: Set Levels hid itself, Record, Pause and
// Needle Drop each earned a refusal, and the Arm button stayed lit and did
// nothing. The person who got there was the one who pressed Stop between
// sides instead of Pause, which is what Audacity teaches.
//
// This is the sequencing, not the predicate: `reset` has to be on the queue
// before `arm` is, or the engine refuses the arm exactly as it did before.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "./api";
import { canArm, useArm } from "./arm";
import type { Store } from "./store";

vi.mock("./api", () => ({
  arm: vi.fn(async () => undefined),
  transport: vi.fn(async () => undefined),
}));

function Harness({ store }: { store: Store }): React.JSX.Element {
  const { arm } = useArm(store, [], null);
  return (
    <button type="button" onClick={() => void arm()}>
      arm
    </button>
  );
}

function store(phase: string, path: string | null): Store {
  return {
    project: { path },
    engine: { phase },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
  } as unknown as Store;
}

/** Mounts the hook, presses the button, and takes the root down after. */
async function press(phase: string, path: string | null = "/library/a.vcw") {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<Harness store={store(phase, path)} />);
  });
  await act(async () => {
    container.querySelector("button")?.click();
  });
  await act(async () => {
    root.unmount();
  });
  container.remove();
}

describe("where the transport can be armed from", () => {
  it("counts a finished capture, because arming clears it", () => {
    expect(canArm("idle")).toBe(true);
    expect(canArm("stopped")).toBe(true);
  });

  it("does not count a device that is already open", () => {
    for (const phase of ["armed", "recording", "paused"]) {
      expect(canArm(phase)).toBe(false);
    }
  });
});

describe("arming", () => {
  beforeEach(() => {
    vi.mocked(api.arm).mockClear();
    vi.mocked(api.transport).mockClear();
  });

  it("resets a finished capture first, and in that order", async () => {
    await press("stopped");
    expect(api.transport).toHaveBeenCalledWith("reset");
    expect(api.arm).toHaveBeenCalledTimes(1);
    // The engine applies one queue in order, so this is the whole claim.
    const reset = vi.mocked(api.transport).mock.invocationCallOrder[0] ?? 0;
    const armed = vi.mocked(api.arm).mock.invocationCallOrder[0] ?? 0;
    expect(reset).toBeLessThan(armed);
  });

  it("sends nothing but the arm from idle", async () => {
    await press("idle");
    expect(api.transport).not.toHaveBeenCalled();
    expect(api.arm).toHaveBeenCalledTimes(1);
  });

  it("does neither without a project to record into", async () => {
    await press("stopped", null);
    expect(api.transport).not.toHaveBeenCalled();
    expect(api.arm).not.toHaveBeenCalled();
  });
});
