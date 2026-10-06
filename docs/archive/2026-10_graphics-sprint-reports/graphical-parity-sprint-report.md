# WGE — GRAPHICAL PARITY SPRINT REPORT

**Codename:** EXIT THE 90s
**Authority:** `docs/world/graphical-parity-audit.md` (gap register GP-01…GP-21)
**Scope rule:** no deferred renderer rewrite, no TetCage P2/P3, no fake parity.

This report is written against fresh measurements only. Where a claim could not
be measured this sprint, it is recorded as **NOT CLOSED** rather than smoothed
over. Human visual review (§8) is explicitly *pending* — the numbers below
establish that the image moved and in which direction; they do not certify that
the result looks like a commercial game.

---

## 1. Baseline state

Frozen at `artifacts/parity/baseline/run-a/` before any renderer change.

Packets AND `native_capture.ppm` are **byte-identical** to the committed
`artifacts/campaign2/live-twelfth/`. Receipts differ only by the adapter v6→v7
revision bump and the addition of `texture_residency` telemetry
(`upload_bytes` 2 701 693 → 3 054 265). Capture bytes unchanged.

| view | packet sha256 (first 16) | capture ppm sha256 (first 16) |
|---|---|---|
| close  | `4615063b79307a6c` | `a95a6ebf852a2850` |
| medium | `3d56d4bed7c32054` | `6b87c2e4e75808a5` |
| wide   | `bd329b881030be86` | `3ce4589a61dde704` |

Baseline quality metrics (`artifacts/parity/baseline/metrics.json`):

| view | sky row-identical % | sky col-identical % | content coverage % | hi≥200 % | distinct RGB |
|---|---|---|---|---|---|
| close  | 98.37 | 30.71 | 70.80 | 0.3400 | 41 815 |
| medium | 100.00 | 94.97 | 68.47 | 0.0741 | 46 081 |
| wide   | 100.00 | 94.97 | 60.96 | 0.0363 | 38 597 |

Replay/certification remained green throughout; the workspace gate is reported
in §10.

---

## 2. GP gap IDs attacked

| GP | Area | Sprint section | Outcome |
|---|---|---|---|
| GP-01 | Deterministic visual measurement did not exist | §2.2 | **CLOSED** — comparator + metric instrument |
| GP-02 | Sky banding unmeasured/unattacked | §3.1 | **CLOSED** — RETAINED |
| GP-03 | No post chain beyond tonemap | §3.5 | **CLOSED** — landed, measured in `full` |
| GP-04 | Hard-coded 0.25 direct-light visibility floor | §5.2 | **CLOSED** — ShadowPolicy, measured |
| GP-05 | Style signals had no execution channel to the renderer | §4 | **CLOSED** — RenderPolicy spine |
| GP-06 | Unsupported policy axes silently ignored | §4/§11 | **CLOSED** — fail-closed refusal |
| GP-07 | Terrain stretched one whole-world 0..1 UV | §3.3 | **CLOSED** — 8x repeat tiling measured, pending human review |
| GP-08 | Anisotropy disabled | §3.4 | **NOT CLOSED** — blocked by device feature |
| GP-09 | Frame-cost clocks not trustworthy | §2.3 | **NOT CLOSED** — root cause identified |
| GP-10 | `dispatch_calls` is a literal zero | §7 | **NOT ATTEMPTED** — out of runway |
| GP-11…GP-21 | Depth capture contract, reference comparator breadth, MeshPacket v8, compute path, local lights, atmosphere, IBL, indexed geometry, recomposed frame | — | **NOT ATTEMPTED** — no runway |

---

## 3. Changes retained

### 3.1 Deterministic Bayer 8×8 dither (display space, immediately pre-quantisation)

- **Defect attacked:** sky banding. 98–100 % of vertically adjacent sky pixel
  pairs were bit-identical — the gradient was quantised into visible steps.
- **Where:** `LavaAdapter.jl` — literal `BAYER_8X8` table, `_bayer8x8`,
  `_apply_dither_lsb`, applied in `_rgba8`/`_capture_bytes` after the sRGB
  encode, alpha never dithered.
- **Policy:** `DitherPolicy.amplitude_milli_lsb` (0…2000), default 0.

### 3.2 Post chain v1 — lift/gamma/gain, saturation, bloom, vignette

