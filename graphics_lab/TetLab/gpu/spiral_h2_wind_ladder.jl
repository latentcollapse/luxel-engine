# Spiral H2 — CUDA wind N-ladder: the comparator P0 was missing.
#
# WHY (this is BENCHMARK COMPLETENESS, not new optimization): the P0 Vulkan
# port gates its wind family against CUDA_WIND_N1, which is an N=1 measurement
# (spiral_h_fusion.jl has no N-ladder — it loops families only). P0-G4d had to
# fall back to a strictly weaker same-cell substitute for wind N>1 because no
# same-(class,N) comparator existed. This driver produces those pins. It
# optimizes NOTHING: it is the existing E-validated wind body, batched.
#
# WHAT IS PORTED, AND FROM WHERE (the arithmetic is transcribed, not authored):
#   * the WIND FIELD body is VERBATIM from spiral_h_fusion.jl's
#     fused_deform_reconstruct! (fam == 1 branch), which is itself verbatim
#     from spiral_e_gpu.jl's deform_corners! (E-validated). The same expression
#     the P0 GLSL shader evaluates, so the comparator computes the same
#     function the port does:
#         t = clamp(y/s, 0, 1); lean = A*s*t*t*sin(phi + 2.1x/s + 1.3z/s)
#   * the RECONSTRUCT pairing is VERBATIM from H's reconstruct!:
#     out = w2*q_b + w3*q_c + w4*q_d + w1*q_a (F-E.2/F-E.3 lesson: numeric
#     conventions are read off the authority, never patched from memory).
#   * the BATCHING is Spiral I's A_param contract (fused_batched_shared!):
#     one dispatch over ntot = N*nV, instance g = (i−1) ÷ nV, SHARED resident
#     cage + weights, output written at 3*(g*nV + j) — i.e. exactly the shape
#     P0 ports, so the two pin families describe the same work.
#   * the TIMING METHOD is Spiral I's (capture -> instantiate -> launch,
#     wall_us = median of 25 reps, REPS/WARMUP/thr identical), NOT H's timing.
#     This is deliberate: the rot comparator pins come from Spiral I, so the
#     wind pins must be produced by the same protocol or the two families are
#     not commensurable (the F-P0.6 lesson, applied in the other direction —
#     do not mix measurement protocols inside one comparator table).
#
# GATES (registered BEFORE the run):
#   HF2-G0 provenance: pinned tet counts per class (Spiral I/F pins), and the
#      harness self-check (bary/weights/flatten agreement to 1e-9*scale) on
#      every class before any timing.
#   HF2-G1 correctness: at EVERY (class, N), the batched fused output must be
#      within 4*eps(f32)*maxcoord of the host f32 wind path (E-G2/HF-G1 scale).
#      VIOLATION = the comparator computes something other than the ported
#      function, which would make every downstream pin meaningless. The FIRST
#      and LAST instance are both checked (spot-checking the last asserts
#      instance-independence instead of assuming it).
#   HF2-G2 determinism: stable columns (kind..N..ident_max) byte-identical
#      across two fresh processes; us_* are machine-state.
#
# OUTPUT: results/spiral_h2_wind_ladder.csv, per (class, N): wall_us, dev_us,
#     ident_max — the wall_us column is what P0 pins against.

using CUDA
using Printf
using SHA
using Statistics

