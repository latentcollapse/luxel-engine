# WGE tooling upgrade plan

Written 2026-07-31, from defects actually hit rather than from speculation. Every
item below cites the incident that motivated it.

The organising observation: WGE's failures are almost never missing features.
They are **disconnections** — a component built correctly and left one wire
short — and **evidence that lies**. A human team catches these immediately
because someone drags the slider and looks. Nobody drags anything here. Tooling
has to do the dragging.

Ordered by leverage. Items 1 and 2 protect the correctness of everything else
and should land first.

---

## 1. Provenance for tools, not just data

**Landed 2026-07-31.** `pipeline/build_viewer.py` computes a content-hash digest
over the viewer's Rust sources (`world_core/{apps,crates}/**/*.rs`) and bakes it
into the binary at build time via `build.rs` + `CODEWEALD_SOURCE_DIGEST`; the
binary self-reports it via `--provenance`. `capture_bevy.py` queries that digest,
recomputes the expected one from current sources, and refuses (`parser.error`,
never a silent pass) on any mismatch or on `"unknown"` (a binary built with plain
`cargo build`/`cargo run`, bypassing the wrapper). The verified digest is also
written into every capture report as `viewer_source_digest`, so the report itself
is self-certifying rather than requiring a reader to separately go re-verify it.
Content-hash based, not mtime-based, per the incident below: mtimes are fragile
against git checkouts, rsync, and backup restores that reset mtimes without
touching content. Default capture path also moved from `target/debug` to
`target/release`, matching the profile the incident says was actually in use.
Covers the viewer binary only; the shader (loaded fresh from disk at runtime, not
compiled in) and other pipeline generators are not yet under this discipline —
next targets if this pattern proves out.

**Incident.** `capture_bevy.py` defaulted to `target/debug/codeweald-world-viewer`
and never built it. Work was being compiled `--release`. Every capture for a full
day ran a binary predating the change under test, and every one reported
"passed". A measured improvement was later found to have come from a shader edit
(loaded from disk at runtime) while the Rust half never executed at all.

**What exists.** `render_plan.json` already carries `zone_spec_sha256`,
`asset_plan_sha256`, `heightfield_sha256`, `terrain_manifest_sha256`. The pattern
is right and stops at *data*.

**The upgrade.** Extend the same discipline to *tools*. Every produced artifact
records the digest of the binary, shader, and generator version that made it. Any
consumer refuses provenance it cannot match. `capture_bevy._stale_viewer_sources`
is the seed of this — generalise from "is the viewer stale" to "does this
artifact's toolchain digest match the one now on disk".

**Why first.** Every other measurement in the system is only as trustworthy as
the artifact it was taken from.

---

## 2. Reachability manifest (generalised no-op detection)

**Landed 2026-07-31.** `pipeline/reachability.py`. Every advertised composition
scalar declares the artifact it controls and the stage that owns it
(`DECLARATIONS`); the sweep perturbs each one to the far end of its own bounds,
re-rasterises, and asserts the heightfield digest moves. `wge_critic
--reachability` reports failures as non-actionable `parameter_unreachable`
findings (a dead wire is fixed in the owning stage, never by editing the DSL
value failing to reach it). Also implements the extension: `module_reachability`
flags pipeline modules nothing imports and nothing names.

Three design points worth keeping:
- **Keyed by (parameter, profile), not parameter.** The rasterizer branches on
  landform profile, so a scalar can be live for `alpine_jagged_massif` and dead
  for `scattered_crag_field`. A per-parameter verdict reports "reaches" and hides
  half the defect behind the working half. Verified against a deliberately
  half-wired scalar.
- **Control check, and an explicit INCONCLUSIVE verdict.** A landform that cannot
  move the heightfield under *any* value — degenerate polygon, mask falling
  between sample points, zero elevation delta — makes every scalar on it look
  dead. The sweep first perturbs `elevation_m` (load-bearing, and not under test)
  to prove the landform is live at all; if it isn't, that landform is reported as
  inconclusive and excluded from the verdict rather than counted as a failure.
  Found the hard way: the first test fixture used normalized polygon coordinates
  where a compiled ZoneSpec uses world metres, and all four correctly-wired
  scalars reported dead. Without the guard this tool's first real output would
  have sent someone hunting a wire that was never broken.
- **Undeclared parameters are themselves a finding.** A knob nobody declared is a
  knob nobody sweeps, which is how a parameter goes decorative unnoticed.

Verified by simulating the original incident (freezing a scalar inside
`_composition_scalars`): the sweep names exactly the frozen parameter, on exactly
the affected profiles, and clears the other three. 18 tests, run against a
synthetic spec at resolution 65 so the suite costs seconds rather than minutes.

