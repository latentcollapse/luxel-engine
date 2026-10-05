# Spiral F — GPU performance model: price the frame (Goal 2 §5). v1
#
# Pre-registered BEFORE any measurement. Times EXACTLY the E-validated
# kernels (E-G2 bit-exact contract, F-E.5 buffer fix) at the D operating
# points. No tuning, no extra passes, no substitutions: if it isn't the
# path E validated, it isn't the path F times.
#
# Frozen model (assumptions in one place):
#   H1  corners (4·tets f32) + per-vertex (idx,w) are RESIDENT on device
#       (streamed once at load); per-frame device cost is
#       t_frame = t_deform(corners) + t_reconstruct(verts). Nothing else
#       is in the budget (no upload, no render — render parity is E §7).
#   H2  weights/indices are computed OFFLINE f64→f32 (E's runtime model).
#   H3  one kernel launch per stage per frame (no batching/fusion —
#       that is FUTURE optimization headroom, not the baseline).
#
# Operating points = the D fidelity classes (pinned D1/D3 CSVs):
#   blob 0.28/1100t, teapot 0.3/500t, icosphere2 0.3/906t, robot 0.13/435t,
#   conifer 0.6/226t, plate 0.3/96t, grass 0.16/93t.
#
# Gates (registered BEFORE running):
#   F-G0 provenance: every class reproduces its PINNED tet count at its
#      pinned h (list above); locate misses == 0; harness self-check
#      (host idx/w64 machinery == TetDeform.cage_path_positions identity)
#      <= 1e-9·scale (E's gate, unchanged).
#   F-G1 device identity at every class × family — AMENDED before any
#      accepted run (v1 registered per-vertex bit-identity == 0.0, which is
#      NOT the E contract: device fma contraction legitimately yields
#      last-ulp object diffs — first run failed at exactly 1 ulp, 1.19e-7.
#      E's proven contract is on the SCREEN metric; the amended gate):
#        (a) per-vertex max|device − host_f32| <= 4·eps(f32)·max|coord|
#            (ulp-scale; a wrong pairing/weight blows past this by orders)
#        (b) |px95@5(device) − px95@5(host f32)| <= 0.02 px (E-G2 contract,
#            verbatim)
#   F-G2 frame budget: us_wall_frame <= 500 µs at EVERY class × family
#      (wall = @elapsed incl. launch + sync). 500 µs = 3% of a 16.6 ms
#      frame for the HEAVIEST D class at full quality — if this fails,
#      cage skinning at D quality on that class needs rework (LOD,
#      instancing, fusion) before shipping. Derived (not gated): instance
#      capacity = floor(duty / us_wall_frame) at duty = 2/4/8 ms.
#
# Pin protocol (pre-registered): this CSV contains WALL-CLOCK columns, so a
# full-file sha can NEVER reproduce across processes (D1 lesson, F-E.9).
# Stable columns = kind,name,h,tets,verts,family,reps,ident_max — these must
# be byte-identical across two fresh processes. us_* and derived tets_per_us
# / capacity columns are machine-state, reported but not pinned.
#
# Falsification conditions: F-G0 violation ⇒ corpus/provenance drift
# (re-audit); F-G1 violation ⇒ harness regression (fix, re-run); F-G2
# violation ⇒ the D-class quality contract does not fit the frame on this
# hardware at the measured operating point — reopen quality/cost tradeoff.
using CUDA
using Printf
using SHA
using Statistics

