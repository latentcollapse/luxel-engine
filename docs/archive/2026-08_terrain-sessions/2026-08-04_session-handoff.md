# Handoff — 2026-08-04

Written to be picked up cold. Everything below is measured, not remembered.

Context: this session ran the remainder of the agreed work order — D26, then
open-decisions item 2, then item 4 — and red-teamed its own work.

It fixed five defects. **Three of them were not on the work order** (D31, D32,
D33), and a fourth was logged unfixed (D34). Of the three, one came out of
fixing D26 and **two were found by the second world on its first build** — which
is the argument for building a second world, stated better than the argument
that scheduled it.

It also corrected two claims this project had already recorded as true, and one
of its own from earlier the same session. Those are first, because they change
what you should believe about work that is already written down.

---

## The correction that matters most

**Two claims made on 2026-08-03 were wrong, and both were about item 7.**

1. *"The scatter samples the field rather than rejecting against it."* Half true.
   It sampled canopy suitability, then applied slope, exclusions and spacing as
   rejection tests afterwards. Suitability knows nothing about lanes, keeps or
   protected landforms, so most darts landed on ground no tree could stand on —
   **3.3% of `central_forest`'s weight was actually reachable**. So *"asks 28
   conifers and its ecology holds nine"* attributed to the ecology a number the
   dart budget had a large share in.

   Rebuilding the arena after the fix, with nothing else changed:

   | forest | before | after | of |
   |---|---|---|---|
   | `western_valley_woodland` | 10 | **27** | 28 |
   | `eastern_valley_woodland` | 3 | **13** | 28 |
   | `central_forest` | 9 | 8 | 28 |
   | total instances | 275 | **301** | |

   `western_valley_woodland` was never held to ten by its ecology.
   `central_forest` lands in the same place either way, which is what a real
   ecological limit looks like. See D33.

2. *"Rebuilds are byte-identical, verified across two full builds."* True, but
   partly for the wrong reason. The render plan read a canopy field that
   **vegetation did not write until 98 lines later in the same function**, so
   every build scattered against the *previous* build's ecology. The input was
   constant because it was stale. See D32.

   **Re-verified properly, and it holds.** With `terrain/` deleted between runs
   — the test the original claim needed and did not have — all five artifacts
   come out byte-identical, including the canopy field itself and the render
   plan that consumes it. **Delete `terrain/` when checking determinism;** a
   rebuild over a populated directory cannot tell a deterministic pipeline from
   a frozen input.

Neither was visible from the arena, because a batch that has been built once
always has the file from last time. Both appeared on the first build of a batch
that had never been built.

---

## Landed

### D26 — the valley transition is a field now

`_transition_run` in `zone_rasterizer.py`. `wall_width_m` is a **ceiling on the
run**, not the run.

**The debt entry's stated cause was wrong**, and this is worth reading before
trusting any of the rest. It said `into` saturated early and left a plateau.
Measured: `into` saturated on only **20.2% of the non-play area**, because
**56.1% of the perimeter had less room than the 34 m run needed** (p10 = 9.4 m
against p90 = 57.3 m of room).

That inverts the consequence. Delivered relief is `massif x into x relief`, so
where `into` never reaches 1 the ridged field is scaled *down*. Inside the
saturated band the massif field averaged **0.080** against a world maximum of
1.000 — every summit sat outside the band and was carved away. Raising
`massif_relief_m` therefore scaled up the low, smooth part of the noise, which
is exactly the reported symptom.

| | before | after |
|---|---|---|
| tallest point | 58.2 m | **94.9 m** |
| delivered fraction of authored relief | 39% | 63% |
| mountain area | 7,109 m² | **10,073 m²** |
| protected cells visually flat | 39.9% | **24.8%** |

The field generalises rather than being fitted to the arena: the second world
authors the *same* `wall_width_m: 34` and reports
`at_authored_ceiling_fraction: 0.285`, median run 27.5 m — a different profile
from the same authored number, on a world with different room, which is what
"ceiling, not run" is supposed to mean.

### D31 — the containment floor was the terrain

Found while fixing D26 and larger than D26. Since the massif inversion the
rampart is applied as `maximum(massif, rampart)` and the code calls it a
containment floor. It kept the play-space and spur terms, so it followed the
rhomboid inland — **and was replacing 87.7% of the protected massif cells**, at
a mean cost of 18.6 m of relief. Nearly nine cells in ten of the "massif-carved"
mountains were the old rampart wearing a new name.

`containment_only` drops those terms. Enclosure still holds exactly:
`enclosed: true`, `leak_length_m: 0`, `edge_reach_fraction: 0.0`.

**Half of it is unfixed and deliberately so.** The floor's own low ground is
`62 x 0.28 = 17.36 m` wherever its noise is near zero, and **10.6% of the world
still sits in one 1.7 m height band** around that value — 92% of it within 28 m
of the map edge. That is the flat rim on the horizon. Fixing it means changing
enclosure geometry, which should not ride along in the same pass as the thing
that exposed it. See D31 for the two candidate fixes.

