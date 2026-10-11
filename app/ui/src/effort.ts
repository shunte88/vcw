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
//
// The caption and the labels are catalog keys, because this is data and the
// two panels that render it only know how to call `t`. The six bare numerals
// are not: a numeral is the same numeral in every language, and nine catalog
// entries of which six read "3" would be six entries a translator has to look
// at to find they have nothing to do. `t` renders a key it does not know as
// itself, which is what makes "3" come out as 3.

import { key } from "./i18n";

/** Which of the two export knobs a format has, and how to offer it. */
export type Effort = {
  /** The settings field and request key this writes to. */
  field: "quality" | "compression";
  /** The catalog key for what the control is called. */
  caption: string;
  /** Value then label - a catalog key, or a numeral - in offering order. */
  options: readonly (readonly [string, string])[];
};

/** `flac -0` to `-8`, the scale everybody has already read about. */
const COMPRESSION: readonly (readonly [string, string])[] = [
  ["0", key("export.compression.fastest")],
  ["1", "1"],
  ["2", "2"],
  ["3", "3"],
  ["4", "4"],
  ["5", key("export.compression.default")],
  ["6", "6"],
  ["7", "7"],
  ["8", key("export.compression.smallest")],
];

const QUALITY: readonly (readonly [string, string])[] = [
  ["transparent", key("export.quality.transparent")],
  ["high", key("export.quality.high")],
  ["compact", key("export.quality.compact")],
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
        caption: key("export.effort.compression"),
        options: COMPRESSION,
      };
    case "mp3":
    case "ogg":
    case "oga":
    case "vorbis":
      return {
        field: "quality",
        caption: key("export.effort.quality"),
        options: QUALITY,
      };
    default:
      return null;
  }
}
