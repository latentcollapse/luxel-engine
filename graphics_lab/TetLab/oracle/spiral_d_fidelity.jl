# Spiral D — fidelity frontier sweep (Goal 2 §3).
#
# Question: per asset class, what is the coarsest cage whose deformation
# error stays inside an explicit screen-space budget? (D3 decision surface.)
#
# Design (pre-registered):
#   meshes: 7 corpus classes — conifer/grass/plate/blob/teapot/robot/
#           icosphere2. Negative controls are INCLUDED as classes: the
#           frontier must show their infeasibility, not exclude them.
#   h-ladder: 7 steps from 3× to ~24× the mesh edge scale.
#   fields (mesh-normalized coordinates: family constants divide by mesh
#           scale so class size does not masquerade as class behavior):
#     rotz 30°       — affine control: must be EXACT at every h (P4)
#     wind A=0.2     — paper-class bend
#     twist A=0.2    — non-affine rotational
#     fold A=0.2     — sharp-gradient adversarial
#     fold A=0.4     — worst case
#   metrics per (mesh, h, field): object-space max/rmse/p95, normal
#     mean/max deg, screen-space px at 5/20/100 units (f=500 px),
#     tets, tris/tet, preprocessing time.
#
# Predictions (falsifiable):
#   P1 rms ∝ h²  (all classes)
#   P2 normal error ∝ h
#   P3 px error ∝ 1/d
#   P4 rotz exact (≤1e-9) at every h
#   P5 tets ∝ h⁻³
#   P7 temporal: reconstruction is continuous across tet faces → no
#      popping between adjacent h values beyond the trend (checked via
#      monotone h-ladder of px error).
using Printf
using SHA

# Single module identity (C8 lesson): TetDeform embeds its own TetCage copy;
# bind ALL cage names to that nested module — never include TetCage.jl twice.
include("TetDeform.jl"); using .TetDeform
using .TetDeform.TetCage
include("TetCorpus.jl"); using .TetCorpus
include("TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)

# F-D.2 (Critic-caught): radial extent degenerates for spherical meshes
# (icosphere2: all vertices at r≈1 → extent ≈ few ulps → scale ≈ 1e16 →
# normalized families collapsed to identity/affine → meaningless zero rows).
# Normalize by the max AABB side instead — well-defined for every class.
mesh_scale(V) = max(maximum(v -> v[1], V) - minimum(v -> v[1], V),
                    maximum(v -> v[2], V) - minimum(v -> v[2], V),
                    maximum(v -> v[3], V) - minimum(v -> v[3], V))

# mesh-normalized deformation fields (affine gate uses pure f_rotz directly —
# it must remain an exact rotation, no normalization)

# mk_twist/mk_wind/mk_fold were defined HERE (source of truth); F-D.3 closed
# by single-sourcing them into TetDeform — this driver consumes the exports.

function sweep_mesh(name, V, T, h_list)
    io = IOBuffer()
    scale = mesh_scale(V)
    for h in h_list
        g = auto_grid(V; h = h)
        t0 = time()
        cage = build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
        t1 = time()
        loc, misses = locate_tet(V, cage)
        t2 = time()
        nT = length(cage.tets)
        tpt = length(T) / nT
        @assert misses == 0 "locate miss on $name h=$h"
        # P4 affine gate: rotz is a pure rotation, exact through the path
        mr = error_metrics([f_rotz(deg2rad(30))(p) for p in V],
                           cage_path_positions(V, cage, loc, f_rotz(deg2rad(30))), T)
        @assert mr.max <= 1e-9 "P4 FAILED: rotz max=$(mr.max) at $name h=$h"
        for (fam, f) in (("wind", mk_wind(scale)), ("twist", mk_twist(scale)),
                         ("fold", mk_fold(scale)), ("fold4", mk_fold(scale; A = 0.4)))
            m, scr = deformation_error(V, T, cage, loc, f)
            @printf(io, "%s,%.4g,%d,%.2f,%s,%.6e,%.6e,%.6e,%.3f,%.3f,%.3f,%.3f,%.3f,%.3f,%.3f,%.3f,%.1f,%.2f\n",
                    name, h, nT, tpt, fam, m.max, m.rmse, m.p95,
                    m.nmean_deg, m.nmax_deg,
                    scr[5.0].mean, scr[5.0].p95,
                    scr[20.0].mean, scr[20.0].p95,
                    scr[100.0].mean, scr[100.0].p95,
                    (t1 - t0) * 1000, (t2 - t1) * 1000)
        end
    end
    return String(take!(io))
end

# canonical class -> h-ladder (edge-scale multiples 3× to 24×, 7 steps)
const LADDERS = Dict(
    "conifer"    => [2.4, 1.7, 1.2, 0.85, 0.6, 0.42, 0.3],
    "grass"      => [1.5, 1.05, 0.75, 0.5, 0.38, 0.27, 0.19],
    "plate"      => [2.4, 1.7, 1.2, 0.85, 0.6, 0.42, 0.3],
    "blob"       => [1.12, 0.8, 0.56, 0.4, 0.28, 0.2, 0.14],
    "teapot"     => [1.2, 0.85, 0.6, 0.42, 0.3, 0.21, 0.15],
    "robot"      => [0.75, 0.53, 0.38, 0.27, 0.19, 0.13, 0.1],
    "icosphere2" => [1.2, 0.85, 0.6, 0.42, 0.3, 0.21, 0.15],
)

rows = String[]
for name in ("conifer", "grass", "plate", "blob", "teapot", "robot", "icosphere2")
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    V, T = m.V, m.T
    println(stderr, "sweeping ", name, " …")
    push!(rows, sweep_mesh(name, V, T, LADDERS[name]))
end

text = "# Spiral D fidelity sweep — mesh-normalized fields\n" *
       "# name,h,tets,tris_per_tet,family,obj_max,obj_rmse,obj_p95,nmean_deg,nmax_deg,px_mean_d5,px_p95_d5,px_mean_d20,px_p95_d20,px_mean_d100,px_p95_d100,build_ms,locate_ms\n" *
       join(rows)
println(text)
# F-E.9: was CWD-relative ("../results") — escaped the TetLab quarantine
# when run from other cwds; now ROOT-anchored like every other driver.
isdir(joinpath(ROOT, "results")) || mkdir(joinpath(ROOT, "results"))
open(joinpath(ROOT, "results", "spiral_d_fidelity_sweep.csv"), "w") do f
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
