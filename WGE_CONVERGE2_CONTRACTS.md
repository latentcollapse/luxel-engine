# WGE CONVERGE-2 Contracts

Status: **CONVERGE-2 CLOSED, 2026-10-04.** N-6 and N-5 implemented and accepted; closeout at the end. Order N-6 → N-5 approved; landmark: ruin from
`modular_fort_01` (option 1).
Source: `WGE_GRAPHICS_CONVERGENCE_AUDIT.md` §8 (N-5, N-6) and the "Carried into
CONVERGE-2" list in `WGE_CONVERGE1_CONTRACTS.md`.

Two contracts: the renderer learns alpha-tested foliage (N-6), then the
procedural shrine and tree balls are replaced by an imported kit (N-5). Each
names the owner layer, the permitted files, the tests, the acceptance evidence,
and the non-goals. Same identity discipline as CONVERGE-1: every new axis is
absent by default and byte-identical when absent.

Proposed order: **N-6 → N-5.** N-6 has no dependencies and the tree in N-5
cannot render without it.

---

## 0. What the code and the sources say (checked 2026-10-04)

**Renderer.** `alphaMode` is parsed in `asset_contract/src/render.rs:984`,
projected through `asset_projection.rs:274` and accepted by the packet decoder
(`WGEGraphics.jl:1088`). Then `LavaAdapter.jl` `_validate_material` refuses
every non-opaque material. `alphaCutoff` is not parsed anywhere.
`doubleSided` is parsed into `RenderMaterial` but not carried into the packet.
Every raster pipeline is already `NoCull()`, so back faces draw, but they are
lit with the front-face normal.

**Scene.** The "fire-hydrant shrine" is `campaign2-hero-*` (core, fins, halo,
inlays, pedestal, rune ring, spire) and the tree balls are
`campaign2-crown-*` / `campaign2-lobe-*` on `campaign2-trunk-*`, all
procedural meshes in `native_graphics_contract/src/lib.rs`.

**Sources (Poly Haven, CC0, glTF 1K, measured from the published files):**

| Asset | Triangles | Geometry | Textures (1K) | Materials |
|---|---|---|---|---|
| `fern_02` | 6 k | 0.2 MB | 1.0 MB | `MASK` 0.5, double-sided |
| `shrub_02` | 27 k | 0.8 MB | 1.2 MB | `MASK` 0.5, double-sided |
| `grass_medium_01` | 25 k | 1.0 MB | 2.0 MB | **`BLEND`** |
| `rock_moss_set_01` | 63 k | 1.5 MB | 0.5 MB | opaque |
| `modular_fort_01` | 28 k | 1.3 MB | 9.3 MB | opaque ×3 |
| `jacaranda_tree` | **3.86 M** | **208 MB** | 6.3 MB | leaves **`BLEND`** |
| `tree_small_02` | **2.06 M** | **95 MB** | 5.9 MB | leaves **`BLEND`** |

Two consequences:

1. **Every Poly Haven tree is 30–80× over budget.** Meshes travel as JSON
   float arrays (`MeshPacket`: positions, normals, uv0, tangents, indices)
   inside the 64 MB worker frame (`supervisor.rs:32`). At roughly 100+ bytes
   per vertex as text, the whole scene has room for a few hundred thousand
   vertices. A tree must arrive at ≤ 60 k triangles. Blender is installed
   (`/usr/bin/blender`), so decimation runs headless in the build tool.
   Decimating leaf cards destroys them; leaves need a different reduction
   (see N-5).
2. **Tree and grass leaves are authored as `BLEND`.** WGE will not render
   blended foliage (sorting, no shadows). The build tool converts them to
   `MASK` with an explicit cutoff and records the conversion in the manifest.

**No landmark structure exists on Poly Haven.** The nearest candidates are
`modular_fort_01` (a fort wall kit), `gothic_statue`, `stone_fire_pit` and
`large_castle_door`. (`fire_hydrant` exists too.) The landmark is a content
decision; see N-5.

---

