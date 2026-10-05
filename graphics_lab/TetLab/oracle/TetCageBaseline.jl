# VERBATIM reconstruction of the A2/A3-accepted TetCage (the state that
# produced reference hash 35b3de9f… on blob5/h=0.28 auto_grid). Differential
# debugging only — DO NOT OPTIMIZE THIS FILE. Module renamed only.
module TetCageBaseline

using SHA

export build_cage, clip_mesh, cage_sha256, auto_grid

const TET_LOCALS = NTuple{4,Int}[
    (0b000, 0b100, 0b110, 0b111),
    (0b000, 0b100, 0b101, 0b111),
    (0b000, 0b010, 0b110, 0b111),
    (0b000, 0b010, 0b011, 0b111),
    (0b000, 0b001, 0b101, 0b111),
    (0b000, 0b001, 0b011, 0b111),
]
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

struct Cage
    origin::NTuple{3,Float64}
    h::Float64
    dims::NTuple{3,Int}
    voxels::Vector{NTuple{3,Int}}
    tets::Vector{Tet}
end

canon_face_id(n1, n2, n3) = Tuple(sort([n1, n2, n3]))
canon_edge_id(n1, n2)     = n1 < n2 ? (n1, n2) : (n2, n1)

@inline sub(a, b)  = (a[1]-b[1], a[2]-b[2], a[3]-b[3])
@inline cross(a,b) = (a[2]*b[3]-a[3]*b[2], a[3]*b[1]-a[1]*b[3], a[1]*b[2]-a[2]*b[1])
@inline dot3(a, b) = a[1]*b[1] + a[2]*b[2] + a[3]*b[3]

function tet_planes(t::Tet)
    out = Tuple{NTuple{3,Float64},NTuple{3,Float64},Int}[]
    for k in 1:4
        i1, i2, i3 = mod1(k+1,4), mod1(k+2,4), mod1(k+3,4)
        a, b, c = t.v[i1], t.v[i2], t.v[i3]
        n = cross(sub(b,a), sub(c,a))
        if dot3(n, sub(t.v[k], a)) > 0
            b, c = c, b
            n = cross(sub(b,a), sub(c,a))
        end
        push!(out, (n, a, k))
    end
    return out
end

@inline sdist(pl, p) = dot3(pl[1], sub(p, pl[2]))

@inline inside_or_on(planes, p) = all(sdist(pl, p) <= 0 for pl in planes)

@inline straddle(dc, dn) = (dc > 0 && dn < 0) || (dc < 0 && dn > 0)

@inline function cut_point(p0, p1, pl)
    d0 = sdist(pl, p0); d1 = sdist(pl, p1)
    s = d0 / (d0 - d1)
    return (p0[1] + s*(p1[1]-p0[1]), p0[2] + s*(p1[2]-p0[2]), p0[3] + s*(p1[3]-p0[3]))
end

function tri_overlap_tet(t::Tet, tri)
    planes = tet_planes(t)
    for p in tri
        inside_or_on(planes, p) && return true
    end
    n = cross(sub(tri[2], tri[1]), sub(tri[3], tri[1]))
    tripl = (n, tri[1], 0)
    function in_tri(p)
        b1 = dot3(n, cross(sub(tri[2], tri[1]), sub(p, tri[1])))
        b2 = dot3(n, cross(sub(tri[3], tri[2]), sub(p, tri[2])))
        b3 = dot3(n, cross(sub(tri[1], tri[3]), sub(p, tri[3])))
        (b1 >= 0 && b2 >= 0 && b3 >= 0) || (b1 <= 0 && b2 <= 0 && b3 <= 0)
    end
    for (i, j) in TET_EDGES
        d0 = sdist(tripl, t.v[i]); d1 = sdist(tripl, t.v[j])
        straddle(d0, d1) || continue
        x = cut_point(t.v[i], t.v[j], tripl)
        in_tri(x) && return true
    end
    for e in ((1,2),(2,3),(3,1))
        p0, p1 = tri[e[1]], tri[e[2]]
        for pl in planes
            d0 = sdist(pl, p0); d1 = sdist(pl, p1)
            straddle(d0, d1) || continue
            x = cut_point(p0, p1, pl)
            ok = true
            for pl2 in planes
                pl2 === pl && continue
                sdist(pl2, x) > 0 && (ok = false; break)
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

struct ClipVert
    x::NTuple{3,Float64}
    vi::Int
    vm::Int
    oe::Int
    planes::NTuple{3,Int}
    npl::Int
end

ClipVert(x, vi)     = ClipVert(x, vi, 0, 0, (0,0,0), 0)
ClipVert(x, vi, vm) = ClipVert(x, vi, vm, 0, (0,0,0), 0)

@inline on_orig_edge(cv::ClipVert, e::Int) =
    cv.oe == e || cv.vi == e || cv.vi == mod1(e + 1, 3)

@inline function fragment_edge(A, B)
    for e in (A.oe, B.oe, A.vi, B.vi)
        e > 0 && on_orig_edge(A, e) && on_orig_edge(B, e) && return e
    end
    return 0
