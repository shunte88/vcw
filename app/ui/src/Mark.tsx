/*
 *  Mark.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The brand mark, drawn small enough to sit in the title bar.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The same record as `app/src-tauri/icons/icon.svg` and the label on the record
// in `assets/vcw_logo_main.svg`, drawn a third time because this one has to
// work at fourteen pixels inside a line of text. Everything that makes the icon
// pleasant at 256 - the groove rings, the radial gradients, the sheen - is a
// grey smear at this size, so none of it is here. What is left is the three
// things that identify it: a dark green disc, a pale label, a red band.
//
// Inline and not an `<img>`, for two reasons. It is about four hundred bytes,
// which is smaller than the request would be. And the project logo is served
// from `public/` as a file, which is the right answer for an illustration and
// the wrong one for a glyph that appears on every frame of every panel.

/** The record mark, sized to the current font. */
export function Mark(): React.JSX.Element {
  return (
    <svg
      className="mark"
      viewBox="0 0 32 32"
      width="14"
      height="14"
      aria-hidden="true"
      focusable="false"
    >
      <circle cx="16" cy="16" r="15" fill="#1f4a34" />
      <circle cx="16" cy="16" r="15" fill="none" stroke="#0c1c14" strokeWidth="1" />
      <circle cx="16" cy="16" r="6.5" fill="#f4f1e9" />
      {/* The band is trimmed back to the label by the ring under it - a stroke
          of 4 centred on 8.5 paints 6.5 outwards in the disc colour - rather
          than by a clipPath, because a clipPath needs an id and an id in an
          inline SVG is a name shared with every other inline SVG on the page. */}
      <rect x="9.5" y="14" width="13" height="4" fill="#e0231c" transform="rotate(-18 16 16)" />
      <circle cx="16" cy="16" r="8.5" fill="none" stroke="#1f4a34" strokeWidth="4" />
      <circle cx="16" cy="16" r="1.1" fill="#0c1c14" />
    </svg>
  );
}
