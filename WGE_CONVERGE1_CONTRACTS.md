# WGE CONVERGE-1 Contracts

Status: **CONVERGE-1 CLOSED, 2026-10-04.** N-1, CALIBRATION-1 and N-2 implemented,
human acceptance met for all three; measured shortfalls and carried items in
"CONVERGE-1 closeout" at the end. (Recipes written 2026-10-03.)
Source: `WGE_GRAPHICS_CONVERGENCE_AUDIT.md` §H step 10, revised by the CONVERGE-0 run
(`artifacts/parity/CONVERGE0_SPIRAL_LEDGER.md`).

Three contracts. Each names the owner layer, the permitted files, the tests, the
acceptance evidence, and the non-goals. None of them may start until the
CONVERGE-0 human review has answered "what does your eye hit first?"

> **Gate answered 2026-10-03:** "the odd fire-hydrant shrine lookin thing, but
> also that we have a real landscape and lighting." Not "a test scene", so the gate
> passes. Order approved: N-1 → CALIBRATION-1 → N-2. The hero prop is CONVERGE-2
> content. "Distance reads as distance" (C0-3) and tile repetition (N4-2) were not
> answered; C0-3 is N-1's human acceptance item.

---

## 1. N-1 — Sky and atmosphere (remainder)

> **Status: IMPLEMENTED (2026-10-04), human acceptance open.** Arm `converge1`
> (`WGE_PARITY_RENDER_POLICY=converge1`, converge0 content). Measured: sun-side
> horizon 2.72x the anti-sun horizon (target >= 1.15x, met); backdrop/foreground
> contrast ratio 0.65 (target <= 0.40, NOT met; converge0 measures 1.21). Null,
> `full` and converge0 render byte-identical to before (9/9 views). Edge
> authority all `good` (657 / 479 / 224 bp). Review sheets:
> `artifacts/parity/review-converge1/` (vs converge0) and
> `artifacts/parity/review-converge1-dense/` (vs a denser haze that measures 0.42).
> Implementation notes at the end of this section.
> **Human acceptance (2026-10-04, close sheet vs converge0):** distance reads as
> distance — "it's reading as distance, yeah." Character: "I wouldn't call it
> milky, I'd call it foggy zone for sure though."
> Density: the 45 bp setting is preferred over 70 bp ("close_baseline looks
> better than close_dense").

### What CONVERGE-0 already landed

- `render_policy.sky`: per-pixel view-direction sky along the same horizon→zenith
  gradient the materials are lit by, horizon colour below the horizon, sun disc
  and two-lobe glow along the key light (`LavaAdapter._sky_view_fragment`).
- converge0 content: fog colour coupled to the horizon colour, so distance fades
  toward the sky.

### What is still missing

1. **Exponential, height-dependent fog.** `_apply_fog` is linear in distance
   (`w = d × density`, clamped 0.92). Aerial perspective is `1 − exp(−∫σ(h) ds)`
   with σ falling off with altitude. Linear fog over-hazes the near field and
   under-hazes the far field; CONVERGE-0 had to pick one density for both.
2. **In-scattering colour that depends on view vs sun angle.** Haze toward the
   sun is warmer and brighter than haze away from it. Today the fog colour is a
   single constant.
3. **An analytic sky.** The gradient is two colours and a `sqrt`. A Preetham- or
   Hosek-class model (turbidity, sun elevation) gives the horizon glow and the
   anti-solar darkening that make a sky read as a sky.

### Contract

- **Policy axis:** `render_policy.atmosphere { height_falloff_milli_per_m,
  density_at_ground_bp, sun_scatter_gain_bp }`. Absent = linear fog, byte-identical.
  Bounds stated in `render_policy.rs`, mirrored and `_exact_keys`-checked in
  `WGEGraphics.jl`.
- **Sky model:** extend `SkyPolicy` with `model: gradient | analytic { turbidity_milli }`.
  `gradient` stays the default so CONVERGE-0 packets keep their bytes.
- **One function:** the analytic sky must also drive ambient
  (`_environment_color`), or the sky the eye sees and the light the materials get
  diverge again.
- **Permitted files:** `render_policy.rs`, `lib.rs` (candidate arms only),
  `WGEGraphics.jl`, `LavaAdapter.jl`, `tests/render_policy.rs`,
  `tests/converge0.rs` (or a new `tests/converge1.rs`), `graphics_lab/test/render_policy.jl`.
- **Tests:** absent axes byte-identical (null and `full` renders vs frozen /
  HEAD renderer, as CONVERGE-0 did); bounds; Julia unknown-key refusal.
- **Acceptance (measured):** in the wide view, the backdrop ridge's luminance
  contrast against the sky ≤ 40% of the foreground's contrast against terrain;
  the sky's sun-side horizon luminance exceeds the anti-sun horizon by ≥ 15%.
- **Acceptance (human, mandatory):** "distance reads as distance" on the
  converge0 wide view, side by side with CONVERGE-0.
- **Non-goals:** volumetric clouds, god rays, multiple scattering, time of day.

### Implementation (N-1)

- **Schema:** `SkyPolicy.model: Option<SkyModel>` (`gradient {}` | `analytic
  { turbidity_milli in [2000, 10000] }`, absent = gradient, key omitted) and
  `RenderPolicy.atmosphere: Option<AtmospherePolicy>` (k in [0, 1000] milli/m,
  σ₀ in [0, 1000] bp/m, sun gain in [0, 20000] bp). Atmosphere requires `sky`.
  Mirrored and `_exact_keys`-checked in `WGEGraphics.jl`. `Gradient` is an
  empty struct variant because serde ignores `deny_unknown_fields` on unit
  variants of internally tagged enums (a test caught `{"kind":"gradient",
  "turbidity_milli":3000}` being accepted).
- **Sky:** luminance = Perez distribution with the Preetham turbidity fit,
  scaled so the zenith keeps `luminance(sky_top_rgb)`. Hue = the authored
  gradient along the same ray. Preetham's chromaticity fits were implemented
  first and dropped: measured on CPU they give a salmon horizon at T = 3 (r/g/b
  0.51/0.44/0.43 across the sun) and magenta at T = 2, the known defect
  (Zotti et al. 2007); the first render's sky read lavender.
- **One function:** `_sky_radiance` is what the sky pass draws, what haze fades
  toward, and (via `_sky_environment`) what ambient samples.
- **Atmosphere:** closed-form optical depth of σ₀·exp(−k·h) along each
  camera→surface segment (checked against quadrature to 2e-5). Surfaces fade
  toward `sky_radiance(ray) + sun lobe` (Henyey–Greenstein, g = 0.6). The sky
  pass gets the same lobe and is NOT fogged: the first version also fogged the
  sky along its (finite) column, which turned the whole sky into an overcast
  sheet at any density that hazed the ridge. With the sky as the asymptote a
  distant ridge converges on exactly the sky behind it, and `fog_color_rgb` /
  `fog_density` are unused under this axis.
- **Plumbing:** two vertex-output slots (18 sky, 19 atmosphere; layered terrain
  layer uniforms moved to 20–26, 27 of 32 slots). Lava's `frag_args` are typed
  but never packed, so fragment shaders still receive per-draw values only as
  vertex outputs.

### Corrections to this contract

1. "Linear fog over-hazes the near field and under-hazes the far field" is
   backwards at equal density: 1 − exp(−σd) ≤ σd for all d, so exponential haze
   is always weaker, and tuning it to haze the ridge raises near-field haze too.
   What N-1 actually gains is (a) haze that converges on the sky behind each
   pixel instead of a constant grey, (b) a sky with sun-side glow and anti-sun
   darkening, (c) valleys hazier than ridges.
2. The ratio target (<= 0.40) was set without measurement. This world's ridge
   is ~150–500 m away; fogging it that hard needs a ~500 m visual range, which
   puts ~18% haze on objects 30 m away (`review-converge1-dense/`, 0.42). The
   chosen setting (σ₀ 45 bp, k 10 milli/m) reaches 0.65 with a clean near
   field. Real aerial perspective acts over kilometres; a larger world, not a
   denser fog, is what closes the gap. The threshold was NOT changed and the
   image was NOT tuned to it.

### Measurement (`tools/atmosphere_measure.py`)

Depth is reconstructed by marching each pixel's ray against the packet's terrain
with the renderer's triangulation (silhouettes verified against the frame);
meshes are masked by projected bounding spheres. Michelson contrast across
terrain occlusion edges, classed by near-side distance: backdrop >= 150 m
against sky, foreground <= 100 m against terrain. The foreground cut was 60 m
as first written; the wide view has no terrain occlusion edge nearer than
~80 m, so it was moved to 100 m from geometry alone before any converge1 frame
was measured. Control: converge0 = 1.21 (fails, as it should).

| Arm | backdrop | foreground | ratio |
|---|---|---|---|
| converge0 | 0.263 | 0.218 | 1.21 |
| converge1, first design (sky fogged, σ₀ 50, k 15) | 0.114 | 0.169 | 0.67 |
| converge1, dense (σ₀ 70, k 10) | 0.058 | 0.137 | 0.42 |
| **converge1 (σ₀ 45, k 10)** | 0.109 | 0.168 | **0.65** |

---

## 2. N-2 — Prefiltered image-based lighting

> **Status: IMPLEMENTED (2026-10-04), human acceptance open.** `render_policy.ibl`
> and the `*-ibl` calibration rigs. Measured in CALIBRATION-1 (run3, 88 frames;
> pre-IBL → IBL, sun rig): chrome ball reflects sky over ground (upper/lower
> luminance 1.63, upper blue/red 1.62 vs lower 1.05): met. Metal vs rough metal
> chromaticity Δ 0.0154 → **0.0102** (target ≤ 0.01): missed by 0.0002. Wet vs
> dry: median luminance Δ 57% → 46% (target ≤ 25%) and specular lobe
> (p99 − median) 1.59x → 1.80x (target ≥ 2x): NOT met — and the luminance
> target contradicts §3's own wet recipe (albedo x 0.6 makes the body ~40%
> darker by construction). Grey card under IBL: −0.1% (sun), −0.3% / −0.1%
> (overcast). With IBL absent, frames are byte-identical to HEAD + the
> transpose fix alone (12/12; see "Renderer defects" below). Sheets:
> `artifacts/calibration/run3/sheets/{sun,sun-ibl,overcast,overcast-ibl,...}.png`.
>
> **Human acceptance (2026-10-04): met.** Metal: "That looks like metal to me.
> Highest quality representation? Not really, but it does, in fact, look like
> metal." Blind wet vs dry (sun-ibl row): identified correctly; wet close-up:
> "very wet stone. Beginning of Nioh 1 lookin stone." Chrome: "mostly polished
> metal".

