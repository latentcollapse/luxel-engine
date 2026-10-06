use std::io::Cursor;

use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::json;
use luxel_asset_contract::{
    Axis, RENDER_ASSET_REQUEST_SCHEMA, RenderConditioningRequest, RenderFindingCode,
    RenderMipPolicy, RenderPreparationStatus, condition_render_asset,
    validate_render_asset_package, validate_render_conditioning_receipt,
};

fn request(require_uv0: bool) -> RenderConditioningRequest {
    RenderConditioningRequest {
        schema_version: RENDER_ASSET_REQUEST_SCHEMA.into(),
        meters_per_unit: 1.0,
        vertical_axis: Axis::Y,
        require_uv0,
        generate_normals: true,
        generate_tangents: true,
        mip_policy: RenderMipPolicy::SingleLevelExplicit,
        max_texture_dimension: 1024,
    }
}

fn png_1x1() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    PngEncoder::new(&mut bytes)
        .write_image(&[255, 64, 32, 255], 1, 1, ColorType::Rgba8.into())
        .unwrap();
    bytes.into_inner()
}

fn png_4x2() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let pixels = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255, 32, 64, 96, 255, 64,
        96, 128, 255, 96, 128, 160, 255, 128, 160, 192, 255,
    ];
    PngEncoder::new(&mut bytes)
        .write_image(&pixels, 4, 2, ColorType::Rgba8.into())
        .unwrap();
    bytes.into_inner()
}

#[derive(Clone, Copy)]
enum TextureTransformCase {
    None,
    Shared { tex_coord: u64 },
    Malformed,
    Conflicting,
}

fn make_glb(include_uv: bool, include_texture: bool) -> Vec<u8> {
    make_glb_with_texture_transform(
        include_uv,
        include_texture,
        false,
        TextureTransformCase::None,
    )
}

fn make_glb_with_texture_transform(
    include_uv: bool,
    include_texture: bool,
    include_tangent: bool,
    transform_case: TextureTransformCase,
) -> Vec<u8> {
    make_glb_with_texture_transform_and_image(
        include_uv,
        include_texture,
        include_tangent,
        transform_case,
        png_1x1(),
    )
}

