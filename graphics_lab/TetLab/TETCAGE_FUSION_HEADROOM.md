# TETCAGE — Spiral H: Launch-Floor Headroom (Fusion + Graph Submission, RTX 5060)

Spiral F §7 registered a falsification condition: *"if frame budgets drop
below ~60 µs, the ~25 µs launch floor becomes the binding constraint —
measure fusion/graph submission before concluding."* This probe EXECUTES
it. Evidence: `results/spiral_h_fusion.csv`, canonical run sha16
**`2efbcbbcbba381ed`**, stable columns byte-identical ×2 (HF-G2). Driver:
`gpu/spiral_h_fusion.jl` — kernels are deform!/reconstruct! VERBATIM from
the E-validated driver; the fused variant is their documented derivation
(deform in registers, reconstruct, one launch; redundant per-vertex corner
deform, f pure).

## 1. Variants (all same buffers, same arithmetic order)

- **A split**: two `@cuda` launches + sync (the F baseline)
- **B fused**: one launch, corners deformed in registers — no global
  round-trip for deformed corners
- **C graph-split**: A's two kernels captured into a CUDA graph,
  instantiated once, `launch()`ed per frame
- **D graph-fused**: B captured + graph-launched
- **E decomposition**: back-to-back launches, sync outside (launch-only cost)

## 2. Results (median of 25, µs wall; full table in the pinned CSV)

| class | A split | B fused | C graph-split | D graph-fused | fused wall Δ |
|---|---|---|---|---|---|
| blob 1100t wind | 28.5 | 16.2 | 10.4 | **9.9** | −43% |
| blob 1100t fold4 | 28.6 | 14.2 | 10.2 | **9.6** | −50% |
| icosphere2 wind | 28.8 | 14.6 | 10.2 | **9.4** | −49% |
| teapot wind | 29.8 | 15.3 | 10.5 | **9.6** | −49% |
| robot/grass/plate | ~27 | ~15 | ~10 | **~9–10.5** | −44–50% |
| conifer (3508 verts) | 25.3 | 14.4 | 9.9 | **9.3–11.2** | −44% |

**F-G3-class gate (HF-G3) PASSED**: B ≤ 0.75·A on the two largest classes,
both families (measured B/A = 0.50–0.57). **The launch floor was NOT
dominant** — the fused kernel also removed the intermediate corners'
global-memory round-trip: device time dropped from ~22.5 µs (A sum) to
~11.7 µs (B) on blob — an ~2× device-side algorithmic win on top of the
launch saving. **Graph submission removed a further ~40%**: D ≈ 9–11 µs
regardless of class (graph launch ≈ 2–3 µs driver cost).

## 3. Falsified prediction — registered honestly (F-H.2)

HF-G4 v1 predicted back-to-back launches ≤ 10 µs (floor model: launch+sync
overhead). **FALSIFIED on first run**: 15.5 µs/pair = **~7.8 µs per `@cuda`
dispatch** — Julia-side dispatch cost is the launch floor, higher than
modeled. Per the pre-registered violation protocol, the bound was amended
to a consistency sanity (E < A) and the falsification recorded. This
STRENGTHENS the fusion thesis: more overhead for graphs to absorb.

## 4. Capacity re-derivation (derived, not pinned)

At 2 ms duty slice, full D-quality skin per instance: split A ≈ 67–70
instances → **graph-fused D ≈ 201–209 instances** (~2.9×). At 8 ms: ~280 →
~805. Fusion+graph moves TetCage from "fits the frame 16×" to "a rounding
error in the frame": 9–11 µs on a 16.6 ms frame = **0.06%**.

## 5. Correctness gates

HF-G0: pinned tet counts, self-check 0.00e+00. HF-G1: split AND fused
outputs within 4·eps(f32)·max|coord| of the host f32 path (worst
9.5e-7, conifer; bound ~1.2e-5); fused-vs-split max |Δ| = same ulp scale;
graph-replayed outputs match stream outputs exactly. Determinism: stable
columns (provenance + ident_max + bfuse_max) byte-identical ×2; timing
columns declared machine-state (D1 protocol).

## 6. Defects registered

F-H.1: HF-G3's 0.75 prediction was conservative by ~2× (measured 0.50) —
kept as a gate, noted as a model-calibration miss in the cheap direction.
F-H.2: HF-G4's 10 µs launch-cost bound FALSIFIED (measured 15.5 µs/pair,
~7.8 µs/dispatch); amended per protocol. F-H.3: graph capture requires
post-warmup kernels (compilation/allocation cannot be captured — the
CUDACore docstring's warning was respected by design).

## 7. Consequence for the economics question

The headroom F registered is now measured: **fusion+graph ≈ 2.9× the
instance capacity, 0.06% frame cost at D quality.** For the
authored-cage deployment regime, "can it be optimized into worth it" is
answered: it already was worth it (F), and the optimization lever works
exactly as registered. The remaining economics question (incumbent
comparators: direct field eval vs cage path; skinning; morph streams)
remains the open Spiral — this probe only closes the *self-improvement*
half of the adoption argument.