### Why it is next

Metals and wet surfaces cannot read correctly without something to reflect.
Today env specular is `_environment_color(reflect(v, n))`: the gradient sampled
along the mirror direction **regardless of roughness**, scaled by un-normalised
constants (`0.08 + 0.16·(1−r)`, diffuse `0.52`) (`LavaAdapter.jl`
`_material_response`). Rough and polished surfaces reflect the same sharp
gradient at different brightness.

### Contract

- **Bake in Rust, not in the shader:** from the packet's environment (and the
  N-1 analytic sky when present), produce (a) a 32×32×6 irradiance cube or 9-coefficient
  SH, (b) a prefiltered specular cube with a mip per roughness level
  (128² base, 6 mips, GGX-importance-sampled), and (c) a 64×64 split-sum BRDF LUT.
  Deterministic, digest-bound, carried as packet textures through the existing
  C2.7/C2.8 mip-chain path.
- **Policy axis:** `render_policy.ibl { enabled: bool }`. Absent = historical
  constants, byte-identical.
- **Shader:** replace `ambient_diffuse_scale` / `ambient_specular_scale` with
  `irradiance(n) × albedo × (1 − F)·(1 − metal)` and
  `prefiltered(r, roughness) × (F0 × A + B)`, multiplied by AO.
- **Permitted files:** a new `ibl.rs` in `native_graphics_contract`,
  `render_policy.rs`, `lib.rs`, `WGEGraphics.jl`, `LavaAdapter.jl`, tests.
