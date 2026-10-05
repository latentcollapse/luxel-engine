use serde_json::json;
use wge_asset_contract::{
    AssetIdentity, AssetPreparationReceipt, AssetProvenance, AssetUse, Axis, CollisionMetadata,
    CollisionShape, LodMetadata, PreparationStatus, ProducerIdentity, RENDER_ASSET_PACKAGE_SCHEMA,
    RenderAlphaMode, RenderAssetPackage, RenderAssetProvenance, RenderMaterial, RenderMesh,
    RenderProducerIdentity, RenderTexture, RenderTextureColorSpace, RenderTextureTransform,
    RenderTransform, RuntimeAssetPackage, RuntimeTarget, RuntimeTransform,
    validate_render_asset_package,
};
use wge_project_ledger::{
    CollisionPolicy, GameplayReference, MaterialAssignment, SCENE_ARTIFACT_SCHEMA,
    SCENE_OBJECT_SCHEMA, SceneArtifactBody, SceneImportance, SceneLodLevel, SceneLodPolicy,
    SceneObject, SceneObjectProvenance, SceneProvenance, SceneTransform, SceneVisibilityPolicy,
    project_scene_for_graphics, seal_scene, seal_scene_with_render_assets,
    validate_scene_against_asset_receipts, validate_scene_against_asset_receipts_and_render_assets,
};

fn ready_asset_receipt() -> AssetPreparationReceipt {
    let source_identity = AssetIdentity {
        asset_id: "asset-fortress-v1".into(),
        source_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        byte_length: 128,
    };
    let mut package = RuntimeAssetPackage {
        package_id: String::new(),
        schema_version: "wge.runtime-asset-package/v1".into(),
        target: RuntimeTarget::Native,
        asset_use: AssetUse::StaticMesh,
        source_identity: source_identity.clone(),
        mesh_ids: vec!["fortress_lod0".into()],
        transform: RuntimeTransform {
            meters_per_unit: 1.0,
            vertical_axis: Axis::Y,
        },
        rig: None,
        animations: vec![],
        sockets: vec![],
        collision: CollisionMetadata {
            shape: CollisionShape::Box,
            center: [0.0, 1.0, 0.0],
            size: [4.0, 2.0, 4.0],
            axis: None,
        },
        lods: vec![LodMetadata {
            level: 0,
            mesh_name: "fortress_lod0".into(),
            switch_below_fraction: 0.0,
        }],
        provenance: AssetProvenance {
            producer: ProducerIdentity {
                name: "scene-test".into(),
                version: "1".into(),
            },
            source_sha256: source_identity.source_sha256.clone(),
            source_byte_length: source_identity.byte_length,
            request_sha256: "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd"
                .into(),
            inspection_report_sha256:
                "fedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcba".into(),
        },
    };
    let mut package_value = serde_json::to_value(&package).unwrap();
    package_value.as_object_mut().unwrap().remove("package_id");
    let package_digest = wge_project_ledger::sha256_hex(
        wge_project_ledger::canonical_json(&package_value).as_bytes(),
    );
    package.package_id = format!("runtime_asset_sha256_{package_digest}");
    let mut receipt = AssetPreparationReceipt {
        schema_version: "wge.asset-runtime-receipt/v1".into(),
        status: PreparationStatus::Ready,
        source_identity,
        request_sha256: "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".into(),
        producer: ProducerIdentity {
            name: "scene-test".into(),
            version: "1".into(),
        },
        package: Some(package),
        findings: vec![],
        receipt_sha256: String::new(),
    };
    let mut receipt_value = serde_json::to_value(&receipt).unwrap();
    receipt_value
        .as_object_mut()
        .unwrap()
        .remove("receipt_sha256");
    receipt.receipt_sha256 = wge_project_ledger::sha256_hex(
        wge_project_ledger::canonical_json(&receipt_value).as_bytes(),
    );
    receipt
}

