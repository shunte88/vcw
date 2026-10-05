/*
 *  Switch.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A two-state switch with a word at each end.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// A checkbox underneath, restyled, and not a `div` with an `onClick`. Everything
// that makes a checkbox a checkbox - Space toggling it, the label clicking
// through to it, a screen reader calling it a checkbox, the focus ring the rest
// of the sheet already draws - is behaviour this file would otherwise have to
// reimplement and get subtly wrong. CSS moves the knob; the input does the work.
//
// Two words rather than one, because both states of the two switches VCW has
// are *answers* rather than an on and an off. "Mono" unticked is "Stereo", not
// "not mono", and a switch labelled only "Mono pressing" makes a person work
// that out from the absence of a tick.

/** One switch, with the state it is in named at the end it points to. */
export function Switch({
  checked,
  onChange,
  off,
  on,
  title,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** What the left end means. */
  off: string;
  /** What the right end means. */
  on: string;
  title?: string;
}): React.JSX.Element {
  return (
    <label className="switch" title={title}>
      <span className={checked ? "end" : "end chosen"}>{off}</span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="track" aria-hidden="true">
        <span className="knob" />
      </span>
      <span className={checked ? "end chosen" : "end"}>{on}</span>
    </label>
  );
}
