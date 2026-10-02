# WGE C2.5 Imported-Asset Inspection Handoff

Status: **green checkpoint**, 2026-09-30.

This closes the bounded C2.5 slice of the native engine path. The permanent
assembled log-hut GLB now passes through Rust-owned conditioning, the canonical
scene/object bridge, Rust packet composition, Julia/Lava Vulkan rendering, Rust
receipt promotion, worker restart, and deterministic replay. This is a static
imported-asset inspection proof; it is not a claim of production visual quality
or a complete game renderer.

## What changed

- Imported tangent streams are now carried in the canonical Rust `MeshPacket`
  and parsed/consumed by Julia/Lava. Procedural packets retain an explicit,
  deterministic fallback.
- Texture policy is typed. The current v1 payload is explicitly
  `single_level_explicit`; a request for CPU-generated mip chains is rejected
  until a versioned multi-level payload contract exists.
- Rust can derive a deterministic imported-asset close camera without mutating
  semantic scene state. The authority supervisor independently recomposes the
  bound packet, including that camera, before promotion.
- The GPU replay test now reproduces the same warm-state sequence on both
  sides: context frame, close frame, worker restart, context frame, close
  frame. Structural telemetry is therefore part of the honest deterministic
  replay identity rather than being weakened or discarded.

## Green evidence

GPU proof:

```sh
WGE_C2_4_CAPTURE_DIR=/home/mattc/Pictures/WGE/c2.5-imported-asset \
cargo test --manifest-path world_core/Cargo.toml --offline \
  --package wge-native-graphics-contract \
  --test real_asset_composition -- --ignored --nocapture
```

The capture-directory variable retains the historical `C2_4` spelling in the
current test harness; the output path and evidence are C2.5-specific.

Result: `1 passed; 0 failed`, NVIDIA GeForce RTX 5060, 408.69 seconds. The
worker was restarted between the two close renders. Close-frame raw RGBA,
capture digest, and deterministic certification receipt all matched exactly.

Focused Rust tests, formatting, and clippy were green before the GPU proof:

```sh
cargo fmt --manifest-path world_core/Cargo.toml --all -- --check
cargo test --manifest-path world_core/Cargo.toml --offline \
  --package wge-native-graphics-contract \
  --test real_asset_composition -- --nocapture
cargo clippy --manifest-path world_core/Cargo.toml --offline \
  --package wge-native-graphics-contract --all-targets -- -D warnings
```

The Julia graphics parser and persistent Lava adapter suites were also green,
including tangent-basis coverage and the restart-capable adapter tests.

## Certified identities

The close packet uses camera `real-asset-close` at `640x480`.

| Evidence | Identity |
| --- | --- |
| close packet digest | `sha256:e87925ec70b7bf6643420cbbfcd1ad951be017104ba674fdcee573a3a5c5c693` |
| close capture digest | `sha256:d21c82a92dae3f05b0613c1059f47de6a919bbdf8fa61e58bb76eabfc1b8ff73` |
| close deterministic receipt | `sha256:fe6d2b707c95210a8bcd418b99be6a6d7261d06fc73d73df9941396a86c75db2` |
| replay packet digest | same as close |
| replay capture digest | same as close |
| replay deterministic receipt | same as close |
| scene artifact | `sha256:19a491d4e29450c1d5983309f17bb4298ae7a68489f06a955df65082cc49d05b` |
| world artifact | `sha256:5f883ff69a9888951c446d86be231e9c52b2df3a8c5162d9fff221095a11ab6d` |
| renderer attestation | `sha256:f87435a5a70169b71e3924191632f42b5548dbf506979908ef907ee8acd0c48a` |

The promoted close frame reports 32 draw calls, 19,392 mesh vertices, 17
instances (16 visible), and 7,512,881 upload bytes. The context warm-up frame
retains its own packet and receipt in the same bundle; it is deliberately not
substituted for the close evidence.

Bundle:

`/home/mattc/Pictures/WGE/c2.5-imported-asset/`

Visual captures:

- [close inspection](</home/mattc/Pictures/WGE/c2.5-imported-asset/native_capture.png>)
- [context composition](</home/mattc/Pictures/WGE/c2.5-imported-asset/context_capture.png>)

The known-good source fixture is the five-mesh assembled log hut:
`sha256:9560590b27ca1b847cc4b96f7659e99acf5b8fb18622e80ef0b4c2ae7ffd068f`.
The supplied malformed GLB remains a permanent rejection control and was not
promoted.

## Honest visual boundary

The close view gives the imported hut visual priority and proves real textured
geometry, normals/tangents, material roles, depth testing, and shadows through
the native path. It still sits inside the existing calibration shrine: the
platform, columns, beacon, and sparse terrain remain visually louder than a
finished authored environment. The frame is therefore a useful inspection
checkpoint, not a production showcase.

Still open after this checkpoint:

- versioned multi-level texture/mip payloads and texture transforms;
- collision and LOD policy carried through the imported render package;
- prefiltered IBL, stronger contact/cascaded shadows, foliage, atmosphere, and
  live window/gameplay rendering;
- character rigging, skinning, retargeting, TetCageRT, and external-engine
  integration, which remain explicitly deferred or research-only.

## Next bounded frontier

C2.6 has since closed the texture-transform portion of this frontier; see
`WGE_C2_6_TEXTURE_TRANSFORM_HANDOFF.md`. The remaining material-quality work is
a versioned mip/residency contract, alpha execution where needed, and a cleaner
authored inspection composition. Do not start another slice from this checkpoint
without preserving this bundle and its replay identities.

Related contracts:

- [render asset contract](WGE_RENDER_ASSET_CONTRACT.md)
- [scene/object contract](WGE_SCENE_OBJECT_CONTRACT.md)
- [native graphics architecture](WGE_NATIVE_GRAPHICS_ARCHITECTURE.md)
- [native quality gaps](WGE_NATIVE_QUALITY_GAPS.md)
