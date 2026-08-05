# The Gaea programme — landform by Gaea, playability by WGE

Written 2026-08-04, the night procedural terrain generation was abandoned.
Audited and red-teamed the same night; see §10 for what the audit changed.

Everything marked **verified** was measured and is reproducible. Everything else
is a **plan** and is labelled as such. That distinction is load-bearing: the
previous two terrain attempts both failed while their metrics improved, so a
document that blurs "we ran it" into "we believe it" is how this project loses
another month.

Companion documents: [OPEN_DECISIONS.md](OPEN_DECISIONS.md) item 9 (the decision
and the headless recipe), [DEBT_LEDGER.md](DEBT_LEDGER.md) D26/D30/D31,
[SESSION_HANDOFF_2026-08-04.md](SESSION_HANDOFF_2026-08-04.md).

---

## 1. Why this exists

**Procedural terrain generation failed twice, for the same structural reason.**

The massif thesis was right: *a border built as a distance function reads as a
wall however much relief is layered on it, because real mountains beyond a valley
are not a function of the valley*. But both implementations made the range a
function of the play space anyway.

- The **rampart** was distance-from-play-space with noise on it.
- The **massif carve** was `carved = height + massif * into`, where `into` rises
  with distance from the lanes. Amplitude is therefore a monotonic function of
  distance, so **a nearer summit can never be taller than a farther one**.
  Foothills in front of peaks are mathematically forbidden. 77.6% of the
  wilderness was still ramping, never at full amplitude.
- The **D26 per-cell transition** made it worse. Where the map edge crowded the
  play space the run shortened to 6 m against ~95 m of relief — an 86° face —
  and because the run's plan shape is the play envelope's offset curve (straight
  lane and polygon segments), it extruded those segments into **vertical slabs**.
- **The apron is the fourth instance, and it is still shipping** — this is the
  "skirt" in every screenshot of the arena. See §5.7.

**Every heightfield metric improved while the world got visibly worse.** Tallest
point 58.2 → 94.9 m, mountain area +42%, roughness ×3. Reviewed as *"a silhouette
that's just the same shape extending way far back."* Reverted the same night.

**The arena has no room for mountains.** 256 m map, play rhomboid occupying
**58.9%** of it, p10 of the perimeter with **9.4 m** between the wilderness
margin and the map edge. No transition function invents space that is not there.

### The rule this programme is built on

> Terrain must be generated **without reference to the play space**, then the
> play space carved into it. Anything that derives relief from distance-to-lanes
> reproduces the defect.

**The rule needs teeth, not just assent.** Both failed attempts were built by
people who agreed with it. What was missing was a measurement that says whether
a given heightfield obeys it, and a bound on how far the carving may go. §5.5
supplies the first (radial monotonicity) and §5.6 the second (the edit budget).
An unenforceable rule is a preference, and preferences lose to deadlines.

---

## 2. The split

```
Gaea graph  ──build──▶  heightfield (PNG16)
                              │
                     import_heightfield.py
                       (datum + resample)
                              │
                  ┌───────────┴───────────┐
                  │                       │
          preview (terrain only)   WGE compiler
                  │                 ├─ symmetry measurement
             Bevy viewer            ├─ natural lane routing
                                    ├─ local terrain edits (budgeted)
                                    ├─ enclosure verify + patch
                                    └─ siting / ecology / navmesh
                                              │
                                        Bevy viewer
```

**Gaea owns appearance.** Erosion, drainage, rock, mountain character.

**WGE owns playability.** Lanes, siting, enclosure, navmesh, determinism.
**No terrain generator has any concept of these** — Gaea does not know what a
lane is, and neither do World Machine, World Creator or Instant Terra. This is
not "replacing WGE with Gaea"; it is deleting the one part of WGE that was
reinventing a solved problem badly.

**Considered and declined: Houdini.** It genuinely could do both — heightfield
erosion *and* curve/constraint logic, native Linux, headless `hython`, cheap
indie licence. It would also subsume WGE and discard months of work. Keep it in
reserve if the Gaea↔WGE seam becomes the bottleneck; do not switch now.

---

## 3. What is already built and verified

| piece | state |
|---|---|
| Headless Gaea build under Proton | **VERIFIED 2026-08-04** — Matt's `MountainRange` graph, marked for export by `gaea_terrain.py`, built at 1024² in under a minute and wrote `Height_Out.png`. Recipe: full `Z:` path, space-free filename, no stdout redirect, judge by files not exit code (§3.3) |
| `pipeline/gaea_terrain.py` | **verified** — reads/edits `.terrain` JSON *and inserts nodes*. `--insert-after` spliced an `Erosion2` into Matt's graph, rewired its consumer, and Gaea built the result — including the node's `Flow`/`Wear`/`Deposits` outputs. **Gaea is now a dependency WGE drives, not an application anyone operates** |
| A real heightfield | **was verified** — 1024², 16-bit, 10,848 distinct values. **No longer on disk**: it was written to a temporary buildpath. See §7 — this is the argument for committing heightfields, already costing us the one artifact the decision rests on |
| `pipeline/import_heightfield.py` | **verified** — writes a digest-pinned preview batch at a meshable resolution, with the vertical mapping declared |
| `pipeline/preview_metrics.py` | **verified** — radial monotonicity, symmetry residual, slope, sink density; non-certifying schema |
| `tests/test_gaea_import.py`, `tests/test_preview_metrics.py` | **verified** — 27 cases, green inside the full suite: **565 tests, OK** |
| Preview rendering in Bevy | **verified** — 1025² preview renders, HUD labelled `PREVIEW -- UNCERTIFIED TERRAIN`, confirmed by capture |
| `pipeline/gaea_crop.py` | **verified** — peak-ranked crops → watertight glTF with a PBR material, edge taper, stride decimation. Crops from *eroded* Gaea terrain show arêtes, gullies and convergent peaks |
| Crop libraries in the viewer | **verified** — `WorldMode::CropLibrary`, discovered by the picker, labelled `CROP LIBRARY -- UNCERTIFIED LANDFORM` |
| World picker sidebar | **built** — `Tab` to open; discovery by manifest presence. List logic unit-tested; the on-screen panel still needs one human keypress to confirm |