- Grade with a **bit-exact identity short-circuit**. This is not a nicety:
  `x ^ 1.0f0` routes through `powf`, which is not guaranteed to return `x`, and
  applying the *default* grade moved 6 bytes of the frozen capture by 1 LSB each.
- Bloom is a 4-tap cross knee, unrolled (Lava's shader JIT cannot lower a loop
  with a constant bound), gated by a host-testable `_bloom_engaged` predicate.

### 3.3 Shadow rework — the 0.25 floor becomes policy

- **Defect attacked:** the audit's finding that the image reads ambient-lit and
  toy-like because a shadowed surface could never receive less than 25 % of
  direct light. Contact shadows were not absent, they were **unrepresentable**.
- `ShadowPolicy { darkness_bp, filter_radius_milli }`, defaults 7500 / 1000.
  `(1 - 0.75) + 0.75*pcf` is arithmetically identical to the historical
  `0.25 + 0.75*pcf`, and 1000 milli = 1.0 texel, so an absent policy is
  byte-identical — proven, not asserted (§5).

### 3.4 RenderPolicy → Lava execution spine

`StyleProfile → validated StylePlan → typed RenderPolicy → packet → Lava state`.
Every field is `Option` + `skip_serializing_if`, so **absence is byte-identity by
construction**. Defaults live in Rust and Julia, never in a shader.

### 3.5 Fail-closed refusal of unhonourable policy axes

A packet asking for a policy the renderer cannot honour is now **refused with a
typed, named, actionable error** rather than rendered as though it had been
applied. This is the direct defence against the sprint's "decorative schema"
failure mode, and it is verified end to end (§5, `full-unsupported`).

---

## 4. Candidates rejected / blocked

### 4.1 Anisotropic filtering — BLOCKED by a real dependency constraint

Lava's pinned revision (`11c7e31`) constructs its `VkDevice` **without** the
`samplerAnisotropy` feature; `samplerAnisotropy` appears nowhere in its device
creation. Setting `anisotropyEnable = true` on a sampler without that feature is
invalid Vulkan usage — undefined behaviour, not a graceful no-op. This cannot be
fixed from the adapter alone; it needs an upstream device-feature change.

**Decision:** kept in the typed schema, **refused at render time**, with the
reason in the error text.

### 4.2 Terrain world-metre UV + repeat wrap — **RESOLVED (F-4/F-5)**

Originally blocked by a structural finding: `_material_texture_resources!` was
keyed by **material id, not by surface**, so terrain and meshes shared one
sampler and terrain could not request `REPEAT` without silently changing every
mesh's UV addressing at `uv == 1.0`.

Both findings are now closed.

**Sampler-per-surface.** `SamplerSpec` (filter + wrap) is now the cache key.
Caches were split into three so a second sampling spec does not re-upload:

```
material_textures   : Dict{(content_sha256, material_id), MaterialTextures}
surface_samplers    : Dict{SamplerSpec, LavaSampler}
material_texture_res: Dict{(content_sha256, material_id, SamplerSpec), ...}
```

The split matters for more than tidiness: `upload_bytes` is independently
validated on the Rust side, so fusing upload with sampler construction would have
multiplied a validated telemetry counter every time a material was asked to
tile. `_surface_sampler_spec(policy, surface)` is the single place the
terrain-vs-mesh asymmetry is decided — meshes are ALWAYS clamped, terrain
follows policy — so it is testable rather than discovered in a screenshot.

**Honest units (F-5).** `uv_scale_milli` was documented as "texture repeats per
world metre" while the adapter has always applied it as a multiplier on the
whole-field 0..1 UV. Those are different units: the documented 1000 (1.0
repeat/metre) would mean 96 tiles on a 96 m field, while the real baseline is a
single stretch. Renamed to **`uv_repeat_scale_milli`** with the actual semantics
stated ("repeats across the terrain extent"). The physically meaningful value
(metres per repeat) cannot be a policy field — it depends on the packet's
terrain extent — so it is exposed as the derived helper
`_metres_per_texture_repeat(repeat_scale, extent_m)`.

**`wrap_repeat` added** as an explicit boolean, because tiling REQUIRES a repeat
wrap and a clamped 8x tile would smear the border texel across most of the
field. It is a cross-field rule in both languages, not an inference:
`uv_repeat_scale_milli > 1000` with `wrap_repeat == false` is rejected.

