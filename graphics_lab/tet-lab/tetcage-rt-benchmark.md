# TetCageRT Benchmark Plan — envelope first, promotion never

Doctrine: the benchmark measures the OPERATING ENVELOPE. It cannot issue a
promotion receipt (roadmap forbids benchmark-only evidence becoming a passing
gameplay/render receipt). Phases follow the roadmap ladder: CPU oracle before
GPU.

## Phase 0 — paper-anchored model (DONE)
`tetcage_weight_model.jl`: deterministic, calibrated (grass anchor exact),
law validated ±25% across six Table-1 cages, N-sweep in
`results/weight-model-sweep.csv`. Absolute-MB caveat F1 applies.

## Phase 1 — CPU oracle (Julia, this lane)
1. Voxel cage generation (6 tets/voxel, Freudenthal) on deterministic
   procedural test meshes + one real WGE conifer proxy mesh.
2. Triangle clipping against tet planes with vertex dedup + sharing;
   measure expansion; assert inside Table-1 band (1.36–2.31× tris) or
   explain deviation for WGE geometry.
3. Rest-basis encoding (Eq. 1); deformation via cage bone weights;
   reconstruction vs ground-truth linear-blend skinning on probe poses.
4. Watertightness probes: escape-rate vs ε sweep on thin card geometry
   (F7) and the paper's sphere reproduction as a cross-check.
5. Boundary duplicate-intersection census (F3).
Accept: deterministic (fixed seeds, no RNG), hash-reproducible artifacts.

## Phase 2 — GPU prototype (Lava/Vulkan, behind capability flag)
`graphics.geometry.tetcage.experimental/v1`; instance-transform variant
only; static μBLAS + per-frame tetLAS rebuild; explicit capability report;
no canonical path contact. Measure: tetLAS rebuild ms, μBLAS memory,
render ms vs conventional animated BLAS on identical scene.

## Phase 3 — comparison matrix (roadmap step 4)
Axes: deformation error, AS memory, animation cost, AS update cost, trace
cost, total frame time, preprocessing expansion, cage resolution.
Sweeps: cage resolution × deformation severity × distance × mesh density ×
animation density. Cold/warm + deterministic replay. Conventional fallback
runs in every cell. Output: operating envelope, not a verdict — the envelope
feeds the Rust materialization policy designed in
tetcage-rt-integration-contract.md.

## Explicit non-goals
No promotion claim, no canonical packet changes, no watertight-variant GPU
path (F2), no hero-character use case (verdict: DO NOT PROMOTE FOR).
