# TetCageRT Failure Register

Living register of every defect, non-reproducibility, and source limitation
found during the reconstruction. Defects are not smoothed over; they bound the
conclusions.

## F1 — Table 2 absolute memory rows are not reproducible from Table 1
Severity: bounds all absolute-MB claims.
The two-constant model, solved exactly on the Grass/500 anchor (145.6 MB,
16.1×), predicts Tree×25 and Frog×81 at −43% to −77% below Table 2, under
both high- and medium-cage labeling hypotheses. Plausible causes [INF]:
per-scene ε-growth differences, compaction state, per-object instance-table
overheads not in the model, or Table 2 measured with different cage
resolutions than Table 1. Consequence: absolute-MB predictions carry ~2×
uncertainty; only RATIOS anchored on 16.1× (paper) / 47× (GPUOpen) are quoted
as findings. Resolution path: re-measure on WGE hardware in the prototype
slice.

## F2 — Watertight variant is not a runtime candidate on current hardware
Severity: bounds the watertightness investigation.
[PAPER Table 2] watertight = 2.3–3.2× memory and 19–80× render time
(software 4D-BVH traversal through DXR procedural geometry). The roadmap's
"watertight 4D barycentric representation" step can therefore only produce a
validation oracle on RDNA4-class hardware, not a shipping path. Instance
variant remains statistical (ε-growth; sphere test 0.01% → 0% escapes).

## F3 — Duplicate primitive intersections at tet boundaries
[PAPER §8] shared edges can be tested multiple times when adjacent tets
deform differently; unique edge ownership named as mitigation but not
implemented in the paper. Unknown: cost on dense foliage card geometry.
Must be quantified in the CPU reference before any prototype.

## F4 — GPUOpen article not machine-readable
The 2026-07-22 GPUOpen article is JS-rendered; text extraction returned nav
boilerplate only. Demo anchors (80 GB→1.7 GB, 300 ms→3.3 ms, 25k plants,
500M tris) were cross-verified through press (wccftech 2026-09-20) and the HN
thread instead, and are labeled secondary wherever used. A human with a
browser should archive the article page for the record.

## F5 — 2× cross-source spread on per-tetrahedron update cost
Paper Table 3 implies 3.575 ns/tet (10.01 ms / 2.8M tets); the GPUOpen demo
implies 1.65 ns/tet (3.3 ms / 2M tets). Both far below the 0.60 ns/tri
per-triangle cost, so the direction of the conclusion is unaffected, but
update-ratio predictions carry a 2× band. The model reports both.

## F6 — Codex plan defect: six deliverables omit the exit gate
Codex's mandate ("integration-ready artifact, not a research essay") is
adopted, but its six-deliverable list omits the mega-sprint §17 entry gate —
"conventional path exposes a measured problem it solves" — which the mainline
owns. The integration contract therefore ships as DESIGN ONLY with explicit
adoption preconditions, and the benchmark plan targets the operating
envelope, not a promotion receipt. Second defect: Codex's benchmark harness
omits the CPU-oracle-first ordering that both the roadmap and the handoff
require; restored in TETCAGERT_BENCHMARK.md phases.

## F7 — WGE-specific unknowns (open, blocking nothing in the mainline)
- Foliage-card thinness vs ε: the paper's watertightness probe used a sphere
  (r=100); WGE conifer cards are far thinner. ε-sensitivity sweep required.
  **CLOSED (Spiral C7):** sweep run on plate/grass/conifer + sphere control
  (`results/spiral_c7_thin_eps_sweep.csv`, sha `02b9b42e…`). Mechanism scales
  as gap_max = K(class)·ε·h³ (sphere K=1068 invariant to 0.1% across h;
  plate 6.0; grass 77; conifer 242–340). Thin geometry is not ε-fragile;
  the ε-policy verdict is recorded in TETCAGE_WATERTIGHTNESS.md.
- Bone-weight transfer quality on joint-heavy geometry is reported
  qualitatively ("could be improved") — no numbers in the paper. The CPU
  reference must quantify the artifact band vs cage resolution.
- Vulkan/Lava equivalence of the DXR instance-transform mapping is asserted
  by API shape, not yet probed on RDNA hardware.

## Oracle defects found and fixed during Spiral A2/A3 (all caught in-lane)
- **F-A2.1 (P0, fixed):** tet-plane orientation inverted (`< 0` instead of
  `> 0` swap) → clipping emitted zero triangles. A pass-shaped output was
  impossible here (zeros are loud), but the same sign error silently
  corrupted cage-build witness tests before the loud failure.
- **F-A2.2 (P0, fixed):** vertices passing exactly through a clip plane
  (d == 0) kept stale plane sets → TE vertices mislabeled as the (proved-
  unreachable) P-class; wrong dedup keys. Fixed: exact incidence recorded
  on every pass. Anomaly census now 0.
- **F-A2.3 (P0, fixed):** hand-set grids missed geometry (conifer covered
  26% of the tree) → sub-1× "expansion", physically impossible for a
  covering cage. Fixed: grids derived from mesh AABB (`auto_grid`).
- **F-A2.4 (fixed):** V-keys were triangle-local; global dedup collapsed
  distinct mesh vertices. Fixed with mesh-vertex ids in keys.
- **F-A2.5 (fixed pre-run):** "mirror" Freudenthal t4–t6 duplicated t3
  (voxel not tiled); TE-key dead code; missing W3 edge (3,1); constructor
  arity mismatch. Caught by derivation-before-code review.
- **F-A2.6 (method finding):** pre-registered expansion prediction was
  direction-inverted; corrected model (expansion monotone decreasing in
  tris/tet) validated against paper Table 1 independent data.
- **F-A1.1 (from corpus turn, promoted here for visibility):** recalled
  icosahedron face list 19/20 bogus; all constants now derived + asserted.

## Spiral C defects found during C7/C8 closure (all caught in-lane)
- **F-C.3 (P1, fixed):** TE-key carried the EXACT f64 edge parameter as its
  5th slot. Cross-tet copies of the SAME chord point disagree at ulp scale
  (the very seam mechanism Spiral C measures), so the exact key gave them
  different keys and the gap census never compared them — undercounting TE
  gaps and making vert counts drift-fragile. Evidence (blob5): 3664 TE
  instances sit on 724 conceptual chord points (1e-12 clusters); 469
  cross-tet pairs differ only at ~1e-15; exact key → 1225 keys. Fix:
  quantized parameter `TE_T_QUANT = 1e-9` (8 decades above copy drift, ~6
  below real chord separation; zero false merges, max in-cell spread
  1.1e-15). TE keys 1225 → 724; vert_exp(blob5) 2.587 → 2.538 (now
  symmetric with tri_exp 2.538, as the model predicts). A5 gates re-verified
  GREEN; corpus audit 0 anomalies, byte-identical across processes.