**Still uncovered:** only heightfield-controlling scalars are declared. The
categorical composition keys (`silhouette`, `massing`, `surface`, `dressing`) and
the material/asset lanes control other artifacts and need their own declarations
and digests — that is the same work, one artifact over.

**Incidents, three in one day.**
- `zone_rasterizer.py` contained zero references to `spine_count`,
  `along_jitter`, `cross_jitter`. The DSL wrote them faithfully; terrain came out
  byte-identical.
- Terrain material textures were accepted, hashed, loaded, and then admitted by
  the shader only on steep faces at 30% strength.
- `comfy_generate_texture.py` had zero callers anywhere in the repo.

**What exists.** `wge_critic.detect_no_ops` compares two `terrain_manifest.json`
files and reports composition scalars that changed while `heightfield_sha256` did
not. It only watches the heightfield.

**The upgrade.** Every advertised parameter declares which artifact digest it
claims to control. CI perturbs each one in turn and asserts that digest moves. A
parameter that cannot move its own artifact is a defect, reported by name with an
owner. This is mechanical — no judgement, no rendering — and it would have caught
all three incidents before a human looked.

**Extension.** The same sweep finds dead code paths: a generator no build stage
can reach is the same defect one level up.

---

## 3. Sensitivity matrix

**Landed 2026-07-31.** `pipeline/sensitivity.py`. Each knob is driven to the far
end of its authored bounds across every landform at once, rebuilt, recaptured,
and every tracked metric compared to baseline. Cost is dominated by rendering
and one render yields all metrics, so it is one build+capture per knob — four
runs, not thirty-two. `wge_critic` reads the published matrix via
`apply_sensitivity_matrix` and **demotes only**: a metric the matrix says nobody
owns becomes non-actionable and loses its patches; a finding whose every
proposed knob was measured powerless is flagged misattributed. It never
promotes — inventing a repair from a correlation is how a loop starts chasing
coincidences.

**Measured on `codeweald_alpine_arena_v1`** (`sensitivity_matrix.json`):

| metric | owners | strongest |
|---|---|---|
| `water_fraction` | all four | `spine_count` **+79.7%** |
| `dark_foreground_fraction` | jitters + bias | `elevation_bias` **−57.7%** |
| `foliage_fraction` | jitters + bias | `elevation_bias` +13.1% |
| `road_fraction` | `elevation_bias` | +6.3% |
| `foreground_edge_density` | `elevation_bias` | −4.7% |
| `detail_density` | `elevation_bias` | −4.0% |
| `surface_variation_coverage` | `elevation_bias` | +3.2% |
| `foreground_dynamic_range` | **none** | best is +0.1% |

Two results worth keeping. `spine_count`'s dominant effect is **drainage**, not
skyline — more ridges means more inter-ridge valleys means more standing water —
which is a knob doing something real that nobody would have guessed from its
name. And `foreground_dynamic_range` has **no DSL owner at all**, while the
critic's one hardcoded actionable rule maps exactly that metric to
`elevation_bias`. The rule is not currently firing (the metric passes today), so
this is a latent misattribution rather than an active one, but it is precisely
the incident below waiting to happen again, and the matrix now defuses it
automatically.

**Honest limits, stated because they are easy to over-read:**
- An entry means "driving this knob to its bound moved this metric by X%". That
  is a bound on authority, **not a derivative** — landforms start at different
  values so the step size varies per landform.
- Each knob is measured in **one direction only** (toward the far bound), so an
  asymmetric response is invisible. `elevation_bias` was measured going *down*
  (0.68→0.0) while the critic's rule wants to push it up.
- `cross_jitter` is **gate-limited**: its declared bound (0.5) fails the terrain
  accessibility gate outright, so it was measured at 0.4335 after backoff. Its
  row is a floor, not a ceiling. See [D1](DEBT_LEDGER.md) — the declared bounds
  are not the buildable bounds.

**Three defects found while building it**, all fixed and regression-tested:
1. `apply_to_scaffold` **silently discarded patches that matched nothing**. The
   authoring surface names the spine count `spines`; the ZoneSpec, manifest and
   rasterizer call it `spine_count`. The mismatched patch vanished without a
   word and produced a matrix row of exactly `0.000000` across all eight
   metrics — a fabricated "controls nothing" for a knob that moves
   `water_fraction` by 79.7%. It now raises. This is the same no-op defect class
   as items 1-2, one level up: in the *repair mechanism* rather than the knobs.
