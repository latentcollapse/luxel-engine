# TetLab Spiral A5 — deformation oracle.
# GT path: analytic deformation f applied directly to mesh vertices.
# Cage path: f applied to CAGE vertices only; each mesh vertex reconstructed
#   by barycentric coordinates in its containing rest tet (paper's
#   piecewise-linear deformation model, §4):
#       λ = M⁻¹ (p − v0);   p' = Σ λᵢ aᵢ
# FAMILIES [PAPER §4/§8]: direct transform functions; bend (wind-like); plus
#   twist (non-affine rotational) and fold (sharp gradient) as adversarial
#   gradients. Bone-based transfer is OUT OF SCOPE for A5 (no rig in corpus).
#
# GATES (all asserted):
#   G1 identity  ⇒ error ≤ 1e-9      (reconstruction sanity)
#   G2 affine    ⇒ error ≤ 1e-9      (barycentric maps reproduce affine maps
#                                     EXACTLY — strongest mathematical gate)
#   G3 amplitude monotonicity        (err(A=0.4) > err(A=0.1) for non-affine)
#   G4 resolution monotonicity       (err(h=0.14) < err(h=0.28) at fixed A)
module TetDeform

using Printf
using SHA

export deformation_error, run_gates, run_sweep,
       locate_tet, cage_path_positions, error_metrics, screen_px_error,
       f_rotz, f_twist, f_wind, f_fold,
       mk_twist, mk_wind_phase, mk_wind, mk_fold, mk_wind_like

include("TetCage.jl"); using .TetCage
include("TetBasis.jl"); using .TetBasis
include("TetCorpus.jl"); using .TetCorpus

# --- deterministic deformation families (f: position -> position) ----------

f_identity(A) = p -> p

f_rotz(θ) = begin
    c, s = cos(θ), sin(θ)
    p -> (c*p[1] - s*p[2], s*p[1] + c*p[2], p[3])
end

f_twist(A; H = 6.6) = p -> begin
    θ = A * p[2] / H
    c, s = cos(θ), sin(θ)
    (c*p[1] - s*p[2], s*p[1] + c*p[2], p[3])
end

f_wind(A; H = 6.6, φ = 0.7) = p -> begin
    t = clamp(p[2] / H, 0.0, 1.0)
    lean = A * t^2 * sin(φ + 2.1*p[1] + 1.3*p[3])
    (p[1] + lean, p[2], p[3])
end

f_fold(A; k = 3.0) = p -> (p[1], p[2], p[3] + A * tanh(k * p[2]))

# --- scaled cage-field constructors (F-D.3 single source; D1 semantics) ------
# Positional constants are normalized by the MESH SCALE (max AABB side, the
# F-D.2 rule). Semantics fixed and measured by D1 (spiral_d_fidelity);
# D2/D3/temporal/E previously carried verbatim copies (F-D.3 debt) and now
# consume these exports. mk_wind is mk_wind_phase at the D1 phase 0.7.
# Argument TYPES flow through unchanged (E passes Float32 A/phi) — the
# constructors must not promote them (byte-identical re-pin requirement).
mk_twist(scale; A = 0.2) = p -> begin
    θ = A * (p[2] / scale)
    c, s = cos(θ), sin(θ)
    (c*p[1] - s*p[2], s*p[1] + c*p[2], p[3])
end
mk_wind_phase(scale; A = 0.2, phi = 0.7) = p -> begin
    t = clamp(p[2] / scale, 0.0, 1.0)
    lean = A * scale * t^2 * sin(phi + 2.1*p[1]/scale + 1.3*p[3]/scale)
    (p[1] + lean, p[2], p[3])
end
mk_wind(scale; A = 0.2) = mk_wind_phase(scale; A = A, phi = 0.7)
mk_fold(scale; A = 0.2) = p -> (p[1], p[2], p[3] + A * scale * tanh(3.0 * p[2] / scale))
mk_wind_like(scale, A, phi) = mk_wind_phase(scale; A = A, phi = phi)  # E signature

# --- point location: containing tet, deterministic voxel-neighborhood scan --

