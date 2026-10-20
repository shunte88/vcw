#!/usr/bin/env python3
#
#  guide-version.py
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Restamp the user guide's edition line for a release, in all three formats.
#
#  MIT License - see the header in any Rust source file for the full text.
#
# The guide in `docs/VCW-User-Guide` is hand-authored and is not ours to edit,
# with one exception: the "About this edition" sentence names the version the
# guide was prepared against, and that sentence is a fact about the release
# rather than a piece of writing. A version bump that leaves it alone ships a
# guide claiming to describe a version that is now two behind, and nothing
# fails, because no test can tell good prose from stale prose.
#
# So this script owns that one sentence. It rewrites it in the Markdown source
# and in the Word document, then rebuilds the PDF from the Word document the
# way the author did - LibreOffice is what produced the shipped PDF, and its
# `Producer` string is the evidence. Every other byte of the Word document is
# copied across untouched, member by member, so a run cannot quietly reflow a
# page or drop an image.
#
# Usage: tools/guide-version.py [version] [date]
#
# Both default to the release being cut: the workspace version, and today.

import re
import subprocess
import sys
import tempfile
import zipfile
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GUIDE = ROOT / "docs" / "VCW-User-Guide"
STAMP = re.compile(r"Prepared \w+ \d{1,2}, \d{4}, against VCW version [\d.]+\.")


def workspace_version() -> str:
    """`workspace.package.version`, which is the first `version` in the manifest."""
    for line in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        if line.startswith("version = "):
            return line.split('"')[1]
    sys.exit("no `version = ` line in Cargo.toml")


def restamp(text: str, sentence: str, where: str) -> str:
    """The one sentence replaced, or a refusal naming where it was not found once."""
    out, found = STAMP.subn(sentence, text)
    if found != 1:
        sys.exit(f"{where}: the edition sentence appears {found} times, expected 1")
    return out


def main() -> None:
    version = sys.argv[1] if len(sys.argv) > 1 else workspace_version()
    when = sys.argv[2] if len(sys.argv) > 2 else date.today().strftime("%B %-d, %Y")
    sentence = f"Prepared {when}, against VCW version {version}."

    markdown = GUIDE / "VCW-User-Guide.md"
    markdown.write_text(
        restamp(markdown.read_text(encoding="utf-8"), sentence, markdown.name),
        encoding="utf-8",
    )

    document = GUIDE / "VCW-User-Guide.docx"
    staged = document.with_suffix(".docx.new")
    with zipfile.ZipFile(document) as source, zipfile.ZipFile(staged, "w") as target:
        for member in source.infolist():
            data = source.read(member.filename)
            if member.filename == "word/document.xml":
                data = restamp(
                    data.decode("utf-8"), sentence, member.filename
                ).encode("utf-8")
            # The member carries its own compression method, so writing it back
            # with its `ZipInfo` keeps a stored entry stored.
            target.writestr(member, data)
    staged.replace(document)

    # A headless LibreOffice still wants a profile, and the one in $HOME may be
    # in use by a window somebody has open.
    with tempfile.TemporaryDirectory() as profile:
        subprocess.run(
            [
                "soffice",
                "--headless",
                f"-env:UserInstallation=file://{profile}",
                "--convert-to",
                # The author's own export kept every image in Flate, and the
                # default re-encodes them all to JPEG: same pixels, same 300
                # dpi ceiling, visibly softer screenshots. `pdfimages -list`
                # is what says which, and it said `jpeg` until this was here.
                'pdf:writer_pdf_Export:{"UseLosslessCompression":'
                '{"type":"boolean","value":"true"}}',
                "--outdir",
                str(GUIDE),
                str(document),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )

    print(sentence)


if __name__ == "__main__":
    main()