- **Tests:** bake determinism (bit-identical across runs); white-furnace test
  (a white environment on a white rough dielectric sphere returns ≈ albedo,
  ±3%); energy test (metal sphere reflectance never exceeds F0 × environment);
  absent axis byte-identical.
- **Acceptance (measured, in the CALIBRATION-1 scene):** chrome ball shows the
  sky gradient and sun; metal and rough-metal spheres differ in highlight blur
  only (mean chromaticity within 0.01); wet stone vs dry stone luminance
  difference ≤ 25% while specular-lobe energy differs ≥ 2×.
- **Acceptance (human, mandatory):** blind identification of wet vs dry stone;
  "the metal looks like metal".
- **Non-goals:** local reflection probes, SSR, planar reflections, GI.

### Implementation (N-2)

- **Bake (`src/ibl.rs`).** From the packet's own sky: gradient or N-1 analytic,
  plus the sky pass's sun glow and the atmosphere's sun lobe, ground below the
  horizon; WITHOUT the sun disc (the key light is lit directly). Leaving the
  glow out was tried first and was wrong: the analytic sky's near-sun luminance
  spike carries the authored blue hue (2.88 in blue at 20° from the sun), and
  the drawn sky reads white-warm there only because the glow is added on top.
  Irradiance/π by SH9 (4096 Fibonacci samples); GGX prefilter (split-sum,
  N = V = R, 128 Hammersley samples) at perceptual roughness k/5; split-sum
  BRDF table (Karis, k = α/2, 256 samples).
