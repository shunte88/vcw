# Using VCW

*VCW - The Vinyl Capture Workstation. (c) 2026 Stue Hunter. MIT License - see
the header in any Rust source file for the full text.*

This is the guide to doing the thing: getting a record off the turntable, into a
project, split into tracks, named, and out as files. It covers the window and the
command line, because §2 and §4.5 say both have to work and neither is the
"real" one.

Two documents sit beside this one. [`SCHEMA.md`](SCHEMA.md) is the file format,
for anyone writing their own reader. [`PROJECT-API.md`](PROJECT-API.md) is the
Rust API, for anyone writing their own tool.

## What a project is

One file, ending `.vcw`. It holds the audio, the track boundaries, the metadata
and the artwork - everything, so a project can be moved, copied or backed up by
copying one file. It is a SQLite database, which means it is also readable by
anything that can open one (see `SCHEMA.md`), and it is written with the
write-ahead log on, which means **two more files appear beside it while it is
open**: `side-a.vcw-wal` and `side-a.vcw-shm`. Those are not junk. If you copy a
project that is open, copy all three or you will copy it mid-sentence.

A project normally holds one side of one record. Nothing stops it holding both,
and the importer will put both faces in one capture if that is how they were
recorded, but the grain that works is a side: it is the length of one continuous
recording, and it is what the transport and the detector are built around.

## In the window

The application - `VCW`, as distinct from the `vcw` command line - opens on the
project browser and has six workspaces, one per step of §50's workflow,
reachable from the tabs or from `Ctrl+1` to `Ctrl+6`:

| Workspace | What it is for |
| --- | --- |
| **Projects** | Every project the library knows, with its album, artist, catalogue number, sides, track count and length. `n` creates one, `Enter` opens it. |
| **Capture** | The device list, the configuration that was negotiated, and the captures the open project already holds with their state and length. |
| **Tracks** | The track list and the boundary list side by side: title, artist, start and length above; confidence, how many detectors agreed, and whether a person has locked it below. |
| **Metadata** | A provider search and its candidates, and the release the project settles on. |
| **Export** | The plan, track by track, before any of it is written. |
| **Settings** | Which credentials are configured, how many characters each is, and the environment variable each comes from - never the credential. |

Across the top of every workspace is the transport - the same arm, record,
pause, resume and stop the `vcw session` verb drives - with the meters and the
waveform under it. **While a record is playing, watch the meters**: the waveform
is drawn from what has been committed and it appears when the capture stops, not
as the side goes by. Nothing is missing while it is blank - the audio is on disk
within a quarter of a second of the stylus reading it either way. `Ctrl+d` opens
the event log, which is every event the core published in this session and the
first place to look when something did not do what you expected.

When the application refuses something it says so in a banner and leaves the
refusal there until you dismiss it with `Escape`. That is deliberate: a refusal
that disappears on its own is a refusal nobody read.

## Before the first capture

Check that VCW can see the machine:

```text
vcw doctor
```

That prints the version, the platform, the SQLite build and the host APIs found.
Then look at what is attached:

```text
vcw devices
vcw devices --hardware          # skip the OS mixer's virtual endpoints
vcw formats "ALC1150 Analog"    # what one device will actually accept
```

Two things to know about that list. First, a device's **id** is what to select
on, not its name: names are not unique and change with the driver. Second, the
backend's own *default* configuration is not a recommendation. On this
development machine the default input is the one configuration guaranteed **not**
to be bit-perfect, because it routes through the system mixer. Pick a rate and
format deliberately.

## Recording a side

The short version, from the command line:

```text
vcw session side-a.vcw --device "ALC1150 Analog" --rate 48000 --channels 2 --format s24
```

That opens the transport and waits for commands: `arm`, `record`, `pause`,
`resume`, `stop`. In the window it is the same five, on the buttons and on the
keys in the table below.

What to watch while it runs:

* **What was negotiated, not what was asked.** VCW never quietly settles for
  less (§9). If the device could not do 24-bit it says so, and a capture that
  diverged from the request records the divergence. Read that line.
