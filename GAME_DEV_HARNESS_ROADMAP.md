# WGE Game Development Harness Roadmap

Status: strategic execution plan
Date: 2026-09-26
Scope: WGE as the model-native game-development harness and eventual NIRA-Prime Game Dev mode

This document is the architecture and sequencing plan. Execution tasks belong in
the Tether board; this is not a markdown ticket backlog.

## North star

WGE is a temporary game studio that an agent can inhabit.

Given a game brief, concept art, design documents, a target runtime, and a
quality profile, Game Dev mode must be able to:

1. interpret the intent and record explicit assumptions;
2. compile a versioned project specification;
3. delegate bounded work to specialist systems;
4. generate, import, run, inspect, and playtest candidates;
5. diagnose failures in semantic terms;
6. repair the source or artifact graph;
7. commit only a verified project snapshot; and
8. deliver a runnable build with evidence and provenance.

“One-shot” means one user request that permits many internal build, critique,
playtest, and repair iterations. It does not mean one inference pass.

## Operating model

    brief + references + requirements
        -> claims, conflicts, style targets, assumptions
        -> project specification
           world + assets + gameplay + scenes + UI + audio + build profile
        -> typed work orders for specialist systems
        -> generate -> compile -> import -> run -> capture -> evaluate
        -> source-level repair proposals
        -> certified project snapshot -> release build

The model owns intent, composition, taste, and design choices. Algorithms own
answers that can be computed: geometry validity, navigation, collision, ability
costs, dependency invalidation, replay correctness, and performance budgets.
WGE owns the boundary, evidence, identity, and acceptance decision.

Three identities must remain distinct:

- Semantic identity: what the game is specified to mean.
- Artifact identity: the exact selected meshes, textures, code, audio, solver
  outputs, and pinned inputs.
- Release identity: the exact imported, tested, platform-specific build.

## Architectural doctrine

1. Model-authored source is parsed, never executed as host-language code.
2. Typed semantic IR is authoritative; backends never reinterpret source text.
3. No silent repair, omission, clamping, downgrade, or unsupported substitution.
4. Every rejection names the responsible semantic declaration or ranked
   contributors and proposes a machine-readable repair where possible.
5. Generated artifacts are content-addressed and provenance-carrying.
6. Candidate work happens against a pinned snapshot. The project pointer advances
   only after required gates pass.
7. A model may propose a new asset or code module, but WGE decides whether it is
   valid, usable, compatible, and releasable.
8. The harness is model-native. Human editor ergonomics are not a design target.

The existing semantic kernel is the first transaction substrate for this
doctrine. It must grow from a two-operation specimen into a project-level
ledger, without losing its single-pointer, multi-evidence, fail-closed model.

## System map

### 1. Brief and concept interpretation

Convert concept art, references, and documents into explicit claims:

- visual regions and their intended meaning;
- gameplay requirements and affordances;
- style targets and exclusions;
- constraints, dependencies, and unresolved conflicts;
- confidence and provenance for every consequential inference.

The agent must be able to distinguish “the reference shows this” from “the
model inferred this.”

### 2. Project specification and dependency graph

Extend the WGE authoring language and IR envelope across:

- world and level structure;
- assets and semantic parts;
- gameplay entities and abilities;
- scenes, cameras, lighting, UI, input, and audio;
- build targets and quality profiles.

Use typed common vocabulary for recurring concepts. Unique mechanics should be
generated as sandboxed code modules with explicit interfaces, tests, and
versioned runtime contracts. Do not build a universal all-genre gameplay DSL.

### 3. World and level construction

WGE owns playability and semantic layout:

- terrain import and datum contracts;
- regions, routes, structures, landmarks, and encounters;
- collision and navigation;
- spawn points, objectives, sightlines, and gameplay volumes;
- camera and traversal plans;
- target-engine handoff.

Specialist systems such as Gaea may own landform appearance. WGE owns the
playable world that is carved from it, its evidence, and its certification.

### 4. Gameplay substrate and GAS equivalent

The initial runtime contract needs:

- entities/components and attributes;
- gameplay tags;
- abilities and activation requirements;
- costs, cooldowns, and resource consumption;
- instant, duration, and periodic effects;
- stacking, removal, dispels, and immunity;
- targeting, hit detection, and events;
- deterministic combat resolution and replay;
- explicit authority boundaries.

Start single-process. Add prediction, replication, rollback, and multiplayer
only when a real target requires them.

### 5. Physics, simulation, and NPCs

WGE should contract with mature runtime systems rather than immediately
reimplement them. The harness must still specify and verify:

- movement and contacts;
- projectiles and collision queries;
- save/state boundaries;
- perception and navigation;
- goals, utility or behavior policies;
- combat decisions and debug traces.

NPCs must expose why they acted, not merely emit opaque behavior.

### 6. Model creation, materials, rigging, and animation

