use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde::Serialize;
use serde_json::{Value, json};
use luxel_certification_authority::schema::{
    AssetReceiptPayload, AssetUse, DeferredReceiptPayload, GameplayReceiptPayload,
    NativeVisualQualityReceiptPayload, RepairReceiptPayload, VisualReceiptPayload,
    WorldReceiptPayload,
};
use luxel_certification_authority::{
    ArtifactBytes, CandidateContext, DEFERRED_GATES, EvidenceBinding, GateDisposition,
    NATIVE_GRAPHICS_FRAME_RECEIPT_KIND, NATIVE_GRAPHICS_PACKET_KIND,
    NATIVE_GRAPHICS_RENDERER_ATTESTATION_KIND, NATIVE_RGBA8_CAPTURE_KIND,
    NATIVE_VISUAL_QUALITY_EVIDENCE_KIND, NATIVE_VISUAL_QUALITY_RECEIPT_SCHEMA, ReceiptEnvelope,
    ReceiptStatus, SUPPLIED_BAD_GLB_SHA256, ValidationRequest, ValidatorRegistry,
    candidate_identity, candidate_identity_bytes, engine_neutral_gate_profile,
    native_mvp_gate_profile, native_repair_evidence_reference, native_repair_receipt_bytes,
    repair_validator_registry, sha256_prefixed, validate_receipt, validate_request,
};
use luxel_intake_repair_contract as intake;
use luxel_native_graphics_contract as graphics;
use luxel_reference_runtime::{GameplayWorldBinding, WorldBuild, build_from_layout_path};

const WORLD_ID: &str = "runtime-world";
const TRAVERSAL_ID: &str = "runtime-traversal";
const LAYOUT_ID: &str = "authored-layout";
const CAPTURE_ID: &str = "reference-capture";
const VISUAL_ID: &str = "visual-evidence";
const GAMEPLAY_ID: &str = "gameplay-binding";
const GAMEPLAY_KIT_ID: &str = "gameplay-kit";
const MANIFEST_ID: &str = "project-manifest";
const NATIVE_PACKET_ID: &str = "native-graphics-packet";
const NATIVE_FRAME_RECEIPT_ID: &str = "native-graphics-frame-receipt";
const NATIVE_ATTESTATION_ID: &str = "native-graphics-renderer-attestation";
const NATIVE_CAPTURE_ID: &str = "native-rgba8-capture";
const NATIVE_QUALITY_ID: &str = "native-visual-quality-evidence";

static RIVERWATCH: OnceLock<WorldBuild> = OnceLock::new();

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("certification authority is nested under the Luxel project")
        .to_path_buf()
}

fn runtime_build() -> &'static WorldBuild {
    RIVERWATCH.get_or_init(|| {
        let root = project_root();
        let layout =
            root.join("world_core/crates/reference_runtime/examples/riverwatch.layout.json");
        let julia = std::env::var_os("LUXEL_JULIA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("julia"));
        build_from_layout_path(&layout, &julia, &root.join("terrain_lab"))
            .expect("the checked-in reference layout must produce a runtime bundle")
    })
}

fn json_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).expect("typed runtime evidence serializes")
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

fn runtime_candidate(snapshot_id: &str) -> CandidateContext {
    let build = runtime_build();
    let mut candidate = CandidateContext {
        project_id: "luxel-runtime-authority-tests".into(),
        snapshot_id: snapshot_id.into(),
        candidate_sha256: String::new(),
        artifacts: BTreeMap::new(),
        authorized_repair_artifact_ids: BTreeSet::new(),
    };
    put(
        &mut candidate,
        WORLD_ID,
        "world_artifact",
        json_bytes(&build.world),
    );
    put(
        &mut candidate,
        TRAVERSAL_ID,
        "traversal_evidence",
        json_bytes(&build.traversal),
    );
    put(
        &mut candidate,
        LAYOUT_ID,
        "authored_layout",
        json_bytes(&build.world.body.authored_layout),
    );
    put(
        &mut candidate,
        CAPTURE_ID,
        "reference_capture_ppm",
        build.capture_bytes.clone(),
    );
    put(
        &mut candidate,
        VISUAL_ID,
        "visual_evidence",
        json_bytes(&build.visual),
    );
    put(
        &mut candidate,
        GAMEPLAY_ID,
        "gameplay_world_binding",
        json_bytes(&build.gameplay),
    );
    put(
        &mut candidate,
        GAMEPLAY_KIT_ID,
        "gameplay_kit",
        json_bytes(&build.gameplay_kit),
    );
    put(
        &mut candidate,
        MANIFEST_ID,
        "project_manifest",
        b"luxel runtime authority integration test candidate v1\n".to_vec(),
    );
    refresh_candidate_identity(&mut candidate);
    candidate
}

