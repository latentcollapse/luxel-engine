# Luxel Style Profile Contract

Status: **typed profile/plan slice implemented; broader lowering remains**, 2026-09-30  
Scope: converting concept art, briefs, and visual constraints into typed, provenance-bound style intent and executable rendering/content policy

This contract defines visual style as a multidimensional semantic object. It deliberately avoids treating a named game, studio, or genre as a complete style specification.

Related sources:

- [Luxel Capability Registry Design](../platform/capability-registry-design.md)
- [Autonomous Game-Construction Forensics](../archive/2026-09_roadmaps-and-audits/autonomous-game-construction-forensics.md)
- [Luxel Native Quality Gaps](native-quality-gaps.md)
- [Luxel Graphics Campaign 2 Report](../archive/2026-10_graphics-sprint-reports/graphics-campaign-2-report.md)
- [Luxel Generality Bridge](../content-sdk/generality-bridge.md)

## Current implementation slice

The typed Rust implementation lives in
`world_core/crates/luxel_control_plane/src/style_profile.rs`. It provides
content-digested `StyleProfile` and `StylePlan` records, explicit evidence
epistemics, bounded basis-point signals, conflict/region/provenance checks,
and a backend-neutral lowering over the currently registered capabilities.

The native control plane exposes the read-only lowering command:

```bash
cargo run --manifest-path world_core/Cargo.toml --offline -p luxel-control-plane -- style-validate PROFILE.json
cargo run --manifest-path world_core/Cargo.toml --offline -p luxel-control-plane -- style-lower PROFILE.json
```

The Python/MCP transport exposes the same operation as `style_compile`. A
validated StylePlan can now be consumed by the Rust-owned ConstructionPlan
slice through `project_plan`; this does not execute renderer state or claim
unsupported reflection,
volumetric, outline, or posterization axes as satisfied; those remain explicit
in `StylePlan.unsupported_axes`.

## Design thesis

Style is a coordinated policy over geometry, surfaces, lighting, environment, camera, animation, effects, composition, and budgets.

```text
brief / concept references
    -> observations and inferences
    -> StyleProfile
    -> StylePlan
    -> capability selection and typed lowering
    -> render/content packets
    -> multidimensional evidence
```

`StyleProfile` is semantic intent. `StylePlan` is an authority-validated lowering of that intent. A graphics packet is an execution artifact. These identities must never collapse into one backend representation.

## What the profile is and is not

`StyleProfile` is:

- typed and versioned;
- region-aware where references support regional interpretation;
- explicit about observations, inferences, assumptions, conflicts, and confidence;
- bounded by performance, memory, platform, and content constraints;
- independent of a renderer backend;
- suitable for deterministic reconstruction when its inputs are deterministic.

`StyleProfile` is not:

- a single aesthetic score;
- a brand-name alias with undocumented meaning;
- a shader graph;
- a collection of raw GPU handles;
- permission for a visual critic to mutate canonical project state;
- proof that an image satisfies the requested style.

## Proposed semantic shape

The eventual Rust type should carry the following conceptual fields:

```yaml
schema_version: luxel.style-profile/v1
profile_id: style-profile-...
source_project_id: project-...
intent:
  geometry: {}
  surfaces: {}
  lighting: {}
  environment: {}
  camera: {}
  animation: {}
  effects: {}
  composition: {}
  budgets: {}
constraints: []
observations: []
inferences: []
assumptions: []
conflicts: []
regions: []
provenance: []
confidence: {}
```

Every section may be partial. Missing evidence is not silently filled with a default that pretends to be observed intent. A default may be applied only when the contract declares it as an assumption and retains its provenance.

## Style dimensions

### Geometry language

Represent observable policy rather than a vague realism label:

- silhouette complexity;
- proportion exaggeration;
- edge softness and bevel prevalence;
- faceting or smoothness;
- shape-frequency distribution;
- hard-edge versus smooth-edge policy;
- acceptable geometric noise;
- LOD simplification policy;
- hero/background geometry budget.

### Surface language

At minimum:

- base-color palette and value separation;
- saturation range;
- roughness distribution;
- metallic usage;
- normal/detail frequency;
- clearcoat or sheen intent;
- emissive usage;
- texture scale and repetition tolerance;
- wetness, dirt, wear, and variation policies;
- alpha/opacity policy.

### Lighting language

Represent:

- key/fill/ambient relationship;
- directional versus local-light emphasis;
- shadow hardness and softness;
- contact-shadow expectation;
- color temperature and chromatic contrast;
- environment intensity;
- exposure range;
- light hierarchy and priority;
- whether a lighting property is measured, inferred, or merely requested.

### Environment and atmosphere

Represent:

- sky and horizon character;
- ground/environment illumination;
- fog color, density, and height behavior;
- aerial perspective;
- weather and volumetric intent;
- atmospheric depth cues;
- reflection/probe expectations;
- time-of-day and seasonal constraints.

### Camera and composition

Represent:

- projection type;
- focal length or field-of-view range;
- camera height and distance;
- framing and subject priority;
- horizon placement;
- depth-of-field intent;
- motion and stabilization policy;
- exposure and tone-mapping intent;
- density and negative-space balance;
- landmark and readability constraints.

### Animation language

Represent separately from rig identity:

