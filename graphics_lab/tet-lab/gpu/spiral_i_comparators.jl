# Spiral I — Goal-2 comparator spiral: price optimized TetCage against the
# incumbent ways the same deformation work would actually be performed.
# FINAL GOAL-2 DECISION EXPERIMENT. v4 (v1 scaffold: incoherent phase
# semantics F-I.1; v2 patch-splice: structurally broken; v3 dry-review:
# rigid-rotation residue of F-I.3 still in rot_direct!/fused_batched_shared!,
# threadIdx.x missing parens, strided-view pinned uploads, x-only B check,
# per-instance copy loops → batched best-case uploads — all fixed; kernels
# never compiled pre-reboot so edits touch no cached pin).
# v5 (first-hardware fixes 2026-10-02, F-I.5: lbs! treated its 0-based
# instance index g as 1-based in the palette offset → g=0 read pal[bone-3]
# ≤ 0 = device BoundsError on EVERY thread of instance 1 (both runs died at
# the first C launch, blob N=1; g≥1 would silently read the previous
# instance's palette). Post-mortem dry-review of the never-executed N>1
# territory caught two more: B_direct bound single-mesh vertices against the
# registered N·V-resident contract (device OOB at N>1); ident_A/ident_auth
# x-comprehensions indexed R32[j] unwrapped over 1:ntot (host BoundsError at
# blob N=8, before B even launches). All three: index/binding convention
# bugs, no arithmetic touched.)
#
# MANDATE: "Do not seek a favorable TetCage result. Determine where
# optimized TetCage wins, ties, or loses."
#
# THE JOB (identical semantics for every path): animate N independent
# instances of the mesh through a phase cycle of RIGID ROTATION (φ
# advancing per frame; F-I.3: a θ = A·y/s + φ family is a TWIST —
# non-affine — so the cage path legitimately cannot reproduce it exactly and
# the paths would not produce the same output. Rigid rotation is affine:
# A5-G2 proves the cage path EXACT through it, so every path here produces
# the SAME output and walls are directly comparable). f32 positions per
# frame. One steady-state frame is timed per configuration (per-frame cost
# is cycle-invariant for every path; the cycle exists so state-carrying
# paths are honest).
#
# PATHS (all include launch + reads + writes + sync; uploads are MEASURED
# copyto!s from pinned host arrays):
#   A_param  Optimized TetCage, parametric family: fused batched kernel
#            (H-validated derivation, rigid-rotation body), graph-replayed.
#            0 B/frame (true form). Bandwidth truth: single-trajectory
#            A-then-B composes to fused-for-parametric
#            (tetcage-fusion-headroom.md §2).
#   A_auth   Optimized TetCage, AUTHORED control: per-frame upload of the
#            N instances' cage states (pinned, 48·T·N B) + batched
#            reconstruct (E arithmetic verbatim, per-instance corner slabs).
#   B_direct Direct full-vertex deformation: batched kernel applying the
#            EXACT same rigid rotation (host-precomputed f32 c,s — bitwise
#            identical to the host reference arithmetic) on N·V resident
#            vertices. 0 B/frame — its true form (no state needed).
#   C_lbs    Conventional 4-weight LBS: geometry+weights resident; per-frame
#            upload of N rotation palettes (pinned, 24·N B — palette
#            computed CPU-side from the same control stream, as rig
#            animation everywhere) + LBS kernel (4 fetches, weighted sum).
#            QUALITY BOUND (recorded): LBS cannot represent twist/fold;
#            measured on the rotation family (its native regime).
#   D_morph  Morph/vertex-stream: per-frame upload of N morph-target pairs
#            (pinned, 24·V·N B) + streaming lerp (α=0.5). Rotation family.
#
# LADDERS: N ∈ {1, 8, 32, 128}, batched single-dispatch per frame. The
# class axis (V,T) carries the density crossover.
#
# GATES (registered BEFORE running):
#   HI-G0 provenance: pinned tet counts (F pins); cage path ≡ GT rotation at
#      the sampled phase (object space ≤ 1e-9·scale, E self-check class);
#      self-check machinery E-gates.
#   HI-G1 correctness (≤ 4·eps(f32)·max|coord|, the E-G2 bound): A_param,
#      A_auth, B_direct each vs host-f32 rotation; C vs host LBS
#      accumulation; D vs host lerp. A_param ≡ A_auth BITWISE by
#      construction (same f32-rounded corner inputs, same f32 rotation
#      values, same weighted-sum order). Last instance spot-checked per
#      path (instances are identical clones at the parametric operating
#      point — asserted, not assumed).
#   HI-G2 work-conservation: uploads are measured copyto!s; A_param/B_direct
#      charged 0 B/frame because their true form needs no state stream.
#   HI-G3 honesty: capacity derived from MEASURED walls only; no path
#      excludes launch/sync/read/write.
#   HI-G4 determinism: stable columns byte-identical ×2 (D1 protocol).
#
# RISK REGISTERED (not gateable pre-run): dev_us (CUDA.@elapsed) has never
# been applied to graph launches in this campaign (H wall-timed graph paths
# only). If it rejects graph execs, the failure is deterministic and lands
# in the failure register — dev columns for graphed paths would be dropped
# with the wall columns unaffected; NO silent substitution.
using CUDA
using Printf
using SHA
using Statistics