fn renderer_attestation(
    frame: &graphics::GraphicsFrameReceipt,
) -> graphics::GraphicsRendererAttestation {
    let ready = graphics::GraphicsReady {
        schema_version: graphics::READY_SCHEMA.into(),
        backend_id: graphics::LAVA_BACKEND_ID.into(),
        adapter_revision: graphics::ADAPTER_REVISION.into(),
        lava_revision: graphics::LAVA_REVISION.into(),
        julia_version: "1.12.6-test".into(),
        vulkan_api_version: "1.3-test".into(),
        device_name: "runtime-authority-test-device".into(),
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
        "script_sha256": &frame.body.worker_script_sha256,
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

fn add_native_quality_artifacts(candidate: &mut CandidateContext) {
    let lowered = graphics::lower_reference_world(&runtime_build().world)
        .expect("runtime world lowers to a native graphics packet");
    let mut packet_body = lowered.body;
    packet_body.overlays.clear();
    let packet = graphics::seal_scene_packet(packet_body)
        .expect("native packet fixture is valid after removing overlays");
    let width = packet.body.capture.width_px as usize;
    let height = packet.body.capture.height_px as usize;
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
    let mut capture = Vec::with_capacity(width * height * 4);
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
            capture.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    let measurements = graphics::measure_frame_capture(&packet, &capture)
        .expect("structured fixture capture satisfies the native minimum measurements");
    let frame_body = graphics::GraphicsFrameReceiptBody {
        schema_version: graphics::FRAME_RECEIPT_SCHEMA.into(),
        packet_sha256: packet.packet_sha256.clone(),
        capture_id: packet.body.capture.capture_id.clone(),
        backend_id: graphics::LAVA_BACKEND_ID.into(),
        adapter_revision: graphics::ADAPTER_REVISION.into(),
        lava_revision: graphics::LAVA_REVISION.into(),
        device_uuid: "runtime-authority-test-device".into(),
        worker_script_sha256: graphics::sha256_prefixed(b"test worker"),
        renderer_identity_sha256: graphics::sha256_prefixed(b"test renderer"),
        status: graphics::FrameStatus::Passed,
        format: packet.body.capture.format,
        width_px: packet.body.capture.width_px,
        height_px: packet.body.capture.height_px,
        capture_sha256: Some(graphics::sha256_prefixed(&capture)),
        measurements,
        telemetry: graphics::GraphicsTelemetry {
            deformation: None,
            texture_residency: None,
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
        detail: "synthetic authority test capture".into(),
    };
    let mut frame = graphics::GraphicsFrameReceipt {
        receipt_sha256: graphics::sha256_prefixed(
            &graphics::canonical_json(&frame_body).expect("frame receipt serializes"),
        ),
        body: frame_body,
    };
    let provisional_attestation = renderer_attestation(&frame);
    frame.body.renderer_identity_sha256 = provisional_attestation.body.renderer_identity_sha256;
    frame.receipt_sha256 = graphics::sha256_prefixed(
        &graphics::canonical_json(&frame.body).expect("frame receipt serializes"),
    );
    let attestation = renderer_attestation(&frame);
    let quality = graphics::assess_visual_quality(
        &packet,
        &frame,
        &capture,
        &graphics::VisualQualityProfile::terrain_reference_v1(
            packet.body.capture.width_px,
            packet.body.capture.height_px,
        ),
    );
    assert_eq!(quality.body.outcome, graphics::QualityOutcome::Good);
    graphics::validate_visual_quality_evidence(&quality, &packet, &frame, &capture)
        .expect("fixture visual-quality evidence revalidates");

    put(
        candidate,
        NATIVE_PACKET_ID,
        NATIVE_GRAPHICS_PACKET_KIND,
        json_bytes(&packet),
    );
    put(
        candidate,
        NATIVE_FRAME_RECEIPT_ID,
        NATIVE_GRAPHICS_FRAME_RECEIPT_KIND,
        json_bytes(&frame),
    );
    put(
        candidate,
        NATIVE_ATTESTATION_ID,
        NATIVE_GRAPHICS_RENDERER_ATTESTATION_KIND,
        json_bytes(&attestation),
    );
    put(
        candidate,
        NATIVE_CAPTURE_ID,
        NATIVE_RGBA8_CAPTURE_KIND,
        capture,
    );
    put(
        candidate,
        NATIVE_QUALITY_ID,
        NATIVE_VISUAL_QUALITY_EVIDENCE_KIND,
        json_bytes(&quality),
    );
}

fn refresh_candidate_identity(candidate: &mut CandidateContext) {
    candidate.candidate_sha256 =
        candidate_identity(candidate).expect("candidate manifest identity is calculable");
}

fn gate_fields(registry: &ValidatorRegistry, gate_id: &str) -> (String, String) {
    let requirement = engine_neutral_gate_profile()
        .into_iter()
        .find(|gate| gate.gate_id == gate_id)
        .unwrap_or_else(|| panic!("gate {gate_id} is part of the registered Luxel profile"));
    assert!(registry.descriptor(&requirement.validator_id).is_some());
    (requirement.validator_id, requirement.receipt_schema)
}

fn bindings(candidate: &CandidateContext, ids: &[&str]) -> Vec<EvidenceBinding> {
    ids.iter()
        .map(|id| {
            let artifact = candidate
                .artifacts
                .get(*id)
                .unwrap_or_else(|| panic!("test candidate is missing artifact {id}"));
            EvidenceBinding {
                artifact_id: (*id).to_owned(),
                kind: artifact.kind.clone(),
                sha256: sha256_prefixed(&artifact.bytes),
            }
        })
        .collect()
}

fn receipt(
    candidate: &CandidateContext,
    registry: &ValidatorRegistry,
    gate_id: &str,
    status: ReceiptStatus,
    evidence_ids: &[&str],
    payload: Value,
) -> ReceiptEnvelope {
    let (validator_id, receipt_schema) = gate_fields(registry, gate_id);
    let mut envelope = ReceiptEnvelope {
        schema_version: luxel_certification_authority::ENVELOPE_SCHEMA.into(),
        receipt_id: String::new(),
        project_id: candidate.project_id.clone(),
        snapshot_id: candidate.snapshot_id.clone(),
        candidate_sha256: candidate.candidate_sha256.clone(),
        gate_id: gate_id.into(),
        validator_id,
        receipt_schema,
        status,
        producer: "runtime-authority-integration-test".into(),
        observed_input_sha256: String::new(),
        evidence: bindings(candidate, evidence_ids),
        payload,
    };
    envelope.seal().expect("receipt identity is deterministic");
    envelope
}

fn world_receipt(candidate: &CandidateContext, registry: &ValidatorRegistry) -> ReceiptEnvelope {
    receipt(
        candidate,
        registry,
        "world",
        ReceiptStatus::Pass,
        &[WORLD_ID, TRAVERSAL_ID, LAYOUT_ID],
        serde_json::to_value(WorldReceiptPayload {
            world_artifact_id: WORLD_ID.into(),
            traversal_artifact_id: TRAVERSAL_ID.into(),
            layout_artifact_id: LAYOUT_ID.into(),
        })
        .unwrap(),
    )
}

fn visual_receipt(candidate: &CandidateContext, registry: &ValidatorRegistry) -> ReceiptEnvelope {
    receipt(
        candidate,
        registry,
        "visual",
        ReceiptStatus::Pass,
        &[WORLD_ID, CAPTURE_ID, VISUAL_ID],
        serde_json::to_value(VisualReceiptPayload {
            world_artifact_id: WORLD_ID.into(),
            capture_artifact_id: CAPTURE_ID.into(),
            visual_evidence_artifact_id: VISUAL_ID.into(),
        })
        .unwrap(),
    )
}

fn visual_quality_receipt(
    candidate: &CandidateContext,
    registry: &ValidatorRegistry,
) -> ReceiptEnvelope {
    let (_, receipt_schema) = gate_fields(registry, "visual_quality");
    assert_eq!(receipt_schema, NATIVE_VISUAL_QUALITY_RECEIPT_SCHEMA);
    receipt(
        candidate,
        registry,
        "visual_quality",
        ReceiptStatus::Pass,
        &[
            WORLD_ID,
            NATIVE_PACKET_ID,
            NATIVE_FRAME_RECEIPT_ID,
            NATIVE_ATTESTATION_ID,
            NATIVE_CAPTURE_ID,
            NATIVE_QUALITY_ID,
        ],
        serde_json::to_value(NativeVisualQualityReceiptPayload {
            world_artifact_id: WORLD_ID.into(),
            packet_artifact_id: NATIVE_PACKET_ID.into(),
            frame_receipt_artifact_id: NATIVE_FRAME_RECEIPT_ID.into(),
            renderer_attestation_artifact_id: NATIVE_ATTESTATION_ID.into(),
            capture_artifact_id: NATIVE_CAPTURE_ID.into(),
            visual_quality_evidence_artifact_id: NATIVE_QUALITY_ID.into(),
        })
        .unwrap(),
    )
}

fn gameplay_receipt(candidate: &CandidateContext, registry: &ValidatorRegistry) -> ReceiptEnvelope {
    receipt(
        candidate,
        registry,
        "gameplay",
        ReceiptStatus::Pass,
        &[
            WORLD_ID,
            TRAVERSAL_ID,
            CAPTURE_ID,
            VISUAL_ID,
            GAMEPLAY_KIT_ID,
            GAMEPLAY_ID,
        ],
        serde_json::to_value(GameplayReceiptPayload {
            world_artifact_id: WORLD_ID.into(),
            traversal_artifact_id: TRAVERSAL_ID.into(),
            capture_artifact_id: CAPTURE_ID.into(),
            visual_evidence_artifact_id: VISUAL_ID.into(),
            gameplay_kit_artifact_id: GAMEPLAY_KIT_ID.into(),
            gameplay_binding_artifact_id: GAMEPLAY_ID.into(),
        })
        .unwrap(),
    )
}

fn semantic_receipt(
    candidate: &mut CandidateContext,
    registry: &ValidatorRegistry,
) -> ReceiptEnvelope {
    let brief =
        b"Build a traversal slice with a safe start and a visible relay objective.".to_vec();
    let layout = runtime_build().world.body.authored_layout.clone();
    // Deliberately use pretty source bytes and compact typed artifact bytes:
    // provenance hashes raw bytes, while layout identity hashes typed content.
    let layout_source = serde_json::to_vec_pretty(&layout).unwrap();
    let source_bytes_by_ref = BTreeMap::from([
        ("brief".to_owned(), brief.clone()),
        ("layout".to_owned(), layout_source.clone()),
    ]);
    let source_draft = intake::SourceBundleDraft {
        schema_version: intake::SOURCE_BUNDLE_DRAFT_SCHEMA.into(),
        request_id: "cert-intake-request-001".into(),
        sources: vec![
            intake::SourceDraft {
                source_ref: "brief".into(),
                kind: intake::SourceKind::Brief,
                content_sha256: sha256_prefixed(&brief),
                media_type: "text/plain".into(),
                provenance: intake::SourceProvenance {
                    origin: intake::ProvenanceOrigin::UserSupplied,
                    origin_ref: "test:brief".into(),
                    provider_id: None,
                    provider_version: None,
                },
            },
            intake::SourceDraft {
                source_ref: "layout".into(),
                kind: intake::SourceKind::DesignDocument,
                content_sha256: sha256_prefixed(&layout_source),
                media_type: "application/json".into(),
                provenance: intake::SourceProvenance {
                    origin: intake::ProvenanceOrigin::UserSupplied,
                    origin_ref: "test:authored-layout".into(),
                    provider_id: None,
                    provider_version: None,
                },
            },
        ],
    };
    let bundle = intake::prepare_source_bundle(source_draft, &source_bytes_by_ref).unwrap();
    let brief_source = bundle
        .sources
        .iter()
        .find(|item| item.kind == intake::SourceKind::Brief)
        .unwrap();
    let layout_source = bundle
        .sources
        .iter()
        .find(|item| item.kind == intake::SourceKind::DesignDocument)
        .unwrap();
    let interpretation = intake::ProviderInterpretation {
        schema_version: intake::PROVIDER_INTERPRETATION_SCHEMA.into(),
        source_bundle_id: bundle.source_bundle_id.clone(),
        claims: vec![
            intake::ClaimDraft {
                claim_ref: "brief-requirement".into(),
                epistemic_kind: intake::EpistemicKind::Observation,
                domain: intake::ClaimDomain::Gameplay,
                statement: "The brief requires a safe start and relay objective.".into(),
                confidence: 1.0,
                evidence: vec![intake::EvidenceLinkDraft {
                    source_id: brief_source.source_id.clone(),
                    region: Some(intake::SourceRegion::TextSpan {
                        start_byte: 0,
                        end_byte: brief.len() as u64,
                    }),
                }],
            },
            intake::ClaimDraft {
                claim_ref: "typed-layout".into(),
                epistemic_kind: intake::EpistemicKind::Observation,
                domain: intake::ClaimDomain::World,
                statement: "The design source provides the typed authored world layout.".into(),
                confidence: 1.0,
                evidence: vec![intake::EvidenceLinkDraft {
                    source_id: layout_source.source_id.clone(),
                    region: Some(intake::SourceRegion::TextSpan {
                        start_byte: 0,
                        end_byte: layout_source.byte_length,
                    }),
                }],
            },
        ],
        conflicts: vec![],
        assumptions: vec![],
    };
    let response = serde_json::to_vec(&interpretation).unwrap();
    let draft = intake::IntakeDraft {
        schema_version: intake::INTAKE_DRAFT_SCHEMA.into(),
        source_bundle_id: bundle.source_bundle_id.clone(),
        provider: intake::ProviderProvenance {
            provider_id: "test.provider".into(),
            provider_version: "1".into(),
            protocol: intake::PROVIDER_INTERPRETATION_SCHEMA.into(),
            request_source_bundle_id: bundle.source_bundle_id.clone(),
            response_sha256: sha256_prefixed(&response),
        },
        interpretation,
    };
    let source_bytes_by_id = BTreeMap::from([
        (brief_source.source_id.clone(), brief),
        (
            layout_source.source_id.clone(),
            layout_source_bytes(&source_bytes_by_ref),
        ),
    ]);
    let semantic =
        intake::normalize_intake(draft, &bundle, &response, &source_bytes_by_id).unwrap();
    put(
        candidate,
        "source-brief",
        "source_bytes",
        source_bytes_by_id[&brief_source.source_id].clone(),
    );
    put(
        candidate,
        "source-layout",
        "source_bytes",
        source_bytes_by_id[&layout_source.source_id].clone(),
    );
    put(
        candidate,
        "provider-response",
        "provider_response",
        response,
    );
    put(
        candidate,
        "source-bundle",
        "source_bundle",
        json_bytes(&bundle),
    );
    put(
        candidate,
        "semantic-intake",
        "semantic_intake",
        json_bytes(&semantic),
    );
    refresh_candidate_identity(candidate);
    receipt(
        candidate,
        registry,
        "semantic",
        ReceiptStatus::Pass,
        &[
            "semantic-intake",
            "provider-response",
            "source-bundle",
            "source-brief",
            "source-layout",
            LAYOUT_ID,
        ],
        serde_json::to_value(
            luxel_certification_authority::schema::SemanticReceiptPayload {
                intake_artifact_id: "semantic-intake".into(),
                provider_response_artifact_id: "provider-response".into(),
                source_bundle_artifact_id: "source-bundle".into(),
                layout_artifact_id: LAYOUT_ID.into(),
                project_spec_artifact_id: None,
                project_template_artifact_id: None,
                source_artifacts: vec![
                    luxel_certification_authority::schema::SourceArtifactBinding {
                        source_id: brief_source.source_id.clone(),
                        artifact_id: "source-brief".into(),
                    },
                    luxel_certification_authority::schema::SourceArtifactBinding {
                        source_id: layout_source.source_id.clone(),
                        artifact_id: "source-layout".into(),
                    },
                ],
            },
        )
        .unwrap(),
    )
}

fn layout_source_bytes(by_ref: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    by_ref.get("layout").unwrap().clone()
}

fn replace_gameplay_binding(candidate: &mut CandidateContext, binding: &GameplayWorldBinding) {
    put(
        candidate,
        GAMEPLAY_ID,
        "gameplay_world_binding",
        json_bytes(binding),
    );
    refresh_candidate_identity(candidate);
}

#[test]
fn authority_revalidates_the_exact_reference_runtime_p6_capture_against_its_world() {
    let build = runtime_build();
    assert!(build.capture_bytes.starts_with(b"P6\n"));
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let candidate = runtime_candidate("visual-exact-p6");
    let receipt = visual_receipt(&candidate, &registry);

    assert_eq!(
        validate_receipt(&receipt, &candidate, &registry)
            .unwrap()
            .status,
        ReceiptStatus::Pass
    );

    // Change a pixel, then recompute the candidate identity, evidence digest,
    // observed-input digest, and receipt ID. The native renderer must still
    // reject a pass-shaped capture that is not the exact render of this world.
    let mut forged_candidate = candidate.clone();
    let capture = &mut forged_candidate
        .artifacts
        .get_mut(CAPTURE_ID)
        .unwrap()
        .bytes;
    let final_byte = capture.len() - 1;
    capture[final_byte] ^= 1;
    refresh_candidate_identity(&mut forged_candidate);
    let forged_receipt = visual_receipt(&forged_candidate, &registry);
    let error = validate_receipt(&forged_receipt, &forged_candidate, &registry).unwrap_err();
    assert!(
        error.detail.contains("capture") || error.detail.contains("visual"),
        "unexpected rejection reason: {error}"
    );
}

#[test]
fn a_rehashed_world_body_cannot_forge_stale_julia_fields() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let mut candidate = runtime_candidate("rehashed-world-field");
    let world_bytes = &mut candidate.artifacts.get_mut(WORLD_ID).unwrap().bytes;
    let mut world: luxel_reference_runtime::WorldArtifact =
        serde_json::from_slice(world_bytes).unwrap();
    world.body.fields.heights_m[0] += 0.25;
    let body = serde_json::to_vec(&world.body).unwrap();
    world.artifact_sha256 = sha256_prefixed(&body);
    world.artifact_id = format!(
        "world-{}",
        world.artifact_sha256.trim_start_matches("sha256:")
    );
    *world_bytes = json_bytes(&world);
    refresh_candidate_identity(&mut candidate);
    let forged = world_receipt(&candidate, &registry);
    let error = validate_receipt(&forged, &candidate, &registry).unwrap_err();
    assert!(
        error.detail.contains("field")
            || error.detail.contains("spatial")
            || error.detail.contains("provenance"),
        "{error}"
    );
}

#[test]
fn gameplay_receipt_rejects_rehashed_binding_to_another_world_or_traversal() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let build = runtime_build();

    for bind_to_other_world in [true, false] {
        let mut candidate = runtime_candidate(if bind_to_other_world {
            "gameplay-other-world"
        } else {
            "gameplay-other-traversal"
        });
        let mut binding = build.gameplay.clone();
        if bind_to_other_world {
            binding.body.world_artifact_sha256 = sha256_prefixed(b"another world artifact");
        } else {
            binding.body.traversal_evidence_sha256 = sha256_prefixed(b"another traversal receipt");
        }
        binding.evidence_sha256 = sha256_prefixed(&json_bytes(&binding.body));
        replace_gameplay_binding(&mut candidate, &binding);
        let receipt = gameplay_receipt(&candidate, &registry);

        let error = validate_receipt(&receipt, &candidate, &registry).unwrap_err();
        assert!(
            error.detail.contains("different world")
                || error.detail.contains("runtime evidence")
                || error.detail.contains("world"),
            "unexpected rejection reason: {error}"
        );
    }
}

#[test]
fn candidate_identity_rejects_changed_artifact_bytes_before_receipt_validation() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let candidate = runtime_candidate("candidate-digest-mismatch");
    let receipt = world_receipt(&candidate, &registry);
    let mut changed = candidate.clone();
    changed
        .artifacts
        .get_mut(WORLD_ID)
        .unwrap()
        .bytes
        .push(b' ');

    let error = validate_receipt(&receipt, &changed, &registry).unwrap_err();
    assert!(error.detail.contains("complete artifact manifest"));

    let mut omitted = candidate.clone();
    put(
        &mut omitted,
        "unlisted-content",
        "source_bytes",
        b"new content".to_vec(),
    );
    let error = validate_receipt(&receipt, &omitted, &registry).unwrap_err();
    assert!(error.detail.contains("complete artifact manifest"));
}

