# Codeweald concept-batch zone pipeline

The pipeline's job is to make a zone faithful and editable; it is not an image-to-heightmap trick.

```text
concept image batch + written brief
            │
            ▼
model-proposed annotations ──► human/automated review ──► ConceptAnnotations v1
                                                              │
                   sandboxed WorldBuilder DSL ─────────────────┤
                                                              ▼
                                                       ZoneSpec v1
                                                 │            │
                                      Julia solve│            │Rust validate
                                                 ▼            ▼
                                           certified compiled world
                                                         │
                                                         ▼
                                          Godot candidate → acceptance
                                                            │
                                                            ▼
                                                   active moba_3d.tscn
```

## Authority boundaries

The model may propose regions, references, and uncertainty. It cannot silently decide topology. Every feature needs image evidence, confidence, and a review state. The compiler rejects low-confidence unreviewed features. A reviewer can retain a low-confidence feature deliberately, and that decision remains visible in the report.

`semantic` and `generation.profile` are separate on purpose. `alpine_massif` with `alpine_jagged_massif` is a bounded geometry request: a renderer must preserve the high, cliff-heavy, snowline-bearing massif character. It must not substitute smooth noise domes. The same pattern distinguishes a ridge, foothills, crag field, and valley floor.

The current terrain grammar is deliberately explicit:

| Semantic | Required generation profile | Intended read |
| --- | --- | --- |
| `alpine_massif` | `alpine_jagged_massif` | Connected primary/shoulder summit chains, granite faces, sparse high snow, and a source-relative projected silhouette. |
| `alpine_ridge` | `alpine_sawtooth_ridge` | Continuous narrow spine with repeated teeth and saddles. |
| `foothills` | `rolling_foothills` | Broad low, traversable shoulders rather than cliff terrain. |
| `crag_field` | `scattered_crag_field` | Separated rocky outcrops with exposed-rock coverage. |
| `cliff_band` | `cliff_escarpment` | Asymmetric steep face and raised shelf. |
| `valley_floor` | `glacial_valley_floor` | U-shaped trough, broad floor, and enclosing shoulders. |

The compiler rejects mismatched semantic/profile pairs; terrain acceptance also
checks the profile's measurable relief, slope, or rock-coverage contract.

## Model-native WorldBuilder DSL

`world_intent.py` is the model-facing construction language. It uses familiar
Python expression syntax but is **never imported or executed**. The
`worldbuilder_dsl.py` frontend parses a strict AST whitelist and rejects general
imports, host calls, loops, control flow, attribute traversal, unpacking, and
unknown constructors. Its output is `codeweald.world-intent/v1`; that intent is
type-checked against reviewed feature IDs and semantics before it may patch the
canonical ZoneSpec.

The first production vocabulary deliberately owns only landform composition:

```python
from worldbuilder import World, clustered_ridges, ridge_network

w = World("codeweald_alpine_arena_v1")
w.landform(
    "western_alps",
    composition=ridge_network(
        spines=3,
        elevation_bias=0.72,
        along_jitter=0.08,
        cross_jitter=0.10,
    ),
)
```

Each scalar has one geometric meaning, and `zone_rasterizer.py` reads all of
them:

| scalar | `ridge_network` (`alpine_jagged_massif`) | `clustered_ridges` (`scattered_crag_field`) |
| --- | --- | --- |
| `spines` | parallel ridge lines across the massif: the crest plus alternating shoulders stepping outward, each rank lower than the last | overlapping rock masses per outcrop |
| `along_jitter` | how far each summit slides off its nominal station along the crest | lobe spread along the outcrop's long axis |
| `cross_jitter` | how far the spine wanders off its principal axis, both noise warp and deterministic meander | lobe spread across the outcrop, plus the crag chain's meander |

Bounds: `spines` is 1..8; both jitters are 0.0..0.5. Values outside those are a
compile error naming the feature, not a silent clamp. Geometry multiplies each
jitter by its *ratio* to the pattern default, so the defaults in
`worldbuilder_dsl._PATTERN_SCALARS` reproduce the hand-tuned reference profile
exactly and every calibrated amplitude in the rasterizer keeps its meaning.

