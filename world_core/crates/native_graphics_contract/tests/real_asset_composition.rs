use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use wge_asset_contract::{
    ASSET_RUNTIME_RECEIPT_SCHEMA, ASSET_RUNTIME_REQUEST_SCHEMA, AssetPreparationRequest, AssetUse,
    Axis, CollisionMetadata, CollisionShape, LodMetadata, PreparationStatus,
    RenderConditioningRequest, RenderMipPolicy, RenderPreparationStatus, RuntimeTarget,
    condition_render_asset, prepare_asset,
};
use wge_native_graphics_contract::{
    AlphaMode, BoundSceneRenderAuthorization, BufferReference, CameraProjection, CaptureFormat,
    CoordinateSystem, EnvironmentIntent, GraphicsCamera, GraphicsCaptureRequest,
    GraphicsScenePacketBody, GraphicsWorkerSupervisor, Handedness, LightIntent, LightKind,
    MaterialIntent, compose_bound_scene, compose_bound_scene_with_camera,
    deterministic_certification_frame_receipt, lower_reference_world, lower_showcase_packet,
    project_render_asset, sha256_prefixed, validate_graphics_asset_projection,
};
use wge_project_ledger::{
    CollisionPolicy, SCENE_ARTIFACT_SCHEMA, SCENE_OBJECT_SCHEMA, SceneArtifact, SceneArtifactBody,
    SceneImportance, SceneLodLevel, SceneLodPolicy, SceneObject, SceneObjectProvenance,
    SceneProvenance, SceneTransform, SceneVisibilityPolicy, canonical_json,
    seal_scene_with_render_assets, sha256_prefixed as ledger_sha256_prefixed,
};
use wge_reference_runtime::build_from_layout_path;

const SOURCE_SHA256: &str = "9560590b27ca1b847cc4b96f7659e99acf5b8fb18622e80ef0b4c2ae7ffd068f";

fn runtime_request() -> AssetPreparationRequest {
    AssetPreparationRequest {
        schema_version: ASSET_RUNTIME_REQUEST_SCHEMA.into(),
        target: RuntimeTarget::Native,
        asset_use: AssetUse::StaticMesh,
        expected_source_sha256: Some(SOURCE_SHA256.into()),
        meters_per_unit: 1.0,
        vertical_axis: Axis::Y,
        rig: None,
        required_animations: Vec::new(),
        required_sockets: Vec::new(),
        collision: Some(CollisionMetadata {
            shape: CollisionShape::Box,
            center: [0.0, 1.469125271, 0.0],
            size: [2.52059126, 2.938250542, 2.26999998],
            axis: None,
        }),
        lods: vec![LodMetadata {
            level: 0,
            mesh_name: "chimney_mesh".into(),
            switch_below_fraction: 1.0,
        }],
    }
}

fn render_request() -> RenderConditioningRequest {
    RenderConditioningRequest {
        schema_version: "wge.render-asset-request/v1".into(),
        meters_per_unit: 1.0,
        vertical_axis: Axis::Y,
        require_uv0: true,
        generate_normals: true,
        generate_tangents: true,
        mip_policy: RenderMipPolicy::SingleLevelExplicit,
        max_texture_dimension: 8192,
    }
}

fn scene_for_asset(
    runtime_receipt: &wge_asset_contract::AssetPreparationReceipt,
    render_package: &wge_asset_contract::RenderAssetPackage,
) -> SceneArtifact {
    scene_for_asset_at(
        runtime_receipt,
        render_package,
        "world-real-log-hut-test",
        ledger_sha256_prefixed(b"world-real-log-hut-test"),
        [0.0, 0.0, 0.0],
    )
}

