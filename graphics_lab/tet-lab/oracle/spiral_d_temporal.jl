# Spiral D — temporal popping probe (Goal 2 §3, P7 follow-up).
#
# Question: does the cage path ANIMATE smoothly (no frame-to-frame popping),
# and how much of the frame budget does f32 vertex storage consume?
#
# Design (pre-registered):
#   animation: wind family with swept phase phi = 2pi*k/N, N=24 frames,
#     A=0.2 (paper-class bend), cage built ONCE per case — isolates temporal
#     deformation smoothness from LOD switching (T3 is static, read from the
#     pinned D1 CSV h-ladder).
#   T1 smoothness (f64): per-frame max per-vertex displacement of the CAGE
#     path (D_k) vs the same motion applied directly (GT, d_k). A popping
#     reconstruction magnifies motion at some frames. Gate:
#       max_k D_k / max(d_k, 1e-3*scale) <= 1.10   (motion ratio)
#   F-D.4 (run-caught, first draft): the original second gate (px95 max/min
#     over the period <= 1.25) CONFLATES two different phenomena — smooth
#     phase-dependent error translation (legitimate: the wave crest slides
#     across the cage, so px95 ranges over the period by design) and actual
#     popping (frame-to-frame DIScontinuity). blob f64 correctly failed it.
#     Amended gates, registered BEFORE the amended run:
#       adjacent-frame px95 growth  max_k px95(k)/px95(k-1)  <= 1.25 (f64)
#         (N=24 -> 15 deg phase steps; a smooth wave cannot move the error
#          field faster than this, a pop spikes far beyond it)
#       curvature |px95(k+1) - 2*px95(k) + px95(k-1)| <= 0.25 * max px95
#         (a one-frame pop is delta-like and violates grossly; f64 0.25,
#          f32 0.35 / adjacent f64 1.25, f32 1.5)
#   T2 f32 storage model: rebuild the ENTIRE pipeline from f32-rounded
#     vertices (mesh AND cage corners; grid pinned from the f64 mesh so both
#     variants share origin/h/dims). This models GPU f32 vertex buffers.
#     Gate: same gates relaxed to 1.25 / 1.5 (registered BEFORE running).
#   T3 LOD pop (static, from spiral-d-fidelity-sweep.csv): adjacent-h px95
#     delta at d=5 IS the hysteresis budget for LOD switching.
#
# Falsifiable predictions:
#   T1 passes (face-continuous reconstruction -> cage animates like GT).
#   T2 jitter is dominated by representation error (C8: dev_p95 ~ 2.5e-4,
#   chord sagitta), phase-invariant -> frame ratios stay near 1.
using Printf
using SHA

