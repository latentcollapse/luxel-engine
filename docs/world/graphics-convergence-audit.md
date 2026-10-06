# Luxel Graphics Convergence Audit

Status: **independent review, no implementation**, 2026-10-03
Reviewer role: graphics / art-pipeline (external to the parity sprint)
Question answered: *what must change before a human looks at a Luxel frame and says "okay, this is actually beautiful"?*

Evidence tags used throughout:

- **[V]**: seen in the review images (`artifacts/parity/review-{full,hero,terrain}/`)
- **[M]**: measured by this audit (numbers below are reproducible; method given inline)
- **[S]**: verified in source at the cited `file:line`

Nothing here is inferred from test status. The hero arm passed its comparator gate (`artifacts/parity/ab-hero3/compare.json`: `"result": "PASS"`, every view `gate_pass: true`) while deleting the copper from the landmark.

---

## 0. Verdict in five lines

1. **Reject `ab-hero3`.** Keep the per-family plumbing, but throw away these maps. The arm makes the frame worse on the one axis it exists to move, and it was rendered on the wrong baseline.
2. **The primitive look is mostly not a material problem.** In the retained `full` arm, flat sky plus void covers **29% / 42% / 62%** of the close / medium / wide frames, while authored objects cover **23.5% / 11% / 6.5%** [M]. The material sprint was tuning the minority of the pixels.
3. **The worst single defect is that the world ends inside the frame.** In medium and wide the island is a square slab floating in a blue-grey void [V]. No material, shadow or post work survives that.
4. **"The renderer is no longer the bottleneck" is half wrong.** It is true for diffuse surfaces. It is false for metal and wet: with no environment reflection, a metal cannot read as metal and wet cannot read as wet, whatever the maps say [S]. The material work was sequenced ahead of the capability it needs.
5. **The shortest path to a beautiful frame** is: world extent and horizon → sky and sun coherence → aerial perspective → environment reflection → three or four real imported assets with real textures → authored composition. Procedural material synthesis is not on that path.

---

## 0.1 Errata from the CONVERGE-0 run (2026-10-03)

Three claims in this audit were tested by implementing them, and failed.
Evidence is in `artifacts/parity/CONVERGE0_SPIRAL_LEDGER.md`.

1. **I-7 / §C / §G — the landmark-separation metric does not catch the hero
   regression.** Two formulations, built and measured, both rank `ab-hero3` as
   *better* separated than `full`: magnitude |ΔL|+|ΔS| 0.25 against 0.13, and
   mean-Lab ΔE 25.9 against 17.1. The grey landmark on saturated grass IS
   separated. The real defect was content (copper albedo swapped for grey), and
   a content-level hue-preservation test now catches it. The image-level gate
   is rejected; focal hierarchy stays a human call.
2. **H.7 — "sky and sun values only, no shader work" was impossible.** The
   legacy sky draw samples the environment only at NDC y = −1 and y = 3, so it
   never uses the horizon colour, and with the adapter's Y flip it puts the
   ground colour at the TOP of the frame. CONVERGE-0 added a typed
   view-direction sky instead.
3. **H.9 — the calibration scene is not a one-step fixture.** It needs the C2
   bound-scene route, an authorized light-rig seam, and imported-texture channel
   semantics. Research found a real defect on that route (glTF roughness read
   from R instead of G; fixed, byte-identical for all existing maps). The scene
   is parked as a closed recipe: `docs/world/converge/converge1-contracts.md` §3.

Also new since this audit: the Julia policy parser did not reject unknown
policy KEYS (fixed), and extending the world requires a view-relative shadow fit
(added), otherwise one 512² map over 480 m drops to ≈0.7 texels/m.

---

## 1. The human review questions

### Q-B1: Do the materials now read as genuinely different substances?

**No.** In the hero arm, everything that isn't grass or leaves reads as the same grey speckled surface: the bell, the halo arch, the fins and the pedestal [V]. At 3× crop the bell looks like grey burlap or granite carpet, the fins like faded denim, and the pedestal like dirty concrete with horizontal smear rings. **The surface meant to be metal reads as stone.** Foliage reads as camouflage paint on a sphere. Terrain differs from foliage only by hue.

Measured [M]: in the close view the bell's mean saturation drops from **0.74 to 0.32**, while terrain rises from **0.44 to 0.57**. The landmark goes from the most saturated object in frame to *less* saturated than the grass (saturation gap +0.30 → −0.26). The pedestal drops from luminance 138 to 108, below the terrain at 114. Separation got worse, and the focal hierarchy inverted.

What identity remains is carried by **tint**, which is exactly what the change was supposed to move away from.

### Q-B2: Is the weaker highlight realistic microstructure, or washed-out / over-roughened response?

**Neither, strictly. It is broken specular, and the visible result is washed out.** The scorecard's hypothesis ("real normal relief scattering specular") is not the primary cause.

Mechanism [S][M]:

- The shader **multiplies** the roughness map by the material roughness factor: `surface_roughness = roughness × roughness_sample` (`graphics_lab/src/LavaAdapter.jl:482`).
- The hero maps store **absolute** roughness (metal 0.18–0.46, `material_maps.rs:285`), but `apply_hero_material_set` swaps the maps without resetting the factors (`material_maps.rs:482-485`). `campaign2-hero-metal` keeps its factor of 0.22 (`lib.rs:3967`).
- Effective metal roughness is therefore **0.059–0.085**. Wet is **pinned at the 0.045 clamp** for every texel, so its roughness map is discarded entirely.
- GGX half-max half-angle: baseline metal (r≈0.18) is **1.20°**; hero metal (r≈0.072) is **0.19°**, about 6× narrower. The perturbed normal tilts 3.9° on average, **about 20× wider than the lobe**. The highlight is shredded into sub-pixel sparkles, and the 4-tap resolve then averages them away.
- With no environment map, a metal surface's ambient term is F0 × (0.08 + 0.16·(1−r)) × a dark three-colour gradient (`LavaAdapter.jl:544-556`). That is a uniform dim grey, which is exactly what the bell shows.

