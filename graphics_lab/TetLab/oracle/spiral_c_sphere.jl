# Spiral C — the paper's sphere watertightness experiment, CPU analog.
# [PAPER §4.1]: sphere r=100, ~1M tris, 9x12x9 cage; rays from center;
#   without mitigation ~0.01% escape; with eps=2.5e-6 growth, zero escapes.
# CPU analog [DERIVED]: the escape mechanism is the shared-face vertex gap.
#   We realize it with STORAGE QUANTIZATION (f32, GPU-realistic) and measure
#   gap incidence directly. eps sweeps the paper's mitigation.
#   Escape proxy metric: fraction of shared-face keys whose two tets'
#   independently-stored copies DISAGREE (a ray through the band misses both).
#   Duplicate proxy: tets whose grown envelope accepts triangles ALSO accepted
#   by a neighbor at the same face (overlap = double-hit potential).
using Printf
using SHA

include("TetCage.jl"); using .TetCage
include("TetCageEps.jl"); using .TetCageEps
include("TetCorpus.jl"); using .TetCorpus

# --- the paper's sphere, scaled to oracle grid units ------------------------
# paper: r=100, cage 9x12x9 → voxel h ≈ 100/4.5 ≈ 22; scale whole scene by
# 1/100: r=1, h≈0.22. icosphere5 = 20480 tris ≈ paper's 1M-scale class.
r = 1.0
V, T = TetCorpus.icosphere(5; radius = r)
h = 0.22

function sweep(eps_list, prec)
    io = IOBuffer()
    println(io, "prec,eps,tets,tris_out,verts_out,gaps,gap_frac,gap_max_abs,ulp_max")
    for eps in eps_list
        g = TetCage.auto_grid(V; h = h)
        tets = build_cage_eps(V, T; origin = g.origin, h = h, dims = g.dims, eps = eps)
        # base bins (eps-independent triangle binning)
        bins = tri_bins(V, T, g.origin, h, g.dims)
        re, an, keys = clip_mesh_eps(V, T, tets, bins; eps = eps, prec = prec, h = h)
        nkeys = length(keys)
        gap_frac = nkeys == 0 ? 0.0 : re.gaps32 / nkeys
        @printf(io, "%d,%.3e,%d,%d,%d,%d,%.6e,%.3e,%d\n",
                prec, eps, re.tet_count, re.tris_out, re.verts_out,
                re.gaps32, gap_frac, re.gap_max, re.ulp_max)
    end
    return String(take!(io))
end

eps_list = [0.0, 1e-7, 2.5e-6, 1e-5, 1e-4]
out64 = sweep(eps_list, 64)
out32 = sweep(eps_list, 32)
text = "# Sphere r=1.0, icosphere5 (20480 tris), h=0.22, eps sweep\n" *
       "# f64 = full-precision storage; f32 = GPU-realistic quantization\n" * out64 * out32
println(text)
resroot = joinpath(dirname(@__DIR__), "results")  # F-E.9: ROOT-anchored, was CWD-relative
isdir(resroot) || mkdir(resroot)
open(joinpath(resroot, "spiral_c_eps_sweep.csv"), "w") do f
    write(f, text)
end
hsh = bytes2hex(sha256(text))
println("# csv sha256: ", hsh[1:16])
