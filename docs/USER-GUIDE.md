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
| **Library** | Every project the library knows: its cover, album, artist, catalogue number, sides, track count, length, size and when it last changed. `n` creates one, `Enter` opens it. |
| **Capture** | The device list, the configuration that was negotiated, and the captures the open project already holds with their state and length. |
| **Tracks** | The track list over the boundary list: title, artist, start and length above; confidence, how many detectors agreed, and whether a person has locked it below. Both headers stay put as you scroll. |
| **Metadata** | A provider search and its candidates, and the release the project settles on. |
| **Export** | The plan, track by track, before any of it is written. |
| **Settings** | Which credentials are configured, how many characters each is, and the environment variable each comes from - never the credential. |

Along the bottom of every workspace are the meters and the transport - the same
arm, record, pause, resume and stop the `vcw session` verb drives - and under
them a status line that says what the core last did, or what to do next if it
has not done anything yet. **While a record is playing, watch the meters**: the
waveform is drawn from what has been committed and it appears when the capture
stops, not as the side goes by. Nothing is missing while it is blank - the audio
is on disk within a quarter of a second of the stylus reading it either way.
`Ctrl+d` opens the event log, which is every event the core published in this
session and the first place to look when something did not do what you expected.

The waveform sits above Capture, Tracks and Export, which are the three
workspaces that point at positions in the recording, and not above Library,
Metadata or Settings, which do not. Under it are the channel it is drawing - a
stereo capture is two pictures and the picker chooses which - the range in view,
and the zoom controls.

Five things you can do to the picture with a pointer. **Click** anywhere on it,
on the rulers or on the track labels to move the playhead there. **Drag** across
it to select a stretch: the stretch lights up as you go and stays lit when you
let go. **Drag either ruler** to scroll, and the picture follows your hand the
way a map does. **Shift and drag** to scrub, which only does anything while
something is playing - the cursor turns into a hand when it will work.
**Wheel** to zoom about the pointer, so the peak under the cursor stays under
it, or hold `Shift` and wheel to pan instead.

A selection is what Play plays. Press `Space` or the Play button with a stretch
selected and the audition starts at the left edge of it and stops at the right
edge, which is how you listen to a track before there is a track - draw a band
around one, play it, move an edge, play it again. Clicking anywhere on the
picture puts the selection away again, and `z` zooms to it while it is there.

The bar under the picture is both where you are in the recording and how much of
it you can see, and you can drag it like a scrollbar. `-` zooms out, `+` zooms
in, and `z` zooms to the selection, or to the selected marker or track when
there is no selection.

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
carry - FLAC is an integer codec - so a float project leaves as **WAV** if you
want it lossless, or as **Ogg Vorbis** if you want it small. `vcw export
--format flac` refuses before it writes anything rather than producing half a
library.

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

### The release decides how the record is cut

Accepting a candidate in the Metadata panel does three things: it writes the
release row, it names the tracks, and it downloads the front cover into the
project. The cover is stored in the `.vcw` file, so it is backed up with
everything else and an export can embed it without going back to the network.

It also lays the sides out. Capture writes one side because one capture is one
take, so recording a double album in a single pass leaves you with seventeen
tracks all called `A`-something - a layout the record cannot have, since a
12-inch side holds about twenty-two minutes. When the release you accepted
lists exactly as many tracks as the project holds, its layout is adopted: the
sides it names are created against the capture your tracks are already in, and
each track moves to the face it is really on. The panel says how many moved.

No audio moves. The boundaries are frames of the same recording either way, and
two faces sharing one capture is what the project format was built for. If the
tracklist has a different number of tracks it is not this pressing, and nothing
is touched - you get the mismatch reported instead.

A track you have confirmed keeps its title through all of this. Confirming a
title is not the same as knowing which side it is on.

## Exporting

```text
vcw export side-a.vcw --into ~/Music --format flac
vcw export side-a.vcw --into ~/Music --format wav
vcw export side-a.vcw --into ~/Music --format mp3 --quality high
vcw export side-a.vcw --into ~/Music --format ogg --quality transparent
vcw export side-a.vcw --into ~/Music --dry-run    # the whole plan, no files
```

Files are laid out and named from a template, tagged from the release and the
track, and the cover goes beside them. The default template is
`{album_artist}/{album}/{tracknum} - {title}`, and `{tracknum}` is the position
printed on the label - so side A's first track is `A1 - The Rainbow.wav` and
side B's is `B1 - ...`. Five things worth knowing:

* **A track with no title is called `Untitled`.** Plenty of records have
  untitled sides, and plenty of provider rows come back with the positions
  filled in and the titles blank, which left the template's separator standing
  with nothing after it: `A1 -.flac`. The file is now `A1 - Untitled.flac`. The
  *tag* is left empty, because a blank title is the truth about the record and
  inventing one would put a made-up word in your library. If you would rather
  have `A1.flac`, set the template to `{tracknum}` and leave the title out
  altogether. There is no form that drops the separator only on the untitled
  tracks: a `[...]` group disappears when everything inside it is blank, but it
  keeps its brackets when it is not, so `{tracknum}[ {title}]` gives `A1` and
  `A2[ Sunshine Recorder]` on the same record.