The invocation, the four blockers and the two dead ends are in
[OPEN_DECISIONS.md](OPEN_DECISIONS.md) item 9. Two things that will otherwise
cost an hour each:

- **Run Gaea from NVMe.** `Swarm --help` is 1 s from `~/.local/share/gaea2`
  against **33 s** from `/mnt/d` with a warm cache. See D30.
- **A graph only builds headlessly if a node carries a `SaveDefinition`.** Only
  3 of the 59 bundled examples do. The rest exit 0 having written nothing, which
  looks exactly like a failure and is not.
- **Two more invocation traps, found during the audit (§3.3): pass the graph as
  a full `Z:` path, and give it a name with no spaces.** A bare relative filename
  exits **0 in six seconds** having written nothing; a name containing spaces
  makes Swarm dump its usage text and exit without building, and all three
  bundled examples that carry a `SaveDefinition` have spaces in their names. With
  a full space-free path the same graph builds. **Never treat a Swarm exit code
  as evidence; check that files appeared.** The driver (§8 step 5) must copy to a
  space-free path and assert on output existence, with a generous wall-clock
  budget and a hard timeout.

### 3.1 The resolution contract — read this before building a graph

The viewer decimates the heightfield by `TERRAIN_STRIDE = 4` to build its mesh
and **refuses any grid where `(resolution - 1) % 4 != 0`**. WGE's own compiler
emits 1025 for exactly that reason.

**Gaea builds at powers of two, and `2**k - 1` is never divisible by 4.** So
512, 1024, 2048 and 4096 are *all* rejected. Before the audit, every import
wrote a preview that no consumer could open, and nothing said so — the file
looked correct and failed at load, which is the failure shape this project keeps
paying for.

`import_heightfield.py` now resamples up to the next compatible grid
(512→513, 1024→1025, 2048→2049) and records `source_resolution` and `resampled`
in the manifest. Build in Gaea at whatever power of two you like; the seam
absorbs it. Do not "fix" this by asking Gaea for 1025 — Swarm's `--resolution`
is not built for it, and a one-row bilinear resample is cheaper and auditable.

### 3.2 The vertical datum

A 16-bit image carries no units, so `--relief-m` is the caller **asserting** how
tall the world is. The mapping is **full scale**: `height_m = raw / 65535 *
relief_m`. The denominator is a property of the encoding, not of the image's
contents.

The importer originally normalised the observed min/max to the full relief. That
meant a build occupying 30% of the range and one occupying 90% imported to the
**same** world — a graph edit that lowered the mountains produced an identical
heightfield. It was the silent-rescale failure the module's own docstring said it
existed to prevent, in the module itself. `--normalize` still offers the stretch
for genuinely unknown-range sources, and the manifest records which rule was
used, because **the same bytes under the two rules are two different worlds and
the digest cannot tell them apart**.

### 3.3 Headless builds are unobservable from an agent session — and two real invocation traps

The audit failed to complete a headless build and initially recorded that as
"the build does not reproduce". **That conclusion was wrong and is retracted.**
Gaea is fine; the *observability* is what is broken, and the difference matters
because one of those framings would have had us debugging the wrong thing.

What is actually established, by filesystem evidence rather than console output:

- **Gaea runs correctly under Proton.** `Gaea.Swarm.exe --help` produced 1664
  bytes of usage — **version 2.3.0.1** — when its output was redirected to a file
  inside the prefix. `cmd.exe` executes in-prefix and writes files.
- **Console output does not survive to an agent session.** Without redirection
  nothing reaches the log, because `script -qec`'s pty is not a real terminal
  here. So an agent gets no progress, no errors, and no stack traces.
- **Redirecting Gaea's stdout is not a workaround — it triggers blocker 4.**
  `> file 2>&1` produces `System.IO.IOException: Invalid handle` from
  `QuadSpinner.Gaea.Swarm.Program`, because redirection destroys the console
  handle Spectre.Console requires. **You can see the output, or you can have a
  working build, but not both from a non-interactive session.**

**Trap 1 — filenames with spaces break argument parsing.** Passing
`Cartography - 3D Map.terrain` makes Swarm dump its usage text and exit without
building, however it is quoted through the `script` → `proton` → `cmd.exe`
layers. **All three of the bundled examples that carry a `SaveDefinition` have
spaces in their names.** Copy the graph to a space-free filename before building;
the driver should do this automatically.

**Trap 2 — a real build is slow, and looks identical to a hang.** The one
invocation that parsed cleanly (`_wge_canyon.terrain`, full `Z:` path) ran
**15 minutes at roughly one core** and was still going when the wrapper timed
out. Given the above, that was almost certainly a legitimate build in progress —
`Canyon River with Sea` is heavy (`Erosion2`, `Trees`, `Sea`, `WaveShine`) — that
the audit killed prematurely. **The 29 s figure does not generalise.**

What follows from this:

- **Verifying the headless build is Matt's task, not an agent's**, and it takes
  about two minutes at a real terminal where Spectre.Console renders. An agent
  can only check whether files appeared, which cannot distinguish "still
  building" from "failed silently".
- **Never treat a Swarm exit code as evidence.** Exit 0 with no files, exit 0
  after a usage dump, and exit 1 were all observed. The driver (§8 step 5) must
  assert on **output file existence**, with a generous wall-clock budget and a
  hard timeout, and must build from a space-free path.
- **§7 is reinforced, not by fragility but by opacity.** A dependency this hard
  to observe is one whose outputs should be committed rather than rebuilt on
  demand.
- **§3's "verified" for the build step stays "worked once".** That is still a
  thin foundation, and step 3 should still confirm a build runs twice — but the
  reason is prudence, not evidence of breakage.


---

## 4. Option A — preview mode in the Bevy viewer

**BUILT 2026-08-04.** A 1025² preview renders, is framed by the overview camera,
and is labelled `PREVIEW -- UNCERTIFIED TERRAIN` in the HUD. Verified by capture.
The sidebar (below) is still to do.

### The problem

`load_compiled_world` requires **seven artifacts** and runs full worldspec
validation across them. That validation is what caught D32. A raw Gaea
heightfield satisfies none of it.

