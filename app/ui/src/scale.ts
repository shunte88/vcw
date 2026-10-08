/*
 *  scale.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  How big the interface is drawn, on a monitor the stylesheet did not expect.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The stylesheet is written in `rem`, so the whole window is one number: the
// root font size. 13px is right on a 1080p or a scaled 4K desktop and is a
// third of a millimeter of letter on an unscaled one, which is what a 4K panel
// at DPR 1 gives you - the window is correct, legible through a magnifying
// glass, and nothing in the application could change it.
//
// Kept in `localStorage` and not in `Settings`, for the same reason the
// library's list-or-tiles choice is: this is a property of the screen someone
// is sitting at, not of their library. The settings file is synced, copied
// between machines and read by the CLI, and none of those want to know how
// large this person's monitor is. `vcw.library.view` in `Browser.tsx` is the
// other, and the two below - the meter style and the button style - are here
// for the same reason.

/** The one base the stylesheet was measured against, in pixels. */
const BASE = 13;

/** The key in `localStorage`. */
const KEY = "vcw.scale";

/** The offered sizes, as a percentage of the stylesheet's own. */
export const SCALES: readonly number[] = [100, 125, 150, 175, 200, 250];

/**
 * The chosen scale, or 100 when there is none or the stored one is nonsense.
 *
 * Nonsense includes a value from a future version with more sizes in it: a
 * percentage that is not on the list is not applied, because a window drawn at
 * a size this build cannot set in its own settings panel is a window nobody
 * can get back.
 */
export function scaleOf(): number {
  const stored = Number(localStorage.getItem(KEY));
  return SCALES.includes(stored) ? stored : 100;
}

/** Draws the window at `percent`, and remembers it. */
export function applyScale(percent: number): void {
  localStorage.setItem(KEY, String(percent));
  document.documentElement.style.fontSize = `${(BASE * percent) / 100}px`;
}

// --- How the meters are drawn -------------------------------------------
//
// The third screen property, and here for the same two reasons as the scale:
// it is about the monitor someone is sitting at rather than about their
// library, and nothing outside the window needs to know it. `vcw --json`
// prints levels as numbers whatever this says.
//
// Why both kinds exist rather than one good one: a bar is a readout and a
// dial is an instrument, and they are good at different things. The bars show
// the peak, the hold and the clip count to a tenth of a decibel, which is
// what setting a level on a phono stage needs. The dial shows loudness
// *moving* - §50's "watch it through the side" is a glance from across the
// room at a needle, and a number cannot be read from across a room.

/** Which meter the bridge draws. */
export type MeterStyle = "bars" | "vu";

/** The key in `localStorage`. */
const METERS = "vcw.meters";

/** An event of our own, because `storage` does not fire in the tab that wrote. */
const CHANGED = "vcw:meters";

/** The chosen meter, or bars when there is none and when it is not one of ours. */
export function meterStyleOf(): MeterStyle {
  return localStorage.getItem(METERS) === "vu" ? "vu" : "bars";
}

/** Changes the meter everywhere, and remembers it. */
export function applyMeterStyle(style: MeterStyle): void {
  localStorage.setItem(METERS, style);
  window.dispatchEvent(new Event(CHANGED));
}

/** Calls `listener` when the meter style changes. Returns the unsubscribe. */
export function onMeterStyle(listener: () => void): () => void {
  window.addEventListener(CHANGED, listener);
  return () => window.removeEventListener(CHANGED, listener);
}

// --- Whether the buttons wear words or glyphs ---------------------------
//
// The fourth screen property. A person who knows the window wants the row
// compact and the pictures are enough; a person meeting it wants to read what
// each button does. Neither is a default the other can live with, so it is a
// switch - and like the three above, it is about this screen and not about
// the library, so nothing outside the window hears about it.
//
// Icons never replace the name in the accessibility tree: `button.iconic`
// hides the label visually and leaves it in the markup, so the button is
// still reachable by its name and still carries its chord in `title`.

/** Whether a button shows its word or its glyph. */
export type ButtonStyle = "text" | "icons";

/** The key in `localStorage`. */
const BUTTONS = "vcw.buttons";

/** An event of our own, for `CHANGED`'s reason. */
const BUTTONS_CHANGED = "vcw:buttons";

/** The chosen style, or text when there is none and when it is not one of ours. */
export function buttonStyleOf(): ButtonStyle {
  return localStorage.getItem(BUTTONS) === "icons" ? "icons" : "text";
}

/** Changes the buttons everywhere, and remembers it. */
export function applyButtonStyle(style: ButtonStyle): void {
  localStorage.setItem(BUTTONS, style);
  window.dispatchEvent(new Event(BUTTONS_CHANGED));
}

/** Calls `listener` when the button style changes. Returns the unsubscribe. */
export function onButtonStyle(listener: () => void): () => void {
  window.addEventListener(BUTTONS_CHANGED, listener);
  return () => window.removeEventListener(BUTTONS_CHANGED, listener);
}
