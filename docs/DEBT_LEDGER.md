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

## D25. The black-pixel threshold was set for a world with no mountains in it

**Evidence.** `bevy_visual_acceptance` fails a world whose
`dark_foreground_fraction` exceeds 0.08. Measured on the alpine arena as the
border gained real relief:

| Border | `dark_foreground_fraction` |
|---|---|
| none | (world open; not comparable) |
| 26 m smooth rampart | 0.0751 |
| 62 m ridged Alpine massif | 0.0940 |

The world did not get worse. It got mountains, and mountains cast shadows —
which is the whole reason the art direction asks for them.

**Why it matters.** The threshold is doing real work: it catches clipped
framing and genuinely unreadable renders, and it caught a 42 m wall that threw
the apron into shade at 13.1%. But it was calibrated against a world whose
tallest feature was a 51 m massif on one flank, and the declared direction
(2026-08-02) is Alpine relief on every flank. A gate tuned for the old world
now argues against the new one, and the cheapest way to satisfy it is to build
flatter mountains — which is the same standing-incentive defect as
[D16](#d16-the-critic-has-a-standing-incentive-to-degrade-the-renderer).

**Fix direction.** Do not tune the mountains down to pass it. Separate the two
things it currently conflates: *unreadable* (crushed blacks with no recoverable
detail, which is a render defect) from *shadowed* (dark but structured, which is
a mountain). Measuring local contrast inside the dark region distinguishes them
— a shadowed cliff still has gradient, a crushed region does not. Re-baseline
once, with the old and new values recorded side by side, against a world built
to the current art direction rather than the previous one.

### Resolved 2026-08-04

`bevy_visual_acceptance.py`. The gate now fails on `unreadable_fraction` — dark
**and** flat — and reports `dark_foreground_fraction` without gating it.

**The crushed threshold is derived, not tuned.** A region whose local luma range
is under two 8-bit code values has no recoverable detail *in the file*: the
difference between neighbouring pixels is at or below the quantisation step, so
no grade recovers structure that was never encoded. That is a property of an
8-bit PNG rather than a number chosen to make a world pass. Local *range* rather
than standard deviation, because std over a 3x3 window rewards how many
neighbours differ, and what matters is whether any difference survived at all.

Measured across every capture in the workspace:

### First firing on a real world, 2026-08-04 — and it localised correctly

The overview of the rebuilt arena fails: `unreadable_fraction` **0.0756**
against the 0.04 limit, `dark_structured_fraction` **0.240**. That is the broken
render signature, not the mountain signature — the taller massif (D26) throws
large shadows and the renderer is **clipping them to black rather than shading
them**.

This is the whole point of the split, demonstrated on a world nobody staged for
it. The old gate saw `dark_foreground_fraction` 0.0995 against 0.08 and would
have said *"too dark — build smaller mountains."* The new one says *"these
regions carry no recoverable detail"*, which points at lighting and exposure in
the viewer and leaves the terrain alone. Same failing capture, opposite
instruction.

| capture | dark | unreadable | structured |
|---|---|---|---|
| **arena overview, 2026-08-04 (fails)** | **0.0995** | **0.0756** | **0.240** |
| arena, `bevy_overview` (earlier, passed) | 0.0027 | 0.0007 | 0.744 |
| arena, `bevy_player` | 0.0114 | 0.0068 | 0.404 |
| arena, `bevy_border` | 0.0270 | 0.0190 | 0.297 |
| arena, runtime frame | 0.0156 | 0.0015 | 0.904 |
| caledonia (failed build) | 0.2260 | 0.1565 | 0.308 |

`UNREADABLE_LIMIT` is 0.04 — about twice the worst good capture (`bevy_border`,
0.0190) and a quarter of the bad one. That is the one re-baseline this entry
authorises, and both sides are recorded above.

**One correction found while red-teaming this.** The range was first measured on
*luma*, and the "one 8-bit code value" rationale does not survive that: two
pixels differing by a full step in blue alone are 0.00028 apart in luma, because
blue carries a 0.0722 weight, so genuinely encoded detail was being called
crushed. Quantisation is per channel, so the range is now taken per channel and
the widest kept. It moved every number in the direction the rationale predicts —
the worst good capture went 0.0208 → 0.0172 and its structured share 0.599 →
0.667, while the broken render barely moved (0.1577 → 0.1565).

`tests/test_bevy_visual_acceptance.py` pins both directions with two images
built from a shared base, identical in luma distribution and dark fraction and
differing only in whether the dark carries a gradient. A third test asserts that
a world more than 8% dark — over the old hard limit — passes when it is
structured, which is the specific regression this entry exists to prevent.

---

## D26. `wall_width_m` is one number deciding the valley profile everywhere

**Evidence.** After the massif inversion took effect (2026-08-03), the world
reads as a range from the east — several summits at different depths, a
snow-capped horn standing behind the nearer ridge — and as a **flat-topped
plateau** from the player-height view looking the other way. Same terrain, same
build, one capture apart:

| View | Reads as |
|---|---|
| `east-wall` | alpine range, peaks behind peaks |
| `player` | one sharp horn, then a mesa with a hard horizontal top line |

**Why it happens.** `_surrounding_massif` derives the carve from
`into = smoothstep((play_distance - wilderness_margin_m) / wall_width_m)`, and
`wall_width_m` is a single scalar (34 m) applied identically in every
direction. Where the play envelope runs close to the map edge the transition
consumes the available distance and the massif is still climbing at the
boundary, which reads as a ridge. Where the envelope is far from the edge,
`into` saturates at 1.0 well before the edge and everything beyond it is flat
massif at full relief — a plateau top, because nothing varies once saturated.

This is the same shape of defect as the play envelope itself: a single global
number standing in for a field that should vary per-cell. The envelope fix on
the same day removed thirteen unbounded landmark holes for the same reason.

**Why it matters.** It puts a ceiling on how alpine the world can read
regardless of `massif_relief_m`. Raising relief makes the plateau taller, not
more mountainous, so the obvious knob does not address it and may look like the
inversion failing when it is this instead.

**Fix direction.** Make the transition a field rather than a scalar — scale the
run by the distance actually available between envelope and map edge, so the
profile completes in the room it has instead of saturating early. Ridged relief
should continue to modulate beyond saturation rather than stopping, so a
saturated cell is still terrain rather than a tabletop. Verify from at least two
opposed views; a single capture cannot see this, which is why it survived the
build that introduced it.

### Resolved 2026-08-04 — and the stated cause above was wrong

`_transition_run` in `zone_rasterizer.py`; `wall_width_m` is now a **ceiling on
the run**, not the run. Tests in `tests/test_massif_transition.py`.

**The mechanism was the opposite of what this entry assumed.** `into` was not
saturating early and flattening everything beyond it. It was barely saturating
at all — measured on the arena, only **20.2% of the non-play area** reached
saturation, because **56.1% of the perimeter had less room than the 34 m run
needed** (p10 = 9.4 m of room against p90 = 57.3 m).

That inverts the consequence. Delivered relief is `massif x into x relief`, so
where `into` never reaches 1 the ridged field is scaled *down* by it. Inside the
saturated band the massif field averaged **0.080** and peaked at 0.453 against a
world maximum of 1.000 — **every summit of the range sat outside the band and
was carved away**. So raising `massif_relief_m` scaled up the low, smooth part
of the noise, which is exactly the reported symptom. The entry's "flat massif at
full relief" reading was a plausible story that the measurement did not support;
it survived because nobody had printed the saturation fraction.

Measured on the alpine arena, before and after:

| | before | after |
|---|---|---|
| tallest point | 58.2 m | **94.9 m** |
| delivered fraction of authored relief | 39% | 63% |
| mountain area | 7,109 m² | **10,073 m²** |
| protected cells that are visually flat | 39.9% | **24.8%** |
| protected roughness (5-cell std) | 0.135 m | **0.420 m** |

Enclosure, accessibility and byte-determinism all still hold. Half the residual
flatness turned out to belong to a different defect and is logged as
[D31](#d31-the-containment-floor-was-the-terrain-and-its-own-low-ground-is-flat).

**Still owed: the two opposed views.** The numbers above are heightfield
measurements, not a visual confirmation, and this entry itself is the reason to
distrust a good-looking number — the previous diagnosis was numerically
plausible and wrong.

---

## D27. The accessibility gate's threshold is inherited, not derived

**Evidence.** The gate moved from `steep_edge_fraction` (steep walkable ÷
walkable) to `steep_edge_world_fraction` (steep walkable ÷ every edge in the
world) on 2026-08-03, because the walkable set is a design variable: raising
protected relief from ~12% to 50.19% halved the denominator without moving a
single steep edge, and the old rule then demanded the ridge flanks be smoothed
to correct an accounting change. Same class as [D16](#) and D25.

| | Value |
|---|---|
| steep edges | 12,490 |
| accessible edges | 1,076,789 (51.3% of world) |
| steep ÷ accessible | 1.1599% — failed the old 1% limit |
| steep ÷ world | 0.5950% — passes the new 0.9% limit |

**What is debt.** The denominator is now principled. **The threshold is not.**
0.009 was chosen to preserve the absolute allowance the previous rule granted —
1% of a world that was then ~87% walkable — so it encodes what the old gate
happened to permit rather than a measured statement about how much
walkable-but-steep ground a player should encounter. It is a defensible
starting point and an undefended number.

**Why it matters.** An inherited threshold looks derived once its provenance
scrolls out of git blame. The next person to hit this gate has no way to tell
whether 0.009 means something or was a translation artifact.

**Fix direction.** Derive it from traversal evidence — what fraction of steep
walkable ground actually costs a player a route, measured against the traversal
probe — or declare it an authored policy value with an owner, per the language
spec's requirement that every policy field carry a semantic owner and affected
gates. Record the old and new values side by side when it changes.

---

## D28. The visual gates now argue against the ecology

**Where:** `bevy_visual_acceptance`, `foliage_fraction` floor 0.008.
**Measured 2026-08-03:** 0.006820, having been 0.011509 the same day.

Nothing regressed. The scatter crossing (open decisions item 7) made the
foliage obey `canopy_suitability`, canopy suitability is non-zero on 2.7%-5.2%
of each forest polygon, and the forests thinned accordingly: 50 conifers where
112 stood, 275 render-plan instances where there were 337. The gate measures
green pixels, so obeying the ecology tripped it.

**This is the third instance of one defect**, after [D16](#) and
[D25](#): a metric whose cheapest satisfaction is to make the world *less*
correct. Planting trees the ecology forbids would turn this gate green.

**The two readings, and they are not equivalent:**

1. The ecology is miscalibrated — 2.7% canopy suitability on a valley floor is
   too strict, and the same question already sits open as decisions item 4
   ("heath scrub takes 69.5% of the map... I could not tell from one map
   whether this is honest or miscalibrated").
2. The gate is wrong — "enough green pixels" is not "the foliage is correct",
   and a genuinely sparse subalpine world should be allowed to look sparse.

**Do not re-baseline 0.008 to make this green.** That answers neither question
and destroys the evidence that they were ever asked. Item 4's second, gentler
world discriminates between the two readings in a single build, which is now a
stronger argument for doing it than the one originally recorded.

### The second world's evidence — 2026-08-04

`glenmara_highland_vale_v1`: the same gameplay graph on highland terrain,
`massif_relief_m` 78 against 150, three of six crag fields removed.

| | alpine arena | highland vale |
|---|---|---|
| heath scrub, share of vegetated area | 85.9% | **92.0%** |
| canopy area | 6,803 m² | 4,265 m² |
| measured treeline | 17.6 m | 47.9 m |
| bare fraction | 18.4% | 19.2% |

**The terrain is not what makes these worlds heath.** Halving the relief, moving
from alpine horns to highland whalebacks and removing half the crag fields made
the world *more* heath-dominated, not less, and raised the treeline nearly
threefold while producing less canopy. The reading that the arena's cliffs were
suppressing the forests is not supported.

**So reading 1 above is narrowed, not confirmed.** What the two worlds show is
that heath dominance is a property of the ecology rather than of any one
terrain. Whether ~90% heath is *wrong* is a different question, and it is an
art-direction call rather than a measurement: an alpine valley floor genuinely
is mostly grass and heath with conifer in bands, which is the reference Matt
gave. If that is the intended world, then the defect is reading 2 — a gate
counting green pixels against a floor inherited from a differently-vegetated
world.

### And then the capture settled it — `foliage_fraction` is frame-relative

Measured on the alpine arena the same night, on two builds of the *same world*
whose only difference is this session's fixes:

| | 2026-08-03 | 2026-08-04 |
|---|---|---|
| render-plan instances | 275 | **301** |
| `foliage_fraction` | 0.006820 | **0.005207** |

**More foliage was placed and the metric went down.** D33 added 26 instances,
and D26 made the surrounding massif taller, which changed what share of the
frame is rock and shadow. The metric moved because the *composition* moved.

That is the same defect already recorded for `road_fraction` in
[D24](#d24-road_fraction-is-frame-relative-and-the-framing-is-not-stable) —
"frame-relative, and the framing is not stable" — and nobody had noticed it
applies to `foliage_fraction` identically. Both are a green-or-warm pixel share
of a frame whose composition is a function of terrain height.

**So D28's answer is reading 2: the gate is wrong.** Not because a sparse world
should be allowed to look sparse — that argument was always available and was
never decisive — but because `foliage_fraction` **does not measure the quantity
of foliage**, and a build that plants 9% more trees can lower it. Whatever
threshold it is given, it is the wrong instrument for the question.

**Do not re-baseline it. Replace it.** The fix has the same shape as D18's:
project the known foliage instances into the frame and measure what fraction of
them are visible, rather than counting green pixels and hoping. The render plan
already has every instance's position, and the capture already records the
camera.

**What remains for Matt, and it is now a smaller question:** whether ~90% heath
is the world he wants. That is taste, and no measurement settles it. But it is
no longer entangled with the gate — the gate is independently broken.

### Resolved 2026-08-06 — the denominator is a count of trees now

`foliage_fraction` no longer gates. It is still computed and reported, the same
way `dark_foreground_fraction` was kept after D25, so the replacement can be
checked against it rather than taken on trust.

**Replacement:** `pipeline/foliage_projection_acceptance.py`, fed by a new
capture-time artifact `<capture>_foliage_projection.json` that the viewer writes
from the same settled frame the screenshot comes from.

**Why the viewer and not Python.** The camera auto-frames the world and was
recorded nowhere — no serialized position, rotation, or FOV existed to project
against. Rather than start serializing a camera, the projection happens in the
process that owns the real one, via `Camera::world_to_viewport`, and Python
consumes the result. That is the shape `overview_projection_acceptance.py`
already uses for Godot (`Camera3D.unproject_position` in-engine, thin gate
outside), so this follows established precedent rather than inventing a second
pattern.

**Why the spawned entity and not `render_plan.json`'s coordinates.** Replaying
the plan's positions would report a tree as present whether or not the viewer
managed to spawn it. The projection queries live `FoliageInstance` entities, so
an instance that never made it into the world is absent from the artifact too.

**What was actually fixed.** The denominator is the instance count. Adding a
tree can now only raise the numerator or leave it alone; no reframing can lower
it. `tests/test_foliage_projection_acceptance.py` pins exactly the transition
that exposed the defect — 275 → 301 instances — and asserts the fraction does
not fall, which is the regression the old metric could not have been given at
any threshold.

**Normalising by `foreground_fraction` would not have been enough**, and D24's
recorded fix direction should be read with this caveat. It removes the *sky's*
share of the composition effect and leaves the *rock's*: a taller massif puts
proportionally more rock among the world's own foreground pixels, so
green-share-of-world still falls with no tree removed. An area denominator is
the defect; changing which area does not fix it.

**What this deliberately does not claim.**

- **Frustum containment is not occlusion.** An instance standing behind a ridge
  is reported in frame. A pixel probe (`probe_hit_fraction`) distinguishes drawn
  from merely-framed by sampling the canopy column above each projected base —
  the base, because `position_m` has the grounding offset already subtracted, so
  the projected point is the foot of the trunk and sampling it reads ground.
- **The probe is reported, not gated.** It has never been measured on a real
  build, and setting a floor from a number nobody has observed is the mistake
  this entry and D25 both exist to record. It needs a real capture to calibrate.
- **Only two failures gate, and both are zero-tests rather than thresholds:**
  no foliage instances at all, and no instance in frame. Those are undeniable
  without calibration. Everything between them is reported.

**Still open:** the probe's threshold, pending a real build; and whether ~90%
heath is the intended world, which remains Matt's call and is unaffected by any
of this.

---

## D29. Moving the obstructing placements cannot clear the navmesh gate

**Where:** `siting_plan.obstructing_placements`, `navigation_plan.lanes[].obstructions`.

[D13](#) says twelve placements obstruct lanes and the villages have not moved.
Closing it was scheduled to clear navmesh acceptance. Measured 2026-08-03, it
will not, because the twelve are not what is blocking two of the three lanes:

| lane | obstructions | owner |
|---|---|---|
| `north_lane` | 14 | **12 `hibernia_keep`**, 2 `westcentral_hamlet` |
| `central_lane` | 10 | `southwest_hamlet` 5, `eastcentral_hamlet` 5 |
| `south_lane` | 17 | **14 `albion_keep`**, 3 hamlets |

Moving every hamlet clears `central_lane` and leaves the other two failing. The
dominant blockers are keep *gate* components — piers, flank roofs, flank towers
— standing in the lane the gate exists to admit. The siting plan does not list
them, correctly: a lane is *meant* to pass through a keep gate, so the keep is
exempt from lane keep-out. The defect is that the gate's collision then blocks
the aperture.

**That is a different problem from D13 and has never been logged.** It is about
whether a gate is a wall with a doorway or a solid object that happens to look
like a gate, and it needs a decision — carve a traversable aperture through gate
collision, model the gate as a doorway with a navigable span, or route the lane
around the keep and stop claiming it runs through.

D13 remains real and worth closing. It is just not sufficient, and scheduling it
as the fix for navmesh acceptance was based on a count nobody had broken down by
owner.

### Decided 2026-08-04 by Matt — a gate is a door, and it starts closed

**A keep gate is solid when closed and an opening when broken.** It is not a
traversable doorway from the start, and it is not permanently solid. So the
navmesh question was malformed: it asked for one static answer to something with
two states.

The design, in his words:

- **Gates begin closed and barred.** They are destructible; they cannot be
  repaired once broken.
- **Minions do not use the gate.** Forces spawn from **posterns** to either side
  and at the front, outside the wall, and make their way down the lanes. So a
  lane never needed to pass through a closed gate — which is why the lane
  routing that assumed it did has been failing.
- **Three mage towers** protect the keep.
- **When the gate falls, the Lord spawns in the courtyard**, visible. Killing
  the Lord ends the game. The Lord is formidable but cannot survive a
  coordinated team attack once the mage towers are gone, so it needs protection.
- Open question he flagged, not yet settled: whether the Lord gets a permanent
  lower-power tower that cannot be killed, or an ability instead.

**What this means for the compiler.** Lane connectivity must be solved against
**postern spawn points outside the wall**, not through the gate aperture. The
gate's collision staying solid is *correct* and was never the defect; the defect
is that `navigation_plan` routes lanes through a closed gate and then reports
its own routing choice as an obstruction. Destruction state is runtime, so the
compiled navmesh should describe the closed world, with the aperture as a
declared dynamic opening rather than a baked one.

**Not implemented** — terrain work is paused pending a rethink (see the
2026-08-04 handoff), and this wants doing after that lands rather than against a
world whose shape is about to change.

---

## D30. `source_tree_digest` over NTFS is slow enough to look like a hang

**Where:** `_viewer_source_digest` / `source_tree_digest`, `pipeline/capture_bevy.py`.

The digest walks `world_core/{apps,crates}` hashing every `.rs` — **9.9s warm** —
and `test_capture_bevy`'s provenance class calls it **once per test**. The
workspace is on `/mnt/d`, an NTFS data disk.

**How it presented.** On 2026-08-03, after a session of builds and Bevy captures,
`python3 -m unittest discover -s tests` appeared to hang in
`ViewerProvenanceMismatchTests` at ~1% CPU. Three experiments went to ruling out
code changes as the cause. Re-run on a cold, idle machine the same suite passes
**513/513 in 199s** — it was never blocked, only slow against a saturated disk.

**Why it matters.** It cost a session's confidence in a green suite, and it
scales the wrong way: every new test in that class adds ~10s of redundant
hashing of files that did not change between tests.

**Fix direction.** Cache the digest per process, keyed on the tree root. The
inputs cannot change mid-run, so there is nothing to invalidate within a test
session.

**Diagnostic note worth keeping:** a stall point that *moves* between runs — 109
dots, then 88, then 4 — means slow, not blocked. Re-run idle before hunting a
deadlock.

### Root cause found 2026-08-04: it is a mechanical disk behind FUSE

This entry blamed NTFS, which was half of it. `/mnt/d` is `/dev/sda2`, an
**HGST HUS724030ALA640 -- a 3 TB 7200 rpm mechanical hard drive** -- formatted
NTFS and mounted through `fuseblk`. Spinning platters, a foreign filesystem, and
a userspace driver, stacked.

Measured with the same binary and prefix, Gaea's `Swarm --help`: **33 s from
`/mnt/d` with a warm page cache, 1 s from NVMe.** `/home` is btrfs on NVMe and
has 61 GB free.

So `source_tree_digest` is not expensive because hashing is expensive. It is
expensive because it walks thousands of small files on a hard disk through a
userspace filesystem. Caching it per process is still worth doing, but the
larger and cheaper fix is to stop putting hot, many-small-file work on that
mount -- starting with `CARGO_TARGET_DIR`, which is entirely disposable.


---

## D31. The containment floor was the terrain, and its own low ground is flat

**Where:** `_border_rampart` run as the containment floor, `zone_rasterizer.py`.

**Found while fixing [D26](#d26-wall_width_m-is-one-number-deciding-the-valley-profile-everywhere), and it is a bigger finding than D26 was.**
Since the massif inversion the rampart is applied as `maximum(massif, rampart)`
and the code calls it "a containment floor". It was not behaving as one.
Measured on the alpine arena, 2026-08-04:

| | before | after `containment_only` |
|---|---|---|
| protected massif cells replaced by the floor | **87.7%** | 69.2% |
| mean relief lost where it replaced | 18.6 m | 15.0 m |

**Nearly nine cells in ten of what the world presented as its new massif-carved
mountains were the old rampart.** The cause was that the floor kept the
play-space and spur terms, so it followed the rhomboid inland instead of
guarding the map edge — a floor shaped like the border is the border. Dropping
those terms in containment mode is landed, and enclosure still holds
(`enclosed: true`, `leak_length_m: 0`, `edge_reach_fraction: 0.0`).

**What remains is the second half.** The floor is `height_m x shape x modulation`
with `crest_relief` 0.72, so where its own ridged noise is near zero the surface
sits at `62 x 0.28 = 17.36 m` — and a ridged multifractal is near zero over most
of its area by construction. Measured after the fix: **10.6% of the world sits
in a single 1.7 m height band around 17.4 m**, 92% of it within 28 m of the map
edge. That is the flat rim on the horizon, and it is the surviving half of the
"hard horizontal top line" D26 was raised for.

**Why it matters.** It caps how alpine the world can read from inside, and it is
invisible to every gate we have — enclosure passes, accessibility passes, and
the visual gates measure shadow and foliage, none of which a flat rim moves.

**Fix direction.** Two candidates, and they are not exclusive. (a) Derive the
floor's height from the containment requirement — agent `max_slope_degrees` 45
and `max_climb_m` 4.0 over the local face — instead of inheriting an authored
62 m chosen when the rampart *was* the border; this is the same defect class as
[D27](#d27-the-accessibility-gates-threshold-is-inherited-not-derived).
(b) Let the floor's low ground follow the massif field rather than an
independent noise field, so a saddle in the floor is a saddle in the range
rather than a plane crossing it.

**Not attempted here**, because both change enclosure geometry, and enclosure is
a hard safety gate that should not be altered in the same pass as the thing that
exposed it.

---

## D32. The render plan scattered against the previous build's ecology

**Where:** `build_zone.py` stage order — `_compile_render_plan` against
`build_vegetation`. **Fixed 2026-08-04, same day it was found.**

`_compile_render_plan` passes `terrain/canopy_suitability_u8.bin` to the Rust
scatter, which reads it to decide where a tree may stand (systems S6/S7,
open-decisions item 7). That file is written by `build_vegetation` — which ran
**98 lines later in the same function.**

**Every build therefore scattered against the previous build's ecology**, and
recorded that stale file's digest as `canopy_suitability_sha256`. The provenance
field asserted the plan was bound to a field the plan had never seen.

**Why it survived a session of testing.** On any batch that has been built once,
the file is simply there from last time, so the build succeeds and the numbers
look stable. It is invisible except on a batch with no `terrain/` directory,
where the build fails outright — which is exactly how it was found, on the first
run of the second world (open-decisions item 4). This is the case for building a
second world stated better than the argument that scheduled it.

**It also weakens a claim made on 2026-08-03.** "Rebuilds are byte-identical,
verified across two full builds on five artifacts" was true, but partly for the
wrong reason: the canopy field was constant across those builds because it was
stale, not because the pipeline is deterministic.

**Re-verified properly on 2026-08-04, with `terrain/` deleted between runs** —
which is the test the original claim needed and did not have, since a surviving
directory is exactly what made the stale input invisible. All five artifacts are
byte-identical, including `canopy_suitability_u8.bin` (the input that was stale)
and `render_plan.json` (the artifact that consumes it):

```
8f377502…83b95f2f  render_plan.json
52dbf6b5…d15e5b8cd1 terrain/heightfield_f32le.bin
ff3cc130…9d908369c  terrain/canopy_suitability_u8.bin
d5b7fc6f…78d9c8f6e9 terrain/splatmap.png
f1ea71a8…6d7937a5bf collision_plan.json
```

**Delete the batch's `terrain/` directory when verifying determinism.** A rebuild
over a populated one cannot distinguish a deterministic pipeline from a frozen
input.

**But do not do it while the test suite is running.** `test_boundary_plan` and
`test_navigation_plan` both have `CompiledBatchTests` that read the alpine
arena's artifacts straight off disk, so a concurrent rebuild takes the files out
from under them — four `FileNotFoundError`s on `terrain/playable_mask.bin`,
which look exactly like real defects until the traceback is read. **The suite
and a build of the arena cannot run at the same time**, and nothing in either
says so. That is the same shared-mutable-directory coupling as the rest of this
entry, seen from the test side.

**Fix.** The S1/S2/S6 block (site conditions, hydrology, vegetation) moved to
directly after the terrain manifest is written, which is where its own comment
already said it belonged — "runs as soon as the terrain is certified and before
anything that wants to read it". None of the three read `asset_plan`,
`placement_plan` or `render_plan`, so nothing else had to move. `build_siting`
audits placements and correctly stays after the render plan.

**The general lesson.** A pipeline whose stages communicate through files in a
shared directory cannot detect its own ordering errors, because a stale file is
indistinguishable from a fresh one. Stage inputs should be passed or hash-checked
against the run that produced them, not found on disk. Not attempted here.

### The provenance field did catch it — downstream, silently

The Bevy viewer refused to load the world built under the old order:

```
ERROR codeweald_world_viewer: compiled-world reload rejected:
      invalid ZoneSpec: canopy_suitability_sha256 does not match its source artifact
```

Which is exactly right, and vindicates binding the plan to the field's bytes.
The render plan hashed the old canopy field; vegetation then overwrote the file;
the digests disagreed. **So the artifact set every build produced was internally
inconsistent, and the one component that checked said so.** Nothing upstream
asked it.

The check belongs in the build, not only in the renderer. Until it is there, a
build can report every stage green and emit a world no viewer will load.
See [D34](#d34-the-viewer-hangs-instead-of-exiting-when-it-rejects-a-world).

**Verified fixed** by running the renderer's own check by hand against the
second world's artifacts — the digest `render_plan.json` records and the
`canopy_suitability_u8.bin` on disk are now the same bytes:

```
field on disk : bdbfd2dfba3f4ccf12c5fbe2e4f3c5d906702afd79327c1ad4052bf12f41cea9
plan records  : bdbfd2dfba3f4ccf12c5fbe2e4f3c5d906702afd79327c1ad4052bf12f41cea9
```

### A second, latent instance of the same class

Found while auditing the fix, not yet triggered. Under
`--cross-engine-handoffs`, `build_zone.py` adds `engine_artifacts.unreal` to
`terrain.manifest` and **rewrites `terrain_manifest.json`** — after
`boundary_plan` has already recorded `terrain_manifest_bytes_sha256` over the
earlier bytes. Any consumer checking that digest against the file on disk would
find them disagreeing, exactly as the viewer did for the canopy field.

It is dormant because the flag is off by default and Godot is the sole
production target, so it has never fired. **Left unfixed on purpose**: fixing it
properly means hashing manifests at the point of consumption rather than
re-ordering another pair of writes, which is the structural change described
above, and doing it piecemeal in a dormant path would create the appearance of
having addressed the class.

---

## D33. The scatter's dart budget was spent on ground no tree could stand on

**Where:** `suitable_cells` / `compile_foliage`, `world_core/crates/worldspec/src/render_plan.rs`.
**Found and fixed 2026-08-04**, on the first build of the second world.

Item 7 (2026-08-03) crossed S6 into the Rust scatter and recorded this design
note: *"the scatter samples the field rather than rejecting against it"*, because
uniform rejection sampling *"would have failed the `placed N of target` contract
intermittently — on terrain, not on code."*

**Half of that was true.** The scatter drew cells in proportion to canopy
suitability, but slope, exclusions and spacing were still applied as rejection
tests *after* the draw. Suitability knows nothing about lanes, keeps, streams or
protected landforms, so most darts landed on ground that was never admissible.
Share of each polygon's suitability weight actually reachable:

| forest | alpine arena | gentler world |
|---|---|---|
| `central_forest` | 3.3% | 1.4% |
| `western_valley_woodland` | 26.0% | 8.5% |
| `eastern_valley_woodland` | 4.3% | **0.0%** |

**So the shortfall counts reported on 2026-08-03 were not what they were said to
be.** "`central_forest` asks 28 conifers and its ecology holds nine" was
presented as the terrain answering. At 3.3% reachable weight and a 1400-dart
budget the expected number of admissible hits is ~46, so the nine was partly a
fact about the dart budget.

**Confirmed by rebuilding the arena after the fix**, with nothing else about the
world changed:

| forest | placed before | placed after | of |
|---|---|---|---|
| `western_valley_woodland` | 10 | **27** | 28 |
| `eastern_valley_woodland` | 3 | **13** | 28 |
| `central_forest` | 9 | 8 | 28 |
| **total render-plan instances** | 275 | **301** | |

`western_valley_woodland` had 26% of its weight reachable and now nearly fills
its quota; it was never the ecology that held it to ten. `central_forest` is
genuinely tight — 3.3% reachable, and it lands in the same place either way,
which is what a real ecological limit looks like. The old numbers conflated the
two cases and reported both as the terrain.

**And on the gentler world it fails the build outright.**
`eastern_valley_woodland` has 2,222 cells of non-zero suitability, 1,599 of them
walkable — and **zero** outside the lanes, streams and the filled
`eastern_alps` protected polygon. The scatter placed nothing and raised
*"its ecology permits ground somewhere in the polygon but spacing, slope or
exclusions rule out every cell"* as a hard contract failure. The message named
three causes without distinguishing them, and treated the honest answer as a
defect.

**Fix.** `suitable_cells` now applies slope and exclusions when it builds the
weighted set, so every dart lands on admissible ground and the placed count is
decided by the ecology and the spacing alone — which is what item 7 claimed. The
empty case is answered before sampling and split in two:

- **field zero across the polygon** — the author sited a forest where its own
  ecology forbids one. Still a hard error; it is a spec defect.
- **field non-zero, but every such cell occupied** by a lane, keep, stream or
  protected landform — the spec is fine and the world has no room. Recorded as a
  shortfall with `limited_by: "occupancy"` and printed. Failing here would demand
  the author measure an occupancy no one can see from the annotations.

Spacing stays a rejection test: it depends on what has already been accepted and
has no per-cell meaning.

**The general lesson.** "Sample the field instead of rejecting against it" only
holds if the field is the *whole* constraint. Where several systems each veto
ground, sampling one of them and rejecting against the rest is rejection
sampling with extra steps — and it reports the resulting scarcity as if it came
from the one system that was sampled.

---

## D34. The viewer hangs instead of exiting when it rejects a world

**Where:** `world_core/apps/world_viewer/src/main.rs`, `capture_certified_world`.

The capture waits for `GeneratedWorld` to exist and then for 120 settled frames.
When the compiled world is **rejected** — as it was on 2026-08-04 with
`canopy_suitability_sha256 does not match its source artifact` ([D32](#d32-the-render-plan-scattered-against-the-previous-builds-ecology)) — the
root is never spawned, `settled_frames` never increments, and the viewer runs its
render loop forever. Measured: **20 minutes at 55% of a core, 587 s of CPU, no
capture written and no exit.**

**Why it matters more than it looks.** It converts a clean, correctly-diagnosed
rejection into an indefinite hang. `capture_bevy.py` calls `subprocess.run`
with no `timeout`, so a viewer that never exits hangs the capture forever, and
`--suite` never reaches the remaining three views.

**Correction to the first version of this entry.** It said `capture_bevy.py`
pipes the viewer through a buffering `tail`, hiding the diagnosis. It does not —
it inherits stdout and stderr directly, and the viewer's `ERROR` line would have
been on the terminal immediately. **The `tail` was in the operator's own shell
command.** Twenty minutes went into inspecting `/proc` for a defect the program
had named in its first second, and the reason it was invisible was the
invocation, not the tool. Worth keeping because the lesson is the opposite of
the one first recorded: do not pipe a long-running subprocess through anything
that buffers.

This is also the second time this shape of thing has cost a session: the
[D30](#d30-source_tree_digest-over-ntfs-is-slow-enough-to-look-like-a-hang) test-suite
"hang" was likewise something slow or stuck presenting with no output.

**Fix direction.** Two independent halves, both cheap:

1. **The viewer should exit non-zero when it rejects a world** rather than
   entering the render loop with nothing to render. A capture run that cannot
   produce a capture has failed.
2. **`capture_bevy.py` should pass a `timeout` to `subprocess.run`** and fail
   loudly when it trips. A capture stage with no upper bound on its runtime
   cannot be run unattended, and `--suite` currently loses the remaining three
   views to a hang on the first.

**Not attempted here** — the build-order fix removes today's trigger, and this
wants to be verified against a deliberately rejected world rather than bundled
into the change that stopped producing one.

---

## Deliberately not listed

**`spine_count` moving no rendered metric** is a *finding*, not debt — see the
sensitivity matrix. Whether it is a defect depends on whether silhouette
complexity is ever measured, which is
[tooling item 5](TOOLING_UPGRADES.md).

**Material fidelity gaps** (mid-distance rock reading as sandpaper, untextured
road and settlement pads, the map-border cliff) are open *work*, tracked from
the 2026-07-31 visual review, not structural debt.
