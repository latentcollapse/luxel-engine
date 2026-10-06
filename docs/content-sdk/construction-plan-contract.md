# Luxel ConstructionPlan Contract

Status: **typed Rust planning slice implemented; semantic execution remains open**, 2026-09-30  
Scope: resolving a model's game-construction intent into an explicit, deterministic, authority-bound build plan.

Related sources:

- [Active Architecture](../platform/active-architecture.md)
- [Capability Registry Design](../platform/capability-registry-design.md)
- [Style Profile Contract](../world/style-profile-contract.md)
- [Gameplay Kit Architecture](../gameplay/gameplay-kit-architecture.md)
- [Demo-Ready Native Engine Mega-Sprint](../archive/2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md)

## Purpose

`ConstructionPlanDraft` is the model-facing proposal. `ConstructionPlan` is the
Rust-owned resolution of that proposal against the registered capability and
validator registries. It answers:

- what project and brief are being built;
- which capabilities are required and what status each currently has;
- which assets and bounded provider jobs are needed;
- which world systems and gameplay kits are part of the slice;
- which StylePlan is bound to the construction decision;
- what evidence must prove completion;
- which unsupported requirements block or merely qualify the plan.

The plan is a planning artifact, not permission to execute arbitrary code. It
does not invoke Blender, Julia, Lava, GPU resources, or gameplay mutation.

## Ownership and runtime rule

Rust owns plan identity, resolution, readiness, registry bindings, validator
bindings, and the content digest. A provider job may be proposed here, but it
must be explicitly fenced from runtime (`runtime_forbidden: true`). Provider
output must later re-enter through the typed asset/scene contracts.

Julia/Lava and external providers never become canonical plan state. Python may
transport the draft and plan through the bounded agent/MCP surface, but does not
interpret readiness or decide whether a requirement is satisfied.

## Readiness

```text
Ready   = no blockers and no advisories
Partial = no blockers, but candidate/experimental capabilities or soft gaps
Blocked = at least one required unavailable/retired capability, unfenced
          provider job, hard unsupported requirement, or invalid authority binding
```

This distinction is deliberate. A plan that can be constructed with a known
candidate capability is not silently promoted to a certified plan, and an
unsupported hard requirement cannot be hidden behind a successful JSON shape.

## Current native commands

```bash
cargo run --manifest-path world_core/Cargo.toml --offline -p luxel-control-plane -- \
  project-plan DRAFT.json STYLE_PLAN.json

cargo run --manifest-path world_core/Cargo.toml --offline -p luxel-control-plane -- \
  construction-validate PLAN.json STYLE_PLAN.json
```

The model-facing transport maps these commands to `project_plan` and
`construction_validate`. Both accept only project-root-relative resource paths;
the Rust control plane remains the semantic authority.

## Evidence and stale-input behavior

The plan binds to:

- the native capability registry identity and digest;
- the current engine-neutral validator registry digest;
- the exact StylePlan identity and digest;
- each resolved capability's current version and status;
- its own content digest.

Validation rejects stale capability status/version, stale validator registry,
stale StylePlan binding, duplicate or malformed requirements, unknown validators,
gate/validator disagreement, unfenced provider jobs, and tampered plan content.

The plan is not itself a certification receipt. Later construction, world,
asset, gameplay, visual, repair, and packaging receipts must still be promoted
through the registered native validators.

## Deliberately deferred

This slice does not yet execute `world.construct`, `scene.compose`,
`asset.prepare`, `character.prepare`, or `gameplay.compose`. It also does not
infer a StyleProfile from images or briefs. Those are subsequent facade and
execution slices; the current contract makes their prerequisites and gaps
inspectable before they are attempted.