* `--dry-run` resolves the whole plan - every path, every tag, every frame
  count - and stops there. It is the cheapest way to find out what an export
  will do, and the way to argue with a naming template without producing a
  gigabyte of files to delete. It **prints the file names**, numbered, exactly
  as the real run prints them, so you can hold one up against the other:

  ```
  $ vcw export rip.vcw --into ~/Music --format ogg --dry-run
    into       /home/you/Music
    format     Ogg Vorbis q6, template "{album_artist}/{album}/{tracknum} - {title}"
    tracks     7 file(s), 598732800 frame(s)
    artwork    106 byte(s) of image/png, embedded and beside the files
      1/7    Talk Talk/Spirit of Eden/A1 - The Rainbow.ogg
      2/7    Talk Talk/Spirit of Eden/A2 - Untitled.ogg
      ...
    cover      Talk Talk/Spirit of Eden/folder.png
    dry run    nothing was written
  ```

  The paths are relative to `--into`, which is on the first line. `--json` adds
  the tags, the frame spans and the track ids for anything that reads output
  rather than looks at it.
* An export **plans before it writes**. If the plan cannot be satisfied - a
  format that cannot carry the audio, a track with no boundary - it refuses
  before encoding, so you do not get half a library.
* **A refused export leaves nothing at all**, not even an empty directory. The
  container is checked against the capture while the plan is being resolved, so
  a format that cannot carry the audio is refused before the filesystem is
  touched. What can still leave files behind is a run that fails part way - a
  full disk, a disappearing drive - and there the files already written are
  real files and are left alone.
* **A 32-bit capture cannot leave as FLAC.** The FLAC encoder stops at 24 bits
  and narrowing 32 to 24 throws signal away, so `--format flac` refuses it by
  name rather than quietly dithering. WAV takes it losslessly and Ogg Vorbis
  takes it lossily; MP3 does not take it at all above 48 kHz. Capture at
  `--format s24` if FLAC is where the rip is going to live.
* **Above 96 kHz, FLAC refuses too.** `flacenc` stops there, though the format
  itself allows far more. A 176.4 or 192 kHz rip leaves as WAV or as Ogg
  Vorbis - which is the one container that will take any rate VCW records.

### The four formats

| `--format` | What it is | Takes |
| --- | --- | --- |
| `flac` | Lossless, compressed. The archival choice. | Up to 24-bit, up to 96 kHz |
| `wav` | Lossless, uncompressed. Takes everything, including float. | Anything, up to 4 GiB a file |
| `mp3` | Lossy, variable bitrate. The one every car stereo reads. | 1 or 2 channels at up to 48 kHz |
| `ogg` | Lossy Vorbis. Smaller than MP3 at the same quality. | Anything VCW records |

`--quality` applies to the two lossy formats and is ignored by the two lossless
ones, so you can leave it set while you change your mind about the format:

| `--quality` | MP3 | Ogg Vorbis | Roughly |
| --- | --- | --- | --- |
| `transparent` | V0 | q8 | 245 kbit/s - as close to the record as the codec gets |
| `high` | V2 | q6 | 190 kbit/s - the default |
| `compact` | V5 | q3 | 130 kbit/s - small enough to stop thinking about |

**MP3 refuses a high-rate capture.** MPEG never defined a sample rate above 48
kHz, so a 96 or 192 kHz rip cannot become an MP3 without resampling it - and
resampling means choosing an anti-alias filter, which is a decision about how
the record sounds and not one an exporter should make on your behalf. The
refusal names the rate it is looking at. Export that rip as Ogg Vorbis, which
has no such limit, or as WAV. FLAC is only an option at 96 kHz or below.

The lossy formats are for the copy you carry around. Keep the lossless one.

### In the window

`Ctrl+5` is the export panel, and it is the same two steps. **Browse...** opens
the system's own directory chooser, starting at your library, so the output
directory does not have to be typed. **Plan** resolves it and lists every file
with the path it will have. **Export** (`Ctrl+E`) writes, reports `Writing 2 of
3...` as it goes, and finishes with a line saying what came out:

```text
Wrote 3 files, 1 cover image, 273.6 MiB.
```

The format and quality selects are beside the directory, and the quality one
appears only when the format is MP3 or Ogg Vorbis. Changing any of them drops
the plan, because a plan resolved against the old settings describes files
nobody asked for - down to the file extension.

### The tags

A track goes out with its title, artists, album, genres, year, composer,
comment, label, catalogue number, country, barcode and both MusicBrainz ids,
plus the cover and the `VINYL_POSITION` the record was cut at. **FLAC and Ogg
get Vorbis comments; WAV and MP3 get a full ID3v2 tag** - in a chunk after the
audio for WAV, at the front of the file for MP3, which is what every player
reads. The field set is the same in all four, and so is the embedded cover.

Track numbers are per record, not per release, and they carry their total: on a
double album the second disc is `1/8` upwards rather than `9/17`, because that
is what a track number means to everything that will read it.

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
| `+` or `=` | Zoom the waveform in | `place-marker` | - |
| `-` | Zoom the waveform out | `place-marker` | - |
| `0` | Show the whole capture | `place-marker` | - |
| `z` | Zoom to the selection, the selected marker, or the selected track | `move-marker` | - |
| `?` or `F1` | Show the keyboard map | `help` | - |
| `Ctrl+1` | Library | `navigate` | - |
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
