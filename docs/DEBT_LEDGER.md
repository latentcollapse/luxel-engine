# WGE debt ledger

Written 2026-07-31, from an end-to-end audit of the compiler, its instrument,
and the batch it produces. Every entry cites the evidence that found it, not a
suspicion. Entries are ordered by what they cost if left alone.

Companion documents: [TOOLING_UPGRADES.md](TOOLING_UPGRADES.md) (the ranked
tooling plan, items 1-3 landed) and [MISSING_INVENTORY.md](MISSING_INVENTORY.md)
(things that do not exist yet, as opposed to things that exist and are wrong).

**Verification state at audit time:** Python 205 tests green, Rust 17 green,
Julia 8 green, full 18-stage build green, Bevy overview capture passing.
Zero `TODO`/`FIXME`/`HACK`/`XXX` markers across `pipeline/`, `world_core/`, and
`terrain_lab/` — the debt here is structural, not annotated, which is exactly
why it needed an audit to find.

---

## D1. The DSL's declared bounds are not the buildable bounds

**Evidence.** `worldbuilder_dsl` documents `cross_jitter` as `0.0-0.5` and
`zone_compiler` validates against that same range. Setting it to `0.5` on every
landform and building fails outright:

```
Terrain analysis failed: codeweald_alpine_arena_v1 accessible p99=2.057276 max=10.213831
```

Found while measuring tooling item 3, which had to grow a backoff loop to work
around it; the matrix records `cross_jitter` as gate-limited to `0.4335`.

**Why it matters.** An author — human or model — who reads the documented range
and picks a legal value gets a hard build failure with a message about slope
percentiles, which names neither the knob they moved nor the range they should
have stayed inside. This is the single most likely way a competent author hits a
wall the pipeline could have prevented, and it is the first thing to fix before
handing the DSL to anyone new.

**Fix direction.** Either validate the feasible range at authoring time
(preferred: the compiler knows the traversability gate exists), or narrow the
documented bounds to the buildable ones and say why. Not "document the sharp
edge" — the point of the DSL is that legal input produces a legal world.

---

## D2. Silently-dropped repairs in `apply_to_scaffold`

**Status: fixed 2026-07-31, listed because the class of defect is the point.**

**Evidence.** The authoring surface names the spine count `spines`; the
ZoneSpec, terrain manifest and rasterizer all call it `spine_count`.
`apply_to_scaffold` regex-matched the patch key against generated lines and
discarded anything that matched nothing — no error, no warning. A
`spine_count` patch therefore did nothing at all, and the first sensitivity
matrix reported `spine_count` as moving all eight metrics by exactly
`0.000000` — a fabricated "this knob controls nothing" for a knob that
demonstrably moves the heightfield.

**Why it matters.** This is the repair mechanism, one level above the knobs
themselves. A dropped repair is indistinguishable from a repair that ran and
achieved nothing, so a critic loop could emit the same dead patch forever. It
now raises, naming the dropped patches and the naming mismatch.

**Residual debt.** The dual naming itself remains. `spines` vs `spine_count`
is a trap that has now caused one real defect; the guard catches it rather than
removing it.

---

## D3. Untested Blender asset generators, one of which shipped a real bug

**Evidence.** Six substantial pipeline modules are imported by no test. Four
are Blender kit generators, including `generate_highland_settlement_kit.py`
(321 lines) — which contained a rotation-pivot bug that put every rotated
house's roof beside its walls instead of on top. The bug was visible in every
capture for an unknown number of builds and was found by a human looking at a
screenshot, not by the pipeline.

| Module | Lines | Imported by a test |
|---|---:|---|
| `generate_highland_settlement_kit.py` | 321 | no |
| `generate_alpine_cliffs.py` | 183 | no |
| `blender_asset_probe.py` | 182 | no |
| `generate_highland_keep_kit.py` | 181 | no |
| `generate_highland_bridge_kit.py` | 176 | no |
| `image_reconciliation.py` | 153 | no |

**Why it matters.** These generators emit the GLBs the whole art pipeline
consumes. They are hard to unit-test because they require Blender, which is
exactly why nothing tests them — and why a geometry defect survived to the
screenshot stage.

**Fix direction.** The generators build meshes from pure arithmetic; the
transform logic can be extracted from `bpy` and tested directly. A test that
asserts "a rotated house's roof centroid stays within its footprint" would have
caught the roof bug in milliseconds. `image_reconciliation.py` has no Blender
dependency and is simply untested.

