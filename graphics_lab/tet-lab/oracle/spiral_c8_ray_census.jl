# Spiral C8 — deterministic CPU ray-caster census, v2 (goal §2).
#
# v1 findings folded in (Critic):
#  - populations MUST be split: generic (paper-comparable) vs vertex-aimed
#    vs gap-aimed (the paper's mitigation targets) vs all.
#  - "false hits vs analytic surface" conflates the CLIPPING-CHORD error
#    (piecewise-linear μMesh surface, sagitta ≈ edge²/8r — a representation
#    property, reported as dev percentiles) with ε-growth artifacts
#    (dev increase vs the ε=0 baseline at fixed ray set).
#  - duplicate intersections appear as SAME-cluster multiplicity (ghost
#    pieces live within the ε-band of the true surface) — multiplicity
#    stats per cluster are the C9 currency.
using Printf
using SHA

# Single module identity: TetCageEps embeds its own TetCage copy.
include("TetCageEps.jl"); using .TetCageEps
using .TetCageEps.TetCage
include("TetCorpus.jl"); using .TetCorpus
include("TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)

# --- ray–triangle, Möller–Trumbore, inclusive edges, no eps ------------------
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

# --- clipped-soup build (one code path: TetCageEps grown clip) ---------------
function build_soup(V, T, tets, bins; eps, prec, h)
    tris = Vector{Vector{NTuple{3,NTuple{3,Float64}}}}(undef, length(tets))
    gapmids = NTuple{3,Float64}[]
    gapmax = 0.0
    vertpos = NTuple{3,Float64}[]
    seen = Dict{Tuple,NTuple{3,Float64}}()
    for (ti, t) in enumerate(tets)
        pl0 = tet_planes(t)
        pl = TetCageEps.grown_planes(pl0, eps, h)
        polys = Vector{ClipVert}[]
        for tj in get(bins, t.vox, Int[])
            idx = T[tj]
            tri = (V[idx[1]+1], V[idx[2]+1], V[idx[3]+1])
            aabb_overlap(tet_aabb(t), tri_aabb(tri)) || continue
            tri_overlap_tet(pl, t, tri) || continue
            poly = TetCageEps.clip_triangle_grown(tri, t, pl, idx)
            length(poly) >= 3 && push!(polys, poly)
        end
        soup = Vector{NTuple{3,NTuple{3,Float64}}}()
        for poly in polys
            for k in 2:(length(poly) - 1)
                push!(soup, (TetCageEps.quantize(poly[1].x, prec),
                             TetCageEps.quantize(poly[k].x, prec),
                             TetCageEps.quantize(poly[k+1].x, prec)))
            end
            for cv in poly
                push!(vertpos, TetCageEps.quantize(cv.x, prec))
                kk = vert_key(cv, t)
                xq = TetCageEps.quantize(cv.x, prec)
                if haskey(seen, kk)
                    x0 = seen[kk]
                    if x0 != xq
                        push!(gapmids, ((x0[1]+xq[1])/2, (x0[2]+xq[2])/2, (x0[3]+xq[3])/2))
                        dd = sqrt(sum(abs2, x0 .- xq))
                        dd > gapmax && (gapmax = dd)
                    end
                else
                    seen[kk] = xq
                end
            end
        end
        tris[ti] = soup
    end
    return tris, gapmids, gapmax, unique(vertpos)
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

# Amanatides–Woo voxel traversal
function dda_voxels(o, d, origin, h, dims, tmax)
    vox = Tuple{Int,Int,Int}[]
    function idx(p, ax)
        clamp(floor(Int, (p[ax] - origin[ax]) / h), 0, dims[ax]-1)
    end
    cur = (idx(o,1), idx(o,2), idx(o,3))
    push!(vox, cur)
    stepi = (d[1] > 0 ? 1 : -1, d[2] > 0 ? 1 : -1, d[3] > 0 ? 1 : -1)
    tmaxax = Vector{Float64}(undef, 3)
    tdelta = Vector{Float64}(undef, 3)
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
    while t <= tmax
        ax = tmaxax[1] <= tmaxax[2] ? (tmaxax[1] <= tmaxax[3] ? 1 : 3) :
                                     (tmaxax[2] <= tmaxax[3] ? 2 : 3)
        t = tmaxax[ax]
        (t > tmax) && break
        nx = cur[ax] + stepi[ax]
        (nx < 0 || nx >= dims[ax]) && break
        cur = ax == 1 ? (nx, cur[2], cur[3]) : ax == 2 ? (cur[1], nx, cur[3]) : (cur[1], cur[2], nx)
        push!(vox, cur)
        tmaxax[ax] += tdelta[ax]
    end
    return vox
end

function cast_ray(o, d, soup, sbins, origin, h, dims, tmax)
    hits = Tuple{Float64,NTuple{3,Float64}}[]
    tested = Set{Int}()
    for vk in dda_voxels(o, d, origin, h, dims, tmax)
        for tid in get(sbins, vk, Int[])
            tid in tested && continue
            push!(tested, tid)
            tri = soup[tid]
            t = mt_hit(o, d, tri[1], tri[2], tri[3])
            t === nothing && continue
            x = (o[1] + t*d[1], o[2] + t*d[2], o[3] + t*d[3])
            push!(hits, (t, x))
        end
    end
    sort!(hits; by = h2 -> h2[1])
    return hits
end

function cluster_hits(hits; tol)
    isempty(hits) && return Vector{Vector{Tuple{Float64,NTuple{3,Float64}}}}()
    clusters = Vector{Vector{Tuple{Float64,NTuple{3,Float64}}}}()
    cur = Tuple{Float64,NTuple{3,Float64}}[hits[1]]
    for i in 2:length(hits)
        if hits[i][1] - cur[end][1] <= tol
            push!(cur, hits[i])
        else
            push!(clusters, cur)
            cur = Tuple{Float64,NTuple{3,Float64}}[hits[i]]
        end
    end
    push!(clusters, cur)
    return clusters
end

# census one population; deviation measured by surf_dev at every hit point
function census_set(soup, sbins, origin, h, dims, rays; tol, tmax, surf_dev)
    n = length(rays)
    esc = 0
    maxmult = 0
    multsum = 0
    nclusters = 0
    dup_rays = 0
    devs = Float64[]
    for (o, d) in rays
        hits = cast_ray(o, d, soup, sbins, origin, h, dims, tmax)
        isempty(hits) && (esc += 1; continue)
        cls = cluster_hits(hits; tol = tol)
        nclusters += length(cls)
        length(cls) > 1 && (dup_rays += 1)
        for cl in cls
            multsum += length(cl)
            length(cl) > maxmult && (maxmult = length(cl))
            for (_, x) in cl
                push!(devs, surf_dev(x))
            end
        end
    end
    sort!(devs)
    p95 = isempty(devs) ? 0.0 : devs[ceil(Int, 0.95 * length(devs))]
    mean_mult = nclusters == 0 ? 0.0 : multsum / nclusters
    dupfrac = nclusters == 0 ? 0.0 : (multsum - nclusters) / multsum
    return (rays = n, escapes = esc, escape_rate = esc / n,
            mean_mult = mean_mult, max_mult = maxmult, dup_frac = dupfrac,
            dup_rays = dup_rays,
            dev_max = isempty(devs) ? 0.0 : devs[end], dev_p95 = p95)
end

# --- analytic surfaces --------------------------------------------------------
mk_sphere_dev(r) = x -> abs(sqrt(x[1]^2 + x[2]^2 + x[3]^2) - r)
# plate: z = 0.002 sin(0.7 i + 1.3 j), x = w i/nx, y = h j/ny, w=h=1, nx=ny=16
mk_plate_dev() = x -> abs(x[3] - 0.002 * sin(0.7 * 16x[1] + 1.3 * 16x[2]))

# --- ray populations ----------------------------------------------------------
fib_dirs(n) = begin
    dirs = NTuple{3,Float64}[]
    ga = π * (3.0 - sqrt(5.0))
    for i in 0:(n-1)
        z = 1.0 - 2.0 * (i + 0.5) / n
        rax = sqrt(max(0.0, 1.0 - z * z))
        th = ga * i
        push!(dirs, (rax * cos(th), rax * sin(th), z))
    end
    dirs
end

function aim(o, target)
    d = sub(target, o)
    n = sqrt(sum(abs2, d))
    n == 0 && return nothing
    return (d[1]/n, d[2]/n, d[3]/n)
end

function sphere_populations(center, verts, gapmids; n_generic)
    generic = Tuple{NTuple{3,Float64},NTuple{3,Float64}}[]
    for d in fib_dirs(n_generic)
        push!(generic, (center, d))
    end
    vert = Tuple{NTuple{3,Float64},NTuple{3,Float64}}[]
    for v in verts
        d = aim(center, v)
        d === nothing && continue
        push!(vert, (center, d))
    end
    gap = Tuple{NTuple{3,Float64},NTuple{3,Float64}}[]
    for m in gapmids
        d = aim(center, m)
        d === nothing && continue
        push!(gap, (center, d))
    end
    return generic, vert, gap
end

function plate_populations(verts, gapmids, w, hgt)
    generic = Tuple{NTuple{3,Float64},NTuple{3,Float64}}[]
    N = 120
    for i in 0:(N-1), j in 0:(N-1)
        x = w * (i + 0.5) / N
        y = hgt * (j + 0.5) / N
        push!(generic, ((x, y, 2.0), (0.0, 0.0, -1.0)))
        push!(generic, ((x, y, -2.0), (0.0, 0.0, 1.0)))
    end
    for el in (5.0, 20.0, 45.0, 60.0, 85.0), az in range(0.0, 2π; length = 12)
        dz = sin(el)
        dxy = cos(el)
        dx = dxy * cos(az); dy = dxy * sin(az)
        o = (0.5*w - 3.0*dx, 0.5*hgt - 3.0*dy, 3.0*dz)
        push!(generic, (o, (dx, dy, -dz)))
    end
    vert = Tuple{NTuple{3,Float64},NTuple{3,Float64}}[]
    for v in verts
        push!(vert, ((v[1], v[2], v[3] + 2.0), (0.0, 0.0, -1.0)))
    end
    gap = Tuple{NTuple{3,Float64},NTuple{3,Float64}}[]
    for m in gapmids
        push!(gap, ((m[1], m[2], m[3] + 2.0), (0.0, 0.0, -1.0)))
    end
    return generic, vert, gap
end

# --- driver --------------------------------------------------------------------
const HDR = "name,prec,h,eps,population,rays,escapes,escape_rate,mean_mult,max_mult,dup_frac,dup_rays,dev_max,dev_p95"

function fmt(name, prec, h, eps, pop, r)
    @sprintf("%s,%d,%.4g,%.3e,%s,%d,%d,%.4e,%.3f,%d,%.4f,%d,%.4e,%.4e",
             name, prec, h, eps, pop, r.rays, r.escapes, r.escape_rate,
             r.mean_mult, r.max_mult, r.dup_frac, r.dup_rays,
             r.dev_max, r.dev_p95)
end

function run_mesh(name, V, T, h, center, tmax, surf_dev, kind; n_generic = 20000)
    rows = String[]
    for eps in (0.0, 2.5e-6, 1e-4), prec in (64, 32)
        g = auto_grid(V; h = h)
        tets = TetCageEps.build_cage_eps(V, T; origin = g.origin, h = h,
                                         dims = g.dims, eps = eps)
        bins = tri_bins(V, T, g.origin, h, g.dims)
        tris_per_tet, gapmids, gapmax, verts =
            build_soup(V, T, tets, bins; eps = eps, prec = prec, h = h)
        allsoup = reduce(vcat, tris_per_tet)
        sbins = soup_bins(tris_per_tet, g.origin, h, g.dims)
        tol = max(1e-9, 3.0 * gapmax)
        generic, vertr, gapr = kind == :sphere ?
            sphere_populations(center, verts, gapmids; n_generic = n_generic) :
            plate_populations(verts, gapmids, 1.0, 1.0)
        for (pop, rays) in (("GENERIC", generic), ("VERTEX", vertr),
                            ("GAP", gapr), ("ALL", vcat(generic, vertr, gapr)))
            isempty(rays) && continue
            r = census_set(allsoup, sbins, g.origin, h, g.dims, rays;
                           tol = tol, tmax = tmax, surf_dev = surf_dev)
            push!(rows, fmt(name, prec, h, eps, pop, r))
        end
    end
    return join(rows, "\n") * "\n"
end

println("# C8 CPU ray census v3 — soup-level instance-transform emulation")
println("# populations: GENERIC (paper-comparable) / VERTEX-aimed / GAP-aimed / ALL")
println("# ", HDR)
rows = String[]

V, T = TetCorpus.icosphere(5)
push!(rows, run_mesh("sphere", V, T, 0.22, (0.0, 0.0, 0.0), 2.0,
                     mk_sphere_dev(1.0), :sphere))

mp = read_obj(joinpath(ROOT, "corpus", "plate.obj"))
push!(rows, run_mesh("plate", mp.V, mp.T, 0.40, (0.5, 0.5, 0.0), 8.0,
                     mk_plate_dev(), :plate))

text = join(rows)
println(text)
isdir(joinpath(ROOT, "results")) || mkdir(joinpath(ROOT, "results"))  # F-E.9: ROOT-anchored, was CWD-relative
open(joinpath(ROOT, "results", "spiral-c8-ray-census.csv"), "w") do f
    println(f, "# C8 CPU ray census v3 — populations GENERIC/VERTEX/GAP/ALL")
    println(f, "# ", HDR)
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
