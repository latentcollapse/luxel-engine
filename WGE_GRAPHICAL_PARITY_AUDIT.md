# WGE Graphical Parity Audit

Status: **audit complete; gap register open; no renderer change made by this audit**, 2026-10-02
Method: source inventory of the shipped render path + direct measurement of the promoted Campaign 2 captures + diff against PS4/Xbox One-era (2013–2015) visual expectations.
Scope: graphical parity only. Gameplay, input, netcode, and the TetCage P2/P3 phases are out of scope except where they are load-bearing for a graphical gap.

Every claim below is tagged. **[M]** = measured from a promoted artifact or from source I read. **[A]** = asserted by an existing document, not re-verified here. **[J]** = my judgement.

---

## 0. The finding that outranks the twelve axes

Two structural gaps sit above the feature list. Fixing features without fixing these produces motion without progress.

### 0.1 The style compiler has a vocabulary and no executor. **[M]**

`world_core/crates/wge_control_plane/src/style_profile.rs` is real, typed, and good: nine policy groups, ~45 basis-point signals with `source_kind` (observed / inferred / assumed / requested / repaired), provenance records, region references, conflict lists, content digests, and an authority-validated `StylePlan::lower` over the registered capability catalog. This is the best-designed piece of the graphics stack and it should not be rewritten.

But trace it forward:

- `capability_for_axis` (`style_profile.rs:814`) returns a **capability id string** and stops. Every other axis returns the same three ids.
- `StylePlan` is consumed by `construction_plan.rs:295`, which stores `style_plan_id` and `style_plan_sha256` — and then validates those two strings match (`construction_plan.rs:334`). It does not read a single policy value.
- Grepping `native_graphics_contract` for `style` returns only `validate_marker_style`, which is about marker radius and colour.

So a `StyleProfile` can say `lighting.shadow_softness = 2000` and `camera.field_of_view = 7000`, be validated, be digested, be bound into a construction plan, and reach the renderer as **nothing**. Every renderer behaviour is a hard-coded constant in `LavaAdapter.jl`.

`style_axis_supported` (`:804`) fences four axes as unsupported — `environment.reflection_expectation`, `environment.volumetrics`, `effects.outline`, `effects.posterization` — and marks the plan `Partial`. That is honest about four axes and silent about the other forty-one. The contract knows what it cannot do; the executor knows it cannot do anything.

**Consequence for the phase framing.** "Compile another coherent policy over the same underlying rendering machinery" is not currently *hard* — it is *impossible by construction*, because there is no compiler output channel. The art-style generality test (two briefs, two games, nobody guesses they share an engine) cannot pass while style is a compile-time constant. This is GP-01 and it gates Tier 4.

### 0.2 The quality authority measures proxies, so it cannot tell good from not-flat. **[M]**

`visual_quality.rs` is a genuinely careful gate: registered integer thresholds, capture/receipt/packet binding, known-good and known-bad controls, independent Rust remeasurement, tampered-capture and tampered-receipt rejection, and honest `indeterminate`. I want to be clear that this is good work.

It is also, for graphical parity, measuring the wrong things. The eleven axes resolve to:

| axis | what is actually measured | source |
| --- | --- | --- |
| silhouette_readability | adjacent-pixel edge-pair fraction | `measure_quality_region` edge counter |
| texture_frequency | edge fraction + distinct RGB bin count | same region stats |
| material_separation | count of distinct 12-bit RGB bins | same region stats |
| composition | projected content coverage (bp of frame) | mask coverage |
| density | visible_instance / total_instance | packet telemetry |
| artifact_rate | non-opaque output alpha pixel count | capture bytes |
| frame_cost / memory | telemetry integers | receipt |
| **grounding_contact** | **indeterminate** | — |
| **lighting_consistency** | **indeterminate** | — |
| **atmospheric_depth** | **indeterminate** | — |

Measured from the promoted campaign2 vector: `grounding_contact` is indeterminate because "the current promoted capture has no depth or contact-classification"; `lighting_consistency` because "no reference" exists; `atmospheric_depth` because "not independently judged."

And there is **no reference-image comparison anywhere in the repository** — no SSIM, no perceptual hash, no target image, no A/B comparator. Grep-verified across `world_core/crates`.

So: the three axes that a PS4-era image actually wins on are the three the engine cannot measure, and the eight it can measure are all satisfiable by a flat grey plane with noise on it.

