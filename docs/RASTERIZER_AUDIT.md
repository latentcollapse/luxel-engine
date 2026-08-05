# Rasterizer audit

Audit of `pipeline/zone_rasterizer.py` (2644 lines, 29 functions) and the
terrain-generation path it drives. Started 2026-08-04. Method as elsewhere:
check the code against its own stated claims, **measure rather than assert**,
and separate verified defects from design concerns.

Companion: [GAEA_PROGRAMME.md](GAEA_PROGRAMME.md) §5.7 (the skirt) and §5.8 (the
displacement-field error), both found before this audit began.

---

## R1 — The corrugation gate measures the wrong axis *(verified)*

**Severity: high.** A gate built to catch exactly the defect now visible in the
arena passes it comfortably, and has been passing it all along.

`_sidewall_corrugation` exists to catch what its docstring calls *"repeated
high-frequency ribs along a terrain-primary wall"*, and `zone_acceptance` fails
a build when `sidewall_corrugation_ratio > 0.04`. The intent is exactly right,
and it is wired up: `codeweald_alpine_arena_v1` declares
`silhouette: continuous_boundary_wall` on two features, so the metric runs.

**Measured on the shipped arena:**

| landform | corrugation ratio | gate (0.04) |
|---|---|---|
| `western_alps` | 0.0084 | pass |
| `eastern_alps` | 0.0187 | pass |

Both pass by a wide margin, while the ribbing is the single most obvious defect
in a screenshot of the border. **This is the §1 signature verbatim — the metric
is healthy while the world is visibly wrong.**

### Why it is blind

Not sensitivity. Reproducing the metric's own detrend (48 bins, 9-bin box,
RMS residual ÷ relief) against pure synthetic ribbing shows it firing hard at
every frequency tested — 2 lobes across a feature already scores 0.078, well
past the 0.04 gate. The detector is not too coarse.

**It measures the wrong quantity.** The metric samples *median height* along the
long axis inside fixed cross-axis bands, and normalises by the feature's total
relief. The actual defect is not height ribbing along a profile — it is the wall
**toe advancing and retreating in plan**.

Measured directly from the shipped heightfield (1025² over 256 m, toe = first
cell above floor + 8 m, detrended with an 81-sample window):

| border | toe ripple σ | peak-to-peak | dominant wavelength |
|---|---|---|---|
| west | **16.2 m** | **215.5 m** | 15 m |
| east | **8.4 m** | **87.3 m** | 28 m |

A ±16 m planform wander at a 15–28 m wavelength is precisely the row of barrels.
It barely moves the *median height* inside a longitudinal band, so a metric
reading height variation normalised by 95 m of massif relief reports 0.008.

The σ also independently corroborates §5.8: `spur_amplitude_m` defaults to 24 m,
and a heavily one-sided ±24 m displacement is exactly what produces a 16 m
planform σ.

### The fix

Measure the toe, not the skyline. Extract the toe polyline along each boundary,
detrend it, and gate on its ripple amplitude as a fraction of the wilderness
margin — a wall whose toe wanders by two thirds of the margin has eaten the
wilderness it was supposed to leave. Roughly twenty lines, and it measures the
thing that is actually wrong.

Keep the existing height metric. It is not wrong, it is incomplete: a wall can
corrugate in elevation *or* in plan, and only one of the two is currently
watched.

### Why it matters beyond this defect

`_sidewall_corrugation` is a well-written detector with a correct docstring,
wired into a real gate with a sensible threshold, that has never once fired on
the defect it was built for. **A gate nobody has seen fail is not evidence of
health.** Every metric in this pipeline deserves the same question: has it ever
gone red, and if not, is that because the world is good or because it is
pointed at the wrong axis?

---

## Still to audit

- `_border_rampart` beyond §5.8 — the slope limit for the vertical slot.
- `_surrounding_massif` — the massif carve, already known defective (§1).
- `_carve_hydrology` / `hydrology.py` — the river valleys.
- `_scattered_summits`, `_grade_corridors`, `_limit_profile_grade`.
- `erosion.py` (514 lines).
- The remaining acceptance gates, against the R1 question: has this ever failed?