`terrain_manifest.json` records `heightfield_sha256` and the resolved
`composition_scalars` per landform, and each build rotates the manifest it
replaces to `terrain_manifest.previous.json`. `wge_critic.detect_no_ops`
compares the two: authoring that changed while the heightfield hash did not is
reported as a non-actionable finding owned by the rasterizer. This is a general
check, not a guard against one bug -- any advertised knob can quietly stop
being wired, and every other signal stays green when it does.

This is a compiler boundary, not a scripting escape hatch:

- ConceptAnnotations remain the evidence and topology authority.
- The DSL states compact semantic construction intent.
- ZoneSpec remains the canonical, engine-independent IR.
- Julia expands bounded intent into deterministic numeric solutions.
- Rust validates identity, topology, assets, and solver contracts.
- Godot realizes the certified scene; it does not reroll placement.

New DSL verbs are added only with a typed IR mapping, deterministic
implementation, validation contract, and regression fixture. Engine backends
must never interpret arbitrary source text themselves.

Faction keeps are semantic landmarks, not team-coloured copies. A
`faction_keep` must name `realm: albion|midgard|hibernia` and a stable team ID.
The Godot scene compiler selects the matching realm keep and retains all bases
in `faction_bases` for native adapters; legacy `team_a`/`team_b` fields remain
for the current two-side lane implementation.

## Start a new concept-art batch

Use the intake tool before asking a model to interpret a new group of images. It
copies and SHA-256-pins every image inside the batch, records dimensions and
roles, and writes both a constrained annotation template and a model-facing
task. That gives a model one stable contract rather than asking it to improvise
file paths, coordinate systems, or a terrain recipe from pixels.

```bash
python3 pipeline/concept_batch_intake.py \
  --output concept_batches/jerall_v1 --batch-id jerall_v1 \
  --brief "Cold Jerall mountain pass: glacier-cut valley, ruined fort, pine forests, and traversable roads." \
  --image western_vista=/absolute/path/to/western_alps_vista.png \
  --role western_vista=perspective \
  --image overview=/absolute/path/to/jerall_overview.png \
  --role overview=painted_overview --canonical-map overview \
  --width-m 3200 --length-m 2200 --engine godot
```

The output contains `intake_manifest.json`, `source/`,
`vision_annotation_task.md`, and `annotations.draft.json`. A vision-capable
model fills a copy named `annotations.json`; only that evidence-bound semantic
file enters `build_zone.py`. The old `minimap_to_map_spec.py` color-threshold
experiment is not a build input and must not be used as topology authority.

Multi-image batches receive an immutable `image_reconciliation` contract.
Exactly one `painted_overview`, `minimap`, or `orthographic_map` owns normalized
map geometry. Perspective/detail/material/elevation references are
`evidence_only`; their role limits them to claims such as silhouette,
elevation, occlusion, material, biome, style, or asset scale. Every feature
geometry names the canonical `source_image_id`, every multi-image evidence
entry names its claim and `direct|partial|inferred` visibility, and every source
image must be cited. An inferred observation cannot enter an unreviewed
feature. The compiler reports per-image use, per-claim counts, and the number
of features reconciled across views, while the evidence sheet labels every
panel, claim, and visibility state.

## Texture candidates become materials only after a gate

ComfyUI is a source of candidates, not an authority that a rendered object or a
framed illustration is a tileable surface. Feed its output through the PBR lane
to measure opposite-edge continuity and exposure, then write provenance-pinned
albedo, normal, and roughness maps only when it passes:

```bash
python3 pipeline/comfy_generate_texture.py \
  --output assets/generated/codeweald_textures/peat_grass_candidate.png \
  --prompt "seamless square PBR albedo, weathered Highland peat grass and soil, top-down surface only" \
  --material-output-dir assets/generated/codeweald_materials/peat_grass \
  --material-id peat_grass
```

`texture_material_pipeline.py` rejects non-square, low-information, badly
exposed, or visibly non-tileable candidates. Accepted surfaces receive
deterministic normal and roughness derivatives plus a material manifest; the
input candidate and every digest remain traceable.