**Dead API removed.** `_lava_texture2d!` accepted `filter` and `wrap` keywords
and never used them — the sampler is built in `_lod_sampler!`. They made the
code read as though per-texture sampling already existed, which is part of why
F-4 was hard to see. Removed.

**A deliberate non-change:** the terrain UV is `normalized * uv_repeat`, not a
world-metre computation. A world-metre form would introduce a float round-trip
through `width_m` and change the low bits of every UV — a byte-identity
regression for no perceptual gain, since the physical scale is already carried
by `uv_repeat`.

### 4.3 Bloom-as-GPU-unit-test — rejected

`Lava.sample_texture_2d` segfaults on the host (`Symbols not found:
[_lava_gfx_sample_2d]`, signal 11). The GPU-dependent tests were replaced with
host-testable extraction of `_bloom_engaged` / `_bloom_knee`; bloom's appearance
is evidenced by the fixed-camera A/B capture instead.

---

## 5. Measurements before/after

Instrument: `tools/parity_measure.py` (metrics) and `tools/parity_compare.py`
(registered comparator). Both refuse to report PASS over zero cells.

### 5.1 Control arm — the load-bearing falsification

`artifacts/parity/ab-null3` (no policy section) vs frozen baseline:

```
close   changed=0.0000%  darker=0.0000%  lighter=0.0000%  dMean=0.0000%
medium  changed=0.0000%  darker=0.0000%  lighter=0.0000%  dMean=0.0000%
wide    changed=0.0000%  darker=0.0000%  lighter=0.0000%  dMean=0.0000%
COMPARATOR (control): PASS
```

All three `native_capture.ppm` files **byte-identical**; all three packet
digests identical to the frozen baseline. This was re-verified *after* the
shadow and refusal plumbing landed, which is what licenses every later A/B.

### 5.2 Dither alone — RETAINED

| view | sky row-identical % | sky col-identical % | distinct RGB | mean luma Δ |
|---|---|---|---|---|
| close  | 98.37 → **18.77** | 30.71 → **3.15** | +1 530 | **+0.0082 %** |
| medium | 100.00 → **18.77** | 94.97 → **0.00** | +1 627 | +0.0042 % |
| wide   | 100.00 → **18.77** | 94.97 → **0.00** | +1 004 | +0.0018 % |

Read this carefully: 54–55 % of pixels changed, yet **12.0 % went darker and
12.8 % went lighter**, and every luminance quantile is unmoved. That symmetry is
the signature of a correct ±1 LSB display-space dither — it redistributes
quantisation error without shifting the image. A brightness hack would not
produce it.

The 18.77 % residual is **not** residual banding. It is the Bayer matrix's own
period, and the proof is that it is *identical to two decimal places across
three views of different size and content* (768×512 and 960×640). Genuine
scene-dependent banding would vary per view.

### 5.3 Shadow alone — RETAINED (pending human review)

| view | changed % | darker % | lighter % | mean luma Δ | p99 |
|---|---|---|---|---|---|
| close  | 10.23 | 8.32 | 1.53 | −0.74 % | 182 → 181 |
| medium | 5.43 | 4.20 | 1.10 | −0.33 % | 153 → 152 |
| wide   | 3.41 | 2.66 | 0.67 | −0.23 % | 133 → **130** |

The asymmetry (darken ≫ lighten) is the intended effect: removing a light floor
can only subtract direct light from already-shadowed surfaces.

**Spatial isolation is independently proven:** the shadow arm's sky banding
delta is exactly **+0.00** on all three views. The change never touched the sky,
so the darkening is localised to shadowed geometry rather than being a global
exposure shift.

### 5.4 Full candidate (dither + post + shadow) — RETAINED

| view | changed % | darker % | mean luma Δ | distinct RGB Δ | sky row % |
|---|---|---|---|---|---|
| close  | 71.39 | 43.22 | −1.74 % | +1 470 | 20.60 |
| medium | 68.76 | 38.75 | −1.27 % | +2 427 | 20.44 |
| wide   | 67.83 | 37.20 | −1.10 % | +1 641 | 20.44 |

