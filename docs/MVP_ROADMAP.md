# WGE MVP roadmap

> **Historical roadmap.** This 2026-07-31 document predates the native
> engine-neutral convergence and is retained as a decision record. It is not
> the current MVP definition. Use [`../ACTIVE_ARCHITECTURE.md`](../ACTIVE_ARCHITECTURE.md)
> and [`../WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md`](../WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md)
> for the active demo-ready native engine roadmap.

Written 2026-07-31, from the [debt ledger](DEBT_LEDGER.md), the
[missing inventory](MISSING_INVENTORY.md), and the
[tooling plan](TOOLING_UPGRADES.md) (items 1-3 landed).

## What "MVP" means here

**A map a player can walk around in, exported to a real engine, produced from
concept art by a model without hand-holding.**

That definition is deliberate. Today WGE produces a *certified, renderable*
world — terrain, materials, placements, roads, all hash-bound and gated. What it
does not produce is a world anything can *move through*: no collision, no
navigation surface, no spawns. The gap between "renders" and "playable" is the
MVP.

It explicitly does **not** include combat, abilities, units, or netcode. Those
are the game; this is the compiler that feeds it. MVP is the handoff point where
game work can start against a real artifact.

**Done means:** a second map, from different concept art, authored by a model
unaided, that a character controller can traverse in the target engine — lanes
walkable end to end, jungle passable, map border solid.

The "second map" clause is the load-bearing one. One map proves the pipeline
runs; two prove it generalises. Everything measured so far comes from
`codeweald_alpine_arena_v1` alone.

---

## Delegation model

Tasks are tagged for the two-agent split Matt set up over Tether:

- **[S]** — Sonnet. Well-specified, mechanically verifiable, single-module,
  clear done-condition. The kind of task where a wrong answer fails a test
  rather than quietly producing a plausible artifact.
- **[O]** — Opus. Architecture, cross-cutting, ambiguous, or where the failure
  mode is "looks right and isn't" — which is this project's characteristic
  failure and the reason the tooling plan exists.
- **[M]** — Matt. Decisions nobody else can make.

Colour-con tiers follow the workspace convention (Grey → Green → Blue → Yellow →
Orange → Red → Purple) as difficulty.

---

## Phase 0 — Unblock authoring (do first, small, high leverage)

Everything downstream is authored through the DSL, and the DSL currently lets an
author write a legal value that fails a gate with an unactionable message.

| # | Task | Who | Tier |
|---|---|---|---|
| 0.1 | ~~Accessibility gate failures name the responsible knob and direction~~ **DONE 2026-07-31** | O | Blue |
| 0.2 | ~~Do the same for the remaining hard gates (traversal probe, hydrology uphill, terrain contract) — each should name the authored input and the repair, following `_accessibility_diagnosis` as the template~~ **DONE 2026-08-01** | S | Blue |
| 0.3 | ~~`worldbuilder_dsl` docstring/scaffold header states that jitter bounds are *documented* limits, not *buildable* ones, and points at the gate~~ **DONE 2026-08-01** | S | Grey |
| 0.4 | ~~Viewer rejects an incomplete batch by name instead of hanging forever with a blank window~~ **VERIFIED 2026-08-01** ([D9](DEBT_LEDGER.md) — already correct, pinned with a regression test) | S | Green |

**Why first:** 0.2 and 0.4 are the two ways a model author currently gets stuck
with no path forward — an opaque number, or silence. Both are cheap.

---

## Phase 1 — Traversability (the actual MVP substance)

This is WGE work, it is derivable from data the spec already carries, and it
does **not** depend on the backend or netcode decisions. This is the phase that
converts "renders" into "playable".

