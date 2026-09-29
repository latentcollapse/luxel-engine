# WGE Native Graphics Handoff

Status: green engine-neutral Lava checkpoint, 2026-09-28.

This checkpoint makes Lava the supervised native graphics backend for WGE's
engine-neutral path. Rust remains semantic and receipt-promotion authority;
Julia owns typed numerical/GPU execution; Lava owns Vulkan machinery behind
the Julia adapter. Python is not semantic authority.

## Frozen identities

- Scene packet: `wge.graphics-scene-packet/v6`.
- Frame receipt: `wge.graphics-frame-receipt/v1`.
- Adapter: `wge.lava-adapter/v5`.
- Lava: `11c7e31bdf62408d22bf379e9e59510f69d2103e`.
- Vulkan.jl: `03b4ca2351477ccbb8ee378f512da50f7eec7bac`.
- VulkanCore.jl: `1d02829e8fa92da430d879db4dd7bf564a872035`.
- Raycore.jl: `d93743b3ac0e8462ac8f9ba26d082f5402b4629d`.

The packet boundary validates terrain buffers, meshes, UVs, normals,
transforms, cameras, lights, materials, textures, provenance, and capture
requests in both Rust and Julia. The native opaque material path now includes
bounded metallic/roughness and clearcoat response with digest-bound albedo,
normal, roughness, occlusion, and emissive roles.

## Evidence

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

Rigging/skinning/retargeting, Unity import/build/playthrough, and the supplied
bad GLB remain deferred or negative controls. Tetrahedral Cage RT is a
research-only roadmap flag; the AMD-paper reconstruction, CPU reference,
Lava/Vulkan prototype, comparison metrics, watertightness investigation, and
Rust-owned materialization policy must precede any TetCageRT promotion.

Do not weaken the Rust promotion gate or begin Unity, rigging, multiplayer,
editor, or broad post-checkpoint work from this handoff.