#[test]
fn repair_metadata_is_excluded_from_candidate_identity_but_content_is_not() {
    let mut candidate = runtime_candidate("repair-metadata-identity");
    let identity = candidate.candidate_sha256.clone();
    put(
        &mut candidate,
        "repair-draft",
        "repair_delta",
        b"evidence-only".to_vec(),
    );
    assert_eq!(candidate_identity(&candidate).unwrap(), identity);
    put(
        &mut candidate,
        "new-source",
        "source_bytes",
        b"content".to_vec(),
    );
    assert_ne!(candidate_identity(&candidate).unwrap(), identity);
}

#[test]
fn status_only_receipt_stays_rejected_after_outer_identity_is_resealed() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let candidate = runtime_candidate("status-only-receipt");
    let mut status_only = world_receipt(&candidate, &registry);
    status_only.evidence.clear();
    status_only.seal().unwrap();
    let error = validate_receipt(&status_only, &candidate, &registry).unwrap_err();
    assert!(
        error.detail.contains("status-only") || error.detail.contains("no evidence"),
        "{error}"
    );
}

#[test]
fn registered_validator_id_requires_its_exact_receipt_schema() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let candidate = runtime_candidate("registered-id-schema-binding");
    let valid = world_receipt(&candidate, &registry);
    assert_eq!(
        validate_receipt(&valid, &candidate, &registry)
            .unwrap()
            .status,
        ReceiptStatus::Pass
    );

    let mut unknown_id = valid.clone();
    unknown_id.validator_id = "luxel.validator.world-traversal/v999".into();
    unknown_id.seal().unwrap();
    let error = validate_receipt(&unknown_id, &candidate, &registry).unwrap_err();
    assert!(error.detail.contains("not registered"), "{error}");

    let mut wrong_schema = valid;
    wrong_schema.receipt_schema = "luxel.world-receipt/v999".into();
    wrong_schema.seal().unwrap();
    let error = validate_receipt(&wrong_schema, &candidate, &registry).unwrap_err();
    assert!(error.detail.contains("another gate or schema"), "{error}");
}

