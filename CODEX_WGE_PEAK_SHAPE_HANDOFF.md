# WGE Peak-Shape Handoff

Date: 2026-09-28
Scope: WGE engine quality and model-native authoring through the native WGE
runtime.

## Current truth

The engine-neutral native MVP path is now green for a fresh, source-bound
vertical slice. Rust owns typed intake validation, project-spec compilation,
world/gameplay/asset/visual/repair receipts, authority promotion, candidate
identity, and snapshot verification. Julia remains behind the Rust-supervised
numerical path. Python stages provider input and packages outputs; it is not
semantic authority.

The certified handoff contains a compiled project spec and template, authored
layout plus Julia/Rust world artifacts, collision/navigation/spawn/encounter
semantics, deterministic reference-runtime traversal evidence, bounded asset
preparation/rigging evidence, deterministic visual captures, repair evidence,
and a revalidated archive. Native authority independently revalidates every
promotable receipt and recompiles the packaged spec before promotion.

The supplied GLB remains a permanent negative control and is rejected. The
known-good rigging input is a structural/provider control; arbitrary
mesh-to-character generation and production-quality skinning/retargeting remain
explicit scope gates rather than implied passes. External-editor/runtime
comparison is not part of the WGE certification scope.

## Verified failure points

### Closed / permanent negative control — the supplied GLB is not a rigging input

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

The native path now has a typed provider boundary and a known-good structural
control, but it still does not claim arbitrary mesh repair, high-quality rig
creation, skinning, or retargeting from this asset.

### Closed — the vertical slice is a real engine-neutral reference runtime

The accepted path consumes a fresh authored layout, Julia numerical/spatial
fields, and Rust-owned world artifacts, then emits collision, navigation,
spawns, encounters, gameplay binding, traversal evidence, and deterministic
reference-runtime telemetry. The acceptance control is intentionally small,
but it is no longer the checked-in Riverwatch fixture.

### Closed — receipt authority is native and typed

Every promotable gate binds to a registered Rust validator and schema. Native
MVP promotion rejects missing or mismatched project-spec bindings, status-only
or producer-only evidence, stale artifacts, malformed receipts, and forged
runtime/visual evidence. A negative authority test covers removal of the
compiled spec/template binding.

### Narrowed — brief/concept/doc intake is typed, but interpretation quality is provider-dependent

The intake boundary now accepts provider-neutral typed observations, inferences,
assumptions, confidence, conflicts, regions, and provenance, with source-byte
binding and known-good/known-bad controls. The contract validates provenance
and consistency; it does not pretend to judge subjective vision-model quality.

### Closed for MVP — repair is typed, bounded, and evidence-compared

The native repair contract diagnoses a failed semantic layer, validates an
authorized proposal against exact before-bytes, applies only bounded layout
changes, rebuilds/re-measures, and records before/after evidence and the
improvement rationale. Broader autonomous content repair remains future work.

### Closed for MVP — visual evidence is an observed fail-closed gate

The native path produces deterministic reference captures at defined camera
parameters and measures engine-neutral visual properties. Reference tooling may
produce the evidence, but Rust owns validation and promotion; visual failure is
not downgraded to a warning.

### Ongoing boundary — legacy Python authority remains outside the native MVP path

The broader repository still contains legacy Python-owned world/build/acceptance
modules identified in `AUTHORITY_RECLAMATION_001.md`. The native MVP does not
add semantic behavior there. Continue reclaiming or fencing those modules as
future slices require them.

## Remaining R&D questions

1. **Asset quality boundary:** What provider and evidence standard is sufficient
   for production-quality topology, skinning, retargeting, contacts, sockets,
   collision, LODs, and semantic parts? The current native control proves the
   contract, not arbitrary character generation.
2. **Interpretation quality:** How should multiple concept/document providers be
   compared on conflicts, confidence calibration, region grounding, and
   subjective visual fidelity while preserving the typed provenance contract?
3. **World breadth:** Which additional terrain, encounter, and gameplay systems
   materially improve the model-facing harness before expanding beyond one
   vertical slice?
4. **Repair breadth:** Which bounded repair classes should follow the current
   layout repair, and what domain-specific evidence proves each repair improved
   the candidate rather than merely changing it?
5. **Visual bar:** Which reference captures and metrics are sufficient for the
   intended quality bar across silhouette, composition, palette, materials,
   lighting, animation, and readability?

## Completed native checkpoint

1. Rust receipt-validator registry and typed project-spec binding are enforced
   at the native authority boundary.
2. A fresh authored/Julia/Rust world path produces collision, navigation,
   traversal, gameplay, and deterministic reference-runtime evidence.
3. Provider-neutral source intake carries observations, inferences, assumptions,
   conflicts, confidence, regions, and provenance into a compiled spec.
4. Typed bounded repair applies only authorized changes and compares evidence
   before and after the rebuild.
5. Deterministic visual evidence, snapshot manifests, safe archive reopening,
   and independent authority revalidation are acceptance gates.
6. Rigging-dependent evidence remains explicitly deferred until the character
   research and materialization policy are complete.

## External decisions / blockers

- No blocker remains for the engine-neutral native MVP checkpoint.
- A production-quality character provider and explicit skinning/retargeting
  acceptance policy are still needed for a future character-content slice. The
  supplied cube-like GLB cannot satisfy that requirement.
- No external editor, runtime, or comparison harness is required for this
  checkpoint. Do not fabricate subjective visual-quality numbers.

## Evidence at hand

- `python3 -m unittest tests.test_wge_native_mvp -v`: passed, including fresh
  intake, native certification, archive revalidation, bad-GLB rejection, and
  archive path-traversal rejection.
- `python3 -m unittest discover -s tests`: 683 tests passed.
- `cargo test --offline --workspace`: passed across the Rust authority, ledger,
  gameplay, asset, intake/repair, reference-runtime, and semantic-kernel suites.
- `cargo fmt --all -- --check` and `cargo clippy --offline --workspace
  --all-targets -- -D warnings`: passed.
- `julia --project=. test/runtests.jl`: passed, 8/8 numerical/hydrology checks.
- Kepler read-only adversarial audit: green; no remaining blocker in the native
  MVP scope.
