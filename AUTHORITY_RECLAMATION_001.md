# WGE Authority Reclamation Audit

Status: complete audit baseline

Date: 2026-09-27

Scope: Python authority in the WGE runtime and development pipeline

## Executive finding

Python is load-bearing in WGE today. That is not because Python is present, or
because a Python file happens to be large. It is because Python currently owns
several decisions that must become durable, typed, deterministic engine
authority:

- the production build graph and stage ordering;
- semantic lowering from reviewed concept annotations into ZoneSpec;
- the Python-shaped authoring language and semantic IR;
- acceptance policy and promotion decisions;
- canonical terrain rasterization and derived terrain fields;
- asset identity, physical acceptance, and placement intent;
- collision, navigation, boundary, and traversal intent;
- parts of the visual acceptance contract.

The original architecture is still visible in the repository. Python was
intentionally chosen for model-facing authoring and provider integration,
Julia for numerical terrain work, and Rust for contracts and identity. The
problem is authority drift: implementation that should be Rust or Julia has
accumulated in Python and is now called directly by the one-command build.

The correction is not to remove Python. Python remains a good model ergonomics
surface and a good home for Blender, Gaea, ComfyUI, network, image, and target
engine adapters. The correction is to ensure that Python cannot silently
define canonical semantics, acceptance, identity, or numerical world state.

## Ground truth used

This inventory is based on the current WGE checkout, not on the roadmap alone.
The relevant architectural statements are in README.md:

- Authoring DSL: Python-shaped, parsed and never executed.
- Compiler and pipeline: currently Python.
- Contracts and identity: Rust.
- Solvers: Julia.
- Backends: Godot, Unreal, Unity, and Bevy.

The production choke point is pipeline/build_zone.py. It imports and orders
most of the pipeline, invokes Julia workers, invokes the Rust worldspec
tooling, writes stage artifacts, and decides which acceptance gates run.
That file is an orchestration symptom, not the only problem.

Current implementation inventory:

- 79 Python implementation modules under pipeline, totaling 26,848 physical
  lines.
- 12 Rust source files under world_core, totaling 9,754 lines.
- 5 Julia source files under terrain_lab, totaling 953 lines.
- 42 Python test modules under tests. Tests are not runtime authority, but
  their assertions must migrate with the behavior they specify.

The current test baseline before this documentation-only change is 654 passing
Python tests, including the generated-GLB asset-intake benchmark. The
asset-intake report is a useful migration fixture:
asset_intake_reports/sample_2026-09-26T091412.074.intake.json.

## Authority policy

The target boundary is deliberately strict.

| Concern | Target authority | Python may do |
| --- | --- | --- |
| WorldSpec, semantic IR, IDs, hashes, receipts | Rust | Parse input and transport records |
| Promotion, acceptance, repair classes, gate composition | Rust | Run provider-specific measurements |
| Terrain arrays, erosion, hydrology, ecology, spatial solves | Julia | Launch workers and encode results |
| Numerical result schemas and artifact manifests | Rust | Serialize provider output |
| Blender, Gaea, ComfyUI, DEM/network, image codecs | External tool plus thin Python adapter | Own the adapter and raw evidence |
| Godot, Unreal, Unity, Bevy process integration | Target engine plus adapter | Launch, capture, package, and collect logs |
| Developer audits and exploratory metrics | Python is acceptable | Must not promote runtime artifacts |

A useful test is: if deleting the Python module would allow a model to produce
a different canonical WorldSpec, different world hash, different placement
decision, or different acceptance verdict, that module is still authoritative
and belongs in the reclamation queue.

The other useful test is: a 150-line Python wrapper around Blender is normal;
a 2,644-line Python rasterizer that writes canonical terrain artifacts is not.

## Priority definitions

- P0: current Python authority blocks semantic ownership. Freeze new scope and
  migrate before adding comparable features.
- P1: semantic or numerical authority that can be migrated after the P0
  contracts are stable.
- P2: split or adapter boundary. First isolate the authoritative portion,
  then decide whether the remaining worker stays Python.
- P3: safe to keep as fenced glue or developer tooling. No urgent port.
- RETIRE: deprecated or exploratory path; remove after confirming no active
  caller.

## Complete Python authority inventory

Every runtime Python module under pipeline is classified below. “Current
authority” describes what the module does today, not what its name implies.
“Target” describes the durable owner after reclamation.

