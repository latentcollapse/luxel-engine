use std::collections::{BTreeMap, BTreeSet};

use crate::schema::*;
use crate::{
    AuthorityError, CandidateContext, NATIVE_GRAPHICS_FRAME_RECEIPT_KIND,
    NATIVE_GRAPHICS_PACKET_KIND, NATIVE_GRAPHICS_RENDERER_ATTESTATION_KIND,
    NATIVE_RGBA8_CAPTURE_KIND, NATIVE_VISUAL_QUALITY_EVIDENCE_KIND,
    NATIVE_VISUAL_QUALITY_RECEIPT_SCHEMA, ReceiptEnvelope, ReceiptStatus, ValidatedReceipt,
    ValidatorKind, parse_artifact, sha256_prefixed,
};
use wge_asset_contract::runtime as asset_runtime;
use wge_intake_repair_contract as intake;
use wge_native_graphics_contract as native_graphics;
use wge_reference_runtime as runtime;

const WORLD_KIND: &str = "world_artifact";
const TRAVERSAL_KIND: &str = "traversal_evidence";
const CAPTURE_KIND: &str = "reference_capture_ppm";
const VISUAL_KIND: &str = "visual_evidence";
const GAMEPLAY_BINDING_KIND: &str = "gameplay_world_binding";
const GAMEPLAY_KIT_KIND: &str = "gameplay_kit";
const INTAKE_KIND: &str = "semantic_intake";
const PROVIDER_RESPONSE_KIND: &str = "provider_response";
const SOURCE_BUNDLE_KIND: &str = "source_bundle";
const SOURCE_BYTES_KIND: &str = "source_bytes";
const PROJECT_SPEC_KIND: &str = "project_spec";
const PROJECT_TEMPLATE_KIND: &str = "project_template";

#[derive(Clone, Debug)]
pub(crate) struct DomainVerdict {
    pub status: ReceiptStatus,
    pub detail: String,
    pub measured_artifact_id: String,
    pub metric_id: intake::MetricId,
    pub metric_value: f64,
    pub failure_code: Option<String>,
}

pub(crate) fn validate(
    kind: ValidatorKind,
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
    candidates: &BTreeMap<&str, &CandidateContext>,
    validated: &BTreeMap<String, ValidatedReceipt>,
) -> Result<DomainVerdict, AuthorityError> {
    match kind {
        ValidatorKind::Semantic => validate_semantic(envelope, candidate),
        ValidatorKind::World => validate_world(envelope, candidate),
        ValidatorKind::Gameplay => validate_gameplay(envelope, candidate),
        ValidatorKind::Asset => validate_asset(envelope, candidate),
        ValidatorKind::Rigging => validate_rigging(envelope, candidate),
        ValidatorKind::Visual => validate_visual(envelope, candidate),
        ValidatorKind::VisualQuality => validate_visual_quality(envelope, candidate),
        ValidatorKind::Repair => validate_repair(envelope, candidate, candidates, validated),
        ValidatorKind::Deferred => validate_deferred(envelope, candidate),
    }
}

