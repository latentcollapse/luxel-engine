# WGE Demo-Ready Native Engine Mega-Sprint

Status: **canonical execution roadmap; demo contract provisional until Campaign 0 freezes it**  
Date: 2026-09-30  
Scope: shortest credible path from the current certified native engine spine to an unattended, visually impressive, playable WGE vertical slice

This document is the execution-focused successor to the broader [Game Development Harness Roadmap](GAME_DEV_HARNESS_ROADMAP.md) and [WGE Generality Bridge](WGE_GENERALITY_BRIDGE.md). It is deliberately narrower than either. It optimizes for a proof that WGE can make a real game, not for general-purpose engine parity or completion of every long-term research direction.

Execution tasks belong in the Tether board. This document defines campaign boundaries, dependencies, exit gates, and non-negotiable doctrine; it is not a Markdown ticket backlog.

## 1. Product doctrine

```text
WGE IS THE GAME ENGINE.

The shipped game does not depend on:
  Unreal Engine
  Unity
  Godot
  Blender
  another game runtime

Preferred core dependency envelope:
  Rust
  Julia
  Lava / Vulkan
  narrowly scoped native or open-source libraries

Blender + MCP is an optional build-time DCC/provider.
```

Blender may model, UV, rig, weight, animate, retarget, bake, and condition assets during construction. It owns no WGE project truth, gameplay semantics, certification, or finished-game runtime. A Blender result must return through a typed WGE import boundary and be independently validated.

The governing rule is:

> Heavy tools may extend WGE. They must never constitute WGE.

Repeated dependence on an external operation is evidence that the operation may eventually deserve a native WGE capability. It is not by itself a reason to recreate the entire external tool.

## 2. North-star demo

The first flagship is provisionally an original **Alpine Citadel** vertical slice. Campaign 0 must freeze the exact brief, references, and acceptance bundle before implementation campaigns depend on it. “Alpine Citadel” is a working wedge, not a request to copy existing intellectual property.

The proposed contract is:

```text
original dark-fantasy alpine valley
dense conifer forest
wet stone, stream, and snow patches
fortress / bridge / gate / courtyard landmark
mountains and atmospheric depth
third-person player character
one enemy family
one elite or boss encounter
simple melee/action ability loop
one traversal objective
one interaction/reward
3–5 minutes of playable content
```

The actual proof is the construction procedure:

```text
concept art + master brief
    -> fresh Luna/Cyan through WGE semantic operations
    -> intent/specification and capability plan
    -> world, asset, style, and gameplay construction
    -> optional bounded Blender provider jobs
    -> native WGE world and runtime
    -> native Julia/Lava renderer
    -> inspect -> criticize -> repair
    -> verified packaged game
```

The final proof permits no human source edits, no manual shader repair, and no source-tree archaeology by the model. The model may use documented semantic operations and bounded provider jobs; the system must own validation and promotion.

## 3. Definition of demo-ready

WGE is demo-complete only when all of the following are true:

- a fresh model can begin from the frozen brief, concept references, and constraints;
- it can plan and build through the AI-native semantic surface without searching Rust internals;
- the result launches in a native WGE window;
- the player can move, collide, fight, interact, and finish a short objective;
- at least one real imported production asset is used;
- at least one rigged/skinned animated character renders natively;
- the environment is visually impressive at player height and in composed vistas;
- Blender may have participated in construction, but is absent from runtime;
- a clean rebuild/restart reproduces the certified snapshot;
- periodic Tier-A evidence certifies the live build;
- at least one injected failure is diagnosed at the correct layer and repaired through the intended surface;
- a second materially different visual identity can be produced without swapping engines;
- a distributable build launches without development tooling or a Python runtime requirement unless a separately justified provider is being run at build time.

“AAA achieved” is not the gate. The gate is a coherent, native, playable, inspectable game whose output is no longer plausibly described as a renderer demo.

## 4. Current capability posture

This is the starting classification for planning. Each campaign must replace “partial” with evidence or an explicit deferred outcome.

