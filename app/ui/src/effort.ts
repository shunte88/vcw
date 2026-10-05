/*
 *  effort.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Which "how hard should the encoder work" field a format has, if any.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Two settings wear the same hole in the panel and they are not the same
// setting. A lossy quality decides what gets thrown away; a FLAC compression
// level decides how long the encoder spends finding a shorter way to say
// exactly the same samples. Labeling the second one "Lossy quality" - which
// is what the Settings panel did - says FLAC discards audio, which is the one
// thing a person choosing FLAC is choosing it not to do.
//
// So: the caption, the options and the field they write to all follow the
// format. Here rather than in either panel because the Export panel and the
// Settings panel have to agree, and a list of options duplicated in two files
// is a list that will be added to in one of them.
//
// WAV gets `null`. It is not that the field is hidden; there is no setting.

/** Which of the two export knobs a format has, and how to offer it. */
export type Effort = {
  /** The settings field and request key this writes to. */
  field: "quality" | "compression";
  /** What the control is called. */
  caption: string;
  /** Value then label, in the order they should be offered. */
  options: readonly (readonly [string, string])[];
};

/** `flac -0` to `-8`, the scale everybody has already read about. */
const COMPRESSION: readonly (readonly [string, string])[] = [
  ["0", "0 - fastest"],
  ["1", "1"],
  ["2", "2"],
  ["3", "3"],
  ["4", "4"],
  ["5", "5 - default"],
  ["6", "6"],
  ["7", "7"],
  ["8", "8 - smallest"],
];

const QUALITY: readonly (readonly [string, string])[] = [
  ["transparent", "Transparent"],
  ["high", "High"],
  ["compact", "Compact"],
];

/**
 * The effort control for a format, or `null` where there is nothing to set.
 *
 * Matched on the same spellings the backend's `from_extension` accepts, so a
 * settings file written by the CLI with `--format oga` still gets its field.
 */
export function effortOf(format: string): Effort | null {
  switch (format.trim().toLowerCase().replace(/^\./, "")) {
    case "flac":
      return {
        field: "compression",
        caption: "Compression",
        options: COMPRESSION,
      };
    case "mp3":
    case "ogg":
    case "oga":
    case "vorbis":
      return { field: "quality", caption: "Lossy quality", options: QUALITY };
    default:
      return null;
  }
}