fn validate_semantic(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
) -> Result<DomainVerdict, AuthorityError> {
    let payload: SemanticReceiptPayload = parse_payload(envelope, "semantic")?;
    let intake_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.intake_artifact_id,
        INTAKE_KIND,
    )?;
    let response = bound_artifact(
        envelope,
        candidate,
        &payload.provider_response_artifact_id,
        PROVIDER_RESPONSE_KIND,
    )?;
    let bundle_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.source_bundle_artifact_id,
        SOURCE_BUNDLE_KIND,
    )?;
    let layout_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.layout_artifact_id,
        "authored_layout",
    )?;
    let semantic: intake::SemanticIntake =
        parse_artifact(intake_bytes, "canonical semantic intake")?;
    let bundle: intake::SourceBundle = parse_artifact(bundle_bytes, "source bundle")?;
    let layout: runtime::AuthoredLayout = parse_artifact(layout_bytes, "typed authored layout")?;
    runtime::validate_layout(&layout).map_err(runtime_error)?;
    let project_spec_id = payload.project_spec_artifact_id.as_ref();
    let project_template_id = payload.project_template_artifact_id.as_ref();
    if project_spec_id.is_some() != project_template_id.is_some() {
        return Err(AuthorityError::new(
            "provenance",
            "compiled project spec and project template must be bound together",
        ));
    }
    let project_definition = if let (Some(spec_id), Some(template_id)) =
        (project_spec_id, project_template_id)
    {
        let spec_bytes = bound_artifact(envelope, candidate, spec_id, PROJECT_SPEC_KIND)?;
        let template_bytes =
            bound_artifact(envelope, candidate, template_id, PROJECT_TEMPLATE_KIND)?;
        let spec: wge_project_ledger::ProjectSpec =
            parse_artifact(spec_bytes, "compiled project spec")?;
        let template: wge_project_ledger::ProjectTemplate =
            parse_artifact(template_bytes, "typed project template")?;
        wge_project_ledger::validate_spec(&spec).map_err(|error| {
            AuthorityError::new("contract", format!("compiled project spec failed: {error}"))
        })?;
        if spec.project_id != candidate.project_id {
            return Err(AuthorityError::new(
                "provenance",
                "compiled project spec project_id differs from the candidate",
            ));
        }
        let compiled =
            wge_project_ledger::compile_project_spec(&semantic, template).map_err(|error| {
                AuthorityError::new(
                    "provenance",
                    format!("project template does not compile against canonical intake: {error}"),
                )
            })?;
        let supplied_value = serde_json::to_value(&spec)
            .map_err(|error| AuthorityError::new("malformed", error.to_string()))?;
        let compiled_value = serde_json::to_value(&compiled)
            .map_err(|error| AuthorityError::new("malformed", error.to_string()))?;
        if wge_project_ledger::canonical_json(&supplied_value)
            != wge_project_ledger::canonical_json(&compiled_value)
        {
            return Err(AuthorityError::new(
                "provenance",
                "compiled project spec differs from the canonical intake/template compilation",
            ));
        }
        Some((spec_id.clone(), template_id.clone()))
    } else {
        None
    };
    if bundle.request_id != semantic.request_id
        || bundle.source_bundle_id != semantic.source_bundle_id
        || bundle.sources != semantic.sources
    {
        return Err(AuthorityError::new(
            "provenance",
            "raw source bundle differs from the canonical intake source manifest",
        ));
    }
    let mut sources = BTreeMap::new();
    let mut expected = BTreeMap::from([
        (payload.intake_artifact_id.clone(), INTAKE_KIND.to_owned()),
        (
            payload.provider_response_artifact_id.clone(),
            PROVIDER_RESPONSE_KIND.to_owned(),
        ),
        (
            payload.source_bundle_artifact_id.clone(),
            SOURCE_BUNDLE_KIND.to_owned(),
        ),
        (
            payload.layout_artifact_id.clone(),
            "authored_layout".to_owned(),
        ),
    ]);
    if let Some((spec_id, template_id)) = project_definition {
        expected.insert(spec_id, PROJECT_SPEC_KIND.to_owned());
        expected.insert(template_id, PROJECT_TEMPLATE_KIND.to_owned());
    }
    let mut source_ids = BTreeSet::new();
    let mut layout_source_typed_match = false;
    for binding in &payload.source_artifacts {
        if !source_ids.insert(binding.source_id.as_str()) {
            return Err(AuthorityError::new(
                "malformed",
                "duplicate source artifact binding",
            ));
        }
        let record = semantic
            .sources
            .iter()
            .find(|source| source.source_id == binding.source_id)
            .ok_or_else(|| {
                AuthorityError::new(
                    "provenance",
                    "source artifact binding names an unknown source",
                )
            })?;
        let bytes = bound_artifact(envelope, candidate, &binding.artifact_id, SOURCE_BYTES_KIND)?;
        if sha256_prefixed(bytes) != record.content_sha256
            || bytes.len() as u64 != record.byte_length
        {
            return Err(AuthorityError::new(
                "provenance",
                "raw source bytes do not match canonical source record",
            ));
        }
        if binding.artifact_id == payload.layout_artifact_id {
            return Err(AuthorityError::new(
                "provenance",
                "authored layout source bytes must use a distinct source_bytes artifact ID",
            ));
        }
        if record.kind == intake::SourceKind::DesignDocument
            && let Ok(source_layout) =
                parse_artifact::<runtime::AuthoredLayout>(bytes, "provider design-document layout")
        {
            if source_layout != layout {
                return Err(AuthorityError::new(
                    "provenance",
                    "provider-bound layout source differs semantically from typed authored layout",
                ));
            }
            layout_source_typed_match = true;
        }
        sources.insert(binding.source_id.clone(), bytes.to_vec());
        expected.insert(binding.artifact_id.clone(), SOURCE_BYTES_KIND.to_owned());
    }
    if source_ids.len() != semantic.sources.len() {
        return Err(AuthorityError::new(
            "provenance",
            "every canonical source must bind exact raw bytes",
        ));
    }
    if !layout_source_typed_match {
        return Err(AuthorityError::new(
            "provenance",
            "authored layout must be linked to a provider-bound source record",
        ));
    }
    require_exact_evidence(envelope, &expected)?;
    intake::validate_intake(&semantic, response, &sources).map_err(|error| {
        AuthorityError::new(
            "contract",
            format!("raw intake revalidation failed: {error}"),
        )
    })?;
    Ok(DomainVerdict {
        status: ReceiptStatus::Pass,
        detail: format!(
            "canonical intake {} revalidated against provider response and {} raw sources",
            semantic.intake_id,
            semantic.sources.len()
        ),
        measured_artifact_id: payload.intake_artifact_id,
        metric_id: intake::MetricId::UnresolvedConflicts,
        metric_value: semantic.conflicts.len() as f64,
        failure_code: None,
    })
}

fn validate_world(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
) -> Result<DomainVerdict, AuthorityError> {
    let payload: WorldReceiptPayload = parse_payload(envelope, "world")?;
    let world_bytes = bound_artifact(envelope, candidate, &payload.world_artifact_id, WORLD_KIND)?;
    let traversal_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.traversal_artifact_id,
        TRAVERSAL_KIND,
    )?;
    let layout_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.layout_artifact_id,
        "authored_layout",
    )?;
    require_exact_evidence(
        envelope,
        &BTreeMap::from([
            (payload.world_artifact_id.clone(), WORLD_KIND.to_owned()),
            (
                payload.traversal_artifact_id.clone(),
                TRAVERSAL_KIND.to_owned(),
            ),
            (
                payload.layout_artifact_id.clone(),
                "authored_layout".to_owned(),
            ),
        ]),
    )?;
    let world: runtime::WorldArtifact =
        parse_artifact(world_bytes, "reference-runtime world artifact")?;
    let traversal: runtime::TraversalEvidence =
        parse_artifact(traversal_bytes, "reference-runtime traversal evidence")?;
    let layout: runtime::AuthoredLayout = parse_artifact(layout_bytes, "typed authored layout")?;
    runtime::validate_world_artifact(&world).map_err(runtime_error)?;
    runtime::validate_layout(&layout).map_err(runtime_error)?;
    let canonical_layout = serde_json::to_vec(&layout)
        .map_err(|error| AuthorityError::new("malformed", error.to_string()))?;
    if world.body.authored_layout != layout
        || world.body.authored_layout_sha256 != sha256_prefixed(&canonical_layout)
    {
        return Err(AuthorityError::new(
            "provenance",
            "world authored_layout_sha256 is detached from the provider-bound typed layout artifact",
        ));
    }
    runtime::validate_traversal_evidence(&world, &traversal).map_err(runtime_error)?;
    Ok(DomainVerdict {
        status: ReceiptStatus::Pass,
        detail: format!(
            "world {} and {} traversal steps independently revalidated",
            world.artifact_id,
            traversal.body.steps.len()
        ),
        measured_artifact_id: payload.world_artifact_id,
        metric_id: intake::MetricId::TraversalBlockedSteps,
        metric_value: 0.0,
        failure_code: None,
    })
}

