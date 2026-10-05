//! Deterministic render-asset conditioning for the native WGE path.
//!
//! This module is deliberately downstream of GLB inspection and upstream of
//! graphics packets. It extracts a bounded, backend-neutral mesh/material/
//! texture package; it does not create GPU resources or decide certification.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    AccessorInfo, AssetContractError, AssetIdentity, Axis, accessor_info, array_or_empty,
    canonical_json, inspect_glb, parse_glb, read_indices, read_positions, sha256_hex,
};

pub const RENDER_ASSET_REQUEST_SCHEMA: &str = "wge.render-asset-request/v1";
pub const RENDER_ASSET_PACKAGE_SCHEMA: &str = "wge.render-asset-package/v2";
pub const RENDER_ASSET_RECEIPT_SCHEMA: &str = "wge.render-asset-receipt/v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderPreparationStatus {
    Ready,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderConditioningRequest {
    pub schema_version: String,
    pub meters_per_unit: f64,
    pub vertical_axis: Axis,
    pub require_uv0: bool,
    pub generate_normals: bool,
    pub generate_tangents: bool,
    /// The native v1 packet carries one deterministic inline level.  A future
    /// mip-chain producer must opt into a new, independently validated path
    /// rather than silently pretending that a single level is a full texture
    /// residency policy.
    pub mip_policy: RenderMipPolicy,
    pub max_texture_dimension: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMipPolicy {
    SingleLevelExplicit,
    GenerateCpuChain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderFindingCode {
    RequestSchemaUnsupported,
    InvalidRequest,
    UnsupportedPrimitiveMode,
    MissingUv0,
    MissingNormals,
    MissingTangents,
    InvalidAttribute,
    InvalidMaterial,
    MissingMaterial,
    MissingTexture,
    ExternalImage,
    UnsupportedImageFormat,
    ImageDecode,
    TextureTooLarge,
    UnsupportedMipPolicy,
    UnsupportedTextureTransform,
    TextureTransformRequiresTangentRebuild,
    NonFiniteConditionedData,
    DegenerateGeometry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderFinding {
    pub code: RenderFindingCode,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderConditioningReceipt {
    pub schema_version: String,
    pub status: RenderPreparationStatus,
    pub source_identity: AssetIdentity,
    pub request_sha256: String,
    pub producer: RenderProducerIdentity,
    pub package: Option<RenderAssetPackage>,
    pub findings: Vec<RenderFinding>,
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderProducerIdentity {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderAssetPackage {
    pub package_id: String,
    pub schema_version: String,
    pub source_identity: AssetIdentity,
    pub transform: RenderTransform,
    pub meshes: Vec<RenderMesh>,
    pub materials: Vec<RenderMaterial>,
    pub textures: Vec<RenderTexture>,
    pub provenance: RenderAssetProvenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderTransform {
    pub meters_per_unit: f64,
    pub source_vertical_axis: Axis,
    pub canonical_vertical_axis: Axis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderMesh {
    pub mesh_id: String,
    pub source_mesh_index: usize,
    pub source_primitive_index: usize,
    pub positions_m: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub uv0: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub material_id: String,
    pub generated_normals: bool,
    pub generated_tangents: bool,
    pub tangent_fallback_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderMaterial {
    pub material_id: String,
    pub base_color_rgba: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub alpha_mode: RenderAlphaMode,
    /// glTF `alphaCutoff`, present exactly when `alpha_mode` is `Mask`
    /// (glTF default 0.5). Absent from serialization otherwise, so packages
    /// of opaque assets keep their digests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_cutoff: Option<f32>,
    pub double_sided: bool,
    pub base_color_texture_id: Option<String>,
    pub metallic_roughness_texture_id: Option<String>,
    pub normal_texture_id: Option<String>,
    pub occlusion_texture_id: Option<String>,
    pub emissive_texture_id: Option<String>,
    pub emissive_factor_rgb: [f32; 3],
    /// The current native bridge supports one canonical UV0 transform per
    /// conditioned primitive. Texture infos with conflicting transforms or a
    /// non-zero alternate texcoord set are rejected before package sealing.
    pub texture_transform: RenderTextureTransform,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderTextureTransform {
    pub offset: [f32; 2],
    pub scale: [f32; 2],
    pub rotation_radians: f32,
}

impl Default for RenderTextureTransform {
    fn default() -> Self {
        Self::identity()
    }
}

impl RenderTextureTransform {
    pub const fn identity() -> Self {
        Self {
            offset: [0.0, 0.0],
            scale: [1.0, 1.0],
            rotation_radians: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderAlphaMode {
    Opaque,
    Mask,
    Blend,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderTexture {
    pub texture_id: String,
    pub source_texture_index: usize,
    pub source_image_index: usize,
    pub source_sha256: String,
    pub mime_type: String,
    pub width_px: u32,
    pub height_px: u32,
    pub mip_levels: u32,
    pub color_space: RenderTextureColorSpace,
    pub rgba8: Vec<u8>,
    /// Additional levels after the base level in descending resolution order.
    /// The base level remains in `rgba8` so v1 consumers can identify the
    /// source dimensions without decoding a tagged payload.
    #[serde(default)]
    pub mip_chain: Vec<RenderTextureMip>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderTextureMip {
    pub width_px: u32,
    pub height_px: u32,
    pub rgba8: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderTextureColorSpace {
    Srgb,
    Linear,
    NormalMap,
    Data,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderAssetProvenance {
    pub producer: RenderProducerIdentity,
    pub source_sha256: String,
    pub source_byte_length: u64,
    pub request_sha256: String,
    pub inspection_report_sha256: String,
}

/// Condition a self-contained GLB into a deterministic render package.
/// Malformed containers remain hard errors; valid-but-unsupported render
/// features return a rejected, finding-carrying receipt.
pub fn condition_render_asset(
    bytes: &[u8],
    request: &RenderConditioningRequest,
) -> Result<RenderConditioningReceipt, AssetContractError> {
    let report = inspect_glb(bytes)?;
    let (document, binary, _) = parse_glb(bytes)?;
    let request_value = serde_json::to_value(request)
        .map_err(|error| AssetContractError::Json(error.to_string()))?;
    let request_sha256 = sha256_hex(canonical_json(&request_value).as_bytes());
    let producer = render_producer_identity();
    let mut findings = Vec::new();
    validate_request(request, &mut findings);

    let mut materials = collect_materials(&document, binary, request, &mut findings);
    let has_unmaterialed_primitive = array_or_empty(&document, "meshes")?.iter().any(|mesh| {
        mesh.get("primitives")
            .and_then(Value::as_array)
            .is_some_and(|primitives| {
                primitives
                    .iter()
                    .any(|primitive| primitive.get("material").is_none())
            })
    });
    if has_unmaterialed_primitive
        && !materials
            .iter()
            .any(|material| material.material_id == "material-default")
    {
        materials.push(default_material());
    }
    let mut meshes = Vec::new();
    let mut mesh_ids = BTreeSet::new();
    for (mesh_index, mesh) in array_or_empty(&document, "meshes")?.iter().enumerate() {
        let mesh_name = mesh
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("mesh_{mesh_index}"));
        let primitives = mesh
            .get("primitives")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AssetContractError::Contract(format!(
                    "mesh {mesh_index}.primitives must be an array"
                ))
            })?;
        for (primitive_index, primitive) in primitives.iter().enumerate() {
            let subject = format!("mesh {mesh_index} primitive {primitive_index}");
            let mode = primitive.get("mode").and_then(Value::as_u64).unwrap_or(4) as u32;
            if mode != 4 {
                finding(
                    &mut findings,
                    RenderFindingCode::UnsupportedPrimitiveMode,
                    &subject,
                    format!("render conditioning supports triangle mode 4, found {mode}"),
                );
                continue;
            }
            let attributes = primitive
                .get("attributes")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    AssetContractError::Contract(format!("{subject} has no attributes object"))
                })?;
            let position_index = required_accessor(attributes, "POSITION", &subject)?;
            let position_info = accessor_info(&document, binary, position_index)?;
            let source_positions = read_positions(binary, &position_info, position_index)?;
            let positions = source_positions
                .iter()
                .map(|point| transform_position(*point, request))
                .collect::<Vec<_>>();
            let mut indices = if let Some(index_value) = primitive.get("indices") {
                let accessor_index = index_value.as_u64().ok_or_else(|| {
                    AssetContractError::Contract(format!(
                        "{subject} indices must be an accessor index"
                    ))
                })? as usize;
                let info = accessor_info(&document, binary, accessor_index)?;
                read_indices(binary, &info, accessor_index)?
            } else {
                (0..positions.len())
                    .map(|index| index as u32)
                    .collect::<Vec<_>>()
            };
            if indices.len() % 3 != 0 {
                finding(
                    &mut findings,
                    RenderFindingCode::InvalidAttribute,
                    &subject,
                    "triangle index count is not divisible by three",
                );
                continue;
            }
            if indices
                .iter()
                .any(|index| *index as usize >= positions.len())
            {
                return Err(AssetContractError::Contract(format!(
                    "{subject} index references a vertex outside POSITION"
                )));
            }
            if transform_has_negative_determinant(request.vertical_axis) {
                for triangle in indices.chunks_exact_mut(3) {
                    triangle.swap(1, 2);
                }
            }
            if indices.chunks_exact(3).any(|triangle| {
                let a = positions[triangle[0] as usize];
                let b = positions[triangle[1] as usize];
                let c = positions[triangle[2] as usize];
                let area = length(cross(sub(b, a), sub(c, a)));
                !area.is_finite() || area <= f32::EPSILON
            }) {
                finding(
                    &mut findings,
                    RenderFindingCode::DegenerateGeometry,
                    &subject,
                    "triangle geometry has zero or non-finite area",
                );
            }

            let material_id = material_id_for(primitive, materials.len(), &subject, &mut findings);
            let texture_transform = materials
                .iter()
                .find(|material| material.material_id == material_id)
                .map(|material| material.texture_transform)
                .unwrap_or_default();
            let uv0 = match attributes.get("TEXCOORD_0") {
                Some(value) => {
                    let accessor_index = accessor_index_value(value, "TEXCOORD_0", &subject)?;
                    read_attribute_vec2(
                        binary,
                        &accessor_info(&document, binary, accessor_index)?,
                        accessor_index,
                    )?
                }
                None if request.require_uv0 => {
                    finding(
                        &mut findings,
                        RenderFindingCode::MissingUv0,
                        &subject,
                        "render conditioning requires TEXCOORD_0",
                    );
                    vec![[0.0; 2]; positions.len()]
                }
                None => vec![[0.0; 2]; positions.len()],
            };
            let uv0 = uv0
                .into_iter()
                .map(|uv| apply_texture_transform(uv, texture_transform))
                .collect::<Vec<_>>();
            if uv0.len() != positions.len() {
                finding(
                    &mut findings,
                    RenderFindingCode::InvalidAttribute,
                    &subject,
                    "TEXCOORD_0 count does not match POSITION count",
                );
            }

            let (normals, generated_normals) = match attributes.get("NORMAL") {
                Some(value) => {
                    let accessor_index = accessor_index_value(value, "NORMAL", &subject)?;
                    let source = read_attribute_vec3(
                        binary,
                        &accessor_info(&document, binary, accessor_index)?,
                        accessor_index,
                    )?;
                    (
                        source
                            .into_iter()
                            .map(|normal| transform_normal(normal, request.vertical_axis))
                            .collect::<Vec<_>>(),
                        false,
                    )
                }
                None if request.generate_normals => (generate_normals(&positions, &indices), true),
                None => {
                    finding(
                        &mut findings,
                        RenderFindingCode::MissingNormals,
                        &subject,
                        "NORMAL is absent and normal generation is disabled",
                    );
                    (vec![[0.0, 1.0, 0.0]; positions.len()], false)
                }
            };
            if normals.len() != positions.len() {
                finding(
                    &mut findings,
                    RenderFindingCode::InvalidAttribute,
                    &subject,
                    "NORMAL count does not match POSITION count",
                );
            }

            let (tangents, generated_tangents, tangent_fallback_count) =
                match attributes.get("TANGENT") {
                    Some(_) if texture_transform != RenderTextureTransform::identity() => {
                        if request.generate_tangents {
                            let generated = generate_tangents(&positions, &normals, &uv0, &indices);
                            (generated.0, true, generated.1)
                        } else {
                            finding(
                                &mut findings,
                                RenderFindingCode::TextureTransformRequiresTangentRebuild,
                                &subject,
                                "a non-identity texture transform requires tangent regeneration",
                            );
                            (vec![[1.0, 0.0, 0.0, 1.0]; positions.len()], false, 0)
                        }
                    }
                    Some(value) => {
                        let accessor_index = accessor_index_value(value, "TANGENT", &subject)?;
                        let source = read_attribute_vec4(
                            binary,
                            &accessor_info(&document, binary, accessor_index)?,
                            accessor_index,
                        )?;
                        (
                            source
                                .into_iter()
                                .map(|tangent| transform_tangent(tangent, request.vertical_axis))
                                .collect::<Vec<_>>(),
                            false,
                            0,
                        )
                    }
                    None if request.generate_tangents => {
                        let generated = generate_tangents(&positions, &normals, &uv0, &indices);
                        (generated.0, true, generated.1)
                    }
                    None => {
                        finding(
                            &mut findings,
                            RenderFindingCode::MissingTangents,
                            &subject,
                            "TANGENT is absent and tangent generation is disabled",
                        );
                        (vec![[1.0, 0.0, 0.0, 1.0]; positions.len()], false, 0)
                    }
                };
            if tangents.len() != positions.len() {
                finding(
                    &mut findings,
                    RenderFindingCode::InvalidAttribute,
                    &subject,
                    "TANGENT count does not match POSITION count",
                );
            }
            if !positions.iter().flatten().all(|value| value.is_finite())
                || !normals.iter().flatten().all(|value| value.is_finite())
                || !tangents.iter().flatten().all(|value| value.is_finite())
                || !uv0.iter().flatten().all(|value| value.is_finite())
            {
                finding(
                    &mut findings,
                    RenderFindingCode::NonFiniteConditionedData,
                    &subject,
                    "conditioned vertex attributes contain non-finite values",
                );
            }
            let mesh_id = if primitives.len() == 1 {
                mesh_name.clone()
            } else {
                format!("{mesh_name}_p{primitive_index}")
            };
            if !mesh_ids.insert(mesh_id.clone()) {
                return Err(AssetContractError::Contract(format!(
                    "duplicate conditioned mesh id {mesh_id}"
                )));
            }
            meshes.push(RenderMesh {
                mesh_id,
                source_mesh_index: mesh_index,
                source_primitive_index: primitive_index,
                positions_m: positions,
                normals,
                tangents,
                uv0,
                indices,
                material_id,
                generated_normals,
                generated_tangents,
                tangent_fallback_count,
            });
        }
    }
    sort_findings(&mut findings);
    let textures = if findings.is_empty() {
        collect_textures(&document, binary, request, &mut findings)?
    } else {
        Vec::new()
    };
    sort_findings(&mut findings);

    let package = if findings.is_empty() {
        let mut package = RenderAssetPackage {
            package_id: String::new(),
            schema_version: RENDER_ASSET_PACKAGE_SCHEMA.into(),
            source_identity: report.identity.clone(),
            transform: RenderTransform {
                meters_per_unit: request.meters_per_unit,
                source_vertical_axis: request.vertical_axis,
                canonical_vertical_axis: Axis::Y,
            },
            meshes,
            materials,
            textures,
            provenance: RenderAssetProvenance {
                producer: producer.clone(),
                source_sha256: report.identity.source_sha256.clone(),
                source_byte_length: report.identity.byte_length,
                request_sha256: request_sha256.clone(),
                inspection_report_sha256: report.canonical_report_sha256.clone(),
            },
        };
        if findings.is_empty() {
            let mut value = serde_json::to_value(&package)
                .map_err(|error| AssetContractError::Json(error.to_string()))?;
            value
                .as_object_mut()
                .expect("render package serializes as an object")
                .remove("package_id");
            package.package_id = format!(
                "render_asset_sha256_{}",
                sha256_hex(canonical_json(&value).as_bytes())
            );
            validate_render_asset_package(&package)?;
            Some(package)
        } else {
            None
        }
    } else {
        None
    };

    let mut receipt = RenderConditioningReceipt {
        schema_version: RENDER_ASSET_RECEIPT_SCHEMA.into(),
        status: if package.is_some() {
            RenderPreparationStatus::Ready
        } else {
            RenderPreparationStatus::Rejected
        },
        source_identity: report.identity,
        request_sha256,
        producer,
        package,
        findings,
        receipt_sha256: String::new(),
    };
    let mut value = serde_json::to_value(&receipt)
        .map_err(|error| AssetContractError::Json(error.to_string()))?;
    value
        .as_object_mut()
        .expect("render receipt serializes as an object")
        .remove("receipt_sha256");
    receipt.receipt_sha256 = sha256_hex(canonical_json(&value).as_bytes());
    Ok(receipt)
}

/// Independently validate a conditioned package without executing a backend.
/// This is the authority hook used by later scene/graphics receipt validators.
pub fn validate_render_asset_package(
    package: &RenderAssetPackage,
) -> Result<(), AssetContractError> {
    if package.schema_version != RENDER_ASSET_PACKAGE_SCHEMA {
        return Err(AssetContractError::Contract(format!(
            "unsupported render package schema {}",
            package.schema_version
        )));
    }
    valid_id(&package.package_id, "render package id")?;
    valid_id(
        &package.source_identity.asset_id,
        "render package source asset id",
    )?;
    if package.source_identity.source_sha256.len() != 64
        || !package
            .source_identity
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(AssetContractError::Contract(
            "render package source digest is not SHA-256 hex".into(),
        ));
    }
    if package.source_identity.byte_length == 0 {
        return Err(AssetContractError::Contract(
            "render package source byte length must be nonzero".into(),
        ));
    }
    if !package.transform.meters_per_unit.is_finite() || package.transform.meters_per_unit <= 0.0 {
        return Err(AssetContractError::Contract(
            "render package meters_per_unit must be finite and positive".into(),
        ));
    }
    if package.meshes.is_empty() {
        return Err(AssetContractError::Contract(
            "render package must contain at least one mesh".into(),
        ));
    }
    if package.materials.is_empty() {
        return Err(AssetContractError::Contract(
            "render package must contain at least one material".into(),
        ));
    }
    if package.provenance.source_sha256 != package.source_identity.source_sha256
        || package.provenance.source_byte_length != package.source_identity.byte_length
    {
        return Err(AssetContractError::Contract(
            "render package provenance does not match source identity".into(),
        ));
    }
    valid_sha256_hex(&package.provenance.request_sha256, "render request digest")?;
    valid_sha256_hex(
        &package.provenance.inspection_report_sha256,
        "render inspection digest",
    )?;
    if package.provenance.producer.name.trim().is_empty()
        || package.provenance.producer.version.trim().is_empty()
    {
        return Err(AssetContractError::Contract(
            "render package producer identity must be non-empty".into(),
        ));
    }
    let mut mesh_ids = BTreeSet::new();
    for mesh in &package.meshes {
        valid_id(&mesh.mesh_id, "render mesh id")?;
        if !mesh_ids.insert(mesh.mesh_id.as_str()) {
            return Err(AssetContractError::Contract(format!(
                "duplicate render mesh {}",
                mesh.mesh_id
            )));
        }
        if mesh.positions_m.is_empty()
            || mesh.indices.is_empty()
            || mesh.positions_m.len() != mesh.normals.len()
            || mesh.positions_m.len() != mesh.tangents.len()
            || mesh.positions_m.len() != mesh.uv0.len()
            || mesh.indices.len() % 3 != 0
            || mesh
                .indices
                .iter()
                .any(|index| *index as usize >= mesh.positions_m.len())
            || mesh.tangent_fallback_count > mesh.indices.len() / 3
            || !mesh
                .positions_m
                .iter()
                .flatten()
                .chain(mesh.normals.iter().flatten())
                .chain(mesh.tangents.iter().flatten())
                .chain(mesh.uv0.iter().flatten())
                .all(|value| value.is_finite())
        {
            return Err(AssetContractError::Contract(format!(
                "render mesh {} has inconsistent or non-finite buffers",
                mesh.mesh_id
            )));
        }
        if mesh.indices.chunks_exact(3).any(|triangle| {
            let a = mesh.positions_m[triangle[0] as usize];
            let b = mesh.positions_m[triangle[1] as usize];
            let c = mesh.positions_m[triangle[2] as usize];
            let area = length(cross(sub(b, a), sub(c, a)));
            !area.is_finite() || area <= f32::EPSILON
        }) {
            return Err(AssetContractError::Contract(format!(
                "render mesh {} contains degenerate geometry",
                mesh.mesh_id
            )));
        }
    }
    let mut material_ids = BTreeSet::new();
    for material in &package.materials {
        valid_id(&material.material_id, "render material id")?;
        if !material_ids.insert(material.material_id.as_str()) {
            return Err(AssetContractError::Contract(format!(
                "duplicate render material {}",
                material.material_id
            )));
        }
        validate_render_material(material)?;
    }
    for mesh in &package.meshes {
        if !material_ids.contains(mesh.material_id.as_str()) {
            return Err(AssetContractError::Contract(format!(
                "render mesh {} references unknown material {}",
                mesh.mesh_id, mesh.material_id
            )));
        }
    }
    let mut texture_ids = BTreeSet::new();
    for texture in &package.textures {
        valid_id(&texture.texture_id, "render texture id")?;
        if !texture_ids.insert(texture.texture_id.as_str()) {
            return Err(AssetContractError::Contract(format!(
                "duplicate render texture {}",
                texture.texture_id
            )));
        }
        validate_render_texture(texture)?;
        valid_sha256_hex(&texture.source_sha256, "render texture source digest")?;
    }
    for material in &package.materials {
        for texture_id in [
            material.base_color_texture_id.as_deref(),
            material.metallic_roughness_texture_id.as_deref(),
            material.normal_texture_id.as_deref(),
            material.occlusion_texture_id.as_deref(),
            material.emissive_texture_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !texture_ids.contains(texture_id) {
                return Err(AssetContractError::Contract(format!(
                    "render material {} references unknown texture {}",
                    material.material_id, texture_id
                )));
            }
        }
    }
    let mut value = serde_json::to_value(package)
        .map_err(|error| AssetContractError::Json(error.to_string()))?;
    value
        .as_object_mut()
        .expect("render package serializes as an object")
        .remove("package_id");
    let expected_id = format!(
        "render_asset_sha256_{}",
        sha256_hex(canonical_json(&value).as_bytes())
    );
    if package.package_id != expected_id {
        return Err(AssetContractError::Contract(
            "render package identity does not match its typed content".into(),
        ));
    }
    Ok(())
}

fn validate_render_texture(texture: &RenderTexture) -> Result<(), AssetContractError> {
    if texture.width_px == 0 || texture.height_px == 0 || texture.mip_levels == 0 {
        return Err(AssetContractError::Contract(format!(
            "render texture {} has invalid dimensions or mip declaration",
            texture.texture_id
        )));
    }
    let expected_base_bytes = texture_byte_length(texture.width_px, texture.height_px)?;
    if texture.rgba8.len() != expected_base_bytes {
        return Err(AssetContractError::Contract(format!(
            "render texture {} base level has {} bytes, expected {expected_base_bytes}",
            texture.texture_id,
            texture.rgba8.len()
        )));
    }
    let expected_additional_levels = usize::try_from(texture.mip_levels - 1)
        .map_err(|_| AssetContractError::Contract("render mip count overflows".into()))?;
    if texture.mip_chain.len() != expected_additional_levels {
        return Err(AssetContractError::Contract(format!(
            "render texture {} declares {} mip levels but carries {} additional levels",
            texture.texture_id,
            texture.mip_levels,
            texture.mip_chain.len()
        )));
    }
    let mut width = texture.width_px;
    let mut height = texture.height_px;
    for (index, mip) in texture.mip_chain.iter().enumerate() {
        width = (width / 2).max(1);
        height = (height / 2).max(1);
        let expected_bytes = texture_byte_length(width, height)?;
        if mip.width_px != width || mip.height_px != height || mip.rgba8.len() != expected_bytes {
            return Err(AssetContractError::Contract(format!(
                "render texture {} mip {} has invalid dimensions or payload length",
                texture.texture_id,
                index + 1
            )));
        }
    }
    Ok(())
}

fn texture_byte_length(width: u32, height: u32) -> Result<usize, AssetContractError> {
    usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height)?.checked_mul(4))
        })
        .ok_or_else(|| AssetContractError::Contract("render texture size overflows".into()))
}

pub fn validate_render_conditioning_receipt(
    bytes: &[u8],
    request: &RenderConditioningRequest,
    supplied: &RenderConditioningReceipt,
) -> Result<(), AssetContractError> {
    let measured = condition_render_asset(bytes, request)?;
    if &measured != supplied {
        return Err(AssetContractError::Contract(
            "render conditioning receipt differs from independent source/request revalidation"
                .into(),
        ));
    }
    if let Some(package) = &supplied.package {
        validate_render_asset_package(package)?;
    }
    Ok(())
}

fn render_producer_identity() -> RenderProducerIdentity {
    RenderProducerIdentity {
        name: "wge-asset-render-conditioning".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

fn validate_render_material(material: &RenderMaterial) -> Result<(), AssetContractError> {
    if material
        .base_color_rgba
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        || !material.metallic.is_finite()
        || !material.roughness.is_finite()
        || !(0.0..=1.0).contains(&material.metallic)
        || !(0.0..=1.0).contains(&material.roughness)
        || material
            .emissive_factor_rgb
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=16.0).contains(value))
    {
        return Err(AssetContractError::Contract(format!(
            "render material {} contains out-of-range or non-finite factors",
            material.material_id
        )));
    }
    match (material.alpha_mode, material.alpha_cutoff) {
        (RenderAlphaMode::Mask, Some(cutoff)) if cutoff.is_finite() && cutoff > 0.0 && cutoff < 1.0 => {}
        (RenderAlphaMode::Opaque | RenderAlphaMode::Blend, None) => {}
        _ => {
            return Err(AssetContractError::Contract(format!(
                "render material {} alpha cutoff must be in (0, 1) for MASK and absent otherwise",
                material.material_id
            )));
        }
    }
    for texture_id in [
        material.base_color_texture_id.as_deref(),
        material.metallic_roughness_texture_id.as_deref(),
        material.normal_texture_id.as_deref(),
        material.occlusion_texture_id.as_deref(),
        material.emissive_texture_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        valid_id(texture_id, "render material texture id")?;
    }
    validate_texture_transform(&material.texture_transform, &material.material_id)?;
    Ok(())
}

fn validate_texture_transform(
    transform: &RenderTextureTransform,
    material_id: &str,
) -> Result<(), AssetContractError> {
    let finite = transform
        .offset
        .iter()
        .chain(transform.scale.iter())
        .all(|value| value.is_finite() && value.abs() <= 1.0e4)
        && transform.rotation_radians.is_finite()
        && transform.rotation_radians.abs() <= 1.0e4;
    finite.then_some(()).ok_or_else(|| {
        AssetContractError::Contract(format!(
            "render material {material_id} has an unsafe texture transform"
        ))
    })
}

fn validate_request(request: &RenderConditioningRequest, findings: &mut Vec<RenderFinding>) {
    if request.schema_version != RENDER_ASSET_REQUEST_SCHEMA {
        finding(
            findings,
            RenderFindingCode::RequestSchemaUnsupported,
            "schema_version",
            format!("expected {RENDER_ASSET_REQUEST_SCHEMA}"),
        );
    }
    if !request.meters_per_unit.is_finite()
        || request.meters_per_unit <= 0.0
        || request.max_texture_dimension == 0
    {
        finding(
            findings,
            RenderFindingCode::InvalidRequest,
            "request",
            "meters_per_unit must be finite and positive; max_texture_dimension must be nonzero",
        );
    }
}

fn collect_materials(
    document: &Value,
    _binary: &[u8],
    request: &RenderConditioningRequest,
    findings: &mut Vec<RenderFinding>,
) -> Vec<RenderMaterial> {
    let Ok(material_values) = array_or_empty(document, "materials") else {
        return Vec::new();
    };
    if material_values.is_empty() {
        return vec![default_material()];
    }
    material_values
        .iter()
        .enumerate()
        .map(|(index, material)| parse_material(document, index, material, request, findings))
        .collect()
}

fn parse_material(
    document: &Value,
    index: usize,
    material: &Value,
    _request: &RenderConditioningRequest,
    findings: &mut Vec<RenderFinding>,
) -> RenderMaterial {
    let material_id = format!("material_{index}");
    let pbr = material.get("pbrMetallicRoughness").unwrap_or(&Value::Null);
    let base_color_rgba = number_array::<4>(pbr.get("baseColorFactor"), [1.0, 1.0, 1.0, 1.0]);
    let metallic = number_value(pbr.get("metallicFactor"), 1.0);
    let roughness = number_value(pbr.get("roughnessFactor"), 1.0);
    let emissive_factor = number_array::<3>(material.get("emissiveFactor"), [0.0, 0.0, 0.0]);
    let texture_transform =
        collect_texture_transform(document, pbr, material, &material_id, findings);
    let valid = base_color_rgba
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        && metallic.is_finite()
        && (0.0..=1.0).contains(&metallic)
        && roughness.is_finite()
        && (0.0..=1.0).contains(&roughness)
        && emissive_factor
            .iter()
            .all(|value| value.is_finite() && (0.0..=16.0).contains(value));
    if !valid {
        finding(
            findings,
            RenderFindingCode::InvalidMaterial,
            &material_id,
            "material scalar/factor is outside the bounded finite domain",
        );
    }
    let alpha_mode = match material
        .get("alphaMode")
        .and_then(Value::as_str)
        .unwrap_or("OPAQUE")
    {
        "OPAQUE" => RenderAlphaMode::Opaque,
        "MASK" => RenderAlphaMode::Mask,
        "BLEND" => RenderAlphaMode::Blend,
        _ => {
            finding(
                findings,
                RenderFindingCode::InvalidMaterial,
                &material_id,
                "alphaMode must be OPAQUE, MASK, or BLEND",
            );
            RenderAlphaMode::Opaque
        }
    };
    let alpha_cutoff = (alpha_mode == RenderAlphaMode::Mask).then(|| {
        let cutoff = number_value(material.get("alphaCutoff"), 0.5);
        if !cutoff.is_finite() || cutoff <= 0.0 || cutoff >= 1.0 {
            finding(
                findings,
                RenderFindingCode::InvalidMaterial,
                &material_id,
                "alphaCutoff must be finite and inside (0, 1)",
            );
            0.5
        } else {
            cutoff
        }
    });
    let base_color_texture_id =
        material_texture_id(document, pbr, "baseColorTexture", &material_id, findings);
    let metallic_roughness_texture_id = material_texture_id(
        document,
        pbr,
        "metallicRoughnessTexture",
        &material_id,
        findings,
    );
    let normal_texture_id =
        material_texture_id(document, material, "normalTexture", &material_id, findings);
    let occlusion_texture_id = material_texture_id(
        document,
        material,
        "occlusionTexture",
        &material_id,
        findings,
    );
    let emissive_texture_id = material_texture_id(
        document,
        material,
        "emissiveTexture",
        &material_id,
        findings,
    );
    RenderMaterial {
        material_id,
        base_color_rgba,
        metallic,
        roughness,
        alpha_mode,
        alpha_cutoff,
        double_sided: material
            .get("doubleSided")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        base_color_texture_id,
        metallic_roughness_texture_id,
        normal_texture_id,
        occlusion_texture_id,
        emissive_texture_id,
        emissive_factor_rgb: emissive_factor,
        texture_transform,
    }
}

fn collect_texture_transform(
    _document: &Value,
    pbr: &Value,
    material: &Value,
    material_id: &str,
    findings: &mut Vec<RenderFinding>,
) -> RenderTextureTransform {
    let infos = [
        (pbr, "baseColorTexture"),
        (pbr, "metallicRoughnessTexture"),
        (material, "normalTexture"),
        (material, "occlusionTexture"),
        (material, "emissiveTexture"),
    ];
    let mut selected = None;
    for (container, key) in infos {
        if container.get(key).is_none() {
            continue;
        }
        let candidate = texture_info_transform(container, key, material_id, findings);
        if let Some(first) = selected {
            if candidate != first {
                finding(
                    findings,
                    RenderFindingCode::InvalidMaterial,
                    material_id,
                    "native conditioning requires one shared KHR_texture_transform across present material texture roles",
                );
            }
        } else {
            selected = Some(candidate);
        }
    }
    selected.unwrap_or_default()
}

fn texture_info_transform(
    container: &Value,
    key: &str,
    material_id: &str,
    findings: &mut Vec<RenderFinding>,
) -> RenderTextureTransform {
    let Some(texture_info) = container.get(key) else {
        return RenderTextureTransform::identity();
    };
    let direct_texcoord = texture_coord(texture_info, key, material_id, findings);
    if direct_texcoord != 0 {
        finding(
            findings,
            RenderFindingCode::UnsupportedTextureTransform,
            material_id,
            format!("{key} selects TEXCOORD_{direct_texcoord}; only TEXCOORD_0 is supported"),
        );
    }
    let Some(transform) = texture_info
        .get("extensions")
        .and_then(|extensions| extensions.get("KHR_texture_transform"))
    else {
        return RenderTextureTransform::identity();
    };
    let extension_texcoord = texture_coord(
        transform,
        &format!("{key}.KHR_texture_transform"),
        material_id,
        findings,
    );
    if extension_texcoord != 0 {
        finding(
            findings,
            RenderFindingCode::UnsupportedTextureTransform,
            material_id,
            format!(
                "{key}.KHR_texture_transform selects TEXCOORD_{extension_texcoord}; only TEXCOORD_0 is supported"
            ),
        );
    }
    let offset = transform_array(
        transform.get("offset"),
        [0.0, 0.0],
        key,
        "offset",
        material_id,
        findings,
    );
    let scale = transform_array(
        transform.get("scale"),
        [1.0, 1.0],
        key,
        "scale",
        material_id,
        findings,
    );
    let rotation = match transform.get("rotation") {
        None => 0.0,
        Some(value) => match value.as_f64() {
            Some(value) => value as f32,
            None => {
                finding(
                    findings,
                    RenderFindingCode::InvalidMaterial,
                    material_id,
                    format!("{key}.KHR_texture_transform.rotation must be a number"),
                );
                0.0
            }
        },
    };
    let candidate = RenderTextureTransform {
        offset,
        scale,
        rotation_radians: rotation,
    };
    if validate_texture_transform(&candidate, material_id).is_err() {
        finding(
            findings,
            RenderFindingCode::InvalidMaterial,
            material_id,
            format!("{key}.KHR_texture_transform is outside the bounded finite domain"),
        );
    }
    candidate
}

fn texture_coord(
    value: &Value,
    label: &str,
    material_id: &str,
    findings: &mut Vec<RenderFinding>,
) -> u64 {
    let Some(value) = value.get("texCoord") else {
        return 0;
    };
    match value.as_u64() {
        Some(value) => value,
        None => {
            finding(
                findings,
                RenderFindingCode::InvalidMaterial,
                material_id,
                format!("{label}.texCoord must be a non-negative integer"),
            );
            0
        }
    }
}

fn transform_array<const N: usize>(
    value: Option<&Value>,
    default: [f32; N],
    texture_key: &str,
    field: &str,
    material_id: &str,
    findings: &mut Vec<RenderFinding>,
) -> [f32; N] {
    let Some(values) = value else {
        return default;
    };
    let Some(values) = values.as_array() else {
        finding(
            findings,
            RenderFindingCode::InvalidMaterial,
            material_id,
            format!("{texture_key}.KHR_texture_transform.{field} must be an array"),
        );
        return default;
    };
    if values.len() != N {
        finding(
            findings,
            RenderFindingCode::InvalidMaterial,
            material_id,
            format!("{texture_key}.KHR_texture_transform.{field} must contain {N} numbers"),
        );
        return default;
    }
    let mut result = default;
    for (index, value) in values.iter().enumerate() {
        let Some(value) = value.as_f64() else {
            finding(
                findings,
                RenderFindingCode::InvalidMaterial,
                material_id,
                format!("{texture_key}.KHR_texture_transform.{field} must contain numbers"),
            );
            return default;
        };
        result[index] = value as f32;
    }
    result
}

fn material_texture_id(
    document: &Value,
    container: &Value,
    key: &str,
    material_id: &str,
    findings: &mut Vec<RenderFinding>,
) -> Option<String> {
    let index = container.get(key)?.get("index")?.as_u64()? as usize;
    let textures = array_or_empty(document, "textures").ok()?;
    if textures.get(index).is_none() {
        finding(
            findings,
            RenderFindingCode::MissingTexture,
            material_id,
            format!("texture index {index} is outside textures"),
        );
        None
    } else {
        Some(format!("texture_{index}"))
    }
}

fn collect_textures(
    document: &Value,
    binary: &[u8],
    request: &RenderConditioningRequest,
    findings: &mut Vec<RenderFinding>,
) -> Result<Vec<RenderTexture>, AssetContractError> {
    let materials = array_or_empty(document, "materials")?;
    let textures = array_or_empty(document, "textures")?;
    let mut usages = BTreeMap::<usize, RenderTextureColorSpace>::new();
    for material in materials {
        let pbr = material.get("pbrMetallicRoughness").unwrap_or(&Value::Null);
        for (key, color_space) in [
            ("baseColorTexture", RenderTextureColorSpace::Srgb),
            ("metallicRoughnessTexture", RenderTextureColorSpace::Data),
        ] {
            record_texture_usage(pbr.get(key), color_space, textures, &mut usages, findings);
        }
        for (key, color_space) in [
            ("normalTexture", RenderTextureColorSpace::NormalMap),
            ("occlusionTexture", RenderTextureColorSpace::Data),
            ("emissiveTexture", RenderTextureColorSpace::Srgb),
        ] {
            record_texture_usage(
                material.get(key),
                color_space,
                textures,
                &mut usages,
                findings,
            );
        }
    }
    let mask_cutoffs = mask_coverage_cutoffs(materials, findings);
    let mut result = Vec::new();
    let images = array_or_empty(document, "images")?;
    for (texture_index, color_space) in usages {
        let Some(texture) = textures.get(texture_index) else {
            finding(
                findings,
                RenderFindingCode::MissingTexture,
                &format!("texture_{texture_index}"),
                "texture reference disappeared during conditioning",
            );
            continue;
        };
        let Some(image_index) = texture.get("source").and_then(Value::as_u64) else {
            finding(
                findings,
                RenderFindingCode::MissingTexture,
                &format!("texture_{texture_index}"),
                "texture has no source image",
            );
            continue;
        };
        let image_index = image_index as usize;
        let Some(image) = images.get(image_index) else {
            finding(
                findings,
                RenderFindingCode::MissingTexture,
                &format!("texture_{texture_index}"),
                format!("source image {image_index} is out of range"),
            );
            continue;
        };
        let mime_type = image
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let encoded = match image_bytes(document, binary, image_index, image, findings) {
            Ok(encoded) => encoded,
            Err(error) => {
                finding(
                    findings,
                    RenderFindingCode::ImageDecode,
                    &format!("texture_{texture_index}"),
                    error.to_string(),
                );
                continue;
            }
        };
        let decoded = match image::load_from_memory(&encoded) {
            Ok(image) => image,
            Err(error) => {
                finding(
                    findings,
                    RenderFindingCode::ImageDecode,
                    &format!("texture_{texture_index}"),
                    error.to_string(),
                );
                continue;
            }
        };
        let width = decoded.width();
        let height = decoded.height();
        if width > request.max_texture_dimension || height > request.max_texture_dimension {
            finding(
                findings,
                RenderFindingCode::TextureTooLarge,
                &format!("texture_{texture_index}"),
                format!(
                    "decoded image is {width}x{height}, maximum is {}",
                    request.max_texture_dimension
                ),
            );
            continue;
        }
        let rgba8 = decoded.to_rgba8().into_raw();
        let mip_chain = match request.mip_policy {
            RenderMipPolicy::SingleLevelExplicit => Vec::new(),
            RenderMipPolicy::GenerateCpuChain => {
                let mut chain = generate_mip_chain(width, height, &rgba8, color_space);
                if let Some(cutoff) = mask_cutoffs.get(&texture_index) {
                    preserve_alpha_coverage(&rgba8, &mut chain, *cutoff);
                }
                chain
            }
        };
        let mip_levels = u32::try_from(mip_chain.len() + 1).map_err(|_| {
            AssetContractError::Contract(format!(
                "texture_{texture_index} generated mip count exceeds the contract"
            ))
        })?;
        result.push(RenderTexture {
            texture_id: format!("texture_{texture_index}"),
            source_texture_index: texture_index,
            source_image_index: image_index,
            source_sha256: sha256_hex(&encoded),
            mime_type,
            width_px: width,
            height_px: height,
            mip_levels,
            color_space,
            rgba8,
            mip_chain,
        });
    }
    result.sort_by(|left, right| left.texture_id.cmp(&right.texture_id));
    Ok(result)
}

/// Base-colour textures of MASK materials, with the cutoff their mips must
/// preserve coverage at. A texture shared by two MASK materials with different
/// cutoffs cannot preserve both and is a finding.
fn mask_coverage_cutoffs(
    materials: &[Value],
    findings: &mut Vec<RenderFinding>,
) -> BTreeMap<usize, f32> {
    let mut cutoffs = BTreeMap::<usize, f32>::new();
    for material in materials {
        if material.get("alphaMode").and_then(Value::as_str) != Some("MASK") {
            continue;
        }
        let Some(texture_index) = material
            .get("pbrMetallicRoughness")
            .and_then(|pbr| pbr.get("baseColorTexture"))
            .and_then(|info| info.get("index"))
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
        else {
            continue;
        };
        let cutoff = number_value(material.get("alphaCutoff"), 0.5);
        if let Some(existing) = cutoffs.insert(texture_index, cutoff) {
            if existing != cutoff {
                finding(
                    findings,
                    RenderFindingCode::InvalidMaterial,
                    &format!("texture_{texture_index}"),
                    "MASK materials sharing a base colour texture must share one alphaCutoff",
                );
            }
        }
    }
    cutoffs
}

/// Fraction of texels that pass the alpha test (`alpha >= cutoff`).
fn alpha_coverage(rgba8: &[u8], cutoff: f32) -> f64 {
    let texels = rgba8.len() / 4;
    if texels == 0 {
        return 0.0;
    }
    let threshold = f64::from(cutoff) * 255.0;
    let passing = rgba8
        .chunks_exact(4)
        .filter(|texel| f64::from(texel[3]) >= threshold)
        .count();
    passing as f64 / texels as f64
}

fn scaled_alpha(alpha: u8, scale: f64) -> u8 {
    (f64::from(alpha) * scale).round().clamp(0.0, 255.0) as u8
}

/// Coverage-preserving alpha mips (Castaño 2010). A box filter averages
/// alpha, so an alpha-tested texture loses coverage at every level and
/// foliage thins to sticks at distance. Each level's alpha is scaled so the
/// fraction of texels passing `cutoff` matches the base level. The scale is
/// found by bisection over the box-filtered (unscaled) level, so no level
/// inherits a previous level's rescale. Colour channels are untouched.
fn preserve_alpha_coverage(base: &[u8], levels: &mut [RenderTextureMip], cutoff: f32) {
    let target = alpha_coverage(base, cutoff);
    let threshold = f64::from(cutoff) * 255.0;
    for level in levels {
        let coverage_at = |scale: f64| {
            let texels = level.rgba8.len() / 4;
            let passing = level
                .rgba8
                .chunks_exact(4)
                .filter(|texel| f64::from(scaled_alpha(texel[3], scale)) >= threshold)
                .count();
            passing as f64 / texels as f64
        };
        // Coverage is monotone in the scale. Bisect for the smallest scale
        // whose coverage reaches the target, then keep whichever neighbour
        // lands closer.
        let (mut low, mut high) = (0.0f64, 255.0f64);
        for _ in 0..40 {
            let middle = 0.5 * (low + high);
            if coverage_at(middle) >= target {
                high = middle;
            } else {
                low = middle;
            }
        }
        let scale = if (coverage_at(low) - target).abs() < (coverage_at(high) - target).abs() {
            low
        } else {
            high
        };
        for texel in level.rgba8.chunks_exact_mut(4) {
            texel[3] = scaled_alpha(texel[3], scale);
        }
    }
}

/// Build a complete deterministic RGBA8 mip chain in the authority plane.
///
/// The reducer is deliberately semantic-aware: sRGB color channels are
/// averaged in linear light, while normal maps are averaged as vectors and
/// renormalized. Data/linear payloads use a bounded box average. Vulkan may
/// later upload these levels directly; it must not reinterpret a base-only
/// payload as a residency policy.
fn generate_mip_chain(
    width: u32,
    height: u32,
    base: &[u8],
    color_space: RenderTextureColorSpace,
) -> Vec<RenderTextureMip> {
    let mut levels = Vec::new();
    let mut current_width = width;
    let mut current_height = height;
    let mut current = base.to_vec();
    while current_width > 1 || current_height > 1 {
        let next_width = (current_width / 2).max(1);
        let next_height = (current_height / 2).max(1);
        let mut next = vec![0u8; next_width as usize * next_height as usize * 4];
        for y in 0..next_height as usize {
            for x in 0..next_width as usize {
                let source_x_start = x * 2;
                let source_y_start = y * 2;
                let source_x_end = (source_x_start + 2).min(current_width as usize);
                let source_y_end = (source_y_start + 2).min(current_height as usize);
                let mut sample_count = 0.0f64;
                let mut channels = [0.0f64; 4];
                let mut normal = [0.0f64; 3];
                for source_y in source_y_start..source_y_end {
                    for source_x in source_x_start..source_x_end {
                        let offset = (source_y * current_width as usize + source_x) * 4;
                        if color_space == RenderTextureColorSpace::NormalMap {
                            normal[0] += f64::from(current[offset]) / 255.0 * 2.0 - 1.0;
                            normal[1] += f64::from(current[offset + 1]) / 255.0 * 2.0 - 1.0;
                            normal[2] += f64::from(current[offset + 2]) / 255.0 * 2.0 - 1.0;
                            channels[3] += f64::from(current[offset + 3]);
                        } else {
                            for channel in 0..3 {
                                let value = f64::from(current[offset + channel]) / 255.0;
                                channels[channel] += if color_space == RenderTextureColorSpace::Srgb
                                {
                                    srgb_to_linear(value)
                                } else {
                                    value
                                };
                            }
                            channels[3] += f64::from(current[offset + 3]);
                        }
                        sample_count += 1.0;
                    }
                }
                let destination = (y * next_width as usize + x) * 4;
                if color_space == RenderTextureColorSpace::NormalMap {
                    let mut length =
                        normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2];
                    length = length.sqrt();
                    if length > f64::EPSILON {
                        next[destination] = unit_to_byte(normal[0] / length);
                        next[destination + 1] = unit_to_byte(normal[1] / length);
                        next[destination + 2] = unit_to_byte(normal[2] / length);
                    } else {
                        next[destination..destination + 3].copy_from_slice(&[128, 128, 255]);
                    }
                } else {
                    for channel in 0..3 {
                        let average = channels[channel] / sample_count;
                        let encoded = if color_space == RenderTextureColorSpace::Srgb {
                            linear_to_srgb(average)
                        } else {
                            average
                        };
                        next[destination + channel] = normalized_to_byte(encoded);
                    }
                }
                next[destination + 3] = normalized_to_byte(channels[3] / sample_count / 255.0);
            }
        }
        levels.push(RenderTextureMip {
            width_px: next_width,
            height_px: next_height,
            rgba8: next.clone(),
        });
        current_width = next_width;
        current_height = next_height;
        current = next;
    }
    levels
}

fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> f64 {
    let value = value.clamp(0.0, 1.0);
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn normalized_to_byte(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

fn unit_to_byte(value: f64) -> u8 {
    normalized_to_byte(value * 0.5 + 0.5)
}

fn record_texture_usage(
    value: Option<&Value>,
    color_space: RenderTextureColorSpace,
    textures: &[Value],
    usages: &mut BTreeMap<usize, RenderTextureColorSpace>,
    findings: &mut Vec<RenderFinding>,
) {
    let Some(index) = value
        .and_then(|value| value.get("index"))
        .and_then(Value::as_u64)
    else {
        return;
    };
    let index = index as usize;
    if textures.get(index).is_none() {
        finding(
            findings,
            RenderFindingCode::MissingTexture,
            &format!("texture_{index}"),
            "material references a missing texture",
        );
    } else if let Some(existing) = usages.get(&index) {
        if existing != &color_space {
            finding(
                findings,
                RenderFindingCode::InvalidMaterial,
                &format!("texture_{index}"),
                format!(
                    "texture is used with conflicting color-space roles: {existing:?} and {color_space:?}"
                ),
            );
        }
    } else {
        usages.insert(index, color_space);
    }
}

fn sort_findings(findings: &mut Vec<RenderFinding>) {
    findings.sort_by(|left, right| {
        (&left.code, &left.subject, &left.detail).cmp(&(&right.code, &right.subject, &right.detail))
    });
    findings.dedup();
}

fn image_bytes(
    document: &Value,
    binary: &[u8],
    image_index: usize,
    image: &Value,
    findings: &mut Vec<RenderFinding>,
) -> Result<Vec<u8>, AssetContractError> {
    if image.get("uri").is_some() {
        finding(
            findings,
            RenderFindingCode::ExternalImage,
            &format!("image_{image_index}"),
            "render conditioning requires images embedded in the GLB",
        );
        return Ok(Vec::new());
    }
    let view_index = image
        .get("bufferView")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AssetContractError::Contract(format!(
                "image {image_index} has neither uri nor bufferView"
            ))
        })? as usize;
    let views = array_or_empty(document, "bufferViews")?;
    let view = views.get(view_index).ok_or_else(|| {
        AssetContractError::Contract(format!("image {image_index} bufferView is out of range"))
    })?;
    if view.get("buffer").and_then(Value::as_u64).unwrap_or(0) != 0 {
        return Err(AssetContractError::Unsupported(format!(
            "image {image_index} references a non-GLB buffer"
        )));
    }
    let offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let length = view
        .get("byteLength")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AssetContractError::Contract(format!(
                "image {image_index} bufferView has no byteLength"
            ))
        })? as usize;
    let end = offset.checked_add(length).ok_or_else(|| {
        AssetContractError::Contract(format!("image {image_index} byte range overflows"))
    })?;
    if end > binary.len() {
        return Err(AssetContractError::Contract(format!(
            "image {image_index} exceeds the GLB BIN chunk"
        )));
    }
    Ok(binary[offset..end].to_vec())
}

fn default_material() -> RenderMaterial {
    RenderMaterial {
        material_id: "material-default".into(),
        base_color_rgba: [1.0, 1.0, 1.0, 1.0],
        metallic: 0.0,
        roughness: 1.0,
        alpha_mode: RenderAlphaMode::Opaque,
        alpha_cutoff: None,
        double_sided: false,
        base_color_texture_id: None,
        metallic_roughness_texture_id: None,
        normal_texture_id: None,
        occlusion_texture_id: None,
        emissive_texture_id: None,
        emissive_factor_rgb: [0.0, 0.0, 0.0],
        texture_transform: RenderTextureTransform::identity(),
    }
}

fn material_id_for(
    primitive: &Value,
    material_count: usize,
    subject: &str,
    findings: &mut Vec<RenderFinding>,
) -> String {
    let Some(index) = primitive.get("material").and_then(Value::as_u64) else {
        return "material-default".into();
    };
    let index = index as usize;
    if index >= material_count {
        finding(
            findings,
            RenderFindingCode::MissingMaterial,
            subject,
            format!("material index {index} is outside materials"),
        );
        "material-default".into()
    } else {
        format!("material_{index}")
    }
}

fn apply_texture_transform(uv: [f32; 2], transform: RenderTextureTransform) -> [f32; 2] {
    let scaled = [uv[0] * transform.scale[0], uv[1] * transform.scale[1]];
    let (sine, cosine) = transform.rotation_radians.sin_cos();
    [
        cosine * scaled[0] - sine * scaled[1] + transform.offset[0],
        sine * scaled[0] + cosine * scaled[1] + transform.offset[1],
    ]
}

fn required_accessor(
    attributes: &serde_json::Map<String, Value>,
    name: &str,
    subject: &str,
) -> Result<usize, AssetContractError> {
    attributes
        .get(name)
        .ok_or_else(|| AssetContractError::Contract(format!("{subject} has no {name} attribute")))
        .and_then(|value| accessor_index_value(value, name, subject))
}

fn accessor_index_value(
    value: &Value,
    name: &str,
    subject: &str,
) -> Result<usize, AssetContractError> {
    value.as_u64().map(|value| value as usize).ok_or_else(|| {
        AssetContractError::Contract(format!("{subject} {name} accessor index is not an integer"))
    })
}

fn read_attribute_vec2(
    binary: &[u8],
    info: &AccessorInfo<'_>,
    accessor_index: usize,
) -> Result<Vec<[f32; 2]>, AssetContractError> {
    if info.accessor_type != "VEC2" || info.component_type != 5126 || info.normalized {
        return Err(AssetContractError::Unsupported(format!(
            "attribute accessor {accessor_index} must be an unnormalized FLOAT VEC2"
        )));
    }
    let mut values = Vec::with_capacity(info.count);
    for index in 0..info.count {
        let base = info.byte_offset + index * info.byte_stride;
        let mut value = [0.0; 2];
        for (component, slot) in value.iter_mut().enumerate() {
            let offset = base + component * 4;
            *slot = f32::from_le_bytes(binary[offset..offset + 4].try_into().unwrap());
        }
        values.push(value);
    }
    Ok(values)
}

fn read_attribute_vec3(
    binary: &[u8],
    info: &AccessorInfo<'_>,
    accessor_index: usize,
) -> Result<Vec<[f32; 3]>, AssetContractError> {
    if info.accessor_type != "VEC3" || info.component_type != 5126 || info.normalized {
        return Err(AssetContractError::Unsupported(format!(
            "attribute accessor {accessor_index} must be an unnormalized FLOAT VEC3"
        )));
    }
    let mut values = Vec::with_capacity(info.count);
    for index in 0..info.count {
        let base = info.byte_offset + index * info.byte_stride;
        let mut value = [0.0; 3];
        for (component, slot) in value.iter_mut().enumerate() {
            let offset = base + component * 4;
            *slot = f32::from_le_bytes(binary[offset..offset + 4].try_into().unwrap());
        }
        values.push(value);
    }
    Ok(values)
}

fn read_attribute_vec4(
    binary: &[u8],
    info: &AccessorInfo<'_>,
    accessor_index: usize,
) -> Result<Vec<[f32; 4]>, AssetContractError> {
    if info.accessor_type != "VEC4" || info.component_type != 5126 || info.normalized {
        return Err(AssetContractError::Unsupported(format!(
            "attribute accessor {accessor_index} must be an unnormalized FLOAT VEC4"
        )));
    }
    let mut values = Vec::with_capacity(info.count);
    for index in 0..info.count {
        let base = info.byte_offset + index * info.byte_stride;
        let mut value = [0.0; 4];
        for (component, slot) in value.iter_mut().enumerate() {
            let offset = base + component * 4;
            *slot = f32::from_le_bytes(binary[offset..offset + 4].try_into().unwrap());
        }
        values.push(value);
    }
    Ok(values)
}

fn transform_position(point: [f64; 3], request: &RenderConditioningRequest) -> [f32; 3] {
    let point = match request.vertical_axis {
        Axis::X => [point[1], point[0], point[2]],
        Axis::Y => point,
        Axis::Z => [point[0], point[2], -point[1]],
    };
    point.map(|value| (value * request.meters_per_unit) as f32)
}

fn transform_normal(normal: [f32; 3], axis: Axis) -> [f32; 3] {
    normalize(match axis {
        Axis::X => [normal[1], normal[0], normal[2]],
        Axis::Y => normal,
        Axis::Z => [normal[0], normal[2], -normal[1]],
    })
}

fn transform_tangent(tangent: [f32; 4], axis: Axis) -> [f32; 4] {
    let mut result = match axis {
        Axis::X => [tangent[1], tangent[0], tangent[2], -tangent[3]],
        Axis::Y => tangent,
        Axis::Z => [tangent[0], tangent[2], -tangent[1], tangent[3]],
    };
    let normalized = normalize([result[0], result[1], result[2]]);
    result[..3].copy_from_slice(&normalized);
    result
}

fn transform_has_negative_determinant(axis: Axis) -> bool {
    axis == Axis::X
}

fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = value
        .iter()
        .map(|component| component * component)
        .sum::<f32>()
        .sqrt();
    if length > f32::EPSILON && length.is_finite() {
        value.map(|component| component / length)
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn generate_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0; 3]; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let a = positions[triangle[0] as usize];
        let b = positions[triangle[1] as usize];
        let c = positions[triangle[2] as usize];
        let normal = cross(sub(b, a), sub(c, a));
        for index in triangle {
            normals[*index as usize] = add(normals[*index as usize], normal);
        }
    }
    normals.into_iter().map(normalize).collect()
}

fn generate_tangents(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uv0: &[[f32; 2]],
    indices: &[u32],
) -> (Vec<[f32; 4]>, usize) {
    let mut tangent = vec![[0.0; 3]; positions.len()];
    let mut bitangent = vec![[0.0; 3]; positions.len()];
    let mut fallback_triangles = 0;
    for triangle in indices.chunks_exact(3) {
        let i0 = triangle[0] as usize;
        let i1 = triangle[1] as usize;
        let i2 = triangle[2] as usize;
        let edge1 = sub(positions[i1], positions[i0]);
        let edge2 = sub(positions[i2], positions[i0]);
        let duv1 = [uv0[i1][0] - uv0[i0][0], uv0[i1][1] - uv0[i0][1]];
        let duv2 = [uv0[i2][0] - uv0[i0][0], uv0[i2][1] - uv0[i0][1]];
        let denominator = duv1[0] * duv2[1] - duv1[1] * duv2[0];
        if denominator.abs() <= f32::EPSILON {
            fallback_triangles += 1;
            continue;
        }
        let inverse = 1.0 / denominator;
        let tangent_delta = scale(sub(scale(edge1, duv2[1]), scale(edge2, duv1[1])), inverse);
        let bitangent_delta = scale(sub(scale(edge2, duv1[0]), scale(edge1, duv2[0])), inverse);
        for index in [i0, i1, i2] {
            tangent[index] = add(tangent[index], tangent_delta);
            bitangent[index] = add(bitangent[index], bitangent_delta);
        }
    }
    let mut result = Vec::with_capacity(positions.len());
    for index in 0..positions.len() {
        let normal = normalize(normals[index]);
        let projected = sub(tangent[index], scale(normal, dot(normal, tangent[index])));
        let tangent_value = if length(projected) > f32::EPSILON {
            normalize(projected)
        } else {
            fallback_tangent(normal)
        };
        let handedness = if dot(cross(normal, tangent_value), bitangent[index]) < 0.0 {
            -1.0
        } else {
            1.0
        };
        result.push([
            tangent_value[0],
            tangent_value[1],
            tangent_value[2],
            handedness,
        ]);
    }
    (result, fallback_triangles)
}

fn fallback_tangent(normal: [f32; 3]) -> [f32; 3] {
    let reference = if normal[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    normalize(cross(reference, normal))
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(value: [f32; 3], factor: f32) -> [f32; 3] {
    [value[0] * factor, value[1] * factor, value[2] * factor]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(value: [f32; 3]) -> f32 {
    dot(value, value).sqrt()
}

fn number_value(value: Option<&Value>, default: f32) -> f32 {
    value
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .unwrap_or(default)
}

fn number_array<const N: usize>(value: Option<&Value>, default: [f32; N]) -> [f32; N] {
    let Some(values) = value.and_then(Value::as_array) else {
        return default;
    };
    if values.len() != N {
        return default;
    }
    let mut result = default;
    for (index, value) in values.iter().enumerate() {
        let Some(value) = value.as_f64() else {
            return default;
        };
        result[index] = value as f32;
    }
    result
}

fn valid_id(value: &str, label: &str) -> Result<(), AssetContractError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
    {
        return Err(AssetContractError::Contract(format!(
            "{label} is not a safe identifier: {value:?}"
        )));
    }
    Ok(())
}

fn valid_sha256_hex(value: &str, label: &str) -> Result<(), AssetContractError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AssetContractError::Contract(format!(
            "{label} is not a raw SHA-256 digest"
        )));
    }
    Ok(())
}

fn finding(
    findings: &mut Vec<RenderFinding>,
    code: RenderFindingCode,
    subject: &str,
    detail: impl Into<String>,
) {
    findings.push(RenderFinding {
        code,
        subject: subject.into(),
        detail: detail.into(),
    });
}

#[cfg(test)]
mod alpha_coverage_tests {
    use super::*;

    /// A foliage-like mask: smooth blobs from summed radial falloffs, so
    /// coverage is a property of shape rather than of single texels.
    fn leaf_mask(size: u32) -> Vec<u8> {
        let mut state = 0x2545_f491_u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(state >> 8) / f64::from(1u32 << 24)
        };
        let blobs: Vec<(f64, f64, f64)> =
            (0..40).map(|_| (next(), next(), 0.02 + 0.05 * next())).collect();
        let mut rgba = vec![0u8; size as usize * size as usize * 4];
        for y in 0..size {
            for x in 0..size {
                let (u, v) = (f64::from(x) / f64::from(size), f64::from(y) / f64::from(size));
                let field: f64 = blobs
                    .iter()
                    .map(|(cx, cy, r)| (1.0 - ((u - cx).hypot(v - cy) / r)).max(0.0))
                    .sum();
                let offset = ((y * size + x) * 4) as usize;
                rgba[offset..offset + 3].copy_from_slice(&[60, 120, 40]);
                rgba[offset + 3] = if field > 0.15 { 255 } else { 0 };
            }
        }
        rgba
    }

    #[test]
    fn mask_mips_keep_base_coverage() {
        let base = leaf_mask(256);
        let target = alpha_coverage(&base, 0.5);
        assert!((0.1..0.9).contains(&target), "fixture coverage {target}");
        let plain = generate_mip_chain(256, 256, &base, RenderTextureColorSpace::Srgb);
        let mut preserved = plain.clone();
        preserve_alpha_coverage(&base, &mut preserved, 0.5);
        for (plain_level, level) in plain.iter().zip(&preserved) {
            if level.width_px < 8 {
                break;
            }
            let coverage = alpha_coverage(&level.rgba8, 0.5);
            assert!(
                (coverage - target).abs() <= 0.02,
                "{}px: coverage {coverage:.4} vs base {target:.4}",
                level.width_px
            );
            let colour = |rgba: &[u8]| rgba.chunks_exact(4).map(|t| [t[0], t[1], t[2]]).collect::<Vec<_>>();
            assert_eq!(colour(&plain_level.rgba8), colour(&level.rgba8));
        }
    }

    #[test]
    fn box_filtered_mask_mips_lose_coverage() {
        // The defect the rescale exists for: without it, a thin-feature mask
        // drops below the cutoff at distance.
        let base = leaf_mask(256);
        let target = alpha_coverage(&base, 0.5);
        let plain = generate_mip_chain(256, 256, &base, RenderTextureColorSpace::Srgb);
        let worst = plain
            .iter()
            .filter(|level| level.width_px >= 8)
            .map(|level| (alpha_coverage(&level.rgba8, 0.5) - target).abs())
            .fold(0.0, f64::max);
        assert!(worst > 0.02, "fixture does not exercise coverage loss ({worst:.4})");
    }

    #[test]
    fn mask_cutoffs_follow_mask_base_colour_only() {
        let materials: Vec<Value> = serde_json::from_str(
            r#"[
                {"alphaMode": "MASK", "alphaCutoff": 0.4, "pbrMetallicRoughness": {"baseColorTexture": {"index": 2}}},
                {"alphaMode": "OPAQUE", "pbrMetallicRoughness": {"baseColorTexture": {"index": 3}}},
                {"alphaMode": "MASK", "pbrMetallicRoughness": {"baseColorTexture": {"index": 5}}, "normalTexture": {"index": 6}}
            ]"#,
        )
        .unwrap();
        let mut findings = Vec::new();
        let cutoffs = mask_coverage_cutoffs(&materials, &mut findings);
        assert!(findings.is_empty());
        assert_eq!(cutoffs.into_iter().collect::<Vec<_>>(), vec![(2, 0.4), (5, 0.5)]);

        let conflicting: Vec<Value> = serde_json::from_str(
            r#"[
                {"alphaMode": "MASK", "alphaCutoff": 0.4, "pbrMetallicRoughness": {"baseColorTexture": {"index": 1}}},
                {"alphaMode": "MASK", "alphaCutoff": 0.6, "pbrMetallicRoughness": {"baseColorTexture": {"index": 1}}}
            ]"#,
        )
        .unwrap();
        mask_coverage_cutoffs(&conflicting, &mut findings);
        assert_eq!(findings.len(), 1);
    }
}