| Capability | Starting posture | Demo implication |
| --- | --- | --- |
| Rust semantic/certification authority | Green | Preserve; every new slice must bind to it |
| deterministic world and spatial fields | Green | Reuse as the world foundation |
| gameplay/GAS-equivalent contracts | Green foundation | Compose a deliberately small combat loop |
| offscreen native renderer | Green foundation | Extend incrementally; do not rewrite wholesale |
| Campaign 2 authored frame | Green calibration slice | Replace procedural-only composition with real scene objects |
| asset intake and rejection | Green/partial | Harden conditioning and source identity |
| canonical scene/object bridge | Missing/partial | Critical predecessor to real imported content |
| live window/present loop | Open/partial | Critical predecessor to playable proof |
| visual quality evidence | Green vector foundation | Add depth/contact/lighting judgment without scalarizing |
| IBL, reflections, shadows, foliage, atmosphere | Partial/open | Highest visual return after live path and assets |
| character provider | Build-time skeleton exists | Wire import, skinning, animation, and runtime |
| native skeletal rendering | Open | Required for demo character proof |
| StyleProfile / StylePlan | Contract proposal | Turn into executable front-door capability |
| autonomous critic/repair | Transactional foundation | Connect to visual/mechanical diagnosis |
| packaged native handoff | Open | Final campaign owns this proof |

## 5. Execution rules

Every sub-sprint follows:

```text
observe -> decide -> implement -> adversarially test -> integrate -> replay -> document
```

Rules:

1. Tether tracks the active sub-sprint, dependencies, blockers, and evidence. Markdown describes the plan; it does not replace the board.
2. Workers have disjoint write scopes. A worker may not edit another worker’s active scope without integration ownership.
3. Every slice names its Rust authority, execution boundary, validator, negative control, restart/replay check, and artifact handoff.
4. Ordinary implementation and test failures are repaired autonomously.
5. Sol or another architecture critic is reserved for contradictory contracts, genuine cross-system ambiguity, or repairs that repeatedly show the design is wrong.
6. A failing gate remains failed. No visual, runtime, provider, or receipt shortcut may convert it into a warning.
7. Do not start a dependent campaign while its predecessor is merely compiling. The predecessor must pass its exit gate.
8. Preserve useful legacy algorithms as quarantined salvage until a typed native replacement exists. Do not let them become accidental canonical dependencies.
9. Record recurring agent friction in [`docs/FRICTION_LEDGER.md`](docs/FRICTION_LEDGER.md). Small local, reversible, testable friction fixes are part of normal implementation; large changes become Tether work.

## 6. Campaign map

```text
C0  Quarantine the archaeological layers
 |\
 | C1  AI-native front door
 |   \
 |    C2  Scene/object/asset bridge ----\
 |                                      \
 C3  Native live game loop ------------- C4  Graphics quality
                                         / \
                                        /   C5  Characters/animation
                                       /     \
                                      C6  World richness
                                             |
                                             C7  Style compiler + critic + repair
                                             |
                                             C8  Unattended construction + shipping proof
```

C2 and C3 may run in parallel after C1 if their contract surfaces are separated. C4 and C5 can then proceed in parallel over the stable scene/runtime bridge. C6 depends on the scene-object contract but may reuse existing world semantics before all graphics work is complete. C7 and C8 are integration campaigns and must not begin as “just another feature lane.”

## 7. Campaign 0 — Quarantine the archaeological layers

### Objective

Make the active native architecture obvious to a fresh agent and structurally prevent canonical code from depending on obsolete external-engine or compatibility paths.

### Sub-sprints

#### C0.1 — Repository topology and ownership inventory

Produce an authoritative map of:

- active Rust authority/runtime crates;
- active Julia/Lava and terrain projects;
- current MCP/control surface;
- current gameplay/GCS contracts;
- current asset/provider seams;
- legacy Python algorithms worth salvaging;
- Unity/Unreal/Godot and other compatibility paths;
- generated output, caches, and duplicate artifacts.

The first inventory artifact is
[`WGE_REPOSITORY_TOPOLOGY.md`](WGE_REPOSITORY_TOPOLOGY.md). It is evidence for this gate, not a
replacement for the active architecture entry point that C0.2 will establish.

Every directory gets one of:

```text
ACTIVE_CANONICAL
PROVIDERS
LEGACY_REFERENCE
ARCHIVE_COMPAT
GENERATED_OR_CACHE
```

#### C0.2 — Active architecture and doctrine surface

Create or update one authoritative `ACTIVE_ARCHITECTURE.md`/equivalent entry point. Rewrite stale README claims that contradict the native renderer. Link the AI-native front door, authority plane, execution plane, provider boundary, and current build/test commands.

#### C0.3 — Dependency fences and default discovery