fn validate_gameplay(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
) -> Result<DomainVerdict, AuthorityError> {
    let payload: GameplayReceiptPayload = parse_payload(envelope, "gameplay")?;
    let (world, traversal, capture, visual, binding) = load_runtime_evidence(
        envelope,
        candidate,
        &payload.world_artifact_id,
        &payload.traversal_artifact_id,
        &payload.capture_artifact_id,
        &payload.visual_evidence_artifact_id,
        &payload.gameplay_binding_artifact_id,
    )?;
    let gameplay_kit_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.gameplay_kit_artifact_id,
        GAMEPLAY_KIT_KIND,
    )?;
    let gameplay_kit: wge_gameplay_contract::ResolvedKit =
        parse_artifact(gameplay_kit_bytes, "resolved gameplay kit")?;
    runtime::validate_gameplay_world_binding(&world, &traversal, capture, &visual, &binding)
        .map_err(runtime_error)?;
    runtime::validate_gameplay_kit(&gameplay_kit).map_err(runtime_error)?;
    runtime::validate_general_gameplay_evidence(&world, &binding.body.general_gameplay)
        .map_err(runtime_error)?;
    let evidence = runtime_evidence_set(
        &payload.world_artifact_id,
        &payload.traversal_artifact_id,
        &payload.capture_artifact_id,
        &payload.visual_evidence_artifact_id,
        Some(&payload.gameplay_binding_artifact_id),
    );
    let mut evidence = evidence;
    evidence.insert(
        payload.gameplay_kit_artifact_id.clone(),
        GAMEPLAY_KIT_KIND.into(),
    );
    require_exact_evidence(envelope, &evidence)?;
    Ok(DomainVerdict {
        status: ReceiptStatus::Pass,
        detail: format!(
            "world-bound gameplay replay produced {:?}",
            binding.body.outcome
        ),
        measured_artifact_id: payload.gameplay_binding_artifact_id,
        metric_id: intake::MetricId::GameplayObjectiveCompletion,
        metric_value: 1.0,
        failure_code: None,
    })
}

fn validate_visual(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
) -> Result<DomainVerdict, AuthorityError> {
    let payload: VisualReceiptPayload = parse_payload(envelope, "visual")?;
    let world_bytes = bound_artifact(envelope, candidate, &payload.world_artifact_id, WORLD_KIND)?;
    let capture = bound_artifact(
        envelope,
        candidate,
        &payload.capture_artifact_id,
        CAPTURE_KIND,
    )?;
    let visual_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.visual_evidence_artifact_id,
        VISUAL_KIND,
    )?;
    let world: runtime::WorldArtifact =
        parse_artifact(world_bytes, "reference-runtime world artifact")?;
    let visual: runtime::VisualEvidence =
        parse_artifact(visual_bytes, "reference-runtime visual evidence")?;
    runtime::validate_world_artifact(&world).map_err(runtime_error)?;
    runtime::validate_visual_evidence(&world, capture, &visual).map_err(runtime_error)?;
    require_exact_evidence(
        envelope,
        &BTreeMap::from([
            (payload.world_artifact_id.clone(), WORLD_KIND.to_owned()),
            (payload.capture_artifact_id.clone(), CAPTURE_KIND.to_owned()),
            (
                payload.visual_evidence_artifact_id.clone(),
                VISUAL_KIND.to_owned(),
            ),
        ]),
    )?;
    let passed = visual.body.status == runtime::VisualGateStatus::Passed;
    let coverage = visual.body.measurements.world_coverage_ratio;
    Ok(DomainVerdict {
        status: if passed {
            ReceiptStatus::Pass
        } else {
            ReceiptStatus::Fail
        },
        detail: if passed {
            "deterministic P6 reference capture passed native visual thresholds".into()
        } else {
            format!(
                "deterministic P6 reference capture failed: {}",
                visual.body.failure_reasons.join("; ")
            )
        },
        measured_artifact_id: payload.visual_evidence_artifact_id,
        metric_id: intake::MetricId::VisualSimilarityScore,
        metric_value: coverage,
        failure_code: if passed {
            None
        } else {
            Some("reference_visual_gate_failed".into())
        },
    })
}

