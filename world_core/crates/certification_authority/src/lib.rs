//! Independent Rust authority for WGE certification receipts.
//!
//! A producer name and a `pass` string never grant authority. Each receipt is
//! checked against a closed native validator registration, the pinned
//! candidate context, and the exact bytes of its supporting artifacts. The
//! engine-neutral profile keeps rigging and Unity evidence indeterminate.

pub mod schema;
mod validators;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
pub use wge_intake_repair_contract as repair_contract;

pub const REQUEST_SCHEMA: &str = "wge.certification-request/v1";
pub const ENVELOPE_SCHEMA: &str = "wge.certification-receipt-envelope/v1";
pub const REPORT_SCHEMA: &str = "wge.certification-report/v1";
pub const MAX_RECEIPTS: usize = 128;
pub const MAX_CANDIDATES: usize = 8;
pub const MAX_ARTIFACTS_PER_CANDIDATE: usize = 256;
pub const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TOTAL_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_ENVELOPE_BYTES: usize = 2 * 1024 * 1024;
/// These typed records are evidence about fixed candidate identities, not
/// project content. Excluding exactly these kinds prevents the proposal/delta
/// -> candidate hash -> proposal/delta identity cycle. All other artifacts,
/// their kinds/raw digests, and authorized target IDs participate in identity.
pub const REPAIR_IDENTITY_EXCLUDED_KINDS: [&str; 3] =
    ["repair_proposal", "repair_delta", "repair_record"];
pub const SUPPLIED_BAD_GLB_SHA256: &str =
    "sha256:858fa104880822d081405579fb5b39d533d3b3b341d38aa1490a44b634f5e2b4";

pub const REQUIRED_GATES: [&str; 6] =
    ["semantic", "world", "gameplay", "asset", "visual", "repair"];
