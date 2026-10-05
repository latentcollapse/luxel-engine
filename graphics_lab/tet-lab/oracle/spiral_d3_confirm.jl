# D3 confirmation probe: measured (not extrapolated) operating points for the
# two wind-bound classes whose 0.5 px@5 budget falls between D1 ladder rungs.
#   grass: ladder min 0.907 px (h=0.19) -> predicted h≈0.14 by the h² law
#   plate: ladder min 1.672 px (h=0.3)  -> predicted h≈0.23 by the h² law
# Registered predictions: grass h=0.14 → px95@5 ∈ [0.35, 0.55]; plate
# h=0.23 → px95@5 ∈ [0.85, 1.30] (wider band: D1 showed wind-plateau aliasing
# on flat classes, so the h² law may UNDERSHOOT the plateau floor there).
# The P4 affine gate must pass at every new (mesh, h).
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

# mk_twist/mk_wind/mk_fold consumed from TetDeform (F-D.3 single source)

rows = String[]
# Round 2 (registered before running): plateau-floor probe BELOW the D1
# ladder minima for the wind-bound flat classes. Prediction: wind-plateau
# holds -> plate h=0.15 wind px95@5 in [0.7, 1.2] (h2 law alone: 0.55);
# grass h=0.12 in [0.35, 0.6]. If h2 wins instead, the plateau story is
# falsified for extrapolation purposes and the surface uses h2 points.
CASES = [("grass", 0.14), ("grass", 0.16), ("grass", 0.12),
         ("plate", 0.23), ("plate", 0.26), ("plate", 0.18), ("plate", 0.15)]
for (name, h) in CASES
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    V, T = m.V, m.T
    scale = mesh_scale(V)
    g = auto_grid(V; h = h)
    cage = build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
    loc, misses = locate_tet(V, cage)
    @assert misses == 0 "locate miss on $name h=$h"
    mr = error_metrics([f_rotz(deg2rad(30))(p) for p in V],
                       cage_path_positions(V, cage, loc, f_rotz(deg2rad(30))), T)
    @assert mr.max <= 1e-9 "P4 FAILED: rotz max=$(mr.max) at $name h=$h"
    nT = length(cage.tets)
    for (fam, f) in (("wind", mk_wind(scale)), ("twist", mk_twist(scale)),
                     ("fold", mk_fold(scale)), ("fold4", mk_fold(scale; A = 0.4)))
        mm, scr = deformation_error(V, T, cage, loc, f)
        push!(rows, @sprintf("%s,%.4g,%d,%.2f,%s,%.6e,%.6e,%.3f,%.3f,%.3f,%.3f,%.3f\n",
                             name, h, nT, length(T) / nT, fam, mm.max, mm.rmse,
                             scr[5.0].mean, scr[5.0].p95,
                             scr[20.0].p95, scr[100.0].p95, mm.nmax_deg))
    end
    println(stderr, "done $name h=$h tets=$nT")
end

text = "# D3 confirmation: wind-bound classes at extrapolated h (grass, plate)\n" *
       "# name,h,tets,tris_per_tet,family,obj_max,obj_rmse,px_mean_d5,px95_d5,px95_d20,px95_d100,nmax_deg\n" *
       join(rows)
println(text)
resdir = joinpath(ROOT, "results")
isdir(resdir) || mkdir(resdir)
open(joinpath(resdir, "spiral-d3-confirm.csv"), "w") do f
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