fn validate_visual_quality(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
) -> Result<DomainVerdict, AuthorityError> {
    let payload: NativeVisualQualityReceiptPayload =
        parse_payload(envelope, "native visual-quality")?;
    let world_bytes = bound_artifact(envelope, candidate, &payload.world_artifact_id, WORLD_KIND)?;
    let packet_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.packet_artifact_id,
        NATIVE_GRAPHICS_PACKET_KIND,
    )?;
    let frame_receipt_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.frame_receipt_artifact_id,
        NATIVE_GRAPHICS_FRAME_RECEIPT_KIND,
    )?;
    let renderer_attestation_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.renderer_attestation_artifact_id,
        NATIVE_GRAPHICS_RENDERER_ATTESTATION_KIND,
    )?;
    let capture_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.capture_artifact_id,
        NATIVE_RGBA8_CAPTURE_KIND,
    )?;
    let quality_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.visual_quality_evidence_artifact_id,
        NATIVE_VISUAL_QUALITY_EVIDENCE_KIND,
    )?;
    if envelope.receipt_schema != NATIVE_VISUAL_QUALITY_RECEIPT_SCHEMA {
        return Err(AuthorityError::new(
            "unregistered",
            "native visual-quality receipt schema differs from the registered contract",
        ));
    }

    let world: runtime::WorldArtifact = parse_artifact(world_bytes, "native visual world")?;
    runtime::validate_world_artifact(&world).map_err(runtime_error)?;
    let packet: native_graphics::GraphicsScenePacket =
        parse_artifact(packet_bytes, "native graphics scene packet")?;
    native_graphics::validate_scene_packet(&packet).map_err(native_graphics_error)?;
    if packet.body.world_artifact_id != world.artifact_id
        || packet.body.world_artifact_sha256 != world.artifact_sha256
        || packet.body.spatial_fields_sha256 != world.body.fields.spatial_sha256
    {
        return Err(AuthorityError::new(
            "provenance",
            "native graphics packet is detached from the bound typed world or spatial fields",
        ));
    }
    let frame_receipt: native_graphics::GraphicsFrameReceipt =
        parse_artifact(frame_receipt_bytes, "native graphics frame receipt")?;
    let renderer_attestation: native_graphics::GraphicsRendererAttestation = parse_artifact(
        renderer_attestation_bytes,
        "native graphics renderer attestation",
    )?;
    native_graphics::validate_renderer_attestation(&renderer_attestation, &frame_receipt)
        .map_err(native_graphics_error)?;
    let evidence: native_graphics::VisualQualityEvidence =
        parse_artifact(quality_bytes, "native visual-quality evidence")?;
    native_graphics::validate_visual_quality_evidence(
        &evidence,
        &packet,
        &frame_receipt,
        capture_bytes,
    )
    .map_err(native_graphics_error)?;

    require_exact_evidence(
        envelope,
        &BTreeMap::from([
            (payload.world_artifact_id.clone(), WORLD_KIND.to_owned()),
            (
                payload.packet_artifact_id.clone(),
                NATIVE_GRAPHICS_PACKET_KIND.to_owned(),
            ),
            (
                payload.frame_receipt_artifact_id.clone(),
                NATIVE_GRAPHICS_FRAME_RECEIPT_KIND.to_owned(),
            ),
            (
                payload.renderer_attestation_artifact_id.clone(),
                NATIVE_GRAPHICS_RENDERER_ATTESTATION_KIND.to_owned(),
            ),
            (
                payload.capture_artifact_id.clone(),
                NATIVE_RGBA8_CAPTURE_KIND.to_owned(),
            ),
            (
                payload.visual_quality_evidence_artifact_id.clone(),
                NATIVE_VISUAL_QUALITY_EVIDENCE_KIND.to_owned(),
            ),
        ]),
    )?;

    let (status, detail, failure_code) = match evidence.body.outcome {
        native_graphics::QualityOutcome::Good => (
            ReceiptStatus::Pass,
            format!(
                "native visual-quality profile {} independently remeasured Good",
                evidence.body.profile.profile_id
            ),
            None,
        ),
        native_graphics::QualityOutcome::Bad => (
            ReceiptStatus::Fail,
            format!(
                "native visual-quality profile {} independently remeasured Bad: {}",
                evidence.body.profile.profile_id,
                evidence
                    .body
                    .reasons
                    .iter()
                    .map(|reason| format!("{:?}", reason.code))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Some("native_visual_quality_failed".into()),
        ),
        native_graphics::QualityOutcome::Failed => (
            ReceiptStatus::Fail,
            format!(
                "native visual-quality assessment deterministically Failed: {}",
                evidence
                    .body
                    .reasons
                    .iter()
                    .map(|reason| format!("{:?}", reason.code))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Some("native_visual_quality_assessment_failed".into()),
        ),
        native_graphics::QualityOutcome::Indeterminate => (
            ReceiptStatus::Indeterminate,
            format!(
                "native visual-quality assessment remains Indeterminate: {}",
                evidence
                    .body
                    .reasons
                    .iter()
                    .map(|reason| format!("{:?}", reason.code))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None,
        ),
    };
    let coverage = evidence
        .body
        .measurements
        .as_ref()
        .map_or(0.0, |measurements| {
            f64::from(measurements.region_coverage_bp) / 10_000.0
        });
    Ok(DomainVerdict {
        status,
        detail,
        measured_artifact_id: payload.visual_quality_evidence_artifact_id,
        metric_id: intake::MetricId::TechnicalVisualQuality,
        metric_value: coverage,
        failure_code,
    })
}

fn validate_asset(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
) -> Result<DomainVerdict, AuthorityError> {
    let payload: AssetReceiptPayload = parse_payload(envelope, "asset")?;
    let source_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.source_artifact_id,
        "static_mesh_source",
    )?;
    let package_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.package_artifact_id,
        "asset_package",
    )?;
    require_exact_evidence(
        envelope,
        &BTreeMap::from([
            (
                payload.source_artifact_id.clone(),
                "static_mesh_source".to_owned(),
            ),
            (
                payload.package_artifact_id.clone(),
                "asset_package".to_owned(),
            ),
        ]),
    )?;
    let source_digest = sha256_prefixed(source_bytes);
    if source_digest == crate::SUPPLIED_BAD_GLB_SHA256 {
        return Ok(DomainVerdict {
            status: ReceiptStatus::Fail,
            detail: "permanent negative-control GLB is rejected".into(),
            measured_artifact_id: payload.source_artifact_id,
            metric_id: intake::MetricId::MissingAssetFeatures,
            metric_value: 1.0,
            failure_code: Some("permanent_bad_glb_control".into()),
        });
    }
    let source: StaticMeshSource = parse_artifact(source_bytes, "static mesh source")?;
    let package: StaticAssetPackage = parse_artifact(package_bytes, "prepared asset package")?;
    if source.schema_version != "wge.static-mesh-source/v1"
        || package.schema_version != "wge.asset-package/v1"
        || source.asset_id != package.asset_id
        || package.source_sha256 != source_digest
        || package.asset_use != payload.asset_use
    {
        return Err(AuthorityError::new(
            "provenance",
            "asset package schema, source digest, identity, or use is mismatched",
        ));
    }
    let facts = measure_mesh(&source)?;
    if package.bounds_min_m != facts.minimum
        || package.bounds_max_m != facts.maximum
        || !covers(
            package.collision_bounds_min_m,
            package.collision_bounds_max_m,
            facts.minimum,
            facts.maximum,
        )
        || package.material_slots != source.material_slots
        || package.lod_triangle_counts.first() != Some(&facts.triangle_count)
        || package.lod_triangle_counts.is_empty()
        || package.lod_triangle_counts.len() > 8
        || package
            .lod_triangle_counts
            .windows(2)
            .any(|pair| pair[1] >= pair[0])
    {
        return Err(AuthorityError::new(
            "contract",
            "asset collision, material, measured bounds, or LOD metadata failed",
        ));
    }
    let status = if payload.asset_use == AssetUse::Character {
        ReceiptStatus::Indeterminate
    } else {
        ReceiptStatus::Pass
    };
    Ok(DomainVerdict {
        status,
        detail: if status == ReceiptStatus::Pass {
            format!(
                "{} mesh triangles and preparation metadata remeasured",
                facts.triangle_count
            )
        } else {
            "asset geometry measured; rigging/skinning/retargeting remain deferred".into()
        },
        measured_artifact_id: payload.source_artifact_id,
        metric_id: intake::MetricId::MissingAssetFeatures,
        metric_value: 0.0,
        failure_code: None,
    })
}

