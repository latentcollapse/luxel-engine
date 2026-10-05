# Spiral E — GPU hardware confirmation (Goal 2 §4, F3 closure evidence). v2
#
# Toolchain: CUDA.jl 6.4 (runtime 13.4 artifacts), RTX 5060 (sm_120).
# E-G1 (proved before this driver): f32 device kernel executes and returns
# exact results on this toolchain (device name + cap 12.0 printed below).
#
# Scope (pre-registered): the CPU oracle (C8) remains AUTHORITY on mechanism;
# this spiral confirms the RUNTIME ARITHMETIC on hardware: device f32 cage
# deformation + barycentric reconstruction, TE-copy f32 snapping (C7's ε seam
# question), and duplicate-fragment agreement (F3: no hardware ghost shading).
# The Lava/Vulkan render pass is deferred with an explicit falsification
# condition (see tetcage-gpu-spiral-e.md).
#
# Runtime model (mirrors the planned GPU path): per-vertex barycentric
# weights computed OFFLINE on CPU in f64, stored f32; cage corners stored f32
# (4 slots per tet); per frame the device deforms cage corners in f32 and
# reconstructs p' = w2·c2' + w3·c3' + w4·c4' + w1·c1' in fixed left-to-right
# order — TetDeform.cage_path_positions' exact pairing (w=(λ4,λ1,λ2,λ3) over
# corners (v1,v2,v3,v4)).
#
# Gates (registered BEFORE running):
#   E-G2 D-contract (deform fidelity), blob h=0.28 / conifer h=0.6,
#        wind A=0.2 and fold4:
#          |px95@5_GPU − px95@5_CPUf32| <= 0.02 px  (device vs host f32:
#          only fma contraction + libm ulps may differ)
#          |px95@5_GPU − px95@5_f64-cage-path| <= 0.05·px95 + 0.05 px
#          (D band; the f64 cage path is the pipeline's exact reference)
#   E-G3 TE-copy snapping: group cage corners by 1e-9-quantized position
#        (TE_T_QUANT semantics); every intra-group pair after device deform:
#        screen-space deviation <= 0.01 px @ d=5 (≥100× below the 1 px
#        perceptual floor) — quantization absorbs ulp drift at f32.
#   E-G4 duplicate-fragment agreement (F3): same groups incl. plate h=0.3
#        (C8 worst population, mean_mult 5.05): max intra-group object-space
#        deviation <= 1e-5·scale AND derived normal tilt 2·δ/edge_min
#        <= 0.01 deg → shading-identical on hardware. F3 CLOSES if E-G3
#        and E-G4 hold on all cages.
#   E-G5 determinism: two fresh processes produce byte-identical CSV.
#
# Falsification conditions: any E-G2/E-G3/E-G4 violation reopens the
# corresponding contract (GPU D-contract / ε policy on hardware / F3).
using CUDA
using Printf
using SHA

