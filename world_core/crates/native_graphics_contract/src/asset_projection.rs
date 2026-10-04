//! Projection from the validated neutral render-asset package into the
//! engine-neutral graphics contract.
//!
//! This is intentionally a bridge, not a scene composer. It preserves the
//! render package and source identities while producing the material/texture/
//! mesh vocabulary that a later `SceneObject` lowering can place into a
//! `GraphicsScenePacket`.

use std::collections::BTreeSet;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use wge_asset_contract::{
    RenderAlphaMode, RenderAssetPackage, RenderTextureColorSpace, validate_render_asset_package,
};

use crate::{
    AlphaMode, GraphicsContractError, MaterialIntent, MeshPacket, TextureColorSpace,
    TextureMipLevel, TexturePayload, TextureReference, sha256_prefixed,
};

pub const GRAPHICS_ASSET_PROJECTION_SCHEMA: &str = "wge.graphics-asset-projection/v1";

/// A conditioned mesh in the current graphics vocabulary. Tangents are part of
/// the packet now, so scene composition cannot silently discard the Rust-owned
/// conditioned stream before it reaches a backend.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsAssetMesh {
    pub packet: MeshPacket,
}

/// A graphics texture plus the encoded-source identity retained by the
/// conditioning layer. `TextureReference.sha256` is the digest of the inline
/// RGBA payload, while `source_sha256` is the digest of the source image
/// bytes in the GLB.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsAssetTexture {
    pub reference: TextureReference,
    pub source_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsAssetProjection {
    pub schema_version: String,
    pub render_package_id: String,
    pub source_asset_id: String,
    pub source_asset_sha256: String,
    pub meshes: Vec<GraphicsAssetMesh>,
    pub materials: Vec<MaterialIntent>,
    pub textures: Vec<GraphicsAssetTexture>,
}

/// Project a validated render package without creating GPU resources or
/// altering canonical scene state.
pub fn project_render_asset(
    package: &RenderAssetPackage,
) -> Result<GraphicsAssetProjection, GraphicsContractError> {
    validate_render_asset_package(package).map_err(|error| {
        GraphicsContractError::provenance(format!("render package is not valid: {error}"))
    })?;

    let source_asset_id = package.source_identity.asset_id.clone();
    let meshes = package
        .meshes
        .iter()
        .map(|mesh| GraphicsAssetMesh {
            packet: MeshPacket {
                mesh_id: mesh.mesh_id.clone(),
                positions_m: mesh.positions_m.clone(),
                normals: mesh.normals.clone(),
                uv0: mesh.uv0.clone(),
                indices: mesh.indices.clone(),
                material_id: mesh.material_id.clone(),
                tangents: mesh.tangents.clone(),
            },
        })
        .collect();

    let materials = package
        .materials
        .iter()
        .map(|material| {
            // `texture_ids` is the ALBEDO slot (the adapter samples it as base
            // colour and refuses more than one); every other map has its own
            // named slot below. This used to list every referenced texture,
            // which the adapter rejected for any imported material carrying
            // more than a base-colour map (found by CALIBRATION-1).
            let texture_ids = material.base_color_texture_id.iter().cloned().collect();
            MaterialIntent {
                material_id: material.material_id.clone(),
                base_color_rgba: material.base_color_rgba,
                metallic: material.metallic,
                roughness: material.roughness,
                clearcoat: 0.0,
                clearcoat_roughness: 0.045,
                alpha_mode: project_alpha_mode(material.alpha_mode),
                texture_ids,
                normal_texture_id: material.normal_texture_id.clone(),
                roughness_texture_id: material.metallic_roughness_texture_id.clone(),
                occlusion_texture_id: material.occlusion_texture_id.clone(),
                emissive_texture_id: material.emissive_texture_id.clone(),
                normal_scale: 1.0,
                occlusion_strength: 1.0,
                emissive_factor_rgb: material.emissive_factor_rgb,
            }
        })
        .collect();

    let textures = package
        .textures
        .iter()
        .map(|texture| GraphicsAssetTexture {
            reference: TextureReference {
                texture_id: texture.texture_id.clone(),
                source_artifact_id: source_asset_id.clone(),
                sha256: texture_payload_sha256(texture),
                width_px: texture.width_px,
                height_px: texture.height_px,
                mip_levels: texture.mip_levels,
                color_space: project_color_space(texture.color_space),
                payload: Some(project_texture_payload(texture)),
            },
            source_sha256: texture.source_sha256.clone(),
        })
        .collect();

    let projection = GraphicsAssetProjection {
        schema_version: GRAPHICS_ASSET_PROJECTION_SCHEMA.into(),
        render_package_id: package.package_id.clone(),
        source_asset_id,
        source_asset_sha256: package.source_identity.source_sha256.clone(),
        meshes,
        materials,
        textures,
    };
    validate_graphics_asset_projection(&projection)?;
    Ok(projection)
}

fn project_texture_payload(texture: &wge_asset_contract::RenderTexture) -> TexturePayload {
    if texture.mip_chain.is_empty() {
        return TexturePayload::Rgba8(STANDARD.encode(&texture.rgba8));
    }
    let mut levels = Vec::with_capacity(texture.mip_chain.len() + 1);
    levels.push(TextureMipLevel {
        width_px: texture.width_px,
        height_px: texture.height_px,
        base64: STANDARD.encode(&texture.rgba8),
    });
    levels.extend(texture.mip_chain.iter().map(|mip| TextureMipLevel {
        width_px: mip.width_px,
        height_px: mip.height_px,
        base64: STANDARD.encode(&mip.rgba8),
    }));
    TexturePayload::Rgba8MipChain { levels }
}

