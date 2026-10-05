# TetCage RPD Spiral Ledger

Per goal §15: one entry per spiral turn — question, pinned facts, derived
model, prediction, experiment, result, decision. Evidence classes per goal §2.
Maintained by the executing agent; never rewritten, only appended.

---

## Turn A1 — Deterministic corpus + mesh I/O (Spiral A foundation)

**Question.** Can a deterministic, hash-verified corpus stand in for the
paper's commercial (unpinnable) test objects without smuggling in
recalled-but-wrong constants?

**Pinned facts.** Paper hash OK, 7 baseline artifacts hashed
(`/tmp/tetlab_baseline.sha256`); model reruns bit-identical; CSV
`7d1ad3cf…` unchanged. [PAPER] Freudenthal uniform 6-tet voxel split,
face-conforming (worked on the shared-face diagonal by hand, 2×1×1 grid).

**Derived/decided.** OBJ subset with 12-digit fixed-point canonical text;
mesh identity = sha256 of canonical text, not file bytes.

**Experiment.** Built 9-mesh corpus (analytic controls cube/icosphere2, thin
plate/grass, WGE conifer proxy, dense blob, 3 negative controls) via
`oracle/TetCorpus.jl`; manifest.csv `16314323…6fa5`, rebuild byte-identical.

**FALSIFIED (F-A1.1).** A "standard" recalled icosahedron face list: 19/20
faces were not faces of the icosahedron built from the canonical 12 vertices
(verified against a geometry-derived edge graph: 30 edges, 20 faces, Euler
check). Symptom chain: 34 base edges → 30 cached midpoints → 4 duplicate
slots → 5 NaN vertices at subdiv=1. **Lesson: recalled constants are not
pinned facts — derive from geometry, assert the topological invariants
(V−E+F=2, F=2T).** The determinism gate caught it exactly as designed.

**Also rejected (F-A1.2).** First conifer proxy (152 tris) as
unrepresentative: the Spiral-1 law (ρ_∞ ≈ 0.309·T/tets) predicts such a mesh
cannot benefit from caging. Rebuilt as card-grid tiers: 3508 v / 4808 tris,
hash `b4101037…a7932`.

**Decision.** KEEP. Corpus + I/O accepted as A1-complete. Next: A2/A3
(tri–tet overlap classification and clipping; expansion bands 1.36–2.31× /
1.42–3.71× [PAPER Table 1] are the falsification targets).

---

## Turn A2/A3 — Overlap classification + provenance clipping

**Question.** Can the oracle clip WGE-representative meshes into μMeshes with
(a) exact no-epsilon vertex identity across shared tet faces, (b) expansion
matching the paper's operating regime, (c) hash-deterministic output?

**Derivations (pre-code, all test-asserted).**
1. Freudenthal split = **6 permutation paths** along the main diagonal. A
   "mirror" t4–t6 set was falsified before running: it duplicates t3 and
   does not tile the voxel. Face-conformity of the 6-perm set verified by
   hand (shared face x=1 splits by the same diagonal 100→111 from both
   neighbors).
2. Convex×triangle overlap needs exactly 3 witness families: tri-vertex
   inside tet; tet-edge × tri-plane inside tri; tri-edge × tet-face inside
   the other three planes. Strict-straddle tests avoid double cuts on d==0.
3. Clipped-vertex identity WITHOUT epsilon: carry provenance (original
   triangle corner, mesh vertex id, original-edge fragment, intrinsic tet-
   plane set). Classes: V / EP(1 plane) / TE(2 planes) / TV(3 planes).
   **P-class (1 plane, no orig edge) proved unreachable** — chord vertices
   lie on their chord plane plus their creation plane. Code asserts this.
4. Canonical keys from ABSOLUTE lattice node ids → side-independent across
   shared faces (local face indices would have broken cross-tet dedup).