function locate_tet(V, cage)
    # voxel index -> tets of that voxel
    byvox = Dict{NTuple{3,Int},Vector{Tet}}()
    for t in cage.tets
        push!(get!(byvox, t.vox, Tet[]), t)
    end
    # global index lookup: O(1) map instead of O(tets) findfirst per vertex
    # (fine-cage sweeps made the linear scan dominate; pure perf fix, the
    # deterministic first-hit-wins iteration order is unchanged)
    idx_of = Dict{Tet,Int}(t => i for (i, t) in enumerate(cage.tets))
    # F-D.1 fix: vertices with axis-aligned coordinates land EXACTLY on
    # lattice planes; the sdist <= 0 rule is a rounding-sign lottery there
    # (a corner point can round "outside" for every candidate tet).
    # Two-pass: exact rule first; residual misses resolved by the tet with
    # the smallest max-plane sdist within a DERIVED noise tolerance
    # (sdist units are ~h³: n ~ h², distance ~ h; 1e-9 of a voxel).
    tol = 1e-9 * cage.h^3
    loc = Vector{Int}(undef, length(V))       # index into cage.tets, 0 = none
    misses = 0
    near_misses = 0
    for (pi, p) in enumerate(V)
        ijk = (clamp(floor(Int, (p[1]-cage.origin[1])/cage.h), 0, cage.dims[1]-1),
               clamp(floor(Int, (p[2]-cage.origin[2])/cage.h), 0, cage.dims[2]-1),
               clamp(floor(Int, (p[3]-cage.origin[3])/cage.h), 0, cage.dims[3]-1))
        found = 0
        for di in -1:1, dj in -1:1, dk in -1:1
            v = (ijk[1]+di, ijk[2]+dj, ijk[3]+dk)
            haskey(byvox, v) || continue
            for t in byvox[v]
                pl = tet_planes(t)
                inside = sdist(pl.n[1], pl.a[1], p) <= 0 && sdist(pl.n[2], pl.a[2], p) <= 0 &&
                         sdist(pl.n[3], pl.a[3], p) <= 0 && sdist(pl.n[4], pl.a[4], p) <= 0
                if inside
                    found = idx_of[t]
                    break
                end
            end
            found != 0 && break
        end
        if found == 0
            # pass 2: deterministic nearest-tet rescue within noise tolerance
            best = 0.0
            besti = 0
            for di in -1:1, dj in -1:1, dk in -1:1
                v = (ijk[1]+di, ijk[2]+dj, ijk[3]+dk)
                haskey(byvox, v) || continue
                for t in byvox[v]
                    pl = tet_planes(t)
                    m = max(sdist(pl.n[1], pl.a[1], p), sdist(pl.n[2], pl.a[2], p),
                            sdist(pl.n[3], pl.a[3], p), sdist(pl.n[4], pl.a[4], p))
                    if m <= tol && (besti == 0 || m < best)
                        best = m
                        besti = idx_of[t]
                    end
                end
            end
            if besti != 0
                found = besti
                near_misses += 1
            else
                misses += 1
            end
        end
        loc[pi] = found
    end
    near_misses > 0 && println(stderr, "locate: $near_misses on-lattice rescues (F-D.1 path)")
    return loc, misses
end

# barycentric reconstruction through deformed tets
function cage_path_positions(V, cage, loc, f)
    out = Vector{NTuple{3,Float64}}(undef, length(V))
    # deformed tet corner cache (applied redundantly per tet — f is pure)
    deformed = Dict{Int,NTuple{4,NTuple{3,Float64}}}()
    for (pi, p) in enumerate(V)
        ti = loc[pi]
        if ti == 0
            out[pi] = f(p)   # uncaged vertex: GT semantics (recorded by misses)
            continue
        end
        t = cage.tets[ti]
        dv = get!(deformed, ti) do
            ntuple(s -> f(t.v[s]), 4)
        end
        # λ = M⁻¹ (p − v0), where M = [v1−v0 | v2−v0 | v3−v0].
        # CORNER PAIRING (F-A5.2): λ1 weights v1, λ2→v2, λ3→v3, λ4=1−Σ→v0.
        # The falsified draft shifted corners by one (λ1·v0 …), which the G1
        # identity gate caught as a 0.46 error despite a perfect λ solve.
        E = rest_basis(t.v[1], t.v[2], t.v[3], t.v[4])
        Ei = inv3(E)
        r = (p[1]-t.v[1][1], p[2]-t.v[1][2], p[3]-t.v[1][3])
        λ1 = Ei.a*r[1] + Ei.b*r[2] + Ei.c*r[3]
        λ2 = Ei.d*r[1] + Ei.e*r[2] + Ei.f*r[3]
        λ3 = Ei.g*r[1] + Ei.h*r[2] + Ei.i*r[3]
        λ4 = 1.0 - λ1 - λ2 - λ3
        out[pi] = (λ1*dv[2][1] + λ2*dv[3][1] + λ3*dv[4][1] + λ4*dv[1][1],
                   λ1*dv[2][2] + λ2*dv[3][2] + λ3*dv[4][2] + λ4*dv[1][2],
                   λ1*dv[2][3] + λ2*dv[3][3] + λ3*dv[4][3] + λ4*dv[1][3])
    end
    return out