## 1. N-6 — Alpha-mask foliage

> **Status: IMPLEMENTED (2026-10-04), human acceptance met.** Mask and
> double-sided materials render through a cutout pipeline with a vendored Lava
> `discard()`; alpha-tested shadows; coverage-preserving alpha mips. Foliage
> row: set `calibration2` (fern, shrub, grass), views `foliage-{5,10,40}m`.
> Identity: 116/116 frames byte-identical to the references (calibration1
> `run3` 88 + `run3-grazing` 16; parity `ab-{null,full,converge0,converge1}-tfix`
> 12), rendered with the vendored Lava. Measured silhouette at 10 m / 40 m: 0.83 / 0.54
> of the analytic prediction (opaque-card geometry alone: 0.87 / 0.70); the
> ±15% gate is not met at 40 m and is corrected below. Sheets:
> `artifacts/calibration/n6-foliage/sheets/`.
>
> **Human acceptance (2026-10-04, sun / sun-ibl sheet at 5 m and 10 m): met.**
> "These actually look really good. The one on the left is clearly a jungle
> fern. The middle one is obviously some kinda underwater plant, and the one on
> the right looks like a simple river reed." Each specimen read as a distinct
> plant. The 40 m stipple (measured shortfall below) was not raised.

### Contract

- **Packet:** `MaterialPacket` gains `alpha_cutoff: Option<f32>` and
  `double_sided: Option<bool>`, both omitted from serialization when `None`
  (packet digests of existing content unchanged). Validation: `alpha_cutoff`
  is required for `Mask` and refused otherwise; range (0, 1).
  `asset_contract` parses `alphaCutoff` (glTF default 0.5) and carries
  `doubleSided` through `asset_projection.rs`.
- **Renderer:** `_validate_material` accepts `:mask`; `:blend` stays refused
  with a message naming the build-tool conversion. Main pass: base-colour
  alpha (factor × texture) below cutoff → `discard`. Shadow pass: the same
  test, so leaf holes appear in shadows (the mesh shadow pipeline needs the
  uv0 and base-colour texture it does not bind today). Double-sided lighting:
  flip the shading normal on back faces (`gl_FrontFacing` equivalent in Lava).
- **Mip coverage:** alpha-tested textures thin out at distance because box
  filtering averages coverage away. For `Mask` materials, the mip chain
  rescales each level's alpha so the fraction above the cutoff matches mip 0
  (coverage-preserving mips). Applies only to textures bound as base colour
  of a `Mask` material.
- **Content:** add a foliage row to CALIBRATION-1 (the row it recorded as
  BLOCKED): `fern_02`, `shrub_02`, and `grass_medium_01` converted to `MASK`,
  fetched with the existing manifest discipline (URL, size, sha256).
- **Permitted files:** `asset_contract/src/render.rs`, `asset_projection.rs`,
  `lib.rs` (packet + validation), `material_maps.rs` (mip coverage),
  `WGEGraphics.jl`, `LavaAdapter.jl`, `calibration.rs`,
  `tools/fetch_calibration_materials.py`, `tools/build_calibration_glb.py`,
  tests.
- **Tests:** packet round-trip with and without the new fields (absent =
  byte-identical serialization); `Mask` without cutoff refused; `Blend`
  refused by the adapter; coverage-preserving mips keep above-cutoff fraction
  within ±2% of mip 0 at every level; null / `full` / converge0 / converge1 /
  calibration renders byte-identical.
- **Acceptance (measured):** foliage row at three camera distances (close,
  10 m, 40 m): above-cutoff coverage in the rendered silhouette within ±15% of
  the close view's (no thinning to sticks); shadow of the shrub has interior
  holes (shadow-pixel fraction inside its silhouette hull < 0.85).
- **Acceptance (human):** "these read as plants"; no visible alpha fringe or
  shimmer at the three distances.
- **Non-goals:** alpha blending, alpha-to-coverage / MSAA, wind, subsurface
  or translucency on leaves, grass scatter (N-7).

### Implementation (N-6)

