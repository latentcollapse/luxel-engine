# TetCage CPU Ray Census — Spiral C8

Deterministic CPU ray-caster against the clipped μMesh soups, emulating the
instance-transform hit model exactly: each tet's clipped triangle soup is
what its micro-BLAS would contain; every ray is tested against every soup it
reaches (voxel-DDA prefilter + exact Möller–Trumbore, inclusive edges), all
hits kept and clustered. No RNG anywhere; ray populations are deterministic.

Driver: `oracle/spiral_c8_ray_census.jl` (v3).
Results: `results/spiral-c8-ray-census.csv`, sha256 `94c7782c0917e340` (16-hex prefix).
Determinism token: the driver-printed data hash, identical across fresh
processes (verified twice; file bytes additionally carry two comment header
lines, same convention as the A5/C CSVs).
Instrumentation probe: `oracle/probe_plate_mult.jl` (raw-hit dumps backing the
duplicate-intersection attribution below).

## Populations

| population | construction | purpose |
|---|---|---|
| GENERIC | 20k Fibonacci-distributed rays from sphere center (paper's experiment); ortho grids + grazing cones for plate | paper-comparable escape rate |
| VERTEX | one ray through every clipped-vertex position | worst case: rays aimed exactly at shared-provenance points |
| GAP | one ray through the midpoint of every measured copy-pair disagreement | aimed exactly into the seam band |
| ALL | union | totals |

Escape = zero hits anywhere. Duplicates = extra hits clustered within 3·gap_max
of the first hit (seam-band scale).

## Finding 1 — the paper's headline reproduces: GENERIC escapes are ZERO

`sphere` (icosphere5, r=1, h=0.22, 1770 tets — paper-scaled sphere control):

| prec | ε | GENERIC rays | escapes |
|---|---|---|---|
| f64 | 0 | 20000 | **0** |
| f64 | 2.5e-6 | 20000 | **0** |
| f32 | 0 | 20000 | **0** |
| f32 | 2.5e-6 | 20000 | **0** |

The paper's "0.01% escape without mitigation, 0 with" [PAPER §4.1] lives
entirely in ADVERSARIAL populations at CPU soup level:

| population | f64 ε=0 | f64 ε=2.5e-6 | f32 ε=0 | f32 ε=2.5e-6 |
|---|---|---|---|---|
| sphere VERTEX | 13.45% | 5.70% | 10.41% | 1.28% |
| sphere GAP | 15.89% | 2.08% | (0 gap keys) | 0.25% |
| sphere ALL | 9.58% | 3.50% | 6.27% | 0.70% |

ε-growth cuts adversarial escapes 2.4–8×; f32 storage cuts them further
(seam band swallowed by rounding, consistent with Turn C's F-C-f32
falsification). Sphere-32/GAP population is empty at ε=0 (zero gaps to aim at).

## Finding 2 — escapes are POSITION-dependent, exactly as the paper says

The escape mechanism is not a uniform leak rate: it fires only where a ray
passes through the exact disagreement band between two tets' independently
computed copies of a shared-face point. Generic rays (measure-zero chance of
hitting a ~1e-15-wide band at ε=0) never escape; rays AIMED at clipped
vertices (which by construction sit on shared faces) escape at 10–13%.
This validates the paper's design choice to fix the mechanism at its source
(grow the tets) rather than in traversal.

## Finding 3 — duplicate intersections are real, bounded, and structural

Instrumented raw-hit dump (`probe_plate_mult.jl`): a plate vertex ray struck
the SAME surface point (t agreeing to ~1e-17) **6 times** — once per adjacent
tet's independently-clipped fragment. Census totals (mean hits per cluster /
max / fraction of all hits that are duplicates):

| case | mean_mult | max_mult | dup_frac |
|---|---|---|---|
| sphere GENERIC (all ε, both prec) | 1.000–1.001 | 1–2 | ≤0.0001 |
| sphere VERTEX ε=0 f64 | 2.157 | 11 | 0.537 |
| plate GENERIC ε=0 f64 | 1.181 | 8 | 0.153 |
| plate VERTEX ε=0 f64 | 5.050 | 9 | 0.802 |
| plate VERTEX ε=2.5e-6 | 4.644 | 16 | 0.785 |

Interpretation [DERIVED from probe]: these are per-tet PROVENANCE FRAGMENTS
of one surface point — the normal operating mode of autonomous per-tet
micro-BLASes, present even at ε=0 (where clip output is deduplicated at the
storage level, the RAY still crosses every tet whose soup contains the
point). ε-growth raises max multiplicity 9→16 (plate) by adding the grown
ghost overlaps. Sphere GENERIC rays essentially never see duplicates.

## Finding 4 — representation error vs ε artifacts (Spiral D input)

dev = |hit − analytic surface| percentiles over all hits:

- sphere: dev_p95 = 2.51e-4 ≈ chord sagitta edge²/8r for edge≈√2·0.22 (the
  piecewise-linear μMesh surface, a REPRESENTATION property, invariant in ε).
- plate: dev_p95 = 6.9e-4, dev_max 9.2e-4–2.8e-3 (ripple-interpolation error,
  also ε-invariant).
- ε=1e-4 does NOT inflate hit deviation (sphere GENERIC identical) — over-
  coverage adds duplicate hits, not displaced ones.

## C9 verdict — unique edge ownership REJECTED

The cure (cross-tet hit dedup / unique per-edge ownership) would break the
instance-transform architecture's core premise: each tet's μMesh is an
independent BLAS; correctness of ONE instance's traversal cannot depend on
global identity state. The disease is bounded (sphere generic 1.00×, worst
adversarial 5–6 hits on ONE point, cost = redundant intersection tests, no
wrong shading: duplicates agree to float noise and cluster to a single hit
in any practical any-hit/first-hit shader). The paper's own mitigation
(ε-growth) is the correct lever, and its cost is now measured (C7).

## Scope and honesty

- CPU soup census, not a GPU traversal: no BVH, no hardware units, no
  motion. It lower-bounds nothing and proves nothing about TLAS/BLAS behavior;
  it isolates the GEOMETRY mechanism, which it reproduces.
- The plate at h=0.40 is a 6×6×4 cage; thin-geometry cage sensitivity is
  C7's subject (this census ran at the canonical plate h).
- Ray populations are adversarial by construction; GENERIC rates are the
  renderer-like numbers.
