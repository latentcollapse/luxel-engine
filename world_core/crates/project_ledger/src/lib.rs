//! Rust-owned project ledger for the WGE MVP vertical slice.
//!
//! The ledger is deliberately small. It is not a universal game DSL and it
//! does not execute authoring text. It records a typed project specification,
//! content-addressed artifact edges, work orders, and evidence receipts. A
//! release snapshot can only be committed after the required receipts pass and
//! every referenced artifact has a well-formed identity.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

mod world;
pub use world::validate_world_bundle;

pub const PROJECT_SPEC_SCHEMA: &str = "wge.project-spec/v1";
pub const EVIDENCE_SCHEMA: &str = "wge.evidence/v1";
pub const SNAPSHOT_SCHEMA: &str = "wge.project-snapshot/v1";
pub const UNITY_IMPORT_SCHEMA: &str = "wge.unity-mvp-import/v1";
pub const WORK_ORDER_SCHEMA: &str = "wge.work-order/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    Io(String),
    Json(String),
    Contract(String),
    Provenance(String),
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "I/O error: {message}"),
            Self::Json(message) => write!(formatter, "JSON error: {message}"),
            Self::Contract(message) => write!(formatter, "contract error: {message}"),
            Self::Provenance(message) => write!(formatter, "provenance error: {message}"),
        }
    }
}