- **Format.** Lava samples with implicit LOD only, so the roughness levels are
  not a mip chain: one single-level 298 x 130 RGBA8 atlas of octahedral tiles
  (128² … 4², then 32² irradiance) with fold-mirrored gutters, stored
  sRGB(radiance / 8), and a 64² table (R = A, G = B). The shader blends two
  adjacent roughness tiles itself.
- **Binding.** Ordinary packet textures with reserved ids; packet validation
  re-bakes and requires an exact match, refuses them when the axis is off, and
  `apply_ibl` is called by every producer after lights, environment and policy
  are final (the calibration rigs). Every surface descriptor set now has 16
  bindings (IBL at 14/15; 1x1 stand-ins when off) so pipelines share a layout.
- **Shader.** `_material_response` IBL branch: diffuse (1 − F)(1 − metal)·albedo
  ·irradiance/π, specular prefiltered·(F0·A + B) with roughness-aware Schlick F,
  clear coat from the prefiltered tile at coat roughness, all × AO. Placed
  before the historical expression, which is textually unchanged.
- **Calibration rigs.** `sun-ibl`, `overcast-ibl`, `grazing-ibl`,
  `sun-albedo-grey-ibl`; the originals still reproduce pre-IBL frames. Exposure
  set on the grey card again: normalised sky light is brighter than the 0.52
  constant (sun 0.78, overcast 1.60). `render-calibration` now records sphere
  probes (mean linear RGB, chromaticity, median / p99 luminance, upper / lower
  halves); `tools/ibl_acceptance.py` computes the measured criteria.
- **Tests (`tests/ibl.rs`, Julia render_policy).** Bake bit-identical across
  runs; Rust sky equals the Julia shader function within 2e-4; white furnace:
  white rough dielectric sphere = albedo ±3%; split-sum energy (A + B ≤ 1;
  F0·A + B ≤ F0 + 0.01 near normal incidence); stale / tampered / smuggled IBL
  textures refused; octahedral and atlas layout parity across languages.

### Correction to this contract (N-2)

"Metal sphere reflectance never exceeds F0 × environment" is false at grazing
incidence, where Fresnel rises toward 1 for every material. The tests assert
what holds: no energy creation (A + B ≤ 1 at F0 = 1) and reflectance ≈ F0 near
normal incidence.

### Renderer defects found on the way (fixed at source)

1. **Every packet texture reached the GPU transposed.** The decoder builds a
   (height, width) Julia matrix, which is column-major; the upload copied its
   raw memory into a tightly packed Vulkan image, which reads x-fastest. Square
   textures rendered mirrored across the diagonal (planks and bark furrows at
   90°; normal maps lit across swapped tangent axes); the non-square IBL atlas
   came out as zebra stripes, which is how it was found. Now `permutedims`
   before upload; regression test on a 3 x 2 payload. This changes every
   textured frame, so the byte-identity chain was re-established: frames from
   HEAD + the fix alone (git worktree) vs this change with IBL absent are
   byte-identical for null, `full`, converge0 and converge1 (12/12), and the
   fix-only frames differ from the old captures in all 12 (expected). The
   CALIBRATION-1 review (run2) was made on transposed textures: the bark the
   reviewer flagged had its furrows horizontal.
2. **Surface sampler cache kept the first material's mip range** (the latent
   N-4 finding): now keyed by (mode, max LOD).
3. **`_material_response` stopped compiling on the CPU** once it sampled IBL
   textures (the CPU clearcoat test segfaulted on the missing GPU intrinsic).
   The IBL source is now statically dispatched (`NoIbl` default, `TextureIbl`
   in shaders, `ConstantIbl` for tests), and the real shader arithmetic is
   tested on the CPU. Re-proven after the change: 12/12 parity frames still
   equal fix-only, 9/9 IBL calibration frames reproduce run3.

**Recorded, not changed:** the supervisor's authorized-projection check reads
`WGE_PARITY_RENDER_POLICY` / `WGE_PARITY_CONTENT` from the process
environment, so a leftover export makes an unrelated `render-calibration`
fail authorization (seen once in a test script). Authorization should take
the parity arm as an explicit input, as lowering already does.

---

## 3. CALIBRATION-1 — The material calibration scene

