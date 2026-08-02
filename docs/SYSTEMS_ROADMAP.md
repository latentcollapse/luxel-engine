# WGE systems roadmap — the algorithmic layer

Written 2026-08-02, out of a working session that fixed two sealed keeps, a
world identity that depended on its own directory, and a Unity adapter that had
been silently downgrading every asset. All three had the same shape: **something
with a correct answer was left to a person, a habit, or a name.**

This roadmap is the inventory of things with correct answers.

Companion documents: [MVP_ROADMAP.md](MVP_ROADMAP.md) (phases 0–4, the path to a
playable map), [DEBT_LEDGER.md](DEBT_LEDGER.md) (what exists and is wrong),
[MISSING_INVENTORY.md](MISSING_INVENTORY.md) (what does not exist yet),
[TOOLING_UPGRADES.md](TOOLING_UPGRADES.md) (the instrument).

---

## The principle this whole document is an application of

**Algorithms own anything with a correct answer.** Drainage, enclosure, slope
feasibility, collision, navigation, treeline, scree, sightlines. These are
*solvers*, not knobs. A solver reports what it derived and why; it never asks
the author to tune toward an answer a computer could compute.

**The model owns intent and taste.** "A bleak highland pass." "The river should
threaten the south lane." "Three lanes, jungle between." Choices with no correct
answer, where being wrong is a matter of judgement rather than of fact.

**Tooling owns the boundary between them.** When authored intent and derived
reality disagree — the painting puts a river where the ridge drains the other
way — that is not a crash. It is a finding, phrased in the author's vocabulary,
naming the knob and the direction: *"lower this saddle by 4 m, or move the
river."*

### Why this is the load-bearing decision, not a style preference

Two consequences fall out of it, and both are strategic.

**It sets the skill floor.** If the model is doing arithmetic, model quality is
irrelevant — arithmetic is right or wrong, and a 3B and a 27B will score the
same because the design set the outcome, not the model. That result is already
on record for this project. Every piece of derivable work left in the model's
lap is a wasted measurement, and a barrier to entry for no return. Push the
arithmetic into solvers and a small local model becomes a viable world author.