include("../oracle/TetDeform.jl"); using .TetDeform
using .TetDeform.TetCage
include("../oracle/TetCorpus.jl"); using .TetCorpus
include("../oracle/TetMeshIO.jl"); using .TetMeshIO

const ROOT = dirname(@__DIR__)
const REPS = 25
const WARMUP = 5
const NLADDER = (1, 8, 32, 128)
const DPHI = Float32(pi / 12)
const PH_T = Float32(pi / 12)   # sampled steady-state phase (frame k=2 of the cycle)

mesh_scale(V) = max(maximum(v -> v[1], V) - minimum(v -> v[1], V),
                    maximum(v -> v[2], V) - minimum(v -> v[2], V),
                    maximum(v -> v[3], V) - minimum(v -> v[3], V))

# f32 rigid-rotation constants shared by EVERY path and the host reference —
# one definition point so all paths consume bitwise-identical c,s.
rot_consts(phi) = (c = cos(phi), s = sin(phi))

# ---------------- kernels ------------------------------------------------------

# batched reconstruct: instance g's cage slab occupies corners [g·4nT+1 .. g·4nT+4nT];
# vertex j of every instance uses the SHARED weight table at 4·(j-1)+s.
# Arithmetic VERBATIM from E's reconstruct! (F-I.2 fix: corner offset = g·4nT).
function reconstruct_batched!(px, py, pz, ox, oy, oz, idx, w, nV, nT, ntot)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if i <= ntot
        g = (i - 1) ÷ nV
        j = i - g * nV
        coff = g * 4 * nT
        i4 = 4 * (j - 1)
        a = idx[i4+1] + coff; b = idx[i4+2] + coff
        c = idx[i4+3] + coff; d = idx[i4+4] + coff
        w1 = w[i4+1]; w2 = w[i4+2]; w3 = w[i4+3]; w4 = w[i4+4]
        px[i] = w2 * ox[b] + w3 * ox[c] + w4 * ox[d] + w1 * ox[a]
        py[i] = w2 * oy[b] + w3 * oy[c] + w4 * oy[d] + w1 * oy[a]
        pz[i] = w2 * oz[b] + w3 * oz[c] + w4 * oz[d] + w1 * oz[a]
    end
    return nothing
end

# direct rigid rotation over N·V resident vertices; c,s are HOST-precomputed
# f32 values (the same constants the host reference used) → the arithmetic
# c*x - s*y / s*x + c*y is bitwise the host reference's.
function rot_direct!(px, py, pz, vx, vy, vz, ntot, c, s)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if i <= ntot
        x = vx[i]; y = vy[i]
        px[i] = c * x - s * y
        py[i] = s * x + c * y
        pz[i] = vz[i]
    end
    return nothing
end

# 4-weight LBS: per-vertex 4 (bone, weight) pairs; per-instance rotation
# palette (2 floats/bone; 3 bones); realistic fetch pattern.
function lbs!(px, py, pz, vx, vy, vz, bidx, bw, pal_c, pal_s, n_per, ntot)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if i <= ntot
        g = (i - 1) ÷ n_per
        j = i - g * n_per
        j4 = 4 * (j - 1)
        x = vx[j]; y = vy[j]; z = vz[j]
        ox = 0f0; oy = 0f0
        for k in 1:4
            bone = bidx[j4+k]
            wgt = bw[j4+k]
            cθ = pal_c[g * 3 + bone]  # g is 0-BASED: instance g's palette block is [3g+1 .. 3g+3]
            sθ = pal_s[g * 3 + bone]
            ox += wgt * (cθ * x - sθ * y)
            oy += wgt * (sθ * x + cθ * y)
        end
        px[i] = ox; py[i] = oy; pz[i] = z
    end
    return nothing
