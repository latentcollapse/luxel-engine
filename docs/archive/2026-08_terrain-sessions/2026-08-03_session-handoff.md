# Handoff — 2026-08-03, late session

Written to be picked up cold. Everything below is measured, not remembered.

Context: Matt reviewed `docs/platform/open-decisions.md` and resolved all eight entries, and
set a work order. This session executed the first three, stopped the fourth on a
finding, and red-teamed its own work.

---

## Landed

### Item 5 — bog depth classification

`pipeline/hydrology.py`. A closed sink with low inflow was called a bog at any
depth, so the arena had a 7.96 m "bog" rendered stagnant green.

**The tell nobody had looked for:** the output already contradicted itself.
Fifteen bodies were classified `bog` and six of those were also flagged
`swimmable`. You cannot swim in a bog; that is what makes it a bog.

The cause of the second half was that `swimmable` was hardcoded at **1.6 m**
against an `agent_height_m` of **8.0** — chest height on a human, shin height on
the actual agent. Both thresholds are now fractions of the agent
(`BOG_DEPTH_FRACTION = 0.25` knee, `SWIM_DEPTH_FRACTION = 0.75` shoulder), so
they stay correct for any actor and a world that changes its actor size does not
silently reclassify its water.

Kinds are now bog / **tarn** / pond. `classify` raises if it ever emits a
swimmable bog again. Result: 10 bog, 5 tarn, 3 pond, zero contradictions.

### Item 6 — the fifth splat layer

Bigger than the decision doc assumed, in a useful way: `wetland_weight` already
existed and was already emitted as `wetland_mask.png`. It fed a 22% darkening of
a preview PNG and a bake no shipped backend reads. Every real backend iterated
four channels, so **a bog rendered as grass**.

Promoting it exposed a defect that had been harmless while wetland was a soft
tint. Measured before the fix:

| slope band | share that was strong wetland |
|---|---|
| 0-5° | 16.9% |
| 25-35° | 24.0% |
| 35-90° | 24.3% |

Mean slope under wetland was 21.6° against 17.8° elsewhere — the field was
*anti*-correlated with ground that can hold water. `water_weight` had been
slope-gated with a comment reading "retain their wetland influence"; that was a
sound call for a tint and wrong for a material.

Wetland is now gated by `forestry.BOG_MAXIMUM_SLOPE_DEGREES`, **imported** so S6
plants sedge and S7 paints peat by the same number. After: 0% strong wetland
above 15°, mean slope under wetland 2.9°, `steep_wetland_fraction` 0.0.

Contract: five layers across two images (`splatmap.png` RGBA + `wetland_mask.png`
L), all five weights summing to 1. Declared in `channel_convention`. Plumbed
through the Unreal weightmaps, Unity layer order, the material bake, and the
Bevy reference renderer — shader included, where the fifth weight had to enter
the normalisation rather than follow it, or it would have been normalised out of
existence.

**Side effect worth knowing:** the D25 black-pixel gate now **passes** (0.0645
against 0.08, from 0.094). A good part of D25 was this bug darkening cliffs.
Verified by experiment that peat compositing is not responsible — with the peat
term removed the figure is 0.0661.

### Item 7 — the Rust scatter crossing

`render_plan.rs` now reads `canopy_suitability_u8.bin`. Four design points, two
of which changed the shape of the work:

1. **The layer declares its field** (`asset_plan.ECOLOGY_FIELDS`) rather than
   Rust sniffing `role` for "canopy". Canopy suitability governs canopy; gating
   groundcover or forest-floor rock on it would strip scree out of the open
   ground it belongs on.
2. **The scatter samples the field rather than rejecting against it.** Canopy
   suitability is non-zero on 2.7%–5.2% of each forest polygon against a
   50-darts-per-instance budget, so uniform rejection sampling would have failed
   the `placed N of target` contract intermittently — on terrain, not on code.
   Proportional drawing also thins the treeline instead of stamping an edge.
3. **The validator checks a floor, not a re-derivation.** Under proportional
   sampling there is no single value to demand without reimplementing the RNG
   and guaranteeing drift. Nothing may stand where its field is zero. The
   roadmap's feared "validator rejects every plan the compiler just built" never
   had to arise.
4. **The authored count became a ceiling** for ecology-governed layers.
   `central_forest` asks 28 conifers and its ecology holds nine. Shortfalls go to
   `render_plan.ecology_shortfalls` and are printed by the build. Layers with no
   declared field keep the strict quota.

Result: 50 conifers where 112 stood, every one on suitability ≥ 151/255, none on
zero. 275 instances, down from 337. `canopy_suitability_sha256` binds the plan to
the field. **Rebuilds are byte-identical** (verified across two full builds on
five artifacts).

