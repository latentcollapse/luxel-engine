# TetLab Spiral A2/A3 — cage build, exact tri–tet overlap, provenance clipping.
# OPTIMIZED (Spiral OPT, hash-gated: identical math, hoisted computation):
#   O1  tet_planes/tet_aabb computed ONCE per tet (was: per (tet,candidate) pair,
#       twice per pair) — model: O(pairs) → O(tets) plane constructions
#   O2  tri_bins stored in Cage (was: recomputed inside clip_mesh)
#   O3  static TetPlanes (no heap Vector per call), allocation-free merge_planes
# Derivations unchanged — see RPD_SPIRAL_LEDGER.md turns A2/A3 and OPT.
module TetCage

using SHA

export Tet, Cage, ClipResult, build_cage, clip_mesh, cage_stats, cage_sha256,
       TET_LOCALS, TET_EDGES, canon_face_id, canon_edge_id, vert_key, auto_grid,
       TetPlanes, tet_planes, sdist, tri_bins, node_of, pos_of, ulp_between,
       inside_or_on, cut_point, straddle, fragment_edge, add_plane, merge_planes,
       tet_aabb, tri_aabb, aabb_overlap, tri_overlap_tet, sub, cross, dot3,
       edge_param, TE_T_QUANT, ClipVert, clip_triangle, mesh_edge

# ---------------------------------------------------------------------------
# 0. Uniform Freudenthal split: 6 permutation paths 000 → 111.
# ---------------------------------------------------------------------------
const TET_LOCALS = NTuple{4,Int}[
    (0b000, 0b100, 0b110, 0b111),
    (0b000, 0b100, 0b101, 0b111),
    (0b000, 0b010, 0b110, 0b111),
    (0b000, 0b010, 0b011, 0b111),
    (0b000, 0b001, 0b101, 0b111),
    (0b000, 0b001, 0b011, 0b111),
]
@assert length(Set(TET_LOCALS)) == 6

const TET_EDGES = ((1,2),(1,3),(1,4),(2,3),(2,4),(3,4))

@inline node_of(ijk, mask) = (
    ijk[1] + (mask & 1),
    ijk[2] + ((mask >> 1) & 1),
    ijk[3] + ((mask >> 2) & 1))

@inline pos_of(origin, h, node) = (
    origin[1] + h * node[1],
    origin[2] + h * node[2],
    origin[3] + h * node[3])

struct Tet
    vox::NTuple{3,Int}
    slot::Int
    nodes::NTuple{4,NTuple{3,Int}}
    v::NTuple{4,NTuple{3,Float64}}
end

canon_face_id(n1, n2, n3) = Tuple(sort([n1, n2, n3]))
canon_edge_id(n1, n2)     = n1 < n2 ? (n1, n2) : (n2, n1)

# ---------------------------------------------------------------------------
# Geometry primitives
# ---------------------------------------------------------------------------
@inline sub(a, b)  = (a[1]-b[1], a[2]-b[2], a[3]-b[3])
@inline cross(a,b) = (a[2]*b[3]-a[3]*b[2], a[3]*b[1]-a[1]*b[3], a[1]*b[2]-a[2]*b[1])
@inline dot3(a, b) = a[1]*b[1] + a[2]*b[2] + a[3]*b[3]

# face k = plane opposite vertex k, normal pointing AWAY from the interior.
# inside ⟺ dot(n, p−a) ≤ 0. Anchor a = t.v[i1] (first face vertex, unchanged
# by the orientation swap — identical arithmetic to the baseline oracle).
struct TetPlanes
    n::NTuple{4,NTuple{3,Float64}}
    a::NTuple{4,NTuple{3,Float64}}
end

function tet_planes(t::Tet)
    n1 = NTuple{3,Float64}[]; sizehint!(n1, 4)
    a1 = NTuple{3,Float64}[]; sizehint!(a1, 4)
    for k in 1:4
        i1, i2, i3 = mod1(k+1,4), mod1(k+2,4), mod1(k+3,4)
        a = t.v[i1]
        b, c = t.v[i2], t.v[i3]
        n = cross(sub(b,a), sub(c,a))
        if dot3(n, sub(t.v[k], a)) > 0   # inward → flip (F-A2.1 fix preserved)
            n = cross(sub(c,a), sub(b,a))
        end
        push!(n1, n); push!(a1, a)
    end
    TetPlanes((n1[1],n1[2],n1[3],n1[4]), (a1[1],a1[2],a1[3],a1[4]))