Sky banding gate PASS on all three views. The residual rises slightly above the
dither-only 18.77 % because vignette darkens the sky's outer rows too.

### 5.5 Full-unsupported — REFUSAL PROVEN END TO END

```
Campaign 2 close render failed: worker_protocol: unsupported_render_policy:
render_policy.sampler.anisotropy=8 is refused: the pinned Lava device is created
without the Vulkan samplerAnisotropy feature, so enabling it here would be
invalid device usage. Request anisotropy 1, or land the upstream
device-feature change first.
```

The runner exited non-zero and produced no capture. A packet cannot declare a
policy that did not happen.

### 5.6 Terrain material scale — RETAINED, pending human review (F-4/F-5)

Candidate: `uv_repeat_scale_milli = 8000` (8 tiles across the field) with
`wrap_repeat = true`. On the 96 m riverwatch field that is one texture repeat
every 12 m, versus the baseline's single 96 m stretch.

| view | changed % | distinct RGB Δ | mean luma Δ | content coverage | sky band Δ |
|---|---|---|---|---|---|
| close  | 48.37 | **+10 875** | −0.58 % | 70.80 (unchanged) | **+0.00** |
| medium | 47.06 | **+11 859** | +0.38 % | 68.47 (unchanged) | **+0.00** |
| wide   | 32.13 | **+8 018** | +0.38 % | 60.96 (unchanged) | **+0.00** |

Two readings matter here:

1. **Distinct colour count rose by ~10.9k**, roughly **7× the entire dither
   candidate** (+1 530). That is the material-scale signal: the terrain now
   carries real surface variation instead of one stretched texel. Dither added
   noise; this added detail.
2. **Content coverage is IDENTICAL to four significant figures on all three
   views.** Combined with the sky delta of exactly 0.00, this shows the change
   is confined to the *appearance of existing terrain* — no geometry moved, no
   new silhouette appeared, and the sky was not touched. That is what a texture
   scale change should look like and nothing else did.

**Honest limit on the isolation claim.** The per-surface divergence (terrain
repeats, meshes clamp) is proven at unit level: `_surface_sampler_spec` returns
`:repeat` for terrain and `:clamp` for meshes under an identical policy, they
are distinct dict keys, an unknown surface kind is refused, and the defaults
stay clamped on both. The end-to-end evidence is the sky delta of 0.00 and
unchanged content coverage. A visual confirmation that the pedestal, copper
object, and foliage did NOT re-tile is part of the pending human review, not
something these metrics establish on their own.

Review bundle: `artifacts/parity/review-terrain/{close,medium,wide}-sheet.png`.

### 5.7 Hero material set — RETAINED, pending human review (goal step 1)

The root finding: **all eleven materials shared the same three maps.** The
normal map was 8x8 of modular arithmetic with a constant Z; roughness spanned
176..239; occlusion spanned 208..239. That is not a resolution problem — it is a
semantic one. No lighting can separate surfaces whose microfacet response is
literally the same image.

`material_maps.rs` now generates, per family (terrain/stone/bark/foliage/metal/
wet), a coherent 256x256 set where **albedo, normal, roughness, and occlusion
are all derived from one shared height field**, so they agree with each other.
Normals come from wrapped central differences; occlusion from a wrapped blur.
Colour space is declared per role (`Srgb` albedo, `NormalMap` normal, `Data`
roughness/AO). 10 of 11 materials now use per-family maps; `campaign2-hero-glow`
is deliberately unmapped because its read comes from its emissive map, and
reassigning surface maps to it would be an art decision made blind.

| view | changed % | distinct RGB Δ | p99 | hi≥200 | sky band Δ |
|---|---|---|---|---|---|
| close  | 69.97 | −1 438 | 182 → 148 | 0.340% → 0.077% | +0.00 |
| medium | 57.43 | **+3 046** | 153 → 141 | 0.074% → 0.014% | +0.00 |
| wide   | 38.26 | **+1 729** | 133 → 141 | 0.036% → 0.008% | +0.00 |

Sky banding delta is **+0.00** on all three views: the change is entirely in
surface shading, never in the sky.

