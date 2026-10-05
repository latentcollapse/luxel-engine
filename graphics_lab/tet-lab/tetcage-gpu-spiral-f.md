# TETCAGE — Spiral F: GPU Frame Pricing (RTX 5060, CUDA.jl)

Goal 2 §5. Times EXACTLY the E-validated kernels (`deform_corners!`,
`reconstruct!` verbatim from `gpu/spiral_e_gpu.jl`) at the D fidelity
operating points. Evidence: `results/spiral-f-frame-budget.csv` — full-file
sha16 of the canonical run `3dc74330cc03b598` (timing columns are
machine-state; the pre-registered pin is the stable-column projection,
verified byte-identical across two fresh processes). Driver:
`gpu/spiral_f_perf.jl`.

## 1. Frozen model (registered before measurement)

- H1: cage corners (4·tets f32) + per-vertex (idx,w) are RESIDENT on device
  (streamed once at load). Per-frame device cost = deform(corners) +
  reconstruct(verts). No upload, no render in the budget (render parity is
  the E §7 deferral).
- H2: weights/indices offline f64→f32 (E's runtime model).
- H3: one kernel launch per stage per frame — no batching/fusion. This is
  the BASELINE; fusion is future headroom, never assumed.

Operating points = the D classes (pinned D1/D3 rows): blob 0.28/1100t,
teapot 0.3/500t, icosphere2 0.3/906t, robot 0.13/435t, conifer 0.6/226t,
plate 0.3/96t, grass 0.16/93t. Families: wind A=0.2 φ=0.7, fold4 A=0.4
(TetDeform exports — F-D.3 single source; local mk_* would be a defect).

## 2. Gates and verdicts

- **F-G0 provenance PASS**: all 7 classes reproduce their pinned tet count
  at the pinned h; locate misses = 0 (2 on-lattice rescues, F-D.1 path);
  harness self-check 0.00e+00 everywhere (machinery bit-exact vs TetDeform).
- **F-G1 device identity PASS (amended before any accepted run)**: v1
  mis-registered per-vertex bit-identity == 0.0 — NOT the E contract; the
  first run failed at exactly 1 ulp (1.19e-7), which is device fma
  contraction, correctly outside px95 aggregation. Amended pre-registered:
  (a) per-vertex max|Δ| ≤ 4·eps(f32)·max|coord| — measured worst 9.5e-7
  (conifer, bound ~1.2e-5); (b) E-G2 screen contract verbatim: |px95 dev −
  px95 host| ≤ 0.02 px — measured **0.0000 on all 14 rows**.
- **F-G2 frame budget PASS**: us_wall_frame ≤ 500 µs at every class ×
  family. Measured worst case **31.92 µs** (blob/fold4, 1100 tets) —
  ~16× headroom against the gate, ~63× against a 2 ms slice.
- **Determinism**: second fresh process — pre-registered stable-column
  projection (kind..family, reps, ident_max) byte-identical; derived
  instance capacities also reproduced on this machine; full-file sha
  differs ONLY in timing columns as the protocol predicts.

## 3. Measured frame cost (median of 25 after 3 warmups)

| class | tets | us_wall frame (wind / fold4) | us_dev frame | tets/µs |
|---|---|---|---|---|
| blob 0.28 | 1100 | 29.2 / 31.9 | 19.3 / 20.1 | ~34–38 |
| teapot 0.3 | 500 | 27.3 / 28.4 | 21.1 / 20.7 | ~18 |
| icosphere2 0.3 | 906 | 26.8 / 26.5 | 19.7 / 19.1 | ~34 |
| robot 0.13 | 435 | 26.5 / 26.4 | 20.2 / 19.9 | ~16 |
| conifer 0.6 | 226 | 29.4 / 25.8 | 20.8 / 19.9 | ~8–9 |
| plate 0.3 | 96 | 26.3 / 26.7 | 20.5 / 21.3 | ~3.6 |
| grass 0.16 | 93 | 26.4 / 28.9 | 19.7 / 20.5 | ~3.5 |

Read of the shape: us_dev grows with tet count (blob/icosphere2 ~19–21 µs
device) but us_wall is dominated by a ~25 µs FLOOR at these sizes — launch
+ sync latency, not throughput. Two consequences, both measured not
extrapolated: (1) at D operating points the baseline cost is roughly
FLAT in cage size — the 1100-tet blob costs barely more than the 93-tet
grass on the wall clock; (2) the obvious optimization is not kernel speed
but the launch floor (fusion of the two launches, or graph submission) —
headroom, not necessity.

## 4. Capacity (derived, not gated — illustrative instances per async slice)

At us_wall_frame ≈ 26–32 µs and 2/4/8 ms duty slices: ~62–77 instances at
2 ms, ~125–155 at 4 ms, ~250–310 at 8 ms per cage-class instance stream
(full D-quality cage skin per instance, H1–H3 baseline, no sharing).
Even the heaviest class (blob 1100t) sustains ~62–68 instances at 2 ms.

## 5. Budget verdict

**The D-quality cage path FITS the frame on RTX 5060 with ~16× margin at
the pre-registered 500 µs gate (3% of 16.6 ms).** The gate was set
deliberately loose; the measured number (≈30 µs) is far inside it. Cost is
no longer an open risk at D operating points on this class of hardware.

## 6. Defects registered during the spiral

- **F-F.1 (gate amendment, registered before any accepted run)**: F-G1 v1
  required per-vertex device/host bit-identity == 0.0 — overstated E's
  proven contract (screen-metric px95, ≤0.02 px) and ignored legitimate
  device fma contraction; first run failed at exactly 1 ulp. Amended to
  (a) ulp-scale object bound + (b) E-G2 screen clause verbatim, BEFORE any
  gate was allowed to pass. Lesson: an identity gate tighter than the
  authority's own contract does not measure the authority — it measures
  the fma contract of the compiler.
- **F-F.2 (fixed, verification tooling)**: the ×2 determinism check
  compared a file against a process-substitution stream read TWICE — the
  second read of a drained fifo silently returned empty, producing a
  bogus 42-line diff and a false alarm (then, when "fixed" by cutting
  different fields, a false PASS). Redone with real files on both sides
  and the exact pre-registered field projection. Same class as F-E.9:
  the verification step is itself code and gets attacked like code.

## 7. Falsification conditions

- F-G2 violation on other hardware tiers (or after adding real render +
  upload to the pipeline): reopen the quality/cost tradeoff (LOD,
  instancing, fusion).
- If frame budgets drop below ~60 µs (240 Hz), the ~25 µs launch floor
  becomes the binding constraint — measure fusion/graph submission before
  concluding.
- Capacity numbers assume H1–H3; multi-cage scenes with shared cages or
  batched launches change the model and must be re-measured, not derived.

## 8. Ordering consequences

- Goal 2 §5 is closed: fidelity (D), hardware arithmetic (E), and now cost
  (F) are all measured. The remaining open items are the Lava/Vulkan
  render-pass parity (E §7 deferral) and the plate-class product decision
  (D3 floor — contract choice, not research).
