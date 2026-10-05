# Probe 3: does the exact-float TE key split ulp-drifted copies of the SAME
# conceptual chord point across tets? (If yes, the Spiral-C gap census
# undercounts TE-class gaps and blob5 vert counts are drift-sensitive.)
include("TetCage.jl"); using .TetCage
include("TetCorpus.jl"); using .TetCorpus
include("TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)
m = read_obj(joinpath(ROOT, "corpus", "blob.obj"))
V, T = m.V, m.T
h = 0.28
g = auto_grid(V; h = h)
cage = build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
cr = clip_mesh(V, T, cage)
println("state: tets=", cr.tet_count, " tris_out=", cr.tris_out, " verts_out=", cr.verts_out)

# collect TE instances per canonical edge: (t_exact, x, tet_id)
function collect_te(cage, V, T)
    by_edge = Dict{Tuple,Vector{Tuple{Float64,NTuple{3,Float64},Int}}}()
    for (tid, t) in enumerate(cage.tets)
        pl = tet_planes(t)
        for ti in get(cage.bins, t.vox, Int[])
            idx = T[ti]
            tri = (V[idx[1]+1], V[idx[2]+1], V[idx[3]+1])
            aabb_overlap(tet_aabb(t), tri_aabb(tri)) || continue
            tri_overlap_tet(pl, t, tri) || continue
            poly = clip_triangle(tri, t, pl, idx)
            for cv in poly
                cv.npl == 2 || continue
                f, g2 = cv.planes[1], cv.planes[2]
                e = setdiff(collect(1:4), [f, g2])
                n1, n2 = t.nodes[e[1]], t.nodes[e[2]]
                E = n1 < n2 ? (n1, n2) : (n2, n1)
                tpar = edge_param(cv.x, t.v[e[1]], t.v[e[2]])
                push!(get!(by_edge, E, Vector{Tuple{Float64,NTuple{3,Float64},Int}}()),
                      (tpar, cv.x, tid))
            end
        end
    end
    return by_edge
end

by_edge = collect_te(cage, V, T)
n_inst = sum(length(v) for v in values(by_edge))
println("TE instances: ", n_inst, " on ", length(by_edge), " canonical edges")

# A) near-duplicate t values across tets (same conceptual point, drifted copy)
function near_dup_stats(by_edge; window = 1e-12)
    split_pairs = 0
    clusters = 0
    for (E, insts) in by_edge
        ts = sort([i[1] for i in insts])
        i = 1
        while i < length(ts)
            if 0 < ts[i+1] - ts[i] < window
                split_pairs += 1
                i += 2
            else
                i += 1
            end
        end
        c = 1
        for j in 2:length(ts)
            ts[j] - ts[j-1] > window && (c += 1)
        end
        clusters += c
    end
    return split_pairs, clusters
end
sp, cl = near_dup_stats(by_edge)
println("A) near-duplicate t pairs (1e-12 window): ", sp)
println("   conceptual chord points (~1e-12 clusters): ", cl)

# B) quantized-t keys vs exact-t keys; false-merge check
function quant_stats(by_edge; QUANT = 1e-9)
    exact_keys = Set{Tuple}()
    quant_keys = Dict{Tuple,Vector{NTuple{3,Float64}}}()
    for (E, insts) in by_edge
        for (tpar, x, tid) in insts
            push!(exact_keys, (E, tpar))
            push!(get!(quant_keys, (E, round(tpar / QUANT)),
                       Vector{NTuple{3,Float64}}()), x)
        end
    end
    worst = 0.0
    merged = 0
    for (k, xs) in quant_keys
        length(xs) > 1 && (merged += 1)
        length(xs) == 1 && continue
        for a in xs, b in xs
            d = sqrt(sum(abs2, a .- b))
            d > worst && (worst = d)
        end
    end
    return length(exact_keys), length(quant_keys), merged, worst
end
ne, nq, mg, ws = quant_stats(by_edge)
println("B) unique exact-t keys: ", ne)
println("   unique quant-t keys: ", nq)
println("   quant keys merging >1 instance set: ", mg)
println("   max position spread inside one quantum cell: ", ws)