Add CI/test checks that:

- reject canonical imports from archived/external-engine paths;
- exclude archived integrations from default build/test discovery;
- fail if generated artifacts or caches enter canonical state;
- make the native build path explicit and reproducible.

### Exit gate

A fresh agent entering the repository sees one active architecture, can build/test the native path without an external game engine, and cannot accidentally make a legacy path authoritative. Useful old algorithms remain available as labelled salvage, not silently deleted.

## 8. Campaign 1 — Build the AI-native front door

### Objective

Move from transactional primitives to a model-facing construction surface that lets a fresh Luna plan a game without knowing internal file paths or backend terminology.

### Sub-sprints

#### C1.1 — Rust-owned capability registry seed

Status: **implemented read-only seed** in
`world_core/crates/wge_control_plane/src/capability_registry.rs`, with
`capabilities`/`capability-explain` CLI discovery and transport mappings. The
remaining C1 work is plan validation, style lowering, semantic execution, and
promotion integration.

Implement the smallest registry over existing machinery:

- world/layout lowering;
- gameplay kit/effects;
- native graphics packet;
- Campaign 2 authored projection;
- fixed capture and visual evidence;
- deterministic replay;
- asset/provider boundary.

Each entry has identity, schema, preconditions, executor, validator, determinism, cost/quality metadata, failure modes, and repair options. A read-only registry is acceptable as the first slice; execution promotion remains Rust-owned.

#### C1.2 — StyleProfile and StylePlan execution slice

Status: **typed semantic slice implemented** in
`world_core/crates/wge_control_plane/src/style_profile.rs`, with native
validation/lowering commands and `style_compile` transport mapping. Renderer
execution and broader capability coverage remain open.

Implement the first typed style intent and lowering path over geometry, surfaces, lighting, environment, camera, composition, and budgets. Preserve observations, inferences, assumptions, conflicts, regions, provenance, confidence, and indeterminate fields.

#### C1.3 — ConstructionPlan

Status: **typed Rust resolution slice implemented** in
`world_core/crates/wge_control_plane/src/construction_plan.rs`, with
`project-plan` / `construction-validate` commands and corresponding
transport/MCP mappings. Execution and promotion remain open.

Add a project-level plan that answers:

- what game is being built;
- required capabilities and their status;
- assets and provider jobs;
- world systems and gameplay kits;
- style policies and assumptions;
- evidence required for completion;
- unsupported requirements and blockers.

#### C1.4 — Semantic facade

Status: **read-only discovery slice implemented** in
`world_core/crates/wge_control_plane/src/semantic_facade.rs`, with
`facade` / `facade-explain` commands and transport/MCP mappings. Planned
verbs remain explicitly non-callable; semantic execution remains open.

Expose model-native operations equivalent in meaning to:

```text
project.intake
project.plan
capability.list / explain / plan
style.compile
world.construct
scene.compose
asset.prepare
character.prepare
gameplay.compose
runtime.launch
quality.inspect
repair.propose / apply
project.verify
project.package
```

Keep the existing transactional tools underneath as authority machinery and advanced operations. Do not prematurely freeze names when semantics are still moving.

#### C1.5 — Recognition-first fresh-agent smoke

Give a fresh agent Campaign 2 references and a short visual brief. It must produce and execute a valid construction/style plan through the semantic surface without source-tree archaeology.

### Exit gate

A fresh agent can discover, explain, plan, validate, execute, inspect, and replay a small authored scene using semantic operations. Unsupported requests are explicit. No registry entry can promote its own evidence.

## 9. Campaign 2 — Close the native scene/object/asset bridge

### Objective

Connect real authored assets to semantic world objects, gameplay references, collision, materials, LOD, and native rendering.

### Canonical shape

```text
RuntimeAssetPackage
    -> SceneObject
    -> World / Scene Artifact
    -> GraphicsScenePacket
    -> Julia/Lava
```

`SceneObject` minimally owns:

- stable identity;
- source asset identity;
- transform and semantic role;
- gameplay references and affordances;
- collision policy;
- material assignments;
- importance, LOD, visibility, and provenance.

### Sub-sprints

#### C2.1 — SceneObject and scene artifact contract

