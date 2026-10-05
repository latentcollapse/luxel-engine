# TetCage Watertightness — Spiral C findings

Scene: sphere r=1.0 (paper: r=100, scaled), icosphere5 = 20480 tris,
cage h=0.22 (paper: 9×12×9 → h≈0.22 scaled), 1770 retained tets, clipping
from origin-outward geometry. Sweep CSV: `results/spiral-c-eps-sweep.csv`
(sha256 `0ff08e7f…`, deterministic).

## Scope correction (recorded honestly)

The paper's experiment counts RAYS escaping a closed sphere (0.01% → 0 with
ε-growth). Escape rates require the ray pipeline (GPU instance-transform
traversal, Spiral E). The CPU oracle measures the underlying MECHANISM
instead: shared-face vertices are computed independently by each adjacent
tet (different plane provenance) and the stored copies can disagree — a ray
through the disagreement band can miss both μMeshes. All findings below are
about this mechanism, not ray escapes. [PAPER §4.1/§5] remains the ray-level
authority; GPU reproduction is deferred to Spiral E with this harness as
blueprint.

## Findings

**F-C-mech (mechanism quantified).** At ε=0, f64 storage: 24% of shared
keys hold two bitwise-different copies; max disagreement 1.6e-15 — machine-
ulp scale, exactly as derived. The seam ambiguity is REAL and now measured,
not assumed.

**F-C-f32 (prediction falsified).** Predicted f32 storage would show MORE
gaps. Measured: ZERO observable gaps at ε=0 (31050 shared keys). f32's
rounding step (~6e-8 at unit scale) is ~10⁷× coarser than the ulp noise, so
both copies round identically on clean geometry. Storage quantization
swallows this mechanism rather than exposing it. (Leaks in production
pipelines arise where transform math amplifies the band past storage
resolution — a GPU-side phenomenon.)

**F-C-eps (mitigation cost model).** ε-growth moves each tet's planes
outward along ITS OWN normals [PAPER §4.1], so the two copies of a shared
face deliberately diverge — that IS the overlap that covers the seam.
Measured: disagreement grows linearly with ε (gap_max ≈ 11.4·ε at h=0.22,
constant ratio across the sweep), plateauing at ε ≥ 1e-5 where every
shared-face chord vertex is duplicated. Duplicate-geometry cost:
tris_out 60576 → 60672 = **+0.16%** at the paper's ε=2.5e-6, saturating
there. This is the quantitative form of the paper's "just large enough…
without too many duplicated intersections" trade-off [PAPER §4.1/§8].

**ε policy implication [INF].** Overlap band width ∝ ε·h and duplicate
geometry saturates at ε ≈ 1e-5 (h=0.22 scale). The paper's ε=2.5e-6 sits
well inside saturation for this cage scale; asset-class-specific ε policy
(Spiral C goal) reduces to ε/h ≥ ~5e-5 for full coverage on this geometry —
pending thin-foliage verification (paper's probe used a sphere; cards are
the open risk, F7).

> **CORRECTED by C7 (see addendum below):** the h-linear band model was a
> single-h measurement artifact. The measured law is gap_max = K(class)·ε·h³.
> The policy conclusion is superseded by the addendum.

## Identity correctness debt paid (found BY this spiral)

The gap census was impossible until two identity defects were fixed — both
invisible to every prior gate because prior gates never compared the same
key across tets:
- **F-C.1**: EP keys carried LOCAL edge indices (collide across triangles);
  fixed to mesh-edge identity (sorted mesh-vertex ids). vert_exp(blob5)
  1.559 → 2.537.
- **F-C.2**: TE chord vertices on one canonical tet edge all shared one key;
  fixed with the exact position parameter along the edge (side-independent:
  canonical node world positions are identical from both tets). TE census
  717 → 1194 on blob5.
New pinned blob5 reference: tets=1100, tris_out=51982, tri_exp=2.538,
verts_out=26463, vert_exp=2.584, anomalies=0, hash16=e730f8965c586853.
A5 gates re-verified GREEN post-fix (deformation path unaffected);
9-mesh corpus re-audit: 0 anomalies, determinism OK.

## Remaining for full C-spiral closure

1. ~~Thin-foliage ε sweep (F7)~~ — CLOSED, see C7 addendum below.
2. Ray-level escape rates — CLOSED at CPU soup level, see C8 addendum and
   tetcage-cpu-ray-census.md. GPU traversal confirmation remains Spiral E's
   (hardware behavior was always out of CPU scope).
3. ~~Duplicate-intersection census per ray~~ — CLOSED, see C8 addendum
   (bounded, structural, C9 rejected).

---

# C7/C8/C9 Addendum — Spiral C closure (this turn)

## C7 — thin-foliage ε sweep: the scale law corrected

Sweep: sphere/plate/grass/conifer × h ∈ {coarse, fine} × ε ∈ {0, 1e-7,
2.5e-6, 1e-5, 1e-4} at f64, f32 spot-checks. CSV:
`results/spiral-c7-thin-eps-sweep.csv`, sha256 `02b9b42eea5af8bb` (16-hex).

