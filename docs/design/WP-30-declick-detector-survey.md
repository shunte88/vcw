# WP-30 - the declick detector survey

**Status:** SURVEY, 2026-10-17. This is WP-30 §8 step 1, "read the declick
literature and pick a detector". It ends in a recommendation and a reading list.
No detector is written and nothing here has been run against a VCW capture.
**Date:** 2026-10-17
**Requirements:** §24 (observations carry provenance and confidence), §46 Phase 3
(click detection and removal)
**Design note:** [WP-30 - the non-destructive processing chain](WP-30-non-destructive-processing.md)
**Measured against:** the papers' own numbers, on the papers' own material. Not one
figure in this note was taken off a VCW rip, and §4 is about exactly that gap.

## Question

WP-30 §7 fixes four things the detector has to do before anything else about it
matters:

> that it runs over a committed capture rather than the live path (§4.2), that it
> emits spans with a confidence and a provenance, that it is re-runnable without
> disturbing locked rows, and that it is honest about false positives

So the survey is not "which declicker sounds best". It is which published method can
be made to answer those four, in a crate that already exists, without a dependency
VCW cannot take.

## 1. Every method has the same two halves

The literature has agreed on the decomposition since the 1980s and has not moved off
it: restoration is **detection** (which samples are corrupt) and **interpolation**
(what to put there instead). Oudre states it flatly, and it is worth quoting because
it is also VCW's §7 split:

> Restoring audio signals corrupted by impulsive noise is a tricky process which can
> be divided into two steps: detection (finding the locations of the degraded
> samples) and interpolation.

That is the same seam WP-30 §2 drew between a stored defect list and a cheap
deterministic repair, arrived at independently and for a different reason. The
literature splits it because the two halves need different mathematics. VCW splits it
because a person has to be able to veto one click without vetoing the rest. The seams
coincide, which is the useful finding of this survey: a detector from the literature
drops into the place WP-30 already built for it.

**Interpolation is solved and VCW does not get a vote.** Janssen, Veldhuis and Vries
(1986) is still the method everybody uses, including work published in 2024: given
the positions of the missing samples and enough intact signal around them, choose the
replacements that minimize the AR prediction error, iterating because the AR
parameters have to be estimated from the damaged signal. It needs the gap positions
as input, which is precisely what the detector produces and what WP-30's `defects`
table stores. Nothing in the last forty years has displaced it for gaps of the length
a click makes.

**Detection is the open half**, and is where this survey spends its time.

## 2. Four families, and why three of them lose

### 2.1 Threshold on a high-pass or a derivative

Take a difference, take an absolute value, threshold it. Kauppinen (2002) surveys
these. They work when the click is short, isolated and much louder than the music.

Rejected, and the reason is the one that bites on vinyl rather than on 78s: a
band-limited click, or a passage with real high-frequency content in it, puts the
music and the defect in the same place in the signal. A cymbal is a broadband
transient with a sharp onset. So is a click. A detector with no model of what the
music was about to do cannot tell them apart, and the failure mode is not a missed
click, it is a hole punched in a cymbal.

### 2.2 Threshold on an AR prediction residual  **(recommended)**

Fit an all-pole model to a short frame, run the inverse filter, and threshold the
residual. The music is what the model predicts; the click is what it cannot. This is
Vaseghi and Rayner (1988, 1990), refined by Esquef et al. (2000, 2002) and given a
complete, reproducible, parameter-by-parameter treatment by Oudre (2015).

It beats §2.1 for the exact reason §2.1 fails: the model is re-fitted every frame, so
"what counts as a surprise here" is set by this passage of this record, not by a
global frequency assumption. The cymbal is in the model. The click is not.

Two refinements are not optional and both come from the same place:

- **Burst fusion.** Thresholding alone marks scattered single samples, because inside
  a burst the residual interferes destructively and dips back under the threshold.
  The fix is a fusion distance `b`: two marked samples closer than `b` have
  everything between them marked too. Oudre credits the idea to Alvarez et al. (2004)
  and measures it: on the same frame of real classical material, `K=2` gives 38 bursts
  at `b=1` and 21 at `b=20`. Without fusion the interpolator is handed rubble instead
  of spans.
- **Two passes.** The AR parameters are first estimated from the damaged signal, so
  they are biased by the very clicks they are meant to find. Running the whole thing
  again over the first pass's output fixes the estimate. Oudre measures the ceiling
  too: two iterations clean, three about the same, and **at four or five, musical
  noise appears**. More is worse, and that is a published number rather than a
  guess.

### 2.3 Bayesian model-based restoration

Godsill and Rayner (1992, 1995, 1998) treat detection and interpolation as one
inference: sample the noise indicator, the AR parameters and the clean signal
together with a Gibbs sampler. The 1998 Springer book is the standard reference for
the whole field and chapter 5 is the clicks chapter.

