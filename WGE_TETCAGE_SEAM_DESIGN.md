# WGE TetCageRT Seam Design — Native Graphics Packet Integration

Status: **design registered 2026-10-02; extends `TetLab/TETCAGERT_INTEGRATION_CONTRACT.md` (DESIGN ONLY) to the measured technology**
Authority chain: WGE Rust kernel owns identity/policy/evidence; Julia/Lava execute; TetLab remains a quarantined research lane until Phase 1 lands behind the capability flag.

## 0. What changed since the integration contract was written

The registered contract pre-built the authority skeleton (`TetCageCandidate`,
RepresentationPolicy → executor, fail-closed capability flag, negative controls).
Since then the campaign **measured** a different, narrower, immediately productizable
facet than the paper's ray-tracing representation:

- **Built and pinned (D–H spirals): parametric cage deformation on the raster path.**
  CPU-f32 oracle bit-identity (E-G2), render-chain parity ~1 ulp (G,
  `6b5164abcf847bd1`), graph-fused single-kernel frames 8.8–11.2 µs, ~200 instances
  per 2 ms slice (H, `2efbcbbcbba381ed`), A_param = 0 B/frame shared-cage economics.
- **Not built: the DXR/Vulkan instance-per-tet representation** (contract §"Mapping to
  GPU"). It stays a registered future facet; the engine has no ray tracing yet
  (audit DEFERRED).

So the seam designed here is for the **deform facet** — the thing that makes Demo A's
living world affordable — and it deliberately leaves the RT facet's memory thesis
(μMesh/μBLAS reuse) registered but dormant.

## 1. The decisive constraint: no CUDA in the shipped game

TetLab's pinned kernels are CUDA.jl (`TetLab/gpu/Project.toml`). WGE's product
envelope is Rust/Julia/Lava-Vulkan; a shipped arena demo that requires the CUDA
runtime violates the doctrine and the "it just works when the window comes up"
requirement. Therefore:

```text
CUDA (TetLab gpu/)   = pinned numeric oracle + economics laboratory   [research]
Vulkan compute (Lava) = the ONLY product execution path               [shipped]
```

Rejected alternative: CUDA↔Vulkan external-memory interop. It would keep the measured
kernels verbatim, but puts two driver stacks in the shipped runtime, adds an interop
fragility surface, and couples NVIDIA ICD behavior into the packet contract. The math
is small and fully specified (4 corner gathers + weighted sum per vertex; fused
single dispatch); the Spiral-G readback methodology already proves how to validate a
Vulkan port against the oracle to ~ulp precision. Port cost is one bounded P0 phase.

What transfers verbatim from the CUDA campaign: the fused kernel SHAPE (deform
corners in registers → reconstruct → one launch), command-buffer replay ≈ CUDA-graph
replay (Vulkan does this natively), the D-band quality bounds, and the pins as
*predictors*. The 2.9× headroom claim is re-measured on the port, never assumed.

## 2. Packet seam (the ask): scene packet v6 → v7, additive and optional

New optional per-object render-binding field, `deformation_policy`:

```text
static            (default; today's behavior, byte-identical when absent)
direct_field      (B_direct: closed-form per-vertex parametric motion — affine
                   families only; no cage; 0 B/frame; cheapest where Spiral-I P1 holds)
cage_parametric   (A_param: shared resident cage + per-vertex barycentric weights;
                   non-affine families (twist/fold); 0 B/frame at any instance count)
cage_authored     (A_auth: art-directed per-frame cage states; 48·T·N B/frame —
                   cinematic authored regime, budgeted explicitly)
```

`CagePayload` (content-addressed, resident, sha'd into packet identity):

```text
cage_topology        tet count, corners (rest), non-degeneracy proven at build time
vertex_binding       per vertex: tet index + 4 weights, normalized sum-to-1
family_descriptor    typed parametric field (wind | fold | twist | blob) + params + seed
legal_use_envelope   explicit class list from the measured verdict; unknown -> unsupported
provenance           producer/validator schema ids, oracle pin refs (E/G/H sha16s)
```

**Identity model.** Policy + payload are packet content (fail-closed validation:
normalized weights, in-range tet indices, finite params, savings-law check
avg_tris_per_tet ≥ s/0.309 from the registered contract). Time is a **frame-variant**
input, like camera: `t = sim_tick / REFERENCE_TICK_RATE_HZ` — deterministic replay,
and Tier-A captures pin the tick. GPU caches keyed on content sha stay warm because
static buffers never change; only the compute output is dynamic.

**Execution shape in Lava.** One fused compute pre-pass per frame, before scene
raster: reads resident (rest corners, cage, weights, family params), writes the
dynamic position buffer; the raster pass binds it instead of the static positions.
Then the normal scene passes run unchanged. Command buffer replay absorbs the
launch-floor problem the CUDA spiral measured.

**Fail-closed everywhere.** Capability flag
`graphics.geometry.tetcage.experimental/v1`, default OFF. Unsupported hardware,
validation failure, or device loss ⇒ **degrade to the static mesh with typed demotion
evidence** — never a driver crash, never a silent visual lie. Negative controls from
the registered contract carry over unchanged: forged candidate, wrong mesh digest,
degenerate tets, unnormalized weights, envelope violation, reuse-count lies.

## 3. Post-reboot sequence (each phase has an exit gate)

```text
P-1  Post-reboot health: GPU kernel compiles again (F-I.4 clears). Run Spiral I v4
     (budget one debug cycle) -> results/spiral_i_comparators.csv fills the
     crossover surface. GATE: negative envelope known per (class, N); policy
     defaults for direct_field vs cage_parametric are measured, not guessed.
P0   Vulkan compute parity port (spiral-G methodology): deform!/reconstruct! as one
     GLSL compute dispatch; oracle = TetDeform CPU path; gate max ≤ 0.5 px vs the
     analytic model, expected ~ulp; CSV evidence + sha pin. GATE: parity + wall
     within 2× of the CUDA pins on the same classes, or explain why.
P1   Packet v7 extension lands behind the flag: conifer-wind single instance on the
     certified worker path; telemetry (wall µs, instances, bytes/frame); typed
     rejections; static fallback proven. GATE: existing suites byte-identical with
     flag off; flagged run presents deformed frames with receipts.
P2   In-engine instancing ladder N ∈ {1,8,32,128} × classes; frame + memory
     telemetry vs the Spiral-I table. GATE: measured in-engine economics ≥ the
     registered predictions, or the gap is explained and the envelope updated.
P3   ARENA A/B: ARENA BASELINE vs ARENA+TetCageRT, identical content/gameplay;
     publish package/runtime memory, GPU memory, animated-geometry cost, frame
     time, density, entity counts, visual error. Then SPEND the savings
     (300 → 1,500 animated background entities, banners, crowds). This is also
     Grindstone §30's representation-policy laboratory.
```

## 4. Honesty ledger

- The negative envelope is part of the product claim: where B_direct wins (pure
  affine families on dense meshes), the arena uses `direct_field` and says so. TetCage
  wins through measured practical advantage, not architecture prestige.
- LBS/morph incumbents stay available as comparators for the A/B document.
- The RT facet (instance-per-tet, μBLAS reuse) remains registered-dormant; this seam
  does not claim it.
- F-I.4 (reboot-dependent compile failure) and the untested-on-hardware Spiral I v4
  driver are the two known risks entering P-1; both are registered in
  `TETCAGERT_FAILURE_REGISTER.md`.
