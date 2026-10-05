# TetCage Fidelity Frontier (Spiral D) — WGE Goal 2

Status: **D1 complete, D2 complete (REJECT), temporal probe complete (4/4 PASS), D3 surface measured.**
All numbers trace to pinned CSVs in `graphics_lab/TetLab/results/`. Hashes are sha256[1:16] of file bytes;
every CSV reproduced byte-identical in ≥2 fresh Julia processes.

| evidence | file | sha16 |
|---|---|---|
| D1 sweep (7 classes × 7 h × 5 fields, 196 rows) | `spiral_d_fidelity_sweep.csv` | `694ed4eb0b5a0ff7` |
| D2 adaptive upper-bound probe (10 cases × 4 families) | `spiral_d2_adaptive_probe.csv` | `b2e37d1ba2cc6784` |
| D temporal popping probe (2 cases × 2 precisions × 25 frames) | `spiral_d_temporal_probe.csv` | `0188b1c7d49131b2` |
| D3 confirmation probe (grass/plate below-ladder points) | `spiral_d3_confirm.csv` | `d93b7dcaab224a64` |

Drivers: `oracle/spiral_d_fidelity.jl`, `oracle/spiral_d2_adaptive.jl`,
`oracle/spiral_d_temporal.jl`, `oracle/spiral_d3_confirm.jl`.
Field semantics: mesh-normalized (bbox-max-side scale, F-D.2); wind/twist A=0.2, fold A=0.2/0.4;
affine control rotz-30° asserted EXACT (P4) on every build.
Screen metric: axis-aligned pinhole, f=500 px, px95 at d=5 (primary), 20, 100.

---

## 1. Error laws (P1–P7 verdicts)

- **P1 rms ∝ h² — CONFIRMED asymptotically, with one class-bound exception.**
  Twist is exact h² on curved classes: blob 0.063→0.128→0.249→0.511 px across successive
  halvings (ratios 2.03/1.95/2.00); conifer step ratios 1.94–2.09 across the whole ladder.
- **Wind plateaus above the field coherence length (~λ/2 ≈ 0.48 normalized).** On flat classes
  the h² law over-predicts improvement: measured effective exponent ≈ **h¹** (plate wind
  1.672@h=0.3 → 0.820@h=0.15: 2.04× per halving; grass 0.907@0.19 → 0.457@0.16).
  D3 round-2 registered bands hit: plate h=0.15 → 0.820 ∈ [0.7, 1.2] (h² said 0.55);
  grass h=0.12 → 0.544 ∈ [0.35, 0.6]. **Do not extrapolate wind error with h² on flat classes.**
- **P2 normal error ∝ h, P3 px ∝ 1/d — CONFIRMED.** px95@20 = px95@5/4 within rounding on
  every row (e.g. conifer wind 0.134/0.034/0.007 ≈ ÷4/÷20); nmean tracks h linearly.
- **P4 affine exactness — CONFIRMED** on every build in every driver (rotz-30° max ≤ 1e-9).
- **P5 tets ∝ h⁻³ — CONFIRMED** (blob: 49→114→261→537→1100→2172→4468 across the ladder).
- **Fold is heavy-tailed** (p95 ≈ 0, max ≫ rmse): error concentrates at the tanh transition —
  the D2 localization signal. fold4 (A=0.4) is the worst-case family on every class except
  grass/plate where **wind** binds.
- **P7 temporal continuity — CONFIRMED** (§4); no popping, including under an f32 storage model.

## 2. D1 — distance-conditioned quality (finest ladder rung per class)