**A regression I introduced and fixed.** The first version used a two-stop
dark/light albedo lerp. It measured **−21 286 distinct colours** (half the
baseline) and collapsed the highlight fraction **5.5x**. That is the image
getting *flatter* — the opposite of the goal — caused by a one-dimensional ramp
that reads as a tint rather than a material. Replaced with a three-stop blend
plus independent hue variation; distinct colours nearly doubled (20 529 →
40 377) and now exceed the baseline on two of three views.

**Remaining honest concern:** the highlight fraction is still roughly 4x below
baseline (p99 148 vs 182 in close). The likely cause is that real normal relief
scatters specular that a flat 8x8 normal kept coherent. Whether that reads as
"materials" or as "the highlight is gone" is a human judgement, not a number.

Review bundle: `artifacts/parity/review-hero/{close,medium,wide}-sheet.png`.

---

| arm | directory | status |
|---|---|---|
| frozen baseline | `artifacts/parity/baseline/run-a/` | control reference |
| null-policy control | `artifacts/parity/ab-null3/` | byte-identical to baseline |
| dither | `artifacts/parity/ab-dither/` | candidate |
| shadow | `artifacts/parity/ab-shadow/` | candidate |
| full | `artifacts/parity/ab-full/` | candidate |
| terrain tiling | `artifacts/parity/ab-terrain/` | candidate (F-4/F-5) |
| hero materials | `artifacts/parity/ab-hero3/` | candidate (goal step 1) |
| full-unsupported | `artifacts/parity/ab-full-unsup/` | expected refusal, no capture |

Each arm carries close / medium / wide captures plus packet, receipt,
attestation, and `metrics.json` + `compare.json`.

---

## 7. Human visual review — **CLOSED. RETAIN `full`.**

The mandatory human gate has been performed and the `full` arm is retained.

**Reviewer verdict:** "This is a real jump, not cope." The assessment is
precisely that the *content* remains primitive (low-detail geometry, tiny
textures, barren terrain, no atmosphere) while the *image formation* now
behaves like a real 3D renderer. Observed in the close shot: stronger material
separation on the pedestal and copper object; specular highlights that read;
substantially darker shadows with objects feeling more planted; terrain no
longer collapsing into a single flat ambient value; much stronger contrast
around the hero object; more perceived depth.

**The review corroborated the measurement rather than merely echoing it.** The
difference sheet showed changes concentrated around **actual geometry and
shadowed regions**, not a global exposure shift. That independently rules out
"we dimmed the frame and called it progress" and agrees with the measured sky
banding delta of exactly 0.00.

Answers to the three questions the metrics could not answer:

- **Q1 — did the shadow rework help or just darken?** *Helped.* Objects read as
  planted; shadows read as plausible. The policy removed a real
  representational limit rather than shifting exposure.
- **Q2 — first-noticed deficiency?** *The assets and environment.* Not
  lighting, not shadows, not post. This confirms the planned Phase C ordering
  and means **the renderer is no longer the bottleneck** — the frame is limited
  by what it is fed, not by how the image is formed. The strongest available
  signal that the foundation work landed.
- **Q3 — is the ~20% residual sky figure visible?** *No.* Consistent with the
  metric's view-independence argument.

**This is the sprint's most important qualitative result:** there is no longer a
foundational graphics seam left to prove. Remaining gains are content and
lighting work — a different and much better class of problem.

Evidence: `artifacts/parity/REVIEW_SCORECARD.md`, with sheets under
`artifacts/parity/review-full/` and `artifacts/parity/review-shadow/`.

---

## 8. Frame-cost attribution — **NOT CLOSED**, partial cost data recovered

### 8.1 What the certified receipt does (correctly) omit

Every frame-cost clock in `graphics_frame_receipt.json` reads **zero** and the
GPU clocks are `None`:

```
frame_time_us=0  prepare_us=0  scene_raster_us=0  resolve_us=0
overlay_us=0  flush_readback_us=0  all gpu_*_us = None
```

This is **deliberate and correct**. `deterministic_certification_frame_receipt`
zeroes the timing fields so identical scene/capture bytes produce identical
evidence. Timings must not perturb certification identity.

### 8.2 Correction to an earlier claim in this report

An earlier draft of this section asserted that the GPU clocks were dead for a
*second, independent* reason — that `_gpu_timestamp_capable` omitted the
`timestampComputeAndGraphics` limit. **That overstatement is withdrawn.**

