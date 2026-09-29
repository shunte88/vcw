#!/usr/bin/env python3
#
#  verify-release.py
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Write and check the SHA256SUMS of a release's artifacts (WP-19).
#
#  MIT License - see the header in any Rust source file for the full text.
#
# Standard library only, and Python 3.8 upwards, because the one moment this is
# needed is on a machine where nothing is installed yet: somebody has just
# downloaded an installer and wants to know whether it arrived intact before
# they run it. A verifier with dependencies is a verifier nobody can run.
#
# What this answers and what it does not: a matching digest says the bytes are
# the bytes the release was built from, and it says nothing at all about who
# built them. Anybody who can replace a download can replace the SHA256SUMS
# beside it. Integrity is what a checksum is for; authenticity needs the
# signature, and `--expect` is how to pin the digest to one you were told out
# of band.

import argparse
import hashlib
import sys
from pathlib import Path

CHUNK = 1 << 20
DIGEST_CHARS = 64


# What a release artifact looks like. Used both to decide what to checksum and
# to decide what counts as an unexpected file beside the checksums: a log or a
# release-notes file in the same directory is not a stray artifact.
ARTIFACTS = [
    "*.AppImage",
    "*.deb",
    "*.dmg",
    "*.msi",
    "*.exe",
    "*.tar.gz",
    "*.zip",
]


def artifacts_in(directory, patterns):
    """Every file in a directory that looks like a release artifact."""
    return sorted(
        {
            found
            for pattern in patterns
            for found in directory.glob(pattern)
            if found.is_file()
        }
    )


def sha256(path):
    """The hex digest of one file, read in chunks so a 200 MB DMG is fine."""
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        while True:
            chunk = handle.read(CHUNK)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def read_sums(path):
    """Parse a SHA256SUMS file into [(digest, name)], loudly.

    The coreutils format is `<digest>  <name>`, two spaces, and a leading `*`
    on the name means binary mode. A line this cannot read is an error rather
    than something to skip: a verifier that ignores what it does not
    understand reports success over a file it never looked at.
    """
    entries = []
    for number, line in enumerate(path.read_text().splitlines(), start=1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(None, 1)
        if len(parts) != 2 or len(parts[0]) != DIGEST_CHARS:
            raise SystemExit(f"{path}:{number}: not a SHA256SUMS line: {line!r}")
        digest, name = parts[0].lower(), parts[1].lstrip("*").strip()
        int(digest, 16)  # raises if it is not hex
        entries.append((digest, name))
    if not entries:
        raise SystemExit(f"{path}: no checksums in it")
    return entries


def write(directory, out, patterns):
    """Write a SHA256SUMS for everything in a directory that matches."""
    names = [
        found.name for found in artifacts_in(directory, patterns) if found.name != out.name
    ]
    if not names:
        raise SystemExit(f"{directory}: nothing matched {patterns}")
    lines = [f"{sha256(directory / name)}  {name}" for name in names]
    out.write_text("\n".join(lines) + "\n")
    for line in lines:
        print(line)
    print(f"\n{len(lines)} file(s) -> {out}", file=sys.stderr)
    return 0


def check(directory, sums, expect):
    """Verify a directory against a SHA256SUMS. Returns an exit code."""
    entries = read_sums(sums)
    listed = {name for _, name in entries}
    failures = 0

    for digest, name in entries:
        path = directory / name
        if not path.is_file():
            print(f"MISSING  {name}")
            failures += 1
            continue
        found = sha256(path)
        if found == digest:
            print(f"ok       {name}")
        else:
            print(f"FAILED   {name}\n  expected {digest}\n  found    {found}")
            failures += 1

    # An artifact that arrived but is not in the list is worth saying out loud.
    # It is usually a partial download or a file from another release, and
    # either way running it was not what the checksums covered.
    for found in artifacts_in(directory, ARTIFACTS):
        if found.name not in listed and found != sums:
            print(f"unlisted {found.name}")

    if expect:
        wanted = expect.lower()
        matching = [name for digest, name in entries if digest == wanted]
        if matching:
            print(f"\nthe digest you pinned is the one on {', '.join(matching)}")
        else:
            print(f"\nFAILED   no file in {sums.name} has the digest {wanted}")
            failures += 1

    total = len(entries)
    if failures:
        print(f"\n{failures} of {total} check(s) failed. Do not run these files.")
        return 1
    print(f"\n{total} file(s) verified.")
    print("This says the bytes are intact. It does not say who made them.")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Check a VCW release's downloads against its SHA256SUMS.",
        epilog="A checksum proves integrity, not authorship. Names in the "
        "SHA256SUMS are resolved beside that file, not against the current "
        "directory: `sha256sum -c` does the latter, so the two agree only when "
        "you run it from where the artifacts are.",
    )
    parser.add_argument(
        "sums",
        nargs="?",
        default="SHA256SUMS",
        type=Path,
        help="the SHA256SUMS file (default: ./SHA256SUMS)",
    )
    parser.add_argument(
        "-C",
        "--directory",
        type=Path,
        help="where the artifacts are (default: beside the SHA256SUMS)",
    )
    parser.add_argument(
        "--expect",
        metavar="DIGEST",
        help="a digest you were given out of band; fails unless some file has it",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="write the SHA256SUMS instead of checking it (for the release job)",
    )
    parser.add_argument(
        "--pattern",
        action="append",
        default=None,
        metavar="GLOB",
        help="with --write, which files to include. Repeatable",
    )
    args = parser.parse_args(argv)

    directory = args.directory or (args.sums.parent if args.sums.parent != Path("") else Path("."))
    if not directory.is_dir():
        raise SystemExit(f"{directory}: not a directory")

    if args.write:
        patterns = args.pattern or ARTIFACTS
        return write(directory, args.sums, patterns)

    if not args.sums.is_file():
        raise SystemExit(f"{args.sums}: not there. Download it beside the artifacts.")
    return check(directory, args.sums, args.expect)


if __name__ == "__main__":
    sys.exit(main())
