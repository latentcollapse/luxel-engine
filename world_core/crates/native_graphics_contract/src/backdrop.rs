//! CONVERGE-4 N-3: the km-scale backdrop (`docs/design/wge-converge4-contracts.md` §1).
//!
//! WHY THIS EXISTS
//! ---------------
//! The converge0 world is 480 x 360 m, closed by a 26 m procedural ridge. N-1's
//! ridge-contrast target (far ridge at <= 40% of the foreground's contrast) has
//! been unmet since CONVERGE-1 because nothing in the frame is far enough away
//! for the atmosphere to act on. The backdrop is a Gaea-built mountain field,
//! 16 km across, that the world is seated in (GAEA_PROGRAMME.md §1: terrain is
//! generated without reference to the play space, then the play space is
//! placed into it).
//!
//! HOW CONTENT ENTERS
//! ------------------
//! The same seam as the kit (`kit.rs`): a committed lock
//! (`tools/backdrop/<set>.lock.json`, written by `tools/build_backdrop.py`)
//! pins one GLB by size and sha256; [`load_backdrop_set`] re-verifies it,
//! conditions it through the calibration render request (any finding is
//! refused) and projects it. The lock also pins the **pixel** digests of the
//! Gaea outputs the GLB was built from, because an eroded Gaea field is a
//! source artifact that cannot be rebuilt byte-exactly.
//!
//! PLACEMENT
//! ---------
//! The GLB is authored in the seat's frame: its origin is the seat, at the
//! field's own ground height there, already yawed and already sunk under the
//! near world. Placement is therefore one translation, to the world's centre at
//! [`BACKDROP_SEAT_DROP_M`] below the source terrain's mean height, with no
//! rotation or scale for a lowering to get wrong.

use std::path::Path;

use serde::Deserialize;
use wge_asset_contract::{RenderPreparationStatus, condition_render_asset};

use crate::asset_projection::{GraphicsAssetProjection, project_render_asset, validate_graphics_asset_projection};
use crate::scene_composition::{AssetNamespace, append_asset_resources};
use crate::{GraphicsContractError, GraphicsScenePacketBody, InstanceImportance, InstancePacket, Transform3d, sha256_prefixed};

pub const BACKDROP_LOCK_SCHEMA: &str = "wge.backdrop-lock/v1";

/// Far plane of a backdrop view. The field reaches ~19 km from the seat along
/// its diagonal; 24 km keeps the whole of it, with margin, inside the frustum.
/// The depth buffer is D32F with a 0.1 m near plane, so this barely moves
/// near-field precision (depth ~ 1 - near / z) and gives ~15 m resolution at
/// 5 km, enough for ridges hundreds of metres apart.
pub const BACKDROP_FAR_PLANE_M: f32 = 24_000.0;

/// The seat sits this far below the source terrain's mean height, so the field
/// floor around the world never meets the world's own ground.
pub const BACKDROP_SEAT_DROP_M: f32 = 2.0;

/// Instance ids are `backdrop-NN`, one per tile mesh.
pub const BACKDROP_INSTANCE_PREFIX: &str = "backdrop-";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    schema_version: String,
    set_id: String,
    spec_sha256: String,
    gaea_pixels: std::collections::BTreeMap<String, String>,
    glb: String,
    bytes: u64,
    sha256: String,
}

/// A verified, conditioned backdrop. `lock_sha256` binds the whole set.
#[derive(Clone, Debug, PartialEq)]
pub struct BackdropSet {
    pub set_id: String,
    pub lock_sha256: String,
    pub glb_sha256: String,
    pub projection: GraphicsAssetProjection,
}