A real capability probe (`GpuCapabilityProfile`, `_probe_gpu_capabilities`) now
queries the live device. On the RTX 5060:

```
device                         = NVIDIA GeForce RTX 5060
max_sampler_anisotropy         = 16.0
sampler_anisotropy_supported   = true
timestamp_compute_and_graphics = true
timestamp_period               = 1.0
timestamp_valid_bits           = 64
gpu_timestamp_capable          = true
```

The old two-term check (period > 0, valid_bits > 0) returns `true` here too. So
on this device it was **accidentally correct**; the omitted limit is a latent
portability bug, not the cause of anything observed. The probe fix is retained
because it is correct and now *demonstrated* rather than assumed, but it changes
nothing on this machine.

### 8.3 The real cost signal that does exist

`campaign2_visual_evidence.json` carries `measurements.gpu_frame_cost_us` per
view, outside the certified receipt. This is the sprint's "100ms-class number":

| arm | close | medium | wide |
|---|---|---|---|
| baseline/run-a | 12.97 s | 0.11 s | 0.12 s |
| ab-null3 | 14.23 s | 0.10 s | 0.38 s |
| ab-dither | 13.97 s | 0.10 s | 0.10 s |
| ab-shadow | 14.34 s | 0.10 s | 0.11 s |
| ab-full | 16.03 s | 0.18 s | 0.11 s |

Two things follow, and neither is yet attribution:

1. **First-frame cost is ~13–16 s, steady state ~0.10–0.11 s** — a ~120×
   ratio. The first view pays pipeline compilation and shader JIT. Whatever
   `gpu_frame_cost_us` measures, it is heavily contaminated by one-time cost on
   the close view, which is why close is not comparable to the others.
2. **Steady state is ~105 ms for a 960×640 frame of this workload**, and none of
   the retained candidates moves it materially. That is a large number for the
   shader load involved, but this measurement is an end-to-end wall figure that
   conflates raster, readback, fence wait, and JIT. **It cannot be decomposed
   with what exists today** — which is exactly the §2.3 request.

### 8.4 Required next slice

A **sidecar** bound to `(packet_sha256, capture_sha256)` but explicitly outside
certification identity and outside the receipt digest. Rules learned here:

- Carry only clocks that are genuinely measured. Of the ~17 fields such a schema
  would naturally declare, only 9 have a measurement behind them
  (`prepare`, `scene_raster`, `resolve`, `overlay`, `flush_readback`, `frame`,
  and four GPU counterparts). There is no `cpu_submit_us`, `cpu_present_us`,
  `queue_wait_us`, `fence_wait_us`, `gpu_shadow_us`, `gpu_post_us`,
  `gpu_compute_us` today.
- Every field carries provenance (`measured` / `unavailable` / `not_implemented`)
  so a zero is never ambiguous between "it was free", "it was not measured", and
  "it does not exist". Without this the sidecar becomes the
  `dispatch_calls = 0` failure wearing a new hat.
- Attribute the ~105 ms before touching any shader. Per §2.3, no shader
  optimisation may begin until then.

Also confirmed: `dispatch_calls = 0` is a **literal** in `LavaAdapter.jl`, not a
measurement.

---

## 9. Determinism / replay results

- Null-policy control arm: 3/3 captures byte-identical, 3/3 packet digests
  identical, re-verified after shadow + refusal plumbing.
- Dither is **ordered**, not hashed or temporal — the certified frame stays a
  pure function of the packet, not of a counter.
- Policy absence is byte-identity **by construction** (`Option` +
  `skip_serializing_if`), and asserted against a *committed* artifact rather
  than a fixture the test also built, so a leak into canonical JSON cannot pass.
- Every policy value is digest-bound: presence of a policy changes the packet
  digest (`presence_actually_changes_the_digest`).
- Tier A / Tier B distinction preserved: nothing temporal was introduced.

---

## 10. Full test / workspace gate results

**`cargo test --workspace`: 342 passed, 0 failed, across 64 test binaries.**

`cargo test --workspace` — **the gate caught a real break**, vindicating the
GraphicsTelemetry standing lesson. Four test files
(`deformation`, `real_asset_composition`, `scene_composition`, `visual_quality`)
and one Julia test constructed `GraphicsScenePacketBody` / `GraphicsScenePacket`
literally and failed to compile on the new `render_policy` field. All fixed.

