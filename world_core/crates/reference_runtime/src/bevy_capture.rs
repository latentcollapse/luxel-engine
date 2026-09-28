use serde::{Deserialize, Serialize};

use crate::fields::prefixed_sha256;
use crate::{ReferenceCamera, ReferenceRuntimeError, WorldArtifact, validate_world_artifact};

pub const BEVY_CAPTURE_PROVENANCE_SCHEMA: &str = "wge.bevy-native-capture-provenance/v2";
pub const BEVY_CAPTURE_FORMAT: &str = "image/png";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BevyRendererIdentity {
    pub renderer_id: String,
    pub viewer_package: String,
    pub viewer_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BevyCaptureProvenance {
    pub schema_version: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub spatial_fields_sha256: String,
    pub camera: ReferenceCamera,
    pub capture_view: String,
    pub capture_format: String,
    pub image_sha256: String,
    pub renderer: BevyRendererIdentity,
}

/// Construct provenance for a screenshot produced by the Bevy inspection
/// backend. The caller must supply the bytes written to disk, so the
/// sidecar cannot describe an image that was never actually captured.
pub fn build_bevy_capture_provenance(
    world: &WorldArtifact,
    image: &[u8],
    capture_view: impl Into<String>,
    renderer: BevyRendererIdentity,
) -> Result<BevyCaptureProvenance, ReferenceRuntimeError> {
    let provenance = BevyCaptureProvenance {
        schema_version: BEVY_CAPTURE_PROVENANCE_SCHEMA.into(),
        world_artifact_id: world.artifact_id.clone(),
        world_artifact_sha256: world.artifact_sha256.clone(),
        spatial_fields_sha256: world.body.fields.spatial_sha256.clone(),
        camera: world.body.authored_layout.reference_camera.clone(),
        capture_view: capture_view.into(),
        capture_format: BEVY_CAPTURE_FORMAT.into(),
        image_sha256: prefixed_sha256(image),
        renderer,
    };
    validate_bevy_capture_provenance(world, image, &provenance)?;
    Ok(provenance)
}

/// Independently revalidate Bevy's descriptive sidecar against the canonical
/// Rust world and the exact screenshot bytes. This is intentionally renderer
/// neutral: Bevy is one evidence producer, while Rust remains the promotion
/// authority.
pub fn validate_bevy_capture_provenance(
    world: &WorldArtifact,
    image: &[u8],
    provenance: &BevyCaptureProvenance,
) -> Result<(), ReferenceRuntimeError> {
    validate_world_artifact(world)?;
    if provenance.schema_version != BEVY_CAPTURE_PROVENANCE_SCHEMA {
        return Err(ReferenceRuntimeError::contract(format!(
            "unsupported Bevy capture provenance schema {:?}",
            provenance.schema_version
        )));
    }
    if provenance.world_artifact_id != world.artifact_id
        || provenance.world_artifact_sha256 != world.artifact_sha256
        || provenance.spatial_fields_sha256 != world.body.fields.spatial_sha256
        || provenance.camera != world.body.authored_layout.reference_camera
    {
        return Err(ReferenceRuntimeError::provenance(
            "Bevy capture provenance is bound to a different native world or camera".into(),
        ));
    }
    if !matches!(
        provenance.capture_view.as_str(),
        "overview" | "west-wall" | "east-wall" | "player" | "border"
    ) {
        return Err(ReferenceRuntimeError::contract(format!(
            "unsupported Bevy capture view {:?}",
            provenance.capture_view
        )));
    }
    if provenance.capture_format != BEVY_CAPTURE_FORMAT {
        return Err(ReferenceRuntimeError::contract(
            "Bevy capture format is not image/png".into(),
        ));
    }
    if !image.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(ReferenceRuntimeError::contract(
            "Bevy capture bytes do not have a PNG signature".into(),
        ));
    }
    if provenance.image_sha256 != prefixed_sha256(image) {
        return Err(ReferenceRuntimeError::provenance(
            "Bevy capture image digest is stale or forged".into(),
        ));
    }
    if provenance.renderer.renderer_id != "bevy"
        || provenance.renderer.viewer_package.trim().is_empty()
        || provenance.renderer.viewer_version.trim().is_empty()
    {
        return Err(ReferenceRuntimeError::contract(
            "Bevy renderer identity is incomplete".into(),
        ));
    }
    Ok(())
}
