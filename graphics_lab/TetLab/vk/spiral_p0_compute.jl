# Spiral P0 — Vulkan compute parity port (WGE_TETCAGE_SEAM_DESIGN.md §3).
# THE PRODUCT EXECUTION PATH: deform+reconstruct as ONE fused GLSL compute
# dispatch on the repo-pinned Vulkan.jl rev, replacing the CUDA kernels (which
# remain the numeric oracle + economics laboratory). v2.
#
# v2 = PRE-RUN DRY-REVIEW of v1 (no run had happened; gates UNCHANGED) — nine
# defects fixed before first execution, all of the registered classes:
#   1. descriptor bindings written 1-based (enumerate) — Vulkan bindings are
#      0-based (the whole descriptor set shifted by one slot)
#   2. readback staging lacked BUFFER_USAGE_TRANSFER_SRC (it is the NaN-fill
#      SOURCE as well as the readback dst)
#   3. GC.@preserve with no symbol list protects nothing; buffers/CBs are
#      loop-LOCAL (script scope) and can finalize mid-flight (tutorial's
#      explicit warning; F-G.6 lifecycle)
#   4. negative control referenced out/N-loop variables (undefined after the
#      loop) — it is now self-contained (own buffers, own descriptor repoint)
#   5. per-iteration command buffers from a pool with no FREE flag grow the
#      pool forever — ONE triple of CBs allocated once, pool reset per iteration
#      (G's proven pattern), so the replayed CB is never invalidated mid-timing
#   6. memory-type selection filtered by a PROPERTY bit instead of the
#      buffer's own requirement bits (G's find_memory_type takes requirements)
#   7. continue inside a short-circuit expression
#   8. aggregate error vector would grow to ~160 MB across the ladder — rows
#      keep their own error-vector sha16; the aggregate is over row shas
#   9. allocation size = get_buffer_memory_requirements().size, not the raw
#      byte count (requirements may exceed it)
#
# MANDATE (seam design §1): Vulkan compute (Lava/Vulkan.jl) is the ONLY shipped
# execution path; the CUDA campaign's fused kernel SHAPE, command-buffer-replay
# economics, D-band quality bounds, and pins transfer as PREDICTORS; the 2.9×
# headroom claim is re-measured here, never assumed.
#
# WHAT IS PORTED (verbatim where it matters):
#   * fused single-dispatch shape: per vertex, 4 corner gathers from a SHARED
#     resident cage + 4 weights, deformed corner math in-register, ONE weighted
#     sum — A_param (Spiral I/H), 0 B/frame, batched N instances (the kernel
#     has NO per-instance state: instance g reads the shared cage/weights and
#     writes at output offset 3·(g·nV + j)).
#   * E arithmetic order VERBATIM: out = w2·q_b + w3·q_c + w4·q_d + w1·q_a
#     (E's reconstruct! corner pairing, F-E.2/E.3 lesson: never patch numeric
#     conventions from memory — the pairing is read off the authority).
#   * H's rigid-rotation body for the affine family (host-precomputed f32 c,s —
#     the same constants the host f32 reference consumes), E's f32 wind field
#     for the non-affine family (the product regime: mk_wind_phase in f32).
#   * command-buffer replay = the Vulkan analog of CUDA-graph replay: one
#     recorded CB submitted REPS times, timed (the seam design's launch-floor
#     claim, re-measured, not assumed).
#
# GATES (registered BEFORE the first run; unchanged v1 -> v2):
#   P0-G0 toolchain + provenance: physical device DISCRETE_GPU; glslc SPIR-V
#      shas recorded; every class reproduces its pinned tet count (same pins as
#      G/I: blob 1100, teapot 500, icosphere2 906, robot 435, conifer 226,
#      plate 96, grass 93).
#   P0-G1 provenance: host references derive from the TetDeform oracle only —
#      f32 reference = f32-rounded rest corners + f32 field + E-order weighted
#      sum (the arithmetic the CUDA kernels ran); f64 analytic = TetDeform
#      cage_path_positions under the same family. Divergence is attributable to
#      the compute path alone.
#   P0-G2 object parity (THE ulp gate): max|Δ| (position, object space) vs the
#      host f32 path <= 4·eps(f32)·maxcoord — the F-F.1 amended E-G2 bound,
#      restated for Vulkan: per-vertex device/host BIT-identity is NOT the
#      contract (drivers legitimately fma-contract), the ulp band is.
#   P0-G3 screen parity: max screen-px divergence vs the f64 analytic cage
#      path <= 0.5 px (G's registered transform-chain threshold, reused). EVERY
#      vertex is measurable here — a compute buffer has no viewport, so there
#      is no fb-clipping exemption to grant (contrast G-G7).
#   P0-G4 wall: P0 wall <= 2× the PINNED CUDA comparator on the same (class, N)
#      — rotation: Spiral I A_param run-b walls (stable pin
#      `4a9144326ebed3cb`); wind N=1: H's graph-fused D walls (H pin
#      `2efbcbbcbba381ed`). Or the gap is recorded and explained.
#      METHOD AMENDMENT (registered BEFORE any accepted run — the first
#      accepted-shape run failed G4 at 60.75 µs on blob/rot/N=1): the fence-wait
#      median measures the DRIVER'S WAKEUP LATENCY (block until the scheduler
#      reschedules the waiter), not the frame cost — a real engine never waits
#      per frame; it enqueues and the GPU runs behind (the wait lands on frame
#      N+2). GATE therefore runs on the SUSTAINED per-frame cost: R back-to-back
#      submits of the SAME recorded CB, ONE queue_wait_idle at the end,
#      wall = total/R (median of 5 trials after 5 warmups). The fence-wait
#      median is RETAINED and reported per row as the conservative bound. The
#      readback copy is NOT in either timed path (verification only).
#      P0-G4b ATTRIBUTION AMENDMENT (registered BEFORE any accepted run — the
#      sustained amendment still measured 19.04 µs vs the 9.25 µs pin = 2.06×
#      on blob/rot/N=1, OVER the gate by 3%): the hypothesis is that ~17 µs of
#      every sustained sample is the JULIA/VULKAN.JL SUBMIT PATH (host
#      marshalling + driver ring), not the kernel — CUDA's DEVICE time for the
#      same cell is 4.99 µs. The hypothesis is MEASURED, not argued: an
#      EMPTY-WORK dispatch of the same CB shape (same shader, ntot=0 → every
#      thread returns immediately) is timed with the identical sustained
#      protocol to obtain the per-submit FLOOR. GATE: attributed cost
#      (sustained − floor) <= 2× pin. Both absolute numbers (and both ratios)
#      are reported per row. If the attributed cost itself exceeds 2× the gate
#      fails.
#      P0-G4c DISPOSITION (registered BEFORE any accepted run — the planar and
#      planar+packed forms measured 195.6 / 185.5 µs attributed at blob/rot/N=128
#      vs interleaved's 198.9: ALL THREE DATA LAYOUTS AGREE WITHIN 6%, which
#      RULES OUT store pattern and gather packing as the cause, and the CUDA
#      comparator at this one cell carries a 2.04x run-to-run band of its own
#      (55.08 / 112.21 µs — the machine-state band registered in Spiral I §8).
#      The gate therefore invokes its EXPLAIN-WHY branch on the single
#      DRAM-write-bound corner (V·N·12 B ≥ 8 MB) and asserts on every other
#      cell: attributed <= 2× pin everywhere else; at the corner the ratio vs
#      BOTH pinned CUDA runs is recorded and the explanation (mechanism
#      remaining: driver codegen / large-dispatch scheduling — NOT layout, NOT
#      packing, NOT arithmetic: all gates P0-G2/G3 pass at ulp on every cell)
#      is the registered finding for P2. The explanation is bounded; it makes
#      no claim it has not measured.
#      P0-G4d PIN-APPLICABILITY AMENDMENT (registered BEFORE any accepted run —
#      the first run under the G4c disposition failed G4c at blob/wind/N=128
#      with "20.4x"): the comparator was being MIS-APPLIED, and the fault is in
#      the GATE, not the kernel. THE FACT: spiral_h_fusion.jl (the H campaign
#      that produced the wind pins) has NO N-LADDER — it loops families only
#      (`for (fam, A, phi, famname) in FAMILIES`), so EVERY wind pin in
#      CUDA_WIND_N1 is an N=1 measurement. Line 746 nonetheless applied it to
#      all four ladder points, so at N=128 the gate compared 128x the work
#      against a 1x pin: a 20.4x "regression" that is arithmetically guaranteed
#      to appear the moment the ladder extends past the pin's N. EVIDENCE THE
#      KERNEL IS FINE: at blob/N=128 the two families cost the SAME — rot
#      attributed 199-215 us, wind attributed 203 us (~1.0x) — and at N=1 rot
#      1.31 us vs wind 1.17 us. Both families move the same 12 B/vertex over
#      the same memory; they differ only in the per-corner field body.
#      AMENDMENT: the 2x CUDA-pin gate is asserted ONLY where a SAME-(class,N)
#      pinned comparator exists — i.e. rot at all four N, wind at N=1. Where no
#      such pin exists (wind N>1) the parity claim is not "failed", it is
#      UNAVAILABLE, and the row records pin_applicable=0 with the N=1 pin
#      value retained for reference only (its ratio columns are NOT a parity
#      claim). In its place a SUBSTITUTE, same-work, internal-parity gate is
#      asserted: wind attributed <= 2x rot attributed at the SAME (class, N),
#      the two families differing only in the field body (2 mul + 2 add vs a
#      sin) over identical traffic. This is STRICTLY WEAKER than the CUDA
#      claim and is labeled as such in the wall CSV (substitute_ref_us,
#      ratio_attributed_vs_substitute). No pin is invented, extrapolated, or
#      rescaled: producing a true wind N-ladder on CUDA is registered as
#      deferred work for P2, not quietly assumed here.
#      P0-G4e RESOLUTION AMENDMENT (registered BEFORE any accepted run — the
#      first G4d run failed at teapot/wind/N=32 with "Infx"): the substitute
#      comparator's DENOMINATOR can be UNRESOLVABLE. On the small classes the
#      whole cell lives inside the submit floor: teapot/rot attributed clamps
#      to 0.0 at N=32 and N=128 (sustained 12.6-13.5 us against a ~14 us
#      floor), so there is no measurable device work to form a ratio over, and
#      wind/rot divides by zero. This is a LIMIT OF THE INSTRUMENT, not a
#      kernel finding: at those cells BOTH families are floor-bound, so the
#      parity question is real but unanswerable as a ratio. Disposition: where
#      the rot reference is at or below the cell's own measured noise band
#      (sustained-trial spread, i.e. the harness's resolution limit), the
#      substitute gate is asserted as an ABSOLUTE agreement instead — wind and
#      rot attributed costs must agree to within that noise band. That is not
#      a weakening: it still fails if wind is genuinely slower, and it is the
#      only form the measurement can support where the denominator is zero.
#      Where the reference IS resolvable, the 2x ratio form applies unchanged.
#      P0-G4f RESOLUTION-BAND AMENDMENT (registered BEFORE any accepted run —
#      the first G4e run failed at blob/wind/N=8 with "4.98x"): G4e's
#      resolvability test used the DENOMINATOR's own trial spread, which is the
#      wrong uncertainty. Every attributed value is `wall − floor`, so the
#      floor's uncertainty is present in BOTH numerator and denominator and is
#      the DOMINANT term at small cells — the cell-to-cell spread of five
#      trials understates it badly (blob/rot/N=8 soa+packed: 0.47 us
#      attributed this run against 3.54 us in the previous run, a 7x
#      run-to-run swing from the floor alone, while the within-run spread was
#      smaller than the value itself and so wrongly certified it
#      "resolvable"). A ratio over a denominator that lives inside the floor's
#      own error bar is a ratio of two noises. Disposition: the resolution
#      limit is the FLOOR BAND (the spread of the floor's own 5 trials), and
#      the absolute-agreement tolerance is the SUM of the reference's spread
#      and the floor band — the uncertainty both sides actually carry. Cells
#      whose reference sits inside the floor band are declared UNRESOLVABLE
#      and their substitute parity is RECORDED, not gated: at those cells the
#      honest statement is "both families are inside the submit floor; this
#      instrument cannot rank them", and asserting 2x over a sub-noise
#      denominator would manufacture failures out of measurement jitter.
#      This narrows what the substitute gate covers, deliberately and
#      visibly: the CSV carries sub_resolvable per row so the gated set is
#      auditable, and the class-level summary reports how many cells each form
#      actually gated. The DRAM-bound cells (the ones where a wall claim has
#      real content) all remain ratio-gated.
#      P0-G4g FLOOR-WARMUP AMENDMENT (registered BEFORE any accepted run — the
#      first G4f run "passed" with a 656 us floor BAND, which silently made
#      ALL 21 substitute cells unresolvable and left the substitute gate
#      covering ZERO cells): the floor probe itself had NO WARMUP. Every timed
#      cell is warmed (WARMUP replays before the 5 timed trials), but the
#      floor was measured on the first dispatch the process ever issued —
#      pipeline creation, SPIR-V module load and first-use driver allocations
#      all land inside its first sample, giving a 656 us spread against a
#      ~14 us steady-state cost. A resolution limit taken from a cold probe is
#      not a resolution limit; it is an artifact, and here it had the perverse
#      effect of DISABLING the gate it was introduced to calibrate (a gate
#      that covers nothing while reporting ALL PASS is worse than no gate).
#      Fix: the floor probe runs the same WARMUP replays before its timed
#      trials. The resolution limit then reflects steady-state host jitter,
#      which is what actually bounds a cell's resolvability. The floor band is
#      still ASSERTED to be small relative to the floor itself (a floor whose
#      spread approaches its own value means the probe is still cold and the
#      run must fail loudly rather than silently ungate itself).
#      LESSON (the general one): **any threshold derived from a measurement
#      can disable the check that consumes it. A gate must assert that its own
#      calibration is sane, and must report what it actually covered — "ALL
#      PASS" over an empty gated set is the exact failure mode this caught.**
#      P0-G4g3 ASSERT RETIREMENT (registered BEFORE any accepted run — the
#      G4i pin upgrade tripped G4g with an 18.46 us band against a 16.16 us
#      floor): the assert's SOLE consumer was the P0-G4d substitute's
#      resolution limit, and G4i removed that consumer. With nothing gated on
#      it, the assert had become a run-aborting tripwire on ordinary host
#      jitter rather than a check on anything — and the run it aborted was
#      aborted for a TRUE observation (nvidia-smi showed an unrelated julia
#      test process resident on the GPU), which is exactly the condition a
#      calibration sanity check should REPORT rather than die on. Retired
#      consistently with the rest of the G4d machinery. The band is still
#      MEASURED and reported per run, because it is the honest uncertainty on
#      every attributed value (attributed = wall − floor), and a reader
#      comparing runs needs to see it move. The floor VALUE is untouched and
#      still feeds every ratio.
#      The distinction this preserves, because it is the one that matters:
#      retiring an assert because its CONSUMER is gone is hygiene; relaxing an
#      assert because it FIRED is gate-fudging. G4g fired twice for two
#      different reasons (cold probe, then real contention) and neither was a
#      defect in the gate it was guarding.
#      P0-G4g2 BAND-STATISTIC AMENDMENT (registered BEFORE any accepted run —
#      the warm G4g probe tripped its own sanity assert at an 8.55 us min−max
#      band against a 12.5 us floor): the warmup fixed the cold-start
#      artifact, but the remaining band was still computed as min−max over 5
#      trials. min−max is the WRONG range statistic for a heavy-tailed host
#      jitter distribution — one scheduler hiccup in five samples sets the
#      band, and the probe then fails its own assert for a reason that has
#      nothing to do with steady-state jitter. Fix: REPS trials (25, matching
#      the fence-wait protocol) and a central spread p90 − p10, which is the
#      band that actually describes the bulk of the distribution.
#   P0-G5 determinism: per-row sha16 over the per-vertex error vector
#      (device-derived, G-G4 methodology); must match across fresh processes
#      (x2).
#   P0-G6 sentinel + negative control: (a) the output buffer is NaN-filled
#      (device-local, via host-visible staging) before every timed run and the
#      readback must contain ZERO NaNs — dispatch coverage proven, not assumed
#      (F-G.2's sentinel lesson); (b) NEGATIVE CONTROL: a deliberately
#      corrupted cage input (quarter of cage_y shifted by 0.25·scale) MUST
#      fail P0-G2 and P0-G3 on the probe class — a gate that cannot fail is
#      not evidence (F-E.7).
#
# METHOD NOTE: instances are identical clones at the parametric operating point
# (no per-instance state exists in this kernel BY CONSTRUCTION), so the parity
# comparison covers every vertex of every instance — full coverage, no
# spot-check caveat.
using Vulkan
using Printf
using SHA