- **F-C.4 (process, admitted):** the pinned blob5 reference (hash16
  `e730f8965c586853`, verts_out=26463) is UNREPRODUCIBLE from the on-disk
  F-C.2 code (measured 26494/1225-TE; hash `d99cf7b1…`) — the pin recorded
  a transient state (exact-t keys on ulp-drifted geometry) without pinning
  the full kinds census that would have exposed it. F-OPT.1 class: hashes
  recorded without enough fields to verify. Corrected pin (post F-C.3):
  blob5 tets=1100, tris_out=51982, verts_out=25993, kinds
  V=10242/EP=15027/TE=724/TV=0, hash16 `3a2b58c3c9139095`, now verified in
  two fresh processes and covered by the pinned corpus audit script
  (`oracle/corpus_audit.jl`, audit sha `5d03c0ac…`).

## Spiral D defects (all caught in-lane or pre-publication)
- **F-D.1 (fixed):** `locate_tet`'s sdist <= 0 containment rule is a
  rounding-sign lottery for axis-aligned vertices sitting EXACTLY on
  lattice planes (plate x=0, z-trough): a corner point can round
  "outside" for every candidate tet, so coarse-h locate missed. Fix:
  two-pass locate — exact rule first, residual misses rescued by the
  tet with smallest max-plane sdist within tol = 1e-9·h³ (sdist units
  are ~h³), `near_misses` reported on stderr (1–8 rescues per coarse
  run observed). Pure-perf companion: O(1) Dict index map replaced the
  O(tets) findfirst per vertex; deterministic first-hit order unchanged.
  Consequence (positive): the cage path became TOTAL at every probed
  operating point — the D2 orphan fallback never engages.
- **F-D.2 (fixed):** mesh_scale by radial extent degenerates on
  spherical meshes — icosphere2 rows showed meaningless EXACT zeros
  (extent ≈ ulps → scale ≈ 1e16 → normalized families collapsed to
  identity/affine). Fix: normalize by max AABB side (well-defined for
  every class); D1 re-run under the corrected normalization.
- **F-D.3 (fixed, debt registered):** the D2 driver referenced
  mk_wind/mk_twist/mk_fold, which live only in the D1 DRIVER —
  TetDeform exports only the fixed-constant f_* variants →
  UndefVarError at first probe. Fixed by verbatim copy into the driver.
  Debt: family semantics now duplicated across 3 files; single-source
  into TetDeform BEFORE Spiral F.
- **F-D.4 (driver gate, amended with registration):** the temporal
  draft's second gate (px95 max/min over the period <= 1.25) conflated
  smooth phase-translation of the error field (legitimate: the wind
  crest slides across the cage) with actual popping (frame-to-frame
  DIScontinuity). blob f64 correctly failed it. Amended gates,
  registered BEFORE the amended run: adjacent-frame px95 growth
  <= 1.25/1.50 (f64/f32) and curvature |Δ²px95| <= 0.25/0.35 of max
  px95 — a one-frame pop is delta-like and violates grossly.
- **F-E.1 (superseded draft):** v1 of the GPU driver used a placeholder
  kernel and flattened only one corner slot per tet; replaced by v2
  before any gate run. Lesson: no driver reaches a gate with scaffold
  internals active.
