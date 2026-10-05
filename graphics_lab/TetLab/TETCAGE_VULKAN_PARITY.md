# TETCAGE — Spiral G: Vulkan Render-Pass Readback Parity (RTX 5060)

Goal 2, Spiral E §7 deferral CLOSED. The pre-registered falsification
condition — *"if the Lava/Vulkan render path disagrees with this harness's
screen-space model by ≥ 0.5 px on any D class at the D operating points,
the GPU D-contract reopens at the render-pass level and a Vulkan readback
harness must be built (render-to-texture → GPU readback → same px95
metric)"* — was EXECUTED: the harness was built, and the render path
**passes with ~6,000× margin**. Evidence:
`results/spiral_g_vulkan_parity.csv`, full-file sha16 **`6b5164abcf847bd1`**,
byte-identical across two fresh processes (G-G4). Driver:
`vk/spiral_g_render.jl` (+ `vk/probe_g.jl` bisect probe, retained as
evidence for F-G.6). Env: `vk/Project.toml` — Vulkan.jl at the SAME
repo-pinned rev as graphics_lab (`03b4ca23…`), VulkanCore `1d02829e…`.

## 1. What was proven

The REAL Vulkan transform chain — f32 vertex shader projection, fixed-
function perspective divide (clip.w), NDC→framebuffer mapping, rasterizer —
reproduces the analytic pinhole model of `TetDeform.screen_px_error`
(camera at (0,0,d) looking down −z, screen = f·(x,y)/max(d−z, 0.05),
f=500 px) to:

**campaign max 8.4e-5 px** (conifer/fold4), typical max ≈ 4–5e-5 px,
mean ≈ 2e-5 px. Physical reading: f32 ulp at the framebuffer coordinate
scale (512 px) is 6.1e-5 px — **the divergence is ~1–1.4 ulps of the
coordinate itself**. There is no render-pass-level systematic error on
this driver/ICD (NVIDIA 615.71.9, Vulkan 1.4.351, conformance 1.4.3).

Method: one GLSL point per mesh vertex (POINT_LIST, gl_PointSize = 1).
The vertex shader computes its own framebuffer coords from its own
gl_Position and transports them as FLAT varyings to an R32G32_SFLOAT
attachment; readback yields the ACTUAL f32 output of the full chain per
vertex, compared against the f64 analytic coords of the same vertex.
No rasterization-grid quantization enters the measurement (F-G.3).

## 2. Gates (pre-registered; amendments marked and registered pre-acceptance)

- **G-G0 toolchain PASS**: PHYSICAL_DEVICE_TYPE_DISCRETE_GPU (RTX 5060);
  all classes reproduce pinned tet counts (cube/skeleton pinned by
  measurement at their h — F-G.8).
- **G-G1 provenance PASS**: render inputs are the TetDeform f64 cage-path
  positions under each family's GT field — divergence is attributable to
  the render chain alone.
- **G-G2 analytic parity PASS (THE gate)**: max ≤ 0.5 px on every
  class × family. Measured worst 8.4e-5 px. 9 classes × 2 families = 18
  rows; cube + skeleton added as strengthening classes (G-G2 set
  amendment, F-G.7). Conifer rows carry `fb-clip(n)` annotations: 821
  (wind) / 2107 (fold4) of its vertices are analytically OUTSIDE the
  registered D camera (GT geometry exceeding the camera; the 0.5 px
  threshold governs the transform chain, not camera coverage).
- **G-G3 record**: mean/p95 per row in the CSV.
- **G-G4 determinism PASS**: two fresh processes → full CSV byte-identical
  (no timing columns; raster deterministic on this platform).

## 3. Method amendments (all registered before the first accepted run)

1. **Chunked value parity** (superseding naive cell-fetch): sub-px f32
   drift can cross cell boundaries (quantizing a cell-fetch by up to
   ~1 px) and same-cell vertices collide (last splat wins). v2 draws
   ROUNDS of pairwise-distinct predicted cells (round r = r-th vertex of
   each cell; distinct cells ⇒ unambiguous attribution), accepts a
   payload only if it value-matches the vertex's analytic coords within
   0.01 px, and re-renders everything else SOLO (one point in the whole
   framebuffer — attribution exact by construction). Solo rates measured
   0–8% (boundary-crossing statistics); solo errors satisfy the same gate.
2. **Rounds batching**: first-fit chunk restart was O(n) tiny chunks on
   dense classes (blob: ~6k submits, timeout); rounds batch the same
   guarantee (blob: 8 rounds).
3. **Orientation**: y-up transported without flip — a presentation
   convention outside the parity model, recorded in the CSV header.

## 4. Defects registered (F-G.1–F-G.9, details in the failure register)

Convention-depot mix-up (F-G.1) · projection double-divide + missing
framebuffer offset, caught by the NaN sentinel (F-G.2) · cell-quantization
and collision method flaw (F-G.3) · O(n) chunk restart timeout (F-G.4) ·
vertex stride 24-vs-12 → device fault + heap corruption (F-G.5) · manual
destroy + finalizer double-free — single-ownership rule for Vulkan.jl
handles (F-G.6) · G-G2 class-set amendment + pin provenance correction
(F-G.7, F-G.8) · solo-rate assert functional-form misfire on cube's
exact-edge corners (F-G.9).

## 5. Scope boundaries

- Proven: transform chain + rasterizer parity vs the analytic model at
  the D operating points, point-splat transport of flat varyings.
- NOT proven by this harness: triangle rasterization interpolation
  (perspective-correct varying interpolation is used by the real
  pipeline; its error contribution is orthogonal to position parity and
  is the subject of a future shading-parity probe if needed), depth
  buffer precision, blending, MSAA.
- The harness exercises Vulkan.jl at the repo-pinned rev on the NVIDIA
  ICD; other ICDs/drivers must re-run before making the same claim.

## 6. Consequence

**Spiral E §7 deferral RESOLVED.** The GPU D-contract (E-G2) now holds
end-to-end: device arithmetic (E) AND the render pass (G) agree with the
CPU oracle's analytic model far below any perceptual threshold. F3
(hardware ghost shading) evidence stands on the same toolchain. Remaining
open item from the Goal-2 stack: the plate-class product decision (D3
floor — a contract choice, not research).
