/*
 *  curves.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The §51 curve catalog, fetched once and shared by the panels that offer it.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Three panels pick a characteristic now - the new-project form, the metadata
// panel's release facts and the per-side override on Tracks - and all three
// want the same list fetched the same way. Once rather than per mount, because
// the catalog is compiled into the build and cannot change while the window is
// up; the `live` flag is there for the unmount that happens before the promise
// lands, which is the ordinary case when somebody tabs straight past a panel.

import { useEffect, useState } from "react";

import * as api from "./api";
import type { Curve } from "./bindings/vcw";
import { type Translate } from "./i18n";

/** Every characteristic a record can be stated as cut to, empty until it lands. */
export function useCurves(): readonly Curve[] {
  const [curves, setCurves] = useState<readonly Curve[]>([]);
  useEffect(() => {
    let live = true;
    void api.curves().then((answer) => {
      if (live) {
        setCurves(answer);
      }
    });
    return () => {
      live = false;
    };
  }, []);
  return curves;
}

/**
 * One curve's name as a picker shows it, which is the name and its era.
 *
 * "Columbia LP (1948-1954)" rather than "Columbia LP": sixteen names with no
 * dates in front of them is a list nobody can choose from, and the era is the
 * one thing that narrows it.
 */
export function label(curve: Curve): string {
  return curve.era === null ? curve.name : `${curve.name} (${curve.era})`;
}

/** What the signal already carried when it reached the card (§51). */
export type Chain = "flat" | "riaa" | "unstated";

/**
 * The characteristic all but a handful of records were cut to.
 *
 * `vcw_eq::RIAA`, and the one curve id the window has to name: it is both the
 * preselected answer in the new-project form and what the Rust side falls
 * back to for a project that states nothing, so the form must not offer a
 * different default from the one playback would use. It is also the one curve
 * an ordinary phono stage has already undone, which is what [`help`] turns on.
 */
export const RIAA = "riaa";

/** `settings.audio.eq` as the three cases the window has advice for. */
export function chainOf(eq: string | null | undefined): Chain {
  return eq === "flat" || eq === RIAA ? eq : "unstated";
}

/**
 * The tracking help for one curve, under the chain the operator stated.
 *
 * Two sentences, and both of them are about the choice in front of the person
 * rather than about equalization in general: what the record was cut with and
 * who cut to it, then what VCW is going to do about it here. The second half
 * is the whole point of asking for the chain - "undo RIAA first, which lifts
 * the noise" is advice for somebody with a phono stage and is simply wrong
 * for somebody capturing flat, and a single paragraph that covers both says
 * neither. Generated rather than written out sixteen times, for the reason in
 * the `refusal-advice-must-be-generated` lesson: a curve added next year
 * would otherwise get a help text that was true about a different list.
 */
export function help(curve: Curve, chain: Chain, text: Translate): string {
  // The period is this function's, not the catalog's. `label` ends in a
  // closing bracket and the clauses are whole sentences, so joining them on a
  // bare space read as one run-on - "Columbia LP (June 1948 to 1956) Cut by
  // Columbia" - in the tooltip that is the only place this text appears.
  const cut =
    curve.labels === null
      ? `${label(curve)}.`
      : `${label(curve)}. ${text("library.new.eq.labels", { labels: curve.labels })}`;
  // Each branch calls `text` on a literal rather than choosing a key and
  // looking it up afterwards: the catalog coverage test reads the source for
  // a literal call and a key assembled in a variable is one it cannot see,
  // which it reports - correctly - as a string nothing asks for.
  const clause =
    chain === "flat"
      ? text("library.new.eq.chain.flat")
      : chain === "unstated"
        ? text("library.new.eq.chain.unstated")
        : curve.id === RIAA
          ? text("library.new.eq.chain.match")
          : text("library.new.eq.chain.undo");
  return `${cut} ${clause}`;
}
