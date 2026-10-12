# WP-29 - playback equalization curves

**Status:** BUILT as `crates/eq` (`vcw-eq`), 2026-10-12. Sixteen curves, the filter
that applies them, and the conformance test §51 asks for. Not yet wired to a caller:
nothing selects a curve, and the schema cannot store one. See section 7.
**Date:** 2026-10-12
**Requirements:** §51 (playback equalization), §8 (sample rates), §9 (bit-perfect
capture, unchanged by this work), §12 (imported projects) · **Plan:** PROJECT_PLAN
Phase 3, "selectable playback equalization curves"
**Measured against:** the analytic response of each curve's published time constants,
at all six rates §8 requires

## Question

§51 names nine curves and sets one hard number against them:

> Every curve's parameters shall carry a citation to the published source they came
> from. The implemented response shall be measured against that source and shall be
> within +/-0.5 dB from 20 Hz to 20 kHz, at every sample rate §8 requires. A curve
> that is approximately right is worse than none, because a listener cannot hear that
> it is approximately right.

Three questions follow. What are the curves, in numbers we can implement? How are they
stated on disk, given §51 also wants "a user-defined curve stated in the same terms"?
And what filter meets +/-0.5 dB at 44.1 kHz, which is the rate that hurts?

### Who the catalog is for

RIAA alone serves anyone transferring a record pressed since the late 1950s, and that
is most people. The other fifteen curves serve a different reader: somebody working
through a collection that predates the standard, where the characteristic varies by
label and by year and getting it wrong is a transfer that cannot be corrected later
without the original disc. That is archival work, and it is an audience VCW can claim
rather than merely accommodate. An audiophile wants one curve to be exact. An archivist
wants sixteen curves, each with a citation they can check, and the curve in force
recorded in the file and carried into the exported metadata so that a transfer made
today is still legible to somebody who opens it without us. §51 already asks for all
three of those; they are what makes the catalog an archival tool and not a preference.

## 1. What a curve is

Every curve in the literature, including all nine §51 names, is three numbers: a bass
shelf, a bass turnover, and a treble rolloff, each a time constant T, each a corner at
f = 1/(2*pi*T). The replay transfer function is

```
H(s) = (1 + s*T2) / ((1 + s*T1) * (1 + s*T3))        normalized to 0 dB at 1 kHz
```

T1 the shelf, T2 the turnover, T3 the rolloff. A curve with no shelf has T1 -> infinity
and the denominator becomes `s` alone: the bass boost continues to DC, which is what
the letter `N` means in the old `500N-16` notation. A curve with no treble network
drops the `(1 + s*T3)` factor.