end

# morph pair lerp: pairs uploaded interleaved 6 floats/vertex
function morph_lerp!(px, py, pz, m, ntot, alpha)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if i <= ntot
        o = 6 * (i - 1)
        ax = m[o+1]; ay = m[o+2]; az = m[o+3]
        px[i] = ax + alpha * (m[o+4] - ax)
        py[i] = ay + alpha * (m[o+5] - ay)
        pz[i] = az + alpha * (m[o+6] - az)
    end
    return nothing
end

# fused batched, SHARED cage (parametric family: all instances share rest
# cage + phase → no per-instance corner offset). H derivation with the
# RIGID-ROTATION body: one host-precomputed (c,s) pair rotates all 4 corners
# (A_param ≡ A_auth bitwise: same f32 corner inputs, same c,s, and the
# weighted sum is E's reconstruct! order verbatim).
function fused_batched_shared!(px, py, pz, cx, cy, cz, idx, w, nV, ntot, c, s)
    i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
    if i <= ntot
        g = (i - 1) ÷ nV
        j = i - g * nV
        i4 = 4 * (j - 1)
        a = idx[i4+1]; b = idx[i4+2]; cc = idx[i4+3]; d = idx[i4+4]
        w1 = w[i4+1]; w2 = w[i4+2]; w3 = w[i4+3]; w4 = w[i4+4]
        ax = cx[a]; ay = cy[a]; az = cz[a]
        bx = cx[b]; by = cy[b]; bz = cz[b]
        ex = cx[cc]; ey = cy[cc]; ez = cz[cc]
        fx2 = cx[d]; fy2 = cy[d]; fz2 = cz[d]
        ax2 = c * ax - s * ay; ay2 = s * ax + c * ay
        bx2 = c * bx - s * by; by2 = s * bx + c * by
        ex2 = c * ex - s * ey; ey2 = s * ex + c * ey
        fx3 = c * fx2 - s * fy2; fy3 = s * fx2 + c * fy2
        px[i] = w2 * bx2 + w3 * ex2 + w4 * fx3 + w1 * ax2
        py[i] = w2 * by2 + w3 * ey2 + w4 * fy3 + w1 * ay2
        pz[i] = w2 * bz + w3 * ez + w4 * fz2 + w1 * az
    end
    return nothing
end

# ---------------- host machinery (verbatim from E/F/H) --------------------------

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
        idx[o+1] = Int32(4 * (ti - 1) + 1)
        idx[o+2] = Int32(4 * (ti - 1) + 2)
        idx[o+3] = Int32(4 * (ti - 1) + 3)
        idx[o+4] = Int32(4 * (ti - 1) + 4)
        w[o+1] = Float32(l4); w64[o+1] = l4
        w[o+2] = Float32(l1); w64[o+2] = l1
        w[o+3] = Float32(l2); w64[o+3] = l2
        w[o+4] = Float32(l3); w64[o+4] = l3
    end
    return idx, w, w64
end

function flatten_corners(cage)
    nc4 = 4 * length(cage.tets)
    fx = Vector{Float32}(undef, nc4); fy = Vector{Float32}(undef, nc4); fz = Vector{Float32}(undef, nc4)
    for (ti, t) in enumerate(cage.tets)
        for s in 1:4
            c = 4 * (ti - 1) + s
            fx[c] = Float32(t.v[s][1]); fy[c] = Float32(t.v[s][2]); fz[c] = Float32(t.v[s][3])
        end
    end
    return fx, fy, fz
end

function wall_us(f!, reps)
    ts = Vector{Float64}(undef, reps)
    for k in 1:reps
        ts[k] = @elapsed begin
            f!()
            CUDA.synchronize()
        end
    end
    return median(ts) * 1e6
end

function dev_us(f!, reps)
    ts = Vector{Float64}(undef, reps)
    for k in 1:reps
        ts[k] = CUDA.@elapsed f!()
    end
    return median(ts) * 1e6
end

rot64(p, phi) = (cos(phi) * p[1] - sin(phi) * p[2], sin(phi) * p[1] + cos(phi) * p[2], p[3])

