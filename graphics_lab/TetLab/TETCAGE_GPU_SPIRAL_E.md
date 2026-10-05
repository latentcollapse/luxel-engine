# TETCAGE — Spiral E: GPU Runtime-Arithmetic Confirmation (RTX 5060, CUDA.jl)

Goal 2 §4. Authority: the CPU oracle (C8/TetDeform) remains THE mechanism
authority; this spiral confirms the **runtime arithmetic** on consumer GPU
hardware and closes **F3 (hardware ghost shading)**. Evidence CSV:
`results/spiral_e_gpu_confirm.csv`, sha16 **`c4b3ec1c9a0adfe3`**,
reproduced byte-identical in ≥2 fresh processes (E-G5). Driver:
`gpu/spiral_e_gpu.jl` (quarantine-local env `gpu/Project.toml`, CUDA dep).

## 1. Toolchain (E-G1, proved before this driver)

- GPU: NVIDIA GeForce RTX 5060 (GB206, compute capability **12.0** = sm_120)
- CUDA user-mode runtime **13.4.0**; driver 615.71.9; nvcc at /opt/cuda
- Julia CUDA.jl stack: CUDACore 6.4.1 w/ runtime 13.4 artifacts (TetLab-local
  `gpu/` project). The repo graphics env (graphics_lab/Project.toml) is
  Lava/Vulkan/Raycore and deliberately has NO CUDA — compute harness lives
  inside the TetLab quarantine.
- E-G1: f32 device kernel executes and returns exact results on this
  toolchain (identity kernel test, pre-driver).

## 2. Pre-registered gates (driver header, registered BEFORE first run)

- **E-G2 D-contract** (blob h=0.28, conifer h=0.6, plate h=0.3; wind A=0.2
  φ=0.7 and fold4 A=0.4):
  `|px95@5_GPU − px95@5_CPUf32| ≤ 0.02 px` (device vs host f32: only fma
  contraction + libm ulps may differ) AND
  `|px95@5_GPU − px95@5_f64-cage-path| ≤ 0.05·px95 + 0.05 px` (D band).
- **E-G3 TE-copy snapping**: cage corners grouped by 1e-9-quantized f64
  position (TE_T_QUANT semantics); every intra-group pair after device
  deform: screen deviation ≤ 0.01 px @ d=5 (≥100× under the 1 px floor).
- **E-G4 duplicate-fragment agreement**: intra-group object deviation
  ≤ 1e-5·scale AND derived normal tilt 2·δ/edge_min ≤ 0.01°.
  **F3 CLOSES iff E-G3 ∧ E-G4 hold on all cages.**
- **E-G5 determinism**: two fresh processes → byte-identical CSV.
- Harness self-check (per case): f64 evaluation of the driver's own
  (idx, w64, f64-corner) machinery must reproduce
  `TetDeform.cage_path_positions` to ≤ 1e-9·scale before any gate runs.

## 3. Results (pinned CSV c4b3ec1c9a0adfe3)