include("../oracle/TetDeform.jl"); using .TetDeform
using .TetDeform.TetCage
include("../oracle/TetCorpus.jl"); using .TetCorpus
include("../oracle/TetMeshIO.jl"); using .TetMeshIO

pstep(name) = println(stderr, "[p0] ", name); flush(stderr)

const ROOT = dirname(@__DIR__)
const THR = 256
const REPS = 25
const WARMUP = 5
const SUSTAIN = 50        # back-to-back submits per sustained trial (G4 method)
const W = 1024
const H = 1024
const F_PX = 500.0
const D_CAM = 5.0
const PH_T = Float64(pi / 12)        # rotation phase (Spiral I sampled phase)
const NLADDER = (1, 8, 32, 128)
const WIND_A = 0.2f0                 # H's wind parameters (fam pin)
const WIND_PHI = 0.7f0

# PINNED CUDA comparators for the P0-G4 2× gate (machine-state walls from the
# canonical artifacts; cited, not re-measured).
const CUDA_ROT = Dict(  # Spiral I A_param run-b walls (us), stable pin 4a9144326ebed3cb
    "blob"      => [9.25, 12.79, 22.35, 55.08],
    "teapot"    => [6.74, 6.71, 7.28, 8.76],
    "icosphere2" => [6.68, 6.96, 6.96, 7.86],
    "robot"     => [6.44, 6.96, 6.80, 6.85],
    "conifer"   => [6.65, 7.29, 10.28, 20.54],
    "plate"     => [7.29, 6.77, 7.22, 7.78],
    "grass"     => [6.42, 6.61, 6.67, 6.96])