fn texture_payload_sha256(texture: &wge_asset_contract::RenderTexture) -> String {
    let mut bytes = Vec::with_capacity(
        texture.rgba8.len()
            + texture
                .mip_chain
                .iter()
                .map(|mip| mip.rgba8.len())
                .sum::<usize>(),
    );
    bytes.extend_from_slice(&texture.rgba8);
    for mip in &texture.mip_chain {
        bytes.extend_from_slice(&mip.rgba8);
    }
    sha256_prefixed(&bytes)
}

/// Independently validate the bridge result before a scene composer consumes
/// it. This does not certify a scene or renderer output.
pub fn validate_graphics_asset_projection(
    projection: &GraphicsAssetProjection,
) -> Result<(), GraphicsContractError> {
    if projection.schema_version != GRAPHICS_ASSET_PROJECTION_SCHEMA {
        return Err(GraphicsContractError::malformed(format!(
            "unsupported graphics asset projection schema {}",
            projection.schema_version
        )));
    }
    valid_id(&projection.render_package_id, "render_package_id")?;
    valid_id(&projection.source_asset_id, "source_asset_id")?;
    valid_hex_sha(&projection.source_asset_sha256, "source_asset_sha256")?;

    let material_ids = projection
        .materials
        .iter()
        .map(|material| material.material_id.as_str())
        .collect::<BTreeSet<_>>();
    if material_ids.len() != projection.materials.len() {
        return Err(GraphicsContractError::provenance(
            "graphics asset projection contains duplicate material ids",
        ));
    }
    for material in &projection.materials {
        super::validate_material(material)?;
        for texture_id in &material.texture_ids {
            valid_id(texture_id, "material texture_id")?;
        }
    }

    let mut mesh_ids = BTreeSet::new();
    for mesh in &projection.meshes {
        super::validate_mesh(&mesh.packet, &material_ids)?;
        if !mesh_ids.insert(mesh.packet.mesh_id.as_str()) {
            return Err(GraphicsContractError::provenance(
                "graphics asset projection contains duplicate mesh ids",
            ));
        }
        if mesh.packet.tangents.len() != mesh.packet.positions_m.len()
            || mesh
                .packet
                .tangents
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
        {
            return Err(GraphicsContractError::malformed(format!(
                "mesh {} has an invalid tangent stream",
                mesh.packet.mesh_id
            )));
        }
    }

    let mut texture_ids = BTreeSet::new();
    for texture in &projection.textures {
        super::validate_texture(&texture.reference)?;
        valid_hex_sha(&texture.source_sha256, "texture source_sha256")?;
        if !texture_ids.insert(texture.reference.texture_id.as_str()) {
            return Err(GraphicsContractError::provenance(
                "graphics asset projection contains duplicate texture ids",
            ));
        }
        if texture.reference.source_artifact_id != projection.source_asset_id {
            return Err(GraphicsContractError::provenance(format!(
                "texture {} is bound to a different source artifact",
                texture.reference.texture_id
            )));
        }
    }
    for material in &projection.materials {
        if material.texture_ids.len() > 1 {
            return Err(GraphicsContractError::provenance(format!(
                "material {} has {} albedo textures; the native path samples one",
                material.material_id,
                material.texture_ids.len()
            )));
        }
        let slots = [
            material.normal_texture_id.as_deref(),
            material.roughness_texture_id.as_deref(),
            material.occlusion_texture_id.as_deref(),
            material.emissive_texture_id.as_deref(),
        ];
        for texture_id in material.texture_ids.iter().map(String::as_str).chain(slots.into_iter().flatten()) {
            if !texture_ids.contains(texture_id) {
                return Err(GraphicsContractError::provenance(format!(
                    "material {} references unknown texture {}",
                    material.material_id, texture_id
                )));
            }
        }
    }
    Ok(())
}

fn project_alpha_mode(mode: RenderAlphaMode) -> AlphaMode {
    match mode {
        RenderAlphaMode::Opaque => AlphaMode::Opaque,
        RenderAlphaMode::Mask => AlphaMode::Mask,
        RenderAlphaMode::Blend => AlphaMode::Blend,
    }
}

fn project_color_space(space: RenderTextureColorSpace) -> TextureColorSpace {
    match space {
        RenderTextureColorSpace::Srgb => TextureColorSpace::Srgb,
        RenderTextureColorSpace::Linear => TextureColorSpace::Linear,
        RenderTextureColorSpace::NormalMap => TextureColorSpace::NormalMap,
        RenderTextureColorSpace::Data => TextureColorSpace::Data,
    }
}

fn valid_id(value: &str, label: &str) -> Result<(), GraphicsContractError> {
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(|character| character.is_whitespace())
    {
        return Err(GraphicsContractError::malformed(format!(
            "{label} is empty, too long, or contains whitespace"
        )));
    }
    Ok(())
}

fn valid_hex_sha(value: &str, label: &str) -> Result<(), GraphicsContractError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GraphicsContractError::malformed(format!(
            "{label} is not a raw SHA-256 digest"
        )));
    }
    Ok(())
}