end

@inline sdist(n, a, p) = dot3(n, sub(p, a))

@inline inside_or_on(pl::TetPlanes, p) =
    sdist(pl.n[1], pl.a[1], p) <= 0 && sdist(pl.n[2], pl.a[2], p) <= 0 &&
    sdist(pl.n[3], pl.a[3], p) <= 0 && sdist(pl.n[4], pl.a[4], p) <= 0

@inline straddle(dc, dn) = (dc > 0 && dn < 0) || (dc < 0 && dn > 0)

@inline function cut_point(p0, p1, n, a)
    d0 = sdist(n, a, p0); d1 = sdist(n, a, p1)
    s = d0 / (d0 - d1)
    return (p0[1] + s*(p1[1]-p0[1]), p0[2] + s*(p1[2]-p0[2]), p0[3] + s*(p1[3]-p0[3]))
end

# ---------------------------------------------------------------------------
# Exact tri–tet overlap (witness families W1–W3), planes passed in (O1)
# ---------------------------------------------------------------------------
function tri_overlap_tet(pl::TetPlanes, t::Tet, tri)
    for p in tri                                   # W1
        inside_or_on(pl, p) && return true
    end
    n = cross(sub(tri[2], tri[1]), sub(tri[3], tri[1]))
    a0 = tri[1]
    function in_tri(p)
        b1 = dot3(n, cross(sub(tri[2], tri[1]), sub(p, tri[1])))
        b2 = dot3(n, cross(sub(tri[3], tri[2]), sub(p, tri[2])))
        b3 = dot3(n, cross(sub(tri[1], tri[3]), sub(p, tri[3])))
        (b1 >= 0 && b2 >= 0 && b3 >= 0) || (b1 <= 0 && b2 <= 0 && b3 <= 0)
    end
    # STRICT straddle = the A2/A3 baseline semantics (d == 0 is NOT a
    # witness). Note: the "restoration" to non-strict seg_plane_hit semantics
    # was itself the drift — the hash gate caught it (F-OPT.1).
    for (i, j) in TET_EDGES                        # W2
        d0 = dot3(n, sub(t.v[i], a0)); d1 = dot3(n, sub(t.v[j], a0))
        straddle(d0, d1) || continue
        s = d0 / (d0 - d1)
        x = (t.v[i][1] + s*(t.v[j][1]-t.v[i][1]),
             t.v[i][2] + s*(t.v[j][2]-t.v[i][2]),
             t.v[i][3] + s*(t.v[j][3]-t.v[i][3]))
        in_tri(x) && return true
    end
    for e in ((1,2),(2,3),(3,1))                   # W3
        p0, p1 = tri[e[1]], tri[e[2]]
        for f in 1:4
            nf = pl.n[f]; af = pl.a[f]
            df0 = sdist(nf, af, p0); df1 = sdist(nf, af, p1)
            straddle(df0, df1) || continue
            x = cut_point(p0, p1, nf, af)
            ok = true
            for f2 in 1:4
                f2 == f && continue
                sdist(pl.n[f2], pl.a[f2], x) > 0 && (ok = false; break)
            end
            ok && return true
        end
    end
    return false
end

@inline tet_aabb(t::Tet) = (
    (min(t.v[1][1],t.v[2][1],t.v[3][1],t.v[4][1]),
     min(t.v[1][2],t.v[2][2],t.v[3][2],t.v[4][2]),
     min(t.v[1][3],t.v[2][3],t.v[3][3],t.v[4][3])),
    (max(t.v[1][1],t.v[2][1],t.v[3][1],t.v[4][1]),
     max(t.v[1][2],t.v[2][2],t.v[3][2],t.v[4][2]),
     max(t.v[1][3],t.v[2][3],t.v[3][3],t.v[4][3])))

@inline tri_aabb(tri) = (
    (min(tri[1][1],tri[2][1],tri[3][1]), min(tri[1][2],tri[2][2],tri[3][2]), min(tri[1][3],tri[2][3],tri[3][3])),
    (max(tri[1][1],tri[2][1],tri[3][1]), max(tri[1][2],tri[2][2],tri[3][2]), max(tri[1][3],tri[2][3],tri[3][3])))