- timing and anticipation;
- interpolation style;
- pose exaggeration;
- squash/stretch policy;
- foot/contact expectations;
- secondary motion;
- responsiveness versus inertia;
- stepped, smooth, or mixed timing;
- animation budget and update policy.

### Effects and presentation

Represent:

- bloom, outlines, posterization, sharpening, and film-response intent;
- particles, trails, decals, and environmental effects;
- UI shape, density, contrast, and motion language;
- audio-reactive or event-reactive presentation if applicable.

## Typed values and provenance

Continuous values should not be naked floats. Use bounded semantic values with a source and confidence, conceptually:

```yaml
value: 0.72
domain: [0.0, 1.0]
meaning: silhouette_exaggeration
source_kind: observed | inferred | assumed | requested | repaired
source_refs: [reference-region-03]
confidence: 0.81
```

Categorical values should have an explicit vocabulary and an `unsupported` outcome. A named style may be accepted as a retrieval hint, but the compiled profile must contain the explicit policies it resolved to and the unresolved portions must remain visible.

## Intake semantics

Concept-art and document intake should produce separate records for:

- `Observation`: directly measurable or visibly present fact;
- `Inference`: interpretation derived from observations;
- `Assumption`: chosen to close an underspecified request;
- `Constraint`: hard requirement or prohibition;
- `Conflict`: incompatible references or instructions;
- `Region`: image/document area supporting a claim;
- `Provenance`: source identity, location, and extraction method;
- `Confidence`: confidence in the claim, not confidence in the final render.

Example:

```yaml
kind: inference
field: surfaces.roughness_range
value: compressed
source:
  artifact_id: concept-art-001
  region: region-07
derived_from: [observation-12, observation-19]
confidence: 0.68
conflicts: []
```

The profile compiler must not convert a low-confidence inference into an unmarked hard constraint.

## StylePlan lowering

`StylePlan` is the Rust-validated answer to “how will this intent be realized in the current Luxel capability set?” It should include:

- selected capability IDs and versions;
- material family assignments;
- geometry/LOD policies;
- lighting and environment policies;
- camera/exposure policy;
- animation policy references;
- asset-provider requirements;
- resource budgets;
- unsupported or indeterminate intent;
- expected evidence axes;
- plan identity and input identities.

A plan may say:

```yaml
requested: prefiltered_environment_reflections
status: unavailable
reason: capability_not_certified
fallback: none
```

It must not silently substitute a different reflection model and report the request as satisfied.

## Authority and execution

```text
Palette/Cyan or another model-facing proposer
    -> StyleProfile proposal
    -> Rust schema/conflict/budget validation
    -> Rust StylePlan lowering
    -> Julia/Lava typed graphics packet
    -> capture and raw measurements
    -> Rust visual evidence validation and promotion
```

The visual critic may:

- identify a failed or weak axis;
- cite the supporting pixels, packet fields, and receipts;
- propose a bounded StyleProfile or StylePlan repair;
- estimate expected improvement and cost.

The visual critic may not:

- promote its own evidence;
- mutate canonical style or world state directly;
- hide an indeterminate axis;
- rewrite source identity to make a repair appear deterministic;
- lower a failed gate into a warning.

## Evidence vector

The profile should map to a vector, not a scalar:

- silhouette readability;
- material separation;
- grounding/contact;
- lighting consistency;
- atmospheric depth;
- texture frequency;
- composition;
- density/population;
- temporal stability;
- artifact rate;
- frame cost;
- memory cost;
- animation coherence when applicable.

Each axis has one of:

```text
measured
indeterminate
failed
passed
```

`indeterminate` is a valid result when the current evidence system cannot judge the property. It is never equivalent to `passed`.

## Repair semantics

A style repair proposal must identify:

- failed axis;
- evidence and receipt identities;
- target fields;
- authorized mutation scope;
- preserved fields;
- expected metric movement;
- cost/memory impact;
- rebuild command;
- before/after comparison rule.

Example:

```yaml
repair_id: repair-...
failed_axis: material_separation
authorized:
  - surfaces.material_family.terrain.roughness_range
  - surfaces.texture_frequency.terrain
preserve:
  - world_identity
  - camera_identity
  - gameplay_state
expected:
  distinct_rgb_bins: increase
  artifact_rate: no_increase
```

The repair is promoted only if the independent evidence comparison supports the diagnosis and no protected identity changed.

## Initial implementation slice

The first implementation should cover only:

1. authored-frame geometry/material/lighting/camera intent;
2. concept-art observations and provenance;
3. explicit unsupported/indeterminate fields;
4. Rust-owned plan identity;
5. lowering to the existing native graphics packet;
6. the existing multidimensional visual evidence path.

Do not begin with a universal style embedding, a learned aesthetic scalar, or a complete post-processing vocabulary. The contract should first prove that a model can request a coherent style, receive an inspectable plan, and repair a failed visual axis without bypassing authority.

## Open questions

- Which style dimensions can be measured reliably from references and captures?
- Which dimensions require human-labelled calibration data before automated judgement is honest?
- How should regional style conflicts be resolved or surfaced to the model?
- How should style profiles compose across hero asset, terrain, foliage, UI, and VFX?
- Which policies belong in Palette versus Luxel semantic contracts?
- How should style intent survive asset-provider substitution?
- What temporal evidence is required before animation style can be promoted?