#[test]
fn pass_shaped_payload_cannot_replace_typed_world_evidence() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let candidate = runtime_candidate("pass-shaped-payload");
    let mut forged = world_receipt(&candidate, &registry);
    forged.payload = serde_json::json!({
        "status": "pass",
        "producer": "luxel-certification-authority",
    });
    forged.seal().unwrap();

    let error = validate_receipt(&forged, &candidate, &registry).unwrap_err();
    assert!(error.detail.contains("world receipt payload"), "{error}");
}

#[test]
fn unknown_omitted_extra_and_duplicate_evidence_bindings_are_rejected() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let candidate = runtime_candidate("evidence-set-adversarial-controls");
    let valid = world_receipt(&candidate, &registry);
    assert_eq!(
        validate_receipt(&valid, &candidate, &registry)
            .unwrap()
            .status,
        ReceiptStatus::Pass
    );

    let mut unknown = valid.clone();
    unknown.evidence.push(EvidenceBinding {
        artifact_id: "not-in-candidate".into(),
        kind: "world_artifact".into(),
        sha256: sha256_prefixed(b"unknown evidence"),
    });
    unknown.seal().unwrap();
    let error = validate_receipt(&unknown, &candidate, &registry).unwrap_err();
    assert!(error.detail.contains("absent from candidate"), "{error}");

    let mut omitted = valid.clone();
    omitted
        .evidence
        .retain(|binding| binding.artifact_id != LAYOUT_ID);
    omitted.seal().unwrap();
    let error = validate_receipt(&omitted, &candidate, &registry).unwrap_err();
    assert!(error.detail.contains("does not bind required"), "{error}");

    let mut extra_candidate_content = candidate.clone();
    put(
        &mut extra_candidate_content,
        "unclaimed-debug-evidence",
        "debug_claim",
        b"producer claim".to_vec(),
    );
    refresh_candidate_identity(&mut extra_candidate_content);
    let mut extra = world_receipt(&extra_candidate_content, &registry);
    let artifact = &extra_candidate_content.artifacts["unclaimed-debug-evidence"];
    extra.evidence.push(EvidenceBinding {
        artifact_id: "unclaimed-debug-evidence".into(),
        kind: artifact.kind.clone(),
        sha256: sha256_prefixed(&artifact.bytes),
    });
    extra.seal().unwrap();
    let error = validate_receipt(&extra, &extra_candidate_content, &registry).unwrap_err();
    assert!(error.detail.contains("evidence set differs"), "{error}");

    let mut duplicate = valid;
    duplicate.evidence.push(duplicate.evidence[0].clone());
    duplicate.seal().unwrap();
    let error = validate_receipt(&duplicate, &candidate, &registry).unwrap_err();
    assert!(error.detail.contains("unique and sorted"), "{error}");

    let mut conflict = world_receipt(&candidate, &registry);
    let mut contradictory_binding = conflict.evidence[0].clone();
    contradictory_binding.sha256 = sha256_prefixed(b"conflicting digest");
    conflict.evidence.push(contradictory_binding);
    conflict.seal().unwrap();
    let error = validate_receipt(&conflict, &candidate, &registry).unwrap_err();
    assert!(error.detail.contains("unique and sorted"), "{error}");
}