### Item 2 / D25 — unreadable is now separated from shadowed

`bevy_visual_acceptance.py` fails on `unreadable_fraction` — dark **and** flat —
and reports `dark_foreground_fraction` without gating it.

The crushed threshold is derived rather than tuned: a local range under two
8-bit code values means the detail is not dim but absent, which is a property of
the file rather than a number chosen to make a world pass.

| capture | dark | unreadable | structured |
|---|---|---|---|
| arena `bevy_overview` (what the gate reads) | 0.0027 | 0.0007 | 0.744 |
| arena `bevy_border` (worst good) | 0.0270 | 0.0190 | 0.297 |
| caledonia (failed build) | 0.2260 | 0.1565 | 0.308 |

`UNREADABLE_LIMIT` is 0.04 — twice the worst good capture, a quarter of the bad
one. Three tests pin both directions, including one asserting that a world more
than 8% dark passes when structured, which is the regression D25 exists to stop.

**Red-teaming this found a real flaw in my own reasoning.** The range was first
measured on *luma*, and "one 8-bit code value" does not survive that: two pixels
differing a full step in blue alone are 0.00028 apart in luma. Quantisation is
per channel, so the range is now per channel. Every number moved the way the
rationale predicts.

### D32 — the render plan scattered against the previous build's ecology

Stage order in `build_zone.py`. The S1/S2/S6 block moved to directly after the
terrain manifest is written, which is where its own comment already said it
belonged.

The Bevy viewer had been refusing the resulting worlds all along —
`canopy_suitability_sha256 does not match its source artifact` — which is
correct and vindicates hashing the field. **Nothing upstream ever asked it.** A
build can still report every stage green and emit a world no viewer will load;
that check belongs in the build.

Verified by running the renderer's check by hand against the second world: the
digest `render_plan.json` records and the field on disk are now the same bytes
(`bdbfd2df…f41cea9`).

### D33 — the scatter's candidate set is the admissible set

`suitable_cells` now applies slope and exclusions when it builds the weighted
set, so every dart lands on admissible ground and the placed count is decided by
the ecology and spacing alone — which is what item 7 claimed.

The empty case is now answered before sampling and split in two: **field zero
across the polygon** stays a hard error (the author sited a forest where its own
ecology forbids one), while **field non-zero but every cell occupied** by a lane,
keep, stream or protected landform is a shortfall with `limited_by: "occupancy"`.
Failing there would demand the author measure an occupancy nobody can see.

---

## Item 4 — the second world

`Game Projects/Codeweald/godot_renderer/concept_batches/glenmara_highland_vale_v1/`,
generated reproducibly by `author_annotations.py` beside it.

Same gameplay graph as the arena — same lanes, keeps, hamlets, forests, streams
— with **`massif_character: highlands`, `massif_relief_m` 78 against 150, and
three of six crag fields removed.** Holding the layout fixed is the point: D28
asks whether the ecology is miscalibrated or the foliage gate measures the wrong
thing, and that is a question about terrain.

**One attempt was thrown away and it is worth knowing why.** The first version
also pulled the playable rhomboid in to 78% of its extent, to exercise the roomy
side of D26. Exclusion radii are absolute metres while geometry is normalised,
so contracting the features **fattened every exclusion relative to the world** —
`eastern_valley_woodland` ended with 1,196 suitable cells and zero admissible
ones. The finding was real (it is D33), but the world was not a fair comparison,
which is the only thing it is for. A roomier world needs its exclusion radii
scaled too, and should be a third world.

---

## What the second world actually settled about D28

D28 asked whether the ecology is miscalibrated or the foliage gate measures the
wrong thing. The second world was scheduled to discriminate between them.

| | alpine arena | highland vale |
|---|---|---|
| heath scrub, share of vegetated area | 85.9% | **92.0%** |
| canopy area | 6,803 m² | 4,265 m² |
| measured treeline | 17.6 m | 47.9 m |

**It clears the terrain, and that is a real result.** Halving the relief, going
from alpine horns to highland whalebacks and removing half the crag fields made
the world *more* heath-dominated and produced *less* canopy. The reading that
the arena's cliffs were suppressing its forests is not supported. Heath dominance
is a property of the ecology, not of any one map.

### Then the capture settled it outright: the gate is broken

Two builds of the *same* arena, differing only by this session's fixes:

| | 2026-08-03 | 2026-08-04 |
|---|---|---|
| render-plan instances | 275 | **301** |
| `foliage_fraction` | 0.006820 | **0.005207** |

**More foliage was planted and the metric fell.** D33 added 26 instances; D26
made the massif taller, changing what share of the frame is rock and shadow. The
metric moved because the composition moved, not because the vegetation did.

That is precisely the defect already recorded for `road_fraction` as
[D24](../../platform/debt-ledger.md) — *"frame-relative, and the framing is not
stable"* — and nobody had noticed it applies to `foliage_fraction` identically.

