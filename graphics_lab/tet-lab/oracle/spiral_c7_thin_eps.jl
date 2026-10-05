# Spiral C7 — thin-foliage ε sweep (goal §2).
#
# Question: should clipping ε be GLOBAL, ASSET-CLASS-SPECIFIC,
# SCALE-NORMALIZED (ε·h), CAGE-RESOLUTION-DEPENDENT, or is generalization
# UNSAFE? The sphere (Turn C) measured gap_max ≈ 11.4·ε at h=0.22, i.e.
# gap_max/(ε·h) ≈ 52 on closed smooth geometry. If that ratio is INVARIANT
# across mesh class AND cage resolution, ε·h normalization is the policy;
# if thin/open geometry shifts it materially, policy must be asset-class-
# specific; if it varies with h at fixed mesh, cage-resolution-dependent.
#
# Meshes: sphere icosphere5 (closed control, Turn C re-run), plate (single
# thin card), grass (12 crossed blades), conifer (WGE card-grid proxy).
# ε ∈ {0, 1e-7, 2.5e-6, 1e-5, 1e-4} at f64; f32 spot-check at ε ∈ {0, 2.5e-6}.
using Printf
using SHA

include("TetCage.jl"); using .TetCage
include("TetCageEps.jl"); using .TetCageEps
include("TetCorpus.jl"); using .TetCorpus
include("TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)

# mesh -> (h_list): controls + thin-geometry cage scales
const CONFIG = [
    ("sphere",  :gen, [0.22, 0.11]),
    ("plate",   :obj, [0.40, 0.20]),
    ("grass",   :obj, [0.25, 0.125]),
    ("conifer", :obj, [0.40, 0.20]),
]
EPS_LIST = [0.0, 1e-7, 2.5e-6, 1e-5, 1e-4]

function load(name::String, kind::Symbol)
    kind == :obj || error("use gensphere for :gen")
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    return m.V, m.T
end

function sweep_mesh(name, V, T, h_list, prec)
    io = IOBuffer()
    for h in h_list
        g = TetCage.auto_grid(V; h = h)
        for eps in EPS_LIST
            tets = build_cage_eps(V, T; origin = g.origin, h = h,
                                  dims = g.dims, eps = eps)
            bins = tri_bins(V, T, g.origin, h, g.dims)
            re, an, keys = clip_mesh_eps(V, T, tets, bins;
                                         eps = eps, prec = prec, h = h)
            nk = length(keys)
            gf = nk == 0 ? 0.0 : re.gaps32 / nk
            ratio = eps > 0 ? re.gap_max / (eps * h) : 0.0
            @printf(io, "%s,%d,%.4g,%.3e,%d,%d,%d,%d,%.6e,%.3e,%.6e,%d,%d\n",
                    name, prec, h, eps, re.tet_count, re.tris_out, re.verts_out,
                    re.gaps32, gf, re.gap_max, ratio, re.ulp_max, an)
        end
    end
    return String(take!(io))
end

rows64 = String[]
rows32 = String[]
for (name, kind, h_list) in CONFIG
    if kind == :gen
        V, T = TetCorpus.icosphere(5)
    else
        V, T = load(name, kind)
    end
    println(stderr, "sweeping ", name, " …")
    push!(rows64, sweep_mesh(name, V, T, h_list, 64))
    if name in ("sphere", "plate")          # f32 spot-check: control + thinnest
        push!(rows32, sweep_mesh(name, V, T, h_list, 32))
    end
end

text = "# C7 thin-foliage eps sweep  (f64 mechanism isolation + f32 spot)\n" *
       "# name,prec,h,eps,tets,tris_out,verts_out,gaps,gap_frac,gap_max,gap_max/(eps*h),ulp_max,anomalies\n" *
       join(rows64) * join(rows32)
println(text)
isdir(joinpath(ROOT, "results")) || mkdir(joinpath(ROOT, "results"))  # F-E.9: ROOT-anchored, was CWD-relative
open(joinpath(ROOT, "results", "spiral-c7-thin-eps-sweep.csv"), "w") do f
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