fn validate_rigging(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
) -> Result<DomainVerdict, AuthorityError> {
    let payload: RiggingReceiptPayload = parse_payload(envelope, "rigging")?;
    let glb = bound_artifact(
        envelope,
        candidate,
        &payload.source_glb_artifact_id,
        "rigging_glb",
    )?;
    let request_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.preparation_request_artifact_id,
        "rigging_request",
    )?;
    let receipt_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.preparation_receipt_artifact_id,
        "rigging_preparation_receipt",
    )?;
    require_exact_evidence(
        envelope,
        &BTreeMap::from([
            (
                payload.source_glb_artifact_id.clone(),
                "rigging_glb".to_owned(),
            ),
            (
                payload.preparation_request_artifact_id.clone(),
                "rigging_request".to_owned(),
            ),
            (
                payload.preparation_receipt_artifact_id.clone(),
                "rigging_preparation_receipt".to_owned(),
            ),
        ]),
    )?;

    let request_value: serde_json::Value = parse_artifact(request_bytes, "rigging request")?;
    let request: asset_runtime::AssetPreparationRequest =
        parse_artifact(request_bytes, "typed rigging preparation request")?;
    if serde_json::to_value(&request).ok().as_ref() != Some(&request_value) {
        return Err(AuthorityError::new(
            "malformed",
            "rigging request contains fields outside the registered typed schema",
        ));
    }
    if request.asset_use != wge_asset_contract::AssetUse::Character || request.rig.is_none() {
        return Err(AuthorityError::new(
            "contract",
            "native rigging evidence requires a character request with a rig contract",
        ));
    }

    let supplied_value: serde_json::Value =
        parse_artifact(receipt_bytes, "asset preparation receipt")?;
    let supplied: asset_runtime::AssetPreparationReceipt =
        parse_artifact(receipt_bytes, "typed asset preparation receipt")?;
    if serde_json::to_value(&supplied).ok().as_ref() != Some(&supplied_value) {
        return Err(AuthorityError::new(
            "malformed",
            "asset preparation receipt contains fields outside its typed schema",
        ));
    }
    let measured = asset_runtime::prepare_asset(glb, &request).map_err(|error| {
        AuthorityError::new("contract", format!("GLB preparation failed: {error}"))
    })?;
    if supplied != measured {
        return Err(AuthorityError::new(
            "provenance",
            "candidate-bound preparation receipt differs from independent GLB/request revalidation",
        ));
    }
    if supplied.status == asset_runtime::PreparationStatus::Ready
        && (supplied.package.is_none() || !supplied.findings.is_empty())
    {
        return Err(AuthorityError::new(
            "contract",
            "ready rigging receipt must carry a prepared package and no findings",
        ));
    }
    let status = match supplied.status {
        asset_runtime::PreparationStatus::Ready => ReceiptStatus::Pass,
        asset_runtime::PreparationStatus::Rejected => ReceiptStatus::Fail,
    };
    Ok(DomainVerdict {
        status,
        detail: match &supplied.package {
            Some(package) => format!(
                "runtime preparation {} independently revalidated: {} joints, {} clips, {} sockets",
                package.package_id,
                package.rig.as_ref().map_or(0, |rig| rig.joint_names.len()),
                package.animations.len(),
                package.sockets.len()
            ),
            None => format!(
                "GLB failed native rigging preparation with {} independently revalidated findings",
                supplied.findings.len()
            ),
        },
        measured_artifact_id: payload.preparation_receipt_artifact_id,
        metric_id: intake::MetricId::MissingAssetFeatures,
        metric_value: supplied.findings.len() as f64,
        failure_code: (status == ReceiptStatus::Fail).then(|| "rigging_preparation_failed".into()),
    })
}