### Asset and content pipeline

| Module | LOC | Current authority | Disposition | Target | Priority |
| --- | ---: | --- | --- | --- | --- |
| pipeline/asset_affordance.py | 231 | Defines and re-measures declared asset affordances | Reclaim | Rust records, validation, and acceptance | P1 |
| pipeline/asset_catalog.py | 106 | Indexes available assets and resolves catalog identity | Reclaim | Rust catalog and canonical asset IDs; Python filesystem scan may remain | P1 |
| pipeline/asset_intake.py | 1067 | Parses GLB structure, derives bounds/topology/material facts, and emits intake identity | Reclaim and split | Rust GLB facts and report schema; Python may remain as a provider worker during transition | P1 |
| pipeline/asset_parts.py | 175 | Determines separable solid parts from glTF data | Reclaim | Rust deterministic geometry/topology analysis | P1 |
| pipeline/asset_physical_acceptance.py | 236 | Decides whether an asset is physically acceptable for use | Reclaim | Rust acceptance policy over measured facts | P1 |
| pipeline/asset_plan.py | 201 | Resolves asset intent against the catalog and chooses placement-ready assets | Reclaim | Rust asset planning and typed resolution | P1 |
| pipeline/asset_visual_preflight.py | 190 | Orchestrates Blender checks and collects visual preflight verdicts | Split | Python Blender worker; Rust evidence schema and verdict policy | P2 |
| pipeline/blender_asset_probe.py | 182 | Reads meshes and materials through bpy | Fence as adapter | Blender worker; Rust validates the returned facts | P3 |
| pipeline/blender_generate_alpine_kit.py | 126 | Generates an asset kit through Blender | Fence as adapter | Blender worker invoked through a provider contract | P3 |
| pipeline/blender_generate_arcane_ruin_kit.py | 139 | Generates an asset kit through Blender | Fence as adapter | Blender worker invoked through a provider contract | P3 |
| pipeline/highland_building_geometry.py | 253 | Pure deterministic building geometry, independent of bpy | Reclaim | Rust geometry generator or typed mesh service | P2 |
| pipeline/highland_keep_geometry.py | 370 | Pure deterministic keep geometry, independent of bpy | Reclaim | Rust geometry generator or typed mesh service | P2 |
| pipeline/material_catalog.py | 122 | Defines material bundle and shader-facing catalog data | Split | Rust material identity and constraints; Python provider metadata may remain | P2 |
| pipeline/material_lane.py | 262 | Dispatches material generation providers and selects results | Split | Rust provider intent and selection; Python provider adapter | P2 |
| pipeline/procedural_surface.py | 148 | Produces procedural surface parameters and material fields | Split | Julia for numerical fields; Rust contract; Python image/provider glue | P3 |
| pipeline/terrain_material_bake.py | 149 | Bakes terrain material layers and derived material artifacts | Split | Julia numeric blend/field work; Rust material artifact contract; Python image IO may remain | P2 |
| pipeline/texture_material_pipeline.py | 306 | Coordinates texture and PBR generation | Fence as adapter | Rust material contract plus Python/image provider workers | P3 |
| pipeline/comfy_generate_texture.py | 95 | Calls ComfyUI to generate texture content | Fence as adapter | ComfyUI provider; Rust validates and records the result | P3 |
| pipeline/generate_granite_material.py | 83 | Calls a material-generation path for granite | Fence as adapter | Provider worker with Rust material receipt | P3 |
| pipeline/generate_alpine_cliffs.py | 183 | Generates Blender cliff geometry | Fence as adapter | Blender provider worker | P3 |
| pipeline/generate_highland_bridge_kit.py | 176 | Generates Blender bridge geometry | Fence as adapter | Blender provider worker | P3 |
| pipeline/generate_highland_building_kit.py | 374 | Generates Blender building geometry | Fence as adapter | Blender provider worker; Rust validates asset facts | P3 |
| pipeline/generate_highland_foliage.py | 150 | Generates Blender foliage content | Fence as adapter | Blender provider worker; Rust ecology/placement contract | P3 |
| pipeline/generate_highland_keep_kit.py | 202 | Generates Blender keep geometry | Fence as adapter | Blender provider worker; Rust validates asset facts | P3 |
| pipeline/generate_highland_settlement_kit.py | 330 | Generates Blender settlement geometry | Fence as adapter | Blender provider worker; Rust validates asset facts | P3 |

