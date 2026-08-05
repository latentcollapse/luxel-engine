# WGE missing inventory

Written 2026-07-31. Things that **do not exist yet**, as distinct from things
that exist and are wrong ([DEBT_LEDGER.md](DEBT_LEDGER.md)) and things that are
built and ranked ([TOOLING_UPGRADES.md](TOOLING_UPGRADES.md)).

Scope: WGE **and the game gap**. Codeweald is a real-time three-lane MOBA; WGE
compiles the world it is played on. As of this audit the pipeline compiles a
certified, renderable world and stops there. Nothing below is broken — it is
unbuilt, and the distinction matters because unbuilt work needs a decision
before it needs a fix.

Each entry states what does exist, so the gap is measured rather than asserted.

---

## Part 1 — WGE (the compiler and its instrument)

### M1. Silhouette complexity has no metric

**Exists:** `detail_density`, `surface_variation_coverage`, `coarse_contrast`,
`foreground_edge_density` — all surface/texture measures.

**Missing:** anything measuring the terrain/sky boundary. This is the reason
`spine_count` and the jitters still have no honest optimisation target even now
that they provably reach geometry. The 2026-07-31 sensitivity matrix makes this
concrete: driving `spine_count` from ~3.5 to 8 moved every tracked metric by
under 1.5% **except** `water_fraction` (+79.7%) — so the strongest signal from
the "how many ridges" knob is *drainage*, not skyline. The knob has real
geometric authority and no metric that represents what it is for.

**Blocks:** any automated loop that tries to improve landform shape. Until it
exists, composition scalars can only be tuned by eye.

### M2. Conditions-of-use metrics beyond terrain material

