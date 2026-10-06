use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use luxel_intake_repair_contract::{
    ContractError, EvidenceReference, FailedLayer, GateId, IntakeDraft, MetricId,
    MetricObservation, NativeFinding, NativeValidatorRegistry, ProviderInterpretation,
    ProviderProvenance, ReceiptOutcome, RepairAssessment, RepairEditClass,
    RepairEvidenceDeltaDraft, RepairProposalDraft, RepairTarget, SemanticIntake, SourceBundle,
    SourceBundleDraft, ValidatedNativeReceipt, normalize_intake, normalize_repair_proposal,
    parse_json, prepare_source_bundle, sha256_prefixed, validate_intake,
    validate_proposal_evidence, validate_repair_delta, validate_repair_proposal,
    validate_source_bundle,
};

const TEST_VALIDATOR: &str = "test.native.navigation";
const TEST_RECEIPT_SCHEMA: &str = "test.navigation-receipt/v1";

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name),
    )
    .unwrap()
}

fn prepared_bundle() -> (SourceBundle, BTreeMap<String, Vec<u8>>) {
    let draft: SourceBundleDraft = parse_json(&fixture("intake/source-bundle-draft.json")).unwrap();
    let mut raw_by_ref = BTreeMap::new();
    raw_by_ref.insert("brief".to_owned(), fixture("intake/brief.txt"));
    raw_by_ref.insert("concept".to_owned(), fixture("intake/concept.svg"));
    let bundle = prepare_source_bundle(draft, &raw_by_ref).unwrap();
    let expected: SourceBundle = parse_json(&fixture("intake/source-bundle.json")).unwrap();
    assert_eq!(bundle, expected);
    let by_id = bundle
        .sources
        .iter()
        .map(|source| {
            let bytes = if source.media_type == "text/plain" {
                fixture("intake/brief.txt")
            } else {
                fixture("intake/concept.svg")
            };
            (source.source_id.clone(), bytes)
        })
        .collect();
    (bundle, by_id)
}

fn good_intake() -> (SemanticIntake, Vec<u8>, BTreeMap<String, Vec<u8>>) {
    let (bundle, source_bytes) = prepared_bundle();
    let response = fixture("intake/provider-response.json");
    let draft: IntakeDraft = parse_json(&fixture("intake/intake-draft.json")).unwrap();
    let intake = normalize_intake(draft, &bundle, &response, &source_bytes).unwrap();
    (intake, response, source_bytes)
}

#[test]
fn source_only_bundle_and_provider_interpretation_form_a_reproducible_intake() {
    let (intake, response, source_bytes) = good_intake();
    validate_intake(&intake, &response, &source_bytes).unwrap();
    assert_eq!(intake.observations.len(), 3);
    assert_eq!(intake.inferences.len(), 1);
    assert_eq!(intake.assumptions.len(), 1);
    assert!(intake.intake_id.starts_with("intake:sha256:"));
    let (bundle, _) = prepared_bundle();
    validate_source_bundle(&bundle, &source_bytes).unwrap();
}

#[test]
fn intake_rejects_old_status_reports_forged_claims_and_unknown_fields() {
    let (bundle, source_bytes) = prepared_bundle();
    assert!(parse_json::<ProviderInterpretation>(&fixture("intake/status-only.json")).is_err());
    assert!(parse_json::<IntakeDraft>(&fixture("intake/unknown-field.json")).is_err());

    let (mut generated_report, _, _) = good_intake();
    generated_report.schema_version = "luxel.semantic-intake/v1".into();
    let old_generated_report = serde_json::to_vec(&generated_report).unwrap();
    assert!(parse_json::<IntakeDraft>(&old_generated_report).is_err());

    let bad_response = fixture("intake/bad-observation.json");
    let interpretation: ProviderInterpretation = parse_json(&bad_response).unwrap();
    let draft = IntakeDraft {
        schema_version: "luxel.semantic-intake-draft/v1".into(),
        source_bundle_id: bundle.source_bundle_id.clone(),
        provider: ProviderProvenance {
            provider_id: "fixture.multimodal".into(),
            provider_version: "1.0".into(),
            protocol: "luxel.provider-interpretation/v1".into(),
            request_source_bundle_id: bundle.source_bundle_id.clone(),
            response_sha256: sha256_prefixed(&bad_response),
        },
        interpretation,
    };
    assert!(normalize_intake(draft, &bundle, &bad_response, &source_bytes).is_err());

    let (mut intake, response, source_bytes) = good_intake();
    let mut forged_identity = intake.clone();
    let final_digit = forged_identity.intake_id.pop().unwrap();
    forged_identity
        .intake_id
        .push(if final_digit == '0' { '1' } else { '0' });
    assert!(validate_intake(&forged_identity, &response, &source_bytes).is_err());

    intake.observations[0].statement.push_str(" forged");
    assert!(validate_intake(&intake, &response, &source_bytes).is_err());
}