@inline aabb_overlap(a, b) = a[2][1] >= b[1][1] && b[2][1] >= a[1][1] &&
                             a[2][2] >= b[1][2] && b[2][2] >= a[1][2] &&
                             a[2][3] >= b[1][3] && b[2][3] >= a[1][3]

# ---------------------------------------------------------------------------
# Provenance clipping (identity machinery unchanged from A2/A3)
# ---------------------------------------------------------------------------
struct ClipVert
    x::NTuple{3,Float64}
    vi::Int              # original TRIANGLE corner (1..3, 0 = cut)
    vm::Int              # original MESH vertex id (1-based, 0 = cut)
    oe::Int              # original-edge fragment, LOCAL index (0 = chord)
    em::NTuple{2,Int}    # MESH-EDGE identity (sorted mesh vertex ids, 0,0 = chord)
    planes::NTuple{3,Int}
    npl::Int
end

ClipVert(x, vi)       = ClipVert(x, vi, 0, 0, (0, 0), (0,0,0), 0)
ClipVert(x, vi, vm)   = ClipVert(x, vi, vm, 0, (0, 0), (0,0,0), 0)

@inline on_orig_edge(cv::ClipVert, e::Int) =
    cv.oe == e || cv.vi == e || cv.vi == mod1(e + 1, 3)

@inline function fragment_edge(A, B)
    for e in (A.oe, B.oe, A.vi, B.vi)
        e > 0 && on_orig_edge(A, e) && on_orig_edge(B, e) && return e
    end
    return 0
end

# F-A2.2 fix preserved: exact plane incidence recorded on every pass (d == 0)
@inline function add_plane(cv::ClipVert, f::Int)
    f in (cv.planes[1], cv.planes[2], cv.planes[3]) && return cv
    ps = sort(vcat(collect(cv.planes[1:cv.npl]), [f]))
    ClipVert(cv.x, cv.vi, cv.vm, cv.oe, cv.em, (ps..., 0, 0, 0)[1:3], length(ps))
end

# mesh-edge identity for local edge e of a triangle with mesh ids tri_ids
@inline mesh_edge(tri_ids::NTuple{3,Int}, e::Int) =
    e == 1 ? (min(tri_ids[1], tri_ids[2]), max(tri_ids[1], tri_ids[2])) :
    e == 2 ? (min(tri_ids[2], tri_ids[3]), max(tri_ids[2], tri_ids[3])) :
             (min(tri_ids[3], tri_ids[1]), max(tri_ids[3], tri_ids[1]))
@inline function merge_planes(a::NTuple{3,Int}, b::NTuple{3,Int}, f::Int)
    c = Int[0, 0, 0]; nc = 0
    for x in (a[1], a[2], a[3])
        x == 0 && continue
        (x == b[1] || x == b[2] || x == b[3]) || continue
        nc += 1; c[nc] = x
    end
    allp = nc == 0 ? (f,) : nc == 1 ? (c[1], f) : nc == 2 ? (c[1], c[2], f) : (c[1], c[2], c[3], f)
    s = sort(collect(allp))
    npl = length(s)
    return (s[1], npl >= 2 ? s[2] : 0, npl >= 3 ? s[3] : 0), npl
end

function clip_triangle(tri, t::Tet, pl::TetPlanes, tri_ids::NTuple{3,Int} = (0, 0, 0))
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

# position along a tet edge as barycentric pair of the two endpoints,
# encoded side-independently from ABSOLUTE node coordinates:
#   λn = [(a−p)·(b−a)] / |b−a|² is NOT robust here; instead solve the 1-D
#   coordinate directly: p = n_i + t*(n_j − n_i) on the axis where the edge
#   is non-degenerate. t is exact for both tets sharing the edge because the
#   world positions of the canonical nodes are identical from both sides.
@inline function edge_param(p, ni, nj)
    d = nj .- ni
    axis = argmax(abs.(d))
    nj[axis] == ni[axis] && return 0.0   # degenerate projection (should not happen)
    return (p[axis] - ni[axis]) / (nj[axis] - ni[axis])
end

# F-C.3: TE-key parameter quantum. The exact-float t fragmented ulp-drifted
# cross-tet copies of the SAME chord point (blob5: 469/3664 instances differ
# only at ~1e-15), making vert counts drift-fragile AND hiding TE copies
# from the Spiral-C gap census entirely. Quantum 1e-9 sits 8 decades above
# copy drift and ~6 below real chord separation [MEASURED: zero false
# merges, max in-cell spread 1.1e-15; TE keys 1225 → 724 = conceptual
# chord-point count].
const TE_T_QUANT = 1e-9