Real microstructure widens and dims a highlight. This one narrowed and vanished. Close p99 luminance fell **182 → 148**, and pixels ≥200 fell from 0.340% to 0.077% (`ab-hero3/compare.json`).

### Q-B3: Was the flattening regression solved, or replaced?

**Replaced, and the fix targeted the metric rather than the image.** The first attempt lost 51% of its distinct colours. The fix (`material_maps.rs:252-268`) added a third ramp stop plus a **fixed red↔blue chroma noise**: `chroma = (index − 1) × hue × 9` pushes R down and B up by the same field for *every* family. That is what puts the blue denim mottling on the stone. It restores the distinct-RGB count (close −1,438 against baseline, medium +3,046) without adding any material information. The red↔blue axis correlates with luminance at +0.79 on foliage and +0.65 on bark [M]: it is decoration riding on the same height field.

The perceptual flatness is worse than before:

- the top of the tonal range compressed (p99 182 → 148)
- the specular anchor is gone
- the landmark/background hierarchy inverted (above)

It also introduced three new failures:

1. **Copper identity deleted.** The copper lived in the old albedo *texture* (`lib.rs:2614-2619`, `[194,94,28]→[252,196,86]`). The base-colour factor is near-white (`[0.98,0.88,0.72]`, `lib.rs:3965`), so swapping in the grey metal ramp turns copper into steel.
2. **UV smear rings** on the pedestal, bell and halo (§3.3).
3. **Terrain turned monotone acid yellow-green.** The baseline's patch variation was itself camouflage blotches, but it at least broke up the field.

### Methodology defect that blocks retention regardless

`ParityPolicyCandidate::from_env` reads one environment variable, so `hero-materials` and `full` are mutually exclusive (`lib.rs:3648-3658`). **`ab-hero3` was rendered on the frozen baseline**: its sky row-identity is 98.37%, identical to baseline, so it has no dither and no shadow rework. The scorecard asks reviewers to judge it as if it stacked on the retained arm. It does not, and it cannot be retained as-is.

---

## 2. Deficiency register, by layer

Layers: A renderer capability · B material system · C texture/content · D lighting · E atmosphere · F geometry/asset semantics · G composition/art direction · H scene density · I motion · J missing engine capability · K calibration-scene ugliness.

"Now?" means the current renderer can already express the fix. "Mach?" means new machinery is needed. "Crit?" means it is on the demo critical path.

| # | Visible symptom | Layer | Technical cause | Now? | Mach? | Crit? |
|---|---|---|---|---|---|---|
| 1 | Island is a floating slab; its edge and the void beneath are in frame (medium, wide) | **G / F** | Terrain is a finite 96×72 m patch; cameras frame its edge; the env "ground" colour fills below it | **Yes** (extend terrain / skirt, distant landforms, reframe) | No | **Yes, #1** |
| 2 | Sky is a flat teal-grey wash with no sun, clouds or gradient interest; it is the largest region in every frame | **E / D** | `_environment_color` three-stop gradient (`LavaAdapter.jl:351`); zenith `[0.006,0.018,0.055]` is near-black | Partly (values yes; an analytic sky needs a shader) | Small | **Yes** |
| 3 | Lighting reads as "studio noon under a night sky"; forms flatten | **D / G** | Sun elevation ≈63° (`lib.rs:4404`, `[0.38,-1,-0.32]`) against a near-black zenith; no warm/cool split | **Yes** (values) | No | **Yes** |
| 4 | Distance does not read; the far ridge has the same contrast as the foreground | **E** | Linear distance fog at density 0.0018 (`LavaAdapter.jl:614`); no height fog, no aerial perspective | Partly | Small | **Yes** |
| 5 | Metal cannot look metallic; wet cannot look wet | **A / J** | No IBL; env specular is the gradient sampled at the reflection vector, *independent of roughness* (`:544`); ambient terms use magic constants 0.52 and 0.08+0.16·(1−r) (`:555-556`) | **No** | **Yes** (prefiltered environment + BRDF LUT) | **Yes**, or cut metal/wet from the hero |
| 6 | Copper bell turned grey | **B / C** | Hero set swaps the albedo texture that carried the hue; factor near-white | Yes | No | Yes (fix before any further review) |
| 7 | Horizontal ring smears on pedestal, bell, halo | **F** (UVs) | `append_band` gives each band V 0→1 whatever its height; U spans the full circumference. Pedestal bands: ~24.5 m × 0.16 m per texture, ≈150:1. Halo: U over 14.3 m, V over 1.0 m. No anisotropic filtering | **Yes** (arc-length UVs, texel-density normalisation) | No (aniso is blocked on a device feature) | **Yes** for any hero lathe object |
| 8 | Highlights collapsed | **B** | Roughness applied twice (§1 Q-B2) | Yes | No | Yes |
| 9 | All non-organic materials read as the same noise | **C** | One value-noise lattice primitive for every family; one height scalar drives all four channels (r = −0.98…−1.00) | Content work | No renderer work; needs real inputs | Yes for hero objects only |
| 10 | Trees are spheres on cylinders | **F / J** | No tree asset; **alpha is fenced**: the native path requires opaque (`LavaAdapter.jl:3016`) | **No** for cards; yes for opaque-silhouette imported trees | Alpha mask (validator + shader + shadow pass) | **Yes** |
| 11 | Landmark and props are lathe/block primitives with no bevels, wear or silhouette | **F** | Authored in Rust (`radial_mesh`, `block_mesh`, `torus_mesh`) | **Yes**: the imported-GLB path is closed (log-hut fixture, C2.x) | No | **Yes** |
| 12 | Objects don't sit in the ground: no debris, grass or rocks at bases; no contact darkening | **H / A** | No scatter; no SSAO/contact term; texture AO is near-inert (§3) | Partly (scatter by instances) | Contact AO is new | Scatter **yes**; SSAO later |
| 13 | Shadows soft and blobby, double-lobed under trees (wide) | **A** | One 512² map, 4-tap PCF, ≈5 texels/m (`:3606`) | Partly | Cascades | **Not yet**: fine for island scale. Becomes critical with item 1's extent |
| 14 | Wide view is empty: 34 instances, 6.5% authored pixels | **H** | Content | Yes | Performance unknown (triangle soup, CPU cull) | Yes, moderate |
| 15 | Terrain is either camo blotches (baseline) or a uniform felt green (hero); no slope or height logic | **C / B** | One albedo; region codes deliberately unmapped (`_terrain_base_color`); no splat | Partly (tiling yes) | 2–4 layer slope/height blend | **Yes** |
| 16 | "Wet pool" reads as a dark slate disc | **F / J** | A disc with a dark low-roughness material and nothing to reflect | No | Reflection (IBL minimum) | Only if water stays in the demo |
| 17 | Emissive inlays read as flat cyan stickers | **B / G** | 32×32 stripe pattern; bloom threshold 0.82 | Yes | No | Low |
| 18 | Faceting on lathe surfaces; edge crawl in motion | **A** | Triangle soup with per-triangle tangents (§1.7 of the parity audit); 4-tap resolve, no TAA | Partly | TAA later | Not for stills |
| 19 | Nothing moves | **I** | No wind, water or cloud animation | No | Yes | Later |
| 20 | Arbitrary cyan runes, a teal pool and a grey slab composition | **K** | Fixture authored for coverage, not beauty | n/a | n/a | Replace, don't polish |

