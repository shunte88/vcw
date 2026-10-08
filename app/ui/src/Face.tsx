/*
 *  Face.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What a button shows: its word, or its glyph with the word kept off the page.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import type { ReactNode } from "react";
import { useSyncExternalStore } from "react";

import { buttonStyleOf, onButtonStyle } from "./scale";

/**
 * One button's face, in whichever style the Appearance panel is set to.
 *
 * Here rather than written out at each of the fourteen buttons for the
 * ordinary reason - the two rows must not end up able to disagree - and for
 * one that is particular to this: in icon mode the word has to survive
 * somewhere a screen reader can still find it, and a rule that is written out
 * fourteen times is a rule that will be written out thirteen times.
 *
 * `aria-hidden` on the glyph and a visually-hidden word rather than an
 * `aria-label` on the button, because the button's `title` already carries its
 * chord and an `aria-label` would hide that: a label overrides the accessible
 * name that the title would otherwise contribute to.
 */
export function Face({
  icon,
  children,
}: {
  /** The glyph's class, which is its file name in `assets/` without the suffix. */
  icon: string;
  /** The word, shown in text mode and kept off the page in icon mode. */
  children: ReactNode;
}) {
  const style = useSyncExternalStore(onButtonStyle, buttonStyleOf);
  if (style === "text") {
    return <>{children}</>;
  }
  return (
    <>
      <span className={`icon ${icon}`} aria-hidden="true" />
      <span className="btn-label">{children}</span>
    </>
  );
}
