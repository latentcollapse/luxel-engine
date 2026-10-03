use wge_native_graphics_contract::{
    ADAPTER_REVISION, AlphaMode, Axis, BufferReference, CaptureFormat, CoordinateSystem,
    EnvironmentIntent, FRAME_RECEIPT_SCHEMA, FrameStatus, GraphicsCamera, GraphicsCaptureRequest,
    GraphicsFrameReceipt, GraphicsFrameReceiptBody, GraphicsPassTimings, GraphicsScenePacket,
    GraphicsScenePacketBody, GraphicsTelemetry, Handedness, InstanceImportance, InstancePacket,
    LAVA_BACKEND_ID, LAVA_REVISION, LightIntent, LightKind, MarkerRole, MaterialIntent, MeshPacket,
    QualityOutcome, QualityReasonCode, QualityRegion, SemanticOverlay, TerrainPacket, Transform3d,
    VisualQualityProfile, assess_visual_quality, canonical_json,
    deterministic_certification_frame_receipt, measure_frame_capture, seal_scene_packet,
    sha256_prefixed, validate_native_visual_gate, validate_registered_visual_quality_profile,
    validate_visual_quality_evidence,
};

const IMAGE_SIDE: usize = 64;

fn packet(overlays: bool) -> GraphicsScenePacket {
    let camera = GraphicsCamera {
        camera_id: "quality-camera".into(),
        projection: wge_native_graphics_contract::CameraProjection::Orthographic { span_m: 16.0 },
        position_xyz_m: [0.0, 10.0, 0.0],
        forward_xyz: [0.0, -1.0, 0.0],
        up_xyz: [0.0, 0.0, -1.0],
        near_plane_m: 0.1,
        far_plane_m: 100.0,
        width_px: IMAGE_SIDE as u32,
        height_px: IMAGE_SIDE as u32,
    };
    let mut semantic_overlays = Vec::new();
    if overlays {
        let markers = [
            (
                "route",
                MarkerRole::Route,
                [-5.0, 0.0, -5.0],
                [0.0, 1.0, 0.0, 1.0],
            ),
            (
                "player",
                MarkerRole::PlayerSpawn,
                [0.0, 0.0, -5.0],
                [0.0, 0.0, 1.0, 1.0],
            ),
            (
                "opponent",
                MarkerRole::OpponentSpawn,
                [5.0, 0.0, -5.0],
                [1.0, 0.0, 0.0, 1.0],
            ),
            (
                "encounter",
                MarkerRole::Encounter,
                [-5.0, 0.0, 5.0],
                [1.0, 1.0, 0.0, 1.0],
            ),
            (
                "objective",
                MarkerRole::Objective,
                [5.0, 0.0, 5.0],
                [1.0, 0.0, 1.0, 1.0],
            ),
        ];
        for (marker_id, role, position_xyz_m, color_rgba) in markers {
            semantic_overlays.push(SemanticOverlay::Point {
                marker_id: marker_id.into(),
                role,
                position_xyz_m,
                radius_m: 2.0,
                color_rgba,
            });
        }
    }
    seal_scene_packet(GraphicsScenePacketBody {
        deformation: None,
        render_policy: None,
        schema_version: wge_native_graphics_contract::SCENE_PACKET_SCHEMA.into(),
        packet_id: if overlays {
            "quality-overlay-packet".into()
        } else {
            "quality-terrain-packet".into()
        },
        scene_artifact_id: None,
        scene_artifact_sha256: None,
        world_artifact_id: "quality-test-world".into(),
        world_artifact_sha256: sha256_prefixed(b"quality-world"),
        spatial_fields_sha256: sha256_prefixed(b"quality-fields"),
        frame_seed: 5,
        coordinate_system: CoordinateSystem {
            up_axis: Axis::Y,
            handedness: Handedness::Right,
            units_per_meter: 1.0,
        },
        camera: camera.clone(),
        terrain: TerrainPacket {
            terrain_id: "quality-terrain".into(),
            width_m: 16.0,
            length_m: 16.0,
            resolution: 3,
            material_id: "quality-ground".into(),
            heights_m: BufferReference::inline_f32("height", vec![0.0; 9]),
            slope_grade: BufferReference::inline_f32("slope", vec![0.0; 9]),
            region_codes: BufferReference::inline_u8("regions", vec![1; 9]),
            layers: None,
        },
        materials: vec![MaterialIntent {
            material_id: "quality-ground".into(),
            base_color_rgba: [0.25, 0.30, 0.20, 1.0],
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
                direction_xyz: [0.2, -1.0, 0.1],
            },
            color_rgb: [1.0; 3],
            intensity: 1.0,
        }],
        environment: EnvironmentIntent {
            sky_top_rgb: [0.1, 0.2, 0.3],
            sky_horizon_rgb: [0.3, 0.4, 0.5],
            ground_rgb: [0.1, 0.1, 0.1],
            fog_color_rgb: [0.2, 0.2, 0.2],
            fog_density: 0.0,
            exposure: 1.0,
        },
        overlays: semantic_overlays,
        capture: GraphicsCaptureRequest {
            capture_id: if overlays {
                "quality-overlay-capture".into()
            } else {
                "quality-terrain-capture".into()
            },
            camera_id: camera.camera_id,
            width_px: IMAGE_SIDE as u32,
            height_px: IMAGE_SIDE as u32,
            format: CaptureFormat::Rgba8Srgb,
            include_depth: false,
            deterministic: true,
        },
    })
    .expect("fixture scene packet is valid")
}