The important distinction in this group is between geometry and Blender. The
Blender scripts are legitimate external-tool adapters. The pure geometry
modules and the asset facts they produce are semantic and deterministic, so
they should not remain Python-owned merely because the final mesh is later
written by Blender.

### Semantic intake, compilation, and acceptance

| Module | LOC | Current authority | Disposition | Target | Priority |
| --- | ---: | --- | --- | --- | --- |
| pipeline/worldbuilder_dsl.py | 786 | Defines the Python-shaped authoring surface and its lowering behavior | Reclaim with compatibility facade | Rust parser/AST/lowering; Python surface may emit the same wire format | P0 |
| pipeline/wge_language_kernel.py | 494 | Parses source and exposes language-kernel semantics | Reclaim | Rust language kernel | P0 |
| pipeline/wge_semantic_ir.py | 195 | Defines canonical intermediate representation and normalization | Reclaim | Rust semantic IR and canonicalization | P0 |
| pipeline/zone_compiler.py | 1334 | Converts reviewed annotations and inputs into ZoneSpec | Reclaim | Rust semantic compiler | P0 |
| pipeline/zone_acceptance.py | 629 | Composes acceptance gates and decides whether a zone is promotable | Reclaim | Rust gate graph and promotion policy | P0 |
| pipeline/concept_batch_intake.py | 290 | Ingests concept batches, source roles, and annotation inputs | Split | Python file/provider IO; Rust source records and intake semantics | P1 |
| pipeline/image_reconciliation.py | 153 | Assigns image roles and reconciles concept evidence | Reclaim | Rust evidence graph and reconciliation policy; Python codec/provider glue | P1 |
| pipeline/derive_arena_annotations.py | 407 | Derives semantic annotations from image and authored evidence | Reclaim | Rust typed annotation derivation; provider output remains external evidence | P1 |
| pipeline/vision_annotation_adapter.py | 199 | Adapts vision-provider output into WGE annotations | Split | Python provider protocol; Rust claim validation and promotion | P1 |
| pipeline/zone_runtime_effects.py | 83 | Defines runtime effect intent for a compiled zone | Reclaim | Rust runtime-effect contract | P1 |
| pipeline/wge_critic.py | 624 | Audits metrics and emits repair suggestions | Fence as tooling | Rust registered repair classes consume the suggestions; Python remains an audit UI/tool | P2 |

The Python-shaped DSL is not itself a mistake. It is the model ergonomics
layer. The mistake would be allowing the convenience frontend to be the only
implementation of the semantic compiler. The compatibility rule should be:
Python can parse or construct requests, but Rust owns the meaning of those
requests and produces the canonical IR, WorldSpec, receipts, and hash.

### Terrain, fields, and spatial planning