| # | Task | Who | Tier |
|---|---|---|---|
| 1.1 | ~~**Collision export.**~~ **DONE 2026-07-31** — `pipeline/collision_plan.py`, now build stage 18 of 19, emitting `collision_plan.json`. See notes below. | O | Orange |
| 1.2 | ~~**Playable-vs-render bounds.**~~ **DONE 2026-07-31** — `pipeline/boundary_plan.py`, build stage 19 of 20, emitting `boundary_plan.json` + `terrain/playable_mask.bin`, plus an apron in the viewer and a `border` capture view. See notes below. | O | Orange |
| 1.3 | ~~**Navigation surface.**~~ **DONE 2026-07-31** — `pipeline/navigation_plan.py`, build stage 20 of 21, emitting `navigation_plan.json`, `terrain/navigation_mask.bin`, `terrain/navigation_clearance_f32le.bin`. See notes below. | O | Red |
| 1.4 | ~~Navmesh acceptance gate: lanes connected end to end, each keep reachable from each other keep, jungle reachable from adjacent lane~~ **DONE 2026-08-01** — `pipeline/navmesh_acceptance.py`, wired into `build_zone.py` as a soft gate (writes the artifact, prints a warning, does not fail the build). `--require-passed` opt-in hard gate on the module's own CLI; **owner of flipping it into `build_zone.py`'s real exit path is [D13](DEBT_LEDGER.md)** (settlements on lane centrelines) — do not leave a soft gate with no named task to harden it. Confirmed FAILED on the current alpine arena, naming all three broken lanes, per this ticket's own acceptance criterion. Extended `navigation_plan.py` to resolve biome-polygon anchors (via centroid) into the existing generic anchor-reachability machinery, since jungle/biome anchors were not yet resolved. | S | Yellow |
| 1.5 | ~~Collision acceptance gate: no walkable surface without a collider; no collider floating off terrain~~ **DONE 2026-08-01** — `pipeline/collision_acceptance.py`, wired into `build_zone.py` as a soft gate (same shape as 1.4; no named owner yet to flip `--require-passed` into the exit path, since nothing broken is currently known-open here the way D13 owns 1.4). Confirmed PASSED on the current alpine arena, and confirmed the gate actually goes red against a deliberately-broken copy of the real batch (deleted collider, placement lifted 25 m off terrain) rather than trusting synthetic fixtures alone. **First version of this gate had a real bug worth recording**: derived "bottom of collider" from `centre_m[1] - half_extents_m[1]`, which assumes an asset's local mesh bounds are vertically symmetric around its placement pivot — they are not, and this produced a false "keep buried 17 m" reading in a design review. Root-caused against `render_plan.rs::apply_grounding`'s `grounding_offset_m`/`position_m[1]` and rewritten to check that instead — the value the pipeline's own grounding solver already targets and keeps within 0.35 m by construction. Verified against a random sample of real placements before trusting it. | S | Blue |

**Note on 1.3:** this is the single largest unbuilt piece between a world and a
map, and it is genuinely hard — but `traversal_probe` already has the inputs and
the slope maths. The work is representation and export, not analysis.

### 1.3 as built (2026-07-31)

**No polygonal navmesh, deliberately.** Recast, Unity, Unreal and Godot all bake
one from collision geometry; baking a second here is undifferentiated work every
backend then undoes — the same argument that made 1.1 emit a heightfield
descriptor. What no baker can do is certify that the *game's* required
connections exist. That certification is the artifact.

What it emits instead is strictly more useful to both a baker and a gate:

- **walkable surface** — standable terrain minus everything solid
- **clearance field in metres** — distance to the nearest thing that stops you.
  One field answers the question for *any* agent radius. 1.2 eroded by one
  hard-coded radius, which silently made that one body the only body the world
  was ever measured for; a siege unit would have needed the whole analysis rerun
- **connected components**, so "reachable" is a fact rather than an assumption
- **resolved anchors** — where a keep can actually be stood *next to*. 2.1 must
  place spawns at the resolved point, never the requested one
- **named lane obstructions** — what is standing in the lane, not just that the
  lane is broken (the 0.1 lesson)

**The gap it closed.** `boundary_plan` (1.2) flood-fills terrain slope only and
never reads `collision_plan`, so **7.66% of the region it calls playable is
inside something solid**. Measured before writing anything. Logged as
[D14](DEBT_LEDGER.md).

