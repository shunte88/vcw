/*
 *  select.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Moving a selection through a list with the arrow keys.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The one piece of list behaviour three panels share, written once.
//
// WP-16a added it after first light. Every verb that acts on "the selected"
// row had a handler and a chord; nothing could move the selection, because the
// rows answered `onClick` and nothing else. Four lists needed the same answer -
// the projects, the tracks, the boundaries, the release candidates - and four
// copies of "where does ArrowDown go" is four chances to disagree about the
// empty list.
//
// It is not business logic. Nothing here knows what a track is: it takes the
// keys a panel already has, the one it has selected, and a direction.

/**
 * The key one step from `current`, in `items`.
 *
 * Clamps at both ends rather than wrapping. Holding `ArrowDown` should come to
 * rest on the last row and stay there - a list that jumps back to the top is a
 * list you cannot arrive at the bottom of, and on the tracks panel the bottom
 * row is the one a person is usually reaching for.
 *
 * With nothing selected the first press selects an end: down takes the top,
 * up takes the bottom. That is what makes a panel reachable from a standing
 * start, which is the whole point of the function - a selection that can only
 * begin with a mouse is the defect it was written to fix.
 *
 * A `current` that is not in `items` is treated as nothing selected, which is
 * the state a panel is left in when the row it had chosen was deleted or
 * renumbered underneath it.
 *
 * @param items The keys on show, in the order they are drawn.
 * @param current The selected key, or null.
 * @param delta -1 for up, 1 for down.
 * @returns The key to select, or null when there is nothing to select.
 */
export function step<T>(
  items: readonly T[],
  current: T | null,
  delta: -1 | 1,
): T | null {
  if (items.length === 0) {
    return null;
  }
  const at = current === null ? -1 : items.indexOf(current);
  // The `?? null` on both reads is `noUncheckedIndexedAccess` being satisfied,
  // not doubt: the empty case returned above, and every index below is clamped
  // into range. Written as a fallback rather than an assertion because a `!`
  // here would be the one place in the file a reader had to check by hand.
  if (at === -1) {
    return (delta > 0 ? items[0] : items[items.length - 1]) ?? null;
  }
  const next = Math.min(Math.max(at + delta, 0), items.length - 1);
  return items[next] ?? null;
}
