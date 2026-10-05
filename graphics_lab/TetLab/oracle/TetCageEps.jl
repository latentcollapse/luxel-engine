# TetLab Spiral C — epsilon-grown clipping (watertightness research fork).
# [PAPER §4.1] mitigation for instance-variant leaks: grow each tet outward
# along its face normals by a small epsilon before clipping triangles, so
# shared-boundary geometry overlaps slightly instead of leaking.
#
# CPU analog of the escape mechanism (DERIVED, recorded in ledger):
#   adjacent tets hold independently-computed copies of shared-face vertices
#   (different plane provenance). Under inexact storage the copies disagree
#   by ~ulp; a ray through the disagreement band misses both μMeshes.
#   STORAGE PRECISION is the knob that realizes the gap on CPU:
#     prec=64 → Float64 (gap ≈ machine-eps scale) → expect ~0 escapes
#     prec=32 → Float32 quantization of clipped verts (GPU-realistic)
# eps semantics: plane offsets grow by eps*h along outward normals BEFORE
# clipping. eps=0 must reproduce the base oracle bit-for-bit (gate).
module TetCageEps

using SHA

export build_cage_eps, clip_mesh_eps, escape_stats, EPS_DEFAULT

const EPS_DEFAULT = 2.5e-6   # [PAPER §4.1]

include("TetCage.jl")
using .TetCage

@inline grow_plane(n, a, eps_h) = (a[1] + eps_h * n[1], a[2] + eps_h * n[2], a[3] + eps_h * n[3])

# grow: move each plane's anchor OUTWARD along +n by eps*h (sdist decreases
# by eps_h everywhere → points formerly on the plane are now inside)
@inline function grown_planes(pl::TetPlanes, eps::Float64, h::Float64)
    eps == 0.0 && return pl
    e = eps * h
    TetPlanes(pl.n,
              (grow_plane(pl.n[1], pl.a[1], e),
               grow_plane(pl.n[2], pl.a[2], e),
               grow_plane(pl.n[3], pl.a[3], e),
               grow_plane(pl.n[4], pl.a[4], e)))
end

function build_cage_eps(V, T; origin, h, dims, eps::Float64)
    # tet SET retained under grown planes (paper envelope semantics: eps>0
    # slightly enlarges each tet's accept region).
    bins = tri_bins(V, T, origin, h, dims)
    voxels = sort(collect(keys(bins)))
    tets = Tet[]
    for ijk in voxels
        for slot in 1:6
            l = TET_LOCALS[slot]
            nodes = ntuple(s -> TetCage.node_of(ijk, l[s]), 4)
            vv = ntuple(s -> TetCage.pos_of(origin, h, nodes[s]), 4)
            t = Tet(ijk, slot, nodes, vv)
            pl0 = tet_planes(t)
            pl = grown_planes(pl0, eps, h)
            lo, hi = tet_aabb(t)
            keep = false
            for ti in get(bins, ijk, Int[])
                idx = T[ti]
                tri = (V[idx[1]+1], V[idx[2]+1], V[idx[3]+1])
                aabb_overlap((lo, hi), tri_aabb(tri)) || continue
                if tri_overlap_tet(pl, t, tri)
                    keep = true
                    break
                end
            end
            keep && push!(tets, t)
        end
    end
    return tets
end

# clipped-vertex quantization (storage precision knob)
@inline quantize(x::NTuple{3,Float64}, prec::Int) =
    prec == 64 ? x : (Float64(Float32(x[1])), Float64(Float32(x[2])), Float64(Float32(x[3])))

struct EpsResult
    tris_out::Int
    verts_out::Int
    tet_count::Int
    ulp_max::Int
    ulp_pairs::Int
    gaps32::Int
    gap_max::Float64
    eps::Float64
end

"""
    clip_mesh_eps(V, T, tets, bins; eps, prec)

Clip with grown planes; quantize stored positions; census the shared-key
position disagreements (the escape mechanism).
"""
function clip_mesh_eps(V, T, tets, bins; eps::Float64, prec::Int, h::Float64)
    tris_out = 0
    verts_out = 0
    ulp_max = 0
    ulp_pairs = 0
    gaps32 = 0
    gap_max = 0.0
    global_keys = Dict{Tuple,NTuple{3,Float64}}()
    anomalies = 0
    for t in tets
        pl0 = tet_planes(t)
        pl = grown_planes(pl0, eps, h)
        polys = Vector{ClipVert}[]
        for ti in get(bins, t.vox, Int[])
            idx = T[ti]
            tri = (V[idx[1]+1], V[idx[2]+1], V[idx[3]+1])
            aabb_overlap(tet_aabb(t), tri_aabb(tri)) || continue
            tri_overlap_tet(pl, t, tri) || continue
            poly = clip_triangle_grown(tri, t, pl, idx)
            length(poly) >= 3 || continue
            push!(polys, poly)
        end
        isempty(polys) && continue
        for poly in polys
            tris_out += length(poly) - 2
            for cv in poly
                k = vert_key(cv, t)
                xq = quantize(cv.x, prec)
                if haskey(global_keys, k)
                    x0 = global_keys[k]
                    if x0 != xq
                        gaps32 += 1
                        d = sqrt(sum(abs2, x0 .- xq))
                        d > gap_max && (gap_max = d)
                        if prec == 64
                            for c in 1:3
                                u = TetCage.ulp_between(x0[c], xq[c])
                                u > ulp_max && (ulp_max = u)
                            end
                        end
                        ulp_pairs += 1
                    end
                else
                    global_keys[k] = xq
                    verts_out += 1
                end
                cv.npl == 1 && cv.vi == 0 && cv.oe == 0 && (anomalies += 1)
            end
        end
    end
    return EpsResult(tris_out, verts_out, length(tets), ulp_max, ulp_pairs,
                     gaps32, gap_max, eps), anomalies, global_keys
end

# Sutherland–Hodgman with grown planes (identity machinery identical to base)
function clip_triangle_grown(tri, t::Tet, pl::TetPlanes, tri_ids = (0, 0, 0))
    verts = ClipVert[
        ClipVert(tri[1], 1, tri_ids[1]),
        ClipVert(tri[2], 2, tri_ids[2]),
        ClipVert(tri[3], 3, tri_ids[3]),
    ]
    for f in 1:4
        nf = pl.n[f]; af = pl.a[f]
        out = ClipVert[]
        m = length(verts)
        for idx in 1:m
            A = verts[idx]
            B = verts[mod1(idx+1, m)]
            dA = sdist(nf, af, A.x)
            dB = sdist(nf, af, B.x)
            dA == 0 && (A = add_plane(A, f))
            dB == 0 && (B = add_plane(B, f))
            dA = sdist(nf, af, A.x)
            if dA <= 0
                push!(out, A)
                straddle(dA, dB) || continue
                x = cut_point(A.x, B.x, nf, af)
                oe = fragment_edge(A, B)
                em = oe > 0 ? mesh_edge(tri_ids, oe) : (0, 0)
                pls, npl = merge_planes(A.planes, B.planes, f)
                push!(out, ClipVert(x, 0, 0, oe, em, pls, npl))
            else
                straddle(dA, dB) || continue
                x = cut_point(A.x, B.x, nf, af)
                oe = fragment_edge(A, B)
                em = oe > 0 ? mesh_edge(tri_ids, oe) : (0, 0)
                pls, npl = merge_planes(A.planes, B.planes, f)
                push!(out, ClipVert(x, 0, 0, oe, em, pls, npl))
            end
        end
        verts = out
        isempty(verts) && break
    end
    return verts
end

end # module