Self-check: **0.00e+00** on all three cases — the driver's λ/idx machinery
is bit-exact vs TetDeform (it delegates to `TetDeform.TetBasis.rest_basis`
+ `inv3` and mirrors `cage_path_positions`' λ lines verbatim; F-E.3).

| case | family | px95 f64 cage | px95 CPU f32 | px95 GPU | \|GPU−CPUf32\| | \|GPU−f64\| |
|---|---|---|---|---|---|---|
| blob h=0.28 | wind | 0.2184 | 0.2184 | 0.2184 | 0.0000 | 0.0000 |
| blob h=0.28 | fold4 | 0.2536 | 0.2536 | 0.2536 | 0.0000 | 0.0000 |
| conifer h=0.6 | wind | 0.5410 | 0.5410 | 0.5410 | 0.0000 | 0.0000 |
| conifer h=0.6 | fold4 | 2.0636 | 2.0635 | 2.0635 | 0.0000 | 0.0001 |
| plate h=0.3 | wind | 1.6722 | 1.6722 | 1.6722 | 0.0000 | 0.0000 |
| plate h=0.3 | fold4 | 0.3647 | 0.3647 | 0.3647 | 0.0000 | 0.0000 |

TE census (per family): blob 389 groups / 27,255 pairs; conifer 96 / 5,348;
plate 50 / 1,632 → **68,470 pairs total per family code**: bit-identical
fraction **1.0000**, max object deviation **0.000e+00**, max screen dev
**0.000e+00 px**, normal tilt **0.000e+00°**.

**Verdicts: E-G2 PASS · E-G3 PASS · E-G4 PASS · E-G5 PASS (2 fresh
processes, identical sha16). World-age/warning grep = 0 on all runs.**

### D1 cross-validation (independent authority check)

The CSV's f64-cage column reproduces D1's pinned px95@5 reference values
(blob wind 0.218 / fold4 0.254; conifer wind 0.541 / fold4 2.064; plate
wind 1.672 / fold4 0.365) at print precision across all six rows — an
independent second-code-path reproduction of the D authority rows, and
proof the in-file field constructors match D semantics (mk_wind/mk_fold;
F-D.3 class — see §6).

## 4. Why exact zeros are the EXPECTED outcome (mechanism note)

Runtime model = planned GPU path: per-vertex barycentric weights computed
ONLINE-host in f64, stored f32; cage corners stored f32 (4 slots/tet);
per frame the device deforms corners in f32 and reconstructs
`p' = w2·c2' + w3·c3' + w4·c4' + w1·c1'` in TetDeform's exact pairing
order. `deform_corners!` is elementwise and deterministic: two TE copies
whose f64 positions differ < 1e-9 round to the same f32 inputs
(1e-9 ≪ f32 ulp ≈ 1.2e-7 at unit scale), and identical f32 inputs through
identical code yield bit-identical f32 outputs → deviation exactly 0.
This is C7's ε-seam policy holding ON HARDWARE: quantization absorbs the
seam. Residual risk quantified: a copy pair straddling BOTH a 1e-9
quantum boundary and an f32 rounding boundary differs by 1 ulp →
deviation ≈ 1 ulp·(F_PX/D_CAM) ≈ 1.2e-5 px, three orders below the 0.01 px
gate — the gate is robust even in the straddle case.

## 5. F3 verdict

**F3 (hardware ghost shading) CLOSES.** E-G3 ∧ E-G4 hold on all cages and
both families: duplicated cage fragments are bit-identical after device
deform; derived normal tilt is exactly zero; no divergence-driven
fragment disagreement exists on this toolchain at the tested operating
points. Reopen condition: any E-G3/E-G4 violation on a future toolchain
or cage population reopens F3.

## 6. Defects registered during the spiral (see failure register F-E.*)

F-E.1 v1 draft superseded (placeholder kernel, 1-corner flatten) ·
F-E.2 `+1` idx misinterpretation (1-based ids shifted again in host
consumers → ~1.5 identity error) · F-E.3 transposed-inverse misdiagnosis
(wrong fix applied before root-cause read; resolved by delegating to
TetDeform.TetBasis) · F-E.4 `@cuda` requires a function call (broadcast
syntax rejected) · F-E.5 reconstruction buffers sized nC (corners) not
nV (vertices) → device BoundsError · F-E.6 TE census read reconstructed
vertex arrays at corner indices (wrong semantics + latent host OOB) ·
**F-E.7 E-G2 D-band scored against the IDENTITY cage path → vacuous gate;
the intermediate CSV sha16 `805d2abf45593c51` is INVALID and superseded
by `c4b3ec1c9a0adfe3`** (caught by cross-checking the CSV against D1 pins).
F-D.3 recurrence: `mk_wind_like` was defined in-file; the single-sourcing
debt was subsequently PAID (ledger turn E.1 — E consumes the TetDeform
export, re-pinned byte-identical `c4b3ec1c9a0adfe3` ×2).

## 7. Scope boundary: Lava/Vulkan render pass DEFERRED — **RESOLVED (Spiral G)**

> **RESOLUTION (ledger turn G):** the falsification condition below was
> executed — the readback harness was built (`vk/spiral_g_render.jl`) and
> the render path passed with ~6,000× margin (campaign max 8.4e-5 px vs
> the 0.5 px threshold; deterministic ×2). See TETCAGE_VULKAN_PARITY.md.
> The text below is retained as registered.

This spiral proves the arithmetic core (deform + reconstruct + TE
snapping) on device f32. It does NOT prove the Lava/Vulkan render pass
(its own f32 transform chain, w-division, viewport mapping, rasterizer
quantization). **Falsification condition for the deferral:** if the
Lava/Vulkan render path disagrees with this harness's screen-space model
by ≥ 0.5 px on any D class at the D operating points, the GPU D-contract
reopens at the render-pass level and a Vulkan readback harness must be
built (render-to-texture → GPU readback → same px95 metric). Until then,
Spiral E's authority is exactly: device arithmetic core confirmed;
render-pipeline parity unproven. **(Superseded by Spiral G: parity
proven at ~1 ulp of the framebuffer coordinate scale.)**

## 8. Ordering consequences

- ~~Spiral F (perf model) is unblocked AFTER the F-D.3 single-sourcing debt
  is paid~~ — PAID (ledger turn E.1: mk_* → TetDeform exports, all five
  drivers re-pinned byte-identical ×2, sha16s unchanged). Spiral F is
  unblocked now.
- E's D-band formulation (vs f64 cage path, not vs GT) should be reused
  by any future GPU gate — scoring vs GT conflates cage error with
  hardware error (the F-E.7 lesson generalized).