2. Two acceptance-report schemas share one filename (`--suite` nests metrics
   under `overview_acceptance`), so every baseline read as zero and a
   `1.0`-on-missing-denominator fallback manufactured a matrix of uniform
   `+100%` entries. Both the reader and the fallback are fixed; unmeasurable is
   now its own reported state, and a wholly-absent baseline refuses rather than
   publishes. See [D10](DEBT_LEDGER.md).
3. Recording `elevation_bias` in the terrain manifest for the first time made
   `detect_no_ops` fire against it, because a key absent from the previous
   manifest compared unequal to its new value — reporting a schema addition as a
   dead knob. Now only keys present in both manifests are compared.

**Incident.** The critic attributed a 6.9x `foreground_edge_density` shortfall to
landform composition and emitted jitter/spine repairs for it. Driving every
composition scalar to its bound moved the metric by 0.3% — while pushing further
broke the traversability gate, proving the knobs had real geometric authority and
none over that metric. The diagnosis was a misattribution that could have
consumed unlimited loop iterations.

**The upgrade.** For every (knob, metric) pair, perturb and record the
derivative. Publish the matrix. Then:
- A metric with no knob above threshold has no DSL owner and must be reported as
  non-actionable, automatically, instead of by hand as it is now.
- A knob that moves no metric is item 2's defect.
- The critic reads the matrix instead of hardcoding which knob fixes what.

Cheap to build: the critic already enumerates both sides.

---

## 4. Acceptance at the conditions of use

**Incident.** Terrain materials were assessed at 1:1 when a screen pixel covers
36–71 texels at overview distance. Two accepted materials (`windpacked_snow`,
`wetland_peat`) were almost entirely fine grain and flattened to a single colour
at any normal viewing distance. Nothing in the pipeline noticed.

**What exists.** `texture_material_pipeline.assess` now reports `coarse_contrast`
and warns below 0.020.

**The upgrade.** Generalise the principle: measure an asset the way it will
actually be sampled. Applies directly to prop LOD selection, foliage cards, and
normal maps, all currently judged in isolation. Each needs its own
conditions-of-use metric, following `coarse_contrast` as the template.

---

## 5. Missing metrics — **SILHOUETTE LANDED 2026-08-01**

`pipeline/silhouette.py`, build stage 21 of 24, emitting `silhouette.json`.

**Measured from the heightfield, not from a render — a deliberate departure
from this item as written.** A rendered skyline is occluded by trees and
buildings, depends on where the camera points, and needs sky segmentation that
fails on a bright horizon. It would mix foliage and material signal into a
number whose whole purpose is to attribute *geometry*. Item 6 had just finished
establishing what that costs.

The measurement is the **horizon profile**: for each of 360 directions, the
highest elevation angle any terrain subtends, from a ring of 8 observers at eye
height. That is what an observer actually sees against the sky.

| | flat | noise | single massif | ridges×2 | ridges×4 | ridges×8 | **real map** |
|---|---|---|---|---|---|---|---|
| relief° | 0.00 | 9.46 | **11.19** | 4.84 | 2.93 | 2.82 | 8.48 |
| peaks/turn | 0.0 | **22.5** | 7.5 | 4.0 | 8.0 | 16.0 | 10.2 |
| coherence | 0.000 | 0.186 | 0.155 | 0.597 | 0.285 | 0.164 | **0.471** |

`horizon_peaks_per_turn` tracks authored ridges exactly — 4/8/16 for 2/4/8 —
which is the property that makes it a usable signal for `spine_count`.

**Two gaming vectors found by probing, both labelled rather than hidden:**
noise beats an eight-ridge world on raw peak count (22.5 vs 16.0), and a single
tall block beside the observer beats four authored ridges on relief (11.19 vs
2.93). Both are honest readings of what those numbers mean; neither is a shape
signal. So **`horizon_coherence` — the share of profile variance in its
strongest harmonic — is the sole optimisation target.** Noise cannot fake it
(0.186) and a lone massif does not win it (0.155).

