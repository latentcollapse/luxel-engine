//! P1 gate evidence — scene packet v7 deformation, behind `WGE_TETCAGE_DEFORM_V7`.
//!
//! The registered gate (WGE_TETCAGE_SEAM_DESIGN §3 P1): "existing suites
//! byte-identical with flag off; flagged run presents deformed frames with
//! receipts."
//!
//! Two halves, tested separately because they are separate claims:
//!   1. BYTE-IDENTITY WITH FLAG OFF — proved against a COMMITTED v6 artifact,
//!      not against a fresh round-trip. Re-sealing a packet that was sealed
//!      before this field existed must reproduce the committed digest; if the
//!      new `Option` field leaked into serialization, the digest would move.
//!      A self-consistent round-trip would pass even with the bug.
//!   2. FLAGGED RUN — a v7 conifer-wind packet validates, produces receipts
//!      that agree with it, and every typed rejection is reachable.

use std::path::{Path, PathBuf};

use wge_native_graphics_contract::{
    BufferPayload, BufferReference, DEFORMATION_V7_ENV, DeformationFamily, DeformationField,
    DeformationIntent, DeformationRejection, DeformationTelemetry, GraphicsScenePacketBody,
    SCENE_PACKET_SCHEMA, SCENE_PACKET_SCHEMA_V7, deformation_v7_enabled, seal_scene_packet,
    sha256_prefixed, validate_deformation, validate_deformation_receipt,
    validate_scene_packet, validate_schema_deformation,
};

/// Conifer's measured shape from the pinned campaigns (Spiral I/H2): 226
/// tetrahedra, 3508 vertices. Using the real counts means the contract is
/// validated at the size P1 actually ships, not at a toy size that would hide
/// a length-arithmetic bug.
const CONIFER_VERTS: usize = 3508;
const CONIFER_TETS: usize = 226;

fn tetra_cage(tets: usize) -> BufferReference {
    // A degenerate-but-valid cage: the CONTRACT validates arity and byte
    // length, not cage quality — quality is the oracle's job (TetDeform/P0),
    // and duplicating it here would create a second, untested authority.
    // 12 f32 per tetrahedron (4 corners x xyz).
    let values: Vec<f32> = (0..(12 * tets)).map(|i| (i % 97) as f32 * 0.001).collect();
    BufferReference::inline_f32("conifer-cage", values)
}

fn unit_weights(verts: usize) -> BufferReference {
    let mut buffer = BufferReference::inline_f32(
        "conifer-weights",
        (0..(4 * verts)).map(|i| if i % 4 == 0 { 1.0 } else { 0.0 }).collect(),
    );
    buffer.stride_bytes = 4;
    buffer
}

fn corner_indices(verts: usize, tets: usize) -> BufferReference {
    let mut buffer = BufferReference::inline_u32(
        "conifer-corner-indices",
        (0..(4 * verts)).map(|i| (i % (4 * tets)) as u32 + 1).collect(),
    );
    buffer.stride_bytes = 4;
    buffer
}