Status: **implemented** in `world_core/crates/project_ledger/src/scene.rs`.
`SceneArtifact`/`SceneObject` bind ready asset receipts, source/package
identity, transforms, roles, gameplay references, collision, materials, LOD,
visibility, and provenance. `project_scene_for_graphics` returns a detached
validated projection; direct graphics mutation of canonical scene state is not
possible through the Rust API. The detached projection is now consumed by the
typed packet-composition seam; native Lava rendering remains open.

Define Rust-owned identity, references, transforms, roles, collision, materials, LOD, visibility, and provenance. Prove that the graphics projection cannot mutate canonical semantic state. The scene contract and detached projection are implemented.

#### C2.2 — GLB conditioning and source identity

Status: **initial neutral conditioning implemented** in
`wge-asset-contract/src/render.rs`, with a native `prepare-render` CLI seam and
`wge-native-graphics-contract::project_render_asset` bridge.

The current slice covers deterministic topology/attribute conditioning,
normal/tangent generation, UV requirements, bounded PBR role extraction,
embedded PNG/JPEG decode to RGBA8, explicit color-space roles, source/package
identity, receipt revalidation, and tamper rejection. It deliberately keeps
the initial explicit mip level and does not yet carry collision/LOD policy
through the render package or create GPU resources. The malformed/bad control
remains permanently rejected. C2.7 below extends this authority-side contract
with a versioned deterministic mip-chain payload.

#### C2.2b — Scene render-package binding

Status: **implemented** in `wge-project-ledger`.
`SceneObject` may carry an optional content-addressed render package and mesh
identity. The render-bound sealing/validation path independently validates the
package, matches its source asset identity, and requires the selected mesh to
exist. The detached scene projection carries only the render identities needed
by a future graphics composer; receipt/gameplay authority remains in the
canonical scene artifact.

`compose_bound_scene` now validates the bound scene and exact conditioned
projection set, namespaces imported resources, lowers transforms and
importance, and retains the `SceneArtifact` identity in the packet. The
composition contract is green; native Lava rendering and certified replay
remain open.

#### C2.3 — Bounded provider job boundary

Make Blender/MCP provider jobs explicit, digest-bound, and optional. Provider output re-enters WGE through ordinary asset validation. No provider-local object or path becomes WGE identity.

#### C2.4 — Real asset vertical slice

The supplied assembled log-hut GLB is now a permanent positive fixture. It
survives native runtime preparation, deterministic render conditioning,
source/package/mesh identity validation, scene binding, and
`GraphicsScenePacket` composition with five distinct source meshes and five
embedded textures. The malformed supplied GLB remains a permanent rejected
control. `GraphicsWorkerSupervisor::render_bound_scene_and_promote` now
independently revalidates the runtime/render receipts, recomposes the exact
bound packet, promotes the Lava capture through Rust authority, restarts the
worker, and proves byte-identical capture plus deterministic certification
receipt replay. The saved 640x480 capture bundle is under
`/home/mattc/Pictures/WGE/c2.4-real-asset/`. This closes the permanent static
asset C2.4 integration slice.

#### C2.5 — Imported-asset authored inspection fidelity

Status: **implemented and green**. The permanent log-hut fixture now carries
Status: **implemented and green**. The permanent log-hut fixture now carries
conditioned tangents through the canonical `MeshPacket` into Julia/Lava. The
historical C2.5 GPU payload is single-level; C2.7 below now provides the
versioned authority-side chain representation without claiming GPU residency.
Rust derives a deterministic
`real-asset-close` camera and independently recomposes the bound packet before
promotion. The GPU proof renders a context frame and close frame, restarts the
worker, repeats the warm-up context frame, and replays the close frame with
byte-identical capture and deterministic receipt. Evidence is preserved in
`/home/mattc/Pictures/WGE/c2.5-imported-asset/`; the checkpoint is
`WGE_C2_5_IMPORTED_ASSET_INSPECTION_HANDOFF.md`.

This closes technical imported-asset inspection, not production asset quality:
GPU mip residency/sampler LOD, collision/LOD carry-through, and a cleaner
authored composition remain the next narrow frontier. Texture-transform
conditioning is closed by C2.6 below.

#### C2.6 — Texture-transform conditioning

Status: **implemented and green**. The Rust render conditioner now supports the
safe subset of glTF `KHR_texture_transform` needed by the native path. One
shared transform across present texture roles is preserved in the render
package and applied to canonical UV0 before tangent generation. Non-zero
alternate UV sets, malformed transform fields, and conflicting per-role
transforms are rejected with typed findings. No backend representation becomes
canonical state, and no new Lava schema is required for this slice.

