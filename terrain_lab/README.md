# Codeweald Terrain Lab

Julia owns numerical terrain experiments; it does not own world authority or
engine rendering. The first executable seam analyzes the canonical Float32
heightfield and separates steep edges inside authored non-traversable relief
from steep edges in nominally accessible terrain.

```bash
julia --project=. -e 'using Pkg; Pkg.instantiate()'
julia --project=. test/runtests.jl
julia --project=. bin/analyze_heightfield.jl \
  --heightfield ../godot_renderer/concept_batches/codeweald_alpine_arena_v1/terrain/heightfield_f32le.bin \
  --protected-mask ../godot_renderer/concept_batches/codeweald_alpine_arena_v1/terrain/protected_relief_mask.bin \
  --semantic-region-mask ../godot_renderer/concept_batches/codeweald_alpine_arena_v1/terrain/semantic_region_mask.bin \
  --manifest ../godot_renderer/concept_batches/codeweald_alpine_arena_v1/terrain/terrain_manifest.json \
  --output ../godot_renderer/concept_batches/codeweald_alpine_arena_v1/terrain/terrain_analysis.json
```

The output is evidence, not permission to mutate terrain. Rust validates its
provenance and policy; a later optimizer must demonstrate better accessible
metrics while preserving protected relief, traversal, and source fidelity.