### The rejected shortcut

Have the importer synthesise a passing `zone_spec.json` and `render_plan.json`
so the existing path "just works". **Do not do this.** A Gaea heightfield is not
a certified world, and faking certification to get a preview puts a lie into the
one mechanism that has been reliably catching our mistakes — D32 was found
*because* the viewer refused a world whose digests disagreed.

The refusal is structural and worth knowing: the viewer requires
`terrain_manifest.json` to carry a `zone_spec_sha256` matching the ZoneSpec it
just validated. A preview manifest has no such field and **cannot acquire one
honestly**, so a preview cannot impersonate a compiled world even by accident.
There is a test pinning that.

### The design

A second, honest load path. `CompiledWorld` has 21 fields and the terrain shader
wants splatmap, wetland mask, four material textures and a rock normal; a preview
has none of these, so it is a separate material path, not a branch.

- `terrain_manifest.json` carries `"preview": true` — already emitted by
  `import_heightfield.py`.
- Preview worlds: build the terrain mesh, plain `StandardMaterial`, empty
  placements and corridors, **skip `validate_value`**, skip the six other
  artifacts.
- The HUD must say **preview** in words. A preview must never be mistakable for
  a certified world in a screenshot, because screenshots become evidence.

**As built.** `WorldMode` is decided once at argument-parse time by reading
`preview` from the manifest; `LoadedWorld` is an enum over `CompiledWorld` and a
separate `PreviewWorld`, so no certified consumer grows a "but maybe not" case.
Three things were not in the estimate and are worth knowing:

- **`ViewerConfig::from_args` rejected previews before loading began.** It
  required the batch to sit inside a `concept_batches` directory whose parent
  holds `project.godot`. A Gaea import lands wherever the importer was pointed,
  so previews skip that check and use the batch as their asset root.
- **`source_files` hard-required all seven artifacts**, and `source_signature`
  errors on any it cannot stat — which would have made every preview permanently
  unloadable through the reload path even after the loader worked. It now returns
  two files for a preview.
- **The preview flag is re-checked inside the loader**, not only at detection. A
  manifest can be rewritten in between, and the certified world must never reach
  the path that skips validation. A manifest claiming *both* `preview` and
  `zone_spec_sha256` is refused outright rather than resolved by guessing.

Twelve Rust tests (20/20 green in the viewer crate) cover one property above all: a preview cannot render as
certified, and a certified world cannot render as a preview. That is the only
part of preview mode that has to be trustworthy — the viewer is the instrument
every visual measurement flows through, and it is already flagged as thinly
tested (D5) and unstable in framing (D6).

**The acceptance gates need no new guard.** `capture_bevy.py` errors without
`zone_spec.json` and `bevy_visual_acceptance.py` takes one as an argument, so
both already refuse previews structurally rather than by convention.

### Measuring previews — the contradiction the audit found

The original spec said visual acceptance gates do **not** run on previews, and
then made its step 3 (now §8) "build 3–4 graphs and fly them, decide whether Gaea's output
is good enough". Those cannot both hold: step 3 is a *judgement about previews*
and it was left with no instrument except taste, which is precisely what lost the
last two attempts.

Both halves are right about different things, so split them:

- **Visual acceptance gates stay off previews.** They measure compiled worlds —
  foliage fractions, road fractions, framing — and several of them are
  frame-relative anyway. Pointing them at raw terrain produces numbers that mean
  nothing, dressed as certification.
- **A preview metric set is defined and explicitly cannot certify.** Built:
  `pipeline/preview_metrics.py`. It measures only terrain-intrinsic properties,
  which need no plan, no placements and no ZoneSpec: relief, slope distribution,
  sink density, **radial monotonicity** (§5.5) and **symmetry residual** (§5.1).
  It writes `preview_metrics.json` under schema
  `codeweald.preview-metrics/v1` — deliberately *not*
  `codeweald.terrain-artifacts/v1` — carrying `"certifies": false`, so no
  downstream tool can mistake one for the other. Tests pin that separation.

That gives step 3 an instrument without giving a preview a certificate.

### The sidebar

**BUILT 2026-08-04.** `Tab` opens the picker, `Up`/`Down` select, `Enter` loads,
`Escape` closes. Roots come from `--worlds-root <dir>` (repeatable), defaulting
to the batch's own parent so the common case needs no argument.

- Discovery is by manifest presence — a directory holding
  `terrain/terrain_manifest.json` is a world — so Gaea imports and compiled
  batches are found by one rule and a new kind of batch needs no picker code.
- Grouped **Certified worlds** then **Heightfield previews**, newest first
  within each group. Certified sorts first deliberately: a preview must never be
  silently the default selection.
- Selecting one rewrites `ViewerConfig` and forces a reload, reusing
  `monitor_compiled_world` rather than adding a second way to bring a world up.

Three things worth knowing, none of them in the original sketch:

- **Keyboard, not mouse.** The free camera grabs the cursor, so a click-targeted
  list would fight the navigation the picker exists to serve.
- **Scanning is one level deep**, and that is a performance decision rather than
  a taste one: batches are direct children of their root, and the workspace is a
  spinning NTFS mount where an unbounded walk stalls the frame that triggers it
  (D30).
- **Switching batches recomputes the renderer root**, via the same helper
  `from_args` uses. Preview and certified worlds resolve asset paths against
  different roots, and getting it wrong renders an untextured world rather than
  failing loudly. A certified batch found outside a renderer tree is refused
  with a message instead of being loaded into a broken state.

Auto-detection is by manifest presence, so Gaea imports and compiled batches are
found by the same rule with no special cases.

---

## 5. The compiler side

**Design agreed, none of it built.**

### 5.1 Symmetry — the load-bearing idea

**Make the heightfield 180° rotationally symmetric.** A double-ended 3-lane MOBA
is inherently a rotational layout, so natural corridors found between the two
keeps are **balanced by construction**. Beauty and fairness fall out of one
operation instead of being traded against each other.

`rot180` is an exact involution on the grid at **both** parities (verified), so
resolution parity imposes no constraint here; 1025 is chosen for the stride
(§3.1), not for symmetry.