**Reading the table:** items 1–4 and 10–12 are the "primitive renderer" signal, and **none of them is a material problem**. Items 1, 2, 3 and 11 need no new machinery at all.

---

## 3. Material system audit

### 3.1 Per-family measurements [M]

Built with `build_material_maps(kind, 256, "v2")` from the crate (scratch probe, release build). Effective roughness = scene factor × map, clamped at 0.045, as the shader computes it.

| Family | Albedo lin. lum (rel. sd) | Saturation | Roughness map | **Effective roughness** | AO mean / p5 | Normal tilt after scale (mean / p95) | corr(lum, rough) | Gradient anisotropy u/v |
|---|---|---|---|---|---|---|---|---|
| terrain | 0.117 (23%) | 0.53 | 0.89–0.93 | factor-dependent | 0.98 / 0.91 | n/a | −0.98 | 0.99 |
| stone | 0.184 (32%) | 0.06 | 0.70–0.82 | **0.32–0.38** | 0.97 / 0.88 | 8.4° / 16.4° | −0.99 | 0.98 |
| bark | 0.065 (34%) | 0.54 | 0.78–0.87 | 0.66–0.73 | 0.94 / 0.75 | 6.6° / 14.0° | −0.98 | **0.96** |
| foliage | 0.084 (48%) | **0.73** | 0.84–0.91 | 0.64–0.69 | 0.97 / 0.86 | 1.5° / 3.0° | −0.98 | 1.00 |
| metal | 0.383 (30%) | 0.08 | 0.27–0.39 | **0.059–0.085** | 0.92 / 0.70 | 3.9° / 7.7° | **−1.00** | **0.99** |
| wet | 0.069 (33%) | 0.26 | 0.15–0.22 | **0.045 (clamped, all texels)** | 1.00 / 0.98 | **0.2°** / 0.5° | −0.98 | 1.07 |

### 3.2 Answers

**Different material logic, or parameterisations of one source?** One source. Every family is `tileable_value_noise` / `tileable_fbm` on an integer lattice, differing only in period and weights (`material_maps.rs:161-221`). There is no Worley/cellular structure for stone, no directional structure for bark or brushed metal, no domain warp, and no layered substances such as moss on stone or rust on metal. Value noise produces the same blobby, isotropic "camo" look at any period, which is why everything converges visually.

**False claims in source comments.** Bark's "vertical fibres: high frequency across U, low across V" calls `tileable_value_noise(u, v, 24, …)` with the same period on both axes (`:185`). Metal's "anisotropic by construction" calls `tileable_value_noise(v, u, 96, …)`; swapping the arguments transposes an isotropic field and leaves it isotropic (`:202`). Measured gradient anisotropy is 0.96 and 0.99. **Neither brushed metal nor fibrous bark exists in the maps.**

**Does one shared height vocabulary make materials converge?** Yes. One height scalar drives albedo (ramp), roughness (inverted ramp), AO (blurred cavity) and normal (gradient). Roughness is a linear function of the same height that sets albedo lightness: |r| = 0.98–1.00 in every family. The surface has one degree of freedom, which is "this texel is high or low", rendered four ways. Real materials decorrelate these channels: a polished patch on dark stone, dirt that is rough but not dark, metal whose colour is constant while roughness varies.

**Are normal amplitudes too strong?** **No, not in absolute terms.** After the scene's `normal_scale`, mean tilt is 1.5–8.4°, which is restrained. They *look* too strong on the lathe objects because of UV stretch: a 150:1 stretch turns isotropic bumps into long horizontal grooves. Wet is the opposite problem: 0.2° is a mirror-flat plane.

**Is roughness too high or too noisy?** Neither as authored. As **rendered** it is wrong because it is applied twice. Metal collapses to near-mirror, wet to the clamp, and stone sits in a narrow 0.32–0.38 band that reads as satin. Terrain roughness 0.89–0.93 is effectively constant, which is fine for grass.

**Is albedo variation doing too much work?** Yes. The albedo carries baked cavity darkening (the height ramp), the red↔blue hue noise, and ±3 speckle. Relative luminance spread is 23–48%. Foliage at 48% spread and 0.73 saturation is the main reason the trees read as camo spheres. Production albedo for grass, stone and bark typically varies far less at texture scale; large-scale variation belongs in macro tinting and lighting.

**Is AO used appropriately?** The shader applies AO only to ambient terms (`:550-556`), which is correct. The maps themselves are near-inert (mean 0.92–1.00) and redundant: AO correlates +0.42…+0.78 with albedo luminance, so recesses are darkened twice, once in albedo and once in AO. The darkening the frame is missing is **geometric contact**: under the pedestal lip, where bell meets pedestal, at tree bases. Texture AO cannot provide that.