**A bug of mine from 1.1, found by measuring.** `lane_crossing_structure` had a
solid **box** collider. A bridge sits *on* the lane centreline by design, so
every one of the 20 crossings was a wall across the route it exists to carry —
and no lane ran end to end. Attribution was unambiguous: of 155 non-navigable
lane-centreline samples, **155 were colliders and 0 were terrain**. Added a
`deck` policy: geometry that carries traffic, `obstructs: false`, so navigation
does not subtract it. Lane navigability went 75–80% → 85–92%.

*Honest limit:* a deck's upper surface is not emitted, so a genuinely elevated
bridge over a gorge would leave a gap. Every crossing in this world sits on
walkable ground, so nothing depends on it yet.

**What the map actually looks like now it can be measured:**

| Lane | Navigable | Longest gap | Blocked by |
|---|---|---|---|
| north (top) | 91.2% | 21 m | `westcentral_hamlet`, `hibernia_keep` |
| central (mid) | 85.3% | 17 m | `southwest_hamlet`, `eastcentral_hamlet` |
| south (bottom) | 91.6% | 13 m | `southern_hamlet`, `southeast_hamlet`, `albion_keep` |

Every remaining obstruction is a **settlement cluster placed on top of a lane**
(up to 9.9 m of overlap) or a **keep at a lane endpoint**. Both are placement
defects, not navigation ones — [D13](DEBT_LEDGER.md). `northwest_watchpost` is
not connected to the keeps at all.

---

### 1.1 as built (2026-07-31)

`collision_plan.json` carries a terrain descriptor plus 177 instance colliders
(337 render-plan instances, 160 deliberately non-colliding). Three decisions
worth knowing before consuming or extending it:

- **Terrain is a heightfield descriptor, not a baked mesh.** Unity
  `TerrainCollider`, Unreal Landscape, Godot `HeightMapShape3D` and Rapier/Avian
  all build a native heightfield collider from the same
  `heightfield_f32le.bin` the renderer reads — faster and more stable than a
  triangle soup, and baking one here would be work every backend then undoes.
- **Instance colliders are oriented boxes (centre + half-extents + yaw), not
  world AABBs.** A recomputed world AABB inflates with rotation: a 40 m keep at
  45° would claim a 57 m footprint and block ground beside itself.
- **Role decides shape, and the policy is declared rather than inferred**
  (`ROLE_COLLISION`). Groundcover and understory get *no* collider — 160 of
  them stand in the lanes, and colliding grass makes the map unwalkable while
  every render still looks correct. Trees get a trunk cylinder rather than a
  canopy-sized box. An undeclared role raises instead of defaulting.

**Caught while building it:** asset bounds are not centred on the asset origin
— `highland_fortified_keep_b.glb` spans z=0..184 in its own space, so its
volume centre sits +92 local (12.0 m at production scale) from where the
instance is placed. Using the instance position as the collider centre would
have put every keep's collider 12 m off its building: present, plausible, and
wrong. The local centre is scaled and rotated with the box.

**Not covered:** corridors/roads rely on the terrain collider (they are meshes
laid on it); no collider is verified to sit *on* the ground — that is 1.5,
which is where a floating or buried collider gets caught.

### 1.2 as built (2026-07-31)

Two defects were wearing one costume. *The player falls off* — nothing emitted
said where a body may stand, and `world_bounds_m` (the extent of the **data**)
was being treated as the extent of the **game** by omission. *The horizon ends
in void* — even a world that contains its players has to look like it continues.

**The playable region is measured, not declared.** `boundary_plan.py` floods the
world with the zone spec's own agent (`traversal_policy`: 2.5 m radius, 45°,
4 m climb) from a keep, eroded by the agent radius so a body is a disc and
cannot squeeze through a gap narrower than itself. The reachable set *is* the
playable region. Nobody picked an inset.

**Measured result: the alpine arena does not contain its players.** 756 m of
the 1024 m perimeter is walkable straight off the edge — north and south open
along their full 256 m. Logged as [D11](DEBT_LEDGER.md).

**No synthetic barrier, deliberately.** Fencing the leak would make the map
playable today and the defect invisible forever. The artifact names the spans so
2.5 knows where to build, and `--require-enclosed` turns it into a hard gate the
moment 2.5 lands. The build prints the leak but does not fail on it.