**It makes style a first-class axis instead of noise.** Once the model supplies
only intent and art, *different models produce recognisably different worlds
from the same terrain*. That is not a defect to normalise away — it is the most
interesting property the system has. It also means asset families can be sourced
per-author: one model may be strong at rock and foliage and weak at masonry;
another the reverse. Mixing them is desirable and needs to be safe, which is
what [S16](#s16-authorship-provenance-and-style-coherence) exists for.

---

## The architectural spine: one shared field, many consumers

Most systems below want the same handful of numbers about every point on the
map. Computing them independently in each system is how the trees, the soil and
the villages end up disagreeing about where the wet hollow is.

**[S1] Site conditions** computes them once. Everything downstream reads it.

```
heightfield ──► S1 site conditions ──┬──► S6 forestry
                     ▲               ├──► S7 surfacing
                     │               ├──► S8 scree / talus
                S2 hydrology         ├──► S10 settlement siting
                     ▲               └──► S9 routing
                S3 erosion
```

That layering is also why the sequencing at the bottom is not negotiable:
forestry that runs before hydrology cannot know where it is wet, and will place
a pine bog in a dry corrie forever.

**A second economy:** hydrology, erosion, scree, wetness and seed dispersal are
all *flow and diffusion on a grid*. One shared numerics kernel serves them, and
`terrain_lab` (Julia) is the right home for it — it already owns the heightfield
numerics and the language suits it.

---

## Terrain and landform

### S1. Site conditions field
**Tier: Blue · Who: S · Depends on: nothing**

Per-cell derived terrain properties, emitted once as a stacked raster:
slope, aspect, planform/profile curvature, topographic wetness index,
insolation (aspect + a declared sun arc), wind exposure, relative elevation
above local drainage, and distance-to-water once [S2](#s2-hydrology) exists.

**Derived, not authored.** The author never sets "wetness"; they set a climate,
and the field says which hollows hold water.

**Gate:** every field is finite, in range, and reproduces byte-identically from
the same heightfield.

---

### S4. Border enclosure — **DONE 2026-08-02**
**Tier: Green · Who: O · Depends on: nothing · Roadmap 2.5**

`zone_rasterizer._border_rampart`, authored through `border_policy`
(`enabled`, `profile`, `height_m`, `crest_m`, `inner_face_m`,
`landmark_clearance_m`). `boundary_plan` now reports `enclosed: true`, zero leak
spans, `edge_reach_fraction 0.0` — from 756 m across seven spans — and
`build_zone` **fails** rather than warns if that regresses.

Three things it taught, all recorded in the tests:

- **Depth is a field, not a number.** A single global depth is set by whichever
  landmark sits nearest the edge; one watchpost 28 m from the west edge thinned
  the border on all four edges. Per-cell, the rampart runs full depth where
  there is room and pinches only around that landmark's exclusion disc — and
  pinching is safe, because the same height over a shorter face is steeper.
- **The rampart is `protected_relief`, not `background`.** Left untagged, the
  terrain accessibility gate measured its face as walkable ground that happens
  to be a cliff, and the build failed *for closing the world* (p99 grade
  1.69 → 7.15).
- **A linear face fails at its own toe.** The face is unwalkable by design, but
  the cells where it meets flat ground are walkable, and a grade discontinuity
  there is a measured defect (1.69 → 2.65). Smoothstep arrives tangentially.

Height turned out to be a purely visual parameter — the face angle is what stops
a body — and 42 m shadowed the apron badly enough to fail the black-pixel gate
at 13.1% against an 8% limit. 26 m passes at 7.5% and reads as a rim.

**Cost:** playable area fell from 44,993 m² to 34,860 m² (68% → 53% of the
world). The rampart is currently *additive*; roadmap 2.5's intent was that it
replaces the bordering mountains hugging the outer lanes, freeing that space for
jungle and camps. That rework is not done and is worth doing before the map is
balanced.

**Authored:** wall character (escarpment or ridge), height, crest and face
depth, how much clearance a landmark is owed.
**Derived:** the per-cell depth field, where the rampart must pinch to avoid
burying something, the face profile, and the protected-relief footprint.

---

### S18. Massif character — what *kind* of mountains
**Tier: Orange · Who: O · Depends on: S1 (and pairs with S3)**

**The same argument as the ponds, applied to rock.** Today a landform is a
`profile` string and a handful of jitter scalars, and every world it builds is
the same mountain in a different arrangement. Reviewed 2026-08-02 against a real
target — Lauterbrunnen — the border read as "a square bowl with a flat rim", and
the honest diagnosis was not that the rampart was tuned wrong but that **the
shaping system has no concept of mountain type.**

An author should say *which orogeny*, and the solver should produce it:

| Character | Signature the solver must produce |
|---|---|
| **Alps** | glacial U-valleys with flat floors, near-vertical walls, hanging valleys and waterfalls off the rim, horns and arêtes above |
| **Scottish Highlands** | lower, rounded, glacially scoured; broad corries, heather benches, wide straths |
| **Patagonian Andes** | granite spires and towers, very steep, ice-carved, high relief over short distance |

Matt's own scoping (2026-08-02): Appalachians are worn enough to be "really
giant hills", which the Highlands profile covers with a lower relief budget, and
the Rockies read close enough to the Alps not to need their own model. Chinese
needle karst is a genuine fourth case and is deliberately deferred — the three
above span most of what the map types need, and the point is to get those
dialled in before widening.

**What this is really about:** each character is a different *erosion history*,
not a different noise seed. Glacial carving produces U-profiles and truncated
spurs; fluvial produces V-profiles and dendritic ridges; the Highlands are the
same glacial process run longer on softer rock. So this is the same machinery as
[S3](#s3-erosion-and-geomorphology) with different parameters and different
stopping points, which is why they pair.

**Authored:** the character, the relief budget, whether the map floor is a
valley or a plateau.
**Derived:** valley cross-section, ridge sharpness, cirque and corrie placement,
where walls go vertical, where waterfalls enter ([S2](#s2-hydrology)).

**Absorbs S4.** The border rampart is currently its own bump function with its
own crest and face parameters. Once mountains have character, the border should
be *the same system applied at the perimeter* — an Alpine map's edge is an
Alpine wall — rather than a separate shape that has to be tuned to look like the
mountains beside it.

---

### S3. Erosion and geomorphology
**Tier: Orange · Who: O · Depends on: S1**

Hydraulic and thermal erosion so terrain reads as weathered rather than
procedural. Carves valleys where water actually flows, rounds ridges, deposits
alluvium in basins.

**Why it is worth the cost:** it is the single largest jump in believability per
line of code, and it makes [S2](#s2-hydrology) *correct* rather than merely
plausible — real drainage follows eroded terrain because eroded terrain was
shaped by drainage. Run it before hydrology classification, iterate.

**Risk to respect:** erosion changes every gate downstream. It must run before
the terrain contract is frozen, never after.

---

## Water

### S2. Hydrology — **v1 DONE 2026-08-02**
**Tier: Red · Who: O · Depends on: S1 (and better after S3)**

`hydrology.py` + `hydrology_plan.json`. Outlet selection and carving, drainage
network, and sink classification into fed ponds and stagnant bogs. On the alpine
arena: standing water fell from **46,157 m² to 19,828 m²** once the world was
given somewhere to drain, leaving 14 genuine bodies — 2 fed ponds, 12 bogs.

**The outlet is measured, not authored.** Every rim cell is scored on the
drainage arriving behind it per metre of rock in the way. Lowest-point alone
would notch wherever the rim happens to dip even if nothing flows there;
wettest-alone would drive a gorge through a summit. The ratio is what makes it
a saddle with a river behind it.

**A disc at the rim is a dimple, not an outlet.** The first carve was a 9 m
radius bowl centred on the rim cell against a ~28 m deep wall: it lowered the
*outside* of the wall and left the basin intact, still holding 46,157 m². The
notch has to run inward far enough to reach the ground it drains, with the bed
falling toward the edge — water level with the ground it drains does not flow.

**Still open for v2:** channel carving along derived reaches (the network is
measured but only the outlet is cut), Strahler ordering, waterfall detection at
gradient breaks, reconciliation against authored water, and depth-aware
classification — a 15 m-deep closed hole currently reads as "bog" on catchment
alone, when it is really a tarn.



The system this roadmap was commissioned for. Current state: eight authored
centrelines, all declared `wetland_rill`, all width 2.0, carving essentially
nothing — so the map has blue splatmap patches on flat ground, and bridges
crossing them ([D20](DEBT_LEDGER.md)).

**Replace authored water with a derived network.** Standard, solved terrain
analysis, all cheap on a 1025² grid, all deterministic:

1. depression filling (priority-flood) → sinks identified
2. D8 / D-infinity flow direction
3. upslope accumulation → channels emerge above a threshold
4. Strahler ordering → width and depth per reach
5. sink classification by catchment and outflow

Everything the art direction asks for is a *consequence of position in that
graph*, not a property anyone authors:

| Wanted | Derived from |
|---|---|
| stagnant green bog, lilypads | closed sink, negligible catchment, no outflow |
| deeper, darker, bluer pond | sink with upstream inflow |
| trickle from the mountains | low-order reach, high elevation |
| river across the map | high-order reach |
| runs off the edge | outlet reaching the map rect |
| waterfall | channel crossing a steep gradient break |
| walkable ford vs swimmable | reach depth vs the agent — feeds navigation |

**Blocking finding from S1 (2026-08-02): the world is now a closed basin.**
Once [S4](#s4-border-enclosure--done-2026-08-02) rings the map, the interior
floor sits 14.1 m below the lowest point of the rim, and the depression-filling
pass reports **52,446 m² of sink in a 65,536 m² world** — 80% of it. Physically
correct and gameplay-correct (that is what "enclosed" means), but it means every
drop of water that lands in the map stays in it: nothing flows, nothing leaves,
and the whole surface reads as one lake bed.

The art direction explicitly asks otherwise — water should "run and drop off the
edge or pool into a small pond". So **the border needs a declared outlet**: one
or two notches in the rim, cut to the valley floor, where the drainage network
terminates in a waterfall off the map. That is a border concern and a hydrology
concern at the same time, and it is the first thing S2 has to settle. Until it
does, sink classification cannot distinguish a bog from the entire map.

**Emits** `hydrology_plan.json`: the network graph, per-body classification
(flow, depth, trophic class), channel carving deltas for the heightfield, and
reconciliation findings against authored intent.

**Reconciliation is the interesting part.** The concept art says where water is;
the terrain says where water goes. Where they disagree, that is a finding with a
named repair, not a silent override in either direction.

**Fixes for free:** [D20](DEBT_LEDGER.md) — a crossing goes where a lane meets a
reach with real depth. Same rule, no re-authoring, and it starts producing
bridges the moment riverbeds are carved.

**Authored:** climate/wetness, whether the map drains off-edge or to an interior
basin, which water is gameplay-relevant.
**Derived:** the entire network and every semantic above.

---

## Ecology and surfacing

### S6. Forestry and vegetation ecology
**Tier: Orange · Who: O · Depends on: S1, S2**

Today foliage is scattered into authored polygons at a declared spacing. That is
placement, not ecology, and it cannot produce a treeline, a riparian fringe, or
a bog that looks like a bog.

**Species by niche, density by suitability, arrangement by dispersal.** Each
species family declares a niche envelope (elevation band, slope tolerance,
wetness range, insolation preference, exposure tolerance). The solver scores
every cell for every species and resolves competition.

Falls out for free: treeline, aspect asymmetry (denser on shaded slopes),
riparian species tracking watercourses, bog vegetation exactly where [S2](#s2-hydrology)
put stagnant water, wind-flagged sparse growth on exposed ridges, clearings
where soil is too thin.

**Authored:** which species exist and the world's character ("bleak", "old
growth").
**Derived:** where each grows, how densely, how large, and how it clusters.

---

### S7. Surface materials
**Tier: Yellow · Who: S · Depends on: S1, S2**

Splatmap layers derived from the same field rather than painted: scree on steep
slopes, peat in wet hollows, exposed rock on high curvature, alluvium in basins,
snow above an elevation-and-aspect line, worn ground along routes.

**Shares its entire input set with [S6](#s6-forestry-and-vegetation-ecology)** —
which is the point. Soil and vegetation derived from one field agree with each
other by construction; painted separately they never quite do.

---

### S8. Scree, talus and rockfall
**Tier: Green · Who: S · Depends on: S1**

Debris accumulates below cliffs by slope and rockfall shadow, at an angle of
repose. Currently hand-scattered boulders. Cheap, and it fixes the "rocks
sitting on a slope they could not rest on" tell.

---

### S5. Snow and ice line
**Tier: Grey · Who: S · Depends on: S1**

Elevation threshold modulated by aspect and shelter. Small, and it makes the
alpine massif read as alpine.

---

## Routes and siting

### S9. Route solving
**Tier: Yellow · Who: O · Depends on: S1, S2**

Roads and lanes as least-cost paths over slope, wetness and crossing cost, given
authored endpoints and intent — instead of authored splines that the terrain
then fights.

**Directly relevant to open defects:** lanes currently fail end-to-end, and one
gap is a lane centreline running through a keep's gatehouse rather than through
its gate. A router that knows the gate's declared threshold ([S13](#s13-affordance-contracts))
routes through it.

**Authored:** endpoints, how many lanes, whether a route prefers valleys or
ridges, gameplay width.
**Derived:** the path, its crossings, its grade profile.

---

### S10. Settlement and structure siting
**Tier: Yellow · Who: O · Depends on: S1, S2, S9**

Sites scored on buildable flatness, water proximity, defensibility, route
access, and exclusion from gameplay corridors.

**This is the correct fix for [D13](DEBT_LEDGER.md)** (settlements on lane
centrelines, still failing all three lanes today). A siting solver that scores
route access *and* corridor exclusion would never place a village across the
road it exists to serve. Tuning the current scatter is treating the symptom.

Also the home for Matt's decomposition note (2026-08-02): the "villages" are
currently a village and a lane-guardian tower fused into one asset. Those are
two different things sited by two different rules — background dressing sited for
plausibility, lane guardians sited for gameplay spacing. They must separate at
the asset level ([S14](#s14-kit-decomposition-and-variation)) before siting can
be correct.

---

### S11. Visibility and tactical analysis
**Tier: Yellow · Who: O · Depends on: S1, S6**

Viewsheds, sightlines, cover, chokepoint identification. Emits the tactical
description of a map — where you can be seen from, where a fight is forced.

**Why it belongs in the engine:** it is the measurable half of "is this map any
good", and it is what tells an author their mid lane has no cover before anyone
plays it.

---

### S12. Gameplay data derivation
**Tier: Blue · Who: S · Depends on: S9, S10 · Roadmap 2.1–2.4**

Spawn transforms from keep placement and gate bearing, lane waypoint chains,
objective and camp siting by declared spacing rules, named inter-lane volumes.
Mostly mechanical once the systems above exist — and mostly blocked on them.

---

## Assets

### S13. Affordance contracts
**Tier: Blue · Who: O · Depends on: nothing · Started 2026-08-02**

Assets declare what they afford — measured from their own geometry — and the
claim is re-verified against the mesh and the placing world's agent.
`enterable` shipped today (`asset_affordance.py`); the pattern generalises to
`climbable`, `provides_cover`, `occupiable`, `destructible`, `attachable`.

**Why this becomes more important, not less, once assets are generated:** a
generated keep has no idea it needs a passable gate. Concept art *does* carry
that intent — a painted keep shows its portcullis and its ramparts — so a
generative pipeline will get **depiction** right, and probably far better than a
hand-authored kit. What it cannot guarantee is that a depicted affordance is
*physically realised at the agent's scale*. That is exactly the failure the hand
kit had: it depicted a gatehouse, a drawbridge and a gate leaf, and was sealed
shut. Generated art makes that failure prettier, not rarer.

**The eventual form:** the affordance should be derived from the *annotation* —
the art declares the gate, the generator builds it, the measurement verifies the
build honours the art. The sidecar shipped today is an intermediate step toward
that loop, not its destination.

---

### S14. Kit decomposition and variation
**Tier: Yellow · Who: S · Depends on: S13**

Composite assets separate into individually placeable parts, and families vary
procedurally rather than repeating.

Two live drivers: village clusters are one baked asset per village
([D13](DEBT_LEDGER.md), partly addressed by the individual-building kit), and
the fused village/lane-tower asset needs splitting before [S10](#s10-settlement-and-structure-siting)
can site either correctly.

---

### S15. Asset generation from reference art
**Tier: Red · Who: G/M · Depends on: S13, S16**

The 2D-image-to-3D-asset pipeline (Blender-MCP, in prototyping). Concept art in,
textured mesh out, with the affordance contract and style descriptors emitted
alongside so the result is checkable rather than merely pretty.

**The acceptance layer is the prerequisite, not the follow-up.** A generative
pipeline will produce beautiful sealed keeps at a higher rate than anyone can
inspect by eye. S13 and S16 are what make it safe to point one at a map.

---

### S16. Authorship provenance and style coherence
**Tier: Orange · Who: O · Depends on: S15**

**New, and the system that makes mix-and-match asset authorship safe.**

Once the model supplies only intent and art, different models produce
recognisably different worlds — and asset families can be sourced from whichever
model is strongest at that family. One may be excellent at rock and foliage and
poor at masonry; another the reverse. Frontier models will each have a coherent
artistic signature across everything.

That is a feature. Unmanaged, it is a collage.

**Three parts:**

1. **Provenance.** Every asset family records its author, prompt/reference, and
   generation parameters. Non-negotiable: without it, "the trees look wrong" is
   unattributable and unfixable.
2. **Style descriptors.** Per-family measurement — palette statistics, silhouette
   complexity, material response, edge/wear character, scale discipline. The
   metrics already exist in scattered form (`style_reference`, `silhouette`,
   `metric_honesty`); this consolidates them per family rather than per frame.
3. **Coherence gate.** Cross-family distance in descriptor space, against a
   declared world style budget. A world may deliberately mix — the gate reports
   *how far apart* families sit and which one is the outlier, so mixing is a
   decision rather than an accident.

**Beware the metric-honesty trap** ([D16](DEBT_LEDGER.md)): a naive coherence
score is minimised by making everything identical and grey. Coherence must be
measured as *consistency of relationships* — does every family treat light,
wear and scale the same way — not as similarity of appearance.

---

### S17. LOD and runtime budget
**Tier: Green · Who: S · Depends on: S14**

Automatic LOD chains, billboard impostors, instancing budgets, triangle budgets
per role. Wholly algorithmic; currently partly hand-authored per kit.

---

## Sequencing

Ordered by dependency and by payoff, not by size:

| Order | System | Tier | Why here |
|---|---|---|---|
| 1 | ~~[S4](#s4-border-enclosure--done-2026-08-02) Border enclosure~~ **DONE 2026-08-02** | Green | Gate already written; immediate visible payoff; no dependencies |
| 2 | [S1](#s1-site-conditions-field) Site conditions | Blue | The spine — everything after this reads it |
| 3 | [S2](#s2-hydrology) Hydrology | Red | Largest single win; fixes D20; unblocks ecology |
| 4 | [S7](#s7-surface-materials) Surfacing + [S5](#s5-snow-and-ice-line) Snow | Yellow/Grey | Cheap once S1+S2 exist; large visual return |
| 5 | [S6](#s6-forestry-and-vegetation-ecology) Forestry | Orange | Needs wetness to be worth doing |
| 6 | [S14](#s14-kit-decomposition-and-variation) Kit decomposition | Yellow | Unblocks correct siting |
| 7 | [S9](#s9-route-solving) Routes + [S10](#s10-settlement-and-structure-siting) Siting | Yellow | Together these close D13 and the lane failures |
| 8 | [S12](#s12-gameplay-data-derivation) Gameplay data | Blue | Mechanical once routes and siting are solvers |
| 9 | [S3](#s3-erosion-and-geomorphology) Erosion | Orange | Highest believability/effort, but re-baselines every gate — do it deliberately |
| 10 | [S16](#s16-authorship-provenance-and-style-coherence) Style coherence | Orange | Needed before generated assets arrive in volume |

**[S8](#s8-scree-talus-and-rockfall), [S11](#s11-visibility-and-tactical-analysis)
and [S17](#s17-lod-and-runtime-budget)** are independent and can be picked up
whenever there is a gap.

**[D1](DEBT_LEDGER.md) folds in rather than being scheduled.** "Which knob values
are buildable" is much easier to answer once solvers own the derivable
parameters — there are simply fewer knobs left to constrain.

---

## What every system on this list owes

Non-negotiable, and the reason the defects in the ledger were findable at all:

- **An emitted artifact**, hash-bound to everything it derives from.
- **A gate that can go red**, verified against deliberately broken input rather
  than synthetic fixtures.
- **Failures phrased as repairs**, naming the authored input and the direction to
  move it.
- **A stated authored/derived boundary**, so nobody has to guess which knobs are
  theirs.
- **Determinism**: same input, byte-identical output, independent of where the
  batch sits on disk ([D22](DEBT_LEDGER.md)).