#[test]
fn provider_observation_without_a_source_region_is_rejected() {
    let (bundle, source_bytes) = prepared_bundle();
    let response = fixture("intake/bad-observation.json");
    let interpretation: ProviderInterpretation = parse_json(&response).unwrap();
    let draft = IntakeDraft {
        schema_version: "luxel.semantic-intake-draft/v1".into(),
        source_bundle_id: bundle.source_bundle_id.clone(),
        provider: ProviderProvenance {
            provider_id: "fixture.multimodal".into(),
            provider_version: "1.0".into(),
            protocol: "luxel.provider-interpretation/v1".into(),
            request_source_bundle_id: bundle.source_bundle_id.clone(),
            response_sha256: sha256_prefixed(&response),
        },
        interpretation,
    };
    assert!(normalize_intake(draft, &bundle, &response, &source_bytes).is_err());
}

#[test]
fn intake_rejects_stale_bytes_and_mismatched_provider_response() {
    let (bundle, mut source_bytes) = prepared_bundle();
    let stale_source_id = bundle.sources[0].source_id.clone();
    source_bytes.get_mut(&stale_source_id).unwrap().push(0);
    assert!(validate_source_bundle(&bundle, &source_bytes).is_err());

    let (intake, response, source_bytes) = good_intake();
    let mut stale_response = response.clone();
    stale_response.push(b' ');
    assert!(validate_intake(&intake, &stale_response, &source_bytes).is_err());

    let mut fresh_draft: SourceBundleDraft =
        parse_json(&fixture("intake/source-bundle-draft.json")).unwrap();
    fresh_draft.request_id.push_str("-new-request");
    let raw_by_ref = BTreeMap::from([
        ("brief".to_owned(), fixture("intake/brief.txt")),
        ("concept".to_owned(), fixture("intake/concept.svg")),
    ]);
    let fresh_bundle = prepare_source_bundle(fresh_draft, &raw_by_ref).unwrap();
    let mut fresh_source_bytes = BTreeMap::new();
    for source in &fresh_bundle.sources {
        let bytes = if source.media_type == "text/plain" {
            fixture("intake/brief.txt")
        } else {
            fixture("intake/concept.svg")
        };
        fresh_source_bytes.insert(source.source_id.clone(), bytes);
    }
    let old_request_draft: IntakeDraft = parse_json(&fixture("intake/intake-draft.json")).unwrap();
    assert!(
        normalize_intake(
            old_request_draft,
            &fresh_bundle,
            &response,
            &fresh_source_bytes
        )
        .is_err()
    );

    let mut draft: IntakeDraft = parse_json(&fixture("intake/intake-draft.json")).unwrap();
    draft.interpretation.claims[0]
        .statement
        .push_str(" not in raw response");
    let (bundle, source_bytes) = prepared_bundle();
    assert!(normalize_intake(draft, &bundle, &response, &source_bytes).is_err());
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestReceipt {
    validator_id: String,
    schema_version: String,
    gate_id: GateId,
    candidate_sha256: String,
    artifact_sha256: String,
    outcome: ReceiptOutcome,
    findings: Vec<NativeFinding>,
    measurements: Vec<MetricObservation>,
}

fn test_native_validator(bytes: &[u8]) -> Result<ValidatedNativeReceipt, ContractError> {
    let receipt: TestReceipt = parse_json(bytes)?;
    if receipt.validator_id != TEST_VALIDATOR || receipt.schema_version != TEST_RECEIPT_SCHEMA {
        return Err(ContractError::Registry(
            "test receipt registration mismatch".into(),
        ));
    }
    Ok(ValidatedNativeReceipt {
        validator_id: receipt.validator_id,
        schema_version: receipt.schema_version,
        gate_id: receipt.gate_id,
        candidate_sha256: receipt.candidate_sha256,
        artifact_sha256: receipt.artifact_sha256,
        outcome: receipt.outcome,
        findings: receipt.findings,
        measurements: receipt.measurements,
    })
}

fn registry() -> NativeValidatorRegistry {
    let mut registry = NativeValidatorRegistry::new();
    registry
        .register(TEST_VALIDATOR, TEST_RECEIPT_SCHEMA, test_native_validator)
        .unwrap();
    registry
}

fn candidate_before() -> Vec<u8> {
    b"candidate-before".to_vec()
}
fn candidate_after() -> Vec<u8> {
    b"candidate-after".to_vec()
}
fn nav_before() -> Vec<u8> {
    b"navigation-before".to_vec()
}
fn nav_after() -> Vec<u8> {
    b"navigation-after".to_vec()
}

fn make_receipt(
    candidate: &[u8],
    artifact: &[u8],
    outcome: ReceiptOutcome,
    components: f64,
) -> Vec<u8> {
    let artifact_sha256 = sha256_prefixed(artifact);
    let findings = if outcome == ReceiptOutcome::Fail {
        vec![NativeFinding {
            code: "navigation.disconnected".into(),
            artifact_sha256: artifact_sha256.clone(),
            detail: "route does not connect spawn and objective".into(),
        }]
    } else {
        Vec::new()
    };
    let receipt = TestReceipt {
        validator_id: TEST_VALIDATOR.into(),
        schema_version: TEST_RECEIPT_SCHEMA.into(),
        gate_id: GateId::Navigation,
        candidate_sha256: sha256_prefixed(candidate),
        artifact_sha256,
        outcome,
        findings,
        measurements: vec![MetricObservation {
            metric_id: MetricId::NavigationComponents,
            value: components,
        }],
    };
    serde_json::to_vec(&receipt).unwrap()
}

fn evidence_ref(candidate: &[u8], receipt: &[u8]) -> EvidenceReference {
    EvidenceReference {
        validator_id: TEST_VALIDATOR.into(),
        schema_version: TEST_RECEIPT_SCHEMA.into(),
        gate_id: GateId::Navigation,
        candidate_sha256: sha256_prefixed(candidate),
        receipt_sha256: sha256_prefixed(receipt),
    }
}

fn proposal(before_receipt: &[u8]) -> luxel_intake_repair_contract::RepairProposal {
    let before = candidate_before();
    let target = nav_before();
    normalize_repair_proposal(RepairProposalDraft {
        schema_version: "luxel.repair-proposal-draft/v1".into(),
        candidate_before_sha256: sha256_prefixed(&before),
        failed_layer: FailedLayer::Navigation,
        failure_evidence: evidence_ref(&before, before_receipt),
        edit_class: RepairEditClass::ReconnectNavigation,
        authorized_targets: vec![RepairTarget {
            artifact_id: "world.navigation".into(),
            before_sha256: sha256_prefixed(&target),
        }],
        max_artifact_changes: 1,
        diagnosis: "Navigation components exceed the required connected route.".into(),
        rationale: "Reconnect the authored spawn-to-objective route and re-run traversal.".into(),
    })
    .unwrap()
}

fn delta_draft(
    proposal: &luxel_intake_repair_contract::RepairProposal,
    before_receipt: &[u8],
    after_receipt: &[u8],
    changed_artifact_id: &str,
) -> RepairEvidenceDeltaDraft {
    RepairEvidenceDeltaDraft {
        schema_version: "luxel.repair-evidence-delta-draft/v1".into(),
        proposal_id: proposal.proposal_id.clone(),
        candidate_before_sha256: sha256_prefixed(&candidate_before()),
        candidate_after_sha256: sha256_prefixed(&candidate_after()),
        before_evidence: evidence_ref(&candidate_before(), before_receipt),
        after_evidence: evidence_ref(&candidate_after(), after_receipt),
        changed_artifacts: vec![luxel_intake_repair_contract::ArtifactChange {
            artifact_id: changed_artifact_id.into(),
            before_sha256: sha256_prefixed(&nav_before()),
            after_sha256: sha256_prefixed(&nav_after()),
        }],
    }
}

#[test]
fn repair_registry_derives_improvement_from_raw_before_after_receipts() {
    let before_candidate = candidate_before();
    let after_candidate = candidate_after();
    let before_artifact = nav_before();
    let after_artifact = nav_after();
    let before_receipt = make_receipt(
        &before_candidate,
        &before_artifact,
        ReceiptOutcome::Fail,
        3.0,
    );
    let proposal = proposal(&before_receipt);
    let registry = registry();
    validate_proposal_evidence(&proposal, &before_candidate, &before_receipt, &registry).unwrap();

    let after_pass = make_receipt(&after_candidate, &after_artifact, ReceiptOutcome::Pass, 1.0);
    let delta = validate_repair_delta(
        &proposal,
        delta_draft(&proposal, &before_receipt, &after_pass, "world.navigation"),
        &before_candidate,
        &after_candidate,
        &before_receipt,
        &after_pass,
        &BTreeMap::from([("world.navigation".into(), before_artifact)]),
        &BTreeMap::from([("world.navigation".into(), after_artifact)]),
        &registry,
    )
    .unwrap();
    assert_eq!(delta.assessment, RepairAssessment::GatePassed);
    assert_eq!(delta.before_outcome, ReceiptOutcome::Fail);
    assert_eq!(delta.after_outcome, ReceiptOutcome::Pass);
    assert!(delta.explanation.contains("now passes"));
}

#[test]
fn non_improving_repair_is_reported_as_still_failing() {
    let before_candidate = candidate_before();
    let after_candidate = candidate_after();
    let before_artifact = nav_before();
    let after_artifact = nav_after();
    assert_ne!(before_candidate, after_candidate);
    assert_ne!(before_artifact, after_artifact);
    let before_receipt = make_receipt(
        &before_candidate,
        &before_artifact,
        ReceiptOutcome::Fail,
        3.0,
    );
    let after_receipt = make_receipt(&after_candidate, &after_artifact, ReceiptOutcome::Fail, 3.0);
    let proposal = proposal(&before_receipt);
    let delta = validate_repair_delta(
        &proposal,
        delta_draft(
            &proposal,
            &before_receipt,
            &after_receipt,
            "world.navigation",
        ),
        &before_candidate,
        &after_candidate,
        &before_receipt,
        &after_receipt,
        &BTreeMap::from([("world.navigation".into(), before_artifact)]),
        &BTreeMap::from([("world.navigation".into(), after_artifact)]),
        &registry(),
    )
    .unwrap();
    assert_eq!(delta.assessment, RepairAssessment::StillFailing);
    assert_eq!(delta.before_outcome, ReceiptOutcome::Fail);
    assert_eq!(delta.after_outcome, ReceiptOutcome::Fail);
}

#[test]
fn repair_rejects_unchanged_candidate_or_artifact_bytes() {
    let before_candidate = candidate_before();
    let after_candidate = candidate_after();
    let before_artifact = nav_before();
    let before_receipt = make_receipt(
        &before_candidate,
        &before_artifact,
        ReceiptOutcome::Fail,
        3.0,
    );
    let proposal = proposal(&before_receipt);
    let registry = registry();

    let after_receipt = make_receipt(
        &after_candidate,
        &before_artifact,
        ReceiptOutcome::Fail,
        3.0,
    );
    let mut unchanged_candidate = delta_draft(
        &proposal,
        &before_receipt,
        &after_receipt,
        "world.navigation",
    );
    unchanged_candidate.candidate_after_sha256 = sha256_prefixed(&before_candidate);
    unchanged_candidate.after_evidence.candidate_sha256 = sha256_prefixed(&before_candidate);
    assert!(
        validate_repair_delta(
            &proposal,
            unchanged_candidate,
            &before_candidate,
            &before_candidate,
            &before_receipt,
            &after_receipt,
            &BTreeMap::from([("world.navigation".into(), before_artifact.clone())]),
            &BTreeMap::from([("world.navigation".into(), before_artifact.clone())]),
            &registry,
        )
        .is_err()
    );

    let mut unchanged_artifact = delta_draft(
        &proposal,
        &before_receipt,
        &after_receipt,
        "world.navigation",
    );
    unchanged_artifact.changed_artifacts[0].after_sha256 = sha256_prefixed(&before_artifact);
    assert!(
        validate_repair_delta(
            &proposal,
            unchanged_artifact,
            &before_candidate,
            &after_candidate,
            &before_receipt,
            &after_receipt,
            &BTreeMap::from([("world.navigation".into(), before_artifact.clone())]),
            &BTreeMap::from([("world.navigation".into(), before_artifact)]),
            &registry,
        )
        .is_err()
    );
}

#[test]
fn repair_reports_partial_metric_improvement_without_claiming_a_pass() {
    let before_candidate = candidate_before();
    let after_candidate = candidate_after();
    let before_artifact = nav_before();
    let after_artifact = nav_after();
    let before_receipt = make_receipt(
        &before_candidate,
        &before_artifact,
        ReceiptOutcome::Fail,
        3.0,
    );
    let after_receipt = make_receipt(&after_candidate, &after_artifact, ReceiptOutcome::Fail, 2.0);
    let proposal = proposal(&before_receipt);
    let delta = validate_repair_delta(
        &proposal,
        delta_draft(
            &proposal,
            &before_receipt,
            &after_receipt,
            "world.navigation",
        ),
        &before_candidate,
        &after_candidate,
        &before_receipt,
        &after_receipt,
        &BTreeMap::from([("world.navigation".into(), before_artifact)]),
        &BTreeMap::from([("world.navigation".into(), after_artifact)]),
        &registry(),
    )
    .unwrap();
    assert_eq!(delta.assessment, RepairAssessment::ImprovedStillFailing);
    assert_eq!(delta.after_outcome, ReceiptOutcome::Fail);
}

#[test]
fn repair_rejects_unauthorized_edits_unknown_or_forged_receipts_and_status_only() {
    let before_candidate = candidate_before();
    let before_artifact = nav_before();
    let before_receipt = make_receipt(
        &before_candidate,
        &before_artifact,
        ReceiptOutcome::Fail,
        3.0,
    );
    let mut unauthorized = RepairProposalDraft {
        schema_version: "luxel.repair-proposal-draft/v1".into(),
        candidate_before_sha256: sha256_prefixed(&before_candidate),
        failed_layer: FailedLayer::Navigation,
        failure_evidence: evidence_ref(&before_candidate, &before_receipt),
        edit_class: RepairEditClass::AdjustGameplayRule,
        authorized_targets: vec![RepairTarget {
            artifact_id: "world.navigation".into(),
            before_sha256: sha256_prefixed(&before_artifact),
        }],
        max_artifact_changes: 1,
        diagnosis: "bad scope".into(),
        rationale: "not allowed".into(),
    };
    assert!(normalize_repair_proposal(unauthorized.clone()).is_err());
    unauthorized.edit_class = RepairEditClass::ReconnectNavigation;
    let proposal = normalize_repair_proposal(unauthorized).unwrap();
    let registry = registry();

    let mut forged_proposal = proposal.clone();
    forged_proposal.proposal_id.push('0');
    assert!(validate_repair_proposal(&forged_proposal).is_err());

    let unknown = NativeValidatorRegistry::new();
    assert!(
        validate_proposal_evidence(&proposal, &before_candidate, &before_receipt, &unknown)
            .is_err()
    );

    let mut forged_receipt: TestReceipt = parse_json(&before_receipt).unwrap();
    forged_receipt.candidate_sha256 = sha256_prefixed(b"another candidate");
    let forged_bytes = serde_json::to_vec(&forged_receipt).unwrap();
    let forged_reference = evidence_ref(&before_candidate, &forged_bytes);
    let mut forged_draft = proposal.clone();
    forged_draft.failure_evidence = forged_reference;
    forged_draft.proposal_id.clear();
    // The proposal is re-sealed structurally, but the registered receipt parser
    // still rejects the candidate binding against the supplied candidate bytes.
    let resealed = normalize_repair_proposal(RepairProposalDraft {
        schema_version: "luxel.repair-proposal-draft/v1".into(),
        candidate_before_sha256: forged_draft.candidate_before_sha256,
        failed_layer: forged_draft.failed_layer,
        failure_evidence: forged_draft.failure_evidence,
        edit_class: forged_draft.edit_class,
        authorized_targets: forged_draft.authorized_targets,
        max_artifact_changes: forged_draft.max_artifact_changes,
        diagnosis: forged_draft.diagnosis,
        rationale: forged_draft.rationale,
    })
    .unwrap();
    assert!(
        validate_proposal_evidence(&resealed, &before_candidate, &forged_bytes, &registry).is_err()
    );

    let status_only = br#"{"validator_id":"test.native.navigation","schema_version":"test.navigation-receipt/v1","gate_id":"navigation","candidate_sha256":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","artifact_sha256":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","outcome":"pass"}"#;
    assert!(
        validate_proposal_evidence(&proposal, &before_candidate, status_only, &registry).is_err()
    );

    // Even with a matching raw digest and a registered validator, a pass-shaped
    // status object cannot substitute for the validator's required receipt schema.
    let bound_status_only = format!(
        r#"{{"validator_id":"{TEST_VALIDATOR}","schema_version":"{TEST_RECEIPT_SCHEMA}","gate_id":"navigation","candidate_sha256":"{}","artifact_sha256":"{}","outcome":"pass"}}"#,
        sha256_prefixed(&before_candidate),
        sha256_prefixed(&before_artifact),
    );
    let status_only_draft = RepairProposalDraft {
        schema_version: "luxel.repair-proposal-draft/v1".into(),
        candidate_before_sha256: sha256_prefixed(&before_candidate),
        failed_layer: FailedLayer::Navigation,
        failure_evidence: evidence_ref(&before_candidate, bound_status_only.as_bytes()),
        edit_class: RepairEditClass::ReconnectNavigation,
        authorized_targets: vec![RepairTarget {
            artifact_id: "world.navigation".into(),
            before_sha256: sha256_prefixed(&before_artifact),
        }],
        max_artifact_changes: 1,
        diagnosis: "A status-only receipt omits native measurements.".into(),
        rationale: "Only registered native receipt evidence may authorize repair.".into(),
    };
    let status_only_proposal = normalize_repair_proposal(status_only_draft).unwrap();
    assert!(
        validate_proposal_evidence(
            &status_only_proposal,
            &before_candidate,
            bound_status_only.as_bytes(),
            &registry,
        )
        .is_err()
    );
}

#[test]
fn repair_delta_rejects_unauthorized_and_stale_artifacts() {
    let before_candidate = candidate_before();
    let after_candidate = candidate_after();
    let before_artifact = nav_before();
    let after_artifact = nav_after();
    let before_receipt = make_receipt(
        &before_candidate,
        &before_artifact,
        ReceiptOutcome::Fail,
        3.0,
    );
    let after_receipt = make_receipt(&after_candidate, &after_artifact, ReceiptOutcome::Pass, 1.0);
    let proposal = proposal(&before_receipt);

    let unauthorized_delta = delta_draft(
        &proposal,
        &before_receipt,
        &after_receipt,
        "world.collision",
    );
    assert!(
        validate_repair_delta(
            &proposal,
            unauthorized_delta,
            &before_candidate,
            &after_candidate,
            &before_receipt,
            &after_receipt,
            &BTreeMap::from([("world.navigation".into(), before_artifact.clone())]),
            &BTreeMap::from([("world.navigation".into(), after_artifact.clone())]),
            &registry(),
        )
        .is_err()
    );

    let stale_delta = delta_draft(
        &proposal,
        &before_receipt,
        &after_receipt,
        "world.navigation",
    );
    let mut stale_before = before_artifact;
    stale_before.push(b'!');
    assert!(
        validate_repair_delta(
            &proposal,
            stale_delta,
            &before_candidate,
            &after_candidate,
            &before_receipt,
            &after_receipt,
            &BTreeMap::from([("world.navigation".into(), stale_before)]),
            &BTreeMap::from([("world.navigation".into(), after_artifact)]),
            &registry(),
        )
        .is_err()
    );
}
