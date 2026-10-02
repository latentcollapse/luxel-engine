# WGE Native Graphics Handoff

Status: adapter-v6 adversarial audit, Campaign 2 authored-frame checkpoint, and
C2.5 imported-asset inspection checkpoint closed; native engine-neutral graphics
is green, while broader production visual quality remains an explicitly measured
gap, 2026-09-30.

This checkpoint makes Lava the supervised native graphics backend for WGE's
engine-neutral path. Rust remains semantic and receipt-promotion authority;
Julia owns typed numerical/GPU execution; Lava owns Vulkan machinery behind
the Julia adapter. Python is not semantic authority.

The current Rust path also has a bounded imported-asset seam:
`RenderAssetPackage` -> `GraphicsAssetProjection` -> optional
`SceneArtifact` render binding -> `compose_bound_scene` -> v6 packet. This is
identity-checked composition machinery. Tangent-aware imported shading now
crosses the canonical packet boundary, but production texture streaming and
full asset LOD/collision lowering remain open.

The C2.4 real-asset integration slice remains preserved at
`/home/mattc/Pictures/WGE/c2.4-real-asset/`. C2.5 extends it with canonical
tangent carry-through, an explicit single-level mip policy, a Rust-owned close
camera, and a real 640x480 close/context capture. The permanent assembled
log-hut GLB now renders through that path, the worker is restarted, and the
warm-state close capture plus deterministic certification receipt replay
byte-identically. The C2.5 handoff and evidence bundle are recorded in
`WGE_C2_5_IMPORTED_ASSET_INSPECTION_HANDOFF.md` and
`/home/mattc/Pictures/WGE/c2.5-imported-asset/`. This remains a certified
static-asset inspection slice, not a claim of production visual quality.

## Frozen identities

- Scene packet: `wge.graphics-scene-packet/v6`.
- Frame receipt: `wge.graphics-frame-receipt/v1`.
- Adapter: `wge.lava-adapter/v6`.
- Lava: `11c7e31bdf62408d22bf379e9e59510f69d2103e`.
- Vulkan.jl: `03b4ca2351477ccbb8ee378f512da50f7eec7bac`.
- VulkanCore.jl: `1d02829e8fa92da430d879db4dd7bf564a872035`.
- Raycore.jl: `d93743b3ac0e8462ac8f9ba26d082f5402b4629d`.

The packet boundary validates terrain buffers, meshes, UVs, normals,
transforms, cameras, lights, materials, textures, provenance, and capture
requests in both Rust and Julia. The native opaque material path now includes
bounded metallic/roughness and clearcoat response with digest-bound albedo,
normal, roughness, occlusion, and emissive roles.

Rust owns promotion authority: a frame cannot be promoted without a separately
validated world artifact bound to the packet, independently remeasured capture
bytes, spatially supported visual variation, and a color-only capture contract.
The Julia worker's direct packet operation is shape checking only; it is not a
certification receipt. Depth attachments remain internal until a versioned
depth-evidence payload is implemented.

The ready handshake validates worker and source identity but intentionally does
not initialize the Vulkan device. `capabilities()` is mandatory before frame
promotion and owns the startup/device-probe deadline; the first promoted frame
may still pay lazy scene-pipeline compilation under the same 120-second
response bound. A timeout fences the transport and requires an explicit
restart, so slow initialization cannot become a pass-shaped partial receipt.

The certification orchestrator now runs `render-quality-layout` through the
provenance-bound `render-world-showcase-layout` composition and independently
promotes its packet, frame receipt, raw capture, and visual-quality evidence.
The showcase is WGE-owned inspection machinery bound to the authored world; it
is not an external-engine comparison or a claim of production visual parity.
The gate remains fail-closed when the measured scene misses any technical
floor.

## Strict technical visual-quality checkpoint

The current `native-world-showcase-v1` capture passes the registered Rust
profile after independent remeasurement:

- packet: `sha256:a65274feffb3d3f2e2c5e454de680b65d7bffb8810a60e7a4b344a3d15ec23cc`;
- deterministic certification frame receipt: `sha256:bddfb7bec9b05649dbc83ec1f658120c929571b8cbb894e0ddb35cb8ccf0c8be`;
- promoted frame capture: `sha256:29eda4cd5b32a579fbf3957f20223e0130850d520d00a99cfb4960762e4c5a54`;
- visual-quality evidence: `sha256:ea4f4eca05532659164177091ea9a4f1369f54344d8f5f41fda3b59123733f63`;
- renderer attestation: `sha256:0f496df5643533681e9734968a0f4f8a63ea12b8700670ab59cbfa1f8128ca66`;
- terrain coverage: `2,281 bp`; authored-geometry coverage: `28,269` pixels;
- combined content coverage: `3,000 bp`; spatial edges: `576 bp`;
- luminance bins/RGB bins: `28/321`; varied tiles: `31/64`.

Terrain coverage is measured independently from authored geometry. Props cannot
masquerade as terrain, but legitimate authored geometry contributes to the
content-level color, edge, and tile measurements. The screenshot and evidence
sidecar are preserved at
`/home/mattc/Pictures/WGE/native-world-showcase-certified-2026-09-29.png`.

A fresh non-fixture `cedar_saddle_relay` layout also passes the same registered
profile after the generic terrain-material revision: `2,571 bp` terrain
coverage, `3,255 bp` combined content coverage, `529 bp` spatial edges, `28/314`
luminance/RGB bins, and `33/64` varied tiles. Its evidence is preserved at
`/home/mattc/Pictures/WGE/cedar-world-showcase-certified-2026-09-29.png`.

## Campaign 2 handoff

The first authored-frame slice is green through the same authority path. Run
`render-campaign2-layout` to produce the fixed close, medium, and wide views.
The final current-binary artifacts are in `artifacts/campaign2/live-twelfth/`,
with the clean replay in `artifacts/campaign2/live-eleventh/`; the viewable
handoff is `/home/mattc/Pictures/WGE/campaign2-2026-09-29/`.

The scene is a Rust-sealed projection over the certified world, with semantic
source instances fenced from the calibration composition but retained in the
world artifact. It contains conditioned terrain, a smooth procedural hero asset,
clustered foliage, a wet reflective puddle, directional shadows, environment
lighting/fog, fixed cameras, and a registered multidimensional visual vector.
The vector’s grounding/contact, lighting-consistency, and atmospheric-depth
fields remain indeterminate until their evidence machinery exists.

Certification-relevant identities from the clean replay are:

- close packet `sha256:db6129922c0cbbcbc7cdbe8be478b7ac899e0955f0b5d6e6381a1931781c085d`, capture `sha256:57e4b618b4f7e19c8af41ac974433da95610a267f5505abff792db1146115260`;
- medium packet `sha256:4ec1168bdbb396055cfb399e612a331650b65797a8b949d084358500aee37032`, capture `sha256:4a37de535b6ae81cebd5d4f50157f524da59f0ff3ab1ea25046a33cbe87e34fa`;
- wide packet `sha256:5c1ae0b334adc527181cb35c6c073e02cf08a442b67c8b656c0eee8f2a5295ba`, capture `sha256:0e1666373754589e2ed5e7c45350f78ca24057b72fcfc90b6db4b4f733af2c00`.

This closes the campaign checkpoint, not the full production renderer. The
remaining frontier is recorded in `WGE_NATIVE_QUALITY_GAPS.md` and the campaign
report; do not treat this procedural hero as imported-asset or character parity.

## Current adapter-v6 evidence

Fresh-process Rust-supervised captures on 2026-09-29 all passed independent
measurement and promotion:

- canonical overview: packet
  `sha256:07f2a88c4acb0cba16ac45db0fd3fb1a3a7bb8fe7aa81a99da6365c6e1cd486a`,
  capture `sha256:066963d333fe3b7da42204ad27d3603c79ea889c53d268556558e438dfbb6081`;