**Do it before erosion, not after.** The original spec offered two options and
preferred "enforce on import" for the sake of the gate. The audit reverses that
preference on physical grounds: blending a heightfield with its rotated copy
*after* erosion averages two independent drainage networks. Rivers meet their own
mirror image head-on along the symmetry locus, producing convergent valleys and
closed sinks that no erosion process would ever produce. It buys exact symmetry
by spending the one thing Gaea was brought in to supply.

Compositing the *macro-shape* with its rotated copy and then eroding the
symmetric field keeps the drainage physical. Erosion of a symmetric input stays
symmetric to the extent the solver is rotation-equivariant.

**The preference for (2) was really a preference for measurement, and that is
separable.** WGE should *measure* residual asymmetry whether or not it imposes
it. So:

1. Symmetrise the macro-shape **in the graph**, before erosion.
2. **Measure** residual asymmetry on import — `max |h - rot180(h)| / relief` and
   its p99 — and gate on it.
3. If the residual exceeds tolerance (Gaea's erosion solvers plausibly carry
   scan-order anisotropy — **untested, and the first thing step 3 should
   check**), apply a final light symmetrising blend as a *correction* to a field
   that is already nearly symmetric, which does far less damage than
   manufacturing symmetry from scratch.

**A 180°-symmetric field has zero gradient at the map centre** — necessarily,
since `h(c+d) = h(c-d)` makes the gradient antisymmetric about `c` and therefore
zero there. Verified numerically. Every such map has a critical point dead centre:
a flat spot, peak or saddle, permanently, in every seed. **Design with it rather
than against it** — it is exactly where a central objective belongs — but do not
be surprised by it and do not let a flatness gate flag it as a defect.

**Sun direction is not rotationally symmetric.** A map balanced by construction
in *geometry* is still asymmetric in *lighting*: one team's approach is lit, the
other's is in shadow. Rotational symmetry buys traversal fairness, not visual or
readability fairness. Either accept it, or use a near-overhead key light for
competitive profiles. Flagging it so it is a decision rather than a surprise.

**Symmetry is a MOBA profile rule, not a global assumption.** An RPG zone must be
able to be asymmetric.

### 5.2 Natural lanes

Routing is a **negotiation**, not an imposition:

1. Least-cost path between the two keeps, cost from slope and traversability, so
   corridors follow valleys and passes.
2. Check against the profile: lane count, minimum width, length parity, keep
   distance.
3. Where a lane fails, **edit locally** — widen this pass, lower that saddle —
   within the budget of §5.6.

**Never a global ramp.** That is precisely what produced the slabs. Local edits
to named features are auditable and bounded; a field applied everywhere is not.

**Three lanes are not three runs of one search.** A least-cost search between a
fixed pair of endpoints returns the same corridor every time; running it three
times yields three copies of one lane. Diversity has to be an explicit
constraint — penalise cells already claimed by a routed lane, or require
vertex-disjointness with a minimum separation, then re-run. Without it the
router silently produces a one-lane map that satisfies "three paths found".

Under 180° symmetry the three lanes are not interchangeable: **mid maps to
itself, and top and bottom map to each other.** So mid must be self-symmetric —
a stricter condition than the other two — and top/bottom are mirror images
rather than copies. Route mid first, on the symmetry-invariant subgraph.

### 5.3 Border mountains

Author the Gaea graph as a **basin macro-shape** so edges are high naturally.
Then `boundary_plan` does what it already does — flood-fill from the keeps with
the declared agent, report the spans that leak — and WGE patches only those
spans. **Verification rather than generation**, which is the opposite of the
rampart.

**This is the highest-risk idea in the document, and §5.5 exists because of it.**

### 5.4 Mountain types and per-genre profiles

`--vars <json>` and `-v name=value` drive a hand-authored graph, with `--seed`
pinning stochastic nodes. Alps / Highlands / Andes become variable sets or
sibling graphs.

**Profiles are the real payoff.** A MOBA profile (double-ended, 3 lanes, strict
symmetry) and an RPG-zone profile (asymmetric, no lane parity) are different
constraint sets over one importer and one compiler. This is what stops WGE being
a one-map tool.

A profile should be a **declared artifact**, not a code path: lane count,
symmetry requirement and tolerance, minimum corridor width, length-parity
tolerance, edit budget (§5.6), and the enclosure rule. If adding a genre means
editing the compiler, the flexibility is notional.

### 5.5 The basin is the old defect in polar form — measure it

A basin is relief rising with distance from the map centre. The failed massif
carve was relief rising with distance from the lanes. **These are the same
construction in different coordinates**, and the basin dodges only the letter of
§1's rule.

Along any radial from the centre, a monotonic basin means a nearer summit can
never be taller than a farther one — foothills in front of peaks are forbidden,
which is *verbatim* the massif defect. A bowl rim rendered from inside is a wall
that happens to be curved, and it will read as "the same shape extending way far
back" for the same reason it did last time.

**It is still the right idea, because of where it sits in the pipeline.** The
massif carve was applied *after* generation as the final word on amplitude. The
basin is a low-frequency bias applied *before* erosion, and erosion is not a
function of the bias — it carves valleys through the rim, leaves outliers and
breaks monotonicity on its own. The difference between the two is entirely
whether erosion is strong enough to destroy the monotonicity. **That is an
empirical question, so measure it rather than assuming it.**

**Radial monotonicity.** Sample rays from the centre; for each, the fraction of
its length over which height is non-decreasing. Report the mean and the fraction
of rays that are fully monotonic.

- A pure cone scores 1.0. The massif carve scored near 1.0 by construction.
- Real terrain scores well below it.

**Gate on the mean, not on the fully-monotonic fraction.** Measured on two
synthetic fields the night this was built:

| field | mean non-descending | fully-monotonic rays |
|---|---|---|
| no radial bias | **0.42** | 0.0% |
| 28% radial basin bias | **0.69** | 0.0% |
| pure cone | ~1.00 | ~100% |