Targeted suites:

| suite | result |
|---|---|
| `cargo test -p wge-native-graphics-contract --test render_policy` | **10 passed, 0 failed** |
| `graphics_lab/test/render_policy.jl` | **all testsets green** (121 assertions) |
| `graphics_lab/test/lava_adapter.jl` | **all testsets green** (103 assertions, incl. 26 GPU adapter, 35 projection, 12 terrain albedo) |

### 10.2 Re-verification after F-4/F-5 (sampler-per-surface + UV semantics)

| check | result |
|---|---|
| `cargo test -p wge-native-graphics-contract --test render_policy` | **13 passed, 0 failed** (was 10; +3 new) |
| `cargo test --workspace` (3rd run) | **345 passed, 0 failed**, 64 binaries (was 342) |
| strict Julia parse gate, `LavaAdapter.jl` + `WGEGraphics.jl` | **PARSE OK** |
| `ab-null5` vs frozen baseline, 3/3 captures | **BYTE-IDENTICAL** |
| `ab-null5` comparator, `--mode control` | **PASS**, sky band delta `+0.00` on all views |

**A regression was reported during this gate and withdrawn.** An early `cmp`
reported all three views as differing. That was a harness error of mine — the
comparison ran while the render chain was still writing the files, so it read
partial output. Re-run after the chain completed: byte-identical, and the
comparator independently reported `0.0000%` changed pixels. No code defect;
recorded because "the gate fired and it was nothing" is still information.
| strict Julia parse gate on `LavaAdapter.jl` | **PARSE OK** |

An over-eager `sed` also injected `render_policy: None` into a
`GraphicsTelemetry` literal in `visual_quality.rs`. Caught by the compiler and
removed — `GraphicsTelemetry` deliberately gains **no** policy field, because
telemetry the producer cannot independently verify is exactly the class of
decoration the sprint forbids.

### 10.1 Re-verification after the capability-probe work

The capability probe changed adapter startup and the policy-refusal path, so the
frozen baseline was re-proven rather than assumed:

| check | result |
|---|---|
| `ab-null4` vs frozen baseline, 3/3 captures | **BYTE-IDENTICAL** |
| `ab-null4` comparator, `--mode control` | **PASS** (0.0000% changed, all views) |
| `ab-null4` sky banding | 98.37 / 100.00 / 100.00 — reproduces the baseline exactly, and correctly **FAILS** the banding gate because the control arm carries no dither |
| `cargo test --workspace` (2nd run) | **342 passed, 0 failed**, 64 binaries |
| `graphics_lab/test/render_policy.jl` | green, incl. 11 anisotropy-diagnosis + 5 timestamp-gate assertions |
| `graphics_lab/test/lava_adapter.jl` | green, incl. 26 GPU adapter tests |

---

## 11. StylePlan executor status — **LANDED (spine only)**

`RenderPolicy` is a real, typed, validated, digest-bound execution channel with
seven sub-policies. Defaults are declared in Rust **and** Julia and cross-checked
by test. Supported axes: grade, bloom, vignette, dither, terrain surface,
sampler anisotropy, shadow.

Five of the seven are genuinely executed. Two (terrain surface, anisotropy) are
**refused** rather than silently ignored. Provenance, digest binding, range
validation, closed field set, serde round trip, and unsupported-axis refusal are
all tested.

Not attempted: mapping from actual `StyleProfile` values into `RenderPolicy`.
The spine exists; the upstream signal mapping does not.

---

## 12. MeshPacket v8 status — **NOT ATTEMPTED**

No runway. No schema change, no fixture, no version rules.

---

## 13. Compute-path status — **NOT ATTEMPTED**

No compute pipeline introduced. `dispatch_calls` remains the literal `0` it was
already known to be.

---

## 14. Remaining S1/S2 gaps

1. ~~**Human visual review** of the `full` arm.~~ **CLOSED** — retained; the
   renderer is no longer the perceived bottleneck (§7).
2. **Frame-cost attribution sidecar** — blocks §2.3 and all shader optimisation.
   Must not ship fields without measurements (F-1c: ~105 ms steady state is
   real but undecomposable today).
3. **Depth capture contract** (GP-…) — `grounding_contact` is still permanently
   indeterminate; without depth, shadow/contact work cannot be *measured*.
