# WGE TetCageRT Seam Design — Native Graphics Packet Integration

Status: **design registered 2026-10-02; extends `tet-lab/tetcage-rt-integration-contract.md` (DESIGN ONLY) to the measured technology**
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

TetLab's pinned kernels are CUDA.jl (`tet-lab/gpu/Project.toml`). WGE's product
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
     (budget one debug cycle) -> results/spiral-i-comparators.csv fills the
     crossover surface. GATE: negative envelope known per (class, N); policy
     defaults for direct_field vs cage_parametric are measured, not guessed.
     STATUS 2026-10-02: health PASS (F-I.4 closed by the 04:49 reboot; probe
     reconstructed as /tmp/p1_health.jl). Spiral I v4 hit the budgeted
     first-run kernel bug (F-I.5: lbs! palette instance offset — the dry-review
     of the never-executed N>1 territory also caught B_direct's vertex binding
     and an unwrapped identity comprehension); v5 fix → rerun ×2 ALL GATES PASS
     (HI-G4 stable columns byte-identical, sha16 `4a9144326ebed3cb`).
     GATE MET: crossover surface filled in results/spiral-i-comparators.csv;
     HI-P1 amended at the (blob, N=128) corner (B_direct stops winning there
     in BOTH runs); policy defaults now measured. Full crossovers + honest
     negative envelope in tet-lab/tetcage-goal2-final.md §4/§9.
P0   Vulkan compute parity port (spiral-G methodology): deform!/reconstruct! as one
     GLSL compute dispatch; oracle = TetDeform CPU path; gate max ≤ 0.5 px vs the
     analytic model, expected ~ulp; CSV evidence + sha pin. GATE: parity + wall
     within 2× of the CUDA pins on the same classes, or explain why.
     >>> CLOSED 2026-10-02. Driver `graphics_lab/tet-lab/vk/spiral_p0_compute.jl`;
     canonical CSV `tet-lab/results/spiral-p0-vulkan-compute.csv` sha16
     `84915b3c1c3932df` (run b); P0-G5 stable parity block (168 rows) sha256
     `5160e8a3e21e018c…` byte-identical x2; logs + run-A CSV archived at
     `tet-lab/logs/spiral-p0-run-{a,b}.{log,csv}`. ALL GATES PASS.
     PARITY: 7 classes x 2 families x N in {1,8,32,128} x 3 layouts = 168 rows.
     Worst object error 9.703e-07 (conifer) against a per-class bound of
     4·eps(f32)·maxcoord; worst screen error 1.09e-04 px against the 0.5 px
     gate — i.e. the arithmetic ported from the E authority was correct on
     the FIRST hardware execution, at ulp, everywhere. WALL: within 2× of the
     pinned CUDA comparators on every cell where a same-(class,N) pin exists
     (worst 1.58x, teapot); the one cell over it, blob/rot/N=128 at 3.72x, is
     the registered DRAM-write-bound corner already carrying a 2.04x CUDA
     band (55.08/112.21 us). G0-G6 all GREEN, including the negative control
     (cage_y quarter shifted 0.52 → obj 1.080 vs bound 5.700e-07: the gates
     detect). Twelve defects registered in tetcage-rt-failure-register.md
     (F-P0.1…F-P0.9); no arithmetic defect in any of them.
     The 2.9× headroom claim was RE-MEASURED, not assumed, per §1: the shipped
     planar+packed form is at or under the CUDA pins wherever a same-cell pin
     exists — the claim survives, with the corner band carried forward.
     >>> RE-VERIFIED 2026-10-02 after Spiral H2 (P0-G4i). H2 measured the
     missing CUDA wind N-ladder (28 rows, HF2-G1 identity at every (class,N),
     HF2-G2 byte-identical x2, sha256 `088909edf19ff5f4…`), so BOTH families
     now have same-(class,N) pins at all four N and the P0-G4d same-cell
     substitute was RETIRED rather than left dormant. Re-run x2 ALL GATES
     PASS, canonical CSV sha16 `ce4be8ca13563bb4`; the parity block sha256
     `5160e8a3e21e018c…` is UNCHANGED, which is the correct outcome and a
     useful check — the comparator table is wall-side only, so the numeric
     evidence must be invariant to it. Worst ratio away from the registered
     corner: 1.65x (teapot). `blob/wind/N=128` — the cell with no valid
     comparator at all before H2 — now carries a real same-(class,N) claim at
     1.47x against H2's 135.79 us pin.
     >>> AND THE STANDING LESSON, because it is the part most likely to be
     re-learned expensively: every amendment in the P0-G4d→G4h chain existed to
     work around a MISSING MEASUREMENT. Each was individually defensible; each
     added machinery, calibration, resolution limits and asserts. The fix was
     ~150 lines of benchmark that measured the thing nobody had measured. **A
     workaround that keeps growing a gate is telling you to go measure, not to
     keep tuning.**