**This is why the C4 exit gate matters more than it looks.** `WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md` §11 defines C4's exit as "independent human review describes the output as a commercially plausible game screenshot." The current quality authority **cannot close C4**, by construction — it has no opinion about plausibility. Human review is the only oracle, and human review does not scale, does not regress-test, and does not tell you *which* change helped.

**Consequence.** Every renderer improvement in this audit is currently unfalsifiable by the engine. That is GP-02, and it is why Tier 0 exists and why it runs before Tier 1. Without a comparator, "make it look expensive" is a vibe with a changelog.

---

## 1. What the shipped renderer actually is

`graphics_lab/src/LavaAdapter.jl`, 4,045 lines, `ADAPTER_REVISION = "wge.lava-adapter/v7"`, bound to Lava `11c7e31b…` and Vulkan API 1.4.351 on an RTX 5060. **[M]**

### 1.1 The whole frame graph is ten pipelines. **[M]**

`probe`, `sky`, `terrain`, `overlay`, `mesh`, `texture_probe`, `depth_probe`, `terrain_shadow`, `mesh_shadow`, `resolve`.

Per frame, at 2× supersample (`render_width = 2 * width`, `:3551`):

1. fullscreen-triangle sky at `z = 0.99`, `DepthOff`, clear `(0.02, 0.03, 0.05)`
2. terrain — 6 vertices per cell, 13,824 verts at res 49
3. instanced mesh batches — one per `(mesh_id, material_id)`, 28 draw calls measured
4. colour→sampled transition
5. resolve — 4-tap box at half-texel, then ACES-fitted tonemap with exposure
6. semantic overlays — point / circle / polyline, `DepthOff`
7. full RGBA CPU readback

There is **no compute pipeline and no `dispatch!` call anywhere in `graphics_lab/src` or `graphics_lab/bin`** — grep-verified. `dispatch_calls` in the telemetry is a **hard-coded literal `0`** at `LavaAdapter.jl:1532` and `:3701`, not a measurement. It is currently *true*, because nothing dispatches. It will silently become false the moment TetCage lands in the product path, and the receipt will report zero.

### 1.2 Lighting: exactly one light, and it must be directional. **[M]**

```julia
length(packet.lights) == 1 ||
    throw(AdapterError("unsupported_lighting", "native material path requires exactly one light intent"))
```
`:2426`. A `PointLightPacket` throws `unsupported_light` outright (`:2420`). The packet schema *has* point lights; the adapter refuses them.

The environment is a three-colour analytic gradient — `sky_top`, `sky_horizon`, `ground`, square-root-blended on the vertical component (`_environment_color`, `:296`). **There is no image-based lighting of any kind**: no irradiance probe, no prefiltered specular, no SH, no BRDF LUT, no reflection probe.

The ambient terms are constants, not integrals: `ambient_diffuse_scale = (1 - metalness) * 0.52` and `ambient_specular_scale = (0.08 + 0.16 * (1 - roughness))` (`:500–501`). They are multiplied by an occlusion texture sample, then by a colour read out of the three-colour gradient.

### 1.3 Materials: the math is era-correct; the inputs are not. **[M]**

`_material_response` (`:388`) is a real Cook–Torrance: GGX `D`, Smith `G` with the Schlick-GGX `k`, Schlick `F` from `view_half`, a separate clearcoat lobe with its own `D`/`G`/`F`, and an emissive term. Tangent-space normal mapping with handedness from `tangent[4]` (`_perturbed_normal`, `:355`). Five role-typed maps: albedo, normal, roughness, occlusion, emissive.

This is 2013-correct shading. It is being fed 8×8 textures.

### 1.4 Shadows: one map, four taps, and a hard-coded visibility floor. **[M]**

```julia
const SHADOW_MAP_SIZE = 512          # :3032
texel = 1.0f0 / 512.0f0              # :1092
bias  = 0.0035f0                     # :1093
return 0.25f0 + 0.75f0 * (visible * 0.25f0)   # :1100
```

The shadow camera is a single orthographic frame fitted to the union of terrain bounds and every instance bound (`_shadow_frame`, `:2157`). No cascades. 4-tap PCF. 512² across a 96 × 72 m field is ≈5.4 texels/metre.