> **Status: IMPLEMENTED (2026-10-04), human acceptance open.** `render-calibration`
> renders the scene through the authorized bound-scene route under four
> enumerated rigs: 52 frames (4 rigs x row + 10 close views, 8 sun grazing
> views). Grey card: sun +2.5%, overcast +0.2% / +0.9% (gate ±5%, met). Null,
> `full`, converge0 and converge1 byte-identical to before (12/12 views).
> Review sheets: `artifacts/calibration/run2/sheets/{sun,overcast,grazing,sun-albedo-grey}.png`
> and `run2/sheets-grazing/sun.png`. As predicted, metal and rough metal read
> near-black and nearly alike: N-2's first input.
>
> **Human review (2026-10-04, sun row and close views):** "These all look really
> good. The dark rusted metal too. Bark could be a tad better ... I could tell
> the difference between wet stone and dry stone." Wet vs dry stone: identified
> (the N-2 blind-identification item, already met on this scene before IBL).
> Open from the review: bark quality; a way to make different VARIETIES of each
> material (content tooling, not this contract).

### Why it was parked from CONVERGE-0

Step 9 assumed a calibration scene was a fixture. Research in CONVERGE-0
(ledger turns 10–11) found three structural dependencies:

1. **The authorized route for imported textures is the C2 bound-scene path**
   (`GLB → RenderAssetPackage → SceneObject → compose_bound_scene_with_camera →
   render_bound_scene_and_promote`). `validate_authorized_world_projection` has
   no place for file-sourced textures in a hand-lowered packet, and should not
   get one.
2. **Light rigs need an authorized seam.** The bound-scene path recomposes from
   the world's base projection plus a Rust-validated camera. Three rigs
   (sun / overcast / grazing) mean three light+environment variants, which the
   supervisor would currently reject as unauthorized.
3. **Channel semantics for imported PBR textures.** `asset_projection` binds a
   glTF `metallicRoughnessTexture` (roughness = G, metallic = B) to the
   roughness slot. CONVERGE-0 fixed the roughness read (shader now reads G,
   byte-identical for all scalar maps); the metallic channel is still ignored.

### Verified inputs (Poly Haven, CC0, 1K, checked 2026-10-03)

| Calibration material | Source set | Notes |
|---|---|---|
| metal | `metal_plate` | has a metal map; metallic factor 1.0 until channel semantics land |
| rough metal | `metal_plate` | SAME albedo, roughness map × 1.0 → remapped to a higher range; isolates roughness |
| stone | `rock_wall_08` | |
| wet stone | `rock_wall_08` | SAME set + wet modifier (albedo × 0.6, saturation +10%, roughness × 0.25, normal scale × 0.4) |
| bark | `bark_willow_02` | on a cylinder as well as the standard bodies |
| wood | `wood_planks_grey` | |
| painted surface | `painted_plaster_wall` | |
| terrain | `forrest_ground_01` | slab only |
| foliage | — | **BLOCKED** on alpha-mask support (audit N-6); placeholder: none |
| emissive | procedural | emissive factor only; no scan needed |

Each set has `diff`, `nor_gl`, `rough`, `ao` at 1K JPG (0.05–1.4 MB each).
Fetch tool must record URL, byte size, and sha256 per file in a manifest; no
file enters the scene without a manifest entry.

### Budget (measured against the 64 MB worker frame, `supervisor.rs:31`)

Albedo + normal at 512², roughness + AO at 256², RGBA8 base64: ≈ 3.3 MB per
material, ≈ 30 MB for the nine textured materials. Fits with headroom; 1K for
all four maps (≈ 213 MB) does not.

### Contract

- **Owner layer:** WGE. **Permitted files:** a new `tools/fetch_calibration_materials.py`
  (download + manifest + resize), a new `tools/build_calibration_glb.py`
  (geometry + glTF PBR materials; arc-length/metric UVs at 1 repeat / 1 m),
  a calibration-rig seam in `scene_composition.rs` (typed `CalibrationRig`
  enum → lights + environment, authorized by enumeration exactly like
  `Campaign2View`), a `render-calibration` subcommand in `main.rs`,
  `tests/calibration.rs`.
- **Geometry (per material, on a plinth row):** 1 m UV sphere, 1 m cube with a
  2–3 cm bevel, 2 × 1 m slab tilted 15° to camera. Plus an 18% grey card,
  white (≈0.85) and black (≈0.03) cards, a chrome ball, a grey diffuse ball,
  a neutral 0.18 ground plane, and a 1 m checker strip.
- **Rigs:** sun 35° elevation / 45° side; overcast (no sun, uniform sky);
  grazing 10° backlight. Fixed exposure; no bloom, vignette, or grade.
- **Cameras:** row-wide (orthographic or 30 mm-eq), per-material close
  (50 mm-eq, sphere ≈ 40% of frame height), grazing (10° over each slab).
- **Debug view:** albedo forced to 0.5 (policy axis `debug.albedo_override_bp`),
  so material identity can be checked with colour removed.
- **Tests:** manifest completeness (every texture has URL + sha256); GLB passes
  `prepare` with zero findings; grey card renders within ±5% of its target in
  the sun rig; each rig is an enumerated, authorized composition.