- **F-E.2 (fixed):** `weights_and_indices` stores idx = 4·(ti−1)+s —
  ALREADY 1-based (Julia CuArrays are 1-based; the "0-based for device"
  comment was wrong). Host consumers (self-check, cpu_f32_path) added a
  spurious `+1`, shifting every corner reference by one slot
  (v1→v2 … v4→next tet's v1) → ~1.5 identity error at first self-check.
  Fix: remove `+1` in both host sites; device kernel was already correct.
- **F-E.3 (fixed, process lesson):** misdiagnosed F-E.2 as a transposed
  cofactor inverse and "fixed" bary() to column form — still failing at
  a different magnitude BEFORE any root-cause read of TetBasis. The
  robust fix delegates to TetDeform.TetBasis.rest_basis/inv3 and mirrors
  cage_path_positions' λ lines verbatim (self-check then 0.00e+00).
  Lesson: never patch numeric conventions from memory; read the authority
  first, then eliminate the convention class by delegation.
- **F-E.4 (fixed):** `@cuda` requires an actual function CALL — a
  broadcast expression as second argument errors at compile time.
  Kernels wrapped in proper call syntax.
- **F-E.5 (fixed):** reconstruction output buffers allocated via
  `similar(dcx)` sized nC (corners) while reconstruct! writes nV
  (vertices) → device BoundsError on blob at global thread nC+1 (7329).
  Fix: `similar(dcx, nV)`.
- **F-E.6 (fixed):** the TE census read the reconstructed VERTEX arrays
  (nV) at CORNER indices (1..nC) — wrong gate semantics AND latent host
  OOB whenever nC > nV (plate: 3× more corners than vertices). Fix:
  census consumes the deformed corner arrays; vertex buffers never
  indexed by corner ids.
- **F-E.7 (fixed, gate-integrity):** E-G2's D-band clause was scored
  against the IDENTITY cage path (P_ref = p→p, computed once for the
  self-check) instead of the f64 DEFORMED cage path under each family's
  GT field — the clause passed vacuously (identity px95 is 30–350× the
  deformed value). Caught by Critic cross-check of the CSV against D1's
  pinned px95 rows. Fix: per-family cage_path_positions(V, cage, loc,
  fgt); the intermediate CSV sha16 `805d2abf45593c51` is INVALID and
  superseded by `c4b3ec1c9a0adfe3`. Lesson: a gate that cannot fail is
  not evidence; always validate a new gate against a known pinned
  reference before trusting its PASS.
- **F-D.3 — CLOSED (debt paid):** mk_twist/mk_wind_phase/mk_wind/mk_fold
  (+ mk_wind_like, E's phi-exposed signature) single-sourced into
  TetDeform as exports; verbatim copies deleted from spiral_d_fidelity,
  spiral_d2_adaptive, spiral_d3_confirm, spiral_d_temporal, and
  gpu/spiral_e_gpu. Re-pin proof that semantics are unchanged: all five
  drivers re-run ×2 fresh processes, every CSV byte-identical to its
  pinned sha16 (D1 `694ed4eb0b5a0ff7`, D2 `b2e37d1ba2cc6784`,
  D3 `d93b7dcaab224a64`, temporal `0188b1c7d49131b2`, E
  `c4b3ec1c9a0adfe3`); A5 gates re-run GREEN (G1 2.5e-16, G2 3.5e-16);
  warnings/world-age = 0 on all runs. Spiral F unblocked.
- **F-E.9 (fixed, re-pin protocol + containment):** three stacked
  defects found by Critic during the F-D.3 re-pin: (1) D1's output path
  was CWD-relative (`open("../results/…")`) while every other driver is
  ROOT-anchored — re-runs from TetLab/ wrote OUTSIDE the quarantine to
  `graphics_lab/results/`; fixed to `joinpath(ROOT, "results")`.
  (2) The D1 re-pin check FALSE-PASSED: it hashed the stale pinned file
  (same relative path from a different cwd) instead of the fresh output;
  caught because the escaped directory appeared in git status.
  (3) D1's CSV embeds wall-clock `build_ms,locate_ms` columns — a
  full-file sha can never reproduce across processes. PROTOCOL AMENDMENT
  (pre-registered here): D1's pin = measurement columns 1–14 (verified
  byte-identical pin == run a == run b via driver stdout, which prints
  the CSV); full-file byte-identity remains the standard for D2/D3/
  temporal/E (no timing columns). Post-fix D1 re-runs land inside
  TetLab/results and reproduce columns 1–14; the pinned artifact
  (sha16 `694ed4eb0b5a0ff7`) is restored byte-intact afterward.
- **F-F.1 (driver gate, amended before any accepted run):** Spiral F's
  F-G1 v1 required per-vertex device/host bit-identity == 0.0 — an
  overstated contract: E proved bit-exactness on the SCREEN metric
  (px95 ≤ 0.02 px), and device fma contraction legitimately produces
  last-ulp object diffs. First run failed at exactly 1 ulp (1.19e-7).
  Amended BEFORE any gate passed: (a) per-vertex |Δ| ≤ 4·eps(f32)·
  max|coord|, (b) E-G2 screen clause verbatim. Lesson: an identity gate
  tighter than the authority's own contract measures the compiler's fma
  policy, not the authority.
- **F-F.2 (fixed, verification tooling):** the ×2 determinism check
  diffed a file against a process-substitution stream consumed TWICE —
  the second read of a drained fifo silently yields empty (bogus 42-line
  diff / false alarm); the first "fix" then compared DIFFERENT field
  projections (false pass until the projection mismatch was noticed).
  Redone with real files on both sides and the exact pre-registered
  projection (fields 1-6,13,15 of perf rows). Same class as F-E.9: the
  verification step is code — attack it like code.
- **F-G.1 (fixed, convention risk):** the harness was drafted against a
  DIFFERENT Vulkan.jl depot copy (~/.julia/packages/Vulkan/12azN,
  registry version) than the one the repo-pinned rev instantiated
  (ZSqbR) — the two differ constructor-by-constructor (kw vs positional,
  argument orders, exported names). First run failed immediately.
  Fix: re-derived every struct/function signature from the pinned depot's
  generated/linux.jl before rewrite. Lesson: for generated API wrappers,
  "the package" is the pinned REV — conventions must be verified against
  the exact depot path the project instantiates.
- **F-G.2 (fixed, sentinel-diagnosed):** the vertex shader put the
  perspective divide in clip_x/clip_y AND relied on the fixed-function
  divide via clip_w (double-divide), and the host fetch omitted the
  W/2, H/2 framebuffer offset — first render produced analytic −52 px
  column reads and sentinel hits. Fix: clip_x = 2·f·x/W with clip_w = zc
  (divide happens ONCE, fixed-function); fetch at (W/2+gx, H/2+gy).
  Caught by the NaN-clear sentinel design within one run.
- **F-G.3 (fixed, method):** naive per-vertex cell-fetch quantizes the
  measurement (sub-px f32 drift crosses cell boundaries) and collides
  when vertices share a predicted cell (last splat wins). Method amended
  pre-acceptance to chunked value parity (see TETCAGE_VULKAN_PARITY.md
  §3): rounds of distinct cells + 0.01 px value-match acceptance + solo
  re-render fallback.
- **F-G.4 (fixed, perf):** first-fit chunk restart produced O(n) tiny
  chunks on dense classes (blob: ~6k submit+8MB-copy cycles → 600 s
  timeout ×2). Amended pre-acceptance to ROUNDS of pairwise-distinct
  cells (blob: 8 rounds) — same attribution guarantee, batched.
- **F-G.5 (fixed, device fault):** vertex buffer written with 12-byte
  stride while the pipeline binding declared 24 — GPU read half its
  vertex slots from padding garbage (points landing arbitrarily →
  sentinel hits) and fetched past the buffer end near the tail (device
  fault, abort). Run-8 log also captured a follow-on host crash during
  Julia error DISPLAY (heap poisoned before showerror). Fix: stride 12.
- **F-G.6 (fixed, lifecycle):** manual destroy_buffer/free_memory per
  chunk + Vulkan.jl wrapper finalizers on the same handles = double-free
  (heap corruption at varying points: runs 9–11 crashed at startup, mid-
  display, and during type inference; the bisect probe — which never
  destroys — ran clean twice, isolating the destructor path). Fix: NO
  manual destruction; single ownership via GC finalizers on the pinned
  rev. Lesson: wrapper libraries with finalizers own their handles —
  never destroy manually on this rev.
- **F-G.7 (registered, scope):** G-G2 class-set amended pre-acceptance:
  conifer's wind GT field is analytically off-camera at D (y=1028.5 >
  1024 fb-px, measured) — falsifying "0.5 px parity" via viewport
  CLIPPING would be out-of-convention; conifer retained with fb-clip
  annotations, cube+skeleton added as strengthening in-camera classes.
- **F-G.8 (fixed, pin provenance):** cube/skeleton tet pins were taken
  from the corpus-audit table (their audit-h, not h=0.3) → G-G0 assert
  failed. Re-pinned by measurement at h=0.3 (cube 300, skeleton 218).
  Lesson: pins must cite the operating point they were measured at.
- **F-G.9 (fixed, gate form):** the solo-rate methodology assert (≤5%→50%)
  misfired on cube: its 8 corners land on EXACT cell-boundary fb coords
  → coverage coin-flip → 8/8 solo. The solo path is exact-attribution, so
  a systematic offset surfaces in G-G2 on solo errors, not in the solo
  count; guard downgraded to a diagnostic println.
- **F-H.1 (model calibration, cheap direction):** HF-G3's fused-vs-split
  prediction (≤0.75) was conservative by ~2× — measured 0.50–0.57, and
  the fused kernel also won ~2× on DEVICE time (no corner global
  round-trip). Gate kept at 0.75; calibration miss noted.