5. Vertex dedup is GLOBAL (paper §3: shared "between the current and all
   adjacent tetrahedra"), not per-tet.

**FALSIFIED DRAFTS (all caught, none shipped).**
- F-A2.1 plane orientation: swap condition `< 0` left normals INWARD;
  clipping emitted 0 triangles. Fixed to `> 0` with comment.
- F-A2.2 stale plane sets: vertices passing exactly through a clip plane
  (d == 0) never recorded the incidence → wrong A∩B merges → mislabeled
  TE as P-class. Fixed: d == 0 adds the plane, every pass. Anomalies → 0.
- F-A2.3 hand-set grids: conifer grid covered y ≤ 1.7 of a y ≤ 6.6 tree →
  sub-1× "expansion" (impossible for a covering cage). Fixed with
  `auto_grid` derived from the mesh AABB. Grids are never guessed now.
- Per-tet V-keys collapsed distinct mesh vertices (blob vert_exp 0.555);
  fixed by mesh-vertex-id in the key.
- Pre-run catches: TE-key dead code (would have thrown), missing W3 edge
  (3,1), 5-arg/6-arg ClipVert constructor mismatch.

**PREDICTION FALSIFIED → MODEL CORRECTED (the finding of this turn).**
My pre-registered prediction — expansion falls as grid refines because
"boundary fraction" grows — was INVERTED. Corrected model: for a fixed
mesh, refining h shrinks tris/tet (∝ T·h²) while each fixed-size triangle
straddles ∝ 1/h² tets ⇒ **expansion is monotone DECREASING in tris/tet**.
Measured (blob5): 18.6 tpt → 2.538×, 5.2 → 3.993×, 1.9 → 6.270×.
Validated against independent paper data [PAPER Table 1]: Grass 50 tpt →
2.31×, Tree 2469 tpt → 1.36× — same law, same direction. This explains
why the paper's objects show low expansion: they are 10³× denser than
their cages. The technique presupposes cage-sparse geometry.

**CONFIRMATION (paper band).** blob6 (81920 tris):
| h | tets | tris/tet | tri_exp | vert_exp |
|---|---|---|---|---|
| 0.28 | 1100 | 74.5 | **1.751×** | 1.145× |
| 0.40 | 537 | 152.6 | 1.521× | 1.072× |
74.5 tpt → 1.751× sits inside the paper band 1.36–2.31×.

**Acceptance.** Anomaly census 0 across all runs; P-class never reached;
blob5 build+clip hash-identical across reruns (`35b3de9f…`).

**Decision.** KEEP. A2/A3 green. Next: A4 rest-basis encoding (M, M⁻¹,
conditioning, degeneracy margins) and A5 deformation oracle vs ground truth.

---

## Turn A4 + OPT — Basis encoding + oracle performance campaign

**Question A4.** Are all cage tets safe for barycentric encoding and the
instance-transform path, and does the rejection policy have real support?

**Derivation.** All six Freudenthal path-tets of a regular voxel are
congruent ⇒ |det M| = h³ and identical κ for every grid tet; degeneracy is
structurally impossible in a regular cage. Policy thresholds exist for
future adaptive refinement (paper Fig. 12 direction). [PAPER] is silent on
numbers — policy [INF] with [MEASURED] support.

**Experiment (TetBasis.jl).** blob5 cage (1100 tets): |det| = 2.1952e-2 =
h³ exactly (range collapsed), vol_frac = 1.0, κ₂ = 4.049 uniform → ACCEPT.
Policy rejections verified: coplanar → singular; flattened (vf=1e-9) →
reject; healthy sliver (κ=1.33) → pass. Frobenius-bound cross-check
asserted inside basis_report.

**Question OPT.** Where does oracle time go and what model-gated
optimizations hold?

**Model.** clip phase = 88–89% of wall (baseline [MEASURED]); within it,
planes rebuilt per (tet × candidate) ≈ 10⁵× excess; bins computed twice;
heap allocs 327–960 MB/pass.

**Retained (O1–O3).** Planes hoisted per tet; bins stored on Cage;
allocation-free merge/static planes. Result: build 2.9×, clip ~2×, allocs
2.35–2.8× — all above the measured noise floor. [MEASURED]

**Rejected (O4).** Per-triangle AABB tables: single-run "regression" was
inside the rig's ±30% cross-process noise floor (407–541 ms on identical
code) — effect unresolvable; rejected on parsimony (unproven gain + memory
+ complexity). This produced the campaign's methodology finding: **single-
run timing cannot resolve <30% on this rig; N≥9 cross-process medians or
>2× effects required.**

**Process defects (F-OPT.1, admitted).** Optimized in-place, destroying the
byte-exact A2/A3 artifact (hash 35b3de9f… unreproducible — hash inputs
underrecorded at acceptance); one cited baseline number (tpt med 32) was
fabricated from memory. Corrections: fork-then-optimize rule; full field
records pinned with the new reference hash (98b989a0…); reconstruction kept
as TetCageBaseline.jl.

**Acceptance.** 9-mesh corpus audit: determinism OK on every mesh, anomaly
census 0, expansion law confirmed across three orders of magnitude of
density (robot 0.08 tpt → 44.7× … blob 18.6 tpt → 2.54×).

**Decision.** KEEP (A4 + O1–O3), REJECT (O4). Next: A5 deformation oracle
vs ground-truth skinning; C spirals (watertightness/ε) thereafter.

---

## Turn A5 — Deformation oracle + bench-noise fix (F-OPT.2 closure)

**Question 1 (bench).** Can any timing claim be made on this rig at all?

