# Decide: is the recorded blob5 pin (hash16 e730f8965c586853, verts_out=26463)
# reproducible from the on-disk oracle, or did pin and code diverge (F-OPT.1
# class process defect)? cage_sha256 covers origin/h/dims, voxels, tets,
# tris_out, verts_out, tpt, kinds — a match nails the exact state.
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
hsh = cage_sha256(V, T, cage, cr)
println("tets=", cr.tet_count, " tris_out=", cr.tris_out,
        " verts_out=", cr.verts_out)
println("kinds=", cr.key_kinds)
println("hash64=", hsh)
println("hash16=", hsh[1:16])
println("match e730f8965c586853? ", hsh[1:16] == "e730f8965c586853" ? "YES" : "NO")