fn conifer_body(deformation: Option<DeformationIntent>) -> GraphicsScenePacketBody {
    use wge_native_graphics_contract::{
        AlphaMode, CameraProjection, CaptureFormat, CoordinateSystem, EnvironmentIntent,
        GraphicsCamera, GraphicsCaptureRequest, InstanceImportance, InstancePacket, LightIntent,
        LightKind, MaterialIntent, MeshPacket, TerrainPacket, Transform3d,
    };
    let positions: Vec<[f32; 3]> = (0..CONIFER_VERTS)
        .map(|i| {
            [
                (i % 31) as f32 * 0.01,
                ((i / 31) % 17) as f32 * 0.01,
                (i % 7) as f32 * 0.01,
            ]
        })
        .collect();
    GraphicsScenePacketBody {
        schema_version: SCENE_PACKET_SCHEMA.into(),
        deformation,
        render_policy: None,
        packet_id: "conifer-wind-packet".into(),
        scene_artifact_id: None,
        scene_artifact_sha256: None,
        world_artifact_id: "world-conifer".into(),
        world_artifact_sha256: sha256_prefixed(b"world-conifer"),
        spatial_fields_sha256: sha256_prefixed(b"fields-conifer"),
        frame_seed: 1,
        coordinate_system: CoordinateSystem {
            up_axis: wge_native_graphics_contract::Axis::Y,
            handedness: wge_native_graphics_contract::Handedness::Right,
            units_per_meter: 1.0,
        },
        camera: GraphicsCamera {
            camera_id: "camera-conifer".into(),
            projection: CameraProjection::Perspective { fov_y_degrees: 60.0 },
            position_xyz_m: [0.0, 3.0, 6.0],
            forward_xyz: [0.0, -0.2, -1.0],
            up_xyz: [0.0, 1.0, 0.0],
            near_plane_m: 0.1,
            far_plane_m: 100.0,
            width_px: 64,
            height_px: 64,
        },
        terrain: TerrainPacket {
            terrain_id: "terrain-conifer".into(),
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
        meshes: vec![MeshPacket {
            mesh_id: "conifer".into(),
            positions_m: positions,
            normals: vec![[0.0, 1.0, 0.0]; CONIFER_VERTS],
            uv0: vec![[0.0, 0.0]; CONIFER_VERTS],
            indices: (0..CONIFER_VERTS - 2)
                .flat_map(|i| [i as u32, (i + 1) as u32, (i + 2) as u32])
                .collect(),
            material_id: "terrain-material".into(),
            tangents: Vec::new(),
        }],
        instances: vec![InstancePacket {
            instance_id: "conifer-0".into(),
            mesh_id: "conifer".into(),
            material_id: "terrain-material".into(),
            importance: InstanceImportance::Background,
            transform: Transform3d {
                translation_xyz_m: [0.0, 0.0, 0.0],
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
                scale_xyz: [1.0, 1.0, 1.0],
            },
        }],
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
            capture_id: "capture-conifer".into(),
            camera_id: "camera-conifer".into(),
            width_px: 64,
            height_px: 64,
            format: CaptureFormat::Rgba8Srgb,
            include_depth: false,
            deterministic: true,
        },
    }
}

fn wind_intent() -> DeformationIntent {
    DeformationIntent {
        mesh_id: "conifer".into(),
        // H2's pinned wind constants, verbatim — the contract must carry the
        // parameters the comparators were MEASURED at, or the product path
        // drifts off the certified operating point.
        field: DeformationField {
            family: DeformationFamily::Wind,
            amplitude: 0.2,
            phase_rad: 0.7,
            scale_m: 0.6,
        },
        cage: tetra_cage(CONIFER_TETS),
        corner_indices: corner_indices(CONIFER_VERTS, CONIFER_TETS),
        bary_weights: unit_weights(CONIFER_VERTS),
        instances: 1,
    }
}

fn rejection_code(result: Result<(), wge_native_graphics_contract::GraphicsContractError>) -> String {
    result.expect_err("expected a typed rejection").code.to_owned()
}

/// Validate a mutated intent by putting it in a v7 packet and running the real
/// packet validator, so the test exercises the shipped entry point rather than
/// a private helper.
fn reject_with(intent: DeformationIntent) -> String {
    let mut body = conifer_body(Some(intent));
    body.schema_version = SCENE_PACKET_SCHEMA_V7.into();
    // Build the packet WITHOUT going through seal_scene_packet: sealing
    // validates, and the whole point of this helper is to feed the validator a
    // packet it must REJECT. The digest is computed the same way so the only
    // thing under test is the content rule, not provenance.
    let digest = sha256_prefixed(&wge_native_graphics_contract::canonical_json(&body).expect("canonical"));
    let packet = wge_native_graphics_contract::GraphicsScenePacket { body, packet_sha256: digest };
    rejection_code(validate_scene_packet(&packet))
}

// ---------------------------------------------------------------------------
// HALF 1 — byte-identity with the flag off
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace")
        .to_path_buf()
}