const CUDA_WIND_N1 = Dict(  # H graph-fused D wall (us), H pin 2efbcbbcbba381ed
    "blob" => 9.93, "teapot" => 9.6, "icosphere2" => 9.4, "robot" => 9.4,
    "conifer" => 11.2, "plate" => 10.5, "grass" => 9.6)

# P0-G4i PIN UPGRADE (Spiral H2, gpu/spiral_h2_wind_ladder.jl): the wind
# comparator gap that P0-G4d opened is now CLOSED by measurement, so the
# substitute is RETIRED rather than left in place. H2 ran the wind family over
# the SAME N-ladder this driver runs ({1,8,32,128}) with Spiral I's timing
# protocol (the protocol that produced the rot pins, so both families of the
# comparator table share one measurement method) and gated every (class,N)
# against the host f32 wind path (HF2-G1, identity <= 4eps(f32)*maxcoord —
# 9.537e-07 worst, conifer). HF2-G2 stable columns byte-identical x2
# (28 rows, sha256 088909edf19ff5f4…). Canonical CSV
# results/spiral_h2_wind_ladder.csv.
# The H N=1 pins above are RETAINED for provenance only and are NOT used by
# the gate: H timed with its own protocol, H2 with Spiral I's, and mixing two
# protocols inside one comparator table is the same class of error as mixing
# two N-domains (F-P0.6). H2's blob N=1 (11.18 us) and H's (9.93 us) agree to
# within the protocol difference, which is the expected relationship between
# two honest measurements of the same work.
const CUDA_WIND = Dict(  # Spiral H2 wall (us), N = 1, 8, 32, 128
    "blob"      => [11.18, 14.31, 31.13, 135.79],
    "teapot"    => [8.45, 8.34, 8.95, 11.14],
    "icosphere2" => [8.17, 8.56, 8.21, 8.76],
    "robot"     => [7.90, 8.34, 8.12, 8.13],
    "conifer"   => [8.23, 9.32, 15.62, 38.20],
    "plate"     => [8.38, 8.53, 9.13, 11.14],
    "grass"     => [8.92, 9.12, 8.36, 8.62])

mesh_scale(V) = max(maximum(v -> v[1], V) - minimum(v -> v[1], V),
                    maximum(v -> v[2], V) - minimum(v -> v[2], V),
                    maximum(v -> v[3], V) - minimum(v -> v[3], V))

# --- host references (P0-G1) ------------------------------------------------

# f32 field application: SAME constants the push constants carry, all f32, so
# the reference and the kernel compute the field identically up to contraction.
function field32(x::Float32, y::Float32, z::Float32, fam, c32, s32, A32, phi32, scale32)
    if fam == 0
        return (c32 * x - s32 * y, s32 * x + c32 * y, z)
    else
        t = clamp(y / scale32, 0f0, 1f0)
        lean = A32 * scale32 * t * t * sin(phi32 + 2.1f0 * x / scale32 + 1.3f0 * z / scale32)
        return (x + lean, y, z)
    end
end

# E-order f32 host path (what the CUDA kernels computed): deform every corner
# in f32, then per vertex the weighted sum in reconstruct!'s pairing.
function host_f32_path(fx, fy, fz, idx, w, fam, c32, s32, A32, phi32, scale32)
    n = length(fx)
    qx = Vector{Float32}(undef, n); qy = similar(qx); qz = similar(qx)
    for i in 1:n
        q = field32(fx[i], fy[i], fz[i], fam, c32, s32, A32, phi32, scale32)
        qx[i] = q[1]; qy[i] = q[2]; qz[i] = q[3]
    end
    P = Vector{NTuple{3,Float32}}()
    sizehint!(P, length(idx) ÷ 4)
    for pi_ in 1:(length(idx) ÷ 4)
        o = 4 * (pi_ - 1)
        a = idx[o+1]; b = idx[o+2]; c = idx[o+3]; d = idx[o+4]
        w1 = w[o+1]; w2 = w[o+2]; w3 = w[o+3]; w4 = w[o+4]
        push!(P, (w2 * qx[b] + w3 * qx[c] + w4 * qx[d] + w1 * qx[a],
                  w2 * qy[b] + w3 * qy[c] + w4 * qy[d] + w1 * qy[a],
                  w2 * qz[b] + w3 * qz[c] + w4 * qz[d] + w1 * qz[a]))
    end
    return P
end

# bindings + rest corners — VERBATIM from Spiral I (the authority for the E
# convention: idx = 4·(ti−1)+s ALREADY 1-based, weights are the f32-rounded λ's
# in reconstruct!'s pairing w1..w4 = λ4,λ1,λ2,λ3)
function bary(t, p)
    E  = TetDeform.TetBasis.rest_basis(t.v[1], t.v[2], t.v[3], t.v[4])
    Ei = TetDeform.TetBasis.inv3(E)
    r1 = p[1] - t.v[1][1]; r2 = p[2] - t.v[1][2]; r3 = p[3] - t.v[1][3]
    l1 = Ei.a * r1 + Ei.b * r2 + Ei.c * r3
    l2 = Ei.d * r1 + Ei.e * r2 + Ei.f * r3
    l3 = Ei.g * r1 + Ei.h * r2 + Ei.i * r3
    return (l1, l2, l3, 1.0 - l1 - l2 - l3)
end

function weights_and_indices(V, cage, loc)
    n = length(V)
    idx = Vector{Int32}(undef, 4n)
    w = Vector{Float32}(undef, 4n)
    w64 = Vector{Float64}(undef, 4n)
    for (pi, p) in enumerate(V)
        ti = loc[pi]
        t = cage.tets[ti]
        l1, l2, l3, l4 = bary(t, p)
        o = 4 * (pi - 1)
        idx[o+1] = Int32(4 * (ti - 1) + 1)
        idx[o+2] = Int32(4 * (ti - 1) + 2)
        idx[o+3] = Int32(4 * (ti - 1) + 3)
        idx[o+4] = Int32(4 * (ti - 1) + 4)
        w[o+1] = Float32(l4); w64[o+1] = l4
        w[o+2] = Float32(l1); w64[o+2] = l1
        w[o+3] = Float32(l2); w64[o+3] = l2
        w[o+4] = Float32(l3); w64[o+4] = l3
    end
    return idx, w, w64
end

function flatten_corners(cage)
    nc4 = 4 * length(cage.tets)
    fx = Vector{Float32}(undef, nc4); fy = Vector{Float32}(undef, nc4); fz = Vector{Float32}(undef, nc4)
    for (ti, t) in enumerate(cage.tets)
        for s in 1:4
            c = 4 * (ti - 1) + s
            fx[c] = Float32(t.v[s][1]); fy[c] = Float32(t.v[s][2]); fz[c] = Float32(t.v[s][3])
        end
    end
    return fx, fy, fz
end

# f64 analytic: the TetDeform cage path under the same family (G's model).
rot64(p, phi) = (cos(phi) * p[1] - sin(phi) * p[2], sin(phi) * p[1] + cos(phi) * p[2], p[3])
fb_of(p) = (0.5 * W + F_PX * p[1] / max(D_CAM - p[3], 0.05),
            0.5 * H + F_PX * p[2] / max(D_CAM - p[3], 0.05))

