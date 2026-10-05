# TetCageRT Paper Reconstruction — GATE-2 artifact

Status: research-only, quarantined lane (`graphics_lab/TetLab/`). NOT canonical WGE.
Mandate: reconstruct Gruen et al. 2026 exactly enough to answer Codex's ten
reconstruction questions, with every claim classified as
`[PAPER]` (source fact, anchored), `[INF]` (inference derived from pinned facts
by the executable model), or `[HYP]` (new WGE hypothesis, unproven).

## 1. Source pin (GATE-1, verified)

```text
Title     Ray Tracing Massive Amounts of Animated Geometry
Authors   Holger Gruen, Carsten Benthin, Michael Kern (AMD, Germany),
          David McAllister (AMD, USA)
Venue     Proc. ACM Comput. Graph. Interact. Tech. 9(4), Article 49,
          HPG 2026 (July 2026), 18 pages
DOI       10.1145/3820014
Award     Third-place Wolfgang Straßer Best Paper Award, HPG 2026
PDF       https://gpuopen.com/download/TetrahedralMeshes_AuthorsVersion.pdf
SHA-256   19289776f4eaa9851e98c09c5184a1c4e87650817ff458b4cc5131801750b739
Local     references/TetrahedralMeshes_AuthorsVersion.pdf (15,108,045 bytes,
          13 pages, verified "PDF document, version 1.5")
Text      references/paper.txt (pdftotext -layout, 851 lines)
Article   https://gpuopen.com/learn/ray-tracing-massive-amounts-animated-geometry/
          (2026-07-22; JS-rendered, machine-read failed — see failure register F4)
Press     wccftech.com 2026-09-20 (numeric anchors cross-check)
HN thread news.ycombinator.com/item?id=49021007 (mechanics cross-check;
          commenter a_e_k discloses working with the authors)
```

