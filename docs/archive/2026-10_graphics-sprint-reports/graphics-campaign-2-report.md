# WGE Graphics Campaign 2 — The Authored Frame

Status: **green** for the Campaign 2 authored-frame checkpoint, 2026-09-29.

This report records the final result of the graphics-only campaign. The goal was
to move the certified but primitive native renderer to its first coherent
authored scene while preserving Rust authority, Julia/Lava packet ownership,
determinism, provenance, restart behavior, and fail-closed validation.

This is not an AAA claim, imported-asset parity claim, character claim, or
external-engine comparison. It is a reusable calibration slice for an
autonomous model-generated game pipeline.

## Scope and outcome

The final command was:

```text
world_core/target/debug/wge-native-graphics-contract render-campaign2-layout \
  world_core/crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia terrain_lab graphics_lab \
  graphics_lab/bin/wge_graphics_worker.jl artifacts/campaign2/live-twelfth
```

The command produced three fixed inspection cuts over the certified Riverwatch
world:

- close material/hero inspection;
- medium terrain/environment inspection;
- wide world-composition inspection.

Every cut passed the Rust-registered `campaign2-authored-frame` profile and
the independent Campaign 2 vector-evidence validator. The clean replay also
passed in a fresh process.

## Baseline freeze

Before Campaign 2 source changes, the adapter-v6 `render-quality-layout`
checkpoint was replayed twice from clean supervised runs. The frozen baseline
is [`artifacts/campaign2/CAMPAIGN2_BASELINE.md`](../../../artifacts/campaign2/CAMPAIGN2_BASELINE.md)
and is retained under
`artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/`.

Both baseline runs matched all seven certification artifacts. The baseline
world identity was `world-5f883ff69a9888951c446d86be231e9c52b2df3a8c5162d9fff221095a11ab6d`.
The frozen capture/evidence identities were:

- capture PPM: `sha256:192e673b7587f20d05871cbb6fc80538d124e1f43ca7d102e7c3a9353f56dd83`;
- raw capture: `sha256:29eda4cd5b32a579fbf3957f20223e0130850d520d00a99cfb4960762e4c5a54`;
- visual evidence: `sha256:ea4f4eca05532659164177091ea9a4f1369f54344d8f5f41fda3b59123733f63`;
- deterministic frame receipt: `sha256:bddfb7bec9b05649dbc83ec1f658120c929571b8cbb894e0ddb35cb8ccf0c8be`;
- scene packet: `sha256:a65274feffb3d3f2e2c5e454de680b65d7bffb8810a60e7a4b344a3d15ec23cc`;
- renderer attestation: `sha256:0f496df5643533681e9734968a0f4f8a63ea12b8700670ab59cbfa1f8128ca66`.

## What changed

### Rust-owned authored projection

`lower_campaign2_packet` was added to the native graphics contract. It:

1. validates the source packet and requires the certified objective anchor;
2. binds the derived packet to the source world, layout, spatial fields, seed,
   and capture contract;
3. clears diagnostic source render instances from the calibration composition;
4. adds deterministic authored meshes, textures, materials, instances, fixed
   cameras, environment intent, and light intent;
5. seals the result through the existing packet validator and supervisor
   allow-list.

The source semantic instances are not deleted from the `WorldArtifact`; they
remain covered by the world/runtime gates. The visual projection cannot mutate
canonical semantic state.

### Visual-quality authority

The Rust `campaign2-authored-frame` profile is closed and registered. It checks
terrain/content coverage, sample count, luma/RGB structure, spatial edges,
tile variation, overlay exclusion, and authored-frame dimensions.

`wge.campaign2-visual-evidence/v1` adds a multidimensional vector with:

- measured: silhouette/readability, material separation, composition, texture
  frequency, density, artifact rate, frame cost, GPU cost, upload/readback
  memory, and visible/total instances;
- indeterminate: grounding/contact, lighting consistency, and atmospheric
  depth, because the current path has no independently promoted depth/contact
  classifier, reference-light comparison, or deterministic atmospheric judge.

`validate_campaign2_visual_evidence` now revalidates both the registered visual
quality evidence bound to the deterministic certification receipt and the
runtime frame receipt. A producer status or runtime-only vector cannot promote
the frame.

### Graphics calibration

The authored slice now uses:

- higher-tessellation, smooth-normal radial hero geometry;
- smooth ellipsoid foliage crowns with asymmetric lobes and explicit trunks;
- conditioned terrain albedo with macro/medium/fine variation;
- a distinct wet/reflective puddle with two deterministic glint rings;
- typed metallic/roughness/clearcoat material roles with digest-bound texture
  payloads;
- the existing linear HDR target, 2× resolve, directional shadow map, and
  environment-lighting path.

The Julia audit also reproduced and fixed the instance-bound culling error in
`_instance_world_bound`: rotation and vector arguments had been passed in the
wrong order. A regression test now proves that a rotated instance bound follows
the instance quaternion.

## Final measurements

The following are the clean replay values from `live-eleventh`; the final
current-binary run was repeated as `live-twelfth` and produced the same
deterministic artifacts. CPU frame time includes the supervised adapter call
and readback; GPU time is the optional Vulkan timestamp interval. The first cut
includes lazy device/pipeline work.

