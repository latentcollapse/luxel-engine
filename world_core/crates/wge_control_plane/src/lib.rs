//! Canonical native WGE project transaction and agent operation surface.
//!
//! This crate owns durable transaction state and capability checks.  Domain
//! validation remains in `wge-project-ledger` and
//! `wge-certification-authority`; the control plane only composes those native
//! contracts and moves the current pointer after an independently validated
//! certification report.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wge_certification_authority::{
    ArtifactBytes, CandidateContext, CertificationStatus, GateRequirement, ReceiptEnvelope,
    ValidationReport, ValidationRequest, ValidatorRegistry, candidate_identity,
    canonical_json as authority_canonical_json, engine_neutral_gate_profile,
    native_mvp_gate_profile, sha256_prefixed as authority_sha256_prefixed,
    validate_candidate_identity, validate_digest, validate_request,
};
use wge_project_ledger::{
    ProjectSpec, WorkOrder, semantic_spec_digest, spec_digest, validate_spec,
    validate_work_order_binding,
};

pub const STORE_SCHEMA: &str = "wge.project-transaction/v1";
pub const POINTER_SCHEMA: &str = "wge.current-pointer/v1";
pub const CANDIDATE_SCHEMA: &str = "wge.candidate-manifest/v1";
pub const SNAPSHOT_SCHEMA: &str = "wge.certified-snapshot/v1";
pub const WORK_RESULT_SCHEMA: &str = "wge.work-order-result/v2";
pub const WORK_RECEIPT_SCHEMA: &str = "wge.work-order-receipt/v2";
pub const WORK_PROPOSAL_SCHEMA: &str = "wge.work-order-proposal/v1";
pub const PLAYTEST_SCHEMA: &str = "wge.reference-playtest/v1";
pub const CAPTURE_SCHEMA: &str = "wge.reference-capture/v1";
pub const REPAIR_PROPOSAL_SCHEMA: &str = "wge.repair-proposal-request/v1";
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPlaneError(pub String);

impl std::fmt::Display for ControlPlaneError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ControlPlaneError {}

type Result<T> = std::result::Result<T, ControlPlaneError>;

fn error(message: impl Into<String>) -> ControlPlaneError {
    ControlPlaneError(message.into())
}

fn map_json(parse_error: impl std::fmt::Display) -> ControlPlaneError {
    error(format!("malformed control-plane JSON: {parse_error}"))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = read_bounded(path)?;
    serde_json::from_slice(&bytes).map_err(map_json)
}

fn read_project_json<T: for<'de> Deserialize<'de>>(
    root: &Path,
    relative: &str,
    label: &str,
) -> Result<T> {
    let path = existing_path_under(root, relative, label)?;
    read_json(&path)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| error(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(parent)
        .map_err(|e| error(format!("cannot create {}: {e}", parent.display())))?;
    let (temporary, mut file) = (0..32)
        .find_map(|_| {
            let sequence = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
            let temporary = path.with_extension(format!("tmp-{}-{sequence}", std::process::id()));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => Some(Ok((temporary, file))),
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(source) => Some(Err(error(format!(
                    "cannot create {}: {source}",
                    temporary.display()
                )))),
            }
        })
        .transpose()?
        .ok_or_else(|| error("could not allocate a unique atomic-write staging file"))?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|e| error(format!("cannot write {}: {e}", temporary.display())))?;
        file.sync_all()
            .map_err(|e| error(format!("cannot sync {}: {e}", temporary.display())))?;
        drop(file);
        fs::rename(&temporary, path)
            .map_err(|e| error(format!("cannot atomically replace {}: {e}", path.display())))?;
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(map_json)?;
    let mut canonical = bytes;
    canonical.push(b'\n');
    write_bytes_atomic(path, &canonical)
}

fn canonical_hash<T: Serialize>(value: &T) -> Result<String> {
    let json = serde_json::to_value(value).map_err(map_json)?;
    Ok(authority_sha256_prefixed(
        authority_canonical_json(&json).as_bytes(),
    ))
}

fn validate_repair_proposal_binding(
    candidate: &StoredCandidate,
    proposal: &RepairProposal,
    failed_layers: &[String],
) -> Result<()> {
    if proposal.schema_version != REPAIR_PROPOSAL_SCHEMA
        || proposal.candidate_id != candidate.candidate_id
        || proposal.candidate_sha256 != candidate.manifest.candidate_sha256
        || proposal.status != "diagnosis_only"
        || proposal.next_step != "submit_a_bounded_repair_work_order"
        || proposal.failed_layers != failed_layers
        || proposal.authorized_artifact_ids != candidate.manifest.authorized_repair_artifact_ids
        || proposal.proposal_sha256 != repair_proposal_digest(proposal)?
    {
        return Err(error(
            "repair proposal is stale, forged, or bound to another candidate",
        ));
    }
    Ok(())
}

fn repair_proposal_digest(proposal: &RepairProposal) -> Result<String> {
    let mut value = serde_json::to_value(proposal).map_err(map_json)?;
    value
        .as_object_mut()
        .ok_or_else(|| error("repair proposal is not an object"))?
        .remove("proposal_sha256");
    Ok(authority_sha256_prefixed(
        authority_canonical_json(&value).as_bytes(),
    ))
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes =
        fs::read(path).map_err(|e| error(format!("cannot read {}: {e}", path.display())))?;
    Ok(authority_sha256_prefixed(&bytes))
}

fn safe_relative(value: &str, label: &str) -> Result<()> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || value.contains('\\')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(error(format!("{label} must be an in-root relative path")));
    }
    Ok(())
}

/// Resolve a supplied relative resource and prove that symlink resolution
/// still leaves it below `root`. A lexical `..` check alone is insufficient
/// for an artifact directory controlled by another process.
fn existing_path_under(root: &Path, relative: &str, label: &str) -> Result<PathBuf> {
    safe_relative(relative, label)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|e| error(format!("cannot resolve {label} root: {e}")))?;
    let resolved = canonical_root
        .join(relative)
        .canonicalize()
        .map_err(|e| error(format!("cannot resolve {label}: {e}")))?;
    if resolved != canonical_root && !resolved.starts_with(&canonical_root) {
        return Err(error(format!("{label} escapes its trusted root")));
    }
    Ok(resolved)
}