fn receipt(packet: &GraphicsScenePacket, pixels: &[u8]) -> GraphicsFrameReceipt {
    let measurements = measure_frame_capture(packet, pixels).expect("capture measures");
    let body = GraphicsFrameReceiptBody {
        schema_version: FRAME_RECEIPT_SCHEMA.into(),
        packet_sha256: packet.packet_sha256.clone(),
        capture_id: packet.body.capture.capture_id.clone(),
        backend_id: LAVA_BACKEND_ID.into(),
        adapter_revision: ADAPTER_REVISION.into(),
        lava_revision: LAVA_REVISION.into(),
        device_uuid: "quality-test-device".into(),
        worker_script_sha256: sha256_prefixed(b"worker"),
        renderer_identity_sha256: sha256_prefixed(b"renderer"),
        status: FrameStatus::Passed,
        format: CaptureFormat::Rgba8Srgb,
        width_px: IMAGE_SIDE as u32,
        height_px: IMAGE_SIDE as u32,
        capture_sha256: Some(sha256_prefixed(pixels)),
        measurements,
        telemetry: GraphicsTelemetry {
            deformation: None,
            upload_bytes: 0,
            readback_bytes: pixels.len(),
            draw_calls: 1,
            dispatch_calls: 0,
            pipeline_compilations: 0,
            instance_count: 0,
            visible_instance_count: 0,
            culled_instance_count: 0,
            background_visible_instance_count: 0,
            background_culled_instance_count: 0,
            landmark_visible_instance_count: 0,
            landmark_culled_instance_count: 0,
            gameplay_critical_visible_instance_count: 0,
            gameplay_critical_culled_instance_count: 0,
            terrain_vertex_count: 9,
            mesh_vertex_count: 0,
            texture_residency: None,
            frame_time_us: 1,
            gpu_frame_time_us: None,
            pass_timings: GraphicsPassTimings {
                prepare_us: 0,
                scene_raster_us: 0,
                resolve_us: 0,
                overlay_us: 0,
                flush_readback_us: 0,
                gpu_prepare_us: None,
                gpu_scene_raster_us: None,
                gpu_resolve_us: None,
                gpu_overlay_us: None,
            },
        },
        detail: "synthetic quality-gate control receipt".into(),
    };
    let receipt_sha256 = sha256_prefixed(&canonical_json(&body).expect("receipt serializes"));
    GraphicsFrameReceipt {
        body,
        receipt_sha256,
    }
}

