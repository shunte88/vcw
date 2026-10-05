#!/usr/bin/env python3
#  vcw-read.py
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  A third-party .vcw reader, written from docs/SCHEMA.md alone. WP-18's exit criterion.
#
# MIT License - see the header in any Rust source file for the full text.
#
"""A third-party reader for the VCW project format.

WP-18's exit criterion is "a third-party tool can read a project using the spec
alone", and the only honest way to test that is to write one. This file is that
tool. It was written against `docs/SCHEMA.md` and uses nothing but the Python
standard library: no VCW code, no generated bindings, no peeking at the Rust.

It does three things, which between them are what a third party would want:

  identify   what the file is, what is in it, and what it says about itself
  extract    a capture, a side or a track out to a WAV file
  verify     recompute every block checksum and every redundant column

`crates/project/tests/third_party_spec.rs` runs this against a project the
product wrote and compares the bytes it extracts, frame for frame, with what the
product's own reader returns. That test is the criterion; this file is only
credible while it stays ignorant of the Rust, so keep it that way.

Nothing here writes to the project. It opens read-only through a SQLite URI, and
`mode=ro` rather than `immutable=1` on purpose: a project may have content in its
`-wal` sidecar that has not been checkpointed, and `immutable=1` tells SQLite to
ignore that file and hand back a stale database.
"""

import argparse
import binascii
import os
import sqlite3
import struct
import sys
import wave

# From the spec's "Identity" table.
APPLICATION_ID = 0x56435700  # 'VCW\0'
SUPPORTED_USER_VERSIONS = (1, 2, 3, 4)
SUPPORTED_FORMAT_VERSION = 1

# Audacity's, for the one case a reader has to recognise and refuse.
AUDACITY_APPLICATION_ID = 0x41554459

# From the spec's "Sample format codes" table, plus the layout rules beneath it.
# (name, bytes per sample, unpacker)
FORMATS = {
    0x00020001: ("Int16", 2),
    0x00030002: ("Int24Packed", 3),
    0x00040001: ("Int24Padded", 4),
    0x00040002: ("Int32", 4),
    0x0004000F: ("Float32", 4),
}

SIDE_LETTERS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"


class Refused(Exception):
    """This file is not a project this tool will read."""