The asset pipeline must treat an asset as a usable game object, not a pretty
mesh. It needs:

- semantic parts and named affordances;
- dimensions, scale, pivots, sockets, and collision intent;
- topology, normals, UVs, materials, and LODs;
- skeleton profiles, skinning, retargeting, and animation state;
- root motion and gameplay contact events;
- runtime import and target-engine validation.

Acceptance must catch unusable doors, sealed spaces, bad pivots, broken
deformation, foot sliding, clipping, missing sockets, and incorrect scale.

### 7. Rendering, VFX, UI, and audio

These remain explicit project domains:

- materials, lighting, shaders, cameras, particles, and postprocessing;
- HUD, menus, state bindings, input, and onboarding;
- sound cues, music states, spatial audio, dialogue, and mixing.

Every domain needs both artifact validity checks and target-runtime captures.

#### Research flag: Tetrahedral Cage RT

Evaluate tetrahedral-cage representations as an optional animation and
ray-tracing lowering for dense, connectivity-preserving geometry. This is a
research track, not a prerequisite for the current engine-neutral vertical
slice and not permission to replace the canonical semantic mesh/asset
representation. WGE may materialize a TetCageRT representation only after the
evidence and profitability policy below are independently green.

The central question is not “should WGE use tetrahedral cages?” It is “when
should WGE materialize animated geometry as tetrahedral cages?” The candidate
policy may ultimately choose a hybrid hierarchy—full BLAS for near detail,
cluster AS for intermediate scale, and TetCageRT for far/dense geometry—but
that is a hypothesis to test, not a design decision.

1. **Paper reconstruction.** Pin the exact AMD paper/source, read it completely,
   and reconstruct its representation, assumptions, update rules, and failure
   modes in a cited research note before implementing a shortcut. Do not mix
   the paper's measurements with later terrain-demo measurements.
2. **CPU reference.** Implement deterministic tetrahedral-cage generation,
   triangle clipping, barycentric encoding, deformation, and correctness
   visualization. Track clipping-induced geometry expansion (reported results
   suggest roughly 1.3x–2.3x depending on cage/scene resolution) rather than
   hiding it in a memory estimate.
3. **Lava/Vulkan prototype.** Build a small, optional prototype behind a typed
   Julia graphics contract: transformed-ray or static mini-BLAS path, explicit
   device capability checks, and no semantic authority in Lava. Treat the
   extra traversal/intersection machinery as a possible cost, not an assumed
   win.
4. **Controlled comparison.** Measure conventional animated BLAS, cluster AS,
   and tetrahedral cage across visual deformation error, AS memory, animation
   cost, AS update cost, trace cost, total frame time, preprocessing expansion,
   and cage resolution. Record cold/warm behavior and deterministic replay.
5. **Watertightness.** Investigate a watertight 4D barycentric representation,
   including temporal/topological continuity, boundary behavior, numerical
   robustness, and adversarial deformation cases. Coarse cages and joint-heavy
   deformation must have visible correctness controls; plausible pixels are not
   sufficient.
6. **Materialization policy.** Encode a Rust-owned decision procedure that
   determines when TetCageRT is legal, bounded, visually acceptable, and
   profitable for the requested quality profile. “Unsupported,” “uncertain,”
   and “not profitable” remain explicit outcomes.

The research track must preserve the source mesh as canonical, retain a
conventional fallback for every comparison, and produce independently
recomputable evidence. It must not block the native raster certification path,
silently change asset identity, or turn a benchmark-only acceleration into a
passing gameplay/render receipt.

### 8. Build, playtest, critique, and release

The harness must be able to launch fresh builds, send scripted input, capture
screenshots/video, collect logs and profiler data, evaluate the run, and
produce source-level repair proposals.

Bevy remains a valuable reference/world-inspection renderer. It is not enough
to certify the Unity or UE runtime; imported target-runtime evidence is
mandatory.

## Quality bar

Quality is a portfolio of independent evidence, not one “looks good” score.

### Mechanical gates

IR validity, capability negotiation, mesh/material validity, collision,
navigation, gameplay rules, build success, runtime errors, memory, frame time,
and complete playthroughs.

### Perceptual gates

Silhouette, composition, color relationships, material response, lighting,
animation, audio, and reference similarity evaluated at the distance, camera,
and motion in which players encounter the content.

### Design gates

Responsiveness, readability, fairness, objective clarity, encounter quality,
and bot-completed playthroughs. Human judgments may calibrate critics and
reference sets, but humans should not be required as an editor in the loop.

### Adversarial controls

Every gate needs known-good and known-bad controls. Include noisy or blurred
textures, broken rigs, disconnected navigation, silent abilities, incorrect
scale, malformed assets, misleading captures, and metrics-gaming inputs.

If evaluators disagree, the candidate remains uncertified. Uncertainty must not
be converted into a passing receipt.

## Execution phases

### Phase 0 — Project truth ledger

