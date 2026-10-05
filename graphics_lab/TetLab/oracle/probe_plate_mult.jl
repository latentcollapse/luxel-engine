# Probe: attribute the plate VERTEX-ray multi-cluster signal (mean ~5 clusters
# per ray). Candidates: (a) real multi-depth surface pieces, (b) DDA clamped-
# entry artifact for rays starting outside the grid, (c) clustering-tolerance
# under-merge. Dump raw hits for a few rays; test grid-interior origins.
include("TetCageEps.jl"); using .TetCageEps
using .TetCageEps.TetCage
include("TetCorpus.jl"); using .TetCorpus
include("TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)

function mt_hit(o, d, a, b, c)
    e1 = sub(b, a); e2 = sub(c, a)
    p = cross(d, e2)
    det = dot3(e1, p)
    det == 0 && return nothing
    inv = 1.0 / det
    s = sub(o, a)
    u = dot3(s, p) * inv
    (u < 0 || u > 1) && return nothing
    q = cross(s, e1)
    v = dot3(d, q) * inv
    (v < 0 || u + v > 1) && return nothing
    t = dot3(e2, q) * inv
    t > 0 || return nothing
    return t
end

function build_soup(V, T, tets, bins; eps, prec, h)
    # census-v2 semantics: EVERY fan triangle of EVERY clipped polygon kept
    tris = [Vector{NTuple{3,NTuple{3,Float64}}}() for _ in 1:length(tets)]
    for (ti, t) in enumerate(tets)
        pl0 = tet_planes(t)
        pl = TetCageEps.grown_planes(pl0, eps, h)
        for tj in get(bins, t.vox, Int[])
            idx = T[tj]
            tri = (V[idx[1]+1], V[idx[2]+1], V[idx[3]+1])
            aabb_overlap(tet_aabb(t), tri_aabb(tri)) || continue
            tri_overlap_tet(pl, t, tri) || continue
            poly = TetCageEps.clip_triangle_grown(tri, t, pl, idx)
            length(poly) >= 3 || continue
            for k in 2:(length(poly) - 1)
                push!(tris[ti], (TetCageEps.quantize(poly[1].x, prec),
                                 TetCageEps.quantize(poly[k].x, prec),
                                 TetCageEps.quantize(poly[k+1].x, prec)))
            end
        end
    end
    return tris
end

function soup_bins(tris_per_tet, origin, h, dims)
    bins = Dict{NTuple{3,Int},Vector{Int}}()
    tid = 0
    for soup in tris_per_tet, tri in soup
        tid += 1
        lo = (min(tri[1][1], tri[2][1], tri[3][1]),
              min(tri[1][2], tri[2][2], tri[3][2]),
              min(tri[1][3], tri[2][3], tri[3][3]))
        hi = (max(tri[1][1], tri[2][1], tri[3][1]),
              max(tri[1][2], tri[2][2], tri[3][2]),
              max(tri[1][3], tri[2][3], tri[3][3]))
        i0 = clamp(floor(Int, (lo[1]-origin[1])/h), 0, dims[1]-1)
        j0 = clamp(floor(Int, (lo[2]-origin[2])/h), 0, dims[2]-1)
        k0 = clamp(floor(Int, (lo[3]-origin[3])/h), 0, dims[3]-1)
        i1 = clamp(floor(Int, (hi[1]-origin[1])/h), 0, dims[1]-1)
        j1 = clamp(floor(Int, (hi[2]-origin[2])/h), 0, dims[2]-1)
        k1 = clamp(floor(Int, (hi[3]-origin[3])/h), 0, dims[3]-1)
        for i in i0:i1, j in j0:j1, k in k0:k1
            push!(get!(bins, (i, j, k), Int[]), tid)
        end
    end
    return bins
end

function cast_all_hits(o, d, soup, sbins, origin, h, dims, tmax)
    hits = Tuple{Float64,NTuple{3,Float64}}[]
    tested = Set{Int}()
    # DDA (same as census)
    function idx(p, ax)
        clamp(floor(Int, (p[ax] - origin[ax]) / h), 0, dims[ax]-1)
    end
    cur = (idx(o,1), idx(o,2), idx(o,3))
    start_clamped = cur
    stepi = (d[1] > 0 ? 1 : -1, d[2] > 0 ? 1 : -1, d[3] > 0 ? 1 : -1)
    tmaxax = Vector{Float64}(undef, 3); tdelta = Vector{Float64}(undef, 3)
    for ax in 1:3
        if d[ax] == 0
            tmaxax[ax] = Inf; tdelta[ax] = Inf
        else
            nb = cur[ax] + (stepi[ax] > 0 ? 1 : 0)
            tmaxax[ax] = (origin[ax] + nb*h - o[ax]) / d[ax]
            tdelta[ax] = h / abs(d[ax])
        end
    end
    t = 0.0
    nvox = 1
    while t <= tmax
        for tid in get(sbins, cur, Int[])
            tid in tested && continue
            push!(tested, tid)
            tri = soup[tid]
            ht = mt_hit(o, d, tri[1], tri[2], tri[3])
            ht === nothing && continue
            x = (o[1] + ht*d[1], o[2] + ht*d[2], o[3] + ht*d[3])
            push!(hits, (ht, x))
        end
        ax = tmaxax[1] <= tmaxax[2] ? (tmaxax[1] <= tmaxax[3] ? 1 : 3) :
                                     (tmaxax[2] <= tmaxax[3] ? 2 : 3)
        t = tmaxax[ax]
        (t > tmax) && break
        nx = cur[ax] + stepi[ax]
        (nx < 0 || nx >= dims[ax]) && break
        cur = ax == 1 ? (nx, cur[2], cur[3]) : ax == 2 ? (cur[1], nx, cur[3]) : (cur[1], cur[2], nx)
        nvox += 1
    end
    sort!(hits; by = hh -> hh[1])
    return hits, start_clamped, nvox
end

# build plate ε=0 f64 soup
m = read_obj(joinpath(ROOT, "corpus", "plate.obj"))
V, T = m.V, m.T
h = 0.40
g = auto_grid(V; h = h)
println("grid: origin=", g.origin, " dims=", g.dims, " (plate z∈[0,0.002])")
tets = TetCageEps.build_cage_eps(V, T; origin = g.origin, h = h,
                                 dims = g.dims, eps = 0.0)
bins = tri_bins(V, T, g.origin, h, g.dims)
tris_per_tet = build_soup(V, T, tets, bins; eps = 0.0, prec = 64, h = h)
allsoup = reduce(vcat, tris_per_tet)
println("soup tris: ", length(allsoup))

# collect clipped-vertex positions (same as census: reuse gap-free route —
# just take all soup vertices)
verts = sort(unique(v for tri in allsoup for v in tri))

# ray 1: a vertex-ray from z=+2 (census style — starts OUTSIDE grid?)
v1 = verts[length(verts) ÷ 2]
o = (v1[1], v1[2], v1[3] + 2.0)
d = (0.0, 0.0, -1.0)
hits, sc, nv = cast_all_hits(o, d, allsoup, soup_bins(tris_per_tet, g.origin, h, g.dims),
                             g.origin, h, g.dims, 8.0)
println("\nRAY from outside  o=", o, "  startvoxel(clamped?)=", sc, "  voxels=", nv)
for (t, x) in hits
    println("   t=", t, "  x=", x)
end

# ray 2: SAME direction but origin just above the plate (INSIDE grid)
o2 = (v1[1], v1[2], v1[3] + 0.05)
hits2, sc2, nv2 = cast_all_hits(o2, d, allsoup, soup_bins(tris_per_tet, g.origin, h, g.dims),
                                g.origin, h, g.dims, 8.0)
println("\nRAY from inside   o=", o2, "  startvoxel=", sc2, "  voxels=", nv2)
for (t, x) in hits2
    println("   t=", t, "  x=", x)
end

# ray 3: grid center column
o3 = (0.5, 0.5, 1.0)
hits3, sc3, nv3 = cast_all_hits(o3, d, allsoup, soup_bins(tris_per_tet, g.origin, h, g.dims),
                                g.origin, h, g.dims, 8.0)
println("\nRAY center column o=", o3, "  startvoxel=", sc3, "  voxels=", nv3)
for (t, x) in hits3
    println("   t=", t, "  x=", x)
end