---

## D4. `CODEWEALD_PIPELINE_ARCHITECTURE.md` documents a pipeline that no longer exists

**Evidence.** Mentions Godot 7 times and Bevy zero times. Describes
`moba_3d.tscn` as the active target and Godot 4.7 as a live backend with
"headless scene build + OpenGL capture + semantic gate". Godot was removed from
the project on 2026-07-31; Bevy is the reference renderer.

**Why it matters.** It is the only architecture document in the Codeweald tree,
so it is what a new reader (or a model given the repo) will believe. It is
confidently wrong rather than merely stale.

---

## D5. The reference renderer is thinly tested for its size

**Evidence.** `world_core/apps/world_viewer/src/main.rs` is 1,653 lines and
carries 2 tests (`decodes_and_meshes_a_bounded_heightfield`,
`rejects_truncated_heightfields`). The `worldspec` crate is better served at 15.

**Why it matters.** Per [three-layer architecture](TOOLING_UPGRADES.md), the
viewer is WGE's *instrument* — every visual acceptance number is measured
through it. Instrument defects are measurement defects, and this session's
tooling item 1 exists because the instrument silently went stale for a day.
Mesh construction, the terrain sampler, camera framing, and the new marker
raycast are all untested.

---

## D6. Camera framing is nondeterministic enough to fail its own gate

**Evidence.** Repeated overview captures of the same unchanged world produced
different framings; one run failed with `"Compiled world is clipped by the
overview frame"` while an adjacent run of the same world passed. Observed twice
during this session while doing unrelated work.

**Why it matters.** It makes capture-to-capture comparison unreliable, which is
the foundation the sensitivity matrix and every visual gate stand on. It also
produces false failures that train a reader to ignore the gate.

**Fix direction.** The overview camera should be a pure function of world
bounds and height range. Worth confirming whether the variance comes from the
camera or from asynchronous asset loading changing the settle frame.

---

## D7. `detect_no_ops` only watches one artifact

**Evidence.** `wge_critic.detect_no_ops` compares two `terrain_manifest.json`
files and reports composition scalars that changed while `heightfield_sha256`
did not. It sees only the heightfield, and only scalars the manifest records.
`elevation_bias` was authorable, reached the geometry, and was **absent from
the manifest** — so the passive detector was structurally blind to it. Fixed
2026-07-31 by recording it; the general shape remains.

**Why it matters.** Materials, splat maps, asset plans and render plans have no
equivalent detector. The active sweep ([tooling item 2](TOOLING_UPGRADES.md))
covers composition scalars only.

---

## D8. `godot_renderer/` is now a misleading directory name

**Evidence.** The directory holds the entire WGE Python pipeline, its tests,
and all concept batches. It contains no live Godot integration; Godot was
removed 2026-07-31. `project.godot` is still present and is load-bearing — the
Bevy viewer's `ViewerConfig::from_args` refuses to start unless it finds one, as
its marker for "this is the renderer root".

**Why it matters.** Every command in every doc is prefixed by a directory that
names a dependency the project dropped, and the viewer asserts on a file from
that dependency. Low urgency, high confusion cost, and the rename is mechanical
apart from the `project.godot` sentinel.

---

### D8 update 2026-08-02: the functional half is RESOLVED; the rename is not

The rename was only ever the visible half. The load-bearing half was that **WGE
could not compile a batch that lived anywhere but inside its own tree**, which
is what actually blocked pointing the engine at a scratch directory:

- `build_zone` refused outright: "Concept annotations must be inside project
  root". That guard exists for the *Godot* subprocess, which addresses batches
  as `res://` paths — a backend requirement applied to every build, including
  headless ones that never invoke Godot. Now enforced only when
  `--godot`/`--capture` is passed.
- `asset_visual_preflight` refused a plan file outside the root, while the
  Blender probe it calls already receives `project_root` separately and resolves
  assets from it. The guard constrained the wrong thing.
- `collision_plan` reconstructed the engine root as `batch_dir.parent.parent`,
  silently encoding "every batch lives exactly two levels under the engine". It
  now takes an explicit `asset_root`.
- The asset catalog was always written back into the engine tree, so a shared or
  read-only engine could not be used and two concurrent runs would race over one
  file. `--catalog-path` now redirects it.