Line `:1100` is the one I would change first in this file. **A shadowed surface can never receive less than 25 % of direct light.** That single constant is why the image reads as ambient-lit and toy-like no matter how correct the BRDF is. Contact shadows, contact hardening, and any shadow-darkening technique are not *absent* — they are **unrepresentable**.

### 1.5 Transparency is fenced at the validator, not just unimplemented. **[M]**

```julia
material.alpha_mode == :opaque ||
    throw(AdapterError("unsupported_material", "native path requires opaque materials"))
```
`:2441`.

Alpha-tested foliage is therefore not a renderer task. It is a validator change plus a shader change plus a depth-sorting policy change.

### 1.6 Terrain ignores the data it is given. **[M]**

`_terrain_base_color` (`:737`) takes slope and region and returns `base_color` unchanged, with a comment explaining that region codes are categorical traversal labels rather than an ordered palette, and that albedo stays tied to the typed material until the packet carries a surface-layer mapping.

That reasoning is correct. The consequence is that terrain UV is `normalized_x, normalized_z` — 0→1 across the whole field (`:818`) — sampling a **160 × 160** albedo over **96 × 72 m**, i.e. ≈1.67 texels/metre, with `wrap = :clamp`. One texture, stretched, clamped, no tiling, no detail layer, no splat, no macro variation.

### 1.7 Geometry is expanded to triangle soup with per-triangle tangents. **[M]**

`_mesh_resources!` (`:2906`) walks `1:3:length(indices)` and pushes position, normal, uv and tangent **per index**. Measured on the promoted wide packet: 8,550 unique vertices, 23,640 indices, and **22,824 submitted mesh vertices** — the indices are fully expanded, so every triangle carries its own tangent and there is no vertex cache reuse and no way to express split normals or hard edges from an imported mesh.

The imported-tangent path is real (C2.5/C2.6/C2.7/C2.8 all closed) and **completely unused by the authored scene**: all sixteen campaign2 meshes have `tangents = []`, so every one of them takes the per-triangle fallback.

### 1.8 Post-processing is a tonemap. **[M]**

`_resolve_fragment` (`:1123`): 4-tap box, then `_tone_map` (`:202`) — an ACES fitted curve with a scalar exposure. That is the entire post chain. No bloom, no grade, no vignette, no chromatic aberration, no sharpen, no film grain, no FXAA, no TAA, no jitter, **no dither**.

### 1.9 Textures are uncompressed float and tiny. **[M]**

`FORMAT_R32G32B32A32_SFLOAT` — 16 bytes per texel, no BC/ASTC anywhere. Measured over all 17 textures in the promoted wide packet:

```
riverwatch-terrain-albedo-v4    128x128   mips=1   srgb
wge-campaign2-terrain-albedo    160x160   mips=1   srgb
wge-campaign2-wet-albedo         32x32    mips=1   srgb
wge-campaign2-emissive           32x32    mips=1   srgb
riverwatch-beacon-emissive       16x16    mips=1   srgb
riverwatch-{stone,foliage,beacon}-albedo, normal, roughness, occlusion, emissive
wge-campaign2-{foliage,bark,hero-stone,hero-metal,hero-glow}-albedo
                                 8x8      mips=1   ...
```

Twelve of seventeen textures are 8×8. One is 160×160. Every one is single-mipped in this packet. An 8×8 albedo is a colour lookup table, not a surface.

### 1.10 The camera has no lens. **[M]**

`CameraPacket` carries `position`, `forward`, `up`, `near`, `far`, `fov_y` or `span_m`, `width`, `height`. Three fixed named cameras in campaign2 (`close`, `medium`, `wide`). No depth of field, no motion blur, no shake, no roll, no dynamic FOV, no cinematic spline. Composition is hand-authored in the packet, which is a legitimate choice — but there is no *lens* to compose through.

### 1.11 `MeshPacket` has nowhere to put a character. **[M]**

```julia
struct MeshPacket
    mesh_id, positions_m, normals, uv0, indices, material_id, tangents
end
```
`WGEGraphics.jl:88`. No joints, no weights, no bone indices, no blend shapes, no animation channels, no morph targets. This is not a renderer gap. It is a schema gap, and it blocks characters *and* TetCageRT in the product path simultaneously. See §3, GP-09.

---

## 2. Measured state of the best frame we can currently produce

Source: `artifacts/campaign2/live-twelfth/` — three promoted views, `outcome: good`, adapter-v6, clean-process replay byte-identical.

