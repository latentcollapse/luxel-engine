# WGE Neutral Render-Asset Conditioning Contract

Status: **C2.7 deterministic mip-chain conditioning implemented; GPU residency remains open**, 2026-09-30  
Owner: Rust `wge-asset-contract`  
Downstream: Rust `wge-project-ledger` and `wge-native-graphics-contract`

## Purpose

This contract turns a self-contained GLB into a deterministic,
backend-neutral render package. It is the seam between ordinary authored or
provider-produced assets and WGE scene/graphics composition.

```text
GLB bytes
  -> Rust inspection and source identity
  -> RenderConditioningRequest
  -> RenderAssetPackage + RenderConditioningReceipt
  -> GraphicsAssetProjection
  -> SceneObject / GraphicsScenePacket lowering
```

The package is not a GPU resource, a scene, or semantic authority. It does
not choose a renderer backend and it cannot promote visual evidence. Runtime
target selection remains separate: the active path uses the Rust-native
`RuntimeTarget::Native`; the retained Unity target is compatibility-only.

## Rust-owned inputs and outputs

`RenderConditioningRequest` (`wge.render-asset-request/v1`) explicitly binds:

- meters per unit;
- source vertical axis;
- whether UV0 is required;
- whether normals and tangents may be generated;
- texture mip policy (`single_level_explicit` or bounded deterministic
  `generate_cpu_chain`);
- maximum decoded texture dimension.

`RenderAssetPackage` (`wge.render-asset-package/v2`) contains:

- source asset identity and conditioning transform;
- deterministic mesh buffers with positions, normals, tangents, UV0, and indices;
- PBR scalar/factor fields and typed texture-role references;
- decoded embedded PNG/JPEG textures as RGBA8, with an optional validated
  descending mip chain;
- a deterministic source-mesh inventory for multi-part static assets;
- explicit color-space roles;
- one validated shared glTF `KHR_texture_transform` lowered into canonical UV0
  for the conditioned primitive;
- producer, source, request, and inspection provenance.

`RenderConditioningReceipt` (`wge.render-asset-receipt/v2`) is content
addressed over the complete typed result. A rejected result carries findings;
a status-only or re-sealed result is not sufficient for promotion.

## Current deterministic behavior

- GLB framing and source identity are measured by the existing Rust asset
  inspector before conditioning.
- Missing normals/tangents may be generated deterministically when requested.
- Missing required UV0 is a hard rejection finding.
- A shared `KHR_texture_transform` is applied to canonical UV0 before tangent
  generation, so normal bases and sampled material roles use the same space.
  Conflicting transforms across present material texture roles and non-zero
  alternate `texCoord` selections are typed rejections.
- Source vertical-axis conversion and winding correction are explicit.
- Only self-contained embedded PNG/JPEG images are accepted in this slice;
  external URIs are rejected rather than fetched implicitly.
- `generate_cpu_chain` preserves the base RGBA8 level and adds deterministic
  levels down to 1x1. sRGB RGB channels are averaged in linear space, normal
  maps are averaged as normalized vectors, and linear/data channels use a
  bounded box filter; alpha remains a linear channel. Rust independently
  validates level count, dimensions, byte lengths, and content identity.
- The neutral graphics projection preserves all levels in a versioned
  `rgba8_mip_chain` payload and binds the texture digest to the concatenated
  decoded levels. The current Lava adapter deliberately rejects that payload
  with a typed unsupported-capability result until GPU residency/upload and
  sampler LOD are implemented; no lower levels are silently discarded.
- The malformed supplied GLB remains a permanent negative/rejection control.

## Native operations

The asset-contract binary exposes:

```text
wge-asset-contract prepare-render ASSET.glb REQUEST.json
```

Exit status `0` means the typed receipt is `Ready`; status `3` means the
asset was measured and rejected; status `2` means the request/container could
not be interpreted as a valid invocation.

The graphics contract exposes `project_render_asset`. It independently
revalidates the render package and produces a detached
  `GraphicsAssetProjection` with:

- current `MeshPacket`/`MaterialIntent`/`TextureReference` views;
- conditioned tangent streams carried inside imported `MeshPacket` values so
  scene composition cannot silently discard them;
- source asset/package identity;
- source-image digests distinct from inline RGBA payload digests.

Projection is not scene composition. `SceneArtifact` now has an optional,
digest-bound render-package/mesh binding, and
`seal_scene_with_render_assets` independently validates that binding against
the source asset identity and package mesh set. The scene still owns
transform, collision/LOD policy, gameplay meaning, and semantic role; the
render package still owns conditioned geometry/material payload.

The resulting `SceneProjection` carries only the render package and mesh
identity needed by the graphics composer. It does not carry the asset receipt
or gameplay authority fields. `compose_bound_scene` now lowers a validated
scene plus exact `GraphicsAssetProjection` set into a digest-bound
`GraphicsScenePacket`, with namespaced mesh/material/texture identities and
scene-artifact identity retained in the packet.

## Explicit next seams

1. Preserve collision and LOD metadata through real asset preparation and
   expose the multi-part source-mesh inventory without pretending it is an LOD
   ladder.
2. Extend the bound-scene promotion path to additional asset families without
   weakening independent scene recomposition or runtime provenance.
3. Add GPU mip residency/upload and sampler LOD behind the existing versioned
   payload, then add alpha execution and preserve the same independent
   validation. CPU conditioning and neutral packet representation are green;
   the current Lava rejection is intentional until that seam exists.