Rejected for the first detector, not on quality. It is the better method. It is a
sampler, so it is slow, it is stochastic, and running it twice on the same capture
can give two different answers. WP-30 §7 requires re-runnable, and a person looking
at a defect overlay needs the list not to shuffle under them. Worth revisiting once
there is a defect table to compare two detectors in, which is the thing a `provenance`
column is for.

### 2.4 Learned and generative

Diffusion-based restoration (Moliner, Välimäki and the Aalto group; see the 2024
review) is the current research front, aimed at declipping, bandwidth extension and
inpainting, with declicking as a near neighbor.

Rejected for VCW, and not provisionally. A generative model invents plausible audio.
VCW's §4.1 promise is that the capture is immutable and every change is an
instruction a person can read and refuse; "a neural network thought this is what the
record would have sounded like" is not a reviewable instruction, and it is the
opposite of what an archivist is using this tool for. Recorded here so the decision
is on the record rather than rediscovered.

## 3. What a shipped implementation chose

FFmpeg's `adeclick` filter (Paul B Mahol, 2018, `libavfilter/af_adeclick.c`) is
§2.2, and it is useful here because it is the same algorithm after seven years of
field use. Read from the binary on this machine:

| option | range | default | Oudre's name |
|---|---|---|---|
| `window` | 10 to 100 ms | 55 | `Nw` |
| `overlap` | 50 to 95 % | 75 | 75% is Oudre's too |
| `arorder` | 0 to 25 | 2 | `p`, here a percentage of the window |
| `threshold` | 1 to 100 | 2 | `K` |
| `burst` | 0 to 10 | 2 | `b` |
| `method` | add / save | add | overlap-add |

The threshold default is **2**, which is Oudre's measured good compromise, arrived at
by a different person on different material. The overlap default is 75%, which is
Oudre's. That is two independent votes for the same operating point and it is the
operating point VCW should start from.

`adeclip`, the sibling filter, is the same machine pointed at clipping with a
histogram in front of it and a much higher threshold default of 10.

## 4. The number that should worry us: five to seven percent

Oudre's recommended settings, on four real degraded extracts (Mussorgsky, two King
Oliver and Louis Armstrong jazz sides, a Mozart piece, the corpus Godsill used),
**detect between 5 and 7 percent of all samples as noise**, and the paper calls that
realistic. The demo's own output for the Mussorgsky extract is 7.23% on the first
iteration and 7.54% on the second.

That material is 78 rpm gramophone transfer. A VCW capture is an LP off a working
turntable, which is one to three orders of magnitude cleaner. **The published
defaults are tuned for a corpus VCW does not have.** At 44.1 kHz, 5% is 2,205 flagged
samples per second; carried onto a clean LP side it would be twenty minutes of
invented repairs, each one a row a person is invited to review.

This is the honest-about-false-positives requirement arriving before any code. Two
consequences:

1. **The defaults have to be re-derived on VCW's own corpus**, not inherited. The
   62 rips at `/data2/source_rips` are the material, and the measurement is: run the
   detector, count flagged samples as a fraction, and read the spans. A detector that
   flags 5% of a clean LP is miscalibrated, not thorough.
2. **A defect rate is a reportable number, not an internal one.** The §7 phrase
   "honest about false positives" cashes out as: the detection run says how many
   spans it found and what fraction of the side they cover, before a person is asked
   to approve anything. A run that flags 5% of a side should say so loudly enough that
   nobody accepts it by reflex.

Neither of those is in WP-30's design note, and both belong in it.

## 5. The recommendation, against §7's four requirements

**An AR residual detector in the style of Oudre (2015), written from the papers, in
`vcw-signal`, with its defaults re-derived on `/data2/source_rips`.**

| §7 requires | how this answers it |
|---|---|
| runs over a committed capture, not the live path | frame-based with 75% overlap over stored blocks; nothing about it is causal or real-time, and §4.2 keeps it off the capture thread |
| emits spans with confidence and provenance | burst fusion makes it emit spans natively rather than samples. The residual-to-threshold ratio is a margin, and `BoundaryObservation` already documents margin-derived confidence as the normal case and a posterior as the exception. `Provenance` grows one variant |
| re-runnable without disturbing locked rows | deterministic. The same capture and the same five parameters give the same spans, which §2.3's sampler cannot promise. Re-running is a set comparison against what is stored |
| honest about false positives | §4. Precision and recall are what the paper is measured in, and the five parameters move along that curve rather than toward a single right answer |

Three things make this the lazy choice as well as the right one:

- **No new crate.** WP-30 §7 guessed "probably a new small crate". It is wrong, and
  pleasantly so. `vcw-signal` is where the boundary detectors already live, already
  writes observations and nothing else, and already carries the `Provenance` and
  confidence vocabulary this needs. The detector is a ninth module beside
  `silence.rs` and `spectral.rs`.
- **No new dependency.** The whole method is autocorrelation, Levinson-Durbin, an
  inverse filter and a threshold. `vcw-signal` depends on `rustfft` and `vcw-types`
  and needs nothing else. Levinson-Durbin is about thirty lines and is in Levinson
  (1947).
- **The repair is the same arithmetic.** Janssen et al. reuses the AR parameters the
  detector already computed, which is the economy Oudre points out and the reason the
  two halves share a crate even though they land on opposite sides of WP-30's
  parametric/repair split.

**Licensing: read, do not vendor.** Oudre's reference implementation is
**GPL-3.0-or-later** and FFmpeg's `af_adeclick.c` is LGPL. VCW is MIT and
`deny.toml`'s allowlist is permissive-only, with LGPL-3.0 named per-crate for
`mp3lame-sys` and one other. Neither implementation can be copied in; the gate's
`deny` leg would catch it if anyone tried. The algorithms are published in IEEE
journals from 1986 and 1998 and are free to implement. This note exists partly so
that the independent-implementation trail is written down before the code is.

## 6. What this survey does not decide

- **The parameter values.** §4 says why the published ones cannot be trusted here.
  Deriving them is a measurement on `/data2/source_rips` and it has not been done.