The focused conditioning, projection, and scene tests are green. Authority-
side multi-level mip conditioning is closed by C2.7 below; GPU
residency/sampler LOD, alpha execution, collision/LOD carry-through, and a
cleaner authored composition remain the next material-quality frontier.

#### C2.7 — Versioned deterministic mip-chain conditioning

Status: **implemented and green**. The Rust render package is now v2 while
the conditioning request remains v1. `generate_cpu_chain` preserves the base
RGBA8 level and deterministically derives levels through 1x1. sRGB RGB data is
filtered in linear space, normal maps are averaged and renormalized as vectors,
and linear/data channels use a bounded box filter; alpha remains linear.
Rust independently validates level dimensions, byte lengths, chain count, and
content identity, including tamper rejection.

The neutral graphics projection and Julia packet parser preserve the chain as
`rgba8_mip_chain`, with a digest over concatenated decoded levels. The current
Lava adapter explicitly rejects multi-level payloads with a typed
unsupported-capability result until GPU residency/upload and sampler LOD are
implemented. No lower level is silently discarded, and no new GPU evidence is
claimed in this contract slice. The C2.5 native replay remains the backend
regression control.

The checkpoint handoff is
[`WGE_C2_7_MIP_CHAIN_HANDOFF.md`](WGE_C2_7_MIP_CHAIN_HANDOFF.md).

### Exit gate

A real external asset enters through the asset contract, retains source identity, becomes a semantic scene object, participates in collision, renders natively, survives restart/replay, and is present in a certified snapshot. Blender is absent from runtime.

## 10. Campaign 3 — Turn the renderer into a game renderer

### Objective

Close the gap between snapshot/capture rendering and a continuously playable native game session.

### Runtime shape

```text
NativeGameSession
    -> input sampling
    -> fixed simulation tick
    -> kinematic physics
    -> gameplay state
    -> scene transforms
    -> Lava rendering
    -> swapchain present
```

### Sub-sprints

#### C3.1 — Native session contract

Define input, fixed-step time, simulation/render separation, checkpoint/restore, restart, session identity, and telemetry. Reuse the existing grounded kinematic seam; do not widen to full rigid-body physics for this demo.

#### C3.2 — Input and third-person camera

Add keyboard/mouse/gamepad abstraction, player movement, collision-aware third-person camera, camera intent, and deterministic input trace capture.

#### C3.3 — Continuous present and frame pacing

Promote the existing window/swapchain path into the native session. Add frame pacing and live capability reporting. Do not create a second renderer.

#### C3.4 — Tier-B live / Tier-A sampled evidence

Keep live frames cheap. Periodically and at semantic events, freeze a snapshot and run the existing full Rust promotion path. Inject visual corruption and prove that the live session is demoted when sampled evidence fails.

### Exit gate

WGE launches a native window, accepts continuous input, traverses the current world, collides/interacts, renders through Lava, and preserves the two-tier evidence model without per-frame certification readback destroying the loop.

## 11. Campaign 4 — “This looks like a game” graphics

### Objective

Produce the first production-quality visual wedge through incremental extensions to the current path, not a wholesale deferred-renderer rewrite.

### Priority order

1. production asset/material conditioning: mip chains, filtering, tangent correctness, texture transforms, alpha policy, PBR ingestion;
2. prefiltered IBL and reflection probes;
3. directional cascades, contact refinement, and improved soft filtering;
4. opaque/alpha-tested foliage baseline, wind, deterministic variation, LOD, instancing;
5. aerial perspective, height-aware fog, sky/environment response;
6. live-only temporal reconstruction where it materially helps;
7. tone mapping, color grade, restrained bloom, and exposure;
8. bounded local lights if the demo composition requires them;
9. attractive bounded water only if the flagship depends on it.

### Sub-sprints

#### C4.1 — Material calibration and imported-texture quality

Use real assets through the C2 bridge. Validate linear/sRGB roles, mip/filter behavior, normals, roughness/metallic/AO, and material separation in close and medium views.

#### C4.2 — Environment/reflection slice

Add prefiltered environment lighting and a bounded reflection-probe path with explicit packet identity, memory budget, and evidence.

#### C4.3 — Shadow/contact slice

Improve directional shadows, cascades, contact grounding, and soft filtering. Add evidence for the axes currently indeterminate rather than converting them into cosmetic warnings.