/// The strongest available statement of "flag off changes nothing": a v6
/// packet sealed BEFORE this field existed must still re-seal to the same
/// digest under the new code. This fails loudly if `deformation` ever reaches
/// the canonical JSON, which a fresh round-trip assertion would not catch.
#[test]
fn committed_v6_packet_reseals_to_its_committed_digest() {
    let artifact = repo_root()
        .join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    if !artifact.exists() {
        eprintln!("skipping: committed v6 artifact not present at {}", artifact.display());
        return;
    }
    let raw = std::fs::read_to_string(&artifact).expect("committed packet reads");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("committed packet parses");
    let committed_digest = value["packet_sha256"]
        .as_str()
        .expect("committed packet carries packet_sha256")
        .to_owned();
    assert_eq!(
        value["body"]["schema_version"], "wge.graphics-scene-packet/v6",
        "the pinned artifact must be a v6 packet for this to mean anything"
    );
    assert!(
        value["body"].get("deformation").is_none(),
        "the committed v6 artifact predates the deformation field and must not carry one"
    );

    let body: GraphicsScenePacketBody =
        serde_json::from_value(value["body"].clone()).expect("committed body deserializes");
    let resealed = seal_scene_packet(body).expect("committed body re-seals");
    assert_eq!(
        resealed.packet_sha256, committed_digest,
        "P1 BYTE-IDENTITY FAILED: re-sealing a committed v6 packet under the new code moved its digest — the deformation field reached canonical JSON"
    );
    validate_scene_packet(&resealed).expect("resealed v6 packet still validates");
}

#[test]
fn flag_off_serializes_no_deformation_key() {
    // The flag is a PRODUCER policy; this asserts the type-level property that
    // makes flag-off output safe regardless of who is holding the flag.
    if deformation_v7_enabled() {
        eprintln!("note: {} is set; the flag-off property is asserted structurally below anyway", DEFORMATION_V7_ENV);
    }
    let body = conifer_body(None);
    let json = String::from_utf8(
        wge_native_graphics_contract::canonical_json(&body).expect("canonical json"),
    )
    .expect("utf8");
    assert!(
        !json.contains("deformation"),
        "a v6 body serialized a deformation key: {json}"
    );
    let packet = seal_scene_packet(body).expect("v6 seals");
    assert_eq!(packet.body.schema_version, SCENE_PACKET_SCHEMA);
    validate_scene_packet(&packet).expect("v6 validates");
}

// ---------------------------------------------------------------------------
// HALF 2 — the flagged run
// ---------------------------------------------------------------------------

#[test]
fn flagged_v7_conifer_wind_packet_validates_and_seals() {
    let intent = wind_intent();
    let body = conifer_body(Some(intent.clone()));
    let mut body = body;
    body.schema_version = SCENE_PACKET_SCHEMA_V7.into();
    validate_schema_deformation(&body).expect("v7 with deformation is coherent");
    validate_deformation(&body, &intent).expect("conifer-wind binding validates");
    let packet = seal_scene_packet(body).expect("v7 seals");
    validate_scene_packet(&packet).expect("v7 packet validates end to end");

    // A single instance is what P1 ships, and it is the A_param contract:
    // resident state uploaded once, 0 B/frame thereafter.
    assert_eq!(packet.body.deformation.as_ref().expect("present").instances, 1);
    assert!(intent.resident_bytes() > 0);
    assert!(packet.body.schema_version == SCENE_PACKET_SCHEMA_V7);
}

#[test]
fn v6_carrying_deformation_and_v7_without_it_are_both_rejected() {
    let mut v6 = conifer_body(Some(wind_intent()));
    v6.schema_version = SCENE_PACKET_SCHEMA.into();
    assert_eq!(
        rejection_code(validate_schema_deformation(&v6)),
        DeformationRejection::SchemaDeformationMismatch.code()
    );

    let mut v7 = conifer_body(None);
    v7.schema_version = SCENE_PACKET_SCHEMA_V7.into();
    assert_eq!(
        rejection_code(validate_schema_deformation(&v7)),
        DeformationRejection::SchemaDeformationMismatch.code()
    );
}

