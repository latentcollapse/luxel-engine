# Probe: is the F-C.2 TE key side-INDEPENDENT?
# vert_key computes edge_param(cv.x, t.v[e1], t.v[e2]) with e = ascending
# LOCAL slot indices. If two tets sharing a canonical lattice edge traverse
# it in opposite local directions, the parameter is t vs (1 - t) → the SAME
# geometric point gets DIFFERENT keys → shared-face dedup silently fails.
#
# Test 1: for every canonical edge (sorted lattice-node pair), do all tets
#         agreeing on that edge use the SAME local direction?
# Test 2: does an ABSOLUTE-node key ((n1, n2, t_abs)) show fewer unique keys
#         on identical geometry? If yes, the current key is broken.
using Printf
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
println("reference check: tets=", cr.tet_count, " tris_out=", cr.tris_out,
        " verts_out=", cr.verts_out, " (pinned: 1100/51982/26463)")

# collect TE-class ClipVerts with their tets (re-run clip machinery collecting)
te_census = []   # (tet_id, canonical_edge_abs, local_dir, t_param, position)
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
            dir_sign = sign(e[1] - e[2])   # +1 if local order ascends in slot
            push!(te_census, (tid, E, dir_sign, tpar, cv.x))
        end
    end
end
println("TE vertices census: ", length(te_census))

# --- Test 1: direction consistency per canonical edge ---
dir_by_edge = Dict{Tuple,Set{Int}}()
for (_, E, s, _, _) in te_census
    push!(get!(dir_by_edge, E, Set{Int}()), s)
end
mixed = count(x -> length(x) > 1, values(dir_by_edge))
println("canonical edges with MIXED local direction: ", mixed, " / ", length(dir_by_edge))

# --- Test 2: current key vs absolute key unique counts on shared points ---
# current key: (E, t) ; absolute key: (E, t_abs) with t_abs measured from the
# SORTED-first node (direction-free because position is exact)
cur_keys = Set{Tuple}()
abs_keys = Set{Tuple}()
for (tid, E, s, tpar, x) in te_census
    n1, n2 = E
    p1 = pos_of(g.origin, h, n1)
    p2 = pos_of(g.origin, h, n2)
    # absolute parameter measured from the SORTED-FIRST node (canonical side)
    t_abs = edge_param(x, p1, p2)
    push!(cur_keys, (E, tpar))
    push!(abs_keys, (E, t_abs))
end
println("unique CURRENT  keys (E, t_local): ", length(cur_keys))
println("unique ABSOLUTE keys (E, t_abs):   ", length(abs_keys))
println("Δ (keys current would fragment):   ", length(cur_keys) - length(abs_keys))

# --- Test 3: geometric duplicates under current key (same x, different key) ---
bypos = Dict{NTuple{3,Float64},Set{Tuple}}()
for (tid, E, s, tpar, x) in te_census
    push!(get!(bypos, x, Set{Tuple}()), (E, tpar))
end
dups = count(x -> length(x) > 1, values(bypos))
println("identical positions carrying >1 current key: ", dups, " / ", length(bypos))