- **F-H.2 (falsified prediction, amended per protocol):** HF-G4's bound
  "back-to-back launches ≤ 10 µs" FALSIFIED on first run — 15.5 µs/pair
  = ~7.8 µs per @cuda dispatch (Julia-side dispatch is the launch floor,
  above the F-era model). Amended to a consistency sanity (E < A) and
  the decomposition is now REPORTED (blob/wind: E=13.0 µs launch-only vs
  A=28.5 µs wall vs D=9.9 µs graph-fused).
- **F-H.3 (constraint, by design):** CUDA graph capture cannot include
  kernel compilation/allocation (CUDACore docstring contract); capture is
  performed post-warmup with all buffers pre-allocated — replayed outputs
  verified ulp-identical to stream outputs before any timing.
- **F-I.4 (OPEN, platform blocker):** at 02:32 (between H's last good
  compile 01:30 and the first Spiral I run), fresh GPU-kernel compilation
  broke MACHINE-WIDE on this host: every NEW kernel (fresh processes,
  fresh Pkg.add CUDA env, --pkgimages=no, full ~/.julia/compiled wipe +
  re-precompile, renamed verbatim clones of passing kernels, appended
  probes inside the passing E driver's own context, JULIA_OBJCACHE=0)
  fails identically. MECHANISM (established 05:40): CUDACore registers
  device intrinsics (blockIdx/threadIdx/...) in an overlay method table
  (CUDACore.method_table, device/utils.jl:7 via Base.Experimental.
  @MethodTable/@overlay); in every failing fresh compile the overlay is
  NOT consulted — `getproperty → getfield` on `blockIdx().x` lands as an
  unknown runtime call (jl_f_getfield) and GPUCompiler rejects the IR
  (invalid for GPU). Pre-breakage kernels still compile/pass through
  persistent compiled state whose store location remains UNIDENTIFIED
  (ruled out: ~/.julia/compiled, CompilerCaching LMDB, GPUCompiler
  scratchspace, objcache env flag; loaded package trees git-clean: CUDA
  6.4.1/wO4Pw, CUDACore 6.4.1/yn2cv, GPUCompiler 2.9.0/sl7Rn, GPUToolbox
  3.2.0/jTqSh, LLVM 9.13.2/JGisQ — all predating the window; julia
  1.12.6 unchanged since Sep 11; gpu/Manifest untouched since 19:43).
  Discriminators archived: T1 old-method/new-signature FAILS; renamed
  verbatim clone FAILS in the passing driver's own process; env-var
  path (JULIA_OBJCACHE=0) does not flip the behavior. Concurrent-
  session evidence: /tmp/probe_err.log (02:47, INSIDE the window) headed
  `cyan-shim-kernel julia=1.12.6 state=/tmp/probe_state.json` — an
  unrelated agent was running kernel-shim experiments on this host
  during the breakage window; multiple agent sessions still alive
  (verified 05:11); host NOT rebooted (uptime since Sep 27). Forensic
  quarantines left in /tmp: gc_scratch_quarantine, compiled_quarantine_*
  (all restorable). Spiral I driver v3 is written and gate-ready;
  measurement BLOCKED on host repair. REPAIR PROTOCOL: (1) reboot or
  have the shim session revert; (2) health probe = /tmp/health_g.jl
  (new kernel must print PASS) + e_clone.jl (renamed clone must print
  RENAMED CLONE OK); (3) rerun Spiral I ×2 → crossovers →
  TETCAGE_GOAL2_FINAL.md. No pins affected: all D/E/F/G/H artifacts
  re-verified byte-identical during this investigation.
  ADDENDUM 06:50 Oct 1: reboot STILL did not land (uptime -s unchanged
  2026-09-27 21:47; probes re-failed with identical signature at 06:46).
  Full read-only elimination sweep completed — ALL CLEAN: concurrent
  julia/cyan agent processes now DEAD (ps); no /etc/ld.so.preload; no
  ccache/shim/JULIA env vars anywhere (shell, /etc/environment,
  /etc/profile.d, systemd user env); no ~/.julia/config/startup.jl;
  default env ~/.julia/environments/v1.12 benign (JSON3/JuliaFormatter/
  Revise/StaticArrays, mtimes Sep 28 pre-window) and NOT on the failing
  resolution path anyway (Base.identify_package_env → none for all of
  LOAD_PATH under --project=gpu); julia binary untouched since Sep 11;
  ldconfig/ld.so.conf.d stock; lsmod stock (no unknown modules).
  Remaining unexcluded mechanism: eBPF/kernel-level shim from the
  cyan-shim-kernel session (bpftool not inspectable without sudo).
  CONCLUSION: only a genuine reboot (or shim-session revert, and that
  session is already dead) can clear it; nothing else on this host can
  cause the observed overlay-method-table bypass. Do NOT retry repair
  without first confirming `uptime -s` shows a NEW boot date.
  ADDENDUM 07:3x Oct 1 (CPU-side work during blockage): driver DRY-REVIEW
  found v3 defects — F-I.3 twist residue in rot_direct!/fused_batched_shared!
  (would have failed HI-G1 immediately), threadIdx.x missing parens,
  strided-view pinned uploads, x-only B check, per-instance copy loops
  measuring our per-call CPU overhead at N=128. ALL FIXED in v4
  (gpu/spiral_i_comparators.jl): rigid-rotation body everywhere, host-
  precomputed f32 (c,s) shared by all paths and the host reference
  (A_param ≡ A_auth bitwise by construction), ONE batched pinned memcpy
  per component per frame (incumbent best case), last-instance spot-checks.
  Verified live: CUDA.pin EXISTS (parentmodule CUDACore — do not "fix"),
  graph API forms match H, tet pins match F, reconstruct! arithmetic
  verbatim-E, parse clean. POST-REBOOT PROTOCOL DELTA: run v4 (not v3);
  registered risk — dev_us (CUDA.@elapsed) on graph launches has no
  campaign precedent; if unsupported, drop dev cols for graphed paths
  (wall unaffected), NO silent substitution. /tmp/ectx forensics context
  self-cleaned (tmp) — no longer available; health probes /tmp/health_g.jl
  confirmed still present 06:46.

- **F-I.5 (CLOSED 2026-10-02, ×2 verified):** post-reboot P-1 health
  PASS (fresh kernels compile via the CUDACore overlay table and run correctly;
  probe reconstructed as /tmp/p1_health.jl after the original /tmp probes were
  wiped; CUDA runtime 13.4.0, RTX 5060) — F-I.4 CLOSED by the 2026-10-02 04:49
  reboot. First Spiral I v4 run then crashed in-kernel: `BoundsError` on thread
  (161,1,1) in block (30,1,1) during kernel execution, first class (blob),
  before any CSV rows were written (/tmp/spiral_i_run1.log). v4 was
  dry-reviewed only — this is the registered "residual bug on first hardware
  run" outcome, not an F-I.4 regression (compilation is healthy; the fault is
  driver indexing, most plausibly the v4 strided-view upload path in one of
  the incumbent kernels at blob's sizes). NEXT: rerun with `-g2` for the device
  stacktrace, inspect the five path kernels' bounds vs (V,T,N) at blob, fix,
  dry-review, rerun ×2 per protocol, then fill crossovers into
  TETCAGE_GOAL2_FINAL.md. No pins affected.
  F-I.5 NARROWED (-g2 rerun 2026-10-02): faulting kernel identified — `lbs!`
  in the C_lbs path, host line 450 (`C!()`), blob class. Host bindings:
  `dpal_c/dpal_s = similar(dvx, 3*N)`, `dbidxN/dbwN` per-vertex bone
  bindings (single-mesh sized), launch `blocks=cld(ntot,thr)` over
  ntot=nV*N vertices. Suspect: palette index scaling in `lbs!` (instance
  offset vs 3-per-instance layout) or dbidxN reuse across instances.
  Full -g2 log preserved at /tmp/spiral_i_g2.log (wipe risk — archive with
  the F-I.5 record). NEXT: read lbs! kernel, fix index math, dry-review,
  rerun ×2, fill crossovers.
  F-I.5 ROOT-CAUSED + FIXED (2026-10-02, v5): both crash logs decode to the
  SAME fault — run1 thread (161,1,1)/block (30,1,1) → global i=7585, -g2 run
  block (14,1,1) → i=3489; blob nV=10242 so both are in instance g=0 at the
  FIRST C launch (blob N=1) — different blocks across runs because EVERY
  g=0 thread faults and the recorded one is a scheduling race. Mechanism:
  `lbs!` computed the palette offset as `(g-1)*3 + bone` while its instance
  index `g = (i-1) ÷ nV` is 0-BASED → at g=0 every thread reads palette
  index bone−3 ≤ 0 (device BoundsError); at g≥1 it silently reads the
  PREVIOUS instance's palette (latent wrong-result, unreachable only
  because g=0 faults first). Fix: `g*3 + bone` (pal layout = repeat of the
  3-float palette, block [3g+1..3g+3]). Post-mortem dry-review of the
  never-executed N>1 territory caught TWO MORE first-hardware defects,
  both fixed in v5 before any rerun: (2) B_direct bound single-mesh
  dvx/dvy/dvz into rot_direct! which indexes vx[i] over ntot=N·nV → device
  OOB for every i>nV at N>1 (masked at N=1); fixed to repeated resident
  arrays per the kernel's REGISTERED contract (header: "N·V resident
  vertices", GOAL2 §7 "0 B/frame, resident geometry" — independently
  deforming objects own their positions). (3) ident_A/ident_auth
  x-comprehensions indexed R32[j] UNwrapped over 1:ntot (y/z were wrapped)
  → HOST BoundsError at blob N=8, before B even launches. All three are
  index/binding convention bugs; no arithmetic or gate changed. Crash logs
  ARCHIVED at TetLab/logs/spiral_i_run1.log and spiral_i_g2.log (the /tmp
  originals remain at wipe risk). v5 parse-checked; boundary review g=0,
  g=N−1, all 5 paths × N∈{1,8,32,128} in-bounds. NEXT: rerun ×2, HI-G4
  stable-columns ×2, fill crossovers into TETCAGE_GOAL2_FINAL.md.
  F-I.5 CLOSED (2026-10-02, v5 rerun ×2): both runs ALL GATES PASS (HI-G0
  provenance, HI-G1 identity all five paths incl. C_lbs at every N,
  HI-G2/G3 honest accounting, HI-G4 stable columns byte-identical ×2 —
  140 rows, stable-columns sha256 `4a9144326ebed3cb…`; canonical CSV
  `results/spiral_i_comparators.csv` full-file sha16 `78c9274aee143184`,
  run b; run logs archived at TetLab/logs/spiral_i_v5_run_{a,b}.log).
  dev_us on graph launches WORKED first try (registered risk retired).
  Crossovers filled into TETCAGE_GOAL2_FINAL.md (ALL CLOSED). One measured
  caveat recorded there: wall ordering at (blob, N=128) is machine-state-
  sensitive (B_direct's loss swung 14×→1.2×, A_param-vs-C_lbs order flipped
  between the runs) — stable columns pin the identity; capacity claims at
  that corner carry the band. HI-P1 amended per protocol (falsified in the
  strong form at that corner in both runs; direction stable). Lesson
  (recurring, F-C.3/F-E.2 class): a 0-based index consumed by a 1-based
  layout formula passes every read of the code and fails only on hardware's
  first instance — dry-review must SIMULATE g=0, not just g≥1.