fn scene_body(receipt: &AssetPreparationReceipt) -> SceneArtifactBody {
    let package = receipt.package.as_ref().unwrap();
    SceneArtifactBody {
        schema_version: SCENE_ARTIFACT_SCHEMA.into(),
        scene_id: "scene-alpine-v1".into(),
        world_artifact_id: "world-alpine-v1".into(),
        world_artifact_sha256:
            "sha256:1111111111111111111111111111111111111111111111111111111111111111".into(),
        objects: vec![SceneObject {
            schema_version: SCENE_OBJECT_SCHEMA.into(),
            object_id: "fortress-gate".into(),
            source_asset_id: receipt.source_identity.asset_id.clone(),
            source_asset_sha256: receipt.source_identity.source_sha256.clone(),
            asset_receipt_sha256: receipt.receipt_sha256.clone(),
            runtime_package_id: package.package_id.clone(),
            mesh_id: "fortress_lod0".into(),
            render_package_id: None,
            render_mesh_id: None,
            semantic_role: "hero environment landmark".into(),
            gameplay_refs: vec![GameplayReference {
                reference_id: "objective-gate".into(),
                kind: "interaction_target".into(),
                required: true,
            }],
            transform: SceneTransform {
                translation_xyz_m: [2.0, 0.0, -4.0],
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
                scale_xyz: [1.0, 1.0, 1.0],
            },
            collision: CollisionPolicy::Static {
                shape: wge_project_ledger::SceneCollisionShape::Box,
            },
            material_assignments: vec![MaterialAssignment {
                slot: 0,
                material_id: "weathered-stone".into(),
                material_artifact_id: None,
                material_sha256: None,
            }],
            importance: SceneImportance::Landmark,
            lod: SceneLodPolicy {
                levels: vec![SceneLodLevel {
                    level: 0,
                    mesh_id: "fortress_lod0".into(),
                    switch_below_fraction: 0.0,
                }],
            },
            visibility: SceneVisibilityPolicy {
                renderable: true,
                casts_shadows: true,
                receives_shadows: true,
                max_distance_m: 500.0,
            },
            provenance: SceneObjectProvenance {
                authoring_id: "authoring-scene-smoke".into(),
                source_refs: vec!["concept-alpine-v1".into()],
                provider_job_id: Some("blender-fortress-conditioning-v1".into()),
            },
        }],
        provenance: SceneProvenance {
            construction_plan_id: "construction-plan-alpine-v1".into(),
            authoring_digest:
                "sha256:2222222222222222222222222222222222222222222222222222222222222222".into(),
            source_refs: vec!["concept-alpine-v1".into(), "brief-alpine-v1".into()],
        },
    }
}

fn ready_render_package(source_identity: &AssetIdentity) -> RenderAssetPackage {
    let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let mut package = RenderAssetPackage {
        package_id: String::new(),
        schema_version: RENDER_ASSET_PACKAGE_SCHEMA.into(),
        source_identity: source_identity.clone(),
        transform: RenderTransform {
            meters_per_unit: 1.0,
            source_vertical_axis: Axis::Y,
            canonical_vertical_axis: Axis::Y,
        },
        meshes: vec![RenderMesh {
            mesh_id: "fortress_lod0".into(),
            source_mesh_index: 0,
            source_primitive_index: 0,
            positions_m: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            tangents: vec![[1.0, 0.0, 0.0, 1.0]; 3],
            uv0: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            material_id: "weathered-stone".into(),
            generated_normals: false,
            generated_tangents: false,
            tangent_fallback_count: 0,
        }],
        materials: vec![RenderMaterial {
            material_id: "weathered-stone".into(),
            base_color_rgba: [0.4, 0.4, 0.4, 1.0],
            metallic: 0.0,
            roughness: 0.8,
            alpha_mode: RenderAlphaMode::Opaque,
            alpha_cutoff: None,
            normal_scale: None,
            occlusion_strength: None,
            double_sided: false,
            base_color_texture_id: None,
            metallic_roughness_texture_id: None,
            normal_texture_id: None,
            occlusion_texture_id: None,
            emissive_texture_id: None,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
            texture_transform: RenderTextureTransform::identity(),
        }],
        textures: vec![RenderTexture {
            texture_id: "unused".into(),
            source_texture_index: 0,
            source_image_index: 0,
            source_sha256: digest.into(),
            mime_type: "image/png".into(),
            width_px: 1,
            height_px: 1,
            mip_levels: 1,
            color_space: RenderTextureColorSpace::Srgb,
            rgba8: vec![255, 255, 255, 255],
            mip_chain: vec![],
        }],
        provenance: RenderAssetProvenance {
            producer: RenderProducerIdentity {
                name: "scene-test-render".into(),
                version: "1".into(),
            },
            source_sha256: source_identity.source_sha256.clone(),
            source_byte_length: source_identity.byte_length,
            request_sha256: digest.into(),
            inspection_report_sha256: digest.into(),
        },
    };
    let mut value = serde_json::to_value(&package).unwrap();
    value.as_object_mut().unwrap().remove("package_id");
    package.package_id = format!(
        "render_asset_sha256_{}",
        wge_project_ledger::sha256_hex(wge_project_ledger::canonical_json(&value).as_bytes())
    );
    validate_render_asset_package(&package).unwrap();
    package
}