### 2.1 Scale **[M]**

| | close | medium | wide |
| --- | --- | --- | --- |
| capture | 768×512 | 768×512 | 960×640 |
| draw calls | 28 | 28 | 28 |
| instances (visible/total) | 22 / 34 | 34 / 34 | 34 / 34 |
| terrain verts | 13,824 | 13,824 | 13,824 |
| mesh verts (submitted) | 22,824 | 22,824 | 22,824 |
| upload / readback | 2.70 / 1.57 MB | 2.70 / 2.46 MB | 2.70 / 2.46 MB |
| **content coverage** | **7.10 %** | 5.77 % | **3.83 %** |

8,550 unique vertices, 7,880 triangles, 34 instances, 16 meshes for an entire authored frame. For reference, a single PS4-era hero character is 30–80k triangles. **The whole scene is smaller than one hand.** All of it procedurally generated in `lower_campaign2_packet` — there is no imported hero asset in the authored frame at all.

### 2.2 Tonal range — this is the "cheap" signal **[M]**

| view | p50 | p90 | p99 | p99.9 | max | ≥200 | ≥240 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| close | 99 | 116 | 182 | 222 | 248 | 0.340 % | 0.008 % |
| medium | 89 | 112 | 153 | 195 | 247 | 0.074 % | 0.002 % |
| wide | 67 | 110 | 133 | 185 | 244 | 0.036 % | 0.001 % |

Over 99 % of the close frame sits below luminance 182, and 99.66 % below 200. Distinct colours: 38,597 (wide), 41,815 (close). Mean saturation 33–38 / 255.

There are essentially **no specular highlights and no bright accents**. That is the signature of an image with no image-based lighting, one bounce-less analytic ambient at a fixed 0.52, and a shadow floor at 0.25.

### 2.3 The sky is visibly banded, and it is measurable **[M]**

Top quarter of frame is 96.1 % (close) and 100 % (medium, wide) bluish gradient. Scanning that sky:

```
sky column:  151/159 adjacent-row pairs have IDENTICAL rgb
sky row:     959/959 adjacent pixels have IDENTICAL rgb
```

A perfectly flat horizontal gradient quantised to 8 bits with **no dither**. This is not a subtle artefact — a smooth sky gradient rendered without dither is one of the most immediately "this looks cheap" reads in real-time graphics, and it is the highest ratio of perceived improvement to code in this entire audit.

### 2.4 Frame cost — neither clock is raster time **[M]**

| view | `frame_cost_us` (wall) | `gpu_frame_cost_us` |
| --- | --- | --- |
| close | **12,651,652** (12.65 s) | 12,521,490 |
| medium | 299,179 | 104,282 |
| wide | 864,216 | 103,468 |

Two observations, both important:

- The close view takes **12.65 seconds** for a promoted frame, with `pipeline_compilations = 0` — so it is not shader compilation. Wall ≈ GPU to within 1 %, which means the GPU clock bracket is measuring *wall-clock elapsed on a queue that is idle*, not rasterisation.
- The warm views sit at ~104 ms "GPU" for 28 draw calls and 36k vertices on an RTX 5060. That is ~150× off what the silicon can do for this workload. **The bottleneck is not fill rate and not draw submission.**

So both clocks are unattributed, and pre-demo audit B-2 already flags the wall-minus-renderer gap as un-attributed. The audit's addition: the *GPU* bracket is also not raster time, so "profile the shaders" is the wrong next move. Attribute instrumented-vs-uninstrumented, readback fence, and present first. **Nobody should add a shadow cascade until this is understood** — but equally, this is not a reason to delay Tier 1, because none of Tier 1 is fill-bound.

### 2.5 The one thing that is already excellent **[M]**

`artifacts/screenshots/visual-axis-*` is a retained **negative** experiment: a slope-readability remap was driven through the real Rust-supervised Lava/Vulkan path, measured against a fixed terrain crop, found to reduce capture luminance spread by 0.95 % and crop spread by 1.78 %, and therefore **rejected — with the candidate absent from the renderer**.

That is a real experimental apparatus: a hypothesis, a before/after pair with full identity binding, a measurement, a rejection, and no retained change. It is the substrate every visual improvement in Tier 1–4 needs. It should be scaled up, not replaced.

---

## 3. Gap register