## Spiral P0 defects — Vulkan compute parity port (all caught in-lane)

P0 is the first execution of the fused deform+reconstruct shape on Vulkan.jl
(the product path). Unlike CUDA, the port had to learn the API surface as well
as the numerics: nine defects were caught in PRE-RUN dry review (v2), and the
remaining ones below surfaced on hardware. All are API/state/lifecycle and
sizing classes; **no arithmetic defect was found, and P0-G2/G3 hold at ulp on
every cell of every class** — the arithmetic transcribed from the E authority
was correct on the first execution.

- **F-P0.1 (fixed, pre-run v2):** nine dry-review defects registered in the
  driver header — 1-based `enumerate` descriptor bindings (Vulkan bindings are
  0-based, shifting the whole set by one slot); readback staging missing
  `BUFFER_USAGE_TRANSFER_SRC` (it is the NaN-fill SOURCE too); `GC.@preserve`
  with no symbol list protecting nothing while buffers/CBs are loop-LOCAL and
  can finalize mid-flight; negative control referencing out-of-loop variables;
  per-iteration CBs from a non-FREE pool growing forever; memory-type selection
  filtered by a PROPERTY bit instead of the buffer's own requirement bits;
  `continue` inside a short-circuit expression; a ~160 MB aggregate error
  vector across the ladder; and allocation sized from the raw byte count rather
  than `get_buffer_memory_requirements().size`.
- **F-P0.2 (fixed):** pinned-rev API mismatches against Vulkan.jl 0.7.0, all
  found by reading the package's own `src/precompile_workload.jl` and compute
  tutorial rather than from memory — `allocate_command_buffers` returns ONE
  handle per call (not a vector); the copy entry point is `cmd_copy_buffer`
  (not `cmd_copy_buffer_to_buffer`); `BufferMemoryBarrier` takes no stage-mask
  keywords and `cmd_pipeline_barrier` takes buffer barriers in slot 2;
  `find_memory_type(mp, type_bits, want)`. Plus four paren-count errors in
  nested `Ptr{UInt8}(unwrap(map_memory(...)))` and GLSL block-member access
  (`outp.d[...]`, not `outp[...]`). GC-owned handles are never destroyed
  manually (F-G.6 lifecycle).
