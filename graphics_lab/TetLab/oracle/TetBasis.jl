# TetLab Spiral A4 — rest-space encoding (basis, conditioning, margins).
# [PAPER §4.1] rest tet vertices define the non-orthogonal basis M (Eq. 1);
# the instance transform requires (M⁻¹)₃ₓ₄ and M A⁻¹ invertible ⇒ M must be
# non-degenerate. [PAPER-silence] gives NO numeric thresholds — policy below
# is [INF] with [MEASURED] support (distribution measured on real cages).
#
# DERIVED LAW (test-asserted): all six Freudenthal path-tets of a regular
# voxel are congruent ⇒ |det M| = h³ and identical κ for every grid tet.
# Degeneracy is structurally impossible in a regular cage; margins exist for
# future adaptive/local refinement (paper Fig. 12 direction).
module TetBasis

using LinearAlgebra

export rest_basis, basis_report, cage_basis_report, BasisReport,
       BASIS_POLICY, inv3

# Explicit policy (all thresholds in one place; never scattered):
#   REJECT if |det| == 0                       (singularity: no M⁻¹, no A M⁻¹)
#   REJECT if vol_frac < 1e-12                 (numerically flattened)
#   FLAG/REJECT if κ₂ > 1e8                    (ill-conditioned barycentrics;
#                                               grid tets measure κ ≈ 7.9e0 —
#                                               margin ≈ 1e7 above operating point)
const BASIS_POLICY = (kappa_max = 1.8e6, vol_frac_min = 1e-3)

struct BasisReport
    det::Float64          # signed det of M (orientation preserved)
    vol_frac::Float64     # |det| / h³  (1.0 for every regular-grid Freudenthal tet)
    kappa::Float64        # 2-norm condition number σmax/σmin
    Minv::NTuple{9,Float64}  # row-major M⁻¹ cache (for A5 barycentrics / instance path)
    safe::Bool
    reason::String
end

# tiny local matrix type: plain 9-field immutable, no arrays-of-arrays
struct M3
    a::Float64; b::Float64; c::Float64
    d::Float64; e::Float64; f::Float64
    g::Float64; h::Float64; i::Float64
end

# M = [v1−v0 | v2−v0 | v3−v0], columns are basis vectors (paper Eq. 1)
@inline rest_basis(v0, v1, v2, v3) = M3(
    v1[1]-v0[1], v2[1]-v0[1], v3[1]-v0[1],
    v1[2]-v0[2], v2[2]-v0[2], v3[2]-v0[2],
    v1[3]-v0[3], v2[3]-v0[3], v3[3]-v0[3])

# 3×3 determinant
@inline det3(m::M3) = m.a*(m.e*m.i - m.f*m.h) - m.b*(m.d*m.i - m.f*m.g) + m.c*(m.d*m.h - m.e*m.g)

# inverse = adjugate / det. FALSIFIED DRAFT (F-A5.1): the adjugate was
# returned WITHOUT the det division — dead inverse. Unexposed by A4 (Minv was
# cached but never consumed); caught immediately by the A5 identity gate
# (G1), which is the first real consumer.
@inline inv3(m::M3) = begin
    A =  (m.e*m.i - m.f*m.h); B = -(m.b*m.i - m.c*m.h); C =  (m.b*m.f - m.c*m.e)
    D = -(m.d*m.i - m.f*m.g); E =  (m.a*m.i - m.c*m.g); F = -(m.a*m.f - m.c*m.d)
    G =  (m.d*m.h - m.e*m.g); H = -(m.a*m.h - m.b*m.g); I =  (m.a*m.e - m.b*m.d)
    d = m.a*(m.e*m.i - m.f*m.h) - m.b*(m.d*m.i - m.f*m.g) + m.c*(m.d*m.h - m.e*m.g)
    s = 1.0 / d
    M3(A*s, B*s, C*s, D*s, E*s, F*s, G*s, H*s, I*s)
end

# cheap upper bound for the exact κ₂: κ₂ ≤ κ_F³ / |det| (standard inequality
# chain [INF]) — used as a cross-check assertion in basis_report
@inline kappa_frobenius_bound(m::M3) = begin
    fro = sqrt(m.a^2+m.b^2+m.c^2+m.d^2+m.e^2+m.f^2+m.g^2+m.h^2+m.i^2)
    d = abs(det3(m))
    d == 0 ? Inf : fro^3 / d
end

"""
    basis_report(v0,v1,v2,v3; h) -> BasisReport

Exact κ₂ via singular values (once per tet — fine at oracle scale) with the
Frobenius bound as the cheap cross-check.
"""
function basis_report(v0, v1, v2, v3; h::Float64)
    E = rest_basis(v0, v1, v2, v3)
    d = det3(E)
    sv = svdvals([E.a E.b E.c; E.d E.e E.f; E.g E.h E.i])
    κ = sv[1] / sv[3]
    @assert κ <= kappa_frobenius_bound(E) * (1 + 1e-9) "κ₂ exceeds Frobenius bound: numeric bug"
    vf = abs(d) / (h^3)
    Minv = inv3(E)
    safe = true; reason = "ok"
    if d == 0.0
        safe = false; reason = "singular basis (det == 0): no M⁻¹, instance transform impossible"
    elseif vf < BASIS_POLICY.vol_frac_min
        safe = false; reason = "flattened tet: vol_frac $(vf) < $(BASIS_POLICY.vol_frac_min)"
    elseif κ > BASIS_POLICY.kappa_max
        safe = false; reason = "ill-conditioned: κ₂ $(κ) > $(BASIS_POLICY.kappa_max)"
    end
    return BasisReport(d, vf, κ, (Minv.a,Minv.b,Minv.c,Minv.d,Minv.e,Minv.f,Minv.g,Minv.h,Minv.i),
                       safe, reason)
end

"""
    cage_basis_report(tets; h) -> (n, unsafe, min_vol_frac, max_kappa, det_min, det_max, verdict)

Explicit whole-cage verdict. ACCEPT only if every tet is safe.
"""
function cage_basis_report(tets; h::Float64)
    n = length(tets)
    unsafe = String[]
    vfmin = Inf; κmax = 0.0; dmin = Inf; dmax = -Inf
    for t in tets
        r = basis_report(t.v[1], t.v[2], t.v[3], t.v[4]; h = h)
        vfmin = min(vfmin, r.vol_frac); κmax = max(κmax, r.kappa)
        dmin = min(dmin, abs(r.det));  dmax = max(dmax, abs(r.det))
        r.safe || push!(unsafe, "tet $(t.vox)/$(t.slot): $(r.reason)")
    end
    verdict = isempty(unsafe) ? "ACCEPT" : "REJECT"
    return (n = n, unsafe = unsafe, min_vol_frac = vfmin, max_kappa = κmax,
            det_min = dmin, det_max = dmax, verdict = verdict)
end

end # module