`--repair-seams` is deliberately narrow: it runs only when edge discontinuity
is the *sole* failed check, then the repaired image must clear the ordinary
gate. If a later assessment rejects it, prior maps and the manifest are
revoked. A batch opts into accepted bundles with `terrain_materials`.
Production Godot terrain currently requires reviewed `grass`, `road`, `rock`,
`wetland`, and `snow` contracts, each with hash-verified, decoded,
resolution-matched albedo, normal, and roughness maps. A batch also declares
`terrain_material_scale_m`; the resolver writes meters per repeat and computed
texels per meter into the terrain manifest. The Godot terrain shader binds all
five complete PBR sets through world-space triplanar projection and blends
wetland from a deterministic bank/peat mask. Lane ribbons and Alpine dressing
consume the relevant road and rock contracts. Acceptance reloads the saved
scene, proves every shader binding and the wetland mask survived serialization,
and fails any layer below 48 texels per meter. It never accepts a loose image
path as a production terrain layer.

## Generated foliage family lane

`generate_highland_foliage.py` creates portable pine, spruce, and fir families
with `lod0`, `lod1`, and `lod2` GLB variants plus Unity FBX copies. A profile
must require `lod0` and `formats: ["glb"]` for close Godot placement, so the
catalog cannot accidentally select a nested Unity copy or an impostor variant.
The portable asset plan joins the catalog-observed sibling LODs into one family,
and Godot consumes all three tiers with explicit visibility ranges. Blender
preflight verifies the exact selected close meshes and their quality floors
before a scene rebuild.

## Vision-provider handoff

`concept_batch_intake.py` now emits `vision_annotation_packet.json` and an
empty `model_response.template.json` beside the prose task. Send the packet and
the listed source images to any vision-capable provider, then ingest its bounded
response with:

```bash
python3 pipeline/vision_annotation_adapter.py concept_batches/<batch> \
  --response /path/to/model_response.json
```

The response is forbidden from changing source hashes, paths, world bounds,
image reconciliation, or the batch seed. The adapter restores those immutable
fields, compiles semantic features immediately, emits a proposal evidence
overlay, and writes a durable diagnostic rather than an invalid
`annotations.proposed.json` on failure. A perspective reference cannot claim
topology, and an unused secondary image makes the proposal fail instead of
being silently ignored.

## Style-reference acceptance

Every `build_zone.py` run writes `style_reference.json` from the canonical map
image, regardless of file order. Perspective images used only for silhouette
or elevation cannot contaminate the terrain palette; secondary images affect
terrain palette extraction only when their accepted evidence claims biome,
material, or style. A captured Godot build receives `style_match` inside its
visual acceptance report, comparing palette distribution, luminance,
saturation, and edge frequency. The reviewed batch chooses the threshold and
whether a mismatch is a failure. Caledonia uses `minimum_style_score: 0.42` and
`style_mismatch: fail`, so a structurally valid but visually weak scene cannot
be promoted.

### Terrain materials are judged at the distance they are seen from

A terrain material is almost never sampled at one texel per pixel. On the
reference overview of a 256 m arena at 1440x900 — 0.284 m/px — one screen pixel
covers 36 to 71 texels of an 8 m-repeat 1024px material. Mipmapping averages
away everything finer than roughly a thirty-sixth of the tile before it is
drawn, so fine grain is authoring effort that no normal viewing distance can
resolve.

`texture_material_pipeline.assess` therefore reports `coarse_contrast`: the
contrast left after removing sub-36-texel detail. Measured across the accepted
bundles this separates cleanly — `highland_granite_v2` keeps 0.074 and reads as
rock from the air, while `caledonia_wetland_peat` keeps 0.018 and
`caledonia_windpacked_snow` 0.019, and both flatten to a single colour. The
floor is 0.020, just above the render-side block-variation floor of 0.012 that a
surface must clear to register as varied at all.

It warns rather than fails. A material may legitimately be authored for close
inspection, and the distance at which it flattens depends on a repeat scale
`assess` cannot see. The point is to answer at accept time what previously took
a full build-and-capture cycle.

The practical consequence for generated textures: ask for coarse structure with
real contrast, not detail. A finely grained rock scan is worthless at range.

### Comparing a render to painted art

`style_reference.detail_density` is the one measurement compared *across* the
two, and both sides call the same function: frame resampled to a common height,
one gradient threshold, whole-frame denominator. Anything else is not a ratio.
The per-side metrics (`edge_density` on the source, `foreground_edge_density` in
the Bevy gate) stay as they are — they are tuned gates, self-consistent within
one side, and re-tuning them is a separate change.

The defect this replaced is worth remembering: the render averaged over
foreground pixels at threshold 0.045 while the source averaged over the whole
frame at 0.055, so the render was flattered on both axes. Normalising made the
measured shortfall *worse* — 6.9x became 15.3x.