**Exists:** `coarse_contrast` (tooling item 4's template) measures terrain
material the way it is actually sampled at overview distance.

**Missing:** the equivalent for prop LOD selection, foliage cards, and normal
maps — all currently judged at 1:1 in isolation.

### M3. Adversarial / honesty guards on metrics

**Exists:** documented gaming vectors in prose (tooling item 6).

**Missing:** the automated check. Known-bad today: `detail_density` is a cliff
at its threshold and pure film grain at sigma 0.065 scores better than honest
texture; `surface_variation_coverage` hits 1.000 from sigma 0.02 noise; an
un-mipmapped render scores *higher* on both than the correctly filtered one,
because moire is high-frequency variation. Nothing runs a noise-injected
control.

### M4. A second geometric representation

**Exists:** a heightfield — one height per point.

**Missing:** caves, overhangs, arches, and tunnels. The README describes "cave
tunnels threading through the peaks with neutral mobs inside"; a heightfield
cannot express any of them. This is the project's one genuinely
blocked-on-a-decision item: it needs a chosen representation (mesh volumes in
the render plan, SDF, voxel patch) before anything can be built.

### M5. Provenance beyond the viewer binary

**Exists:** tooling item 1 — the viewer self-reports a source digest and
`capture_bevy` refuses a mismatch.

**Missing:** the same discipline for the WGSL shader (loaded from disk at
runtime, so it needs a capture-time digest rather than a build-time one) and for
the asset/material generators.

### M6. Reachability beyond heightfield scalars

**Exists:** tooling item 2 — composition scalars declare and prove they move the
heightfield.

**Missing:** declarations for the categorical composition keys (`silhouette`,
`massing`, `surface`, `dressing`) and for the material/asset lanes, each of
which controls a different artifact and needs its own digest.

### M7. Authoring-time feasibility validation

**Missing:** the DSL accepts values the pipeline then rejects (see
[D1](DEBT_LEDGER.md)). There is no way for an author to know a legal value is
unbuildable except by building it.

---

## Part 2 — The game gap

The pipeline currently emits: terrain (heightfield, splat, normal, water and
wetland masks), a render plan of placed instances, compiled road corridors,
terrain materials, and per-engine handoff manifests. Gameplay semantics exist
in the **spec** but stop there.

**What already survives into `zone_spec.json`** — more than expected, and worth
not rebuilding:
- Portal keeps tagged by team (`albion_keep` → `team_a`, `hibernia_keep` →
  `team_b`)
- Three lanes tagged `top` / `mid` / `bottom` as `corridor` features
- Bridges tagged with the `lane_id` they serve
- 20 `structure`, 13 `landmark`, 8 `hydrology`, 4 `biome` features

So the world *knows* it is a three-lane MOBA. Nothing downstream consumes that.

### G1. Collision geometry

**Evidence:** `collision` appears only in `intake_manifest.json` and
`vision_annotation_packet.json` — the intake brief. No emitted artifact carries
collision data. The Bevy viewer renders terrain with no collider.

**Needed:** terrain collision export, per-structure colliders, and the
playable-vs-render boundary (the map-border cliff from the visual review is the
same gap wearing an art-shaped name).

### G2. Navigation mesh

**Evidence:** `navmesh` appears in zero files. `navigation` appears only in
intake. `traversal_probe.py` exists and gates *whether* terrain is walkable
(slope percentiles) but emits no navigable surface.

**Needed:** a navmesh or flow-field for minion pathing down lanes. This is the
single largest missing piece between "a world" and "a MOBA map", and
`traversal_probe` already computes much of the input.

### G3. Spawn points, objectives, and gameplay volumes

**Evidence:** `spawn_point` appears in zero files. Keeps carry a team tag but no
spawn transform. No tower/objective placements — the README's Laser Towers and
Archwizards do not exist as data.

**Needed:** spawn transforms, lane waypoints, tower sites, jungle camp volumes,
and the inter-lane/jungle regions Matt described. Some of this is derivable from
existing corridor centerlines.

### G4. A gameplay ability system

**Exists:** nothing. Matt's framing: "our version of GAS".

**Needed:** the whole thing — attributes, effects, cooldowns, costs,
application/removal, replication boundaries. This is the largest single unbuilt
system and the one most worth borrowing rather than inventing (per the
workspace rule on mature libraries for undifferentiated work).

### G5. Physics

**Exists:** nothing. Bevy is running with no physics plugin.

**Needed:** at minimum character movement against terrain collision; projectile
motion if abilities are physical.

### G6. Units, entities, and combat

**Exists:** nothing in the current tree. The prior tick-based hex-combat sim was
archived 2026-07-31 as superseded (`_archive/codeweald-pre-wge/`) — a real-time
MOBA does not reuse it, though its combat-resolution logic is readable
reference.

**Needed:** entity model, champions, minions, towers, stats, targeting,
damage — all of it, real-time rather than tick-resolved.

### G7. Netcode / authority model

**Exists:** nothing. No decision recorded on client-server vs deterministic
lockstep, which constrains everything above it.

**Needed:** the decision first. This is architecture, not implementation, and it
should be made before G4 and G6 harden.

### G8. A chosen target backend

**Exists:** adapter directories for `unity` and `unreal`; a Godot adapter was
removed with Godot on 2026-07-31. Bevy is the reference renderer and
[explicitly not a backend](TOOLING_UPGRADES.md).

**DECIDED 2026-08-05: Unity for the arena and the toolkit; UE5 reserved for
Sidhe.**

The decision splits rather than resolving to one engine, which the artifact
layer exists to permit: WGE emits engine-neutral certified artifacts, so the
compiler and the asset pipeline do not care what renders them.

**Why Unity here.** The MOBA arena is an *LLM arena* -- a test ground where
agents are given the rules and compete -- not a netcoded product. That removes
the only strong argument for Unreal, since GAS and replication were the case for
it and neither is needed.

What remains favours Unity decisively:

- **AI operability, measured rather than assumed.** `unrealMCP` is configured in
  this workspace and exposes **zero tools**. `UnityMCP` exposes scene, GameObject,
  prefab, material, shader, texture, physics, animation, UI, VFX, build and test
  control, plus `execute_code` for arbitrary C# inside the editor. `godotMCP` is
  modest. On the axis this project is *about*, it is not close.
- **Commercial.** The Asset Store is the strongest marketplace for tools and
  editor extensions, which is the high-margin piece. Since Fab went cross-engine,
  generated *content* is not locked to the runtime choice -- only the importer
  plugin is.
- **Adapter maturity.** `engine_adapters/unity` is the further along of the two.

**Why UE5 stays reserved.** GAS has no real equivalent in Unity or Godot, and its
replication integration is most of its value. Sidhe wants it. Nothing about
choosing Unity here forecloses that -- both adapters are scaffolded.

**Caveat that shapes the workflow.** UnityMCP requires the editor *running and
connected*; it is a live-session dependency, not headless CI. WGE's own pipeline
therefore stays headless, and Unity is the presentation and sale target rather
than the place work happens.

---

## The shortest path to something playable

Ordered by what unblocks the most:

1. ~~**G8** (pick a backend)~~ — **DONE 2026-08-05: Unity here, UE5 for Sidhe.**
2. **G7** (authority model) — same: a decision that constrains G4/G6.
3. **G1 + G2** (collision + navmesh) — the pipeline already has the inputs;
   this converts a rendered world into a traversable one and is squarely WGE's
   job rather than the game's.
4. **G3** (spawns, lanes, objectives) — largely derivable from data the spec
   already carries.
5. **G4/G5/G6** — the game proper, and the point at which "WGE and the game are
   built side by side" starts being literally true.

Items 3 and 4 are WGE work and can proceed now, independent of the decisions in
1 and 2. That is the useful observation from this audit: **the compiler can be
taken meaningfully further toward a playable map before any game-side
architecture is chosen.**