#### C4.4 — Foliage/population slice

Introduce alpha policy, deterministic wind/variation, instancing, distance LOD, and population semantics. Keep the scene’s density and memory budget visible.

#### C4.5 — Atmosphere/image shaping

Add aerial perspective, height-aware fog, exposure/tone mapping, restrained post, and composition tuning. Keep live-only temporal features separated from deterministic certification where necessary.

### Exit gate

At player height and in fixed vistas, independent human review describes the output as a commercially plausible game screenshot. The relevant visual vector improves without hiding indeterminate axes or breaking deterministic certification.

## 12. Campaign 5 — Native characters and animation

### Objective

Deliver one excellent humanoid path without attempting universal auto-rigging.

### Sub-sprints

#### C5.1 — `humanoid_v1` skeleton profile

Freeze joints, naming, axes, sockets, bind conventions, animation clips, and compatibility rules. One compatible skeleton family is sufficient for the flagship hero and enemy family.

#### C5.2 — Provider import and deformation validation

Use Blender at build time for rig creation, weights, retargeting, cleanup, and animation preparation. Import the result and validate skeleton identity, inverse bind matrices, influences, sockets, bounds, and deterministic deformation probes.

#### C5.3 — GPU skinning packet and Lava execution

Extend the graphics contract for joints, weights, inverse bind matrices, skeleton identity, animation clips, and joint matrices. Execute skinning natively through Lava. Blender must not be present at runtime.

#### C5.4 — Minimal animation graph

Implement idle, locomotion, attack, hit reaction, death, and optionally dodge. Bind gameplay timing to windup, hit window, and recovery.

#### C5.5 — Character gameplay integration

Bind the character to input, collision, abilities, enemy policy, damage/effects, camera, and runtime evidence.

### Exit gate

A Blender-prepared character is independently validated, animated natively, controlled by WGE, rendered in Lava, and participates in gameplay with zero Blender runtime dependency.

## 13. Campaign 6 — Make the world worthy of the renderer

### Objective

Turn the correct native heightfield foundation into a semantic place rather than a heightfield with props.

### Sub-sprints

#### C6.1 — Native landform vocabulary

Add typed semantic landforms such as massif, ridge, valley, saddle, basin, river corridor, plateau, cliff band, talus, snow field, and wet ground. Julia owns numerical generation; Rust owns semantic identity and validation.

#### C6.2 — Salvage and port legacy numerics

Evaluate existing erosion, hydrology, forestry, site-condition, and placement work. Port useful mechanisms behind typed contracts; do not reactivate legacy Python as semantic authority.

#### C6.3 — Terrain material semantics

Use height, slope, curvature, wetness, flow, snow exposure, and soil/rock class for layered material lowering.

#### C6.4 — Ecological population

Drive vegetation families, rocks, logs, and ground cover from terrain semantics, moisture, elevation, slope, and exposure. Keep population identity and density deterministic.

#### C6.5 — Fortress/landmark placement

Place the flagship fortress through `SceneObject`, collision, navigation, encounter, sightline, and gameplay contracts. It must not be injected into a graphics-only calibration packet.

### Exit gate

At ordinary third-person height, the world reads as a coherent place with landform, material, ecological, traversal, and landmark semantics. The scene remains reproducible and inspectable.

## 14. Campaign 7 — Complete the style compiler and autonomous critic

### Objective

Make visual intent executable and make repair evidence-driven without granting a model-side critic promotion authority.

### Sub-sprints

#### C7.1 — Reference intake to StyleProfile

Compile concept art, brief, and constraints into observations, inferences, assumptions, conflicts, regions, provenance, confidence, and explicit unsupported fields.

#### C7.2 — StyleProfile to StylePlan

Lower style intent into capability selections and policies for geometry, materials, lighting, environment, camera, animation, effects, composition, and budgets.

#### C7.3 — Multidimensional visual critic

Measure or propose judgements for silhouette/readability, material differentiation, grounding, lighting hierarchy, atmosphere, texture frequency, composition, density, temporal stability, artifacts, frame cost, and memory. Retain `indeterminate` where evidence cannot honestly decide.

#### C7.4 — Bounded repair loop

Diagnose the failed layer, propose authorized mutations, rebuild, compare before/after evidence, and explain why the candidate improved or still fails. The critic proposes; Rust validates and promotes.