#[test]
fn engine_neutral_rigging_cannot_be_promoted() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let candidate = runtime_candidate("deferred-gates-stay-deferred");
    for gate in DEFERRED_GATES {
        let payload = DeferredReceiptPayload {
            reason_code: "deferred_by_scope".into(),
            deferral_scope: gate.into(),
            detail: "explicitly deferred by engine-neutral certification scope".into(),
        };
        let valid = receipt(
            &candidate,
            &registry,
            gate,
            ReceiptStatus::Indeterminate,
            &[MANIFEST_ID],
            serde_json::to_value(&payload).unwrap(),
        );
        assert_eq!(
            validate_receipt(&valid, &candidate, &registry)
                .unwrap()
                .status,
            ReceiptStatus::Indeterminate,
            "{gate} must remain indeterminate in the engine-neutral registry"
        );

        let mut promoted = valid;
        promoted.status = ReceiptStatus::Pass;
        promoted.seal().unwrap();
        let error = validate_receipt(&promoted, &candidate, &registry).unwrap_err();
        assert!(error.detail.contains("cannot accept"), "{gate}: {error}");

        let mut gates = engine_neutral_gate_profile();
        gates
            .iter_mut()
            .find(|requirement| requirement.gate_id == gate)
            .unwrap()
            .disposition = GateDisposition::RequiredPass;
        let request = ValidationRequest {
            current_snapshot_id: candidate.snapshot_id.clone(),
            candidates: vec![candidate.clone()],
            gates,
            receipts: vec![world_receipt(&candidate, &registry)],
        };
        let error = validate_request(&request, &registry).unwrap_err();
        assert!(
            error.detail.contains("gate profile must exactly match"),
            "{gate}: {error}"
        );
    }

    let native_profile = native_mvp_gate_profile();
    let request = ValidationRequest {
        current_snapshot_id: candidate.snapshot_id.clone(),
        candidates: vec![candidate.clone()],
        gates: native_profile,
        receipts: vec![world_receipt(&candidate, &registry)],
    };
    let error = validate_request(&request, &registry).unwrap_err();
    assert!(error.detail.contains("unknown validator"), "{error}");
}

