# Bench child: ONE measurement in ONE fresh process (cross-process noise is
# what we are taming; see F-OPT.2). Parent (bench2.jl) spawns N of these.
# Usage: julia --startup-file=no bench_child.jl <label> <module_path> <mesh_subdiv> <h>
using Printf

label = ARGS[1]
modpath = ARGS[2]
subdiv = parse(Int, ARGS[3])
h = parse(Float64, ARGS[4])

include(modpath)
using .TetCage
include("TetCorpus.jl"); using .TetCorpus

V, T = TetCorpus.blob(subdiv)
g = TetCage.auto_grid(V; h = h)

# compile warmup (excluded from timing)
cage = TetCage.build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
cr = TetCage.clip_mesh(V, T, cage)
hash_ref = TetCage.cage_sha256(V, T, cage, cr)

# median of 3 in-process reps (function scope: no soft-scope traps)
function measure!(t_build, t_clip, alloc, V, T, g, h, hash_ref)
    for _ in 1:3
        gc0 = Base.gc_bytes()
        t0 = time_ns()
        cage = TetCage.build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
        t1 = time_ns()
        cr = TetCage.clip_mesh(V, T, cage)
        t2 = time_ns()
        gc1 = Base.gc_bytes()
        push!(t_build, (t1 - t0) / 1e6)
        push!(t_clip, (t2 - t1) / 1e6)
        push!(alloc, (gc1 - gc0) / 1e6)
        TetCage.cage_sha256(V, T, cage, cr) == hash_ref || error("hash drift within child")
    end
end
t_build = Float64[]; t_clip = Float64[]; alloc = Float64[]
measure!(t_build, t_clip, alloc, V, T, g, h, hash_ref)
med(v) = (s = sort(v); s[ceil(Int, end/2)])
@printf("RESULT,%s,%.2f,%.2f,%.1f,%s\n", label, med(t_build), med(t_clip), med(alloc), hash_ref[1:16])