fn structured_terrain() -> Vec<u8> {
    let palette = [
        [34u8, 63u8, 39u8],
        [77, 89, 45],
        [121, 112, 67],
        [47, 92, 105],
        [91, 58, 48],
        [106, 119, 81],
    ];
    let mut pixels = Vec::with_capacity(IMAGE_SIDE * IMAGE_SIDE * 4);
    for y in 0..IMAGE_SIDE {
        for x in 0..IMAGE_SIDE {
            let tile = ((x / 8) + 2 * (y / 8)) % palette.len();
            let mut rgb = palette[tile];
            let ridge = (x % 4 < 2) ^ (y % 4 < 2);
            for channel in &mut rgb {
                *channel = if ridge {
                    channel.saturating_add(18)
                } else {
                    channel.saturating_sub(12)
                };
            }
            pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    pixels
}

#[test]
fn terrain_quality_excludes_projected_authored_geometry() {
    let base_packet = packet(false);
    let pixels = structured_terrain();
    let base_receipt = receipt(&base_packet, &pixels);
    let base = assess_visual_quality(
        &base_packet,
        &base_receipt,
        &pixels,
        &VisualQualityProfile::terrain_reference_v1(IMAGE_SIDE as u32, IMAGE_SIDE as u32),
    );
    let mut body = base_packet.body;
    body.packet_id = "quality-geometry-mask-packet".into();
    body.capture.capture_id = "quality-geometry-mask-capture".into();
    body.meshes.push(MeshPacket {
        mesh_id: "quality-prop".into(),
        positions_m: vec![
            [-2.0, 0.0, -2.0],
            [2.0, 0.0, -2.0],
            [2.0, 0.0, 2.0],
            [-2.0, 0.0, 2.0],
        ],
        normals: vec![[0.0, 1.0, 0.0]; 4],
        uv0: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        indices: vec![0, 1, 2, 0, 2, 3],
        material_id: "quality-ground".into(),
        tangents: Vec::new(),
    });
    body.instances.push(InstancePacket {
        instance_id: "quality-prop-instance".into(),
        mesh_id: "quality-prop".into(),
        material_id: "quality-ground".into(),
        importance: InstanceImportance::Landmark,
        transform: Transform3d {
            translation_xyz_m: [0.0, 0.0, 0.0],
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            scale_xyz: [1.0, 1.0, 1.0],
        },
    });
    let geometry_packet = seal_scene_packet(body).expect("geometry fixture reseals");
    let geometry_receipt = receipt(&geometry_packet, &pixels);
    let geometry = assess_visual_quality(
        &geometry_packet,
        &geometry_receipt,
        &pixels,
        &VisualQualityProfile::terrain_reference_v1(IMAGE_SIDE as u32, IMAGE_SIDE as u32),
    );
    assert_eq!(base.body.outcome, QualityOutcome::Good);
    assert_eq!(geometry.body.outcome, QualityOutcome::Good);
    assert!(
        geometry
            .body
            .measurements
            .as_ref()
            .expect("geometry evidence has measurements")
            .terrain_region_pixels
            < base
                .body
                .measurements
                .as_ref()
                .expect("base evidence has measurements")
                .terrain_region_pixels
    );
    let geometry_measurements = geometry
        .body
        .measurements
        .as_ref()
        .expect("geometry evidence has measurements");
    assert!(geometry_measurements.authored_geometry_pixels > 0);
    assert!(
        geometry_measurements.content_region_pixels > geometry_measurements.terrain_region_pixels
    );
}

fn flat_with_semantic_discs(packet: &GraphicsScenePacket) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(IMAGE_SIDE * IMAGE_SIDE * 4);
    for _ in 0..IMAGE_SIDE * IMAGE_SIDE {
        pixels.extend_from_slice(&[48, 56, 42, 255]);
    }
    let radius = 11.5f32;
    let colors = [
        [0u8, 255u8, 0u8, 255u8],
        [0, 0, 255, 255],
        [255, 0, 0, 255],
        [255, 255, 0, 255],
        [255, 0, 255, 255],
    ];
    for (overlay, color) in packet.body.overlays.iter().zip(colors) {
        let SemanticOverlay::Point { position_xyz_m, .. } = overlay else {
            unreachable!("fixture uses points")
        };
        let screen_x = IMAGE_SIDE as f32 * 0.5 + position_xyz_m[0] * 4.0;
        // Match the Vulkan positive-height framebuffer convention used by the
        // Rust and Julia camera lowerings: semantic camera-up is a smaller
        // top-to-bottom pixel Y.
        let screen_y = IMAGE_SIDE as f32 * 0.5 + position_xyz_m[2] * 4.0;
        for y in 0..IMAGE_SIDE {
            for x in 0..IMAGE_SIDE {
                let dx = x as f32 + 0.5 - screen_x;
                let dy = y as f32 + 0.5 - screen_y;
                if dx * dx + dy * dy <= radius * radius {
                    let offset = (y * IMAGE_SIDE + x) * 4;
                    pixels[offset..offset + 4].copy_from_slice(&color);
                }
            }
        }
    }
    pixels
}

fn assert_reason(
    evidence: &wge_native_graphics_contract::VisualQualityEvidence,
    code: QualityReasonCode,
) {
    assert!(
        evidence
            .body
            .reasons
            .iter()
            .any(|reason| reason.code == code)
    );
}

#[test]
fn structured_terrain_passes_and_evidence_revalidates() {
    let packet = packet(false);
    let pixels = structured_terrain();
    let receipt = receipt(&packet, &pixels);
    let profile = VisualQualityProfile::terrain_reference_v1(64, 64);

    let evidence = assess_visual_quality(&packet, &receipt, &pixels, &profile);
    assert_eq!(
        evidence.body.outcome,
        QualityOutcome::Good,
        "{:#?}",
        evidence.body.reasons
    );
    validate_visual_quality_evidence(&evidence, &packet, &receipt, &pixels)
        .expect("native evidence revalidates");
}

#[test]
fn certification_receipt_omits_host_dependent_timing_identity() {
    let packet = packet(false);
    let pixels = structured_terrain();
    let mut first = receipt(&packet, &pixels);
    first.body.telemetry.frame_time_us = 11_000;
    first.body.telemetry.gpu_frame_time_us = Some(10_000);
    first.body.telemetry.pass_timings.prepare_us = 2_000;
    first.body.telemetry.pass_timings.gpu_prepare_us = Some(1_000);
    let mut second = first.clone();
    second.body.telemetry.frame_time_us = 17_000;
    second.body.telemetry.gpu_frame_time_us = Some(16_000);
    second.body.telemetry.pass_timings.prepare_us = 3_000;
    second.body.telemetry.pass_timings.gpu_prepare_us = Some(2_000);

    let first = deterministic_certification_frame_receipt(&packet, &first, &pixels)
        .expect("first deterministic receipt seals");
    let second = deterministic_certification_frame_receipt(&packet, &second, &pixels)
        .expect("second deterministic receipt seals");

    assert_eq!(first, second);
    assert_eq!(first.body.telemetry.frame_time_us, 0);
    assert_eq!(first.body.telemetry.gpu_frame_time_us, None);
    assert_eq!(first.body.telemetry.pass_timings.prepare_us, 0);
    assert_eq!(first.body.telemetry.pass_timings.gpu_prepare_us, None);
    assert!(first.body.detail.contains("timing telemetry omitted"));
}

#[test]
fn overlay_heavy_flat_terrain_passes_old_gate_but_fails_quality_gate() {
    let packet = packet(true);
    let pixels = flat_with_semantic_discs(&packet);
    let receipt = receipt(&packet, &pixels);
    let measured = measure_frame_capture(&packet, &pixels).expect("old gate measurements");
    validate_native_visual_gate(&packet, &measured, &pixels)
        .expect("known-bad control passes only the old minimum-presence gate");

    let evidence = assess_visual_quality(
        &packet,
        &receipt,
        &pixels,
        &VisualQualityProfile::terrain_reference_v1(64, 64),
    );
    assert_eq!(evidence.body.outcome, QualityOutcome::Bad);
    assert!(
        evidence
            .body
            .measurements
            .as_ref()
            .expect("valid capture is measured")
            .semantic_overlay_coverage_bp
            > 2_500
    );
    assert_reason(
        &evidence,
        QualityReasonCode::ExcessiveSemanticOverlayCoverage,
    );
    assert_reason(&evidence, QualityReasonCode::InsufficientLuminanceRange);
    validate_visual_quality_evidence(&evidence, &packet, &receipt, &pixels)
        .expect("negative quality evidence is reproducible");
}

#[test]
fn declared_content_rectangle_is_intersected_with_terrain() {
    let packet = packet(false);
    let pixels = structured_terrain();
    let receipt = receipt(&packet, &pixels);
    let mut profile = VisualQualityProfile::terrain_reference_v1(64, 64);
    profile.region = QualityRegion::DeclaredContentRect {
        region_id: "west-content".into(),
        x_px: 0,
        y_px: 0,
        width_px: 32,
        height_px: 64,
    };

    let evidence = assess_visual_quality(&packet, &receipt, &pixels, &profile);
    assert_eq!(
        evidence.body.outcome,
        QualityOutcome::Good,
        "{:#?}",
        evidence.body.reasons
    );
    assert_eq!(
        evidence
            .body
            .measurements
            .as_ref()
            .expect("declared region is measured")
            .region_coverage_bp,
        5_000
    );
}

#[test]
fn tampered_capture_and_receipt_fail_closed() {
    let packet = packet(false);
    let pixels = structured_terrain();
    let receipt = receipt(&packet, &pixels);
    let profile = VisualQualityProfile::terrain_reference_v1(64, 64);

    let mut tampered_pixels = pixels.clone();
    tampered_pixels[0] ^= 0xff;
    let capture_failure = assess_visual_quality(&packet, &receipt, &tampered_pixels, &profile);
    assert_eq!(capture_failure.body.outcome, QualityOutcome::Failed);
    assert_reason(&capture_failure, QualityReasonCode::CaptureDigestMismatch);

    let mut tampered_receipt = receipt.clone();
    tampered_receipt.body.detail.push_str(" altered");
    let receipt_failure = assess_visual_quality(&packet, &tampered_receipt, &pixels, &profile);
    assert_eq!(receipt_failure.body.outcome, QualityOutcome::Failed);
    assert_reason(&receipt_failure, QualityReasonCode::InvalidReceipt);
}

#[test]
fn dimensions_and_profile_schema_are_closed_and_bound() {
    let packet = packet(false);
    let pixels = structured_terrain();
    let receipt = receipt(&packet, &pixels);
    let mut mismatched = VisualQualityProfile::terrain_reference_v1(32, 64);
    let evidence = assess_visual_quality(&packet, &receipt, &pixels, &mismatched);
    assert_eq!(evidence.body.outcome, QualityOutcome::Failed);
    assert_reason(&evidence, QualityReasonCode::CaptureBindingMismatch);

    mismatched.capture_width_px = 64;
    let mut json = serde_json::to_value(&mismatched).expect("profile serializes");
    json.as_object_mut()
        .expect("profile is an object")
        .insert("unregistered_threshold".into(), serde_json::json!(true));
    assert!(serde_json::from_value::<VisualQualityProfile>(json).is_err());
}

#[test]
fn revalidation_rejects_thresholds_not_registered_by_rust() {
    let packet = packet(false);
    let pixels = structured_terrain();
    let receipt = receipt(&packet, &pixels);
    let mut unregistered = VisualQualityProfile::terrain_reference_v1(64, 64);
    unregistered.minimum_edge_pair_fraction_bp = 0;
    let evidence = assess_visual_quality(&packet, &receipt, &pixels, &unregistered);
    assert_eq!(evidence.body.outcome, QualityOutcome::Good);
    let error = validate_visual_quality_evidence(&evidence, &packet, &receipt, &pixels)
        .expect_err("authority revalidation must reject caller-selected thresholds");
    assert!(
        error
            .message
            .contains("registered Rust certification profile")
    );
}

#[test]
fn campaign2_authored_frame_profile_is_registered_without_replacing_v1() {
    let profile = VisualQualityProfile::campaign2_authored_frame_v1(960, 640);
    validate_registered_visual_quality_profile(&profile)
        .expect("Campaign 2 profile is registered by Rust authority");
    let terrain_profile = VisualQualityProfile::terrain_reference_v1(960, 640);
    validate_registered_visual_quality_profile(&terrain_profile)
        .expect("terrain reference profile remains registered");
    let mut tampered = profile.clone();
    tampered.minimum_edge_pair_fraction_bp -= 1;
    assert!(validate_registered_visual_quality_profile(&tampered).is_err());
}