* **Whether the OS confirmed it.** VCW asks the operating system what the stream
  really is, because a backend reporting 24/96 while the mixer resamples is the
  failure mode that a level meter cannot show. A capture that could not be
  confirmed is recorded as unconfirmed rather than claimed as bit-perfect.
* **The four counters**: overruns, underruns, dropped frames, stream errors. All
  four at zero is necessary for a clean rip and is not sufficient - see the
  unconfirmed case above.

A capture is committed continuously, not at the end. There is no "save": every
250 ms of audio is written and fsynced as it arrives, so the worst a crash can
cost is the last block plus whatever the driver was still holding.

## When something goes wrong mid-capture

If the process died, the machine lost power, or the device vanished, the project
is still there and so is almost all of the audio.

```text
vcw recover side-a.vcw            # report only; writes nothing
vcw recover side-a.vcw --apply    # close the capture honestly
vcw recover side-a.vcw --repair   # also drop blocks stranded past the end
vcw recover side-a.vcw --verify   # recheck every block's checksum afterwards
```

The report first is the default on purpose. It tells you how many frames are
usable and whether any blocks were stranded past the last consistent point, and
only `--apply` writes. `--repair` additionally deletes the stranded blocks, and
is the only thing in VCW that makes recovery discard audio. `--verify`
recomputes every block's checksum afterwards, which reads the whole project and
takes minutes on a full side.

A recovered capture is marked `recovered` rather than `finalised`, which is a
distinction worth keeping: an interrupted capture stopped for a reason the
writer observed and recorded, while a recovered one stopped
without warning and everything known about it was worked out afterwards from the
blocks that had already been committed.

One caveat that cannot be designed away: **opening the project is what consumes
the crash evidence.** SQLite replays a hot log on open and folds it in on close.
So the first `vcw recover` reads the log whatever flags it was given. If you want
the crashed state kept exactly as it is, copy the `.vcw`, the `-wal` and the
`-shm` together, before running anything at all.

## Importing from Audacity

A `.aup3` or `.aup4` project comes in as an ordinary capture:

```text
vcw import ~/rips/geronimo.aup3 --output geronimo.vcw
vcw import ~/rips/geronimo.aup3 --dry-run        # read it and report, write nothing
```

The source is opened **read-only** and never touched. Labels become locked track
boundaries, tags become the release, and every tag is also kept verbatim so
nothing is lost in translation. What lands is a capture like any other:
playback, detection, editing, tagging and export all work on it with no special
case.

Two things to expect. The audio is **re-blocked** rather than adopted, so an
import writes a new file of comparable size and takes a couple of minutes for a
full side. And Audacity records at 32-bit float by default, which FLAC cannot
carry - FLAC is an integer codec - so a float project can currently only be
exported as WAV. `vcw export --format flac` refuses before it writes anything
rather than producing half a library.

## Finding the tracks

```text
vcw detect side-a.vcw                          # propose boundaries
vcw detect side-a.vcw --adaptive --evidence    # and show the reasoning
vcw tracks side-a.vcw list --boundaries        # what is there now
vcw tracks side-a.vcw adopt                    # write what the policy accepts
vcw tracks side-a.vcw split 3 12345678         # track 3, at that frame
vcw tracks side-a.vcw merge 3 4
vcw tracks side-a.vcw move 7 12345678          # boundary 7, to that frame
vcw tracks side-a.vcw lock 7
vcw tracks side-a.vcw delete 3                 # keeps every sample it covered
```

`vcw detect` proposes and writes nothing; `vcw tracks ... adopt` is what commits
a pass. That split is the point: detection proposes, it does not decide. Every
boundary carries where it came
from - detected, with its confidence and the evidence behind it, or placed by a
person - and a boundary a person has touched is **locked**: re-running detection
will not move it. That is the rule that makes it safe to re-detect after fixing
one mistake by hand.

A track is the span between two boundaries, half-open: it starts at one and stops
just before the next, so adjacent tracks share an edge and there is no gap and no
overlap.

