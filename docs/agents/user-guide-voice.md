# The user guide's voice

`docs/VCW-User-Guide/` is hand-authored. Editing it is allowed now, under the
rules below; the point of writing them down is that an edit should be unreadable
as an edit. If a paragraph can be identified as the machine-written one, it is
wrong whatever it says.

Everything here was measured against the guide as it stood at 0.3.8, not
invented. The counts are from the Markdown source and are worth re-running
before a large change.

## The register

The model is a *For Dummies* book written by somebody who has actually
transferred a few hundred sides: plain, quick, and unembarrassed about saying
that something is hard or that the software cannot do it.

- **Second person, imperative, present tense.** "Open **Export**." "Read every
  proposed filename." The guide instructs; it does not narrate what the software
  does on the reader's behalf.
- **Short declaratives.** Median sentence is 14 words, mean 16. A sentence over
  about 30 words is out of register, and three clauses is one too many.
- **No contractions.** There are exactly two in 5,500 words. Possessives take a
  straight `'`; the two contractions use a curly `’`.
- **No sales words.** `simply`, `just`, `easy`, `easily`, `powerful`,
  `seamless`, `robust`, `leverage` appear zero times. Neither does an
  exclamation mark outside Markdown image syntax.
- **US spelling.** `organized`, `recognizes`, `labeled`, `catalog`.
- **No em dash.** Zero in the file, and the AGENTS.md rule covers it. The en
  dash is only for numeric ranges: `Chapters 2–3`, `1–8 channels`.
- **No first person.** One slip exists in Chapter 15. Do not add a second.
- **Curly double quotes only for scare-quoting a term** - "Lossless",
  "The meters looked fine". Four uses in the whole guide.

## The joke

There is roughly one per chapter, and it obeys three rules.

1. **It lands last.** Always the final sentence of its paragraph, never mid-flow.
2. **It is deadpan.** No exclamation, no signposting, no winking. "Software is
   remarkably poor at removing a fingerprint or a digestive biscuit crumb from
   the actual vinyl."
3. **The target is the situation, never the reader.** "There is no prestige
   award for saving an unusable recording." "Randomly rotating every knob is an
   excellent way to create a second mystery." "Put on something you are not
   currently measuring."

One per chapter is a ceiling, not a quota. A chapter that needs none gets none.

## The furniture

- **Chapter headings** are `N Verb phrase`, three to five words, imperative:
  "9 Export without surprises", "11 Know what bit perfect means".
- **Bold lead-ins** open a note paragraph and end in a period, then one space:
  either an audience tag (`**Audiophile note.**`, `**Archivist note.**`,
  `**Weekend listener note.**`, `**Automation note.**`), one of the two standing
  markers (`**Remember.**` for an idea worth keeping, `**Watch out.**` for
  something that costs audio, time or an afternoon), or a short claim used as a
  title (`**Pause is not Stop.**`, `**A marker is not a track.**`,
  `**Mono records.**`). A new claim-shaped lead-in is fine; a new audience tag
  is not, because Chapter 1 enumerates the audiences.
- **UI nouns are bold**, spelled exactly as the window spells them:
  **Export**, **Plan**, **Settings \> Audio**. Check the i18n catalog, not
  memory.
- **Keys, paths, flags, tokens and filenames are monospace**: `Ctrl+E`, `.vcw`,
  `--no-replaygain`, `{tracknum}`.
- **Numbered steps** are a flat list of whole sentences. They are literal
  `1. ` text in the Word document, not a Word list.
- **Command lines live in fenced `text` blocks**, one command per block.

## What the guide refuses to do

This is the part that makes it credible, and it is the easiest thing to break.

- **It never claims more than is true.** "About 6 dB of peak headroom is a
  useful starting point, not a VCW requirement." "Zero counters are necessary
  evidence of a clean transfer, but do not prove the complete analog or digital
  chain was correct."
- **It names the limit in the same breath as the feature.** The format table's
  third column is called "Important limits" and every row has one.
- **It does not describe what has not shipped.** A sentence saying a feature is
  coming ships false the day it lands and nothing fails. Document the build the
  edition line names.
- **It does not teach audio engineering** except where a chapter's audience
  needs it. Chapters 8, 10 and 11 go deep because the audiophile and the
  archivist came for exactly that. Chapters 3 to 7 do not, and a dB figure
  dropped into them is a mistake.

## Mechanics of an edit

The `.docx` is the golden record. The `.md` carries the identical sentences, and
`crates/cli/tests/the_user_guide_names_real_commands.rs` reads the `.md`, so the
two must not drift.

