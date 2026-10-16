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