The fully-monotonic fraction **saturates at zero** for anything carrying real
detail — it separates a cone from terrain, and nothing else. The mean is the
statistic that actually tracks how strong the basin bias is, and it moved 0.42 →
0.69 for a bias that is invisible in the other column. A gate written on the
fully-monotonic fraction would have passed a substantially bowl-shaped world.

Calibrate the threshold against real Gaea output in §8 step 3 rather than
picking a number from synthetic noise — erosion is not fractal noise, and the
whole question is whether *erosion* breaks the monotonicity. If a candidate
basin scores like a cone, the bias is too strong, and the fix is to weaken it
and let enclosure be patched by §5.3 instead of generated.

This is the metric that would have caught both previous failures on the night
they were built, and it costs about thirty lines.

### 5.5a TESTED 2026-08-04 — an imposed radial basin makes a crater, not a valley

§5.5 asked an empirical question: is erosion strong enough to break the
monotonicity a basin bias imposes? **Tested, and the answer is no.**

`gaea_terrain.py --insert-basin` builds `RadialGradient → Combine(Subtract)`
upstream of `Erosion2` — the basin applied before the solver, which is the most
favourable placement §5.5 allows. Three strengths were built and measured:

| field | rim strict-rise |
|---|---|
| eroded, no basin | 0.55 |
| basin ratio 0.10 | 0.75 |
| basin ratio 0.55 | 0.81 |
| perfect cone (reference) | 0.96 |

The basin moves the rim roughly two thirds of the way from natural terrain
toward a cone, and **rendering confirms what the number implies**: the result is
a caldera. Every gully points radially at the centre, because a radial function
imposes radial drainage. It is the massif defect in polar coordinates, arrived
at from the other direction.

**So do not impose a basin.** The failure is not the strength setting — it is
that any radial function produces radial drainage, and drainage is what makes
terrain read as landform.

**What to do instead.** Take the natural valley, not a manufactured one. A real
eroded valley has an outlet and asymmetric drainage, which is exactly what the
radial basin cannot have. The measured obstacle there was that a found valley
has no flat floor (largest flat square: 41 m at 18% grade, against an arena
needing 256 m) and weak enclosure. That is a **local** problem, and §5.6 already
governs local edits: flatten a slab into the chosen valley within a declared
budget, rather than reshaping the whole field to produce a floor.

Two measurement lessons, both found by using the metric rather than reasoning
about it:

- **Non-descending is the wrong test.** Flat ground counts as monotonic, so a
  broad flat valley floor — the thing we want — read as cone-like. A strong
  basin scored 0.90 non-descending while scoring 0.55 on strict ascent, the same
  as no basin at all. `mean_strictly_rising_rim` is the number to gate on.
- **Sample at one step per cell.** Sampling a ray finer than the grid makes
  consecutive samples land on the same cell, and a zero delta fails a strict
  test — an oversampled *perfect cone* scored 0.56 instead of 0.96, so the metric
  was reading its own step size. Both bugs were in the first version of this
  metric, and both would have mis-gated the design.

### 5.6 The edit budget — bounding the negotiation

§1 says terrain is generated without reference to the play space. §5.2 then edits
the terrain because a lane failed. **Those edits are, by definition, reference to
the play space** — the spec permits exactly the class of operation it forbids,
distinguished only by being "local", which was never defined.

Undefined, "local" drifts. Widening one pass is local. Widening every pass on
three lanes is a corridor network. Widening it a little more each time a gate
fails is the massif carve arrived at incrementally.

So bound it, declare it in the profile, and emit it as an artifact:

- **Extent** — fraction of cells whose height changed at all (target: low single
  digits).
- **Amplitude** — max and p99 `|Δh|` as a fraction of relief.
- **Contiguity** — size of the largest connected edited region. A budget spent as
  one big region is a ramp; spent as many small ones it is genuinely local.
- **Locality** — every edited region must be attributable to a *named* feature
  ("saddle at x,y lowered 3.1 m for lane 2"). An edit no report can name is a
  field, and fields are what this programme exists to stop.

`terrain_edit_plan.json`, gated. When a profile cannot be satisfied inside its
budget, **the correct outcome is rejecting the heightfield and building another**,
not spending more budget. Editing until it fits is how the vertical slabs
happened.

That only works if rerolling is genuinely cheap, and **the 29 s figure does not
generalise**: the audit built `Canyon River with Sea` at 512 and it ran
single-threaded for over ten minutes under Proton. Budget graph authoring
accordingly — keep the WGE graph lean, measure its build time before relying on
reroll-driven workflows, and if a reroll costs ten minutes then the profile
constraints have to be loose enough to usually pass on the first build.

---

### 5.7a DECIDED 2026-08-04 — the backdrop becomes Gaea crops

**The border stops being generated and becomes authored assets.** Flat skirt,
mountains cropped from Gaea heightfields and placed as meshes, invisible walls at
their bases doing enclosure.

The argument that settled it: a good mountain is one where ridgelines **converge**
and the peak has a back. `_border_rampart` is a swept profile — every
cross-section identical by construction — so it **cannot produce convergence at
any tuning**. That is topology, not parameters. Three failed attempts (rampart,
massif carve, D26) and a purpose-built detector that never fired (audit R1) are
consistent with a structural ceiling rather than bad luck.

**Split by role, not by style.** ~721 lines retire (`_surrounding_massif`,
`_border_rampart`, `_scattered_summits`, the corrugation metric,
`massif_character.py`). ~2,107 stay: interior relief, hydrology, corridors,
village pads, materials, semantic regions. The interior — the part that works —
is untouched.

**Style range comes from varying one system's inputs**, not from two engines.
Alps / Highlands / Andes / soft old-school-MMO are Gaea graphs and crop
libraries. A swept profile's entire reachable space is "swept profile"; no amount
of work widens it.

**`_border_rampart` is kept behind a profile flag as a containment fallback, not
a style option, and is left unfixed.** It is adequate at enclosure and bad at
beauty; labelling it honestly costs nothing and gives a floor if Gaea goes away.

**Consequences.** §5.8 and audit R1 are **not to be implemented** — they are
careful repairs to retiring code. Their value was diagnostic: they are how we
learned the wall cannot get there. Crops are also the *lowest*-dependency use of
Gaea, because they are committed assets: once a library exists, Gaea is needed to
author new styles, never to build a map.