- **Lava has no discard.** `graphics_lab/vendor/Lava` is upstream `11c7e31`
  plus `lava-0001-fragment-discard.patch`: `Lava.discard()` emits SPIR-V
  `OpKill` through the same block-terminator path as
  `OpIgnoreIntersectionKHR`. Proven on the RTX 5060 before landing (a
  discarded near quad wrote neither colour nor depth). The patch only adds
  code; `LAVA_REVISION` stays the base commit (`vendor/README.md`).
- **Packet:** `MaterialIntent.alpha_cutoff` (required for `Mask`, refused
  otherwise) and `double_sided` (`Some(true)` or absent; `Some(false)` is
  refused so single-sided has one encoding). `RenderMaterial.alpha_cutoff`
  carries glTF `alphaCutoff` (default 0.5). All omitted when absent: the
  committed-packet reseal test still passes.
- **Renderer:** a material is "cutout" when it is `Mask` or double-sided.
  Cutout batches use `mesh_cutout_pipeline`: the shared shading body
  (`_surface_fragment_color`, factored out of `_terrain_fragment`), with the
  normal and the whole tangent frame negated on back faces of double-sided
  materials, and `discard` when alpha < cutoff, placed after every texture
  sample so implicit-LOD derivatives stay defined. `Mask` batches cast shadows
  through `mesh_cutout_shadow_pipeline`, which binds only the albedo (the
  shadow map cannot be sampled while it is the render target). Opaque
  single-sided batches keep the historical pipelines. `BLEND` is still
  refused, now with a message naming the conversion.
- **Coverage-preserving mips** (`asset_contract` texture conditioning): base
  colour textures of `MASK` materials get each level's alpha scaled
  (bisection) so the fraction passing the cutoff matches the base level
  (Castaño 2010). Unit tests: within ±2% down to 8 px on a leaf-like mask,
  while plain box mips drift past 2% (the test proves it can fail). A texture
  shared by two cutoffs is a finding.
- **Content:** `tools/fetch_models.py` pins every file of a Poly Haven model
  (URL, bytes, sha256, provider md5); `tools/models/foliage1.json`.
  `tools/gltf_model.py` bakes one root node's TRS into its triangles.
  `tools/build_calibration_glb.py` builds `calibration2` = calibration1 +
  `fern_02_b`, `shrub_02_a`, `grass_medium_01_tall_a_LOD0` in front of the
  plinth at z = 3 m. The glTF base colour is a JPEG without alpha; the
  separate `Alpha` map is merged into an RGBA base colour; `grass_medium_01`
  is `BLEND` and is conditioned to `MASK` 0.5. Without a `foliage` section the
  builder still writes calibration1 byte for byte (verified).
- **Views:** `CalibrationView::Foliage(d)`, d = 5, 10, 40 m, one ray and one
  45° field of view from a constant point, so any calibration set renders
  them (calibration1 is the no-foliage baseline). `--views all` includes them
  only for a GLB that carries foliage.

### Measurement (`tools/foliage_measure.py`)

Overcast rig (no cast shadows), calibration2 vs calibration1 from the same
cameras: the differing pixels are the silhouette. The raw 1/d² ratio reads
1.09 / 1.27 at 10 / 40 m, i.e. *larger* than expected, because partly covered
edge pixels count whole. The tool instead predicts the far silhouette by
box-filtering the close mask by d/5 and compares like with like:

| Arm (overcast) | 10 m actual / predicted | 40 m actual / predicted |
|---|---|---|
| Alpha-tested, coverage-preserving mips (shipped) | 0.83 | 0.54 |
| Control: alpha-tested, plain box mips | 0.83 | 0.48 |
| Control: same cards rendered opaque (geometry only) | 0.87 | 0.70 |

Reading: point sampling (2x supersample, 4 samples per pixel) drops leaves
narrower than half a pixel. At 40 m the pixel footprint is ~5 cm and a leaf is
~2 cm, so geometry alone loses 30%. The alpha test loses a further 16 points;
coverage-preserving mips win back 6 of them. The rest is the global rescale's
known limit: one scale per level saturates thick regions and still loses thin
ones.