fn make_glb_with_texture_transform_and_image(
    include_uv: bool,
    include_texture: bool,
    include_tangent: bool,
    transform_case: TextureTransformCase,
    image_bytes: Vec<u8>,
) -> Vec<u8> {
    let mut binary = Vec::new();
    let positions_offset = binary.len();
    for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    let normals_offset = binary.len();
    for value in [0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0] {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    let tangent_offset = binary.len();
    if include_tangent {
        for value in [
            0.0f32, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0,
        ] {
            binary.extend_from_slice(&value.to_le_bytes());
        }
    }
    let uv_offset = binary.len();
    if include_uv {
        for value in [0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0] {
            binary.extend_from_slice(&value.to_le_bytes());
        }
    }
    let index_offset = binary.len();
    for value in [0u16, 1, 2] {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let image_offset = binary.len();
    if include_texture {
        binary.extend_from_slice(&image_bytes);
        while binary.len() % 4 != 0 {
            binary.push(0);
        }
    }

    let mut attributes = json!({"POSITION": 0, "NORMAL": 1});
    let mut accessors = vec![
        json!({"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}),
        json!({"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"}),
    ];
    let mut next_buffer_view = 2;
    if include_tangent {
        let accessor_index = accessors.len();
        attributes["TANGENT"] = json!(accessor_index);
        accessors.push(json!({
            "bufferView": next_buffer_view,
            "componentType": 5126,
            "count": 3,
            "type": "VEC4"
        }));
        next_buffer_view += 1;
    }
    if include_uv {
        let accessor_index = accessors.len();
        attributes["TEXCOORD_0"] = json!(accessor_index);
        accessors.push(json!({
            "bufferView": next_buffer_view,
            "componentType": 5126,
            "count": 3,
            "type": "VEC2"
        }));
        next_buffer_view += 1;
    }
    let index_accessor = accessors.len();
    accessors.push(json!({
        "bufferView": next_buffer_view,
        "componentType": 5123,
        "count": 3,
        "type": "SCALAR"
    }));
    let primitive = json!({"attributes": attributes, "indices": index_accessor, "material": 0});
    let mut buffer_views = vec![
        json!({"buffer": 0, "byteOffset": positions_offset, "byteLength": 36}),
        json!({"buffer": 0, "byteOffset": normals_offset, "byteLength": 36}),
    ];
    if include_tangent {
        buffer_views.push(json!({"buffer": 0, "byteOffset": tangent_offset, "byteLength": 48}));
    }
    if include_uv {
        buffer_views.push(json!({"buffer": 0, "byteOffset": uv_offset, "byteLength": 24}));
    }
    buffer_views.push(json!({"buffer": 0, "byteOffset": index_offset, "byteLength": 6}));
    let image_buffer_view = buffer_views.len();
    if include_texture {
        buffer_views.push(json!({"buffer": 0, "byteOffset": image_offset, "byteLength": binary.len() - image_offset}));
    }
    let materials = if include_texture {
        let mut materials = json!([{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}]);
        let transform = match transform_case {
            TextureTransformCase::None => None,
            TextureTransformCase::Shared { tex_coord } => Some(json!({
                "offset": [0.25, -0.5],
                "scale": [2.0, 3.0],
                "rotation": 0.0,
                "texCoord": tex_coord
            })),
            TextureTransformCase::Malformed => Some(json!({
                "offset": [0.25],
                "scale": [2.0, 3.0],
                "rotation": 0.0
            })),
            TextureTransformCase::Conflicting => Some(json!({
                "offset": [0.25, -0.5],
                "scale": [2.0, 3.0],
                "rotation": 0.0
            })),
        };
        if let Some(transform) = transform {
            materials[0]["pbrMetallicRoughness"]["baseColorTexture"]["extensions"] = json!({
                "KHR_texture_transform": transform
            });
        }
        if matches!(transform_case, TextureTransformCase::Conflicting) {
            materials[0]["normalTexture"] = json!({
                "index": 0,
                "extensions": {
                    "KHR_texture_transform": {
                        "offset": [0.5, -0.5],
                        "scale": [2.0, 3.0],
                        "rotation": 0.0
                    }
                }
            });
        }
        materials
    } else {
        json!([{"pbrMetallicRoughness": {"baseColorFactor": [0.6, 0.4, 0.2, 1.0]}}])
    };
    let mut document = json!({
        "asset": {"version": "2.0", "generator": "luxel-render-test"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "triangle", "mesh": 0}],
        "meshes": [{"name": "triangle", "primitives": [primitive]}],
        "materials": materials,
        "buffers": [{"byteLength": binary.len()}],
        "bufferViews": buffer_views,
        "accessors": accessors
    });
    if include_texture {
        document["textures"] = json!([{"source": 0}]);
        document["images"] = json!([{"bufferView": image_buffer_view, "mimeType": "image/png"}]);
    }
    let mut json_bytes = serde_json::to_vec(&document).unwrap();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let total = 12 + 8 + json_bytes.len() + 8 + binary.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4e4f_534au32.to_le_bytes());
    glb.extend_from_slice(&json_bytes);
    glb.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004e_4942u32.to_le_bytes());
    glb.extend_from_slice(&binary);
    glb
}

#[test]
fn conditioning_generates_tangents_and_is_byte_deterministic() {
    let bytes = make_glb(true, false);
    let first = condition_render_asset(&bytes, &request(true)).unwrap();
    let second = condition_render_asset(&bytes, &request(true)).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.status, RenderPreparationStatus::Ready);
    let package = first.package.unwrap();
    assert_eq!(package.meshes.len(), 1);
    assert!(package.meshes[0].generated_tangents);
    assert_eq!(package.meshes[0].tangents.len(), 3);
    assert_eq!(package.materials[0].material_id, "material_0");
}

#[test]
fn conditioning_rejects_missing_uv_when_the_request_requires_it() {
    let receipt = condition_render_asset(&make_glb(false, false), &request(true)).unwrap();
    assert_eq!(receipt.status, RenderPreparationStatus::Rejected);
    assert!(
        receipt
            .findings
            .iter()
            .any(|finding| finding.code == RenderFindingCode::MissingUv0)
    );
}

#[test]
fn conditioning_extracts_embedded_texture_and_color_space() {
    let receipt = condition_render_asset(&make_glb(true, true), &request(true)).unwrap();
    assert_eq!(receipt.status, RenderPreparationStatus::Ready);
    let texture = &receipt.package.unwrap().textures[0];
    assert_eq!(texture.width_px, 1);
    assert_eq!(texture.height_px, 1);
    assert_eq!(texture.rgba8, vec![255, 64, 32, 255]);
}

#[test]
fn conditioning_applies_shared_texture_transform_before_tangent_generation() {
    let receipt = condition_render_asset(
        &make_glb_with_texture_transform(
            true,
            true,
            false,
            TextureTransformCase::Shared { tex_coord: 0 },
        ),
        &request(true),
    )
    .unwrap();
    assert_eq!(receipt.status, RenderPreparationStatus::Ready);
    let package = receipt.package.unwrap();
    assert_eq!(package.materials[0].texture_transform.offset, [0.25, -0.5]);
    assert_eq!(package.materials[0].texture_transform.scale, [2.0, 3.0]);
    assert_eq!(package.meshes[0].uv0[0], [0.25, -0.5]);
    assert_eq!(package.meshes[0].uv0[1], [2.25, -0.5]);
    assert_eq!(package.meshes[0].uv0[2], [0.25, 2.5]);
    assert!(package.meshes[0].generated_tangents);
    assert_eq!(package.meshes[0].tangent_fallback_count, 0);
}