def open_project(path):
    """Opens a project read-only and checks it is one, from the header alone.

    The spec says to dispatch on `application_id` and `user_version` and never
    on the extension, so the two header words are read out of the first 100
    bytes rather than queried: a file that is not SQLite at all should be
    refused as such rather than by a failing SELECT.
    """
    with open(path, "rb") as handle:
        header = handle.read(100)
    if len(header) < 100 or header[:16] != b"SQLite format 3\x00":
        raise Refused(f"{path} is not a SQLite database")
    # Both fields are big-endian in the SQLite header, at the offsets SQLite
    # documents: application_id at 68, user_version at 60.
    (user_version,) = struct.unpack_from(">i", header, 60)
    (application_id,) = struct.unpack_from(">I", header, 68)

    if application_id == AUDACITY_APPLICATION_ID:
        raise Refused(
            f"{path} is an Audacity project (application_id 0x41554459), not a .vcw"
        )
    if application_id != APPLICATION_ID:
        raise Refused(
            f"{path} has application_id 0x{application_id:08X}, "
            f"expected 0x{APPLICATION_ID:08X}"
        )
    if user_version not in SUPPORTED_USER_VERSIONS:
        raise Refused(
            f"{path} is schema version {user_version}; this tool reads "
            f"{', '.join(str(v) for v in SUPPORTED_USER_VERSIONS)}"
        )

    uri = "file:" + os.path.abspath(path).replace("?", "%3f").replace("#", "%23")
    conn = sqlite3.connect(uri + "?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row

    # The format version is the meaning of the schema, and it lives in `meta`
    # rather than in the header. A reader that ignored it would happily
    # misinterpret a future file whose tables still look familiar.
    row = conn.execute("SELECT value FROM meta WHERE key = 'format_version'").fetchone()
    if row is None:
        raise Refused(f"{path} has no format_version in meta")
    if int(row[0]) != SUPPORTED_FORMAT_VERSION:
        raise Refused(
            f"{path} declares format_version {row[0]}; this tool reads "
            f"{SUPPORTED_FORMAT_VERSION}"
        )
    return conn, user_version


def capture(conn, capture_id):
    row = conn.execute(
        "SELECT * FROM captures WHERE capture_id = ?", (capture_id,)
    ).fetchone()
    if row is None:
        raise Refused(f"no capture {capture_id} in this project")
    return row


def format_of(code):
    if code not in FORMATS:
        raise Refused(f"unknown sampleformat code 0x{code:08X}")
    return FORMATS[code]


def read_frames(conn, capture_id, first_frame, frames):
    """Returns `frames` frames of interleaved little-endian PCM from a capture.

    The spec's rules, in the order it states them:

      * blocks are per channel, never interleaved, so one frame is assembled
        from `channels` blocks;
      * `sequence` is contiguous per channel and `start_frame` says where each
        block begins, so a span is found by arithmetic rather than by scanning;
      * `samples` is little-endian and holds exactly `frame_count` samples of
        one channel in the capture's `storage_format`.
    """
    row = capture(conn, capture_id)
    channels = row["channels"]
    name, width = format_of(row["storage_format"])
    total = row["frames"]

    if first_frame < 0 or first_frame > total:
        raise Refused(f"frame {first_frame} is outside a capture of {total} frames")
    frames = min(frames, total - first_frame)

    out = bytearray(frames * channels * width)
    for channel in range(channels):
        blocks = conn.execute(
            """SELECT start_frame, frame_count, samples
                 FROM capture_blocks
                 JOIN sampleblocks USING (blockid)
                WHERE capture_id = ? AND channel = ?
                  AND start_frame + frame_count > ?
                  AND start_frame < ?
                ORDER BY sequence""",
            (capture_id, channel, first_frame, first_frame + frames),
        )
        for block in blocks:
            samples = block["samples"] or b""
            begin = max(block["start_frame"], first_frame)
            end = min(block["start_frame"] + block["frame_count"], first_frame + frames)
            for frame in range(begin, end):
                at = (frame - block["start_frame"]) * width
                sample = samples[at : at + width]
                if len(sample) < width:
                    raise Refused(
                        f"block at frame {block['start_frame']} is short: "
                        f"{len(samples)} bytes for {block['frame_count']} frames"
                    )
                to = ((frame - first_frame) * channels + channel) * width
                out[to : to + width] = sample
    return bytes(out), channels, width, name, row["sample_rate"]


def write_wav(path, pcm, channels, width, name, rate):
    """Writes extracted PCM to a WAV file.

    Python's `wave` module writes integer PCM only, and the spec's formats are
    already little-endian integers of 2, 3 or 4 bytes, which is exactly what a
    WAV data chunk holds - so for those four formats this is a copy. Float32 is
    not representable by `wave`, so it is written by hand as
    `WAVE_FORMAT_IEEE_FLOAT`.
    """
    if name == "Float32":
        with open(path, "wb") as handle:
            block_align = channels * width
            data = len(pcm)
            handle.write(b"RIFF" + struct.pack("<I", 36 + data) + b"WAVE")
            handle.write(b"fmt " + struct.pack("<I", 16))
            handle.write(
                struct.pack(
                    "<HHIIHH", 3, channels, rate, rate * block_align, block_align, 32
                )
            )
            handle.write(b"data" + struct.pack("<I", data) + pcm)
        return
    with wave.open(path, "wb") as handle:
        handle.setnchannels(channels)
        handle.setsampwidth(width)
        handle.setframerate(rate)
        handle.writeframes(pcm)


def sides(conn):
    return conn.execute(
        """SELECT side_id, side_index, capture_id, title
             FROM sides ORDER BY side_index"""
    ).fetchall()


def tracks(conn, side_id):
    """A side's tracks, with the frames each one spans.

    A track holds no frame positions of its own: it points at two boundary rows
    and `at_frame` on each is "frames from the start of the side's capture".
    """
    return conn.execute(
        """SELECT t.number, t.title, t.artist, s.at_frame AS start, e.at_frame AS end
             FROM tracks t
             JOIN track_boundaries s ON s.boundary_id = t.start_boundary
             JOIN track_boundaries e ON e.boundary_id = t.end_boundary
            WHERE t.side_id = ?
            ORDER BY t.number""",
        (side_id,),
    ).fetchall()


def identify(conn, user_version, path, as_json):
    meta = dict(conn.execute("SELECT key, value FROM meta").fetchall())
    captures = conn.execute("SELECT * FROM captures ORDER BY capture_id").fetchall()
    release = conn.execute("SELECT * FROM releases WHERE release_id = 1").fetchone()

    report = {
        "path": path,
        "schema_version": user_version,
        "format_version": int(meta["format_version"]),
        "created_by": meta.get("created.by"),
        "created_at": meta.get("created.at"),
        "last_written_by": meta.get("last_written.by"),
        "captures": [],
        "sides": [],
    }
    if release is not None:
        report["release"] = {
            "album": release["album"],
            "album_artist": release["album_artist"],
            "year": release["year"],
            "genres": [g for g in release["genres"].split("; ") if g],
            "discs": release["discs"],
            "numbering": release["numbering"],
        }
    for row in captures:
        name, width = format_of(row["storage_format"])
        blocks = conn.execute(
            "SELECT COUNT(*) FROM capture_blocks WHERE capture_id = ?",
            (row["capture_id"],),
        ).fetchone()[0]
        report["captures"].append(
            {
                "capture_id": row["capture_id"],
                "sample_rate": row["sample_rate"],
                "channels": row["channels"],
                "storage_format": name,
                "bytes_per_sample": width,
                "capture_mode": row["capture_mode"],
                "state": row["state"],
                # v3's addition. A v1 or v2 project predates the column and says
                # nothing about the curve, which is what 'unknown' means anyway.
                "capture_eq": row["capture_eq"] if user_version >= 3 else "unknown",
                "frames": row["frames"],
                "seconds": row["frames"] / row["sample_rate"],
                "blocks": blocks,
                "interrupted": row["finished_at"] is None,
            }
        )
    for side in sides(conn):
        report["sides"].append(
            {
                "side": SIDE_LETTERS[side["side_index"]],
                "disc": side["side_index"] // 2 + 1,
                "capture_id": side["capture_id"],
                "tracks": [
                    {
                        "number": t["number"],
                        "title": t["title"],
                        "start_frame": t["start"],
                        "end_frame": t["end"],
                    }
                    for t in tracks(conn, side["side_id"])
                ],
            }
        )

    if as_json:
        import json

        print(json.dumps(report, sort_keys=True))
        return

    print(f"{path}")
    print(f"  schema      v{user_version}, format version {report['format_version']}")
    print(f"  created by  {report['created_by']}")
    if release is not None and release["album"]:
        print(f"  release     {release['album_artist']} - {release['album']}")
    for cap in report["captures"]:
        print(
            f"  capture {cap['capture_id']:<3} {cap['sample_rate']} Hz, "
            f"{cap['channels']} ch, {cap['storage_format']}, "
            f"{cap['frames']} frames ({cap['seconds']:.3f} s), "
            f"{cap['blocks']} blocks, {cap['state']}, eq {cap['capture_eq']}"
        )
    for side in report["sides"]:
        print(f"  side {side['side']}      capture {side['capture_id']}")
        for track in side["tracks"]:
            print(
                f"    {side['side']}{track['number']}       "
                f"{track['start_frame']}..{track['end_frame']}  {track['title']}"
            )


def verify(conn):
    """Recomputes what the spec says is redundant, and reports disagreement.

    Three checks: the block checksum (CRC-32, the zlib/IEEE one), `start_frame`
    against `sequence` and `frame_count`, and the capture's `frames` against the
    blocks that claim to be in it.
    """
    problems = 0
    checked = 0
    for cap in conn.execute("SELECT * FROM captures ORDER BY capture_id"):
        name, width = format_of(cap["storage_format"])
        for channel in range(cap["channels"]):
            expected_sequence = 0
            expected_frame = 0
            for block in conn.execute(
                """SELECT blockid, sequence, start_frame, frame_count, checksum, samples
                     FROM capture_blocks JOIN sampleblocks USING (blockid)
                    WHERE capture_id = ? AND channel = ?
                    ORDER BY sequence""",
                (cap["capture_id"], channel),
            ):
                checked += 1
                samples = block["samples"] or b""
                crc = binascii.crc32(samples) & 0xFFFFFFFF
                # The column is a signed SQLite integer, so a CRC above 2^31
                # may have been stored negative by a writer using i32.
                stored = block["checksum"] & 0xFFFFFFFF
                if crc != stored:
                    problems += 1
                    print(
                        f"  block {block['blockid']}: checksum {stored:#010x}, "
                        f"computed {crc:#010x}"
                    )
                if block["sequence"] != expected_sequence:
                    problems += 1
                    print(
                        f"  capture {cap['capture_id']} channel {channel}: "
                        f"sequence {block['sequence']} where {expected_sequence} "
                        "was expected; the spec says contiguous with no gaps"
                    )
                if block["start_frame"] != expected_frame:
                    problems += 1
                    print(
                        f"  block {block['blockid']}: start_frame "
                        f"{block['start_frame']}, expected {expected_frame}"
                    )
                if len(samples) != block["frame_count"] * width:
                    problems += 1
                    print(
                        f"  block {block['blockid']}: {len(samples)} bytes for "
                        f"{block['frame_count']} frames of {name}"
                    )
                expected_sequence = block["sequence"] + 1
                expected_frame = block["start_frame"] + block["frame_count"]
            if expected_frame != cap["frames"]:
                problems += 1
                print(
                    f"  capture {cap['capture_id']} channel {channel}: blocks hold "
                    f"{expected_frame} frames, the capture row says {cap['frames']}"
                )
    print(f"  checked     {checked} block(s), {problems} problem(s)")
    return problems


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Read a VCW project using docs/SCHEMA.md alone."
    )
    parser.add_argument("project")
    sub = parser.add_subparsers(dest="command", required=True)

    said = sub.add_parser("identify", help="what the file is and what is in it")
    said.add_argument("--json", action="store_true")

    out = sub.add_parser("extract", help="write audio to a WAV file")
    out.add_argument("--out", required=True)
    out.add_argument("--capture", type=int)
    out.add_argument("--side", help="a side letter, e.g. A")
    out.add_argument("--track", type=int, help="a track number within the side")

    sub.add_parser("verify", help="recompute checksums and redundant columns")

    args = parser.parse_args(argv)
    try:
        conn, user_version = open_project(args.project)
    except Refused as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        return 2

    try:
        if args.command == "identify":
            identify(conn, user_version, args.project, args.json)
            return 0
        if args.command == "verify":
            return 1 if verify(conn) else 0

        capture_id = args.capture
        first, count = 0, None
        if args.side is not None:
            letter = args.side.upper()
            if letter not in SIDE_LETTERS:
                raise Refused(f"{args.side} is not a side letter")
            found = [s for s in sides(conn) if s["side_index"] == SIDE_LETTERS.index(letter)]
            if not found:
                raise Refused(f"this project has no side {letter}")
            capture_id = found[0]["capture_id"]
            if capture_id is None:
                raise Refused(f"side {letter} has not been recorded")
            if args.track is not None:
                spans = [t for t in tracks(conn, found[0]["side_id"])
                         if t["number"] == args.track]
                if not spans:
                    raise Refused(f"side {letter} has no track {args.track}")
                first = spans[0]["start"]
                count = spans[0]["end"] - first
        if capture_id is None:
            raise Refused("say which audio to extract: --capture or --side")
        if count is None:
            count = capture(conn, capture_id)["frames"]

        pcm, channels, width, name, rate = read_frames(conn, capture_id, first, count)
        write_wav(args.out, pcm, channels, width, name, rate)
        print(
            f"  wrote       {args.out}: {len(pcm) // (channels * width)} frames, "
            f"{channels} ch, {name} at {rate} Hz"
        )
        return 0
    except Refused as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
