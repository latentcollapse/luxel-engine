# Spiral C closure gate — the 9-mesh corpus audit, pinned as a script.
# History: run ad hoc after F-C.1 (pre F-C.2), again post F-C.2 by hand.
# This file makes it re-runnable: any identity regression must reproduce
# these numbers and this hash, or the audit FAILS (nonzero exit).
#
# Asserts per mesh:
#   anomalies == 0          (P-class unreachable, F-A2.2 plane bookkeeping)
#   tri_exp > 1.0           (covering cage must expand, never contract)
#   verts_out >= 8          (sanity floor)
# and prints ONE stable audit hash over all meshes (order-fixed) for
# cross-process determinism comparison.
using Printf
using SHA

include("TetMeshIO.jl"); using .TetMeshIO
include("TetCage.jl");   using .TetCage
include("TetCorpus.jl"); using .TetCorpus

const ROOT = dirname(@__DIR__)                      # tet-lab/
const CORPUS = joinpath(ROOT, "corpus")

# mesh -> cage voxel size (density-scaled: thin/sparse meshes get finer
# cages so they straddle multiple tets — same policy as the A4 audit)
const H_POLICY = Dict(
    "cube"       => 0.40,
    "icosphere2" => 0.40,
    "plate"      => 0.40,
    "grass"      => 0.25,
    "conifer"    => 0.40,
    "blob"       => 0.28,
    "skeleton"   => 0.40,
    "robot"      => 0.25,
    "teapot"     => 0.40,
)
const ORDER = ["cube", "icosphere2", "plate", "grass", "conifer",
               "blob", "skeleton", "robot", "teapot"]

io_all = IOBuffer()
println(io_all, "# TetLab 9-mesh corpus audit — post F-C.1+F-C.2 identity")
println(io_all, "# mesh,verts,tris,tets,tris_out,verts_out,tri_exp,vert_exp,anomalies")
failed = false
for name in ORDER
    m = read_obj(joinpath(CORPUS, name * ".obj"))
    V, T = m.V, m.T
    h = H_POLICY[name]
    g = auto_grid(V; h = h)
    cage = build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
    cr = clip_mesh(V, T, cage)
    n_anom = length(cr.anomalies)
    @printf("%-10s v=%-6d t=%-6d tets=%-5d tri_out=%-6d tri_exp=%.3f vert_exp=%.3f anom=%d\n",
            name, length(V), length(T), cr.tet_count, cr.tris_out,
            cr.tri_expansion, cr.vert_expansion, n_anom)
    # gates
    n_anom == 0              || (println("  GATE FAIL: anomalies");  global failed = true)
    cr.tri_expansion > 1.0   || (println("  GATE FAIL: expansion"); global failed = true)
    cr.verts_out >= 8        || (println("  GATE FAIL: verts");     global failed = true)
    println(io_all, join([name, length(V), length(T), cr.tet_count, cr.tris_out,
                          cr.verts_out, cr.tri_expansion, cr.vert_expansion, n_anom], ","))
end
text = String(take!(io_all))
hsh = bytes2hex(sha256(text))
println(text)
println("# audit sha256: ", hsh)
failed && error("CORPUS AUDIT FAILED — identity regression")