- **Acceptance (human, mandatory):** the §4 pass conditions in the audit.
- **Non-goals:** foliage (until alpha), IBL (N-2 consumes this scene, it does
  not wait for it — metal and wet are EXPECTED to fail before N-2 and that
  failure is the scene's first useful output).

### Implementation (CALIBRATION-1)

- **Content route.** `tools/calibration_materials/calibration1.json` pins 25 Poly
  Haven CC0 files (URL, bytes, sha256, provider md5 — all 25 matched; the
  `forrest_ground_01` digests equal the N-4 manifest's). `tools/fetch_calibration_materials.py`
  downloads and box-reduces them in numpy (albedo in linear light, normals
  renormalised), deterministically. `tools/build_calibration_glb.py` refuses any
  set whose digests do not match, and writes one deterministic GLB (8.1 MB,
  32 meshes, 16 materials, 29 images, 13.4k vertices) to the ignored tree.
- **Geometry.** Plinth row of nine columns (metal, rough metal, stone, wet
  stone, bark + cylinder, wood, painted, terrain slab, emissive sphere), each a
  1 m UV sphere, a 1 m cube with 2.5 cm chamfer and a 2 x 1 m slab leaning 15°;
  a calibration group (0.03 / 0.18 / 0.85 cards, chrome and 0.18 balls, 1 m
  checker strip of 5 cm checks); a 34 x 7 m 0.18 ground plane. UVs are metric
  divided by each scan's measured size. Rough metal remaps roughness to
  0.45 + 0.55 r; wet stone is albedo x 0.6 linear +10% saturation, normals at
  0.4 slope, roughness factor 0.25.
- **Gates (met).** `prepare` and render conditioning: ready, ZERO findings, zero
  tangent fallbacks, every scan mipped (`tests/calibration.rs`, run with
  `--include-ignored` after fetching). Packet 34.3 MB of the 64 MB worker frame.
- **Rig seam.** `CalibrationRig { Sun, Overcast, Grazing, SunAlbedoGrey }`
  (`src/calibration.rs`) sets lights, environment and render policy from
  constants; `compose_bound_scene_with_view` applies it and
  `BoundSceneRenderAuthorization.rig` makes the supervisor re-apply it and
  require an exact match, exactly like the camera. `None` is byte-identical to
  the camera-only composition (`tests/scene_composition.rs`). No bloom,
  vignette, grade, dither or fog; 22 m view-fitted shadows (~4 cm texels; the
  40 m converge0 fit drew a 0.5 m ball's shadow as a blob).
- **Debug view.** `render_policy.debug { albedo_override_bp }` (Rust, Julia,
  shaders); absent = authored albedo, byte-identical.
- **Host placement.** riverwatch, anchor (−16, −26.5), yawed 180° so cameras
  look into the world: the 34 x 7 m footprint is clear of all 43 instances
  (searched, not guessed) with 0.33 m of terrain relief; the ground plane sits
  3 cm above the highest terrain sample under it. The row view had to move up
  and in (and to 2.4:1) because from 21 m back the camera stood outside the
  world and half the frame was void.
- **Grey card.** Target = middle grey through the renderer's tone map:
  f(0.18) = 0.2669 → sRGB8 141.1. Fixed exposure per lighting setup, set on the
  card as a calibration shoot would: sun 1.0 (measured 144.7, +2.5%); overcast
  2.57 (1.0 measured sRGB 77 = 0.070 linear; now 141.3 / 142.3, +0.2% / +0.9%);
  grazing keeps the sun's 1.0 (a backlit card should read dark: 58–70).
  `render-calibration` exits non-zero if a sun or overcast view composed to
  frame the card measures outside ±5%.

### Defects found and fixed on the way (all byte-identical for existing content)

1. **Imported PBR materials could not render.** `project_render_asset` put every
   referenced map into `texture_ids`, which the adapter samples as the single
   albedo slot ("native path supports one albedo texture per material"). The
   C2.5 log-hut had base-colour maps only, so no imported asset with a full PBR
   set had ever reached the renderer. Now: base colour only; the validator
   requires every named slot to resolve (`tests/asset_projection.rs`).
2. **A vertical sun failed shadow-basis construction.** `_shadow_up_hint` tested
   z instead of y. Now switches above |y| = 0.99; every existing light
   (steepest |y| = 0.952) keeps its basis.
3. **Alpha 1.0000001 at extreme grazing.** Filtering an opaque texture returned
   one ulp over 1 and the capture check refused the frame; alpha is clamped
   where it is produced. The capture error now names the pixel and value.

### Findings recorded, not changed

- Render conditioning drops glTF `normalTexture.scale` and
  `occlusionTexture.strength` (the projection hardcodes 1.0), the same class of
  channel-semantics gap as the ignored metallic channel; wet stone's normal
  scale is baked into its own map instead.

---

## 4. N-4 — Terrain surface content (from spiral EDGE-1)

> **Status: IMPLEMENTED (spiral N4-1, 2026-10-03).** Acceptance (measured) met:
> converge0 close/medium/wide = **761 / 658 / 369 bp** against the unchanged
> 220 bp floor (all `good`; previously 295 / 148 / 123). Null and `full` render
> byte-identical to before. Human acceptance items (twig scale, no visible
> tile repetition) are open for review: `artifacts/parity/review-converge0-n4c/`.
> Implementation summary at the end of this section.

### Evidence

EDGE-1 investigated the CONVERGE-0 open item: the Campaign 2 edge-density
authority (`minimum_edge_pair_fraction_bp: 220`) flagged converge0 medium/wide.
A Python replica of the authority metric reproduced its numbers exactly, then
scored deliberately broken renders of the same scene (throwaway probe, raw
`render_packet` requests, never promoted):

| Arm | close | medium | wide |
|---|---|---|---|
| C3 fog ×40 (broken) | 24 | 0 | 0 |
| C2 sun off (broken) | 174 | 70 | 52 |
| C1 all textures stripped (broken) | 216 | 133 | 125 |
| converge0 | 295 | 148 | 123 |
| `full` (retained) | 529 | 313 | 355 |
| X1 scanned CC0 ground, no mips | 4209 | 5052 | 5103 |
| X2 scanned CC0 ground, mipped | 2366 | 2843 | 2239 |

Findings:

1. **The floor is not recalibrated.** Pre-registered rule: a new floor must be
   1.5 × the highest broken-render score. converge0 sits AT control level, so
   no defensible floor passes it.
2. **The gate detects "no pixel-scale detail" — and that diagnosis of converge0
   is correct.** Its textures carry almost no information at frame scale
   (stripping them costs ~80 bp close, ~0 wide).
3. **The gate cannot tell detail from defects.** ~70% of `full`'s edge lead came
   from the parametric ring smear on one mesh (MD-4), and unmipped scanned
   ground scores 5000 bp from aliasing. It is a necessary floor, never a
   quality signal.
4. **Real content clears it by ~10×** (X2), and reads as ground in the frame
   (`artifacts/parity/review-x2-scanned-ground/`).
5. **Bug fixed en route:** `_texture_levels` computed mip sizes as UInt64
   (`UInt32 ÷ Int`), so every packet texture carrying a mip chain crashed the
   worker. No test had pushed a real chain through it; one does now.
6. **Contract limitation:** `terrain_surface.uv_repeat_scale_milli` is repeats
   across the whole extent, capped at 32. On the 480 m converge0 world the finest
   tiling is 15 m per repeat; a scanned ground set represents ~2–4 m, so X2's
   twigs render ~4× too large.

### Contract

- **Texel scale in metres:** add `metres_per_repeat_milli` to the terrain layer
  (per layer, not per policy), so tiling is a property of the texture's physical
  size and independent of world extent. Keep `uv_repeat_scale_milli` for
  existing packets.
- **Layers:** 2–4 scanned CC0 sets (ground litter, grass, dirt, rock) blended by
  slope and height, plus a low-frequency macro variation term (the policy already
  reserves `macro_variation_bp`; implement it rather than refusing it).
- **Assets via the asset route:** scanned sets enter through a manifest
  (URL, size, sha256, licence) and the asset pipeline. Not through campaign
  lowering: a lowering that reads files breaks "packet = pure function of world".
- **Mips are mandatory** for every scanned texture (X1 shows why).
- **Tests:** manifest completeness; every terrain texture mipped; repeat
  scale in metres within ±1% of the declared physical size; existing packets
  byte-identical.
- **Acceptance (measured):** converge0 medium/wide clear the existing 220 bp
  floor with no threshold change.
- **Acceptance (human, mandatory):** ground reads at the right scale (twig
  test) and shows no visible tile repetition in the wide view.
- **Non-goals:** virtual texturing, runtime splat painting, displacement.


### Implementation (N4-1)

- **Schema:** `TerrainPacket.layers: Option<TerrainLayers>` (absent = byte-identical);
  2–3 layers, per-layer `metres_per_repeat_milli` from the scan's measured size,
  coverage = product of optional slope / height / macro ramps (a falling ramp is
  `[a, b]` with `a > b`). Validation: mips mandatory, neutral layer-material factors
  (the scans carry colour and roughness), CC0 only, ramps of nonzero width.
- **Content route:** `tools/terrain_layers/converge0.json` pins 12 Poly Haven CC0
  files (URL, byte size, sha256; md5 cross-checked against the provider's API).
  `tools/fetch_terrain_layers.py` fetches them into the ignored artifacts tree.
  `load_terrain_layer_set` re-verifies every digest, decodes, and builds
  deterministic box-filtered mip chains (albedo averaged in linear light,
  normals renormalised). The set is an explicit input to lowering and to
  `GraphicsWorkerSupervisor::render_and_promote_with_terrain_layers`, which
  re-derives the authorized packet with the same set — no lowering reads files.
- **Renderer:** a separate layered terrain pipeline (the single-material terrain
  and every mesh keep their shaders): 14 bindings, world-metric UVs, slope /
  height / macro coverage, `macro_variation_bp` executed (refused on unlayered
  terrain), and far-field resampling (albedo blended toward a 7.13× larger
  scale over 25–90 m) because a 2–3 m tile repeats as a visible grid at 200 m.
- **Tuning was measured, not guessed:** a first pass with guessed thresholds put
  rock on 0.5% of the terrain (slope p90 is only ~0.05) and dirt on nearly all
  of it (84% of the ground lies below the height cut). Thresholds now come from
  the measured slope and macro-noise distributions.
- **Defects found on the way:** packet mip chains crashed the worker (EDGE-1);
  `_surface_sampler!` caches one sampler per spec with the first material's max
  LOD, which would have stripped the layers' mips (the layered path builds its
  own sampler; the latent cache issue is recorded, not changed); a converge0
  render took 15 min in the debug binary (authorization now re-lowers only the
  named view; `WGE_PARITY_BIN` selects a release binary, proven byte-identical
  on the null arm: 190 s).

### Residuals (not blocking, for review)

- A faint diagonal pattern remains on the far-left ridge in the wide view.
- The meadow reads olive/tan at distance; the base scan is litter-heavy. A
  greener base set is a content choice, not machinery.
- Tree crowns and props are untouched placeholders (CONVERGE-2).

---

## CONVERGE-1 closeout (2026-10-04)

Commits: `d217e33` N-1 · `5be9da5` CALIBRATION-1 · `86b279a` texture-transpose fix ·
N-2 (next commit).

| Contract | Human acceptance | Measured acceptance |
|---|---|---|
| N-1 sky + atmosphere | Met: "it's reading as distance" ("foggy zone, not milky"); 45 bp preferred over 70 | Sun-side horizon 2.72x (≥ 1.15x) met; ridge/foreground contrast 0.65 (≤ 0.40) NOT met — the 480 m world is too small for the target without milking the near field |
| CALIBRATION-1 | Met: materials distinct; wet vs dry identified | Zero findings; grey card ±5% met (all calibrated rigs); 34 MB packet |
| N-2 IBL | Met: metal reads as metal; blind wet/dry correct; chrome reads as polished metal | Chrome sky/ground met; metal chromaticity 0.0102 (≤ 0.01) missed by 0.0002; wet/dry luminance 46% (≤ 25%) and lobe 1.80x (≥ 2x) NOT met — the luminance target contradicts §3's wet recipe |

Identity discipline held throughout: every new axis is absent-by-default and
byte-identical when absent. The one deliberate break is the transpose fix,
whose renders are the new references (`artifacts/parity/ab-*-tfix`), and N-2
with IBL absent reproduces them exactly (12/12).

**Defects found and fixed in CONVERGE-1** (beyond the contracts): textures
uploaded transposed (since the first textured packet); imported PBR materials
could not render at all (`texture_ids`); vertical sun failed shadow basis;
alpha overshoot at grazing; sampler cache kept the first material's mip range;
`_material_response` lost CPU testability; serde accepted stray fields on unit
variants.

**Carried into CONVERGE-2** (from reviews and findings):

1. Material varieties: several scans per family plus per-instance tint, scale,
   rotation and wear / wetness blends ("there needs to be a way to make
   different varieties of all of these").
2. The hero prop ("the odd fire-hydrant shrine") and the tree balls: imported
   kit + alpha foliage (audit N-6), as planned.
3. Bark depth: parallax / displacement for deep relief; recheck bark now that
   textures upload the right way round.
4. Channel semantics in render conditioning: metallic (B), `normalTexture.scale`,
   `occlusionTexture.strength`.
5. Authorization should take the parity arm as an explicit input, not read it
   from the process environment.
6. A larger world (km-scale backdrop) for aerial perspective that reads as
   distance without fogging the near field; and restate N-2's wet/dry target
   against the wet recipe it measures.

