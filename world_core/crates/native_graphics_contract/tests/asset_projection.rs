use sha2::{Digest, Sha256};
use wge_asset_contract::{
    AssetIdentity, RENDER_ASSET_PACKAGE_SCHEMA, RenderAlphaMode, RenderAssetPackage,
    RenderAssetProvenance, RenderMaterial, RenderMesh, RenderProducerIdentity, RenderTexture,
    RenderTextureColorSpace, RenderTextureMip, RenderTextureTransform, RenderTransform,
};
use wge_native_graphics_contract::{
    TextureColorSpace, TexturePayload, project_render_asset, validate_graphics_asset_projection,
};

fn package() -> RenderAssetPackage {
    let source_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    RenderAssetPackage {
        package_id: String::new(),
        schema_version: RENDER_ASSET_PACKAGE_SCHEMA.into(),
        source_identity: AssetIdentity {
            asset_id: "asset_sha256_0123456789abcdef".into(),
            source_sha256: source_sha256.into(),
            byte_length: 512,
        },
        transform: RenderTransform {
            meters_per_unit: 1.0,
            source_vertical_axis: wge_asset_contract::Axis::Y,
            canonical_vertical_axis: wge_asset_contract::Axis::Y,
        },
        meshes: vec![RenderMesh {
            mesh_id: "hero-mesh".into(),
            source_mesh_index: 0,
            source_primitive_index: 0,
            positions_m: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            tangents: vec![[1.0, 0.0, 0.0, 1.0]; 3],
            uv0: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            material_id: "hero-material".into(),
            generated_normals: true,
            generated_tangents: true,
            tangent_fallback_count: 0,
        }],
        materials: vec![RenderMaterial {
            material_id: "hero-material".into(),
            base_color_rgba: [0.7, 0.4, 0.2, 1.0],
            metallic: 0.1,
            roughness: 0.65,
            alpha_mode: RenderAlphaMode::Opaque,
            double_sided: false,
            base_color_texture_id: Some("albedo".into()),
            metallic_roughness_texture_id: None,
            normal_texture_id: None,
            occlusion_texture_id: None,
            emissive_texture_id: None,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
            texture_transform: RenderTextureTransform::identity(),
        }],
        textures: vec![RenderTexture {
            texture_id: "albedo".into(),
            source_texture_index: 0,
            source_image_index: 0,
            source_sha256: source_sha256.into(),
            mime_type: "image/png".into(),
            width_px: 1,
            height_px: 1,
            mip_levels: 1,
            color_space: RenderTextureColorSpace::Srgb,
            rgba8: vec![255, 128, 64, 255],
            mip_chain: vec![],
        }],
        provenance: RenderAssetProvenance {
            producer: RenderProducerIdentity {
                name: "test".into(),
                version: "1".into(),
            },
            source_sha256: source_sha256.into(),
            source_byte_length: 512,
            request_sha256: source_sha256.into(),
            inspection_report_sha256: source_sha256.into(),
        },
    }
}

fn sealed_package() -> RenderAssetPackage {
    seal(package())
}