end

# error metrics between two vertex sets + per-triangle normal angles
function error_metrics(Pgt, Pcage, T; nsample = 512)
    n = length(Pgt)
    errs = Float64[]
    for i in 1:n
        d = Pgt[i] .- Pcage[i]
        push!(errs, sqrt(d[1]^2 + d[2]^2 + d[3]^2))
    end
    sort!(errs)
    maxe = errs[end]
    rmse = sqrt(sum(abs2, errs) / n)
    p95 = errs[ceil(Int, 0.95*n)]
    # normal angle errors on strided sample
    stride = max(1, length(T) ÷ nsample)
    nang = Float64[]
    for ti in 1:stride:length(T)
        idx = T[ti]
        a1, b1, c1 = Pgt[idx[1]+1], Pgt[idx[2]+1], Pgt[idx[3]+1]
        a2, b2, c2 = Pcage[idx[1]+1], Pcage[idx[2]+1], Pcage[idx[3]+1]
        n1 = cross3(a1, b1, c1); n2 = cross3(a2, b2, c2)
        l1 = sqrt(sum(abs2, n1)); l2 = sqrt(sum(abs2, n2))
        (l1 == 0 || l2 == 0) && continue
        cθ = clamp(sum(n1 .* n2) / (l1 * l2), -1.0, 1.0)
        push!(nang, acosd(cθ))
    end
    nmean = isempty(nang) ? 0.0 : sum(nang) / length(nang)
    nmax = isempty(nang) ? 0.0 : maximum(nang)
    return (max = maxe, rmse = rmse, p95 = p95,
            nmean_deg = nmean, nmax_deg = nmax)
end

@inline cross3(a, b, c) = ((b[2]-a[2])*(c[3]-a[3]) - (b[3]-a[3])*(c[2]-a[2]),
                           (b[3]-a[3])*(c[1]-a[1]) - (b[1]-a[1])*(c[3]-a[3]),
                           (b[1]-a[1])*(c[2]-a[2]) - (b[2]-a[2])*(c[1]-a[1]))

# screen-space error (px): axis-aligned pinhole camera looking down −z
function screen_px_error(Pgt, Pcage, d; f_px = 500.0)
    eyez = d
    errs = Float64[]
    for i in 1:length(Pgt)
        pg, pc = Pgt[i], Pcage[i]
        zg = max(eyez - pg[3], 0.05); zc = max(eyez - pc[3], 0.05)
        gx = f_px * pg[1] / zg; gy = f_px * pg[2] / zg
        cx = f_px * pc[1] / zc; cy = f_px * pc[2] / zc
        push!(errs, hypot(gx - cx, gy - cy))
    end
    sort!(errs)
    return (mean = sum(errs)/length(errs), p95 = errs[ceil(Int, 0.95*length(errs))], max = errs[end])
end

"""
    deformation_error(V, T, cage, loc, f) -> metrics (+ screen-space at 3 distances)
"""
function deformation_error(V, T, cage, loc, f)
    Pgt = [f(p) for p in V]
    Pcage = cage_path_positions(V, cage, loc, f)
    m = error_metrics(Pgt, Pcage, T)
    scr = Dict(d => screen_px_error(Pgt, Pcage, d) for d in (5.0, 20.0, 100.0))
    return m, scr
end