| Module | LOC | Current authority | Disposition | Target | Priority |
| --- | ---: | --- | --- | --- | --- |
| pipeline/zone_rasterizer.py | 2644 | Rasterizes ZoneSpec into canonical heightfields, masks, and terrain artifacts | Reclaim | Julia numerical kernel plus Rust artifact contract | P0 |
| pipeline/erosion.py | 514 | Computes erosion and terrain change numerically | Reclaim | Julia terrain solver | P1 |
| pipeline/hydrology.py | 500 | Computes drainage, flow, basins, and water-related fields | Reclaim | Julia hydrology solver | P1 |
| pipeline/site_conditions.py | 400 | Computes shared derived terrain/site fields | Reclaim | Julia field kernel; Rust typed output contract | P1 |
| pipeline/forestry.py | 325 | Computes vegetation/ecology fields and forestry placement inputs | Reclaim | Julia ecology/field solver; Rust placement contract | P1 |
| pipeline/massif_character.py | 182 | Computes terrain character and massif descriptors | Reclaim | Julia terrain analysis | P1 |
| pipeline/siting.py | 272 | Computes spatial siting and placement decisions | Reclaim | Julia spatial solver; Rust validates decisions | P1 |
| pipeline/boundary_plan.py | 593 | Derives playable boundaries, masks, and boundary intent | Split and reclaim | Julia spatial field calculation; Rust boundary policy and manifest | P1 |
| pipeline/navigation_plan.py | 670 | Derives navigable surfaces, clearances, and navigation intent | Split and reclaim | Julia spatial calculation; Rust navigation contract | P1 |
| pipeline/traversal_probe.py | 526 | Probes traversability and produces gameplay traversal evidence | Split and reclaim | Julia probe computation; Rust traversal verdict and receipt | P1 |
| pipeline/collision_plan.py | 414 | Defines collision surfaces, layers, and collision intent | Reclaim | Rust collision contract; Julia may provide geometry measurements | P1 |
| pipeline/collision_acceptance.py | 228 | Accepts or rejects collision geometry and collision behavior | Reclaim | Rust collision acceptance gate | P1 |
| pipeline/navmesh_acceptance.py | 117 | Applies engine-neutral navigation acceptance rules | Reclaim | Rust navigation acceptance gate | P1 |
| pipeline/navigation_acceptance.py | 173 | Runs Godot-specific navigation connectivity checks | Split | Python/Godot adapter; Rust consumes portable evidence and owns verdict | P2 |
| pipeline/import_heightfield.py | 274 | Imports, resamples, and normalizes heightfields | Split and reclaim | Julia numeric resampling; Rust import manifest and identity | P1 |
| pipeline/import_dem.py | 310 | Reads DEM data, handles geospatial metadata, and converts elevation arrays | Split | Python GeoTIFF/CRS IO; Julia conversion and field math; Rust manifest | P2 |
| pipeline/fetch_dem.py | 146 | Fetches DEM data over the network | Fence as adapter | Network/provider adapter; Rust records source and checksum | P3 |
| pipeline/gaea_crop.py | 520 | Crops and converts Gaea heightfield output into terrain artifacts | Split | Python/Gaea IO; Julia numerical conversion; Rust artifact contract | P2 |
| pipeline/gaea_terrain.py | 439 | Edits and invokes Gaea graphs | Fence as adapter | Gaea provider adapter; Rust records graph inputs and outputs | P3 |
| pipeline/preview_metrics.py | 225 | Computes terrain preview metrics | Split and reclaim | Julia measurement kernel; Rust metric schema and thresholds | P2 |
| pipeline/silhouette.py | 446 | Computes skyline and silhouette metrics | Split and reclaim | Julia image/terrain measurement; Rust acceptance policy | P2 |

This is the largest semantic Python tumor. A Python numerical worker can be
useful while the migration is staged, but it must not write the artifact that
the rest of the build treats as canonical without a Rust receipt and a
versioned Julia-compatible contract.

### Visual acceptance and presentation evidence

| Module | LOC | Current authority | Disposition | Target | Priority |
| --- | ---: | --- | --- | --- | --- |
| pipeline/visual_acceptance.py | 615 | Measures rendered images and applies visual acceptance checks | Split | Python/NumPy measurement worker; Rust threshold policy and promotion verdict | P2 |
| pipeline/bevy_visual_acceptance.py | 403 | Applies Bevy-specific visual integrity checks | Split | Bevy capture adapter; Rust portable verdict and receipt | P2 |
| pipeline/foliage_projection_acceptance.py | 201 | Checks foliage projection and presentation constraints | Split | Python image measurement; Rust policy | P2 |
| pipeline/overview_projection_acceptance.py | 251 | Compares runtime overview projection against concept evidence | Split | Python image measurement; Rust evidence/policy contract | P2 |
| pipeline/perspective_acceptance.py | 90 | Checks player-scale perspective capture conditions | Split | Capture adapter plus Rust acceptance rule | P2 |
| pipeline/style_reference.py | 251 | Extracts and stores style-reference measurements | Split | Python image measurement; Rust style contract and identity | P2 |
| pipeline/style_calibration.py | 116 | Calibrates render style against references | Split | Provider/render worker; Rust calibrated style record | P2 |
| pipeline/evidence_overlay.py | 132 | Produces visual evidence overlays for inspection | Fence as presentation glue | Python/image tool; never a source of canonical semantics | P3 |
| pipeline/metric_honesty.py | 254 | Performs adversarial checks on whether visual metrics are meaningful | Fence as tooling | Python audit tool; results can inform Rust gate design but cannot promote artifacts | P3 |

Visual measurement is a legitimate Python/NumPy worker boundary. Visual
acceptance is not legitimate Python authority when it changes promotion. The
worker should emit measurements and raw evidence; Rust should own the rule,
threshold, gate identity, and final receipt.

### Build orchestration and backend adapters

