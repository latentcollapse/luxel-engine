//! Rust-owned semantic scene objects and the asset-to-world bridge.
//!
//! A scene object is the canonical semantic claim that a validated runtime
//! asset exists in a particular world for a particular reason. Graphics
//! packets are later projections of this artifact; they cannot rewrite it.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use luxel_asset_contract::{
    ASSET_RUNTIME_RECEIPT_SCHEMA, AssetPreparationReceipt, PreparationStatus, RenderAssetPackage,
    RuntimeAssetPackage, validate_render_asset_package,
};

use crate::{LedgerError, canonical_json, sha256_hex, sha256_prefixed};

pub const SCENE_ARTIFACT_SCHEMA: &str = "luxel.scene-artifact/v1";
pub const SCENE_OBJECT_SCHEMA: &str = "luxel.scene-object/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneArtifact {
    pub schema_version: String,
    pub artifact_id: String,
    pub artifact_sha256: String,
    pub body: SceneArtifactBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneArtifactBody {
    pub schema_version: String,
    pub scene_id: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub objects: Vec<SceneObject>,
    pub provenance: SceneProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneObject {
    pub schema_version: String,
    pub object_id: String,
    pub source_asset_id: String,
    pub source_asset_sha256: String,
    pub asset_receipt_sha256: String,
    pub runtime_package_id: String,
    pub mesh_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_package_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_mesh_id: Option<String>,
    pub semantic_role: String,
    pub gameplay_refs: Vec<GameplayReference>,
    pub transform: SceneTransform,
    pub collision: CollisionPolicy,
    pub material_assignments: Vec<MaterialAssignment>,
    pub importance: SceneImportance,
    pub lod: SceneLodPolicy,
    pub visibility: SceneVisibilityPolicy,
    pub provenance: SceneObjectProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GameplayReference {
    pub reference_id: String,
    pub kind: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneTransform {
    pub translation_xyz_m: [f64; 3],
    pub rotation_xyzw: [f64; 4],
    pub scale_xyz: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CollisionPolicy {
    None,
    Static { shape: SceneCollisionShape },
    Trigger { shape: SceneCollisionShape },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SceneCollisionShape {
    Box,
    Sphere,
    Capsule,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MaterialAssignment {
    pub slot: u32,
    pub material_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SceneImportance {
    Background,
    Landmark,
    GameplayCritical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneLodPolicy {
    pub levels: Vec<SceneLodLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneLodLevel {
    pub level: u8,
    pub mesh_id: String,
    pub switch_below_fraction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneVisibilityPolicy {
    pub renderable: bool,
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    pub max_distance_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SceneObjectProvenance {
    pub authoring_id: String,
    pub source_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_job_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SceneProvenance {
    pub construction_plan_id: String,
    pub authoring_digest: String,
    pub source_refs: Vec<String>,
}

/// Backend-neutral detached projection. It intentionally contains no mutable
/// reference into the canonical SceneArtifact and no authority-bearing fields
/// such as asset receipt identity or gameplay semantics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneProjection {
    pub schema_version: String,
    pub source_scene_artifact_id: String,
    pub source_scene_artifact_sha256: String,
    pub world_artifact_id: String,
    pub objects: Vec<SceneObjectProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneObjectProjection {
    pub object_id: String,
    pub mesh_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_package_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_mesh_id: Option<String>,
    pub material_assignments: Vec<MaterialAssignment>,
    pub transform: SceneTransform,
    pub importance: SceneImportance,
    pub visibility: SceneVisibilityPolicy,
}

/// Seal a semantic scene only after each referenced asset receipt has been
/// checked independently. Reusing one validated package for several objects
/// is allowed; silently substituting another package is not.
pub fn seal_scene(
    body: SceneArtifactBody,
    asset_receipts: &[AssetPreparationReceipt],
) -> Result<SceneArtifact, LedgerError> {
    seal_scene_internal(body, asset_receipts, &[])
}

/// Seal a semantic scene and bind any explicitly renderable objects to
/// independently validated neutral render packages. The render package is an
/// input identity; it does not become canonical scene state or a GPU object.
pub fn seal_scene_with_render_assets(
    body: SceneArtifactBody,
    asset_receipts: &[AssetPreparationReceipt],
    render_packages: &[RenderAssetPackage],
) -> Result<SceneArtifact, LedgerError> {
    seal_scene_internal(body, asset_receipts, render_packages)
}

fn seal_scene_internal(
    body: SceneArtifactBody,
    asset_receipts: &[AssetPreparationReceipt],
    render_packages: &[RenderAssetPackage],
) -> Result<SceneArtifact, LedgerError> {
    validate_body(&body)?;
    validate_asset_bindings(&body.objects, asset_receipts)?;
    validate_render_bindings(&body.objects, render_packages)?;
    let artifact_sha256 = sha256_prefixed(
        canonical_json(&serde_json::to_value(&body).map_err(json_error)?).as_bytes(),
    );
    let artifact_id = format!("scene_sha256_{}", &artifact_sha256[7..]);
    let artifact = SceneArtifact {
        schema_version: SCENE_ARTIFACT_SCHEMA.into(),
        artifact_id,
        artifact_sha256,
        body,
    };
    validate_scene_artifact(&artifact)?;
    Ok(artifact)
}

pub fn validate_scene_artifact(artifact: &SceneArtifact) -> Result<(), LedgerError> {
    if artifact.schema_version != SCENE_ARTIFACT_SCHEMA {
        return Err(contract_error(format!(
            "unsupported scene artifact schema {}",
            artifact.schema_version
        )));
    }
    valid_id(&artifact.artifact_id, "artifact_id")?;
    valid_prefixed_sha(&artifact.artifact_sha256, "artifact_sha256")?;
    validate_body(&artifact.body)?;
    let expected_sha = sha256_prefixed(
        canonical_json(&serde_json::to_value(&artifact.body).map_err(json_error)?).as_bytes(),
    );
    if artifact.artifact_sha256 != expected_sha
        || artifact.artifact_id != format!("scene_sha256_{}", &expected_sha[7..])
    {
        return Err(LedgerError::Provenance(
            "scene artifact identity does not match its canonical body".into(),
        ));
    }
    Ok(())
}

pub fn validate_scene_against_asset_receipts(
    artifact: &SceneArtifact,
    asset_receipts: &[AssetPreparationReceipt],
) -> Result<(), LedgerError> {
    validate_scene_artifact(artifact)?;
    validate_asset_bindings(&artifact.body.objects, asset_receipts)?;
    validate_render_bindings(&artifact.body.objects, &[])
}

pub fn validate_scene_against_asset_receipts_and_render_assets(
    artifact: &SceneArtifact,
    asset_receipts: &[AssetPreparationReceipt],
    render_packages: &[RenderAssetPackage],
) -> Result<(), LedgerError> {
    validate_scene_artifact(artifact)?;
    validate_asset_bindings(&artifact.body.objects, asset_receipts)?;
    validate_render_bindings(&artifact.body.objects, render_packages)
}

pub fn project_scene_for_graphics(
    artifact: &SceneArtifact,
) -> Result<SceneProjection, LedgerError> {
    validate_scene_artifact(artifact)?;
    Ok(SceneProjection {
        schema_version: "luxel.scene-projection/v1".into(),
        source_scene_artifact_id: artifact.artifact_id.clone(),
        source_scene_artifact_sha256: artifact.artifact_sha256.clone(),
        world_artifact_id: artifact.body.world_artifact_id.clone(),
        objects: artifact
            .body
            .objects
            .iter()
            .map(|object| SceneObjectProjection {
                object_id: object.object_id.clone(),
                mesh_id: object.mesh_id.clone(),
                render_package_id: object.render_package_id.clone(),
                render_mesh_id: object.render_mesh_id.clone(),
                material_assignments: object.material_assignments.clone(),
                transform: object.transform.clone(),
                importance: object.importance,
                visibility: object.visibility.clone(),
            })
            .collect(),
    })
}

fn validate_body(body: &SceneArtifactBody) -> Result<(), LedgerError> {
    if body.schema_version != SCENE_ARTIFACT_SCHEMA {
        return Err(contract_error(format!(
            "unsupported scene body schema {}",
            body.schema_version
        )));
    }
    valid_id(&body.scene_id, "scene_id")?;
    valid_id(&body.world_artifact_id, "world_artifact_id")?;
    valid_prefixed_sha(&body.world_artifact_sha256, "world_artifact_sha256")?;
    validate_provenance(&body.provenance)?;
    let mut object_ids = BTreeSet::new();
    for object in &body.objects {
        validate_object(object)?;
        if !object_ids.insert(object.object_id.as_str()) {
            return Err(contract_error(format!(
                "duplicate scene object {}",
                object.object_id
            )));
        }
    }
    Ok(())
}

fn validate_object(object: &SceneObject) -> Result<(), LedgerError> {
    if object.schema_version != SCENE_OBJECT_SCHEMA {
        return Err(contract_error(format!(
            "scene object {} uses unsupported schema {}",
            object.object_id, object.schema_version
        )));
    }
    for (label, value) in [
        ("object_id", object.object_id.as_str()),
        ("source_asset_id", object.source_asset_id.as_str()),
        ("runtime_package_id", object.runtime_package_id.as_str()),
        ("mesh_id", object.mesh_id.as_str()),
    ] {
        valid_id(value, label)?;
    }
    valid_sha256_hex(&object.source_asset_sha256, "source_asset_sha256")?;
    valid_sha256_hex(&object.asset_receipt_sha256, "asset_receipt_sha256")?;
    match (&object.render_package_id, &object.render_mesh_id) {
        (Some(package_id), Some(mesh_id)) => {
            valid_id(package_id, "render_package_id")?;
            valid_id(mesh_id, "render_mesh_id")?;
        }
        (Some(package_id), None) => valid_id(package_id, "render_package_id")?,
        (None, Some(_)) => {
            return Err(contract_error(
                "render_mesh_id requires a render_package_id",
            ));
        }
        (None, None) => {}
    }
    valid_text(&object.semantic_role, "semantic_role")?;
    validate_transform(&object.transform)?;
    validate_gameplay_refs(&object.gameplay_refs)?;
    validate_materials(&object.material_assignments)?;
    validate_lod(&object.lod, &object.mesh_id)?;
    if !object.visibility.max_distance_m.is_finite() || object.visibility.max_distance_m <= 0.0 {
        return Err(contract_error(format!(
            "scene object {} has invalid visibility distance",
            object.object_id
        )));
    }
    validate_provenance_object(&object.provenance)
}

fn validate_transform(transform: &SceneTransform) -> Result<(), LedgerError> {
    if !transform
        .translation_xyz_m
        .iter()
        .chain(transform.rotation_xyzw.iter())
        .chain(transform.scale_xyz.iter())
        .all(|value| value.is_finite())
        || !transform.scale_xyz.iter().all(|value| *value > 0.0)
    {
        return Err(contract_error(
            "scene transform contains a non-finite or non-positive value",
        ));
    }
    let rotation_length = transform
        .rotation_xyzw
        .iter()
        .map(|value| value * value)
        .sum::<f64>();
    if rotation_length <= f64::EPSILON {
        return Err(contract_error("scene rotation quaternion cannot be zero"));
    }
    Ok(())
}

fn validate_gameplay_refs(refs: &[GameplayReference]) -> Result<(), LedgerError> {
    let mut ids = BTreeSet::new();
    for reference in refs {
        valid_id(&reference.reference_id, "gameplay reference id")?;
        valid_text(&reference.kind, "gameplay reference kind")?;
        if !ids.insert(reference.reference_id.as_str()) {
            return Err(contract_error(format!(
                "duplicate gameplay reference {}",
                reference.reference_id
            )));
        }
    }
    Ok(())
}

fn validate_materials(materials: &[MaterialAssignment]) -> Result<(), LedgerError> {
    if materials.is_empty() {
        return Err(contract_error(
            "scene object needs at least one material assignment",
        ));
    }
    let mut slots = BTreeSet::new();
    for material in materials {
        valid_id(&material.material_id, "material_id")?;
        if !slots.insert(material.slot) {
            return Err(contract_error(format!(
                "duplicate material slot {}",
                material.slot
            )));
        }
        match (&material.material_artifact_id, &material.material_sha256) {
            (Some(artifact), Some(digest)) => {
                valid_id(artifact, "material_artifact_id")?;
                valid_prefixed_sha(digest, "material_sha256")?;
            }
            (None, None) => {}
            _ => {
                return Err(contract_error(
                    "material provenance must include both id and digest",
                ));
            }
        }
    }
    Ok(())
}

fn validate_lod(lod: &SceneLodPolicy, primary_mesh_id: &str) -> Result<(), LedgerError> {
    if lod.levels.is_empty() {
        return Err(contract_error("scene object needs at least one LOD level"));
    }
    let mut levels = BTreeSet::new();
    for level in &lod.levels {
        if !levels.insert(level.level) {
            return Err(contract_error(format!(
                "duplicate LOD level {}",
                level.level
            )));
        }
        valid_id(&level.mesh_id, "lod.mesh_id")?;
        if !level.switch_below_fraction.is_finite()
            || !(0.0..=1.0).contains(&level.switch_below_fraction)
        {
            return Err(contract_error(
                "LOD switch coverage must be finite in [0, 1]",
            ));
        }
        if level.level == 0 && level.mesh_id != primary_mesh_id {
            return Err(contract_error(
                "LOD 0 mesh must match the scene object's primary mesh",
            ));
        }
    }
    if !levels.contains(&0) {
        return Err(contract_error(
            "scene object LOD policy must include level 0",
        ));
    }
    Ok(())
}

fn validate_provenance(provenance: &SceneProvenance) -> Result<(), LedgerError> {
    valid_id(&provenance.construction_plan_id, "construction_plan_id")?;
    valid_prefixed_sha(&provenance.authoring_digest, "authoring_digest")?;
    validate_refs(&provenance.source_refs, "scene source reference")
}

fn validate_provenance_object(provenance: &SceneObjectProvenance) -> Result<(), LedgerError> {
    valid_id(&provenance.authoring_id, "object authoring_id")?;
    validate_refs(&provenance.source_refs, "object source reference")?;
    if let Some(provider) = &provenance.provider_job_id {
        valid_id(provider, "provider_job_id")?;
    }
    Ok(())
}

fn validate_asset_bindings(
    objects: &[SceneObject],
    receipts: &[AssetPreparationReceipt],
) -> Result<(), LedgerError> {
    let mut receipt_ids = BTreeSet::new();
    for receipt in receipts {
        if !receipt_ids.insert(receipt.source_identity.asset_id.as_str()) {
            return Err(contract_error(format!(
                "duplicate asset receipt {}",
                receipt.source_identity.asset_id
            )));
        }
    }
    for object in objects {
        let matches = receipts
            .iter()
            .filter(|receipt| receipt.source_identity.asset_id == object.source_asset_id)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(LedgerError::Provenance(format!(
                "scene object {} needs exactly one receipt for asset {}",
                object.object_id, object.source_asset_id
            )));
        }
        let receipt = matches[0];
        validate_receipt(receipt)?;
        let package = receipt.package.as_ref().expect("ready receipt has package");
        if receipt.source_identity.source_sha256 != object.source_asset_sha256
            || receipt.receipt_sha256 != object.asset_receipt_sha256
            || package.package_id != object.runtime_package_id
            || package.source_identity.source_sha256 != object.source_asset_sha256
        {
            return Err(LedgerError::Provenance(format!(
                "scene object {} asset/package identity does not match its receipt",
                object.object_id
            )));
        }
        if !package
            .mesh_ids
            .iter()
            .any(|mesh_id| mesh_id == &object.mesh_id)
        {
            return Err(LedgerError::Provenance(format!(
                "scene object {} primary mesh {} is absent from validated runtime package mesh set",
                object.object_id, object.mesh_id
            )));
        }
    }
    Ok(())
}

fn validate_render_bindings(
    objects: &[SceneObject],
    render_packages: &[RenderAssetPackage],
) -> Result<(), LedgerError> {
    let mut package_ids = BTreeSet::new();
    for package in render_packages {
        validate_render_asset_package(package).map_err(|error| {
            LedgerError::Provenance(format!(
                "render package {} is invalid: {error}",
                package.package_id
            ))
        })?;
        if !package_ids.insert(package.package_id.as_str()) {
            return Err(LedgerError::Provenance(format!(
                "duplicate render package {}",
                package.package_id
            )));
        }
    }
    for object in objects {
        let Some(render_package_id) = &object.render_package_id else {
            if object.render_mesh_id.is_some() {
                return Err(LedgerError::Provenance(format!(
                    "scene object {} has a render mesh without a render package",
                    object.object_id
                )));
            }
            continue;
        };
        let matches = render_packages
            .iter()
            .filter(|package| &package.package_id == render_package_id)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(LedgerError::Provenance(format!(
                "scene object {} needs exactly one render package {}",
                object.object_id, render_package_id
            )));
        }
        let package = matches[0];
        if package.source_identity.asset_id != object.source_asset_id
            || package.source_identity.source_sha256 != object.source_asset_sha256
        {
            return Err(LedgerError::Provenance(format!(
                "scene object {} render package source identity disagrees with its asset",
                object.object_id
            )));
        }
        let render_mesh_id = object
            .render_mesh_id
            .as_deref()
            .unwrap_or(object.mesh_id.as_str());
        if !package
            .meshes
            .iter()
            .any(|mesh| mesh.mesh_id == render_mesh_id)
        {
            return Err(LedgerError::Provenance(format!(
                "scene object {} render mesh {} is absent from render package {}",
                object.object_id, render_mesh_id, render_package_id
            )));
        }
    }
    Ok(())
}

fn validate_receipt(receipt: &AssetPreparationReceipt) -> Result<(), LedgerError> {
    if receipt.schema_version != ASSET_RUNTIME_RECEIPT_SCHEMA {
        return Err(contract_error(format!(
            "asset receipt uses unsupported schema {}",
            receipt.schema_version
        )));
    }
    if receipt.status != PreparationStatus::Ready || !receipt.findings.is_empty() {
        return Err(LedgerError::Provenance(
            "scene composition requires a ready asset receipt without findings".into(),
        ));
    }
    let package = receipt.package.as_ref().ok_or_else(|| {
        LedgerError::Provenance("ready asset receipt is missing its runtime package".into())
    })?;
    validate_runtime_package(package)?;
    let mut value = serde_json::to_value(receipt).map_err(json_error)?;
    value
        .as_object_mut()
        .ok_or_else(|| contract_error("asset receipt is not an object"))?
        .remove("receipt_sha256");
    if receipt.receipt_sha256 != sha256_hex(canonical_json(&value).as_bytes()) {
        return Err(LedgerError::Provenance(
            "asset receipt digest does not match its typed content".into(),
        ));
    }
    if receipt.source_identity != package.source_identity {
        return Err(LedgerError::Provenance(
            "asset receipt and runtime package source identities disagree".into(),
        ));
    }
    Ok(())
}

fn validate_runtime_package(package: &RuntimeAssetPackage) -> Result<(), LedgerError> {
    if package.schema_version != "luxel.runtime-asset-package/v1" {
        return Err(contract_error(format!(
            "unsupported runtime package schema {}",
            package.schema_version
        )));
    }
    valid_id(&package.package_id, "runtime package id")?;
    valid_sha256_hex(
        &package.source_identity.source_sha256,
        "runtime source digest",
    )?;
    let mut value = serde_json::to_value(package).map_err(json_error)?;
    value
        .as_object_mut()
        .ok_or_else(|| contract_error("runtime package is not an object"))?
        .remove("package_id");
    let expected = format!(
        "runtime_asset_sha256_{}",
        sha256_hex(canonical_json(&value).as_bytes())
    );
    if package.package_id != expected {
        return Err(LedgerError::Provenance(
            "runtime package identity does not match its typed content".into(),
        ));
    }
    if package.provenance.source_sha256 != package.source_identity.source_sha256 {
        return Err(LedgerError::Provenance(
            "runtime package provenance source digest disagrees with source identity".into(),
        ));
    }
    Ok(())
}

fn validate_refs(values: &[String], label: &str) -> Result<(), LedgerError> {
    let mut refs = BTreeSet::new();
    for value in values {
        valid_text(value, label)?;
        if !refs.insert(value.as_str()) {
            return Err(contract_error(format!("duplicate {label} {value}")));
        }
    }
    Ok(())
}

fn valid_id(value: &str, label: &str) -> Result<(), LedgerError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
    {
        return Err(contract_error(format!(
            "{label} is not a safe identifier: {value:?}"
        )));
    }
    Ok(())
}

fn valid_text(value: &str, label: &str) -> Result<(), LedgerError> {
    if value.trim().is_empty() {
        return Err(contract_error(format!("{label} cannot be empty")));
    }
    Ok(())
}

fn valid_sha256_hex(value: &str, label: &str) -> Result<(), LedgerError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(contract_error(format!(
            "{label} must be a 64-character SHA-256 hex digest"
        )));
    }
    Ok(())
}

fn valid_prefixed_sha(value: &str, label: &str) -> Result<(), LedgerError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(contract_error(format!("{label} must be a sha256: digest")));
    }
    Ok(())
}

fn contract_error(message: impl Into<String>) -> LedgerError {
    LedgerError::Contract(message.into())
}

fn json_error(error: impl std::fmt::Display) -> LedgerError {
    LedgerError::Json(error.to_string())
}
