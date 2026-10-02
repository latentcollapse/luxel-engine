# WGE SceneObject and SceneArtifact Contract

Status: **C2.1–C2.4 implemented for the permanent static-asset slice; broader production asset work remains open**, 2026-09-30  
Scope: bind validated runtime assets to semantic world objects without making a renderer packet canonical.

## Canonical shape

```text
AssetPreparationReceipt
    -> RuntimeAssetPackage identity
    -> SceneObject
    -> SceneArtifact
    -> detached SceneProjection + GraphicsAssetProjection
    -> GraphicsScenePacket projection
```

`SceneObject` records why an asset exists in a world, not merely that a mesh
can be drawn. It carries stable object identity, source asset identity and
exact preparation-receipt identity, runtime package identity and primary mesh,
semantic role and gameplay references, transform, collision policy, material
assignments, importance, LOD, visibility, and authoring/provider provenance.

An object may additionally carry an optional `render_package_id` and
`render_mesh_id`. These are content identities, not renderer state. The
`seal_scene_with_render_assets` path requires exactly one independently
validated `RenderAssetPackage`, matches its source asset identity, and checks
that the selected render mesh exists. A render-bound object cannot be sealed
through the older receipt-only path by accident.

`SceneArtifact` binds the object set to a world artifact identity, construction
plan, source references, and a content digest. Its artifact ID is derived from
the canonical body.

## Asset authority

Scene composition accepts only a `Ready` asset preparation receipt with no
findings. Rust independently checks the receipt and runtime-package schemas,
receipt/package/source identity, package and receipt digests, exact receipt
identity stored on each object, and the presence of each object's primary mesh
in the validated package mesh inventory. A multi-part static asset may expose
several source mesh identities; that inventory is deliberately distinct from
the package's LOD policy.

The supplied malformed GLB remains upstream of this contract as the permanent
negative control. A status-only or re-sealed fake receipt cannot become a scene
object because the receipt/package identities are recomputed.

## Graphics boundary

`project_scene_for_graphics(&SceneArtifact)` validates the canonical artifact
and returns a detached `SceneProjection`. The projection contains only the
render-relevant subset. It does not carry authority-bearing asset receipt or
gameplay identity, and it cannot mutate the source artifact through Rust
borrowing. A later graphics packet must bind back to the source scene artifact
identity and remain a projection, never a replacement.

The C2.2 render-conditioning slice now decodes the bounded self-contained GLB
subset into a deterministic `RenderAssetPackage` and projects it into a
detached `GraphicsAssetProjection`. The projection preserves source/package
identity, including the conditioned per-vertex tangent stream, and does not
become a scene or Lava resource. A typed binding from a
validated `SceneArtifact` to that render package is present, and
`compose_bound_scene` produces a detached packet while preserving world and
scene identity. `compose_bound_scene_with_camera` additionally binds an
explicit derived inspection camera without changing semantic world state.
`GraphicsWorkerSupervisor::render_bound_scene_and_promote`
independently revalidates the scene/package receipts, recomposes the exact
packet (including any inspection camera) from the authorized base projection,
and only then sends it to Lava.
The permanent assembled log-hut fixture now passes native capture, Rust
promotion, worker restart, and deterministic replay. Keeping that boundary
explicit prevents a graphics backend from quietly becoming WGE semantic state.

## Current API

The Rust project-ledger crate exposes:

```rust
seal_scene(body, asset_receipts)
seal_scene_with_render_assets(&body, &asset_receipts, &render_packages)
validate_scene_artifact(artifact)
validate_scene_against_asset_receipts(artifact, asset_receipts)
validate_scene_against_asset_receipts_and_render_assets(
    &artifact, &asset_receipts, &render_packages
)
project_scene_for_graphics(artifact)
```

All paths are deterministic and reject unknown wire fields, malformed IDs,
non-finite transforms, zero rotations, invalid LOD policies, duplicate object
or material/gameplay identities, stale asset receipts, and tampered scene
digests.