fn scene_for_asset_at(
    runtime_receipt: &wge_asset_contract::AssetPreparationReceipt,
    render_package: &wge_asset_contract::RenderAssetPackage,
    world_artifact_id: &str,
    world_artifact_sha256: String,
    translation_xyz_m: [f64; 3],
) -> SceneArtifact {
    let runtime_package = runtime_receipt.package.as_ref().unwrap();
    let objects = render_package
        .meshes
        .iter()
        .enumerate()
        .map(|(index, mesh)| SceneObject {
            schema_version: SCENE_OBJECT_SCHEMA.into(),
            object_id: format!("hut-part-{index}"),
            source_asset_id: runtime_receipt.source_identity.asset_id.clone(),
            source_asset_sha256: runtime_receipt.source_identity.source_sha256.clone(),
            asset_receipt_sha256: runtime_receipt.receipt_sha256.clone(),
            runtime_package_id: runtime_package.package_id.clone(),
            mesh_id: mesh.mesh_id.clone(),
            render_package_id: Some(render_package.package_id.clone()),
            render_mesh_id: Some(mesh.mesh_id.clone()),
            semantic_role: if mesh.mesh_id == "walls_mesh" {
                "hero static landmark shell".into()
            } else {
                "hero static authored part".into()
            },
            gameplay_refs: Vec::new(),
            transform: SceneTransform {
                translation_xyz_m,
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
                scale_xyz: [1.0, 1.0, 1.0],
            },
            collision: if mesh.mesh_id == "foundation_mesh" {
                CollisionPolicy::Static {
                    shape: wge_project_ledger::SceneCollisionShape::Box,
                }
            } else {
                CollisionPolicy::None
            },
            material_assignments: vec![wge_project_ledger::MaterialAssignment {
                slot: 0,
                material_id: mesh.material_id.clone(),
                material_artifact_id: None,
                material_sha256: None,
            }],
            importance: if mesh.mesh_id == "walls_mesh" {
                SceneImportance::Landmark
            } else {
                SceneImportance::Background
            },
            lod: SceneLodPolicy {
                levels: vec![SceneLodLevel {
                    level: 0,
                    mesh_id: mesh.mesh_id.clone(),
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
                authoring_id: "real-assembled-log-hut-v007".into(),
                source_refs: vec!["assembled_log_hut_v007.glb".into()],
                provider_job_id: None,
            },
        })
        .collect();
    let body = SceneArtifactBody {
        schema_version: SCENE_ARTIFACT_SCHEMA.into(),
        scene_id: "scene-real-log-hut-v1".into(),
        world_artifact_id: world_artifact_id.into(),
        world_artifact_sha256,
        objects,
        provenance: SceneProvenance {
            construction_plan_id: "construction-real-log-hut-v1".into(),
            authoring_digest: ledger_sha256_prefixed(b"assembled_log_hut_v007"),
            source_refs: vec!["assembled_log_hut_v007.glb".into()],
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

fn base_body() -> GraphicsScenePacketBody {
    GraphicsScenePacketBody {
        schema_version: "wge.graphics-scene-packet/v6".into(),
        packet_id: "real-asset-packet".into(),
        scene_artifact_id: None,
        scene_artifact_sha256: None,
        world_artifact_id: "world-real-log-hut-test".into(),
        world_artifact_sha256: sha256_prefixed(b"world-real-log-hut-test"),
        spatial_fields_sha256: sha256_prefixed(b"real-log-hut-fields"),
        frame_seed: 1,
        coordinate_system: CoordinateSystem {
            up_axis: wge_native_graphics_contract::Axis::Y,
            handedness: Handedness::Right,
            units_per_meter: 1.0,
        },
        camera: GraphicsCamera {
            camera_id: "real-asset-camera".into(),
            projection: CameraProjection::Perspective {
                fov_y_degrees: 60.0,
            },
            position_xyz_m: [0.0, 2.0, 5.0],
            forward_xyz: [0.0, 0.0, -1.0],
            up_xyz: [0.0, 1.0, 0.0],
            near_plane_m: 0.1,
            far_plane_m: 100.0,
            width_px: 64,
            height_px: 64,
        },
        terrain: wge_native_graphics_contract::TerrainPacket {
            terrain_id: "real-asset-terrain".into(),
            width_m: 20.0,
            length_m: 20.0,
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
            capture_id: "real-asset-capture".into(),
            camera_id: "real-asset-camera".into(),
            width_px: 64,
            height_px: 64,
            format: CaptureFormat::Rgba8Srgb,
            include_depth: false,
            deterministic: true,
        },
    }
}

fn normalize(vector: [f32; 3]) -> [f32; 3] {
    let length = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    [vector[0] / length, vector[1] / length, vector[2] / length]
}

fn imported_asset_close_camera(scene: &SceneArtifact) -> GraphicsCamera {
    let [x, y, z] = scene
        .body
        .objects
        .first()
        .expect("real asset scene has an object")
        .transform
        .translation_xyz_m
        .map(|value| value as f32);
    let target = [x, y + 1.25, z];
    let position = [x + 3.8, y + 2.55, z + 4.8];
    let forward = normalize([
        target[0] - position[0],
        target[1] - position[1],
        target[2] - position[2],
    ]);
    let right = normalize([-forward[2], 0.0, forward[0]]);
    let up = normalize([
        right[1] * forward[2] - right[2] * forward[1],
        right[2] * forward[0] - right[0] * forward[2],
        right[0] * forward[1] - right[1] * forward[0],
    ]);
    GraphicsCamera {
        camera_id: "real-asset-close".into(),
        projection: CameraProjection::Perspective {
            fov_y_degrees: 42.0,
        },
        position_xyz_m: position,
        forward_xyz: forward,
        up_xyz: up,
        near_plane_m: 0.05,
        far_plane_m: 64.0,
        width_px: 640,
        height_px: 480,
    }
}

fn julia_executable() -> PathBuf {
    std::env::var_os("WGE_JULIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("julia"))
}

fn graphics_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("real asset graphics test lock is not poisoned")
}

fn terrain_height_at(world: &wge_reference_runtime::WorldArtifact, x: f64, z: f64) -> f64 {
    let layout = &world.body.authored_layout;
    let resolution = world.body.fields.resolution;
    let denominator = (resolution - 1) as f64;
    let column = (((x + layout.width_m / 2.0) / layout.width_m) * denominator)
        .round()
        .clamp(0.0, denominator) as usize;
    let row = (((layout.length_m / 2.0 - z) / layout.length_m) * denominator)
        .round()
        .clamp(0.0, denominator) as usize;
    world.body.fields.heights_m[row * resolution + column]
}

fn rgba8_to_ppm(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let expected = width as usize * height as usize * 4;
    assert_eq!(rgba.len(), expected, "capture has the expected RGBA8 size");
    let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
    ppm.reserve(width as usize * height as usize * 3);
    for pixel in rgba.chunks_exact(4) {
        ppm.extend_from_slice(&pixel[..3]);
    }
    ppm
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) {
    let bytes = serde_json::to_vec_pretty(value).expect("artifact serializes");
    fs::write(path, bytes).expect("artifact writes");
}

#[test]
fn supplied_real_asset_survives_native_prepare_condition_project_and_scene_composition() {
    let bytes = include_bytes!("../../asset_contract/tests/fixtures/assembled_log_hut.glb");
    let runtime_receipt = prepare_asset(bytes, &runtime_request()).expect("runtime inspection");
    assert_eq!(runtime_receipt.status, PreparationStatus::Ready);
    assert_eq!(runtime_receipt.schema_version, ASSET_RUNTIME_RECEIPT_SCHEMA);
    assert_eq!(runtime_receipt.package.as_ref().unwrap().mesh_ids.len(), 5);

    let render_receipt =
        condition_render_asset(bytes, &render_request()).expect("render conditioning");
    assert_eq!(render_receipt.status, RenderPreparationStatus::Ready);
    assert!(render_receipt.findings.is_empty());
    let render_package = render_receipt.package.as_ref().unwrap();
    assert_eq!(render_package.meshes.len(), 5);
    assert_eq!(render_package.textures.len(), 5);
    let projection = project_render_asset(render_package).expect("graphics projection");
    validate_graphics_asset_projection(&projection).expect("projection revalidates");

    let scene = scene_for_asset(&runtime_receipt, render_package);
    let runtime_receipt_ref = runtime_receipt;
    let scene = seal_scene_with_render_assets(
        scene.body,
        std::slice::from_ref(&runtime_receipt_ref),
        std::slice::from_ref(render_package),
    )
    .expect("render-bound scene seals");
    let packet = compose_bound_scene(base_body(), &scene, &[projection])
        .expect("real asset composes into native packet");
    assert_eq!(packet.body.instances.len(), 5);
    assert_eq!(packet.body.meshes.len(), 5);
    assert_eq!(packet.body.textures.len(), 5);
    assert_eq!(packet.body.scene_artifact_id, Some(scene.artifact_id));
}

#[test]
#[ignore = "requires a Vulkan device and persistent Julia/Lava worker; run explicitly for C2.4"]
fn supplied_real_asset_renders_in_lava_and_replays_after_worker_restart() {
    let _guard = graphics_test_guard();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the WGE workspace");
    let layout = manifest_dir
        .join("../reference_runtime/examples/riverwatch.layout.json")
        .canonicalize()
        .expect("reference layout exists");
    let graphics_lab = workspace_root.join("graphics_lab");
    let terrain_lab = workspace_root.join("terrain_lab");
    let worker = graphics_lab.join("bin/wge_graphics_worker.jl");
    let output_dir = std::env::var_os("WGE_C2_4_CAPTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("wge-c2-4-real-asset-render-{}", std::process::id()))
        });
    fs::create_dir_all(&output_dir).expect("capture output directory is writable");
    let input = output_dir.join("riverwatch.layout.json");
    fs::copy(&layout, &input).expect("layout copies into the capture bundle");

    let build = build_from_layout_path(&input, &julia_executable(), &terrain_lab)
        .expect("Julia-backed reference world builds");
    let reference_packet =
        lower_reference_world(&build.world).expect("validated world lowers to graphics packet");
    let base_packet = lower_showcase_packet(&reference_packet)
        .expect("existing showcase camera lowers without changing world identity");
    let base_instance_count = base_packet.body.instances.len();
    let base_mesh_count = base_packet.body.meshes.len();

    let bytes = include_bytes!("../../asset_contract/tests/fixtures/assembled_log_hut.glb");
    let runtime_receipt = prepare_asset(bytes, &runtime_request()).expect("runtime inspection");
    assert_eq!(runtime_receipt.status, PreparationStatus::Ready);
    let render_receipt =
        condition_render_asset(bytes, &render_request()).expect("render conditioning");
    assert_eq!(render_receipt.status, RenderPreparationStatus::Ready);
    assert!(render_receipt.findings.is_empty());
    let render_package = render_receipt
        .package
        .as_ref()
        .expect("render package exists");
    let projection = project_render_asset(render_package).expect("graphics projection");
    validate_graphics_asset_projection(&projection).expect("projection revalidates");

    let objective = build
        .world
        .body
        .authored_layout
        .traversal
        .objective_position_xz_m;
    let scene = scene_for_asset_at(
        &runtime_receipt,
        render_package,
        &build.world.artifact_id,
        build.world.artifact_sha256.clone(),
        [
            objective[0],
            terrain_height_at(&build.world, objective[0], objective[1]),
            objective[1],
        ],
    );
    let scene = seal_scene_with_render_assets(
        scene.body,
        std::slice::from_ref(&runtime_receipt),
        std::slice::from_ref(render_package),
    )
    .expect("real asset scene seals against the live world");
    let context_packet = compose_bound_scene(
        base_packet.body.clone(),
        &scene,
        std::slice::from_ref(&projection),
    )
    .expect("real asset composes into the live native context packet");
    let close_camera = imported_asset_close_camera(&scene);
    let mut close_body = base_packet.body.clone();
    let close_packet = compose_bound_scene_with_camera(
        &mut close_body,
        &scene,
        std::slice::from_ref(&projection),
        Some(&close_camera),
    )
    .expect("real asset composes into the live close-inspection packet");
    assert_eq!(
        context_packet.body.world_artifact_id,
        build.world.artifact_id
    );
    assert_eq!(
        context_packet.body.scene_artifact_id,
        Some(scene.artifact_id.clone())
    );
    assert_eq!(context_packet.body.instances.len(), base_instance_count + 5);
    assert_eq!(context_packet.body.meshes.len(), base_mesh_count + 5);
    assert_eq!(close_packet.body.camera.camera_id, "real-asset-close");
    assert_eq!(close_packet.body.capture.camera_id, "real-asset-close");
    assert_eq!(close_packet.body.instances.len(), base_instance_count + 5);
    assert_eq!(close_packet.body.meshes.len(), base_mesh_count + 5);
    assert!(
        close_packet
            .body
            .meshes
            .iter()
            .any(|mesh| mesh.mesh_id.starts_with("render_asset_sha256_"))
    );
    assert!(
        close_packet
            .body
            .meshes
            .iter()
            .filter(|mesh| mesh.mesh_id.starts_with("render_asset_sha256_"))
            .all(|mesh| mesh.tangents.len() == mesh.positions_m.len())
    );

    let mut supervisor =
        GraphicsWorkerSupervisor::start(julia_executable(), &graphics_lab, &worker)
            .expect("Rust supervisor starts the persistent Lava worker");
    supervisor
        .capabilities()
        .expect("Rust independently validates Lava capabilities");
    let context_frame = supervisor
        .render_bound_scene_and_promote(
            &context_packet,
            &build.world,
            BoundSceneRenderAuthorization {
                base_packet: &base_packet,
                scene: &scene,
                asset_receipts: std::slice::from_ref(&runtime_receipt),
                render_packages: std::slice::from_ref(render_package),
                assets: std::slice::from_ref(&projection),
                camera: None,
            },
        )
        .expect("Rust promotes the real asset context frame");
    let context_certified = deterministic_certification_frame_receipt(
        &context_packet,
        &context_frame.receipt,
        &context_frame.capture_bytes,
    )
    .expect("context frame has a deterministic certification projection");
    let first = supervisor
        .render_bound_scene_and_promote(
            &close_packet,
            &build.world,
            BoundSceneRenderAuthorization {
                base_packet: &base_packet,
                scene: &scene,
                asset_receipts: std::slice::from_ref(&runtime_receipt),
                render_packages: std::slice::from_ref(render_package),
                assets: std::slice::from_ref(&projection),
                camera: Some(&close_camera),
            },
        )
        .expect("Rust promotes the real asset close frame");
    assert!(first.receipt.body.telemetry.mesh_vertex_count > 0);
    assert!(first.receipt.body.telemetry.visible_instance_count >= 1);
    let first_certified = deterministic_certification_frame_receipt(
        &close_packet,
        &first.receipt,
        &first.capture_bytes,
    )
    .expect("close frame has a deterministic certification projection");

    supervisor
        .restart()
        .expect("worker restart succeeds before replay");
    supervisor
        .capabilities()
        .expect("restarted worker capabilities revalidate");
    supervisor
        .render_bound_scene_and_promote(
            &context_packet,
            &build.world,
            BoundSceneRenderAuthorization {
                base_packet: &base_packet,
                scene: &scene,
                asset_receipts: std::slice::from_ref(&runtime_receipt),
                render_packages: std::slice::from_ref(render_package),
                assets: std::slice::from_ref(&projection),
                camera: None,
            },
        )
        .expect("restarted worker reproduces the context warm-up");
    let replay = supervisor
        .render_bound_scene_and_promote(
            &close_packet,
            &build.world,
            BoundSceneRenderAuthorization {
                base_packet: &base_packet,
                scene: &scene,
                asset_receipts: std::slice::from_ref(&runtime_receipt),
                render_packages: std::slice::from_ref(render_package),
                assets: std::slice::from_ref(&projection),
                camera: Some(&close_camera),
            },
        )
        .expect("restarted worker promotes the same real asset close frame");
    let replay_certified = deterministic_certification_frame_receipt(
        &close_packet,
        &replay.receipt,
        &replay.capture_bytes,
    )
    .expect("replayed close frame has a deterministic certification projection");
    assert_eq!(first.capture_bytes, replay.capture_bytes);
    assert_eq!(first.frame.capture_sha256, replay.frame.capture_sha256);
    assert_eq!(
        first_certified.receipt_sha256,
        replay_certified.receipt_sha256
    );

    write_json(&output_dir.join("world_artifact.json"), &build.world);
    write_json(&output_dir.join("scene_artifact.json"), &scene);
    write_json(
        &output_dir.join("context_graphics_scene_packet.json"),
        &context_packet,
    );
    write_json(
        &output_dir.join("context_graphics_frame_receipt.json"),
        &context_certified,
    );
    fs::write(
        output_dir.join("context_capture.rgba"),
        &context_frame.capture_bytes,
    )
    .expect("context RGBA capture writes");
    fs::write(
        output_dir.join("context_capture.ppm"),
        rgba8_to_ppm(
            &context_frame.capture_bytes,
            context_frame.frame.width_px,
            context_frame.frame.height_px,
        ),
    )
    .expect("context PPM capture writes");
    write_json(
        &output_dir.join("graphics_scene_packet.json"),
        &close_packet,
    );
    write_json(
        &output_dir.join("graphics_frame_receipt.json"),
        &first_certified,
    );
    write_json(
        &output_dir.join("graphics_renderer_attestation.json"),
        &first.renderer_attestation,
    );
    write_json(
        &output_dir.join("replay_graphics_frame_receipt.json"),
        &replay_certified,
    );
    fs::write(output_dir.join("native_capture.rgba"), &first.capture_bytes)
        .expect("raw RGBA capture writes");
    fs::write(
        output_dir.join("native_capture.ppm"),
        rgba8_to_ppm(
            &first.capture_bytes,
            first.frame.width_px,
            first.frame.height_px,
        ),
    )
    .expect("PPM capture writes");
    fs::write(
        output_dir.join("replay_capture.rgba"),
        &replay.capture_bytes,
    )
    .expect("replay RGBA capture writes");
}