#[test]
fn scene_artifact_binds_real_asset_identity_and_is_deterministic() {
    let receipt = ready_asset_receipt();
    let body = scene_body(&receipt);
    let artifact = seal_scene(body.clone(), std::slice::from_ref(&receipt)).unwrap();
    validate_scene_against_asset_receipts(&artifact, std::slice::from_ref(&receipt)).unwrap();
    let replay = seal_scene(body, std::slice::from_ref(&receipt)).unwrap();
    assert_eq!(artifact, replay);
    assert_eq!(artifact.schema_version, SCENE_ARTIFACT_SCHEMA);
    assert!(artifact.artifact_id.starts_with("scene_sha256_"));
}

#[test]
fn scene_rejects_stale_asset_receipt_and_graphics_cannot_reseal_semantics() {
    let receipt = ready_asset_receipt();
    let mut body = scene_body(&receipt);
    let artifact = seal_scene(body.clone(), std::slice::from_ref(&receipt)).unwrap();
    let mut projection = project_scene_for_graphics(&artifact).unwrap();
    projection.objects[0].transform.translation_xyz_m[0] = 999.0;
    assert_eq!(artifact.body.objects[0].transform.translation_xyz_m[0], 2.0);
    body.objects[0].transform.translation_xyz_m[0] = 999.0;
    assert!(seal_scene(body, std::slice::from_ref(&receipt)).is_ok());
    assert!(validate_scene_against_asset_receipts(&artifact, &[]).is_err());

    let mut tampered = artifact.clone();
    tampered.body.objects[0].material_assignments[0].material_id = "forged-material".into();
    assert!(
        validate_scene_against_asset_receipts(&tampered, std::slice::from_ref(&receipt)).is_err()
    );
}

#[test]
fn scene_rejects_invalid_transform_lod_and_package_identity() {
    let receipt = ready_asset_receipt();
    let mut body = scene_body(&receipt);
    body.objects[0].transform.scale_xyz[1] = 0.0;
    assert!(seal_scene(body.clone(), std::slice::from_ref(&receipt)).is_err());

    let mut body = scene_body(&receipt);
    body.objects[0].lod.levels[0].mesh_id = "not-in-package".into();
    assert!(seal_scene(body, std::slice::from_ref(&receipt)).is_err());

    let mut body = scene_body(&receipt);
    body.objects[0].runtime_package_id =
        "runtime_asset_sha256_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .into();
    assert!(seal_scene(body, std::slice::from_ref(&receipt)).is_err());
}

#[test]
fn scene_wire_shape_rejects_unknown_fields() {
    let value = json!({"schema_version": SCENE_ARTIFACT_SCHEMA, "unexpected": true});
    assert!(serde_json::from_value::<SceneArtifactBody>(value).is_err());
}

#[test]
fn render_binding_requires_matching_package_identity_and_mesh() {
    let receipt = ready_asset_receipt();
    let render_package = ready_render_package(&receipt.source_identity);
    let mut body = scene_body(&receipt);
    body.objects[0].render_package_id = Some(render_package.package_id.clone());
    body.objects[0].render_mesh_id = Some("fortress_lod0".into());
    let artifact = seal_scene_with_render_assets(
        body,
        std::slice::from_ref(&receipt),
        std::slice::from_ref(&render_package),
    )
    .unwrap();
    validate_scene_against_asset_receipts_and_render_assets(
        &artifact,
        std::slice::from_ref(&receipt),
        std::slice::from_ref(&render_package),
    )
    .unwrap();
    let projection = project_scene_for_graphics(&artifact).unwrap();
    assert_eq!(
        projection.objects[0].render_package_id,
        Some(render_package.package_id.clone())
    );

    let mut wrong_source = render_package.clone();
    wrong_source.source_identity.source_sha256 =
        "fedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcba".into();
    let mut value = serde_json::to_value(&wrong_source).unwrap();
    value.as_object_mut().unwrap().remove("package_id");
    wrong_source.package_id = format!(
        "render_asset_sha256_{}",
        wge_project_ledger::sha256_hex(wge_project_ledger::canonical_json(&value).as_bytes())
    );
    assert!(
        validate_scene_against_asset_receipts_and_render_assets(
            &artifact,
            std::slice::from_ref(&receipt),
            std::slice::from_ref(&wrong_source),
        )
        .is_err()
    );
}
