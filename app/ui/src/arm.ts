/*
 *  arm.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What the next capture will be opened with, and the one way of opening it.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Arming used to live in the Capture panel, with the three pickers it is made
// of, and that was right while the panel was the only way to do it. The
// toolbar's Set Levels button is the second, and two places building the same
// request out of two copies of the same three answers is how a person pins a
// device in the panel, presses the button on the bar and listens to a
// different one. So the answers and the request moved up to the root, which is
// where the devices and the settings already are, and both buttons now call
// the same function.
//
// A hook rather than a module-level store: this is window state and dies with
// the window, unlike the four in `scale.ts` which outlive it on purpose.

import { useEffect, useState } from "react";

import * as api from "./api";
import type { Device, Settings } from "./bindings/vcw";
import type { Store } from "./store";

/** The three things a person can pin before arming. `null` is "whatever §39 says". */
export type Choice = {
  device: string | null;
  rate: number | null;
  format: string | null;
};

/**
 * The device the host itself would pick, if it offers one that can record.
 *
 * Not the same as "no device": `null` arms the simulated source, which is how
 * the transport is driven with nothing plugged in.
 */
export function defaultInput(devices: readonly Device[]): string | null {
  return devices.find((row) => row.canCapture && row.isDefaultInput)?.id ?? null;
}

/**
 * Whether the transport can be armed from where it is.
 *
 * `stopped` counts, which is the whole point of it being a function. A
 * finished capture leaves the engine in `stopped`, and §35's state machine
 * only leaves that state on `reset` - so every button that opens a device was
 * refused for the rest of the session, and the window offered no verb that
 * would clear it. One side of a record and the app had to be restarted.
 * [`useArm`]'s `arm` sends the `reset` itself, so a caller only has to ask
 * whether arming is possible, not which of the two states it is in.
 */
export function canArm(phase: string): boolean {
  return phase === "idle" || phase === "stopped";
}

/** The pinned answers, a setter for them, and the one way to open the device. */
export function useArm(
  store: Store,
  devices: readonly Device[],
  settings: Settings | null,
): {
  choice: Choice;
  setChoice: (choice: Choice) => void;
  arm: () => Promise<void>;
} {
  const [choice, setChoice] = useState<Choice>({
    device: null,
    rate: null,
    format: null,
  });

  // The settings are the default and the pickers are the override, which is
  // why this runs once per settings load and not on every render: a person who
  // picked a device should not have it replaced by §39's when something else
  // causes a re-read.
  //
  // The fallback to the host's own default input is WP-19's: `device: null` is
  // the *simulated source* in `contract::command::Arm`, so a first run with
  // nothing pinned in §39 and nothing chosen here armed a synthetic tone and
  // showed full-scale meters. Found by arming the packaged AppImage. The row
  // said "simulated source" and the picker said "Host default", which is two
  // names for one value and only one of them true.
  useEffect(() => {
    if (settings !== null) {
      setChoice((held) => ({
        device: held.device ?? settings.audio.input ?? defaultInput(devices),
        rate: held.rate ?? settings.audio.rate,
        format: held.format ?? settings.audio.format,
      }));
    }
  }, [settings, devices]);

  // Returns its promise rather than firing and forgetting, because the
  // needle-drop trigger has to send `record` after the device is open and not
  // beside it. A caller with nothing to sequence ignores the result.
  const arm = async () => {
    // Both halves of the question in one place, because the fourth caller had
    // neither. The panel's button disables itself on `canArm`, the toolbar's
    // hides itself on it and the needle drop asks before it arms - and the
    // `a` chord, which is the same action and the one §44 promises, asked
    // nothing and sent an arm into a running capture for the engine to refuse.
    if (store.project.path === null || !canArm(store.engine.phase)) {
      return;
    }
    // Clear the finished capture out of the way first. The engine takes its
    // commands on one queue and applies them in order, so the `arm` below
    // lands after this even though neither call waits for the machine to
    // move - which is as close to atomic as the boundary gets, and closer
    // than reading the phase back would be.
    if (store.engine.phase === "stopped") {
      await store.run(() => api.transport("reset"));
    }
    await store.run(() =>
      api.arm({
        device: choice.device,
        project: store.project.path ?? "",
        rate: choice.rate,
        channels: null,
        format: choice.format,
        mode: settings?.audio.mode ?? null,
        eq: settings?.audio.eq ?? null,
        ringMillis: settings?.audio.ringMillis ?? null,
      }),
    );
  };

  return { choice, setChoice, arm };
}