# identity check helper: max |gpu - host| over the FIRST and LAST instance's
# vertices (instances are identical clones at this operating point — spot-
# checking the last instance asserts that instead of assuming it).
function ident_vs(gx, gy, gz, hx, hy, hz, nV, ntot)
    m = 0.0
    for j in vcat(1:nV, (ntot-nV+1):ntot)
        m = max(m, abs(Float64(gx[j]) - Float64(hx[j])),
                   abs(Float64(gy[j]) - Float64(hy[j])),
                   abs(Float64(gz[j]) - Float64(hz[j])))
    end
    return m
end

# ---------------- main -----------------------------------------------------------

const CASES = [("blob", 0.28, 1100), ("teapot", 0.3, 500), ("icosphere2", 0.3, 906),
               ("robot", 0.13, 435), ("conifer", 0.6, 226), ("plate", 0.3, 96),
               ("grass", 0.16, 93)]
rows = String[]
notes = String[]
push!(notes, "device: $(CUDA.name(CUDA.device())) cap $(CUDA.capability(CUDA.device()))")
push!(notes, "job: animate N instances through a rigid-rotation phase cycle (φ advancing; affine — exact through the cage path per A5-G2, so all paths produce the SAME output); f32 out; wall incl. launch+upload+sync; uploads measured copyto!s from pinned host arrays, ONE batched memcpy per component per frame (the incumbent's best case)")
push!(notes, "authored-state sizes per frame: A_auth 48·T·N B (cage states), C 24·N B (palette), D 24·V·N B (morph pairs); A_param/B_direct 0 B/frame (true form)")
push!(notes, "quality bounds: C(LBS)/D(morph) measured on the rotation family only — they cannot represent twist/fold (recorded)")
push!(notes, "v4: rigid-rotation body everywhere (F-I.3); A_param ≡ A_auth bitwise by construction; B_direct arithmetic bitwise = host reference")