function vert_key(cv::ClipVert, t::Tet)
    if cv.vi != 0
        return ("V", cv.vm, 0, (0, 0), 0.0)
    elseif cv.npl == 1
        f = cv.planes[1]
        F = canon_face_id(t.nodes[mod1(f+1,4)], t.nodes[mod1(f+2,4)], t.nodes[mod1(f+3,4)])
        # F-C.1: MESH-EDGE identity (sorted mesh vertex ids), not local index
        return ("EP", 0, cv.em, F, 0.0)
    elseif cv.npl == 2
        f, g = cv.planes[1], cv.planes[2]
        e = setdiff(collect(1:4), [f, g])
        n1, n2 = t.nodes[e[1]], t.nodes[e[2]]
        E = canon_edge_id(n1, n2)
        # F-C.2: chord vertices on the same canonical tet edge are DISTINCT
        # points; key needs the position along the edge (exact, shared by
        # both adjacent tets because canonical node positions are identical).
        tv = edge_param(cv.x, t.v[e[1]], t.v[e[2]])
        # F-C.3: quantize to the shared quantum so both tets' copies of one
        # chord point land on the same key and the gap census can compare them.
        tq = round(Int, tv / TE_T_QUANT)
        return ("TE", 0, cv.em, E, tq)
    else
        @assert cv.npl == 3
        k = setdiff(collect(1:4), collect(cv.planes[1:3]))[1]
        return ("TV", 0, (0, 0), t.nodes[k], 0.0)
    end
end

# ---------------------------------------------------------------------------
# Cage build (bins computed once, stored in Cage — O2)
# ---------------------------------------------------------------------------
function tri_bins(V, T, origin, h, dims)
    bins = Dict{NTuple{3,Int},Vector{Int}}()
    for (ti, idx) in enumerate(T)
        tri = (V[idx[1]+1], V[idx[2]+1], V[idx[3]+1])
        lo, hi = tri_aabb(tri)
        i0 = clamp(floor(Int, (lo[1]-origin[1])/h), 0, dims[1]-1)
        j0 = clamp(floor(Int, (lo[2]-origin[2])/h), 0, dims[2]-1)
        k0 = clamp(floor(Int, (lo[3]-origin[3])/h), 0, dims[3]-1)
        i1 = clamp(floor(Int, (hi[1]-origin[1])/h), 0, dims[1]-1)
        j1 = clamp(floor(Int, (hi[2]-origin[2])/h), 0, dims[2]-1)
        k1 = clamp(floor(Int, (hi[3]-origin[3])/h), 0, dims[3]-1)
        for i in i0:i1, j in j0:j1, k in k0:k1
            push!(get!(bins, (i, j, k), Int[]), ti)
        end
    end
    return bins
end

struct Cage
    origin::NTuple{3,Float64}
    h::Float64
    dims::NTuple{3,Int}
    voxels::Vector{NTuple{3,Int}}
    tets::Vector{Tet}
    bins::Dict{NTuple{3,Int},Vector{Int}}
end

# O4 (precomputed per-triangle AABB tables) was REJECTED on measurement:
# blob6 clip 404→532 ms, blob5 151→162 ms — table lookups (7.8 MB scattered)
# cost more than the inline min/max they replaced. Reverted; inline recompute
# retained. See TETCAGE_OPTIMIZATION_LEDGER.md.

function build_cage(V, T; origin, h, dims)
    bins = tri_bins(V, T, origin, h, dims)
    voxels = sort(collect(keys(bins)))
    tets = Tet[]
    for ijk in voxels
        for slot in 1:6
            l = TET_LOCALS[slot]
            nodes = ntuple(s -> node_of(ijk, l[s]), 4)
            vv = ntuple(s -> pos_of(origin, h, nodes[s]), 4)
            t = Tet(ijk, slot, nodes, vv)
            pl = tet_planes(t)          # O1: once per tet, not per candidate
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
    return Cage(origin, h, dims, voxels, tets, bins)
end