**Verified:** the alpine arena batch copied to `/tmp` and compiled with
`--project-root` pointing at the engine completes all 24 stages, and **13 of 13
artifacts are byte-identical to the in-tree build** — heightfield, zone spec,
asset/render/collision/navigation/boundary/placement plans, and every acceptance
report.

**Still open:** the directory is still called `godot_renderer/` and the Bevy
viewer still uses `project.godot` as its renderer-root sentinel. That rename is
mechanical but touches ~50 references and a live tree under **no version
control**, so it wants a `git init` first rather than being done blind.

---

## D22. The world's identity hash depended on where the batch sat on disk

**Found while verifying D8's fix, and worse than the thing being fixed.**

**Evidence.** Compiling the same batch from `/tmp` instead of from
`concept_batches/` produced `zone_spec_canonical_sha256: 31130e71d470` against
the in-tree `f2037d5e77ea` — a different world identity for a byte-identical
world. The heightfield hashed the same; everything above it did not.

Two root causes, both provenance strings hashed into artifacts:

- `zone_spec.json` recorded the `world_intent.py` source as an engine-relative
  path, which becomes absolute for an out-of-tree batch. That string is inside
  the canonicalised spec, so it is inside the world's identity.
- `asset_visual_preflight_report.json` recorded `asset_plan` the same way, and
  its bytes are hashed into `render_plan.json`, cascading into the collision and
  navigation plans.

**Why it matters.** WGE's README states the valuable artifact is the world, "a
verified data structure with a stable identity hash", and that two engines
pointed at the same world must produce the same place. An identity that moves
when the directory moves is not an identity of the world — it is an identity of
the world *plus the machine it was built on*. It also silently breaks every
downstream `_sha256` provenance check for anyone who relocates a batch, and it
would have made two Gemini sandboxes disagree about identical output for no
reason a reader could see.

**Fixed 2026-08-02.** Batch artifacts now name batch-relative paths, never
engine-relative or absolute: `_provenance_path` tries `batch_dir` before
`project_root`, and the preflight report names its sibling plan by basename.
Verified by compiling the same batch in two locations and diffing all 13
artifacts.

**One-time consequence:** the alpine arena's world identity changed from
`f2037d5e77ea` to `ea6391312dbe`, because the recorded intent path changed from
`concept_batches/codeweald_alpine_arena_v1/world_intent.py` to
`world_intent.py`. The world is unchanged; its name for itself is now
location-independent. Any doc or screenshot citing the old hash predates this.

**The general rule this encodes:** an artifact inside a batch may only name
paths relative to that batch. Anything else makes the batch non-relocatable and
its hashes unstable, and neither failure is visible until someone moves it.

---

## D9. Stale derived artifacts persist in the batch directory — hang RESOLVED 2026-08-01 (roadmap 0.4)

**Evidence (original).** `concept_batches/caledonia_v1/` is a batch dated
2026-07-27 missing `placement_plan.json`, `render_plan.json`, and
`terrain/heightfield_f32le.bin`. Pointing the viewer at it was reported to not
error — hanging indefinitely with a blank window, because
`monitor_compiled_world` allegedly failed to stat the missing files every poll
and never spawned a world.

**Verification 2026-08-01.** `source_signature()` and the `monitor_compiled_world`
error path (`world_viewer/src/main.rs`) were already propagating a named
`fs::metadata` error per missing file (`.with_context(|| format!("cannot stat
{}", path.display()))`) and surfacing it as `RELOAD REJECTED: ...` in
`update_status_text` — this codepath predates 0.4 and was not, in fact,
silently hanging by the time it was checked. Added a regression test,
`source_signature_names_the_missing_file_instead_of_hanging`
(`world_viewer/src/main.rs`, `#[cfg(test)] mod tests`), that points a
`ViewerConfig` at the real `caledonia_v1` batch and asserts the error names
`placement_plan.json` by name, to lock the behavior in against a future
regression rather than leave it as an unverified claim.

**Residue, out of scope for 0.4.** `caledonia_v1/style_reference.json` still
contains a `Language Projects` path fragment from before the workspace rename.
Cosmetic — not the hang, not user-facing in the viewer — left for whoever next
touches that batch's provenance rather than folded into this fix.

**Why it matters.** A silent infinite hang would be the worst failure mode
available, and this is the batch named in `CODEWEALD_PIPELINE_ARCHITECTURE.md`
as "the current proof zone." Confirmed the viewer rejects an incomplete batch
by name rather than hanging, and pinned that with a test.