`detail_density` counts texture, material break-up, and shading, not silhouette.
Landform composition therefore cannot move it: rebuilding
`codeweald_alpine_arena_v1` with every composition scalar at its bound changed it
by under one percent, while pushing the jitters further broke the traversability
gate. `wge_critic` reports the shortfall against terrain materials and renderer
shading, and emits no DSL patches for it. Silhouette complexity is a real and
separate property that nothing currently measures; a skyline metric could drive
composition honestly, which this never could.

Lighting begins with a bounded white balance and grade derived from the pinned
source profile. If the first real Godot capture is below threshold, the
pipeline writes a provenance-bound `style_calibration.json`, rebuilds once, and
measures the feedback capture. `style_calibration_report.json` records both
scores and the selected pass. If feedback regresses, the first candidate scene,
capture, projection evidence, calibration, and build provenance are restored
together.

Visual acceptance also compares each feature's rendered region with its cited
source-evidence region. Mean RGB distance plus luminance and saturation ratios
are reported per feature. The reviewed
`maximum_regional_palette_mismatch_fraction` is a hard gate, not only a warning.
Godot projects every biome polygon against the actual terrain and compares
rendered foliage coverage with its source region; every landmark reports a
real mesh footprint from an explicitly composed imported transform hierarchy
and must fit `landmark_evidence_fill_ratio`. This keeps a globally plausible
grade from hiding a wrongly colored keep, empty forest, toy-scale landmark, or
massif. It also caught and corrected settlement feature roots that carried
world offsets in their child instance instead of local anchored transforms.

## Runtime overview-projection acceptance

Image-wide style or edge metrics cannot detect a mirrored or rotated map. The
Godot overview capture writes `terrain/godot_overview_projection.json` from two
actual `Camera3D` nodes. The oblique beauty camera provides the render, feature
crops, and vertical-relief measurements. A source-aligned orthographic evidence
camera provides cartographic X/Z coordinates for topology checks, so composition
changes cannot masquerade as mirrored or displaced data. The report includes
screen-space deltas for world +X/+Z and the projected position of every
landmark and lane centerline. The projection gate writes
`terrain/overview_projection_acceptance_report.json` and requires:

- world +X to project toward source-image right;
- world +Z to project toward source-image top;
- every landmark to project inside its first reviewed evidence rectangle.
- every lane to remain inside its reviewed region and near its reviewed source
  trace.
- every landform to provide dense projected height samples; Alpine massifs must
  retain at least 40 pixels of visible relief.
- every biome to provide a terrain-conforming projected polygon for coverage
  measurement.
- every landmark to provide world bounds and a source-relative projected mesh
  footprint.

The combined semantic gate consumes this report. A scene with the right asset
counts but reversed faction keeps is therefore a hard failure, independently
of its global style score.

Every Godot lane also compiles into two separately counted resources: a
`Path3D` for movement and a terrain-conforming ribbon mesh for visible packed
dirt. Render acceptance samples pixels along that runtime-projected centerline
and compares them with adjacent shoulder pixels. This avoids the old
false-negative test that searched a guessed top-down rectangle for vaguely
warm pixels.

Alpine acceptance uses the same runtime projection evidence. It crops the
render around the actual elevated massif samples, compares that crop with the
reviewed source region, and requires at least 45% of the source edge density
plus 42% of its local luminance contrast. This prevents a numerically steep
heightmap from passing when it still reads as smooth hills. Caledonia's
reviewed profiles place 24 granite formations per massif plus eight formations
in each of six interior crag fields: 96 evidence-bound crag instances in total.

## Current vertical slice

`concept_batches/caledonia_v1/annotations.json` is a reviewed, evidence-linked interpretation of the supplied overview image. Its source art is vendored in `source/overview.png` and SHA-256 pinned, so a copied batch cannot silently reference a workstation-local Downloads file. Compile it with:

```bash
python3 pipeline/zone_compiler.py concept_batches/caledonia_v1/annotations.json \
  --output concept_batches/caledonia_v1/zone_spec.json \
  --report concept_batches/caledonia_v1/validation_report.json

python3 pipeline/zone_spec_to_godot.py concept_batches/caledonia_v1/zone_spec.json \
  --output concept_batches/caledonia_v1/godot_map_definition.json

python3 pipeline/zone_rasterizer.py concept_batches/caledonia_v1/zone_spec.json \
  --output-dir concept_batches/caledonia_v1/terrain --resolution 1025
```