Define the minimum project manifest, target runtime profile, artifact graph,
dependency invalidation, work-order schema, evidence model, and canonical
inspect/build/verify commands. Record current Codeweald failures as regression
controls.

Exit condition: an agent can inspect a project snapshot, understand its failing
gates, and identify the semantic source or artifact that must change.

### Phase 1 — A genuinely playable WGE world

Connect the semantic kernel to the actual world build path. Make collision and
navigation acceptance hard requirements, finish spawns and objectives, export to
the chosen Unity target, and prove traversal with an automated controller.

This is the immediate continuation of the current WGE MVP work.

Exit condition: a fresh build produces a traversable Codeweald arena with
correct collision, navigation, spawns, and target-runtime behavior.

### Phase 2 — One complete game loop

Add the first game-domain contracts and compact GAS equivalent. Implement two
playable entities, enemies or minions, an objective, win/loss, basic NPC
behavior, input, HUD, VFX, audio, and deterministic replay.

Exit condition: an automated player can start, understand, fight, use abilities,
reach the objective, and finish successfully.

### Phase 3 — One complete content pipeline

Take one concept-art character and one environment kit through model creation,
materials, rigging, animation, LOD, collision, lighting, effects, sound,
engine import, and runtime capture.

Exit condition: the polished slice passes technical gates and reference
comparison without editor-only hand repair.

### Phase 4 — Closed-loop critique and repair

Add scripted input/replay, captures, telemetry, multimodal critics, performance
budgets, gameplay diagnosis, source-level repairs, and a regression corpus.

Exit condition: WGE can observe a bad playthrough, identify the responsible
semantic layer, repair it, rebuild, and verify the improvement.

### Phase 5 — Generality

Build a second map from different art and then a compact second game with
different mechanics and visual language. Measure whether smaller models can
author and repair bounded tasks using scaffolds and diagnostics.

Exit condition: new content extends registries and adapters without changing
the transaction model, and the result is not Codeweald-specific.

### Phase 6 — NIRA-Prime Game Dev mode

Load WGE as a native capability bundle containing project state, specialist
workers, tool registry, artifact cache, quality profile, budgets, memory, and
the WGE verifier as completion authority.

Exit condition: one game brief produces a runnable, inspected, traceable build
or an honest failure report with the remaining blockers.

## Packaging for external coding harnesses

The authoritative implementation should be a local daemon/CLI plus typed SDK.
MCP and skill/plugin layers should be adapters over that API, not the source of
truth.

The model-facing operation surface should remain small and semantic:

- inspect project or artifact;
- propose or apply a work order;
- build a candidate;
- run a playtest;
- capture evidence;
- evaluate a candidate;
- commit or roll back a snapshot.

Avoid exposing hundreds of fragile editor actions. Codex, Claude Code, and
similar systems should receive a workflow skill and a typed MCP surface. NIRA-
Prime can eventually bypass that transport and load the same protocol natively.

## Initial asset benchmark

The first real content-pipeline test asset is the user-provided GLB:

Source path:

    /home/mattc/Pictures/Generated 2D Images/sample_2026-09-26T091412.074.glb

Recorded intake facts:

- glTF binary model, version 2;
- size: 13,980,532 bytes;
- SHA-256:
  858fa104880822d081405579fb5b39d533d3b3b341d38aa1490a44b634f5e2b4;
- current state: not rigged.

The benchmark ladder is:

1. inspect and report meshes, materials, transforms, bounds, and scale;
2. identify semantic parts and propose a skeleton hypothesis;
3. create and validate a rig and skinning;
4. generate a minimal animation set;
5. add sockets, collision, LOD, and gameplay metadata;
6. import into the target runtime;
7. exercise the asset in a real gameplay slice;
8. retain all source, tool, and repair provenance.

The original file remains external until an explicit asset-ingestion step
copies it into a content-addressed project store. Its hash is the intake
identity; a generated rig is a derived artifact.

The first structural benchmark is now implemented by
[pipeline/asset_intake.py](pipeline/asset_intake.py), covered by
[tests/test_asset_intake.py](tests/test_asset_intake.py), and materialized as
[the deterministic intake report](asset_intake_reports/sample_2026-09-26T091412.074.intake.json).
Its canonical report digest is
14301dc1dc20272b7d35e270a3d0221a310b3376470c0cc025f97f8d06638f71.

## Explicit deferrals

Do not begin with full multiplayer, universal engine parity, a new renderer or
physics engine, unrestricted text-to-3D at scale, or a giant all-genre gameplay
language. First prove one complete agent-operated game-development loop.

The first decisive milestone is:

> An agent receives one Codeweald brief, builds the world, imports it, traverses
> it, plays a complete loop, detects failures, repairs them, and produces a
> verified runnable build through WGE alone.

Once that loop is real, each additional subsystem becomes a certified
extension to an operating game-development studio rather than another isolated
generator.