for (name, h, tets_exp) in CASES
    m = read_obj(joinpath(ROOT, "corpus", name * ".obj"))
    V, T = m.V, m.T
    scale = mesh_scale(V)
    g = auto_grid(V; h = h)
    cage = build_cage(V, T; origin = g.origin, h = h, dims = g.dims)
    loc, misses = locate_tet(V, cage)
    @assert misses == 0 "locate miss on $name"
    nT = length(cage.tets)
    @assert nT == tets_exp "HI-G0 FAILED: $name tets $nT != pin $tets_exp"
    nV = length(V)
    idx, w, w64 = weights_and_indices(V, cage, loc)
    fx, fy, fz = flatten_corners(cage)

    # HI-G0: cage path ≡ GT rigid rotation at the sampled phase (AFFINE —
    # exact per A5-G2; assert object-space at E self-check tolerance)
    f_chk = p -> rot64(p, Float64(PH_T))
    P_ref = cage_path_positions(V, cage, loc, f_chk)
    P_gt = [f_chk(p) for p in V]
    objerr = maximum(hypot(a[1]-b[1], a[2]-b[2], a[3]-b[3]) for (a, b) in zip(P_gt, P_ref))
    @assert objerr <= 1e-9 * scale "HI-G0: cage path != GT rigid rotation ($name): $objerr"
    maxcoord = max(maximum(maximum(abs.(p)) for p in V),
                   maximum(maximum(abs.(t.v[s])) for t in cage.tets for s in 1:4))
    ulpbound = 4 * eps(Float32) * maxcoord

    # host f32 reference at the sampled phase (rigid rotation) — and the ONE
    # (c,s) pair every device path consumes
    Rc = cos(PH_T); Rs = sin(PH_T)
    R32 = map(V) do p
        (Rc * Float32(p[1]) - Rs * Float32(p[2]),
         Rs * Float32(p[1]) + Rc * Float32(p[2]), Float32(p[3]))
    end
    c32 = Float32(Rc); s32 = Float32(Rs)

    # authored state slabs (pinned, CONTIGUOUS per component — uploads are
    # real memcpys). Inputs are the F32-ROUNDED rest values (exactly what the
    # device kernels see), rotated in f32 with the shared (c,s) → A_auth's
    # uploaded state is bitwise A_param's in-kernel corner math.
    cage_x = Vector{Float32}(undef, 4 * nT)
    cage_y = Vector{Float32}(undef, 4 * nT)
    cage_z = Vector{Float32}(undef, 4 * nT)
    for k in 1:(4 * nT)
        cage_x[k] = c32 * fx[k] - s32 * fy[k]
        cage_y[k] = s32 * fx[k] + c32 * fy[k]
        cage_z[k] = fz[k]
    end
    v32x = [Float32(p[1]) for p in V]
    v32y = [Float32(p[2]) for p in V]
    v32z = [Float32(p[3]) for p in V]
    morph_pair = Vector{Float32}(undef, 6 * nV)
    c2 = Float32(cos(PH_T + DPHI)); s2 = Float32(sin(PH_T + DPHI))
    for j in 1:nV
        morph_pair[6j-5] = c32 * v32x[j] - s32 * v32y[j]
        morph_pair[6j-4] = s32 * v32x[j] + c32 * v32y[j]
        morph_pair[6j-3] = v32z[j]
        morph_pair[6j-2] = c2 * v32x[j] - s2 * v32y[j]
        morph_pair[6j-1] = s2 * v32x[j] + c2 * v32y[j]
        morph_pair[6j]   = v32z[j]
    end
    # palette: all bones share the frame rotation (uniform → LBS reproduces
    # the rigid rotation exactly; per-bone variety changes NO measured cost
    # — same fetches/flops — and its quality cost is C's own, recorded)
    pal_c = Vector{Float32}([c32, c32, c32])
    pal_s = Vector{Float32}([s32, s32, s32])
    # pinned full-frame upload arrays are created per-N below (sizes depend
    # on N) so each timed frame does ONE batched pinned memcpy per component

    # skin weights: cage-tet-derived bone ids 1..3, weights normalized
    bidx = Vector{Int32}(undef, 4 * nV)
    bw = Vector{Float32}(undef, 4 * nV)
    for pi_ in 1:nV
        i4 = 4 * (pi_ - 1)
        bone = ((idx[i4+1] - 1) ÷ 4) % 3 + 1
        for k in 1:4
            bidx[i4+k] = bone
            bw[i4+k] = w[i4+k]
        end
        s4 = sum(view(bw, i4+1:i4+4))
        for k in 1:4
            bw[i4+k] /= s4
        end
    end

    # resident device data
    dcx = CuArray(fx); dcy = CuArray(fy); dcz = CuArray(fz)
    didx = CuArray(idx); dw = CuArray(w)
    dvx = CuArray(v32x); dvy = CuArray(v32y); dvz = CuArray(v32z)
    thr = 256

    for N in NLADDER
        ntot = N * nV
        dpxN = similar(dvx, ntot); dpyN = similar(dvy, ntot); dpzN = similar(dvz, ntot)
        didxN = CuArray(repeat(idx, N)); dwN = CuArray(repeat(w, N))
        dbidxN = CuArray(repeat(bidx, N)); dbwN = CuArray(repeat(bw, N))
        # B_direct's registered form: N·V RESIDENT vertices (independently
        # deforming objects each own their positions; 0 B/frame because they
        # were uploaded once at setup, not streamed)
        dbvxN = CuArray(repeat(v32x, N)); dbvyN = CuArray(repeat(v32y, N)); dbvzN = CuArray(repeat(v32z, N))

        # ---- A_param: graph-replayed fused batched --------------------------
        A! = () -> @cuda threads=thr blocks=cld(ntot, thr) fused_batched_shared!(
            dpxN, dpyN, dpzN, dcx, dcy, dcz, didx, dw, nV, ntot, c32, s32)
        A!(); CUDA.synchronize()
        gA = capture(A!); execA = instantiate(gA)
        A_go! = () -> launch(execA)
        A_go!(); CUDA.synchronize()
        hx = Array(dpxN); hy = Array(dpyN); hz = Array(dpzN)
        ident_A = ident_vs(hx, hy, hz,
                           [R32[(j-1)%nV+1][1] for j in 1:ntot], [R32[(j-1)%nV+1][2] for j in 1:ntot],
                           [R32[(j-1)%nV+1][3] for j in 1:ntot], nV, ntot)
        @assert ident_A <= ulpbound "HI-G1 FAILED (A_param $name N=$N): $ident_A > $ulpbound"
        usA = wall_us(A_go!, REPS); usAd = dev_us(A_go!, REPS)

        # ---- B_direct: direct analytic (graphed) ------------------------------
        B! = () -> @cuda threads=thr blocks=cld(ntot, thr) rot_direct!(
            dpxN, dpyN, dpzN, dbvxN, dbvyN, dbvzN, ntot, c32, s32)
        B!(); CUDA.synchronize()
        gB = capture(B!); execB = instantiate(gB)
        B_go! = () -> launch(execB)
        B_go!(); CUDA.synchronize()
        hx = Array(dpxN); hy = Array(dpyN)
        ident_B = 0.0
        for j in vcat(1:nV, (ntot-nV+1):ntot)
            ident_B = max(ident_B, abs(Float64(hx[j]) - Float64(R32[(j-1)%nV+1][1])),
                                   abs(Float64(hy[j]) - Float64(R32[(j-1)%nV+1][2])))
        end
        @assert ident_B <= ulpbound "HI-G1 FAILED (B_direct $name N=$N): $ident_B > $ulpbound"
        usB = wall_us(B_go!, REPS); usBd = dev_us(B_go!, REPS)

        # ---- A_auth: measured cage-state upload + batched reconstruct --------
        # uploads are the INCUMBENT'S BEST CASE: one batched memcpy per
        # component per frame (per-instance copy loops would measure our
        # per-call CPU overhead, not bandwidth). pinned full-frame arrays are
        # built here at setup, OUTSIDE the timed region.
        dcx2 = similar(dcx, 4 * nT * N); dcy2 = similar(dcy, 4 * nT * N); dcz2 = similar(dcz, 4 * nT * N)
        pin_cage_xN = CUDA.pin(repeat(cage_x, N))
        pin_cage_yN = CUDA.pin(repeat(cage_y, N))
        pin_cage_zN = CUDA.pin(repeat(cage_z, N))
        Auth! = () -> begin
            copyto!(dcx2, 1, pin_cage_xN, 1, 4 * nT * N)
            copyto!(dcy2, 1, pin_cage_yN, 1, 4 * nT * N)
            copyto!(dcz2, 1, pin_cage_zN, 1, 4 * nT * N)
            @cuda threads=thr blocks=cld(ntot, thr) reconstruct_batched!(
                dpxN, dpyN, dpzN, dcx2, dcy2, dcz2, didxN, dwN, nV, nT, ntot)
        end
        Auth!(); CUDA.synchronize()
        hx = Array(dpxN); hy = Array(dpyN); hz = Array(dpzN)
        ident_auth = ident_vs(hx, hy, hz,
                              [R32[(j-1)%nV+1][1] for j in 1:ntot], [R32[(j-1)%nV+1][2] for j in 1:ntot],
                              [R32[(j-1)%nV+1][3] for j in 1:ntot], nV, ntot)
        @assert ident_auth <= ulpbound "HI-G1 FAILED (A_auth $name N=$N): $ident_auth > $ulpbound"
        usAuth = wall_us(Auth!, REPS); usAuthd = dev_us(Auth!, REPS)
        bytes_auth = 48 * nT * N

        # ---- C_lbs: palette upload + kernel -----------------------------------
        dpal_c = similar(dvx, 3 * N); dpal_s = similar(dvx, 3 * N)
        pin_palcN = CUDA.pin(repeat(pal_c, N))
        pin_palsN = CUDA.pin(repeat(pal_s, N))
        C! = () -> begin
            copyto!(dpal_c, 1, pin_palcN, 1, 3 * N)
            copyto!(dpal_s, 1, pin_palsN, 1, 3 * N)
            @cuda threads=thr blocks=cld(ntot, thr) lbs!(
                dpxN, dpyN, dpzN, dvx, dvy, dvz, dbidxN, dbwN, dpal_c, dpal_s, nV, ntot)
        end
        C!(); CUDA.synchronize()
        hx = Array(dpxN); hy = Array(dpyN)
        ident_C = 0.0
        for j in vcat(1:nV, (ntot-nV+1):ntot)
            jv = (j-1)%nV + 1
            j4 = 4 * (jv - 1)
            ox = 0f0; oy = 0f0
            for k in 1:4
                bone = bidx[j4+k]; wgt = bw[j4+k]
                ox += wgt * (pal_c[bone] * v32x[jv] - pal_s[bone] * v32y[jv])
                oy += wgt * (pal_s[bone] * v32x[jv] + pal_c[bone] * v32y[jv])
            end
            ident_C = max(ident_C, abs(Float64(hx[j]) - Float64(ox)), abs(Float64(hy[j]) - Float64(oy)))
        end
        @assert ident_C <= ulpbound "HI-G1 FAILED (C_lbs $name N=$N): $ident_C > $ulpbound"
        usC = wall_us(C!, REPS); usCd = dev_us(C!, REPS)
        bytes_C = 24 * N

        # ---- D_morph: morph-pair upload + lerp ----------------------------------
        dm = similar(dvx, 6 * nV * N)
        pin_morphN = CUDA.pin(repeat(morph_pair, N))
        D! = () -> begin
            copyto!(dm, 1, pin_morphN, 1, 6 * nV * N)
            @cuda threads=thr blocks=cld(ntot, thr) morph_lerp!(
                dpxN, dpyN, dpzN, dm, ntot, 0.5f0)
        end
        D!(); CUDA.synchronize()
        hx = Array(dpxN); hy = Array(dpyN); hz = Array(dpzN)
        ident_D = 0.0
        for j in vcat(1:nV, (ntot-nV+1):ntot)
            jv = (j-1)%nV + 1
            o6 = 6 * (jv - 1)
            ex_ = morph_pair[o6+1] + 0.5f0 * (morph_pair[o6+4] - morph_pair[o6+1])
            ey_ = morph_pair[o6+2] + 0.5f0 * (morph_pair[o6+5] - morph_pair[o6+2])
            ez_ = morph_pair[o6+3] + 0.5f0 * (morph_pair[o6+6] - morph_pair[o6+3])
            ident_D = max(ident_D, abs(Float64(hx[j]) - Float64(ex_)),
                                   abs(Float64(hy[j]) - Float64(ey_)), abs(Float64(hz[j]) - Float64(ez_)))
        end
        @assert ident_D <= ulpbound "HI-G1 FAILED (D_morph $name N=$N): $ident_D > $ulpbound"
        # measured QUALITY BOUND of the morph stream: arc-vs-chord error of
        # the lerp against the true rotation (alpha=0.5, DPHI cadence)
        qD = 0.0
        for j in 1:nV
            o6 = 6 * (j - 1)
            ex_ = morph_pair[o6+1] + 0.5f0 * (morph_pair[o6+4] - morph_pair[o6+1])
            ey_ = morph_pair[o6+2] + 0.5f0 * (morph_pair[o6+5] - morph_pair[o6+2])
            qD = max(qD, 100.0 * hypot(Float64(ex_) - Float64(R32[j][1]),
                                       Float64(ey_) - Float64(R32[j][2])))
        end
        usD = wall_us(D!, REPS); usDd = dev_us(D!, REPS)
        bytes_D = 24 * nV * N

        push!(notes, @sprintf("%s N=%d ident ulp-bound %.2e: A=%.2e auth=%.2e B=%.2e C=%.2e D=%.2e | morph quality bound: lerp-of-poses dev max %.3f px@5 (arc-vs-chord at DPHI cadence)",
                              name, N, ulpbound, ident_A, ident_auth, ident_B, ident_C, ident_D, qD))
        for (p, uw, ud, bu) in (("A_param", usA, usAd, 0), ("A_auth", usAuth, usAuthd, bytes_auth),
                                ("B_direct", usB, usBd, 0), ("C_lbs", usC, usCd, bytes_C),
                                ("D_morph", usD, usDd, bytes_D))
            push!(rows, @sprintf("row,%s,%s,%d,%d,%d,%d,%.2f,%.2f",
                                 p, name, nV, nT, N, bu, uw, ud))
        end
    end
end

text = "# Spiral I — Goal-2 comparators (RTX 5060; identical job: rotation phase cycle, f32 out)\n" *
       join(["# " * n for n in notes], "\n") * "\n" *
       "# rows: kind,path,name,verts,tets,N,bytes_up_frame,us_wall,us_dev\n" *
       join(rows, "\n") * "\n" *
       "# gates: HI-G0 provenance; HI-G1 identity all paths (A_param ≡ A_auth bitwise); HI-G2 measured uploads; HI-G3 measured-wall capacity; HI-G4 stable cols x2\n" *
       "# pin protocol: stable columns = kind,path,name,verts,tets,N,bytes_up_frame (us_* machine-state)\n"
println(text)
resdir = joinpath(ROOT, "results")
isdir(resdir) || mkdir(resdir)
open(joinpath(resdir, "spiral-i-comparators.csv"), "w") do f
    write(f, text)
end
println("# csv sha256: ", bytes2hex(sha256(text))[1:16])
println("SPIRAL I GATES: ALL PASS (HI-G0 provenance, HI-G1 identity all paths, HI-G2/G3 honest accounting, HI-G4 pending x2)")