include("TetDeform.jl"); using .TetDeform
using .TetDeform.TetCage
include("TetCorpus.jl"); using .TetCorpus
include("TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)

mesh_scale(V) = max(maximum(v -> v[1], V) - minimum(v -> v[1], V),
                    maximum(v -> v[2], V) - minimum(v -> v[2], V),
                    maximum(v -> v[3], V) - minimum(v -> v[3], V))

# mk_wind_phase consumed from TetDeform (F-D.3 single source; phi exposed)

f32model(V) = [((Float64(Float32(p[1])), Float64(Float32(p[2])),
                 Float64(Float32(p[3])))) for p in V]

function probe(name, V, T, h; prec::Symbol, nframes = 24)
    scale = mesh_scale(V)
    g = auto_grid(V; h = h)                       # grid pinned from f64 mesh
    Vrun = prec === :f32 ? f32model(V) : V
    cage = build_cage(Vrun, T; origin = g.origin, h = h, dims = g.dims)
    loc, misses = locate_tet(Vrun, cage)
    @assert misses == 0 "locate miss on $name $prec h=$h"
    # P4 affine gate per build (rotation exact through the path)
    mr = error_metrics([f_rotz(deg2rad(30))(p) for p in Vrun],
                       cage_path_positions(Vrun, cage, loc, f_rotz(deg2rad(30))), T)
    @assert mr.max <= 1e-9 "P4 FAILED: rotz max=$(mr.max) at $name $prec"

    io = IOBuffer()
    prev_cage = nothing
    prev_gt = nothing
    worst_ratio = 0.0
    worst_frame = 0
    px_series = Float64[]
    for k in 0:nframes
        phi = 2.0 * pi * k / nframes
        f = mk_wind_phase(scale; A = 0.2, phi = phi)
        Pgt = [f(p) for p in Vrun]
        Pc = cage_path_positions(Vrun, cage, loc, f)
        scr = screen_px_error(Pgt, Pc, 5.0)
        push!(px_series, scr.p95)
        if prev_cage !== nothing
            D = 0.0; d = 0.0
            for i in eachindex(Vrun)
                a, b = Pc[i], prev_cage[i]
                D = max(D, sqrt((a[1]-b[1])^2 + (a[2]-b[2])^2 + (a[3]-b[3])^2))
                g1, g0 = Pgt[i], prev_gt[i]
                d = max(d, sqrt((g1[1]-g0[1])^2 + (g1[2]-g0[2])^2 + (g1[3]-g0[3])^2))
            end
            floord = 1e-3 * scale
            ratio = D / max(d, floord)
            @printf(io, "%s,%s,%d,%.6e,%.6e,%.4f,%.3f,%.3f\n", name,
                    String(prec), k, D, d, ratio, scr.p95, scr.max)
            if ratio > worst_ratio
                worst_ratio = ratio
                worst_frame = k
            end
        end
        prev_cage = Pc
        prev_gt = Pgt
    end
    px_max = maximum(px_series)
    adj = [px_series[k] / max(px_series[k-1], 1e-9) for k in 2:length(px_series)]
    admax = maximum(adj)
    curv = maximum(abs(px_series[k+1] - 2*px_series[k] + px_series[k-1])
                   for k in 2:length(px_series)-1)
    return String(take!(io)), worst_ratio, worst_frame, admax, curv, px_max
end

ratio_gate(prec) = prec === :f32 ? 1.25 : 1.10
adj_gate(prec) = prec === :f32 ? 1.5 : 1.25
curv_gate(prec) = prec === :f32 ? 0.35 : 0.25

rows = String[]
verdicts = String[]
CASES = [("blob", 0.28), ("conifer", 0.6)]   # blob5 is procedural; on-disk class file is blob.obj
for (name, h) in CASES
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    V, T = m.V, m.T
    for prec in (:f64, :f32)
        println(stderr, "probing ", name, " h=", h, " prec=", prec, " …")
        r, wr, wf, admax, curv, pxmax = probe(name, V, T, h; prec = prec)
        push!(rows, r)
        ok_r = wr <= ratio_gate(prec)
        ok_a = admax <= adj_gate(prec)
        ok_c = curv <= curv_gate(prec) * pxmax
        push!(verdicts, @sprintf("%s h=%.2f %s: worst_ratio=%.4f (frame %d, gate %.2f) adj_px95=%.3f (gate %.2f) curv/max=%.3f (gate %.2f) px95_max=%.3f -> %s",
                                 name, h, String(prec), wr, wf, ratio_gate(prec),
                                 admax, adj_gate(prec), curv / pxmax,
                                 curv_gate(prec), pxmax,
                                 ok_r && ok_a && ok_c ? "PASS" : "FAIL"))
        @assert ok_r "T-gate FAILED (motion ratio): $name $prec"
        @assert ok_a "T-gate FAILED (adjacent px95 growth): $name $prec"
        @assert ok_c "T-gate FAILED (curvature): $name $prec"
    end
end

text = "# D temporal probe: frame k, cage-path max displacement D, GT max displacement d, motion ratio, px95(d=5), px_max(d=5)\n" *
       "# name,prec,frame,D,d,ratio,px95_d5,px_max_d5\n" * join(rows)
foreach(println, verdicts)
isempty(verdicts) || (text *= join(["# " * v for v in verdicts], "\n") * "\n")
println(text)
resdir = joinpath(ROOT, "results")
isdir(resdir) || mkdir(resdir)
open(joinpath(resdir, "spiral-d-temporal-probe.csv"), "w") do f
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
