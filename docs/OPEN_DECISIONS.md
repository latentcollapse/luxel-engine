# Open decisions — calls made without review

Written 2026-08-02. Ten systems were built in one session past the last point
Matt confirmed anything. Everything in it is tested and committed in small
pieces, but a decision nobody has looked at is not the same as a decision that
has been made, and the further the work goes the more expensive each of these is
to change.

**This document exists to be argued with.** Each entry is a call I made alone,
with the value, why, where it lives, and what it costs to reverse today. None of
them is load-bearing on the others unless the note says so.

Ordered by what they cost if left wrong.

---

## 1. Vertical budget: 62 m of border relief on a 256 m map

**Where:** `zone_compiler.DEFAULT_BORDER_POLICY["height_m"]`
**Reverse:** one number.

A ratio of roughly 1:4. Lauterbrunnen — the stated reference — is nearer 1:1;
the walls are as tall as the valley is wide. Genuinely Alpine proportions here
would mean 150–250 m walls.

I stopped at 62 m because playable area had already fallen from 68% to 50% of
the world, and past this the border stops framing the map and starts eating it.
That is a design trade, not a technical limit, and it is Matt's to make.

**Load-bearing on:** everything visual. Raising it changes shadow, camera
framing, how much sky a third-person camera sees, and the snowline (which is a
fraction of relief). Worth settling before more is built on top.

---

## 2. Refusing the black-pixel gate rather than flattening the mountains

**Where:** `bevy_visual_acceptance`, threshold 0.08; current value 0.094.
**Reverse:** either re-baseline the threshold or lower `height_m`.

The gate fails. I did not fix it, because the cheapest way to satisfy it is to
build flatter mountains, which is the same standing-incentive defect as
[D16](DEBT_LEDGER.md) — a metric arguing against the art direction it is meant
to serve. Logged as [D25](DEBT_LEDGER.md) with a fix direction (separate
*unreadable* from *shadowed* by measuring contrast inside the dark region).

**The risk of my choice:** a red gate that everyone learns to ignore is worse
than no gate. If this is not re-baselined reasonably soon it becomes noise.

---

## 3. Rivers capped at 1.15 m deep

**Where:** `hydrology.channel_depth_field(maximum_depth_m=1.15)`
**Reverse:** one number.

Taken directly from the stated direction — rivers shallow enough to walk,
bridges as an aesthetic choice. The consequence I did not anticipate: at that
depth on a 256 m map they are **invisible from the overview camera**. They will
read at player height and they matter to navigation, but the map does not look
like it has rivers.

The fix is probably not deeper water. It is surfacing that follows the channels
— wet stone, gravel banks, riparian green — which is blocked on item 6.

---

## 4. Heath scrub takes 69.5% of the map

**Where:** `forestry.HIGHLAND_NICHES`, the slope and wetness bands.
**Reverse:** band values, but see below.

Canopy covers 7%. The conifer's 34° slope limit excludes most of a world that is
now mostly cliff. **I could not tell from one map whether this is honest or
miscalibrated**, and I would rather say so than tune numbers until the picture
looks nice.

Disambiguating it wants a second, gentler world — which is exactly the Phase 3
experiment. Tuning it against this map alone risks fitting the niches to one
piece of terrain.

---

## 5. Bogs classified on catchment alone

**Where:** `hydrology.BOG_CATCHMENT`, `hydrology.classify`.
**Reverse:** add a depth term; small and self-contained.

A closed sink with low inflow is called a bog regardless of depth, so the alpine
arena currently has a **15.19 m deep "bog"**. That is a tarn. The concept is
right and the implementation is cruder than the concept — depth needs to enter
the classification.

This one I think is simply wrong and would fix without asking, given the chance.

---

## 6. Peat and alluvium deferred rather than added

**Where:** the splatmap's fixed RGBA of grass/road/rock/snow.
**Reverse:** not cheap — a contract change touching material contracts and every
backend shader.

S2 now identifies twenty stagnant water bodies and S1 knows where the ground is
wet, and none of it reaches the surface because there is no channel to put it
in. A bog that renders as grass is the same class of lie as a keep that reads as
enterable.

I deferred it rather than half-doing it. But it is the blocker on items 3 and 4
both looking right, so it may deserve to jump the queue.

---

## 7. The Rust scatter crossing, specified but not made

**Where:** `render_plan.rs::compile_foliage`, and its validator.
**Reverse:** n/a — not yet done.

Four solvers are correct and unobeyed: forestry does not place trees, siting
does not move villages, S14 split the guardian out and nothing places one. The
field is emitted and the change is specified in
[SYSTEMS_ROADMAP.md](SYSTEMS_ROADMAP.md#s6-forestry-and-vegetation-ecology--v1-done-2026-08-02).

I stopped short deliberately: compile *and* validate must apply an identical
rule or the validator rejects every plan the compiler just built, and starting
that at the end of a long unreviewed session is how you get a broken render plan
and no idea which of eleven changes caused it.

---

## 8. D13 is named, not closed

**Where:** `siting_plan.json`, `obstructing_placements`.

Twelve placements obstruct lanes, worst `southwest_hamlet over central_lane by
19.5 m`. The audit reports structure, lane, metres and repair — a better place
than "no lane runs end to end", but the villages have not moved and the three
lanes still fail.

Moving them means adopting the proposals into `annotations.json` or teaching the
Rust anchor compiler to read the plan. Both are item 7's neighbourhood.

---

## What I would do next, if asked

1. Settle item 1 — it is upstream of every other visual judgement.
2. Fix item 5 without ceremony; it is a defect, not a decision.
3. Then item 6, because items 3 and 4 both stay wrong-looking until surfacing
   can express wet ground.
4. Then item 7, which converts four correct systems into a visibly different
   world and closes item 8 with it.
