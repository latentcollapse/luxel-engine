# Luxel Capability Registry Design

Status: **seed implemented; execution/planning slices remain**, 2026-09-30  
Scope: model-facing discovery, selection, execution, validation, and promotion of reusable game-construction capabilities

This document turns recurring model procedures into a registry concept. It does not authorize a new semantic authority plane and does not make Julia, Lava, Blender, an external provider, or a renderer backend canonical Luxel state.

Related sources:

- [Luxel Generality Bridge](../content-sdk/generality-bridge.md)
- [Autonomous Game-Construction Forensics](../archive/2026-09_roadmaps-and-audits/autonomous-game-construction-forensics.md)
- [Luxel Native Graphics Architecture](native-graphics-architecture.md)
- [Luxel Gameplay Kit Architecture](../gameplay/gameplay-kit-architecture.md)
- [Authority Reclamation](../archive/2026-09_roadmaps-and-audits/authority-reclamation-001.md)

## Current implementation slice

The first read-only registry is implemented in
`world_core/crates/luxel_control_plane/src/capability_registry.rs` and is
exposed through the Rust control-plane commands:

```bash
cargo run --manifest-path world_core/Cargo.toml --offline -p luxel-control-plane -- capabilities
cargo run --manifest-path world_core/Cargo.toml --offline -p luxel-control-plane -- capability-explain graphics.scene.packet/v1
```

The transport surface exposes the same discovery as `capability_list` and
`capability_explain`; the typed `style_compile` and `project_plan` operations
now consume the StyleProfile/StylePlan and ConstructionPlan slices. The
catalog is content-digested, dependency-closed, and validator-bound. It is
still deliberately non-executing: it does not invoke arbitrary capability
plans or promote evidence.

## Design thesis

The model should compose known capabilities and generate only the semantics unique to the requested game.

```text
typed intent
    -> capability discovery
    -> constrained capability plan
    -> typed lowering
    -> provider / Julia / Lava execution
    -> independent evidence
    -> promotion or bounded repair
```

The registry is therefore not a plugin marketplace and not a bag of executable prompts. It is a Rust-owned catalog of contracts, preconditions, validators, quality/cost metadata, and authorized execution seams.

## Ownership boundary

| Layer | Owns | Must not own |
| --- | --- | --- |
| Rust semantic plane | capability identity, versions, inputs, outputs, plans, constraints, canonical state, validators, receipts, promotion | backend handles, GPU objects, provider-local mutable state |
| Julia / Lava | numerical lowering, GPU execution, buffer/resource lifetime, renderer implementation, measurable runtime telemetry | semantic identity, project truth, promotion decisions |
| External providers | bounded asset/rig/material/tool transformations | Luxel identity, gameplay semantics, certification |
| Python / transport glue | orchestration, process supervision, transport, provider invocation | semantic behavior, evidence authority, canonical state |
| Model | intent interpretation, plan proposal, project-specific code/content, repair proposal | direct evidence promotion or unvalidated canonical mutation |

The registry must preserve the existing rule: producers produce evidence; independent Rust validators decide whether evidence is admissible.

## Capability classes

The first registry should support these classes without pretending they are all equally mature:

- `world`: terrain, spatial fields, navigation, encounter layout, population placement;
- `asset`: intake, conditioning, conversion, material extraction, collision, LOD preparation;
- `character`: skeleton profile, bind/weight generation, animation, retargeting, deformation probes;
- `graphics`: material family, lighting policy, atmosphere, camera, environment, visual capture;
- `gameplay`: abilities, effects, resources, interaction, AI policy, objectives, save/replay;
- `runtime`: fixed-step execution, input injection, state query, checkpoint, restart, packaging;
- `verification`: semantic, mechanical, traversal, visual, performance, provenance, and failure injection;
- `repair`: diagnosis, bounded proposal, authorized application, rebuild, before/after evidence comparison.

An entry may depend on another entry, but dependencies are explicit and versioned. A renderer backend is an executor of a graphics capability, never a capability identity by itself.

## Capability descriptor

The eventual Rust type should be equivalent in meaning to:

```yaml
id: graphics.material.surface.wet-reflective
version: 1
class: graphics
status: experimental | candidate | certified | retired
owner: rust-authority
intent_schema: luxel.material-intent/v1
output_schema: luxel.material-plan/v1
preconditions:
  - texture_identity_available
  - linear_color_space
  - packet_material_budget >= 1
dependencies:
  - graphics.texture.conditioning/v1
  - graphics.capture.reference/v1
executor:
  kind: julia_lava_packet
  entrypoint: typed-operation-name
  packet_schema: luxel.graphics-scene-packet/v6
determinism:
  mode: deterministic
  seed_fields: [project_seed, asset_identity]
quality_axes:
  - material_separation
  - artifact_rate
  - frame_cost
cost_model:
  cpu: declared or measured
  gpu: declared or measured
  memory: declared or measured
failure_modes:
  - unsupported_texture_format
  - missing_reflection_path
  - budget_exceeded
validators:
  - registered-rust-validator-id
repair_strategies:
  - graphics.material.reduce-detail/v1
provenance:
  source: campaign-or-case-id
  evidence: [receipt-id]
```

The descriptor is declarative. It does not contain executable source, hidden fallback behavior, or an authority bypass.

## Capability use and receipts

Every selected capability should produce a `CapabilityUseReceipt` bound to the project and its inputs.

Minimum fields:

```yaml
receipt_id: capability-use-...
capability_id: graphics.material.surface.wet-reflective
capability_version: 1
project_id: project-...
input_identities:
  - world-artifact-id
  - asset-id
  - style-profile-id
output_identities:
  - material-plan-id
provider:
  kind: julia_lava_packet | blender_provider | rust_native | other
  version: pinned-or-explicitly-unknown
execution:
  seed: 1234
  packet_sha256: sha256:...
  started_at: recorded
  duration_us: recorded
evidence:
  - validator_id
  - evidence_id
status: staged | passed | failed | indeterminate | superseded
```

A receipt is not proof merely because it has a success field. Promotion re-runs the registered validator over the referenced inputs and output artifacts. Missing, stale, malformed, producer-only, or status-only receipts fail closed.

## Lifecycle

### 1. Discover

The model or intake layer asks for capabilities by semantic need, not implementation name:

```text
need: reflective wet ground under fixed directional light
constraints: deterministic, opaque, <= 4 MiB texture payload
quality: material separation, grounding evidence, frame budget
```

The registry returns only entries whose schemas and preconditions are inspectable.

### 2. Select

The model proposes a capability graph. Rust validates:

- dependency closure;
- schema compatibility;
- source/asset/world identities;
- capability status;
- quality and resource budgets;
- unsupported-feature declarations;
- conflicts and alternatives.

### 3. Stage

The plan is staged as semantic intent. Providers and executors do not mutate canonical state while a plan is still unpromoted.

### 4. Lower and execute

Rust lowers the plan to coarse typed packets. Julia/Lava or a provider executes the packet. Backend objects, provider-local paths, GPU handles, and process state remain outside the canonical artifact.

### 5. Measure

The executor returns raw artifacts and telemetry. Rust independently computes or validates the evidence required for promotion.

### 6. Promote

Only a registered validator can promote the capability output. Promotion binds:

- input identity;
- capability and version;
- output identity;
- validator/schema identity;
- evidence identity;
- deterministic seed or explicit nondeterministic envelope.

### 7. Repair

A failure identifies the semantic layer and emits a bounded repair proposal. The repair must name:

- authorized fields it may change;
- fields it may not change;
- expected evidence improvement;
- budget impact;
- rebuild command;
- comparison rule.

The proposal is applied only after Rust validates its scope. Before/after evidence is retained.

### 8. Retire

A capability can be retired without rewriting history. Existing receipts retain their exact version and validator identity; new plans cannot select the retired version.

## Model-facing surface

The model should not need to understand Lava, Vulkan, Blender internals, or packet layout for normal construction. The minimum model-facing operations are semantic:

- `capability.list(constraints)`;
- `capability.explain(id)`;
- `capability.plan(intent)`;
- `capability.validate(plan)`;
- `capability.stage(plan)`;
- `capability.execute(stage_id)`;
- `capability.inspect(receipt_id)`;
- `capability.replay(receipt_id)`;
- `capability.propose_repair(failure_id)`;
- `capability.apply_repair(repair_id)`;
- `capability.promote(evidence_id)`.

These operations should be exposed through the existing typed control/MCP surface rather than as arbitrary code execution. The model can still author project-specific code, but the harness must make the stable machinery discoverable and inspectable.

## Registry entry requirements

An entry is not `candidate` until it has:

- a typed input and output schema;
- a registered validator;
- a declared owner and executor;
- a determinism statement;
- explicit unsupported cases;
- a failure-injection test;
- a restart/replay test where applicable;
- an identity/provenance test;
- a bounded repair or an explicit “not repairable” outcome.

An entry is not `certified` until it has passed at least two materially different tasks or a stronger formal conformance suite approved by the authority plane.

## Initial registry inventory

The first inventory should point at machinery Luxel already has rather than inventing parallel systems:

- certified world/layout lowering;
- terrain/spatial field generation;
- gameplay kit and ability/effect contracts;
- native graphics scene packet;
- Campaign 2 authored-frame projection;
- fixed camera/capture contracts;
- visual-quality evidence vector;
- deterministic reference runtime;
- asset intake and negative-control validation;
- Blender/provider boundary for rigging research;
- repair and evidence comparison contracts.

Entries that are not implemented must remain visibly unavailable. A registry should make gaps easier to see, not make them look complete.

## Relationship to Palette and Gesso

Palette/Cyan may become a high-level proposal and inference layer for visual intent. Gesso/Lava may become a powerful execution surface for graphics operations. Neither changes the ownership rule:

```text
Palette proposal
    -> Rust StyleProfile / CapabilityPlan authority
    -> typed Gesso/Lava packet
    -> execution telemetry and artifacts
    -> Rust validation and promotion
```

This preserves the useful artistic abstraction without letting a backend or model-side critic quietly become semantic authority.

## Open design questions

- Should capability dependency resolution be a Rust graph type or a serialized plan validated by Rust?
- Which quality/cost fields need hard bounds before planning is safe?
- How should incompatible providers express alternatives without silent fallback?
- How should capability versions interact with project snapshots and replay?
- What is the minimum receipt needed for a live, non-certification execution?
- Which capabilities deserve a first-class repair language rather than generic field patches?

## First implementation slice

Do not build the entire registry at once. Start with a read-only Rust registry for:

1. Campaign 2 visual capabilities;
2. terrain/world lowering;
3. fixed capture and visual evidence;
4. deterministic replay;
5. one external asset-provider seam.

The exit condition is a model-readable plan that can select, validate, execute, replay, fail, and repair one complete authored scene without knowing backend implementation names.
