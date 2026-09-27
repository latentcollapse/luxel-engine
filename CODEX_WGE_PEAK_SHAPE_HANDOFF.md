# WGE Peak-Shape Handoff

Date: 2026-09-27
Scope: WGE engine quality and model-native authoring; Unity comparison deferred.

## Current truth

The Rust/Julia/Python boundary is materially healthier: Rust owns the project
ledger, world acceptance, gameplay contract, asset acceptance, receipts, and
identity; Julia is behind the Rust-supervised numerical path; Python is
orchestration and transport.

The offline candidate is deterministic and inspectable. Semantic specification,
world/navigation validation, and the Rust gameplay loop pass. The candidate
correctly refuses certification because the supplied asset is unusable and no
target-runtime evidence exists.

The conventional Unity+MCP benchmark is intentionally deferred. Do not spend
work on Unity licensing, target-runtime comparison, or post-MVP breadth until
the engine-native WGE shape below is stronger.

## Verified failure points

### P0 — The supplied GLB is not a rigging input

`/home/mattc/Pictures/Generated 2D Images/sample_2026-09-26T091412.074.glb`

- 13,980,532 bytes; SHA-256
  `858fa104880822d081405579fb5b39d533d3b3b341d38aa1490a44b634f5e2b4`.
- No skins, animations, `JOINTS_0`, or `WEIGHTS_0`.
- The bounded Rust asset contract rejects it for invalid skin, missing required
  animations/socket/LOD, collision bounds, and unsupported required
  `EXT_texture_webp`.
- Blender preview is a cube-like render, not a defensible humanoid/character
  asset. Auto-attaching a skeleton or substituting a primitive would violate
  the quality bar and provenance doctrine.

The current asset system validates a prepared asset; it does not create a
high-quality rig, retarget animation, repair topology, or establish semantic
parts from an arbitrary mesh.

### P0 — The vertical slice is not yet a real engine-neutral runtime

The Rust gameplay contract is real and deterministic, but the WGE candidate
world is still a compact fixture: a flat terrain manifest, four authored route
nodes, and declarative collision/navigation data. It is not yet the full
terrain-build → collision → traversal runtime path that the roadmap describes.

### P0 — Evidence envelopes are stronger than the current domain registry

The ledger verifies receipt identity, artifact digests, gate coverage, and
canonical references. The canonical Python orchestration invokes the native
Rust contracts first. However, the generic ledger does not independently parse
and revalidate every gameplay/asset receipt payload. A caller with access to
the low-level API could construct a pass-shaped receipt around a bad artifact.
The next authority seam must bind each gate to a registered native validator,
not merely to a producer string and a signed envelope.

### P1 — Brief/concept/doc intake is still fixture-driven

The MVP command copies a known brief and emits hard-coded claims, assumptions,
style targets, and constraints. It demonstrates the schema, but not the
general path from arbitrary concept art and design documents to a typed,
provenance-backed semantic project specification.

### P1 — Repair is rebuild, not diagnosis-to-repair

The current repair command detects stale bytes and failed gates, then rebuilds
from pinned inputs. It does not yet produce typed source-level repair proposals,
apply a bounded repair, compare the new evidence against the old candidate, or
explain why the responsible semantic layer changed.

### P1 — Visual quality is not yet an observed gate

The concept-art reference is recorded, but no candidate capture is compared at
the player camera/distance. Bevy remains the reference-renderer instrument;
visual measurement must remain evidence-producing tooling while Rust owns the
promotion policy.

### P1 — Legacy Python authority remains outside the narrow MVP path

The current MVP wrapper is glue, but the broader WGE repository still contains
large Python-owned world/build/acceptance modules identified in
`AUTHORITY_RECLAMATION_001.md`. Do not add new semantic behavior there. Reclaim
or fence the next load-bearing modules behind typed Rust/Julia contracts.

## R&D questions that need answers

1. **Asset creation boundary:** Is WGE expected to repair/generate a character
   from an unrigged mesh, or must it require a provider-produced riggable asset?
   What evidence is sufficient for semantic part assignment, skeleton choice,
   deformation quality, foot contacts, sockets, collision, and LODs?
2. **Provider protocol:** What is the typed request/response contract for
   Blender, an auto-rigger, a model-generation system, or a retargeter? Which
   bytes are provider output, and which acceptance decisions must remain Rust?
3. **Concept interpretation:** How are observed claims separated from model
   inferences? How are confidence, regions, conflicts, assumptions, and source
   provenance represented so an agent can repair a bad interpretation without
   rewriting the whole spec?
4. **World authority:** What is the smallest real terrain/world build path that
   turns Julia fields plus authored layout into collision, navigation, spawns,
   encounter volumes, and traversal evidence without making Python semantic?
5. **Receipt registry:** Should each gate register a Rust validator and typed
   receipt schema, with the ledger refusing unregistered producers or
   status-only evidence?
6. **Repair model:** What bounded repair classes should exist first—asset
   preparation, navigation, collision, gameplay contract, or spec claims—and
   what observation proves each repair improved the candidate rather than merely
   changing it?
7. **Visual bar:** Which deterministic reference captures and metrics are
   required for silhouette, composition, palette, material response, lighting,
   animation, and readability before a slice can be called high quality?
8. **Milestone definition:** For the next checkpoint, is “WGE-certified” an
   engine-neutral snapshot with reference-runtime evidence, while Unity import
   remains a later adapter gate? This is the recommended interim definition.

## Immediate execution order

1. Freeze the interim milestone as **engine-neutral WGE certification**; keep
   Unity import/build/playthrough and the baseline comparison deferred, never
   silently passed.
2. Add a Rust receipt-validator registry and make the ledger consume typed
   gameplay, asset, world, visual, and repair receipts.
3. Build the smallest real world path: authored/Julia terrain fields → Rust
   world artifact → collision/navigation/traversal evidence → deterministic
   reference runtime.
4. Run an asset R&D spike with a genuinely riggable source asset. Keep the
   supplied GLB as a permanent rejection control; do not use it as a character
   through substitution.
5. Replace fixture claims with a provider-neutral concept/document intake
   contract, then add known-good/known-bad interpretation controls.
6. Add typed repair proposals and evidence deltas before broadening content
   generation.

## External decisions / blockers

- A suitable riggable character source, or an explicit decision to build a
  high-quality model/rigging provider, is required for the content-pipeline
  slice. The supplied cube-like GLB cannot satisfy it.
- Unity licensing and a connected UnityMCP Editor are required only when the
  deferred target-runtime phase resumes. They are not a reason to weaken the
  current WGE engine-native gates.
- Do not fabricate visual/runtime receipts or benchmark numbers while either
  dependency is absent.

## Evidence at hand

- Final candidate: `/tmp/wge-mvp-final-0927b`.
- `cargo test --offline --workspace`: passed.
- Ledger format/clippy checks: passed.
- Focused MVP, benchmark, asset transport, Julia-worker, and repair tests:
  passed.
- Final `verify-all`: correctly exits `2`; semantic, navigation, and gameplay
  pass; asset fails; visual/Unity/runtime remain indeterminate.