## Naming the record

```text
vcw metadata search --artist "Kraftwerk" --album "Trans-Europe Express"
vcw metadata fetch <id>                    # one release, with its tracklist
vcw release side-a.vcw set --album "Trans-Europe Express" --artist "Kraftwerk" --year 1977
vcw release side-a.vcw artwork front.jpg
vcw tracks side-a.vcw set 1 --title "Europe Endless"
```

MusicBrainz needs no credential. Discogs does, and **credentials are never
stored in a project or in a settings file** (§39): they come from the
environment, and `vcw metadata credentials` will tell you which are configured
without printing them. Passing `--offline` hands the providers a transport that
refuses, so every path here can be exercised with no network and no credential.

```text
export VCW_DISCOGS_TOKEN=...        # never committed, never written to disk by VCW
export VCW_CONTACT=you@example.com  # the contact string §40 asks providers for
```

One thing that catches people searching by hand: a provider's `format:` field is
an exact medium-name match, so `format:vinyl` finds nothing. Vinyl is spelled
four different ways across the catalogue, and VCW's own search asks for all four.

## Exporting

```text
vcw export side-a.vcw --into ~/Music --format wav
vcw export side-a.vcw --into ~/Music --format flac
vcw export side-a.vcw --into ~/Music --dry-run    # the whole plan, no files
```

Files are laid out and named from a template, tagged from the release and the
track, and the cover goes beside them. The default template is
`{album_artist}/{album}/{tracknum} - {title}`, and `{tracknum}` is the position
printed on the label - so side A's first track is `A1 - The Rainbow.wav` and
side B's is `B1 - ...`. Four things worth knowing:

* `--dry-run` resolves the whole plan - every path, every tag, every frame
  count - and stops there. It is the cheapest way to find out what an export
  will do.
* An export **plans before it writes**. If the plan cannot be satisfied - a
  format that cannot carry the audio, a track with no boundary - it refuses
  before encoding, so you do not get half a library.
* A refusal can still leave the empty directories the plan created. An empty
  `Album/` left behind is untidy; half a FLAC would be a corrupt library, and
  that is the trade being made.
* **A 32-bit capture can only leave as WAV.** The FLAC encoder stops at 24 bits
  and narrowing 32 to 24 throws signal away, so `--format flac` refuses it by
  name rather than quietly dithering. Capture at `--format s24` if FLAC is
  where the rip is going to live.

## When you need to report a problem

```text
vcw bundle side-a.vcw --out bundle.json
vcw bundle --out bundle.json                 # no project: just the machine
vcw bundle side-a.vcw --checksums --out bundle.json
```

That writes one JSON document: the version and build, the OS, the audio backend
and every device it can see, the project's identity and integrity, the row
counts, and every capture's configuration and counters.

It is built to be **safe to send**. There is no recorded audio in it, no track or
album titles, no credentials, and not even the project's path - only its file
name. `--checksums` additionally recomputes every block's CRC-32, which is the
check SQLite's own `integrity_check` cannot do, and takes minutes on a full side.

A bundle of a project that will not open is still a bundle: the open failure is
recorded in the document and everything else in it survives. That is the case the
verb exists for.

For a running problem rather than a stored one, turn the log up. It goes to
stderr, so it never disturbs a verb's output:

```text
vcw --log debug session side-a.vcw ...
VCW_LOG=vcw_audio=trace vcw capture ...      # one crate, loudly
VCW_LOG=warn,vcw_project=debug vcw ...       # a filter per target
```

Default is `warn`, which in practice is silence. Nothing in the audio callback
logs, by rule (§42) and by test: a log line on the audio thread takes a lock and
touches a file descriptor, which at 192 kHz is a click in the recording.

## The keyboard

Every workflow §44 lists is reachable from the keyboard, and the map below is
generated from the application's own binding table rather than transcribed, so
it cannot drift from what the window actually does. `?` or `F1` shows the same
map in the window.