### 5.7 The skirt is the same defect, and the programme deletes it

Diagnosed 2026-08-04 from the arena as it currently renders. **The apron — the
"skirt" — is the abandoned defect a fourth time**, and unlike the other three it
was never reverted.

`build_apron_mesh` computes each vertex beyond the world bounds as:

```
height = sample(position clamped to the boundary) - depth * (distance beyond / extent)
```

That is the edge profile **extruded outward**, minus a term that rises with
distance from the play space. Structurally identical to the massif carve: relief
outside the play space is a function of the play space and of distance from it.
It is *verbatim* "a silhouette that's just the same shape extending way far
back", which is why the skirt reads as folded cardboard with long straight
creases running perpendicular to each map edge. The creases are the edge's own
height profile, swept.

The distance is **Chebyshev** (`max` of the two axis excesses), so the rings are
square rather than radial, and the corners extrude a single corner sample
diagonally — those are the big flat triangular wedges.

**Measured on `codeweald_alpine_arena_v1`:**

| | |
|---|---|
| world | 256 m (65,536 m²) |
| apron extent | 181 m per side |
| total footprint | 618 m (381,971 m²) |
| **apron share of visible surface** | **82.8%** |
| apron falloff | 10.9 m over 181 m — a **6% grade** |

**More than four fifths of every screenshot is apron**, and it is essentially
flat. That also explains why it reads *lighter* than the terrain despite being
deliberately tinted darker: a near-flat up-facing surface takes the full
directional light plus ambient 240, while the terrain is steep and
self-shadowing. The tint is working; the flatness is overwhelming it.

**Do not polish it.** Any apron built from the edge profile extrudes, and adding
noise to a distance function is precisely the rampart. There is no version of
this that is not the defect.

**The programme deletes it structurally.** The apron exists only because WGE's
terrain stops dead at the data edge. If the heightfield is authored as a basin
macro-shape over a larger world (§9), the land beyond the play space is *real
eroded terrain from Gaea* and there is nothing to extrude — `boundary_plan`
returns to what §5.3 wants it for, verifying enclosure rather than manufacturing
a horizon.

The arithmetic also puts a number on §9's world-size question: at 800 m, the same
181 m apron falls from **82.8% to 52.6%** of the visible surface before any other
change. World size is not only art direction — it is most of why the skirt
dominates the frame.

**Until then, judge the arena by the interior** and treat the skirt as a known
placeholder. It is the single biggest reason the current world photographs badly,
and none of that is a statement about the terrain inside the bounds.

### 5.8 The "giant's tilled field" — a height field used as a displacement field

Diagnosed 2026-08-04 from the arena interior. The row of rounded barrels along
the border — reviewed as *"a tiny section of a giant's tilled field"* — is a
distribution error, not a noise-tuning problem.

`zone_rasterizer._boundary_rampart` displaces the wall toe with:

```python
wandering = play_distance + spur * (massif - 0.5) * 2.0
```

The intent (stated in the comment above it) is right: *"pushing the distance
field in and out makes the toe of the wall advance and retreat. That is what a
spur and a re-entrant are."* The implementation centres on **0.5**, which
assumes `massif` is symmetric about its midpoint. It is not, and it is not
supposed to be.

`shape_relief` is a **height** field. `valley_flatten` raises it to a power of
about 3.4 precisely so it reads as mostly-valley-floor with isolated summits —
that is what makes it good mountains. Measured for `alps` at 512²:

| | |
|---|---|
| mean | **0.105** |
| median | **0.027** |
| cells below 0.5 | **95.7%** |

So `(massif - 0.5) * 2` has mean **−0.79**, and at the default `spur = 24 m`:

| displacement | mean | pulled one way | p99 the other |
|---|---|---|---|
| current | **−19.0 m** | **95.7%** | only **+8.1 m** |

**The toe does not wander. It is uniformly offset by ~19 m over 96% of the
perimeter, with isolated spikes over the remaining 4%.** Those spikes are the
barrels. Because `shape_relief` is exponentiated they have steep flanks, and
where two nearly meet the horizontal run collapses — that is the narrow vertical
slot cutting through the ridge in the same screenshots, D26's slab mechanism in
miniature.

There is a second defect stacked on it. The same `massif` field also drives
`modulation`, which sets crest height, so **displacement and height are rank-1
correlated (ρ = 1.000)**: every lobe is simultaneously the furthest forward *and*
the tallest, every notch the furthest back *and* the lowest. Real spurs and
re-entrants are only loosely related that way, and the perfect correlation is
what makes the row read as manufactured.

**The formula.** Drive displacement from an independent, zero-mean field and
leave `massif` driving height:

```python
drift = ridged_multifractal(shape, rng_displacement, character)
drift = (drift - drift.mean()) / drift.std() * 0.5
wandering = play_distance + spur * np.clip(drift, -1.0, 1.0)
```

Measured against the current version:

| | mean | inward | p1 / p99 | ρ with crest height |
|---|---|---|---|---|
| current | −19.0 m | 95.7% | −24.0 / +8.1 m | **1.000** |
| independent field | **−0.4 m** | 64.3% | −16.1 / +24.0 m | **0.121** |

A third change is needed for the slot: **slope-limit `wandering`** so the toe's
horizontal run cannot collapse faster than the face can absorb, which is the
same bound §5.6 puts on edits.

**Caveat before touching it.** Enclosure, navmesh and the visual gates all ride
on this function, and a symmetric toe consumes wilderness the current inward
bias gives back. This is a real fix rather than a polish — but it is a fix to
machinery the programme intends to replace, so do it only if the arena has to
look right *before* Gaea lands, and re-run the gates when you do.

### 5.9 Seating a world in landform, and what contains the player

**Built 2026-08-05.** A compiled world can declare landform it sits inside:

```json
{"landform": "../../landforms/<name>",
 "offset_m": {"x": -28.0, "y": -97.6, "z": -294.0},
 "collides": false}
```

The viewer reads `landform_surround.json`, renders the landform as a second
mesh, and **does not draw the apron** — the landform is the horizon, so a flat
extruded skirt beside it is the §5.7 defect drawn over real eroded ground.