end

@inline function add_plane(cv::ClipVert, f::Int)
    f in (cv.planes[1], cv.planes[2], cv.planes[3]) && return cv
    ps = sort(vcat(collect(cv.planes[1:cv.npl]), [f]))
    ClipVert(cv.x, cv.vi, cv.vm, cv.oe, (ps..., 0, 0, 0)[1:3], length(ps))
end

@inline function merge_planes(a, b, f)
    common = Int[]
    for x in (a[1], a[2], a[3])
        x == 0 && continue
        (x == b[1] || x == b[2] || x == b[3]) && push!(common, x)
    end
    allp = sort(vcat(common, [f]))
    return (allp..., 0, 0, 0)[1:3], length(allp)
end

function clip_triangle(tri, t::Tet, tri_ids = (0, 0, 0))
    planes = tet_planes(t)
    verts = ClipVert[
        ClipVert(tri[1], 1, tri_ids[1]),
        ClipVert(tri[2], 2, tri_ids[2]),
        ClipVert(tri[3], 3, tri_ids[3]),
    ]
    for f in 1:4
        pl = planes[f]
        out = ClipVert[]
        m = length(verts)
        for idx in 1:m
            A = verts[idx]
            B = verts[mod1(idx+1, m)]
            dA = sdist(pl, A.x)
            dB = sdist(pl, B.x)
            dA == 0 && (A = add_plane(A, f))
            dB == 0 && (B = add_plane(B, f))
            dA = sdist(pl, A.x)
            if dA <= 0
                push!(out, A)
                straddle(dA, dB) || continue
                x = cut_point(A.x, B.x, pl)
                oe = fragment_edge(A, B)
                pls, npl = merge_planes(A.planes, B.planes, f)
                push!(out, ClipVert(x, 0, 0, oe, pls, npl))
            else
                straddle(dA, dB) || continue
                x = cut_point(A.x, B.x, pl)
                oe = fragment_edge(A, B)
                pls, npl = merge_planes(A.planes, B.planes, f)
                push!(out, ClipVert(x, 0, 0, oe, pls, npl))
            end
        end
        verts = out
        isempty(verts) && break
    end
    return verts
end

function vert_key(cv::ClipVert, t::Tet)
    if cv.vi != 0
        return ("V", cv.vm, 0, (0,0,0))
    elseif cv.npl == 1
        f = cv.planes[1]
        F = canon_face_id(t.nodes[mod1(f+1,4)], t.nodes[mod1(f+2,4)], t.nodes[mod1(f+3,4)])
        return ("EP", cv.oe, 0, F)
    elseif cv.npl == 2
        f, g = cv.planes[1], cv.planes[2]
        e = setdiff(collect(1:4), [f, g])
        E = canon_edge_id(t.nodes[e[1]], t.nodes[e[2]])
        return ("TE", cv.oe, 0, E)
    else
        @assert cv.npl == 3
        k = setdiff(collect(1:4), collect(cv.planes[1:3]))[1]
        return ("TV", 0, 0, t.nodes[k])
    end
end

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
            keep = false
            for ti in get(bins, ijk, Int[])
                idx = T[ti]
                tri = (V[idx[1]+1], V[idx[2]+1], V[idx[3]+1])
                aabb_overlap(tet_aabb(t), tri_aabb(tri)) || continue
                if tri_overlap_tet(t, tri)
                    keep = true
                    break
                end
            end
            keep && push!(tets, t)
        end
    end
    return Cage(origin, h, dims, voxels, tets)
end

function auto_grid(V; h::Float64, pad = 1)
    lo = (minimum(v[1] for v in V), minimum(v[2] for v in V), minimum(v[3] for v in V))
    hi = (maximum(v[1] for v in V), maximum(v[2] for v in V), maximum(v[3] for v in V))
    origin = (lo[1] - pad*h, lo[2] - pad*h, lo[3] - pad*h)
    dims = (ceil(Int, (hi[1]-lo[1])/h) + 1 + 2*pad,
            ceil(Int, (hi[2]-lo[2])/h) + 1 + 2*pad,
            ceil(Int, (hi[3]-lo[3])/h) + 1 + 2*pad)
    return (origin = origin, dims = dims)
end

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
    bins = tri_bins(V, T, cage.origin, cage.h, cage.dims)
    tris_out = 0
    verts_out = 0
    tpt = Int[]
    kinds = Dict("V" => 0, "EP" => 0, "TE" => 0, "TV" => 0)
    anomalies = String[]
    global_keys = Set{Tuple}()
    for t in cage.tets
        polys = Vector{ClipVert}[]
        for ti in get(bins, t.vox, Int[])
            idx = T[ti]
            tri = (V[idx[1]+1], V[idx[2]+1], V[idx[3]+1])
            aabb_overlap(tet_aabb(t), tri_aabb(tri)) || continue
            tri_overlap_tet(t, tri) || continue
            poly = clip_triangle(tri, t, idx)
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

end # module