Genealogy `[PAPER]`: Luton & Tricard, HPG 2025 ("Real-time rendering of
animated meshless representations") applied the cage-deform/ray-or-geometry-
transform idea to rasterizing meshless geometry; this paper extends it to ray
tracing of skinned triangle meshes. AMD is preparing DXR samples and a
header-only C++ library.

Hardware/demo envelope `[PAPER]`: RX 9070 XT (RDNA4, 16 GB), DX12/HLSL,
1080p, two rays/pixel (primary + shadow), `ALLOW_UPDATE`+`PREFER_FAST_BUILD`
BLAS, `PREFER_FAST_TRACE` TLAS.

## 2. Algorithm reconstruction (Codex's ten questions)

**Cage generation** `[PAPER]`: voxel approach — regular grid over rest-pose
object → drop empty voxels → split each remaining voxel into 6 tetrahedra
(Freudenthal 1942 Kuhn subdivision) → drop tets that intersect no geometry.
No fixed triangle budget per tet; triangles/tet determined by cage resolution
and geometry distribution. Alternative tetrahedralization methods (TetGen,
bubble meshes) are cited but not used.

**Triangle clipping** `[PAPER]`: every original triangle is clipped against
the four bounding planes of each intersecting tet → one μMesh per tet.
Clipping is mandatory, not optional: it establishes the bounding property
(all μMesh vertices inside their tet volume; vertices shared with neighbors
sit exactly on the shared bounding triangle). Clipped vertices are
deduplicated and shared across adjacent tets. Consequence: 1.36–2.31×
triangle expansion, 1.42–3.71× vertex expansion (Table 1, exact numbers in
§5 below).

**Barycentric encoding** `[PAPER]`: rest-pose tet vertices v0..v3 define a
non-orthogonal basis M (Eq. 1); μMesh vertices are expressed relative to it.
The watertight variant goes further: 4D barycentric coordinates (Eq. 3,
p = Σ bᵢvᵢ) for ALL μMesh positions AND BVH node bounds, with a global
vertex-ID sort order for bit-exact shared reconstruction, snapping thresholds
for near-degenerate cases, and a "4D BVH" whose AABBs are reconstructed
on-the-fly in world space during traversal (min-sum interval heuristic,
Nießner 2013; Appendix A proves the bounding property survives ANY tet
deformation, including inversion/zero-scale/mirror).

**Canonical/rest coordinates** `[PAPER]`: all μMeshes and μBLASes are built
and stored in rest pose and never change. The animated copy is a `tetAnimCopy`
= cage vertices only.

**Deformation** `[PAPER]`: animate tetMesh vertices only — either direct
transform functions or bone-based: for each cage vertex take the bone IDs/
weights of the closest original vertex; clip bone data along with triangles
(≤12 unique bone IDs per tri, zero-weight fill, re-normalize top-4 after
clipping). Shading skin-fetches the three ORIGINAL (undeformed) vertex
normals of the hit triangle and skins those normals at shade time.

**Ray transformation** `[PAPER]`: ray traced through tetLAS; on tet hit,
transform ray world→tet-local and intersect the static μBLAS. Instance
variant: instance transform = O·A·(M⁻¹)₃ₓ₄, where M is the rest basis (Eq. 1,
static, precomputed per tet) and A the animated basis (Eq. 2, recomputed per
frame in a compute shader); O combines per-tet static offsets. Normals need
the inverse-transpose. Watertight variant: ray stays in world space;
geometry+bounds are reconstructed from 4D into world space inside a DXR
procedural-intersection shader.

**Intersection behavior** `[PAPER]`: shared edges can be intersected
multiple times when adjacent tets deform differently (duplicate-hit risk at
boundaries); unique edge ownership is named as a mitigation. Watertightness
of the instance variant is statistical, not guaranteed: tets are grown
outward by ε = 2.5×10⁻⁶ along face normals before clipping; on the isolated
sphere test (r=100, ~1M tris → ~1.2M after clipping, 9×12×9 cage), escape
rate dropped from ~0.01% of rays to zero.

**Acceleration-structure layout** `[PAPER]`: two levels. μBLAS per tet
(DXR triangle BLAS, static, compaction possible, never modified) + tetLAS
over all tets (rebuilt every frame; each tet is a DXR instance → instance-
transform variant; or a DXR procedural-primitive instance → watertight
variant). Same object uniquely animated N times = ONE shared μBLAS set + N
tetAnimCopies.

**Preprocessing expansion** `[PAPER, Table 1]`: triangles 1.36× (Tree,
5×8×5) to 2.31× (Grass, 10×9×10); vertices 1.42× to 3.71×.

**Cage-resolution tradeoffs** `[PAPER]`: "Both resolutions provide
sufficient accuracy, so that no visible differences compared to animating
all triangles along the chosen camera path were observed." Higher resolution
= better animation fidelity + more memory + slower tetLAS update. Local
refinement (Fig. 12) fixes joint artifacts from insufficient resolution.

**Numerical failure modes** `[PAPER]`: (a) instance variant — clipping
float error can put clipped vertices slightly off the shared bounding
triangle, breaking the bounding property → misses near shared edges;
mitigated by ε-growth, guaranteed only by the 4D variant. (b) μBLAS instance
transforms must be invertible (M A⁻¹ must exist) → degenerate/inverted tets
forbidden. (c) duplicate primitive intersections at shared boundaries.
(d) Limitations list: topology changes (explosions/destruction/cutting),
sub-cage-scale high-frequency motion (cloth, facial), sharp skin-weight
changes near joints, rigid objects (plain instancing is better), keyframe
vertex animation (needs per-keyframe transform recomputation).

## 3. Measured anchors [PAPER, exact]

Table 1 (single object): six object×cage rows — see §5 output.
Table 2 (scene): Trees×25 / Grass×500 / Frogs×81 — tet non-watertight:
209.7 MB @ 2.96 ms, 145.6 MB @ 1.35 ms, 190 MB @ 1 ms. Watertight: 2.3–3.2×
memory, 19–80× render time (software 4D traversal). 
Fig. 10 (Grass 500): memory 16.1× smaller than standard; render time
approximately equal; total time up to 9× better.
Table 3 (combined 584M animated tris, 2.8M tets): anim 0.35 ms (3%), update
(anim+tetLAS) 9.66 ms (78%), render 2.42 ms (19%), total 12.43 ms ≈ 60 fps,
memory 770.10 MB.
GPUOpen demo (secondary source, press-verified): 25,000 independently
animated plants, ~500M animated tris after LOD, BVH memory 80 GB → 1.7 GB
(47×), update 300 ms → 3.3 ms (90×), 60+ fps on RX 9070 XT.

## 4. The weight-savings law (the R&D deliverable)

Executable model: `tetcage_weight_model.jl` (deterministic, stdlib-only,
run with `julia --startup-file=no tetcage_weight_model.jl`; CSV sweep in
`results/weight_model_sweep.csv`).

Model `[INF]`, two constants solved from paper facts, never fitted:
```
m_std(N) = N·(12·V + k·T)                    standard animated VB + BLAS
m_tet(N) = k·T_clip + 12·V_clip + N·(12·V_tet + c·tets)
k = 33.06 B/tri (implied RDNA4 uncompacted BLAS+VB cost)
c = 106.97 B/(tet,copy) (DXR instance desc + TLAS node share)
```

**The law** `[INF]`: marginal (large-N) weight savings

    ρ_∞ ≈ (k/c) · (T/tets) = 0.309 · (triangles per tetrahedron)

Predicts all six Table-1 configurations within ±22% (worst: Tree
5×8×5 predicted 762.9× vs 970.1× full-model, −21.4%; Frog 25×25×25 +13.5%). Policy inversion: to hit a
savings target s, the cage must average ≥ s/0.309 triangles per tet —
81 tris/tet for ≥25×, 162 for ≥50×, 324 for ≥100×, 647 for ≥200×.

Update-time law `[INF]`: per-copy update ratio ≈ (t_std/t_tet)·(T/tets)
with t_std = 0.60 ns/tri (GPUOpen) and t_tet = 3.575 ns/tet (Table 3).
Cross-source band `[INF/F5]`: Table 3 implies 3.575 ns/tet; the GPUOpen
25k-plant demo implies 1.65 ns/tet — 2× spread across sources, both orders
of magnitude below per-triangle cost. Per-copy update ratios: 8× (grass,
high-res cage) to 414× (tree, medium cage).

Break-even `[INF]`: N* ≈ 1.4–2.7 uniquely animated copies per object for
memory parity; from copy 3 onward the representation is strictly lighter.

Calibration honesty `[INF/F1]`: the model reproduces the grass anchor
exactly (145.6 MB, 16.1×) by construction and predicts the law ±25% across
six hold-outs, but does NOT reproduce Table 2's absolute Tree/Frog rows
(−43% to −77% under either cage-resolution labeling). Absolute-MB
predictions therefore carry ~2× uncertainty; RATIO predictions are anchored
on 16.1× (paper) and 47× (GPUOpen). Do not quote absolute GB numbers from
this model without re-measurement.

WGE archetypes `[HYP]` (model extrapolation, pre-integration): alpine
conifer forest 1k unique wind animations — 68.8 GB standard vs 0.37 GB caged
(187×); aggressive-canopy 2.5k unique — 633×; grass meadow 5k — 23.4 GB vs
1.35 GB (17×); background crowd 200 unique walks — 48×. These numbers assume
the paper's objects/cages as proxies for WGE content and inherit the ratio
anchors, not absolute-MB trust.

## 5. Classification ledger (Codex's fact/inference/hypothesis demand)

- `[PAPER]` every §2–§3 item above, traceable to paper.txt line ranges or
  tables, plus the PDF sha256.
- `[INF]` k, c, t_std, t_tet, ρ law, N* table, update ratios, sensitivity
  band (k∈[24,60] → c∈[100,110]), archetype table — all recomputable by
  running the model script.
- `[HYP]` WGE archetype applicability; semantic-representation-policy
  integration (§7); "hybrid BLAS/cluster-AS/TetCage hierarchy" (roadmap
  hypothesis, untested here); foliage wind as the first WGE fit.

## 6. Verdict (unambiguous, per mandate)

**PROMOTE ONLY FOR**: dense, connectivity-preserving, uniquely-animated
deforming geometry at ≥ ~80 triangles/tet cage resolution — conifer canopies
and wind-animated foliage (best fit, ρ_∞ ≈ 269–970× marginal, update 114–414×
faster), grass/undergrowth (ρ_∞ ≈ 18–60×, update 8–31×), background crowds at
distance (ρ_∞ ≈ 20–77×). 
**DO NOT PROMOTE FOR**: hero/boss characters (sharp joint weights, sub-cage
motion), interactive/destructible objects, rigid props, keyframe-virtual-
vertex workflows, any use requiring watertight guarantees on current
hardware (watertight variant is 19–80× slower — reference oracle only).
Not "promising" — the above is the operating envelope with its measured
boundaries.

## 7. Semantic representation policy (novel WGE surface, `[HYP]`)

The paper's technique has no concept of gameplay meaning. WGE does. Sketch
of the meaning-aware selection policy (design only; nothing implemented):
gameplay-critical hero → conventional skinned mesh; background NPC beyond
interaction radius → cage; forest canopy/grass → cage aggressively;
interactive/destructible → conventional; cinematic focal object → forced
conventional. This composes with `SceneObject` importance/role fields that
already exist in the C2 contract shape — but that integration is EXACTLY the
part that must not be built until the entry gate (mega-sprint §17) opens.

## 8. Open questions for the next research slice (step 2/3 of the ladder)

1. Deterministic tetrahedralization on real WGE terrain content (Julia) —
   does the 6-tet voxel cage respect foliage vertex distribution?
2. CPU clipping reference (Julia) with expansion measured on WGE meshes —
   does the 1.36–2.31× band hold for conifer cards?
3. Bone-to-cage weight transfer quality on a rigged WGE character proxy —
   quantify the joint-artifact band as a function of cage resolution.
4. ε (watertight growth) sensitivity sweep on thin geometry (foliage cards
   are thinner than the paper's sphere test).
5. Lava/Vulkan capability probe for the instance-transform path (needs
   per-instance transform feedback — the DXR instance-transform trick maps
   to Vulkan `vkCmdBuildAccelerationStructures` instance matrices; confirm
   equivalence on RDNA3/4 under Lava).
