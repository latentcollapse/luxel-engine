use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use serde_json::{Value, json};
use wge_certification_authority::{
    ArtifactBytes, CandidateContext, GateRequirement, MAX_ARTIFACT_BYTES, MAX_CANDIDATES,
    MAX_ENVELOPE_BYTES, MAX_RECEIPTS, MAX_TOTAL_ARTIFACT_BYTES, REQUEST_SCHEMA, ReceiptEnvelope,
    ValidationRequest, ValidatorRegistry, candidate_identity, candidate_identity_bytes,
    canonical_json, engine_neutral_gate_profile, native_repair_evidence_reference,
    native_repair_receipt_bytes, repair_validator_registry, sha256_prefixed,
    validate_candidate_identity, validate_request,
};
use wge_intake_repair_contract as repair_contract;

const MAX_REQUEST_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestFile {
    schema_version: String,
    current_snapshot_id: String,
    candidates: Vec<CandidateFile>,
    gates: Vec<GateRequirement>,
    receipts: Vec<ReceiptEnvelope>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateFile {
    project_id: String,
    snapshot_id: String,
    candidate_sha256: String,
    artifact_root: String,
    authorized_repair_artifact_ids: Vec<String>,
    artifacts: Vec<ArtifactFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactFile {
    artifact_id: String,
    kind: String,
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepairDeltaRequestFile {
    schema_version: String,
    before_candidate: Value,
    after_candidate: Value,
    before_receipt: Value,
    after_receipt: Value,
    proposal: Value,
    delta_draft: Value,
}

fn main() -> ExitCode {
    match dispatch() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn dispatch() -> Result<ExitCode, String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("seal") => {
            let input = PathBuf::from(args.next().ok_or_else(usage)?);
            let output = PathBuf::from(args.next().ok_or_else(usage)?);
            if args.next().is_some() {
                return Err(usage());
            }
            let metadata =
                fs::metadata(&input).map_err(|error| format!("cannot stat receipt: {error}"))?;
            if metadata.len() == 0 || metadata.len() > MAX_ENVELOPE_BYTES as u64 {
                return Err("receipt envelope byte length is out of bounds".into());
            }
            let bytes =
                fs::read(&input).map_err(|error| format!("cannot read receipt: {error}"))?;
            let mut receipt: ReceiptEnvelope = serde_json::from_slice(&bytes)
                .map_err(|error| format!("malformed receipt envelope: {error}"))?;
            receipt.seal().map_err(|error| error.to_string())?;
            let value = serde_json::to_value(receipt).map_err(|error| error.to_string())?;
            fs::write(&output, format!("{}\n", canonical_json(&value)))
                .map_err(|error| format!("cannot write sealed receipt: {error}"))?;
            Ok(ExitCode::SUCCESS)
        }
        Some("registry") if args.next().is_none() => {
            let registry = ValidatorRegistry::wge_engine_neutral_v1();
            let result = json!({
                "schema_version": "wge.validator-registry/v1",
                "registry_sha256": registry.digest(),
                "validators": registry.descriptors(),
            });
            println!("{}", canonical_json(&result));
            Ok(ExitCode::SUCCESS)
        }
        Some("profile") if args.next().is_none() => {
            let registry = ValidatorRegistry::wge_engine_neutral_v1();
            let result = json!({
                "schema_version": "wge.certification-profile/v1",
                "registry_sha256": registry.digest(),
                "gates": engine_neutral_gate_profile(),
            });
            println!("{}", canonical_json(&result));
            Ok(ExitCode::SUCCESS)
        }
        Some("candidate-id") => {
            let candidate_path = PathBuf::from(args.next().ok_or_else(usage)?);
            let artifact_root_flag = args.next().ok_or_else(usage)?;
            if artifact_root_flag != "--artifact-root" {
                return Err(usage());
            }
            let artifact_root = PathBuf::from(args.next().ok_or_else(usage)?);
            if args.next().is_some() {
                return Err(usage());
            }
            let candidate_value = read_json_value(&candidate_path)?;
            let candidate_file: CandidateFile = serde_json::from_value(candidate_value)
                .map_err(|error| format!("malformed candidate manifest: {error}"))?;
            let canonical_root = artifact_root
                .canonicalize()
                .map_err(|error| format!("cannot resolve artifact root: {error}"))?;
            let candidate = load_candidate(&candidate_file, &canonical_root, &mut 0, false)?;
            let identity = candidate_identity(&candidate).map_err(|error| error.to_string())?;
            let result = json!({
                "schema_version": "wge.candidate-identity/v1",
                "project_id": candidate.project_id,
                "snapshot_id": candidate.snapshot_id,
                "candidate_sha256": identity,
                "authorized_repair_artifact_ids": candidate.authorized_repair_artifact_ids,
                "artifact_count": candidate.artifacts.len(),
            });
            println!("{}", canonical_json(&result));
            Ok(ExitCode::SUCCESS)
        }
        Some("repair-reference") => {
            let receipt_path = PathBuf::from(args.next().ok_or_else(usage)?);
            if args.next().as_deref() != Some("--candidate") {
                return Err(usage());
            }
            let candidate_path = PathBuf::from(args.next().ok_or_else(usage)?);
            if args.next().as_deref() != Some("--artifact-root") {
                return Err(usage());
            }
            let artifact_root = PathBuf::from(args.next().ok_or_else(usage)?);
            let output_bridge = match args.next().as_deref() {
                None => None,
                Some("--output-bridge") => Some(PathBuf::from(args.next().ok_or_else(usage)?)),
                Some(_) => return Err(usage()),
            };
            if args.next().is_some() {
                return Err(usage());
            }
            let canonical_root = artifact_root
                .canonicalize()
                .map_err(|error| format!("cannot resolve artifact root: {error}"))?;
            let candidate_value = read_json_value(&candidate_path)?;
            let candidate_file: CandidateFile = serde_json::from_value(candidate_value)
                .map_err(|error| format!("malformed candidate manifest: {error}"))?;
            let candidate = load_candidate(&candidate_file, &canonical_root, &mut 0, true)?;
            let receipt_value = read_json_value(&receipt_path)?;
            let receipt: ReceiptEnvelope = serde_json::from_value(receipt_value)
                .map_err(|error| format!("malformed receipt envelope: {error}"))?;
            let registry = ValidatorRegistry::wge_engine_neutral_v1();
            let reference = native_repair_evidence_reference(&receipt, &candidate, &registry)
                .map_err(|error| error.to_string())?;
            if let Some(path) = output_bridge {
                let bridge = native_repair_receipt_bytes(&receipt, &candidate, &registry)
                    .map_err(|error| error.to_string())?;
                fs::write(&path, bridge)
                    .map_err(|error| format!("cannot write native repair bridge: {error}"))?;
            }
            let result = serde_json::to_value(reference).map_err(|error| error.to_string())?;
            println!("{}", canonical_json(&result));
            Ok(ExitCode::SUCCESS)
        }
        Some("repair-delta") => {
            let request_path = PathBuf::from(args.next().ok_or_else(usage)?);
            if args.next().as_deref() != Some("--artifact-root") {
                return Err(usage());
            }
            let artifact_root = PathBuf::from(args.next().ok_or_else(usage)?);
            if args.next().as_deref() != Some("--output") {
                return Err(usage());
            }
            let output_path = PathBuf::from(args.next().ok_or_else(usage)?);
            if args.next().is_some() {
                return Err(usage());
            }
            let request_value = read_json_value(&request_path)?;
            let request: RepairDeltaRequestFile = serde_json::from_value(request_value)
                .map_err(|error| format!("malformed repair-delta request: {error}"))?;
            if request.schema_version != "wge.repair-delta-request/v1" {
                return Err("unsupported repair-delta request schema".into());
            }
            let canonical_root = artifact_root
                .canonicalize()
                .map_err(|error| format!("cannot resolve artifact root: {error}"))?;
            let before_candidate = candidate_from_value(
                &request.before_candidate,
                &request_path,
                &canonical_root,
                true,
            )?;
            let after_candidate = candidate_from_value(
                &request.after_candidate,
                &request_path,
                &canonical_root,
                true,
            )?;
            let before_receipt: ReceiptEnvelope =
                typed_value(&request.before_receipt, &request_path, "before receipt")?;
            let after_receipt: ReceiptEnvelope =
                typed_value(&request.after_receipt, &request_path, "after receipt")?;
            let proposal: repair_contract::RepairProposal =
                typed_value(&request.proposal, &request_path, "repair proposal")?;
            let delta_draft: repair_contract::RepairEvidenceDeltaDraft =
                typed_value(&request.delta_draft, &request_path, "repair delta draft")?;
            let registry = ValidatorRegistry::wge_engine_neutral_v1();
            let before_native =
                native_repair_receipt_bytes(&before_receipt, &before_candidate, &registry)
                    .map_err(|error| error.to_string())?;
            let after_native =
                native_repair_receipt_bytes(&after_receipt, &after_candidate, &registry)
                    .map_err(|error| error.to_string())?;
            let native_registry = repair_validator_registry().map_err(|error| error.to_string())?;
            let delta = repair_contract::validate_repair_delta(
                &proposal,
                delta_draft,
                &candidate_identity_bytes(&before_candidate).map_err(|error| error.to_string())?,
                &candidate_identity_bytes(&after_candidate).map_err(|error| error.to_string())?,
                &before_native,
                &after_native,
                &candidate_bytes(&before_candidate),
                &candidate_bytes(&after_candidate),
                &native_registry,
            )
            .map_err(|error| error.to_string())?;
            let result = serde_json::to_value(&delta).map_err(|error| error.to_string())?;
            fs::write(&output_path, format!("{}\n", canonical_json(&result)))
                .map_err(|error| format!("cannot write repair delta: {error}"))?;
            println!("{}", delta.delta_id);
            Ok(ExitCode::SUCCESS)
        }
        Some("validate") => {
            let request_path = PathBuf::from(args.next().ok_or_else(usage)?);
            let artifact_root_flag = args.next().ok_or_else(usage)?;
            if artifact_root_flag != "--artifact-root" {
                return Err(usage());
            }
            let artifact_root = PathBuf::from(args.next().ok_or_else(usage)?);
            if args.next().is_some() {
                return Err(usage());
            }
            let request = load_request(&request_path, &artifact_root)?;
            let registry = ValidatorRegistry::wge_engine_neutral_v1();
            let report =
                validate_request(&request, &registry).map_err(|error| error.to_string())?;
            println!(
                "{}",
                canonical_json(&serde_json::to_value(&report).map_err(|error| error.to_string())?)
            );
            if report.status
                == wge_certification_authority::CertificationStatus::EngineNeutralCertified
            {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(3))
            }
        }
        _ => Err(usage()),
    }
}

fn load_request(request_path: &Path, artifact_root: &Path) -> Result<ValidationRequest, String> {
    let metadata =
        fs::metadata(request_path).map_err(|error| format!("cannot stat request: {error}"))?;
    if metadata.len() > MAX_REQUEST_FILE_BYTES {
        return Err("request file exceeds 16 MiB limit".into());
    }
    let request_bytes =
        fs::read(request_path).map_err(|error| format!("cannot read request: {error}"))?;
    let wire: RequestFile = serde_json::from_slice(&request_bytes)
        .map_err(|error| format!("malformed request JSON: {error}"))?;
    if wire.schema_version != REQUEST_SCHEMA {
        return Err(format!(
            "unsupported request schema {:?}",
            wire.schema_version
        ));
    }
    if wire.candidates.is_empty() || wire.candidates.len() > MAX_CANDIDATES {
        return Err("candidate count is out of bounds".into());
    }
    if wire.receipts.is_empty() || wire.receipts.len() > MAX_RECEIPTS {
        return Err("receipt count is out of bounds".into());
    }
    let canonical_root = artifact_root
        .canonicalize()
        .map_err(|error| format!("cannot resolve artifact root: {error}"))?;
    let mut aggregate_bytes = 0usize;
    let mut candidates = Vec::with_capacity(wire.candidates.len());
    for input in wire.candidates {
        let candidate = load_candidate(&input, &canonical_root, &mut aggregate_bytes, true)?;
        validate_candidate_identity(&candidate).map_err(|error| error.to_string())?;
        candidates.push(candidate);
    }
    Ok(ValidationRequest {
        current_snapshot_id: wire.current_snapshot_id,
        candidates,
        gates: wire.gates,
        receipts: wire.receipts,
    })
}

fn load_candidate(
    input: &CandidateFile,
    canonical_root: &Path,
    aggregate_bytes: &mut usize,
    verify_identity: bool,
) -> Result<CandidateContext, String> {
    let candidate_root = safe_existing_path(canonical_root, &input.artifact_root, true)?;
    let mut artifacts = BTreeMap::new();
    for artifact in &input.artifacts {
        let path = safe_existing_path(&candidate_root, &artifact.path, false)?;
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("cannot stat artifact {}: {error}", artifact.artifact_id))?;
        if metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES as u64 {
            return Err(format!(
                "artifact {} byte length is out of bounds",
                artifact.artifact_id
            ));
        }
        let bytes = fs::read(&path)
            .map_err(|error| format!("cannot read artifact {}: {error}", artifact.artifact_id))?;
        *aggregate_bytes = aggregate_bytes.saturating_add(bytes.len());
        if *aggregate_bytes > MAX_TOTAL_ARTIFACT_BYTES {
            return Err("aggregate artifact bytes exceed limit".into());
        }
        if sha256_prefixed(&bytes) != artifact.sha256 {
            return Err(format!(
                "artifact {} digest does not match raw bytes",
                artifact.artifact_id
            ));
        }
        if artifacts
            .insert(
                artifact.artifact_id.clone(),
                ArtifactBytes {
                    kind: artifact.kind.clone(),
                    bytes,
                },
            )
            .is_some()
        {
            return Err("candidate has duplicate artifact IDs".into());
        }
    }
    let candidate = CandidateContext {
        project_id: input.project_id.clone(),
        snapshot_id: input.snapshot_id.clone(),
        candidate_sha256: input.candidate_sha256.clone(),
        artifacts,
        authorized_repair_artifact_ids: input
            .authorized_repair_artifact_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
    };
    if verify_identity {
        validate_candidate_identity(&candidate).map_err(|error| error.to_string())?;
    }
    Ok(candidate)
}