#[test]
fn semantic_gate_revalidates_provider_response_sources_and_typed_layout_provenance() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let mut candidate = runtime_candidate("typed-intake-layout-binding");
    let good = semantic_receipt(&mut candidate, &registry);
    assert_eq!(
        validate_receipt(&good, &candidate, &registry)
            .unwrap()
            .status,
        ReceiptStatus::Pass
    );

    let response = &mut candidate
        .artifacts
        .get_mut("provider-response")
        .unwrap()
        .bytes;
    response.push(b' ');
    refresh_candidate_identity(&mut candidate);
    let forged = receipt(
        &candidate,
        &registry,
        "semantic",
        ReceiptStatus::Pass,
        &[
            "semantic-intake",
            "provider-response",
            "source-bundle",
            "source-brief",
            "source-layout",
            LAYOUT_ID,
        ],
        good.payload,
    );
    let error = validate_receipt(&forged, &candidate, &registry).unwrap_err();
    assert!(
        error.detail.contains("provider") || error.detail.contains("raw intake"),
        "{error}"
    );
}

#[test]
fn native_seal_cli_computes_receipt_identity_and_writes_canonical_json() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let candidate = runtime_candidate("native-seal-cli");
    let mut draft = world_receipt(&candidate, &registry);
    draft.receipt_id.clear();
    draft.observed_input_sha256.clear();
    let directory = std::env::temp_dir().join(format!("luxel-cert-seal-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let input = directory.join("draft.json");
    let output = directory.join("sealed.json");
    fs::write(&input, serde_json::to_vec(&draft).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_luxel-certification-authority"))
        .arg("seal")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let sealed_bytes = fs::read(&output).unwrap();
    let sealed: ReceiptEnvelope = serde_json::from_slice(&sealed_bytes).unwrap();
    assert!(!sealed.receipt_id.is_empty());
    assert!(!sealed.observed_input_sha256.is_empty());
    let value: Value = serde_json::from_slice(&sealed_bytes).unwrap();
    assert_eq!(
        sealed_bytes,
        format!("{}\n", luxel_certification_authority::canonical_json(&value)).as_bytes()
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rehashed_receipt_with_stale_artifact_binding_is_rejected() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let mut candidate = runtime_candidate("stale-receipt");
    let mut receipt = visual_receipt(&candidate, &registry);

    let capture = &mut candidate.artifacts.get_mut(CAPTURE_ID).unwrap().bytes;
    let final_byte = capture.len() - 1;
    capture[final_byte] ^= 1;
    refresh_candidate_identity(&mut candidate);
    receipt.candidate_sha256 = candidate.candidate_sha256.clone();
    // Deliberately retain the old evidence digest. `seal` makes the outer
    // receipt internally fresh, while its evidence still names stale bytes.
    receipt.seal().unwrap();

    let error = validate_receipt(&receipt, &candidate, &registry).unwrap_err();
    assert!(
        error.detail.contains("stale or forged"),
        "unexpected rejection reason: {error}"
    );
}

#[test]
fn a_resealed_rigging_deferral_cannot_be_changed_to_pass() {
    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let candidate = runtime_candidate("deferred-pass-control");
    let payload = DeferredReceiptPayload {
        reason_code: "deferred_by_scope".into(),
        deferral_scope: "rigging".into(),
        detail: "Rigging is deferred by the engine-neutral certification scope.".into(),
    };
    let mut deferred = receipt(
        &candidate,
        &registry,
        "rigging",
        ReceiptStatus::Indeterminate,
        &[MANIFEST_ID],
        serde_json::to_value(payload).unwrap(),
    );
    assert_eq!(
        validate_receipt(&deferred, &candidate, &registry)
            .unwrap()
            .status,
        ReceiptStatus::Indeterminate
    );

    deferred.status = ReceiptStatus::Pass;
    deferred.seal().unwrap();
    let error = validate_receipt(&deferred, &candidate, &registry).unwrap_err();
    assert!(error.detail.contains("cannot accept"));
}

#[test]
fn supplied_bad_glb_bytes_remain_a_permanent_asset_rejection_control() {
    let supplied_path = std::env::var_os("LUXEL_NEGATIVE_CONTROL_GLB")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                "/home/mattc/Pictures/Generated 2D Images/sample_2026-09-26T091412.074.glb",
            )
        });
    let bytes = std::fs::read(&supplied_path).unwrap_or_else(|error| {
        panic!(
            "cannot read supplied negative-control GLB {}: {error}",
            supplied_path.display()
        )
    });
    assert_eq!(sha256_prefixed(&bytes), SUPPLIED_BAD_GLB_SHA256);

    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let mut candidate = CandidateContext {
        project_id: "luxel-runtime-authority-tests".into(),
        snapshot_id: "negative-glb-control".into(),
        candidate_sha256: String::new(),
        artifacts: BTreeMap::new(),
        authorized_repair_artifact_ids: BTreeSet::new(),
    };
    put(&mut candidate, "bad-glb", "static_mesh_source", bytes);
    put(
        &mut candidate,
        "bad-package",
        "asset_package",
        b"{}".to_vec(),
    );
    refresh_candidate_identity(&mut candidate);
    let rejected = receipt(
        &candidate,
        &registry,
        "asset",
        ReceiptStatus::Fail,
        &["bad-glb", "bad-package"],
        serde_json::to_value(AssetReceiptPayload {
            source_artifact_id: "bad-glb".into(),
            package_artifact_id: "bad-package".into(),
            asset_use: AssetUse::StaticEnvironment,
        })
        .unwrap(),
    );
    assert_eq!(
        validate_receipt(&rejected, &candidate, &registry)
            .unwrap()
            .status,
        ReceiptStatus::Fail
    );
    let mut fake_pass = rejected;
    fake_pass.status = ReceiptStatus::Pass;
    fake_pass.seal().unwrap();
    let error = validate_receipt(&fake_pass, &candidate, &registry).unwrap_err();
    assert!(
        error.detail.contains("negative control GLB"),
        "unexpected rejection reason: {error}"
    );
}