Three things in §44 are deliberately **not** here: device enumeration, block
storage and project recovery. They are not things a person does, they are things
that happen - enumeration is what the capture workspace shows when it opens,
storage is the writer, and recovery is a prompt the shell raises. A keyboard map
that claimed to cover them would be describing the wrong thing.

### Global

| Key | Does | Workflow (§44) | §43 default |
| --- | --- | --- | --- |
| `space` | Play or pause the audition | `play` | yes |
| `r` | Start recording | `record` | yes |
| `s` | Stop | `stop` | yes |
| `m` | Place a marker at the playhead | `place-marker` | yes |
| `ArrowLeft` | Seek back | `seek` | yes |
| `ArrowRight` | Seek forward | `seek` | yes |
| `Ctrl+s` | Commit what has been recorded so far | `checkpoint` | yes |
| `Ctrl+e` | Export | `export` | yes |
| `p` | Pause or resume recording | `pause` | - |
| `Shift+ArrowRight` | Skip to the next track | `skip` | - |
| `Shift+ArrowLeft` | Skip to the previous track | `skip` | - |
| `?` or `F1` | Show the keyboard map | `help` | - |
| `Ctrl+1` | Projects | `navigate` | - |
| `Ctrl+2` | Capture | `navigate` | - |
| `Ctrl+3` | Tracks | `navigate` | - |
| `Ctrl+4` | Metadata | `navigate` | - |
| `Ctrl+5` | Export | `navigate` | - |
| `Ctrl+6` | Settings | `navigate` | - |
| `Ctrl+d` | Show the event log | `navigate` | - |
| `Escape` | Close the overlay, or clear the last refusal | `navigate` | - |

### Browser

| Key | Does | Workflow (§44) | §43 default |
| --- | --- | --- | --- |
| `Enter` | Open the selected project | `navigate` | - |
| `n` | Create a project | `navigate` | - |
| `ArrowUp` | Select the project above | `navigate` | - |
| `ArrowDown` | Select the project below | `navigate` | - |

### Capture

| Key | Does | Workflow (§44) | §43 default |
| --- | --- | --- | --- |
| `a` | Arm the chosen device on the open project | `arm` | - |
| `d` | Focus the device list | `choose-device` | - |
| `f` | Focus the rate and format fields | `choose-rate-format` | - |

### Tracks

| Key | Does | Workflow (§44) | §43 default |
| --- | --- | --- | --- |
| `Delete` or `Backspace` | Delete the selected marker | `delete-marker` | yes |
| `Ctrl+ArrowLeft` | Nudge the selected marker earlier | `move-marker` | - |
| `Ctrl+ArrowRight` | Nudge the selected marker later | `move-marker` | - |
| `t` | Run track detection over the capture | `detect-tracks` | - |
| `Enter` | Edit the selected track | `edit-track-metadata` | - |
| `ArrowUp` | Select the track above | `edit-track-metadata` | - |
| `ArrowDown` | Select the track below | `edit-track-metadata` | - |
| `Shift+ArrowUp` | Select the marker above | `move-marker` | - |
| `Shift+ArrowDown` | Select the marker below | `move-marker` | - |

### Metadata

| Key | Does | Workflow (§44) | §43 default |
| --- | --- | --- | --- |
| `l` | Look the record up with a provider | `search-metadata` | - |
| `Enter` | Accept the selected release | `choose-release` | - |
| `ArrowUp` | Select the candidate above | `choose-release` | - |
| `ArrowDown` | Select the candidate below | `choose-release` | - |

## Where things are

| What | Where |
| --- | --- |
| The file format | [`docs/SCHEMA.md`](SCHEMA.md) |
| A third-party reader, in Python | [`tools/vcw-read.py`](../tools/vcw-read.py) |
| The Rust API | [`docs/PROJECT-API.md`](PROJECT-API.md) |
| A worked example of that API | [`crates/project/examples/read_a_project.rs`](../crates/project/examples/read_a_project.rs) |
| What is built and what is not | [`docs/STATUS.md`](STATUS.md) |
| The requirements this all answers to | [`REQUIREMENTS.md`](../REQUIREMENTS.md) |