# F-A2.3 fix preserved: grids derived from mesh AABB, never guessed
function auto_grid(V; h::Float64, pad = 1)
    lo = (minimum(v[1] for v in V), minimum(v[2] for v in V), minimum(v[3] for v in V))
    hi = (maximum(v[1] for v in V), maximum(v[2] for v in V), maximum(v[3] for v in V))
    origin = (lo[1] - pad*h, lo[2] - pad*h, lo[3] - pad*h)
    dims = (ceil(Int, (hi[1]-lo[1])/h) + 1 + 2*pad,
            ceil(Int, (hi[2]-lo[2])/h) + 1 + 2*pad,
            ceil(Int, (hi[3]-lo[3])/h) + 1 + 2*pad)
    return (origin = origin, dims = dims)
end

# ---------------------------------------------------------------------------
# clip_mesh: uses cage.bins (O2), hoisted planes (O1)
# ---------------------------------------------------------------------------
struct ClipResult
    tris_out::Int
    verts_out::Int
    tet_count::Int
    tri_expansion::Float64
    vert_expansion::Float64
    tpt_sorted::Vector{Int}
    key_kinds::Dict{String,Int}
    anomalies::Vector{String}
end

function clip_mesh(V, T, cage::Cage)
    bins = cage.bins
    tris_out = 0
    verts_out = 0
    tpt = Int[]
    kinds = Dict("V" => 0, "EP" => 0, "TE" => 0, "TV" => 0)
    anomalies = String[]
    global_keys = Set{Tuple}()
    for t in cage.tets
        pl = tet_planes(t)              # O1: once per tet
        polys = Vector{ClipVert}[]
        for ti in get(bins, t.vox, Int[])
            idx = T[ti]
            tri = (V[idx[1]+1], V[idx[2]+1], V[idx[3]+1])
            aabb_overlap(tet_aabb(t), tri_aabb(tri)) || continue
            tri_overlap_tet(pl, t, tri) || continue
            poly = clip_triangle(tri, t, pl, idx)
            length(poly) >= 3 || continue
            push!(polys, poly)
        end
        isempty(polys) && continue
        push!(tpt, length(polys))
        for poly in polys
            tris_out += length(poly) - 2
            for cv in poly
                k = vert_key(cv, t)
                k in global_keys && continue
                push!(global_keys, k)
                verts_out += 1
                kinds[k[1]] += 1
                cv.npl == 1 && cv.vi == 0 && cv.oe == 0 &&
                    push!(anomalies, "P-class reached at tet $(t.vox) slot $(t.slot)")
            end
        end
    end
    T0, V0 = length(T), length(V)
    return ClipResult(tris_out, verts_out, length(cage.tets),
                      tris_out / T0, verts_out / V0, sort(tpt), kinds, anomalies)
end

function cage_stats(cr::ClipResult)
    tp = cr.tpt_sorted
    n = length(tp)
    return (tets_used = n,
            tris_out  = cr.tris_out,
            verts_out = cr.verts_out,
            tri_exp   = cr.tri_expansion,
            vert_exp  = cr.vert_expansion,
            tpt_min   = n == 0 ? 0 : tp[1],
            tpt_med   = n == 0 ? 0 : tp[ceil(Int, n/2)],
            tpt_p95   = n == 0 ? 0 : tp[ceil(Int, 0.95*n)],
            tpt_max   = n == 0 ? 0 : tp[end],
            kinds     = cr.key_kinds,
            anomalies = cr.anomalies)
end

function cage_sha256(V, T, cage, cr::ClipResult)
    io = IOBuffer()
    println(io, "origin=", cage.origin, " h=", cage.h, " dims=", cage.dims)
    println(io, "voxels=", length(cage.voxels), " tets=", cr.tet_count)
    println(io, "tris_out=", cr.tris_out, " verts_out=", cr.verts_out)
    foreach(vx -> println(io, "vx ", vx), cage.voxels)
    println(io, "tpt=", cr.tpt_sorted)
    println(io, "kinds=", sort(collect(cr.key_kinds)))
    return bytes2hex(sha256(take!(io)))
end

# ulp distance between two float64 values (restored: dropped in O1–O3 rewrite,
# needed by the Spiral-C fork's shared-key gap census)
function ulp_between(a::Float64, b::Float64)
    (isnan(a) || isnan(b)) && return typemax(Int)
    ia = reinterpret(Int64, a == 0.0 ? 0.0 : a)
    ib = reinterpret(Int64, b == 0.0 ? 0.0 : b)
    return abs(ia - ib)
end

end # module
