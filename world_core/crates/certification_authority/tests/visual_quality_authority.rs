use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;
use serde_json::json;
use wge_certification_authority::schema::NativeVisualQualityReceiptPayload;
use wge_certification_authority::{
    ArtifactBytes, CandidateContext, EvidenceBinding, NATIVE_GRAPHICS_FRAME_RECEIPT_KIND,
    NATIVE_GRAPHICS_PACKET_KIND, NATIVE_GRAPHICS_RENDERER_ATTESTATION_KIND,
    NATIVE_RGBA8_CAPTURE_KIND, NATIVE_VISUAL_QUALITY_EVIDENCE_KIND,
    NATIVE_VISUAL_QUALITY_RECEIPT_SCHEMA, ReceiptEnvelope, ReceiptStatus, ValidatorRegistry,
    candidate_identity, engine_neutral_gate_profile, native_mvp_gate_profile, sha256_prefixed,
    validate_receipt,
};
use wge_native_graphics_contract as graphics;
use wge_reference_runtime::{WorldBuild, build_from_layout_path};

const WORLD_ID: &str = "native-quality-world";
const PACKET_ID: &str = "native-quality-packet";
const FRAME_RECEIPT_ID: &str = "native-quality-frame-receipt";
const ATTESTATION_ID: &str = "native-quality-renderer-attestation";
const CAPTURE_ID: &str = "native-quality-capture";
const QUALITY_ID: &str = "native-quality-evidence";

static WORLD_BUILD: OnceLock<WorldBuild> = OnceLock::new();

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("certification authority is nested under the WGE project")
        .to_path_buf()
}

fn world_build() -> &'static WorldBuild {
    WORLD_BUILD.get_or_init(|| {
        let root = project_root();
        let layout =
            root.join("world_core/crates/reference_runtime/examples/riverwatch.layout.json");
        let julia = std::env::var_os("WGE_JULIA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("julia"));
        build_from_layout_path(&layout, &julia, &root.join("terrain_lab"))
            .expect("checked-in world fixture builds through the Rust/Julia runtime path")
    })
}

fn json_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).expect("fixture serializes")
}

fn put(candidate: &mut CandidateContext, id: &str, kind: &str, bytes: Vec<u8>) {
    candidate.artifacts.insert(
        id.to_owned(),
        ArtifactBytes {
            kind: kind.to_owned(),
            bytes,
        },
    );
}

fn structured_capture(width: usize, height: usize) -> Vec<u8> {
    let palette = [
        [34u8, 63u8, 39u8],
        [77, 89, 45],
        [121, 112, 67],
        [47, 92, 105],
        [91, 58, 48],
        [106, 119, 81],
        [55, 70, 105],
        [125, 87, 59],
    ];
    let mut pixels = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let tile = ((x / 10) + 3 * (y / 10)) % palette.len();
            let ridge = (x % 5 < 2) ^ (y % 5 < 2);
            let mut rgb = palette[tile];
            for channel in &mut rgb {
                *channel = if ridge {
                    channel.saturating_add(16)
                } else {
                    channel.saturating_sub(9)
                };
            }
            pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    pixels
}

fn graphics_packet() -> graphics::GraphicsScenePacket {
    let lowered = graphics::lower_reference_world(&world_build().world)
        .expect("world lowers to a native graphics packet");
    let mut body = lowered.body;
    // This authority fixture isolates terrain quality; overlay coverage is
    // tested by the native graphics contract's dedicated negative controls.
    body.overlays.clear();
    graphics::seal_scene_packet(body).expect("fixture packet reseals after overlay removal")
}

