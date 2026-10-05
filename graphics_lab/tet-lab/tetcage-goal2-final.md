# TETCAGE GOAL 2 — FINAL REPORT

**Status: ALL CLOSED PASSED with pinned evidence (2026-10-02). Spiral I
(comparators) measured on the RTX 5060 after the F-I.4 reboot cleared the
platform blocker and F-I.5's first-hardware kernel bug was fixed (driver
v4 → v5; full chain in tetcage-rt-failure-register.md). Gates: HI-G0 provenance,
HI-G1 identity all five paths, HI-G2/G3 honest accounting, HI-G4 stable
columns byte-identical ×2 fresh processes (140 rows, stable-columns sha16
`4a9144326ebed3cb`). Canonical CSV: `results/spiral-i-comparators.csv`
(full-file sha16 `78c9274aee143184`, run b; us_* columns are machine-state —
see §8 for the one corner where run-to-run wall order is not stable).**

Driver: `gpu/spiral_i_comparators.jl` **v5** (v4 + first-hardware fixes,
2026-10-02: F-I.5 `lbs!` palette instance offset — g is 0-based; B_direct bound
to its registered N·V-resident vertex arrays; ident_A/ident_auth x-comprehension
instance wrap. All three are index/binding convention bugs; no arithmetic or
gate changed. Crash logs archived at `logs/spiral-i-run1.log`,
`logs/spiral-i-g2.log`; clean run logs `logs/spiral-i-v5-run-{a,b}.log`.)

Pins cited: D1 `694ed4eb0b5a0ff7`, D2 `b2e37d1ba2cc6784`, D3 `d93b7dcaab224a64`,
temporal `0188b1c7d49131b2`, E `c4b3ec1c9a0adfe3` (err-vector `7c7102c2615a6ddb`),
G `6b5164abcf847bd1`, H `2efbcbbcbba381ed`. Corpus audit `5d03c0ac…`.
Spiral I stable columns `4a9144326ebed3cb` (×2).

---

## 1. WHAT TETCAGE BEATS (evidence-backed)

- **Fused per-vertex deformation vs two-launch pipelines (H, `2efbcbbcbba381ed`).**
  Fused A-then-B composes: wall 28.5 → 16.2 µs on blob/wind (−43% to −50% across
  all 7 classes × 2 families); device time 22.5 → 11.7 µs. Fusion+graphs give a
  **2.9× frame-economics headroom** (blob wind: 29.17 µs split wall → 9.9 µs
  graph-fused).
- **Launch-floor economics (H).** Per-@cuda dispatch floor measured ≈ 7.8 µs
  (15.5 µs/pair back-to-back). TetCage's one-kernel frame amortizes it once;
  any incumbent that must upload-then-deform pays dispatch + transfer.
- **Authoritative deformation quality (D/E).** GPU ≡ CPU-f32 bit-identical
  (E-G2); px95 in the pinned D-band on all tested classes (blob wind 0.2184 px,
  blob fold4 0.2536 px, conifer wind 0.5410 px, plate wind 1.6722 px — E CSV);
  render-chain parity vs Vulkan max divergence **8.4e-5 px** (G, ~1 ulp of fb
  coords, 6000× under the 0.5 px gate). E §7 deferral RESOLVED.
- **Instancing economics vs uploaded-state incumbents (Spiral I, MEASURED).**
  A_auth (48·T·N B/frame) loses to A_param (0 B) in **28/28 (class, N) cells**,
  from N=1 (blob: 9.3 vs 55.9 µs) to N=128 (55 vs 1096 µs). Per-frame cage-state
  upload never amortizes anywhere in the measured ladder. C_lbs (24·N B/frame)
  also beats A_auth in 28/28 cells — despite a ~16-19 µs device kernel floor of
  its own (§2 caveat). On this job the cage-state incumbent is never
  competitive, and the palette incumbent pays a kernel floor, not a bandwidth
  bill.