### Correction to this contract (N-6)

- The ±15% silhouette gate compared a point-sampled raster against analytic
  coverage. Pure geometry fails it at 10 m (0.87). Restated: alpha-tested
  coverage relative to the opaque-card control. Measured 0.96 at 10 m (met)
  and 0.77 at 40 m (NOT met). Closing the 40 m gap needs anti-aliased coverage
  (alpha-to-coverage with MSAA, or TAA: audit L-2) or distance LOD cards, not
  more mip work.
  **Decision (2026-10-04): deferred to L-2.** The 40 m shortfall is a known
  limit of the current rasterizer; `tools/foliage_measure.py` is the
  measurement L-2 must move.
- The shadow-hole criterion is visual, not numeric: in a frame diff the
  shadow cannot be separated from the silhouette. Evidence:
  `sheets/foliage_sun_ibl_5m_crop.png`. The shrub and fern cast dappled,
  leaf-shaped shadows, not card rectangles; soft at the 512² shadow map.

---

## 2. N-5 — Hero asset kit, imported

> **Status: IMPLEMENTED (2026-10-04), human acceptance met.** Arm `converge2`
> (`WGE_PARITY_RENDER_POLICY=converge2`, `WGE_KIT_SET=tools/kit/kit1.lock.json`):
> converge1's policy unchanged, the procedural shrine and tree balls replaced
> by the kit. Packet 52.6 MB (converge1 37.6 MB). Identity: 122/122 frames byte-identical (calibration1 `run3` 88 + `run3-grazing` 16;
> parity `ab-{null,full,converge0,converge1}-tfix` 12; foliage `n6-foliage` 6), with
> the channel-semantics shader and the 128 MiB frame bound. Rust 399 passed / 0 failed;
> Julia protocol, render-policy and adapter suites all pass.
> Sheets: `artifacts/parity/review-converge2/{close,medium,wide}-sheet.png`.
>
> **Human acceptance (2026-10-04, close / wide sheets): met.** "That is night
> and day ... It genuinely reminds me almost of something I'd see in RIFT ...
> It's giving Freemarch. Which is miles better than the fire hydrant shrine
> thing." The eye no longer goes to the shrine or the tree balls. The pool
> effect was checked and does not clip. The brickwork read as "early Ottoman".

### Contract

- **Build tool** (`tools/build_kit.py`, driving `blender -b --python`):
  fetch with manifest (URL, size, sha256 per file); decimate opaque meshes to a
  per-asset triangle budget; convert `BLEND` leaves to `MASK` with an explicit
  cutoff; resize textures to the frame budget; write one GLB per asset that
  passes `prepare` with zero findings. Deterministic: same inputs, same GLB
  bytes (Blender version recorded in the manifest).
- **Trees:** decimate trunk and branches as ordinary meshes. Leaves are not
  decimated: the tool keeps a seeded subset of leaf cards (or rebuilds them as
  larger cluster cards textured from the same leaf atlas) until the tree fits
  ≤ 60 k triangles. Silhouette check: the decimated tree's alpha-tested
  silhouette from 3 views covers ≥ 85% of the source's.
- **Kit contents:** one landmark (a `modular_fort_01` ruin, below), two rock types
  (`rock_moss_set_01`, `namaqualand_boulder_0x` or `boulder_01` decimated),
  one tree species, one ground cover (`fern_02` or `shrub_02` from N-6).
- **How the kit enters the world:** as an explicit, digest-verified input to
  lowering, the same seam N-4 used for terrain layers
  (`render_and_promote_with_terrain_layers`): the supervisor re-derives the
  authorized packet with the same kit, no lowering reads files. The
  `campaign2-hero-*` and `campaign2-crown-*` / `lobe` / `trunk` instances are
  replaced only under a new policy arm (`converge2`); every existing arm stays
  byte-identical.