| Module | LOC | Current authority | Disposition | Target | Priority |
| --- | ---: | --- | --- | --- | --- |
| pipeline/build_zone.py | 1502 | Orders the production build, invokes all stages, and controls which artifacts become deliverable | Reclaim orchestration | Rust build graph/CLI; Python remains process/provider glue | P0 |
| pipeline/build_viewer.py | 56 | Builds the Bevy viewer | Fence as adapter | Bevy build wrapper | P3 |
| pipeline/capture_bevy.py | 283 | Launches Bevy and captures runtime evidence | Fence as adapter | Bevy capture wrapper with Rust receipt | P3 |
| pipeline/zone_assets_to_godot.py | 125 | Converts planned assets into Godot resources | Fence as adapter | Godot adapter; Rust supplies typed asset plan | P3 |
| pipeline/zone_spec_to_godot.py | 130 | Converts ZoneSpec into Godot scene/runtime artifacts | Fence as adapter | Godot adapter; Rust supplies canonical spec | P3 |
| pipeline/zone_to_unity.py | 167 | Emits Unity import/manifest artifacts | Fence as adapter | Unity adapter; Rust supplies canonical manifest | P3 |
| pipeline/zone_to_unreal.py | 129 | Emits Unreal import/manifest artifacts | Fence as adapter | Unreal adapter; Rust supplies canonical manifest | P3 |
| pipeline/unreal_artifacts.py | 62 | Packages Unreal artifacts | Fence as adapter | Unreal packaging adapter | P3 |
| pipeline/tool_provenance.py | 43 | Records tool versions and hashes | Reclaim | Rust provenance/receipt records; Python may collect raw versions | P1 |
| pipeline/wge_kernel_demo.py | 96 | Demonstrates the semantic kernel through a Python CLI | Fence or retire | Rust CLI demo; retain Python only as an example harness | P3 |

The orchestrator is P0 because it is the current authority boundary, but it
should be migrated last among the P0 items. Replacing it before the contracts
exist would only move the same implicit policy into another file.

### Audit, exploratory, and deprecated paths

| Module | LOC | Current authority | Disposition | Target | Priority |
| --- | ---: | --- | --- | --- | --- |
| pipeline/reachability.py | 467 | Audits whether parameters can reach observable outputs | Fence as tooling | Python audit suite; no production promotion | P3 |
| pipeline/sensitivity.py | 415 | Computes sensitivity matrices for terrain/style parameters | Fence as tooling | Python or Julia analysis tool; no production authority | P3 |
| pipeline/minimap_to_map_spec.py | 92 | Deprecated exploratory conversion path | Retire | Remove after caller audit | RETIRE |

These files can remain Python without compromising engine authority as long as
their output is explicitly diagnostic and cannot be silently consumed as a
certified WorldSpec or acceptance verdict.

## Reclamation summary

The exact module-level inventory above is the source of truth. The following
summary groups it by action, not by current line count:

| Action | Meaning | Modules |
| --- | --- | ---: |
| P0 | Direct semantic, canonical rasterizer, promotion, or build authority | 7 |
| P1 | Asset, evidence, numerical, spatial, collision, navigation, or provenance authority | 25 |
| P2 | Split boundary or deferred reclamation | 19 |
| P3 | Legitimate external-tool adapter or developer tooling | 27 |
| RETIRE | Deprecated path | 1 |

The boundary counts are intentionally more useful than a Python-vs-Rust line
count. A P2 split is not an immediate rewrite request: it is a requirement to
separate measurement or provider execution from the semantic decision it feeds.
The P3 set is not a backlog of pointless ports.

## Recommended execution slices

### Slice 0: freeze and prove the boundary

1. Declare the authority policy above as a repository invariant.
2. Add a build-time inventory check that rejects new canonical artifacts whose
   only producer is an unregistered Python module.
3. Give every cross-language artifact a schema version, producer identity,
   input digest, and receipt.
4. Keep the current Python test suite as the behavioral oracle while adding
   cross-language golden vectors.

Exit condition: the existing build still works, but each stage can say whether
it is a semantic owner, a numerical worker, or an adapter.

### Slice 1: semantic kernel and DSL compatibility

Move wge_semantic_ir.py, wge_language_kernel.py, and the semantic core of
worldbuilder_dsl.py into Rust. Keep a Python-shaped compatibility facade that
serializes the same requests. Then move the semantic portion of
zone_compiler.py.