**Do metals behave like metals?** No, for four independent reasons:

1. grey albedo with 30% luminance spread (real F0 varies little across a surface; the variation should live in roughness and dirt)
2. `metallic = 0.78`, a non-physical blend; metals are 1.0
3. roughness collapsed by double application
4. no environment to reflect

**Are wet materials distinguishable from merely dark ones?** No. Wet is implemented as a separate *family* (darker albedo plus low roughness), with `metallic = 0.34` (`lib.rs:3898`). Water is a dielectric, so that value is wrong. Without reflections, low roughness only shows as a pinpoint sun glint, so wet reads as dark slate. Wetness is properly a **modifier on an underlying material**: darken and saturate its albedo, drop its roughness, flatten its normal in puddles. And it is only legible when the environment is reflected.

**Do foliage and bark use sensible response models?** No. Foliage has no transmission or wrap lighting, no alpha cutout (it can't: opaque-only), and no shape. Bark has no directional structure (above). Both are the default GGX dielectric with different noise, which is acceptable for bark once it has real structure. Foliage needs, at minimum, alpha-tested cards or imported leaf geometry plus a cheap transmission/wrap term.

**Is identity encoded through texture noise instead of physical response?** Yes, almost entirely. The physical axes that separate substances in a real frame (specular colour, roughness *contrast* between materials, reflection of the environment, Fresnel at grazing angles, translucency) are either broken (roughness), absent (environment, translucency) or unused (Fresnel has nothing to reflect).

### 3.3 Concrete defects (fix list, not judgement)

| ID | Defect | Location |
|---|---|---|
| MD-1 | Hero set swaps maps without resetting factors → roughness double-applied; albedo hue lost | `material_maps.rs:482-485`; factors at `lib.rs:3896-3990` |
| MD-2 | Bark "fibres" and metal "streaks" are isotropic | `material_maps.rs:185`, `:202` |
| MD-3 | One red↔blue chroma axis applied to every family; unused closure parameter `c` (compiler warning) | `material_maps.rs:262-268` |
| MD-4 | Band UVs: V 0→1 per band regardless of height; torus U over 14.3 m; no texel-density normalisation | `lib.rs` `BeaconMeshBuffers::append_band`, `torus_mesh` |
| MD-5 | `hero-materials` cannot combine with `full`; hero arm judged on frozen baseline | `lib.rs:3648-3658` |
| MD-6 | Non-physical metallic values: hero-metal 0.78, wet 0.34, glow 0.24, objective 0.42 | `lib.rs:3866`, `:3898`, `:3966`, `:3983` |
| MD-7 | Env specular ignores roughness; ambient uses un-normalised constants | `LavaAdapter.jl:544-556` |
| MD-8 | `Terrain` variant doc says "24 times"; constant is 8000 milli = 8 | `lib.rs:3617` vs `:3640` |

### 3.4 Verdict on the generated set

**This generated material set is not suitable as production calibration, and neither is any procedural set.** Calibration needs ground truth. If a scanned CC0 stone (ambientCG / Poly Haven class, 1–2K, measured albedo) renders wrong, the renderer is wrong. If a procedurally invented stone renders wrong, you cannot tell whether the renderer or the generator is at fault. That ambiguity is how this sprint spent its effort.

Keep `build_material_maps` as a **fixture generator** (tileability tests, residency/mip tests, per-family plumbing). Stop using it as the source of material appearance.

The system now *can* express separate materials per family, and that plumbing is worth keeping. *These* materials are not good, and no amount of tuning the generator's constants will make them good.

---

## 4. The material calibration scene

The shrine is a renderer fixture. Calibration needs a scene where every error has exactly one explanation.

### Geometry

- **Ten identical test bodies on a row of plinths**, one per material: metal, rough metal, stone, bark, wood, foliage, wet stone, painted surface, terrain, emissive. Each body is:
  - a **sphere** (UV sphere with arc-length-correct UVs, about 1 m) to read the specular lobe, Fresnel and roughness across all normal angles;
  - a **beveled cube** (1 m, 2–3 cm bevel) for flat-face albedo, normal direction and edge highlights;
  - a **material slab** (2 × 1 m, flat, tilted 15° toward camera) for texel density and tiling.
- **Foliage** gets an alpha-tested card cluster once alpha exists. Until then, it gets one imported opaque-silhouette shrub.
- **Bark** gets a cylinder (log) as well as the standard bodies, because its anisotropy is only legible along an axis.
- **Wet stone** is the *same* stone sphere/cube/slab with the wet modifier applied, placed next to the dry stone. That is the only valid wet test.
- **Rough metal** is the *same* albedo as metal with only roughness changed, side by side. That isolates roughness.
- **Reference objects in every shot:** an 18% grey card, a white card (albedo ~0.85) and a black card (~0.03). Also a chrome ball and a grey diffuse ball. These are the VFX-standard lighting references: the chrome ball shows what the environment is, the grey ball shows what the light is.
- **Ground:** a flat neutral grey plane (albedo 0.18), *not* terrain, so terrain is tested only on its own slab.
- **Scale markers:** a 1 m checker strip along the plinth row, for texel density.

### Lighting

Three fixed rigs, each rendered for every camera:

1. **Sun + neutral sky**: sun at 35° elevation, 45° side angle; uniform grey-blue sky IBL. This is the primary rig.
2. **Overcast**: no sun; bright uniform sky IBL only. This exposes albedo and AO errors with no specular to hide behind.
3. **Grazing / rim**: sun at 10° behind the subjects. This exposes Fresnel, roughness and normal-map errors.

Until IBL exists, rigs 1–3 use the current gradient environment. **Expect metal and wet to fail; that failure is the point** (§1 Q-B2).

### Cameras

- **Row wide**: all ten materials in one frame, orthographic or 30 mm-equivalent, for relative comparison.
- **Per-material close**: about 2 m from each sphere, 50 mm-equivalent, filling about 40% of frame height.
- **Grazing**: 10° above each slab, for mip, aniso and normal-crawl errors.
- **Fixed exposure** across all shots. No auto-exposure, no bloom, no vignette, no grade during calibration.

### Pass conditions (what "calibrated" means)

- The grey card renders at a fixed target value in rig 1, and albedo cards land within ±5% of their linear values.
- The metal sphere shows a coloured, environment-shaped reflection. The chrome ball shows a readable environment.
- Rough metal and metal differ **only** in highlight size and blur, not in colour.
- Wet stone differs from dry stone in roughness and reflection, not only in darkness. A blind viewer can say which one is wet.
- No material's identity survives desaturating the frame to greyscale *and* hiding the albedo (albedo forced to 0.5). If one does, its identity is coming from noise, not physical response. *Recommendation: add that albedo-0.5 override as a debug view.*

---

## 5. "First beauty frame" dependencies, by perceptual gain per cost

Target state: the largest complaint is no longer "this looks like a primitive renderer".

| Rank | Item | Why it ranks here | Cost |
|---|---|---|---|
| 1 | **World extent and horizon**: terrain continues past the frame; distant landforms (low-poly ridges, silhouette cards); the camera never sees an edge | Removes the single most "test scene" signal; affects 40–60% of wide pixels | Low (content + camera) |
| 2 | **Sky and sun coherence**: analytic sky (Preetham/Hosek-class or a baked sky texture) with a bright horizon, a sun disc and a few cloud forms; sun lowered to 15–30°; warm sun against a cool sky | The sky is the largest single region in every frame. A golden-hour sun gives long shadows and form shading for free | Low–medium |
| 3 | **Aerial perspective and height fog**: distance-dependent desaturation and blue shift toward the horizon colour, plus valley fog | This is what makes scale read. The core of both target games' look | Low–medium |
| 4 | **Real assets for three to five hero items**: one landmark structure, two rock types, one tree species, one ground-cover type. Imported, with 1–2K real textures | Turns "primitives" into "things" | Medium (sourcing + import, which is closed) |
| 5 | **Terrain layering**: 2–4 layers blended by slope and height (grass / dirt / rock / path), each tiled, plus macro variation | The ground is the second-largest region | Medium |
| 6 | **Prefiltered environment / IBL**: irradiance, prefiltered specular, BRDF LUT; env specular becomes roughness-aware | Unblocks metal and wet. Grounds every object in the sky's colour | Medium |
| 7 | **Grounding and density**: scattered rocks and debris at object bases; ground-cover instances; screen-space contact AO | Objects stop floating | Medium |
| 8 | **Alpha-tested foliage** (validator + shader + shadow pass) | Required for believable trees and grass | Medium |
| 9 | **Authored composition**: a hero shot framed on the landmark with foreground, midground and background layers and a value structure | Turns the above into an image instead of a sample | Low (but needs a human eye) |
| 10 | **Cascaded shadows** | Needed once the world extends; not before | Medium–high |
| 11 | Local lights | Only matters for interiors and night. Not for a first daylight beauty frame | Medium |
| 12 | Water | Only if a hero composition needs it; requires IBL first | High |
| 13 | Depth capture | Engineering infrastructure for grounding metrics. **Not perceptual**: schedule it with item 7 so contact AO can be measured | Medium |

Items 1–3 and 9 need no new rendering features beyond a sky shader and fog terms. **They should come before any further material work.**

---

## 6. Rendering versus world authoring

Luxel has a strong **authority chain**: `AssetPreparationReceipt → RuntimeAssetPackage → SceneObject → SceneArtifact → GraphicsScenePacket` (`docs/platform/scene-object-contract.md`). It has a **semantic facade** that lists `world.construct` and `scene.compose` but marks them planned/partial (`docs/content-sdk/semantic-facade-contract.md`). The renderer receives `mesh + material + transform`. Nothing in between knows what a tree, a road or a village is.

The missing layer is not an ontology. It is a small number of **placement grammars**: functions that take a semantic intent plus a few parameters and emit SceneObjects with correct ground contact, variation and density.

### Needed immediately (real abstractions; the first beauty frame depends on them)

| Abstraction | What it does | Why now |
|---|---|---|
| **Landmark** | One hero object with an importance tier, a guaranteed sightline/camera relationship, and a silhouette check | Elden-Ring-class presentation is landmark-driven. Composition needs a typed focal point |
| **Ground-cover / scatter region** | Area + asset set + density + slope/height constraints + seed → instances conformed to terrain | Density and grounding are impossible to hand-place at scale |
| **Terrain surface layers** | Layer set + blend rules (slope, height, mask) bound to the terrain material | Region codes exist but are deliberately not mapped. This is the mapping |
| **Horizon / backdrop** | Distant landform ring + sky + fog intent as one unit | Solves item 1 permanently instead of per-camera |
| **Forest** (as scatter with a canopy-density rule) | Tree instances with spacing, edge falloff and species mix | The single most common Witcher-3 landscape element |

### Semantic stubs (vocabulary entry, typed intent, a trivial implementation that places an imported asset)

`shrine`, `ruin`, `house`, `fort`, `bridge`. Each is "place this imported asset (or small kit) at this anchor with terrain conformance". Their semantics (doors, interiors, connectivity) can wait.

`road` / `path` as a **terrain-layer stamp along a spline** (a texture/height carve, no mesh). This is cheap and a big readability gain.

`river` as a stub that only carves terrain and places a flat dark plane, until water exists.

### Ordinary imported assets (no abstraction at all)

Rocks, props, individual trees, fences, debris, the shrine's own pieces. They become `SceneObject`s through the existing GLB path. Do not wrap them in semantics.

### Explicitly not now

`village`, settlement layout, road networks, biome systems, procedural architecture. Those are world-generation systems. They are not needed for one beautiful frame, and building them first is the feature-accumulation trap.

---

## 7. The beauty target, stated correctly

Witcher 3 and Elden Ring are not beautiful because of feature count. Both shipped on hardware and techniques that Luxel's contract can mostly express. What they share:

1. **Every frame has a sky worth looking at.** Strong, art-directed skies with a light source you can locate, colour temperature contrast between sun and shade, and weather/time-of-day as mood. The sky is the largest region and gets the most authoring attention.
2. **Atmosphere carries scale.** Aerial perspective pushes distant layers toward the sky colour, so the eye reads depth in planes: foreground, midground, background, sky. Elden Ring's Erdtree and Witcher's Skellige mountains are legible because of fog, not geometry detail.
3. **Landmarks and sightlines.** Elden Ring is built around "see it, go there". A strong silhouette on the horizon, placed deliberately relative to the player's view. Luxel has no concept of a sightline or a view-of-landmark.
4. **Density with hierarchy.** Witcher's ground is never empty: grass, flowers, rocks, debris. It is still organised: paths are clear, the landmark area is calmer. Density without hierarchy is noise.
5. **Material credibility, not material complexity.** The surfaces are usually well-scanned or well-painted textures with consistent texel density and roughness that *contrasts between materials*. Wet stone reflects the sky. Metal reflects the environment. Nothing is cleverer than that.
6. **Composition and value structure.** Dark foreground framing, a lit focal point, a recessive background. That is painterly organisation of light and dark, authored per vista.
7. **Motion.** Wind in foliage, clouds, water, particles. A still Witcher frame is good; a moving one is alive.

What Luxel should **not** chase for this target: path tracing, virtual geometry, ray-traced GI, high-end SSS, hair systems, volumetric clouds with full multiple scattering. None of those is why those games are beautiful.

The practical consequence: **beauty is an authoring problem on top of a modest, complete feature set**. The feature set is nearly complete: it lacks IBL, an analytic sky, aerial perspective, alpha masking and cascades. The authoring layer barely exists.

---

## 8. Convergence plan

### IMMEDIATE (before another large graphics sprint)

| # | Item | Payoff | Cost | Depends on | Success condition | Human review? |
|---|---|---|---|---|---|---|
| I-1 | **Reject `ab-hero3`**; record the reasons in the scorecard (copper loss, double roughness, UV smear, wrong baseline) | Stops a regression shipping | Trivial | None | Scorecard Q-B1/B2/B3 answered "No / broken / replaced" with this audit linked | Yes (sign-off) |
| I-2 | **Fix MD-1** (factor = 1.0 when a map carries absolute values, or author maps as multipliers) and **MD-5** (hero composes with `full`) | Makes any future material arm judgeable | Low | None | Effective metal roughness within ±0.02 of the authored map value; hero arm sky row-identity = `full`'s 18.77% | No |
| I-3 | **Fix MD-4** (arc-length UVs on band, torus and cap meshes; one texel-density target per mesh in texels/m) | Removes ring smear | Low | None | Measured texels/m along U and V within 2:1 on every authored mesh (new test) | Yes (one look) |
| I-4 | **Reframe the cameras and extend the world** so no camera sees an edge or the void | Largest perceptual gain per hour available | Low | None | Non-content pixels in the medium/wide views come only from sky above the horizon (void fraction = 0) | Yes |
| I-5 | **Sky and sun values pass**: lower the sun to 20–30°, warm it; raise the zenith to a daylight value; widen the horizon band | Lighting coherence | Trivial | None | Sky gradient has a visible sun-side/anti-sun asymmetry; shadow length ≥1× object height | **Yes** |
| I-6 | **Build the calibration scene (§4)** with the current renderer, using 6–10 CC0 scanned material sets | Ground truth for every later decision | Low–medium | I-2, I-3 | Grey card lands on target; the albedo-0.5 override view exists; the known failures (metal, wet) are documented as expected | Yes |
| I-7 | ~~**Add one measurement**: landmark-to-background separation~~ **REJECTED in CONVERGE-0 (§0.1): both formulations rank the hero regression as better.** Replaced by a content-level hue-preservation test | — | — | — | — | — |

### NEXT (to the first credible beauty frame)

| # | Item | Payoff | Cost | Depends on | Success condition | Human review? |
|---|---|---|---|---|---|---|
| N-1 | **Analytic sky + aerial perspective + height fog** as typed environment policy | Very high | Medium | I-5 | The far ridge's contrast against the sky is ≤40% of the foreground's contrast; sky has a sun disc and horizon glow | **Yes** |
| N-2 | **Prefiltered IBL** from the same sky (irradiance + specular mips + BRDF LUT); roughness-aware env specular; delete the magic ambient constants | High (metal, wet, grounding colour) | Medium | N-1 | Calibration: chrome ball shows the sky; metal ≠ rough metal only in blur; a blind viewer identifies wet vs dry stone | **Yes** |
| N-3 | **Backdrop/horizon abstraction** + terrain extended with distant landforms | High | Medium | I-4 | No edge visible from any of N authored cameras plus 20 random cameras on the playable area | Yes |
| N-4 | **Terrain layers** (2–4 scanned sets, slope/height blend, macro variation) | High | Medium | I-6 | No visible tile period at 50 m in the wide view; layer transitions follow slope | **Yes** |
| N-5 | **Hero asset kit, imported**: one landmark structure, two rocks, one tree, one ground cover; real textures; real metal on the landmark | Very high | Medium (sourcing) | I-6, N-2 | The shrine primitives are gone from the beauty frame | **Yes** |
| N-6 | **Alpha-mask foliage**: validator, shader discard, shadow pass | High | Medium | None | One card-based tree and grass clump render with correct shadows; no alpha artefacts at 3 camera distances | Yes |
| N-7 | **Scatter region + forest abstraction** | High | Medium | N-5, N-6 | Wide-view authored-pixel fraction ≥30% (from 6.5%) at stable frame cost | Yes |
| N-8 | **Contact AO (screen-space) + depth capture** | Medium | Medium | Depth capture | `grounding_contact` moves from `indeterminate` to measured | Yes |
| N-9 | **Author the beauty shot**: one golden-hour vista with foreground framing, the landmark lit, background in haze | Decisive | Low (eye-bound) | N-1…N-7 | Independent viewer, shown the frame unlabelled next to two commercial screenshots, does not pick it out as the engine test | **Mandatory** |

### LATER (full Sidhe / Arena production quality)

| # | Item | Payoff | Cost | Depends on | Success condition | Human review? |
|---|---|---|---|---|---|---|
| L-1 | Cascaded shadows (2–3) + better filtering | High once the world extends | Medium–high | N-3 | Shadow texel density ≥20 texels/m within 30 m of camera | Yes |
| L-2 | TAA (or a stable alternative) + specular AA (roughness from normal variance) | Medium; high in motion | Medium–high | Motion vectors | No highlight or foliage shimmer in a 10 s camera pan | Yes |
| L-3 | Wind animation (vertex) + cloud motion | High in motion | Medium | N-6 | Foliage moves coherently with one wind field | Yes |
| L-4 | Local lights (bounded list) | Interiors, night, torches | Medium | N-2 | Arena torches light the arena floor with shadowed falloff | Yes |
| L-5 | Water (planar or SSR + IBL fallback, depth fade, shoreline) | Medium–high per scene | High | N-2, depth | Shoreline reads without a hard line; the sky reflects | Yes |
| L-6 | Texture compression (BC7/BC5) + streaming | Scale, memory | Medium | None | 1–2K sets for 30+ materials fit the residency budget | No |
| L-7 | Indexed geometry + imported tangents everywhere (end triangle soup) | Faceting, perf | Medium | None | Imported smoothing groups survive; vertex count = unique vertices | No |
| L-8 | Time-of-day and weather as style policy | High for mood | Medium | N-1, N-2 | Two style briefs produce two distinct moods from one scene | **Yes** |
| L-9 | Volumetric light shafts (cheap, sun-only) | Medium; signature Elden Ring | Medium | N-1, L-1 | Shafts only where shadowed geometry occludes the sun | Yes |

---

## A. What the renderer can already do

- **Correct image formation for diffuse and dielectric surfaces.** GGX + Smith + Schlick with a clearcoat lobe (`LavaAdapter.jl:443-588`), correct sRGB decode of albedo/emissive (`:2125`), ACES tone map, ordered dither that measurably fixed sky banding (98.37% → 18.77% row identity), restrained bloom and vignette.
- **Real shadows without the old 0.25 floor**, from one directional light. Reviewer-confirmed "more planted".
- **Per-material normal, roughness, occlusion and emissive maps** with UV-derived tangents, mip chains (C2.7/C2.8), and per-surface samplers with repeat wrap and tiling (F-4).
- **Imported static assets** through a validated GLB → RenderAssetPackage → SceneObject → packet chain, with deterministic replay (log-hut fixture).
- **Typed render policy that fails closed**, and a deterministic A/B harness with difference sheets. That infrastructure is genuinely good, and it is what made this audit's measurements cheap.
- **Linear distance fog** and a three-stop gradient environment, which can be retuned today without code.

## B. What the current materials are doing wrong

- **Roughness is applied twice.** Metal renders near-mirror (0.06–0.085), wet is pinned to the 0.045 clamp, and the highlights shred and disappear (MD-1).
- **The copper is gone.** The hue lived in the replaced texture; the landmark now reads as grey stone.
- **One noise, four channels.** Roughness = −1 × albedo luminance (|r| ≥ 0.98) in every family. Every family is isotropic value noise; the "fibres" and "brushed streaks" don't exist (MD-2).
- **Identity is carried by albedo noise and a red↔blue chroma axis** shared by every family (MD-3), not by physical response.
- **Lathe UVs smear textures up to 150:1** into horizontal rings (MD-4).
- **Wet is a family instead of a modifier**, metallic values are non-physical (0.78, 0.34), and texture AO double-counts albedo cavity darkening.
- **Metal and wet are being asked to work without an environment to reflect.** No material map can fix that.
- **They are procedural inventions used as calibration**, so a bad result cannot be attributed to the renderer or the generator.

## C. What the calibration scene is hiding

- **It is not a calibration scene.** It has no grey card, no chrome ball, no reference albedo, no same-material/different-roughness pairs, no dry/wet pair, and no fixed exposure reference. Every material appears on a different geometry type at a different UV density under one fixed light.
- **The island-in-a-void framing** puts 29–62% of the frame in featureless sky and void. A materials change can only move 6.5–23.5% of pixels, so frame-level metrics (distinct RGB, p99) are dominated by sky and terrain and say little about the materials.
- **The distinct-RGB metric rewards noise.** Adding chroma speckle passed it while making the frame worse. "Material separation" measured as colour-bin count cannot distinguish substances from noise.
- **There is no hierarchy measurement**, so a change that inverts the landmark/background relationship scores as neutral.
- **Lathe-object UVs** make every texture look wrong in a way that resembles "bad material", masking whether the material itself is bad.
- **The single high sun** hides normal-map, Fresnel and roughness errors that a grazing light would expose.
- **The hero arm being on the frozen baseline** hid its interaction with the retained shadow and post work.

## D. What the beauty scene must prove

The first beauty frame must prove, in one unlabelled screenshot:

1. **Scale**: a world that continues past every edge of the frame, with at least three depth planes separated by atmosphere.
2. **A located light**: a sky with a sun the viewer can point to, warm/cool contrast between sun and shade, and shadows long enough to describe form.
3. **A landmark**: one silhouette that reads at the wide distance and is the brightest or highest-contrast element in its region.
4. **Credible materials on real assets**: stone that reads as stone at both distances, metal that reflects the sky, ground that varies by slope, and vegetation with leaf-scale silhouette.
5. **Grounding**: no object floats. Contact darkening and scatter at every base.
6. **Authorship**: a framed composition with foreground, a focal point and recession, not a turntable.

It does **not** need to prove characters, interiors, night, water, motion, many lights, or scale beyond one vista.

## E. The minimum path to Witcher-3-class visual completeness

1. I-1 to I-5: reject the hero arm, fix MD-1/4/5, extend the world, fix the sky and sun values. **About a week of focused work; no new machinery.**
2. I-6: build the calibration scene with scanned CC0 materials. From here on, no procedural material is used for appearance.
3. N-1: analytic sky + aerial perspective + height fog.
4. N-2: prefiltered IBL from that sky. Metals and wet become possible.
5. N-4: terrain layers by slope and height.
6. N-5 + N-6: an imported hero kit, alpha-masked foliage, real trees.
7. N-7: scatter and forest regions; density to ≥30% authored pixels in wide.
8. N-8: contact AO.
9. L-1 + L-2 + L-3: cascades, temporal stability, wind. "Completeness" in Witcher's sense is a moving, dense, stable world, and that is where these land.

Completeness means *no missing category*: sky, atmosphere, terrain, vegetation, architecture, props, grounding, motion. Every category gets a credible, not cutting-edge, implementation.

## F. The minimum path to Elden-Ring-class total presentation

Everything in E, plus the parts that are authorship rather than features:

1. **Landmark + sightline as typed concepts.** The landmark's silhouette and the cameras/paths that reveal it are authored together. A reveal is a camera event: an occluder that opens onto a vista.
2. **A per-vista value structure.** Dark foreground framing, a lit focal plane, a hazy background. This is an art-direction pass per beauty shot, checked by the hierarchy metric (I-7) and by a human.
3. **A strong palette under a style profile.** Elden Ring's golden Erdtree light against desaturated grey-green land is a style decision. `docs/world/style-profile-contract.md` already has the lighting, atmosphere and composition dimensions; they need to drive N-1/N-2 values instead of hard-coded constants.
4. **Time of day / weather as mood** (L-8), and cheap sun shafts (L-9).
5. **Scale contrast**: tiny foreground detail against huge distant forms. This needs the backdrop abstraction (N-3) to carry genuinely large distant geometry.

None of F is a renderer feature that doesn't already appear in E. **The difference between E and F is authorship and art direction, not technology.**

## G. What NOT to build yet

- **More procedural material synthesis.** No Worley stone, no fancier noise, no fourth ramp stop. Use scanned sets.
- **Anisotropic filtering workarounds.** It is correctly refused on a missing device feature. Fix the device feature when convenient; don't fake it.
- **Cascaded shadows before the world extends** (N-3). At island scale they buy nothing visible.
- **Local lights / clustered lighting** before a daylight beauty frame exists.
- **Water** before IBL.
- **SSR, ray tracing, GI, volumetric clouds, virtual geometry, meshlets.**
- **Village / settlement / road-network / biome generators.** Stubs only (§6).
- **More frame-level proxy metrics** like distinct-RGB. The next metric should be the landmark-separation one (I-7), and after that the calibration-scene pass conditions (§4).
- **Character, skinning or TetCage rendering work** on the beauty-frame path. That is necessary for the product, but orthogonal to "is this frame beautiful".
- **Further A/B arms on the shrine fixture.** It has answered every question it can.

## H. Exact next handoff to Buffy/Codex

**Sprint name:** CONVERGE-0: *Fix the frame before feeding it*
**Scope rule:** no new rendering features except where listed. No procedural material appearance work.

1. **Reject `ab-hero3`.** In `artifacts/parity/REVIEW_SCORECARD.md`, mark B as **REJECTED**, answer Q-B1 "No", Q-B2 "broken specular (double-applied roughness), reads washed-out", Q-B3 "replaced by identity loss + UV smear; fix targeted the metric", and link this audit.
2. **MD-1.** When `apply_hero_material_set` (or any absolute-value map) is applied, set the material's `roughness` factor to 1.0 and keep a hue-bearing albedo for coloured metals; or change the generator to emit multipliers. Add a test asserting effective roughness = authored map value ±0.02 for every remapped material.
3. **MD-5.** Make `hero-materials` a content flag orthogonal to `LUXEL_PARITY_RENDER_POLICY`, so `full + hero` is renderable. Re-render only after items 2 and 4 land.
4. **MD-4.** Arc-length UVs for `append_band` (V proportional to band slant length, U proportional to circumference) and `torus_mesh`, scaled to a declared texels-per-metre target. Add a test computing texels/m along U and V per authored mesh, asserting ≤2:1.
5. **MD-6.** Metallic values to 0.0 or 1.0 for all campaign2 materials (wet → 0.0).
6. **World extent.** Extend the terrain (or add a skirt plus distant low-poly landforms) so that **no** campaign2 camera sees the terrain edge. Gate: `void_fraction = 0` per view (new measurement: pixels below the horizon that are not terrain or geometry).
7. **Sky and sun values only** (no shader work). Sun elevation 20–30°, warm colour; zenith raised to a daylight blue; horizon brightened. Produce a review sheet `review-converge0/` with `full` vs `full+fixes`.
8. **Landmark-separation metric** (I-7) added to `visual_vector`: luminance and saturation contrast of `InstanceImportance::Landmark` pixels against an annulus around them. Assert it fails on `ab-hero3` and passes on `full`.
9. **Calibration scene** (§4) as a new fixture `calibration-materials`: ten bodies × (sphere, beveled cube, slab), grey/white/black cards, chrome and grey balls, three light rigs, fixed exposure, the albedo-0.5 debug override. Import 6–10 CC0 scanned material sets through the existing texture path at 1K. Record the expected failures (metal, wet) explicitly as "blocked on IBL".
10. **Do not start** N-1 (sky/atmosphere) or N-2 (IBL) in this sprint. Write their contracts only, as the hand-off for CONVERGE-1, with the calibration scene as their acceptance fixture.

**Exit gate for CONVERGE-0:** all tests green, plus three human-reviewed sheets (calibration row wide, medium shot, wide shot) where the reviewer answers one question: *"What does your eye hit first?"* If the answer is still "it's a test scene", CONVERGE-0 is not done. Expected answers after CONVERGE-0: "the metal doesn't look like metal" and "the trees are balls". Those are exactly what CONVERGE-1 (IBL + sky/atmosphere) and CONVERGE-2 (imported kit + alpha foliage) are for.
