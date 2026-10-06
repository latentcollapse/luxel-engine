# Luxel Repository Topology and Ownership Inventory

Status: **C0.1 inventory**, 2026-09-30  
Starting source checkpoint: `c19a25c` (`record native graphics handoff`)  
Worktree status: intentionally dirty; existing user/Codex changes are preserved and are not classified as a clean commit

This is the first executable artifact of [Luxel Demo-Ready Native Engine Mega-Sprint](../archive/2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md). It distinguishes the current native engine spine from provider machinery, salvageable historical algorithms, external-engine compatibility paths, and generated output. It is an inventory, not a deletion or migration plan.

## Classification rules

Every path is assigned one primary zone:

```text
ACTIVE_CANONICAL  current Luxel authority, runtime, execution, or model-facing path
PROVIDERS         optional build-time/tooling boundary that returns typed artifacts
LEGACY_REFERENCE  useful historical algorithm, regression lane, or predecessor contract
ARCHIVE_COMPAT    external-engine or obsolete delivery path; never current authority
GENERATED_OR_CACHE output, build products, caches, archives, or evidence bulk
```

“Active” does not mean “complete.” It means the path is allowed to participate in the current native architecture. “Legacy” does not mean “worthless”; it means the path must not become an accidental dependency of canonical state.

## Active canonical spine

### Rust workspace

The authoritative Rust workspace is `world_core/` and its workspace members in `world_core/Cargo.toml`:

| Path | Role |
| --- | --- |
| `world_core/crates/intake_repair_contract/` | provider-neutral intake, evidence-bound repair, and repair contract |
| `world_core/crates/project_ledger/` | typed project specification, artifact graph, evidence, snapshot, and compatibility delivery metadata; Unity-specific compatibility still requires fencing |
| `world_core/crates/asset_contract/` | bounded GLB facts, runtime asset preparation, and rejection controls |
| `world_core/crates/gameplay_contract/` | deterministic gameplay/GCS contracts, abilities/effects, live runtime state, and replay data |
| `world_core/crates/reference_runtime/` | engine-neutral world compilation, physics seam, gameplay binding, and reference inspection provenance |
| `world_core/crates/certification_authority/` | independent Rust receipt registry, validators, and promotion authority |
| `world_core/crates/live_evidence_contract/` | Tier-A/Tier-B live-session evidence contract |
| `world_core/crates/native_graphics_contract/` | Rust-owned graphics packet lowering, native Lava supervision, window path, visual evidence, and campaign profiles |
| `world_core/crates/luxel_control_plane/` | canonical project transaction, candidate, evidence, repair, promotion, rollback, and agent operation authority |
| `world_core/apps/world_viewer/` | Bevy/reference inspection instrument with Rust provenance validation; not the canonical Lava representation |

The following workspace members are present but are not the current semantic control-plane authority:

| Path | Classification | Reason |
| --- | --- | --- |
| `world_core/crates/semantic_kernel/` | `LEGACY_REFERENCE` / compatibility | older lane-overlap semantic transaction specimen; retained for regression and vocabulary history |
| `world_core/crates/worldspec/` | `LEGACY_REFERENCE` / compatibility | predecessor Codeweald/WorldSpec contract; useful history and tests, not the current project pointer |

The workspace is therefore active as a build boundary, but not every crate in it is equally canonical. Campaign 0.2 must make that distinction visible to build/test discovery and documentation.

### Julia/Lava execution

| Path | Role |
| --- | --- |
| `graphics_lab/Project.toml` | pinned graphics project declaration |
| `graphics_lab/Manifest.toml` | resolved graphics dependency lock |
| `graphics_lab/src/LavaAdapter.jl` | coarse typed packet adapter, resource/runtime boundary, and geometry/culling execution |
| `graphics_lab/src/LuxelGraphics.jl` | graphics-side packet lowering/execution helpers |
| `graphics_lab/bin/luxel_graphics_worker.jl` | supervised long-lived graphics worker entry point |
| `graphics_lab/test/` | Julia graphics adapter and worker tests |
| `terrain_lab/Project.toml` / `Manifest.toml` | pinned terrain project and lock |
| `terrain_lab/src/CodewealdTerrainLab.jl` | numerical/spatial field execution |
| `terrain_lab/src/Erosion.jl` | numerical erosion machinery; active execution candidate but some comments/paths retain legacy naming |
| `terrain_lab/bin/` and `terrain_lab/test/` | terrain worker entry points and tests |