**So D28's answer is that the gate is wrong**, and for a stronger reason than
"a sparse world should be allowed to look sparse": `foliage_fraction` does not
measure the quantity of foliage. **Do not re-baseline it — replace it**, the way
D18 says to fix roads: project the render plan's known instances into the frame
and measure how many are visible. Positions and camera are both already
recorded.

Whether ~90% heath is the world Matt wants is still his call. It is just no
longer tangled up with a broken instrument.

---

## Blockers and what is owed

1. **D29 still needs Matt's decision, unchanged.** Is a keep gate a wall with a
   traversable doorway or a solid object shaped like one? `north_lane` is 12/14
   blocked by `hibernia_keep`, `south_lane` 14/17 by `albion_keep`. Nothing this
   session touched it.
2. **The two opposed views D26 requires are not captured yet.** Everything above
   about D26 is heightfield measurement, not a visual confirmation — and this
   entry is itself the reason to distrust a good-looking number, since the
   previous diagnosis was numerically plausible and wrong.

   The path is clear now, and the order matters:

   ```
   python3 pipeline/build_viewer.py          # Rust sources changed this session
   python3 pipeline/capture_bevy.py <batch> --view east-wall
   python3 pipeline/capture_bevy.py <batch> --view player
   ```

   `capture_bevy.py` refuses to run a viewer older than the sources — the
   tooling item 1 guard, which fired correctly here — so the rebuild is not
   optional. **Do not pipe it through `tail`**; see D34.

   **Both captures now succeed** (`exit=0`, no rejection), which confirms the
   D32 fix end to end: the viewer accepts the world it had been refusing.

   **But neither framing answers the D26 question.** `east-wall` and `player`
   both come out dominated by near-field ground — a sloped surface and a hamlet,
   no horizon, no valley profile. I first assumed the taller world had put the
   cameras underground; **measured, that is wrong** — the eye sits 26–27 m above
   local ground and the aim points 18–43 m above theirs. The framing is simply
   shallow: the eye is 26 m up looking at a target 15 m higher, 130 m away, so
   intervening ground fills the frame. Whether that is the wrong framing or the
   wrong pair of views for this check is a judgement about the instrument, and
   the instrument is already flagged as thinly tested (D5) and unstable in
   framing (D6).

   So the honest position is: **the two opposed views remain owed, and the
   automated pair as currently framed will not supply them.** Flying the viewer
   is the reliable check here.
3. **D34 — the viewer hangs instead of exiting when it rejects a world.** 20
   minutes at 55% of a core, 587 s of CPU, no capture and no exit, for a world
   it had correctly diagnosed in its first second. `capture_bevy.py` calls
   `subprocess.run` with no `timeout`, so `--suite` loses the other three views
   to a hang on the first. (The first version of D34 also blamed a buffering
   `tail` in `capture_bevy.py`; that was wrong — the `tail` was in my own shell
   command. Corrected in the ledger.)
4. **The rebuilt arena's overview now FAILS three visual gates**, and this is
   the most actionable thing in this document:
   - `unreadable_fraction` 0.0756 vs 0.04, `dark_structured_fraction` 0.240 —
     the taller massif throws large shadows and **the renderer clips them to
     black instead of shading them.** A lighting/exposure defect in the viewer,
     not a terrain one. The new gate is what distinguishes those.
   - `foliage_fraction` 0.0052 and `road_fraction` 0.0075 — both frame-relative
     metrics measuring the wrong thing (D24, D28). Neither should be
     re-baselined.

   The shadow crush is the one worth fixing first: it is a real defect, it is
   visible, and it got worse because the world got better.
5. **D28 no longer needs a measurement, only taste.** Is ~90% heath the world
   you want? The terrain is ruled out and the gate is independently broken.
6. **D31's second half is unfixed on purpose.** The containment floor's own low
   ground is a near-flat 17.36 m surface holding 10.6% of the world, mostly the
   rim at the map edge. Fixing it changes enclosure geometry, which should not
   ride along in the pass that exposed it.

## Suite status

| suite | result |
|---|---|
| Python | **543/543 green in 168 s** — up from 513 |
| Rust | **32/32 green** — up from 29, the three new ones are D33's |
| Julia | 8/8 green |

All three confirmed on a quiet machine after the last change.

The Python suite passed **513/513 in 199 s** on an idle machine at the start of
this session, resolving the 2026-08-03 "hang" as NTFS I/O contention rather than
a defect (D30). It now runs **543 tests, green in 168 s**.

**Two intermediate runs failed for reasons that were not defects**, and both are
worth knowing:

- One caught a stale copy of a test I had corrected mid-run — the report field
  it asserts on had just been changed to average over governed cells instead of
  the whole grid. Fixed and re-run.
- Four `FileNotFoundError`s in `test_boundary_plan` / `test_navigation_plan`
  came from **running the suite while rebuilding the alpine arena**. Both have
  `CompiledBatchTests` reading that batch's files off disk, so the rebuild
  deleted `terrain/` under them. They pass 46/46 on their own. **The suite and
  an arena build cannot run concurrently**, and nothing in either says so.
