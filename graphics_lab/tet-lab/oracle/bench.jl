# TetLab performance bench — timings + allocations + correctness hash.
# Usage: julia --startup-file=no bench.jl [label]
# Every run prints the cage hash so the Critic gate can compare
# baseline vs optimized: TIMES may change, HASHES MUST NOT.
using Printf

include("TetCage.jl"); using .TetCage
include("TetCorpus.jl"); using .TetCorpus

label = isempty(ARGS) ? "run" : ARGS[1]

function bench_workload(name, subdiv, h; reps = 3)
    V, T = TetCorpus.blob(subdiv)
    g = TetCage.auto_grid(V; h = h)
    # warmup (compile)
    cage = TetCage.build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
    cr = TetCage.clip_mesh(V, T, cage)
    hash_ref = TetCage.cage_sha256(V, T, cage, cr)

    t_build = Float64[]; t_clip = Float64[]; allocs = Float64[]
    for _ in 1:reps
        gc0 = Base.gc_bytes()
        t0 = time_ns()
        cage = TetCage.build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
        t1 = time_ns()
        cr = TetCage.clip_mesh(V, T, cage)
        t2 = time_ns()
        gc1 = Base.gc_bytes()
        push!(t_build, (t1 - t0) / 1e6)
        push!(t_clip, (t2 - t1) / 1e6)
        push!(allocs, (gc1 - gc0) / 1e6)   # MB allocated during phase pair
        h_now = TetCage.cage_sha256(V, T, cage, cr)
        h_now == hash_ref || error("NONDETERMINISTIC HASH between reps: $h_now vs $hash_ref")
    end
    median(v) = (s = sort(v); s[ceil(Int, end/2)])
    mb(x) = x / 1e6
    @printf("%s,%s,%d,%d,%.1f,%.2f,%.2f,%.1f,%s\n",
            label, name, length(V), length(T),
            length(T) / max(cr.tet_count, 1),
            median(t_build), median(t_clip), median(allocs),
            hash_ref[1:16])
    return hash_ref
end

println("label,mesh,verts,tris,tris_per_tet,build_ms,clip_ms,alloc_mb,hash16")
h1 = bench_workload("blob5", 5, 0.28)
h2 = bench_workload("blob6", 6, 0.28)
println("# baseline hashes: blob5=", h1[1:16], " blob6=", h2[1:16])