pub const DEFERRED_GATES: [&str; 4] = [
    "rigging",
    "unity_import",
    "unity_build",
    "unity_playthrough",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityError {
    pub class: &'static str,
    pub detail: String,
}

impl AuthorityError {
    fn new(class: &'static str, detail: impl Into<String>) -> Self {
        Self {
            class,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for AuthorityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.class, self.detail)
    }
}

impl std::error::Error for AuthorityError {}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    Pass,
    Fail,
    Indeterminate,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBinding {
    pub artifact_id: String,
    pub kind: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReceiptEnvelope {
    pub schema_version: String,
    pub receipt_id: String,
    pub project_id: String,
    pub snapshot_id: String,
    pub candidate_sha256: String,
    pub gate_id: String,
    pub validator_id: String,
    pub receipt_schema: String,
    pub status: ReceiptStatus,
    pub producer: String,
    pub observed_input_sha256: String,
    pub evidence: Vec<EvidenceBinding>,
    pub payload: Value,
}

impl ReceiptEnvelope {
    /// Create deterministic identity. This does not grant trust: validation
    /// still opens and rechecks every referenced artifact.
    pub fn seal(&mut self) -> Result<(), AuthorityError> {
        self.evidence
            .sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
        self.observed_input_sha256 = observed_input_digest(self)?;
        self.receipt_id.clear();
        self.receipt_id = receipt_id(self)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ArtifactBytes {
    pub kind: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct CandidateContext {
    pub project_id: String,
    pub snapshot_id: String,
    pub candidate_sha256: String,
    pub artifacts: BTreeMap<String, ArtifactBytes>,
    /// Artifact IDs explicitly granted to the repair work order in this
    /// candidate. The repair validator compares every change against it.
    pub authorized_repair_artifact_ids: BTreeSet<String>,
}

/// Canonical, language-neutral candidate manifest bytes. The candidate digest
/// covers every raw artifact and its kind, plus the complete authorized repair
/// set; callers cannot choose or omit the identity hash.
pub fn candidate_identity_bytes(candidate: &CandidateContext) -> Result<Vec<u8>, AuthorityError> {
    let artifacts = candidate
        .artifacts
        .iter()
        .filter(|(_, artifact)| !REPAIR_IDENTITY_EXCLUDED_KINDS.contains(&artifact.kind.as_str()))
        .map(|(artifact_id, artifact)| {
            serde_json::json!({
                "artifact_id": artifact_id,
                "kind": artifact.kind,
                "sha256": sha256_prefixed(&artifact.bytes),
            })
        })
        .collect::<Vec<_>>();
    let manifest = serde_json::json!({
        "schema_version": "wge.candidate-identity/v1",
        "project_id": candidate.project_id,
        "snapshot_id": candidate.snapshot_id,
        "artifacts": artifacts,
        "authorized_repair_artifact_ids": candidate.authorized_repair_artifact_ids,
    });
    Ok(canonical_json(&manifest).into_bytes())
}

pub fn candidate_identity(candidate: &CandidateContext) -> Result<String, AuthorityError> {
    Ok(sha256_prefixed(&candidate_identity_bytes(candidate)?))
}

pub fn validate_candidate_identity(candidate: &CandidateContext) -> Result<(), AuthorityError> {
    let expected = candidate_identity(candidate)?;
    if candidate.candidate_sha256 != expected {
        return Err(AuthorityError::new(
            "provenance",
            format!(
                "candidate {} identity does not match its complete artifact manifest",
                candidate.snapshot_id
            ),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateDisposition {
    RequiredPass,
    DeferredIndeterminate,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GateRequirement {
    pub gate_id: String,
    pub validator_id: String,
    pub receipt_schema: String,
    pub disposition: GateDisposition,
}

#[derive(Clone, Debug)]
pub struct ValidationRequest {
    pub current_snapshot_id: String,
    pub candidates: Vec<CandidateContext>,
    pub gates: Vec<GateRequirement>,
    pub receipts: Vec<ReceiptEnvelope>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CertificationStatus {
    EngineNeutralCertified,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReceiptDecision {
    pub receipt_id: String,
    pub gate_id: String,
    pub validator_id: String,
    pub status: ReceiptStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ValidationReport {
    pub schema_version: String,
    pub report_id: String,
    pub registry_sha256: String,
    pub project_id: String,
    pub snapshot_id: String,
    pub candidate_sha256: String,
    pub status: CertificationStatus,
    pub receipts: Vec<ReceiptDecision>,
    pub deferred_gates: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ValidatorDescriptor {
    pub validator_id: String,
    pub gate_id: String,
    pub receipt_schema: String,
    pub revision: u32,
    pub accepted_statuses: Vec<ReceiptStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValidatorKind {
    Semantic,
    World,
    Gameplay,
    Asset,
    Visual,
    Repair,
    Deferred,
}

#[derive(Clone, Debug)]
struct RegisteredValidator {
    descriptor: ValidatorDescriptor,
    kind: ValidatorKind,
}

#[derive(Clone, Debug)]
pub struct ValidatorRegistry {
    entries: BTreeMap<String, RegisteredValidator>,
    digest: String,
}

impl ValidatorRegistry {
    pub fn wge_engine_neutral_v1() -> Self {
        let definitions = [
            (
                "wge.validator.semantic-spec/v1",
                "semantic",
                "wge.semantic-receipt/v1",
                ValidatorKind::Semantic,
                vec![ReceiptStatus::Pass, ReceiptStatus::Fail],
            ),
            (
                "wge.validator.world-traversal/v1",
                "world",
                "wge.world-receipt/v1",
                ValidatorKind::World,
                vec![ReceiptStatus::Pass, ReceiptStatus::Fail],
            ),
            (
                "wge.validator.gameplay-replay/v1",
                "gameplay",
                "wge.gameplay-receipt/v1",
                ValidatorKind::Gameplay,
                vec![ReceiptStatus::Pass, ReceiptStatus::Fail],
            ),
            (
                "wge.validator.asset-preparation/v1",
                "asset",
                "wge.asset-receipt/v1",
                ValidatorKind::Asset,
                vec![
                    ReceiptStatus::Pass,
                    ReceiptStatus::Fail,
                    ReceiptStatus::Indeterminate,
                ],
            ),
            (
                "wge.validator.visual-reference/v1",
                "visual",
                "wge.visual-receipt/v1",
                ValidatorKind::Visual,
                vec![ReceiptStatus::Pass, ReceiptStatus::Fail],
            ),
            (
                "wge.validator.evidence-repair/v1",
                "repair",
                "wge.repair-receipt/v1",
                ValidatorKind::Repair,
                vec![ReceiptStatus::Pass, ReceiptStatus::Fail],
            ),
            (
                "wge.validator.rigging-deferred/v1",
                "rigging",
                "wge.deferred-receipt/v1",
                ValidatorKind::Deferred,
                vec![ReceiptStatus::Indeterminate],
            ),
            (
                "wge.validator.unity-import-deferred/v1",
                "unity_import",
                "wge.deferred-receipt/v1",
                ValidatorKind::Deferred,
                vec![ReceiptStatus::Indeterminate],
            ),
            (
                "wge.validator.unity-build-deferred/v1",
                "unity_build",
                "wge.deferred-receipt/v1",
                ValidatorKind::Deferred,
                vec![ReceiptStatus::Indeterminate],
            ),
            (
                "wge.validator.unity-playthrough-deferred/v1",
                "unity_playthrough",
                "wge.deferred-receipt/v1",
                ValidatorKind::Deferred,
                vec![ReceiptStatus::Indeterminate],
            ),
        ];
        let entries = definitions
            .into_iter()
            .map(
                |(validator_id, gate_id, receipt_schema, kind, accepted_statuses)| {
                    let descriptor = ValidatorDescriptor {
                        validator_id: validator_id.to_owned(),
                        gate_id: gate_id.to_owned(),
                        receipt_schema: receipt_schema.to_owned(),
                        revision: 1,
                        accepted_statuses,
                    };
                    (
                        validator_id.to_owned(),
                        RegisteredValidator { descriptor, kind },
                    )
                },
            )
            .collect::<BTreeMap<_, _>>();
        let descriptors = entries
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect::<Vec<_>>();
        let digest =
            sha256_prefixed(canonical_json(&serde_json::to_value(descriptors).unwrap()).as_bytes());
        Self { entries, digest }
    }

    pub fn descriptors(&self) -> Vec<ValidatorDescriptor> {
        self.entries
            .values()
            .map(|entry| entry.descriptor.clone())
            .collect()
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn descriptor(&self, validator_id: &str) -> Option<&ValidatorDescriptor> {
        self.entries
            .get(validator_id)
            .map(|entry| &entry.descriptor)
    }
}

pub fn engine_neutral_gate_profile() -> Vec<GateRequirement> {
    ValidatorRegistry::wge_engine_neutral_v1()
        .descriptors()
        .into_iter()
        .map(|descriptor| GateRequirement {
            disposition: if DEFERRED_GATES.contains(&descriptor.gate_id.as_str()) {
                GateDisposition::DeferredIndeterminate
            } else {
                GateDisposition::RequiredPass
            },
            gate_id: descriptor.gate_id,
            validator_id: descriptor.validator_id,
            receipt_schema: descriptor.receipt_schema,
        })
        .collect()
}

#[derive(Clone, Debug)]
struct ValidatedReceipt {
    pub(crate) envelope: ReceiptEnvelope,
    pub(crate) detail: String,
    pub(crate) native_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RepairReceiptBridge {
    schema_version: String,
    raw_receipt_bytes: Vec<u8>,
    raw_receipt_sha256: String,
    native: repair_contract::ValidatedNativeReceipt,
}

pub fn validate_repair_bridge(
    bytes: &[u8],
) -> Result<repair_contract::ValidatedNativeReceipt, repair_contract::ContractError> {
    let bridge: RepairReceiptBridge = repair_contract::parse_json(bytes)?;
    if bridge.schema_version != "wge.certification-native-repair-bridge/v1"
        || sha256_prefixed(&bridge.raw_receipt_bytes) != bridge.raw_receipt_sha256
    {
        return Err(repair_contract::ContractError::Provenance(
            "native repair bridge has stale raw receipt bytes".into(),
        ));
    }
    let envelope: ReceiptEnvelope = repair_contract::parse_json(&bridge.raw_receipt_bytes)?;
    if receipt_id(&envelope)
        .map_err(|error| repair_contract::ContractError::Identity(error.to_string()))?
        != envelope.receipt_id
        || observed_input_digest(&envelope)
            .map_err(|error| repair_contract::ContractError::Identity(error.to_string()))?
            != envelope.observed_input_sha256
        || bridge.native.validator_id != envelope.validator_id
        || bridge.native.schema_version != envelope.receipt_schema
        || bridge.native.candidate_sha256 != envelope.candidate_sha256
        || bridge.native.gate_id
            != repair_gate_id(&envelope.gate_id).ok_or_else(|| {
                repair_contract::ContractError::Registry("gate is not repairable".into())
            })?
        || bridge.native.outcome
            != match envelope.status {
                ReceiptStatus::Pass => repair_contract::ReceiptOutcome::Pass,
                ReceiptStatus::Fail => repair_contract::ReceiptOutcome::Fail,
                ReceiptStatus::Indeterminate => repair_contract::ReceiptOutcome::Indeterminate,
            }
    {
        return Err(repair_contract::ContractError::Provenance(
            "native repair bridge does not agree with its raw receipt envelope".into(),
        ));
    }
    Ok(bridge.native)
}

/// Independently validate one receipt against a trusted candidate context.
/// Repair receipts require `validate_request`, because their before/after
/// evidence must be resolved and revalidated together.
pub fn validate_receipt(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
    registry: &ValidatorRegistry,
) -> Result<ReceiptDecision, AuthorityError> {
    validate_candidate_identity(candidate)?;
    let registered = validate_envelope(envelope, candidate, registry)?;
    if registered.kind == ValidatorKind::Repair {
        return Err(AuthorityError::new(
            "provenance",
            "repair receipt validation requires the evidence bundle",
        ));
    }
    let candidates = BTreeMap::from([(candidate.snapshot_id.as_str(), candidate)]);
    let verdict = validators::validate(
        registered.kind,
        envelope,
        candidate,
        &candidates,
        &BTreeMap::new(),
    )?;
    validate_derived_status(envelope, &registered.descriptor, verdict.status)?;
    Ok(ReceiptDecision {
        receipt_id: envelope.receipt_id.clone(),
        gate_id: envelope.gate_id.clone(),
        validator_id: envelope.validator_id.clone(),
        status: verdict.status,
        detail: verdict.detail,
    })
}

/// Return the exact native receipt reference used by the bounded repair
/// contract. This invokes the same registered validator as promotion; callers
/// must submit this reference in `RepairProposal.failure_evidence` and in the
/// typed delta draft. It prevents coordinator reimplementation of Rust hashes.
pub fn native_repair_evidence_reference(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
    registry: &ValidatorRegistry,
) -> Result<repair_contract::EvidenceReference, AuthorityError> {
    let bytes = native_repair_receipt_bytes(envelope, candidate, registry)?;
    let gate_id = repair_gate_id(&envelope.gate_id)
        .ok_or_else(|| AuthorityError::new("policy", "gate is not eligible for typed repair"))?;
    Ok(repair_contract::EvidenceReference {
        validator_id: envelope.validator_id.clone(),
        schema_version: envelope.receipt_schema.clone(),
        gate_id,
        candidate_sha256: candidate.candidate_sha256.clone(),
        receipt_sha256: sha256_prefixed(&bytes),
    })
}

/// Rust-native raw-receipt bridge bytes for `intake_repair_contract`.
/// Consumers should hash these bytes only through
/// `native_repair_evidence_reference` when constructing a proposal.
pub fn native_repair_receipt_bytes(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
    registry: &ValidatorRegistry,
) -> Result<Vec<u8>, AuthorityError> {
    validate_candidate_identity(candidate)?;
    let registered = validate_envelope(envelope, candidate, registry)?;
    if registered.kind == ValidatorKind::Repair {
        return Err(AuthorityError::new(
            "policy",
            "repair receipts cannot diagnose another repair",
        ));
    }
    let candidates = BTreeMap::from([(candidate.snapshot_id.as_str(), candidate)]);
    let verdict = validators::validate(
        registered.kind,
        envelope,
        candidate,
        &candidates,
        &BTreeMap::new(),
    )?;
    validate_derived_status(envelope, &registered.descriptor, verdict.status)?;
    native_bridge_bytes(envelope, candidate, &verdict)
}

fn repair_gate_id(gate: &str) -> Option<repair_contract::GateId> {
    use repair_contract::GateId;
    match gate {
        "semantic" => Some(GateId::SemanticIntake),
        "world" => Some(GateId::WorldLayout),
        "gameplay" => Some(GateId::Gameplay),
        "asset" => Some(GateId::AssetPreparation),
        "visual" => Some(GateId::VisualQuality),
        _ => None,
    }
}

/// Native intake-contract registry for revalidating certificate-bound before
/// and after receipts during repair. It accepts only authority-produced bridge
/// bytes, never producer-supplied statuses.
pub fn repair_validator_registry()
-> Result<repair_contract::NativeValidatorRegistry, AuthorityError> {
    let mut registry = repair_contract::NativeValidatorRegistry::new();
    for (validator, schema) in [
        ("wge.validator.semantic-spec/v1", "wge.semantic-receipt/v1"),
        ("wge.validator.world-traversal/v1", "wge.world-receipt/v1"),
        (
            "wge.validator.gameplay-replay/v1",
            "wge.gameplay-receipt/v1",
        ),
        ("wge.validator.asset-preparation/v1", "wge.asset-receipt/v1"),
        ("wge.validator.visual-reference/v1", "wge.visual-receipt/v1"),
    ] {
        registry
            .register(validator, schema, validate_repair_bridge)
            .map_err(|error| AuthorityError::new("registry", error.to_string()))?;
    }
    Ok(registry)
}

fn native_bridge_bytes(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
    verdict: &validators::DomainVerdict,
) -> Result<Vec<u8>, AuthorityError> {
    if repair_gate_id(&envelope.gate_id).is_none() {
        return Ok(Vec::new());
    }
    let artifact = candidate
        .artifacts
        .get(&verdict.measured_artifact_id)
        .ok_or_else(|| {
            AuthorityError::new(
                "provenance",
                "native validator measured an artifact outside the candidate",
            )
        })?;
    let artifact_sha256 = sha256_prefixed(&artifact.bytes);
    let outcome = match verdict.status {
        ReceiptStatus::Pass => repair_contract::ReceiptOutcome::Pass,
        ReceiptStatus::Fail => repair_contract::ReceiptOutcome::Fail,
        ReceiptStatus::Indeterminate => repair_contract::ReceiptOutcome::Indeterminate,
    };
    let findings = verdict
        .failure_code
        .as_ref()
        .map(|code| {
            vec![repair_contract::NativeFinding {
                code: code.clone(),
                artifact_sha256: artifact_sha256.clone(),
                detail: verdict.detail.clone(),
            }]
        })
        .unwrap_or_default();
    let raw_receipt_bytes = serde_json::to_vec(envelope)
        .map_err(|error| AuthorityError::new("malformed", error.to_string()))?;
    let native = repair_contract::ValidatedNativeReceipt {
        validator_id: envelope.validator_id.clone(),
        schema_version: envelope.receipt_schema.clone(),
        gate_id: repair_gate_id(&envelope.gate_id).expect("eligible gate checked above"),
        candidate_sha256: candidate.candidate_sha256.clone(),
        artifact_sha256,
        outcome,
        findings,
        measurements: vec![repair_contract::MetricObservation {
            metric_id: verdict.metric_id,
            value: verdict.metric_value,
        }],
    };
    let bridge = RepairReceiptBridge {
        schema_version: "wge.certification-native-repair-bridge/v1".into(),
        raw_receipt_sha256: sha256_prefixed(&raw_receipt_bytes),
        raw_receipt_bytes,
        native,
    };
    serde_json::to_vec(&bridge).map_err(|error| AuthorityError::new("malformed", error.to_string()))
}

pub fn validate_request(
    request: &ValidationRequest,
    registry: &ValidatorRegistry,
) -> Result<ValidationReport, AuthorityError> {
    validate_request_bounds(request)?;
    validate_gate_profile(&request.gates, registry)?;

    let candidates = request
        .candidates
        .iter()
        .map(|candidate| (candidate.snapshot_id.as_str(), candidate))
        .collect::<BTreeMap<_, _>>();
    if candidates.len() != request.candidates.len() {
        return Err(AuthorityError::new(
            "malformed",
            "candidate snapshot IDs must be unique",
        ));
    }
    let current = candidates
        .get(request.current_snapshot_id.as_str())
        .copied()
        .ok_or_else(|| AuthorityError::new("provenance", "current snapshot is not supplied"))?;

    let mut by_receipt_id = BTreeSet::new();
    for receipt in &request.receipts {
        if !by_receipt_id.insert(receipt.receipt_id.as_str()) {
            return Err(AuthorityError::new(
                "provenance",
                format!("duplicate receipt ID {}", receipt.receipt_id),
            ));
        }
    }

    let mut validated = BTreeMap::<String, ValidatedReceipt>::new();
    for envelope in request
        .receipts
        .iter()
        .filter(|receipt| receipt.validator_id != "wge.validator.evidence-repair/v1")
    {
        let candidate = find_candidate(envelope, &candidates)?;
        let registered = validate_envelope(envelope, candidate, registry)?;
        if registered.kind == ValidatorKind::Repair {
            return Err(AuthorityError::new(
                "provenance",
                "repair receipts must be evaluated after their referenced evidence",
            ));
        }
        let verdict = validators::validate(
            registered.kind,
            envelope,
            candidate,
            &candidates,
            &validated,
        )?;
        validate_derived_status(envelope, &registered.descriptor, verdict.status)?;
        let native_bytes = native_bridge_bytes(envelope, candidate, &verdict)?;
        validated.insert(
            envelope.receipt_id.clone(),
            ValidatedReceipt {
                envelope: envelope.clone(),
                detail: verdict.detail,
                native_bytes,
            },
        );
    }

    for envelope in request
        .receipts
        .iter()
        .filter(|receipt| receipt.validator_id == "wge.validator.evidence-repair/v1")
    {
        let candidate = find_candidate(envelope, &candidates)?;
        let registered = validate_envelope(envelope, candidate, registry)?;
        let verdict = validators::validate(
            registered.kind,
            envelope,
            candidate,
            &candidates,
            &validated,
        )?;
        validate_derived_status(envelope, &registered.descriptor, verdict.status)?;
        let native_bytes = native_bridge_bytes(envelope, candidate, &verdict)?;
        validated.insert(
            envelope.receipt_id.clone(),
            ValidatedReceipt {
                envelope: envelope.clone(),
                detail: verdict.detail,
                native_bytes,
            },
        );
    }

    let current_receipts = request
        .receipts
        .iter()
        .filter(|receipt| receipt.snapshot_id == current.snapshot_id)
        .collect::<Vec<_>>();
    validate_semantic_world_layout_binding(&current_receipts)?;
    let mut current_by_gate = BTreeMap::<&str, Vec<&ReceiptEnvelope>>::new();
    for receipt in &current_receipts {
        current_by_gate
            .entry(receipt.gate_id.as_str())
            .or_default()
            .push(receipt);
    }

    let mut decisions = Vec::with_capacity(request.gates.len());
    let mut reasons = Vec::new();
    let mut deferred = Vec::new();
    for gate in &request.gates {
        let matching = current_by_gate
            .get(gate.gate_id.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        if matching.len() != 1 {
            return Err(AuthorityError::new(
                "provenance",
                format!(
                    "gate {} requires exactly one current receipt; found {}",
                    gate.gate_id,
                    matching.len()
                ),
            ));
        }
        let receipt = matching[0];
        if receipt.validator_id != gate.validator_id
            || receipt.receipt_schema != gate.receipt_schema
        {
            return Err(AuthorityError::new(
                "unregistered",
                format!(
                    "gate {} receipt does not match the pinned gate profile",
                    gate.gate_id
                ),
            ));
        }
        let trusted = validated.get(&receipt.receipt_id).ok_or_else(|| {
            AuthorityError::new(
                "provenance",
                "current gate receipt was not independently validated",
            )
        })?;
        match gate.disposition {
            GateDisposition::RequiredPass if receipt.status != ReceiptStatus::Pass => {
                reasons.push(format!(
                    "required gate {} is {:?}",
                    gate.gate_id, receipt.status
                ));
            }
            GateDisposition::DeferredIndeterminate => {
                if receipt.status != ReceiptStatus::Indeterminate {
                    reasons.push(format!(
                        "deferred gate {} must remain indeterminate",
                        gate.gate_id
                    ));
                } else {
                    deferred.push(gate.gate_id.clone());
                }
            }
            _ => {}
        }
        decisions.push(ReceiptDecision {
            receipt_id: receipt.receipt_id.clone(),
            gate_id: receipt.gate_id.clone(),
            validator_id: receipt.validator_id.clone(),
            status: receipt.status,
            detail: trusted.detail.clone(),
        });
    }
    for receipt in &current_receipts {
        if !request
            .gates
            .iter()
            .any(|gate| gate.gate_id == receipt.gate_id)
        {
            return Err(AuthorityError::new(
                "unregistered",
                format!(
                    "current receipt gate {} is outside the pinned profile",
                    receipt.gate_id
                ),
            ));
        }
    }
    decisions.sort_by(|left, right| left.gate_id.cmp(&right.gate_id));
    deferred.sort();
    reasons.sort();
    let status = if reasons.is_empty() {
        CertificationStatus::EngineNeutralCertified
    } else {
        CertificationStatus::Rejected
    };
    let mut report = ValidationReport {
        schema_version: REPORT_SCHEMA.to_owned(),
        report_id: String::new(),
        registry_sha256: registry.digest().to_owned(),
        project_id: current.project_id.clone(),
        snapshot_id: current.snapshot_id.clone(),
        candidate_sha256: current.candidate_sha256.clone(),
        status,
        receipts: decisions,
        deferred_gates: deferred,
        reasons,
    };
    report.report_id = report_id(&report)?;
    Ok(report)
}

fn validate_semantic_world_layout_binding(
    receipts: &[&ReceiptEnvelope],
) -> Result<(), AuthorityError> {
    let semantic = receipts
        .iter()
        .find(|receipt| receipt.gate_id == "semantic")
        .ok_or_else(|| AuthorityError::new("provenance", "current semantic receipt is missing"))?;
    let world = receipts
        .iter()
        .find(|receipt| receipt.gate_id == "world")
        .ok_or_else(|| AuthorityError::new("provenance", "current world receipt is missing"))?;
    let semantic: schema::SemanticReceiptPayload = serde_json::from_value(semantic.payload.clone())
        .map_err(|error| AuthorityError::new("malformed", format!("semantic payload: {error}")))?;
    let world: schema::WorldReceiptPayload = serde_json::from_value(world.payload.clone())
        .map_err(|error| AuthorityError::new("malformed", format!("world payload: {error}")))?;
    if semantic.layout_artifact_id != world.layout_artifact_id {
        return Err(AuthorityError::new(
            "provenance",
            "semantic intake and runtime world are detached from different authored layouts",
        ));
    }
    let semantic_layout = receipts
        .iter()
        .find(|receipt| receipt.gate_id == "semantic")
        .unwrap()
        .evidence
        .iter()
        .find(|item| item.artifact_id == semantic.layout_artifact_id);
    let world_layout = receipts
        .iter()
        .find(|receipt| receipt.gate_id == "world")
        .unwrap()
        .evidence
        .iter()
        .find(|item| item.artifact_id == world.layout_artifact_id);
    if semantic_layout.is_none()
        || world_layout.is_none()
        || semantic_layout.unwrap() != world_layout.unwrap()
    {
        return Err(AuthorityError::new(
            "provenance",
            "semantic/world gates do not bind the identical typed layout bytes",
        ));
    }
    Ok(())
}

fn validate_request_bounds(request: &ValidationRequest) -> Result<(), AuthorityError> {
    if request.receipts.is_empty() || request.receipts.len() > MAX_RECEIPTS {
        return Err(AuthorityError::new(
            "malformed",
            format!("receipt count must be 1..={MAX_RECEIPTS}"),
        ));
    }
    if request.candidates.is_empty() || request.candidates.len() > MAX_CANDIDATES {
        return Err(AuthorityError::new(
            "malformed",
            format!("candidate count must be 1..={MAX_CANDIDATES}"),
        ));
    }
    let mut total_bytes = 0usize;
    for candidate in &request.candidates {
        nonempty("candidate.project_id", &candidate.project_id)?;
        nonempty("candidate.snapshot_id", &candidate.snapshot_id)?;
        validate_digest(&candidate.candidate_sha256, "candidate")?;
        validate_candidate_identity(candidate)?;
        if candidate.artifacts.is_empty() || candidate.artifacts.len() > MAX_ARTIFACTS_PER_CANDIDATE
        {
            return Err(AuthorityError::new(
                "malformed",
                format!(
                    "candidate {} has invalid artifact count",
                    candidate.snapshot_id
                ),
            ));
        }
        for (artifact_id, artifact) in &candidate.artifacts {
            nonempty("artifact_id", artifact_id)?;
            nonempty("artifact.kind", &artifact.kind)?;
            if artifact.bytes.is_empty() || artifact.bytes.len() > MAX_ARTIFACT_BYTES {
                return Err(AuthorityError::new(
                    "malformed",
                    format!("artifact {artifact_id} byte length is out of bounds"),
                ));
            }
            total_bytes = total_bytes.saturating_add(artifact.bytes.len());
        }
    }
    if total_bytes > MAX_TOTAL_ARTIFACT_BYTES {
        return Err(AuthorityError::new(
            "malformed",
            "request exceeds aggregate artifact byte limit",
        ));
    }
    if request.gates.len() != REQUIRED_GATES.len() + DEFERRED_GATES.len() {
        return Err(AuthorityError::new(
            "policy",
            "engine-neutral profile must contain six required and four deferred gates",
        ));
    }
    Ok(())
}

fn validate_gate_profile(
    gates: &[GateRequirement],
    registry: &ValidatorRegistry,
) -> Result<(), AuthorityError> {
    let mut gate_ids = BTreeSet::new();
    for gate in gates {
        if !gate_ids.insert(gate.gate_id.as_str()) {
            return Err(AuthorityError::new(
                "policy",
                format!("duplicate gate {} in profile", gate.gate_id),
            ));
        }
        let registered = registry.entries.get(&gate.validator_id).ok_or_else(|| {
            AuthorityError::new(
                "unregistered",
                format!(
                    "gate {} names unknown validator {}",
                    gate.gate_id, gate.validator_id
                ),
            )
        })?;
        if registered.descriptor.gate_id != gate.gate_id
            || registered.descriptor.receipt_schema != gate.receipt_schema
        {
            return Err(AuthorityError::new(
                "unregistered",
                format!("gate {} does not match its registered schema", gate.gate_id),
            ));
        }
        let should_defer = DEFERRED_GATES.contains(&gate.gate_id.as_str());
        if should_defer != (gate.disposition == GateDisposition::DeferredIndeterminate) {
            return Err(AuthorityError::new(
                "policy",
                format!("gate {} has an unauthorized disposition", gate.gate_id),
            ));
        }
    }
    for required in REQUIRED_GATES {
        if !gate_ids.contains(required) {
            return Err(AuthorityError::new(
                "policy",
                format!("required gate {required} is missing"),
            ));
        }
    }
    for deferred in DEFERRED_GATES {
        if !gate_ids.contains(deferred) {
            return Err(AuthorityError::new(
                "policy",
                format!("deferred gate {deferred} must be explicit"),
            ));
        }
    }
    Ok(())
}

fn find_candidate<'a>(
    envelope: &ReceiptEnvelope,
    candidates: &'a BTreeMap<&str, &CandidateContext>,
) -> Result<&'a CandidateContext, AuthorityError> {
    let candidate = candidates
        .get(envelope.snapshot_id.as_str())
        .copied()
        .ok_or_else(|| {
            AuthorityError::new(
                "provenance",
                format!(
                    "receipt {} references unknown snapshot",
                    envelope.receipt_id
                ),
            )
        })?;
    if envelope.project_id != candidate.project_id
        || envelope.candidate_sha256 != candidate.candidate_sha256
    {
        return Err(AuthorityError::new(
            "provenance",
            format!(
                "receipt {} is stale for its candidate context",
                envelope.receipt_id
            ),
        ));
    }
    Ok(candidate)
}

fn validate_envelope<'a>(
    envelope: &ReceiptEnvelope,
    candidate: &CandidateContext,
    registry: &'a ValidatorRegistry,
) -> Result<&'a RegisteredValidator, AuthorityError> {
    if serde_json::to_vec(envelope)
        .map_err(|error| AuthorityError::new("malformed", error.to_string()))?
        .len()
        > MAX_ENVELOPE_BYTES
    {
        return Err(AuthorityError::new(
            "malformed",
            "receipt envelope exceeds size limit",
        ));
    }
    if envelope.schema_version != ENVELOPE_SCHEMA {
        return Err(AuthorityError::new(
            "malformed",
            format!("unsupported envelope schema {:?}", envelope.schema_version),
        ));
    }
    for (field, value) in [
        ("project_id", envelope.project_id.as_str()),
        ("snapshot_id", envelope.snapshot_id.as_str()),
        ("gate_id", envelope.gate_id.as_str()),
        ("validator_id", envelope.validator_id.as_str()),
        ("receipt_schema", envelope.receipt_schema.as_str()),
        ("producer", envelope.producer.as_str()),
    ] {
        nonempty(field, value)?;
    }
    validate_digest(&envelope.candidate_sha256, "receipt candidate")?;
    validate_digest(&envelope.observed_input_sha256, "receipt input")?;
    if envelope.project_id != candidate.project_id
        || envelope.snapshot_id != candidate.snapshot_id
        || envelope.candidate_sha256 != candidate.candidate_sha256
    {
        return Err(AuthorityError::new(
            "provenance",
            format!("receipt {} candidate binding is stale", envelope.receipt_id),
        ));
    }
    if envelope.evidence.is_empty() {
        return Err(AuthorityError::new(
            "provenance",
            "status-only or producer-only receipt has no evidence artifacts",
        ));
    }
    let mut previous: Option<&str> = None;
    for binding in &envelope.evidence {
        nonempty("evidence.artifact_id", &binding.artifact_id)?;
        nonempty("evidence.kind", &binding.kind)?;
        validate_digest(&binding.sha256, "evidence artifact")?;
        if previous.is_some_and(|id| id >= binding.artifact_id.as_str()) {
            return Err(AuthorityError::new(
                "malformed",
                "evidence references must be unique and sorted by artifact_id",
            ));
        }
        previous = Some(&binding.artifact_id);
        if binding.sha256 == SUPPLIED_BAD_GLB_SHA256
            && envelope.gate_id == "asset"
            && envelope.status == ReceiptStatus::Pass
        {
            return Err(AuthorityError::new(
                "contract",
                "permanent negative control GLB cannot support a passing asset receipt",
            ));
        }
        let artifact = candidate
            .artifacts
            .get(&binding.artifact_id)
            .ok_or_else(|| {
                AuthorityError::new(
                    "provenance",
                    format!(
                        "evidence artifact {} is absent from candidate",
                        binding.artifact_id
                    ),
                )
            })?;
        if artifact.kind != binding.kind {
            return Err(AuthorityError::new(
                "provenance",
                format!(
                    "evidence artifact {} kind does not match candidate",
                    binding.artifact_id
                ),
            ));
        }
        if sha256_prefixed(&artifact.bytes) != binding.sha256 {
            return Err(AuthorityError::new(
                "provenance",
                format!(
                    "evidence artifact {} byte digest is stale or forged",
                    binding.artifact_id
                ),
            ));
        }
    }
    if observed_input_digest(envelope)? != envelope.observed_input_sha256 {
        return Err(AuthorityError::new(
            "provenance",
            "observed-input digest does not match candidate and artifact bindings",
        ));
    }
    if receipt_id(envelope)? != envelope.receipt_id {
        return Err(AuthorityError::new(
            "provenance",
            "receipt ID does not match canonical envelope contents",
        ));
    }
    let registered = registry
        .entries
        .get(&envelope.validator_id)
        .ok_or_else(|| {
            AuthorityError::new(
                "unregistered",
                format!("validator {} is not registered", envelope.validator_id),
            )
        })?;
    if registered.descriptor.gate_id != envelope.gate_id
        || registered.descriptor.receipt_schema != envelope.receipt_schema
    {
        return Err(AuthorityError::new(
            "unregistered",
            format!(
                "validator {} is registered for another gate or schema",
                envelope.validator_id
            ),
        ));
    }
    if !registered
        .descriptor
        .accepted_statuses
        .contains(&envelope.status)
    {
        return Err(AuthorityError::new(
            "policy",
            format!(
                "validator {} cannot accept {:?}",
                envelope.validator_id, envelope.status
            ),
        ));
    }
    Ok(registered)
}

fn validate_derived_status(
    envelope: &ReceiptEnvelope,
    descriptor: &ValidatorDescriptor,
    derived: ReceiptStatus,
) -> Result<(), AuthorityError> {
    if envelope.status != derived {
        return Err(AuthorityError::new(
            "contract",
            format!(
                "receipt {} claims {:?}, validator {} derives {:?}",
                envelope.receipt_id, envelope.status, descriptor.validator_id, derived
            ),
        ));
    }
    Ok(())
}

pub fn observed_input_digest(envelope: &ReceiptEnvelope) -> Result<String, AuthorityError> {
    let value = serde_json::json!({
        "candidate_sha256": envelope.candidate_sha256,
        "evidence": envelope.evidence,
        "gate_id": envelope.gate_id,
        "project_id": envelope.project_id,
        "receipt_schema": envelope.receipt_schema,
        "snapshot_id": envelope.snapshot_id,
        "validator_id": envelope.validator_id,
    });
    Ok(sha256_prefixed(canonical_json(&value).as_bytes()))
}

pub fn receipt_id(envelope: &ReceiptEnvelope) -> Result<String, AuthorityError> {
    let mut value = serde_json::to_value(envelope)
        .map_err(|error| AuthorityError::new("malformed", error.to_string()))?;
    value
        .as_object_mut()
        .ok_or_else(|| AuthorityError::new("malformed", "receipt envelope is not an object"))?
        .remove("receipt_id");
    Ok(format!(
        "wge_receipt_{}",
        sha256_hex(canonical_json(&value).as_bytes())
    ))
}

fn report_id(report: &ValidationReport) -> Result<String, AuthorityError> {
    let mut value = serde_json::to_value(report)
        .map_err(|error| AuthorityError::new("malformed", error.to_string()))?;
    value
        .as_object_mut()
        .ok_or_else(|| AuthorityError::new("malformed", "report is not an object"))?
        .remove("report_id");
    Ok(format!(
        "wge_cert_{}",
        sha256_hex(canonical_json(&value).as_bytes())
    ))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

pub fn canonical_json(value: &Value) -> String {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(object) => {
                let sorted = object
                    .iter()
                    .map(|(key, child)| (key.clone(), canonical(child)))
                    .collect::<BTreeMap<_, _>>();
                Value::Object(sorted.into_iter().collect())
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&canonical(value)).expect("JSON values serialize")
}

pub fn validate_digest(value: &str, label: &str) -> Result<(), AuthorityError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(AuthorityError::new(
            "malformed",
            format!("{label} digest must use sha256:<64 hex>"),
        ));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AuthorityError::new(
            "malformed",
            format!("{label} digest must use sha256:<64 hex>"),
        ));
    }
    Ok(())
}

pub(crate) fn nonempty(label: &str, value: &str) -> Result<(), AuthorityError> {
    if value.trim().is_empty() {
        Err(AuthorityError::new(
            "malformed",
            format!("{label} must not be empty"),
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn parse_artifact<T: for<'de> Deserialize<'de>>(
    bytes: &[u8],
    label: &str,
) -> Result<T, AuthorityError> {
    serde_json::from_slice(bytes).map_err(|error| {
        AuthorityError::new("malformed", format!("{label} artifact is invalid: {error}"))
    })
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn registry_and_envelope_ids_are_deterministic_and_content_sensitive() {
        let registry = ValidatorRegistry::wge_engine_neutral_v1();
        assert_eq!(
            registry.digest(),
            ValidatorRegistry::wge_engine_neutral_v1().digest()
        );
        let mut envelope = ReceiptEnvelope {
            schema_version: ENVELOPE_SCHEMA.into(),
            receipt_id: String::new(),
            project_id: "p".into(),
            snapshot_id: "s".into(),
            candidate_sha256: sha256_prefixed(b"candidate"),
            gate_id: "rigging".into(),
            validator_id: "wge.validator.rigging-deferred/v1".into(),
            receipt_schema: "wge.deferred-receipt/v1".into(),
            status: ReceiptStatus::Indeterminate,
            producer: "test".into(),
            observed_input_sha256: String::new(),
            evidence: vec![EvidenceBinding {
                artifact_id: "manifest".into(),
                kind: "project_manifest".into(),
                sha256: sha256_prefixed(b"manifest"),
            }],
            payload: serde_json::json!({"reason_code":"deferred_by_scope","deferral_scope":"rigging","detail":"deferred"}),
        };
        envelope.seal().unwrap();
        let id = envelope.receipt_id.clone();
        envelope.seal().unwrap();
        assert_eq!(id, envelope.receipt_id);
        envelope.producer = "different-producer".into();
        assert_ne!(id, receipt_id(&envelope).unwrap());
    }
}