fn safe_id(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty()
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        return Err(error(format!(
            "{label} is not a safe transaction identifier"
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoreManifest {
    pub schema_version: String,
    pub project_id: String,
    pub profile_id: String,
    pub project_spec_sha256: String,
    pub semantic_spec_sha256: String,
    pub registry_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_snapshot_id: Option<String>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub snapshot_id: String,
    pub candidate_sha256: String,
    pub snapshot_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CurrentPointer {
    pub schema_version: String,
    pub snapshot_id: String,
    pub snapshot_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CandidateManifest {
    pub project_id: String,
    pub snapshot_id: String,
    pub candidate_sha256: String,
    pub artifact_root: String,
    pub authorized_repair_artifact_ids: Vec<String>,
    pub artifacts: Vec<ArtifactManifest>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifest {
    pub artifact_id: String,
    pub kind: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoredCandidate {
    pub schema_version: String,
    pub candidate_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_snapshot_id: Option<String>,
    pub manifest: CandidateManifest,
    pub artifact_root: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_report_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CertifiedSnapshot {
    pub schema_version: String,
    pub snapshot_id: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_snapshot_id: Option<String>,
    pub candidate_sha256: String,
    pub project_spec_sha256: String,
    pub semantic_spec_sha256: String,
    pub certification_report_sha256: String,
    pub certification_status: CertificationStatus,
    #[serde(default)]
    pub history_depth: usize,
    pub snapshot_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderResult {
    pub schema_version: String,
    pub work_order_id: String,
    pub parent_snapshot_id: String,
    pub output_artifacts: Vec<String>,
    pub writes: Vec<WorkWrite>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkWrite {
    pub artifact_id: String,
    pub path: String,
    pub sha256: String,
    pub byte_length: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderReceipt {
    pub schema_version: String,
    pub work_order_id: String,
    pub parent_snapshot_id: String,
    pub result_sha256: String,
    pub outputs: Vec<String>,
    pub writes: Vec<WorkWrite>,
    pub receipt_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderProposal {
    pub schema_version: String,
    pub work_order_id: String,
    pub parent_snapshot_id: String,
    pub order_sha256: String,
    pub granted_capabilities: Vec<String>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepairProposal {
    pub schema_version: String,
    pub candidate_id: String,
    pub candidate_sha256: String,
    pub failed_layers: Vec<String>,
    pub authorized_artifact_ids: Vec<String>,
    pub status: String,
    pub next_step: String,
    pub proposal_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RequestFile {
    schema_version: String,
    current_snapshot_id: String,
    candidates: Vec<RequestCandidate>,
    gates: Vec<GateRequirement>,
    receipts: Vec<ReceiptEnvelope>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RequestCandidate {
    project_id: String,
    snapshot_id: String,
    candidate_sha256: String,
    artifact_root: String,
    authorized_repair_artifact_ids: Vec<String>,
    artifacts: Vec<ArtifactManifest>,
}

pub struct ProjectStore {
    root: PathBuf,
    manifest: StoreManifest,
    spec: ProjectSpec,
}

impl ProjectStore {
    pub fn create(root: &Path, spec_path: &Path, profile_id: &str) -> Result<Self> {
        if root.join("store.json").exists() {
            return Err(error("project transaction already exists"));
        }
        let spec: ProjectSpec = read_json(spec_path)?;
        validate_spec(&spec).map_err(|e| error(e.to_string()))?;
        if profile_id != "engine-neutral" && profile_id != "native-mvp" {
            return Err(error("profile_id must be engine-neutral or native-mvp"));
        }
        fs::create_dir_all(root).map_err(|e| error(format!("cannot create project root: {e}")))?;
        fs::create_dir_all(root.join("candidates"))
            .map_err(|e| error(format!("cannot create candidates directory: {e}")))?;
        fs::create_dir_all(root.join("snapshots"))
            .map_err(|e| error(format!("cannot create snapshots directory: {e}")))?;
        fs::create_dir_all(root.join("work-orders"))
            .map_err(|e| error(format!("cannot create work-orders directory: {e}")))?;
        let project_spec_path = root.join("project-spec.json");
        write_atomic(&project_spec_path, &spec)?;
        let registry = match profile_id {
            "native-mvp" => ValidatorRegistry::wge_native_mvp_v1(),
            _ => ValidatorRegistry::wge_engine_neutral_v1(),
        };
        let manifest = StoreManifest {
            schema_version: STORE_SCHEMA.into(),
            project_id: spec.project_id.clone(),
            profile_id: profile_id.into(),
            project_spec_sha256: spec_digest(&spec).map_err(|e| error(e.to_string()))?,
            semantic_spec_sha256: semantic_spec_digest(&spec).map_err(|e| error(e.to_string()))?,
            registry_sha256: registry.digest().into(),
            current_snapshot_id: None,
            history: Vec::new(),
        };
        // The store manifest is the creation sentinel. Write it after its
        // immutable specification and required directories so an interrupted
        // create can be retried without opening a half-created transaction.
        write_atomic(&root.join("store.json"), &manifest)?;
        Self::open(root)
    }

    pub fn open(root: &Path) -> Result<Self> {
        let manifest: StoreManifest = read_project_json(root, "store.json", "store manifest")?;
        if manifest.schema_version != STORE_SCHEMA {
            return Err(error("unsupported project transaction schema"));
        }
        safe_id(&manifest.project_id, "project_id")?;
        let spec: ProjectSpec =
            read_project_json(root, "project-spec.json", "project specification")?;
        validate_spec(&spec).map_err(|e| error(e.to_string()))?;
        if spec.project_id != manifest.project_id
            || spec_digest(&spec).map_err(|e| error(e.to_string()))? != manifest.project_spec_sha256
            || semantic_spec_digest(&spec).map_err(|e| error(e.to_string()))?
                != manifest.semantic_spec_sha256
        {
            return Err(error(
                "project specification identity does not match store manifest",
            ));
        }
        let expected_registry = match manifest.profile_id.as_str() {
            "native-mvp" => ValidatorRegistry::wge_native_mvp_v1(),
            "engine-neutral" => ValidatorRegistry::wge_engine_neutral_v1(),
            _ => return Err(error("store profile is not registered")),
        };
        if manifest.registry_sha256 != expected_registry.digest() {
            return Err(error("store registry digest is stale or forged"));
        }
        for directory in ["candidates", "snapshots", "work-orders"] {
            let resolved = existing_path_under(root, directory, "transaction directory")?;
            if !resolved.is_dir() {
                return Err(error(format!("transaction {directory} is not a directory")));
            }
        }
        validate_store_history(root, &manifest)?;
        let pointer_path = root.join("current-pointer.json");
        let pointer_exists = match fs::symlink_metadata(&pointer_path) {
            Ok(_) => true,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => false,
            Err(source) => return Err(error(format!("cannot inspect current pointer: {source}"))),
        };
        match (&manifest.current_snapshot_id, pointer_exists) {
            (None, false) => {}
            (Some(expected), true) => {
                let pointer: CurrentPointer =
                    read_project_json(root, "current-pointer.json", "current pointer")?;
                if pointer.schema_version != POINTER_SCHEMA || &pointer.snapshot_id != expected {
                    return Err(error("current pointer disagrees with store manifest"));
                }
                let snapshot: CertifiedSnapshot = read_project_json(
                    root,
                    &format!("snapshots/{expected}.json"),
                    "current snapshot",
                )?;
                validate_snapshot_record(&snapshot)?;
                if snapshot.snapshot_sha256 != pointer.snapshot_sha256 {
                    return Err(error("current pointer snapshot digest is stale"));
                }
            }
            _ => return Err(error("current pointer and store manifest disagree")),
        }
        let store = Self {
            root: root.to_path_buf(),
            manifest,
            spec,
        };
        if let Some(current_snapshot_id) = &store.manifest.current_snapshot_id {
            store.revalidate_snapshot_certification(current_snapshot_id)?;
        }
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn manifest(&self) -> &StoreManifest {
        &self.manifest
    }
    pub fn spec(&self) -> &ProjectSpec {
        &self.spec
    }

    pub fn inspect_project(&self) -> Value {
        json!({
            "schema_version": STORE_SCHEMA,
            "project_id": self.manifest.project_id,
            "profile_id": self.manifest.profile_id,
            "project_spec_sha256": self.manifest.project_spec_sha256,
            "semantic_spec_sha256": self.manifest.semantic_spec_sha256,
            "current_snapshot_id": self.manifest.current_snapshot_id,
            "history_depth": self.manifest.history.len(),
            "work_order_count": self.spec.work_orders.len(),
            "required_gate_count": self.spec.required_gates.len(),
        })
    }

    pub fn inspect_current(&self) -> Result<Value> {
        let Some(snapshot_id) = &self.manifest.current_snapshot_id else {
            return Ok(json!({"status":"uncommitted","current_snapshot_id":null}));
        };
        let snapshot: CertifiedSnapshot = read_project_json(
            &self.root,
            &format!("snapshots/{snapshot_id}.json"),
            "current snapshot",
        )?;
        serde_json::to_value(snapshot).map_err(map_json)
    }

    pub fn inspect_failures(&self, candidate_id: &str) -> Result<Value> {
        let candidate = self.load_candidate_record(candidate_id)?;
        let candidate_root = existing_path_under(
            &self.root.join("candidates"),
            candidate_id,
            "candidate directory",
        )?;
        let report_path = candidate_root.join("validation-report.json");
        match fs::symlink_metadata(&report_path) {
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(json!({"status":"not_validated","candidate_id":candidate_id}));
            }
            Err(source) => {
                return Err(error(format!("cannot inspect validation report: {source}")));
            }
        }
        let report_path = existing_path_under(
            &candidate_root,
            "validation-report.json",
            "candidate validation report",
        )?;
        let persisted: ValidationReport = read_json(&report_path)?;
        let persisted_hash = canonical_hash(&persisted)?;
        if candidate.validation_report_sha256.as_deref() != Some(persisted_hash.as_str()) {
            return Err(error(
                "stored validation report is not bound to the candidate record",
            ));
        }
        let report = self.validate_candidate(candidate_id)?;
        if report != persisted {
            return Err(error(
                "stored validation report does not match native revalidation",
            ));
        }
        Ok(json!({
            "status": format!("{:?}", report.status).to_lowercase(),
            "candidate_id": candidate.candidate_id,
            "reasons": report.reasons,
            "receipts": report.receipts,
        }))
    }

    /// Inspect an artifact through the stored candidate context. The native
    /// authority rechecks the bytes and digest before any metadata is exposed;
    /// this is not a producer-supplied status lookup.
    pub fn inspect_artifact(&self, candidate_id: &str, artifact_id: &str) -> Result<Value> {
        safe_id(artifact_id, "artifact_id")?;
        let candidate = self.load_candidate_record(candidate_id)?;
        let context = self.candidate_context(&candidate)?;
        let descriptor = candidate
            .manifest
            .artifacts
            .iter()
            .find(|artifact| artifact.artifact_id == artifact_id)
            .ok_or_else(|| error("artifact is not declared by the candidate"))?;
        let bytes = context
            .artifacts
            .get(artifact_id)
            .ok_or_else(|| error("artifact is absent from the validated candidate context"))?;
        Ok(json!({
            "schema_version": "wge.artifact-inspection/v1",
            "candidate_id": candidate.candidate_id,
            "artifact_id": descriptor.artifact_id,
            "kind": descriptor.kind,
            "sha256": descriptor.sha256,
            "byte_length": bytes.bytes.len(),
            "authority_revalidated": true,
        }))
    }

    /// Authorize a bounded work order without executing it. The proposal is
    /// content-addressed and persisted as an immutable audit record; execution
    /// still requires a separately verified worker result.
    pub fn propose_work_order(
        &self,
        work_order: &WorkOrder,
        granted_capabilities: &BTreeSet<String>,
    ) -> Result<WorkOrderProposal> {
        self.ensure_fresh()?;
        let parent = self
            .manifest
            .current_snapshot_id
            .as_deref()
            .unwrap_or("GENESIS");
        self.validate_work_order(work_order, parent, granted_capabilities)?;
        let proposal = WorkOrderProposal {
            schema_version: WORK_PROPOSAL_SCHEMA.into(),
            work_order_id: work_order.work_order_id.clone(),
            parent_snapshot_id: parent.into(),
            order_sha256: canonical_hash(work_order)?,
            granted_capabilities: granted_capabilities.iter().cloned().collect(),
            status: "authorized_pending_result".into(),
        };
        let path = self
            .root
            .join("work-orders")
            .join(format!("{}.proposal.json", work_order.work_order_id));
        if path.is_file() {
            let existing: WorkOrderProposal = read_json(&path)?;
            if existing != proposal {
                return Err(error(
                    "work-order proposal already exists with a different identity",
                ));
            }
        } else {
            write_atomic(&path, &proposal)?;
        }
        Ok(proposal)
    }

    /// Run the reference traversal from the candidate's world artifact. The
    /// result is newly computed by Rust and is never accepted merely because a
    /// candidate contains a pass-shaped traversal file.
    pub fn run_playtest(&self, candidate_id: &str, world_artifact_id: &str) -> Result<Value> {
        let world = self.load_world_artifact(candidate_id, world_artifact_id)?;
        let traversal = wge_reference_runtime::run_playthrough(&world)
            .map_err(|e| error(format!("reference playtest failed: {e}")))?;
        wge_reference_runtime::validate_traversal_evidence(&world, &traversal)
            .map_err(|e| error(format!("reference playtest revalidation failed: {e}")))?;
        Ok(json!({
            "schema_version": PLAYTEST_SCHEMA,
            "world_artifact_id": world.artifact_id,
            "world_artifact_sha256": world.artifact_sha256,
            "traversal_evidence_sha256": traversal.evidence_sha256,
            "outcome": traversal.body.outcome,
            "steps": traversal.body.steps.len(),
            "visited_encounter_ids": traversal.body.visited_encounter_ids,
            "authority_revalidated": true,
        }))
    }

    /// Produce fresh deterministic reference capture evidence into a bounded
    /// project-relative directory. The bytes can then be included in a new
    /// candidate manifest and independently promoted by the receipt registry.
    pub fn capture_evidence(
        &self,
        candidate_id: &str,
        world_artifact_id: &str,
        output_relative: &str,
    ) -> Result<Value> {
        let world = self.load_world_artifact(candidate_id, world_artifact_id)?;
        let (capture, visual) = wge_reference_runtime::render_reference_capture(&world)
            .map_err(|e| error(format!("reference capture failed: {e}")))?;
        let output = self.writable_directory(output_relative)?;
        let capture_path = output.join("reference_capture.ppm");
        write_bytes_atomic(&capture_path, &capture)?;
        write_atomic(&output.join("visual_evidence.json"), &visual)?;
        let manifest = json!({
            "schema_version": CAPTURE_SCHEMA,
            "candidate_id": candidate_id,
            "world_artifact_id": world.artifact_id,
            "world_artifact_sha256": world.artifact_sha256,
            "capture_path": format!("{output_relative}/reference_capture.ppm"),
            "capture_sha256": visual.body.capture_sha256,
            "visual_evidence_path": format!("{output_relative}/visual_evidence.json"),
            "visual_evidence_sha256": visual.evidence_sha256,
            "status": visual.body.status,
            "authority_revalidated": true,
        });
        write_atomic(&output.join("capture-manifest.json"), &manifest)?;
        Ok(manifest)
    }

    pub fn propose_repair(&self, candidate_id: &str) -> Result<RepairProposal> {
        self.ensure_fresh()?;
        let failures = self.inspect_failures(candidate_id)?;
        if failures.get("status").and_then(Value::as_str) != Some("rejected") {
            return Err(error(
                "repair proposals require a natively revalidated failed candidate",
            ));
        }
        let candidate = self.load_candidate_record(candidate_id)?;
        let failed_layers = failures
            .get("reasons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let mut proposal = RepairProposal {
            schema_version: REPAIR_PROPOSAL_SCHEMA.into(),
            candidate_id: candidate.candidate_id,
            candidate_sha256: candidate.manifest.candidate_sha256,
            failed_layers,
            authorized_artifact_ids: candidate.manifest.authorized_repair_artifact_ids,
            status: "diagnosis_only".into(),
            next_step: "submit_a_bounded_repair_work_order".into(),
            proposal_sha256: String::new(),
        };
        proposal.proposal_sha256 = repair_proposal_digest(&proposal)?;
        write_atomic(
            &self
                .root
                .join("work-orders")
                .join(format!("{candidate_id}.repair-proposal.json")),
            &proposal,
        )?;
        Ok(proposal)
    }

    /// Apply a worker result under the persisted repair diagnosis. The
    /// candidate identity, proposal digest, parent pointer, and authorized
    /// output set are checked again at application time.
    pub fn execute_repair_work_order(
        &self,
        candidate_id: &str,
        work_order: &WorkOrder,
        result: &WorkOrderResult,
        granted_capabilities: &BTreeSet<String>,
    ) -> Result<WorkOrderReceipt> {
        self.ensure_fresh()?;
        safe_id(candidate_id, "repair candidate_id")?;
        if work_order.snapshot_id != candidate_id {
            return Err(error("repair work order targets a different candidate"));
        }
        let candidate = self.load_candidate_record(candidate_id)?;
        if candidate.parent_snapshot_id != self.manifest.current_snapshot_id {
            return Err(error(
                "repair candidate is stale relative to the current snapshot",
            ));
        }
        let proposal_path = self
            .root
            .join("work-orders")
            .join(format!("{candidate_id}.repair-proposal.json"));
        let proposal: RepairProposal = read_json(&proposal_path)
            .map_err(|_| error("repair candidate has no valid persisted repair proposal"))?;
        let failures = self.inspect_failures(candidate_id)?;
        if failures.get("status").and_then(Value::as_str) != Some("rejected") {
            return Err(error(
                "repair application requires a natively revalidated failed candidate",
            ));
        }
        let failed_layers = failures
            .get("reasons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        validate_repair_proposal_binding(&candidate, &proposal, &failed_layers)?;
        let authorized = proposal
            .authorized_artifact_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if work_order
            .output_artifacts
            .iter()
            .any(|artifact_id| !authorized.contains(artifact_id.as_str()))
        {
            return Err(error(
                "repair work order writes outside the authorized repair target set",
            ));
        }
        self.execute_work_order(work_order, result, granted_capabilities)
    }

    pub fn create_candidate(
        &self,
        manifest_path: &Path,
        external_artifact_root: &Path,
        parent_snapshot_id: Option<&str>,
    ) -> Result<StoredCandidate> {
        self.ensure_fresh()?;
        let manifest: CandidateManifest = read_json(manifest_path)?;
        if manifest.project_id != self.manifest.project_id {
            return Err(error("candidate project_id does not match the transaction"));
        }
        safe_id(&manifest.snapshot_id, "candidate snapshot_id")?;
        let expected_parent = self.manifest.current_snapshot_id.as_deref();
        if parent_snapshot_id != expected_parent {
            return Err(error(
                "candidate parent is stale relative to the current pointer",
            ));
        }
        let source_root = external_artifact_root
            .canonicalize()
            .map_err(|e| error(format!("cannot resolve candidate artifact root: {e}")))?;
        let context = load_external_candidate(&manifest, &source_root)?;
        validate_candidate_identity(&context).map_err(|e| error(e.to_string()))?;
        if manifest.candidate_sha256
            != candidate_identity(&context).map_err(|e| error(e.to_string()))?
        {
            return Err(error("candidate manifest identity is stale"));
        }
        let candidates_dir = existing_path_under(&self.root, "candidates", "candidates directory")?;
        let candidate_dir = candidates_dir.join(&manifest.snapshot_id);
        if candidate_dir.exists() {
            return Err(error("candidate snapshot_id already exists"));
        }
        let staging_dir = create_staging_directory(&candidates_dir, &manifest.snapshot_id)?;
        let staged = (|| {
            let artifact_dir = staging_dir.join("artifacts");
            fs::create_dir(&artifact_dir)
                .map_err(|e| error(format!("cannot create candidate artifact root: {e}")))?;
            let mut internal_artifacts = Vec::with_capacity(manifest.artifacts.len());
            for artifact in &manifest.artifacts {
                safe_id(&artifact.artifact_id, "artifact_id")?;
                let bytes = context
                    .artifacts
                    .get(&artifact.artifact_id)
                    .ok_or_else(|| error("candidate artifact binding disappeared"))?;
                let destination = artifact_dir.join(format!("{}.bin", artifact.artifact_id));
                write_bytes_atomic(&destination, &bytes.bytes)?;
                internal_artifacts.push(ArtifactManifest {
                    artifact_id: artifact.artifact_id.clone(),
                    kind: artifact.kind.clone(),
                    path: format!("artifacts/{}.bin", artifact.artifact_id),
                    sha256: artifact.sha256.clone(),
                });
            }
            let internal_manifest = CandidateManifest {
                artifact_root: ".".into(),
                artifacts: internal_artifacts,
                ..manifest
            };
            let record = StoredCandidate {
                schema_version: CANDIDATE_SCHEMA.into(),
                candidate_id: internal_manifest.snapshot_id.clone(),
                parent_snapshot_id: expected_parent.map(str::to_owned),
                manifest: internal_manifest,
                artifact_root: ".".into(),
                status: "candidate".into(),
                validation_report_sha256: None,
            };
            write_atomic(&staging_dir.join("candidate.json"), &record)?;
            Ok(record)
        })();
        let record = match staged {
            Ok(record) => record,
            Err(failure) => {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(failure);
            }
        };
        if let Err(failure) = fs::rename(&staging_dir, &candidate_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error(format!(
                "cannot publish candidate atomically: {failure}"
            )));
        }
        Ok(record)
    }

    pub fn attach_evidence(&self, candidate_id: &str, request_path: &Path) -> Result<()> {
        self.ensure_fresh()?;
        let candidate = self.load_candidate_record(candidate_id)?;
        let request: RequestFile = read_json(request_path)?;
        validate_request_shape(&request)?;
        let matching = request
            .candidates
            .iter()
            .find(|item| item.snapshot_id == candidate.manifest.snapshot_id)
            .ok_or_else(|| error("evidence request does not contain the candidate"))?;
        if matching.project_id != candidate.manifest.project_id
            || matching.candidate_sha256 != candidate.manifest.candidate_sha256
        {
            return Err(error(
                "evidence request candidate identity does not match stored candidate",
            ));
        }
        validate_request_candidate_binding(matching, &candidate.manifest)?;
        write_atomic(
            &self
                .root
                .join("candidates")
                .join(candidate_id)
                .join("evidence-request.json"),
            &serde_json::to_value(&request).map_err(map_json)?,
        )
    }

    pub fn validate_candidate(&self, candidate_id: &str) -> Result<ValidationReport> {
        let candidate = self.load_candidate_record(candidate_id)?;
        let request_path = existing_path_under(
            &self.root.join("candidates").join(candidate_id),
            "evidence-request.json",
            "candidate evidence request",
        )?;
        let request: RequestFile = read_json(&request_path)?;
        validate_request_shape(&request)?;
        if request.current_snapshot_id != candidate.manifest.snapshot_id {
            return Err(error(
                "evidence request is not bound to the candidate snapshot",
            ));
        }
        let registry = self.registry();
        let mut contexts = Vec::new();
        for wire in &request.candidates {
            let stored = self.load_candidate_record(&wire.snapshot_id)?;
            if stored.manifest.project_id != wire.project_id
                || stored.manifest.candidate_sha256 != wire.candidate_sha256
            {
                return Err(error("request candidate diverges from stored candidate"));
            }
            validate_request_candidate_binding(wire, &stored.manifest)?;
            contexts.push(self.candidate_context(&stored)?);
        }
        let native = ValidationRequest {
            current_snapshot_id: request.current_snapshot_id,
            candidates: contexts,
            gates: request.gates,
            receipts: request.receipts,
        };
        let report = validate_request(&native, &registry).map_err(|e| error(e.to_string()))?;
        if report.status != CertificationStatus::Rejected
            && report.status != self.expected_certification_status()
        {
            return Err(error(
                "certification report does not match the transaction profile",
            ));
        }
        write_atomic(
            &self
                .root
                .join("candidates")
                .join(candidate_id)
                .join("validation-report.json"),
            &report,
        )?;
        let mut updated = candidate;
        updated.status = if report.status == CertificationStatus::Rejected {
            "rejected".into()
        } else {
            "validated".into()
        };
        updated.validation_report_sha256 = Some(canonical_hash(&report)?);
        write_atomic(
            &self
                .root
                .join("candidates")
                .join(candidate_id)
                .join("candidate.json"),
            &updated,
        )?;
        Ok(report)
    }

    pub fn commit_candidate(&mut self, candidate_id: &str) -> Result<CertifiedSnapshot> {
        self.ensure_fresh()?;
        let report = self.validate_candidate(candidate_id)?;
        if report.status == CertificationStatus::Rejected {
            return Err(error(
                "candidate is not certified; current pointer was not moved",
            ));
        }
        let candidate = self.load_candidate_record(candidate_id)?;
        if candidate.parent_snapshot_id.as_deref() != self.manifest.current_snapshot_id.as_deref() {
            return Err(error(
                "candidate parent is stale; current pointer was not moved",
            ));
        }
        let report_sha256 = canonical_hash(&report)?;
        let mut snapshot = CertifiedSnapshot {
            schema_version: SNAPSHOT_SCHEMA.into(),
            snapshot_id: candidate.manifest.snapshot_id.clone(),
            project_id: self.manifest.project_id.clone(),
            parent_snapshot_id: candidate.parent_snapshot_id.clone(),
            candidate_sha256: candidate.manifest.candidate_sha256.clone(),
            project_spec_sha256: self.manifest.project_spec_sha256.clone(),
            semantic_spec_sha256: self.manifest.semantic_spec_sha256.clone(),
            certification_report_sha256: report_sha256,
            certification_status: report.status.clone(),
            history_depth: self.manifest.history.len() + 1,
            snapshot_sha256: String::new(),
        };
        snapshot.snapshot_sha256 = snapshot_digest(&snapshot)?;
        write_atomic(
            &self
                .root
                .join("snapshots")
                .join(format!("{}.json", snapshot.snapshot_id)),
            &snapshot,
        )?;
        let previous_manifest = self.manifest.clone();
        let mut next_manifest = previous_manifest.clone();
        next_manifest.current_snapshot_id = Some(snapshot.snapshot_id.clone());
        next_manifest.history.push(HistoryEntry {
            snapshot_id: snapshot.snapshot_id.clone(),
            candidate_sha256: snapshot.candidate_sha256.clone(),
            snapshot_sha256: snapshot.snapshot_sha256.clone(),
        });
        // The pointer is published last. If that final atomic replacement
        // fails, restore the old manifest so reopen observes the old state.
        write_atomic(&self.root.join("store.json"), &next_manifest)?;
        let pointer = CurrentPointer {
            schema_version: POINTER_SCHEMA.into(),
            snapshot_id: snapshot.snapshot_id.clone(),
            snapshot_sha256: snapshot.snapshot_sha256.clone(),
        };
        if let Err(pointer_error) = write_atomic(&self.root.join("current-pointer.json"), &pointer)
        {
            if let Err(restore_error) =
                write_atomic(&self.root.join("store.json"), &previous_manifest)
            {
                return Err(error(format!(
                    "pointer publication failed ({pointer_error}) and prior manifest restoration failed ({restore_error}); reopen will reject inconsistent state"
                )));
            }
            return Err(pointer_error);
        }
        self.manifest = next_manifest;
        Ok(snapshot)
    }

    pub fn rollback(&mut self, snapshot_id: &str) -> Result<CertifiedSnapshot> {
        self.ensure_fresh()?;
        safe_id(snapshot_id, "snapshot_id")?;
        if !self
            .manifest
            .history
            .iter()
            .any(|entry| entry.snapshot_id == snapshot_id)
        {
            return Err(error("rollback target is not a certified history entry"));
        }
        let snapshot: CertifiedSnapshot = read_project_json(
            &self.root,
            &format!("snapshots/{snapshot_id}.json"),
            "rollback snapshot",
        )?;
        validate_snapshot_record(&snapshot)?;
        self.revalidate_snapshot_certification(snapshot_id)?;
        let previous_manifest = self.manifest.clone();
        let mut next_manifest = previous_manifest.clone();
        next_manifest.current_snapshot_id = Some(snapshot_id.into());
        write_atomic(&self.root.join("store.json"), &next_manifest)?;
        if let Err(pointer_error) = write_atomic(
            &self.root.join("current-pointer.json"),
            &CurrentPointer {
                schema_version: POINTER_SCHEMA.into(),
                snapshot_id: snapshot.snapshot_id.clone(),
                snapshot_sha256: snapshot.snapshot_sha256.clone(),
            },
        ) {
            if let Err(restore_error) =
                write_atomic(&self.root.join("store.json"), &previous_manifest)
            {
                return Err(error(format!(
                    "rollback pointer publication failed ({pointer_error}) and prior manifest restoration failed ({restore_error}); reopen will reject inconsistent state"
                )));
            }
            return Err(pointer_error);
        }
        self.manifest = next_manifest;
        Ok(snapshot)
    }

    pub fn execute_work_order(
        &self,
        work_order: &WorkOrder,
        result: &WorkOrderResult,
        granted_capabilities: &BTreeSet<String>,
    ) -> Result<WorkOrderReceipt> {
        self.ensure_fresh()?;
        let parent = self
            .manifest
            .current_snapshot_id
            .as_deref()
            .unwrap_or("GENESIS");
        self.validate_work_order(work_order, parent, granted_capabilities)?;
        let proposal_path = self
            .root
            .join("work-orders")
            .join(format!("{}.proposal.json", work_order.work_order_id));
        let proposal: WorkOrderProposal = read_json(&proposal_path)
            .map_err(|_| error("work order has no valid persisted authorization proposal"))?;
        if proposal.schema_version != WORK_PROPOSAL_SCHEMA
            || proposal.work_order_id != work_order.work_order_id
            || proposal.parent_snapshot_id != parent
            || proposal.order_sha256 != canonical_hash(work_order)?
            || proposal.granted_capabilities
                != granted_capabilities.iter().cloned().collect::<Vec<_>>()
            || proposal.status != "authorized_pending_result"
        {
            return Err(error(
                "work-order execution differs from its persisted authorization proposal",
            ));
        }
        if result.schema_version != WORK_RESULT_SCHEMA
            || result.work_order_id != work_order.work_order_id
            || result.parent_snapshot_id != parent
        {
            return Err(error(
                "work-order result is not bound to the authorized order or parent",
            ));
        }
        let expected: BTreeSet<&str> = work_order
            .output_artifacts
            .iter()
            .map(String::as_str)
            .collect();
        if result
            .output_artifacts
            .iter()
            .any(|id| !expected.contains(id.as_str()))
            || result.output_artifacts.is_empty()
            || result.output_artifacts.len() != expected.len()
            || result
                .output_artifacts
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != result.output_artifacts.len()
        {
            return Err(error(
                "work-order result contains an unauthorized output artifact",
            ));
        }
        if result.writes.len() != expected.len() {
            return Err(error(
                "work-order result must bind exactly one file write to each output artifact",
            ));
        }
        let mut write_artifacts = BTreeSet::new();
        let mut write_paths = BTreeSet::new();
        let mut total_bytes = 0u64;
        let scope = work_order
            .write_scope
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        for write in &result.writes {
            if !expected.contains(write.artifact_id.as_str())
                || !write_artifacts.insert(write.artifact_id.as_str())
            {
                return Err(error(
                    "work-order writes contain an undeclared or duplicate artifact",
                ));
            }
            safe_relative(&write.path, "work result path")?;
            if !write_paths.insert(write.path.as_str()) {
                return Err(error("work-order result contains duplicate write paths"));
            }
            if !scope.iter().any(|prefix| {
                write.path == *prefix || write.path.starts_with(&format!("{prefix}/"))
            }) {
                return Err(error(format!(
                    "write {} escapes the work-order scope",
                    write.path
                )));
            }
            let path = existing_path_under(&self.root, &write.path, "work output path")?;
            let metadata = fs::metadata(&path)
                .map_err(|e| error(format!("work output {} is missing: {e}", write.path)))?;
            if !metadata.is_file() {
                return Err(error(format!(
                    "work output {} is not a regular file",
                    write.path
                )));
            }
            if metadata.len() != write.byte_length || sha256_file(&path)? != write.sha256 {
                return Err(error(format!("work output {} has stale bytes", write.path)));
            }
            total_bytes = total_bytes
                .checked_add(write.byte_length)
                .ok_or_else(|| error("work output byte count overflowed"))?;
        }
        let budget = work_order
            .budget
            .as_ref()
            .ok_or_else(|| error("work order must declare an execution budget"))?;
        if result.writes.len() > budget.max_artifacts as usize
            || total_bytes > budget.max_total_bytes
        {
            return Err(error(
                "work-order result exceeds its declared execution budget",
            ));
        }
        let result_sha256 = canonical_hash(result)?;
        let mut receipt = WorkOrderReceipt {
            schema_version: WORK_RECEIPT_SCHEMA.into(),
            work_order_id: work_order.work_order_id.clone(),
            parent_snapshot_id: parent.into(),
            result_sha256,
            outputs: result.output_artifacts.clone(),
            writes: result.writes.clone(),
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = canonical_hash(&receipt)?;
        write_atomic(
            &self
                .root
                .join("work-orders")
                .join(format!("{}.json", work_order.work_order_id)),
            &receipt,
        )?;
        Ok(receipt)
    }

    fn validate_work_order(
        &self,
        work_order: &WorkOrder,
        parent: &str,
        granted_capabilities: &BTreeSet<String>,
    ) -> Result<()> {
        validate_work_order_binding(&self.spec, work_order, parent, granted_capabilities)
            .map_err(|e| error(e.to_string()))?;
        safe_id(&work_order.work_order_id, "work_order_id")?;
        safe_id(&work_order.snapshot_id, "work_order snapshot_id")?;
        let budget = work_order
            .budget
            .as_ref()
            .ok_or_else(|| error("work order must declare an execution budget"))?;
        let mut unique = BTreeSet::new();
        for artifact_id in &work_order.output_artifacts {
            if !unique.insert(artifact_id.as_str()) {
                return Err(error("work order contains duplicate output artifact IDs"));
            }
        }
        if work_order.output_artifacts.len() > budget.max_artifacts as usize {
            return Err(error("work order outputs exceed its artifact budget"));
        }
        for (label, values) in [
            ("input_artifacts", &work_order.input_artifacts),
            ("allowed_artifacts", &work_order.allowed_artifacts),
            ("write_scope", &work_order.write_scope),
            ("required_capabilities", &work_order.required_capabilities),
            ("required_gates", &work_order.required_gates),
        ] {
            let mut seen = BTreeSet::new();
            if values.iter().any(|value| !seen.insert(value.as_str())) {
                return Err(error(format!("work order contains duplicate {label}")));
            }
        }
        let required_capabilities = work_order
            .required_capabilities
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if &required_capabilities != granted_capabilities {
            return Err(error(
                "granted capabilities must exactly match the work-order capability contract",
            ));
        }
        Ok(())
    }

    fn registry(&self) -> ValidatorRegistry {
        if self.manifest.profile_id == "native-mvp" {
            ValidatorRegistry::wge_native_mvp_v1()
        } else {
            ValidatorRegistry::wge_engine_neutral_v1()
        }
    }

    fn expected_certification_status(&self) -> CertificationStatus {
        if self.manifest.profile_id == "native-mvp" {
            CertificationStatus::NativeMvpCertified
        } else {
            CertificationStatus::EngineNeutralCertified
        }
    }

    fn revalidate_snapshot_certification(&self, snapshot_id: &str) -> Result<()> {
        let snapshot: CertifiedSnapshot = read_project_json(
            &self.root,
            &format!("snapshots/{snapshot_id}.json"),
            "certified snapshot",
        )?;
        validate_snapshot_record(&snapshot)?;
        if snapshot.project_id != self.manifest.project_id
            || snapshot.project_spec_sha256 != self.manifest.project_spec_sha256
            || snapshot.semantic_spec_sha256 != self.manifest.semantic_spec_sha256
        {
            return Err(error(
                "snapshot is bound to a different project specification",
            ));
        }
        let candidate = self.load_candidate_record(snapshot_id)?;
        if candidate.manifest.candidate_sha256 != snapshot.candidate_sha256
            || candidate.parent_snapshot_id != snapshot.parent_snapshot_id
        {
            return Err(error("snapshot candidate binding is stale or forged"));
        }
        let report = self.validate_candidate(snapshot_id)?;
        if report.status == CertificationStatus::Rejected
            || report.status != snapshot.certification_status
            || canonical_hash(&report)? != snapshot.certification_report_sha256
            || report.project_id != snapshot.project_id
            || report.snapshot_id != snapshot.snapshot_id
            || report.candidate_sha256 != snapshot.candidate_sha256
        {
            return Err(error(
                "snapshot certification report failed independent native revalidation",
            ));
        }
        Ok(())
    }

    fn load_candidate_record(&self, candidate_id: &str) -> Result<StoredCandidate> {
        safe_id(candidate_id, "candidate_id")?;
        let candidates_root =
            existing_path_under(&self.root, "candidates", "candidates directory")?;
        let candidate_root =
            existing_path_under(&candidates_root, candidate_id, "candidate directory")?;
        let record: StoredCandidate = read_json(&candidate_root.join("candidate.json"))?;
        if record.schema_version != CANDIDATE_SCHEMA || record.candidate_id != candidate_id {
            return Err(error("stored candidate identity is malformed"));
        }
        if record.manifest.snapshot_id != candidate_id {
            return Err(error("stored candidate manifest identity is malformed"));
        }
        validate_candidate_manifest(&record.manifest)?;
        safe_relative(&record.artifact_root, "stored candidate artifact_root")?;
        if let Some(parent) = &record.parent_snapshot_id {
            safe_id(parent, "stored candidate parent_snapshot_id")?;
        }
        Ok(record)
    }

    fn candidate_context(&self, candidate: &StoredCandidate) -> Result<CandidateContext> {
        let root = self.root.join("candidates").join(&candidate.candidate_id);
        let artifact_root = existing_path_under(
            &root,
            &candidate.artifact_root,
            "stored candidate artifact_root",
        )?;
        let mut artifacts = BTreeMap::new();
        for artifact in &candidate.manifest.artifacts {
            let path =
                existing_path_under(&artifact_root, &artifact.path, "candidate artifact path")?;
            let bytes = read_bounded(&path)?;
            if authority_sha256_prefixed(&bytes) != artifact.sha256 {
                return Err(error(format!(
                    "candidate artifact {} is stale",
                    artifact.artifact_id
                )));
            }
            artifacts.insert(
                artifact.artifact_id.clone(),
                ArtifactBytes {
                    kind: artifact.kind.clone(),
                    bytes,
                },
            );
        }
        let context = CandidateContext {
            project_id: candidate.manifest.project_id.clone(),
            snapshot_id: candidate.manifest.snapshot_id.clone(),
            candidate_sha256: candidate.manifest.candidate_sha256.clone(),
            artifacts,
            authorized_repair_artifact_ids: candidate
                .manifest
                .authorized_repair_artifact_ids
                .iter()
                .cloned()
                .collect(),
        };
        validate_candidate_identity(&context).map_err(|e| error(e.to_string()))?;
        Ok(context)
    }

    fn load_world_artifact(
        &self,
        candidate_id: &str,
        artifact_id: &str,
    ) -> Result<wge_reference_runtime::WorldArtifact> {
        let candidate = self.load_candidate_record(candidate_id)?;
        let context = self.candidate_context(&candidate)?;
        let artifact = context
            .artifacts
            .get(artifact_id)
            .ok_or_else(|| error("world artifact is absent from the candidate"))?;
        if artifact.kind != "world_artifact" {
            return Err(error("requested artifact is not a world_artifact"));
        }
        let world: wge_reference_runtime::WorldArtifact =
            serde_json::from_slice(&artifact.bytes).map_err(map_json)?;
        wge_reference_runtime::validate_world_artifact(&world)
            .map_err(|e| error(format!("world artifact failed native validation: {e}")))?;
        Ok(world)
    }

    fn writable_directory(&self, relative: &str) -> Result<PathBuf> {
        safe_relative(relative, "output directory")?;
        let canonical_root = self
            .root
            .canonicalize()
            .map_err(|e| error(format!("cannot resolve project root: {e}")))?;
        let path = canonical_root.join(relative);
        let mut cursor = canonical_root.clone();
        for component in Path::new(relative).components() {
            match component {
                Component::CurDir => continue,
                Component::Normal(part) => cursor.push(part),
                _ => return Err(error("output directory must be an in-root relative path")),
            }
            match fs::symlink_metadata(&cursor) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(error("output directory path may not contain symlinks"));
                }
                Ok(_) => {
                    let resolved = cursor
                        .canonicalize()
                        .map_err(|e| error(format!("cannot resolve output directory: {e}")))?;
                    if !resolved.starts_with(&canonical_root) {
                        return Err(error("output directory escapes the project root"));
                    }
                }
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => break,
                Err(source) => {
                    return Err(error(format!("cannot inspect output directory: {source}")));
                }
            }
        }
        fs::create_dir_all(&path)
            .map_err(|e| error(format!("cannot create output directory: {e}")))?;
        let resolved = path
            .canonicalize()
            .map_err(|e| error(format!("cannot resolve output directory: {e}")))?;
        if !resolved.starts_with(&canonical_root) || !resolved.is_dir() {
            return Err(error("output directory is not a safe project directory"));
        }
        Ok(resolved)
    }

    fn ensure_fresh(&self) -> Result<()> {
        let disk = Self::open(&self.root)?;
        if disk.manifest != self.manifest {
            return Err(error(
                "project transaction changed since this store handle was opened",
            ));
        }
        Ok(())
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let metadata =
        fs::metadata(path).map_err(|e| error(format!("cannot stat {}: {e}", path.display())))?;
    if !metadata.is_file() {
        return Err(error(format!("{} is not a regular file", path.display())));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(error(format!(
            "{} exceeds the artifact byte limit",
            path.display()
        )));
    }
    fs::read(path).map_err(|e| error(format!("cannot read {}: {e}", path.display())))
}

fn create_staging_directory(parent: &Path, stem: &str) -> Result<PathBuf> {
    for _ in 0..32 {
        let sequence = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let staging = parent.join(format!(".{stem}.staging-{}-{sequence}", std::process::id()));
        match fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(error(format!(
                    "cannot create candidate staging directory: {source}"
                )));
            }
        }
    }
    Err(error(
        "could not allocate a unique candidate staging directory",
    ))
}

fn load_external_candidate(manifest: &CandidateManifest, root: &Path) -> Result<CandidateContext> {
    validate_candidate_manifest(manifest)?;
    let artifact_root =
        existing_path_under(root, &manifest.artifact_root, "candidate artifact_root")?;
    let mut artifacts = BTreeMap::new();
    for artifact in &manifest.artifacts {
        let path = existing_path_under(&artifact_root, &artifact.path, "candidate artifact path")?;
        let bytes = read_bounded(&path)?;
        if authority_sha256_prefixed(&bytes) != artifact.sha256 {
            return Err(error(format!(
                "candidate artifact {} digest mismatch",
                artifact.artifact_id
            )));
        }
        artifacts.insert(
            artifact.artifact_id.clone(),
            ArtifactBytes {
                kind: artifact.kind.clone(),
                bytes,
            },
        );
    }
    Ok(CandidateContext {
        project_id: manifest.project_id.clone(),
        snapshot_id: manifest.snapshot_id.clone(),
        candidate_sha256: manifest.candidate_sha256.clone(),
        artifacts,
        authorized_repair_artifact_ids: manifest
            .authorized_repair_artifact_ids
            .iter()
            .cloned()
            .collect(),
    })
}

fn validate_request_shape(request: &RequestFile) -> Result<()> {
    if request.schema_version != "wge.certification-request/v1" {
        return Err(error("unsupported certification request schema"));
    }
    if request.candidates.is_empty() || request.receipts.is_empty() {
        return Err(error(
            "certification request must contain candidates and receipts",
        ));
    }
    safe_id(&request.current_snapshot_id, "current_snapshot_id")?;
    let mut candidate_ids = BTreeSet::new();
    for candidate in &request.candidates {
        if !candidate_ids.insert(candidate.snapshot_id.as_str()) {
            return Err(error("certification request contains duplicate candidates"));
        }
        validate_request_candidate_shape(candidate)?;
    }
    Ok(())
}

fn validate_candidate_manifest(manifest: &CandidateManifest) -> Result<()> {
    safe_id(&manifest.project_id, "candidate project_id")?;
    safe_id(&manifest.snapshot_id, "candidate snapshot_id")?;
    validate_digest(&manifest.candidate_sha256, "candidate").map_err(|e| error(e.to_string()))?;
    safe_relative(&manifest.artifact_root, "candidate artifact_root")?;
    if manifest.artifacts.is_empty() {
        return Err(error("candidate must contain at least one artifact"));
    }
    let mut artifact_ids = BTreeSet::new();
    for artifact in &manifest.artifacts {
        safe_id(&artifact.artifact_id, "artifact_id")?;
        if !artifact_ids.insert(artifact.artifact_id.as_str()) {
            return Err(error("candidate contains duplicate artifact IDs"));
        }
        if artifact.kind.trim().is_empty() {
            return Err(error("candidate artifact kind must not be empty"));
        }
        safe_relative(&artifact.path, "candidate artifact path")?;
        validate_digest(&artifact.sha256, "candidate artifact")
            .map_err(|e| error(e.to_string()))?;
    }
    let mut authorized = BTreeSet::new();
    for artifact_id in &manifest.authorized_repair_artifact_ids {
        safe_id(artifact_id, "authorized repair artifact_id")?;
        if !authorized.insert(artifact_id.as_str()) {
            return Err(error("candidate contains duplicate authorized repair IDs"));
        }
        if !artifact_ids.contains(artifact_id.as_str()) {
            return Err(error(format!(
                "authorized repair artifact {artifact_id} is not declared by the candidate"
            )));
        }
    }
    Ok(())
}

fn validate_request_candidate_shape(candidate: &RequestCandidate) -> Result<()> {
    let manifest = CandidateManifest {
        project_id: candidate.project_id.clone(),
        snapshot_id: candidate.snapshot_id.clone(),
        candidate_sha256: candidate.candidate_sha256.clone(),
        artifact_root: candidate.artifact_root.clone(),
        authorized_repair_artifact_ids: candidate.authorized_repair_artifact_ids.clone(),
        artifacts: candidate.artifacts.clone(),
    };
    validate_candidate_manifest(&manifest)
}

fn validate_request_candidate_binding(
    request: &RequestCandidate,
    stored: &CandidateManifest,
) -> Result<()> {
    validate_request_candidate_shape(request)?;
    if request.project_id != stored.project_id
        || request.snapshot_id != stored.snapshot_id
        || request.candidate_sha256 != stored.candidate_sha256
        || request.authorized_repair_artifact_ids != stored.authorized_repair_artifact_ids
    {
        return Err(error("request candidate diverges from stored candidate"));
    }
    let request_artifacts = request
        .artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.artifact_id.as_str(),
                artifact.kind.as_str(),
                artifact.sha256.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    let stored_artifacts = stored
        .artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.artifact_id.as_str(),
                artifact.kind.as_str(),
                artifact.sha256.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    if request_artifacts != stored_artifacts {
        return Err(error(
            "request candidate artifact manifest diverges from stored candidate",
        ));
    }
    Ok(())
}

fn validate_store_history(root: &Path, manifest: &StoreManifest) -> Result<()> {
    let mut snapshot_ids = BTreeSet::new();
    for (index, entry) in manifest.history.iter().enumerate() {
        safe_id(&entry.snapshot_id, "history snapshot_id")?;
        validate_digest(&entry.candidate_sha256, "history candidate")
            .map_err(|e| error(e.to_string()))?;
        validate_digest(&entry.snapshot_sha256, "history snapshot")
            .map_err(|e| error(e.to_string()))?;
        if !snapshot_ids.insert(entry.snapshot_id.as_str()) {
            return Err(error("store history contains duplicate snapshot IDs"));
        }
        let snapshot: CertifiedSnapshot = read_project_json(
            root,
            &format!("snapshots/{}.json", entry.snapshot_id),
            "history snapshot",
        )?;
        validate_snapshot_record(&snapshot)?;
        if snapshot.project_id != manifest.project_id
            || snapshot.candidate_sha256 != entry.candidate_sha256
            || snapshot.snapshot_sha256 != entry.snapshot_sha256
            || snapshot.history_depth != index + 1
        {
            return Err(error("store history entry disagrees with its snapshot"));
        }
    }
    if let Some(current) = &manifest.current_snapshot_id {
        safe_id(current, "current_snapshot_id")?;
        if !snapshot_ids.contains(current.as_str()) {
            return Err(error("store current snapshot is absent from history"));
        }
    }
    Ok(())
}

fn validate_snapshot_record(snapshot: &CertifiedSnapshot) -> Result<()> {
    if snapshot.schema_version != SNAPSHOT_SCHEMA {
        return Err(error("unsupported certified snapshot schema"));
    }
    if snapshot.snapshot_sha256 != snapshot_digest(snapshot)? {
        return Err(error("certified snapshot digest is forged or stale"));
    }
    safe_id(&snapshot.project_id, "snapshot project_id")?;
    safe_id(&snapshot.snapshot_id, "snapshot_id")?;
    validate_digest(&snapshot.candidate_sha256, "snapshot candidate")
        .map_err(|e| error(e.to_string()))?;
    validate_digest(&snapshot.project_spec_sha256, "snapshot project spec")
        .map_err(|e| error(e.to_string()))?;
    validate_digest(&snapshot.semantic_spec_sha256, "snapshot semantic spec")
        .map_err(|e| error(e.to_string()))?;
    validate_digest(&snapshot.certification_report_sha256, "snapshot report")
        .map_err(|e| error(e.to_string()))?;
    if let Some(parent) = &snapshot.parent_snapshot_id {
        safe_id(parent, "snapshot parent_snapshot_id")?;
    }
    Ok(())
}

fn snapshot_digest(snapshot: &CertifiedSnapshot) -> Result<String> {
    let mut value = serde_json::to_value(snapshot).map_err(map_json)?;
    value
        .as_object_mut()
        .ok_or_else(|| error("certified snapshot is not an object"))?
        .remove("snapshot_sha256");
    Ok(authority_sha256_prefixed(
        authority_canonical_json(&value).as_bytes(),
    ))
}

pub fn profile_gates(profile: &str) -> Result<Vec<GateRequirement>> {
    match profile {
        "native-mvp" => Ok(native_mvp_gate_profile()),
        "engine-neutral" => Ok(engine_neutral_gate_profile()),
        _ => Err(error("profile must be engine-neutral or native-mvp")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "wge-control-plane-{label}-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn fixture_store(root: &Path, profile: &str) -> ProjectStore {
        fs::create_dir_all(root).unwrap();
        let spec_path = root.join("control_project_spec.json");
        fs::write(
            &spec_path,
            include_str!("../tests/fixtures/control_project_spec.json"),
        )
        .unwrap();
        ProjectStore::create(&root.join("store"), &spec_path, profile).unwrap()
    }

    fn create_candidate_fixture(
        store: &ProjectStore,
        workspace: &Path,
        snapshot_id: &str,
        bytes: &[u8],
        authorized_repair_artifact_ids: &[&str],
    ) -> StoredCandidate {
        let external_root = workspace.join(format!("external-{snapshot_id}"));
        fs::create_dir_all(&external_root).unwrap();
        let artifact_id = authorized_repair_artifact_ids
            .first()
            .copied()
            .unwrap_or("payload");
        let artifact_path = format!("{artifact_id}.bin");
        fs::write(external_root.join(&artifact_path), bytes).unwrap();
        let artifact = ArtifactManifest {
            artifact_id: artifact_id.into(),
            kind: "control_payload".into(),
            path: artifact_path,
            sha256: authority_sha256_prefixed(bytes),
        };
        let authorized = authorized_repair_artifact_ids
            .iter()
            .map(|id| (*id).to_owned())
            .collect::<BTreeSet<_>>();
        let candidate = CandidateContext {
            project_id: store.manifest.project_id.clone(),
            snapshot_id: snapshot_id.into(),
            candidate_sha256: String::new(),
            artifacts: BTreeMap::from([(
                artifact.artifact_id.clone(),
                ArtifactBytes {
                    kind: artifact.kind.clone(),
                    bytes: bytes.to_vec(),
                },
            )]),
            authorized_repair_artifact_ids: authorized.clone(),
        };
        let manifest = CandidateManifest {
            project_id: candidate.project_id.clone(),
            snapshot_id: snapshot_id.into(),
            candidate_sha256: candidate_identity(&candidate).unwrap(),
            artifact_root: ".".into(),
            authorized_repair_artifact_ids: authorized.into_iter().collect(),
            artifacts: vec![artifact],
        };
        let manifest_path = workspace.join(format!("{snapshot_id}-manifest.json"));
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        store
            .create_candidate(
                &manifest_path,
                &external_root,
                store.manifest.current_snapshot_id.as_deref(),
            )
            .unwrap()
    }

    fn status_only_semantic_receipt(candidate: &StoredCandidate) -> ReceiptEnvelope {
        let mut receipt = ReceiptEnvelope {
            schema_version: "wge.certification-receipt-envelope/v1".into(),
            receipt_id: String::new(),
            project_id: candidate.manifest.project_id.clone(),
            snapshot_id: candidate.manifest.snapshot_id.clone(),
            candidate_sha256: candidate.manifest.candidate_sha256.clone(),
            gate_id: "semantic".into(),
            validator_id: "wge.validator.semantic-spec/v1".into(),
            receipt_schema: "wge.semantic-receipt/v1".into(),
            status: wge_certification_authority::ReceiptStatus::Pass,
            producer: "untrusted-test-producer".into(),
            observed_input_sha256: String::new(),
            evidence: Vec::new(),
            payload: json!({}),
        };
        receipt.seal().unwrap();
        receipt
    }

    fn request_for_candidate(candidate: &StoredCandidate, receipt: ReceiptEnvelope) -> RequestFile {
        RequestFile {
            schema_version: "wge.certification-request/v1".into(),
            current_snapshot_id: candidate.manifest.snapshot_id.clone(),
            candidates: vec![RequestCandidate {
                project_id: candidate.manifest.project_id.clone(),
                snapshot_id: candidate.manifest.snapshot_id.clone(),
                candidate_sha256: candidate.manifest.candidate_sha256.clone(),
                artifact_root: candidate.manifest.artifact_root.clone(),
                authorized_repair_artifact_ids: candidate
                    .manifest
                    .authorized_repair_artifact_ids
                    .clone(),
                artifacts: candidate.manifest.artifacts.clone(),
            }],
            gates: engine_neutral_gate_profile(),
            receipts: vec![receipt],
        }
    }

    fn work_order(
        work_order_id: &str,
        snapshot_id: &str,
        output_artifact: &str,
        write_scope: &str,
        budget_bytes: u64,
    ) -> WorkOrder {
        WorkOrder {
            schema_version: "wge.work-order/v1".into(),
            work_order_id: work_order_id.into(),
            operation: "build".into(),
            snapshot_id: snapshot_id.into(),
            allowed_artifacts: vec![output_artifact.into()],
            required_capabilities: vec!["terrain".into()],
            required_gates: vec!["semantic".into()],
            parent_snapshot_id: Some("GENESIS".into()),
            input_artifacts: Vec::new(),
            output_artifacts: vec![output_artifact.into()],
            write_scope: vec![write_scope.into()],
            budget: Some(wge_project_ledger::WorkOrderBudget {
                max_artifacts: 1,
                max_total_bytes: budget_bytes,
            }),
        }
    }

    fn work_result(
        order: &WorkOrder,
        artifact_id: &str,
        path: &str,
        bytes: &[u8],
    ) -> WorkOrderResult {
        WorkOrderResult {
            schema_version: WORK_RESULT_SCHEMA.into(),
            work_order_id: order.work_order_id.clone(),
            parent_snapshot_id: "GENESIS".into(),
            output_artifacts: vec![artifact_id.into()],
            writes: vec![WorkWrite {
                artifact_id: artifact_id.into(),
                path: path.into(),
                sha256: authority_sha256_prefixed(bytes),
                byte_length: bytes.len() as u64,
            }],
        }
    }

    #[test]
    fn store_create_and_open_is_fail_closed() {
        let root = temp_root("open");
        let spec_path = root.join("spec.json");
        let spec = serde_json::json!({
            "schema_version":"wge.project-spec/v1",
            "project_id":"control-test",
            "title":"Control test",
            "brief":{"text":"brief","sources":[{"source_id":"brief","kind":"brief","path":"brief.md","sha256":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}],"claims":[],"conflicts":[],"style_target":{"visual_language":"clear","palette":["green"],"camera":"overview","reference_source_ids":[]},"assumptions":[],"design_constraints":[],"intake_provenance":null},
            "target":{"engine":"reference","engine_version":"wge.reference-runtime/v1","platform":"linux","coordinate_system":"right-handed-xz-up-y","build_profile":"inspection"},
            "world":{"world_id":"world","dimensions_m":[8.0,8.0],"terrain":{"artifact_id":"terrain","kind":"fixture","schema_version":"fixture/v1","path":"terrain.bin","sha256":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","producer":"test"},"collision":{"artifact_id":"collision","kind":"fixture","schema_version":"fixture/v1","path":"collision.bin","sha256":"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","producer":"test"},"navigation":{"artifact_id":"navigation","kind":"fixture","schema_version":"fixture/v1","path":"navigation.bin","sha256":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","producer":"test"},"spawns":[{"spawn_id":"player","team":"player","position_xz_m":[0.0,0.0],"required":true}],"objective":{"objective_id":"goal","kind":"reach","required_interaction_tag":"claim","win_condition":"won","loss_condition":"lost"}},
            "assets":[],"gameplay":{"runtime_package":{"artifact_id":"gameplay","kind":"fixture","schema_version":"fixture/v1","path":"gameplay.bin","sha256":"sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","producer":"test"},"input_trace":{"artifact_id":"input","kind":"fixture","schema_version":"fixture/v1","path":"input.bin","sha256":"sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","producer":"test"},"start_entity_id":"player","objective_id":"goal"},
            "artifact_graph":[
              {"artifact":{"artifact_id":"terrain","kind":"fixture","schema_version":"fixture/v1","path":"terrain.bin","sha256":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","producer":"test"},"dependencies":[]},
              {"artifact":{"artifact_id":"collision","kind":"fixture","schema_version":"fixture/v1","path":"collision.bin","sha256":"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","producer":"test"},"dependencies":[]},
              {"artifact":{"artifact_id":"navigation","kind":"fixture","schema_version":"fixture/v1","path":"navigation.bin","sha256":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","producer":"test"},"dependencies":[]},
              {"artifact":{"artifact_id":"gameplay","kind":"fixture","schema_version":"fixture/v1","path":"gameplay.bin","sha256":"sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","producer":"test"},"dependencies":[]},
              {"artifact":{"artifact_id":"input","kind":"fixture","schema_version":"fixture/v1","path":"input.bin","sha256":"sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","producer":"test"},"dependencies":[]}
            ],"work_orders":[],"required_gates":[{"gate_id":"semantic","evidence_kind":"semantic"}]
        });
        fs::write(&spec_path, serde_json::to_vec(&spec).unwrap()).unwrap();
        let store_root = root.join("store");
        let store = ProjectStore::create(&store_root, &spec_path, "engine-neutral").unwrap();
        assert_eq!(store.inspect_project()["current_snapshot_id"], Value::Null);
        let reopened = ProjectStore::open(&store_root).unwrap();
        assert_eq!(reopened.manifest().project_id, "control-test");
        fs::write(store_root.join("project-spec.json"), b"{}\n").unwrap();
        assert!(ProjectStore::open(&store_root).is_err());
    }

    #[test]
    fn unsafe_paths_and_invalid_profile_are_rejected() {
        assert!(safe_relative("../escape", "path").is_err());
        assert!(safe_id("../escape", "id").is_err());
        assert!(profile_gates("unity").is_err());
    }

    #[test]
    fn project_open_binds_registry_current_pointer_and_profile() {
        let root = temp_root("registry");
        let store = fixture_store(&root, "engine-neutral");
        assert_eq!(
            store.manifest.registry_sha256,
            ValidatorRegistry::wge_engine_neutral_v1().digest()
        );
        assert_eq!(store.inspect_current().unwrap()["status"], "uncommitted");

        let native_root = root.join("native");
        let native = fixture_store(&native_root, "native-mvp");
        assert_eq!(
            native.manifest.registry_sha256,
            ValidatorRegistry::wge_native_mvp_v1().digest()
        );
        assert_ne!(
            ValidatorRegistry::wge_engine_neutral_v1().digest(),
            ValidatorRegistry::wge_native_mvp_v1().digest()
        );

        write_atomic(
            &store.root.join("current-pointer.json"),
            &CurrentPointer {
                schema_version: POINTER_SCHEMA.into(),
                snapshot_id: "forged-current".into(),
                snapshot_sha256: authority_sha256_prefixed(b"forged"),
            },
        )
        .unwrap();
        assert!(ProjectStore::open(&store.root).is_err());
    }

    #[test]
    fn commit_and_rollback_reject_a_store_handle_with_stale_manifest_state() {
        let root = temp_root("stale-handle");
        let store = fixture_store(&root, "engine-neutral");
        let mut changed = store.manifest.clone();
        changed.profile_id = "native-mvp".into();
        changed.registry_sha256 = ValidatorRegistry::wge_native_mvp_v1().digest().into();
        write_atomic(&store.root.join("store.json"), &changed).unwrap();

        let mut stale = store;
        let commit = stale.commit_candidate("unseen-candidate").unwrap_err();
        assert!(commit.to_string().contains("changed since"));
        let rollback = stale.rollback("unseen-snapshot").unwrap_err();
        assert!(rollback.to_string().contains("changed since"));
        assert!(!stale.root.join("current-pointer.json").exists());
    }

    #[test]
    fn stored_candidate_identity_and_artifact_bytes_are_revalidated() {
        let root = temp_root("candidate-integrity");
        let store = fixture_store(&root, "engine-neutral");
        let candidate =
            create_candidate_fixture(&store, &root, "integrity-candidate", b"original", &[]);
        let inspected = store
            .inspect_artifact(&candidate.candidate_id, "payload")
            .unwrap();
        assert_eq!(inspected["authority_revalidated"], true);
        assert_eq!(inspected["sha256"], authority_sha256_prefixed(b"original"));

        let stored_payload = store
            .root
            .join("candidates/integrity-candidate/artifacts/payload.bin");
        fs::write(&stored_payload, b"tampered").unwrap();
        assert!(
            store
                .inspect_artifact(&candidate.candidate_id, "payload")
                .is_err()
        );

        let candidate_json = store
            .root
            .join("candidates/integrity-candidate/candidate.json");
        let mut value: Value = read_json(&candidate_json).unwrap();
        value["manifest"]["candidate_sha256"] =
            Value::String(authority_sha256_prefixed(b"forged identity"));
        write_atomic(&candidate_json, &value).unwrap();
        assert!(
            store
                .inspect_artifact(&candidate.candidate_id, "payload")
                .is_err()
        );
    }

    #[test]
    fn evidence_binding_and_status_only_pass_cannot_promote_or_move_pointer() {
        let root = temp_root("evidence");
        let store = fixture_store(&root, "engine-neutral");
        let candidate =
            create_candidate_fixture(&store, &root, "candidate-evidence", b"evidence", &[]);
        let request = request_for_candidate(&candidate, status_only_semantic_receipt(&candidate));
        let request_path = root.join("evidence-request.json");
        write_atomic(&request_path, &request).unwrap();
        store
            .attach_evidence(&candidate.candidate_id, &request_path)
            .unwrap();

        let mut mismatched = request.clone();
        mismatched.candidates[0].artifacts[0].sha256 = authority_sha256_prefixed(b"other bytes");
        let mismatched_path = root.join("mismatched-request.json");
        write_atomic(&mismatched_path, &mismatched).unwrap();
        assert!(
            store
                .attach_evidence(&candidate.candidate_id, &mismatched_path)
                .is_err()
        );

        assert!(store.validate_candidate(&candidate.candidate_id).is_err());
        let mut mutable_store = ProjectStore::open(&store.root).unwrap();
        assert!(
            mutable_store
                .commit_candidate(&candidate.candidate_id)
                .is_err()
        );
        assert_eq!(mutable_store.manifest.current_snapshot_id, None);
        assert!(!store.root.join("current-pointer.json").exists());
        assert!(mutable_store.rollback("uncertified").is_err());
    }

    #[test]
    fn work_order_proposal_binds_exact_capabilities_outputs_scope_and_budget() {
        let root = temp_root("work-order");
        let store = fixture_store(&root, "engine-neutral");
        let order = store.spec.work_orders[0].clone();
        let capabilities = BTreeSet::from(["terrain".to_owned()]);
        let proposal = store.propose_work_order(&order, &capabilities).unwrap();
        assert_eq!(proposal.status, "authorized_pending_result");

        let output_path = store.root.join("work-output/terrain.bin");
        fs::create_dir_all(output_path.parent().unwrap()).unwrap();
        fs::write(&output_path, b"terrain bytes").unwrap();
        let result = work_result(
            &order,
            "terrain",
            "work-output/terrain.bin",
            b"terrain bytes",
        );
        let receipt = store
            .execute_work_order(&order, &result, &capabilities)
            .unwrap();
        assert_eq!(receipt.outputs, vec!["terrain"]);

        let too_many_capabilities = BTreeSet::from(["terrain".to_owned(), "semantic".to_owned()]);
        assert!(
            store
                .propose_work_order(&order, &too_many_capabilities)
                .is_err()
        );

        let mut over_budget = order.clone();
        over_budget.work_order_id = "over-budget".into();
        over_budget.budget.as_mut().unwrap().max_total_bytes = 2;
        store
            .propose_work_order(&over_budget, &capabilities)
            .unwrap();
        let over_path = store.root.join("work-output/over-budget.bin");
        fs::write(&over_path, b"too large").unwrap();
        let oversized_result = work_result(
            &over_budget,
            "terrain",
            "work-output/over-budget.bin",
            b"too large",
        );
        assert!(
            store
                .execute_work_order(&over_budget, &oversized_result, &capabilities)
                .is_err()
        );

        let mut unproposed = order.clone();
        unproposed.work_order_id = "unproposed".into();
        assert!(
            store
                .execute_work_order(&unproposed, &result, &capabilities)
                .is_err()
        );

        let mut escaped = result.clone();
        escaped.writes[0].path = "work-output-escape/terrain.bin".into();
        assert!(
            store
                .execute_work_order(&order, &escaped, &capabilities)
                .is_err()
        );
    }

    #[test]
    fn repair_execution_requires_candidate_bound_authorization() {
        let root = temp_root("repair");
        let store = fixture_store(&root, "engine-neutral");
        let candidate = create_candidate_fixture(
            &store,
            &root,
            "repair-candidate",
            b"repair bytes",
            &["terrain"],
        );
        assert!(store.propose_repair(&candidate.candidate_id).is_err());
        let mut proposal = RepairProposal {
            schema_version: REPAIR_PROPOSAL_SCHEMA.into(),
            candidate_id: candidate.candidate_id.clone(),
            candidate_sha256: candidate.manifest.candidate_sha256.clone(),
            failed_layers: vec!["visual_quality".into()],
            authorized_artifact_ids: candidate.manifest.authorized_repair_artifact_ids.clone(),
            status: "diagnosis_only".into(),
            next_step: "submit_a_bounded_repair_work_order".into(),
            proposal_sha256: String::new(),
        };
        proposal.proposal_sha256 = repair_proposal_digest(&proposal).unwrap();
        write_atomic(
            &store
                .root
                .join("work-orders/repair-candidate.repair-proposal.json"),
            &proposal,
        )
        .unwrap();
        assert_eq!(proposal.status, "diagnosis_only");
        assert_eq!(proposal.authorized_artifact_ids, vec!["terrain"]);
        assert_eq!(
            proposal.proposal_sha256,
            repair_proposal_digest(&proposal).unwrap()
        );

        let mut forged = proposal.clone();
        forged.authorized_artifact_ids.push("collision".into());
        write_atomic(
            &store
                .root
                .join("work-orders/repair-candidate.repair-proposal.json"),
            &forged,
        )
        .unwrap();
        let mut authorized_order = work_order(
            "repair-terrain",
            &candidate.candidate_id,
            "terrain",
            "repair-output",
            128,
        );
        authorized_order.operation = "repair".into();
        store
            .propose_work_order(&authorized_order, &BTreeSet::from(["terrain".to_owned()]))
            .unwrap();
        let authorized_path = store.root.join("repair-output/terrain.bin");
        fs::create_dir_all(authorized_path.parent().unwrap()).unwrap();
        fs::write(&authorized_path, b"repaired terrain").unwrap();
        let authorized_result = work_result(
            &authorized_order,
            "terrain",
            "repair-output/terrain.bin",
            b"repaired terrain",
        );
        assert!(
            store
                .execute_repair_work_order(
                    &candidate.candidate_id,
                    &authorized_order,
                    &authorized_result,
                    &BTreeSet::from(["terrain".to_owned()]),
                )
                .is_err()
        );

        write_atomic(
            &store
                .root
                .join("work-orders/repair-candidate.repair-proposal.json"),
            &proposal,
        )
        .unwrap();
        assert!(
            store
                .execute_repair_work_order(
                    &candidate.candidate_id,
                    &authorized_order,
                    &authorized_result,
                    &BTreeSet::from(["terrain".to_owned()]),
                )
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn artifact_and_work_output_symlinks_cannot_escape_roots() {
        use std::os::unix::fs::symlink;

        let root = temp_root("symlink");
        let store = fixture_store(&root, "engine-neutral");
        let candidate =
            create_candidate_fixture(&store, &root, "symlink-candidate", b"inside", &[]);
        let outside = root.join("outside.bin");
        fs::write(&outside, b"inside").unwrap();
        let payload = store
            .root
            .join("candidates/symlink-candidate/artifacts/payload.bin");
        fs::remove_file(&payload).unwrap();
        symlink(&outside, &payload).unwrap();
        assert!(
            store
                .inspect_artifact(&candidate.candidate_id, "payload")
                .is_err()
        );

        let work_root = temp_root("work-output-link");
        let work_store = fixture_store(&work_root, "engine-neutral");
        let order = work_store.spec.work_orders[0].clone();
        let capabilities = BTreeSet::from(["terrain".to_owned()]);
        work_store
            .propose_work_order(&order, &capabilities)
            .unwrap();
        let linked_output = work_store.root.join("work-output/terrain.bin");
        fs::create_dir_all(linked_output.parent().unwrap()).unwrap();
        fs::write(&outside, b"terrain bytes").unwrap();
        symlink(&outside, &linked_output).unwrap();
        let result = work_result(
            &order,
            "terrain",
            "work-output/terrain.bin",
            b"terrain bytes",
        );
        assert!(
            work_store
                .execute_work_order(&order, &result, &capabilities)
                .is_err()
        );

        let pointer_link_root = temp_root("pointer-link");
        let pointer_store = fixture_store(&pointer_link_root, "engine-neutral");
        let external_pointer = pointer_link_root.join("external-pointer.json");
        write_atomic(
            &external_pointer,
            &CurrentPointer {
                schema_version: POINTER_SCHEMA.into(),
                snapshot_id: "outside".into(),
                snapshot_sha256: authority_sha256_prefixed(b"outside"),
            },
        )
        .unwrap();
        symlink(
            &external_pointer,
            pointer_store.root.join("current-pointer.json"),
        )
        .unwrap();
        assert!(ProjectStore::open(&pointer_store.root).is_err());
    }
}
