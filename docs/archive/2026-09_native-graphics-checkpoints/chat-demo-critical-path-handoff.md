# WGE Chat Demo-Critical-Path Handoff

Date: 2026-09-30  
Purpose: provide Chat with the current WGE tree needed to derive a short, implementation-sensitive roadmap for an impressive self-sufficient AI-native game demo.

## Product boundary

WGE is the game engine and the project-truth owner.

The target flow is:

```text
concept art + master brief
    -> AI-native WGE semantic surface
    -> world / asset / style / gameplay construction
    -> native WGE runtime and graphics
    -> inspection and evidence-driven repair
    -> playable high-quality vertical slice
```

The primary runtime path is Rust + Julia + Lava/Vulkan. External tools may prepare content, but they must not own WGE semantic identity, project truth, certification, or the finished game's runtime.

## Blender boundary

Blender is an allowed optional heavy-machine-shop provider for the current demo path. It may perform bounded asset operations such as:

- mesh preparation and UV work;
- rigging, skinning, weighting, and animation authoring;
- baking and export.

The intended boundary is:

```text
WGE semantic character/asset intent
    -> bounded provider job
    -> mesh / skeleton / skin / animation artifact
    -> WGE import and independent validation
    -> native WGE runtime and rendering
```

Blender is not the runtime, semantic authority, or required player-facing dependency. Native replacement is a later dependency-elimination campaign unless repeated benchmark evidence shows it is immediately critical.

## Package contents

The package includes:

- `world_core/`: Rust semantic kernel, authority, gameplay, runtime, asset, live-evidence, native graphics, and manifests;
- `graphics_lab/`: Julia/Lava graphics execution path, tests, and locked dependencies;
- `terrain_lab/`: Julia terrain/spatial execution path, tests, and locked dependencies;
- `pipeline/`: current intake, asset, material, style, critic, provider, orchestration, and compatibility tooling;
- `tests/`: Python/control-plane tests, semantic fixtures, gameplay fixtures, asset controls, and negative controls;
- `docs/`: gameplay kit/GCS, semantic-kernel, physics/live-loop, visual-authority, and other design material;
- current root architecture, roadmap, handoff, benchmark, quality-gap, authority, and research documents;
- Campaign 2 final captures, baseline replay, manifests, and representative visual evidence;
- current Blender/provider experiments and rigging fixtures;
- current examples, layouts, Cargo manifests/lockfiles, Julia Project/Manifest files, and README files;
- `docs/archive/2026-09_roadmaps-and-audits/autonomous-game-construction-forensics.md`, `docs/platform/capability-registry-design.md`, `docs/world/style-profile-contract.md`, and `docs/archive/2026-09_roadmaps-and-audits/construction-leverage-benchmark.md`.

## Deliberate exclusions

The package omits:

- `.git/` history and repository internals;
- Rust `target/` build products;
- Julia package caches and local environments;
- Python bytecode and test caches;
- `graphify-out/` generated output;
- historical Campaign 2 iteration directories beyond the representative final/current replay;
- video bulk and duplicate archives;
- generated packet/capture bulk not needed to understand the current visual frontier.

The exclusions are packaging decisions only. The workspace remains the source of truth.

## Current checkpoint to inspect first

Start with:

1. this file;
2. `docs/archive/2026-09_native-graphics-checkpoints/peak-shape-handoff.md`;
3. `docs/content-sdk/generality-bridge.md`;
4. `docs/archive/2026-10_graphics-sprint-reports/graphics-campaign-2-report.md`;
5. `docs/platform/native-capability-matrix.md`;
6. `docs/world/native-quality-gaps.md`;
7. `docs/gameplay/gameplay-kit-architecture.md` and `docs/gameplay/gcs-foundation-design.md`;
8. `world_core/README.md`, `world_core/Cargo.toml`, and the crate manifests;
9. `pipeline/wge_agent_surface.py`, `pipeline/wge_engine_neutral.py`, and `pipeline/wge_native_mvp.py`;
10. the final Campaign 2 capture/evidence manifest.

Then inspect the actual Rust, Julia, and provider code at the boundaries described by those documents. Do not infer implementation maturity from a roadmap claim alone.

## Questions the package is meant to answer

- What is already green and genuinely reusable for a demo?
- Which contracts exist only on paper or are only partially wired?
- What is the smallest native capability set needed for one beautiful playable vertical slice?
- Which work should remain an optional Blender provider for the first demo?
- What must be exposed through the model-facing semantic surface?
- Where are old or duplicate paths still hanging around?
- Which graphics improvements have the highest perceptual leverage without a renderer rewrite?
- Which failures would stop a fresh model run, and which are ordinary hardening?
- What is the shortest credible sequence from brief/reference to a verified handoff?

## Required roadmap posture

Optimize for **demo-capable, not engine-complete**. Keep the authority, determinism, provenance, restart, and validation guarantees. Do not broaden into MMO infrastructure, universal engine parity, or hypothetical feature breadth before the demo critical path is proven.