Severity: **S1** = blocks the C4 exit gate or the demo's credibility · **S2** = the dominant perceptual gap on its axis · **S3** = real but bounded · **S4** = structural debt.

| ID | Axis | Gap | Sev | Evidence | Tier |
| --- | --- | --- | --- | --- | --- |
| GP-01 | all | `StylePlan` has no executor; policy never reaches renderer state | **S1** | `style_profile.rs:814`, `construction_plan.rs:295` | 4 |
| GP-02 | all | quality gate measures proxies; 3 of 11 axes permanently indeterminate; no reference comparator exists | **S1** | `visual_quality.rs`, campaign2 vector, grep | 0 |
| GP-03 | lighting | no IBL of any kind; ambient is two hard-coded constants | **S2** | `:500–501`, no prefilter/SH/LUT | 2 |
| GP-04 | shadows | shadow visibility floor of 0.25; one 512² map; no cascades, no contact, no PCSS | **S2** | `:1100`, `:3032`, `:1092` | 2 |
| GP-05 | assets | 12/17 textures are 8×8; largest is 160×160; none tiled; `R32G32B32A32_SFLOAT` | **S2** | packet `textures[]`, `:1997` | 1 |
| GP-06 | post | no dither — measurable sky banding (959/959 identical pixels) | **S2** | §2.3 | 1 |
| GP-07 | lighting | exactly one directional light; point lights throw | **S2** | `:2420`, `:2426` | 2 |
| GP-08 | materials | `alpha_mode != :opaque` is a hard validator refusal | **S2** | `:2441` | 3 |
| GP-09 | animation | `MeshPacket` has no joints/weights/clips; zero skinning; also blocks TetCage in-product | **S1** | `WGEGraphics.jl:88` | 3 |
| GP-10 | terrain | region/slope ignored; 1.67 texels/m; no splat, detail or macro layer | **S2** | `:737`, `:818` | 1 |
| GP-11 | post | post chain is a tonemap; no bloom/grade/vignette/sharpen/FXAA/TAA | **S3** | `:1123`, `:202` | 1 |
| GP-12 | geometry | full triangle-soup expansion; per-triangle tangents; no index reuse | **S3** | `:2906`, 22,824 submitted | 1 |
| GP-13 | reflections | no SSR, no probes, no cubemap fallback; env specular is a 3-colour gradient | **S2** | `_environment_color:296` | 2 |
| GP-14 | atmosphere | linear distance fog clamped at 0.92; no height fog, no aerial perspective, no scattering | **S2** | `:565`, `:232` | 2 |
| GP-15 | compute | product adapter has no compute pipeline; `dispatch_calls` is a hard-coded `0` | **S2** | grep; `:1532`, `:3701` | 3 |
| GP-16 | perf | 12.65 s close frame; ~104 ms warm "GPU"; neither clock attributed | **S1** | §2.4 | 0 |
| GP-17 | camera | no DoF, motion blur, shake, roll, or dynamic FOV | **S3** | `CameraPacket` | 2 |
| GP-18 | VFX | no particles, decals, trails, or screen-space effects anywhere in the shipped path | **S2** | grep | 3 |
| GP-19 | scale | CPU sphere-frustum cull on 34 instances; no LOD, occlusion, residency, streaming, meshlets | **S3** | `_instance_visible:2806` | 3 |
| GP-20 | assets | no imported hero asset in the authored scene; 7,880 tris total; no compression | **S3** | §2.1 | 1 |
| GP-21 | composition | wide view is 3.83 % content; 100 % of top quarter is sky | **S3** | §2.1, §2.3 | 1 |

---

## 4. Staged plan

Ordered by perceived gain ÷ risk, with dependencies made explicit. Tier 0 first because it is cheap and it is what makes the rest falsifiable.

### Tier 0 — falsifiability and attribution (do first, it is small)