| class | finest h | tets | tris/tet | wind px95@5/20/100 | twist | fold4 | binding family |
|---|---|---|---|---|---|---|---|
| blob | 0.14 | 4468 | ~11.6 | 0.057 / 0.014 / 0.003 | 0.063 | 0.065 | fold4 |
| conifer | 0.3 | 929 | 5.2 | 0.134 / 0.034 / 0.007 | 0.085 | **0.487** | fold4 |
| grass | 0.19 | 70 | 6.1 | **0.907** / 0.227 / 0.045 | 0.259 | 0.069 | **wind** |
| icosphere2 | 0.15 | 3780 | ~13 | 0.064 / 0.015 / 0.003 | 0.072 | 0.075 | fold4 |
| plate | 0.3 | 96 | 7.3 | **1.672** / 0.418 / 0.084 | 0.566 | 0.365 | **wind** |
| robot | 0.1 | 905 | ~7 | 0.105 / 0.027 / 0.005 | 0.048 | 0.014 | wind |
| teapot | 0.15 | 1908 | ~10 | 0.055 / 0.013 / 0.003 | 0.074 | 0.040 | fold4 |

Viewing at d=20 buys 4×; at d=100, 20× — a 0.5 px@5 budget is a 0.125 px@20 budget, etc.

## 3. D3 — decision surface: quality budget → cage (cheapest MEASURED point, worst family binding)

Budget = px95 at d=5 across ALL families (worst-case per class).

| class | ≤ 0.5 px | tets | ≤ 1.0 px | tets | verdict |
|---|---|---|---|---|---|
| blob | h=0.28 | 1100 | h=0.4 | 537 | cheap; fold4 binds |
| conifer | h=0.3 | 929 | h=0.3 (h=0.42 gives 1.026 — fails strict 1.0) | 929 | **fold4 binds hard** (2.06 px @ h=0.6) |
| grass | h=0.16 | **93** | h=0.19 | 70 | wind-bound; **non-monotone: h=0.16 beats h=0.12 (211 t) AND h=0.14 (143 t)** |
| icosphere2 | h=0.3 | 906 | h=0.42 | 468 | fold4/wind co-bind |
| plate | **none measured** | — | h=0.15 | 294 | **wind plateau floor ≈ 0.82 px@5** (h=0.15→0.820, 0.18→1.093, 0.23→1.298, 0.26→1.245) |
| robot | h=0.13 | 435 | h=0.19 | 184 | wind binds |
| teapot | h=0.3 | 500 | h=0.6 | 64 | cheapest quality-per-tet in corpus |

Operating rules derived:
1. **Choose h from the WIND curve on flat/thin classes, from the FOLD4 curve on curved ones.**
2. **Wind plateau ⇒ refinement below the coherence length wastes tets** (grass h=0.12 vs 0.16;
   plate h=0.23 vs 0.26). Coarsen toward the plateau knee, never past the ladder minimum
   without re-measuring — the surface is non-monotone there.
3. **Plate cannot reach 0.5 px@5 by uniform refinement in the measured regime** (floor ~0.8 px
   @ 294 tets). Options: relax budget for plates, or accept d≥20 viewing (0.82 px@5 = 0.21@20).
   NOT safe to extrapolate below h=0.15 (plateau ≠ asymptote-proof).
4. Conifer worst case is fold4, not wind: budgeting by wind alone (the paper-class family)
   under-provisions conifer by ~3.6× in px (0.134 vs 0.487 @ h=0.3).

## 4. Temporal — animation smoothness (P7) and f32 storage

Pre-registered gates (F-D.4 amendment, registered before the amended run):
motion ratio max_k D_k/max(d_k, 1e-3·scale) ≤ 1.10 (f64) / 1.25 (f32);
adjacent-frame px95 growth ≤ 1.25 / 1.50; curvature |Δ²px95| ≤ 0.25 / 0.35 of max px95.

| case | motion ratio | adj-frame px95 | curvature/max | px95 range | verdict |
|---|---|---|---|---|---|
| blob h=0.28 f64 | 1.014 | 1.130 | 0.060 | 0.16–0.237 | PASS |
| blob h=0.28 f32 | 1.014 | 1.130 | 0.060 | identical | PASS |
| conifer h=0.6 f64 | 0.993 | 1.209 | 0.197 | —0.821 | PASS |
| conifer h=0.6 f32 | 0.993 | 1.209 | 0.197 | identical | PASS |