**Three bugs the guard caught before anything trusted the metric:** a flat world
scored 1.26° relief and 3 peaks (rays leaving the map contributed negative
angles, so the metric described the world's rectangular *boundary*); noise
scored 72.8 peaks (no angular acuity, so grain read as summits); and relief was
briefly listed as an optimisation target before the massif probe existed.

**The real map scores coherence 0.471** — above every synthetic ridge probe
except the trivial 2-ridge case, and 2.5× the noise floor. The alpine arena has
genuinely coherent skyline structure, and the composition knobs now have a
number to move that cannot be won by adding grain.

**Still missing from this item:** macro terrain variation — where a world's
variation sits in the 0.3–4.5 m band `surface_variation_coverage` needs.

### Original entry

**Silhouette complexity.** Nothing measures it. This is why `spine_count` and the
jitters still have no honest optimisation target even now that they reach
geometry. Buildable from the terrain/sky boundary in a render. Until it exists,
the geometry knobs serve nothing.

**Macro terrain variation.** `surface_variation_coverage` needs variation between
roughly 0.3 m and 4.5 m to register at overview scale. Nothing currently reports
where a world's variation sits in that band.

---

## 6. Metric honesty guards — **LANDED 2026-08-01**

`pipeline/metric_honesty.py`. Scores every frame-level metric against fixed
synthetic probes — flat, structured relief, shuffled pixels, three noise
sigmas, and the same content point-sampled vs correctly averaged — and
classifies each as `quality_score`, `coverage_gate`, or `histogram_statistic`.

The verdict is a property of the *metric*, not of any frame, so it is computed
from the live metric functions with a fixed seed. It cannot go stale the way a
checked-in table would, it is deterministic, and it costs 0.5 s once per
process.

**Measured verdicts:**

| metric | structured | noise σ0.02 | filtered | aliased | real capture | verdict |
|---|---|---|---|---|---|---|
| `surface_variation_coverage` | 0.838 | **1.000** | 1.000 | 1.000 | 0.369 | coverage gate |
| `detail_density` | 0.000 | 0.000 | 0.077 | **0.624** | 0.041 | coverage gate |
| `edge_density` | 0.000 | 0.001 | 0.328 | **0.832** | 0.038 | coverage gate |
| `coarse_contrast` | 0.132 | 0.001 | 0.132 | 0.132 | 0.068 | **quality score** |

`filtered` and `aliased` carry identical content and differ only in filtering,
so that column pair is the fair comparison. Aliasing beats correct filtering
**8×** on `detail_density` and 2.5× on `edge_density`. Sigma-0.02 noise aces
`surface_variation_coverage` outright, at 2.7× the real render's score.

`coarse_contrast` is the only one of the four that measures what it claims: it
drops 20× on shuffled pixels and is indifferent to filtering.

**Labelled in its own output**, as the item required: `bevy_visual_acceptance`
emits `metric_kinds`, and `wge_critic.Finding` carries `metric_kind` and prints
a warning when a finding is driven by a coverage gate.

**The live consequence, logged as [D16](DEBT_LEDGER.md):** `wge_critic` drives
repairs toward `detail_density ≥ 0.65 × source`. That target is satisfiable by
aliasing the render harder — a standing incentive to degrade the instrument in
order to pass the gate.

**Validation.** The guard reproduces both recorded incidents from measurement
alone, and the classifier is pinned in both directions — a synthetic
structure-only metric must come back `quality_score`, a synthetic grain counter
must come back `coverage_gate`. A classifier that only ever returned one verdict
would look equally convincing on this map.

### Original entry

**Incidents.** `detail_density` is a cliff at its threshold — pure film grain at
sigma 0.065 scores better than any honest texture. `surface_variation_coverage`
hits 1.000 from sigma 0.02 of noise. An un-mipmapped render scored *higher* on
both metrics than the correctly filtered one, because moire is high-frequency
variation.

**The upgrade.** Every metric ships with its documented gaming vector and, where
possible, an automated adversarial check: score the metric on a noise-injected
version of the same frame and report the ratio. A metric a noise field can ace is
a coverage gate, not a quality score, and must be labelled as such in its own
output.

---

## 7. Artifact identity

**Incident.** `capture_bevy --view player` wrote the player camera over
`bevy_overview.png` and stubbed the overview's acceptance report, because both
default paths ignored `--view`. Two different cameras were then compared with
nothing indicating it had happened.

**Fixed** for capture. **The general rule:** an artifact's filename encodes its
identity, and a writer never produces a path whose name does not match what it
contains. Worth a sweep of the other stages for the same shape.

---

## Not tooling

These are listed so they stop being mistaken for work that is merely undone.

**Representation limits.** A heightfield stores one height per point. Caves,
overhangs, and arches need a second representation emitted into the render plan
as geometry. No detector or model substitutes for choosing one. This is the only
blocked-on-a-decision item in the project.

**The medium gap.** At 0.284 m/px a correctly filtered render carries less
per-pixel contrast than a painter puts down. Part of the distance to the concept
art is the medium, not the material, and no texture work closes it. Targets
derived from painted source art should be discounted accordingly, or measured at
player-camera distance where texture legitimately survives.

**Taste.** Whether a world reads as *this specific place*. Human.

---

## Human visual passes

When a pass is needed, the request states four things and nothing else:

1. The question, as yes/no or pick-one — never "how does this look".
2. What a pass looks like, concretely.
3. What a fail looks like, including the most likely specific failure.
4. The exact named views, already captured.

And say plainly when a pass is *not* needed, so that a request means something.