# --- gates ------------------------------------------------------------------

function run_gates(; h = 0.28, verbose = true)
    V, T = TetCorpus.blob(5)
    g = TetCage.auto_grid(V; h = h)
    cage = TetCage.build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
    loc, misses = locate_tet(V, cage)
    verbose && println("locate: misses = $misses / $(length(V)) (uncaged vertices take GT path)")
    misses == 0 || println("  WARNING: $misses uncaged vertices")

    # G1 identity
    m1, _ = deformation_error(V, T, cage, loc, f_identity(0.0))
    verbose && @printf("G1 identity : max=%.3e rmse=%.3e\n", m1.max, m1.rmse)
    @assert m1.max <= 1e-9 "G1 FAILED: identity reconstruction error $(m1.max)"

    # G2 affine (30° rotation) — must be exact through piecewise-linear path
    m2, _ = deformation_error(V, T, cage, loc, f_rotz(deg2rad(30)))
    verbose && @printf("G2 affine   : max=%.3e rmse=%.3e\n", m2.max, m2.rmse)
    @assert m2.max <= 1e-9 "G2 FAILED: affine reconstruction error $(m2.max)"

    # G3 amplitude monotonicity (twist at fixed h)
    m_lo, _ = deformation_error(V, T, cage, loc, f_twist(0.1))
    m_hi, _ = deformation_error(V, T, cage, loc, f_twist(0.4))
    verbose && @printf("G3 twist A=0.1 rmse=%.4f  A=0.4 rmse=%.4f\n", m_lo.rmse, m_hi.rmse)
    @assert m_hi.rmse > m_lo.rmse "G3 FAILED: error not monotone in amplitude"

    # G4 resolution monotonicity (twist at fixed A, finer cage)
    g2 = TetCage.auto_grid(V; h = h/2)
    cage2 = TetCage.build_cage(V, T; origin = g2.origin, h = h/2, dims = g2.dims)
    loc2, _ = locate_tet(V, cage2)
    m_fine, _ = deformation_error(V, T, cage2, loc2, f_twist(0.2))
    m_coarse, _ = deformation_error(V, T, cage, loc, f_twist(0.2))
    verbose && @printf("G4 twist A=0.2: h=%.3f rmse=%.4f  h=%.3f rmse=%.4f\n",
                        h, m_coarse.rmse, h/2, m_fine.rmse)
    @assert m_fine.rmse < m_coarse.rmse "G4 FAILED: error not monotone in resolution"

    verbose && println("ALL GATES GREEN")
    return true
end

# --- sweep ------------------------------------------------------------------

function run_sweep(; h_list = (0.28, 0.14), A_list = (0.05, 0.1, 0.2, 0.4))
    V, T = TetCorpus.blob(5)
    io = IOBuffer()
    println(io, "family,A,h,tets,max_err,rmse,p95,nmean_deg,nmax_deg,px_mean_d5,px_p95_d5,px_mean_d20,px_p95_d20,px_mean_d100,px_p95_d100")
    for h in h_list
        g = TetCage.auto_grid(V; h = h)
        cage = TetCage.build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
        loc, _ = locate_tet(V, cage)
        for (fam, fgen) in (("twist", f_twist), ("wind", f_wind), ("fold", f_fold))
            for A in A_list
                m, scr = deformation_error(V, T, cage, loc, fgen(A))
                @printf(io, "%s,%.2f,%.3f,%d,%.6f,%.6f,%.6f,%.3f,%.3f,%.3f,%.3f,%.3f,%.3f,%.3f,%.3f\n",
                        fam, A, h, length(cage.tets), m.max, m.rmse, m.p95,
                        m.nmean_deg, m.nmax_deg,
                        scr[5.0].mean, scr[5.0].p95,
                        scr[20.0].mean, scr[20.0].p95,
                        scr[100.0].mean, scr[100.0].p95)
            end
        end
    end
    text = String(take!(io))
    resroot = joinpath(dirname(@__DIR__), "results")  # F-E.9: ROOT-anchored, was CWD-relative
    isdir(resroot) || mkdir(resroot)
    open(joinpath(resroot, "deform_sweep.csv"), "w") do f
        write(f, text)
    end
    return bytes2hex(sha256(text))
end

end # module