The raster compiler produces a 16-bit heightmap, RGBA splatmap (grass/road/rock/snow), normal map, water mask, preview, and an artifact manifest. The manifest records actual relief and slope per named landform, so an adapter can fail a batch where an `alpine_massif` was reduced to gentle rolling terrain.

Roads are terrain-graded corridors, not only a splatmap color. After landform
composition, the rasterizer broad-filters the local heightfield and blends that
grade across each authored road width while retaining shoulders outside the
corridor. The deterministic traversal probe then samples the complete lane
polyline, longitudinal and cross-corridor grades, keep spawn neighborhoods, and
objective access against the ZoneSpec `traversal_policy`:

```bash
python3 pipeline/traversal_probe.py \
  concept_batches/caledonia_v1/zone_spec.json \
  concept_batches/caledonia_v1/terrain/heightmap_r16.png \
  --terrain-manifest concept_batches/caledonia_v1/terrain/terrain_manifest.json \
  --output concept_batches/caledonia_v1/terrain/traversal_probe_report.json
```

This is engine-neutral evidence and fails the build before a visually plausible
but mechanically unusable terrain reaches Godot.

## Asset intent and provenance

`asset_profiles` belong to the reviewed ConceptAnnotations document, not the Godot scene. A feature requests a semantic profile such as `highland_conifer_mixed` or `alpine_granite_crags`; the profile defines required tags, a variant count, target placement count, and physical scale. The catalog is built by scanning the project source assets, hashing them, and recording import readiness. This keeps a model from guessing a path or claiming an asset exists when the engine cannot import it.

```bash
python3 pipeline/asset_catalog.py --project-root . --asset-root assets/models \
  --output assets/generated/codeweald_asset_catalog.json

python3 pipeline/asset_plan.py concept_batches/caledonia_v1/zone_spec.json \
  assets/generated/codeweald_asset_catalog.json \
  --output concept_batches/caledonia_v1/asset_plan.json

python3 pipeline/zone_assets_to_godot.py concept_batches/caledonia_v1/asset_plan.json \
  --output concept_batches/caledonia_v1/godot_asset_plan.json
```

The generic `asset_plan.json` remains engine-neutral (`source_path`, format, tags, digest). `godot_asset_plan.json` is the narrow adapter that converts proven import-ready sources into `res://` paths. The Godot scene compiler refuses to use a hard-coded fallback list; missing or unimportable selected assets surface in its build report and fail acceptance.

Before an autonomous build can use the selected models, it invokes Blender in
headless mode. The preflight imports every selected GLB/glTF/FBX/OBJ, requires
real mesh geometry and non-degenerate bounds, enforces any profile-declared
triangle/material floor, records triangle/material/bounds facts, and writes a
thumbnail contact sheet. This is deliberately a gate rather than a cosmetic
preview: a model cannot claim a filename is a usable pine, cliff, or ruin when
Blender cannot import and render it.

```bash
python3 pipeline/asset_visual_preflight.py \
  concept_batches/caledonia_v1/asset_plan.json --project-root . \
  --output-dir concept_batches/caledonia_v1/asset_visual_preflight
```

`build_zone.py` runs this automatically before a Godot candidate is accepted.
Review
`asset_visual_preflight/selected_assets_contact_sheet.png` when changing a
semantic asset profile or when a vision model proposes replacement assets.

Asset profiles can contain multiple **ecological layers**. For example, the
Caledonia forest resolves independent canopy, understory, and forest-floor-rock
families with their own variants, scales, instance counts, and spacing. The
same layer intent is preserved for Godot placement, Unity prefab import, and
Unreal PCG data; a feature is never reduced to a generic “tree scatter.”

## Deterministic generated kits

When the source batch calls for an authored landmark and no suitable licensed
repository asset exists, a Blender source kit is generated first and then
cataloged normally. Current reproducible generators are:

```bash
blender --background --python pipeline/blender_generate_alpine_kit.py -- assets/generated/codeweald_alpine
blender --background --python pipeline/blender_generate_arcane_ruin_kit.py -- assets/generated/codeweald_arcane
blender --background --python pipeline/generate_highland_settlement_kit.py -- assets/generated/codeweald_highland_settlement
blender --background --python pipeline/generate_highland_bridge_kit.py -- assets/generated/codeweald_highland_bridge
```