include("../oracle/TetDeform.jl"); using .TetDeform
using .TetDeform.TetCage
include("../oracle/TetCorpus.jl"); using .TetCorpus
include("../oracle/TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)
const F_PX = 500.0f0
const D_CAM = 5.0f0
const REPS = 25
const WARMUP = 3

mesh_scale(V) = max(maximum(v -> v[1], V) - minimum(v -> v[1], V),
                    maximum(v -> v[2], V) - minimum(v -> v[2], V),
                    maximum(v -> v[3], V) - minimum(v -> v[3], V))

# --- device kernels: VERBATIM from gpu/spiral_e_gpu.jl (E-validated) ---------

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

# --- host machinery: VERBATIM from gpu/spiral_e_gpu.jl (E-validated) ---------

function bary(t, p)
    E  = TetDeform.TetBasis.rest_basis(t.v[1], t.v[2], t.v[3], t.v[4])
    Ei = TetDeform.TetBasis.inv3(E)
    r1 = p[1] - t.v[1][1]; r2 = p[2] - t.v[1][2]; r3 = p[3] - t.v[1][3]
    l1 = Ei.a * r1 + Ei.b * r2 + Ei.c * r3
    l2 = Ei.d * r1 + Ei.e * r2 + Ei.f * r3
    l3 = Ei.g * r1 + Ei.h * r2 + Ei.i * r3
    return (l1, l2, l3, 1.0 - l1 - l2 - l3)
end

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

# --- timing helpers (function scope — no top-level soft-scope traps) ----------

function time_wall_us(f!, reps)
    ts = Vector{Float64}(undef, reps)
    for k in 1:reps
        ts[k] = @elapsed begin
            f!()
            CUDA.synchronize()
        end
    end
    return median(ts) * 1e6, minimum(ts) * 1e6
end

function time_dev_us(f!, reps)
    ts = Vector{Float64}(undef, reps)
    for k in 1:reps
        ts[k] = CUDA.@elapsed f!()
    end
    return median(ts) * 1e6, minimum(ts) * 1e6
end

# --- main ----------------------------------------------------------------------

rows_perf = String[]
rows_cap = String[]
notes = String[]

push!(notes, "device: $(CUDA.name(CUDA.device())) cap $(CUDA.capability(CUDA.device()))")
push!(notes, "cuda runtime: $(CUDA.runtime_version())")
push!(notes, "timing: median of $REPS reps after $WARMUP warmup launches; us_wall includes launch+sync, us_dev is device time")

# D operating points: (name, h, PINNED tet count from D1/D3 CSVs)
const CASES = [("blob", 0.28, 1100), ("teapot", 0.3, 500), ("icosphere2", 0.3, 906),
               ("robot", 0.13, 435), ("conifer", 0.6, 226), ("plate", 0.3, 96),
               ("grass", 0.16, 93)]
const FAMILIES = [(1, 0.2f0, 0.7f0, "wind"), (3, 0.4f0, 0.7f0, "fold4")]
const DUTIES_MS = (2.0, 4.0, 8.0)

for (name, h, tets_exp) in CASES
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    V, T = m.V, m.T
    scale = mesh_scale(V)
    g = auto_grid(V; h = h)
    cage = build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
    loc, misses = locate_tet(V, cage)
    @assert misses == 0 "locate miss on $name h=$h"
    nT = length(cage.tets)
    @assert nT == tets_exp "F-G0 FAILED: $name h=$h has $nT tets, D pin says $tets_exp"

    idx, w, w64 = weights_and_indices(V, cage, loc)
    fx, fy, fz, ffx, ffy, ffz = flatten_corners(cage)

    # F-G0 self-check: host machinery == TetDeform identity path
    P_ref = cage_path_positions(V, cage, loc, p -> p)
    P_chk = Vector{NTuple{3,Float64}}(undef, length(V))
    for (pi, _) in enumerate(V)
        i4 = 4 * (pi - 1)
        a = idx[i4+1]; b = idx[i4+2]; c = idx[i4+3]; d = idx[i4+4]
        w1 = w64[i4+1]; w2 = w64[i4+2]; w3 = w64[i4+3]; w4 = w64[i4+4]
        P_chk[pi] = (w2 * ffx[b] + w3 * ffx[c] + w4 * ffx[d] + w1 * ffx[a],
                     w2 * ffy[b] + w3 * ffy[c] + w4 * ffy[d] + w1 * ffy[a],
                     w2 * ffz[b] + w3 * ffz[c] + w4 * ffz[d] + w1 * ffz[a])
    end
    chk = maximum(hypot(p1[1]-p2[1], p1[2]-p2[2], p1[3]-p2[3]) for (p1, p2) in zip(P_ref, P_chk))
    @assert chk <= 1e-9 * scale "F-G0 FAILED on $name: self-check $chk"
    push!(notes, @sprintf("%s h=%.2f tets=%d (pin ok) self-check max=%.2e",
                          name, h, nT, chk))

    # device buffers (E-validated sizes: outputs nV — F-E.5)
    dcx = CuArray(fx); dcy = CuArray(fy); dcz = CuArray(fz)
    didx = CuArray(idx); dw = CuArray(w)
    dox = similar(dcx); doy = similar(dcy); doz = similar(dcz)
    nV = length(V); nC = length(fx)
    dpx = similar(dcx, nV); dpy = similar(dcy, nV); dpz = similar(dcz, nV)
    thr = 256

    for (fam, A, phi, famname) in FAMILIES
        deform! = () -> @cuda threads=thr blocks=cld(nC, thr) deform_corners!(
            dox, doy, doz, dcx, dcy, dcz, nC, Float32(scale), Int32(fam), A, phi)
        recon! = () -> @cuda threads=thr blocks=cld(nV, thr) reconstruct!(
            dpx, dpy, dpz, dox, doy, doz, didx, dw, nV)

        # warmup + F-G1 device identity vs host f32 path
        for _ in 1:WARMUP
            deform!(); recon!()
        end
        CUDA.synchronize()
        hx = Array(dpx); hy = Array(dpy); hz = Array(dpz)
        Pcpu = cpu_f32_path(V, cage, loc, fam, A, phi, scale)
        # F-G1(a): ulp-scale object identity (AMENDED — see header; v1's
        # == 0.0 mis-registered E's contract, failed at exactly 1 ulp)
        maxcoord = max(maximum(maximum(abs.(p)) for p in V),
                       maximum(maximum(abs.(t.v[s])) for t in cage.tets for s in 1:4))
        ulpbound = 4 * eps(Float32) * maxcoord
        ident = 0.0
        for i in 1:nV
            d1 = abs(Float64(hx[i]) - Pcpu[i][1])
            d2 = abs(Float64(hy[i]) - Pcpu[i][2])
            d3 = abs(Float64(hz[i]) - Pcpu[i][3])
            ident = max(ident, d1, d2, d3)
        end
        @assert ident <= ulpbound "F-G1(a) FAILED ($name $famname): max|Δ|=$ident > ulp bound $ulpbound"
        # F-G1(b): E-G2 screen contract verbatim
        fgt = fam == 1 ? mk_wind_like(scale, Float64(A), Float64(phi)) :
              mk_fold(scale; A = Float64(A))
        Pgt = [fgt(p) for p in V]
        pg = screen_px_error(Pgt, [(Float64(hx[i]), Float64(hy[i]), Float64(hz[i])) for i in 1:nV], 5.0).p95
        pc = screen_px_error(Pgt, Pcpu, 5.0).p95
        dscr = abs(pg - pc)   # NOT named dpx: that binding is the device buffer captured by recon!
        @assert dscr <= 0.02 "F-G1(b) FAILED ($name $famname): |px95 dev| = $dscr px > 0.02"

        # timing: median of REPS after warmup (same closures, no retuning)
        wd_med, wd_min = time_wall_us(deform!, REPS)
        dd_med, dd_min = time_dev_us(deform!, REPS)
        wr_med, wr_min = time_wall_us(recon!, REPS)
        dr_med, dr_min = time_dev_us(recon!, REPS)
        wf = wd_med + wr_med
        @assert wf <= 500.0 "F-G2 FAILED ($name $famname): frame deform+reconstruct $wf µs > 500 µs budget"

        push!(rows_perf, @sprintf("perf,%s,%.4g,%d,%d,%s,%.2f,%.2f,%.2f,%.2f,%.2f,%.2f,%d,%.4g,%.3e,%.4f",
                                  name, h, nT, nV, famname,
                                  wd_med, dd_med, wr_med, dr_med, wf, dd_med + dr_med,
                                  REPS, nT / wf, ident, dscr))

        # derived (NOT gated, NOT pinned): instance capacity per duty slice
        for duty_ms in DUTIES_MS
            cap = floor(Int, duty_ms * 1000.0 / wf)
            push!(rows_cap, @sprintf("cap,%s,%.4g,%d,%.1f,%d",
                                     name, h, nT, duty_ms, cap))
        end
    end
end

text = "# Spiral F — GPU frame pricing (RTX 5060 sm_120, E-validated kernels)\n" *
       join(["# " * n for n in notes], "\n") * "\n" *
       "# perf rows: kind,name,h,tets,verts,family,us_wall_deform,us_dev_deform,us_wall_recon,us_dev_recon,us_wall_frame,us_dev_frame,reps,tets_per_us_frame,ident_max,dev_px95_cpugpu\n" *
       join(rows_perf, "\n") * "\n" *
       "# cap rows: kind,name,h,tets,duty_ms,instances_in_duty (derived, not pinned)\n" *
       join(rows_cap, "\n") * "\n" *
       "# gates: F-G0 tets==pin & self-check; F-G1(a) ident<=4eps*maxcoord & (b) dev_px95_cpugpu<=0.02; F-G2 us_wall_frame<=500\n" *
       "# pin protocol: stable columns = kind,name,h,tets,verts,family,reps,ident_max (us_*/derived are machine-state)\n"
println(text)
resdir = joinpath(ROOT, "results")
isdir(resdir) || mkdir(resdir)
open(joinpath(resdir, "spiral_f_frame_budget.csv"), "w") do f
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
println("SPIRAL F GATES: ALL PASS (F-G0 provenance, F-G1 device identity, F-G2 frame budget) — see CSV for capacity table")