- **0.1 · Depth into the capture contract.** `render_depth_probe` (`:1538`) already proves the depth attachment works; the packet sets `include_depth = false` (`lib.rs:2777`, `:5501`) and Julia *requires* it false (`WGEGraphics.jl:1153`). Flip it behind a flag: capture depth, bind it into the receipt, and `grounding_contact` stops being indeterminate. **[M]** the capability is proven; only the contract refuses it.
- **0.2 · A registered reference comparator.** Deterministic, integer thresholds, registered in the same catalog as `terrain_reference_v1`. It does not need to be SSIM; it needs to answer "did this change move the frame toward the brief, and by how much." Register the *target* the same way profiles are registered, so the threshold cannot be re-derived from the measurement it consumes (FR-0016).
- **0.3 · Attribute the frame cost.** Instrumented vs uninstrumented, with and without the readback fence, with and without named timestamps. Establish which of the two clocks means anything. Then set the 16.6 ms budget against the number that is real.
- **0.4 · Self-asserting calibrations.** Every new threshold in 0.1–0.2 must prove it can fail on a known-bad control before it is allowed to gate a known-good one.

### Tier 1 — the cheap perception wins (no contract change; days)

- **1.1 · Dither before quantisation.** Kills GP-06. Five lines in `_resolve_fragment`/capture. Highest ratio in the audit.
- **1.2 · Real textures and real tiling.** GP-05, GP-10. 1024² hero albedo, BC/ASTC compression, terrain UV derived from world metres with tiling enabled, a detail-normal layer, macro variation. Break `wrap = :clamp` on terrain.
- **1.3 · Anisotropic filtering.** The sampler is built with anisotropy disabled (`_lod_sampler!`, `:1778`). Turn it on; the grazing-angle terrain shimmer goes away.
- **1.4 · Grade.** GP-11. Lift/gamma/gain, a restrained bloom, a vignette. All deterministic, all after resolve, all measurable by 0.2.
- **1.5 · Index the geometry.** GP-12. Stop expanding to triangle soup; keep the index buffer. Recovers vertex cache and unblocks split normals from imported meshes.
- **1.6 · Recompose the wide view.** GP-21. The engine already measures composition at 3.83 %. That is a scene-authoring problem with a gauge attached, which is the cheapest kind of problem.

### Tier 2 — the perceptual big one (contract + renderer; this is the "expensive" look)

- **2.1 · Prefiltered IBL.** GP-03, GP-13. Irradiance + prefiltered specular cubemap + BRDF LUT, baked in Rust from the packet's existing environment intent, digest-bound, GPU-resident, mip-chained through the C2.8 path that already exists. **This single change moves the image from "correct 2005" to "2013."** It also un-fences `environment.reflection_expectation`, which `style_axis_supported` currently marks unsupported for lack of a renderer capability.
- **2.2 · Shadow rebuild.** GP-04. 2–3 cascades by view depth; PCSS or a Poisson disk with a real filter width; **remove the 0.25 floor**; add a screen-space contact term (GTAO-lite) so objects sit on the ground. Contact shadows become measurable once 0.1 lands, which is why 0.1 precedes it.
- **2.3 · Multiple lights.** GP-07. Clustered-forward or a bounded local-light list. The packet schema already has point lights — the adapter refuses them. This is mostly a matter of deleting a refusal responsibly.
- **2.4 · Atmosphere.** GP-14. Height fog, aerial perspective, a sky that is not a two-stop gradient (cheap Preetham or a sky cubemap). Un-fences `environment.volumetrics` on the shadow side.

### Tier 3 — the ones that need the schema to move first

- **3.1 · `MeshPacket` v8: joints, weights, blend shapes, animation channels.** GP-09. This is the highest-leverage item in the repository and it is currently blocked by a JSON schema, not by capability: **TetCageRT's compute skinning already exists and is proven in TetLab.** A v8 packet carrying `joint_indices` / `weights` / `inv_bind_matrices` / clip channels unblocks characters *and* moves TetCage from a quarantined research lane into the product adapter in one change. Whoever does this touches shared contract types on both the Rust and Julia sides — see the standing item in §6.
- **3.2 · Alpha policy.** GP-08. Decide cutout vs blend; add alpha-test; add depth-prepass / no-write for foliage; add alpha-to-coverage. The packet field exists; the validator refuses it.
- **3.3 · A compute path in the product adapter.** GP-15. Zero pipelines today. Particles, decals, TAA, GPU skinning, and TetCage all need it. **And fix `dispatch_calls` to be a measurement, not a literal**, in the same change — otherwise the receipt will report zero dispatches for a compute-heavy frame and the certification will be quietly false.
- **3.4 · Scale.** GP-19. LOD chains, occlusion culling, residency/streaming, index the draws. 34 instances on a CPU sphere test is fine; 30,000 is not.