#[test]
fn typed_repair_gate_revalidates_raw_before_after_receipts_and_exact_artifact_delta() {
    let supplied_path = std::env::var_os("LUXEL_NEGATIVE_CONTROL_GLB")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                "/home/mattc/Pictures/Generated 2D Images/sample_2026-09-26T091412.074.glb",
            )
        });
    let bad_glb =
        fs::read(supplied_path).expect("supplied bad GLB negative control must be present");
    assert_eq!(sha256_prefixed(&bad_glb), SUPPLIED_BAD_GLB_SHA256);

    let registry = ValidatorRegistry::luxel_engine_neutral_v1();
    let mut before = runtime_candidate("repair-before");
    add_native_quality_artifacts(&mut before);
    refresh_candidate_identity(&mut before);
    put(&mut before, "mesh-source", "static_mesh_source", bad_glb);
    put(&mut before, "mesh-package", "asset_package", b"{}".to_vec());
    let _before_semantic = semantic_receipt(&mut before, &registry);
    let before_asset = receipt(
        &before,
        &registry,
        "asset",
        ReceiptStatus::Fail,
        &["mesh-source", "mesh-package"],
        serde_json::to_value(AssetReceiptPayload {
            source_artifact_id: "mesh-source".into(),
            package_artifact_id: "mesh-package".into(),
            asset_use: AssetUse::StaticEnvironment,
        })
        .unwrap(),
    );
    assert_eq!(
        validate_receipt(&before_asset, &before, &registry)
            .unwrap()
            .status,
        ReceiptStatus::Fail
    );

    let source = luxel_certification_authority::schema::StaticMeshSource {
        schema_version: "luxel.static-mesh-source/v1".into(),
        asset_id: "replacement-stone".into(),
        positions_m: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ],
        triangle_indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        material_slots: vec!["stone".into()],
    };
    let after_source_bytes = json_bytes(&source);
    let after_package = luxel_certification_authority::schema::StaticAssetPackage {
        schema_version: "luxel.asset-package/v1".into(),
        asset_id: source.asset_id.clone(),
        source_sha256: sha256_prefixed(&after_source_bytes),
        asset_use: AssetUse::StaticEnvironment,
        bounds_min_m: [0.0, 0.0, 0.0],
        bounds_max_m: [1.0, 1.0, 1.0],
        pivot_m: [0.0, 0.0, 0.0],
        collision_bounds_min_m: [-0.1, -0.1, -0.1],
        collision_bounds_max_m: [1.1, 1.1, 1.1],
        material_slots: vec!["stone".into()],
        lod_triangle_counts: vec![4],
    };
    let after_package_bytes = json_bytes(&after_package);
    let mut after = before.clone();
    after.snapshot_id = "repair-after".into();
    after.authorized_repair_artifact_ids =
        BTreeSet::from(["mesh-source".into(), "mesh-package".into()]);
    put(
        &mut after,
        "mesh-source",
        "static_mesh_source",
        after_source_bytes.clone(),
    );
    put(
        &mut after,
        "mesh-package",
        "asset_package",
        after_package_bytes.clone(),
    );
    let semantic_after = semantic_receipt(&mut after, &registry);
    let after_asset = receipt(
        &after,
        &registry,
        "asset",
        ReceiptStatus::Pass,
        &["mesh-source", "mesh-package"],
        serde_json::to_value(AssetReceiptPayload {
            source_artifact_id: "mesh-source".into(),
            package_artifact_id: "mesh-package".into(),
            asset_use: AssetUse::StaticEnvironment,
        })
        .unwrap(),
    );
    assert_eq!(
        validate_receipt(&after_asset, &after, &registry)
            .unwrap()
            .status,
        ReceiptStatus::Pass
    );

    let failure_evidence =
        native_repair_evidence_reference(&before_asset, &before, &registry).unwrap();
    let after_evidence = native_repair_evidence_reference(&after_asset, &after, &registry).unwrap();
    let proposal = intake::normalize_repair_proposal(intake::RepairProposalDraft {
        schema_version: intake::REPAIR_PROPOSAL_DRAFT_SCHEMA.into(),
        candidate_before_sha256: before.candidate_sha256.clone(),
        failed_layer: intake::FailedLayer::AssetPreparation,
        failure_evidence: failure_evidence.clone(),
        edit_class: intake::RepairEditClass::AdjustAssetMetadata,
        authorized_targets: vec![
            intake::RepairTarget {
                artifact_id: "mesh-source".into(),
                before_sha256: sha256_prefixed(&before.artifacts["mesh-source"].bytes),
            },
            intake::RepairTarget {
                artifact_id: "mesh-package".into(),
                before_sha256: sha256_prefixed(&before.artifacts["mesh-package"].bytes),
            },
        ],
        max_artifact_changes: 2,
        diagnosis: "The supplied GLB is the registered malformed negative control.".into(),
        rationale: "Replace the rejected static source and rebuild its package metadata.".into(),
    })
    .unwrap();
    let delta_draft = intake::RepairEvidenceDeltaDraft {
        schema_version: intake::REPAIR_DELTA_DRAFT_SCHEMA.into(),
        proposal_id: proposal.proposal_id.clone(),
        candidate_before_sha256: before.candidate_sha256.clone(),
        candidate_after_sha256: after.candidate_sha256.clone(),
        before_evidence: failure_evidence,
        after_evidence,
        changed_artifacts: vec![
            intake::ArtifactChange {
                artifact_id: "mesh-package".into(),
                before_sha256: sha256_prefixed(&before.artifacts["mesh-package"].bytes),
                after_sha256: sha256_prefixed(&after.artifacts["mesh-package"].bytes),
            },
            intake::ArtifactChange {
                artifact_id: "mesh-source".into(),
                before_sha256: sha256_prefixed(&before.artifacts["mesh-source"].bytes),
                after_sha256: sha256_prefixed(&after.artifacts["mesh-source"].bytes),
            },
        ],
    };
    let before_native = native_repair_receipt_bytes(&before_asset, &before, &registry).unwrap();
    let after_native = native_repair_receipt_bytes(&after_asset, &after, &registry).unwrap();
    let contract_registry = repair_validator_registry().unwrap();
    let before_artifacts = before
        .artifacts
        .iter()
        .map(|(id, a)| (id.clone(), a.bytes.clone()))
        .collect();
    let after_artifacts = after
        .artifacts
        .iter()
        .map(|(id, a)| (id.clone(), a.bytes.clone()))
        .collect();
    let delta = intake::validate_repair_delta(
        &proposal,
        delta_draft.clone(),
        &candidate_identity_bytes(&before).unwrap(),
        &candidate_identity_bytes(&after).unwrap(),
        &before_native,
        &after_native,
        &before_artifacts,
        &after_artifacts,
        &contract_registry,
    )
    .unwrap();
    assert_eq!(delta.assessment, intake::RepairAssessment::GatePassed);

    put(
        &mut after,
        "repair-proposal",
        "repair_proposal",
        json_bytes(&proposal),
    );
    put(
        &mut after,
        "repair-draft",
        "repair_delta",
        json_bytes(&delta_draft),
    );
    put(
        &mut after,
        "repair-record",
        "repair_record",
        json_bytes(&delta),
    );
    assert_eq!(
        candidate_identity(&after).unwrap(),
        after.candidate_sha256,
        "evidence metadata does not perturb snapshot identity"
    );
    let repair_payload = RepairReceiptPayload {
        proposal_artifact_id: "repair-proposal".into(),
        delta_draft_artifact_id: "repair-draft".into(),
        delta_artifact_id: "repair-record".into(),
        before_snapshot_id: before.snapshot_id.clone(),
        before_receipt_id: before_asset.receipt_id.clone(),
        after_receipt_id: after_asset.receipt_id.clone(),
        proposal,
        delta_draft,
        delta,
    };
    let repair = receipt(
        &after,
        &registry,
        "repair",
        ReceiptStatus::Pass,
        &["repair-proposal", "repair-draft", "repair-record"],
        serde_json::to_value(repair_payload).unwrap(),
    );

    let mut receipts = vec![
        semantic_after,
        world_receipt(&after, &registry),
        gameplay_receipt(&after, &registry),
        after_asset,
        visual_receipt(&after, &registry),
        visual_quality_receipt(&after, &registry),
        repair,
        before_asset,
    ];
    for gate in DEFERRED_GATES {
        receipts.push(receipt(
            &after,
            &registry,
            gate,
            ReceiptStatus::Indeterminate,
            &[MANIFEST_ID],
            serde_json::to_value(DeferredReceiptPayload {
                reason_code: "deferred_by_scope".into(),
                deferral_scope: gate.into(),
                detail: "explicitly deferred by engine-neutral certification scope".into(),
            })
            .unwrap(),
        ));
    }
    let report = validate_request(
        &luxel_certification_authority::ValidationRequest {
            current_snapshot_id: after.snapshot_id.clone(),
            candidates: vec![before, after],
            gates: engine_neutral_gate_profile(),
            receipts,
        },
        &registry,
    )
    .unwrap();
    assert_eq!(
        report.status,
        luxel_certification_authority::CertificationStatus::EngineNeutralCertified
    );
}