Julia/Lava is execution machinery behind typed Rust-owned packets. It is not semantic authority and must not own canonical project identity.

### Current model-facing/control surface

| Path | Ownership | Role |
| --- | --- | --- |
| `pipeline/luxel_agent_surface.py` | transport | bounded JSON-RPC/MCP-shaped model surface; delegates all semantic decisions to `luxel-control-plane` |
| `pipeline/luxel_neura_mcp.py` | transport | stable Neura-MCP launcher over the agent surface |
| `pipeline/luxel_native_transaction.py` | transport/orchestration | passes typed project/candidate/evidence requests to Rust transaction commands |
| `pipeline/luxel_source_intake.py` | transport/staging | stages caller-owned source bytes and typed provider interpretation; Rust assigns/validates identity |
| `pipeline/luxel_engine_neutral.py` | orchestration | stages the current engine-neutral certification flow; Rust owns measurement, repair, identity, and promotion |
| `pipeline/luxel_native_mvp.py` | orchestration/regression oracle | packages the native MVP smoke path and invokes native CLIs; not semantic authority |
| `pipeline/luxel_authoring.py` / `pipeline/worldbuilder_dsl.py` | authoring/compatibility | parsed model-facing authoring vocabulary and predecessor DSL machinery; exact current status requires C0.2 fencing |

The current surface has transactional operations such as inspect, propose work, build candidate, attach evidence, playtest, capture, evaluate, repair, verify, commit, and rollback. The read-only facade now exposes the intended higher-level vocabulary and maps `project.plan` / `style.compile` to bounded partial slices; `world.construct`, `scene.compose`, and `gameplay.compose` remain explicitly planned rather than callable.

## Providers

These paths may participate in build-time construction but return ordinary artifacts to Luxel for reimport and independent validation:

| Path family | Provider role | Boundary rule |
| --- | --- | --- |
| `pipeline/rigging_provider.py` | deterministic Blender rig/control GLB provider | Luxel asset contract owns identity, facts, acceptance, and runtime use |
| `pipeline/blender_asset_probe.py` | Blender import/visual preflight | reports facts; does not repair or certify |
| `pipeline/blender_generate_alpine_kit.py` | deterministic Blender environment asset generation | generated GLB re-enters asset/scene object path |
| `pipeline/blender_generate_arcane_ruin_kit.py` | deterministic Blender environment asset generation | same provider boundary |
| `pipeline/comfy_generate_texture.py` | optional texture provider | output must be digest-bound and conditioned before promotion |
| `pipeline/vision_annotation_adapter.py` | optional visual/document provider | observations are not semantic truth until Rust intake validation |
| `pipeline/tool_provenance.py` | provider provenance helper | provenance metadata must bind to source/output identities |
| selected `pipeline/asset_*` modules | legacy/provider asset preparation candidates | each module must be classified before inclusion in canonical default discovery |

Blender is not present in the native runtime. A provider process may be unavailable and produce an explicit unsupported outcome; it may not silently substitute a different asset or claim certification.

## Legacy reference and salvage

These paths contain useful algorithms, predecessor contracts, or regression specimens, but the current native path must not depend on them without a typed port and independent validation:

| Path family | Salvage value | Current risk |
| --- | --- | --- |
| `pipeline/hydrology.py` | hydrology reasoning and uphill-reversal controls | Python semantic behavior can be mistaken for current authority |
| `pipeline/forestry.py` | ecological placement and population heuristics | old world/layout vocabulary and output identities |
| `pipeline/site_conditions.py`, `siting.py`, `reachability.py` | landform/site/placement reasoning | predecessor artifact contracts |
| `pipeline/erosion.py`, `procedural_surface.py`, `import_dem.py`, `import_heightfield.py` | terrain/numerical preparation | legacy path assumptions and generated outputs |
| `pipeline/gaea_*.py` | landform/provider integration concepts | external provider/runtime naming |
| `pipeline/luxel_critic.py` | repair-carrying visual/world-language diagnosis | currently coupled to predecessor `worldbuilder_dsl` and Bevy-era metrics |
| `pipeline/bevy_visual_acceptance.py`, `foliage_projection_acceptance.py`, `capture_bevy.py` | reference inspection/evidence history | must remain clearly separate from native Lava promotion |
| `pipeline/zone_*`, `generate_*`, `build_*`, `capture_*` compatibility scripts | historical world construction and regression specimens | broad mixed responsibilities and external-engine output |
| `world_core/crates/semantic_kernel/` | semantic transaction vocabulary and golden vectors | older store/pointer model |
| `world_core/crates/worldspec/` | predecessor deterministic world contract | predecessor Codeweald vocabulary |

The salvage rule is port, do not reactivate: a useful algorithm is promoted only behind a Rust-owned typed contract, with explicit identity, validator, determinism, and negative controls.

## Archive/compatibility paths

The following are deliberately outside the current runtime and certification path:

| Path | Classification | Evidence |
| --- | --- | --- |
| `engine_adapters/unity/` | `ARCHIVE_COMPAT` | Unity package/editor/runtime adapter and contract harness |
| `engine_adapters/unreal/` | `ARCHIVE_COMPAT` | Unreal plugin/importer and preflight path |
| `pipeline/zone_to_unity.py` | `ARCHIVE_COMPAT` | Unity delivery projection |
| `pipeline/zone_to_unreal.py` / `unreal_artifacts.py` | `ARCHIVE_COMPAT` | Unreal delivery projection |
| `pipeline/zone_spec_to_godot.py` / `zone_assets_to_godot.py` | `ARCHIVE_COMPAT` | Godot delivery projection |
| `pipeline/*.gd` and `.gd.uid` scripts | `ARCHIVE_COMPAT` | Godot/editor/runtime-era scripts |
| references to `godot_renderer/` and `Game Projects/Codeweald/` | `ARCHIVE_COMPAT` or stale documentation | paths are not inside the current Luxel workspace and appear in old examples/tests/docs |

These paths should remain available for history and regression until Campaign 0.2 establishes a deliberate archive boundary, but they must not be in default native build/test discovery or active-agent examples.

## Generated output and caches

| Path | Classification | Action for active path |
| --- | --- | --- |
| `.git/` | `GENERATED_OR_CACHE` for package/build purposes | repository metadata; never part of agent source package |
| `world_core/**/target/` | generated build products | exclude from source discovery and packages; note that targets exist both at workspace and nested crate paths |
| `graphics_lab/**/.julia/`, Julia depot/cache locations | generated/cache | never canonical |
| `pipeline/**/__pycache__/`, `tests/**/__pycache__/`, `.pytest_cache/` | generated/cache | exclude from discovery |
| `graphify-out/` | generated output/cache | exclude from active source and package |
| `artifacts/` | evidence/generated bulk | retain selected certified evidence; exclude historical/duplicate runs from default source discovery |
| `artifacts/LUXEL_GRAPHICS_PIPELINE_RESEARCH_KIT_2026-09-29.zip` and other archives | duplicate/package artifact | not source; retain only as handoff history |
| `.freebuff/` and empty `kvfold/` | local tool state/empty workspace | exclude unless a specific tool contract claims ownership |

## Native build and test entry points

These are the current entry points to verify the native spine. They are recorded here as commands, not as a claim that every historical suite is already clean.

### Rust

Run from `world_core/`:

```sh
# Default native gate (compatibility/reference members are excluded)
python3 pipeline/luxel_native_gate.py

# Explicit full-workspace audit, including quarantined compatibility members
cargo fmt --all -- --check
cargo check --offline --workspace --all-targets
cargo test --offline --workspace --all-targets
cargo clippy --offline --workspace --all-targets -- -D warnings
```

Focused packages for the demo path include `luxel-control-plane`, `luxel-project-ledger`, `luxel-asset-contract`, `luxel-gameplay-contract`, `luxel-reference-runtime`, `luxel-certification-authority`, `luxel-live-evidence-contract`, and `luxel-native-graphics-contract`.

### Julia

```sh
julia --project=graphics_lab --startup-file=no graphics_lab/test/runtests.jl
julia --project=terrain_lab --startup-file=no terrain_lab/test/runtests.jl
```

### Python transport/control tests

The native-focused modules are:

```sh
python3 -m unittest tests.test_luxel_control_plane tests.test_luxel_engine_neutral tests.test_luxel_native_mvp
```

The broader `tests/` discovery currently includes legacy zone/compiler/Bevy/Unity/Godot assumptions and must not be treated as the default native gate until Campaign 0.2 fences it.

### Native graphics smoke

The Campaign 2 report records the current supervised command:

```sh
world_core/target/debug/luxel-native-graphics-contract render-campaign2-layout \
  world_core/crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia terrain_lab graphics_lab \
  graphics_lab/bin/luxel_graphics_worker.jl artifacts/campaign2/<run-id>
```

### Model-facing launcher

```sh
python3 -m pipeline.luxel_neura_mcp \
  --project-root <project-root> \
  --control-plane world_core/target/debug/luxel-control-plane
```

The launcher is transport-only. Rust control-plane commands and registered validators own semantic decisions.

## Drift and fencing findings

The following are confirmed C0.1 findings for Campaign 0.2, not yet fixed in this inventory slice:

1. `README.md` still presents older Codeweald/Godot-era structure, Bevy-era status tables, and external-delivery examples alongside the native convergence narrative.
2. `world_core/README.md` still describes Godot rendering/GDScript and old paths despite the native workspace being the current authority spine.
3. `terrain_lab/README.md` still points at `../godot_renderer/concept_batches/...` paths.
4. `docs/archive/2026-09_roadmaps-and-audits/mvp-roadmap.md`, `docs/platform/missing-inventory.md`, `docs/archive/2026-07_codeweald-migration/codeweald-migration.md`, and several historical session handoffs still describe Unity/Unreal/Godot as target delivery decisions. They are valuable history but need an explicit historical/archive label.
5. `world_core/crates/project_ledger/` still contains Unity/Unreal/Godot delivery enums, a Unity import manifest, Unity CLI help, and a package description mentioning Unity. These may remain compatibility readers, but active engine-neutral plans must be structurally fenced from them.
6. `world_core/crates/luxel_control_plane/` retains a Unity profile gate in tests/compatibility logic. It must not be presented as a current product target.
7. `tests/` default discovery includes `test_zone_compiler.py` and other modules that import external-engine adapters, and several native-facing tests reference the absent historical `Game Projects/Codeweald/godot_renderer` path.
8. Native Rust/Julia code does not show a runtime dependency on Unity, Unreal, or Godot. Remaining mentions are comments, compatibility types, provenance for the Bevy inspection instrument, or stale documentation; this should be enforced by CI rather than inferred from grep.
9. The current front door is transactional (`inspect_*`, `propose_work`, `build_candidate`, `verify_candidate`, etc.), not yet the higher-level construction surface required by Campaign 1.

## C0.1 conclusion

The native spine is coherent enough to proceed:

```text
Rust authority/runtime
    -> typed world/gameplay/asset/live/graphics contracts
    -> Julia/Lava and Julia terrain execution
    -> Python transport/provider seams
    -> Rust revalidation and promotion
```

The repository is not yet agent-coherent because old and current paths are interleaved in documentation, test discovery, and compatibility types. The next slice is C0.2: publish one active architecture entry point, rewrite stale doctrine, and fence archive/compatibility paths without deleting salvage.