### Tier 4 — the compiler (GP-01; makes the rest into policy instead of constants)

- **4.1 · `StylePlan` → render policy executor.** `capability_for_axis` must return *parameters that reach the packet and the pipeline state*, not a capability id. Every value produced in Tiers 1–3 becomes a policy input rather than a hard-coded constant. This is the single change that converts "one dark-fantasy look" into "a renderer with a policy surface."
- **4.2 · Retire the four hard-coded axis frictions.** `effects.outline` and `effects.posterization` are cheap post effects and could close in Tier 1. `reflection_expectation` closes with 2.1, `volumetrics` with 2.4/3.3.
- **4.3 · The generality test.** C7.5: hold mechanics and scene constant, compile two materially different profiles, prove two images nobody would guess share an engine. Only meaningful once 4.1 exists.

### Explicitly not recommended yet

- **Do not rewrite to deferred.** Forward+ / clustered-forward is the era-appropriate answer, costs less to get right, and preserves the existing determinism posture.
- **Do not start TetCage P2/P3 yet.** P2's instancing ladder is legitimate work but P3's A/B is unmeasurable today: the product adapter has no compute path and no deformation reference (`LavaAdapter.jl` contains zero occurrences of `deformation`), and `MeshPacket` has no joints. **3.1 first makes P2 and P3 cheap; doing them first means doing them twice.**
- **Do not optimise the shaders.** §2.4. The workload is 28 draw calls and 36k vertices. The bottleneck is attribution, not fill.

---

## 5. What this means for the phase framing

The four fronts are not independent, and this audit mostly lands on **ease**.

- **Graphical parity** is not blocked by missing shader features — the BRDF is correct. It is blocked by (a) inputs that are 8×8, (b) an ambient term that is two constants, (c) a shadow floor that forbids contact, and (d) a quality authority that cannot tell good from not-flat. Three of those four are small.
- **Reproducibility** is in unusually good shape. The negative experiment in §2.5 is the proof: hypothesis → measured A/B → rejected → not retained. The risk in Tier 2–3 is not losing this; it is being tempted to buy temporal reconstruction (TAA, motion blur, stochastic soft shadows) with it. Those belong in a **live-only** path with the deterministic certification frame kept intact, which the sprint doc §11.6 already anticipates.
- **Ease** is where the compounding return is. GP-01 says every one of the ~45 style signals is currently decorative. A `StyleProfile` that a model can write and that changes the image is the difference between reasoning about shadows every session and naming `lighting.shadow_softness`. That is exactly the `model reasons → recipe → semantic operation → helper → primitive` ladder, and the top two rungs of it are currently unbuilt.
- **Generality** is correctly deferred per the framing, and GP-01 is the concrete reason it *must* be: you cannot demonstrate art-style generality against a renderer whose style is a constant. But the framing's own route — make one game excellent, then a radically different one, then generalise only the seams the new game exposed — argues for resisting Tier 4 until Demo A has actually beaten on the system. **Tier 4.1 is the one item here I would defend doing early anyway**, because retrofitting an executor onto forty-five hard-coded constants is strictly more expensive than building the channel once.

---

## 6. Standing items

1. **P1 evidence is contract-level only, and this audit hardens that.** `LavaAdapter.jl` has zero references to `deformation` and zero compute pipelines. TetCageRT is currently a schema with a proven kernel behind a quarantined lab. It contributes nothing to graphical parity until 3.1 and 3.3 land.
2. **`dispatch_calls = 0` is a literal.** `:1532` and `:3701`. Currently true, silently false the moment compute ships. This is a "telemetry that cannot fail" candidate for the friction ledger — same family as FR-0016, where a derived value disabled the check that consumed it.
3. **A slice that changes a shared contract type owes the workspace gate, not its own crate's suite.** `wge-certification-authority` was already broken at HEAD once (missing `texture_residency` in two tests) and had to be repaired. Tier 3.1 changes `MeshPacket` and `GraphicsScenePacketBody` on both the Rust and Julia sides — that is the exact shape of change that broke it. Whoever does 3.1 runs the full workspace gate.
4. **The visual-quality gate cannot close C4.** C4's exit is human review of commercial plausibility; the authority has no plausibility opinion. Either the comparator in 0.2 becomes real, or C4's exit stays a human judgement forever. That is a decision, not an implementation detail, and it should be made explicitly rather than discovered at C4.