---

## D10. Two report schemas share one filename — RESOLVED 2026-08-01 (roadmap T7)

**Evidence (original).** `capture_bevy.py --suite` writes
`bevy_visual_acceptance_report.json` with metrics nested under
`overview_acceptance`; a single-view capture writes the same filename with
metrics at the top level. A consumer reading `report["metrics"]` gets `None`
from a suite report. This silently zeroed every baseline in the first
sensitivity matrix run and produced a matrix of uniform `+100%` entries.

**Fix.** `capture_bevy.py`'s `--suite` now defaults to
`bevy_visual_acceptance_suite_report.json`, a distinct filename from the
single-view overview's `bevy_visual_acceptance_report.json`, closing the
ambiguity at the writer rather than leaving every reader to disambiguate a
shared name. `sensitivity.py`'s `metrics_from_report` keeps its dual-shape
handling as harmless backward compatibility for any already-written files
under the old collision; `wge_critic.py:load_reports`, which read
`report.get("metrics", {})` with no defensive fallback at all, is now
protected structurally rather than by luck. Regression test:
`test_suite_report_does_not_collide_with_the_overview_report`
(`tests/test_capture_bevy.py`) — writes a sentinel to the overview report,
runs `--suite`, and asserts the sentinel is untouched and the suite writes
elsewhere with its own schema.

**Why it matters.** This is [tooling item 7](TOOLING_UPGRADES.md) (artifact
identity) in a second location: same name, two contents.

---

## D11. The world does not contain its players

**Evidence.** `boundary_plan.json` (roadmap 1.2, added 2026-07-31) flood-fills
`codeweald_alpine_arena_v1` with the zone spec's own declared agent (radius
2.5 m, 45°, 4 m climb) from a keep, against a world padded with a flat
continuation of its own edge. The agent stands on **756 m of the 1024 m
perimeter** — 74.2%. The north and south edges are open along their entire
256 m; the east edge is mostly closed.

The heightfield's own edge samples run −2.9 m to +1.9 m against a world maximum
of 51.0 m, so the map border is not a mountain wall. It is flat walkable ground
that stops.