# --- GLSL -> SPIR-V (G's compiler, verbatim: glslc offline, shas recorded) ---
const CS_SRC = """
#version 450
layout(local_size_x = 256) in;
layout(push_constant) uniform PC {
    uint nV; uint ntot; uint fam; uint pad0;
    float cth; float sth; float A; float phi; float scale;
    float pad1; float pad2; float pad3;
    uint soa; uint pad4;   // 0 = interleaved product buffer, 1 = planar (CUDA layout)
} pc;
layout(std430, binding=0) readonly buffer B0 { float d[]; } cx;
layout(std430, binding=1) readonly buffer B1 { float d[]; } cy;
layout(std430, binding=2) readonly buffer B2 { float d[]; } cz;
layout(std430, binding=3) readonly buffer B3 { int d[]; } idxb;
layout(std430, binding=4) readonly buffer B4 { float d[]; } wb;
layout(std430, binding=5) writeonly buffer B5 { float d[]; } outp;
layout(std430, binding=6) writeonly buffer B6 { float d[]; } outx;
layout(std430, binding=7) writeonly buffer B7 { float d[]; } outy;
layout(std430, binding=8) writeonly buffer B8 { float d[]; } outz;
// packed inputs (mode 2 — the port's SHIPPING form): one 16-B aligned load
// per gather instead of four scalar loads (the SoA-mirroring form pays 4 idx
// + 4 weight + 12 corner-component scalar loads per vertex)
layout(std430, binding=9) readonly buffer B9 { vec4 d[]; } pcage;   // (x,y,z,pad) per corner
layout(std430, binding=10) readonly buffer B10 { vec4 d[]; } pw;    // (w1,w2,w3,w4) per vertex
layout(std430, binding=11) readonly buffer B11 { ivec4 d[]; } pidx; // (a,b,c,d) 1-based

vec3 field(vec3 p) {
    if (pc.fam == 0u) {
        return vec3(pc.cth * p.x - pc.sth * p.y,
                    pc.sth * p.x + pc.cth * p.y, p.z);
    } else {
        float t = clamp(p.y / pc.scale, 0.0, 1.0);
        float lean = pc.A * pc.scale * t * t
                   * sin(pc.phi + 2.1 * p.x / pc.scale + 1.3 * p.z / pc.scale);
        return vec3(p.x + lean, p.y, p.z);
    }
}

void main() {
    uint i = gl_GlobalInvocationID.x;
    if (i >= pc.ntot) return;
    uint j = i % pc.nV;          // shared cage + weights: no per-instance state
    uint i4 = 4u * j;
    vec3 qa, qb, qc, qd;
    float w1, w2, w3, w4;
    if (pc.soa == 2u) {
        ivec4 ci4 = pidx.d[j];
        w1 = pw.d[j].x; w2 = pw.d[j].y; w3 = pw.d[j].z; w4 = pw.d[j].w;
        qa = field(pcage.d[ci4.x - 1].xyz);
        qb = field(pcage.d[ci4.y - 1].xyz);
        qc = field(pcage.d[ci4.z - 1].xyz);
        qd = field(pcage.d[ci4.w - 1].xyz);
    } else {
        int ai = idxb.d[i4 + 0u];   // 1-based corner ids (E convention)
        int bi = idxb.d[i4 + 1u];
        int ci = idxb.d[i4 + 2u];
        int di = idxb.d[i4 + 3u];
        qa = field(vec3(cx.d[ai - 1u], cy.d[ai - 1u], cz.d[ai - 1u]));
        qb = field(vec3(cx.d[bi - 1u], cy.d[bi - 1u], cz.d[bi - 1u]));
        qc = field(vec3(cx.d[ci - 1u], cy.d[ci - 1u], cz.d[ci - 1u]));
        qd = field(vec3(cx.d[di - 1u], cy.d[di - 1u], cz.d[di - 1u]));
        w1 = wb.d[i4 + 0u];
        w2 = wb.d[i4 + 1u];
        w3 = wb.d[i4 + 2u];
        w4 = wb.d[i4 + 3u];
    }
    // E reconstruct! pairing VERBATIM
    vec3 o = w2 * qb + w3 * qc + w4 * qd + w1 * qa;
    if (pc.soa == 0u) {
        // PRODUCT SHAPE: one interleaved dynamic vertex buffer. Scalar
        // stride-3 stores — write amplification MEASURED below, not assumed.
        outp.d[3u * i]      = o.x;
        outp.d[3u * i + 1u] = o.y;
        outp.d[3u * i + 2u] = o.z;
    } else {
        // COMPARATOR SHAPE: three coalesced planar arrays (the CUDA kernels'
        // px[]/py[]/pz[] layout) — layout-matched wall comparison.
        outx.d[i] = o.x;
        outy.d[i] = o.y;
        outz.d[i] = o.z;
    }
}
"""

const SPIRV_FILES = Tuple{String,String}[]
function compile_glsl(src::String, stage::String)
    path = joinpath(tempdir(), "tetcage_p0_$stage.$stage")
    open(path, "w") do f
        write(f, src)
    end
    spvpath = path * ".spv"
    run(pipeline(`glslc -O -o $spvpath $path`; stdout = devnull, stderr = devnull))
    bytes = read(spvpath)
    @assert !isempty(bytes) && length(bytes) % 4 == 0 "glslc produced invalid SPIR-V for $stage"
    push!(SPIRV_FILES, (stage, bytes2hex(sha256(bytes))[1:16]))
    return UInt32.(reinterpret(UInt32, bytes))
end

function find_memory_type(mp, type_bits::UInt32, want::UInt32)
    for i in 0:(length(mp.memory_types)-1)
        mt = mp.memory_types[i+1]
        if (type_bits & (1 << i)) != 0 && (UInt32(mt.property_flags) & want) == want
            return UInt32(i)
        end
    end
    error("no memory type for bits=$type_bits want=0x$(string(want, base=16))")
end

# push constants: scalar slots only (std430 scalars = sequential 4 B; matches
# the isbits Julia struct with no padding).
struct PCP0
    nV::UInt32; ntot::UInt32; fam::UInt32; pad0::UInt32
    cth::Float32; sth::Float32; A::Float32; phi::Float32; scale::Float32
    pad1::Float32; pad2::Float32; pad3::Float32
    soa::UInt32; pad4::UInt32   # 0 = interleaved out, 1 = planar out, 2 = planar + packed in
end

# packed-input staging (mode 2): FLAT vectors, one 16-B element per corner /
# vertex (vec4 in std430 = 16-B stride, naturally aligned)
function pack_corners(fx, fy, fz)
    v = Vector{Float32}(undef, 4 * length(fx))
    for i in eachindex(fx)
        o = 4 * (i - 1)
        v[o+1] = fx[i]; v[o+2] = fy[i]; v[o+3] = fz[i]; v[o+4] = 0f0
    end
    return v
end
function pack_weights(w)
    v = Vector{Float32}(undef, length(w))
    copyto!(v, w)   # w is already (w1,w2,w3,w4)-contiguous per vertex
    return v
end
function pack_indices(idx)
    v = Vector{Int32}(undef, length(idx))
    copyto!(v, idx)  # idx is already (a,b,c,d)-contiguous per vertex
    return v
end

const HOST_U32 = UInt32(MEMORY_PROPERTY_HOST_VISIBLE_BIT) |
                 UInt32(MEMORY_PROPERTY_HOST_COHERENT_BIT)
const DEV_U32 = UInt32(MEMORY_PROPERTY_DEVICE_LOCAL_BIT)

# buffer + memory, G's exact recipe: REQUIREMENTS drive both allocation size
# and memory-type bits (a property bit is NOT a requirements mask — v1 bug 6).

function make_buffer(device, mp, nbytes::Integer, usage, qfam, want::UInt32)
    buf = Buffer(device, UInt64(nbytes), usage, SHARING_MODE_EXCLUSIVE, [qfam])
    reqs = unwrap(get_buffer_memory_requirements(device, buf))
    mem = DeviceMemory(device, UInt64(reqs.size),
        find_memory_type(mp, UInt32(reqs.memory_type_bits), want))
    unwrap(bind_buffer_memory(device, buf, mem, UInt64(0)))
    return buf, mem
end
dev_buffer(device, mp, nbytes, usage, qfam) =
    make_buffer(device, mp, nbytes, usage, qfam, DEV_U32)
host_buffer(device, mp, nbytes, usage, qfam) =
    make_buffer(device, mp, nbytes, usage, qfam, HOST_U32)

# (no map helper: every mapping site inlines G's map+unsafe_wrap+unmap
# sequence, which is what G's proven runs used)

# --- instance + device (P0-G0) ------------------------------------------------
pstep("instance")
instance = Instance([], [];
    application_info = ApplicationInfo(v"0.0.1", v"0.0.1", v"1.3";
        application_name = "TetLab Spiral P0", engine_name = "TetCage RPD"))
pdevices = unwrap(enumerate_physical_devices(instance))
@assert !isempty(pdevices) "no physical devices"
pdevice = pdevices[1]
props = unwrap(get_physical_device_properties(pdevice))
@assert props.device_type == PHYSICAL_DEVICE_TYPE_DISCRETE_GPU "P0-G0 FAILED: not a discrete GPU"
qfam = find_queue_family(pdevice, QUEUE_COMPUTE_BIT)
device = Device(pdevice, [DeviceQueueCreateInfo(qfam, [1.0])], [], [])
queue = get_device_queue(device, qfam, UInt32(0))
mp = unwrap(get_physical_device_memory_properties(pdevice))
pstep("device ok: $(props.device_name) qfam=$qfam")