fn candidate_bytes(candidate: &CandidateContext) -> BTreeMap<String, Vec<u8>> {
    candidate
        .artifacts
        .iter()
        .map(|(id, artifact)| (id.clone(), artifact.bytes.clone()))
        .collect()
}

fn read_json_value(path: &Path) -> Result<Value, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("malformed JSON {}: {error}", path.display()))
}

fn typed_value<T: serde::de::DeserializeOwned>(
    value: &Value,
    request_path: &Path,
    label: &str,
) -> Result<T, String> {
    let value = if let Value::String(relative) = value {
        let path = request_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(relative);
        read_json_value(&path)?
    } else {
        value.clone()
    };
    serde_json::from_value(value).map_err(|error| format!("malformed {label}: {error}"))
}

fn candidate_from_value(
    value: &Value,
    request_path: &Path,
    artifact_root: &Path,
    verify_identity: bool,
) -> Result<CandidateContext, String> {
    let value = if let Value::String(relative) = value {
        let path = request_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(relative);
        read_json_value(&path)?
    } else {
        value.clone()
    };
    let candidate: CandidateFile = serde_json::from_value(value)
        .map_err(|error| format!("malformed candidate manifest: {error}"))?;
    load_candidate(&candidate, artifact_root, &mut 0, verify_identity)
}

fn safe_existing_path(root: &Path, relative: &str, directory: bool) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err("artifact paths must be relative and may not traverse parents".into());
    }
    let resolved = root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("cannot resolve artifact path: {error}"))?;
    if !resolved.starts_with(root) {
        return Err("artifact path escapes its declared root".into());
    }
    let metadata = fs::metadata(&resolved)
        .map_err(|error| format!("cannot inspect artifact path: {error}"))?;
    if directory != metadata.is_dir() {
        return Err("artifact path has the wrong file type".into());
    }
    Ok(resolved)
}

fn usage() -> String {
    "usage:\n  wge-certification-authority registry\n  wge-certification-authority profile\n  wge-certification-authority seal RECEIPT.json OUTPUT.json\n  wge-certification-authority candidate-id CANDIDATE.json --artifact-root DIR\n  wge-certification-authority repair-reference RECEIPT.json --candidate CANDIDATE.json --artifact-root DIR [--output-bridge BRIDGE.json]\n  wge-certification-authority repair-delta REQUEST.json --artifact-root DIR --output DELTA.json\n  wge-certification-authority validate REQUEST.json --artifact-root DIR".into()
}