**Containment does not come from the landform, and never came from the border.**
Verified by deleting the border entirely (`border_policy.enabled: false`) and
re-measuring:

| | playable | playable cells on the map edge |
|---|---|---|
| with rampart + massif | 52.1% | **0 / 1028** |
| border deleted | 52.5% | **0 / 1028** |

Read directly from `terrain/playable_mask.bin`, not from the gate's own verdict.
The playable area barely moved — 34,415 → 34,695 m². **The rampart was never
containing anything**; interior relief and the ~18 m median edge terrain were,
and both survive its removal. Four attempts at a border traded silhouette for an
enclosure it was not providing.

That result is trustworthy because the containment gate has a negative control:
`test_an_open_plain_is_not_enclosed` proves it *can* fail. Compare audit R1,
where a gate had never fired on the defect it was built for.

**The landform is scenery and must stay scenery.** It carries no collision —
`collision_plan` emits the certified heightfield and instance colliders, and the
landform appears in neither. `"collides": false` says so in the artifact rather
than leaving it to whoever writes the importer, because a backend that gave the
landform a collider would change containment **silently**, and boundary_plan
would go on reporting a world it no longer describes.

If landform ever needs to be walkable, it stops being a surround and becomes
terrain: it has to enter the certified heightfield and be re-gated. There is no
middle state where it is half-real.

### 5.10 Real-world elevation — `fetch_dem.py` and `import_dem.py`

**Built 2026-08-05.** The reference photographs for this project are of places
that exist and have been surveyed. Synthesising them is strictly worse than
downloading them: a generator approximates the character of a U-valley, a DEM
*is* the valley.

So §5.7a's split extends one step. **Gaea is for terrain that must be fictional
or must satisfy gameplay constraints. Real references arrive as real data.**
Everything downstream already consumes a heightfield and does not care which
produced it.

```bash
# Public AWS Terrain Tiles -- no API key, global coverage.
python3 pipeline/fetch_dem.py --lat 49.05 --lon -113.90     --zoom 13 --tiles 3 --output waterton.tif

python3 pipeline/import_dem.py waterton.tif --output <batch> [--side-m 2600]
python3 pipeline/preview_metrics.py <batch>
```

Fetch and ingest are separate so the ingest path stays testable offline and the
network lives in exactly one module. `import_dem.py` reads any GeoTIFF —
point it at USGS 3DEP lidar and nothing else changes.

**No GDAL.** A GeoTIFF is a TIFF with georeferencing tags and PIL reads TIFF via
libtiff, so tag parsing plus a pixel read needs no heavy dependency on a machine
nobody has provisioned.

**The projection trap is the reason this has tests.** A DEM in a projected CRS
has pixel scale in metres. One in a geographic CRS has it in *degrees*, and a
degree of longitude shrinks with latitude. Reading degrees as metres makes
terrain 100,000× too small, which is obvious. Applying the latitude correction to
one axis, or neither, stretches terrain by `1/cos(latitude)` — **52% at
Waterton's 49°** — and produces a perfectly plausible wrong valley. A DEM that
does not declare its units is **refused**, not guessed at, because a wrong guess
in that direction is invisible.

Voids are filled to the local minimum: a `-32768` sentinel left alone becomes a
hole kilometres deep that dominates the vertical range.

**Verified end to end 2026-08-05** on Waterton Lakes at 49.05N, -113.90W:
9.6 km at 12.52 m/px, **1132 m of real relief**, rendered in the viewer.

**Know what the data can carry.** Terrarium at zoom 13 is ~12.5 m/px, but the
Canadian source beneath it is 20-30 m, so cliff bands and scree aprons are simply
not present — the result is markedly smoother than the photograph. Zooming
further interpolates rather than revealing. Where that detail matters, use 3DEP
lidar; the ingest is unchanged.

## 6. Determinism

WGE emits or drives the graph, builds it headlessly, and **digest-pins the built
heightfield as a declared input**. §17 of the language spec grades determinism
over *equal declared inputs*, so a pinned import is Grade A. The world stays both
regenerable and byte-exact.

**Declared inputs are only as good as the declaration.** The audit found the
importer pinning the source digest while leaving the pixels-to-metres rule
implicit, so two different worlds shared one digest. Now recorded:
`vertical_mapping`, `relief_m`, `world_m`, `source_resolution`, `resampled`, and
`importer_version`.

Still to pin once the driver exists — **none of these are in place yet**:

- the `.terrain` graph file digest;
- the variable set (`--vars` / `-v`) and the `--seed`;
- the build resolution;
- **the Gaea version**. `gaea_terrain.py` already documents that `$type` strings
  carry assembly-qualified names differing between Gaea builds. A tool whose
  serialisation format moves between versions cannot be treated as a fixed
  function of its inputs, and an unpinned version is an undeclared input.

**Gaea is not reproducible until proven reproducible.** Nobody has yet built one
graph twice and compared digests. That is a five-minute experiment (`--ignorecache`,
fixed `--seed`, diff the outputs) and it should be **step 3a**, because if Gaea's
erosion is not bit-stable across runs then the heightfield is a *source artifact*
that must be committed, not a *derived artifact* that can be rebuilt — and that
changes §7 and the repository policy below, not just a footnote.

---

## 7. Dependency risk, and what happens when Gaea breaks

Gaea is proprietary, Windows-only, and reached through Proton, `dotnet48`,
`dotnetdesktop8` and a pty shim. **Four blockers were needed to get one build to
run.** A Proton update, a Gaea update or a licence-tier change can break all of
it, and that is not a hypothetical for a stack this deep.

The mitigation already exists implicitly and should be policy explicitly:

> **A built heightfield is a committed source artifact. Gaea is a dev-time
> dependency, not a build-time one.**

So a Gaea outage stalls *authoring new landform* and never blocks rebuilding,
re-certifying or shipping an existing world. Every downstream stage consumes the
committed heightfield, not Gaea.