- **F-P0.3 (fixed):** **cage buffers sized `4nT` bytes instead of
  `4·length(fx)`.** The cage holds 4·nT FLOATS per component, so the correct
  byte count is 4x larger — and **Vulkan does not bounds-check storage
  buffers**, so the 4x-short buffer did not fault: the shader read garbage
  past the end. Caught by P0-G2 as obj ≈ 1.08 while the untouched weights still
  matched at 1 ulp (the asymmetry is what fingered it: a bad weight/index
  binding would have moved both). Same class for the packed buffers, sized
  `16·nT` instead of `16·length(fx)`, and allocated inside the `GC.@preserve`
  block whose symbol list must see existing bindings (`UndefVarError: bpc`).
  Lesson (new, worth keeping): **an out-of-bounds storage-buffer read on
  Vulkan is silent** — unlike CUDA it produces no BoundsError, so the ONLY
  detector is a numeric gate. Every port needs a gate tight enough to notice.
- **F-P0.4 (fixed):** **the negative control corrupted shared product state.**
  P0-G6b shifts the first quarter of cage_y by 0.25·scale to prove the gates
  can fail; it wrote that cage into the SHARED `bcy` and relied on a later
  re-upload to heal it. No such re-upload exists in the family loop (the
  restore at the class level had already run), so the SUBSEQUENT wind family
  read the shifted cage and P0-G2 fired on `blob wind N=1` with obj 0.523 —
  a gate failure manufactured by the probe, reporting a corruption it had not
  detected. Diagnosis: device y − host y = 0.5204 = **exactly 0.25·scale**, and
  vertex 1's tet corners (1067–1070) lie inside the corrupted quarter; a
  host-only probe of all 10 field variants confirmed the host path was correct
  at y = 0.8645. Fix: the negative control gets its OWN cage_y buffer, so it
  cannot perturb product state at all. **Lesson: a negative control that
  mutates shared state is itself a state-corruption bug waiting to be
  misread as a gate result — controls must be as isolated as the thing they
  control.**
- **F-P0.5 (gate amendment, registered before the accepted run):** planar
  readback is three CONTIGUOUS BLOCKS (x[i], y[ntot+i], z[2·ntot+i]), not
  interleaved — the NaN sentinel caught 3414 then 10242 NaNs before the block
  seams were understood. Recording it here because the sentinel is what found
  it: a layout assumption that is wrong in a way that leaves *some* slots
  written is invisible to a mean-error metric and glaring to a coverage
  metric.
- **F-P0.6 (GATE defect, amended P0-G4d):** the wind CUDA comparator was
  MIS-APPLIED across the ladder. `spiral_h_fusion.jl` (which produced the wind
  pins) loops **families only, with no N-ladder** — every value in
  `CUDA_WIND_N1` is an N=1 measurement — yet the driver applied it to all four
  ladder points, so at N=128 it compared **128x the work against a 1x pin** and
  reported a "20.4x regression" that is arithmetically guaranteed the moment
  the ladder extends past the pin's N. The kernel was fine: at blob/N=128 the
  two families cost the same (rot attributed 199–215 us, wind 203 us) and at
  N=1 rot 1.31 us vs wind 1.17 us — both move identical traffic and differ
  only in the per-corner field body. Amendment: **the 2x CUDA-pin gate is
  asserted only where a same-(class,N) comparator exists** (rot at all four N;
  wind at N=1). Elsewhere the parity claim is not "failed", it is UNAVAILABLE:
  rows record `pin_applicable=0`, and a strictly weaker, explicitly labeled
  substitute applies — wind within 2x of rot **at the same cell**. No pin is
  invented, extrapolated, or rescaled; a true wind N-ladder on CUDA is
  registered as deferred P2 work. **Lesson: a comparator pin carries its
  (config, N) domain, and a gate that widens a pin's domain silently converts
  a measurement into a fiction. Pins must be matched on every axis, and the
  mismatch declared rather than divided through.**
- **F-P0.7 (GATE defect, amended P0-G4e):** the F-P0.6 substitute's
  DENOMINATOR can be unresolvable. On the small classes the whole cell lives
  inside the ~14 us submit floor (teapot/rot attributed clamps to 0.0 at N=32
  and N=128: sustained 12.6–13.5 us against a 14 us floor), so there is no
  measurable device work to form a ratio over and wind/rot divides by zero
  ("Infx"). This is a limit of the instrument, not a kernel finding: at those
  cells BOTH families are floor-bound. Disposition: where the rot reference
  does not stand above that cell's own measured noise band (the spread of its 5
  sustained trials — the harness's resolution limit), the substitute asserts
  ABSOLUTE agreement within the band instead of a ratio. Not a weakening: it
  still fails if wind is genuinely slower, and it is the only form the
  measurement supports when the denominator is zero. **Lesson: any ratio
  metric must declare what happens when its denominator falls below the
  instrument's resolution — silence is the worst option, since a ratio gate
  then reports `Inf` as if it were a finding.**
- **F-P0.8 (MEASUREMENT finding, gates held):** the DRAM-write-bound corner
  (`blob/N=128`, 12·ntot = 15.7 MB) has a **5.4x CROSS-RUN band on the Vulkan
  side**, on the `soa+packed` layout only: 211.72 us (run A), 39.10 us (next
  run), 214.56 us (run A final). The other two layouts at the same cell were
  stable at ~208-214 us in every run, and the within-run spread is tight
  (sust_min..sust_max = 193.79..216.42 us in the final run), so this is NOT
  measurement jitter — it is process-level state, stable within a run and
  differing between them. Mechanism NOT established; the measured facts are
  recorded and the candidates (device-memory placement of the freshly
  allocated per-iteration output buffers, or GPU clock/power state) are named
  as candidates only. Registered as deferred investigation for P2, NOT
  explained here. Why it matters: this is the same corner that already carries
  a 2.04x run-to-run band on the CUDA side (55.08/112.21 us, registered in
  Spiral I §8), which is why P0-G4c invoked its explain-why branch there in
  the first place. The two bands are independent and compound.
  Consequence for the gates: because a single run cannot resolve this cell,
  the run-to-run-stable evidence is the PARITY column (P0-G2/G3 at ulp, error
  vectors byte-identical), not the wall. **Lesson: a wall measured once per
  process is a sample of one machine state; a wall measured 50x within a
  process characterises that state only. "Stable across trials" and "stable
  across runs" are different claims, and only the second one is the one gates
  usually mean.**