1. Edit `word/document.xml` inside the `.docx` surgically, member by member, the
   way `tools/guide-version.py` does. Do not round-trip the whole document
   through an editor: the shipped file was produced by LibreOffice and a
   re-save can reflow a page or re-encode an image for no reason.
   Paragraph XML to copy:
   - body: `<w:p><w:pPr><w:pStyle w:val="Normal"/><w:rPr></w:rPr></w:pPr>` then runs
   - bold run: `<w:r><w:rPr><w:b/></w:rPr><w:t>…</w:t></w:r>`
   - inline monospace: `<w:rFonts w:ascii="DejaVu Sans Mono" w:hAnsi="DejaVu Sans Mono"/><w:sz w:val="17"/>`
   - command line: the paragraph carries `<w:spacing w:before="0" w:after="60"/>`
     and the run uses the same font at `<w:sz w:val="15"/>`
2. Make the identical change to the `.md`.
3. Rebuild the PDF from the `.docx` with the same LibreOffice invocation
   `tools/guide-version.py` uses, lossless image compression included.
4. Diff the extracted `.docx` paragraph text against the `.md` prose and confirm
   they still say the same thing.
5. Run the gate. Three tests read this guide.

The edition line is still `tools/guide-version.py`'s and nobody else's.

## The pages

Every chapter starts at the top of a page, and no page holds only a tail.

The break is `<w:pageBreakBefore/>`, first child of the `Heading1` paragraph's
`<w:pPr>`. It is not an empty paragraph before the heading: an empty paragraph
at the foot of a page that is already full starts a page of its own and leaves
a blank one behind.

Do not assume the breaks are there. Before this was written three chapters - 6,
8 and 10 - started a page only because the chapter before each of them happened
to fill one to within a line, and nothing in the document said so.

Length is the whole fight. A figure's frame height and a note's line count both
move everything after them, so rebuild the PDF and read it after any edit that
changes either:

```sh
cd docs/VCW-User-Guide
for p in $(seq 1 "$(pdfinfo VCW-User-Guide.pdf | awk '/^Pages/{print $2}')"); do
    low=$(pdftotext -f "$p" -l "$p" -bbox VCW-User-Guide.pdf - |
          grep -o 'yMax="[0-9.]*"' | sed 's/[^0-9.]//g' | sort -n |
          awk '$1 < 750' | tail -1)
    printf '%3d  %7s  %s\n' "$p" "$low" \
           "$(pdftotext -f "$p" -l "$p" VCW-User-Guide.pdf - | head -1)"
done
```

Every page but the title page and the first page of a chapter should print a
heading or continuing prose, and `low` - the lowest text row above the footer -
should be near 730 on a full page. A page whose `low` is under about 200 is
holding a tail and needs the length before it taken out, not a break added.

The arithmetic, if you need to work out how much: a body line is 16.2pt, the
gap between paragraphs is 23.2pt, and the text area ends at about 732pt.

## The figures

`assets/` holds drawn diagrams and they are the author's. `screenshots/` holds
shots of this application, and a figure showing a panel the window no longer has
is false in a way no test can see, so they are retaken whenever the window they
show changes. A figure is in two places and both must be the same bytes:
`screenshots/SNN-name.png` and `word/media/imageN.png` inside the `.docx`.
S01 is `image2`, S02 `image3`, S03 `image5`, S04 `image7`; the numbering
is the order Word met them, so `image1`, `image4` and `image6` are the
drawn diagrams and match it by nothing but luck. Check the pixel
dimensions before you overwrite one.

`/data2/vcw-scratch/guideshot/shoot.sh` takes them. It is outside the repository
because it is a rig, not a tool, and it is worth reading before you change it:

- **`GDK_SCALE=2` is the device pixel ratio**, and it is not optional. The
  author's desktop is HiDPI. Without it the chips in the lane band come out 20
  device pixels tall instead of 40 and the new figure is half the size of the
  ones printed beside it. Measure the chip strip to check: it is 40 rows.
- Interface scale 200%, **Buttons** set to icons, **Level meters** set to VU
  dials. That is what the existing figures show. They live in a throwaway
  profile under this rig's `XDG_CONFIG_HOME` and `XDG_DATA_HOME`, so the
  author's own settings are never opened, let alone written.
- The window is 3840 wide and taller than the screen the author used, because
  the export panel has gained a row since and the old client area no longer
  holds it. The figures crop the top of the window, so the extra height costs
  nothing.
- The crop is 3832 wide at x=8. The author's crops are inset eight pixels and
  the track boundaries line up only at that offset.
- A workspace figure starts one row above the lane band's chip strip.

Driving it, because none of this is obvious from a failed attempt: there is no
window manager, so `xdotool windowfocus` is needed or no key ever arrives; the
first keystroke after a click is swallowed; a focused `<select>` eats `Return`
and moves one option per `Down`, and a `Down` that misses the select reaches the
window and moves the playhead instead.

The frame height is set by fit, not by aspect. All seven pictures are 6.80in
wide and none of them is at its natural aspect - S01 is squashed 43%, S02 38% -
so choosing a height that makes the page work is the house style and not a
defect. Set it on both `<wp:extent>` and the `<a:ext>` inside `pic:spPr`, in
EMU, 12700 to the point.