include("../oracle/TetDeform.jl"); using .TetDeform
using .TetDeform.TetCage
include("../oracle/TetCorpus.jl"); using .TetCorpus
include("../oracle/TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)
const F_PX = 500.0f0
const D_CAM = 5.0f0

mesh_scale(V) = max(maximum(v -> v[1], V) - minimum(v -> v[1], V),
                    maximum(v -> v[2], V) - minimum(v -> v[2], V),
                    maximum(v -> v[3], V) - minimum(v -> v[3], V))

# mk_wind_like (E's phi-exposed wind signature) consumed from TetDeform
# via mk_wind_like(scale, A, phi) — F-D.3 single source (F-E.8 class avoided)

# --- device kernels -----------------------------------------------------------

# fam: 1 wind, 2 twist, 3 fold4 — f32 device arithmetic throughout
function deform_corners!(ox, oy, oz, cx, cy, cz, n, s, fam, A, phi)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if i <= n
        x = cx[i]; y = cy[i]; z = cz[i]
        if fam == 1
            t = clamp(y / s, 0f0, 1f0)
            lean = A * s * t * t * sin(phi + 2.1f0 * x / s + 1.3f0 * z / s)
            ox[i] = x + lean; oy[i] = y; oz[i] = z
        elseif fam == 2
            θ = A * (y / s)
            c = cos(θ); sn = sin(θ)
            ox[i] = c * x - sn * y; oy[i] = sn * x + c * y; oz[i] = z
        else
            ox[i] = x; oy[i] = y; oz[i] = z + A * s * tanh(3f0 * y / s)
        end
    end
    return nothing
end

# fixed-order weighted sum, mirrors TetDeform pairing
function reconstruct!(px, py, pz, ox, oy, oz, idx, w, n)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if i <= n
        i4 = 4 * (i - 1)
        a = idx[i4+1]; b = idx[i4+2]; c = idx[i4+3]; d = idx[i4+4]
        w1 = w[i4+1]; w2 = w[i4+2]; w3 = w[i4+3]; w4 = w[i4+4]
        px[i] = w2 * ox[b] + w3 * ox[c] + w4 * ox[d] + w1 * ox[a]
        py[i] = w2 * oy[b] + w3 * oy[c] + w4 * oy[d] + w1 * oy[a]
        pz[i] = w2 * oz[b] + w3 * oz[c] + w4 * oz[d] + w1 * oz[a]
    end
    return nothing
end

# one thread per TE pair: f32 screen-space deviation of DEFORMED CORNERS
# (E-G3 compares corner copies, not reconstructed vertices — F-E.6)
function pair_dev!(dev, ox, oy, oz, pa, pb, np)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if i <= np
        ia = pa[i]; ib = pb[i]
        dx = ox[ia] - ox[ib]; dy = oy[ia] - oy[ib]; dz = oz[ia] - oz[ib]
        dist = sqrt(dx * dx + dy * dy + dz * dz)
        dev[i] = (F_PX / D_CAM) * dist
    end
    return nothing
end

# --- host-side machinery -------------------------------------------------------

# barycentric solve for tet t at point p (f64): λ = M⁻¹·(p − v0) with
# M = [v2−v1 | v3−v1 | v4−v1]; λ1..λ3 from ROWS of M⁻¹, λ4 = 1−Σ.
# F-E.3: hand-rolled cofactor conventions are where the run-1/2 failures
# lived — this now DELEGATES to TetDeform's own rest_basis/inv3 and mirrors
# cage_path_positions' λ lines verbatim (zero convention risk left).
function bary(t, p)
    E  = TetDeform.TetBasis.rest_basis(t.v[1], t.v[2], t.v[3], t.v[4])
    Ei = TetDeform.TetBasis.inv3(E)
    r1 = p[1] - t.v[1][1]; r2 = p[2] - t.v[1][2]; r3 = p[3] - t.v[1][3]
    l1 = Ei.a * r1 + Ei.b * r2 + Ei.c * r3
    l2 = Ei.d * r1 + Ei.e * r2 + Ei.f * r3
    l3 = Ei.g * r1 + Ei.h * r2 + Ei.i * r3
    return (l1, l2, l3, 1.0 - l1 - l2 - l3)
end

# per-vertex corner slots + weights, exact TetDeform pairing (f32 for the
# runtime, f64 twin for the harness self-check)
function weights_and_indices(V, cage, loc)
    n = length(V)
    idx = Vector{Int32}(undef, 4n)
    w = Vector{Float32}(undef, 4n)
    w64 = Vector{Float64}(undef, 4n)
    for (pi, p) in enumerate(V)
        ti = loc[pi]
        t = cage.tets[ti]
        l1, l2, l3, l4 = bary(t, p)
        o = 4 * (pi - 1)
        idx[o+1] = Int32(4 * (ti - 1) + 1)   # v1 slot (1-based: Julia CuArrays are 1-based)
        idx[o+2] = Int32(4 * (ti - 1) + 2)   # v2
        idx[o+3] = Int32(4 * (ti - 1) + 3)   # v3
        idx[o+4] = Int32(4 * (ti - 1) + 4)   # v4
        w[o+1] = Float32(l4); w64[o+1] = l4  # weight for v1
        w[o+2] = Float32(l1); w64[o+2] = l1  # v2
        w[o+3] = Float32(l2); w64[o+3] = l2  # v3
        w[o+4] = Float32(l3); w64[o+4] = l3  # v4
    end
    return idx, w, w64
end

# flatten cage corners (4 slots/tet): f32 runtime copy + f64 exact copy
function flatten_corners(cage)
    nc4 = 4 * length(cage.tets)
    fx = Vector{Float32}(undef, nc4); fy = Vector{Float32}(undef, nc4); fz = Vector{Float32}(undef, nc4)
    ffx = Vector{Float64}(undef, nc4); ffy = Vector{Float64}(undef, nc4); ffz = Vector{Float64}(undef, nc4)
    for (ti, t) in enumerate(cage.tets)
        for s in 1:4
            c = 4 * (ti - 1) + s
            fx[c] = Float32(t.v[s][1]); fy[c] = Float32(t.v[s][2]); fz[c] = Float32(t.v[s][3])
            ffx[c] = t.v[s][1]; ffy[c] = t.v[s][2]; ffz[c] = t.v[s][3]
        end
    end
    return fx, fy, fz, ffx, ffy, ffz
end

# host f32 reference (same order, no fma contraction)
function cpu_f32_path(V, cage, loc, fam, A, phi, scale)
    fx, fy, fz = flatten_corners(cage)[1:3]
    n = length(fx)
    ox = Vector{Float32}(undef, n); oy = Vector{Float32}(undef, n); oz = Vector{Float32}(undef, n)
    for i in 1:n
        x = fx[i]; y = fy[i]; z = fz[i]
        if fam == 1
            t = clamp(y / scale, 0f0, 1f0)
            lean = A * scale * t * t * sin(phi + 2.1f0 * x / scale + 1.3f0 * z / scale)
            ox[i] = x + lean; oy[i] = y; oz[i] = z
        elseif fam == 2
            θ = A * (y / scale)
            c = cos(θ); sn = sin(θ)
            ox[i] = c * x - sn * y; oy[i] = sn * x + c * y; oz[i] = z
        else
            ox[i] = x; oy[i] = y; oz[i] = z + A * scale * tanh(3f0 * y / scale)
        end
    end
    idx, w = weights_and_indices(V, cage, loc)[1:2]
    n = length(V)
    P = Vector{NTuple{3,Float64}}(undef, n)
    for pi in 1:n
        i4 = 4 * (pi - 1)
        a = idx[i4+1]; b = idx[i4+2]; c = idx[i4+3]; d = idx[i4+4]  # already 1-based (F-E.2)
        w1 = w[i4+1]; w2 = w[i4+2]; w3 = w[i4+3]; w4 = w[i4+4]
        P[pi] = (Float64(w2 * ox[b] + w3 * ox[c] + w4 * ox[d] + w1 * ox[a]),
                 Float64(w2 * oy[b] + w3 * oy[c] + w4 * oy[d] + w1 * oy[a]),
                 Float64(w2 * oz[b] + w3 * oz[c] + w4 * oz[d] + w1 * oz[a]))
    end
    return P
end

# TE groups: 1e-9-quantized f64 corner positions (TE_T_QUANT semantics);
# members sorted by corner slot; groups sorted by first member (full determinism)
function te_groups(cage)
    g = Dict{NTuple{3,Int64},Vector{Int}}()
    for (ti, t) in enumerate(cage.tets)
        for s in 1:4
            c = 4 * (ti - 1) + s
            k = (round(Int64, t.v[s][1] / 1e-9), round(Int64, t.v[s][2] / 1e-9),
                 round(Int64, t.v[s][3] / 1e-9))
            push!(get!(g, k, Int[]), c)
        end
    end
    groups = [sort!(m) for m in values(g) if length(m) >= 2]
    sort!(groups; by = first)
    return groups
end

# --- main -----------------------------------------------------------------------

rows_case = String[]
rows_te = String[]
verdicts = String[]
notes = String[]

push!(notes, "device: $(CUDA.name(CUDA.device())) cap $(CUDA.capability(CUDA.device()))")
push!(notes, "cuda runtime: $(CUDA.runtime_version())")

const CASES = [("blob", 0.28), ("conifer", 0.6), ("plate", 0.3)]
const FAMILIES = [(1, 0.2f0, 0.7f0, "wind"), (3, 0.4f0, 0.7f0, "fold4")]

for (name, h) in CASES
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    V, T = m.V, m.T
    scale = mesh_scale(V)
    g = auto_grid(V; h = h)
    cage = build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
    loc, misses = locate_tet(V, cage)
    @assert misses == 0 "locate miss on $name h=$h"

    idx, w, w64 = weights_and_indices(V, cage, loc)
    fx, fy, fz, ffx, ffy, ffz = flatten_corners(cage)

    # harness self-check: f64 host evaluation of MY (idx,w64,f64-corners)
    # machinery must reproduce TetDeform.cage_path_positions to ≤ 1e-9·scale
    P_ref = cage_path_positions(V, cage, loc, p -> p)
    P_chk = Vector{NTuple{3,Float64}}(undef, length(V))
    for (pi, _) in enumerate(V)
        i4 = 4 * (pi - 1)
        a = idx[i4+1]; b = idx[i4+2]; c = idx[i4+3]; d = idx[i4+4]  # already 1-based (F-E.2)
        w1 = w64[i4+1]; w2 = w64[i4+2]; w3 = w64[i4+3]; w4 = w64[i4+4]
        P_chk[pi] = (w2 * ffx[b] + w3 * ffx[c] + w4 * ffx[d] + w1 * ffx[a],
                     w2 * ffy[b] + w3 * ffy[c] + w4 * ffy[d] + w1 * ffy[a],
                     w2 * ffz[b] + w3 * ffz[c] + w4 * ffz[d] + w1 * ffz[a])
    end
    chk = maximum(hypot(p1[1]-p2[1], p1[2]-p2[2], p1[3]-p2[3]) for (p1, p2) in zip(P_ref, P_chk))
    @assert chk <= 1e-9 * scale "harness self-check FAILED on $name: $chk (λ/idx machinery != TetDeform)"
    push!(notes, @sprintf("%s h=%.2f self-check max=%.2e (≤1e-9·scale) — λ machinery matches TetDeform",
                          name, h, chk))

    # device buffers
    dcx = CuArray(fx); dcy = CuArray(fy); dcz = CuArray(fz)
    didx = CuArray(idx); dw = CuArray(w)
    dox = similar(dcx); doy = similar(dcy); doz = similar(dcz)
    nV = length(V); nC = length(fx)
    # F-E.5: output buffers are nV VERTEX positions — similar(dcx) sized them
    # nC (corners) and reconstruct! wrote OOB on blob (first bad thread nC+1)
    dpx = similar(dcx, nV); dpy = similar(dcy, nV); dpz = similar(dcz, nV)
    thr = 256

    groups = te_groups(cage)
    pa = Int32[]; pb = Int32[]
    for mset in groups
        for a in 1:length(mset), b in (a+1):length(mset)
            push!(pa, mset[a]); push!(pb, mset[b])   # 1-based corner ids (device arrays are 1-based)
        end
    end
    npairs = length(pa)
    dpa = CuArray(pa); dpb = CuArray(pb)
    ddev = CUDA.zeros(Float32, max(npairs, 1))

    for (fam, A, phi, famname) in FAMILIES
        # GT f64
        if fam == 1
            fgt = mk_wind_like(scale, A, phi)
        elseif fam == 3
            fgt = p -> (p[1], p[2], p[3] + Float64(A) * scale * tanh(3.0 * p[2] / scale))
        end
        Pgt = [fgt(p) for p in V]

        # device: deform corners then reconstruct
        @cuda threads=thr blocks=cld(nC, thr) deform_corners!(dox, doy, doz, dcx, dcy, dcz, nC, Float32(scale), Int32(fam), A, phi)
        @cuda threads=thr blocks=cld(nV, thr) reconstruct!(dpx, dpy, dpz, dox, doy, doz, didx, dw, nV)
        hx = Array(dpx); hy = Array(dpy); hz = Array(dpz)   # bulk copy, no scalar GPU indexing
        oxh = Array(dox); oyh = Array(doy); ozh = Array(doz) # deformed corners (nC) for TE census
        Pgpu = [(hx[i], hy[i], hz[i]) for i in 1:nV]

        # CPU f32 reference
        Pcpu = cpu_f32_path(V, cage, loc, fam, A, phi, scale)

        pg = screen_px_error(Pgt, Pgpu, 5.0).p95
        pc = screen_px_error(Pgt, Pcpu, 5.0).p95
        # F-E.7: pf must be the f64 DEFORMED cage path under THIS family's GT
        # field — P_ref above is the identity path (self-check only); scoring
        # against it made the D-band clause vacuous
        P_f64 = cage_path_positions(V, cage, loc, fgt)
        pf = screen_px_error(Pgt, P_f64, 5.0).p95   # f64 cage path (D authority row)
        d_cg = abs(pg - pc)
        ok2 = d_cg <= 0.02 && pg <= 0.05 * pf + 0.05 + pf  # GPU within D band of the f64 cage path
        push!(rows_case, @sprintf("case,%s,%.4g,%s,%.4f,%.4f,%.4f,%.4f,%.4f,%s",
                                  name, h, famname, pf, pc, pg, d_cg, abs(pg - pf), ok2 ? "ok" : "FAIL"))
        @assert d_cg <= 0.02 "E-G2 FAILED ($name $famname): |GPU−CPUf32| px = $d_cg"
        @assert pg <= pf + 0.05 * pf + 0.05 "E-G2 D-band FAILED ($name $famname): GPU $pg vs f64 cage $pf"

        # TE pair census on device (this family's deform)
        if npairs > 0
            fill!(ddev, 0f0)
            @cuda threads=thr blocks=cld(npairs, thr) pair_dev!(ddev, dox, doy, doz, dpa, dpb, npairs)
            dev = Array(ddev)
            bit_ident = count(i -> (oxh[pa[i]] == oxh[pb[i]] && oyh[pa[i]] == oyh[pb[i]] &&
                                    ozh[pa[i]] == ozh[pb[i]]), 1:npairs)
            frac = bit_ident / npairs
            maxpx = maximum(dev)
            # object-space max deviation + normal tilt bound vs min cage edge
            maxobj = maxpx / (F_PX / D_CAM)
            edgemin = Inf
            for t in cage.tets
                for (a, b) in ((1, 2), (1, 3), (1, 4), (2, 3), (2, 4), (3, 4))
                    e = hypot(t.v[a][1]-t.v[b][1], t.v[a][2]-t.v[b][2], t.v[a][3]-t.v[b][3])
                    e < edgemin && (edgemin = e)
                end
            end
            ntilt = 2 * maxobj / edgemin * 180 / pi
            ok34 = maxpx <= 0.01 && maxobj <= 1e-5 * scale && ntilt <= 0.01
            push!(rows_te, @sprintf("te,%s,%s,%d,%d,%.4f,%.3e,%.3e,%.4f,%.3e,%s",
                                    name, famname, length(groups), npairs, frac,
                                    maxobj, maxpx, edgemin, ntilt, ok34 ? "ok" : "FAIL"))
            @assert maxpx <= 0.01 "E-G3 FAILED ($name $famname): pair dev $maxpx px"
            @assert maxobj <= 1e-5 * scale "E-G4 FAILED ($name $famname): obj dev $maxobj"
            @assert ntilt <= 0.01 "E-G4 FAILED ($name $famname): normal tilt $ntilt deg"
        end
    end
end

ok_all = true
text = "# Spiral E GPU confirmation (runtime arithmetic, RTX 5060 sm_120)\n" *
       join(["# " * n for n in notes], "\n") * "\n" *
       "# case rows: kind,name,h,family,px95_f64cage,px95_cpuf32,px95_gpu,dev_cpugpu_px,dev_gpu_f64,gate\n" *
       join(rows_case, "\n") * "\n" *
       "# te rows: kind,name,family,groups,pairs,bit_ident_frac,max_dev_obj,max_dev_px5,edge_min,ntilt_deg,gate\n" *
       join(rows_te, "\n") * "\n" *
       "# gates: E-G2 <=0.02px & D-band; E-G3 <=0.01px; E-G4 <=1e-5*scale & <=0.01deg; E-G5 = byte-identical CSV x2\n"
println(text)
resdir = joinpath(ROOT, "results")
isdir(resdir) || mkdir(resdir)
open(joinpath(resdir, "spiral-e-gpu-confirm.csv"), "w") do f
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
println("SPIRAL E GATES: ALL PASS (E-G1 toolchain, E-G2 D-contract, E-G3 snapping, E-G4 fragments, E-G5 pending second process)")