## 2. WHAT BEATS TETCAGE (honest envelope)

- **B_direct — direct analytic deformation of resident vertices — needs no
  representation at all, and it shows: it wins wall in 26/28 (class, N) cells**
  on the affine rotation job (floors ≈ 6.0 µs wall / 1.8-2.0 µs device,
  graph-replayed, 0 B/frame). The registered prediction HI-P1 ("B_direct device
  time < A_param device time for vertex-dense classes") is **AMENDED per
  protocol (F-H.2 class): falsified in the strong form at (blob, N=128) in
  BOTH runs** — B device 66.4 vs A_param 50.9 µs (run b); 1563 vs 103 µs (run
  a). Direction of the flip is stable across runs; its magnitude is
  machine-state. **Measured boundary: the shared-cage gather (A_param: 4·T
  corner transforms + N·V gathers from a SHARED, cache-resident cage)
  overtakes per-instance vertex streaming (B: N·V reads + writes) only at the
  vertex-dense × high-instance corner (V=10242, N=128).** At N ≤ 32 B wins even
  on blob, and on every class with V ≤ 3508 B wins through N=128. **Amended
  negative envelope: on affine parametric motion, skip the cage — deform
  vertices directly — EXCEPT at extreme (V × N) working sets, where
  shared-state wins.**
- **C_lbs byte-economy, now measured, with a recorded caveat:** the 24·N B/frame
  upload is real (3072 B at N=128 vs A_auth's 6.76 MB) and C beats A_auth
  everywhere; at the (blob, N=128) corner C ties A_param for fastest
  (66.7 vs 55.1 µs run b — the A/C ordering flips between runs, machine-state).
  CAVEAT (measured): our registered LBS kernel has a FLAT ~16-19 µs device
  floor independent of vertex count (robot 48 verts ≈ blob 10242 verts at N=1)
  — the per-vertex, per-bone dependent palette-gather fetch pattern, not bytes,
  dominates C's wall at small N. A production rig with bone matrices in
  constant memory would not carry this floor; C's walls here are an UPPER
  bound on a tuned LBS.
- TetCage's surviving regime stands, now measured-around rather than
  measured-in: the rotation family is B_direct's and C's native regime. Where
  the family is NON-affine (twist/fold), B_direct has no closed form and C_lbs
  cannot represent it at all (structural), while A_param executes it exactly
  through the cage path (A5-G2). The mandate's honest answer: on the ONE family
  all five paths can execute, an incumbent posts the fastest wall in 26/28
  cells and A_param/C_lbs split the remaining corner; TetCage earns its
  complexity only outside the affine closed-form regime.

## 3. WHY (mechanism — now measured)

- TetCage prices O(4·T) corner transforms + O(V) weighted gather; the gather
  reads a SHARED cage across instances (parametric) → bandwidth per instance
  tends to zero as N grows. Incumbents that upload state grow linearly in
  N·(state size). Incumbents that don't upload (B_direct) skip the gather —
  and win on device time up to the measured (V × N) corner. The cage earns its
  keep only when the deformation is not expressible per-vertex in closed form,
  or the state is authored (art-directed cages), or at working sets where
  streaming N·V vertices costs more than gathering from shared state
  (measured: only blob N=128).
- Measured cost shape (wall): A_param = 6.4-6.8 µs floor (launch + gather) +
  superlinear growth only at the largest working set (blob N=128: 55-112 µs
  across runs); B_direct = 6.0 µs floor + growth with N·V; C_lbs = ~17 µs
  kernel floor (§2 caveat) + growth; A_auth and D_morph scale linearly in N
  with slopes set by state size (48·T vs 24·V bytes/instance).
- Frame economics: graph-replayed single kernel ≈ 9.9 µs wall on the largest
  class (H) → ~200 instances per 2 ms slice (F) before any batching wins.

## 4. CROSSOVER SURFACES (MEASURED 2026-10-02)

Source: `results/spiral-i-comparators.csv` (stable columns sha16
`4a9144326ebed3cb`, ×2; walls quoted from run b, machine-state band noted).

- **A_param vs A_auth: NO crossover** — A_param wins from N=1 at every class
  (28/28). Per-frame cage-state upload never pays.
- **C_lbs vs A_auth: NO crossover** — C wins from N=1 at every class (28/28);
  the byte model (24·N vs 48·T·N) is confirmed on the wall even at N=1 despite
  C's kernel floor.
- **D_morph becomes worst** exactly on the vertex-heavy classes (blob, conifer:
  worst-of-five at ALL N; e.g. blob N=8: 1174 µs vs A_auth 278). On cage-dense
  small-V classes (teapot, icosphere2, robot, grass — V < 2T) D actually BEATS
  A_auth at all N (18/28 cells overall): the morph stream's 24·V·N bytes beat
  the cage stream's 48·T·N bytes when the mesh is cage-dense. Pre-registered
  "D strictly dominated" therefore needs the B_direct caveat (§9.8): B beats D
  in 28/28 cells, but A_auth does not dominate D.
- **Device-time B vs A sign (the P1 boundary):** flips ONLY at (blob, N=128)
  (B 66.4 vs A 50.9 dev, run b; 1563 vs 103, run a). The boundary sits between
  N=32 and N=128 at V=10242 and is not reached for V ≤ 3508 within N ≤ 128.
- **Negative envelope:** cells where an incumbent's wall < A_param's: 27/28 via
  B_direct (all except blob N=128). At (blob, N=128) A_param wins its cell
  against B in both runs but ties C_lbs (run a: C 67.3 < A 112.2; run b: A
  55.1 < C 66.7 — ordering machine-state). A_param is never slower than ALL
  incumbents, and never faster than B anywhere except that corner.

## 5. QUALITY BOUNDS

- LBS (C): cannot represent twist/fold — cage-path families out of reach;
  measured here only on the rotation family (its native regime).
- Morph streams (D): arc-vs-chord lerp error at DPHI cadence — MEASURED
  (CSV notes): 11.3-18.5 px@5 on small classes (grass 11.3, teapot 12.4,
  icosphere2 13.1, blob 13.8, plate 18.5, robot 15.1), 86.3 px@5 on conifer.
- TetCage: D-band px95 (§1); exact (0 error) on affine families through the
  cage path (A5-G2).
- Measured on the rotation family (the one family all paths share): all five
  paths land inside the E-G2 ulp bound of the host f32 reference (ident
  0.03-2.5× of 4·eps(f32)·maxcoord; D_morph exactly 0.00e+00; A_param ≡ A_auth
  identical ident values, as constructed). Recorded per class × N in the CSV
  notes.

## 6. FRAME ECONOMICS (pinned + measured)

- Split frame (E kernels, F pins): 26.4-31.9 µs wall across classes; dev
  19.1-21.1 µs.
- Graph-fused (H): 8.8-11.2 µs wall; blob wind 9.9 µs; ~200 inst / 2 ms slice.
- Launch floor ≈ 7.8 µs/dispatch — the binding constraint below ~10 µs frames.
- MEASURED incumbent walls at N ∈ {1,8,32,128} (upload + kernel + sync; ONE
  batched pinned memcpy per component per frame = the incumbent's best case):
  B_direct 5.9-66.8 µs; A_param 6.4-112 µs; C_lbs 16.8-67.3 µs; A_auth
  18.9-1096 µs; D_morph 12.1-4860 µs (blob N=128; corner band in §8).
- Measured graph-replayed single-kernel wall floor ≈ 6.0 µs (B_direct on small
  classes, all N) — consistent with H's 8.8-11.2 µs on real classes; the 7.8
  µs @cuda dispatch floor remains the binding constraint below ~10 µs frames.

## 7. MEMORY ECONOMICS (per frame, N instances, T tets, V verts)

Verified by HI-G2 measured uploads (bytes_up_frame column = registered model):
A_param **0 B**; A_auth 48·T·N B; C_lbs 24·N B; D_morph 24·V·N B; B_direct **0 B**
(resident geometry, uploaded once at setup — its true form, N·V vertices).

## 8. REMAINING PRODUCT RISK

- ~~F-I.4 platform blocker~~ CLOSED (genuine reboot 2026-10-02 04:49; health
  PASS). ~~F-I.5 first-hardware kernel bug~~ CLOSED (v5; ×2 gates GREEN).
- ~~dev-timing on graph launches~~ RETIRED: CUDA.@elapsed worked on graph execs
  on the first try; device columns present for all graphed paths, no
  substitution.
- NEW (measured): wall ordering at the largest working set (blob, N=128) is
  machine-state-sensitive — between the two runs, B_direct's loss swung
  14× → 1.2× and the A_param-vs-C_lbs order flipped. Stable columns are pinned;
  capacity claims at that corner carry this band. All other cells ordered
  identically across runs.
- NEW (measured): our C_lbs kernel's flat ~16-19 µs device floor (dependent
  palette-gather fetch pattern) makes C's small-N walls an upper bound on a
  tuned production LBS.
- LBS/morph bounds are structural claims beyond the rotation family (§5); a
  real product rig may behave differently. A_auth's regime (art-authored
  cages, non-affine families) is not measurable on this job by construction.

## 9. MISSION ANSWERS — FINAL

1. **What does optimized TetCage beat?** Split pipelines (2× device-side, H),
   launch-floor-bound frame budgets (2.9× headroom, H), and every
   state-uploading incumbent at every measured (class, N) on the shared affine
   job — A_auth 0/28 cells, C_lbs behind at small N on its kernel floor — and
   at the one corner where direct deformation loses (blob, N=128), A_param is
   the fastest path in run b (55.1 µs vs B 66.8 / C 66.7).
2. **What beats TetCage?** B_direct: closed-form affine families win 26/28
   cells (≈6.0 µs floor, 0 B/frame) — on the one family all five paths can
   execute, "no representation" beats the cage almost everywhere; the exception
   is the extreme V×N corner (P1 amended). Plus the art-authored-cage regime
   (a tie by construction, unmeasurable on this job).
3. **Why?** Shared-state gather vs per-instance state upload; compute vs
   bandwidth; launch-floor amortization. Measured: the gather wins only when
   N·V streaming overwhelms it (blob N=128); everywhere else the missing
   representation wins.
4. **Crossover?** §4: no A_auth or C-vs-A_auth crossovers (decided at N=1);
   the B-vs-A device sign flips between N=32 and N=128 at V=10242 only;
   D_morph is worst-of-five only on vertex-heavy classes.
5. **Quality bounds?** §5 (structural + measured rotation-family identity and
   morph arc-vs-chord numbers).
6. **Frame economics?** §6.
7. **Memory economics?** §7.
8. **RepresentationPolicy first decision boundary — MEASURED SIGN-OFF:**
   choose B_direct when the family is closed-form affine AND (V·N) sits below
   the measured shared-state corner (safe through V ≤ 3508 × N = 128 measured;
   the one flip is V=10242 × N=128); choose A_param (TetCage) when the family
   is non-affine but parametric — exactness there is proven (A5-G2), and its
   shared-state economics win the only corner where B_direct loses, at 0 B/frame;
   choose A_auth only for art-authored per-frame cages — never on wall
   economics (0/28 cells); never choose D_morph for rigid/affine jobs when
   B_direct is available (B beats D 28/28) — D's surviving regime is
   authored-pose streams for non-affine families, unmeasured here; choose
   C_lbs only when a production rig already exists (org cost, not measured
   cost) — and budget its kernel floor, not just its 24 B/frame.