- **Folded in from the carried list:**
  - Channel semantics (carried item 4): metallic from the B channel of
    `metallicRoughnessTexture`, `normalTexture.scale`,
    `occlusionTexture.strength`. Needed for "real metal on the landmark".
  - Parity arm as an explicit input to authorization (carried item 5): the
    new arm is the first one added under this rule, so it lands here instead of
    adding one more environment read.
- **Budget:** measured, not estimated, as the contract's first step: bytes per
  vertex in the serialized frame, then per-asset triangle budgets so the
  converge2 frame stays ≤ 48 MB (16 MB headroom).
- **Permitted files:** `tools/build_kit.py`, `tools/kit/*.json` (manifests),
  `asset_contract/src/render.rs` and `asset_projection.rs` (channel
  semantics), a new `kit.rs` in `native_graphics_contract`, `lib.rs`,
  `supervisor.rs`, `render_policy.rs`, `main.rs`, `LavaAdapter.jl`
  (channel semantics only), `tools/parity_ab_policy.sh`, tests.
- **Tests:** manifest completeness; each kit GLB passes `prepare` with zero
  findings; build determinism (two runs, identical GLB digests); kit digest
  mismatch refused by the supervisor; converge2 frame ≤ 48 MB; all prior arms
  byte-identical; channel-semantics unit tests (metallic B read, normal scale,
  occlusion strength) with absent values byte-identical.
- **Acceptance (measured):** the `campaign2-hero-*` and crown/lobe primitives
  are absent from the converge2 packet; landmark metal measured on the
  calibration rig reads as metal (chromaticity carries the albedo tint).
- **Acceptance (human, mandatory):** the CONVERGE-0 question again on the
  close / medium / wide sheets: "what does your eye hit first?" The answer
  must no longer be the shrine or the tree balls.
- **Non-goals:** scatter and forest density (N-7), per-instance variety
  (below), LODs, wind, interiors.

### Implementation (N-5)

- **Sources** (`tools/models/kit1.json`, pinned by `tools/fetch_models.py`):
  `modular_fort_01`, `rock_moss_set_01`, `rock_moss_set_02`, `tree_small_02`
  (+ `leaves_alpha`), `fern_02` (+ `Alpha`).
- **Build** (`tools/build_kit.py`, 46 s, deterministic: a rebuild matches the
  committed `tools/kit/kit1.lock.json` byte for byte; Blender 5.2.2 LTS):

  | Asset | Content | Triangles | GLB |
  |---|---|---|---|
  | ruin | fort gate + broken wall end 2 m clear, 7° off true | 1,368 + 442 (authored) | 1.11 MB |
  | rock_a / rock_b | one rock from each mossy set, Blender collapse | 10,996 → 2,500 / 8,000 → 2,499 | 0.27 / 0.30 MB |
  | tree | `tree_small_02`: trunk 28,293 → 3,000, branches 94,814 → 6,000, 1.94 M leaf triangles → 370 cluster cards (185 clusters, 24 px tiles, one 512² atlas) | ~9.7 k | 1.11 MB |
  | fern | `fern_02_b` as authored | 2,384 | 0.20 MB |

  Branch UVs come from `TEXCOORD_1` with a `KHR_texture_transform`; WGE
  carries one UV set, so `tools/gltf_model.py` bakes the transform into UV0.
  Collapse left 36 zero normals on degenerate branch triangles; the builder
  drops zero-area triangles and gives any other zero normal its face normal.
- **Leaf cards** (`tools/leaf_cards.py`): leaves grouped by a uniform grid
  (cell grows until <= 220 clusters), two crossed cards per cluster through its
  area-weighted centre, each a tile rasterised in numpy from the real leaves
  (UV-mapped albedo and alpha, front-most fragment, 2x2 supersampled coverage),
  darkened toward the crown centre (AO floor 0.65), normals leaning outward
  from the crown so the canopy shades as a volume.