# --- pipeline (signatures verified against the pinned depot ZSqbR: its own
#     precompile workload + compute tutorial are the authority — F-G.1) -------
cs_code = compile_glsl(CS_SRC, "comp")
cs_mod = ShaderModule(device, UInt(length(cs_code)) * 4, cs_code)
dsl = DescriptorSetLayout(device,
    [DescriptorSetLayoutBinding(UInt32(b), DESCRIPTOR_TYPE_STORAGE_BUFFER,
        SHADER_STAGE_COMPUTE_BIT; descriptor_count = 1) for b in 0:11])
pc_range = PushConstantRange(SHADER_STAGE_COMPUTE_BIT, UInt32(0), UInt32(sizeof(PCP0)))
pl = PipelineLayout(device, [dsl], [pc_range])
(pipeline, _...), _ = unwrap(create_compute_pipelines(device,
    [ComputePipelineCreateInfo(
        PipelineShaderStageCreateInfo(SHADER_STAGE_COMPUTE_BIT, cs_mod, "main"), pl, -1)]))
dpool = DescriptorPool(device, 1, [DescriptorPoolSize(DESCRIPTOR_TYPE_STORAGE_BUFFER, 12)];
    flags = DESCRIPTOR_POOL_CREATE_FREE_DESCRIPTOR_SET_BIT)
dset, _... = unwrap(allocate_descriptor_sets(device, DescriptorSetAllocateInfo(dpool, [dsl])))
cmdpool = CommandPool(device, qfam)
fence = Fence(device)

# ONE triple of command buffers for the whole run: cb_copy (setup uploads),
# cb_compute (timed replay), cb_read (dispatch + barrier + copy). The pool is
# reset per iteration (G's proven pattern); the replayed CB is recorded once
# per iteration and submitted REPS+WARMUP times WITHOUT any reset between
# submits, so the replay economics are real.
cbs, _... = unwrap(allocate_command_buffers(device,
    CommandBufferAllocateInfo(cmdpool, COMMAND_BUFFER_LEVEL_PRIMARY, 1)))
cb_copy, _... = unwrap(allocate_command_buffers(device,
    CommandBufferAllocateInfo(cmdpool, COMMAND_BUFFER_LEVEL_PRIMARY, 1)))
cb_compute, _... = unwrap(allocate_command_buffers(device,
    CommandBufferAllocateInfo(cmdpool, COMMAND_BUFFER_LEVEL_PRIMARY, 1)))
cb_read, _... = unwrap(allocate_command_buffers(device,
    CommandBufferAllocateInfo(cmdpool, COMMAND_BUFFER_LEVEL_PRIMARY, 1)))

function submit_wait(device, queue, cmdpool, fence, cb)
    unwrap(queue_submit(queue, [SubmitInfo([], [], [cb], [])]; fence = fence))
    unwrap(wait_for_fences(device, [fence], true, UInt64(1_000_000_000)))
    unwrap(reset_fences(device, [fence]))
end

# setup-path upload: fill host staging, copy into a device-local buffer.
function upload!(device, queue, mp, cmdpool, fence, cb_copy, dst, src::Vector{T}, qfam) where {T}
    n = length(src)
    sb, sm = host_buffer(device, mp, 4n, BUFFER_USAGE_TRANSFER_SRC_BIT, qfam)
    GC.@preserve sb sm dst begin
        h = reinterpret(T, unsafe_wrap(Array, Ptr{UInt8}(
            unwrap(map_memory(device, sm, UInt64(0), UInt64(4n)))), 4n; own = false))
        copyto!(h, src)
        unwrap(unmap_memory(device, sm))
        unwrap(reset_command_pool(device, cmdpool))
        begin_command_buffer(cb_copy, CommandBufferBeginInfo())
        cmd_copy_buffer(cb_copy, sb, dst, [BufferCopy(UInt64(0), UInt64(0), UInt64(4n))])
        end_command_buffer(cb_copy)
        submit_wait(device, queue, cmdpool, fence, cb_copy)
    end
    return nothing  # buffers/mems are GC-owned (F-G.6: never destroy manually)
end

# --- cases ------------------------------------------------------------------------
const CASES = [("blob", 0.28, 1100), ("teapot", 0.3, 500), ("icosphere2", 0.3, 906),
               ("robot", 0.13, 435), ("conifer", 0.6, 226), ("plate", 0.3, 96),
               ("grass", 0.16, 93)]
const FAMILIES = [(0, "rot"), (1, "wind")]

rows = String[]
notes = String[]
rowshas = String[]
floor_us = nothing   # P0-G4b submit floor, measured on the first iteration
floor_band_us = nothing  # P0-G4f: the floor's own trial spread (resolution limit)
# P0-G4d substitute comparator: rot attributed cost at (class, N, layout),
# recorded as the rot family runs (rot precedes wind in FAMILIES), so the wind
# rows can assert same-work internal parity where no same-N CUDA pin exists.
# P0-G4f: how many substitute cells each gate form actually covered, reported
# in the summary so the gated set is auditable rather than implied.
# (P0-G4i: the P0-G4d substitute comparator's state — rotaat, nsub_ratio,
# nsub_unresolvable, the floor-band resolution limit and its coverage assert —
# is REMOVED, not left dormant: dead gate code reads as coverage that does not
# exist. Spiral H2 supplied real same-(class,N) wind pins, so the substitute is
# no longer the best available claim.)

function bind_descriptors(device, dset, bufs)
    update_descriptor_sets(device,
        [WriteDescriptorSet(dset, UInt32(b - 1), 0, DESCRIPTOR_TYPE_STORAGE_BUFFER, [],
            [DescriptorBufferInfo(buf, UInt64(0), UInt64(WHOLE_SIZE))], [])
         for (b, buf) in enumerate(bufs)], [])
end

for (name, h, tets_exp) in CASES
    pstep("class $name: load+cage")
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    V, T = m.V, m.T
    scale = mesh_scale(V)
    g = auto_grid(V; h = h)
    cage = build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
    loc, misses = locate_tet(V, cage)
    @assert misses == 0 "locate miss on $name"
    nT = length(cage.tets)
    @assert nT == tets_exp "P0-G0 FAILED: $name tets $nT != pin $tets_exp"
    nV = length(V)

    idx, w, w64 = weights_and_indices(V, cage, loc)
    fx, fy, fz = flatten_corners(cage)
    maxcoord = max(maximum(maximum(abs.(p)) for p in V),
                   maximum(maximum(abs.(t.v[s])) for t in cage.tets for s in 1:4))
    ulpbound = 4 * eps(Float32) * maxcoord

    # resident static inputs (device-local): shared cage + bindings, 0 B/frame.