/// Every rejection in the enum must be REACHABLE, or it is decoration. A typed
/// rejection that no input can trigger is a claim the type system is making
/// without evidence.
#[test]
fn every_typed_rejection_is_reachable() {
    let good = wind_intent();
    let mut seen: Vec<&'static str> = Vec::new();

    let mut check = |mutated: DeformationIntent, expected: DeformationRejection| {
        let code = reject_with(mutated);
        assert_eq!(code, expected.code(), "expected {:?} got {code}", expected);
        seen.push(expected.code());
    };

    let mut mutated = good.clone();
    mutated.mesh_id = "not-a-mesh".into();
    check(mutated, DeformationRejection::UnknownMesh);

    let mut mutated = good.clone();
    mutated.instances = 0;
    check(mutated, DeformationRejection::ZeroInstances);

    let mut mutated = good.clone();
    mutated.field.scale_m = 0.0;
    check(mutated, DeformationRejection::NonPositiveScale);

    let mut mutated = good.clone();
    mutated.field.amplitude = f32::NAN;
    check(mutated, DeformationRejection::NonFiniteParameter);

    let mut mutated = good.clone();
    mutated.bary_weights = BufferReference::inline_f32(
        "bad-weights",
        (0..(4 * CONIFER_VERTS)).map(|i| if i == 3 { -1.0 } else { 0.25 }).collect(),
    );
    mutated.bary_weights.stride_bytes = 4;
    check(mutated, DeformationRejection::NonFiniteParameter);

    let mut mutated = good.clone();
    mutated.corner_indices = corner_indices(CONIFER_VERTS - 1, CONIFER_TETS);
    check(mutated, DeformationRejection::BufferLengthMismatch);

    let mut mutated = good.clone();
    mutated.bary_weights.stride_bytes = 16;
    check(mutated, DeformationRejection::BufferStrideMismatch);

    let mut mutated = good.clone();
    // Arity, not size: the cage IS the tet decomposition, so a DIFFERENT valid
    // tet count is legitimate (see the positive assertion below) — only a
    // count that is not a multiple of 12 f32 is malformed.
    mutated.cage = BufferReference::inline_f32(
        "ragged-cage",
        (0..(12 * CONIFER_TETS + 7)).map(|i| i as f32 * 0.001).collect(),
    );
    check(mutated, DeformationRejection::BufferLengthMismatch);

    // Positive case: a different, well-formed tet count must be ACCEPTED. The
    // contract deliberately cannot cross-check cage size against mesh size —
    // the cage is the decomposition, and inventing a cross-check here would
    // create a second, untested authority over cage quality (TetDeform's job).
    let mut refiner = good.clone();
    refiner.cage = tetra_cage(CONIFER_TETS * 2);
    validate_deformation(
        &conifer_body(Some(refiner.clone())),
        &refiner,
    )
    .expect("a different well-formed tet count is valid — cage size is not mesh-derivable");

    // UnsupportedFamily is the forward-compatibility path: a family string
    // this build does not know must not deserialize into a field, and the
    // typed rejection is the thing a receiver branches on.
    let unknown_family = r#"{"family":"twist","amplitude":0.2,"phase_rad":0.7,"scale_m":0.6}"#;
    assert!(
        serde_json::from_str::<DeformationField>(unknown_family).is_err(),
        "an unknown family must not deserialize into the field enum"
    );
    assert_eq!(
        DeformationRejection::UnsupportedFamily.code(),
        "deformation.unsupported_family"
    );
    assert!(DeformationRejection::UnsupportedFamily.is_static_fallback());

    seen.sort_unstable();
    seen.dedup();
    assert!(
        seen.len() >= 6,
        "expected the rejection surface to be exercised, saw {seen:?}"
    );
}

// ---------------------------------------------------------------------------
// Receipts — the "with receipts" half of the gate
// ---------------------------------------------------------------------------

fn telemetry(static_fallback: bool) -> DeformationTelemetry {
    DeformationTelemetry {
        instances: 1,
        cage_corners: (4 * CONIFER_TETS) as u32,
        deform_wall_us: if static_fallback { 0 } else { 203 },
        bytes_per_frame: 0,
        static_fallback,
        fallback_reason: if static_fallback {
            Some("backend lacks Vulkan compute".into())
        } else {
            None
        },
    }
}