include("../oracle/TetDeform.jl"); using .TetDeform
using .TetDeform.TetCage
include("../oracle/TetCorpus.jl"); using .TetCorpus
include("../oracle/TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)
const REPS = 25
const WARMUP = 5
const NLADDER = (1, 8, 32, 128)
const WIND_A = 0.2f0
const WIND_PHI = 0.7f0
const THR = 256

mesh_scale(V) = max(maximum(v -> v[1], V) - minimum(v -> v[1], V),
                    maximum(v -> v[2], V) - minimum(v -> v[2], V),
                    maximum(v -> v[3], V) - minimum(v -> v[3], V))

# --- the comparator kernel -----------------------------------------------------
# Batched form of H's fused_deform_reconstruct!, wind branch only, with
# Spiral I's instance indexing. Both parents are cited above; the two
# transcriptions are the ONLY edits (batching + family restriction).
function fused_batched_wind!(px, py, pz, cx, cy, cz, idx, w, nV, ntot, s, A, phi)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if i <= ntot
        g = (i - 1) ÷ nV
        j = i - g * nV
        i4 = 4 * (j - 1)
        a = idx[i4+1]; b = idx[i4+2]; cc = idx[i4+3]; d = idx[i4+4]
        w1 = w[i4+1]; w2 = w[i4+2]; w3 = w[i4+3]; w4 = w[i4+4]
        ax = cx[a]; ay = cy[a]; az = cz[a]
        bx = cx[b]; by = cy[b]; bz = cz[b]
        ex = cx[cc]; ey = cy[cc]; ez = cz[cc]
        fx2 = cx[d]; fy2 = cy[d]; fz2 = cz[d]
        # wind body, VERBATIM from H fused_deform_reconstruct! fam==1
        t = clamp(ay / s, 0f0, 1f0)
        ax = ax + A * s * t * t * sin(phi + 2.1f0 * ax / s + 1.3f0 * az / s)
        t = clamp(by / s, 0f0, 1f0)
        bx = bx + A * s * t * t * sin(phi + 2.1f0 * bx / s + 1.3f0 * bz / s)
        t = clamp(ey / s, 0f0, 1f0)
        ex = ex + A * s * t * t * sin(phi + 2.1f0 * ex / s + 1.3f0 * ez / s)
        t = clamp(fy2 / s, 0f0, 1f0)
        fx2 = fx2 + A * s * t * t * sin(phi + 2.1f0 * fx2 / s + 1.3f0 * fz2 / s)
        # reconstruct pairing, VERBATIM from H reconstruct!
        px[i] = w2 * bx + w3 * ex + w4 * fx2 + w1 * ax
        py[i] = w2 * by + w3 * ey + w4 * fy2 + w1 * ay
        pz[i] = w2 * bz + w3 * ez + w4 * fz2 + w1 * az
    end
    return nothing
end

# --- host machinery (VERBATIM from spiral_h_fusion.jl; debt per F-D.3) --------

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
    ffx = Vector{Float64}(undef, nc4); ffy = Vector{Float64}(undef, nc4); ffz = Vector{Float64}(undef, nc4)
    for (ti, t) in enumerate(cage.tets)
        for s in 1:4
            c = 4 * (ti - 1) + s
            fx[c] = Float32(t.v[s][1]); fy[c] = Float32(t.v[s][2]); fz[c] = Float32(t.v[s][3])
            ffx[c] = t.v[s][1]; ffy[c] = t.v[s][2]; ffz[c] = t.v[s][3]
        end
    end
    return fx, fy, fz, ffx, ffy, ffz
end

# host f32 wind path, VERBATIM from spiral_h_fusion.jl (fam == 1 branch)
function cpu_f32_wind(V, cage, loc, A, phi, scale)
    fx, fy, fz = flatten_corners(cage)[1:3]
    n = length(fx)
    ox = Vector{Float32}(undef, n); oy = Vector{Float32}(undef, n); oz = Vector{Float32}(undef, n)
    for i in 1:n
        x = fx[i]; y = fy[i]; z = fz[i]
        t = clamp(y / scale, 0f0, 1f0)
        lean = A * scale * t * t * sin(phi + 2.1f0 * x / scale + 1.3f0 * z / scale)
        ox[i] = x + lean; oy[i] = y; oz[i] = z
    end
    idx, w = weights_and_indices(V, cage, loc)[1:2]
    nv = length(V)
    P = Vector{NTuple{3,Float64}}(undef, nv)
    for pi in 1:nv
        i4 = 4 * (pi - 1)
        a = idx[i4+1]; b = idx[i4+2]; c = idx[i4+3]; d = idx[i4+4]
        w1 = w[i4+1]; w2 = w[i4+2]; w3 = w[i4+3]; w4 = w[i4+4]
        P[pi] = (Float64(w2 * ox[b] + w3 * ox[c] + w4 * ox[d] + w1 * ox[a]),
                 Float64(w2 * oy[b] + w3 * oy[c] + w4 * oy[d] + w1 * oy[a]),
                 Float64(w2 * oz[b] + w3 * oz[c] + w4 * oz[d] + w1 * oz[a]))
    end
    return P
end

# --- timing (Spiral I protocol, VERBATIM) -------------------------------------

function wall_us(f!, reps)
    ts = Vector{Float64}(undef, reps)
    for k in 1:reps
        ts[k] = @elapsed begin
            f!()
            CUDA.synchronize()
        end
    end
    return median(ts) * 1e6
end

function dev_us(f!, reps)
    ts = Vector{Float64}(undef, reps)
    for k in 1:reps
        ts[k] = CUDA.@elapsed f!()
    end
    return median(ts) * 1e6
end

const CASES = [("blob", 0.28, 1100), ("teapot", 0.3, 500), ("icosphere2", 0.3, 906),
               ("robot", 0.13, 435), ("conifer", 0.6, 226), ("plate", 0.3, 96),
               ("grass", 0.16, 93)]

rows = String[]
notes = String[]
push!(notes, "device: $(CUDA.name(CUDA.device())) cap $(CUDA.capability(CUDA.device()))")
push!(notes, "job: close the P0 comparator gap — the CUDA wind family measured across the same N-ladder {1,8,32,128} the Vulkan port runs, so P0-G4 can assert a same-(class,N) 2x claim for wind instead of the weaker same-cell substitute")
push!(notes, "kernel: H's fused_deform_reconstruct! wind branch, batched per Spiral I's A_param contract (shared resident cage + weights, one dispatch over ntot=N*nV, 0 B/frame)")
push!(notes, "timing: Spiral I protocol (capture->instantiate->launch, wall_us median of $REPS after $WARMUP warmups, thr=$THR) — deliberately NOT H's timing, so both pin families share one measurement protocol")
push!(notes, "params: A=$WIND_A phi=$WIND_PHI (H's wind constants, identical to the P0 driver's)")

for (name, h, tets_exp) in CASES
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    V, T = m.V, m.T
    scale = mesh_scale(V)
    gr = auto_grid(V; h = h)
    cage = build_cage(V, T; origin = gr.origin, h = h, dims = gr.dims)
    loc, misses = locate_tet(V, cage)
    @assert misses == 0 "HF2-G0 FAILED: locate miss on $name"
    nT = length(cage.tets)
    @assert nT == tets_exp "HF2-G0 FAILED: $name tets $nT != pin $tets_exp"
    nV = length(V)

    idx, w, w64 = weights_and_indices(V, cage, loc)
    fx, fy, fz, ffx, ffy, ffz = flatten_corners(cage)

    # HF2-G0 harness self-check (H's f64 identity construction, verbatim):
    # the weights must reconstruct the input vertices through the cage
    # BEFORE any timing, so a gate failure below is the kernel, not the cage.
    P_chk = Vector{NTuple{3,Float64}}(undef, nV)
    for (pi, _) in enumerate(V)
        i4 = 4 * (pi - 1)
        a = idx[i4+1]; b = idx[i4+2]; c = idx[i4+3]; d = idx[i4+4]
        w1 = w64[i4+1]; w2 = w64[i4+2]; w3 = w64[i4+3]; w4 = w64[i4+4]
        P_chk[pi] = (w2 * ffx[b] + w3 * ffx[c] + w4 * ffx[d] + w1 * ffx[a],
                     w2 * ffy[b] + w3 * ffy[c] + w4 * ffy[d] + w1 * ffy[a],
                     w2 * ffz[b] + w3 * ffz[c] + w4 * ffz[d] + w1 * ffz[a])
    end
    chk = maximum(hypot(p[1] - v[1], p[2] - v[2], p[3] - v[3]) for (p, v) in zip(P_chk, V))
    @assert chk <= 1e-9 * scale "HF2-G0 FAILED on $name: self-check $chk"
    println(stderr, "[h2] $name tets=$nT verts=$nV self=$chk")

    dcx = CuArray(fx); dcy = CuArray(fy); dcz = CuArray(fz)
    didx = CuArray(idx); dw = CuArray(w)
    maxcoord = max(maximum(maximum(abs.(p)) for p in V),
                   maximum(maximum(abs.(t.v[s])) for t in cage.tets for s in 1:4))
    ulpbound = 4 * eps(Float32) * maxcoord
    Pcpu = cpu_f32_wind(V, cage, loc, WIND_A, WIND_PHI, scale)

    for N in NLADDER
        ntot = N * nV
        dpx = CuArray(zeros(Float32, ntot)); dpy = CuArray(zeros(Float32, ntot))
        dpz = CuArray(zeros(Float32, ntot))
        W! = () -> @cuda threads=THR blocks=cld(ntot, THR) fused_batched_wind!(
            dpx, dpy, dpz, dcx, dcy, dcz, didx, dw, nV, ntot,
            Float32(scale), WIND_A, WIND_PHI)
        for _ in 1:WARMUP
            W!()
        end
        CUDA.synchronize()
        gW = capture(W!); execW = instantiate(gW)
        W_go! = () -> launch(execW)
        W_go!(); CUDA.synchronize()

        hx = Array(dpx); hy = Array(dpy); hz = Array(dpz)
        # HF2-G1: every instance, first AND last vertex of each
        ident = 0.0
        for j in 1:nV
            for g in (0, N - 1)
                i = g * nV + j
                ref = Pcpu[j]
                ident = max(ident, abs(Float64(hx[i]) - ref[1]),
                                abs(Float64(hy[i]) - ref[2]),
                                abs(Float64(hz[i]) - ref[3]))
            end
        end
        @assert ident <= ulpbound "HF2-G1 FAILED ($name wind N=$N): identity $ident > $ulpbound — the comparator is not computing the ported function"
        usW = wall_us(W_go!, REPS)
        usWd = dev_us(W_go!, REPS)
        push!(rows, @sprintf("ladder,wind,%s,%.4g,%d,%d,%d,%d,%d,%.2f,%.2f,%.3e",
                             name, h, nT, nV, N, ntot, REPS, usW, usWd, ident))
        println(stderr, "[h2] $name wind N=$N ntot=$ntot wall=$(round(usW; digits=2))us " *
                        "dev=$(round(usWd; digits=2))us ident=$ident")
    end
end

text = "# Spiral H2 — CUDA wind N-ladder (RTX 5060)\n" *
       "# $(notes[1])\n" *
       join(map(n -> "# " * n * "\n", notes[2:end]), "") *
       "# gates: HF2-G0 provenance + harness self-check; HF2-G1 identity vs host f32 wind path at every (class,N), bound 4*eps(f32)*maxcoord; HF2-G2 stable columns byte-identical x2\n" *
       "# ladder rows: kind,family,name,h,tets,verts,N,ntot,reps,wall_us,dev_us,ident_max\n" *
       "# pin protocol: stable columns = kind..reps + ident_max (us_* are machine-state)\n" *
       join(rows, "\n") * "\n"
resdir = joinpath(ROOT, "results")
isdir(resdir) || mkdir(resdir)
open(joinpath(resdir, "spiral_h2_wind_ladder.csv"), "w") do f
    write(f, text)
end
println(text)
println("# csv sha256: ", bytes2hex(sha256(text)))
println("SPIRAL H2 GATES: HF2-G0 provenance OK; HF2-G1 identity OK at every (class,N); HF2-G2 determinism pending x2")