4. **Anisotropy** — needs the upstream Lava device-feature change.
5. ~~**Terrain tiling**~~ — **RESOLVED** (F-4/F-5). Anisotropy remains the one
   blocked sampling axis, pending the Vulkan device-feature change.
6. **MeshPacket v8**, compute path, local lights, atmosphere, IBL — entire tiers
   untouched.
7. **§3.7 recomposed frame** — the wide view still has only 60 % content coverage.
8. **§3.2 real texture conditioning** — a coherent per-family material set now
   exists and is measured; it is still **256-class procedural, not scanned
   production assets**, and its highlight fraction sits ~4x below baseline.
   Human review pending.

---

## 15. New defects / friction found this sprint

| # | Finding | Severity |
|---|---|---|
| F-1 | Certification receipt deliberately zeroes all timing → attribution must come from a sidecar | high (blocks §2.3) |
| F-1b | `_gpu_timestamp_capable` omitted the `timestampComputeAndGraphics` limit — **latent portability bug only**; verified true on this device via a live probe, so it caused nothing observed | low (fixed) |
| F-1c | Steady-state `gpu_frame_cost_us` ≈ 105 ms and first-frame ≈ 13–16 s; neither is decomposable with current instrumentation | high |
| F-2 | `dispatch_calls` is a literal `0`, not telemetry | high |
| F-3 | Lava pins a device without `samplerAnisotropy`; anisotropy is unsafe to enable | high (blocks §3.4) |
| F-4 | Material sampler cache keyed by material, not surface → wrap mode cannot be scoped per surface | **RESOLVED** (sampler-per-surface) |
| F-4b | `_lava_texture2d!` accepted `filter`/`wrap` keywords and silently ignored them, making per-texture sampling look already-solved | **RESOLVED** (dead kwargs removed) |
| F-5 | `uv_scale_milli` documented as "repeats/metre"; actual semantics are "repeats across the whole field" | **RESOLVED** (renamed + derived helper) |
| F-6 | `powf(x, 1.0)` is not bit-exact; identity grade must short-circuit | medium (already fixed, worth remembering) |
| F-7 | Lava shader JIT cannot lower loops with constant bounds (`LLVMICmp` in `ConstantExpr`) | medium (all taps must stay unrolled) |
| F-8 | Host-side execution of any shader calling `sample_texture_2d` segfaults | medium (forces GPU-only coverage) |
| F-9 | Rebuilding the binary mid-render trips the worker source-identity guard | low (harness friction; guard is correct) |
| F-10 | All 11 materials shared one 8x8 normal / near-constant roughness / near-constant AO set — a *semantic* material defect, not a resolution one | **RESOLVED** (per-family coherent sets) |
| F-11 | Octave seed derivation `octave * 0x9e3779b9` overflows `u32` from octave 2: debug panics, release wraps silently, so seeds differ between profiles — a determinism bug disguised as an optimisation | **RESOLVED** (`wrapping_mul`) |
| F-12 | The wet ripple used `(u * 12.0).sin()`, which is 1.91 turns across the tile and therefore did not tile. Caught by a property test, not by looking | **RESOLVED** (integer turn count) |
| F-13 | The hero material swap ran before the campaign2 materials were appended, so it rewrote them and was overwritten: **24 textures shipped that nothing referenced** — payload with no effect, the exact decorative-schema failure this sprint exists to prevent | **RESOLVED** (moved to last mutation before seal) |

---

## 16. Exact next recommended slice

**Ship a frame-cost sidecar, then re-open Tier 1.**

1. Emit the raw CPU/GPU pass timings the adapter already computes into a
   sidecar artifact that is outside the receipt digest and outside
   certification identity. Do **not** touch `deterministic_certification_frame_receipt`.
2. Re-run the §2.3 matrix — instrumented/uninstrumented × readback/no-readback ×
   present/no-present — and produce the actual attribution table.
3. Only then, and only with attribution in hand, begin shader work.

Rationale: §2.3 exists precisely to stop optimisation against clocks whose
meaning is unknown. F-1 shows the clocks currently mean *nothing* in the
certified path. Building Tier 2 (IBL, cascades, local lights) on top of that
would produce cost regressions nobody could detect — which is the exact failure
mode this sprint was called to exit.