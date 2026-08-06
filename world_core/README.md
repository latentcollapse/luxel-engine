# Codeweald World Core

The World Core is the deterministic, engine-neutral boundary for Codeweald
worlds.  It consumes the existing `codeweald.zone-spec/v1` contract and does
not replace Godot rendering, GDScript adapters, or the Odin match simulator.

Its first crate, `codeweald-worldspec`, validates a compiled ZoneSpec and emits
a canonical SHA-256 fingerprint.  This establishes reproducible world identity
before terrain compilation and evaluation logic move across from the Python
reference pipeline.

A solver-facing placement plan must bind itself to both the ZoneSpec and the
resolved asset plan fingerprints. Rust rejects unknown features/assets,
duplicate placements, out-of-bounds transforms, ungrounded Y positions, and
invalid scale. Julia is therefore free to optimize geometry while the result
remains auditable and deterministic.

```bash
cargo run -p codeweald-worldspec -- validate \
  ../godot_renderer/concept_batches/caledonia_v1/zone_spec.json

cargo run -p codeweald-worldspec -- validate-plan \
  ../godot_renderer/concept_batches/caledonia_v1/zone_spec.json \
  ../godot_renderer/concept_batches/caledonia_v1/asset_plan.json \
  placement_plan.json
```