- **F-P0.9 (gate calibration, P0-G4g/g2):** the per-submit floor probe had no
  warmup and a 5-trial min−max band. Measured cold, its band was **656 us
  against a ~14 us steady-state floor** — and because the band is the
  resolution limit for the substitute comparator (F-P0.7), that artifact
  silently marked **all 21 substitute cells UNRESOLVABLE, leaving the gate
  covering ZERO cells while printing ALL PASS**. Fixes: warm the probe like
  every timed cell, take REPS=25 trials, and use a central spread (p90 − p10)
  rather than min−max, which is the wrong statistic for a heavy-tailed host
  jitter. Sanity assert added (`floor_band < 0.5 · floor`, it caught the
  intermediate 8.55 us band) plus a coverage assert (P0-G4h: a DRAM-bound
  substitute cell must be resolvable, so the gate can never again quietly skip
  the cells that matter). Final calibration: floor 13.88 us, band 2.71 us,
  **5 cells ratio-gated, 16 declared unresolvable** — recorded per row.
  **Lesson: any threshold derived from a measurement can disable the check
  that consumes it. A calibration must assert its own sanity, and a gate must
  report what it actually covered — "ALL PASS" over an empty gated set is
  strictly worse than no gate at all, because it looks like coverage.**

### P0 CLOSED (2026-10-02) — Vulkan compute parity port

Both accepted runs ALL GATES PASS. Canonical CSV
`results/spiral_p0_vulkan_compute.csv` sha16 `84915b3c1c3932df` (run b);
**P0-G5 stable parity block = 168 rows, sha256 `5160e8a3e21e018c…`, byte-identical
across two fresh processes.** Logs + run-A CSV archived at
`TetLab/logs/spiral_p0_run_{a,b}.{log,csv}`.

- **Coverage:** 7 classes x 2 families x N in {1,8,32,128} x 3 layouts = 168
  parity rows, every one of them NaN-sentinel-verified (zero unwritten slots).
- **P0-G2 (object, ≤ 4·eps(f32)·maxcoord per class):** worst 9.703e-07 at
  conifer, 2.389e-07 at blob, 1.408e-07 at robot; every class at or under its
  own bound. **P0-G3 (screen, ≤ 0.5 px):** worst 1.09e-04 px — four orders of
  magnitude inside the gate.
- **The headline:** the arithmetic ported from the E authority (corner pairing,
  deform/reconstruct order, host-f32 wind field) was **correct on the first
  hardware execution, at ulp, on all 168 cells.** Every P0 defect was API,
  sizing, lifecycle, or gate-calibration. None was numeric.
- **P0-G4 (wall ≤ 2× pinned CUDA, attributed):** worst 1.58x (teapot), 1.27x
  (grass), 1.24x (plate), 1.16x (icosphere2), 0.77x (conifer), ~0.001x
  (robot, floor-bound). The single cell over 2× is blob/rot/N=128 at 3.72× —
  the registered DRAM-write-bound corner carrying its own 2.04x CUDA band.
- **P0-G6:** NaN sentinel zero coverage failures across all 168 cells; negative
  control fires (cage_y quarter shifted 0.52 → obj 1.080 against bound
  5.700e-07), and now with its own cage buffer so it cannot perturb the
  product path it is checking (F-P0.4).
- **Mandate honoured (§1):** the 2.9x headroom claim was re-measured, never
  assumed. The shipped planar+packed form sits at or under the CUDA pins
  wherever a same-cell pin exists; the claim survives, and the corner band is
  carried into P2 rather than rounded away.
- **Deferred to P2, explicitly NOT closed here:** (a) a real wind N-ladder on
  CUDA, so the wind family gets a same-(class,N) pin instead of the weaker
  same-cell substitute (4-5 of 21 cells gated, 16 floor-bound and honestly
  unrankable by this harness); (b) root cause of the F-P0.8 cross-run band at
  the DRAM-write-bound corner. Naming a candidate mechanism without measuring
  it is how the ledger gets fictional, so both are registered as open work
  rather than written up as findings.

### Spiral H2 CLOSED (2026-10-02) — CUDA wind N-ladder (benchmark completeness)

Driver `gpu/spiral_h2_wind_ladder.jl`. Both runs ALL GATES PASS. CSV
`results/spiral_h2_wind_ladder.csv` sha16 `5ca1e3d47b3039a7`; **HF2-G2 stable
columns byte-identical x2** (28 rows, sha256 `088909edf19ff5f4…`); logs at
`TetLab/logs/spiral_h2_run_{a,b}.log`.

- **HF2-G1 identity at every (class, N)**: worst 9.537e-07 (conifer) against
  the 4·eps(f32)·maxcoord bound, with the first AND last instance of each
  class checked. **No new correctness defect** — the batched wind kernel
  computes the ported function on first execution, so P1 proceeded.
- **Scope held to benchmark completeness.** Nothing was optimized: the wind
  body is VERBATIM from H's `fused_deform_reconstruct!` (itself verbatim from
  E), the reconstruct pairing is H's `reconstruct!` verbatim, the batching is
  Spiral I's A_param contract (shared resident cage + weights, one dispatch
  over ntot = N·nV), and the timing is Spiral I's protocol verbatim.
- **One deliberate methodological choice, and why it matters:** H2 uses
  **Spiral I's timing protocol, not H's**. The rot pins come from Spiral I, so
  the wind pins must be produced by the same method or the two families are
  not commensurable. Mixing two protocols inside one comparator table is the
  same error class as F-P0.6 (mixing two N-domains) — just in the other
  direction. The old H N=1 pins are RETAINED for provenance and NOT used by
  the gate; H2's blob N=1 (11.18 us) and H's (9.93 us) agree to within the
  protocol difference, which is the expected relationship between two honest
  measurements of the same work.
- **Debt registered (pre-existing, F-D.3 class):** `bary`,
  `weights_and_indices`, `flatten_corners` and the f32 wind host path are now
  duplicated across a FOURTH file. Single-sourcing them into TetDeform stays
  on the books; this driver did not make it worse than the existing pattern
  but it did extend it.

### P0-G4i — pin upgrade, and the substitute RETIRED

