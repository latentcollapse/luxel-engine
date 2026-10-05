# Spiral D2 — adaptive-refinement potential gate (Goal 2 §3).
#
# Question: can LOCAL refinement (adding tets only where fidelity requires)
# beat uniform refinement on the quality/memory Pareto frontier?
#
# Cheapest decisive upper bound: the current cage path routes UNCAGED
# vertices (orphans — vertices whose voxel has no retained tet) through the
# GT path (direct deformation). A hybrid that refits each orphan to its
# truly NEAREST retained tet (exact barycentric, no rebuild) is a strict
# upper bound on what any local-refinement policy can extract from the
# SAME tet count — refinement policies can only approach this by rebuilding
# locally. If the hybrid does not move the frontier, adaptive refinement
# cannot earn its complexity (gate: KEEP only if frontier improves).
#
# Falsifiable prediction (derived): fold-family error is spatially
# concentrated (p95 ≈ 0, max ≫ rmse — D1 sweep) → refitting orphans near
# the fold band should recover a measurable slice of error at ZERO extra
# tets. If even the upper bound is ≪ uniform-refinement gains per tet,
# REJECT adaptive refinement for WGE.
using Printf
using SHA

include("TetDeform.jl"); using .TetDeform
using .TetDeform.TetCage
include("TetCorpus.jl"); using .TetCorpus
include("TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)

# nearest retained tet by centroid distance (deterministic: strict < keeps
# the lowest index on ties)
function nearest_tet_index(p, cents)
    best = Inf
    bi = 0
    for (ti, c) in enumerate(cents)
        d = (p[1]-c[1])^2 + (p[2]-c[2])^2 + (p[3]-c[3])^2
        d < best && (best = d; bi = ti)
    end
    return bi
end

tet_centroids(cage) = [((t.v[1][1]+t.v[2][1]+t.v[3][1]+t.v[4][1])/4,
                        (t.v[1][2]+t.v[2][2]+t.v[3][2]+t.v[4][2])/4,
                        (t.v[1][3]+t.v[2][3]+t.v[3][3]+t.v[4][3])/4) for t in cage.tets]

# refit orphan vertices to their nearest retained tet
function refit_orphans!(loc, V, cage)
    cents = tet_centroids(cage)
    n = 0
    for (pi, p) in enumerate(V)
        loc[pi] == 0 || continue
        bi = nearest_tet_index(p, cents)
        bi != 0 || continue
        loc[pi] = bi
        n += 1
    end
    return n
end

# direct nearest-tet for ALL vertices (geometric upper bound)
function nearest_tet_all(V, cage)
    cents = tet_centroids(cage)
    return [nearest_tet_index(p, cents) for p in V]
end

function probe(name, V, T, h; fields)
    io = IOBuffer()
    g = auto_grid(V; h = h)
    cage = build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
    loc0, misses0 = locate_tet(V, cage)
    norph = count(==(0), loc0)
    # variant A: current behavior (orphans → GT path)
    # variant B: orphans refit to nearest retained tet (upper bound, same tets)
    locB = copy(loc0)
    refit_orphans!(locB, V, cage)
    # variant C: ALL vertices nearest-tet (geometric upper bound, ignores voxel rule)
    locC = nearest_tet_all(V, cage)
    for (fam, f) in fields
        mA, _ = deformation_error(V, T, cage, loc0, f)
        mB, _ = deformation_error(V, T, cage, locB, f)
        mC, _ = deformation_error(V, T, cage, locC, f)
        @printf(io, "%s,%.4g,%d,%d,%s,%.6e,%.6e,%.6e,%.6e\n", name, h,
                length(cage.tets), norph, fam,
                mA.rmse, mB.rmse, mC.rmse, mC.rmse / max(mA.rmse, 1e-30))
    end
    return String(take!(io))
end

# F-D.3 (D2 blocker, CLOSED): scaled_fields referenced mk_wind/mk_twist/
# mk_fold, which lived only in spiral_d_fidelity.jl → UndefVarError at first
# probe; verbatim driver copies carried the debt until the families were
# single-sourced into TetDeform (consumed below via the module exports).
mesh_scale(V) = max(maximum(v -> v[1], V) - minimum(v -> v[1], V),
                    maximum(v -> v[2], V) - minimum(v -> v[2], V),
                    maximum(v -> v[3], V) - minimum(v -> v[3], V))

# normalize each mesh by its own bbox side for family constants
function scaled_fields(V)
    s = mesh_scale(V)
    return [("wind", mk_wind(s)), ("twist", mk_twist(s)),
            ("fold", mk_fold(s)), ("fold4", mk_fold(s; A = 0.4))]
end

rows = String[]
# coarse-cage operating points where orphans exist (from D1 ladder coarse end)
CASES = [
    ("conifer", 2.4), ("conifer", 1.7), ("conifer", 1.2),
    ("grass", 1.5), ("grass", 1.05),
    ("plate", 2.4), ("plate", 1.7),
    ("teapot", 1.2), ("icosphere2", 1.2), ("robot", 0.75),
]
for (name, h) in CASES
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    V, T = m.V, m.T
    println(stderr, "probing ", name, " h=", h, " …")
    push!(rows, probe(name, V, T, h; fields = scaled_fields(V)))
end

text = "# D2 adaptive upper-bound probe: A=current(orphan->GT) B=orphan-refit C=all-nearest\n" *
       "# name,h,tets,orphans,family,rmse_A,rmse_B,rmse_C,rmse_C/rmse_A\n" *
       join(rows)
println(text)
resdir = joinpath(ROOT, "results")
isdir(resdir) || mkdir(resdir)
open(joinpath(resdir, "spiral_d2_adaptive_probe.csv"), "w") do f
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