#### C7.5 — Style whiplash

Hold mechanics and world semantics approximately constant while changing to a materially different visual identity, such as a bright stylized adventure or colorful sci-fi industrial scene. Prove that style changes policy rather than hardcoded renderer state.

### Exit gate

Two materially different visual identities emerge from the same native engine and capability substrate through StyleProfile changes, without engine swapping or manual source surgery.

## 15. Campaign 8 — Unattended construction and shipping proof

### Objective

Prove the whole thesis in a fresh run.

### Sub-sprints

#### C8.1 — Clean-room construction run

Fresh environment, fresh Luna-class model, frozen concept images, master brief, constraints, and WGE semantic surface. No hidden source-tree instructions.

#### C8.2 — Failure-injection run

Inject at least one malformed asset, blocked traversal, bad lighting/material, unsupported style request, or unauthorized mutation. The model must identify the correct semantic layer and repair through the intended operation.

#### C8.3 — Packaging and dependency audit

Produce a distributable native build. Prove Blender is absent from runtime and Python is not required unless an explicitly documented optional provider is being run during construction.

#### C8.4 — Cold start and rebuild program

Measure and improve Julia sysimage/precompile, shader/pipeline cache, persistent worker, capability cache, first window, first playable frame, rebuild, and restart. Preserve artifact identities.

#### C8.5 — Construction-leverage benchmark

Run the same task through WGE and an ordinary model-accessible stack only if the benchmark is ready. Compare model/tool cost, time, repair, manual intervention, evidence, determinism, and quality vector. This is not a Unity/Unreal/Godot product gate.

### Exit gate

The full demo-ready definition in Section 3 passes from a fresh run, including a second style and an injected failure. The final artifact is a verified native snapshot and runnable handoff.

## 16. Permanent friction-elimination mandate

Every WGE implementation campaign also observes the system as a model user. Repeated model friction is research evidence. Record it in [`docs/FRICTION_LEDGER.md`](docs/FRICTION_LEDGER.md).

Track at minimum:

- tool calls to first valid plan;
- tool calls to first runnable artifact;
- tool calls to first certified build;
- invalid operations;
- repair iterations and restarts;
- source-code searches;
- manual interventions;
- tokens spent reconstructing state;
- wall time spent in avoidable choreography;
- semantic capability calls versus raw work orders.

Small local friction may be fixed immediately when the change is reversible, testable, authority-preserving, and scope-local. Large architectural improvements become Tether work and do not ride along as “cleanup.”

Every new capability gets an AI-native acceptance test:

- discoverable;
- inspectable;
- composable;
- localized failures;
- legal repair suggestions;
- usable by recognition rather than recall;
- free of backend jargon during normal use;
- replayable and verifiable;
- economical in context footprint.

## 17. Explicitly outside the demo critical path

Do not let the swarm disappear into:

- multiplayer, netcode, or MMO infrastructure;
- editor GUI or human-facing authoring ergonomics;
- full Blender replacement;
- universal auto-rigging, facial animation, or arbitrary creatures;
- full rigid-body physics, ragdolls, or destruction;
- voxel/SDF/cave representation;
- TetCageRT promotion;
- ray tracing as a requirement;
- giant meshlet/virtual-geometry architecture without measured need;
- full streaming-world technology;
- every gameplay kit;
- external engine parity or Unity/Unreal/Godot adapters;
- universal procedural asset generation.

TetCageRT remains research until the conventional path exposes a measured problem it solves. The demo is a proof of the architecture, not an excuse to implement the entire future of graphics before showing a game.

## 18. Cross-campaign artifact requirements

Every completed campaign must update:

- the relevant architecture document;
- the capability/status matrix;
- the benchmark or quality-gap register;
- the convergence/handoff document;
- the Tether task and evidence links;
- the friction ledger where applicable.

Every promoted artifact must retain:

- semantic identity;
- source/content identity;
- capability/provider identity;
- validator/schema identity;
- deterministic seed or explicit nondeterministic envelope;
- before/after evidence for repairs;
- restart/replay result;
- known limitations and indeterminate axes.

## 19. First sub-sprint

The next executable unit is **C0.1 — Repository topology and ownership inventory**. It should produce the active/ provider/legacy/archive/generated map and a short list of canonical import/build/test entry points. No graphics feature work should start until that map and the Campaign 0 gate are green.