---

## Found while red-teaming my own work

### A real defect in code written the same hour

`draw_weighted` jittered a drawn cell by `rng.range(-half, half)`. That returns
exactly `-half` whenever `unit()` returns 0.0, and Rust rounds halves *away from
zero*, so that offset reads the **previous** cell. If that neighbour is
unsuitable, the compiler places a tree the validator then rejects — a hard build
failure at roughly 2^-53 per draw, which would have presented as a ghost failure
on one machine and not another. Exactly the compile/validate divergence the
crossing was written to avoid.

Same function also jittered **both axes by the width-derived cell size**, wrong
on any non-square world.

Both fixed, both pinned by tests that check the worst case directly rather than
by sampling — no number of random draws surfaces a 2^-53 event.

### A provenance hole I had just widened

`capture_bevy` hashes `.rs` under `world_core` to prove the binary matches
current sources. The terrain shader is **WGSL loaded at runtime from the game
repo**, so it is invisible to that digest — edit the shader, change every pixel,
and the capture reports the same viewer digest as the run before. I had just
edited that shader. Captures now record `terrain_shader_sha256`.

---

## Stopped, and why

### Item 8 / D13 — will not do what it was scheduled to do

D13 says twelve placements obstruct lanes; closing it was scheduled to clear
navmesh acceptance. Broken down by owner — which nobody had done:

| lane | obstructions | owner |
|---|---|---|
| `north_lane` | 14 | **12 `hibernia_keep`**, 2 `westcentral_hamlet` |
| `central_lane` | 10 | `southwest_hamlet` 5, `eastcentral_hamlet` 5 |
| `south_lane` | 17 | **14 `albion_keep`**, 3 hamlets |

Moving every hamlet clears `central_lane` and leaves two lanes failing. The
dominant blockers are keep **gate components** — piers, flank roofs, flank towers
— standing in the lane the gate exists to admit. They are correctly absent from
the siting plan: a keep is exempt from lane keep-out because the lane is *meant*
to pass through its gate. The defect is that the gate's collision then blocks the
aperture.

Logged as **D29**. It needs a decision — carve a traversable aperture through
gate collision, model the gate as a doorway with a navigable span, or route the
lane around the keep and stop claiming it runs through. Work stopped rather than
build a fix that could not achieve its stated purpose.

---

## Open blockers for the next session

1. **D29 — needs Matt's decision.** Above. Blocks navmesh acceptance.
2. **Python suite is unconfirmed** — unconfirmed, *not* known-broken.
   `unittest discover -s tests` stalls in `test_capture_bevy` at ~1% CPU. The
   same module passes 12/12 via `python3 -m unittest tests.test_capture_bevy`
   (91s). **Rust 29/29 and Julia 8/8 are confirmed green.**

   Ruled out by experiment: my `terrain_shader_sha256` addition (removed it, the
   stall persisted); the viewer's new `canopy_suitability_u8.bin` requirement
   (these tests use a *fake* viewer script, never the compiled binary); GPU and
   cargo lock contention (reproduces with nothing else running).

   **Leading hypothesis is I/O, not deadlock.** The stall point *moved* between
   runs — 109 dots, then 88, then 4 — which is the signature of something slow
   rather than something blocked. `_viewer_source_digest` walks
   `world_core/{apps,crates}` hashing every `.rs`, measured at 9.9s warm, once
   per test in that class; the workspace is on NTFS (`/mnt/d`) and the machine
   had been building and capturing all session.

   **Run the suite on a cold, idle machine before assuming a defect.** If it
   passes, the real finding is that `source_tree_digest` over NTFS wants a
   per-process cache.
3. **D28 — the visual gates now argue against the ecology.** `foliage_fraction`
   0.00682 against 0.008, because obeying S6 thinned the forests. Third instance
   of the D16/D25 defect class. **Do not re-baseline it.** Either the ecology is
   miscalibrated or the gate measures the wrong thing, and item 4's second world
   settles which — which is now a stronger argument for doing item 4 than the one
   originally recorded.
4. **Road readability gate** fails at 0.00686 against 0.008. Pre-existing,
   previously unrecorded, confirmed unrelated to this session's work.

## Remaining work order

D26 (transition as a field) → item 2 (D25, now largely prophylactic) →
item 4 (second gentler world, which settles D28 and item 4's own question).

Item 9 (drive Gaea from WGE, emitting `.terrain` and building headlessly via
`Gaea.Swarm.exe`) is agreed in principle and scoped nowhere.