| View | Packet | Capture | Terrain / content bp | Luma span | RGB bins | Edge bp | CPU frame | GPU frame | Upload / readback | Visible / total |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| close | `db612992…085d` | `57e4b618…5260` | 4,757 / 7,104 | 122 | 527 | 515 | 12.658 s | 12.485 s | 2,701,693 / 1,572,864 B | 22 / 34 |
| medium | `4ec1168b…7032` | `4a37de53…e34fa` | 4,669 / 5,765 | 86 | 475 | 293 | 0.300 s | 0.106 s | 2,703,037 / 2,457,600 B | 34 / 34 |
| wide | `5c1ae0b3…95ba` | `0e166637…c00` | 3,182 / 3,831 | 80 | 450 | 328 | 0.438 s | 0.271 s | 2,703,037 / 2,457,600 B | 34 / 34 |

Full identities are recorded in the benchmark and handoff documents and in
each view directory. All three outcomes were `good`.

## Determinism and replay

`live-eleventh` and `live-twelfth` were independent fresh-process runs. For
each view, these matched byte-for-byte:

- `world_artifact.json`;
- `graphics_scene_packet.json`;
- `graphics_frame_receipt.json`;
- `graphics_renderer_attestation.json`;
- `native_capture.rgba` and `native_capture.ppm`;
- `visual_quality_evidence.json`.

`campaign2_visual_evidence.json` is intentionally runtime-bound: it includes the
non-deterministic runtime receipt and timing telemetry. Its digest differs
between runs by design and is not used as deterministic certification identity.

## Verification record

Green gates after the final source changes:

- `cargo fmt --all` — pass;
- `cargo clippy --offline -p wge-native-graphics-contract --all-targets -- -D warnings` — pass;
- `cargo check --offline -p wge-native-graphics-contract` — pass;
- full `cargo test --offline -p wge-native-graphics-contract --lib --tests` —
  pass: 18 unit, 2 main, 5 live, 7 native graphics, 9 visual quality, and 6
  window tests;
- `cargo test --offline -p wge-certification-authority --test
  visual_quality_authority` — pass: 5/5;
- `julia --project=graphics_lab --startup-file=no graphics_lab/test/runtests.jl`
  — pass, including the rotated-bound regression;
- two fresh live Campaign 2 GPU runs — pass, all three views each;
- deterministic replay comparison — pass for every certification artifact.

The normal host Vulkan loader emitted the known `liblsfg-vk-layer.so` skip
warning. It is an environment loader note, not a validation failure.

## Known limits and regressions

- The Campaign 2 hero is deterministic procedural authored geometry, not an
  imported hero asset with source-asset identity, production conditioning, or
  mesh/texture streaming.
- Foliage is opaque and clustered. Alpha testing, wind, ecological placement,
  impostors, LOD, occlusion, and residency are still absent.
- The environment is analytic sky/horizon/ground lighting, not prefiltered IBL
  or reflection probes. The puddle is a bounded reflective material probe, not
  water simulation.
- The shadow path is one fixed directional map with bounded PCF. Contact,
  cascaded, and soft-shadow evidence remain open.
- The scene remains small. Cold startup is expensive and the CPU/GPU interval
  does not yet explain the full wall-time distribution for a production budget.
- The three vector axes listed as indeterminate remain indeterminate. No visual
  status was fabricated to make the report look more complete.
- Rigging, skinning, retargeting, arbitrary mesh-to-character generation,
  TetCageRT, Unity, and post-campaign breadth were not started.

Known rejected paths preserved by the campaign:

- leaving legacy diagnostic instances in the authored composition produced a
  visually contaminated frame; those instances remain semantically exercised
  but are fenced from this inspection projection;
- a one-shot terrain slope/contrast remap was not retained after its measured
  spread regressed; terrain variation remains a typed texture/material problem;
- a wholesale deferred-renderer rewrite was rejected in favor of the existing
  linear HDR, shadow, texture, and resolve path.

## Exact next frontier

The next graphics slice should be one coherent asset/material and lighting
advance, in this order:

1. typed imported-asset/source identity and texture conditioning for a real hero
   asset, retaining the bad GLB as a rejection control;
2. prefiltered environment lighting/reflection probes with independent evidence;
3. depth-backed contact/soft/cascaded shadow evidence;
4. semantic terrain layer mapping and richer authored surface transitions;
5. production foliage alpha/wind/LOD/residency and denser authored population;
6. temporal reconstruction and a deterministic visual critic for the currently
   indeterminate axes.

Do not begin rigging, TetCageRT materialization, Unity integration, multiplayer,
or broad post-campaign features from this report.

## Artifact locations

- final current-binary run: `artifacts/campaign2/live-twelfth/`;
- clean replay: `artifacts/campaign2/live-eleventh/`;
- prior campaign iteration: `artifacts/campaign2/live-tenth/`;
- screenshots and handoff copies:
  `/home/mattc/Pictures/WGE/campaign2-2026-09-29/`;
- baseline freeze: `artifacts/campaign2/CAMPAIGN2_BASELINE.md`;
- architecture: `docs/platform/native-graphics-architecture.md`;
- benchmark: `docs/world/native-graphics-benchmark.md`;
- quality gaps: `docs/world/native-quality-gaps.md`;
- handoff: `docs/archive/2026-09_native-graphics-checkpoints/native-graphics-handoff.md`.
