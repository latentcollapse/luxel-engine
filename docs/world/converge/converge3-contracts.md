# Luxel CONVERGE-3 Contracts

Status: **CONVERGE-3 CLOSED, 2026-10-05.** W-1, R-1, L-2a and N-7 accepted. Closeout at the end. (Order W-1 → R-1 → L-2a → N-7
approved 2026-10-04; pool: still water, option a.)
Source: "Carried into CONVERGE-3" in `docs/world/converge/converge2-contracts.md`, and
`docs/world/graphics-convergence-audit.md` §8 (N-7, L-2) and §E.

Four contracts. CONVERGE-2 made the close frame's objects real; CONVERGE-3
finishes that job (the pool, the ruin's damage) and then fills the world
(distant foliage that holds up, then scatter and forest). Each contract names
the owner layer, the permitted files, the tests, the acceptance evidence and
the non-goals.

Identity discipline as before: every new axis is absent by default and
byte-identical when absent. All CONVERGE-3 content lands under a new arm,
`converge3` (= converge2 plus these contracts). converge2 and every earlier arm
stay byte-identical, and so do the calibration and foliage references.

Proposed order: **W-1 → R-1 → L-2a → N-7.** The two small content fixes come
first because they finish the close frame. L-2a comes before N-7 because a
forest is mostly foliage seen at 40 m and beyond, which is exactly where
alpha-tested leaves stipple today.

---

## 0. What the code says (checked 2026-10-04)

**Instances share meshes.** `InstancePacket` is (instance id, mesh id,
material id, importance, transform), `lib.rs:453`. A scattered instance costs
about 150 bytes of JSON, not a mesh copy. The kit's 9.7 k-triangle tree is
paid for once however many trees are placed.

**The renderer already instances.** `LavaAdapter.jl:4760` groups instances by
(mesh, material) and uploads per-instance vectors for translation, rotation,
scale, base colour, metallic / roughness / normal scale / occlusion, and
emissive. Today those colour and material vectors are the material's values
copied per instance. Per-instance tint is therefore a new optional packet field
and a different value in a vector that already exists, not new machinery.

**Baseline numbers** (`artifacts/parity/ab-converge2-n5`, converge1 in brackets):

| View | Authored-geometry pixels | GPU frame (offline capture) |
|---|---|---|
| close | 42.6% (23.5%) | 32.4 s (27.4 s) |
| medium | 25.7% (11.0%) | 14.3 s (9.4 s) |
| wide | **14.1%** (6.5%) | 14.2 s (9.9 s) |

The audit's N-7 gate is wide ≥ 30%. N-5 took the wide view from 6.5% to
14.1%; scatter has to roughly double it again. Frame cost is the offline
capture's wall time (2x supersample, IBL prefilter included) and has not been
profiled. Nothing here says what dominates it.

**Terrain.** The source world is 96 × 72 m on a 49² grid (2 m cells).
converge0 embeds it in a 5× grid: 480 × 360 m, 241², rolling hills over the
first 50 m outside the source rectangle and a ridge from 50 m to 140 m out
(`extend_terrain_with_backdrop`, `lib.rs:4308`).

Two placement defects follow, and N-7 has to fix both:

1. `campaign2_terrain_height` (`lib.rs:2295`) is a **nearest-cell** lookup.
   The rasterised terrain interpolates between vertices, so on a 20% slope a
   placed object can float or sink by up to ~0.2 m. The kit hides this with a
   constant +2 cm and an 18 cm rock sink.
2. The kit's `ground` closure (`lib.rs:5315`) samples the **source** packet.
   `nearest_cell` clamps, so any position outside the 96 × 72 m source
   rectangle gets the height of the nearest source edge. Scatter in the
   extended 480 × 360 m world would float over the hills and sink into the
   ridge.

**Wet pool.** A 48-segment disc (`campaign2-wet`: metallic 0.34, roughness
0.06, clearcoat 0.86, a procedural sine-ripple albedo) and two emissive rings
on `campaign2-hero-glow` (`lib.rs:4505`, `4795`, `5107`-`5134`). The rings are
the blue neon arcs in the close sheet. The metallic 0.34 contradicts MD-6
(water is a dielectric, metallic 0). IBL exists now (N-2), so a smooth
dielectric surface reflects the analytic sky with no further renderer work.

**Ruin.** `modular_fort_01` has 22 pieces (walls, corners, ends, gate, tower,
stairs, walkways) and **no broken pieces and no rubble**. The kit uses
`wall_thin_gate_01` and `wall_thick_end_01` (`tools/build_kit.py:58`). Damage
has to be cut, not selected.

**MSAA.** Lava hard-codes `SAMPLE_COUNT_1_BIT` with alpha-to-coverage off
(`vendor/Lava/src/graphics/pipeline.jl:101`). The adapter renders at 2x
(`render_width = 2 * width`, `LavaAdapter.jl:5530`) and box-resolves.
Alpha-to-coverage needs a second vendored patch: a sample count and an
alpha-to-coverage flag on the pipeline, plus multisampled colour and depth
targets with a resolve. This is the same route as the `discard` patch.

**Carried item 5 elsewhere.** `render_and_promote` and
`render_and_promote_with_terrain_layers` still read the arm from the
environment. `converge3` is added through `Campaign2Inputs`, so it adds no new
environment read.

---

## 1. W-1 — The pool: still water instead of neon rings

> **Status: ACCEPTED (2026-10-05), revised after the first review.**
> Arm `converge3` (`LUXEL_PARITY_RENDER_POLICY=converge3`, kit `tools/kit/kit2.lock.json`).
> First review: "the water looks like a large round tree shadow. No depth to it."
> v2 adds a planar reflection pass (the scene rendered from the camera mirrored
> about the water surface, sampled by the water by Fresnel), a shallow margin
> where the bed shows through, and a less regular shore. Sheets:
> `artifacts/parity/review-converge3-w1b/`. Identity: see closeout.
>
> **Human acceptance (2026-10-05, close sheet): met.** "That's *muuuuuch*
> better. And I can see that you gave water the ability to have depth."

> **Decided 2026-10-04: option (a), still water.** Options considered:
>
> a. **A still puddle / spring.** Natural water: dark, sky-reflecting, a wet
>    rim. Matches the scanned world.
> b. **A deliberate magic spring.** Keep a glow, but in the water (a faint
>    emissive tint under a reflective surface), not as rings floating on top.
> c. **Remove it.** The ruin and the ferns carry the close frame alone.

### Contract (option a)

- **Content, under `converge3` only:** the two `campaign2-wet-ripple-*`
  instances are gone. The disc becomes an irregular-outline water surface
  (seeded radial noise on the outline, so it does not read as a circle). Its
  material is a dielectric (metallic 0, roughness ≤ 0.05, dark albedo,
  F0 0.02) with no ripple albedo, so the IBL sky reflection carries it. A wet
  rim, 0.3-0.6 m wide, darkens the ground and lowers its roughness, using the
  CALIBRATION-1 wet recipe. The rim is a decal ring mesh conformed to the
  terrain, or a terrain wetness mask if the layer system takes one cheaply. The
  choice is made after measuring both, and recorded.
- **Grounding:** the water plane sits 2-4 cm below the rim's mean height, so
  the rim reads as the edge of a depression.
- **Permitted files:** `lib.rs` (campaign2 pool section, `converge3`
  content), `render_policy.rs` (arm), `tests/`.
- **Tests:** converge3 packet has no `campaign2-wet-ripple*` instance and no
  emissive material on the pool; the water material is a dielectric
  (metallic 0); every earlier arm byte-identical.
- **Acceptance (measured):** in the close view the pool's mean colour is
  within ΔE 10 of the sky colour reflected at its viewing angle (it reflects
  the sky, it does not glow); zero pool pixels above the scene's sky
  luminance.
- **Acceptance (human):** "is that water?" on the close sheet.
- **Non-goals:** animated ripples, refraction, depth fade, shoreline foam,
  SSR. Real water is audit L-5.

---

### Implementation (W-1)

- **First version, rejected on its own render:** carve a bowl into the terrain
  and fill it with a flat water mesh (`ab-converge3-w1`, first pass). On the 2 m
  terrain grid (the pool's minor radius is 2.7 m) the shoreline came out as 2 m
  straight segments and the ground near the shore z-fought with the water. A
  prototype in Palette also showed the converge2 water level (12.546 m) sat
  above the ruin's ground (12.396 m): any lip would have pushed ground through
  the gate floor.
- **Shipped:** `TerrainPacket.wet_zone: Option<TerrainWetZone>` (centre, radii,
  falloff, the CALIBRATION-1 wet multipliers) with
  `standing_water: Option<StandingWater>` (albedo, roughness, edge wobble and
  its frequency), all omitted when absent. Both validators refuse a wet zone on
  non-layered terrain. The layered terrain shades it through its own pipeline
  (`terrain_wet_pipeline`, varyings 28-31); the dry path is the same function
  with `Val(false)`, and every existing frame stayed byte-identical.
  Inside the shore (ellipse displaced by the macro-noise texture at 0.7
  cycles/m, +-0.12): flat world-up normal, albedo (0.018, 0.022, 0.016), roughness
  0.02, no occlusion. Outside it: albedo x0.6 (+10% saturation), normals x0.4,
  roughness x0.25, fading over 0.6 m. No mesh, no carving: the shore is per
  pixel and grounded by construction.
- **Grounding (N-7's fix, used early):** `still_water::TerrainSurface` samples
  the lowered terrain exactly as rasterised (each cell split on the
  (col,row)-(col+1,row+1) diagonal). Under converge3 the kit and the ruin anchor
  stand on it; converge2 keeps its nearest-cell lookup (byte identity).
- **Tests** (`tests/converge3.rs`): rings, disc, glow and ripple albedo gone;
  water dark and smooth; terrain heights unchanged; render policy and camera
  equal to converge2's; converge2 serialises no `wet_zone`; wet-zone validation
  (no layers, out-of-range values, bad standing water); every kit instance on
  the rasterised surface, plants out of the water, rocks allowed on the shore.

### Correction to this contract (W-1)

- "Pool mean colour within dE 10 of the reflected sky colour" was wrong: at a
  20 degree view water reflects 8-14% of the sky, not the sky. Restated: the
  water carries the sky's hue (channel ratios within 0.02 of each other: 0.006
  measured) at a reflectance between the split-sum prediction and Fresnel
  (0.06-0.15: 0.082 measured), and no water pixel is brighter than the sky
  (0.53 measured). The renderer's dielectric F0 is 0.04 (water 0.02); not
  changed.

### Revision after the first review (W-1 v2)

- **Planar reflection.** `_record_scene_with_reflection!` (both render paths
  call it): when the terrain has standing water, the scene is first recorded
  from the camera mirrored about the water plane (`StandingWater.surface_y_m`,
  the mean rasterised ground height over the inner pool) into its own target.
  Right, up and forward are mirrored explicitly, so every point of the plane
  lands on the same pixel in both images and the water looks its reflection up
  at its own `frag_coord`. In that pass the terrain discards everything below
  the plane and the water's own ground (both would occlude the mirror from
  below); meshes are not clipped (a sunk rock's buried part can show in the
  reflection at the shore; not seen at review distances). The mirrored pass
  keeps its own mesh cache (it culls with its own camera) and the wet-terrain
  bindings are cached, so the window path allocates nothing per frame.
- **Water term.** Body lit directly, plus the reflection, by Schlick Fresnel
  with water's F0 0.02: `(1 - F) direct + F reflection`. A first version added
  the reflection on top of the full lighting and counted the sky twice (its
  ambient specular is ~24% at this smoothness); full occlusion in a second,
  water-only lighting call removes every ambient term.
- **Depth cues.** The outer 28% of the pool is shallow: its body colour runs
  from the wet bed (x0.55) at the shore to the water's own colour inside.
  Shore wobble raised to +-0.2 ellipse units at 0.45 cycles/m (from +-0.12 at
  0.7): a pool, not a disc.
- **Packing.** Varyings 0-31 were all in use; the wet variant carries the
  target's inverse size in `macro_parameters.zw` (zero on every other path)
  and the plane height and pass mode in `wet_d.zw`. Slot 16 binds the
  reflection image, or the IBL stand-in in the reflection pass (a target
  cannot be sampled while it is drawn).
- **Measured.** Reflection present: the pool's inner 60% is 0.264 of the
  frame-top sky's displayed luminance (painted only: 0.082). A ratio to the
  tone-mapped frame-top sky is not a physical check (the reflected sky at +20
  degrees is brighter in HDR than the displayed frame top), so the physical
  claim rests on construction: Fresnel times reflected radiance plus direct
  light on the body. Human judgement is the gate.
- **Tests.** Julia: wet zone and standing water decode and refuse (unknown
  field, brightening, degenerate ellipse, wobble out of range, missing surface
  height), 8 assertions. Rust: `standing_water` validation incl. surface
  height; W-1 tests unchanged otherwise.

## 2. R-1 — The ruin gets damage

> **Status: ACCEPTED (2026-10-05), revised after the first review.**
> kit2 (`python3 tools/build_kit.py --set kit2`, lock `tools/kit/kit2.lock.json`);
> converge3 renders kit2, converge2 kit1 (enforced by the lock's `set_id`).
> First review: the stepped break was "super jagged ... almost saw-toothy, a
> brick wall wouldn't break that way", and the fallen blocks did not look like
> what was knocked off. v2 fractures the top along a seeded Voronoi tessellation
> and the debris IS the removed cells. Measured (restated gate below): outline
> relief 1.43 m, RMS from a straight line 0.346 m (control: 0.57 m / 0.073 m);
> 35 debris pieces (wall end 27, 16.4 m3; gate 8, 0.66 m3). Ruin GLB 1.27 MB
> (+0.19 MB), ~3,140 triangles.
>
> **Human acceptance (2026-10-05, close sheet): met.** "The broken pieces are
> perfectly fine for now, and the breaks look much more natural." The corner
> pinnacle was not raised.

- **Build tool** (`tools/build_kit.py`, `kit2`): headless Blender boolean cuts
  with a seeded, noise-displaced cutter. Cuts go through the top of the wall
  end, through one or two merlons on the gate and through one corner of the
  gate's parapet. The cut faces take the stone material with box-projected UVs
  at the scan's texel density. The removed volumes are split along the cutter
  into 3-6 fallen blocks, laid at the wall foot and partly sunk, with seeded
  rotations. Deterministic like kit1: two builds, identical GLB digests;
  `tools/kit/kit2.lock.json`. kit1 stays unchanged and is still what converge2
  uses.
- **Budget:** the ruin GLB grows by ≤ 0.5 MB; triangles ≤ 2× kit1's ruin
  (1,810).
- **Permitted files:** `tools/build_kit.py`, `tools/kit/kit2.*`, `kit.rs`
  (fallen-block placement only, if not baked into the ruin GLB), `tests/kit.rs`.
- **Tests:** kit2 lock completeness; determinism; converge2 with kit1
  byte-identical; no fallen block intersects the wall or floats (lowest vertex
  within 5 cm of the bilinear terrain height, which is N-7's fix used early).
- **Acceptance (measured):** the wall end's top outline seen from the close
  camera is no longer a straight line: at least three height breaks of
  ≥ 0.4 m along it.
- **Acceptance (human):** "does the wall end read as a broken wall or as a
  block?" (the CONVERGE-2 finding, asked again).
- **Non-goals:** destruction at runtime, vines and moss decals, interior
  rooms, new architecture beyond the fort kit.


### Implementation (R-1)

- `tools/blender_ruin_damage.py` (headless Blender): exact-boolean cuts with
  stepped boxes, each step tilted; removed volume split into 1.3 m cell chunks.
  Closing an open shell first lets the boolean cap the cut, but on the gate it
  sealed the arch passage, so closing is per piece (on for the wall end, off for
  the gate, whose cuts touch only closed merlons and parapet).
- `tools/ruin_damage.py`: surviving faces keep their authored UVs and normals,
  transferred barycentrically from the source triangle they lie on (plane match
  in either orientation; Blender flips faces of open shells). Cut faces, and
  hole caps no source triangle carries, become stone with UVs box-projected at
  the wall's measured 0.087 UV/m. Zero-area slivers dropped at |cross| < 2e-6
  (render conditioning refuses <= f32::EPSILON after placement). Cuts are
  authored against previews: a regular staircase read as stairs; uneven steps
  plus a second bite at 125 degrees read as a break.
- Fallen blocks: the four largest stone chunks of the wall end, seeded yaw,
  tilt 8-28 degrees, laid around its foot. `kit::apply_kit` grounds each block
  instance on the terrain under it (lowest point 15% of its height below the
  rasterised surface): baked into ruin space they floated up to 6.4 cm where the
  ground falls away from the anchor (caught by the test). The gate yields no
  chunk above 0.12 m3 (open shell), so it has no blocks.
- Determinism: two kit2 builds give the pinned digests; kit1 still rebuilds to
  its lock byte for byte; rocks, tree and fern are identical in both locks.
- Tests: kit set per arm (converge3 refuses kit1, converge2 kit2); kit2 differs
  from kit1 only in the ruin, which grows <= 0.5 MB; every fallen block touches
  the ground and none is buried more than 0.45 m.
- Measurement: silhouette of the wall-end triangles projected through the close
  camera; plateaus >= 0.25 m wide more than 2 m above ground; a break is a step
  >= 0.4 m between adjacent plateaus. The first version counted the block's
  vertical side edge as breaks (control: 4); plateau width in metres fixed it
  (control: 0). Developed in Palette (seam S-6: no export of session code).


### Revision after the first review (R-1 v2)

- **Fracture, not steps.** `tools/ruin_damage.py` seeds a jittered grid of
  Voronoi sites over each piece (wall end 1.3 x 0.75 x 1.3 m, gate 0.9 x 0.55 x
  0.9 m), builds the cells in space squashed vertically (x1.6 / x1.5, so cells
  sit like courses) and breaks off every cell whose site lies above an uneven
  line: high at one end, descending toward the other, with per-site noise.
  `tools/blender_ruin_damage.py` builds each convex cell by bisection, grows the
  removed ones by 3 mm and unites them into one cutter, then subtracts once.
  Subtracting cell by cell left zero-thickness fins on every shared face
  (neighbouring cells compute that face separately and disagree by float
  error). A limited dissolve (0.5 degrees, per material) restores the faces the
  booleans split: the wall end is 553 triangles, not 13,563.
- **Debris is what broke off.** Each removed cell intersected with the stone
  (every material's shell: the gate's merlons are trim) is one piece. Its
  surface faces keep the wall's own UVs and normals, transferred from the
  source before it moves, so a fragment shows the masonry it carried; fracture
  faces are stone. Pieces lie on their flattest axis, land outward from where
  they broke off, mostly within a metre of the foot, not overlapping, and not
  toward the neighbouring piece (the gate from the wall end, the wall end from
  the gate). `kit::apply_kit` grounds each on the terrain under it. The gate's
  debris is cut from a closed copy of the gate, which itself stays open (closing
  it sealed the arch passage).
- **Gate restated.** ">= 3 breaks of >= 0.4 m between plateaus" encoded the
  stepped design the review rejected: the fractured top measures 2. Restated:
  the top outline (silhouette points steeper than 68 degrees, the block's side
  edges, dropped) has relief >= 1.0 m and RMS deviation from a straight line
  >= 0.2 m, and the intact converge2 wall end fails both. Tool:
  `julia --project=graphics_lab tools/ruin_outline_measure.jl CANDIDATE CONTROL`.
  Measured: candidate 1.426 m / 0.346 m (met), control 0.569 m / 0.073 m.
- **For review:** a narrow corner pillar stands at the wall end's intact end
  (where the break line starts); it may read as a pinnacle.

---

## 3. L-2a — Distant foliage holds up (alpha-to-coverage)

> **Status: ACCEPTED (2026-10-05).** "I'm not seeing any stippling or
> fringing. This actually looks really good in that case for the tree."
>
> converge3 now
> renders at 4x MSAA with alpha-to-coverage (`render_policy.foliage_coverage`).
> Measured (overcast-coverage rig, `tools/foliage_measure.py`, alpha-tested
> coverage relative to the opaque-card control, both under the policy):
> **0.977 at 10 m, 0.952 at 40 m** (gate >= 0.92 at both; before L-2a 0.96 /
> 0.77). Captures fully opaque (non-opaque alpha 0 bp in all three converge3
> views, was 188-712 bp: alpha-to-one). GPU cost per view roughly unchanged
> (33.0 / 13.1 / 13.0 s). Sheets: foliage
> `artifacts/calibration/l2a-foliage-v2-sun/sheets/foliage_coverage_review_sun_ibl.png`
> (N-6 alpha test vs L-2a coverage at 5 / 10 / 40 m, x1 / x2 / x4 crops);
> converge3 `artifacts/parity/review-converge3-l2a/` (vs W-1/R-1 converge3).

### Implementation (L-2a)

- **Lava** `lava-0002-multisample.patch` (verified: applied to the pre-patch
  tree it reproduces `vendor/Lava/src`): `LavaFramebuffer(samples=)` renders
  into an MSAA colour image and depth, resolved (average) into the readable
  single-sample `color_image`, so readback, sampling and the resolve pass need
  no change; `GraphicsPipeline(alpha_to_coverage=)`; the sample count joins
  the pipeline cache key only when > 1; `alphaToOne` is enabled at device
  creation when supported and used with alpha-to-coverage, so covered samples
  are opaque (without it, foliage edges came out translucent in the capture).
  The adapter refuses the policy on a device without it.
- **Adapter:** the scene target is multisampled under the policy; mask
  materials draw through `mesh_coverage_pipeline` (`_mesh_coverage_fragment`):
  alpha = max(sharpened, raw), sharpened = `(a - cutoff)/fwidth(a) + 0.5`,
  raw = `a / (2 cutoff)`. Sharpening alone measured 0.855 at 40 m: it rounds a
  far texel's averaged, below-cutoff alpha to nothing; the raw term keeps that
  thin coverage, sharpening keeps near edges crisp. Shadows keep the alpha
  test. The W-1 reflection target stays single-sample.
- **Calibration:** rigs `sun-coverage`, `overcast-coverage`,
  `sun-ibl-coverage` (the base rig plus the policy) for the foliage views; not
  in `ALL`, so `--rigs all` and the frozen runs are unchanged.
- **Tests:** Rust policy validation (2/4/8 accepted; 0/1/3/16 refused; absent
  serialises nothing), coverage rigs (parse, not in `ALL`, base lighting and
  IBL kept); the full-policy round trip carries the new field.

- **Vendor patch** `lava-0002-multisample.patch`: pipeline sample count and
  `alphaToCoverageEnable`; multisampled colour and depth attachments with a
  resolve. Patch only adds parameters, defaulting to today's values.
  `LAVA_REVISION` stays the base commit.
- **Policy:** `render_policy.foliage_coverage: Option<FoliageCoverage>`, with
  `AlphaToCoverage { samples: 4 }` as the only variant. Absent means today's
  pipelines at one sample, so frames are byte-identical. Present: every raster
  pass runs at 4x MSAA on top of the 2x supersample, and `Mask` materials
  write coverage from their alpha instead of discarding. The shadow pass keeps
  `discard` (a depth-only shadow map has no use for coverage).
- **Alpha sharpening:** with A2C, alpha is rescaled around the cutoff over
  one pixel's derivative (`(a - cutoff) / max(fwidth(a), ε) + 0.5`). This is
  the standard trick that keeps A2C edges crisp up close and soft far away.
  Without derivatives in Lava, a mip-level-based width is used. Which one is
  recorded.
- **Permitted files:** `graphics_lab/vendor/Lava` (patch), `vendor/README.md`,
  `LavaAdapter.jl`, `LuxelGraphics.jl` (policy decode), `render_policy.rs`,
  `tools/foliage_measure.py`, tests.
- **Tests:** policy round-trip and validation (samples ∈ {2, 4, 8}); every
  existing reference byte-identical with the policy absent; adapter unit test
  that a half-covered pixel of an A2C card resolves to 0.5 ± 0.13 coverage.
- **Acceptance (measured):** foliage row, alpha-tested coverage relative to
  the opaque-card control ≥ 0.92 at 40 m (from 0.77) and still ≥ 0.92 at 10 m;
  the GPU frame-cost increase is recorded per view.
- **Acceptance (human):** the 5 / 10 / 40 m foliage sheet: "no stipple, no
  fringe".
- **Non-goals:** TAA, motion vectors, specular AA (the rest of L-2), MSAA for
  its own sake on opaque edges (it comes along but is not the gate).

---

## 4. N-7 — Scatter and forest

> **Status: ACCEPTED (2026-10-05).** "Overall, this actually looks kinda
> incredible and what you'd see in the late PS3 era." Notes from the review:
> some tree shadows look off (carried: L-1 cascaded shadows, below), and the
> clumping suits a different biome than this kind of tree ("swap a bunch of
> textures and the landscape and you've got a Pacific Northwest evergreen cloud
> forest") — a biome/content question, not a capability one.
>
> converge3 now
> places its trees, ferns and rocks by scatter instead of constants: 2,825
> trees, ~220 fern and ~24 rock instances, one world in all three views.
> Measured: **wide authored pixels 31.6%** (gate >= 30%; converge2 14.1%),
> medium 34.4%, close 44.4%; **no tree vertex nearer than the ruin lies in
> its screen box in any view** (vertex-exact, leaf cards counted solid), so
> trees hide none of the landmark (gate >= 90% of converge2's visibility);
> GPU frame cost 34.3 / 14.4 / 15.1 s vs converge2 32.4 / 14.3 / 14.2 s
> (+6% at most; gate: profile above +50%). Sheets:
> `artifacts/parity/review-converge3-n7/` (vs converge2).

### Implementation (N-7)

- `scatter.rs` (pure): SplitMix64 (no new dependency), Bridson Poisson-disc,
  hash value noise, exclusions (disc, ellipse, sightline corridor widening
  from camera to landmark), slope from the rasterised surface, `Style`
  (scale range, tilt, sink, value and hue jitter), `scatter()`. Every random
  value is drawn for every candidate, so one candidate's fate never shifts
  another's. `tint()` shifts hue around the grey axis with zero-sum channel
  offsets (brightness unchanged) and scales value.
- `campaign2_layout`: forest over everything the cameras look toward (to the
  world's edge), a clearing ramping in from 14 m to 30 m around the ruin,
  stands from 55 m noise (near-full density inside, stragglers in the
  glades), 3.5 m spacing, scale 1.0-1.6, slope <= 0.55; a corridor from each
  camera to the ruin (3 m wide at the camera, 8 m at the ruin) and 9 m around
  each camera kept clear. Ferns in 9 m noise patches, denser near the ruin,
  never in the water or the ruin's footprint. Rocks sparse, sunk and tilted.
  Tuning, by render: 5.5 m spacing with a soft mask read as an orchard
  (wide 19.1%); stands plus fern patches gave 20.7%; the full extent 28.1%;
  the 14-30 m clearing (forest on the left hill) 31.6%.
- `InstancePacket.variation: Option<InstanceVariation { tint_rgb }>`
  (validated [0.5, 1.5], omitted when absent); the adapter multiplies the
  per-instance base colour it already uploads (an untinted instance multiplies
  by exactly 1.0). `kit::apply_kit` takes `KitLayout::Authored` (converge2,
  N-5's constants, byte-identical) or `KitLayout::Scattered`.
- Camera offsets are one constant shared by the view table and the corridors.
- Tests: Poisson spacing and seeding, corridor widening, tint brightness,
  scatter constraints and determinism (unit); one scattered world in all three
  views, no tree in any sightline, scattered instances tinted, converge2's
  not (`"variation":` absent from its JSON).

### Corrections to this contract (N-7)

- **Packet growth** "<= 2 MB" assumed ~150-250 bytes per instance; measured
  ~880 (ids, full-precision transforms, tint). Converge3 is 61.8 MB against
  converge2's 52.6 MB (+9.2 MB: +1.7 MB L-2a/W-1/R-1, +7.5 MB scatter), under
  the 128 MiB frame bound. A far LOD or instance compaction is the remedy if
  the forest grows; not needed at this cost.
- **Far LOD** not needed: GPU cost +6%, under the +50% profile threshold.

The placement grammar the audit §6 calls for: an area, an asset set, a
density, slope and height constraints and a seed, out of which come instances
conformed to the terrain. Material varieties (carried item 3) land here,
because variety pays off only once there are many instances.

- **Ground conformance first (fixes both defects in §0):** one
  `terrain_height_bilinear(terrain, x, z)` on the **lowered** (extended)
  terrain. It matches the rasterised surface to within float error, so the
  kit's +2 cm fudge and the scatter both use it. The kit keeps its current
  call under converge2 (byte identity) and switches under converge3.
- **Scatter region** (`scatter.rs`, pure): `ScatterRegion { area (polygon or
  ring), assets with weights, density per m², min spacing, slope range, height
  range, exclusion zones, seed }` → `Vec<Placement>`. Poisson-disc
  (Bridson, seeded), then constraint filtering. It is a pure function: same
  inputs, same placements, bit for bit.
- **Forest:** a scatter region whose density falls off over an edge band
  (no hard forest wall) and whose spacing scales with tree size. Species mix
  by weight.
- **Hierarchy (audit §7.4):** an exclusion radius around the landmark that
  ramps density up from zero, plus a sightline corridor from each authored
  camera to the landmark that is kept clear of trees (ground cover allowed).
  "Density without hierarchy is noise."
- **Per-instance variety:** `InstancePacket` gains
  `variation: Option<InstanceVariation { tint_rgb, ... }>`, omitted when
  `None`. The adapter multiplies the per-instance base colour it already
  uploads. Scatter sets a seeded hue / value jitter (±6% value, ±3° hue), a
  scale range per asset and a random yaw. Rocks also get a random tilt.
  Wear / wetness blends stay out (below).
- **Content under `converge3`:** forest bands on the extended hills and
  toward the ridge, using the kit tree; fern and rock scatter at the ruin's
  base and along the slope breaks; the hand-placed tree, rock and fern
  constants retire under converge3 (they stay for converge2).
- **Budget:** instances only, no new meshes required. The kit tree at 9.7 k
  triangles × several hundred trees is a few million triangles per frame. If
  the measured frame cost says that is too slow, a far LOD (a
  cards-only tree from the existing leaf-card baker) lands here with its own
  numbers. Packet growth ≤ 2 MB.
- **Permitted files:** new `scatter.rs`, `kit.rs`, `lib.rs` (converge3
  composition, bilinear height), `render_policy.rs`, `LavaAdapter.jl` /
  `LuxelGraphics.jl` (instance variation only), `tools/build_kit.py` (far LOD
  only if needed), `tests/scatter.rs`.
- **Tests:** scatter determinism (same seed, identical placements; different
  seed, different placements); every placement satisfies its slope / height /
  spacing / exclusion constraints; every instance's base is within 5 cm of the
  bilinear terrain height; sightline corridors contain no tree; variation
  absent means byte-identical packets; all earlier arms byte-identical.
- **Acceptance (measured):** wide-view authored-geometry pixels ≥ 30% (from
  14.1%); the landmark's visible pixel count in the close, medium and wide
  views ≥ 90% of converge2's (the forest does not bury it); GPU frame cost per
  view recorded against converge2, and any increase above 50% gets a profile
  before acceptance.
- **Acceptance (human):** close / medium / wide sheets, two questions:
  "what does your eye hit first?" (still the ruin) and "does the forest read
  as a forest or as copies of one tree?". If it reads as copies, the remedy is
  a second species through the existing leaf-card pipeline, inside this
  contract.
- **Non-goals:** wear / wetness blends and several scans per family (they
  need a material-layer system; separate contract), biome rules, roads and
  paths, wind (L-3), runtime collision for scattered instances, streaming.

---

## Deferred to CONVERGE-4

| Item | Why not now | Lands with |
|---|---|---|
| N-3 km-scale backdrop + N-1's ridge-contrast target (0.65 vs ≤ 0.40) | Its own contract. It decides the world's scale, and N-9's composition depends on it | CONVERGE-4, before N-9 |
| N-8 contact AO + depth capture | Scatter creates most of the contacts it would darken. Measure after N-7 | CONVERGE-4 |
| L-2 remainder: TAA, specular AA | Only visible in motion; captures are stills | With L-3 wind |
| Wear / wetness blends; several scans per family | Needs a material-layer system | CONVERGE-4 |
| Bark depth (parallax) | Review the N-7 close sheet first; bark may never be close enough to matter | Review only |
| Kit `prepare` receipts (collision, LODs) | Runtime, not frame | Gameplay track |
| Carried item 5 elsewhere | Hygiene. Remove the two env-reading wrappers once their callers move to `Campaign2Inputs` | Any session; small |
| N-2 wet/dry target restatement | Doc-only correction | With N-3 |


---

## CONVERGE-3 closeout (2026-10-05)

| Contract | Human acceptance | Measured acceptance |
|---|---|---|
| W-1 still water (planar reflection) | Met (v2): "muuuuuch better ... you gave water the ability to have depth". v1 (painted only) read as "a large round tree shadow" | Rings, disc and glow gone met; water carries the sky's hue (ratios within 0.006) met; reflectance restated (see W-1) |
| R-1 ruin damage (Voronoi fracture) | Met (v2): "the breaks look much more natural ... broken pieces are perfectly fine". v1 (tilted steps) read as saw-toothed | Restated gate: outline relief 1.43 m >= 1.0, RMS 0.346 m >= 0.2, control 0.57 / 0.073 fails both: met; debris = the removed cells (35 pieces) |
| L-2a alpha-to-coverage | Met: "not seeing any stippling or fringing ... looks really good" | Coverage vs opaque-card control 0.977 (10 m) / 0.952 (40 m), gate >= 0.92: met (was 0.96 / 0.77); captures fully opaque; GPU cost ~unchanged |
| N-7 scatter and forest | Met: "looks kinda incredible ... late PS3 era" | Wide authored pixels 31.6% >= 30% met (was 14.1%); no tree hides the ruin (vertex-exact) met; GPU +6% met; packet +7.5 MB vs a "<= 2 MB" guess (restated) |

**Identity:** 128/128 frames byte-identical on the final code (parity
`ab-{null,full,converge0,converge1}-tfix`, `ab-converge2-n5`; calibration
`run3`, `run3-grazing`, `n6-foliage`), `tools/verify_identity.py`. Every new
axis is absent by default and byte-identical when absent: wet zone and
standing water, the reflection pass, foliage coverage (MSAA), instance
variation, the coverage calibration rigs, kit2.

**Tests:** Rust crate all pass (incl. converge3 7/7 and kit 5/5 with the built
kits); Julia protocol (incl. wet zone 8, instance variation 5), render policy,
and the GPU adapter suite all pass.

**New references:** `artifacts/parity/ab-converge3-final` (converge3: W-1 + R-1 +
L-2a + N-7, with the trim UV fix below; needs `LUXEL_KIT_SET=tools/kit/kit2.lock.json` and
`python3 tools/build_kit.py --set kit2`), foliage coverage
`artifacts/calibration/l2a-foliage-v2` (+ `-base`, `-control-opaque`, `-sun`).

**Infrastructure landed beyond the contracts:** `tools/verify_identity.py`
(re-renders every frozen reference, exits nonzero on any byte change);
`still_water::TerrainSurface` (heights exactly as rasterised, used for all
converge3 grounding); the planar reflection pass shared by both render paths;
Lava `lava-0002-multisample.patch` (MSAA targets, alpha-to-coverage,
alpha-to-one); `tools/blender_ruin_damage.py` + `tools/ruin_damage.py`
(Voronoi fracture with attribute transfer); `tools/ruin_outline_measure.jl`;
`scatter.rs`.

**Defects found and fixed on the way:** a carved water mesh on the 2 m grid gave
a staircase shoreline (replaced by per-pixel water); the reflection counted the
sky twice (ambient specular plus the mirror); the first fix for that swapped in
`NoIbl` and changed nothing, because the campaign arms run the constant-ambient
branch, not IBL (found from a byte-identical capture; fixed by full occlusion in
the water's direct-light call); exact booleans left zero-thickness fins
between neighbouring cells (united, grown cutter); `holes_fill` sealed the gate's
arch passage (closing is per piece); booleans left slivers render conditioning
refuses (|cross| < 2e-6 dropped); fallen blocks floated where the ground falls
away (grounded per instance); the outline measure counted the block's side edge
(width in metres, steep segments dropped); the ruin's trim smeared into diagonal
streaks after review (UV transfer chose each corner's source triangle on its
own, so a corner on a UV seam took the neighbouring atlas region; and the
coplanar merge fused quads across UV islands): transfer is now per triangle
(one source triangle's affine map for all three corners) and the merge is
delimited by UV, which also fixed the arch passage floor; MSAA foliage edges came out
translucent (alpha-to-one); fwidth sharpening erased thin far leaves (max with
raw alpha).

**Palette test-drive** (`Palette Delivery/PALETTE_SEAMS_CONVERGE3.md`): seven
seams; three fixed in the installed bundle with regression tests (S-2 snapshot
cost O(objects x loaded modules), 56.7 s -> under 5 s; S-3 one unprintable
definition aborted revival; S-7 a test left `Base.show(::Int)` pirated).

**Carried into CONVERGE-4:**

1. **Cascaded shadows (L-1)** — first. The single 512^2 view-fitted shadow map
   covers 60 m: coarse near-tree shadows and none on the distant stands (N-7
   review: "some of the shadows from the trees seem kinda off").
2. **Biome and species:** the forest's clumping and the small deciduous tree
   suit different settings (review: "swap a bunch of textures and the landscape
   and you've got a Pacific Northwest evergreen cloud forest"). Species mix,
   biome-driven density, a second tree through the leaf-card pipeline. The
   trees and terrain read as arid (Middle Eastern, not desert); the fern is a
   jungle plant and does not belong (review, 2026-10-05): ground cover chosen
   per biome (dry grasses, low shrubs here).
3. **N-3 km-scale backdrop** and N-1's ridge-contrast target; **N-8 contact AO
   + depth capture**; **L-2 remainder** (TAA, specular AA) with L-3 wind.
4. **Wear / wetness blends, several scans per family** (material layers).
5. **Water beyond still pools (L-5):** ripples, refraction, shoreline; the
   reflection pass clips terrain but not meshes below the plane.
6. **Instance compaction** if scatter grows (instances cost ~880 bytes of JSON).
7. Bark depth review; kit `prepare` receipts; carried item 5 elsewhere (the
   two env-reading wrappers).