This collides with the workspace rule against committing large binaries, so
decide it rather than discovering it: a 1025² float32 heightfield is **4.2 MB**
and its PNG16 source ~2 MB. A handful of worlds is tens of megabytes — worth it,
given the alternative is a world that cannot be rebuilt without a working Proton
prefix. **Commit the PNG16 source** (smaller, and the thing Gaea actually
produced) and treat the `.bin` as derived, since the importer is deterministic
and tested. Do not commit intermediate build-path spam.

Reserve of last resort remains Houdini (§2), and the seam makes it cheap: every
stage after `import_heightfield.py` consumes a heightfield and a manifest, not
anything Gaea-shaped.

---

## 8. Order of work

1. ~~**Preview mode in the viewer** (§4)~~ — **DONE 2026-08-04.**
2. ~~**The sidebar** (§4)~~ — **DONE 2026-08-04.**
3. **Re-establish a reproducible headless build first** (§3.3). It did not
   reproduce during the audit, and every step below assumes it works. Start from
   a graph known to be cheap, confirm files actually appear, and time it. Nothing
   else in this list is worth doing until a build runs twice in a row.
   Then **build 3–4 example graphs** at 512–1024 and fly them, which answers "is
   Gaea's output actually good enough" against our own renderer rather than
   against Gaea's.
   - **3a. Build one graph twice and diff the digests** (§6). Decides whether
     heightfields are derived or source artifacts.
   - **3b. Measure radial monotonicity and symmetry residual** on those examples
     (§5.5, §5.1) to calibrate the gates against real terrain rather than
     guesses.
4. **Author a WGE graph** in the Gaea GUI: basin macro-shape, symmetric
   composite before erosion, exposed variables, marked height export.
   Interactive; Matt's job.
5. **The driver** — `--vars` / `--seed` / `--resolution`, digest pinning of
   graph, vars, seed and Gaea version.
6. **Symmetry measurement, then enforcement if needed** (§5.1).
7. **Lane routing against real terrain** (§5.2), with the edit budget (§5.6) in
   place *before* the first edit, not after the first failure.

Steps 1–3 are worth doing before 4, because they are what tells us whether the
whole approach is worth the investment. Steps 3a and 3b cost under an hour
between them and each can invalidate a later step, so they are cheap insurance.

---

## 9. Open questions

- **Vertical scale.** `--relief-m` is an assertion because a 16-bit image carries
  no units. What relief does a 3-lane MOBA actually want, and over what world
  size? The arena's 256 m is almost certainly too small — at `agent_height_m:
  8.0` it is 32 body-lengths across. **Matt's call**; it is gameplay and art
  direction, not engineering.
- **World size, and its cost.** For mountains to read as landscape the wilderness
  has to dominate: 600–800 m with a ~256 m playable rhomboid puts wilderness at
  ~85% against today's 41%. But this is **not** a free art-direction knob. At
  1025² a 256 m world has 0.25 m cells and an 800 m world has 0.78 m — a 3×
  coarsening that lands on navmesh cell size, collision resolution, ecology
  density, asset scale and the viewer's single-mesh terrain. Matt owns the
  aesthetic call; the migration cost is engineering's and should be estimated
  before, not after.
- **Erosion at final resolution.** Iterate at 512–1024, bake at 2K–4K. Note the
  viewer meshes one decimated grid with no LOD: a 4097² bake decimates to 1025²
  ≈ 1.05M vertices in a single mesh. That is the practical ceiling until terrain
  LOD exists, and 8K is well past it for a MOBA map.
- **Symmetry tolerance.** What residual is acceptable before the corrective blend
  in §5.1 step 3 fires? Calibrate in step 3b.

---

## 10. What the audit changed

Red-teamed 2026-08-04, against the code rather than the prose. Four defects were
verified in the codebase and fixed; six were design gaps and are now written into
the sections above.

**Fixed in code** (`pipeline/import_heightfield.py`, `tests/test_gaea_import.py`):

1. **Every Gaea-native resolution was unmeshable.** `(resolution - 1) % 4 != 0`
   for all powers of two, so the viewer would have rejected every preview the
   importer had ever written. §4 was blocked before it began and nothing said so.
   Importer now resamples; §3.1.
2. **Min/max normalisation silently rescaled every world.** Builds occupying 30%
   and 90% of the range imported identically. Now full-scale mapping, with
   `--normalize` opt-in and the choice recorded; §3.2.
3. **Non-square input was accepted**, writing a manifest resolution that
   disagreed with the buffer length. Now refused at read time.
4. **Neither Gaea module had a single test**, in a repo where all 32 other
   modules do. 14 cases added, green.

**Design gaps now addressed in the spec:**

5. §5.5 — the basin macro-shape is the massif defect in polar coordinates, and
   needed a metric rather than an assurance.
6. §5.6 — "local edits" were unbounded, permitting the forbidden operation by
   increments.
7. §5.1 — post-erosion symmetry blending destroys drainage; the preference for
   import-side enforcement was backwards, and measurement is separable from
   imposition. Plus the forced centre critical point and the lighting asymmetry.
8. §5.2 — three least-cost runs between one pair of endpoints return one lane.
9. §4 — previews could not be measured, yet step 3 required judging them.
10. §6/§7 — Gaea's reproducibility was assumed rather than tested, its version
    was an undeclared input, and there was no stated behaviour for the day the
    Proton stack breaks.

**And one retraction** (§3.3). The audit first recorded "the headless build does
not reproduce". That was wrong. Gaea runs fine under Proton (v2.3.0.1 confirmed);
what is broken is *observability* — console output cannot reach a non-interactive
session, and redirecting it to see the error triggers blocker 4 and crashes the
build. Two real invocation traps did fall out of chasing it: **filenames with
spaces break argument parsing** (all three exportable examples have them), and
**a genuine build is slow enough to look exactly like a hang**. Confirming the
build is a two-minute job at a real terminal and effectively impossible from an
agent session.

The pattern across all eleven is one thing: **this seam fails silently.** A
preview no consumer can open, a rescale no digest reveals, a manifest that
disagrees with its own buffer, and a build tool that reports success having done
nothing. Every gate added above exists to convert a silent failure into a loud
one, because the two previous terrain attempts were lost precisely to metrics
that improved while the world got worse.
