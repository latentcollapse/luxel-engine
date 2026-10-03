# WGE CONVERGE-1 Contracts

Status: **closed recipes, not implemented**, 2026-10-03
Source: `WGE_GRAPHICS_CONVERGENCE_AUDIT.md` §H step 10, revised by the CONVERGE-0 run
(`artifacts/parity/CONVERGE0_SPIRAL_LEDGER.md`).

Three contracts. Each names the owner layer, the permitted files, the tests, the
acceptance evidence, and the non-goals. None of them may start until the
CONVERGE-0 human review has answered "what does your eye hit first?"

---

## 1. N-1 — Sky and atmosphere (remainder)

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

---

## 2. N-2 — Prefiltered image-based lighting

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

---

## 3. CALIBRATION-1 — The material calibration scene

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