fn frame_receipt(
    packet: &graphics::GraphicsScenePacket,
    capture: &[u8],
    status: graphics::FrameStatus,
) -> graphics::GraphicsFrameReceipt {
    let measurements =
        graphics::measure_frame_capture(packet, capture).expect("fixture capture is measurable");
    let body = graphics::GraphicsFrameReceiptBody {
        schema_version: graphics::FRAME_RECEIPT_SCHEMA.into(),
        packet_sha256: packet.packet_sha256.clone(),
        capture_id: packet.body.capture.capture_id.clone(),
        backend_id: graphics::LAVA_BACKEND_ID.into(),
        adapter_revision: graphics::ADAPTER_REVISION.into(),
        lava_revision: graphics::LAVA_REVISION.into(),
        device_uuid: "authority-quality-test-device".into(),
        worker_script_sha256: graphics::sha256_prefixed(b"test worker"),
        renderer_identity_sha256: graphics::sha256_prefixed(b"test renderer"),
        status,
        format: packet.body.capture.format,
        width_px: packet.body.capture.width_px,
        height_px: packet.body.capture.height_px,
        capture_sha256: (status == graphics::FrameStatus::Passed)
            .then(|| graphics::sha256_prefixed(capture)),
        measurements,
        telemetry: graphics::GraphicsTelemetry {
            upload_bytes: 0,
            readback_bytes: capture.len(),
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
            terrain_vertex_count: packet.body.terrain.resolution.pow(2),
            mesh_vertex_count: 0,
            frame_time_us: 1,
            gpu_frame_time_us: None,
            pass_timings: graphics::GraphicsPassTimings {
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
        detail: "synthetic authority boundary fixture".into(),
    };
    graphics::GraphicsFrameReceipt {
        receipt_sha256: graphics::sha256_prefixed(
            &graphics::canonical_json(&body).expect("frame receipt body serializes"),
        ),
        body,
    }
}

fn renderer_attestation(
    frame: &graphics::GraphicsFrameReceipt,
) -> graphics::GraphicsRendererAttestation {
    let worker_script_sha256 = frame.body.worker_script_sha256.clone();
    let ready = graphics::GraphicsReady {
        schema_version: graphics::READY_SCHEMA.into(),
        backend_id: graphics::LAVA_BACKEND_ID.into(),
        adapter_revision: graphics::ADAPTER_REVISION.into(),
        lava_revision: graphics::LAVA_REVISION.into(),
        julia_version: "1.12.6-test".into(),
        vulkan_api_version: "1.3-test".into(),
        device_name: "authority-test-device".into(),
        device_uuid: frame.body.device_uuid.clone(),
        features: graphics::GraphicsFeatures {
            offscreen_raster: true,
            depth_attachment: true,
            texture_sampling: true,
            readback: true,
            hardware_ray_tracing: true,
            gpu_timestamps: true,
        },
    };
    let worker_ready_message = json!({
        "schema": graphics::WORKER_SCHEMA,
        "kind": "ready",
        "script_sha256": worker_script_sha256,
        "lava_revision": graphics::LAVA_REVISION,
        "adapter_revision": graphics::ADAPTER_REVISION,
    });
    let source_digests = BTreeMap::from([
        (
            "graphics_contract".into(),
            graphics::sha256_prefixed(b"graphics contract"),
        ),
        (
            "lava_adapter".into(),
            graphics::sha256_prefixed(b"lava adapter"),
        ),
        ("manifest".into(), graphics::sha256_prefixed(b"manifest")),
        ("project".into(), graphics::sha256_prefixed(b"project")),
        (
            "worker_script".into(),
            frame.body.worker_script_sha256.clone(),
        ),
    ]);
    let source_identity_sha256 = graphics::sha256_prefixed(
        &graphics::canonical_json(&source_digests).expect("source digest manifest serializes"),
    );
    let renderer_identity_sha256 = graphics::sha256_prefixed(
        &graphics::canonical_json(&json!({
            "capabilities": &ready,
            "worker_ready": &worker_ready_message,
            "sources": &source_digests,
        }))
        .expect("renderer identity serializes"),
    );
    graphics::seal_renderer_attestation(graphics::GraphicsRendererAttestationBody {
        schema_version: graphics::RENDERER_ATTESTATION_SCHEMA.into(),
        backend_id: graphics::LAVA_BACKEND_ID.into(),
        adapter_revision: graphics::ADAPTER_REVISION.into(),
        lava_revision: graphics::LAVA_REVISION.into(),
        worker_ready_message,
        ready,
        source_digests,
        source_identity_sha256,
        renderer_identity_sha256,
    })
    .expect("synthetic renderer attestation validates")
}

fn candidate_with_quality_evidence() -> CandidateContext {
    let world = &world_build().world;
    let packet = graphics_packet();
    let capture = structured_capture(
        packet.body.capture.width_px as usize,
        packet.body.capture.height_px as usize,
    );
    let mut frame = frame_receipt(&packet, &capture, graphics::FrameStatus::Passed);
    let provisional_attestation = renderer_attestation(&frame);
    frame.body.renderer_identity_sha256 = provisional_attestation.body.renderer_identity_sha256;
    frame.receipt_sha256 = graphics::sha256_prefixed(
        &graphics::canonical_json(&frame.body).expect("frame receipt body serializes"),
    );
    let attestation = renderer_attestation(&frame);
    let evidence = graphics::assess_visual_quality(
        &packet,
        &frame,
        &capture,
        &graphics::VisualQualityProfile::terrain_reference_v1(
            packet.body.capture.width_px,
            packet.body.capture.height_px,
        ),
    );
    assert_eq!(
        evidence.body.outcome,
        graphics::QualityOutcome::Good,
        "test control must meet strict visual-quality thresholds: {:?}",
        evidence.body.reasons
    );
    graphics::validate_visual_quality_evidence(&evidence, &packet, &frame, &capture)
        .expect("fixture evidence independently revalidates");

    let mut candidate = CandidateContext {
        project_id: "wge-native-visual-quality-authority-tests".into(),
        snapshot_id: "quality-candidate".into(),
        candidate_sha256: String::new(),
        artifacts: BTreeMap::new(),
        authorized_repair_artifact_ids: BTreeSet::new(),
    };
    put(
        &mut candidate,
        WORLD_ID,
        "world_artifact",
        json_bytes(world),
    );
    put(
        &mut candidate,
        PACKET_ID,
        NATIVE_GRAPHICS_PACKET_KIND,
        json_bytes(&packet),
    );
    put(
        &mut candidate,
        FRAME_RECEIPT_ID,
        NATIVE_GRAPHICS_FRAME_RECEIPT_KIND,
        json_bytes(&frame),
    );
    put(
        &mut candidate,
        ATTESTATION_ID,
        NATIVE_GRAPHICS_RENDERER_ATTESTATION_KIND,
        json_bytes(&attestation),
    );
    put(
        &mut candidate,
        CAPTURE_ID,
        NATIVE_RGBA8_CAPTURE_KIND,
        capture,
    );
    put(
        &mut candidate,
        QUALITY_ID,
        NATIVE_VISUAL_QUALITY_EVIDENCE_KIND,
        json_bytes(&evidence),
    );
    candidate.candidate_sha256 = candidate_identity(&candidate).unwrap();
    candidate
}

fn evidence_bindings(candidate: &CandidateContext, ids: &[&str]) -> Vec<EvidenceBinding> {
    let mut bindings = ids
        .iter()
        .map(|id| {
            let artifact = &candidate.artifacts[*id];
            EvidenceBinding {
                artifact_id: (*id).to_owned(),
                kind: artifact.kind.clone(),
                sha256: sha256_prefixed(&artifact.bytes),
            }
        })
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    bindings
}

fn receipt(candidate: &CandidateContext, status: ReceiptStatus) -> ReceiptEnvelope {
    let mut envelope = ReceiptEnvelope {
        schema_version: wge_certification_authority::ENVELOPE_SCHEMA.into(),
        receipt_id: String::new(),
        project_id: candidate.project_id.clone(),
        snapshot_id: candidate.snapshot_id.clone(),
        candidate_sha256: candidate.candidate_sha256.clone(),
        gate_id: "visual_quality".into(),
        validator_id: "wge.validator.visual-quality/v1".into(),
        receipt_schema: NATIVE_VISUAL_QUALITY_RECEIPT_SCHEMA.into(),
        status,
        producer: "authority-test-fixture".into(),
        observed_input_sha256: String::new(),
        evidence: evidence_bindings(
            candidate,
            &[
                WORLD_ID,
                PACKET_ID,
                FRAME_RECEIPT_ID,
                ATTESTATION_ID,
                CAPTURE_ID,
                QUALITY_ID,
            ],
        ),
        payload: serde_json::to_value(NativeVisualQualityReceiptPayload {
            world_artifact_id: WORLD_ID.into(),
            packet_artifact_id: PACKET_ID.into(),
            frame_receipt_artifact_id: FRAME_RECEIPT_ID.into(),
            renderer_attestation_artifact_id: ATTESTATION_ID.into(),
            capture_artifact_id: CAPTURE_ID.into(),
            visual_quality_evidence_artifact_id: QUALITY_ID.into(),
        })
        .unwrap(),
    };
    envelope.seal().expect("receipt has deterministic identity");
    envelope
}

fn reidentify(candidate: &mut CandidateContext, envelope: &mut ReceiptEnvelope) {
    candidate.candidate_sha256 = candidate_identity(candidate).unwrap();
    envelope.candidate_sha256 = candidate.candidate_sha256.clone();
    envelope.seal().unwrap();
}

#[test]
fn strict_visual_quality_is_registered_required_and_independently_remeasured() {
    let registry = ValidatorRegistry::wge_engine_neutral_v1();
    let descriptor = registry
        .descriptor("wge.validator.visual-quality/v1")
        .expect("strict native validator is registered");
    assert_eq!(descriptor.gate_id, "visual_quality");
    assert_eq!(
        descriptor.receipt_schema,
        NATIVE_VISUAL_QUALITY_RECEIPT_SCHEMA
    );
    assert!(engine_neutral_gate_profile().iter().any(|gate| {
        gate.gate_id == "visual_quality"
            && gate.validator_id == descriptor.validator_id
            && gate.receipt_schema == descriptor.receipt_schema
            && gate.disposition == wge_certification_authority::GateDisposition::RequiredPass
    }));
    assert!(native_mvp_gate_profile().iter().any(|gate| {
        gate.gate_id == "visual_quality"
            && gate.validator_id == descriptor.validator_id
            && gate.disposition == wge_certification_authority::GateDisposition::RequiredPass
    }));
    assert!(
        ValidatorRegistry::wge_native_mvp_v1()
            .descriptor("wge.validator.visual-quality/v1")
            .is_some()
    );

    let candidate = candidate_with_quality_evidence();
    let valid = receipt(&candidate, ReceiptStatus::Pass);
    let decision = validate_receipt(&valid, &candidate, &registry).unwrap();
    assert_eq!(decision.status, ReceiptStatus::Pass);
    assert!(decision.detail.contains("independently remeasured Good"));
}

#[test]
fn packet_receipt_capture_and_quality_tampering_are_rejected_after_outer_resealing() {
    let registry = ValidatorRegistry::wge_engine_neutral_v1();
    let candidate = candidate_with_quality_evidence();
    let valid = receipt(&candidate, ReceiptStatus::Pass);
    assert_eq!(
        validate_receipt(&valid, &candidate, &registry)
            .unwrap()
            .status,
        ReceiptStatus::Pass
    );

    let mut packet_tamper = candidate.clone();
    let mut packet: serde_json::Value =
        serde_json::from_slice(&packet_tamper.artifacts[PACKET_ID].bytes).unwrap();
    packet["packet_sha256"] = serde_json::json!(sha256_prefixed(b"forged packet"));
    packet_tamper.artifacts.get_mut(PACKET_ID).unwrap().bytes = json_bytes(&packet);
    let mut envelope = valid.clone();
    reidentify(&mut packet_tamper, &mut envelope);
    assert!(validate_receipt(&envelope, &packet_tamper, &registry).is_err());

    let mut capture_tamper = candidate.clone();
    capture_tamper.artifacts.get_mut(CAPTURE_ID).unwrap().bytes[0] ^= 1;
    let mut envelope = valid.clone();
    reidentify(&mut capture_tamper, &mut envelope);
    let error = validate_receipt(&envelope, &capture_tamper, &registry).unwrap_err();
    assert!(error.detail.contains("capture") || error.detail.contains("evidence"));

    let mut frame_tamper = candidate.clone();
    let mut frame: graphics::GraphicsFrameReceipt =
        serde_json::from_slice(&frame_tamper.artifacts[FRAME_RECEIPT_ID].bytes).unwrap();
    frame.body.detail.push_str(" altered");
    frame.receipt_sha256 =
        graphics::sha256_prefixed(&graphics::canonical_json(&frame.body).unwrap());
    frame_tamper
        .artifacts
        .get_mut(FRAME_RECEIPT_ID)
        .unwrap()
        .bytes = json_bytes(&frame);
    let mut envelope = valid.clone();
    reidentify(&mut frame_tamper, &mut envelope);
    assert!(validate_receipt(&envelope, &frame_tamper, &registry).is_err());

    let mut quality_tamper = candidate.clone();
    let mut quality: graphics::VisualQualityEvidence =
        serde_json::from_slice(&quality_tamper.artifacts[QUALITY_ID].bytes).unwrap();
    quality.body.outcome = graphics::QualityOutcome::Bad;
    quality.evidence_sha256 =
        graphics::sha256_prefixed(&graphics::canonical_json(&quality.body).unwrap());
    quality_tamper.artifacts.get_mut(QUALITY_ID).unwrap().bytes = json_bytes(&quality);
    let mut envelope = valid;
    reidentify(&mut quality_tamper, &mut envelope);
    assert!(validate_receipt(&envelope, &quality_tamper, &registry).is_err());
}

#[test]
fn status_only_extra_artifact_and_pass_shaped_quality_claims_are_rejected() {
    let registry = ValidatorRegistry::wge_engine_neutral_v1();
    let candidate = candidate_with_quality_evidence();
    let valid = receipt(&candidate, ReceiptStatus::Pass);

    let mut status_only = valid.clone();
    status_only.payload = serde_json::json!({"status": "pass", "producer": "trusted"});
    status_only.seal().unwrap();
    assert!(validate_receipt(&status_only, &candidate, &registry).is_err());

    let mut extra = valid;
    let mut extra_candidate = candidate.clone();
    put(
        &mut extra_candidate,
        "unclaimed-native-debug",
        "debug_claim",
        b"claim".to_vec(),
    );
    extra_candidate.candidate_sha256 = candidate_identity(&extra_candidate).unwrap();
    extra.snapshot_id = extra_candidate.snapshot_id.clone();
    extra.candidate_sha256 = extra_candidate.candidate_sha256.clone();
    extra.evidence.push(EvidenceBinding {
        artifact_id: "unclaimed-native-debug".into(),
        kind: "debug_claim".into(),
        sha256: sha256_prefixed(b"claim"),
    });
    extra
        .evidence
        .sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    extra.seal().unwrap();
    assert!(validate_receipt(&extra, &extra_candidate, &registry).is_err());
}

#[test]
fn producer_claimed_status_must_match_rust_derived_quality_outcome() {
    let registry = ValidatorRegistry::wge_engine_neutral_v1();
    let candidate = candidate_with_quality_evidence();
    let forged = receipt(&candidate, ReceiptStatus::Fail);
    let error = validate_receipt(&forged, &candidate, &registry).unwrap_err();
    assert!(error.detail.contains("claims Fail") && error.detail.contains("derives Pass"));
}

#[test]
fn bad_failed_and_indeterminate_quality_outcomes_have_explicit_authority_statuses() {
    let registry = ValidatorRegistry::wge_engine_neutral_v1();
    let base = candidate_with_quality_evidence();
    let packet: graphics::GraphicsScenePacket =
        serde_json::from_slice(&base.artifacts[PACKET_ID].bytes).unwrap();
    let frame: graphics::GraphicsFrameReceipt =
        serde_json::from_slice(&base.artifacts[FRAME_RECEIPT_ID].bytes).unwrap();
    let capture = base.artifacts[CAPTURE_ID].bytes.clone();

    let bad_capture = (0..capture.len() / 4)
        .flat_map(|index| match index % 3 {
            0 => [48u8, 48, 48, 255],
            1 => [54, 54, 54, 255],
            _ => [60, 60, 60, 255],
        })
        .collect::<Vec<_>>();
    let mut bad_frame = frame_receipt(&packet, &bad_capture, graphics::FrameStatus::Passed);
    let provisional_bad_attestation = renderer_attestation(&bad_frame);
    bad_frame.body.renderer_identity_sha256 =
        provisional_bad_attestation.body.renderer_identity_sha256;
    bad_frame.receipt_sha256 = graphics::sha256_prefixed(
        &graphics::canonical_json(&bad_frame.body).expect("bad frame receipt serializes"),
    );
    let bad_attestation = renderer_attestation(&bad_frame);
    let bad_profile = graphics::VisualQualityProfile::terrain_reference_v1(
        packet.body.capture.width_px,
        packet.body.capture.height_px,
    );
    let bad = graphics::assess_visual_quality(&packet, &bad_frame, &bad_capture, &bad_profile);
    assert_eq!(bad.body.outcome, graphics::QualityOutcome::Bad);
    let mut bad_candidate = base.clone();
    bad_candidate.artifacts.get_mut(CAPTURE_ID).unwrap().bytes = bad_capture;
    bad_candidate
        .artifacts
        .get_mut(FRAME_RECEIPT_ID)
        .unwrap()
        .bytes = json_bytes(&bad_frame);
    bad_candidate
        .artifacts
        .get_mut(ATTESTATION_ID)
        .unwrap()
        .bytes = json_bytes(&bad_attestation);
    bad_candidate.artifacts.get_mut(QUALITY_ID).unwrap().bytes = json_bytes(&bad);
    bad_candidate.candidate_sha256 = candidate_identity(&bad_candidate).unwrap();
    let decision = validate_receipt(
        &receipt(&bad_candidate, ReceiptStatus::Fail),
        &bad_candidate,
        &registry,
    )
    .unwrap();
    assert_eq!(decision.status, ReceiptStatus::Fail);
    assert!(decision.detail.contains("remeasured Bad"));

    let mut failed_capture = capture.clone();
    failed_capture[0] ^= 0xff;
    let failed_profile = graphics::VisualQualityProfile::terrain_reference_v1(
        packet.body.capture.width_px,
        packet.body.capture.height_px,
    );
    let failed = graphics::assess_visual_quality(&packet, &frame, &failed_capture, &failed_profile);
    assert_eq!(failed.body.outcome, graphics::QualityOutcome::Failed);
    let mut failed_candidate = base.clone();
    failed_candidate
        .artifacts
        .get_mut(QUALITY_ID)
        .unwrap()
        .bytes = json_bytes(&failed);
    failed_candidate
        .artifacts
        .get_mut(CAPTURE_ID)
        .unwrap()
        .bytes = failed_capture;
    failed_candidate.candidate_sha256 = candidate_identity(&failed_candidate).unwrap();
    let decision = validate_receipt(
        &receipt(&failed_candidate, ReceiptStatus::Fail),
        &failed_candidate,
        &registry,
    )
    .unwrap();
    assert_eq!(decision.status, ReceiptStatus::Fail);
    assert!(decision.detail.contains("deterministically Failed"));

    let mut unsupported = frame_receipt(&packet, &capture, graphics::FrameStatus::Unsupported);
    let provisional_unsupported_attestation = renderer_attestation(&unsupported);
    unsupported.body.renderer_identity_sha256 = provisional_unsupported_attestation
        .body
        .renderer_identity_sha256;
    unsupported.receipt_sha256 = graphics::sha256_prefixed(
        &graphics::canonical_json(&unsupported.body).expect("unsupported frame receipt serializes"),
    );
    let unsupported_attestation = renderer_attestation(&unsupported);
    let indeterminate = graphics::assess_visual_quality(
        &packet,
        &unsupported,
        &capture,
        &graphics::VisualQualityProfile::terrain_reference_v1(
            packet.body.capture.width_px,
            packet.body.capture.height_px,
        ),
    );
    assert_eq!(
        indeterminate.body.outcome,
        graphics::QualityOutcome::Indeterminate
    );
    let mut indeterminate_candidate = base;
    indeterminate_candidate
        .artifacts
        .get_mut(FRAME_RECEIPT_ID)
        .unwrap()
        .bytes = json_bytes(&unsupported);
    indeterminate_candidate
        .artifacts
        .get_mut(ATTESTATION_ID)
        .unwrap()
        .bytes = json_bytes(&unsupported_attestation);
    indeterminate_candidate
        .artifacts
        .get_mut(QUALITY_ID)
        .unwrap()
        .bytes = json_bytes(&indeterminate);
    indeterminate_candidate.candidate_sha256 =
        candidate_identity(&indeterminate_candidate).unwrap();
    let decision = validate_receipt(
        &receipt(&indeterminate_candidate, ReceiptStatus::Indeterminate),
        &indeterminate_candidate,
        &registry,
    )
    .unwrap();
    assert_eq!(decision.status, ReceiptStatus::Indeterminate);
}