**The apron is a rule, not a mesh** (same treatment as 1.1's terrain): extent =
one world diagonal, falloff = the terrain's own relief, `colliding: false`. One
apron, not one per consumer. An apron a body could stand on would extend the
very leak above.

**Three bugs caught, all the same shape — a check that cannot fail:**

- The containment test asked whether a reachable cell sat on the outermost row.
  Erosion by the agent radius has already removed that row, so it pronounced
  this world *enclosed*. Now measured against a control: the world padded with a
  flat continuation of its own edge, which is exactly what the apron renders.
  Caught only because a pre-implementation prototype had measured 77%.
- The flood was seeded from *every* keep, so the "are the keeps connected"
  check had every keep in the reached set by construction. Seeded from one now.
- `standable` used `np.roll`, so a cell on the north edge sampled the *south*
  edge of the world for its slope ring.

**The renderer half was silently dead.** The apron mesh built, spawned, and
rendered nothing: the apron grid walks +Z as its row index rises, the terrain
grid walks −Z, so copying the terrain's index order mirrored every triangle into
backface culling. An A/B capture with the artifact removed caught it as **zero
differing pixels**. Eyeballing the screenshot had already convinced me it worked.

**The capture suite could not see the defect it was built to catch.** All four
inspection views (`overview`, `west-wall`, `east-wall`, `player`) point *inward*
— the border cliff was only ever found by flying the free camera. Added a fifth,
`border`, which is the only view that looks outward. It is also what makes the
apron verifiable: 25,560 pixels along the horizon band change with it.

---

## Phase 2 — Gameplay data (mostly derivable, mostly mechanical)

The spec already knows this is a three-lane MOBA: keeps tagged `team_a`/`team_b`,
corridors tagged `top`/`mid`/`bottom`, bridges tagged by lane. Nothing consumes
it.

| # | Task | Who | Tier |
|---|---|---|---|
| 2.1 | Spawn transforms at each keep, derived from keep placement + lane entry direction | S | Green |
| 2.2 | Lane waypoint chains from existing corridor centerlines (minion paths) | S | Blue |
| 2.3 | Tower/objective sites along lanes, spaced by a declared rule, gated on being reachable and off the lane centerline | S | Yellow |
| 2.4 | Jungle/inter-lane volumes as named regions (Matt: playable area = keeps + lanes + inter-lane + jungle) | O | Yellow |
| 2.5 | **Close the border.** A straight rectangular mountain wall at the map perimeter; the space freed between it and the lanes becomes jungle and mob camps. Target: `boundary_plan.json` reports `enclosed: true`, then turn on `--require-enclosed` | O | Orange |

**2.5 is a design decision already made, and revised 2026-07-31.** Matt chose
geometry over invisible walls — WGE's model is that geometry *is* the boundary,
and an invisible wall would be a per-backend hack re-authored for every target
engine. The revision: rather than bordering mountains hugging the outsides of
the top and bottom lanes, a **straight rectangular border wall at the map
perimeter**, with the space that frees up becoming jungle and mob camps. It is
simpler to author, it gives the outer lanes real space instead of a cliff, and
it makes the acceptance criterion trivially checkable — 1.2 already emits the
leak spans, and a straight border either closes all four edges or it does not.

1.4 and 1.5 acceptance criteria should assume a straight border.

---

## The algorithmic layer

Phases 1–2 make a map playable. **[SYSTEMS_ROADMAP.md](SYSTEMS_ROADMAP.md)** is
the inventory of what makes it *good*, and it subsumes several items here:
border enclosure (2.5) is S4, and most of Phase 2's gameplay derivation (2.1–2.4)
is S12 and blocked on route and siting solvers.

Its organising claim is the one this project keeps rediscovering the hard way:
anything with a correct answer belongs in a solver, not in an author's hands.
The model owns intent and taste; algorithms own everything derivable; tooling
owns the boundary and phrases disagreements as repairs.

---

## Phase 3 — Prove it generalises

| # | Task | Who | Tier |
|---|---|---|---|
| 3.1 | **[M] Provide second concept-art batch** — different terrain character (Matt mentioned marsh) | M | — |
| 3.2 | Intake + annotate the second batch through the existing pipeline | S | Blue |
| 3.3 | **Sonnet authors the second map unaided.** Every place it gets stuck is a pipeline defect, logged as such — not a model failure | S | Red |
| 3.4 | Fix what 3.3 surfaces | O | Red |
| 3.5 | Both maps pass all gates from a clean build | S | Blue |

**3.3 is the actual experiment.** Per the skill-floor result (a 3B and a 27B
scored identically — the design set the outcome, not the model), where a
competent model gets stuck is a measurement of the pipeline, not the model. That
is the most valuable data this project can generate right now, and it is why
Sonnet authoring beats Opus authoring here: I know too much about the internals
to hit the same walls.

---

## Phase 4 — Export to a real engine

| # | Task | Who | Tier |
|---|---|---|---|
| 4.1 | ~~**[M] Pick the target backend**~~ **DONE 2026-08-05 — Unity** ([G8](MISSING_INVENTORY.md)) | M | — |
| 4.2 | Backend adapter emits terrain + collision + navmesh + spawns as native assets | O | Red |
| 4.3 | Character controller walks the map end to end in the target engine | S | Yellow |
| 4.4 | Import-side acceptance: the imported map matches the certified artifact digests | S | Blue |

**4.1 is answered: Unity for the arena and the sellable toolkit, UE5 reserved for
Sidhe** (see [G8](MISSING_INVENTORY.md) for the reasoning and the measurements
behind it). 4.2-4.4 are unblocked.

The decision splits by product rather than resolving to one engine, and the
artifact layer is what makes that legitimate — WGE emits engine-neutral certified
artifacts, so `spawn transform` and `region volume` serialise once and each
adapter reads them. Do **not** let Unity-shaped assumptions leak back into the
compiler; the moment they do, the Sidhe path costs a rewrite rather than an
adapter.

---

## Running alongside: tooling items 4-7

Not a phase — these keep the loop honest while the phases run. Item 6 is the one
I would not skip.

| # | Task | Who | Tier |
|---|---|---|---|
| T4 | Conditions-of-use metrics for prop LOD, foliage cards, normal maps (template: `coarse_contrast`) | S | Yellow |
| T5 | ~~**Silhouette complexity metric**~~ **DONE 2026-08-01** — `pipeline/silhouette.py`, build stage 21 of 24. `horizon_coherence` is the sole honest target; peak count and relief are both gameable and labelled as such. Real map scores 0.471 | O | Red |
| T6 | ~~**Metric honesty guards**~~ **DONE 2026-08-01** — `pipeline/metric_honesty.py`. Measured: `detail_density`, `surface_variation_coverage`, `edge_density` are all coverage gates; only `coarse_contrast` is a quality score. See [D16](DEBT_LEDGER.md) | O | Orange |
| T7 | ~~Artifact identity sweep — two report schemas share one filename ([D10](DEBT_LEDGER.md)), *and* `zone_spec_sha256` names two different hashes ([D12](DEBT_LEDGER.md))~~ **DONE 2026-08-01** | S | Blue |

**T6 first among these.** Four "the pipeline works" claims turned out to be
wrong in a single session (2026-07-31). Until metrics have adversarial controls,
every gate result is provisional — including the ones this roadmap's acceptance
criteria depend on.

**T5 matters more than its position suggests:** until silhouette complexity is
measured, no automated loop can improve landform *shape*, which is what the
composition knobs exist for. Phase 1-2 do not depend on it; any future
auto-tuning does.

---

## Suggested first slice after restart

Parallel, no shared files, no blocking:

- **Sonnet:** 0.2, 0.3, 0.4 (all mechanical, all testable, all unblock authoring)
- **Opus:** 1.1 then 1.2 (collision + bounds — the architecture-shaped half)
- **Matt:** 4.1 (backend decision) whenever convenient; it unblocks Phase 4 and
  informs Phase 2 serialisation

Deliberately not started before the decisions land: 1.3 (navmesh
representation should know its consumer), and anything in Phase 4.
