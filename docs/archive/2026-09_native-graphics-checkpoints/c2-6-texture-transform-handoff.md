# Luxel C2.6 Texture-Transform Conditioning Handoff

Status: **green contract checkpoint**, 2026-09-30.

C2.6 closes one narrow material-fidelity seam without widening the Vulkan/Lava
packet: Rust now understands the safe glTF `KHR_texture_transform` subset and
produces canonical UV0 for downstream rendering.

## Boundary

- A shared transform across present material texture roles is accepted.
- Offset, scale, and rotation are bounded and finite.
- The transform is applied before generated tangents, keeping normal mapping
  in the same UV space as sampled material roles.
- Authored tangents are regenerated when the request permits it; otherwise a
  non-identity transform is rejected rather than leaving a stale tangent basis
  attached to transformed UVs.
- Non-zero `texCoord` selections are rejected because the current conditioner
  has only a canonical `TEXCOORD_0` channel.
- Conflicting transforms across present texture roles are rejected rather than
  silently applying one role's transform to another.
- The source transform remains part of the Rust `RenderAssetPackage` identity;
  the graphics packet receives the resulting canonical UV0.

## Verification

```sh
cargo fmt --manifest-path world_core/Cargo.toml --all -- --check
cargo test --manifest-path world_core/Cargo.toml --offline \
  --package luxel-asset-contract --test render_conditioning
cargo clippy --manifest-path world_core/Cargo.toml --offline \
  --package luxel-asset-contract --all-targets -- -D warnings
cargo test --manifest-path world_core/Cargo.toml --offline \
  --package luxel-native-graphics-contract --test asset_projection
cargo test --manifest-path world_core/Cargo.toml --offline \
  --package luxel-project-ledger --test scene
```

Observed gates:

- render conditioning: `10 passed; 0 failed`;
- asset projection: `2 passed; 0 failed`;
- scene binding: `5 passed; 0 failed`;
- strict asset-contract clippy: passed;
- formatting: passed.

The adversarial tests cover deterministic transformed UVs, tangent generation
after transformation, stale authored-tangent rejection/regeneration, missing
UV rejection, undeclared CPU mip-chain rejection, alternate UV-set rejection,
and receipt tamper revalidation.

No new GPU capture is claimed here: Lava already consumes canonical UV0, and
the C2.5 real-asset Vulkan replay remains the authoritative backend proof. A
future transformed-asset fixture should exercise the same close capture once
material transforms are combined with the next visual composition slice.

## Still open

- versioned multi-level texture payloads and actual GPU mip residency;
- alpha-mask/blend execution and texture transforms beyond the shared subset;
- collision/LOD carry-through;
- a cleaner imported-asset inspection composition and stronger material review.

The supplied malformed GLB remains a permanent rejection control. Rigging,
characters, TetCageRT, and external-engine integration remain outside this
checkpoint.