Each writes a `.blend` authoring source and one or more portable `.glb` files;
the established Alpine and ruin generators also write an FBX sidecar in
`unity/`. Their files are not special engine-only fallbacks. The central
`arcane_objective_ruin` profile uses the GLB in Godot and its verified FBX
sidecar in Unity, and
`capture_zone_objective.gd` provides a close render for landmark review in
addition to the map overview.

## Portable runtime effects

`runtime_effects.json` is generated from reviewed feature semantics during every
one-command build. It declares looped water flow, objective pulses, and foliage
wind without embedding Godot, C#, or Unreal script in ZoneSpec. The Godot scene
compiler consumes this contract to create a flowing water shader, serialized
objective autoplay animation, and deterministic per-instance foliage sway.
Unity and Unreal handoff manifests preserve the same effect payload for their
respective runtime/PCG implementations. The Unity package consumes it as a
point-light pulse, stream-line flow, deterministic prefab-root wind sway, and a
realm-coloured animated banner proxy; Unreal preserves it for the project
runtime/PCG integration.

```bash
godot --path . --rendering-driver opengl3 \
  --script res://pipeline/capture_runtime_effects.gd -- \
  --batch concept_batches/caledonia_v1 \
  --scene res://.codeweald_candidate_moba_3d.tscn
```

This writes two fresh-scene runtime frames plus
`terrain/runtime_effects_acceptance_report.json`. The report requires a
measurable image delta and is run automatically by `build_zone.py --capture`,
so generated motion must survive scene serialization rather than only playing
in the compiler.

## Unity and Unreal handoff manifests

The same ZoneSpec and generic asset plan now emit native-handoff manifests rather than a Godot-only approximation:

```bash
python3 pipeline/zone_to_unity.py concept_batches/caledonia_v1/zone_spec.json \
  concept_batches/caledonia_v1/terrain/terrain_manifest.json \
  concept_batches/caledonia_v1/asset_plan.json \
  --output concept_batches/caledonia_v1/unity_zone_import.json

python3 pipeline/zone_to_unreal.py concept_batches/caledonia_v1/zone_spec.json \
  concept_batches/caledonia_v1/terrain/terrain_manifest.json \
  concept_batches/caledonia_v1/asset_plan.json \
  --output concept_batches/caledonia_v1/unreal_zone_import.json
```

The Unity manifest provides normalized 16-bit height range, Terrain dimensions, splat-layer channels, spline/feature geometry, and verified native-FBX source assets. Its local package creates TerrainData, feature roots, lane/stream lines, deterministic prefab placement, and portable runtime-effect components; see `engine_adapters/unity/README.md`. The Unreal manifest provides 16-bit Landscape height and layer sources, centimeter dimensions, Z scale, and PCG-ready spline/polygon/point feature data. During the one-command build it also resamples the canonical `1025` raster into `terrain/unreal/` at a valid `1009` Landscape layout (`16 x 63` quads plus one shared edge) and writes one grayscale weight PNG per terrain layer. In particular, Unreal's Z scale is derived from the actual terrain span using `height_span_m * 100 / 512`, matching Landscape's documented 512-unit signed height range. The bundled Unreal plugin now contains a compiled Editor module which exposes **Tools > Codeweald > Preflight Zone Manifest** and writes a native acceptance report before it makes any assets. It remains deliberately preflight/import-only until its final LandscapeEditor transaction is exercised in a real UE5 installation; it does not pretend to have created a Landscape actor.

Factions use a portable `highland_fortified_keep` kit rather than a Godot-only
CSG keep. It is selected as a normal `structure`/`fortification` profile, has a
GLB source plus Unity FBX sidecar, and leaves realm-specific banner colour and
motion in the portable runtime-effects contract. This makes landmark geometry
available to every adapter instead of encoding it in one engine's scene script.

Godot consumes `terrain_manifest.json` through `ZoneTerrainBuilder`, not a hard-coded scene. A headless preview can be rendered with:

```bash
godot --headless --path . --script res://pipeline/render_zone_spec_preview.gd
```

The scene compiler writes a temporary Godot candidate from the reviewed
ZoneSpec and selected assets:

