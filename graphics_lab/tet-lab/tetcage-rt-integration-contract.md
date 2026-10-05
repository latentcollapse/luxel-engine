# TetCageRT Integration Contract — DESIGN ONLY

Status: research lane. Nothing here modifies canonical WGE. Adoption requires
the mega-sprint §17 entry gate (conventional path exposes a measured problem)
plus a Rust-owned materialization policy. This document pre-builds the
contract shape so later integration is additive.

## Object: `TetCageCandidate`

```text
TetCageCandidate {
  source_asset_identity      : digest of the imported asset (C2 contract)
  source_mesh_identity       : digest of the exact rest-pose mesh used
  animation_identity         : digest of the deformation source (bones, wind
                               field, keyframe set) that drives the cage
  cage_topology {
    resolution               : voxel grid dims
    tetrahedra               : count, non-degeneracy proven at build time
    avg_tris_per_tet         : must satisfy the savings law (>= s/0.309)
  }
  encoded_geometry_artifact  : per-tet muMesh digests (rest pose, clipped,
                               deduplicated, never mutated afterwards)
  error_bounds {
    deformation_probe_max    : max vertex deviation vs ground-truth skinned
                               mesh over a probe animation set
    watertight_escape_rate   : measured, with epsilon growth value
  }
  memory_estimate            : fixed (muMeshes+muBLASes) + per-copy (cage+instances)
  capability_requirements    : instance-transform RT support, BLAS flags used
  legal_use_envelope         : explicit class list from the research verdict;
                               unknown -> "unsupported", never guessed
  provenance                 : producer, validator schema ids, seeds, timestamps
}
```

## Decision flow (authority preserved)

```text
Rust RepresentationPolicy (owns WHETHER)
    inputs: TetCageCandidate + quality profile + gameplay role semantics
    outputs: CONVENTIONAL | CLUSTERED | TETCAGE_EXPERIMENTAL
    "unsupported" / "uncertain" / "not profitable" are explicit outcomes
        |
Julia/Lava executor (owns HOW)
    coarse-grained packet in: encoded cage + animation identity
    packet out: frames / evidence digests
    no semantic authority, no per-frame FFI choreography
```

Invariants (inherited from roadmap research-track rules):
1. Source mesh stays canonical; TetCage is a lowering, never authority.
2. Conventional fallback exists for every comparison, permanently.
3. μBLAS reuse across uniquely-animated copies is the memory thesis —
   copies share encoded geometry, only cage vertices are per-copy.
4. Evidence is independently recomputable; benchmark-only artifacts can
   never satisfy a gameplay/render receipt.
5. Capability flag: `graphics.geometry.tetcage.experimental/v1` — default off,
   fails closed, prints explicit capability report on unsupported hardware.

## Mapping to GPU (from paper §6 [PAPER])

Instance variant: one DXR/Vulkan instance per tet; instance transform =
O·A·(M⁻¹); static μBLAS = ordinary triangle BLAS; tetLAS = TLAS rebuilt per
frame. Watertight variant: procedural intersection + 4D-BVH software
traversal — reference/validation only at current hardware generation.

## Validation ladder additions (extends WGE_NATIVE_GRAPHICS_ARCHITECTURE.md)

- R: policy unit tests — negative controls for degenerate tets, inflated
  error bounds, unknown animation identity, envelope violations.
- Julia: deterministic cage build + clipping reference; expansion factor
  must match Table-1 band (1.36–2.31× tris) or explain why WGE geometry
  differs; deformation probes vs ground truth skinning.
- Adversarial: forged candidate, cage built from wrong mesh digest, reuse
  count lied in memory estimate, epsilon omitted (watertight escape probe).