- **Stereo.** Every method above is single-channel. A click is usually one groove
  wall and therefore one channel, which is information a per-channel detector throws
  away and a vector AR model (Niedzwiecki's group) uses. WP-30's `defects` table
  already has a channel bitmask, so the schema is ready for the answer either way.
- **Whether crackle is the same problem as clicks.** The literature treats dense
  low-amplitude crackle as the hard end of the same scale. WP-30 lists declick,
  decrackle and dropout as three kinds. Nothing here confirms they are one detector.
- **How a span is drawn on a waveform**, which is WP-30 §8 step 6 and needs the rows
  to exist first.

## References

Every item was consulted for this note. The verdict column is what WP-30 does with
it, so that a later reader does not re-read a paper this one already rejected.

### Used

**Oudre, L.** "Automatic Detection and Removal of Impulsive Noise in Audio Signals."
*Image Processing On Line*, vol. 5 (2015), pp. 267-281. DOI
[10.5201/ipol.2015.64](https://doi.org/10.5201/ipol.2015.64). Open access at
<https://www.ipol.im/pub/art/2015/64/>.
> **The primary source for the recommended detector.** IPOL publishes a reproducible
> algorithm with its code and a running demo, so every parameter is stated rather than
> alluded to: `K = 2`, `b = 20`, `p = 3*Nmax + 2 = 302`, `Nw = 8p = 2416`, 75% overlap,
> Hamming overlap-add, `Niter = 2`. Measures the precision-recall tradeoff directly,
> reports 5-7% of samples flagged on gramophone material, and measures where iteration
> stops helping (four to five, musical noise). Reference code is GPL-3.0-or-later and
> **must not be vendored**; the article is the thing VCW uses.

**Janssen, A.J.E.M., Veldhuis, R.N.J., and Vries, L.B.** "Adaptive Interpolation of
Discrete-Time Signals That Can Be Modeled as Autoregressive Processes." *IEEE
Transactions on Acoustics, Speech, and Signal Processing*, vol. 34, no. 2 (1986),
pp. 317-330. DOI
[10.1109/TASSP.1986.1164824](https://doi.org/10.1109/TASSP.1986.1164824).
> **The repair half, and still unbeaten.** Needs the gap positions as input, which is
> what WP-30's `defects` table holds. Iterative: each pass re-estimates the AR
> parameters using the current estimates of the missing samples. A 2024 audio
> inpainting survey still ranks it among the best for gaps up to about 80 ms, which is
> two orders of magnitude longer than a click.

**Godsill, S.J. and Rayner, P.J.W.** *Digital Audio Restoration: A Statistical
Model-Based Approach.* Springer-Verlag London, 1998. Chapter 5, "Removal of clicks",
pp. 99-134. Companion site with audio examples:
<https://sigproc.eng.cam.ac.uk/Main/SJGSpringer>.
> **The standard reference for the field**, and the source of the 10%-of-samples
> figure that §4 measures VCW against. Read for chapter 5's framing of detection as
> the hard half. The sampler itself is rejected for the first detector (§2.3) on
> determinism, not on quality.

**Alvarez, E., Mendez, R., and Langwagen, G.** "Detection of clicks using sinusoidal
modeling for the confirmation of the clicks." *Proc. DAFx*, Naples, 2004, pp. 27-32.
> **Where burst fusion comes from.** The `b = 20` default (0.45 ms at 44.1 kHz) is
> theirs. The sinusoidal-plus-AR model it sits in is not adopted; the fusion parameter
> is, and §2.2 says why it is not optional.

**Levinson, N.** "The Wiener RMS error criterion in filter design and prediction."
*Journal of Mathematics and Physics*, vol. 25 (1947), pp. 261-278.
> The AR solver. Thirty lines, no dependency, and the reason §5 can claim this adds
> nothing to `Cargo.toml`.

**FFmpeg, `libavfilter/af_adeclick.c`** (Paul B Mahol, 2018). Documentation:
<https://ffmpeg.org/ffmpeg-filters.html#adeclick>.
> **The shipped implementation of the recommended method**, read here for its defaults
> rather than its code (§3). Independent confirmation of `threshold = 2` and 75%
> overlap. LGPL, so the same read-do-not-vendor rule applies.

### Consulted and set aside

**Vaseghi, S.V. and Rayner, P.J.W.** "A new application of adaptive filters for
restoration of archived gramophone recordings." *Proc. ICASSP*, New York, 1988,
pp. 2548-2551. DOI [10.1109/ICASSP.1988.197163](https://doi.org/10.1109/ICASSP.1988.197163).
And "Detection and suppression of impulsive noise in speech communication systems."
*IEE Proceedings I - Communications, Speech and Vision*, vol. 137 (1990), pp. 38-46.
> The origin of AR-residual detection. Superseded for implementation purposes by
> Oudre, which states the parameters these do not.

**Esquef, P.A.A., Biscainho, L.W.P., Diniz, P.S.R., and Freeland, F.P.**
"A double-threshold-based approach to impulsive noise detection in audio signals."
*Proc. EUSIPCO*, Tampere, 2000. And **Esquef, P., Karjalainen, M., and Valimaki, V.**
"Detection of clicks in audio signals using warped linear prediction." *Proc. DSP*,
2002, pp. 1085-1088.
> Two refinements of §2.2 worth returning to if the single threshold calibrates badly
> on `/data2/source_rips`: a second threshold, and a frequency-warped predictor that
> spends model order where the ear is. Not needed to start.

**Kauppinen, I.** "Methods for detecting impulsive noise in speech and audio signals."
*Proc. DSP*, 2002, pp. 967-970. DOI [10.1109/ICDSP.2002.1028251](https://doi.org/10.1109/ICDSP.2002.1028251).
> The survey of the §2.1 family VCW is rejecting. Read to be sure the rejection was
> informed.

**Niedzwiecki, M. and Cisowski, K.** "Adaptive scheme for elimination of broadband
noise and impulsive disturbances from AR and ARMA signals." *IEEE Transactions on
Signal Processing*, vol. 44 (1996), pp. 528-537. DOI
[10.1109/78.489026](https://doi.org/10.1109/78.489026). The same group's later vector
AR work on stereo recordings extends it to two channels jointly.
> The answer to §6's stereo question when it is asked. A click on one groove wall is
> correlated across channels in a way a per-channel detector cannot see.

**Murphy, J. and Godsill, S.** "Joint Bayesian removal of impulse and background
noise." *Proc. ICASSP*, 2011, pp. 261-264. DOI
[10.1109/ICASSP.2011.5946390](https://doi.org/10.1109/ICASSP.2011.5946390).
> Clicks and hiss removed in one inference. Relevant if decrackle and denoise ever
> merge; see §6.

**Lemercier, J.-M., Richter, J., Welker, S., Moliner, E., Valimaki, V., and
Gerkmann, T.** "Diffusion Models for Audio Restoration." arXiv:2402.09821 (2024).
<https://arxiv.org/abs/2402.09821>.
> The current research front, and the reference behind §2.4's rejection. Kept so that
> the decision is traceable: VCW declines generative repair on §4.1 grounds, not on
> performance grounds.

**Copeland, P.** *Manual of Analogue Sound Restoration Techniques.* British Library,
2008. <https://www.bl.uk/britishlibrary/~/media/subjects%20images/sound/analoguesoundrestoration.pdf>
> Already cited in `crates/eq/curves.toml` for the playback curves. Consulted here for
> the operator's view of what a click is physically and what restoration is allowed to
> do to a document. Not an algorithm source.

### Where to obtain

Oudre (2015), the Copeland manual and the Lemercier review are free downloads at the
links above. The Godsill and Rayner book is in print and the companion site carries
the audio examples the paper figures refer to, including the Mussorgsky extract
Oudre's percentages were measured on. Everything with a DOI above is on IEEE Xplore.