**Fix (bench2/bench_child).** N=9 cross-process medians with reported spread
and hash agreement. [MEASURED] main module: build 40.15 ms ±13%, clip 511.19
ms ±10%, alloc 347.7 MB ±0%. The ±30% single-run band is tamed to ±10–13%.
**O4 re-adjudicated under admissible statistics** (its ledger re-admission
condition): fork measured clip 513.1 ms (dead even), build 48.4 ms (20%
slower — table construction), allocs +19.6 MB, hashes identical. **REJECT
confirmed with evidence.**

**Question 2 (A5).** How faithful is cage-path deformation, and can the
deformation oracle's own correctness be mechanically gated?

**Gates (all [MEASURED], blob5).** G1 identity ≤ 1e-9 → measured 2.5e-16.
G2 affine exact (derived: piecewise-linear reproduces affine maps) → 3.5e-16.
G3 amplitude monotonicity: twist rmse 0.00025→0.000995 (A .1→.4). G4
resolution monotonicity: rmse 0.000497 (h .28) → 0.000126 (h .14). ALL GREEN.

**Fidelity law [MEASURED].** err ∝ A · C(family) · h² — halving h exactly
quarters error (4.0× measured across families); C ranks wind < twist < fold
(tanh k=3 ≈ 6× twist). Screen-space: worst case (fold A=.4, h=.28) p95 =
0.365 px @ 5 units, 0.022 px @ 20 — sub-pixel everywhere tested, consistent
with [PAPER §7] "no visible differences". Full table:
results/deform_sweep.csv (hash 2143581115c1a2c0, deterministic).