Do not preserve Python object identity or Python-specific evaluation behavior.
Preserve only the documented source syntax and canonical output vectors.

Exit condition: identical input produces byte-identical canonical IR and
WorldSpec from Python facade and Rust CLI, with identical diagnostics.

### Slice 2: asset identity and the supplied GLB

Use the generated sample GLB and its intake report as the first concrete
migration fixture. Move the deterministic GLB facts, part analysis, catalog
identity, affordances, asset plan, and physical acceptance into Rust. Python
may continue to invoke Blender for measurements that require bpy.

Exit condition: the Rust path reproduces the benchmark's canonical asset
facts, stable IDs, and acceptance verdict, and rejects tampered input or
changed source digest.

### Slice 3: numerical terrain seam

Move the canonical numerical core of zone_rasterizer.py behind a Julia worker
contract, then migrate erosion, hydrology, site conditions, massif character,
forestry, and siting. Rust should validate dimensions, domains, checksums,
solver version, and field provenance, but not duplicate array math.

Exit condition: Julia produces versioned field artifacts accepted by Rust,
golden vectors cover edge cases, and Python is only launching/serializing the
worker.

### Slice 4: spatial gameplay contracts

Move boundary, navigation, collision, and traversal intent into typed Rust
contracts with Julia doing spatial calculations where appropriate. Keep the
Godot navigation check as an adapter until the portable verdict is stable.

Exit condition: a backend-independent acceptance receipt can be generated
before any Godot or Bevy process is launched.

### Slice 5: acceptance and promotion

Move zone_acceptance.py, collision_acceptance.py, navmesh_acceptance.py, and
the policy portion of visual acceptance into Rust. Python and engine workers
emit raw measurements and evidence only.

Exit condition: changing a Python threshold or gate-composition branch cannot
change a certified result because no such branch remains in Python.

### Slice 6: replace the orchestrator

Implement a Rust build graph/CLI that consumes the typed semantic, terrain,
asset, spatial, and acceptance contracts. Retain Python wrappers for Blender,
Gaea, ComfyUI, network retrieval, image codecs, and target-engine processes.
Only after the Rust graph is authoritative should build_zone.py become a thin
compatibility launcher or be retired.

Exit condition: the same stage receipts, hashes, and final world identity are
produced without importing Python semantic modules.

## Non-goals and anti-patterns

- Do not port Blender bpy calls to Rust. Fence them behind a provider contract.
- Do not port network or GeoTIFF/CRS convenience code solely to reduce the
  Python line count.
- Do not port image capture and engine process management before the acceptance
  contract is stable.
- Do not rewrite build_zone.py first. It will conceal missing contracts and
  reproduce the tumor in a new language.
- Do not treat passing Python tests as proof that Python no longer owns the
  behavior. Add Rust and Julia golden vectors at every seam.
- Do not let a vision model's raw annotation become a certified fact without a
  typed provenance record and a Rust promotion decision.
- Do not let generated visual evidence become canonical world state.

## First implementation recommendation

The first production slice should be the supplied GLB intake followed by the
semantic IR seam. Asset intake is already deterministic and tested, and the
GLB gives the migration a concrete artifact rather than an abstract rewrite.
The semantic IR then establishes the rule that every later worker and adapter
must follow: Python can supply evidence and requests, but Rust owns meaning,
identity, and promotion.

The full terrain port should follow once the contract is exercised by a real
asset and real WorldSpec. That sequence minimizes the risk of building a
beautiful Rust/Julia shell around still-implicit Python semantics.

## Audit conclusion

The diagnosis is confirmed: Python has become partially load-bearing in WGE,
but it is not universally a tumor. Thirty-two P0/P1 modules are direct
authority that should be reclaimed by Rust or Julia, nineteen P2 modules need a
contract split, and twenty-seven P3 modules are valuable integration surface
that should be explicitly fenced and retained.

The right end state is a model-native game-development harness with:

- a Python-shaped ergonomic request surface;
- a Rust semantic compiler, identity system, receipt ledger, gate system, and
  build graph;
- Julia numerical and spatial solvers;
- Python providers for the tools models cannot use through a typed native
  contract;
- backend adapters that are replaceable without changing world meaning.

That architecture preserves the original Rust/Julia intent while keeping the
model-facing ergonomics that made Python attractive in the first place.