```bash
godot --headless --path . --script res://pipeline/build_zone_spec_scene.gd \
  -- --batch concept_batches/caledonia_v1 \
  --output res://.codeweald_candidate_moba_3d.tscn
```

`build_zone.py` now passes its actual batch path to the Godot compiler and
overview capture. Before catalog and asset-plan generation it runs Godot's
headless import scan, so newly generated model and material resources are
loadable and truthfully marked import-ready without opening the editor or
manually reimporting files. No batch identifier is compiled into the
active-scene tool:

```bash
python3 pipeline/build_zone.py concept_batches/jerall_v1/annotations.json --project-root . --capture
```

That command writes and renders a candidate from `jerall_v1`, writes its build
evidence under `concept_batches/jerall_v1/terrain/`, writes an
`evidence_overlay.png` and `evidence_report.json` beside the reviewed
annotations, and atomically replaces `moba_3d.tscn` only after asset, topology,
overview-projection, perspective, deterministic traversal, native-navigation,
semantic, and strict visual gates all pass. Failed candidates do not disturb
the active scene.

During an autonomous art-development loop, use the explicit preview mode:

```bash
python3 pipeline/build_zone.py concept_batches/caledonia_v1/annotations.json \
  --project-root . --capture --preview-active-on-failure
```

This still returns a failed build and preserves the red acceptance reports, but
copies the best measured candidate over `moba_3d.tscn` so the already-open Godot
editor reloads the latest map without manual scene selection. Its build report
says `preview_promoted_with_failed_acceptance`; it is never confused with
production promotion.

Evidence is a compiler contract, not a suggestion: every feature must cite a
declared, hash-pinned source image, an ordered normalized rectangle, and a
non-empty explanation. The automatically generated contact sheet draws each
feature's evidence rectangle on the exact input art, making bad model grounding
visible before terrain, assets, or an engine scene are accepted.

It creates external heightmap collision, atmosphere and lighting, faction keeps,
evidence-bound settlements and streams, and deterministic conifer/crag
placement from real GLB assets. Caledonia currently compiles 4,676 ecological
instances and 96 crags as 4,772 logical placements. All 2,860 canopy placements
have observed `lod0`/`lod1`/`lod2` render siblings, producing 10,492 serialized
render transforms split across deterministic 480 m cells (607 batches and at
most 89 transforms per cell in the current batch). The build report is
collected from a reload of the saved scene and proves every tier has complete,
correctly sized serialized buffers; in-memory counts alone cannot pass. The
terrain mesh and heightmap collision are compressed external resources, leaving
the generated text scene around 9.7 MB instead of 109.6 MB. It also includes ten generated Highland settlements, eight
waterways, 17 derived full-lane stone bridges, two keeps, and one objective
ruin. Bridge sockets are exact intersections of reviewed lane and stream
polylines; each retains both evidence chains and must resolve through the normal
catalog/preflight contract. It reports a missing selected
asset as a warning; it never substitutes a random primitive while claiming the
intended asset was used.

The candidate compiler also serializes a native `NavigationRegion3D` from the
generated heightfield and the ZoneSpec traversal policy. Caledonia currently
produces 2,501 navigation vertices and 2,133 polygons at a 40 m build cell.
Capture instantiates the candidate and queries the real Godot
`NavigationServer3D`; acceptance requires the keep-to-keep route and both
keep-to-objective routes, plausible detours, and bounded start/end anchor
snapping. `NavigationPath3D` lane nodes remain inspectable semantic references,
but they are not accepted as proof that an agent can traverse the native map.

For alpine dressing, the compiler samples a seeded candidate pool inside each
reviewed massif, ranks candidates by generated terrain height, and honors the
profile's spacing before placement. Semantic acceptance requires every planned
alpine instance; a partial scatter is a failed build, not a successful map.

Run rendered review before semantic acceptance:

```bash
python3 pipeline/visual_acceptance.py \
  concept_batches/caledonia_v1/zone_spec.json \
  concept_batches/caledonia_v1/source/overview.png \
  concept_batches/caledonia_v1/terrain/godot_active_scene.png \
  --projection-report concept_batches/caledonia_v1/terrain/godot_overview_projection.json \
  --output concept_batches/caledonia_v1/terrain/visual_acceptance_report.json
```

This integrity gate rejects empty/black/clipped renders and verifies that
reviewed landforms, forest, and lanes have readable visual structure. It is not
a false claim of pixel-perfect art matching.

