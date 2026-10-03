use wge_native_graphics_contract::{
    AlphaMode, BufferReference, CameraProjection, CaptureFormat, CoordinateSystem,
    EnvironmentIntent, GraphicsAssetMesh, GraphicsAssetProjection, GraphicsCamera,
    GraphicsCaptureRequest, GraphicsScenePacketBody, Handedness, InstanceImportance, LightIntent,
    LightKind, MaterialIntent, MeshPacket, compose_bound_scene, sha256_prefixed,
};
use wge_project_ledger::{
    CollisionPolicy, GameplayReference, MaterialAssignment, SCENE_ARTIFACT_SCHEMA,
    SCENE_OBJECT_SCHEMA, SceneArtifact, SceneArtifactBody, SceneImportance, SceneLodLevel,
    SceneLodPolicy, SceneObject, SceneObjectProvenance, SceneProvenance, SceneTransform,
    SceneVisibilityPolicy, canonical_json, sha256_prefixed as ledger_sha256_prefixed,
};

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const RENDER_PACKAGE_ID: &str = "render_package_v1";

fn scene() -> SceneArtifact {
    let body = SceneArtifactBody {
        schema_version: SCENE_ARTIFACT_SCHEMA.into(),
        scene_id: "scene-v1".into(),
        world_artifact_id: "world-v1".into(),
        world_artifact_sha256: sha256_prefixed(b"world-v1"),
        objects: vec![SceneObject {
            schema_version: SCENE_OBJECT_SCHEMA.into(),
            object_id: "hero-object".into(),
            source_asset_id: "asset-v1".into(),
            source_asset_sha256: SOURCE_SHA.into(),
            asset_receipt_sha256: SOURCE_SHA.into(),
            runtime_package_id: "runtime-package-v1".into(),
            mesh_id: "runtime-mesh".into(),
            render_package_id: Some(RENDER_PACKAGE_ID.into()),
            render_mesh_id: Some("hero-mesh".into()),
            semantic_role: "hero landmark".into(),
            gameplay_refs: vec![GameplayReference {
                reference_id: "hero-ref".into(),
                kind: "landmark".into(),
                required: true,
            }],
            transform: SceneTransform {
                translation_xyz_m: [2.0, 0.0, -1.0],
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
                scale_xyz: [1.0, 1.0, 1.0],
            },
            collision: CollisionPolicy::None,
            material_assignments: vec![MaterialAssignment {
                slot: 0,
                material_id: "hero-material".into(),
                material_artifact_id: None,
                material_sha256: None,
            }],
            importance: SceneImportance::Landmark,
            lod: SceneLodPolicy {
                levels: vec![SceneLodLevel {
                    level: 0,
                    mesh_id: "runtime-mesh".into(),
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
                authoring_id: "authoring-v1".into(),
                source_refs: vec!["brief-v1".into()],
                provider_job_id: None,
            },
        }],
        provenance: SceneProvenance {
            construction_plan_id: "plan-v1".into(),
            authoring_digest: sha256_prefixed(b"authoring-v1"),
            source_refs: vec!["brief-v1".into()],
        },
    };
    let body_sha =
        ledger_sha256_prefixed(canonical_json(&serde_json::to_value(&body).unwrap()).as_bytes());
    SceneArtifact {
        schema_version: SCENE_ARTIFACT_SCHEMA.into(),
        artifact_id: format!("scene_sha256_{}", &body_sha[7..]),
        artifact_sha256: body_sha,
        body,
    }
}

fn asset() -> GraphicsAssetProjection {
    GraphicsAssetProjection {
        schema_version: "wge.graphics-asset-projection/v1".into(),
        render_package_id: RENDER_PACKAGE_ID.into(),
        source_asset_id: "asset-v1".into(),
        source_asset_sha256: SOURCE_SHA.into(),
        meshes: vec![GraphicsAssetMesh {
            packet: MeshPacket {
                mesh_id: "hero-mesh".into(),
                positions_m: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                normals: vec![[0.0, 0.0, 1.0]; 3],
                uv0: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                indices: vec![0, 1, 2],
                material_id: "hero-material".into(),
                tangents: vec![[1.0, 0.0, 0.0, 1.0]; 3],
            },
        }],
        materials: vec![MaterialIntent {
            material_id: "hero-material".into(),
            base_color_rgba: [0.6, 0.4, 0.2, 1.0],
            metallic: 0.0,
            roughness: 0.8,
            clearcoat: 0.0,
            clearcoat_roughness: 0.5,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: Vec::new(),
            normal_texture_id: None,
            roughness_texture_id: None,
            occlusion_texture_id: None,
            emissive_texture_id: None,
            normal_scale: 1.0,
            occlusion_strength: 1.0,
            emissive_factor_rgb: [0.0; 3],
        }],
        textures: Vec::new(),
    }
}

fn body() -> GraphicsScenePacketBody {
    GraphicsScenePacketBody {
        deformation: None,
        render_policy: None,
        schema_version: "wge.graphics-scene-packet/v6".into(),
        packet_id: "packet-v1".into(),
        scene_artifact_id: None,
        scene_artifact_sha256: None,
        world_artifact_id: "world-v1".into(),
        world_artifact_sha256: sha256_prefixed(b"world-v1"),
        spatial_fields_sha256: sha256_prefixed(b"fields-v1"),
        frame_seed: 1,
        coordinate_system: CoordinateSystem {
            up_axis: wge_native_graphics_contract::Axis::Y,
            handedness: Handedness::Right,
            units_per_meter: 1.0,
        },
        camera: GraphicsCamera {
            camera_id: "camera-v1".into(),
            projection: CameraProjection::Perspective {
                fov_y_degrees: 60.0,
            },
            position_xyz_m: [0.0, 3.0, 6.0],
            forward_xyz: [0.0, -0.2, -1.0],
            up_xyz: [0.0, 1.0, 0.0],
            near_plane_m: 0.1,
            far_plane_m: 100.0,
            width_px: 64,
            height_px: 64,
        },
        terrain: wge_native_graphics_contract::TerrainPacket {
            terrain_id: "terrain-v1".into(),
            width_m: 10.0,
            length_m: 10.0,
            resolution: 3,
            material_id: "terrain-material".into(),
            heights_m: BufferReference::inline_f32("heights", vec![0.0; 9]),
            slope_grade: BufferReference::inline_f32("slopes", vec![0.0; 9]),
            region_codes: BufferReference::inline_u8("regions", vec![1; 9]),
        },
        materials: vec![MaterialIntent {
            material_id: "terrain-material".into(),
            base_color_rgba: [0.2, 0.3, 0.2, 1.0],
            metallic: 0.0,
            roughness: 0.9,
            clearcoat: 0.0,
            clearcoat_roughness: 0.5,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: Vec::new(),
            normal_texture_id: None,
            roughness_texture_id: None,
            occlusion_texture_id: None,
            emissive_texture_id: None,
            normal_scale: 1.0,
            occlusion_strength: 1.0,
            emissive_factor_rgb: [0.0; 3],
        }],
        textures: Vec::new(),
        meshes: Vec::new(),
        instances: Vec::new(),
        lights: vec![LightIntent {
            light_id: "sun".into(),
            kind: LightKind::Directional {
                direction_xyz: [0.0, -1.0, 0.0],
            },
            color_rgb: [1.0; 3],
            intensity: 1.0,
        }],
        environment: EnvironmentIntent {
            sky_top_rgb: [0.1, 0.2, 0.3],
            sky_horizon_rgb: [0.4, 0.5, 0.6],
            ground_rgb: [0.1, 0.1, 0.1],
            fog_color_rgb: [0.4, 0.5, 0.6],
            fog_density: 0.001,
            exposure: 1.0,
        },
        overlays: Vec::new(),
        capture: GraphicsCaptureRequest {
            capture_id: "capture-v1".into(),
            camera_id: "camera-v1".into(),
            width_px: 64,
            height_px: 64,
            format: CaptureFormat::Rgba8Srgb,
            include_depth: false,
            deterministic: true,
        },
    }
}

#[test]
fn bound_scene_composes_namespaced_assets_and_scene_identity() {
    let packet = compose_bound_scene(body(), &scene(), &[asset()]).expect("composition works");
    assert!(!packet.body.scene_artifact_id.as_deref().unwrap().is_empty());
    assert_eq!(packet.body.instances.len(), 1);
    assert!(
        packet.body.meshes[0]
            .mesh_id
            .starts_with("render_package_v1::")
    );
    assert_eq!(
        packet.body.instances[0].importance,
        InstanceImportance::Landmark
    );
    assert!(packet.body.instances[0].transform.translation_xyz_m[0] > 1.9);
}

#[test]
fn bound_scene_rejects_world_or_source_identity_drift() {
    let mut wrong_body = body();
    wrong_body.world_artifact_id = "other-world".into();
    assert!(compose_bound_scene(wrong_body, &scene(), &[asset()]).is_err());

    let mut wrong_asset = asset();
    wrong_asset.source_asset_sha256 =
        "fedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcba".into();
    assert!(compose_bound_scene(body(), &scene(), &[wrong_asset]).is_err());
}