- **Seam** (`kit.rs`): `load_kit_set` checks every GLB's size and digest
  against the lock, conditions it with the calibration render request (any
  finding refused) and projects it. `apply_kit` drops the `campaign2-hero-*`,
  `-trunk-`, `-crown-`, `-lobe-` instances, prunes the meshes, materials and
  textures only they used, and places the kit relative to the shrine anchor:
  ruin (scale 0.8, yaw 126°, arch toward all three cameras), trees at the
  seven authored tree spots and scales, five rocks (sunk 18 cm x scale), twelve
  ferns. Placement constants live in `kit.rs`; a packet is a pure function of
  (world, view, arm, terrain layers, kit).
- **Carried item 5 closed for this route:** `Campaign2Inputs` (arm, content,
  terrain layers, kit) is passed explicitly to lowering and to
  `GraphicsWorkerSupervisor::render_and_promote_campaign2`, which re-derives
  the authorized packet from exactly those inputs. `render-campaign2-layout`
  reads the environment once and never inside authorization. (The older
  `render_and_promote_with_terrain_layers` remains as an env-reading wrapper.)
- **Channel semantics (carried item 4):** `RenderMaterial` carries glTF
  `normalTexture.scale` and `occlusionTexture.strength` when set (findings
  outside [0, 2] / [0, 1]); they reach the existing packet fields. A material
  with a glTF metallicRoughness texture gets `metallic_from_texture`, and the
  shader multiplies metallic by that texture's BLUE channel (flag passed as
  emissive w = 2). Absent, the select returns the factor exactly: every
  existing frame is byte-identical, calibration included.
- **Frame bound** raised from 64 MiB to 128 MiB on both ends
  (`supervisor.rs`, `wge_graphics_worker.jl`); converge2 measures 52.6 MB.
- **Tests:** `tests/kit.rs` (lock completeness; converge2 needs a kit and no
  other arm accepts one; with the built kit: primitives gone, pool kept, kit
  instances present, policy and camera equal to converge1; kit adds <= 24 MB
  (11.7 MB on the synthetic layer set); a one-bit-tampered GLB is refused),
  channel-semantics tests in `alpha_mask.rs` and `asset_contract`.

### Corrections to this contract (N-5)

- **Budget:** "converge2 frame <= 48 MB" was a guess. Measured, the converge
  packets already carry 31.5 MB of terrain-layer textures and meshes cost
  ~96 bytes per vertex as JSON. Restated: the kit adds <= 24 MB, the frame
  bound is 128 MiB; measured 52.6 MB.
- **Tree silhouette:** ">= 85% of the source's silhouette from 3 views"
  measured pixel-exact leaf placement, which no card representation can
  match off-axis (a card flattens leaves up to +/-0.25 m off its plane;
  coverage stayed 0.81-0.88 from 254 to 878 cards). Restated at canopy scale
  (10 cm cells): outline IoU >= 0.85 and leaf mass within +/-20%. Measured
  front / side / diagonal: IoU 0.91 / 0.90 / 0.86, mass 1.12 / 1.18 / 1.12
  (pixel-exact 0.88 / 0.84 / 0.81, reported).