```bash
python3 pipeline/zone_acceptance.py concept_batches/caledonia_v1/zone_spec.json \
  concept_batches/caledonia_v1/terrain/terrain_manifest.json \
  --godot-build concept_batches/caledonia_v1/terrain/godot_build_report.json \
  --visual-report concept_batches/caledonia_v1/terrain/visual_acceptance_report.json \
  --overview-projection-report concept_batches/caledonia_v1/terrain/overview_projection_acceptance_report.json \
  --traversal-report concept_batches/caledonia_v1/terrain/traversal_probe_report.json \
  --navigation-report concept_batches/caledonia_v1/terrain/navigation_acceptance_report.json \
  --runtime-effects-report concept_batches/caledonia_v1/terrain/runtime_effects_acceptance_report.json \
  --output concept_batches/caledonia_v1/terrain/acceptance_report.json
```

The combined gate rejects missing source evidence, a mirrored overview, lost
lanes/keeps/waterways/settlements/objective ruins, an Alpine profile that no
longer has steep relief, unsafe lane grades, unavailable native routes,
incomplete material tokens, a visually broken render, or a Godot scene that
claims but cannot place its selected asset families.

The Godot adapter is intentionally a compatibility adapter for the existing `CaledoniaMapDefinition` builders. It is the first consumer of ZoneSpec, not the definition of ZoneSpec. Unity and Unreal adapters must consume the same canonical JSON and emit their native terrain, spline, foliage, navigation, and placement resources.

## Native engine targets (researched July 26, 2026)

The shared artifact is always `ZoneSpec`; raster and scene files are disposable adapter outputs. This keeps the semantic zone editable even when a project changes engine.

| Engine | Adapter output | Native final authority |
| --- | --- | --- |
| Godot 4.7 | map-definition JSON, `.glb` props, generated static terrain meshes | `ArrayMesh`/`SurfaceTool` geometry and `NavigationMesh`; Godot recommends glTF 2.0 for 3D scene interchange. |
| Unity | 16-bit height tiles, control masks, C# placement manifest | `TerrainData` plus splines/instancing; batch height writes must avoid recomputing LOD after each edit. |
| Unreal Engine 5 | 16-bit grayscale PNG or tiled heightmaps, landscape layer masks, PCG attributes | Landscape plus PCG spatial data (heightfields, splines, points, and volumes). |

Adapters own coordinate conversion and native import quirks. ZoneSpec uses centered, right-handed X/Z ground coordinates with Y up. Unreal's documented landscape import supports 16-bit grayscale PNG and tiled heightmaps; its Z-scale is explicit. Unity's terrain API stores normalized height samples and its height APIs can trigger expensive LOD/vegetation rebuilds, so adapters should write in batches. Godot supports static custom geometry through `ArrayMesh`/`SurfaceTool`, while imported props should use glTF/GLB rather than treating OBJ or FBX as the portable interchange contract.

Reference: [Godot 3D scene import](https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_3d_scenes/index.html), [Godot generated geometry](https://docs.godotengine.org/en/stable/tutorials/3d/introduction_to_3d.html), [Unity TerrainData.SetHeights](https://docs.unity3d.com/ja/2022.2/ScriptReference/TerrainData.SetHeights.html), [Unreal landscape heightmaps](https://dev.epicgames.com/documentation/en-us/unreal-engine/importing-and-exporting-landscape-heightmaps-in-unreal-engine), and [Unreal PCG data types](https://dev.epicgames.com/documentation/en-us/unreal-engine/procedural-content-generation-framework-data-types-reference-in-unreal-engine).

## Acceptance evidence for a concept batch

1. Every gameplay-relevant region has a declared source image, a valid evidence region, and an inspectable generated overlay.
2. The ZoneSpec compiles deterministically from reviewed annotations.
3. The report identifies all retained uncertainty; there are no unreviewed low-confidence topology features.
4. An engine adapter imports the same ZoneSpec, preserves semantic landform profiles, and produces a scene plus a validation capture.
5. A comparison pass checks the rendered scene against the concept's declared silhouettes, road/river topology, POIs, biome coverage, and traversal constraints.

Godot performs this build, capture, acceptance, and promotion loop
automatically. Cross-engine handoff generation is dormant and opt-in while the
Godot pipeline is brought to production fidelity.