impl std::error::Error for LedgerError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectSpec {
    pub schema_version: String,
    pub project_id: String,
    pub title: String,
    pub brief: BriefSpec,
    pub target: TargetProfile,
    pub world: WorldSpec,
    pub assets: Vec<AssetBinding>,
    pub gameplay: GameplayBinding,
    pub artifact_graph: Vec<ArtifactNode>,
    pub work_orders: Vec<WorkOrder>,
    pub required_gates: Vec<GateRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BriefSpec {
    pub text: String,
    pub sources: Vec<SourceReference>,
    pub claims: Vec<SemanticClaim>,
    pub conflicts: Vec<SpecConflict>,
    pub style_target: StyleTarget,
    pub assumptions: Vec<SpecAssumption>,
    pub design_constraints: Vec<DesignConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceReference {
    pub source_id: String,
    pub kind: SourceKind,
    pub path: String,
    pub sha256: String,
    pub region_normalized: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Brief,
    ConceptArt,
    DesignDocument,
    SourceAsset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    Observed,
    Inferred,
    Constraint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticClaim {
    pub claim_id: String,
    pub source_id: String,
    pub kind: ClaimKind,
    pub statement: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecConflict {
    pub conflict_id: String,
    pub left_claim_id: String,
    pub right_claim_id: String,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleTarget {
    pub visual_language: String,
    pub palette: Vec<String>,
    pub camera: String,
    pub reference_source_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpecAssumption {
    pub assumption_id: String,
    pub statement: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DesignConstraint {
    pub constraint_id: String,
    pub category: String,
    pub statement: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TargetProfile {
    pub engine: Engine,
    pub engine_version: String,
    pub platform: String,
    pub coordinate_system: String,
    pub build_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Unity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldSpec {
    pub world_id: String,
    pub dimensions_m: [f64; 2],
    pub terrain: ArtifactRef,
    pub collision: ArtifactRef,
    pub navigation: ArtifactRef,
    pub spawns: Vec<SpawnPoint>,
    pub objective: ObjectiveSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpawnPoint {
    pub spawn_id: String,
    pub team: String,
    pub position_xz_m: [f64; 2],
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObjectiveSpec {
    pub objective_id: String,
    pub kind: String,
    pub required_interaction_tag: String,
    pub win_condition: String,
    pub loss_condition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AssetBinding {
    pub asset_id: String,
    pub source: ArtifactRef,
    pub runtime_package: ArtifactRef,
    pub role: String,
    pub required_features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GameplayBinding {
    pub runtime_package: ArtifactRef,
    pub input_trace: ArtifactRef,
    pub start_entity_id: String,
    pub objective_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub kind: String,
    pub schema_version: String,
    pub path: String,
    pub sha256: String,
    pub producer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactNode {
    pub artifact: ArtifactRef,
    pub dependencies: Vec<DependencyRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DependencyRef {
    pub artifact_id: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkOrder {
    pub schema_version: String,
    pub work_order_id: String,
    pub operation: String,
    pub snapshot_id: String,
    pub allowed_artifacts: Vec<String>,
    pub required_capabilities: Vec<String>,
    pub required_gates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GateRequirement {
    pub gate_id: String,
    pub evidence_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Pass,
    Fail,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReceipt {
    pub schema_version: String,
    pub receipt_id: String,
    pub gate_id: String,
    pub evidence_kind: String,
    pub status: EvidenceStatus,
    pub artifact_id: String,
    pub artifact_sha256: String,
    pub observed_input_sha256: String,
    pub producer: String,
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBundle {
    pub schema_version: String,
    pub receipts: Vec<EvidenceReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectSnapshot {
    pub schema_version: String,
    pub snapshot_id: String,
    pub project_id: String,
    pub spec_sha256: String,
    pub artifact_graph_sha256: String,
    pub artifacts: Vec<ArtifactRef>,
    pub required_gates: Vec<GateRequirement>,
    pub evidence: Vec<EvidenceReceipt>,
    pub target: TargetProfile,
    pub status: SnapshotStatus,
    pub snapshot_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotStatus {
    Certified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UnityImportManifest {
    pub schema_version: String,
    pub project_id: String,
    pub snapshot_sha256: String,
    pub snapshot_path: String,
    pub target: TargetProfile,
    pub artifacts: Vec<ArtifactRef>,
    pub world_id: String,
    pub gameplay_artifact_id: String,
    pub required_gates: Vec<GateRequirement>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

/// Canonical JSON recursively orders object keys without changing array order.
pub fn canonical_json(value: &Value) -> String {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(object) => {
                let ordered: BTreeMap<String, Value> = object
                    .iter()
                    .map(|(key, child)| (key.clone(), canonical(child)))
                    .collect();
                Value::Object(ordered.into_iter().collect())
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&canonical(value)).expect("serde_json values are serializable")
}

fn digest_without<T: Serialize>(value: &T, field: &str) -> Result<String, LedgerError> {
    let mut object =
        serde_json::to_value(value).map_err(|error| LedgerError::Json(error.to_string()))?;
    object
        .as_object_mut()
        .ok_or_else(|| LedgerError::Contract("digest input must be an object".into()))?
        .remove(field);
    Ok(sha256_prefixed(canonical_json(&object).as_bytes()))
}

pub fn spec_digest(spec: &ProjectSpec) -> Result<String, LedgerError> {
    Ok(sha256_prefixed(
        canonical_json(
            &serde_json::to_value(spec).map_err(|error| LedgerError::Json(error.to_string()))?,
        )
        .as_bytes(),
    ))
}

pub fn artifact_graph_digest(nodes: &[ArtifactNode]) -> Result<String, LedgerError> {
    Ok(sha256_prefixed(
        canonical_json(
            &serde_json::to_value(nodes).map_err(|error| LedgerError::Json(error.to_string()))?,
        )
        .as_bytes(),
    ))
}

pub fn evidence_digest(receipts: &[EvidenceReceipt]) -> Result<String, LedgerError> {
    Ok(sha256_prefixed(
        canonical_json(
            &serde_json::to_value(receipts)
                .map_err(|error| LedgerError::Json(error.to_string()))?,
        )
        .as_bytes(),
    ))
}

/// Mint the deterministic identity of an evidence receipt. The ID covers the
/// observation, gate, artifact, producer, and details, but not itself.
pub fn evidence_receipt_id(receipt: &EvidenceReceipt) -> Result<String, LedgerError> {
    let mut value =
        serde_json::to_value(receipt).map_err(|error| LedgerError::Json(error.to_string()))?;
    value
        .as_object_mut()
        .ok_or_else(|| LedgerError::Contract("evidence receipt must be an object".into()))?
        .remove("receipt_id");
    let digest = sha256_hex(canonical_json(&value).as_bytes());
    Ok(format!("receipt_{}", &digest[..32]))
}

pub fn validate_spec(spec: &ProjectSpec) -> Result<(), LedgerError> {
    if spec.schema_version != PROJECT_SPEC_SCHEMA {
        return Err(LedgerError::Contract(format!(
            "unsupported project schema {:?}",
            spec.schema_version
        )));
    }
    nonempty("project_id", &spec.project_id)?;
    nonempty("title", &spec.title)?;
    if spec.brief.text.trim().is_empty() {
        return Err(LedgerError::Contract("brief.text is required".into()));
    }
    if spec.brief.sources.is_empty() {
        return Err(LedgerError::Contract(
            "brief.sources must not be empty".into(),
        ));
    }
    for source in &spec.brief.sources {
        nonempty("brief.sources.source_id", &source.source_id)?;
        nonempty("brief.sources.path", &source.path)?;
        validate_digest(&source.sha256, "brief source")?;
        if let Some(region) = source.region_normalized
            && (region
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0 || *value > 1.0)
                || region[2] <= region[0]
                || region[3] <= region[1])
        {
            return Err(LedgerError::Contract(format!(
                "source {} region_normalized must be a non-degenerate [0,1] rectangle",
                source.source_id
            )));
        }
    }
    let source_ids: BTreeSet<&str> = spec
        .brief
        .sources
        .iter()
        .map(|source| source.source_id.as_str())
        .collect();
    let mut claim_ids = BTreeSet::new();
    for claim in &spec.brief.claims {
        nonempty("brief.claim.claim_id", &claim.claim_id)?;
        nonempty("brief.claim.source_id", &claim.source_id)?;
        nonempty("brief.claim.statement", &claim.statement)?;
        if !source_ids.contains(claim.source_id.as_str()) {
            return Err(LedgerError::Contract(format!(
                "claim {} references missing source {}",
                claim.claim_id, claim.source_id
            )));
        }
        if !claim_ids.insert(claim.claim_id.as_str()) {
            return Err(LedgerError::Contract(format!(
                "duplicate claim id {}",
                claim.claim_id
            )));
        }
        if !claim.confidence.is_finite() || !(0.0..=1.0).contains(&claim.confidence) {
            return Err(LedgerError::Contract(format!(
                "claim {} confidence must be in [0,1]",
                claim.claim_id
            )));
        }
    }
    for conflict in &spec.brief.conflicts {
        nonempty("brief.conflict.conflict_id", &conflict.conflict_id)?;
        if !claim_ids.contains(conflict.left_claim_id.as_str())
            || !claim_ids.contains(conflict.right_claim_id.as_str())
        {
            return Err(LedgerError::Contract(format!(
                "conflict {} references an unknown claim",
                conflict.conflict_id
            )));
        }
        nonempty("brief.conflict.resolution", &conflict.resolution)?;
    }
    nonempty(
        "brief.style_target.visual_language",
        &spec.brief.style_target.visual_language,
    )?;
    nonempty("brief.style_target.camera", &spec.brief.style_target.camera)?;
    if spec.brief.style_target.palette.is_empty() {
        return Err(LedgerError::Contract(
            "brief.style_target.palette must not be empty".into(),
        ));
    }
    for source_id in &spec.brief.style_target.reference_source_ids {
        if !source_ids.contains(source_id.as_str()) {
            return Err(LedgerError::Contract(format!(
                "style target references missing source {}",
                source_id
            )));
        }
    }
    for assumption in &spec.brief.assumptions {
        nonempty("brief.assumption.assumption_id", &assumption.assumption_id)?;
        nonempty("brief.assumption.statement", &assumption.statement)?;
        nonempty("brief.assumption.reason", &assumption.reason)?;
    }
    if spec.target.engine != Engine::Unity {
        return Err(LedgerError::Contract(
            "MVP target engine must be Unity".into(),
        ));
    }
    nonempty("target.engine_version", &spec.target.engine_version)?;
    nonempty("target.platform", &spec.target.platform)?;
    nonempty("target.coordinate_system", &spec.target.coordinate_system)?;
    nonempty("target.build_profile", &spec.target.build_profile)?;
    if spec
        .world
        .dimensions_m
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err(LedgerError::Contract(
            "world dimensions must be finite and positive".into(),
        ));
    }
    if spec.world.spawns.is_empty() {
        return Err(LedgerError::Contract(
            "world.spawns must not be empty".into(),
        ));
    }
    for spawn in &spec.world.spawns {
        nonempty("spawn.spawn_id", &spawn.spawn_id)?;
        nonempty("spawn.team", &spawn.team)?;
        if spawn.position_xz_m.iter().any(|value| !value.is_finite()) {
            return Err(LedgerError::Contract(format!(
                "spawn {} position must be finite",
                spawn.spawn_id
            )));
        }
    }
    nonempty(
        "world.objective.objective_id",
        &spec.world.objective.objective_id,
    )?;
    nonempty(
        "world.objective.required_interaction_tag",
        &spec.world.objective.required_interaction_tag,
    )?;
    nonempty("gameplay.start_entity_id", &spec.gameplay.start_entity_id)?;
    validate_artifact_graph(&spec.artifact_graph)?;
    let artifacts_by_id: BTreeMap<&str, &ArtifactRef> = spec
        .artifact_graph
        .iter()
        .map(|node| (node.artifact.artifact_id.as_str(), &node.artifact))
        .collect();
    let artifact_ids: BTreeSet<&str> = artifacts_by_id.keys().copied().collect();
    for reference in referenced_artifacts(spec) {
        let Some(canonical) = artifacts_by_id.get(reference.artifact_id.as_str()) else {
            return Err(LedgerError::Contract(format!(
                "referenced artifact {} is absent from artifact_graph",
                reference.artifact_id
            )));
        };
        if *canonical != reference {
            return Err(LedgerError::Provenance(format!(
                "referenced artifact {} does not match its canonical artifact_graph node",
                reference.artifact_id
            )));
        }
    }
    let gate_ids: BTreeSet<&str> = spec
        .required_gates
        .iter()
        .map(|gate| gate.gate_id.as_str())
        .collect();
    if gate_ids.len() != spec.required_gates.len() || gate_ids.is_empty() {
        return Err(LedgerError::Contract(
            "required_gates must be non-empty and unique".into(),
        ));
    }
    for order in &spec.work_orders {
        if order.schema_version != WORK_ORDER_SCHEMA {
            return Err(LedgerError::Contract(format!(
                "work order {} has unsupported schema",
                order.work_order_id
            )));
        }
        nonempty("work_order.work_order_id", &order.work_order_id)?;
        nonempty("work_order.operation", &order.operation)?;
        nonempty("work_order.snapshot_id", &order.snapshot_id)?;
        for artifact_id in &order.allowed_artifacts {
            if !artifact_ids.contains(artifact_id.as_str()) {
                return Err(LedgerError::Contract(format!(
                    "work order {} allows undeclared artifact {}",
                    order.work_order_id, artifact_id
                )));
            }
        }
        for gate in &order.required_gates {
            if !gate_ids.contains(gate.as_str()) {
                return Err(LedgerError::Contract(format!(
                    "work order {} requires undeclared gate {}",
                    order.work_order_id, gate
                )));
            }
        }
    }
    Ok(())
}

pub fn validate_artifact_graph(nodes: &[ArtifactNode]) -> Result<(), LedgerError> {
    if nodes.is_empty() {
        return Err(LedgerError::Contract(
            "artifact_graph must not be empty".into(),
        ));
    }
    let mut by_id = BTreeMap::new();
    for node in nodes {
        nonempty("artifact.artifact_id", &node.artifact.artifact_id)?;
        nonempty("artifact.kind", &node.artifact.kind)?;
        nonempty("artifact.schema_version", &node.artifact.schema_version)?;
        nonempty("artifact.path", &node.artifact.path)?;
        nonempty("artifact.producer", &node.artifact.producer)?;
        validate_digest(&node.artifact.sha256, "artifact")?;
        if by_id
            .insert(node.artifact.artifact_id.as_str(), &node.artifact)
            .is_some()
        {
            return Err(LedgerError::Contract(format!(
                "duplicate artifact id {}",
                node.artifact.artifact_id
            )));
        }
    }
    for node in nodes {
        for dependency in &node.dependencies {
            let Some(target) = by_id.get(dependency.artifact_id.as_str()) else {
                return Err(LedgerError::Contract(format!(
                    "artifact {} depends on missing {}",
                    node.artifact.artifact_id, dependency.artifact_id
                )));
            };
            if target.sha256 != dependency.sha256 {
                return Err(LedgerError::Provenance(format!(
                    "artifact {} dependency {} digest is stale",
                    node.artifact.artifact_id, dependency.artifact_id
                )));
            }
        }
    }
    let edges: BTreeMap<&str, Vec<&str>> = nodes
        .iter()
        .map(|node| {
            (
                node.artifact.artifact_id.as_str(),
                node.dependencies
                    .iter()
                    .map(|dependency| dependency.artifact_id.as_str())
                    .collect(),
            )
        })
        .collect();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for artifact_id in edges.keys().copied() {
        visit_graph(artifact_id, &edges, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit_graph<'a>(
    artifact_id: &'a str,
    edges: &BTreeMap<&'a str, Vec<&'a str>>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
) -> Result<(), LedgerError> {
    if visited.contains(artifact_id) {
        return Ok(());
    }
    if !visiting.insert(artifact_id) {
        return Err(LedgerError::Contract(format!(
            "artifact dependency cycle includes {}",
            artifact_id
        )));
    }
    if let Some(dependencies) = edges.get(artifact_id) {
        for dependency in dependencies {
            visit_graph(dependency, edges, visiting, visited)?;
        }
    }
    visiting.remove(artifact_id);
    visited.insert(artifact_id);
    Ok(())
}

pub fn invalidated_artifacts(nodes: &[ArtifactNode]) -> Result<Vec<String>, LedgerError> {
    let mut by_id = BTreeMap::new();
    for node in nodes {
        if by_id
            .insert(
                node.artifact.artifact_id.as_str(),
                node.artifact.sha256.as_str(),
            )
            .is_some()
        {
            return Err(LedgerError::Contract("duplicate artifact id".into()));
        }
    }
    let mut invalidated = Vec::new();
    for node in nodes {
        if node.dependencies.iter().any(|dependency| {
            by_id.get(dependency.artifact_id.as_str()).copied() != Some(dependency.sha256.as_str())
        }) {
            invalidated.push(node.artifact.artifact_id.clone());
        }
    }
    Ok(invalidated)
}

pub fn commit_snapshot(
    spec: &ProjectSpec,
    evidence: &EvidenceBundle,
) -> Result<ProjectSnapshot, LedgerError> {
    validate_spec(spec)?;
    validate_evidence(spec, evidence)?;
    let spec_sha256 = spec_digest(spec)?;
    let artifact_graph_sha256 = artifact_graph_digest(&spec.artifact_graph)?;
    let artifacts = spec
        .artifact_graph
        .iter()
        .map(|node| node.artifact.clone())
        .collect::<Vec<_>>();
    let mut snapshot = ProjectSnapshot {
        schema_version: SNAPSHOT_SCHEMA.to_owned(),
        snapshot_id: format!("snapshot_{}", &spec_sha256[7..23]),
        project_id: spec.project_id.clone(),
        spec_sha256,
        artifact_graph_sha256,
        artifacts,
        required_gates: spec.required_gates.clone(),
        evidence: evidence.receipts.clone(),
        target: spec.target.clone(),
        status: SnapshotStatus::Certified,
        snapshot_sha256: String::new(),
    };
    snapshot.snapshot_sha256 = digest_without(&snapshot, "snapshot_sha256")?;
    Ok(snapshot)
}

pub fn validate_snapshot(snapshot: &ProjectSnapshot) -> Result<(), LedgerError> {
    if snapshot.schema_version != SNAPSHOT_SCHEMA {
        return Err(LedgerError::Contract("unsupported snapshot schema".into()));
    }
    if snapshot.status != SnapshotStatus::Certified {
        return Err(LedgerError::Contract("snapshot is not certified".into()));
    }
    validate_digest(&snapshot.spec_sha256, "snapshot spec")?;
    validate_digest(&snapshot.artifact_graph_sha256, "snapshot artifact graph")?;
    validate_digest(&snapshot.snapshot_sha256, "snapshot")?;
    let expected = digest_without(snapshot, "snapshot_sha256")?;
    if expected != snapshot.snapshot_sha256 {
        return Err(LedgerError::Provenance(
            "snapshot digest does not match contents".into(),
        ));
    }
    let required: BTreeSet<&str> = snapshot
        .required_gates
        .iter()
        .map(|gate| gate.gate_id.as_str())
        .collect();
    let mut passing = BTreeSet::new();
    for receipt in &snapshot.evidence {
        if receipt.schema_version != EVIDENCE_SCHEMA {
            return Err(LedgerError::Contract(format!(
                "receipt {} has unsupported schema",
                receipt.receipt_id
            )));
        }
        if evidence_receipt_id(receipt)? != receipt.receipt_id {
            return Err(LedgerError::Provenance(format!(
                "receipt {} has a forged or stale receipt id",
                receipt.receipt_id
            )));
        }
        if receipt.status == EvidenceStatus::Pass {
            passing.insert(receipt.gate_id.as_str());
        }
    }
    if required != passing {
        return Err(LedgerError::Contract(format!(
            "snapshot evidence does not exactly cover required gates: required={required:?} passing={passing:?}"
        )));
    }
    Ok(())
}

pub fn validate_evidence(spec: &ProjectSpec, evidence: &EvidenceBundle) -> Result<(), LedgerError> {
    if evidence.schema_version != EVIDENCE_SCHEMA {
        return Err(LedgerError::Contract("unsupported evidence schema".into()));
    }
    let required: BTreeMap<&str, &GateRequirement> = spec
        .required_gates
        .iter()
        .map(|gate| (gate.gate_id.as_str(), gate))
        .collect();
    let artifacts: BTreeMap<&str, &ArtifactRef> = spec
        .artifact_graph
        .iter()
        .map(|node| (node.artifact.artifact_id.as_str(), &node.artifact))
        .collect();
    let mut seen = BTreeSet::new();
    for receipt in &evidence.receipts {
        if receipt.schema_version != EVIDENCE_SCHEMA {
            return Err(LedgerError::Contract(format!(
                "receipt {} has unsupported schema",
                receipt.receipt_id
            )));
        }
        if evidence_receipt_id(receipt)? != receipt.receipt_id {
            return Err(LedgerError::Provenance(format!(
                "receipt {} has a forged or stale receipt id",
                receipt.receipt_id
            )));
        }
        let Some(gate) = required.get(receipt.gate_id.as_str()) else {
            return Err(LedgerError::Contract(format!(
                "receipt covers undeclared gate {}",
                receipt.gate_id
            )));
        };
        if gate.evidence_kind != receipt.evidence_kind {
            return Err(LedgerError::Contract(format!(
                "receipt {} has wrong evidence kind",
                receipt.receipt_id
            )));
        }
        let Some(artifact) = artifacts.get(receipt.artifact_id.as_str()) else {
            return Err(LedgerError::Contract(format!(
                "receipt references missing artifact {}",
                receipt.artifact_id
            )));
        };
        if artifact.sha256 != receipt.artifact_sha256 {
            return Err(LedgerError::Provenance(format!(
                "receipt {} artifact digest mismatch",
                receipt.receipt_id
            )));
        }
        validate_digest(&receipt.observed_input_sha256, "receipt input")?;
        if !seen.insert(receipt.gate_id.as_str()) {
            return Err(LedgerError::Contract(format!(
                "duplicate receipt for gate {}",
                receipt.gate_id
            )));
        }
        if receipt.status != EvidenceStatus::Pass {
            return Err(LedgerError::Contract(format!(
                "required gate {} did not pass",
                receipt.gate_id
            )));
        }
    }
    let required_ids: BTreeSet<&str> = required.keys().copied().collect();
    if required_ids != seen {
        return Err(LedgerError::Contract("evidence is incomplete".into()));
    }
    Ok(())
}

pub fn build_unity_import_manifest(
    spec: &ProjectSpec,
    snapshot: &ProjectSnapshot,
) -> Result<UnityImportManifest, LedgerError> {
    validate_spec(spec)?;
    validate_snapshot(snapshot)?;
    let gameplay_artifact_id = spec.gameplay.runtime_package.artifact_id.clone();
    if !snapshot
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_id == gameplay_artifact_id)
    {
        return Err(LedgerError::Provenance(
            "gameplay artifact is absent from snapshot".into(),
        ));
    }
    Ok(UnityImportManifest {
        schema_version: UNITY_IMPORT_SCHEMA.to_owned(),
        project_id: spec.project_id.clone(),
        snapshot_sha256: snapshot.snapshot_sha256.clone(),
        snapshot_path: "project_snapshot.json".into(),
        target: spec.target.clone(),
        artifacts: snapshot.artifacts.clone(),
        world_id: spec.world.world_id.clone(),
        gameplay_artifact_id,
        required_gates: snapshot.required_gates.clone(),
    })
}

pub fn load_json<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> Result<T, LedgerError> {
    let text = fs::read_to_string(path.as_ref())
        .map_err(|error| LedgerError::Io(format!("{}: {error}", path.as_ref().display())))?;
    serde_json::from_str(&text)
        .map_err(|error| LedgerError::Json(format!("{}: {error}", path.as_ref().display())))
}

pub fn write_json<T: Serialize>(path: impl AsRef<Path>, value: &T) -> Result<(), LedgerError> {
    let value =
        serde_json::to_value(value).map_err(|error| LedgerError::Json(error.to_string()))?;
    let text = serde_json::to_string_pretty(&value)
        .map_err(|error| LedgerError::Json(error.to_string()))?;
    fs::write(path.as_ref(), format!("{text}\n"))
        .map_err(|error| LedgerError::Io(format!("{}: {error}", path.as_ref().display())))
}

fn referenced_artifacts(spec: &ProjectSpec) -> Vec<&ArtifactRef> {
    let mut references = vec![
        &spec.world.terrain,
        &spec.world.collision,
        &spec.world.navigation,
        &spec.gameplay.runtime_package,
        &spec.gameplay.input_trace,
    ];
    for asset in &spec.assets {
        references.push(&asset.source);
        references.push(&asset.runtime_package);
    }
    references
}

fn validate_digest(value: &str, label: &str) -> Result<(), LedgerError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(LedgerError::Provenance(format!(
            "{label} digest must use sha256:<64 hex>"
        )));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(LedgerError::Provenance(format!(
            "{label} digest is not a SHA-256 value"
        )));
    }
    Ok(())
}

fn nonempty(label: &str, value: &str) -> Result<(), LedgerError> {
    if value.trim().is_empty() {
        return Err(LedgerError::Contract(format!("{label} is required")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: &str) -> String {
        sha256_prefixed(seed.as_bytes())
    }

    fn artifact(id: &str, seed: &str) -> ArtifactRef {
        ArtifactRef {
            artifact_id: id.into(),
            kind: "fixture".into(),
            schema_version: "fixture/v1".into(),
            path: format!("fixtures/{id}.bin"),
            sha256: digest(seed),
            producer: "fixture".into(),
        }
    }

    fn spec() -> ProjectSpec {
        let terrain = artifact("terrain", "terrain");
        let collision = artifact("collision", "collision");
        let navigation = artifact("navigation", "navigation");
        let gameplay = artifact("gameplay", "gameplay");
        let input = artifact("input", "input");
        let source = artifact("source", "source");
        let runtime = artifact("runtime", "runtime");
        let all = [
            terrain.clone(),
            collision.clone(),
            navigation.clone(),
            gameplay.clone(),
            input.clone(),
            source.clone(),
            runtime.clone(),
        ];
        ProjectSpec {
            schema_version: PROJECT_SPEC_SCHEMA.into(),
            project_id: "wge-mvp-fixture".into(),
            title: "Fixture Gate Run".into(),
            brief: BriefSpec {
                text: "Traverse the gate, activate the ability, and claim the objective.".into(),
                sources: vec![SourceReference {
                    source_id: "brief".into(),
                    kind: SourceKind::Brief,
                    path: "brief.md".into(),
                    sha256: digest("brief"),
                    region_normalized: None,
                }],
                claims: vec![SemanticClaim {
                    claim_id: "reachable-objective".into(),
                    source_id: "brief".into(),
                    kind: ClaimKind::Constraint,
                    statement: "The objective must be reachable.".into(),
                    confidence: 1.0,
                }],
                conflicts: Vec::new(),
                style_target: StyleTarget {
                    visual_language: "readable stylized highland arena".into(),
                    palette: vec!["moss green".into(), "warm stone".into()],
                    camera: "third-person gameplay distance".into(),
                    reference_source_ids: vec!["brief".into()],
                },
                assumptions: vec![SpecAssumption {
                    assumption_id: "single-process".into(),
                    statement: "The first slice does not require network replication.".into(),
                    reason: "MVP roadmap explicitly defers multiplayer.".into(),
                }],
                design_constraints: vec![DesignConstraint {
                    constraint_id: "playable".into(),
                    category: "mechanical".into(),
                    statement: "The objective must be reachable.".into(),
                    required: true,
                }],
            },
            target: TargetProfile {
                engine: Engine::Unity,
                engine_version: "2022.3".into(),
                platform: "linux-desktop".into(),
                coordinate_system: "right-handed-xz-up-y".into(),
                build_profile: "mvp-debug".into(),
            },
            world: WorldSpec {
                world_id: "gate-run".into(),
                dimensions_m: [64.0, 64.0],
                terrain,
                collision,
                navigation,
                spawns: vec![SpawnPoint {
                    spawn_id: "player".into(),
                    team: "player".into(),
                    position_xz_m: [-20.0, 0.0],
                    required: true,
                }],
                objective: ObjectiveSpec {
                    objective_id: "obelisk".into(),
                    kind: "capture".into(),
                    required_interaction_tag: "can_claim".into(),
                    win_condition: "objective_claimed".into(),
                    loss_condition: "player_defeated".into(),
                },
            },
            assets: vec![AssetBinding {
                asset_id: "hero".into(),
                source,
                runtime_package: runtime,
                role: "character".into(),
                required_features: vec![
                    "rig".into(),
                    "idle".into(),
                    "move".into(),
                    "collision".into(),
                ],
            }],
            gameplay: GameplayBinding {
                runtime_package: gameplay,
                input_trace: input,
                start_entity_id: "player".into(),
                objective_id: "obelisk".into(),
            },
            artifact_graph: all
                .into_iter()
                .map(|artifact| ArtifactNode {
                    artifact,
                    dependencies: Vec::new(),
                })
                .collect(),
            work_orders: vec![WorkOrder {
                schema_version: WORK_ORDER_SCHEMA.into(),
                work_order_id: "build-world".into(),
                operation: "build".into(),
                snapshot_id: "candidate".into(),
                allowed_artifacts: vec!["terrain".into()],
                required_capabilities: vec!["terrain".into()],
                required_gates: vec!["semantic".into()],
            }],
            required_gates: vec![
                GateRequirement {
                    gate_id: "semantic".into(),
                    evidence_kind: "semantic".into(),
                },
                GateRequirement {
                    gate_id: "gameplay".into(),
                    evidence_kind: "runtime".into(),
                },
            ],
        }
    }

    fn evidence(spec: &ProjectSpec) -> EvidenceBundle {
        let artifact = &spec.artifact_graph[0].artifact;
        EvidenceBundle {
            schema_version: EVIDENCE_SCHEMA.into(),
            receipts: spec
                .required_gates
                .iter()
                .map(|gate| {
                    let mut receipt = EvidenceReceipt {
                        schema_version: EVIDENCE_SCHEMA.into(),
                        receipt_id: String::new(),
                        gate_id: gate.gate_id.clone(),
                        evidence_kind: gate.evidence_kind.clone(),
                        status: EvidenceStatus::Pass,
                        artifact_id: artifact.artifact_id.clone(),
                        artifact_sha256: artifact.sha256.clone(),
                        observed_input_sha256: digest("input"),
                        producer: "test".into(),
                        details: BTreeMap::new(),
                    };
                    receipt.receipt_id = evidence_receipt_id(&receipt).unwrap();
                    receipt
                })
                .collect(),
        }
    }

    #[test]
    fn valid_spec_commits_and_unity_manifest_is_deterministic() {
        let spec = spec();
        let evidence = evidence(&spec);
        let snapshot = commit_snapshot(&spec, &evidence).unwrap();
        validate_snapshot(&snapshot).unwrap();
        let manifest = build_unity_import_manifest(&spec, &snapshot).unwrap();
        let first = canonical_json(&serde_json::to_value(&manifest).unwrap());
        let second = canonical_json(
            &serde_json::to_value(build_unity_import_manifest(&spec, &snapshot).unwrap()).unwrap(),
        );
        assert_eq!(first, second);
        assert_eq!(manifest.snapshot_sha256, snapshot.snapshot_sha256);
    }

    #[test]
    fn failed_receipt_cannot_commit() {
        let spec = spec();
        let mut evidence = evidence(&spec);
        evidence.receipts[0].status = EvidenceStatus::Fail;
        evidence.receipts[0].receipt_id = evidence_receipt_id(&evidence.receipts[0]).unwrap();
        let error = commit_snapshot(&spec, &evidence).unwrap_err();
        assert!(error.to_string().contains("did not pass"));
    }

    #[test]
    fn stale_dependency_is_rejected_and_known_bad_gate_is_real() {
        let mut spec = spec();
        spec.artifact_graph[1].dependencies.push(DependencyRef {
            artifact_id: "terrain".into(),
            sha256: digest("old-terrain"),
        });
        let error = validate_spec(&spec).unwrap_err();
        assert!(error.to_string().contains("digest is stale"));
    }

    #[test]
    fn referenced_artifact_identity_cannot_diverge_from_graph_node() {
        let mut spec = spec();
        spec.world.navigation.path = "fixtures/not-navigation.bin".into();
        let error = validate_spec(&spec).unwrap_err();
        assert!(error.to_string().contains("does not match its canonical"));
    }

    #[test]
    fn dependency_cycles_are_rejected() {
        let mut spec = spec();
        let first = spec.artifact_graph[0].artifact.clone();
        let second = spec.artifact_graph[1].artifact.clone();
        spec.artifact_graph[0].dependencies.push(DependencyRef {
            artifact_id: second.artifact_id,
            sha256: second.sha256,
        });
        spec.artifact_graph[1].dependencies.push(DependencyRef {
            artifact_id: first.artifact_id,
            sha256: first.sha256,
        });
        let error = validate_spec(&spec).unwrap_err();
        assert!(error.to_string().contains("cycle"));
    }

    #[test]
    fn forged_receipt_id_is_rejected() {
        let spec = spec();
        let mut evidence = evidence(&spec);
        evidence.receipts[0].receipt_id = "receipt_forged".into();
        let error = commit_snapshot(&spec, &evidence).unwrap_err();
        assert!(error.to_string().contains("forged or stale"));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let mut value = serde_json::to_value(spec()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("silent_repair".into(), Value::Bool(true));
        let error = serde_json::from_value::<ProjectSpec>(value).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }
}