- **"Real metal on the landmark":** the chosen landmark is stone and plaster;
  it has no metal. The metal channel is verified by tests and by calibration
  (metal_plate's B = 255 gives the factor exactly).
- **`prepare` with zero findings:** the kit is checked by render conditioning
  (zero findings required at load), not by `prepare_asset`, which needs a
  per-asset runtime request (collision, LODs) the kit does not author yet.

### Landmark: decided 2026-10-04 — option 1, ruin from `modular_fort_01`

Poly Haven has no shrine. Options considered, roughly by effort:

1. **Ruin from `modular_fort_01`** — a broken fort-wall section as the
   landmark. Real stone, plaster and trim; reads as "old structure in a
   field". Cheapest.
2. **Composed shrine** — `gothic_statue` on a plinth built from fort pieces,
   with `stone_fire_pit` and rocks around it. Keeps the shrine idea; more
   composition work.
3. **External CC0 source** (ambientCG has no models; Sketchfab CC0 /
   Quixel-free terms vary) — better fit possible, licensing check required
   per asset.
4. **A model you supply.**

---

## Deferred to CONVERGE-3 (carried items not taken here)

| Carried item | Why not now | Lands with |
|---|---|---|
| 1. Material varieties (several scans per family, per-instance tint / scale / rotation / wear / wetness) | Variety pays off when there are many instances; N-5 places a handful | N-7 scatter + forest |
| 3. Bark depth (parallax / displacement) | Recheck after the imported tree's bark is on screen; may be unnecessary at kit distances | Review after N-5 |
| 6. Km-scale backdrop; restate N-2's wet/dry target | Independent of the kit; its own contract | N-3 backdrop |

---

## CONVERGE-2 closeout (2026-10-04)

Commits: `7164966` N-6 · N-5 (next commit).

| Contract | Human acceptance | Measured acceptance |
|---|---|---|
| N-6 alpha-mask foliage | Met: "clearly a jungle fern ... some kinda underwater plant ... a simple river reed" | Identity 116/116 met; mip coverage ±2% (unit) met; silhouette vs opaque-card geometry 0.96 at 10 m met, 0.77 at 40 m NOT met (deferred to L-2); shadows visual, met |
| N-5 hero kit | Met: "night and day ... It's giving Freemarch"; the eye no longer hits the shrine or tree balls | Primitives absent met; kit <= 24 MB met (adds 15 MB; frame 52.6 MB); kit build deterministic (lock) met; canopy outline IoU >= 0.85 met (0.86-0.91), mass ±20% met (+12-18%); identity 122/122 met |

Identity discipline held: every new axis is absent by default and
byte-identical when absent (alpha cutoff, double-sided, metallic-from-texture,
kit, foliage views). New references: `artifacts/parity/ab-converge2-n5`
(= `ab-converge2`), foliage `artifacts/calibration/n6-foliage` (= `n5-foliage`).

**Infrastructure landed beyond the contracts:** Lava vendored with a fragment
`discard` (OpKill) patch; pinned model fetcher, glTF node importer (TRS and
texture-transform baking), headless Blender decimation, numpy leaf-card baker;
`Campaign2Inputs` explicit-input authorization (carried item 5, for
`render-campaign2-layout`); glTF channel semantics (carried item 4); worker
frame bound 128 MiB.

**Defects found and fixed on the way:** glTF base colours that are JPEG lose
their alpha (merged from the separate alpha map); Blender collapse emits zero
normals on degenerate triangles (dropped / face-normal fallback); the first
ruin layout read as an intact fort block (broken apart); the silhouette
measure itself counted partial edge pixels whole and was corrected before any
conclusion was drawn from it.

**Carried into CONVERGE-3** (from reviews and findings):

1. **Distance foliage (L-2):** alpha-tested leaves stipple at 40 m (0.77 of
   geometry); needs alpha-to-coverage + MSAA or TAA, or distance LOD cards.
   `tools/foliage_measure.py` is the measurement to move.
2. **Wet pool:** the procedural glowing ripple rings are the last campaign2
   primitive in the frame and now read as artificial next to scanned assets.
3. **Material varieties** (CONVERGE-1 item 1): several scans per family plus
   per-instance tint / scale / rotation / wear / wetness. Pays off with N-7
   scatter and forest.
4. **Scatter + forest (N-7):** seven trees and twelve ferns are hand-placed
   constants; density needs the placement grammar.
5. **Ruin wall end** reads as a solid block more than a broken wall; a ruin
   needs damage geometry (broken tops, fallen blocks), not only gaps.
6. **Bark depth** (CONVERGE-1 item 3) and **km-scale backdrop + wet/dry
   target restatement** (CONVERGE-1 item 6, N-3), still open.
7. **Kit `prepare` receipts:** kit assets are checked by render conditioning;
   runtime receipts (collision, LODs) are not authored yet.
8. **Carried item 5 elsewhere:** `render_and_promote` and
   `render_and_promote_with_terrain_layers` still read the arm from the
   environment for their other callers.
