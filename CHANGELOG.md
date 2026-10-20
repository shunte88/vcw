# Changelog

*VCW - The Vinyl Capture Workstation. (c) 2026 Stue Hunter. MIT License - see
the header in any Rust source file for the full text.*

This file records what changed in each release, for the person installing it.
Every entry is something somebody can see: a capability, a fix, a refusal that
used to be a crash. The work packages behind them are in `PROJECT_PLAN.md`, and
`docs/STATUS.md` is the running snapshot of what is built and what is not.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
versions are [semantic](https://semver.org/spec/v2.0.0.html). While the major
version is 0 the project format may change between releases; every release from
0.1.1 onwards reads every project written by an earlier one, and says so out
loud if it cannot.

## Unreleased

### Added

* **Exported files carry their loudness.** Every track is measured as it is
  written and tagged with ReplayGain: how far it sits from the reference
  level, how close it came to clipping, and the same two numbers for the side
  as a whole. A player that understands the tags levels a shelf of transfers
  made on different days at different settings, and one that does not plays
  the files exactly as before. **The audio is not touched.** VCW writes the
  measurement down and never applies it, which is the only version of this
  feature that can coexist with a bit-perfect export - see
  `docs/adr/0008-normalization-is-measurement.md`. On by default; the Export
  panel has a tick and the command line has `--no-replaygain`.

### Fixed

* **The `a` key no longer reaches for the sound card in the middle of a
  take.** Arming is one action with four ways in - the Capture panel's
  button, the toolbar's Set Levels, the needle-drop trigger and the key - and
  the key was the one that never asked whether the transport could be armed
  from where it was. Pressing it while recording put a refusal in the status
  bar instead of doing nothing at all.

## 0.3.8-alpha

2026-10-20

### Added

* **The stylus can start the recorder.** Cue a side the way you always have -
  press Needle Drop, walk to the deck, lower the arm - and VCW starts writing
  when the groove does. It arms, begins the capture and holds it paused; the
  first signal that stays above -35 dBFS releases it. The threshold is not a
  guess: it was measured off four real needle drops, where the silence before
  the stylus lands sits fifty decibels below the landing itself. A click on
  the lead-in is too brief to fool it.

  Nothing about the manual sequence changed. Record, Pause, cue, Resume still
  works exactly as before; this is a second way in, not a replacement.

* **Set Levels, on the toolbar.** Opening the device to check levels before a
  take used to mean finding the Capture panel's Arm button. It is now on the
  transport bar beside Record, and it is the same action - the meters go live,
  nothing is recorded, and Stop closes the device again.

  There is deliberately no gain slider beside it. What the sound card delivers
  is what VCW writes, and a fader in front of the writer would end that. Set
  the level where it is actually set: the phono stage, the interface's input
  trim, or your operating system's mixer.

* **A backlight behind the VU meters.** Off by default, with a warm classic
  bulb and a blue one to choose from in Settings under Appearance. It is lit
  behind the dial face only, so the needles and the scale read exactly as they
  did before.

### Changed

* **The equalization picker now knows what your phono stage already did.**
  Settings asks what the signal carried when it reached the card, and that
  answer now defaults to RIAA - which is what all but a handful of setups
  have. Each of the sixteen curves carries its own tracking help written for
  that answer, instead of one paragraph underneath all of them describing a
  chain that might not be yours. The warning about undoing RIAA first appears
  only when there is RIAA to undo.

  This changes advice, not audio, and nothing already recorded. A capture that
  never said what its chain was still says so.

* **Rename a track where you can see it.** Double-click a track's label on the
  waveform and edit it in place. Enter or Tab commits, Escape abandons, and
  clicking away saves what you typed.

* **The project list leads with the artist**, then the album, then the catalog
  number.

* **A new user guide**, written for the person with a record in their hand
  rather than the person reading the source. It ships as Word, PDF and
  Markdown together with its diagrams and screenshots in
  `docs/VCW-User-Guide/`, and it replaces `docs/USER-GUIDE.md`, which is gone.
  The one part of the old guide the new one does not cover - writing a message
  catalog - moved to `docs/TRANSLATING.md`.

  Its opening paragraph says which version it was prepared against, and a
  release now restamps that in all three formats together, so the guide in the
  package always names the build it shipped beside.

### Fixed

* **Pressing Stop between sides no longer ends the session.** The manual the
  guide teaches is Pause between sides, and that always worked - but a person
  who reached for Stop instead, which is what Audacity trains you to do, found
  the transport would not come back. Set Levels disappeared, Record and Needle
  Drop refused, and the Capture panel's Arm button stayed lit and did nothing
  until the application was restarted. It now arms again from a finished
  capture, so either habit records both sides of a record.

* **A reloaded window knows what the recorder is doing.** The transport read
  "idle" over a recorder that was running until the next thing happened, which
  made every button in the bar wrong for as long as it lasted. It now asks on
  open. This matters most over `vcw serve`, where refreshing the page is an
  ordinary thing to do and the deck is in another room.

* **A take that caught nothing no longer appears in the list.** Pressing
  Needle Drop and walking away, or starting a take and stopping it before the
  stylus was down, filed a finished side with no audio under it - one for
  every change of mind, sitting in the captures list beside the real ones.
  Disarming always cleaned up after itself; stopping now does the same.

* **The clock agrees with the capture it is over.** The elapsed time only ever
  caught up while a recording was running, so every moment either side of one
  showed the wrong number: a freshly armed deck wearing the last take's four
  seconds, and a finished capture of a quarter of a million frames reading
  00:00:00.000. The position is now published whenever the transport moves,
  including when a failed writer ends a capture by itself, and a window that
  has just opened is told the time along with the phase.

### Thanks

* Jim Ross, for forty years of audio engineering at the BBC and then NPR, and
  for reading the manual before anybody else saw it.

## 0.3.7-alpha

2026-10-19

### Added

* **A click VCW found is now a click VCW can mend.** The detector in 0.3.6
  wrote down where the damage was and changed nothing; this build acts on
  those rows. The repair happens on the way out - in playback and in every
  exported file - and the capture in the project is never touched, so the
  same project can be exported repaired today and untouched tomorrow with no
  copy kept and nothing to undo.

  A gap is drawn across with a curve fitted to the groove either side of it.
  Ten milliseconds is the limit: beyond that a curve is invention, and a gap
  too long to mend is silenced instead, which is the honest failure. The
  decision is made about the damage and not about where it happens to fall in
  the file, so the same click cannot be drawn across in one place and muted
  in another, and a defect sitting on a track boundary is mended once, the
  same way, in both files it lands in.

  The mended samples land on the converter's own grid. A sixteen-bit rip
  carried in a wider container has sixteen unused bits under every sample,
  and one value between two steps the converter could never have produced
  throws that away - the first declicked FLAC came out 11.6% *larger* than
  the untouched one for thirty-two changed samples. It now comes out smaller,
  which is the right answer: a click is an expensive thing to encode.

* **The defects are drawn over the waveform, and you can overrule them.**
  Every span the detector found is marked on the audio it describes, and a
  double-click on one flips it between mend and leave alone. A vetoed defect
  stays on screen rather than vanishing, because a veto you cannot see is a
  veto you cannot undo, and nothing you have ruled on yourself is touched by
  a later detector run.

* **An exported file says what was done to it.** Anything VCW applied to the
  samples is written into the file as `VCW_PROCESSING`, in the order it was
  applied - the repairs, and the playback curve where one was used. Both
  repair numbers are there, a zero included: *"412 repaired, 0 vetoed"* is a
  side somebody reviewed, and *"412 repaired"* on its own could be a side
  nobody looked at. A file nothing was done to carries no such tag, and the
  bit-perfect verdict in the window agrees with it, because the two are
  generated from the same answer.

### Fixed

* **The Linux packages install and run on Debian 12, Ubuntu 22.04 and
  Raspberry Pi OS Bookworm again.** They were built on a runner with a newer C
  library and refused to start anywhere older, with
  `` version `GLIBC_2.39' not found `` and nothing else to go on. Two unused
  symbols were the whole cost; the floor is now glibc 2.35, it is stated as a
  number the build checks itself against, and the `.deb` says so in its own
  dependencies so a machine too old to run it declines to install it rather
  than failing at first launch.

## 0.3.6-alpha

2026-10-18

### Added

* **`vcw import` now takes a WAV file, not only an Audacity project.** Point it
  at a rip and it lands as a capture like any other - playback, the waveform,
  detection, tagging and export all work on it with nothing special about where
  it came from. The audio is copied, not converted: 16, 24 and 32-bit integer
  PCM at whatever rate and channel count the file declares, byte for byte into
  the blocks. A file that would have to be converted to land is refused and
  says which part of it was the problem, because an import that quietly changed
  the audio would be indistinguishable from one that did not.

  Nothing is taken on trust from the header. The audio is found by walking the
  container's chunks rather than assuming it starts at byte 44, which it does
  not in any file that has been tagged or recorded above 16 bits, and the
  length the file declares is checked against the length it has - a recorder
  killed mid-take leaves a header it never went back to correct. If the two
  disagree the import says so and lands what is really there.

  `--dry-run` reports what would land without writing, as it already did for an
  Audacity project, and a source that is neither kind now says so instead of
  reporting that it is not a database.

* **VCW can find the clicks in a side.** `vcw declick <project>` fits a model to
  every fiftieth of a second of the capture, inverse-filters it and reports what
  the model could not predict - which is what a click is. It writes one row per
  span of damage and changes no audio at all: the samples stay exactly as they
  were recorded, and what gets stored is where the damage is and what to do
  about it. Nothing acts on those rows yet; repairing them is the next piece of
  work.

  The run says, before anything else, what fraction of the side it flagged.
  That number is the whole reason to look: the published settings this detector
  is built from were tuned on 78s and mark five to seven percent of every sample
  on that material, which on an LP would be nonsense.

  So the settings were derived again, on records. Eight sides, four and a half
  hours of it, from solo piano to dense noise, swept across the whole useful
  range of the threshold; and separately three hundred clicks of a known size
  put into one of those sides at known places, so that there was something to
  miss. The clicks are all found well past the point where the false alarms
  stop, which is what settles it: the shipped threshold flags between one
  sample in forty thousand and one in nine thousand of a real side, and still
  finds nineteen clicks in twenty at the faintest size tested - a click no
  louder than the music around it. What it will not do is tell you a clean
  record is five percent broken.

  All six settings are on the command line - `--threshold`, `--burst`,
  `--window-ms`, `--overlap`, `--order` and `--passes` - so a side that reads
  wrong can be argued with. `--dry-run` reports without writing, and `--spans`
  lists every one found.

  Run it twice and you get one answer, not two: a second pass replaces the
  first pass's findings rather than adding to them, so raising the threshold
  shortens the list. Anything you have ruled on yourself survives untouched.

### Changed

* **Projects now carry a list of defects, and upgrade to schema 6 when opened.**
  The table is empty: nothing writes to it yet. It lands first on purpose, so
  that the click detector that fills it arrives as a detector rather than as a
  detector and a migration at once. What you can see today is the version
  number: a project opened by this build is schema 6, and VCW 0.3.4 and earlier
  will say it was written by a newer VCW and decline to open it. Reading in the
  other direction is unchanged, as it has been since 0.1.1 - this build opens
  every project any earlier one wrote, upgrading it on the way in, and `vcw
  migrate <project>` does it without opening anything else.

## 0.3.4-alpha

2026-10-17

### Added

* **VCW can work out which release a project is, on its own.** `vcw identify
  <project>` and the Metadata panel's **Identify** button take what the project
  already holds - whatever was typed at setup, how many tracks were laid out,
  how long each captured side runs - and escalate through the providers until
  the evidence settles: the catalog number at Discogs first, then the two name
  searches, then the audio. It stops at the first lookup that answers, so a
  record whose catalog number is on the sleeve costs one request, and the
  report says how many were made. Nothing is written by the walk itself: even a
  release it is certain of is offered to be accepted, because choosing between
  two pressings is the operator's decision. `--apply` is the opt-in for the
  command line, and it refuses a shortlist.

* **The window says when a newer release is out.** On launch VCW asks the
  repository what the newest published release is, and if it is ahead of the
  running build a single **Update to ...** button appears beside the title. It
  opens that release's own page in your own browser and does nothing else: VCW
  does not replace itself, because the copy on disk belongs to whatever
  installed it - a package manager, a deb, or an unpacked archive on a Pi. The
  check is one request with a three second ceiling, it is silent if it fails,
  and turning the network off in Settings turns it off too.

* **A shortlist says what tells two pressings apart.** A walk nearly always
  comes back with several pressings of one record, which agree on everything
  that was weighed. The country and the format are now carried through to the
  list, where before every row read identically.

* **The pressing can be corrected after setup.** Mono and the cutting
  characteristic were asked for once, at new-project, and could not be changed
  afterwards from the window - only from the command line. The Metadata panel
  now carries a **The pressing** box with both, and the Tracks panel's side
  header carries a per-side **Cut with** picker, so a two-sided capture whose
  faces came from different pressings can say so.

* **`vcw migrate <project>`.** An older project has always been brought
  forward when a newer VCW opens it, but only the window ever did that, and it
  did it without saying so. The verb makes the same upgrade something a person
  asks for and watches: it prints the version the file is at, the version this
  build wants, and a line for every step in between. `--dry-run` prints exactly
  the same report and writes nothing, and a project that is already current says
  so and exits cleanly, so it is safe in a script. It does not keep a backup,
  and it says so before it writes: a migration only goes forwards, so an older
  VCW will refuse the file afterwards. This is for anybody working without a
  window - a headless box or a Pi - who until now got upgraded by accident.

* **`vcw fingerprint --duration <SECONDS>`.** AcoustID filters on the declared
  duration exactly, so a 30-second excerpt that declared itself as 30 seconds
  could never match the four-minute recording it came from. `--identify` over a
  `--length` window now says what it is an excerpt *of*, and warns when it has
  not been told.

### Fixed

* **`vcw migrate` now checks what the project holds, not only what version it
  says it is.** A project's version number records which migrations ran, and
  every one of them did run - but the first migration only ever runs against an
  empty database, so an edit to it reaches every project made afterwards and no
  project made before. One such edit happened in this project's own history:
  two covering indexes were added to the waveform tables on 2026-09-25, and a
  project written the week before has neither, is current by every number in
  it, and answers `vcw waveform` with a SQL error. That edit landed before
  0.1.1-alpha, so no released VCW ever wrote such a file - the one on this
  machine is a development soak project, and it is what the check was built
  against. `vcw migrate` now compares the project
  against the schema this build would create and names anything missing,
  anything it does not recognize and anything whose definition differs, with
  the statement that puts it right; `--json` carries each statement whole, so
  the report is the remedy. A project this build wrote has nothing to report
  and says nothing. The check waits until the version number is current,
  because before that the answer is just the migration that is about to run.

* **A capture that starts before the stylus lands still finds its tracks.** The
  adaptive threshold estimates the noise floor from the quietest 3% of a
  capture, which is the groove on a capture made at the deck. Start the capture
  from a browser in another room - which is what `vcw serve` is for - and the
  quietest 3% is the minute you spent walking to the turntable, roughly 35 dB
  below any groove. The threshold went with it, every lead-in and inter-track
  gap read as music, and the side came back as one track. Levels too quiet to be
  a record are now held out of that estimate, so the floor lands in the groove
  whether or not the recording opens with an empty room. A capture made at the
  deck is unaffected: the test is on the answer, not on the audio, and it only
  engages when the estimate has come back below anything a record produces.

* **Looking at a project no longer upgrades it.** `vcw waveform` and `vcw
  release <project> show` took a handle that can write, and opening a project
  for writing *is* the schema migration - so asking what was in a project
  silently brought it forward a version, and copying a project to be safe before
  looking inside the copy defeated the copy. Both now read. `vcw waveform` draws
  an old project as it is, because nothing it draws is newer than the first
  schema; `vcw release show` and `vcw release covers` say to run `vcw migrate`,
  because the release tables they read arrive in the second. `--rebuild` and
  every `set` still write, and still upgrade, because asking for one is asking
  for the other.

* **An old project is refused by name rather than by SQLite.** `vcw export`,
  `vcw identify` and `vcw play --track` against a project written before the
  vinyl data model existed answered `no such table: releases` - a true statement
  about a database and a useless one to put in front of somebody holding a
  record. `vcw fingerprint --tracks` was worse: it ran a full detection pass
  first and then said no boundaries were agreed, which named neither the cause
  nor the cure. All four now say which file, which version, and to run `vcw
  migrate`. The readers that are genuinely happy with an old project - playing a
  capture, fingerprinting a span given by time - are untouched.

* **`vcw eq list` no longer says nothing selects a curve.** Its closing note,
  and the command's own help, were written for 0.3.2-alpha when the catalog
  really was read-only. 0.3.3-alpha shipped them unchanged, telling anybody who
  ran the command that a feature they had just installed did not exist. Both now
  point at `vcw release album.vcw set --playback-eq <id>`.

## 0.3.3-alpha

2026-10-15

### Added

* **Say what the record was cut with.** The sixteen curves 0.3.2-alpha made
  readable can now be chosen, and the choice is kept in the project. Set it for
  the whole release with `vcw release album.vcw set --playback-eq columbia_lp`,
  for one side with `vcw tracks album.vcw side-eq --side A --curve decca_lp`, or
  for a single track with `vcw tracks album.vcw set 3 --playback-eq emi` - a
  compilation cut from several sources needs all three. A level that says
  nothing inherits from the one above it, and a project that says nothing
  anywhere is RIAA, which is what almost every record after 1954 is.
  `vcw tracks list` prints the curve in force at each level, not the one written
  there, so a track that inherits still shows what it will be played back
  through. Clearing is `--playback-eq ""` on a track and `side-eq --side A` with
  no curve. A name no curve goes by is refused, and the refusal lists the ones
  that exist.
* **The new-project sheet asks.** The RIAA checkbox is now a picker listing
  every curve with the era it belongs to. It opens on RIAA.
* **The curve is applied.** What the last release could only record, this one
  plays: the characteristic in force is undone on the way out of the project
  every time you audition a side, render it to a file, or export it, and never
  on the way in - the capture keeps the samples the stylus made. The curve on
  screen beside the playhead is the one you are hearing, the event log says
  which it was, and a side playing through one is reported as not bit-perfect,
  because it is not. Response is within 0.08 dB of the published characteristic
  from 20 Hz to 20 kHz at every rate VCW records at.
* **Exported files say what was done to them.** A file written with a curve
  applied carries `VCW_PLAYBACK_EQ` - the curve's name and its code letter, as
  `Columbia LP (500C-16)` - and `VCW_PLAYBACK_EQ_APPLIED`, which is `yes` or
  `no`. A corrected transfer and one still needing correction are otherwise
  identical files, and in an archive that difference has to survive the
  catalog. A project that states no curve carries neither tag.
* **State what your own signal chain did, after the fact.** `vcw eq provenance
  album.vcw flat` says the chain was flat, `riaa` says it applied RIAA, and
  `unknown` takes it back; `--capture N` names one capture rather than all of
  them. The capture panel grew a **Chain** column that does the same per row.
  Without this an imported project could never be played through a curve: VCW
  refuses to correct audio nobody has described, and Audacity does not record
  what the preamp did.

### Changed

* **The RIAA box became a question with sixteen answers, and a clearer one.**
  It used to ask whether to apply RIAA, which meant holding two facts at once:
  what the record was cut with, and what your phono stage had already done
  about it. The app had to warn you about getting it wrong and could not help
  further. It now asks only the first - what the record was cut with - because
  VCW already records what your signal chain did, and works the rest out. The
  advice that remains appears only when the curve is not RIAA, and says the one
  thing worth knowing: an ordinary phono stage undoes RIAA whatever the record
  was cut with, so putting a different characteristic right means undoing RIAA
  first, and that lifts the noise along with it.
* **Projects are upgraded on open, as always.** A project written by 0.3.2 or
  earlier gains the new field; a box you had ticked becomes RIAA, and one you
  had not becomes unstated rather than a decision you never made. The new field
  is a decision, not an edit: it is applied on the way out and the stored
  audio is never touched, so changing your mind costs nothing.

### Fixed

* **A session script refuses an option typed onto a verb.** `--script 'arm
  --capture-eq flat,record'` armed, ignored the option, and left the capture
  with nothing stated - which only showed up much later, as a refusal to apply
  a curve. The verb now fails on the line that has the typo in it and says
  where the option belongs.
* **`vcw eq list | head` no longer panics.** Closing a pipe early - `head`,
  `less` and a quit, a script that stops reading - printed a Rust panic and a
  failure exit code from any VCW command. It now ends quietly and successfully,
  the way every other command-line tool does.

## 0.3.2-alpha

*2026-10-12.*

The equalization curves a record was actually cut with. Sixteen of them, with
the published source each one's numbers came from. Nothing selects a curve yet
- that is the next piece of work - but the catalog is here, it is readable, and
its accuracy can be checked by the person relying on it rather than only by the
test suite.

### Added

* **`vcw eq list` and `vcw eq show <id>`.** RIAA, and the fifteen label and
  standards-body characteristics that came before it or alongside it: Columbia
  LP, Decca FFRR, EMI/HMV, NAB, AES, Teldec, CCIR, London, the 78 curves and the
  flat-treble ones. `show` prints three things worth having for each. The
  **source** is a citation to the published document the parameters came from,
  because a curve nobody can trace is a curve nobody can check. The **response**
  is computed from the curve's own time constants rather than copied off a
  chart, which matters more than it sounds: in most published sources a figure
  given as "dB at 10 kHz" describes the treble section alone and not the
  finished curve, and a careful reader who takes it at face value ends up 1.5 dB
  out. And the **measured error** is how far the filter VCW really builds
  departs from that definition, at every sample rate VCW records at. The budget
  is 0.5 dB from 20 Hz to 20 kHz; the worst curve at the worst rate is 0.078.
* **Your own curve, in the same terms.** A bass turnover, a treble rolloff, and
  where it has one a bass shelf, in a TOML file merged over the built-in set
  with `vcw eq list --curves my-curves.toml`. An id that already exists replaces
  that curve, a new one is added, and a citation is required on yours as much as
  on ours - "measured on my own bench, March 2026" is a perfectly good one.
* **Where the sources disagree, the entry says so.** Columbia LP's bass shelf is
  published as both 1590 and 1950 microseconds, which is 1.675 dB apart at 20 Hz
  and more than three times the tolerance. `vcw eq show columbia_lp` prints the
  number VCW uses, the number you will find elsewhere, and the evidence that
  decided between them.

## 0.3.1-alpha

*2026-10-11.*

Three things a piCorePlayer user found in the first hour of the 0.3.0 browser
build, all in the window.

### Added

* **Make a track out of what you selected.** Drag across the waveform and press
  *Add track from selection* in the Tracks panel. That is the verb that was
  missing: a marker on its own does not make a track, and until now there was
  no way to say "this stretch is a track" without the CLI. The side renumbers
  itself afterwards in timeline order, so a track added in front of the others
  becomes the first and the rest move down, and a stretch that lies over a
  track that is already there is refused with both spans named rather than
  written.
* **Drag a boundary to move it.** Point at a marker on the waveform and the
  cursor says it is grabbable, the tooltip names which edge of which track it
  is, and dragging it puts it where you let go.
* **Delete a marker.** Every row in the boundary list has a Delete button. One
  a track is using is refused - merge or delete the track first - and one that
  bounds nothing goes away.
* **What the input device actually is.** The device picker now reads
  `name - interface - transport`, and under it a panel gives the identity
  string, the host API, the path, the channel counts, the sample rates and the
  sample formats the host reports. Picking the right one out of an ALSA list of
  plug devices no longer takes `arecord -L` in another window.

### Fixed

* **A marker a person placed can now be moved and deleted.** Every hand-placed
  boundary is locked - that is what makes it survive re-analysis - and the
  window was then refusing to move or delete it, so a misplaced marker was
  permanent. A person moving a boundary is a person placing it: the window now
  says so, and the lock still holds against the detector.
* **A marker you place appears.** The Place marker button and the `m` key wrote
  the boundary and did not reload, so nothing was drawn, nothing was listed and
  nothing could be deleted until the project was reopened.
* **An Audacity label that overlaps another no longer costs the whole import.**
  Audacity stacks region labels freely and a VCW side may not; one such label is
  now named in the import report beside the three other kinds that cannot be a
  track, and the rest of the project lands. Marking tracks in Audacity and
  importing the result is a supported route, and the first one to try if you
  would rather work there.
* **The settings panel no longer types over you.** Settings save themselves, and
  the save refetched and adopted the answer while a field was still being
  typed into - a millisecond round trip on the desktop, a keystroke eater over
  `vcw serve` to a Pi in another room. A field is written when you leave it or
  press Enter, never mid-word, and an edit that has not been written yet says
  *Unsaved* beside the panel title.

## 0.3.0-alpha

*2026-10-11.*

Three things this release, and the first one is a second way to reach VCW.

### Added

* **`vcw serve` - the same window, over a socket.** A machine with no desktop
  on it can still be driven: `vcw serve` puts the window on a port, and a
  browser anywhere you can reach it gets the panels, the meters, the waveform
  and the keyboard map. Audio still plays on the VCW host, which the transport
  says out loud. Every request is authenticated, loopback included: the token
  comes from `VCW_SERVE_TOKEN` or is minted for the run and printed once, and
  nothing is written to disk. The connection is plain HTTP, which is said out
  loud too - put it behind a tunnel or a reverse proxy if it leaves the
  machine. A wildcard bind is refused until you name the host you will type.
* **ALAC, in an `.m4a`.** Apple Lossless joins WAV, FLAC and AIFF as a
  bit-perfect export, at 16, 24 and 32 bits and at every rate VCW captures.
  Tags go in as the atoms iTunes and Apple Music read. This replaces the AAC
  encoder the plan used to name: a lossless format fills the gap a lossy one
  does not.
* **The window reads its sentences from a catalog.** Every word the window
  says is now one of 472 entries in `i18n/en-US.toml`. To add a language, copy
  that file out of the repository, replace each value with yours, and put it
  beside your settings file as `i18n/<language tag>.toml`; it appears in
  Settings as soon as you reopen the panel, and anything you have not
  translated stays in English rather than going blank. The CLI's reports and
  the error text in a diagnostic bundle stay English on purpose - they get
  piped, grepped and pasted into bug reports.

### Changed

* **Every package now carries the browser UI.** The deb, the AppImage, the
  `.msi` and the macOS app all ship the built frontend, so `vcw serve` finds
  it with nothing stated. Before this only the deb had it, and only at a path
  no other platform used.
* **No source map in the shipped build.** It was 1.6 MB of the deb, four times
  the bundle it described.

## 0.2.2-alpha

*2026-10-10.*

The same person, a day later, now editing tracks. He merged two of them by
mistake and could not get them back - and the reason turned out to be
underneath the three things that each looked like the cause.

### Fixed

* **The playhead is a position you can put somewhere.** It used to move only
  while audio was playing and snap back to zero when it stopped, which made
  **Split at playhead** and **Place marker** two buttons nobody could aim: the
  only way to put the playhead on a frame was to play up to it and press the
  button at the right instant. Click the waveform and the playhead goes there.
  Nothing plays.
* **A click near a track boundary snaps to it.** Within a few pixels of a drawn
  boundary the playhead lands on the frame it was drawn at, not on the pixel
  you managed to hit - so splitting a track exactly where it was split before
  is one click and one button. This is the way back from a merge you did not
  mean: the audio was never touched, and the cut can be made again on the
  sample.
* **The playhead readout no longer hides when nothing is playing.** The number
  Split is about has to be legible exactly when you are lining Split up.

### Documentation

* **A project is one release, not one side.** The guide said a project normally
  holds one side of one record. The format says otherwise - one file is one
  release, both faces of it, and both discs of a double - which is why the
  library lists a project by album and why a metadata lookup matches a whole
  tracklist against it. The example filename throughout is now `album.vcw`
  rather than `side-a.vcw`, which had been quietly teaching the wrong thing.
* **How to move where a track starts and ends**, which the guide never said.
  There is no "make this an end" or "make this a start": adjacent tracks share
  one boundary, so an edge is both at once and moving it moves both. The new
  section covers that, what Split and Merge each do, and that **Merge has no
  undo** - it discards the boundaries between the tracks it joins unless they
  were locked.

### Added

* **A "Thanks to" section in the About dialog**, with the first person to run
  VCW who did not write it at the top of it.

## 0.2.1-alpha

*2026-10-09.*

The first release shaped by somebody else's record player. A piCorePlayer user
recorded a side on a Raspberry Pi, copied the project to Windows and opened it
in the window, and four things went wrong in a row. Three of them were ours.
One of them loses audio, so install this one.

### Fixed

* **A capture whose disk fills no longer goes on silently.** The writer stops
  on a failed commit rather than leave a hole in the recording, and until now
  nothing said so: the only sign was a position that stopped moving, which is
  also what a pause looks like. One person went on recording into a stopped
  writer for ten minutes before pressing stop told them. The transport now
  checks on its own, ends the capture the moment the writer dies, and keeps
  everything that did reach the disk.
* **Stop no longer says "refused" when it failed.** A stop that fails does
  move the transport, so saying nothing happened was wrong twice over: the
  window kept showing *recording* against an engine that had already stopped.
  It now reports what happened and where it ended up.
* **Detection works on a project recorded with `vcw session`.** The window
  would draw the waveform and play the audio and then refuse to analyze it,
  because the command line does not write the side row the detector insisted
  on. Three separate places insisted on it; none do now, and detection also
  records which recording a side came from, so markers and the release layout
  work afterwards.
* **A one-sided rip can be told which side it is.** The Tracks panel has a
  side picker beside Detect. Without it, a side B recording had its tracks
  filed under side A, and identification then wrote side A's titles over side
  B's music with nothing to show for it.

### Added

* **A fresh install opens where there is something to do.** With no library
  set the window landed on an empty browser that said nothing about why it was
  empty. It now opens *Settings > Library* with the field focused, and the
  status line names what is missing instead of pointing at an empty panel.
  Only on a first run: once you have chosen a settings group, that is where
  you land.
* **`vcw soak --max-pages`** fills the project's disk on purpose, which is how
  the fix above is tested without a real full disk.

## 0.2.0-alpha

*2026-10-08.*

Another release about the window, and the first one you can reshape: the
buttons, the track numbers and the settings all now do what you tell them to.
Nothing here changes a byte that gets recorded.

### The window

* **Buttons can be icons instead of words.** *Settings > Appearance > Buttons*
  switches the panel tabs along the top and the transport along the bottom at
  once, and gives both rows back about a third of their width. Every button
  keeps its name: the tooltip still reads `Record (r)` and a screen reader
  still hears the word. Remembered on this machine only, like the interface
  scale and the meter style.
* **Opening a project lands on Tracks**, which is where you were going.
* **An expanded event-log line reads white on the accent blue**, so the line
  you opened is obvious among the ones you did not.

### Settings

* **Settings save themselves and the Save button is gone.** Change anything and
  it is written a moment after you stop. While that write is outstanding the
  panel says `Saving` beside its title; if the core refuses the value the word
  stays and the reason appears in the status line, so the panel only ever shows
  you what is actually stored.

### Export

* **Track numbers have three spellings.** *Settings > Export > Track numbers*
  chooses between the position on the label (`A1`, `B2`) and a plain number,
  and *Counted* then chooses whether the number restarts on each side (`01`
  again on side B) or runs across the sides of one disc (`01`..`0n`, restarting
  on the next disc). The command line takes the same choice as
  `vcw export --numbering`. It applies to the export in hand and is not written
  back to the project: exporting once as a sequence does not renumber the
  record.
* **FLAC narrows a float capture at 24-bit by default** rather than refusing
  it. 24 bits is where an `f32` significand survives intact, with a triangular
  dither and no attenuation. *Settings > Export* still changes the width, the
  dither and the headroom, and can be set back to refusing.

### Under it

* **The application's icons are its own files.** The webview used to keep a
  second copy of each glyph, which had already drifted from the first. There is
  one copy of each now, and no editor state ships inside it.

## 0.1.3-alpha

*2026-10-07.*

A release about the window rather than the audio. Nothing here changes a byte
that gets recorded or exported; all of it is about what the application tells
you while you use it.

### About

* **Ctrl+I opens the About dialog**, which previously had a button and no key.
* **The logo is the way to the source.** Clicking it opens the repository in
  your browser.
* **A sentence saying what this is**, read from the project's own description
  rather than typed into the dialog, so it cannot drift from what the
  repository says.
* **Every third-party component has a globe beside it** that opens that
  component's source. The addresses used to sit in a column that had been
  invisible for some time; a mark you can click is both shorter and the thing
  the license obligation actually asks for.

### Settings

* **Metadata reads as switches rather than tickboxes.** *Allow network
  lookups*, *MusicBrainz* and *Discogs* are the same three settings, shown the
  way the rest of the application shows a two-state choice.
* **Each one is explained where it is, rather than in a paragraph underneath
  all of them.** In particular: *Allow network lookups* is the only setting that
  stops an AcoustID fingerprint lookup or a cover art download, neither of which
  the other two cover - so turning both catalogs off is not the same as going
  offline.
* **Get Discogs API Token** opens the page where you generate one.
* **The genre map field says what a genre map is**, and what a line in one looks
  like.
* **Credentials explains why a variable you exported can read as not set.** A
  desktop launcher starts VCW from the session environment rather than from your
  shell, which is the usual reason; the fix differs per platform and is now
  written down.

### Event log

* **Double-click a line to read it in full.** Rows are one line each so the log
  stays scannable, which clipped exactly the thing worth copying into a bug
  report: a refusal carrying generated advice.
* **A refused command now appears in the log.** Until now a refusal was shown in
  the status bar and then forgotten, so the log - the panel a bug report is
  copied out of - was missing the event the report is about. The line names the
  command: `detect`, `split`, `merge`.
* **The filter no longer takes focus when the log opens.** It did, which made
  the status bar's standing offer of "? for the keyboard map" a lie for as long
  as the log was up: a text field is right to take a literal `?`.
* **A long refusal wraps in the status bar** instead of being clipped at the
  window edge.

### Recording from a script

* **`vcw session --script` now records for as long as the script says.** `arm`
  sent the command and moved straight on, but arming is where the device is
  opened, so a script that armed and immediately recorded was sleeping through
  the device opening rather than through the recording. On ALSA that costs
  21 ms and nobody noticed; on WASAPI it cost 1.6 s, and `arm,record,sleep
  3,stop` produced a side of 1.37 s. `arm` now waits for the engine to say it is
  armed, bounded at ten seconds, and released at once by a refusal.

### Library and Tracks

* **The first project is selected when the library loads**, so Enter opens
  something without an arrow key first.
* **A project VCW cannot read shows dashes rather than zeros.** "0 sides, 0
  tracks" is a statement about a record; a dash is a statement about VCW. The
  reason is spelled out on the selected row.
* **The Tracks panel says what is missing.** With no project open it points at
  the library, with no capture it points at Capture, and only then does it talk
  about detection. Detect is disabled until there is a capture to detect in, and
  the waveform appears once there is audio to draw rather than as an empty frame.

## 0.1.2-alpha

*2026-10-06.*

### Export

* **FLAC now takes the format VCW records by default.** A device negotiation
  asks for the widest integer format the hardware offers, so an ordinary rip is
  32-bit, and 32-bit had no FLAC path at all: `vcw export --format flac` refused
  it and sent you to WAV. The same went for anything above 96 kHz, which ruled
  out a 192 kHz capture outright. Both ceilings are gone. FLAC carries up to
  32-bit integer at any rate VCW records, and the refusal that used to name a
  rate limit no longer exists.
* **A 32-bit float capture can now leave as FLAC too, once you have said how.**
  FLAC is an integer codec, and choosing how to bring float down to integers is a
  decision about headroom that belongs to a person - so VCW asks instead of
  guessing. **Settings > Export** has three switches, and `vcw export` has the
  same three as `--narrow`, `--dither` and `--headroom`. *Narrow to* is `refuse`
  until you change it, which is exactly the old behavior; set it to `24` or `32`
  and a float capture exports as FLAC like any other. *Dither* adds a little
  under a bit of triangular noise before rounding, on by default, and is the same
  noise every time so an export stays reproducible. *Headroom* attenuates first,
  in dB, for a rip recorded hot; anything still over full scale is clamped rather
  than wrapped. Nothing is narrowed where the container does not need it, so an
  integer capture, and a float one going out as WAV or Ogg, are untouched.
* Worth knowing when you pick a width: **FLAC has no mid/side at 32 bits.** The
  difference channel needs one bit more than the samples and the format stops at
  32, so a 32-bit stereo file cannot use one channel to predict the other, and
  compression levels `-0` and `-1` come out byte-identical. A 24-bit capture is
  three quarters of the samples *and* the width where the encoder can still do
  its best work.

### Under the hood

* The FLAC encoder is now [`flac-codec`](https://crates.io/crates/flac-codec)
  rather than `flacenc`, which is where both ceilings came from. Still pure Rust,
  still MIT OR Apache-2.0, no C library to relink. Output was checked against
  reference libFLAC: a 32-bit 192 kHz file passes `flac -t`, which verifies the
  decoded audio against the MD5 in the stream, and lands within 0.08% of
  `flac -8` on real material.

## 0.1.1-alpha

*2026-10-06. The first public release.*

An alpha: the capture, recovery, editing, export and cataloging paths all work
end to end and are covered by the test suite, but the product has not been used
in anger by anybody except its author, and the format limits in
[`README.md`](README.md) are real. Everything below is what this release
contains rather than a history of changes to it, because there is nothing
earlier to have changed from.

### Installing

* Packages for every Tier 1 platform, built by CI: a **deb** and an
  **AppImage** for Linux on x86_64 and arm64, an **MSI** for Windows, and a
  **.app** and **DMG** for macOS on Apple silicon.
* **The command line ships inside the package.** Every workflow in
  `docs/USER-GUIDE.md` is a `vcw ...` line and `vcw bundle` is what to send when
  something breaks, so neither needs a Rust toolchain: the deb puts `vcw` on
  your `PATH` at `/usr/bin/vcw`.
* Downloads come with a `SHA256SUMS`, and `tools/verify-release.py` or
  `sha256sum -c` will check them. Signed releases are signed; an unsigned build
  says so in its own job summary rather than hoping nobody notices, because
  Gatekeeper and SmartScreen will both have something to say about it.
* Linux needs ALSA at runtime (`libasound2`), which the deb asks for.

### Capture

* Bit-perfect capture on ALSA, WASAPI and CoreAudio, with the configuration
  **negotiated and reported** rather than assumed: a request the device cannot
  honor is refused or recorded as a divergence, never silently resampled.
* The operating system is asked to **confirm** the stream it opened, because a
  backend reporting 24-bit while the mixer resamples is the failure a level
  meter cannot show. A capture that could not be confirmed is recorded as
  unconfirmed rather than claimed as bit-perfect.
* **Continuous commit**: audio is written and fsynced every 250 ms, so a crash
  or a power cut costs the last block and nothing else. There is no "save".
* Overruns, underruns, dropped frames and stream errors are counted per capture
  and kept with it.

### Recovery

* `vcw recover` finds captures left unfinished by a crash, **reports before it
  writes**, and closes them honestly. A recovered capture is marked `recovered`
  rather than `finalised`.
* `--repair` is the only thing in VCW that makes recovery discard audio, and it
  has to be asked for.
* Every block carries a CRC-32 of its own samples, and `--verify` recomputes
  all of them.

### Editing and identification

* Track detection from three detectors and a resolver, with the **evidence kept
  for every boundary**: a boundary a person has touched is locked, so
  re-detection cannot undo hand work.
* Non-destructive editing throughout: split, merge, move, delete, lock and
  retitle are statements about where the music is, and no edit rewrites a
  sample.
* Metadata from MusicBrainz and Discogs, with genre normalization, artwork, and
  an offline mode that is a first-class path rather than a simulation.
* **Credentials are never stored in a project or in a settings file**: they come
  from the environment, and the application reports which are configured without
  printing them.

### Import and export

* Audacity `.aup3` and `.aup4` projects import as ordinary captures, with labels
  becoming locked boundaries and tags becoming the release. The source is opened
  read-only and never touched.
* WAV and FLAC export from a plan that is resolved in full before a byte is
  written, with tagging, artwork and naming templates. A format that cannot
  carry the audio is refused up front rather than half way through a library.
* **Files are named by the position on the label**, `A1`, `B2`, the way VRipr
  named them: the default template writes `A1 - The Rainbow.flac`. A track's
  number in its tags counts across the disc's sides, so a two-sided record no
  longer produces two tracks numbered 2, and the position itself is kept in a
  `VINYL_POSITION` tag - the one thing a vinyl rip knows that a CD rip does
  not.

### Interface

* A window with six workspaces and a full keyboard map: every workflow is
  completable from the keyboard alone, and the map in `docs/USER-GUIDE.md` is
  checked against the application's own binding table by a test.
* A command line that can do all of it with no window, including a whole capture
  session driven from a script.
* **A side that will not play says why.** A device that cannot play a capture's
  rate is refused rather than resampled, and the refusal now reaches the window
  as a refusal: the transport stops showing itself as playing and the reason is
  on screen. It used to arrive as a generic warning, which left a transport
  claiming to play something that had never opened.

### Diagnostics

* `vcw bundle` writes one JSON document describing the build, the machine, the
  audio backend, the devices and the project's integrity, and is **safe to
  send**: no recorded audio, no titles, no credentials, not even the project's
  path. There are tests whose only job is to prove those absences.
* `vcw doctor` reports the platform and what it can see; `--log` and `VCW_LOG`
  turn structured logging up per crate. Nothing on the audio callback path
  logs, by rule and by test.
* **The waveform keeps up with the stylus, and there is a number for it.**
  `vcw soak` measures how far the drawable waveform trails the device while a
  capture runs: a quarter of a second at worst, which is the length of one
  commit. A build where it falls behind by more than a second fails its own
  test rather than shipping.

### For other tools

* The project format is documented in `docs/SCHEMA.md` in enough detail to be
  read without VCW, and `tools/vcw-read.py` is a working third-party reader in
  the standard library alone. A test drives it over projects the product wrote
  and requires the two to agree.
* `docs/PROJECT-API.md` is the supported Rust surface for reading, validating,
  recovering and migrating a project, with a compiled example beside it.
* `tools/verify-release.py` checks a download against the release's
  `SHA256SUMS`, needs nothing installed, and says plainly that a checksum
  proves integrity and not authorship.