fn validate_repair(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
    candidates: &BTreeMap<&str, &CandidateContext>,
    validated: &BTreeMap<String, ValidatedReceipt>,
) -> Result<DomainVerdict, AuthorityError> {
    let payload: RepairReceiptPayload = parse_payload(envelope, "repair")?;
    let proposal_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.proposal_artifact_id,
        "repair_proposal",
    )?;
    let draft_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.delta_draft_artifact_id,
        "repair_delta",
    )?;
    let delta_bytes = bound_artifact(
        envelope,
        candidate,
        &payload.delta_artifact_id,
        "repair_record",
    )?;
    require_exact_evidence(
        envelope,
        &BTreeMap::from([
            (
                payload.proposal_artifact_id.clone(),
                "repair_proposal".to_owned(),
            ),
            (
                payload.delta_draft_artifact_id.clone(),
                "repair_delta".to_owned(),
            ),
            (
                payload.delta_artifact_id.clone(),
                "repair_record".to_owned(),
            ),
        ]),
    )?;
    let raw_proposal: intake::RepairProposal =
        parse_artifact(proposal_bytes, "typed repair proposal")?;
    let raw_draft: intake::RepairEvidenceDeltaDraft =
        parse_artifact(draft_bytes, "typed repair delta draft")?;
    let raw_delta: intake::RepairEvidenceDelta = parse_artifact(delta_bytes, "typed repair delta")?;
    if raw_proposal != payload.proposal
        || raw_draft != payload.delta_draft
        || raw_delta != payload.delta
    {
        return Err(AuthorityError::new(
            "provenance",
            "repair payload differs from its exact candidate-bound typed proposal/delta artifacts",
        ));
    }
    let before_candidate = candidates
        .get(payload.before_snapshot_id.as_str())
        .copied()
        .ok_or_else(|| AuthorityError::new("provenance", "repair before snapshot is absent"))?;
    if before_candidate.snapshot_id == candidate.snapshot_id
        || before_candidate.project_id != candidate.project_id
        || before_candidate.candidate_sha256 == candidate.candidate_sha256
    {
        return Err(AuthorityError::new(
            "provenance",
            "repair must cross distinct snapshots of the same project",
        ));
    }
    let before = validated.get(&payload.before_receipt_id).ok_or_else(|| {
        AuthorityError::new(
            "provenance",
            "repair before receipt was not independently validated",
        )
    })?;
    let after = validated.get(&payload.after_receipt_id).ok_or_else(|| {
        AuthorityError::new(
            "provenance",
            "repair after receipt was not independently validated",
        )
    })?;
    if before.envelope.snapshot_id != before_candidate.snapshot_id
        || after.envelope.snapshot_id != candidate.snapshot_id
        || before.envelope.gate_id != after.envelope.gate_id
        || before.envelope.status != ReceiptStatus::Fail
    {
        return Err(AuthorityError::new(
            "provenance",
            "repair receipts do not prove a historical failure and current same-gate remeasurement",
        ));
    }
    let proposal = &payload.proposal;
    let draft = &payload.delta_draft;
    intake::validate_repair_proposal(proposal).map_err(contract_error)?;
    intake::validate_delta_identity(&payload.delta).map_err(contract_error)?;
    if proposal.candidate_before_sha256 != before_candidate.candidate_sha256
        || draft.candidate_after_sha256 != candidate.candidate_sha256
        || draft.before_evidence.receipt_sha256 != sha256_prefixed(&before.native_bytes)
        || draft.after_evidence.receipt_sha256 != sha256_prefixed(&after.native_bytes)
        || proposal.failure_evidence.receipt_sha256 != sha256_prefixed(&before.native_bytes)
        || draft.proposal_id != proposal.proposal_id
    {
        return Err(AuthorityError::new(
            "provenance",
            "repair proposal/delta candidate or raw native receipt binding is stale",
        ));
    }
    let targets = proposal
        .authorized_targets
        .iter()
        .map(|target| target.artifact_id.as_str())
        .collect::<BTreeSet<_>>();
    if !targets.is_subset(
        &candidate
            .authorized_repair_artifact_ids
            .iter()
            .map(String::as_str)
            .collect(),
    ) {
        return Err(AuthorityError::new(
            "policy",
            "repair targets exceed candidate-authorized artifact IDs",
        ));
    }
    let actual_changes = content_artifact_changes(before_candidate, candidate)?;
    let declared_changes = draft
        .changed_artifacts
        .iter()
        .map(|change| change.artifact_id.as_str())
        .collect::<BTreeSet<_>>();
    let actual_ids = actual_changes
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if declared_changes != actual_ids {
        return Err(AuthorityError::new(
            "provenance",
            "repair delta changed_artifacts must exactly equal the before/after content artifact change set",
        ));
    }
    // A native visual-quality repair carries the packet, promoted frame,
    // capture, and independently remeasured evidence alongside the ordinary
    // world/runtime bundle. Keep the bound finite while allowing that typed
    // verification bundle to participate in the before/after delta.
    const MAX_REPAIR_ARTIFACT_CHANGES: usize = 12;
    if actual_changes.len() > MAX_REPAIR_ARTIFACT_CHANGES {
        return Err(AuthorityError::new(
            "policy",
            "repair change count exceeds the certification hard limit of twelve artifacts",
        ));
    }
    if actual_changes.iter().any(|id| {
        !targets.contains(id.as_str()) || !candidate.authorized_repair_artifact_ids.contains(id)
    }) {
        return Err(AuthorityError::new(
            "policy",
            "every changed content artifact must be an authorized proposal target and candidate-authorized repair ID",
        ));
    }
    let before_artifacts = candidate_bytes(before_candidate);
    let after_artifacts = candidate_bytes(candidate);
    let native_registry = repair_native_registry()?;
    let recomputed = intake::validate_repair_delta(
        proposal,
        draft.clone(),
        &crate::candidate_identity_bytes(before_candidate)?,
        &crate::candidate_identity_bytes(candidate)?,
        &before.native_bytes,
        &after.native_bytes,
        &before_artifacts,
        &after_artifacts,
        &native_registry,
    )
    .map_err(contract_error)?;
    if recomputed != payload.delta {
        return Err(AuthorityError::new(
            "provenance",
            "repair delta differs from independently recomputed before/after evidence",
        ));
    }
    Ok(DomainVerdict {
        status: ReceiptStatus::Pass,
        detail: format!(
            "{:?} repair: {}",
            recomputed.assessment, recomputed.explanation
        ),
        measured_artifact_id: proposal
            .authorized_targets
            .first()
            .map(|target| target.artifact_id.clone())
            .unwrap_or_default(),
        metric_id: recomputed
            .metric_deltas
            .first()
            .map(|item| item.metric_id)
            .unwrap_or(intake::MetricId::VisualSimilarityScore),
        metric_value: 1.0,
        failure_code: None,
    })
}