Spiral H2 closed the gap P0-G4d opened, so the weaker substitute was **removed,
not left dormant** (`rotaat`, the floor-band resolution limit, the coverage
assert, and the substitute's three CSV columns). Every P0 row now carries
`pin_applicable=1` and a real same-(class,N) 2x claim. Re-run x2, ALL GATES
PASS: canonical CSV sha16 `ce4be8ca13563bb4`; **parity block sha256
`5160e8a3e21e018c…` — UNCHANGED from the pre-upgrade pin**, which is the
correct outcome and a useful cross-check: the comparator table is wall-side
only, so the numeric evidence must be invariant to it, and it is.

With real pins on both families, the wind family stops being the weak leg:
worst ratio anywhere except the registered corner is **1.65x** (teapot), and
`blob/wind/N=128` — the cell that previously had no valid comparator at all and
drove the whole P0-G4d/f/g/h amendment chain — now carries a genuine
same-(class,N) claim at **1.47x** against H2's 135.79 us pin.

**The through-line worth keeping: every amendment in the G4d→G4h chain
existed to work around a MISSING MEASUREMENT.** Each one was individually
defensible and each one added machinery, calibration, resolution limits and
asserts. The actual fix was ~150 lines of benchmark that measured the thing
that had never been measured. A workaround that keeps growing a gate is
telling you to go measure, not to keep tuning.

- **F-P0.10 (run condition, not a defect):** one P0 run aborted on the G4g
  floor-band assert (18.46 us band vs 16.16 us floor). Cause was REAL and
  external: `nvidia-smi` showed an unrelated third-party julia test process
  resident on the GPU. The assert was correct to be suspicious and wrong to
  be fatal, because its consumer had been removed at G4i — so it was retired
  under G4g3 and the band is now reported per run instead. The distinction
  preserved deliberately: **retiring an assert because its consumer is gone is
  hygiene; relaxing an assert because it fired is gate-fudging.** G4g fired
  twice for two different reasons (cold probe, then genuine contention) and
  neither was a defect in the gate it guarded. Run A and B were taken after the
  competing workload exited; their bands (6.28 us / see CSV) are the honest
  uncertainty on every attributed value.

### CARRIED INTO P2 (explicit, not closed)

- **F-P0.8 corner band — P2 OWNS THE ROOT CAUSE.** `blob/N=128` (12·ntot =
  15.7 MB, the DRAM-write-bound corner) measures, on the `soa+packed` layout
  only: **211.72 / 39.10 / 214.56 us across three runs of the same binary**,
  while the other two layouts at the same cell held ~208–214 us in every run
  and the within-run trial spread stayed tight (193.79–216.42 us). Stable
  within a process, 5.4x apart between processes. Mechanism NOT established;
  candidates (device-memory placement of the per-iteration output buffers;
  GPU clock/power state) are named as candidates ONLY. **P2's first act:
  reproduce it deliberately — repeated fresh processes at that one cell with
  buffer placement logged — because P2 is where in-engine capacity claims are
  published and that claim inherits this band.** Until then the capacity
  number at that working set carries the band, and the determinism evidence
  for P0 rests on the parity columns (byte-identical x2), which are unaffected.

### P1 CLOSED (2026-10-02) — scene packet v7 deformation behind a flag

Gate (seam design §3 P1), verbatim: "existing suites byte-identical with flag
off; flagged run presents deformed frames with receipts."

**Contract (Rust, `native_graphics_contract::deformation`)**
- `SCENE_PACKET_SCHEMA_V7`, `GraphicsScenePacketBody::deformation:
  Option<DeformationIntent>`, `GraphicsTelemetry::deformation:
  Option<DeformationTelemetry>`, flag `WGE_TETCAGE_DEFORM_V7`.
- **v6 and v7 are mutually exclusive in BOTH directions** — v6 carrying a
  deformation is rejected, v7 missing one is rejected. Rationale recorded in
  the module header: a receiver that ignores the section renders a static
  mesh and one that honours it does not, so those are different artifacts and
  must not share a version string. A receiver must never have to guess.
- **9 typed rejections**, each with a stable code and an `is_static_fallback`
  predicate so a supervisor can branch between "fix the scene" and "fall back"
  without matching prose.
- Julia boundary kept in lockstep (`WGEGraphics.jl`): same schema constants,
  same mutual exclusion, `deformation` added as an OPTIONAL key so v6 packets
  parse exactly as before.

**Gate evidence**
- **Byte-identity, flag off — proved against a COMMITTED artifact, not a
  round-trip.** `committed_v6_packet_reseals_to_its_committed_digest`
  re-seals `artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/
  run-a/graphics_scene_packet.json` under the new code and reproduces its
  committed `packet_sha256` EXACTLY. A self-consistent round-trip assertion
  would have passed even with the field leaking into canonical JSON; this one
  cannot. Byte-identity is therefore a property of the TYPE
  (`Option` + `skip_serializing_if`), not of a test that happens to pass.
- **Existing suites green:** `cargo test -p wge-native-graphics-contract`
  68 passed / 0 failed BEFORE, 77 passed / 0 failed AFTER (the +9 being the
  new P1 suite). Julia `lava_adapter` protocol suites pass unchanged. Logs
  archived at `TetLab/logs/spiral_p1_{baseline,after}_cargo.log`.
- **Flagged run:** v7 conifer-wind packet validates, seals, round-trips
  through serde, and carries receipts that agree with it (instances, cage
  corners). `every_typed_rejection_is_reachable` drives each rejection through
  the REAL `validate_scene_packet` entry point.

**Two things I got wrong and fixed, both worth the record**
- The validator initially conflated `BufferReference::count` (f32 ELEMENTS)
  with corner count. Same silent class as F-P0.3: a length check that is wrong
  in a way only the wrong-type fixture can see. The relation is now spelled
  out in elements (12 f32 per tetrahedron) with a named `tet_count()`/
  `cage_corner_count()` accessor instead of an implicit division.
- My first reachability fixture asserted that a cage with a DIFFERENT tet
  count must be rejected. It must NOT be: the cage IS the tet decomposition,
  so its size is not derivable from the mesh, and cross-checking it here would
  create a second, untested authority over cage quality (TetDeform's job). The
  test now asserts the POSITIVE case — a different well-formed tet count is
  valid — alongside the ragged one that is not. A test that demanded a
  rejection for a legitimate input was wrong about the contract, not the code.

- **F-P1.1 (PRE-EXISTING, repaired here, not introduced by P1):
  `wge-certification-authority` did not compile at HEAD.**
  `GraphicsTelemetry` gained `texture_residency` in the earlier mip-residency
  work and two authority tests were never updated, so `cargo test` on that
  crate failed to BUILD. Confirmed pre-existing before touching it: `git show
  HEAD:...` shows the field present in lib.rs and absent from both test files,
  and neither file was modified by P1. Repaired (one field each). This is a
  process finding, not just a compile fix: **the mip-residency slice closed
  without re-running the workspace, so a crate outside its own lane was left
  broken and the breakage was invisible until a later slice happened to touch
  the same struct.** A slice that changes a shared contract type must run the
  WORKSPACE gate, not only its own crate's.