- **The cage path animates like GT** (motion ratio ≈ 1.01; face-continuous reconstruction).
- **f32 vertex storage changes NOTHING temporally** — statistics identical to the last digit;
  f32 jitter is invisible next to the systematic cage error (consistent with C8: representation
  error ≈ chord sagitta, ε-invariant).
- **T3 LOD-switch pop budget (static, from D1):** switching rungs h_i→h_j pops by ~ the local
  px95 delta — worst adjacent movers: conifer fold4 0.487→1.026 px (h=0.3→0.42), blob fold4
  0.254→0.530 (0.28→0.4). **Hysteresis rule: switch only when the finer rung's improvement
  exceeds the coarser rung's absolute px95** (i.e. stay on coarse until px95(h_coarse) −
  px95(h_fine) > px95(h_coarse) is false — conservative: switch at 2× improvement).

## 5. D2 — adaptive refinement: **REJECTED** for WGE

Design: A = current (uncaged verts → GT path), B = orphans refit to nearest retained tet
(upper bound, same tet count), C = ALL vertices nearest-tet (geometric upper bound, ignores
the voxel rule). Pre-registered gate: KEEP only if the frontier moves.

- **orphans = 0 in all 40 rows** — the F-D.1 two-pass locate makes the cage path total at every
  probed operating point; the orphan→GT fallback never engages (variant A ≡ B exactly).
- **C/A ratio: median ≈ 1.00, best 0.974 (grass/wind), worst 1.122 (conifer/wind h=1.7)** —
  reassigning vertices to nearest (non-containing) tets does NOT move the quality frontier;
  extrapolating barycentrics through foreign tets usually loses accuracy.
- Verdict per gate: **REJECT adaptive refinement.** Registered caveat: C bounds reassignment
  policies decisively; a rebuild-based local-refinement scheme would have to beat uniform by
  >2.5% at equal tet count to reopen this question. D1's fold heavy-tail says localization has
  headroom in principle — but the measured bound shows barycentric transfer cannot harvest it.

## 6. Registered defects (this spiral)

- **F-D.1** — `locate_tet` sdist≤0 rule is a rounding-sign lottery for axis-aligned vertices
  sitting exactly ON lattice planes (plate x=0, z-trough): coarse-h locate misses. Fix: two-pass
  locate, residual misses rescued by min-max-sdist within tol = 1e-9·h³, `near_misses` on stderr.
  Perf note: O(1) Dict index map replaced O(tets) findfirst (1–8 rescues per coarse run observed).
- **F-D.2** — mesh_scale by radial extent degenerates on spherical meshes (icosphere2: scale ≈ 1e16,
  families collapse → meaningless EXACT-zero rows). Fix: bbox-max-side normalization.
- **F-D.3** — D2 driver referenced `mk_wind/mk_twist/mk_fold` that live only in the D1 driver
  (TetDeform exports only fixed-constant `f_*`) → UndefVarError. Fixed by verbatim copy into the
  driver; family semantics were duplicated across 5 files at peak. **CLOSED (turn E.1):**
  single-sourced into TetDeform exports; all five drivers re-pinned byte-identical ×2
  (sha16s unchanged); A5 gates re-run GREEN.
- **F-D.4** — temporal draft gate conflated smooth phase-translation of the error field with
  popping (px95 max/min over period ≤ 1.25; blob f64 correctly failed it: the wave crest slides).
  Amended to adjacent-frame growth + curvature tests; amendment registered before the amended run.

## 7. Ordering consequences

- Spiral E (GPU) is unblocked: D's fidelity contracts (§1 laws + §3 surface) are the acceptance
  criteria the GPU path must reproduce; F3 (C9 follow-up: no hardware ghost shading) checks there.
- ~~Before Spiral F (performance model): single-source the `mk_*` families (F-D.3)~~ DONE (turn E.1).
  Remaining before Spiral F: consider a measured h≈0.15 point for plate at d=20 as the honest
  plate contract (0.21 px@20).