P1   Packet v7 extension lands behind the flag: conifer-wind single instance on the
     certified worker path; telemetry (wall µs, instances, bytes/frame); typed
     rejections; static fallback proven. GATE: existing suites byte-identical with
     flag off; flagged run presents deformed frames with receipts.
     >>> CLOSED 2026-10-02. `native_graphics_contract::deformation` +
     `WGEGraphics.jl`, flag `WGE_TETCAGE_DEFORM_V7`. **GATE MET on both
     halves.** (a) BYTE-IDENTICAL WITH FLAG OFF, proved against a COMMITTED
     v6 artifact rather than a round-trip: re-sealing
     `artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/
     graphics_scene_packet.json` under the new code reproduces its committed
     `packet_sha256` exactly — an assertion a leaked field cannot pass.
     Byte-identity is a property of the TYPE (`Option` +
     `skip_serializing_if`), not of a lucky test. Suites: 68→77 passed, 0
     failed (the +9 is the new P1 suite); Julia `lava_adapter` protocol suites
     unchanged. (b) FLAGGED RUN: v7 conifer-wind (1 instance, 226 tets, 3508
     verts, H2's pinned A=0.2/phi=0.7/scale) validates, seals, round-trips and
     carries receipts that agree with it; all 9 typed rejections reachable
     through the real validator; static fallback validated as EVIDENCE (a
     fallback frame must report no deform wall and must carry a reason).
     v6/v7 are mutually exclusive in both directions on BOTH sides of the
     Julia/Rust boundary — a receiver never has to guess whether the section
     was ignored.
     >>> AND ONE FINDING P1 SURPRISED US WITH, recorded because it will recur:
     `wge-certification-authority` did not compile at HEAD — the earlier
     mip-residency slice added a field to `GraphicsTelemetry` and closed
     without running the WORKSPACE gate, so a crate outside its own lane sat
     broken and invisible until P1 happened to touch the same struct.
     **A slice that changes a shared contract type owes the workspace gate,
     not just its own crate's suite.**
P2   In-engine instancing ladder N ∈ {1,8,32,128} × classes; frame + memory
     telemetry vs the Spiral-I table. GATE: measured in-engine economics ≥ the
     registered predictions, or the gap is explained and the envelope updated.
     >>> INHERITS AN OPEN ITEM (P2 owns the root cause, deliberately not closed
     at P0): the DRAM-write-bound corner `blob/N=128` (12·ntot = 15.7 MB) has a
     **5.4x CROSS-RUN band on the `soa+packed` layout** — 211.72 / 39.10 /
     214.56 us across three runs of the same binary — while the other two
     layouts held ~208–214 us in every run and the WITHIN-run trial spread
     stayed tight (193.79–216.42 us). Stable within a process, 5.4x apart
     between processes; mechanism not established (candidates only: per-
     iteration output-buffer placement, GPU clock/power state). P2's first act
     is to reproduce it deliberately — repeated fresh processes at that one
     cell with buffer placement logged — because P2 is where in-engine
     CAPACITY claims are published and every such claim inherits this band.
     Until then the capacity number at that working set carries the band.
     P0's determinism does not depend on it: the parity columns are
     byte-identical x2 and are unaffected by wall variance.
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
  driver were the two known risks entering P-1; both are CLOSED (reboot;
  v5 fix, ×2 gates GREEN) and registered in
  `tetcage-rt-failure-register.md`. Entering P0: the measured wall ordering at
  the (blob, N=128) corner is machine-state-sensitive across runs (2-run
  spread in the CSV's machine-state columns) — capacity claims at that
  working set carry that band.
- CLOSED at P0, and the most useful thing the port taught the campaign: **a
  comparator pin carries its (config, N) domain.** H produced wind pins with
  no N-ladder, and the P0 driver applied them across the whole ladder,
  reporting a "20.4x regression" that was 128x-the-work-against-a-1x-pin. The
  ported kernel was never at fault (wind and rot cost the same at every N
  measured). The 2× gate now asserts only where a same-(class,N) pin exists
  and labels the rest UNAVAILABLE. Registering the domain of a pin is part of
  using it, exactly as registering a gate is part of running it.
- Second P0 lesson, recorded because it nearly shipped as a false PASS: **a
  threshold derived from a measurement can disable the check that consumes
  it.** The floor probe's own spread is the resolution limit for the
  substitute wall gate; measured cold it was 656 µs against a 14 µs floor and
  silently marked all 21 cells UNRESOLVABLE — the gate reported ALL PASS while
  covering nothing. The calibration now asserts its own sanity and the gate
  reports how many cells each form actually gated (4-5 of 21).
- Third: **"stable across trials" and "stable across runs" are different
  claims.** The (blob, N=128) soa+packed cell is tight within a run
  (193.79–216.42 µs over its trials) and swings 5.4x between runs
  (39.10 / 211.72 / 214.56 µs). Determinism here rests on the parity columns,
  which are byte-identical x2; the wall at that corner is a sample of one
  machine state, and P2 inherits the band.