/// Compute changed content IDs by the exact before/after union. Repair
/// proposal/delta/record artifacts are excluded by the same documented rule as
/// candidate identity, otherwise the evidence describing a repair would itself
/// become a repair target. A kind-only change is detected but rejected because
/// the upstream typed delta schema expresses byte digests only.
fn content_artifact_changes(
    before: &CandidateContext,
    after: &CandidateContext,
) -> Result<Vec<String>, AuthorityError> {
    let ids = before
        .artifacts
        .keys()
        .chain(after.artifacts.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut changed = Vec::new();
    for id in ids {
        let old = before.artifacts.get(&id);
        let new = after.artifacts.get(&id);
        if old
            .is_some_and(|item| crate::REPAIR_IDENTITY_EXCLUDED_KINDS.contains(&item.kind.as_str()))
            || new.is_some_and(|item| {
                crate::REPAIR_IDENTITY_EXCLUDED_KINDS.contains(&item.kind.as_str())
            })
        {
            continue;
        }
        let old_digest = old.map(|item| sha256_prefixed(&item.bytes));
        let new_digest = new.map(|item| sha256_prefixed(&item.bytes));
        let old_kind = old.map(|item| item.kind.as_str());
        let new_kind = new.map(|item| item.kind.as_str());
        if old_digest != new_digest || old_kind != new_kind {
            if old.is_none() || new.is_none() {
                return Err(AuthorityError::new(
                    "policy",
                    format!(
                        "repair delta schema cannot represent artifact creation/removal for {id}"
                    ),
                ));
            }
            if old_digest == new_digest {
                return Err(AuthorityError::new(
                    "policy",
                    format!("repair delta schema cannot certify kind-only change for {id}"),
                ));
            }
            changed.push(id);
        }
    }
    Ok(changed)
}

fn validate_deferred(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
) -> Result<DomainVerdict, AuthorityError> {
    if envelope.status != ReceiptStatus::Indeterminate {
        return Err(AuthorityError::new(
            "policy",
            "deferred rigging evidence cannot be promoted to pass or fail",
        ));
    }
    let payload: DeferredReceiptPayload = parse_payload(envelope, "deferred")?;
    if payload.reason_code != "deferred_by_scope"
        || payload.deferral_scope != envelope.gate_id
        || payload.detail.trim().is_empty()
    {
        return Err(AuthorityError::new(
            "policy",
            "deferred receipt must name its explicit gate scope",
        ));
    }
    if envelope.evidence.len() != 1 {
        return Err(AuthorityError::new(
            "provenance",
            "deferred receipt must bind one candidate artifact",
        ));
    }
    let binding = &envelope.evidence[0];
    let artifact = candidate
        .artifacts
        .get(&binding.artifact_id)
        .ok_or_else(|| AuthorityError::new("provenance", "deferred evidence artifact is absent"))?;
    if artifact.kind != binding.kind
        || !matches!(
            binding.kind.as_str(),
            "source_asset" | "project_manifest" | "project_spec"
        )
    {
        return Err(AuthorityError::new(
            "provenance",
            "deferred evidence is not bound to candidate inputs",
        ));
    }
    Ok(DomainVerdict {
        status: ReceiptStatus::Indeterminate,
        detail: payload.detail,
        measured_artifact_id: binding.artifact_id.clone(),
        metric_id: intake::MetricId::ReachableObjectives,
        metric_value: 0.0,
        failure_code: None,
    })
}

fn load_runtime_evidence<'a>(
    envelope: &ReceiptEnvelope,
    candidate: &'a CandidateContext,
    world_id: &str,
    traversal_id: &str,
    capture_id: &str,
    visual_id: &str,
    binding_id: &str,
) -> Result<
    (
        runtime::WorldArtifact,
        runtime::TraversalEvidence,
        &'a [u8],
        runtime::VisualEvidence,
        runtime::GameplayWorldBinding,
    ),
    AuthorityError,
> {
    let world_bytes = bound_artifact(envelope, candidate, world_id, WORLD_KIND)?;
    let traversal_bytes = bound_artifact(envelope, candidate, traversal_id, TRAVERSAL_KIND)?;
    let capture = bound_artifact(envelope, candidate, capture_id, CAPTURE_KIND)?;
    let visual_bytes = bound_artifact(envelope, candidate, visual_id, VISUAL_KIND)?;
    let binding_bytes = bound_artifact(envelope, candidate, binding_id, GAMEPLAY_BINDING_KIND)?;
    Ok((
        parse_artifact(world_bytes, "world artifact")?,
        parse_artifact(traversal_bytes, "traversal evidence")?,
        capture,
        parse_artifact(visual_bytes, "visual evidence")?,
        parse_artifact(binding_bytes, "gameplay world binding")?,
    ))
}