**Defects caught by the gates (the turn's lesson).**
- **F-A5.1**: inv3 returned the adjugate WITHOUT ÷det — a dead inverse that
  A4 never exposed because Minv was cached but never consumed. First real
  consumer (G1) caught it immediately.
- **F-A5.2**: corner-weight pairing shifted by one (λ₁·v0 … instead of
  λ₁·v1 …); G1 failed with 0.46 error while the λ solve itself was perfect —
  gates localize faults to the exact broken stage.
- Julia 1.12 world-age trap: include() inside functions is now a latent
  error (was silent warning in older Julia); hoisted to module scope.
- Process: closed-module export misses (sdist, inv3) surfaced as UndefVar —
  minor, fixed by exporting; note for the A6 harness to run modules, not
  probe scripts.

**Decision.** KEEP. A5 green; quality envelope recorded
(TETCAGE_QUALITY_ENVELOPE.md). Next: C-spiral watertightness/ε probes on
thin geometry (paper's sphere experiment first, then foliage cards), then
B-spiral content fit.

---

## Turn C — Watertightness mechanism (paper sphere experiment, CPU scope)

**Question.** Can the CPU oracle reproduce the paper's watertightness
mitigation result (sphere, ε-growth), and what does the escape mechanism
cost?

**Scope correction (honest).** Ray-escape rates need the GPU ray pipeline
(Spiral E). The CPU oracle measures the MECHANISM: shared-face vertices are
two independently-computed copies; disagreement = the seam a ray can fall
through. Harness: TetCageEps.jl + spiral_c_sphere.jl; sphere r=1 (paper
r=100 scaled), 20480 tris, h=0.22.

**Gate.** ε=0 fork must match base oracle exactly — PASSED (tets=1100,
tris=51982, verts count-identical, 0 anomalies) after fixing export drift.

**Falsified prediction (F-C-f32).** Predicted f32 storage exposes MORE gaps.
Measured ZERO at ε=0 (vs 24% of keys at f64, max 1.6e-15): the rounding
step (~6e-8) is 10⁷× coarser than the ulp noise and swallows it. Prediction
inverted — recorded.

**Mitigation cost model (F-C-eps).** ε-growth diverges the two copies BY
DESIGN (overlap = seam coverage). gap_max ≈ 11.4·ε·(unit h), linear until
saturation at ε≈1e-5; duplicate geometry +0.16% at paper's ε=2.5e-6,
saturating. This quantifies [PAPER §4.1]'s "just large enough" trade-off.

**Identity defects found BY the census (paid before results were trusted).**
- F-C.1: EP keys carried LOCAL edge indices → cross-triangle collisions.
  Fixed: mesh-edge identity. vert_exp(blob5) 1.559 → 2.537.
- F-C.2: all chord vertices on one canonical tet edge shared a single key.
  Fixed: exact position parameter along the edge. TE census 717 → 1194.
- Post-fix: new blob5 reference e730f896…, vert_exp=2.584; A5 gates GREEN;
  9-mesh corpus 0 anomalies, determinism OK.

**Decision.** KEEP (mechanism + cost model + identity fixes). Open: thin-
foliage ε sweep (F7), ray-level census (needs Spiral E GPU or a CPU ray-
caster), duplicate-intersection-per-ray count.

---

## Turn C-pre — F-C.3/F-C.4 identity integrity (found at Goal-2 start)

**Question.** The post-F-C.2 9-mesh re-audit was promised but never pinned;
re-running it mechanically, does the oracle reproduce its recorded state?

**Found.** Two defects:
- **F-C.3** (P1, fixed): the F-C.2 TE key carried the EXACT f64 edge
  parameter. Cross-tet copies of one chord point disagree at ulp scale —
  so the exact key gave them different keys: (a) blob5 vert counts became
  drift-fragile (explaining an unreproducible pin), (b) the gap census
  could NEVER compare TE copies — it undercounted the very mechanism it
  exists to measure. Evidence: 3664 TE instances on 724 conceptual chord
  points; 469 cross-tet pairs differ only at ~1e-15; exact key → 1225
  keys. Fix: quantized parameter (TE_T_QUANT = 1e-9, zero false merges,
  max in-cell spread 1.1e-15) → 724 keys = conceptual count.
- **F-C.4** (process): pinned blob5 hash16 `e730f8965c586853` /
  verts_out=26463 is unreproducible from on-disk code (26494/TE=1225,
  hash d99cf7b1…). F-OPT.1 class: pin underrecorded the kinds census that
  would have exposed it.

**Validation after fix.** New reproducible pin: kinds V=10242/EP=15027/
TE=724/TV=0, verts_out=25993, hash16 `3a2b58c3c9139095` (verified twice,
fresh processes). vert_exp(blob5) 2.587 → 2.538 = tri_exp (model-symmetric).
A5 gates GREEN (2.5e-16/3.5e-16). Corpus audit pinned as
`oracle/corpus_audit.jl`: 0 anomalies ×9 meshes, byte-identical across
processes, audit sha `5d03c0ac…`.

**Decision.** KEEP. Censuses were trusted only after this was paid —
C7/C8 below run on the corrected identity.

---

## Turn C7 — thin-foliage ε sweep: scale law corrected, ε policy closed

**Question.** Should clipping ε be global, asset-class-specific,
ε·h-normalized, cage-resolution-dependent, or unsafe to generalize?

**Pre-registered prediction.** gap_max ≈ C·ε·h with one constant C
(sphere 52); if C collapses across classes, ε·h normalization wins.

**FALSIFIED → law corrected.** Measured gap_max = **K(class)·ε·h³**:
sphere K=1068 (invariant to 0.1% across h=0.22/0.11; Turn C's 11.4·ε was
11.4/0.22³ = 1071 ✓ — a single-h artifact), plate K=6.0 (invariant to
0.1%), grass K=77, conifer K=242→340 (the only 1.4× multiscale drift).
[DERIVED] mechanism: a tet face samples O(h²) chord points, each displaced
O(ε·h) ⇒ aggregate band O(ε·h³); K = seam-bearing faces per unit area.
**ε·h normalization is wrong by h².**

**Second finding.** Duplicate-geometry cost SATURATES by ε = 1e-7 on all
classes (+0.16% sphere, +9.4% plate — thin cards duplicate more,
structurally; flat in ε beyond saturation). Over-coverage is nearly free
in clip output; its cost lives in ray-level duplicate hits (C8).

**Policy verdict [MEASURED].** Adopt per-cage **ε = E₀·h³, E₀ = 2.35e-4**
(paper ratio at the sphere anchor): reproduces the paper at h₀=0.22,
scales across resolutions; conifer worst-case band at h=0.05 = 1.0e-5 =
six orders above the f64 copy-drift floor, ~170× the f32 unit step;
margin ∝ h² — re-check with one line of arithmetic below h=0.05. f32 at
ε=0 shows ZERO observable gaps on every mesh (F-C-f32 generalized).
UNSAFE to generalize K to unmeasured classes (hair sheets, parallax); the
h³ form with a per-class K check is the portable statement.

**Artifacts.** `oracle/spiral_c7_thin_eps.jl`; CSV
`results/spiral_c7_thin_eps_sweep.csv` sha16 `02b9b42eea5af8bb`;
policy in TETCAGE_WATERTIGHTNESS.md (C7 addendum). F7 bullet 1 CLOSED.

**Decision.** KEEP. No asset-class ε constants ship; one h³ law + K table.

---

## Turn C8/C9 — CPU ray census: paper result reproduces; C9 rejected

**Question.** What are the ray-level escape rates, position dependence,
and duplicate-hit costs through the actual clipped μMeshes (instance-
transform hit model, CPU soup emulation)?

**Harness.** Deterministic: voxel-DDA prefilter + exact Möller–Trumbore
(inclusive edges) over per-tet soups; populations GENERIC (Fibonacci/
paper-style) / VERTEX-aimed / GAP-aimed / ALL; no RNG.
`oracle/spiral_c8_ray_census.jl` v3; CSV `results/spiral_c8_ray_census.csv`
sha16 `94c7782c0917e340`; doc TETCAGE_CPU_RAY_CENSUS.md.

**Harness defects caught before publication (Critic, in-lane).** v1
conflated populations (adversarial rays dominated the headline); counted
chord-sagitta representation error (dev_p95 2.5e-4 ≈ edge²/8r) as "false
hits"; plate surface function had wrong ripple phase. A raw-hit probe
(probe_plate_mult.jl) attributed the plate multi-hit signal to per-tet
PROVENANCE FRAGMENTS (6 hits, one per adjacent tet, agreeing to ~1e-17)
— after catching the probe's own soup-overwrite bug (1 tri/tet).

**Results [MEASURED].**
- GENERIC escapes: sphere **0/20000 at every ε ∈ {0, 2.5e-6, 1e-4} and
  both precisions** — the paper's mitigation headline reproduces; the
  mechanism lives in adversarial populations: vertex-aimed 13.45% (ε=0,
  f64) → 5.70% (paper ε) → 1.28% (f32+ε); gap-aimed 15.9% → 2.1% → 0.25%.
- Escapes are POSITION-dependent (fire only inside the seam band) — the
  paper's fix-at-source design is validated.
- Duplicates: structural (exist at ε=0; per-tet fragments), bounded
  (mean 5.05/cluster worst population; max 16 under ε), shading-safe
  (copies agree at float noise, cluster to one hit).

**C9 verdict: unique edge ownership REJECTED.** Cross-tet hit dedup breaks
per-tet μMesh autonomy (the architecture's core); the disease is bounded
and harmless to shading. The paper's ε-growth lever, costed in C7, is the
sufficient mitigation. F3's "must be quantified before any prototype" —
now quantified; F3 can close when Spiral E confirms no hardware-level
ghost shading.

**Decision.** KEEP. **SPIRAL C CLOSE GATE: PASSED** (mechanism quantified
at ray level; ε policy bounded; thin foliage characterized; duplicate
behavior characterized; oracle deterministic — audit sha 5d03c0ac…).
Next: Spiral D (fidelity frontier) per Goal 2 §3, then Spiral E gate.

---

## Turn D — Fidelity frontier (D1 + D2 + temporal + D3) [CLOSE GATE: PASSED]

**Question (Goal 2 §3).** Per asset class: what is the coarsest cage whose
worst-family screen error stays inside an explicit budget? Can adaptive
refinement beat uniform on the quality/memory Pareto frontier? Does the
cage path animate without popping, and what does f32 storage cost
temporally?

**Pre-registration.** P1 rms∝h², P2 normals∝h, P3 px∝1/d, P4 rotz exact
at every h, P5 tets∝h⁻³, P7 temporal continuity. D2 KEEP-gate = frontier
must move. Temporal gates (motion ratio / adjacent-frame px95 growth /
curvature) registered before the amended run (F-D.4). D3 round-2 bands
registered before running: plate h=0.15 wind ∈ [0.7, 1.2] px, grass
h=0.12 ∈ [0.35, 0.6] px.

**Harness.** `oracle/spiral_d_fidelity.jl` (D1: 7 classes × 7-step
h-ladder × 5 fields, bbox-normalized, P4 asserted per run),
`oracle/spiral_d2_adaptive.jl` (A current / B orphan-refit / C
all-nearest-tet), `oracle/spiral_d_temporal.jl` (24-frame wind phase
sweep, f64 + f32-storage model), `oracle/spiral_d3_confirm.jl`
(below-ladder points for wind-bound flat classes). Pins (all reproduced
byte-identical in ≥2 fresh processes): D1 `694ed4eb0b5a0ff7`, D2
`b2e37d1ba2cc6784`, temporal `0188b1c7d49131b2`, D3 `d93b7dcaab224a64`.
Doc: TETCAGE_FIDELITY_FRONTIER.md.

**Defects caught (Critic, in-lane).** F-D.1 on-lattice locate lottery;
F-D.2 spherical scale degeneracy (icosphere2 fake-exact zeros); F-D.3
driver-only mk_* families (UndefVarError); F-D.4 gate conflation. All
registered in TETCAGERT_FAILURE_REGISTER.md; none survive into results.

**Results [MEASURED].**
- Laws: twist exact h² on curved classes (blob ratios 2.03/1.95/2.00;
  conifer 1.94–2.09 across the ladder). WIND PLATEAUS above the field
  coherence length (~λ/2 ≈ 0.48 normalized) — flat classes show
  effective h¹ (plate wind 1.672@h=0.3 → 0.820@h=0.15 = 2.04× per
  halving; grass non-monotone: h=0.16/93t gives 0.457 vs h=0.12/211t
  0.544). D3 registered bands HIT: plate 0.820 ∈ [0.7,1.2], grass
  0.544 ∈ [0.35,0.6] — h² point estimates missed 2×. px∝1/d exact
  (÷4 at d=20, ÷20 at d=100 on every row). P4 exact on every build.
  tets∝h⁻³ confirmed.
- D3 surface (worst family, px95@5): blob 0.5px @ h=0.28/1100t;
  conifer 0.5px @ h=0.3/929t (fold4 binds — wind-only budgeting
  under-provisions conifer ~3.6×); teapot 0.5px @ h=0.3/500t (cheapest
  quality-per-tet); icosphere2 0.5px @ h=0.3/906t; robot 0.5px @
  h=0.13/435t; grass 0.5px @ h=0.16/93t; PLATE CANNOT REACH 0.5px by
  uniform refinement in the measured regime — wind floor ≈0.82 px @
  294 tets; honest plate contract is d≥20 viewing (0.21 px@20) or a
  relaxed budget. Plateau rule: refinement below the coherence length
  wastes tets; never extrapolate wind with h² on flat classes.
- D2: **REJECTED** per pre-registered gate. orphans=0 in all 40 rows
  (F-D.1 made the path total; variant B ≡ A exactly); all-nearest-tet
  reassignment median ratio 1.00, best 0.974 (grass/wind), worst 1.122
  (conifer/wind h=1.7) — the frontier does not move; barycentric
  transfer through non-containing tets usually loses. Reopen condition:
  a rebuild-based scheme beating uniform by >2.5% at equal tet count.
- Temporal: 4/4 PASS. Motion ratio ≈ 1.01 (cage animates like GT);
  f32-storage model statistically IDENTICAL to f64 (jitter invisible
  next to the systematic cage error; consistent with C8 chord-sagitta
  representation error); LOD-switch pop budget = adjacent-h px95 delta
  (worst movers: conifer fold4 0.487→1.026, blob fold4 0.254→0.530);
  hysteresis rule: switch rungs only at >2× improvement.
- Critic verification at close: corpus audit re-run — 9 meshes,
  0 anomalies, sha `5d03c0ac…` byte-identical; A5 gates GREEN
  (G1 2.5e-16, G2 3.5e-16, G3/G4 monotone).

**Decision.** KEEP. **SPIRAL D CLOSE GATE: PASSED** (error laws measured
including the wind-plateau exception; decision surface measured, not
extrapolated; adaptive refinement rejected by its own pre-registered
gate; temporal continuity verified including f32). Next: Spiral E (GPU)
per Goal 2 §4 — this turn's fidelity contracts are E's acceptance
criteria; F3 (hardware ghost shading) closes there. Debt before Spiral
F: single-source the mk_* families (F-D.3).

## Turn E — GPU runtime-arithmetic confirmation (RTX 5060 sm_120) [CLOSE GATE: PASSED]

- E-G1 toolchain PROVED: RTX 5060 (GB206, cap 12.0), CUDA UMD 13.4,
  CUDA.jl 6.4.1/runtime-13.4 artifacts in quarantine-local `gpu/` env
  (repo graphics env is Lava/Vulkan — no CUDA — by design; compute
  harness stays inside TetLab quarantine). f32 identity kernel exact.
- E-G2 D-contract PASS (6 case rows): |px95_GPU − px95_CPUf32| = 0.0000
  px (bit-identical device vs host f32); |GPU − f64 cage| ≤ 0.0001 px,
  deep inside the pre-registered D band (≤0.05·px95+0.05).
- E-G3 PASS: 68,470 intra-group TE corner pairs across 3 cages × 2
  families — bit-identical fraction 1.0000, max screen deviation
  0.000e+00 px (gate 0.01 px). ε-seam policy (C7) holds on hardware.
- E-G4 PASS: max object deviation 0, derived normal tilt 0.000e+00°
  (gate 0.01°). **F3 (hardware ghost shading) CLOSES** per its
  pre-registered close condition (E-G3 ∧ E-G4 on all cages).
- E-G5 PASS: two fresh processes byte-identical CSV, sha16
  `c4b3ec1c9a0adfe3`; self-check 0.00e+00 on all cases (driver λ/idx
  machinery bit-exact vs TetDeform — delegation to TetBasis).
- D1 cross-validation: E's f64-cage column reproduces D1's pinned
  px95@5 values on all 6 rows (0.218/0.254/0.541/2.064/1.672/0.365) —
  independent second-code-path reproduction of D authority rows.
- Defects found + registered F-E.1..F-E.7; incl. F-E.7: E-G2's D-band
  was first scored against the IDENTITY cage path (vacuous) — CSV
  `805d2abf45593c51` declared INVALID, superseded by `c4b3ec1c9a0adfe3`.
  F-D.3 recurrence: mk_wind_like defined in-file; single-sourcing debt
  into TetDeform stands before Spiral F.
- Critic verification at close: corpus audit re-run — 9 meshes,
  0 anomalies, sha `5d03c0ac…` byte-identical; quarantine intact
  (only `graphics_lab/TetLab/` untracked); warning/world-age grep = 0.

**Decision.** KEEP. **SPIRAL E CLOSE GATE: PASSED** (device arithmetic
core confirmed bit-exact vs host f32; F3 closed on hardware; Lava/Vulkan
render pass explicitly DEFERRED with falsification condition: ≥0.5 px
disagreement vs this harness's screen-space model on any D class reopens
the GPU D-contract at render-pass level — see TETCAGE_GPU_SPIRAL_E.md §7).
Next: Spiral F (perf model) after paying the F-D.3 mk_* single-sourcing
debt.

## Turn E.1 — F-D.3 debt paid: mk_* families single-sourced into TetDeform

- TetDeform now exports mk_twist / mk_wind_phase / mk_wind / mk_fold /
  mk_wind_like (D1 semantics; mk_wind ≡ mk_wind_phase(φ=0.7)). Argument
  types flow through unchanged — E's Float32 A/φ must not promote
  (byte-identical re-pin requirement).
- Verbatim copies deleted from spiral_d_fidelity, spiral_d2_adaptive,
  spiral_d3_confirm, spiral_d_temporal, gpu/spiral_e_gpu.
- Re-pin gate (AMENDED — see F-E.9): all five affected drivers re-run
  ×2 fresh processes. D2/D3/temporal/E: CSV byte-identical to pinned
  sha16 (D2 `b2e37d1b…`, D3 `d93b7dca…`, temporal `0188b1c7…`,
  E `c4b3ec1c…`). D1: measurement columns 1–14 byte-identical pin ==
  run a == run b (its wall-clock build_ms/locate_ms columns can never
  reproduce; full-file sha is not a valid D1 pin criterion) — the
  single-sourcing changed NO measured number anywhere. A5 regression
  gates re-run GREEN (G1 2.517e-16, G2 3.511e-16); run_sweep artifact
  byte-identical; warnings = 0.
- F-E.9 (caught by Critic, fixed): D1's output path was CWD-relative
  and the first D1 re-pin FALSE-PASSED (hashed the stale pinned file
  while fresh output escaped to graphics_lab/results/ — outside the
  quarantine). Path ROOT-anchored (also in C7/C8/C-sphere/run_sweep,
  same class), escaped dir removed, pinned artifact restored intact
  (`694ed4eb…`), protocol amended and pre-registered in the register.

**Decision.** KEEP. F-D.3 CLOSED; the family-semantics duplication class
is retired (new drivers must consume the TetDeform exports — registering
a new local mk_* is a defect). **Spiral F (perf model) unblocked.**

## Turn F — GPU frame pricing (RTX 5060 sm_120) [CLOSE GATE: PASSED]

- Times EXACTLY the E-validated kernels (verbatim from spiral_e_gpu.jl)
  at the 7 D operating points (pinned D1/D3 rows: blob 1100t, teapot 500t,
  icosphere2 906t, robot 435t, conifer 226t, plate 96t, grass 93t), wind
  + fold4, median of 25 reps after 3 warmups. Model frozen pre-run:
  resident device state (H1), offline weights (H2), one launch per stage
  (H3 — fusion is headroom, never assumed).
- F-G0 PASS: all 7 classes reproduce pinned tet counts; self-check
  0.00e+00 everywhere.
- F-G1 PASS (amended pre-acceptance, F-F.1): per-vertex |Δ| ≤ 4·eps·coord
  (worst 9.5e-7, bound ~1.2e-5); E-G2 screen contract 0.0000 px on all
  14 rows.
- F-G2 PASS: deform+reconstruct wall ≤ 500 µs everywhere; measured worst
  31.92 µs (blob/fold4/1100t) — ~16× inside the gate. Wall cost at D
  operating points is roughly FLAT in cage size (~25 µs launch+sync
  floor dominates); throughput only shows in device time (~19–21 µs at
  906–1100 tets).
- Capacity (derived, illustrative): ~62–77 instances per 2 ms slice at
  full D-quality skin per instance, heaviest class included.
- Determinism: pre-registered stable-column projection byte-identical
  across two fresh processes (full-file sha differs only in timing
  columns, as the protocol predicts; canonical run sha16
  `3dc74330cc03b598`). Verification tooling itself defect-caught
  (F-F.2: drained-fifo double read → bogus diff/false pass; redone
  file-vs-file with the exact registered projection).
- Critic verification at close: corpus audit 9 meshes, 0 anomalies,
  sha `5d03c0ac…` exact; all prior pinned CSVs byte-intact; quarantine
  intact.

**Decision.** KEEP. **SPIRAL F CLOSE GATE: PASSED.** The D-quality cage
path FITS the frame on RTX 5060 with ~16× margin at the pre-registered
500 µs gate — cost is no longer an open risk at D operating points on
this hardware class. Goal 2 §5 closed: fidelity (D), hardware arithmetic
(E), cost (F) all measured. Open items: Lava/Vulkan render-pass parity
(E §7 deferral, falsification condition registered) and the plate-class
product decision (D3 floor — contract choice, not research).

## Turn G — Vulkan render-pass readback parity (E §7 deferral CLOSED) [CLOSE GATE: PASSED]

- Built the harness E's falsification condition named: offscreen
  render-to-texture → GPU readback, real Vulkan transform chain (f32
  shader projection, fixed-function w-divide, viewport, rasterizer) vs
  the analytic pinhole model of screen_px_error. Vulkan.jl at the SAME
  repo-pinned rev; NVIDIA ICD (api 1.4.351, conformance 1.4.3); glslc
  offline SPIR-V; no swapchain/window.
- G-G2 PASS with ~6,000× margin: campaign max divergence **8.4e-5 px**
  (typical 4–5e-5; f32 ulp at 512 px is 6.1e-5 — parity holds at ~1 ulp
  of the framebuffer coordinate scale). 18 rows (9 classes × wind/fold4;
  cube+skeleton added as strengthening classes; conifer rows carry
  fb-clip annotations: its wind GT field is analytically off-camera —
  geometry, not chain).
- Method: chunked value parity — ROUNDS of pairwise-distinct predicted
  cells, payload accepted only on 0.01 px value-match, else solo
  re-render (attribution exact by construction). Deterministic: full CSV
  byte-identical ×2, sha16 `6b5164abcf847bd1`, error-vector sha
  `7c7102c2615a6ddb`.
- Defects F-G.1–F-G.9 registered; heaviest: convention-depot mix-up (two
  Vulkan.jl depot copies differ constructor-by-constructor — verify
  against the ACTUAL pinned rev), sentinel-caught projection double-
  divide, vertex-stride 24-vs-12 device fault, and the Vulkan.jl manual-
  destroy + finalizer double-free (single-ownership rule).
- Critic at close: corpus audit `5d03c0ac…` exact; all prior CSV pins
  byte-intact; quarantine intact.

**Decision.** KEEP. **SPIRAL G CLOSE GATE: PASSED.** E §7 deferral
RESOLVED: the GPU D-contract holds end-to-end — device arithmetic (E) AND
render pass (G) agree with the CPU oracle far below any perceptual
threshold. Goal-2 open stack: plate-class product decision only.

## Turn H — launch-floor headroom: fusion + graph submission [CLOSE GATE: PASSED]

- Executed F §7's falsification condition. Kernels verbatim from the
  E-validated driver; fused variant = documented derivation (registers,
  one launch, no corner global round-trip); graph capture post-warmup
  (CUDACore capture/instantiate/launch — API verified from the depot).
- Headline (blob 1100t): split 28.5 → fused 16.2 → graph-fused **9.9 µs**
  (2.9×); fused device time ~11.7 vs split's ~22.5 µs sum — the fused
  kernel is an ~2× DEVICE-side win, not just launch savings. All classes:
  graph-fused 9–11 µs = 0.06% of a 16.6 ms frame at D quality.
- HF-G3 PASSED (B ≤ 0.75·A on largest classes; measured 0.50–0.57).
- **HF-G4's 10 µs prediction FALSIFIED** (measured 15.5 µs/pair =
  ~7.8 µs/@cuda dispatch) — amended per its own pre-registered protocol;
  strengthens the fusion thesis (F-H.2).
- Capacity re-derivation: 67–70 → **201–209 instances per 2 ms slice**
  (heaviest class), ~805 at 8 ms.
- HF-G2 determinism: stable columns byte-identical ×2; canonical CSV
  sha16 `2efbcbbcbba381ed` restored on disk.
- Critic at close: corpus audit `5d03c0ac…` exact; all prior pins
  byte-intact; quarantine intact; warnings 0.

**Decision.** KEEP. **SPIRAL H CLOSE GATE: PASSED.** The self-improvement
half of the economics question is closed: fusion+graph ≈ 2.9× capacity,
frame cost 0.06%. Open: incumbent-comparator spiral (direct field eval,
skinning, morph streams; authored-cage scenario) → then the plate-class
product decision.
