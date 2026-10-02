# WGE C2.7 Deterministic Mip-Chain Handoff

Status: **green contract checkpoint**, 2026-09-30  
Parent campaign: `WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md`  
Previous checkpoint: `WGE_C2_6_TEXTURE_TRANSFORM_HANDOFF.md`

## Outcome

C2.7 closes the authority-side representation of deterministic texture mip
chains without pretending that the current Lava adapter already has GPU
residency or sampler-LOD support.

The Rust render package is now `wge.render-asset-package/v2` and the receipt is
`wge.render-asset-receipt/v2`; the conditioning request remains
`wge.render-asset-request/v1`.

`generate_cpu_chain` now:

- preserves the decoded base RGBA8 level;
- derives bounded descending levels through 1x1;
- averages sRGB RGB channels in linear space;
- averages and renormalizes normal-map vectors;
- box-filters linear/data channels; and
- averages alpha as a linear channel.

Rust revalidates every level's dimensions, byte length, count, and content
identity. A tampered level is rejected.

The neutral graphics projection carries the levels as
`rgba8_mip_chain`. Its `TextureReference.sha256` covers the concatenated
decoded bytes in level order. Julia validates the same shape and digest before
the adapter boundary.

## Deliberate boundary

The current Lava adapter accepts only one-level uploads. A parsed,
authority-valid multi-level payload is rejected with a typed
`unsupported_texture` adapter error until the GPU residency/upload and sampler
LOD seam is implemented. Lower mip levels are never silently discarded, and
this checkpoint makes no new GPU-rendering claim.

The C2.5 native imported-asset capture/restart/replay remains the backend
regression control. No new capture is required for C2.7.

## Verification

The focused gates are:

```text
cargo fmt --manifest-path world_core/Cargo.toml --all -- --check
cargo test --manifest-path world_core/Cargo.toml --offline --package wge-asset-contract --test render_conditioning
cargo clippy --manifest-path world_core/Cargo.toml --offline --package wge-asset-contract --all-targets -- -D warnings
cargo test --manifest-path world_core/Cargo.toml --offline --package wge-native-graphics-contract --test asset_projection
cargo test --manifest-path world_core/Cargo.toml --offline --package wge-project-ledger --test scene
julia --project=graphics_lab graphics_lab/test/runtests.jl
```

Expected focused results:

- render conditioning: 11/11;
- native projection: 3/3;
- scene contract: 5/5;
- Julia worker/parser suite: 7/7, 2/2, 13/13, 11/11, 5/5, 3/3;
- strict asset-contract clippy: green;
- graphify synchronization: completed after this checkpoint.

The full native graphics integration regression remains the C2.5 control:
7/7 Vulkan integration tests passed after the warm-state replay repair.

## Next bounded frontier: C2.8

Implement GPU mip residency/upload and sampler LOD behind this versioned,
authority-validated payload. Preserve independent Rust/Julia validation,
explicit resource ownership and lifetime, deterministic single-level fallback
only when the packet requests it, and a typed unsupported result when the
device/profile cannot provide the requested residency. Alpha execution,
collision/LOD carry-through, and broader material quality remain separate
frontiers.

The supplied malformed GLB remains a permanent rejection control. Rigging,
Unity, TetCageRT promotion, and unrelated post-MVP breadth remain out of
scope.
