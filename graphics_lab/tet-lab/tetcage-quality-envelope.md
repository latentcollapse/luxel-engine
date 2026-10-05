# TetCage Quality Envelope (A5 — deformation fidelity)

Test rig: blob5 (10242 v / 20480 t, smooth), cage via uniform Freudenthal,
GT = analytic deformation on vertices, cage path = barycentric reconstruction
through deformed tets (paper §4 model). Deterministic: sweep CSV hash
`2143581115c1a2c0…`, rerun-identical.

## Mathematical gates (all [MEASURED])

| gate | result |
|---|---|
| G1 identity reconstruction | max err 2.5e-16 (machine epsilon) |
| G2 affine reproduction (30° rotation) | max err 3.5e-16 — piecewise-linear cages are EXACT for affine maps [DERIVED, confirmed] |
| G3 amplitude monotonicity | twist rmse 0.00025 (A=.1) → 0.000995 (A=.4), strictly increasing |
| G4 resolution monotonicity | twist rmse 0.000497 (h=.28) → 0.000126 (h=.14) |

## The fidelity law [MEASURED, clean]

    deformation error ∝ A · C(family) · h²

- **h² exact**: halving h quarters error (measured 4.0× across twist/wind/fold).
- **C(family) tracks curvature**: wind (smooth, C≈0.08×twist) < twist <
  fold (tanh k=3, C≈6×twist). Sharp-gradient deformation is the cost driver.
- Point location: 0 uncaged vertices on blob5 (all vertices inside cage).

## Screen-space envelope (blob5, worst fold A=0.4, h=0.28)

| distance | mean px | p95 px |
|---|---|---|
| 5 | 0.114 | 0.365 |
| 20 | 0.007 | 0.022 |
| 100 | 0.000 | 0.001 |

All sub-pixel at these amplitudes — consistent with the paper's "no visible
differences" claim at medium/high cage resolutions [PAPER §7]. The envelope's
failure threshold was NOT reached by these amplitudes; harsher families
(fold with k≥8, near-topology-change folding) are the A5b follow-up.

## Limitations of this envelope (honest)

- blob5 is SMOOTH — foliage cards and creased geometry will show larger
  C(family) and earlier artifacts (Spiral B corpus classes exist; sweeps pending).
- No bone-skinning GT yet: A5 covers analytic families only [UNKNOWN → bone
  transfer quality remains the paper's own "could be improved" gap].
- Temporal stability (per-frame jitter under animation) not yet measured.

## Defects found by the A5 gates (fixed in-lane)

- **F-A5.1**: `inv3` returned the adjugate without ÷det — a dead inverse that
  A4 never exposed (cached, never consumed). Caught by G1 on first use.
- **F-A5.2**: corner-weight pairing shifted by one (λ₁·v0 instead of λ₁·v1).
  Caught by G1 despite a perfect λ solve — the gates localize faults.