/// Load and verify a backdrop from its lock. `repo_root` resolves the lock's
/// repo-relative GLB path.
pub fn load_backdrop_set(lock_path: &Path, repo_root: &Path) -> Result<BackdropSet, GraphicsContractError> {
    let text = std::fs::read(lock_path)
        .map_err(|error| GraphicsContractError::provenance(format!("backdrop lock {}: {error}", lock_path.display())))?;
    let lock: Lock = serde_json::from_slice(&text).map_err(|error| {
        GraphicsContractError::malformed(format!("backdrop lock {} is malformed: {error}", lock_path.display()))
    })?;
    if lock.schema_version != BACKDROP_LOCK_SCHEMA {
        return Err(GraphicsContractError::malformed(format!(
            "backdrop lock schema {} is unsupported",
            lock.schema_version
        )));
    }
    if !lock.gaea_pixels.contains_key("Height_Out.png") {
        return Err(GraphicsContractError::malformed(
            "a backdrop lock must pin the pixel digest of the Gaea height it was built from",
        ));
    }
    let _ = &lock.spec_sha256;
    let path = repo_root.join(&lock.glb);
    let glb = std::fs::read(&path).map_err(|error| {
        GraphicsContractError::provenance(format!(
            "backdrop {} at {}: {error} (build it with tools/build_backdrop.py --set {})",
            lock.set_id,
            path.display(),
            lock.set_id
        ))
    })?;
    let digest = sha256_prefixed(&glb);
    if glb.len() as u64 != lock.bytes || digest != lock.sha256 {
        return Err(GraphicsContractError::provenance(format!(
            "backdrop {} is {} bytes {digest}; the lock pins {} bytes {}",
            lock.set_id,
            glb.len(),
            lock.bytes,
            lock.sha256
        )));
    }
    backdrop_set_from_glb(&lock.set_id, &sha256_prefixed(&text), &glb)
}

/// Condition and project backdrop GLB bytes whose digest the caller has
/// already verified. Public so contract tests can exercise placement with a
/// generated GLB instead of a built Gaea field.
pub fn backdrop_set_from_glb(set_id: &str, lock_sha256: &str, glb: &[u8]) -> Result<BackdropSet, GraphicsContractError> {
    let receipt = condition_render_asset(glb, &crate::calibration::render_request())
        .map_err(|error| GraphicsContractError::provenance(format!("backdrop {set_id} conditioning: {error}")))?;
    if receipt.status != RenderPreparationStatus::Ready || !receipt.findings.is_empty() {
        return Err(GraphicsContractError::provenance(format!(
            "backdrop {set_id} failed render conditioning: {:?}",
            receipt.findings
        )));
    }
    let package = receipt.package.as_ref().expect("a ready receipt has a package");
    let projection = project_render_asset(package)?;
    validate_graphics_asset_projection(&projection)?;
    if projection.meshes.is_empty() {
        return Err(GraphicsContractError::malformed(format!("backdrop {set_id} has no meshes")));
    }
    Ok(BackdropSet {
        set_id: set_id.to_owned(),
        lock_sha256: lock_sha256.to_owned(),
        glb_sha256: sha256_prefixed(glb),
        projection,
    })
}

/// Seat the backdrop under the world: one instance per tile mesh at
/// (0, `seat_y`, 0), and the camera's far plane pushed out to hold it.
pub fn apply_backdrop(
    body: &mut GraphicsScenePacketBody,
    set: &BackdropSet,
    seat_y: f32,
) -> Result<(), GraphicsContractError> {
    if !seat_y.is_finite() {
        return Err(GraphicsContractError::malformed("backdrop seat height is not finite"));
    }
    if body.instances.iter().any(|instance| instance.instance_id.starts_with(BACKDROP_INSTANCE_PREFIX)) {
        return Err(GraphicsContractError::malformed("the packet already has a backdrop"));
    }
    let namespace = AssetNamespace::new(&set.projection);
    append_asset_resources(body, &namespace, &set.projection)?;
    for (k, mesh) in set.projection.meshes.iter().enumerate() {
        body.instances.push(InstancePacket {
            instance_id: format!("{BACKDROP_INSTANCE_PREFIX}{k:02}"),
            mesh_id: namespace.mesh(&mesh.packet.mesh_id)?.clone(),
            material_id: namespace.material(&mesh.packet.material_id)?.clone(),
            importance: InstanceImportance::Background,
            transform: Transform3d {
                translation_xyz_m: [0.0, seat_y, 0.0],
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
                scale_xyz: [1.0; 3],
            },
            variation: None,
        });
    }
    body.camera.far_plane_m = body.camera.far_plane_m.max(BACKDROP_FAR_PLANE_M);
    Ok(())
}