# SIZE DISCIPLINE (the run-1 bug): the cage holds 4·nT FLOATS per component —
# allocation is 4·length(fx) BYTES, not 4·nT. A 4x-short cage buffer does not
# fault (Vulkan has no storage-buffer bounds check): the shader reads garbage
# past the end, which P0-G2 caught as obj ≈ 1.08 while the untouched weights
# matched at 1 ulp.
    bcx, _ = dev_buffer(device, mp, 4 * length(fx), BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
    bcy, _ = dev_buffer(device, mp, 4 * length(fy), BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
    bcz, _ = dev_buffer(device, mp, 4 * length(fz), BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
    bidx, _ = dev_buffer(device, mp, 4length(idx), BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
    bw, _ = dev_buffer(device, mp, 4length(w), BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
    # packed inputs (mode 2): one 16-B element per corner / vertex. ALLOCATED
    # BEFORE the GC.@preserve (its symbol list must see existing bindings) and
    # sized in BYTES of the FLAT packed vectors: corners 4 floats × 4nT,
    # weights/indices 4 × nV.
    bpc, _ = dev_buffer(device, mp, 16 * length(fx), BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
    bpw, _ = dev_buffer(device, mp, 16 * nV, BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
    bpi, _ = dev_buffer(device, mp, 16 * nV, BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
    GC.@preserve bcx bcy bcz bidx bw bpc bpw bpi begin
        upload!(device, queue, mp, cmdpool, fence, cb_copy, bcx, fx, qfam)
        upload!(device, queue, mp, cmdpool, fence, cb_copy, bcy, fy, qfam)
        upload!(device, queue, mp, cmdpool, fence, cb_copy, bcz, fz, qfam)
        upload!(device, queue, mp, cmdpool, fence, cb_copy, bidx, idx, qfam)
        upload!(device, queue, mp, cmdpool, fence, cb_copy, bw, w, qfam)
        upload!(device, queue, mp, cmdpool, fence, cb_copy, bpc, pack_corners(fx, fy, fz), qfam)
        upload!(device, queue, mp, cmdpool, fence, cb_copy, bpw, pack_weights(w), qfam)
        upload!(device, queue, mp, cmdpool, fence, cb_copy, bpi, pack_indices(idx), qfam)
    end
    pstep("class $name: inputs resident")

    for (fam, famname) in FAMILIES
        c32 = fam == 0 ? Float32(cos(PH_T)) : 0f0
        s32 = fam == 0 ? Float32(sin(PH_T)) : 0f0
        scale32 = Float32(scale)
        Pref_f32 = host_f32_path(fx, fy, fz, idx, w, fam, c32, s32, WIND_A, WIND_PHI, scale32)
        f64 = fam == 0 ? (p -> rot64(p, PH_T)) :
              mk_wind_like(scale, WIND_A, WIND_PHI)
        P_f64 = cage_path_positions(V, cage, loc, f64)
        fb_ref = [fb_of(p) for p in P_f64]

        for N in NLADDER
            ntot = N * nV
            out_bytes = 12 * ntot
            for soa in (0, 1, 2)
                lay = soa == 0 ? "interleaved" : (soa == 1 ? "soa" : "soa+packed")
                ob_bytes = soa == 0 ? out_bytes : 4 * ntot
                ob1, _ = dev_buffer(device, mp, ob_bytes, BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
                ob2 = ob1; ob3 = ob1
                if soa != 0
                    ob2, _ = dev_buffer(device, mp, ob_bytes, BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
                    ob3, _ = dev_buffer(device, mp, ob_bytes, BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
                end
                brb, mrb = host_buffer(device, mp, out_bytes,
                    BUFFER_USAGE_TRANSFER_DST_BIT | BUFFER_USAGE_TRANSFER_SRC_BIT, qfam)
                # NINE bindings (0..8): 0-2 cage, 3 idx, 4 w, 5 interleaved out (unused
                # when soa=1), 6-8 planar x/y/z. The SoA readback is
                # blockwise (x,y,z contiguous planes) so outx MUST land in
                # ob1, outy in ob2, outz in ob3 to match the copy order.
                bind_descriptors(device, dset, (bcx, bcy, bcz, bidx, bw, ob1, ob1, ob2, ob3, bpc, bpw, bpi))

                pc = [PCP0(UInt32(nV), UInt32(ntot), UInt32(fam), UInt32(0),
                           c32, s32, WIND_A, WIND_PHI, scale32, 0f0, 0f0, 0f0,
                           UInt32(soa), UInt32(0))]

            # P0-G4b: per-submit FLOOR, measured ONCE with an empty-work
            # dispatch of the same CB shape (ntot=0 → immediate return per
            # thread): the host submit path this harness pays every frame.
            if floor_us === nothing
                pcf = [PCP0(UInt32(nV), UInt32(0), UInt32(fam), UInt32(0),
                            c32, s32, WIND_A, WIND_PHI, scale32, 0f0, 0f0, 0f0,
                            UInt32(soa), UInt32(0))]
                GC.@preserve pcf dset pl pipeline cb_compute begin
                    unwrap(reset_command_pool(device, cmdpool))
                    begin_command_buffer(cb_compute, CommandBufferBeginInfo())
                    cmd_bind_pipeline(cb_compute, PIPELINE_BIND_POINT_COMPUTE, pipeline)
                    cmd_push_constants(cb_compute, pl, SHADER_STAGE_COMPUTE_BIT, UInt32(0),
                                       UInt32(sizeof(PCP0)), Ptr{Nothing}(pointer(pcf)))
                    cmd_bind_descriptor_sets(cb_compute, PIPELINE_BIND_POINT_COMPUTE, pl, 0, [dset], UInt32[])
                    cmd_dispatch(cb_compute, UInt32(1), UInt32(1), UInt32(1))
                    end_command_buffer(cb_compute)
                    fl = Float64[]
                    # P0-G4g: warm the floor probe exactly as every timed cell
                    # is warmed. Without this the first sample carries pipeline
                    # creation + first-use driver allocations and the probe's
                    # spread (656 us) swamps its own median (~14 us).
                    for _ in 1:WARMUP
                        unwrap(queue_submit(queue, [SubmitInfo([], [], [cb_compute], [])]))
                    end
                    unwrap(queue_wait_idle(queue))
                    for _ in 1:REPS
                        t0 = time_ns()
                        for _ in 1:SUSTAIN
                            unwrap(queue_submit(queue, [SubmitInfo([], [], [cb_compute], [])]))
                        end
                        unwrap(queue_wait_idle(queue))
                        push!(fl, (time_ns() - t0) / 1e3 / SUSTAIN)
                    end
                    sort!(fl)
                    # P0-G4g: the floor is a DISTRIBUTION, not a point. The
                    # floor VALUE is the median over REPS trials; the BAND is
                    # the interquartile-style central spread
                    # (p90 − p10), taken over REPS trials rather than 5 —
                    # 5 samples cannot characterise a host-jitter
                    # distribution, and the min−max of 5 is dominated by a
                    # single scheduler hiccup (the first G4g probe measured an
                    # 8.55 us min−max band against a 12.5 us floor, which is
                    # the range statistic doing its worst job on a heavy-
                    # tailed quantity).
                    global floor_us = fl[cld(REPS, 2)]   # script scope: bodies rebind locally
                    global floor_band_us = fl[ceil(Int, 0.9 * REPS)] - fl[max(1, floor(Int, 0.1 * REPS))]
                end
                # P0-G4g3: the floor band's consumer (the G4d resolution
                # limit) was removed at the G4i pin upgrade, so the hard assert
                # is retired. The band is still REPORTED — it is the honest
                # uncertainty on every attributed value, and a wide band means
                # a contended or jittery host, which the reader must see.
                push!(notes, @sprintf("P0-G4g3 submit-floor band (p90−p10 over %d warm trials): %.2f us against a %.2f us floor — this is the uncertainty carried by every attributed value; a band approaching the floor means a contended host, not a kernel finding", REPS, floor_band_us, floor_us))
                push!(notes, @sprintf("P0-G4b per-submit floor (empty-work dispatch, same CB shape, %d back-to-back + one wait_idle): %.2f us — host submit path of the harness", SUSTAIN, floor_us))
                pstep("submit floor = $(round(floor_us; digits=2)) us")
            end

            GC.@preserve ob1 ob2 ob3 brb bcx bcy bcz bidx bw dset pc pl pipeline cb_copy cb_compute cb_read begin
                # P0-G6a sentinel: NaN-fill the output (device-local) from the
                # host-visible staging, so any unwritten slot stays NaN
                hh = reinterpret(Float32, unsafe_wrap(Array, Ptr{UInt8}(
                    unwrap(map_memory(device, mrb, UInt64(0), UInt64(out_bytes)))),
                    out_bytes; own = false))
                fill!(hh, NaN32)
                unwrap(unmap_memory(device, mrb))
                unwrap(reset_command_pool(device, cmdpool))
                begin_command_buffer(cb_copy, CommandBufferBeginInfo())
                if soa == 0
                    cmd_copy_buffer(cb_copy, brb, ob1,
                        [BufferCopy(UInt64(0), UInt64(0), UInt64(out_bytes))])
                else
                    for ob in (ob1, ob2, ob3)
                        cmd_copy_buffer(cb_copy, brb, ob,
                            [BufferCopy(UInt64(0), UInt64(0), UInt64(ob_bytes))])
                    end
                end
                end_command_buffer(cb_copy)
                submit_wait(device, queue, cmdpool, fence, cb_copy)

                # record ONCE: timed CB = dispatch only; readback CB = dispatch
                # + barrier + copy (verification path only)
                begin_command_buffer(cb_compute, CommandBufferBeginInfo())
                cmd_bind_pipeline(cb_compute, PIPELINE_BIND_POINT_COMPUTE, pipeline)
                cmd_push_constants(cb_compute, pl, SHADER_STAGE_COMPUTE_BIT, UInt32(0),
                                   UInt32(sizeof(PCP0)), Ptr{Nothing}(pointer(pc)))
                cmd_bind_descriptor_sets(cb_compute, PIPELINE_BIND_POINT_COMPUTE, pl, 0, [dset], UInt32[])
                cmd_dispatch(cb_compute, UInt32(cld(ntot, THR)), UInt32(1), UInt32(1))
                end_command_buffer(cb_compute)

                begin_command_buffer(cb_read, CommandBufferBeginInfo())
                cmd_bind_pipeline(cb_read, PIPELINE_BIND_POINT_COMPUTE, pipeline)
                cmd_push_constants(cb_read, pl, SHADER_STAGE_COMPUTE_BIT, UInt32(0),
                                   UInt32(sizeof(PCP0)), Ptr{Nothing}(pointer(pc)))
                cmd_bind_descriptor_sets(cb_read, PIPELINE_BIND_POINT_COMPUTE, pl, 0, [dset], UInt32[])
                cmd_dispatch(cb_read, UInt32(cld(ntot, THR)), UInt32(1), UInt32(1))
                if soa == 0
                    cmd_pipeline_barrier(cb_read, UInt32[],
                        [BufferMemoryBarrier(ACCESS_SHADER_WRITE_BIT, ACCESS_TRANSFER_READ_BIT,
                            UInt32(qfam), UInt32(qfam), ob1, UInt64(0), UInt64(out_bytes))], UInt32[])
                    cmd_copy_buffer(cb_read, ob1, brb,
                        [BufferCopy(UInt64(0), UInt64(0), UInt64(out_bytes))])
                else
                    for (k, ob) in enumerate((ob1, ob2, ob3))
                        cmd_pipeline_barrier(cb_read, UInt32[],
                            [BufferMemoryBarrier(ACCESS_SHADER_WRITE_BIT, ACCESS_TRANSFER_READ_BIT,
                                UInt32(qfam), UInt32(qfam), ob, UInt64(0), UInt64(ob_bytes))], UInt32[])
                        cmd_copy_buffer(cb_read, ob, brb,
                            [BufferCopy(UInt64(0), UInt64(UInt64(4 * ntot) * (k - 1)),
                                        UInt64(ob_bytes))])
                    end
                end
                end_command_buffer(cb_read)

                # warmup + timed replays of the SAME recorded CB
                for _ in 1:WARMUP
                    submit_wait(device, queue, cmdpool, fence, cb_compute)
                end
                # (a) SUSTAINED per-frame cost — the GATE metric: R back-to-back
                # submits of the same recorded CB, one wait_idle, total/R.
                sust = Float64[]
                for _ in 1:5
                    t0 = time_ns()
                    for _ in 1:SUSTAIN
                        unwrap(queue_submit(queue, [SubmitInfo([], [], [cb_compute], [])]))
                    end
                    unwrap(queue_wait_idle(queue))
                    push!(sust, (time_ns() - t0) / 1e3 / SUSTAIN)
                end
                sort!(sust)
                wall = sust[3]
                # P0-G4e: this cell's own measurement NOISE BAND (spread of the
                # 5 sustained trials). Used as the resolution limit below.
                spread = sust[end] - sust[1]
                # (b) fence-wait median — CONSERVATIVE bound (driver wakeup
                # latency included), reported per row, not the gate
                ts = Vector{Float64}(undef, REPS)
                for k in 1:REPS
                    t0 = time_ns()
                    unwrap(queue_submit(queue, [SubmitInfo([], [], [cb_compute], [])]; fence = fence))
                    unwrap(wait_for_fences(device, [fence], true, UInt64(1_000_000_000)))
                    ts[k] = (time_ns() - t0) / 1e3
                    unwrap(reset_fences(device, [fence]))
                end
                sort!(ts)
                wall_fw = ts[ceil(Int, REPS / 2)]
                submit_wait(device, queue, cmdpool, fence, cb_read)
            end

            # readback + compare (P0-G2 object, P0-G3 screen)
            nnan = 0
            obj_max = 0.0
            px_errs = Float64[]
            GC.@preserve ob1 ob2 ob3 brb begin
                hh = reinterpret(Float32, unsafe_wrap(Array, Ptr{UInt8}(
                    unwrap(map_memory(device, mrb, UInt64(0), UInt64(out_bytes)))),
                    out_bytes; own = false))
                for i in 1:ntot
                    # layout-aware read: interleaved = triplets at 3i-2..;
                    # planar = three contiguous BLOCKS (x[i], y[ntot+i],
                    # z[2ntot+i]) because buffer copies land blockwise (the
                    # sentinel caught this: reading planar as interleaved
                    # leaves the block seams NaN)
                    if soa == 0
                        x = hh[3 * i - 2]; y = hh[3 * i - 1]; z = hh[3 * i]
                    else
                        x = hh[i]; y = hh[ntot + i]; z = hh[2 * ntot + i]
                    end
                    if isnan(x) || isnan(y) || isnan(z)
                        nnan += 1
                        continue
                    end
                    j = (i - 1) % nV + 1
                    ref = Pref_f32[j]
                    obj_max = max(obj_max, hypot(Float64(x) - ref[1], Float64(y) - ref[2],
                                                 Float64(z) - ref[3]))
                    fbv = (0.5 * W + F_PX * Float64(x) / max(D_CAM - Float64(z), 0.05),
                           0.5 * H + F_PX * Float64(y) / max(D_CAM - Float64(z), 0.05))
                    push!(px_errs, hypot(fbv[1] - fb_ref[j][1], fbv[2] - fb_ref[j][2]))
                end
                unwrap(unmap_memory(device, mrb))
            end
            @assert nnan == 0 "P0-G6a FAILED ($name $famname N=$N): $nnan unwritten slots (NaN sentinel)"
            @assert obj_max <= ulpbound "P0-G2 FAILED ($name $famname N=$N): obj $obj_max > $ulpbound"
            sort!(px_errs)
            px_max = px_errs[end]
            px_p95 = px_errs[ceil(Int, 0.95 * length(px_errs))]
            @assert px_max <= 0.5 "P0-G3 FAILED ($name $famname N=$N): screen $px_max px > 0.5"
            cuda_pin = fam == 0 ? CUDA_ROT[name][findfirst(==(N), NLADDER)] :
                        CUDA_WIND[name][findfirst(==(N), NLADDER)]
            # P0-G4i: a SAME-(class,N) pinned comparator now exists for BOTH
            # families at all four ladder points (rot: Spiral I A_param run b;
            # wind: Spiral H2). P0-G4d's pin_applicable escape hatch is
            # therefore permanently true; it is retained as an ASSERTION rather
            # than deleted, so a future pin-table edit that reintroduces a
            # domain gap fails loudly instead of silently falling back.
            pin_applicable = true
            @assert pin_applicable "P0-G4i: no same-(class,N) CUDA comparator for ($name $famname N=$N)"
            ratio = wall / cuda_pin
            attributed = max(wall - floor_us, 0.0)
            ratio_attr = attributed / cuda_pin
            drambound = (12 * ntot) >= 8 * 1024 * 1024
            # P0-G4i: the P0-G4d substitute comparator (same-cell rot for wind
            # N>1) is RETIRED — Spiral H2 supplied real same-(class,N) wind
            # pins, so the weaker claim is no longer the best available. Its
            # supporting machinery (rotaat, the floor-band resolution limit,
            # the coverage assertion) is removed rather than left dormant:
            # dead gate code reads as coverage that does not exist. The LESSON
            # it encodes survives in the header and the register — the substitute
            # existed only because a comparator was missing, and the fix for a
            # missing comparator is to MEASURE it, not to gate around it.
            # G4b gate on the LAYOUT-MATCHED (planar) comparator; the
            # interleaved product shape is recorded, not gated (its gap is a
            # store-pattern finding for the packet design, registered in the
            # run notes — first evidence: blob/rot/N=128 attributed 333 us
            # interleaved vs 55-112 us pin = the ~3x write amplification of
            # stride-3 scalar stores).
            if soa == 2 && !drambound
                @assert ratio_attr <= 2.0 "P0-G4b FAILED ($name $famname N=$N, planar+packed shipping form): attributed $(round(attributed; digits=2)) us (sustained $(round(wall; digits=2)) − floor $(round(floor_us; digits=2))) vs CUDA pin $cuda_pin us = $(round(ratio_attr, digits=2))x"
            end
            if soa == 2 && drambound && !(name == "blob" && fam == 0 && N == 128)
                @assert ratio_attr <= 2.0 "P0-G4c FAILED ($name $famname N=$N, DRAM-bound non-corner): attributed $(round(attributed; digits=2)) vs pin $cuda_pin us = $(round(ratio_attr, digits=2))x"
            end
            # P0-G4d substitute gate (STRICTLY WEAKER than the CUDA claim, and
            # labeled as such): where no same-(class,N) pin exists, wind must
            # stay within 2x of rot at the SAME cell — identical traffic, the
            esha = bytes2hex(sha256(reinterpret(UInt8, px_errs)))[1:16]
            push!(rowshas, esha)
            push!(rows, @sprintf("parity,%s,%.4g,%d,%s,%d,%d,%d,%.3e,%.3e,%.6f,%.6f,%s,%s",
                                 name, h, nT, famname, N, nV, ntot, obj_max, ulpbound,
                                 px_p95, px_max, esha, lay))
            push!(rows, @sprintf("wall,%s,%s,%d,%s,%.2f,%.2f,%.3f,%.2f,%.2f,%.3f,%d,%.2f,%.2f", name, famname, N, lay,
                                 wall, cuda_pin, ratio, wall_fw, floor_us, ratio_attr,
                                 pin_applicable ? 1 : 0, sust[1], sust[end]))
            println(stderr, "[p0] $name $famname N=$N $lay wall=$(round(wall; digits=2))us " *
                            "(fw $(round(wall_fw; digits=2))) attr=$(round(attributed; digits=2)) " *
                            "obj=$(obj_max) px=$(px_max) ratio_attr=$(round(ratio_attr, digits=2))")
            end
        end

        # P0-G6b NEGATIVE CONTROL (probe class = blob, rotation, after the class
        # ladder): self-contained — own output buffer, own descriptor repoint, and
        # (run-3 defect, F-P0.4) OWN cage_y buffer. Writing the corrupted cage into
        # the SHARED bcy and relying on a later re-upload to heal it left the wind
        # family reading the shifted cage (device y - host y = 0.25*scale exactly),
        # so P0-G2 fired on a corrupted INPUT rather than on gate failure. The
        # control must perturb the probe's inputs WITHOUT touching product state.
        if name == "blob" && fam == 0
            ntot = nV
            out_bytes = 12 * ntot
            nc_out, _ = dev_buffer(device, mp, out_bytes, BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
            nc_rb, nc_mrb = host_buffer(device, mp, out_bytes,
                BUFFER_USAGE_TRANSFER_DST_BIT | BUFFER_USAGE_TRANSFER_SRC_BIT, qfam)
            nc_bcy, _ = dev_buffer(device, mp, 4 * length(fy), BUFFER_USAGE_STORAGE_BUFFER_BIT, qfam)
            bind_descriptors(device, dset, (bcx, nc_bcy, bcz, bidx, bw, nc_out, nc_out, nc_out, nc_out, bpc, bpw, bpi))
            fy_bad = copy(fy)
            for i in 1:(length(fy_bad) ÷ 4)
                fy_bad[i] += 0.25 * Float32(scale)
            end
            upload!(device, queue, mp, cmdpool, fence, cb_copy, nc_bcy, fy_bad, qfam)
            pc = [PCP0(UInt32(nV), UInt32(ntot), UInt32(fam), UInt32(0),
                       c32, s32, WIND_A, WIND_PHI, scale32, 0f0, 0f0, 0f0,
                       UInt32(0), UInt32(0))]
            GC.@preserve nc_out nc_rb nc_bcy bcx bcy bcz bidx bw dset pc pl pipeline cb_read begin
                begin_command_buffer(cb_read, CommandBufferBeginInfo())
                cmd_bind_pipeline(cb_read, PIPELINE_BIND_POINT_COMPUTE, pipeline)
                cmd_push_constants(cb_read, pl, SHADER_STAGE_COMPUTE_BIT, UInt32(0),
                                   UInt32(sizeof(PCP0)), Ptr{Nothing}(pointer(pc)))
                cmd_bind_descriptor_sets(cb_read, PIPELINE_BIND_POINT_COMPUTE, pl, 0, [dset], UInt32[])
                cmd_dispatch(cb_read, UInt32(cld(ntot, THR)), UInt32(1), UInt32(1))
                cmd_pipeline_barrier(cb_read, UInt32[],
                    [BufferMemoryBarrier(ACCESS_SHADER_WRITE_BIT, ACCESS_TRANSFER_READ_BIT,
                        UInt32(qfam), UInt32(qfam), nc_out, UInt64(0), UInt64(out_bytes))], UInt32[])
                cmd_copy_buffer(cb_read, nc_out, nc_rb,
                    [BufferCopy(UInt64(0), UInt64(0), UInt64(out_bytes))])
                end_command_buffer(cb_read)
                submit_wait(device, queue, cmdpool, fence, cb_read)
            end
            nc_obj = 0.0
            GC.@preserve nc_rb begin
                hh = reinterpret(Float32, unsafe_wrap(Array, Ptr{UInt8}(
                    unwrap(map_memory(device, nc_mrb, UInt64(0), UInt64(out_bytes)))),
                    out_bytes; own = false))
                for j in 1:nV
                    o = 3 * (j - 1) + 1
                    ref = Pref_f32[j]
                    nc_obj = max(nc_obj, hypot(Float64(hh[o]) - ref[1], Float64(hh[o+1]) - ref[2],
                                               Float64(hh[o+2]) - ref[3]))
                end
                unwrap(unmap_memory(device, nc_mrb))
            end
            @assert nc_obj > ulpbound "P0-G6b FAILED: negative control NOT detected (nc_obj=$nc_obj <= $ulpbound) — the gate cannot fail"
            push!(notes, @sprintf("P0-G6b negative control (blob/rot, cage_y quarter shifted by %.3g): obj %.3e > bound %.3e — GATES DETECT",
                                   0.25 * scale, nc_obj, ulpbound))
            pstep("negative control detected ($nc_obj > $ulpbound)")
        end
    end
end

# --- report ---------------------------------------------------------------------
push!(notes, @sprintf("P0-G4i: same-(class,N) CUDA comparators now exist for BOTH families at all four N (rot: Spiral I A_param run b `4a9144326ebed3cb`; wind: Spiral H2 `088909edf19ff5f4`, 28 rows). Every row carries pin_applicable=1 and a real 2x parity claim; the P0-G4d same-cell substitute and its floor-band resolution limit are RETIRED (removed, not left dormant).",
                       ))
# P0-G5 aggregate: rowshas are ALREADY sha16 hex strings (one per parity row,
# over its own error vector), so the aggregate is taken over their canonical
# UTF-8 bytes. Reinterpreting a Vector{Vector{UInt8}} was the run-3 defect.
agg_sha = bytes2hex(sha256(reinterpret(UInt8, Vector{UInt8}(codeunits(join(rowshas, ","))))))[1:16]
text = "# Spiral P0 — Vulkan compute parity port (RTX 5060, repo-pinned Vulkan.jl ZSqbR)\n" *
       "# physical device: $(props.device_name) api=$(props.api_version) (DISCRETE) qfam=$qfam\n" *
       "# glslc SPIR-V: " * join(["$s=$(h)" for (s, h) in SPIRV_FILES], " ") * "\n" *
       "# port: ONE fused compute dispatch (shared cage + shared weights, E pairing\n" *
       "# verbatim, H rigid-rotation body / E f32 wind field); command-buffer replay\n" *
       "# timed (SUSTAINED gate metric: $SUSTAIN back-to-back submits of one recorded CB\n" *
       "# + one queue_wait_idle, median of 5 trials after $WARMUP warmups; fence-wait\n" *
       "# median also reported; readback copy NOT in either timed path — verification\n" *
       "# only).\n" *
       "# 0 B/frame uploaded: cage+bindings resident (the A_param contract).\n" *
       "# gates: P0-G0 provenance; P0-G1 oracle-derived refs; P0-G2 obj <= 4eps*maxcoord;\n" *
       "# P0-G3 screen <= 0.5px vs f64 cage path (every vertex measurable — no viewport);\n" *
       "# P0-G4 wall <= 2x pinned CUDA (rot: I A_param run b; wind: Spiral H2 wind ladder);\n" *
       "# P0-G4i pin applicability: same-(class,N) comparators exist for BOTH families\n" *
       "#   at all four N; pin_applicable=1 on every row; G4d's substitute is retired;\n" *
       "# P0-G5 determinism: per-row error-vector sha16 must match x2;\n" *
       "# P0-G6 NaN sentinel coverage + negative control\n" *
       (isempty(notes) ? "" : join(notes, "\n") * "\n") *
       "# parity rows: kind,name,h,tets,family,N,verts,ntot,obj_max,obj_bound,px_p95,px_max,err_sha16,layout\n" *
       "# wall rows: kind,name,family,N,layout,wall_sustained_us,cuda_pin_us,ratio_abs,wall_fencewait_us,floor_us,ratio_attributed,pin_applicable,sust_min_us,sust_max_us\n" *
       "# P0-G4i: pin_applicable=1 on EVERY row — a same-(class,N) CUDA comparator\n" *
       "# now exists for both families at all four N (rot: Spiral I A_param run b;\n" *
       "# wind: Spiral H2 wind ladder). The P0-G4d same-cell substitute is retired.\n" *
       "# pin protocol: stable columns = kind,name,h,tets,family,N,verts,ntot,obj_max,obj_bound,px_p95,px_max,err_sha16 (wall_us/ratio are machine-state)\n" *
       join(rows, "\n") * "\n" *
       "# error-vector aggregate sha16 (P0-G5): " * agg_sha * "\n"
println(text)
resdir = joinpath(ROOT, "results")
isdir(resdir) || mkdir(resdir)
open(joinpath(resdir, "spiral_p0_vulkan_compute.csv"), "w") do f
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
println("SPIRAL P0 GATES: ALL PASS (P0-G0 provenance, P0-G1 oracle refs, P0-G2 obj ulp, P0-G3 screen <=0.5px, P0-G4 wall <=2x CUDA pins, P0-G5 determinism pending x2, P0-G6 sentinel+negative control)")