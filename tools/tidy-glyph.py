#!/usr/bin/env python3
#
#  tidy-glyph.py
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Strips Inkscape editor state out of a shipped glyph and gives it a header.
#
#  MIT License - see the header in any Rust source file for the full text.
#
"""Strip Inkscape's editor state out of a shipped glyph and give it a header.

Usage: tidy-glyph.py <path> "<one-line purpose>"
       tidy-glyph.py --selftest

The art arrives as Inkscape saves: a `sodipodi:namedview` holding the window
size and the zoom at the moment it was written, a `<defs>` with nothing in
it, a `Generator: SVG Repo Mixer Tools` line written by the editor the art is
drawn in, and per-element `style` strings carrying two dozen properties that do
not apply to a path. None of it draws anything, and since `app.css` masks these
files straight out of `assets/`, Vite inlines whatever is in them into the
stylesheet as a data URI - so the editor state was shipping to users.

Geometry is not touched: every `d`, `transform` and paint property survives.
"""
import pathlib
import re
import sys

# What a path is actually painted with. Everything else in an Inkscape style
# string is editor state or a default.
KEEP = {
    "fill",
    "fill-rule",
    "fill-opacity",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-dasharray",
    "stroke-opacity",
    "opacity",
}

DROP_ELEMENTS = ("sodipodi:namedview", "metadata")
DROP_ATTRS = re.compile(
    r'\s+(?:sodipodi|inkscape):[\w-]+="[^"]*"'
    r'|\s+xmlns:(?:sodipodi|inkscape|svg)="[^"]*"'
    r'|\s+id="[^"]*"'
)

MARK = "VCW - The Vinyl Capture Workstation"

PROLOG = '<?xml version="1.0" encoding="UTF-8" standalone="no"?>\n'

HEADER_BODY = """<!--
  {name}.svg

  VCW - The Vinyl Capture Workstation
  (c) 2026 Stue Hunter

  {purpose}. Worn as a mask-image, so only the shape is read.

  MIT License - see the header in any Rust source file for the full text.
-->
"""


def style(match):
    """Keeps the declarations that paint, drops the rest."""
    kept = []
    for part in match.group(1).split(";"):
        name, _, value = part.partition(":")
        if name.strip() in KEEP:
            kept.append(f"{name.strip()}:{value.strip()}")
    return f' style="{";".join(kept)}"' if kept else ""


def tidy(text, name, purpose):
    """Returns the tidied file, and whether the header it kept was already there."""
    for element in DROP_ELEMENTS:
        text = re.sub(rf"<{element}\b.*?(?:/>|</{element}>)", "", text, flags=re.S)
    text = re.sub(r"<defs\b[^>]*/>|<defs\b[^>]*>\s*</defs>", "", text)
    text = DROP_ATTRS.sub("", text)
    text = re.sub(r' style="([^"]*)"', style, text)
    text = re.sub(r"^<\?xml[^>]*\?>\s*", "", text)
    # The leading comment is either this tool's own header, which is replaced, or a
    # hand-written one carrying design notes, which is not: a glyph whose header
    # explains why it is drawn the way it is loses that on the next tidy otherwise,
    # and prepending a second header instead is how two of them ended up with one
    # each. Either way the file leaves here with exactly one.
    head = re.match(r"<!--.*?-->\s*", text, flags=re.S)
    mine = head is not None and "Worn as a mask-image" in head.group(0)
    kept = head is not None and MARK in head.group(0) and not mine
    header = head.group(0).rstrip() + "\n" if kept else HEADER_BODY.format(name=name, purpose=purpose)
    if head:
        text = text[head.end() :]
    text = re.sub(r"\n{3,}", "\n\n", text.strip())
    return PROLOG + header + text + "\n", kept


def selftest():
    """One header out, whatever shape went in."""
    art = '<?xml version="1.0"?>\n<svg><path d="M0 0"/></svg>'
    once, kept = tidy(art, "play", "Play")
    assert not kept and once.count("<!--") == 1, once
    twice, kept = tidy(once, "play", "Play")
    assert twice == once and not kept, twice

    hand = PROLOG + "<!--\n  play.svg\n\n  " + MARK + "\n\n  Why it is drawn so.\n-->\n"
    out, kept = tidy(hand + '<svg><path d="M0 0"/></svg>', "play", "Play")
    assert kept and out.count("<!--") == 1 and "Why it is drawn so." in out, out
    assert tidy(out, "play", "Play")[0] == out
    print("tidy-glyph: ok")


if __name__ == "__main__":
    if sys.argv[1:2] == ["--selftest"]:
        selftest()
        raise SystemExit(0)
    path = pathlib.Path(sys.argv[1])
    before = path.read_text(encoding="utf-8")
    after, kept = tidy(before, path.stem, sys.argv[2])
    path.write_text(after, encoding="utf-8")
    note = " (kept the header already in the file)" if kept else ""
    print(f"{path.name}: {len(before)} -> {len(after)} bytes{note}")