- objective close: packet
  `sha256:cd4234d1ad807dbedc72157cf1f5671d59bb46cef3052097804b0c15915cee5c`,
  capture `sha256:6c83607c720222b75a4928cf30d7549ebcd2dcfca39b00b96c9df0ce028e1f9a`;
- material showcase: packet
  `sha256:bd96a891e590b84aec8e61f9a27a01efc81bd98c09338df28235fa82c2ad119b`,
  capture `sha256:fda34e0e82898ae29505f854b3374099e11ebf77ca92643e8b5c0056a3790332`;
- composed world showcase: packet
  `sha256:ab8162df9af469c48da81747fa9c2691230c50c2ea31be10a201698e17139183`,
  capture `sha256:7d11a5caecc99c4f0b03f1f83f02105c938c1bee901ab59843d98172f54934b1`.

The overview receipt measured 10/10/63/14 visible pixels for player,
opponent, encounter, and objective markers respectively. The native integration
suite also replayed the overview after a clean worker restart with byte-identical
capture bytes.

## Historical evidence

The hashes below are the prior adapter-v5 checkpoint and remain useful
historical regression controls. Current adapter-v6 certification evidence is
listed above.

- Canonical gameplay/world capture: packet
  `sha256:b7f7b3b8b8c3b4d0589f98bcea516f7f4c464bea7a45da000c06b0cda78e9a63`,
  capture `sha256:9e29309aa8a8afd1ea79e9a73278f8b0659b3efdd11ee8d6415a175df2c2f043`.
- Material showcase: packet
  `sha256:dac4f498b67e09239c9c2b34c794dc40c8d8d32832fe4f23cc9fee27cf96a983`,
  capture `sha256:e8f960e61145f44c5803a24cfe5e3c2d9b19ac6d4df0dc59c04a2e1dd2172b27`.
- Composed-world stress profile: packet
  `sha256:28a91341960a2b67057a5266f0c538f0ca1510bd344d8f6508a56fba987cb2b5`,
  capture `sha256:e68989b5d54a73117fbdc4da7829a27ee3edb6802f67b3c5ca13a1880fd02e33`.

All three receipts were decoded, hashed, remeasured, and promoted by Rust.
The persistent worker has a bounded response deadline; a timeout fences and
tears down the transport, and reuse requires an explicit restart.

## Reproduction

From `world_core`:

```sh
cargo test -q -p wge-native-graphics-contract --lib -- --test-threads=1
cargo test -q -p wge-native-graphics-contract --test native_graphics -- --test-threads=1
/home/mattc/.juliaup/bin/julia --project=graphics_lab --startup-file=no graphics_lab/test/runtests.jl
/home/mattc/.juliaup/bin/julia --project=graphics_lab --startup-file=no graphics_lab/test/lava_adapter.jl
```

The render commands and benchmark commands are collected in
`WGE_NATIVE_GRAPHICS_BENCHMARK.md`. The normal host emits a known
`liblsfg-vk-layer.so` loader warning; Vulkan skips that layer and the native
validation gates remain green.

## Explicit limits

The current path does not claim imported hero-asset parity, production
foliage/terrain, alpha-tested vegetation, LOD/meshlets/streaming/occlusion,
prefiltered IBL, cascaded/contact/soft shadows, temporal AA, water, particles,
post-processing, skeletal rendering, or arbitrary mesh-to-character creation.
Those gaps are measured in `WGE_NATIVE_QUALITY_GAPS.md`.

Rigging/skinning/retargeting and the supplied bad GLB remain deferred or
negative controls. Tetrahedral Cage RT is a
research-only roadmap flag; the AMD-paper reconstruction, CPU reference,
Lava/Vulkan prototype, comparison metrics, watertightness investigation, and
Rust-owned materialization policy must precede any TetCageRT promotion.

Do not weaken the Rust promotion gate or begin rigging, multiplayer, editor, or
broad post-checkpoint work from this handoff.