**Why it matters.** This is the [visual review's map-border cliff](TOOLING_UPGRADES.md)
measured rather than observed, and it is the largest single gap between
"renders" and "playable". Until it closes, no character controller can be given
this map without falling out of it.

**Deliberately not fixed by this artifact.** A synthetic barrier over the leak
would make the map playable today and make the defect invisible forever.
Roadmap 2.5 closes it with landform. `boundary_plan.json` names the spans so
2.5 knows where to build, and `--require-enclosed` turns it into a hard gate the
moment 2.5 lands.

---

## D12. `zone_spec_sha256` is one key name with two meanings — RESOLVED 2026-08-01 (roadmap T7)

**Evidence (original).** `zone_rasterizer.py:1688` writes `zone_spec_sha256` into
`terrain_manifest.json` as the sha256 of the *canonicalised JSON*
(`sort_keys=True`, compact separators). `build_zone.py:522` writes
`zone_spec_sha256` into `build_report.json` as the sha256 of the *file bytes*.
The two values never match.

Found by writing a provenance check that compared them: it refused a perfectly
healthy batch. `collision_plan.py:240` propagated the manifest's value onward
under the bare name.

**Fix.** `boundary_plan.json` and `navigation_plan.json` already emitted both
under names that say what they hash (`zone_spec_bytes_sha256`,
`zone_spec_canonical_sha256`); `collision_plan.py` now does too, and gained a
provenance check it did not have before (`test_terrain_built_from_another_zone_spec_is_refused`)
comparing the manifest's `zone_spec_sha256` against the *canonical* digest on
its own terms rather than a bare mismatch. The sweep also found the same
ambiguity one level down: `terrain_manifest_sha256` and `asset_plan_sha256`
are each a canonical-JSON hash in `world_core/render_plan.rs`'s own
self-contained fingerprint contract, but a raw file-bytes hash in
`collision_plan.py`/`boundary_plan.py`/`navigation_plan.py`. No consumer
compared the two contexts against each other (verified), so this was latent
rather than a live bug like the original — renamed the Python-side fields to
`terrain_manifest_bytes_sha256`/`asset_plan_bytes_sha256` and left a comment
pointing at `render_plan.rs`'s unrelated canonical use of the same bare names,
rather than touch the Rust fingerprint contract (a separate, load-bearing
system) for a collision nothing was actually hitting.

**Why it matters.** Hash-binding is the discipline the whole compiler rests on,
and this is a place where a consumer doing exactly the right thing gets a false
mismatch — or, worse, gives up on the check. It is [D10](#d10-two-report-schemas-share-one-filename)
in a second location, which is what upgraded [tooling item 7](TOOLING_UPGRADES.md)
from a tidy-up to a sweep.

---

## D13. Settlements are placed on top of the lanes

**Evidence.** `navigation_plan.json` (roadmap 1.3) attributes every
non-navigable sample on every lane centreline. Of 155 such samples across the
three lanes, **155 are colliders and 0 are terrain**. After the bridge fix (see
[D15](#d15-bridges-were-solid-boxes-across-the-lanes-they-carry)), every
remaining obstruction is a settlement cluster overlapping a lane centreline —
`southwest_hamlet` by 9.9 m, `westcentral_hamlet` by 9.3 m,
`eastcentral_hamlet` by 7.7 m — or a keep sitting at a lane endpoint.

Lanes declare `minimum_width_m: 10.0`. Nothing enforces it against placement.

**Why it matters.** No lane runs end to end, so no minion wave can traverse one.
This is the placement solver, not navigation: settlement clusters have a
`scatter_exclusion_radius_m` but no lane exclusion. A keep at a lane *endpoint*
is a separate question — the authored centreline runs into the keep's solid
volume, so 2.2 must terminate lane waypoints outside it.

`northwest_watchpost` is also not connected to the keeps by any route.

**Owns flipping a switch.** Fixing this is what turns roadmap 1.4's
`navmesh_acceptance.py --require-passed` from an opt-in check into
`build_zone.py`'s real exit-path gate (currently soft: it reports and warns,
never fails the build). Wire that in as part of this fix, not as a separate
follow-up nobody remembers to do.

---

## D14. Two artifacts disagree about what "playable" means

**Evidence.** `boundary_plan.json` (1.2) measures the playable region by
flood-filling terrain slope. It never reads `collision_plan.json`, so buildings,
keeps, rocks and trees do not block it: **7.66% of the region it calls playable
is inside something solid**. `navigation_plan.json` (1.3) subtracts colliders
and reports a smaller, correct surface.

**Why it matters.** Two emitted artifacts now answer "where can a body be?" with
different numbers, and the more optimistic one is the one whose name sounds
authoritative. Either `boundary_plan` should consume the collision plan, or its
`playable` block should be renamed to say it is terrain-only. Leaving both is
how a consumer picks the wrong one.

The containment measurement in 1.2 is unaffected — colliders only ever *remove*
ground, so a world that leaks on terrain alone leaks at least as much with them.

---

## D15. Bridges were solid boxes across the lanes they carry

**Fixed 2026-07-31**, recorded because the shape of the mistake is worth keeping.

**Evidence.** `collision_plan`'s `ROLE_COLLISION` gave
`lane_crossing_structure` a `box`. A bridge is placed *on* the lane centreline
by design, so all 20 crossings obstructed their own lane. Every capture still
looked correct — the bridges render exactly where they should.

**The fix.** A `deck` policy: geometry with extent, `obstructs: false`, so
navigation does not subtract it. Lane navigability went 75–80% → 85–92%.

**Why it matters as a record.** This was invisible until something *consumed*
the collision plan. It is the project's characteristic failure — a component
built correctly and left one wire short — and it argues that every emitted
artifact needs a consumer before it can be trusted, not just tests.

---

## D16. The critic has a standing incentive to degrade the renderer

**Evidence.** `metric_honesty.py` (tooling item 6, landed 2026-08-01) scores
each metric against probes carrying identical content that differ only in
filtering. `detail_density` scores the point-sampled version **8×** higher than
the correctly averaged one (0.624 vs 0.077); `edge_density` 2.5× higher (0.832
vs 0.328).

`wge_critic.diagnose` raises a fidelity finding when
`detail_density < 0.65 × source detail_density`, and the current render sits
about 3× short of that target.

**Why it matters.** The cheapest way to satisfy that finding is not better
materials — it is **removing mipmaps**. Aliasing is high-frequency luminance
variation, and this metric counts high-frequency luminance variation. Any
automated repair loop optimising against it would find that out. This is not
hypothetical: the item 6 incident record already contains an un-mipmapped render
out-scoring the correctly filtered one, which is what motivated the guard.

**Mitigated, not fixed.** `Finding.metric_kind` now labels it and `describe()`
prints "treat it as a floor, never as a fidelity target". The finding still
drives repairs.

**Fix direction.** Re-point the surface-detail fidelity target at
`coarse_contrast`, the only one of the four measured metrics that is a genuine
quality score — it drops 20× on shuffled pixels and is indifferent to filtering,
and it is already the metric designed for the 36–71 texels-per-pixel
minification regime this world actually renders at. That is a change to
acceptance behaviour and should be made deliberately, not folded into a tooling
pass.

---

## D17. Both faction keeps were sealed inside their own curtain walls — RESOLVED 2026-08-02

**Listed late, and that is the first half of the entry.** `navigation_plan.py`
cited "(D17)" in three separate comments from 2026-08-01. This ledger stopped at
D16. A defect that is named in code but never written down cannot be scheduled,
assigned, or closed — it can only accumulate commentary, which is exactly what
happened for a day while both keeps stayed unreachable.

**Evidence.** `navigation_plan.json` on the alpine arena reported
`keeps_share_one_component: false` with `albion_keep` in component 38 (24 cells)
and `hibernia_keep` in component 13, against a primary component of 38,623 m².
`generate_highland_keep_kit.py` emitted a closed ring of 20 curtain segments
*plus* a solid 54-unit `Gatehouse` cube straddling the gate azimuth *plus* a
`Gate_shadow` iron leaf drawn across the threshold. The entrance was a cube
named `Gatehouse`; the name was the entire claim.

It was two defects, not one. Opening the wall was not enough: `Great_hall`
reached 6.9 m from centre and the `Outer_bastion` corners plugged the ring at
8.3 m against a 12.4 m inner face, leaving under 5 m of courtyard for an agent
that needs 5 m to fit. The keeps stayed stranded, now in a courtyard they could
enter but not stand in.

**Why it mattered.** A MOBA keep exists so units spawn inside it and leave. Both
were solid objects with a decorative door. It also hid behind a measurement
artefact: before per-part collision (1.1), the whole keep was one box and the
courtyard did not exist to be disconnected from anything, so the defect became
*visible* only when collision got better.

**Fixed 2026-08-02.** Aperture carved by measurement rather than by an index
list, so the 24-segment variant opens by the same rule; gatehouse split into
flanking piers; gates modelled open; interior mass budgeted against the
courtyard. Threshold 8.35 m and courtyard ring 7.27 m against a 5 m
requirement. Both keeps now resolve into component 1, and the nav window around
each contains exactly one component where it previously held five.

**The gateway is deliberately open to the sky.** `rasterize_colliders`
subtracts a part's whole ground footprint whenever its top exceeds the climb
limit and never reads the underside, so an arch blocks exactly as a wall does.
A walk-under rule would not rescue it either: this zone declares
`agent_height_m: 8.0` against a 4.96 m curtain wall, so no arch this kit could
build would clear that body. **`agent_height_m` is unadjudicated and worth a
decision** — nothing currently reads it, and it is nonsense at this scale.

**What stops the recurrence** (the structural half, also 2026-08-02):

- `highland_keep_geometry.py` — layout and both measurements extracted from
  `bpy`, per D3's fix direction. 20 unit tests in 0.027 s replacing a 70-second
  Blender round trip, each paired with the negative case it catches.
- `asset_affordance.py` — assets now *declare* what they afford
  (`threshold_clear_m`, `interior_ring_m`, `local_bearing_degrees`,
  `threshold_band_m`) in a sidecar beside the GLB, and the claim is re-measured
  from the shipped mesh against the placing zone's own agent.
  `ROLE_REQUIRED_AFFORDANCES` makes the declaration mandatory for
  `faction_fortification`, so a fortification that cannot be entered is not
  merely detectable but unrepresentable.
- Wired into `asset_physical_acceptance`, which is already `build_zone`'s hard
  gate — so this fails at asset selection rather than being discovered three
  stages later. Verified red against both a missing sidecar and the genuine
  pre-fix sealed GLB, not against synthetic fixtures.
- The kit generator takes the agent policy and placed scale as arguments
  instead of baking them, so "does this keep work" is asked per world.

---

## D18. `asset_parts` ignores node rotation, so rotated parts collide crooked

**Evidence.** `asset_parts._node_transform` reads translation and scale and
documents that rotation is deliberately not applied. The keep's 20 curtain
segments are rotated tangentially on their Blender nodes, so every one of them
collides as an axis-aligned 29×12 box regardless of its actual orientation. At
the gate azimuth the two happen to coincide; at 0° the wall runs along Z while
its collider runs along X.

**Why it matters.** Colliders sit where the mesh is not, for every rotated part
in every kit — invisible in any render, and wrong in exactly the way that is
hardest to attribute when a lane mysteriously fails to run end to end. The
module's own docstring anticipates this ("if a kit ever rotates its parts this
should grow a real transform"); a kit now does.

It did not affect the D17 fix — the gate was checked under both the rotated and
unrotated readings, and `highland_keep_geometry.Part.half_extent_x` carries both
deliberately — but that is a mitigation local to one kit, not a fix.

**Fix direction.** Apply node rotation in `asset_parts` and emit a yaw per part,
which `collision_plan.instance_collider` already knows how to consume (it
applies the *instance* yaw today). Blast radius is every collider in every
batch, so it wants its own pass and a before/after collider diff, not a fold-in.

---

## D19. A kit's candidate pool can contain an asset nobody regenerated

**Evidence.** The `highland_fortified_keep` profile declares `variant_count: 3`
over `source_prefixes: ["assets/generated/codeweald_highland_keep/"]` — a
directory glob. The generator writes `_a` and `_b`. A third, unsuffixed
`highland_fortified_keep.glb` from an earlier single-variant run (2026-07-27
06:49, predating the a/b split at 13:54) sat alongside them, selectable, and was
a *different build* — confirmed by hash, not by timestamp alone.

**Why it matters.** Regenerating a kit does not regenerate the pool. A fix
applied to every file the generator writes can still be bypassed by selection,
silently, because the profile counts files rather than naming them. Had
placement picked the unsuffixed variant, the D17 fix would have appeared to fail
for reasons invisible in the generator's own output.

**Mitigated 2026-08-02**, not fixed: the stale file was refreshed rather than
deleted, and the affordance sidecar now catches a stale *enterable* asset by
re-measuring it. Neither addresses the general case — an asset with no declared
affordance can still go stale in the pool undetected.

**Fix direction.** Have the generator write a manifest of what it produced and
have `asset_catalog` refuse a prefix directory containing files the manifest
does not name. `variant_count` should be a check against that manifest, not a
count of whatever is on disk.

---

## D20. Bridges are placed over channels the terrain never cut

**Evidence.** The alpine arena declares 8 hydrology features, every one of them
`channel_profile: wetland_rill`, `width_m: 2.0`. `zone_rasterizer._conform_channels`
documents that wetland rills "follow local relief with strictly bounded
lowering, so a marsh centerline cannot become a slot canyon" — that is, a rill
cuts essentially nothing. Crossing derivation does not know that: it fires on a
lane/stream *centreline contact*, so `lane_crossing_structure` instances are
emitted wherever a lane passes near a rill, regardless of whether there is a
channel there to cross.

The result is visible in every viewer screenshot from 2026-08-02: bridge decks
scattered along the lanes and across open flat ground, crossing nothing.

**Why it matters.** Bridges are the one asset whose entire justification is the
terrain underneath them. Placing them off a declared centreline rather than off
measured relief means the world asserts a feature it does not have — the same
class as the keep whose entrance was a cube named `Gatehouse`. It also makes the
map read as buggy at exactly the moment a reviewer is looking closely, and it
spends collision budget (`lane_crossing_structure` → `deck`) on geometry that
carries nothing.

**Design intent (Matt, 2026-08-02).** Rivers will be carved and shallow enough
to walk; the bridges over them are an aesthetic choice, not a traversal
requirement. So bridges are wanted — but only where a riverbed exists.

**Fix direction.** Gate crossing derivation on measured channel depth at the
contact point, read from the heightfield the rasteriser actually produced,
rather than on the centreline existing. A rill that lowered terrain by under
some threshold gets no bridge. That is testable in both directions and it stays
correct when the riverbeds *are* carved, which is the point: the same rule then
starts producing bridges without anyone re-authoring placement.

---

## D21. The close-LOD conifers have warts

**Evidence.** `generate_highland_foliage._needle_clump` builds each "needle
clump" as `primitive_ico_sphere_add(subdivisions=2)` scaled `(0.78, 0.78, 0.42)`,
four per alternate crown layer, LOD 0 only. Their placement reaches
`radius * 0.34..0.72` from the crown axis with a sphere radius of
`radius * 0.24..0.37`, so the clump's outer edge lands at up to ~1.09 × the
crown radius — outside the cone it is meant to break up.

A subdivision-2 ico-sphere is a visibly faceted ball. Sitting proud of a smooth
cone, it reads as a growth on the tree rather than as foliage. Confirmed by
visual review 2026-08-02 ("the trees look weird and have warts upon closer
inspection").

**Why it matters.** Small, but it is a *quality* defect in the asset family with
the highest instance count in the world (160+ placements), and it only appears
at close range — which is where the map will be judged once there is a camera at
player height. The intent in the code is sound ("a tree family rather than a
single procedural traffic cone"); the primitive chosen for it is not.

**Fix direction.** Inset the clumps so `reach + clump_radius <= crown_radius` at
that layer's height, and use a form with needle-like silhouette rather than a
sphere. Worth doing alongside any wider foliage pass rather than on its own —
and worth checking against the same close-range capture that found it, since
this is precisely the defect class that survives an overview gate.

---

## D23. WGE's tests read Codeweald's content

**Evidence.** Ten test modules assert against a real compiled batch (the alpine
arena, caledonia) and real generated kits. When the engine moved out of the game
(D8) they broke: 60 errors, 47 skips, because `parents[1]` had silently been
both "the engine" and "the art".

`tests/reference_content.py` names the coupling in one place and makes it
redirectable via `WGE_REFERENCE_CONTENT`, which restored the suite. That is a
signpost, not a fix.

**Why it matters.** Asserting against a real world is *good* testing -- it is
what caught the sealed keeps and the bridges over nothing, and synthetic
fixtures would not have. But WGE is supposed to compile any game's world, and a
test suite that cannot run without one specific game's concept batches is a
suite that cannot certify the engine on its own. It also blocks the Gemini
sandbox plan: a throwaway WGE checkout has no Codeweald beside it.

**Fix direction.** WGE needs a small reference batch of its own -- a deliberately
minimal world with one keep, one lane, one water feature -- committed as engine
fixture data. Not a copy of Codeweald's: something small enough to version and
boring enough that nobody is tempted to make it pretty. The Codeweald-facing
tests then become integration tests that skip loudly when the content is absent.

---

## D24. `road_fraction` is frame-relative, and the framing is not stable

**Evidence.** Measured across four border configurations while building S4:

| Config | `road_fraction` | `foreground_fraction` | ratio |
|---|---|---|---|
| 42 m flat crest | 0.01459 | 0.633 | 0.023 |
| 26 m ragged crest | 0.00761 | 0.340 | 0.022 |
| 38 m ragged crest | 0.00761 | 0.316 | 0.024 |

The roads never changed. The *world* got smaller in frame, so the absolute road
pixel count halved and the gate went red with "Compiled roads are not visually
readable" -- about roads that are exactly as readable as they were.

**Why it matters.** `bevy_visual_acceptance` measures several metrics as a
fraction of the whole frame while the camera auto-frames the world, so any
terrain change that moves the camera moves every absolute metric with it. That
makes a red gate unattributable: it is not distinguishable from a real
regression without manually recomputing the ratio, which is what had to be done
here. This compounds [D6](#d6-camera-framing-is-nondeterministic-enough-to-fail-its-own-gate).

**Fix direction.** Normalise the coverage metrics by `foreground_fraction` so
they measure "how much of the *world* is road" rather than "how much of the
*image* is road". `dark_foreground_fraction` is already named for the right
denominator; `road_fraction`, `foliage_fraction` and `water_fraction` are not.
Re-baseline the thresholds once, in one commit, with the old and new values
recorded side by side.

---

## Deliberately not listed

**`spine_count` moving no rendered metric** is a *finding*, not debt — see the
sensitivity matrix. Whether it is a defect depends on whether silhouette
complexity is ever measured, which is
[tooling item 5](TOOLING_UPGRADES.md).

**Material fidelity gaps** (mid-distance rock reading as sandpaper, untextured
road and settlement pads, the map-border cliff) are open *work*, tracked from
the 2026-07-31 visual review, not structural debt.