fn seal(mut package: RenderAssetPackage) -> RenderAssetPackage {
    let mut value = serde_json::to_value(&package).expect("package serializes");
    value
        .as_object_mut()
        .expect("package is an object")
        .remove("package_id");
    let mut hasher = Sha256::new();
    hasher.update(wge_asset_contract::canonical_json(&value).as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    package.package_id = format!("render_asset_sha256_{digest}");
    package
}

#[test]
fn projection_preserves_identity_and_maps_pbr_contract() {
    let projection = project_render_asset(&sealed_package()).expect("projection is valid");
    assert_eq!(projection.source_asset_id, "asset_sha256_0123456789abcdef");
    assert_eq!(projection.meshes[0].packet.tangents.len(), 3);
    assert_eq!(projection.materials[0].texture_ids, vec!["albedo"]);
    assert_eq!(
        projection.textures[0].reference.color_space,
        TextureColorSpace::Srgb
    );
    validate_graphics_asset_projection(&projection).expect("projection revalidates");
}

#[test]
fn projection_rejects_tampered_source_binding() {
    let mut projection = project_render_asset(&sealed_package()).expect("projection is valid");
    projection.textures[0].reference.source_artifact_id = "other-asset".into();
    assert!(validate_graphics_asset_projection(&projection).is_err());
}

#[test]
fn projection_preserves_multilevel_payload_and_digest() {
    let base = vec![255, 0, 0, 255, 0, 255, 0, 255];
    let mip = vec![128, 128, 0, 255];
    let mut package = package();
    package.textures[0].width_px = 2;
    package.textures[0].height_px = 1;
    package.textures[0].mip_levels = 2;
    package.textures[0].rgba8 = base.clone();
    package.textures[0].mip_chain = vec![RenderTextureMip {
        width_px: 1,
        height_px: 1,
        rgba8: mip.clone(),
    }];
    let mut value = serde_json::to_value(&package).expect("package serializes");
    value
        .as_object_mut()
        .expect("package is an object")
        .remove("package_id");
    let digest = format!(
        "{:x}",
        Sha256::digest(wge_asset_contract::canonical_json(&value).as_bytes())
    );
    package.package_id = format!("render_asset_sha256_{digest}");

    let projection = project_render_asset(&package).expect("mip projection is valid");
    let reference = &projection.textures[0].reference;
    assert_eq!(reference.mip_levels, 2);
    assert_eq!(
        reference.sha256,
        wge_native_graphics_contract::sha256_prefixed(&[base, mip].concat())
    );
    let Some(TexturePayload::Rgba8MipChain { levels }) = reference.payload.as_ref() else {
        panic!("multi-level projection must retain the mip-chain payload");
    };
    assert_eq!(levels.len(), 2);
    assert_eq!(levels[1].width_px, 1);
    assert_eq!(levels[1].height_px, 1);
    validate_graphics_asset_projection(&projection).expect("mip projection revalidates");

    let mut tampered = projection.clone();
    let Some(TexturePayload::Rgba8MipChain { levels }) =
        tampered.textures[0].reference.payload.as_mut()
    else {
        panic!("multi-level projection must retain the mip-chain payload");
    };
    levels[1].width_px = 2;
    assert!(validate_graphics_asset_projection(&tampered).is_err());
}

fn data_texture(texture_id: &str, index: usize, color_space: RenderTextureColorSpace) -> RenderTexture {
    let mut texture = package().textures.remove(0);
    texture.texture_id = texture_id.into();
    texture.source_texture_index = index;
    texture.source_image_index = index;
    texture.color_space = color_space;
    texture
}

/// CALIBRATION-1 regression: a material with every PBR map used to project
/// ALL of them into `texture_ids`, which the adapter samples as the single
/// albedo slot and refuses ("native path supports one albedo texture per
/// material"). Only base colour belongs there; the rest have named slots.
#[test]
fn full_pbr_material_projects_one_albedo_and_named_slots() {
    let mut full = package();
    full.materials[0].metallic_roughness_texture_id = Some("orm".into());
    full.materials[0].normal_texture_id = Some("normal".into());
    full.materials[0].occlusion_texture_id = Some("ao".into());
    full.materials[0].emissive_texture_id = Some("glow".into());
    full.textures.extend([
        data_texture("orm", 1, RenderTextureColorSpace::Linear),
        data_texture("normal", 2, RenderTextureColorSpace::NormalMap),
        data_texture("ao", 3, RenderTextureColorSpace::Linear),
        data_texture("glow", 4, RenderTextureColorSpace::Srgb),
    ]);
    let projection = project_render_asset(&seal(full)).expect("full PBR material projects");
    let material = &projection.materials[0];
    assert_eq!(material.texture_ids, vec!["albedo"], "texture_ids is the albedo slot only");
    assert_eq!(material.roughness_texture_id.as_deref(), Some("orm"));
    assert_eq!(material.normal_texture_id.as_deref(), Some("normal"));
    assert_eq!(material.occlusion_texture_id.as_deref(), Some("ao"));
    assert_eq!(material.emissive_texture_id.as_deref(), Some("glow"));
    validate_graphics_asset_projection(&projection).expect("projection revalidates");

    let mut dangling = projection.clone();
    dangling.materials[0].normal_texture_id = Some("missing".into());
    assert!(validate_graphics_asset_projection(&dangling).is_err(), "named slots must resolve");
    let mut doubled = projection;
    doubled.materials[0].texture_ids.push("orm".into());
    assert!(validate_graphics_asset_projection(&doubled).is_err(), "one albedo texture per material");
}