This is the same three-parameter model Pspatial Audio states ("if the time-constant
governing the HF pre-emphasis is t3, that governing the turnover is t2, and that
governing the corner of the bass-shelf is t1, then the overall response of the replay
equaliser is given by...") and the same one the Audacity published table uses ("the
three parameters are the necessary conditions to compute and plot any EQ curve").

**Validated.** Against RIAA's published figures the expression gives +19.27 dB at
20 Hz and -13.73 dB at 10 kHz, where the standard says +19.274 and -13.7. Checked
against the Audacity table's published gain columns across nineteen curves at 50 Hz and
10 kHz, **fifteen agree within 0.15 dB at both points**, most of them within 0.05.

The four that do not are worth naming, because none of them is a fault in the model.
Three are the flat-treble curves - European 78, Blumlein 300, 500-FLAT - and they are
trap 3 below. The fourth is **RCA 45 (800N-13.7), where the source is simply wrong**:
its gain column restates the code's -13.7, which is the treble section's figure, where
the complete curve reads -14.89. The page computes the equivalent row correctly
elsewhere - Old RCA is coded `800N-8` and its gain column says -9.9, not -8 - so this is
an inconsistency in one row rather than a different convention. It is recorded in the
catalog entry.

The IEC amendment is a fourth constant, **T0 = 7950 us (20.02 Hz)**, a first-order
highpass added to IEC 98 in September 1976, applied on replay only and never adopted by
the RIAA. **Parked, 2026-10-12.** It is a rumble filter and not part of any cutting
characteristic, so it belongs with noise reduction, click and pop removal and the rest
of the post-processing recipes rather than here. It is noted because parking it has a
consequence this work had to absorb: it was the only thing bounding the bass of a
shelfless curve. See section 5.

## 2. Three traps in the published numbers

These are the findings that change the design. All three are ways a published figure
means something other than what it looks like, and each one is larger than the +/-0.5 dB
§51 allows.

**Trap 1: "dB at 10 kHz" usually describes the treble section alone.** Galo says it
outright: "When the high-frequency portion of the curve is combined with the other two
turnover frequencies, 10kHz may not appear at +16dB, at least not relative to 1kHz."
Computed as a complete curve against 1 kHz, Columbia LP reads **-15.54 dB** and NAB
**-15.58 dB** where both are published as "-16". The Audacity table confirms this by
listing those same two curves at -15.5 and -15.6 in its gain column while coding them
`-16` in their names. RIAA is the exception: its -13.7 is a whole-curve figure, which
is why RIAA is the one curve where the published number can be used as a test.

**Trap 2: NAB puts 0 dB at 700 Hz.** Pspatial Audio: "NAB's decision to put the 0dB
reference at 700Hz rather than 1kHz confuses everyone and is what appears to skew the
table values slightly." A NAB table read as though it were referenced at 1 kHz is
wrong by a **constant 1.49 dB at every frequency**.

**Trap 3: a trailing `-0` means there is no treble network, not 0 dB at 10 kHz.** The
three curves that appeared to contradict the model are exactly the three with flat
treble: European 78 (250N-0), Blumlein 300 (300N-0), 500-FLAT (500N-0). Their complete
responses at a 1 kHz reference are -0.26, -0.37 and -0.96 dB at 10 kHz, not zero,
because at 1 kHz the bass boost has not yet reached its asymptote. The published `0` is
a statement about the recording chain, not a measurement of the replay curve.

**What this settles: the time constants are the definition.** The dB columns in every
secondary chart are derived, under a convention the chart usually does not state. The
conformance test in §51 must compare the implemented filter against the analytic
response of the declared time constants. A published dB figure is a sanity check whose
convention has to be named, never the gate.

## 3. The catalog

Curves §51 requires, in bold. Times in microseconds, `-` meaning the network is absent.

| Curve | T1 shelf | T2 turnover | T3 treble | Source |
|---|---|---|---|---|
| **Flat** | - | - | - | no curve applied |
| **RIAA** | 3180 | 318 | 75 | IEC 98 (1964); BS 1928:1955 |
| **NAB / NARTB** | 3180 | 318 | 100 | Galo 2009; Pspatial. See trap 2 |
| **Columbia LP** | 1590 *or* 1950 | 318 | 100 | **sources conflict, see below** |
| **AES** | - | 398 | 63.7 | Audacity table (400N-12.3) |
| **Teldec** | 3180 | 318 | 50 | CCIR Rec. 208 (1956); DIN 45533/45536/45537 |
| **EMI** | - | 531 | - | Blumlein 300-0; Langford-Smith 1952, Moir 1958 |
| **HMV** | - | 531 | - | same curve as EMI to July 1953; Copeland 2008 |
| **Decca FFRR (78)** | - | 636 | 25 | Moir 1958 via Pspatial. **Not one curve, see below** |
| Decca LP | 1590 | 318 | 60 | Pspatial, from Moir's table (500C-11.5) |
| CCIR Rec. 134 / BS coarse groove | 3180 | 450 | 50 | BS 1928:1955 |
| London LP | 1590 | 318 | 50 *or* 60 | Leister, ARSC 2017; FUNKSCHAU 1958 |
| Columbia 78 | - | 530 | 100 | Audacity table (300N-16) |
| Old RCA | - | 199 | 36.7 | Audacity table (800N-8) |
| RCA 45 | - | 200 | 75 | Moyer, Audio Engineering 1953 |
| American 78 | - | 636 | 25 or 36 | Audacity table (250N-6, 250N-8) |

Self-consistency check on the one curve whose name encodes its own answer: Decca FFRR
78 is called "250-5.5", and 636 us gives a 250 Hz turnover while 25 us gives
**-5.55 dB** at 10 kHz on the complete curve. The name and the model agree to 0.05 dB.

### Where the sources disagree, and what to do

**Columbia LP is not settled.** Galo determined 1590 us by simulating candidate time
constants against the published Snepvangers graph measured with dial calipers, picking
the combination with sub-0.3 dB error. Pspatial Audio says 1950 us. These are the two
most careful sources on the single most disputed curve in the literature, and they
differ by **1.675 dB at 20 Hz** - more than three times §51's tolerance. The two
cannot be averaged into something defensible.

**Decca FFRR is three different things in the literature, and one of them is
fictitious.** §51 names it as one curve. It is at least two real ones: the ffrr 78
(636/25) and the Decca 33 1/3 (1590/318/60), used from June 1950. The widely cited
"640 us / 53 us with a 3 dB per octave slope" is, per Pspatial Audio, a misreading of
where a published graph departs from flat, mistaken for time constants: "the indicated
frequencies are not related to the time constants". IASA lists three FFRR vintages and
inherits some of this.

**EMI and HMV are the same curve, and it is Blumlein 300-0.** This resolves the entry
the secondary charts scatter between 200, 250, 300 and 500 Hz. Pspatial Audio compared
Langford-Smith's EMI curve against Moir's tabulated HMV 78 data and matched them, and
Copeland (2008) extends the same characteristic to EMI LPs and 45s until mid-July 1953.
After that date HMV 33s cut in England used the Columbia LP characteristic, so **HMV is
two curves separated by a date**, exactly as Decca is.

**Columbia LP is settled, by its own code.** `500C-16` means a +14 dB player shelf,
and the letter states a shelf-to-turnover ratio, so it predicts t1 = 318 * 10^(14/20)
= **1593.8 us**. Galo's 1590 is that to 0.02 dB. Pspatial's 1950 would be +15.75 dB,
which is not a letter in the table at all. The same check passes exactly on RIAA
(500R, 3180/318 = +20.00), Teldec, Decca LP and London LP, so it is a real constraint
and not a coincidence. **Galo's 1590 us ships**, with Pspatial's figure recorded in the
entry's `disputed` field. `the_code_letter_predicts_the_published_shelf` is the test.

**The consequence for §51.** Its list of nine is a minimum, not a schema. Three of its
entries (Columbia LP, Decca FFRR, HMV) cannot be honored by one row each. The catalog
must be able to carry a curve twice under different date ranges, and must be able to
say that a parameter is disputed rather than pick a side silently. Where sources
conflict the catalog ships the better-evidenced value, names the other in the entry,
and the +/-0.5 dB claim is made against the value shipped - not against "the
literature", which does not agree with itself to 0.5 dB.

## 4. The file format: TOML, parametric

**Decision: TOML, parametric, shipped in the binary, extended by an optional user
file.** XML was ruled out by the request; the remaining call was JSON against TOML.

TOML, for three reasons that are about this data specifically:

- **Comments.** Every entry needs provenance, a date range, and in several cases a note
  that a source disagrees. JSON has no comments, so all of that becomes string fields
  nobody reads, or it is lost. Section 3 above is the strongest argument for TOML in
  the whole document.
- **It is what the repo already reads.** `i18n/en-US.toml` and every `Cargo.toml`.
  No new dependency, no new idiom.
- **A person can edit it.** §51 wants a user-defined curve "stated in the same terms"
  and explicitly does not want a curve editor. A commented TOML file *is* the editor.

Parametric rather than sampled points is the more consequential half of the decision,
and it is worth stating why Audacity's own file is no guide here. `EQCurves.xml` stores
`<point f="100.0" d="-20.0"/>` lists - the coordinates of its Draw Curves view. That is
a sampled curve: it has to be fitted to be used, its accuracy is bounded by its sample
density, and it cannot be checked against a citation. The Audacity *published table*,
by contrast, is parametric, and that table is the one the research actually comes from.
Three time constants are exact, are what every source publishes, and are what the
conformance test needs. We take Audacity's data and not Audacity's file format.

```toml
# Reference: 0 dB at 1 kHz. Times in microseconds. Omit a field for an absent network.
[riaa]
name        = "RIAA"
shelf_us    = 3180
turnover_us = 318
treble_us   = 75
citation    = "IEC 98 (1964); BS 1928:1955"
era         = "1954 onwards"
labels      = "effectively all, after the mid-1950s"

[emi]
name        = "EMI / HMV (Blumlein 300-0)"
turnover_us = 531                 # no shelf: bass boost continues to DC
citation    = "Langford-Smith, Radiotron Designer's Handbook, 4th ed. 1952; Moir, High Quality Sound Reproduction, 1958"
era         = "1931 to July 1953"
# Copeland (2008) extends this to EMI LPs and 45s, not 78s alone.

[columbia_lp]
name        = "Columbia LP"
shelf_us    = 1590
turnover_us = 318
treble_us   = 100
citation    = "Galo, 'The Columbia Lp Equalization Curve', ARSC Journal, Spring 2009"
era         = "June 1948 to 1956"
# Pspatial Audio gives 1950 us for this shelf. The two differ by 1.68 dB at 20 Hz.
# Galo's figure is shipped because his method is stated and reproducible.
disputed    = "shelf_us: Pspatial Audio gives 1950"
```

## 5. The filter: a fitted biquad cascade, not a bilinear transform

§8 requires 44.1 kHz, and 44.1 kHz is where this gets hard. 20 kHz sits at 0.907 of
Nyquist, and the bilinear transform maps it onto the analog prototype's 95.6 kHz while
forcing a spurious zero at Nyquist. Measured worst error over 20 Hz to 20 kHz, RIAA,
against the analytic target:

| method, RIAA | 44.1 k | 48 k | 88.2 k | 96 k | 176.4 k | 192 k |
|---|---|---|---|---|---|---|
| bilinear, per-section prewarped | **13.471** | 9.010 | 1.644 | 1.357 | 0.371 | 0.312 |
| matched-z | **3.142** | 2.621 | 0.745 | 0.627 | 0.184 | 0.155 |
| matched-z + one fitted pole/zero pair | **0.078** | 0.037 | 0.001 | 0.000 | 0.000 | 0.000 |

A textbook biquad misses §51's budget by a factor of twenty-seven. Matched-z is six
times better and still four times over. **Matched-z plus one free pole/zero pair clears
it with room to spare**: across all sixteen catalog curves at all six rates §8 requires,
the worst error is **0.078 dB** - NAB at 44.1 kHz - against a budget of 0.5.

Only the free pair is fitted. Letting the matched-z roots move as well was tried and
buys 0.001 dB, so the optimizer is **two-dimensional**, which is small enough to be
obviously terminating and cheap enough to run on demand. The objective is a worst-case
error and therefore not smooth, so it is Nelder-Mead rather than a gradient method.

Because every pole and zero is real, the result is a **cascade of first-order sections**
rather than biquads, and every magnitude in the crate is computed without complex
arithmetic: `|j*w - r|` is `sqrt(w^2 + r^2)` for real `r`, and `|1 - q*e^(-j*theta)|`
is `sqrt(1 - 2*q*cos(theta) + q^2)`. Runtime is three or four first-order sections a
sample. Design is a few hundred microseconds, paid once per curve and rate, and it
works identically for a user-defined curve - which is why it is preferred over shipping
a precomputed coefficient table.

### A curve with no bass shelf needs a replay bass limit, and parking the rumble filter made it mandatory

The fit returns a stable filter for every curve with a shelf, and a pole exactly on the
unit circle for every one of the seven without one. That is not a fitting artifact: an
`N` type curve has an analog pole at s = 0, so its replay response rises 20 dB per
decade all the way down with no limit. Measured on EMI/HMV with no rumble filter: +49 dB
at 1 Hz, +69 dB at 0.1 Hz, and climbing. Applied to a real capture, which always carries
some DC offset, that filter is an integrator and will run away into the clipping point.

The period literature says the same thing in its own terms. Pspatial Audio: "whilst a
filter may easily be arranged to 'cut' frequencies with no limit, it isn't possible to
have an infinite 'boost'. So the compensating bass-restoration filter on the reproducer
side had to be a shelving type." That is exactly what the code letters encode - `N` no
shelf, `X` +12 dB, `C` +14, `A` +16, `B` +18, `R` +20 - and they describe the *player*,
not the record. A real machine always limited the boost somewhere.

The IEC highpass would have cancelled that DC pole exactly, both sitting at z = 1. With
the rumble filter parked there is nothing to cancel it, so **the catalog carries the
replay shelf itself**: `replay_shelf_db`, default +20 dB, applied to any curve with no
shelf of its own.

It is stated as the shelf-to-turnover ratio `20*log10(t1/t2)`, because that is exactly
what the letters mean - RIAA is `500R` and 3180/318 is 10, which is +20.00 dB. The
boost actually realized sits a few tenths under the stated figure, since the 1 kHz
reference point is not quite on the turnover's asymptote: EMI at +20 dB settles at
19.63 dB. Defining the field on the ratio keeps it consistent with every published code
and with the shelf of every curve that has one, which is worth more than a round number
at DC.

**FIR was tested and rejected.** A windowed linear-phase FIR built from the exact
analytic magnitude needs its main lobe narrower than the features it must resolve, and
the IEC highpass puts a real feature at 20 Hz. At 44.1 kHz that is of order 50,000
taps. Correct, and absurd. These curves are first-order analog sections and want an
IIR.

**The conformance test writes itself** and is the same code as the fit: for every curve
in the catalog, at all six rates, evaluate the implemented cascade and the analytic
response of the declared time constants, normalize both at 1 kHz, and assert the worst
divergence over 20 Hz to 20 kHz is under 0.5 dB. That is one test with a loop in it,
and it is the only honest reading of §51's sentence. It should assert stability in the
same pass: after the z = 1 pair of a shelfless curve is cancelled, every remaining pole
strictly inside the unit circle. That check is what found the shelfless-curve problem
above, and it fails loudly if the cancellation is ever left to floating point.

## 6. As built

**A new crate, `vcw-eq`.** The three consumers §51 names - playback, render and export
- do not share a crate today. `vcw-export` does not depend on `vcw-signal`, and
`vcw-audio`, which owns `playback.rs`, depends only on `vcw-types`. `crates/signal` is
analysis (features, hmm, meter, regions, resolve, silence, spectral, waveform) and
putting a processing chain in it would make export depend on the analyzer. The honest
boundary is a small crate of its own that depends on nothing but `vcw-types`.

**`releases.riaa_eq` cannot carry this.** It is a `bool`
(`crates/project/src/release.rs:103`) and §51 needs a curve name, selectable per side
and overridable per track. It is also, per its own definition, a setup intent and not a
measurement - it is not `capture_eq`. A migration is required, and per the five-places
rule a new column touches the DDL parser, the generated `SCHEMA.md`, the Python
reader's allowlist and the shell's wind-back fixture as well as the table.

**`CaptureEq` is the gate on the whole feature and it already worked.** `Flat` takes a
curve directly. `Riaa` divides by RIAA first, which falls out of the representation:
RIAA's poles become the filter's zeros and the other way round, so inverting it is not
a special case in the code, only in what it does to the noise floor. `Filter::inverts_riaa`
reports when that is happening so a caller can say so, as §51 requires, and
`response_db` gives the number to say - Teldec over an RIAA capture is +2.83 dB at
10 kHz. `Unknown`, the default and what every imported project gets, is refused:
`Error::ProvenanceUnknown`, whose message names both ways out rather than just
refusing.

**The shape of it.** `Catalog` holds the sixteen curves in file order, from a
compiled-in `curves.toml`, and `with_overrides` merges a user file over it - new ids
appended, existing ids replaced. That is §51's "user-defined curve stated in the same
terms", and it needed no editor. `Curve::response_db` is the analytic truth;
`Filter::response_db` is what was realized; the conformance test is the distance
between them. Validation refuses a curve with no citation, a duplicate id, a
non-positive time constant, and a shelf at or below its turnover, which is the
commonest way to mistype a curve.

**Sixteen tests.** The two that matter: `every_curve_meets_the_tolerance_at_every_rate`
is §51's sentence, and it asserts the margin as well as the tolerance, so eating the
headroom is caught before eating the budget. `the_audio_leaving_the_filter_matches_the_curve`
runs sine waves through `process` and measures what comes out, because every other test
reads coefficients and only this one proves the running filter is the filter that was
designed.

## 7. Selecting a curve, as built

**Schema v5, and the boolean is dropped rather than kept.** `releases.riaa_eq` becomes
`releases.playback_eq TEXT`, and `sides` and `tracks` gain the same column. Keeping the
boolean beside the curve id would have been two places the same decision could be
written and a chance for them to disagree; `riaa_eq = 1` carries across to
`playback_eq = 'riaa'` in the migration, which is all the information the bit held.
`0` becomes NULL, which is honest - v4 could not tell a declined box from an unasked
question.

**NULL means nobody has said**, at all three levels, rather than `NOT NULL DEFAULT
'riaa'`. Writing `'riaa'` into every existing row would put a statement in the file
that nobody made, which is the mistake `SCHEMA_V3` deliberately avoided for
`capture_eq`. `vcw_eq::in_force(track, side, release)` resolves the three and answers
RIAA when none of them speaks. It lives in `vcw-eq` because the default is a fact about
curves, and a project crate that knew which curve was usual would be a second home for
that fact.

**The five-places rule cost two parser changes.** `crates/project/src/doc.rs` only knew
`CREATE` and `ALTER ... ADD COLUMN`, and this is the first migration to drop a column
and the first to carry data. `DROPPED_COLUMN` folds onto the table by removing the
column - the document describes the schema a reader meets, not the one it passed
through - and `DATA` is parsed and discarded. The wind-back fixtures needed nothing:
both drop the whole table.

**Semantics changed, not just the type.** v4's boolean meant *apply RIAA*, and the
window had to warn that a phono stage may already have done it. `playback_eq` names
what the record was **cut** with, and `capture_eq` already records what the signal
arrived with, so VCW works the difference out instead of asking a person to. The
"equalized twice" warning is gone; what replaced it is advice shown only when the
chosen curve is not RIAA, which is the case where undoing RIAA first lifts the noise.

**Three levels on the command line, and one in the window.** `vcw release set
--playback-eq`, `vcw tracks side-eq --side A --curve`, `vcw tracks set N --playback-eq`.
`vcw tracks list` prints the curve **in force** at each level rather than the one
stated there. The window asks at setup, as a picker filled from `library::curves()` and
preselected to RIAA - a revisable decision next to the irrevocable `capture_eq`, which
is what item 3 above asked for. A curve the shipped catalog does not know is refused at
the command: a `--curves` file extends what `vcw eq` prints, but a name that resolves
only on the machine that typed it would be a dangling reference in a file meant to
outlive the application.

## 8. Not done, and in what order

1. **Render and export have to apply it**, and the curve in force has to reach exported
   metadata (§51). Playback too - `Filter::playback` exists and nothing calls it.
2. **The window can only set it at setup.** Per-side and per-track pickers, and showing
   the curve in force beside the audio, need a release-edit command the contract does
   not have - the same gap that leaves `is_mono` unchangeable after setup.
3. **A user-defined curve cannot be stored in a project.** `--curves` is per
   invocation; storing one needs the catalog to have a settled home on disk.

## What this does not build

No curve editor, by instruction. No sampled-point import. No Audacity `EQCurves.xml`
reader. Nothing touches the capture path: §9 is unchanged and this is a stored
decision applied on the way out, like every other edit instruction.