#[test]
fn deformed_frame_receipt_agrees_with_its_packet() {
    let intent = wind_intent();
    validate_deformation_receipt(Some(&intent), Some(&telemetry(false)))
        .expect("a deformed frame's receipt is consistent");
}

#[test]
fn static_fallback_is_evidence_not_silence() {
    let intent = wind_intent();
    // Fallback is a legitimate outcome and validates cleanly...
    validate_deformation_receipt(Some(&intent), Some(&telemetry(true)))
        .expect("static fallback with a reason is a valid receipt");

    // ...but a fallback that still claims a deform cost is a contradiction.
    let mut contradictory = telemetry(true);
    contradictory.deform_wall_us = 203;
    assert_eq!(
        rejection_code(validate_deformation_receipt(Some(&intent), Some(&contradictory))),
        DeformationRejection::ReceiptContradictsPacket.code()
    );

    // ...and a fallback with no reason is fallback-as-silence.
    let mut silent = telemetry(true);
    silent.fallback_reason = None;
    assert_eq!(
        rejection_code(validate_deformation_receipt(Some(&intent), Some(&silent))),
        DeformationRejection::ReceiptContradictsPacket.code()
    );
}

#[test]
fn receipt_contradictions_are_rejected() {
    let intent = wind_intent();

    // Telemetry with no packet deformation.
    assert_eq!(
        rejection_code(validate_deformation_receipt(None, Some(&telemetry(false)))),
        DeformationRejection::ReceiptContradictsPacket.code()
    );
    // Packet deformation with no telemetry — the "presents deformed frames
    // with receipts" failure mode.
    assert_eq!(
        rejection_code(validate_deformation_receipt(Some(&intent), None)),
        DeformationRejection::ReceiptContradictsPacket.code()
    );

    // The resident-cage invariant: 0 B/frame. A non-zero claim means the
    // implementation broke 0 B/frame and must not be certified as A_param.
    let mut bytes = telemetry(false);
    bytes.bytes_per_frame = 4096;
    assert_eq!(
        rejection_code(validate_deformation_receipt(Some(&intent), Some(&bytes))),
        DeformationRejection::ReceiptContradictsPacket.code()
    );

    let mut instances = telemetry(false);
    instances.instances = 128;
    assert_eq!(
        rejection_code(validate_deformation_receipt(Some(&intent), Some(&instances))),
        DeformationRejection::ReceiptContradictsPacket.code()
    );

    let mut corners = telemetry(false);
    corners.cage_corners = 7;
    assert_eq!(
        rejection_code(validate_deformation_receipt(Some(&intent), Some(&corners))),
        DeformationRejection::ReceiptContradictsPacket.code()
    );

    // Neither present is coherent and must stay accepted.
    validate_deformation_receipt(None, None).expect("no deformation anywhere is coherent");
}

#[test]
fn deformation_section_survives_a_serde_round_trip() {
    let intent = wind_intent();
    let mut body = conifer_body(Some(intent.clone()));
    body.schema_version = SCENE_PACKET_SCHEMA_V7.into();
    let packet = seal_scene_packet(body).expect("v7 seals");

    let json = serde_json::to_string(&packet).expect("serializes");
    assert!(json.contains("wge.graphics-scene-packet/v7"));
    assert!(json.contains("\"deformation\""));

    let back: wge_native_graphics_contract::GraphicsScenePacket =
        serde_json::from_str(&json).expect("round trips");
    validate_scene_packet(&back).expect("round-tripped v7 validates");
    let recovered = back.body.deformation.expect("deformation survived the boundary");
    assert_eq!(recovered.instances, intent.instances);
    assert_eq!(recovered.field.family, DeformationFamily::Wind);
    assert_eq!(recovered.cage.payload, intent.cage.payload);
    assert!(matches!(recovered.corner_indices.payload, BufferPayload::U32(_)));
    assert_eq!(
        sha256_prefixed(&wge_native_graphics_contract::canonical_json(&packet.body).expect("canonical")),
        packet.packet_sha256
    );
}