fn runtime_evidence_set(
    world: &str,
    traversal: &str,
    capture: &str,
    visual: &str,
    gameplay: Option<&str>,
) -> BTreeMap<String, String> {
    let mut set = BTreeMap::from([
        (world.to_owned(), WORLD_KIND.to_owned()),
        (traversal.to_owned(), TRAVERSAL_KIND.to_owned()),
        (capture.to_owned(), CAPTURE_KIND.to_owned()),
        (visual.to_owned(), VISUAL_KIND.to_owned()),
    ]);
    if let Some(gameplay) = gameplay {
        set.insert(gameplay.to_owned(), GAMEPLAY_BINDING_KIND.to_owned());
    }
    set
}

fn parse_payload<T: for<'de> serde::Deserialize<'de>>(
    envelope: &ReceiptEnvelope,
    label: &str,
) -> Result<T, AuthorityError> {
    serde_json::from_value(envelope.payload.clone()).map_err(|error| {
        AuthorityError::new("malformed", format!("{label} receipt payload: {error}"))
    })
}

fn bound_artifact<'a>(
    envelope: &ReceiptEnvelope,
    candidate: &'a CandidateContext,
    id: &str,
    kind: &str,
) -> Result<&'a [u8], AuthorityError> {
    let binding = envelope
        .evidence
        .iter()
        .find(|item| item.artifact_id == id && item.kind == kind)
        .ok_or_else(|| {
            AuthorityError::new(
                "provenance",
                format!("receipt does not bind required {kind} artifact {id}"),
            )
        })?;
    let artifact = candidate
        .artifacts
        .get(id)
        .ok_or_else(|| AuthorityError::new("provenance", format!("artifact {id} is absent")))?;
    if artifact.kind != kind || sha256_prefixed(&artifact.bytes) != binding.sha256 {
        return Err(AuthorityError::new(
            "provenance",
            format!("artifact {id} bytes or kind differ from evidence binding"),
        ));
    }
    Ok(&artifact.bytes)
}

fn require_exact_evidence(
    envelope: &ReceiptEnvelope,
    expected: &BTreeMap<String, String>,
) -> Result<(), AuthorityError> {
    let actual = envelope
        .evidence
        .iter()
        .map(|item| (item.artifact_id.clone(), item.kind.clone()))
        .collect::<BTreeMap<_, _>>();
    if &actual != expected {
        return Err(AuthorityError::new(
            "provenance",
            format!(
                "{} evidence set differs from its native schema",
                envelope.gate_id
            ),
        ));
    }
    Ok(())
}

fn runtime_error(error: runtime::ReferenceRuntimeError) -> AuthorityError {
    AuthorityError::new("contract", error.to_string())
}

fn native_graphics_error(error: native_graphics::GraphicsContractError) -> AuthorityError {
    AuthorityError::new("contract", error.to_string())
}

fn contract_error(error: intake::ContractError) -> AuthorityError {
    AuthorityError::new("contract", error.to_string())
}

fn candidate_bytes(candidate: &CandidateContext) -> BTreeMap<String, Vec<u8>> {
    candidate
        .artifacts
        .iter()
        .map(|(id, artifact)| (id.clone(), artifact.bytes.clone()))
        .collect()
}

fn repair_native_registry() -> Result<intake::NativeValidatorRegistry, AuthorityError> {
    crate::repair_validator_registry()
}

fn measure_mesh(source: &StaticMeshSource) -> Result<MeshFacts, AuthorityError> {
    if source.positions_m.len() < 3
        || source.triangle_indices.is_empty()
        || !source.triangle_indices.len().is_multiple_of(3)
        || source.material_slots.is_empty()
    {
        return Err(AuthorityError::new(
            "contract",
            "mesh needs vertices, triangles, and material slots",
        ));
    }
    if source
        .positions_m
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
        || source
            .triangle_indices
            .iter()
            .any(|index| *index as usize >= source.positions_m.len())
    {
        return Err(AuthorityError::new(
            "contract",
            "mesh contains nonfinite coordinates or invalid indices",
        ));
    }
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for point in &source.positions_m {
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(point[axis]);
            maximum[axis] = maximum[axis].max(point[axis]);
        }
    }
    for tri in source.triangle_indices.chunks_exact(3) {
        let a = source.positions_m[tri[0] as usize];
        let b = source.positions_m[tri[1] as usize];
        let c = source.positions_m[tri[2] as usize];
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        if cross.iter().map(|value| value * value).sum::<f64>() <= 1.0e-16 {
            return Err(AuthorityError::new(
                "contract",
                "mesh contains degenerate triangle",
            ));
        }
    }
    if (0..3).any(|axis| maximum[axis] <= minimum[axis]) {
        return Err(AuthorityError::new(
            "contract",
            "mesh bounds have zero extent",
        ));
    }
    Ok(MeshFacts {
        minimum,
        maximum,
        triangle_count: source.triangle_indices.len() / 3,
    })
}

struct MeshFacts {
    minimum: [f64; 3],
    maximum: [f64; 3],
    triangle_count: usize,
}

fn covers(
    minimum: [f64; 3],
    maximum: [f64; 3],
    required_minimum: [f64; 3],
    required_maximum: [f64; 3],
) -> bool {
    (0..3).all(|axis| {
        minimum[axis].is_finite()
            && maximum[axis].is_finite()
            && minimum[axis] <= required_minimum[axis]
            && maximum[axis] >= required_maximum[axis]
    })
}