**F-C-eps-h (prior model falsified).** Turn C's gap_max ≈ 11.4·ε was a
single-h artifact. Measured across two cage scales per mesh:
**gap_max = K(class)·ε·h³** — sphere K = 1068 (invariant to 0.1% across
h = 0.22 and 0.11: 11.4/0.22³ = 1071 ✓), plate K = 6.0 (invariant to 0.1%),
grass K = 77, conifer K = 242 (h=0.2) / 340 (h=0.4) — the only multiscale
drift (1.4×), and the ONLY mesh class where h³ normalization is not exact.
[DERIVED] mechanism: cross-tet copy disagreement arises where triangle
chords cut the grown face plane; a face samples O(h²) chord points, each
displaced O(ε·h) ⇒ aggregate band O(ε·h³); K counts seam-bearing faces per
unit area, a geometry-class constant.

**Duplicate-geometry cost saturates by ε = 1e-7** on all four meshes
(tris_out +0.16% sphere, +9.4% plate at h=0.4 — thin geometry duplicates
MORE, structurally: every card straddles a face — and it is ε-flat beyond
saturation). Over-coverage is nearly free in CLIP OUTPUT.

**ε POLICY VERDICT [MEASURED → POLICY].**
1. The paper's GLOBAL constant is UNSAFE as an absolute number: at h=0.11
   the paper's ε gives 2.4× MORE band than at h=0.22 for the same coverage
   need (wasted, though dup cost is ε-flat so the waste is invisible in
   clip output); at coarser cages it UNDER-covers relative to the anchor.
2. ε·h normalization is WRONG by h² (band would shrink 4× per halving of
   h; measured band grows 8×).
3. **Adopted policy: per-cage ε = E₀·h³ with the single global coefficient
   E₀ = 2.5e-6/h₀³ = 2.35e-4 (paper ratio at the sphere anchor h₀ = 0.22).**
   Reproduces the paper exactly at the anchor (ε(0.22) = 2.5e-6,
   band 2.84e-5), scales correctly across cage resolutions, and holds for
   every measured class because the band is graded against the copy-
   disagreement floor, not against K: worst-case K (conifer 340) at a
   dense-foliage cage h = 0.05 gives band = 340·E₀·h³ = 1.0e-5 — six
   orders of magnitude above the f64 copy-drift floor (~1.6e-15 at unit
   scale, Turn C) and ~170× above the f32 unit-scale storage step (~6e-8).
   Margin shrinks linearly with h², so a future sub-0.05 cage must re-check
   this inequality; that check is one line of arithmetic, not a study.
   Duplicate-geometry cost at policy ε: ε-flat saturation (+0.16% sphere,
   +9.4% plate h=0.4 — thin cards duplicate MORE, structurally, and it
   does not grow with ε), conifer flat within ±0.01%.
4. f32: at ε = 0 storage quantization swallows the mechanism entirely
   (0 observable gaps on ALL meshes, confirming F-C-f32 beyond the sphere).
   The ε policy is therefore f64-identity insurance; on f32 storage paths
   its role shifts to covering transform-amplified bands (GPU, Spiral E).
5. UNSAFE TO GENERALIZE beyond these classes: K varies 178× across
   geometry, so any future asset class (dense parallax meshes, hair
   sheets) must re-measure K before trusting the policy. The POLICY is the
   h³ form with a measured-E₀ check per new class; sphere/plate/grass/
   conifer anchors bound the current corpus.

## C8 — CPU ray census: escapes, duplicates, position dependence

Full document: `tetcage-cpu-ray-census.md`. CSV:
`results/spiral-c8-ray-census.csv`, sha256 `94c7782c0917e340` (16-hex).
Headlines: GENERIC (paper-comparable) escapes = 0/20000 at every ε and
precision on the sphere — the paper's 0.01%→0 mitigation result reproduces
with the seam mechanism located in ADVERSARIAL populations (vertex-aimed
13.4% → 5.7% under ε; gap-aimed 15.9% → 2.1%). Duplicate intersections:
per-tet provenance fragments of single surface points (raw-hit probe),
mean 5.05 hits/cluster on plate vertex rays, max 16 under ε-growth —
bounded, agreement at float noise, no wrong-shading channel.

## C9 — unique edge ownership: REJECTED

Cure (cross-tet hit dedup) breaks per-tet μMesh autonomy — the core of the
instance-transform architecture; the disease is bounded and shading-safe.
The paper's ε-growth lever, now costed (C7), is the correct and sufficient
mitigation at CPU level. Revisit only if Spiral E shows hardware-level
ghost shading.

## Identity integrity after closure

F-C.3 found and fixed during C8 instrumentation (TE exact-t key fragmented
ulp-drifted copies; see failure register). New reproducible blob5 pin:
kinds V=10242/EP=15027/TE=724/TV=0, hash16 `3a2b58c3c9139095`; corpus audit
pinned as `oracle/corpus_audit.jl` (0 anomalies ×9 meshes, byte-identical
across processes, audit sha `5d03c0ac…`). Old pin `e730f896…` registered
F-C.4 (unreproducible, F-OPT.1 class). A5 gates GREEN post-fix.