#[test]
fn conditioning_rejects_texture_transform_that_selects_another_uv_set() {
    let receipt = condition_render_asset(
        &make_glb_with_texture_transform(
            true,
            true,
            false,
            TextureTransformCase::Shared { tex_coord: 1 },
        ),
        &request(true),
    )
    .unwrap();
    assert_eq!(receipt.status, RenderPreparationStatus::Rejected);
    assert!(
        receipt
            .findings
            .iter()
            .any(|finding| { finding.code == RenderFindingCode::UnsupportedTextureTransform })
    );
}

#[test]
fn conditioning_rebuilds_authored_tangents_after_transform_or_rejects_without_permission() {
    let bytes = make_glb_with_texture_transform(
        true,
        true,
        true,
        TextureTransformCase::Shared { tex_coord: 0 },
    );
    let receipt = condition_render_asset(&bytes, &request(true)).unwrap();
    assert_eq!(receipt.status, RenderPreparationStatus::Ready);
    let package = receipt.package.unwrap();
    assert!(package.meshes[0].generated_tangents);
    assert_eq!(package.meshes[0].tangents[0], [1.0, 0.0, 0.0, 1.0]);

    let mut no_tangent_generation = request(true);
    no_tangent_generation.generate_tangents = false;
    let receipt = condition_render_asset(&bytes, &no_tangent_generation).unwrap();
    assert_eq!(receipt.status, RenderPreparationStatus::Rejected);
    assert!(receipt.findings.iter().any(|finding| {
        finding.code == RenderFindingCode::TextureTransformRequiresTangentRebuild
    }));
}

#[test]
fn conditioning_rejects_malformed_texture_transform() {
    let receipt = condition_render_asset(
        &make_glb_with_texture_transform(true, true, false, TextureTransformCase::Malformed),
        &request(true),
    )
    .unwrap();
    assert_eq!(receipt.status, RenderPreparationStatus::Rejected);
    assert!(
        receipt
            .findings
            .iter()
            .any(|finding| finding.code == RenderFindingCode::InvalidMaterial)
    );
}

#[test]
fn conditioning_rejects_conflicting_present_role_transforms() {
    let receipt = condition_render_asset(
        &make_glb_with_texture_transform(true, true, false, TextureTransformCase::Conflicting),
        &request(true),
    )
    .unwrap();
    assert_eq!(receipt.status, RenderPreparationStatus::Rejected);
    assert!(
        receipt
            .findings
            .iter()
            .any(|finding| finding.code == RenderFindingCode::InvalidMaterial)
    );
}

#[test]
fn conditioning_generates_a_deterministic_cpu_mip_chain() {
    let mut request = request(true);
    request.mip_policy = RenderMipPolicy::GenerateCpuChain;
    let first = condition_render_asset(&make_glb(true, true), &request).unwrap();
    let second = condition_render_asset(&make_glb(true, true), &request).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.status, RenderPreparationStatus::Ready);
    let package = first.package.unwrap();
    let texture = &package.textures[0];
    assert_eq!(texture.mip_levels, 1);
    assert!(texture.mip_chain.is_empty());
}

#[test]
fn conditioning_generates_expected_mip_dimensions_and_revalidation_rejects_tampering() {
    let mut request = request(true);
    request.mip_policy = RenderMipPolicy::GenerateCpuChain;
    let bytes = make_glb_with_texture_transform_and_image(
        true,
        true,
        false,
        TextureTransformCase::None,
        png_4x2(),
    );
    let receipt = condition_render_asset(&bytes, &request).unwrap();
    assert_eq!(receipt.status, RenderPreparationStatus::Ready);
    let mut package = receipt.package.unwrap();
    let texture = &package.textures[0];
    assert_eq!(texture.mip_levels, 3);
    assert_eq!(texture.mip_chain[0].width_px, 2);
    assert_eq!(texture.mip_chain[0].height_px, 1);
    assert_eq!(texture.mip_chain[0].rgba8.len(), 8);
    assert_eq!(texture.mip_chain[1].width_px, 1);
    assert_eq!(texture.mip_chain[1].height_px, 1);
    assert_eq!(texture.mip_chain[1].rgba8.len(), 4);
    validate_render_asset_package(&package).unwrap();

    package.textures[0].mip_chain[0].width_px = 1;
    assert!(validate_render_asset_package(&package).is_err());
}

#[test]
fn conditioning_revalidation_rejects_resealed_package_tampering() {
    let bytes = make_glb(true, false);
    let mut receipt = condition_render_asset(&bytes, &request(true)).unwrap();
    validate_render_conditioning_receipt(&bytes, &request(true), &receipt).unwrap();
    receipt.package.as_mut().unwrap().meshes[0].positions_m[0][0] = 99.0;
    assert!(validate_render_conditioning_receipt(&bytes, &request(true), &receipt).is_err());

    let receipt = condition_render_asset(&bytes, &request(true)).unwrap();
    validate_render_asset_package(receipt.package.as_ref().unwrap()).unwrap();
}
