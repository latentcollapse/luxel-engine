//! Typed, provider-neutral semantic intake and bounded repair contracts.
//!
//! This crate accepts provider assertions as data. Rust validates the closed
//! schema, checks source and response bytes, and assigns all durable identity.
//! Repair evidence is accepted only through a Rust-registered validator that
//! parses the native receipt bytes; a producer-supplied status is never enough.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const INTAKE_DRAFT_SCHEMA: &str = "wge.semantic-intake-draft/v1";
pub const INTAKE_SCHEMA: &str = "wge.semantic-intake/v1";
pub const SOURCE_BUNDLE_DRAFT_SCHEMA: &str = "wge.source-bundle-draft/v1";
pub const SOURCE_BUNDLE_SCHEMA: &str = "wge.source-bundle/v1";
pub const PROVIDER_INTERPRETATION_SCHEMA: &str = "wge.provider-interpretation/v1";
pub const REPAIR_PROPOSAL_DRAFT_SCHEMA: &str = "wge.repair-proposal-draft/v1";
pub const REPAIR_PROPOSAL_SCHEMA: &str = "wge.repair-proposal/v1";
pub const REPAIR_DELTA_DRAFT_SCHEMA: &str = "wge.repair-evidence-delta-draft/v1";
pub const REPAIR_DELTA_SCHEMA: &str = "wge.repair-evidence-delta/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    Json(String),
    Schema(String),
    Identity(String),
    Provenance(String),
    Registry(String),
    Repair(String),
}

impl fmt::Display for ContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(message) => write!(formatter, "invalid JSON: {message}"),
            Self::Schema(message) => write!(formatter, "schema error: {message}"),
            Self::Identity(message) => write!(formatter, "identity error: {message}"),
            Self::Provenance(message) => write!(formatter, "provenance error: {message}"),
            Self::Registry(message) => write!(formatter, "validator registry error: {message}"),
            Self::Repair(message) => write!(formatter, "repair error: {message}"),
        }
    }
}

impl std::error::Error for ContractError {}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

/// Canonical JSON compatible with the semantic-kernel convention: object keys
/// are sorted recursively and array order remains meaningful.
pub fn canonical_json(value: &Value) -> String {
    serde_json::to_string(&canonicalize(value)).expect("JSON values are serializable")
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let ordered: BTreeMap<_, _> = object
                .iter()
                .map(|(key, child)| (key.clone(), canonicalize(child)))
                .collect();
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

fn canonical_digest<T: Serialize>(value: &T) -> Result<String, ContractError> {
    let value =
        serde_json::to_value(value).map_err(|error| ContractError::Json(error.to_string()))?;
    Ok(sha256_prefixed(canonical_json(&value).as_bytes()))
}

fn identity(prefix: &str, digest: &str) -> String {
    format!("{prefix}:{digest}")
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn require_text(value: &str, path: &str, max: usize) -> Result<(), ContractError> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(ContractError::Schema(format!(
            "{path} must be nonempty, at most {max} bytes, and contain no control characters"
        )));
    }
    Ok(())
}

fn require_schema(actual: &str, expected: &str) -> Result<(), ContractError> {
    if actual != expected {
        return Err(ContractError::Schema(format!(
            "expected schema {expected}, found {actual}"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderProvenance {
    pub provider_id: String,
    pub provider_version: String,
    pub protocol: String,
    pub request_source_bundle_id: String,
    pub response_sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Brief,
    ConceptArt,
    DesignDocument,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceOrigin {
    UserSupplied,
    ProviderGenerated,
    Retrieved,
    Derived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceProvenance {
    pub origin: ProvenanceOrigin,
    pub origin_ref: String,
    pub provider_id: Option<String>,
    pub provider_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceDraft {
    /// Request-local name used only to bind source bytes during source intake.
    pub source_ref: String,
    pub kind: SourceKind,
    pub content_sha256: String,
    pub media_type: String,
    pub provenance: SourceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceBundleDraft {
    pub schema_version: String,
    /// Fresh caller-generated request token; it is pinned into the bundle ID.
    pub request_id: String,
    pub sources: Vec<SourceDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceBundle {
    pub schema_version: String,
    pub request_id: String,
    pub source_bundle_id: String,
    pub sources: Vec<SourceRecord>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ClaimDomain {
    Visual,
    Gameplay,
    World,
    Style,
    Constraint,
    Interaction,
    Asset,
    Narrative,
    Accessibility,
    Performance,
    Exclusion,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicKind {
    Observation,
    Inference,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceRegion {
    TextSpan {
        start_byte: u64,
        end_byte: u64,
    },
    ImageRect {
        x_min: f64,
        y_min: f64,
        x_max: f64,
        y_max: f64,
    },
    PageRect {
        page: u32,
        x_min: f64,
        y_min: f64,
        x_max: f64,
        y_max: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceLinkDraft {
    pub source_id: String,
    pub region: Option<SourceRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClaimDraft {
    /// Request-local name used only for conflict/assumption references.
    pub claim_ref: String,
    pub epistemic_kind: EpistemicKind,
    pub domain: ClaimDomain,
    pub statement: String,
    pub confidence: f64,
    pub evidence: Vec<EvidenceLinkDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConflictDraft {
    pub conflict_ref: String,
    pub left_claim_ref: String,
    pub right_claim_ref: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AssumptionDraft {
    pub assumption_ref: String,
    pub statement: String,
    pub rationale: String,
    pub confidence: f64,
    pub related_claim_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderInterpretation {
    pub schema_version: String,
    pub source_bundle_id: String,
    pub claims: Vec<ClaimDraft>,
    pub conflicts: Vec<ConflictDraft>,
    pub assumptions: Vec<AssumptionDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IntakeDraft {
    pub schema_version: String,
    pub source_bundle_id: String,
    pub provider: ProviderProvenance,
    pub interpretation: ProviderInterpretation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub source_id: String,
    pub kind: SourceKind,
    pub content_sha256: String,
    pub byte_length: u64,
    pub media_type: String,
    pub provenance: SourceProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceLink {
    pub source_id: String,
    pub region: Option<SourceRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticClaim {
    pub claim_id: String,
    pub domain: ClaimDomain,
    pub statement: String,
    pub confidence: f64,
    pub evidence: Vec<EvidenceLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticConflict {
    pub conflict_id: String,
    pub left_claim_id: String,
    pub right_claim_id: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticAssumption {
    pub assumption_id: String,
    pub statement: String,
    pub rationale: String,
    pub confidence: f64,
    pub related_claim_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticIntake {
    pub schema_version: String,
    pub intake_id: String,
    pub request_id: String,
    pub source_bundle_id: String,
    pub provider: ProviderProvenance,
    pub sources: Vec<SourceRecord>,
    pub observations: Vec<SemanticClaim>,
    pub inferences: Vec<SemanticClaim>,
    pub conflicts: Vec<SemanticConflict>,
    pub assumptions: Vec<SemanticAssumption>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct IntakeIdentity<'a> {
    schema_version: &'a str,
    request_id: &'a str,
    source_bundle_id: &'a str,
    provider: &'a ProviderProvenance,
    sources: &'a [SourceRecord],
    observations: &'a [SemanticClaim],
    inferences: &'a [SemanticClaim],
    conflicts: &'a [SemanticConflict],
    assumptions: &'a [SemanticAssumption],
}

fn validate_provider(
    provider: &ProviderProvenance,
    response_bytes: &[u8],
) -> Result<(), ContractError> {
    require_text(&provider.provider_id, "provider.provider_id", 128)?;
    require_text(&provider.provider_version, "provider.provider_version", 128)?;
    require_text(&provider.protocol, "provider.protocol", 128)?;
    if !valid_digest(&provider.response_sha256) {
        return Err(ContractError::Provenance(
            "provider response digest is malformed".into(),
        ));
    }
    if !provider
        .request_source_bundle_id
        .starts_with("source-bundle:sha256:")
        || !valid_digest(
            provider
                .request_source_bundle_id
                .strip_prefix("source-bundle:")
                .unwrap_or_default(),
        )
    {
        return Err(ContractError::Provenance(
            "provider request source-bundle identity is malformed".into(),
        ));
    }
    if sha256_prefixed(response_bytes) != provider.response_sha256 {
        return Err(ContractError::Provenance(
            "provider response bytes are stale or mismatched".into(),
        ));
    }
    Ok(())
}

fn validate_provenance(provenance: &SourceProvenance) -> Result<(), ContractError> {
    require_text(&provenance.origin_ref, "source.provenance.origin_ref", 512)?;
    match (
        provenance.origin,
        &provenance.provider_id,
        &provenance.provider_version,
    ) {
        (ProvenanceOrigin::UserSupplied, None, None) => Ok(()),
        (ProvenanceOrigin::UserSupplied, _, _) => Err(ContractError::Provenance(
            "user_supplied sources must not claim provider identity".into(),
        )),
        (_, Some(provider_id), Some(provider_version)) => {
            require_text(provider_id, "source.provenance.provider_id", 128)?;
            require_text(provider_version, "source.provenance.provider_version", 128)
        }
        _ => Err(ContractError::Provenance(
            "provider_generated, retrieved, and derived sources require provider id and version"
                .into(),
        )),
    }
}

fn validate_media_type(media_type: &str) -> Result<(), ContractError> {
    require_text(media_type, "source.media_type", 128)?;
    let mut parts = media_type.split('/');
    let (Some(top), Some(sub), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ContractError::Schema(
            "source.media_type must be a media type".into(),
        ));
    };
    if top.is_empty()
        || sub.is_empty()
        || !media_type
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-/".contains(&byte))
    {
        return Err(ContractError::Schema(
            "source.media_type is malformed".into(),
        ));
    }
    Ok(())
}

fn validate_region(
    region: &SourceRegion,
    source_kind: SourceKind,
    source_byte_length: u64,
) -> Result<(), ContractError> {
    let valid_rect = |x_min: f64, y_min: f64, x_max: f64, y_max: f64| {
        [x_min, y_min, x_max, y_max]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && x_min < x_max
            && y_min < y_max
    };
    match region {
        SourceRegion::TextSpan {
            start_byte,
            end_byte,
        } if *start_byte < *end_byte
            && *end_byte <= source_byte_length
            && matches!(source_kind, SourceKind::Brief | SourceKind::DesignDocument) =>
        {
            Ok(())
        }
        SourceRegion::ImageRect {
            x_min,
            y_min,
            x_max,
            y_max,
        } if valid_rect(*x_min, *y_min, *x_max, *y_max)
            && source_kind == SourceKind::ConceptArt =>
        {
            Ok(())
        }
        SourceRegion::PageRect {
            page,
            x_min,
            y_min,
            x_max,
            y_max,
        } if *page > 0
            && valid_rect(*x_min, *y_min, *x_max, *y_max)
            && source_kind == SourceKind::DesignDocument =>
        {
            Ok(())
        }
        _ => Err(ContractError::Schema(
            "source region is empty, out of bounds, or incompatible with its source kind".into(),
        )),
    }
}

fn validate_confidence(value: f64, path: &str) -> Result<(), ContractError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(ContractError::Schema(format!(
            "{path} must be finite and in [0,1]"
        )));
    }
    Ok(())
}

fn source_identity(source: &SourceRecord) -> Result<String, ContractError> {
    #[derive(Serialize)]
    struct Body<'a> {
        kind: SourceKind,
        content_sha256: &'a str,
        byte_length: u64,
        media_type: &'a str,
        provenance: &'a SourceProvenance,
    }
    let body = Body {
        kind: source.kind,
        content_sha256: &source.content_sha256,
        byte_length: source.byte_length,
        media_type: &source.media_type,
        provenance: &source.provenance,
    };
    Ok(identity("source", &canonical_digest(&body)?))
}

fn source_bundle_identity(
    request_id: &str,
    sources: &[SourceRecord],
) -> Result<String, ContractError> {
    #[derive(Serialize)]
    struct Body<'a> {
        schema_version: &'a str,
        request_id: &'a str,
        sources: &'a [SourceRecord],
    }
    let mut sorted = sources.to_vec();
    sorted.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    let body = Body {
        schema_version: SOURCE_BUNDLE_SCHEMA,
        request_id,
        sources: &sorted,
    };
    Ok(identity("source-bundle", &canonical_digest(&body)?))
}

/// Convert a source-only request into a Rust-authored manifest before any
/// model/provider interpretation is accepted.
pub fn prepare_source_bundle(
    draft: SourceBundleDraft,
    source_bytes_by_ref: &BTreeMap<String, Vec<u8>>,
) -> Result<SourceBundle, ContractError> {
    require_schema(&draft.schema_version, SOURCE_BUNDLE_DRAFT_SCHEMA)?;
    require_text(&draft.request_id, "source_bundle.request_id", 128)?;
    if draft.request_id.starts_with("source-bundle:") {
        return Err(ContractError::Schema(
            "request_id must be an opaque fresh request token".into(),
        ));
    }
    if draft.sources.is_empty() {
        return Err(ContractError::Schema(
            "source bundle must contain at least one source".into(),
        ));
    }
    let mut refs = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut sources = Vec::with_capacity(draft.sources.len());
    for source in draft.sources {
        require_text(&source.source_ref, "source.source_ref", 64)?;
        if !source
            .source_ref
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            || !refs.insert(source.source_ref.clone())
        {
            return Err(ContractError::Schema(
                "source_ref is invalid or duplicated".into(),
            ));
        }
        validate_provenance(&source.provenance)?;
        validate_media_type(&source.media_type)?;
        if !valid_digest(&source.content_sha256) {
            return Err(ContractError::Provenance(format!(
                "source {} has malformed digest",
                source.source_ref
            )));
        }
        let bytes = source_bytes_by_ref.get(&source.source_ref).ok_or_else(|| {
            ContractError::Provenance(format!("missing source bytes for {}", source.source_ref))
        })?;
        if sha256_prefixed(bytes) != source.content_sha256 {
            return Err(ContractError::Provenance(format!(
                "source {} bytes are stale or mismatched",
                source.source_ref
            )));
        }
        let mut record = SourceRecord {
            source_id: String::new(),
            kind: source.kind,
            content_sha256: source.content_sha256,
            byte_length: bytes.len() as u64,
            media_type: source.media_type,
            provenance: source.provenance,
        };
        record.source_id = source_identity(&record)?;
        if !ids.insert(record.source_id.clone()) {
            return Err(ContractError::Identity(
                "duplicate source identity in bundle".into(),
            ));
        }
        sources.push(record);
    }
    sources.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    let mut bundle = SourceBundle {
        schema_version: SOURCE_BUNDLE_SCHEMA.to_owned(),
        request_id: draft.request_id,
        source_bundle_id: String::new(),
        sources,
    };
    bundle.source_bundle_id = source_bundle_identity(&bundle.request_id, &bundle.sources)?;
    Ok(bundle)
}

/// Recompute source identities and verify the actual bytes before provider
/// output can consume this bundle.
pub fn validate_source_bundle(
    bundle: &SourceBundle,
    source_bytes_by_id: &BTreeMap<String, Vec<u8>>,
) -> Result<(), ContractError> {
    require_schema(&bundle.schema_version, SOURCE_BUNDLE_SCHEMA)?;
    require_text(&bundle.request_id, "source_bundle.request_id", 128)?;
    if bundle.request_id.starts_with("source-bundle:") {
        return Err(ContractError::Schema(
            "request_id must be an opaque fresh request token".into(),
        ));
    }
    if bundle.sources.is_empty() {
        return Err(ContractError::Schema("source bundle is empty".into()));
    }
    let mut ids = BTreeSet::new();
    for source in &bundle.sources {
        validate_provenance(&source.provenance)?;
        validate_media_type(&source.media_type)?;
        if !valid_digest(&source.content_sha256)
            || source_identity(source)? != source.source_id
            || !ids.insert(source.source_id.clone())
        {
            return Err(ContractError::Identity(format!(
                "forged or duplicate source id {}",
                source.source_id
            )));
        }
        let bytes = source_bytes_by_id.get(&source.source_id).ok_or_else(|| {
            ContractError::Provenance(format!("missing bytes for source {}", source.source_id))
        })?;
        if sha256_prefixed(bytes) != source.content_sha256
            || bytes.len() as u64 != source.byte_length
        {
            return Err(ContractError::Provenance(format!(
                "stale bytes for source {}",
                source.source_id
            )));
        }
    }
    if source_bundle_identity(&bundle.request_id, &bundle.sources)? != bundle.source_bundle_id {
        return Err(ContractError::Identity(
            "forged or stale source_bundle_id".into(),
        ));
    }
    Ok(())
}

fn assumption_identity(assumption: &SemanticAssumption) -> Result<String, ContractError> {
    #[derive(Serialize)]
    struct Body<'a> {
        statement: &'a str,
        rationale: &'a str,
        confidence: f64,
        related_claim_ids: &'a [String],
    }
    let body = Body {
        statement: &assumption.statement,
        rationale: &assumption.rationale,
        confidence: assumption.confidence,
        related_claim_ids: &assumption.related_claim_ids,
    };
    Ok(identity("assumption", &canonical_digest(&body)?))
}

fn conflict_identity(conflict: &SemanticConflict) -> Result<String, ContractError> {
    #[derive(Serialize)]
    struct Body<'a> {
        left_claim_id: &'a str,
        right_claim_id: &'a str,
        explanation: &'a str,
    }
    let body = Body {
        left_claim_id: &conflict.left_claim_id,
        right_claim_id: &conflict.right_claim_id,
        explanation: &conflict.explanation,
    };
    Ok(identity("conflict", &canonical_digest(&body)?))
}

fn intake_identity(intake: &SemanticIntake) -> Result<String, ContractError> {
    let mut sources = intake.sources.clone();
    let mut observations = intake.observations.clone();
    let mut inferences = intake.inferences.clone();
    let mut conflicts = intake.conflicts.clone();
    let mut assumptions = intake.assumptions.clone();
    sources.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    observations.sort_by(|a, b| a.claim_id.cmp(&b.claim_id));
    inferences.sort_by(|a, b| a.claim_id.cmp(&b.claim_id));
    conflicts.sort_by(|a, b| a.conflict_id.cmp(&b.conflict_id));
    assumptions.sort_by(|a, b| a.assumption_id.cmp(&b.assumption_id));
    let body = IntakeIdentity {
        schema_version: &intake.schema_version,
        request_id: &intake.request_id,
        source_bundle_id: &intake.source_bundle_id,
        provider: &intake.provider,
        sources: &sources,
        observations: &observations,
        inferences: &inferences,
        conflicts: &conflicts,
        assumptions: &assumptions,
    };
    Ok(identity("intake", &canonical_digest(&body)?))
}

/// Normalize provider interpretation against a fresh Rust-authored source
/// bundle. Raw source bytes are indexed by their stable source IDs.
pub fn normalize_intake(
    draft: IntakeDraft,
    source_bundle: &SourceBundle,
    provider_response_bytes: &[u8],
    source_bytes_by_id: &BTreeMap<String, Vec<u8>>,
) -> Result<SemanticIntake, ContractError> {
    require_schema(&draft.schema_version, INTAKE_DRAFT_SCHEMA)?;
    validate_source_bundle(source_bundle, source_bytes_by_id)?;
    validate_provider(&draft.provider, provider_response_bytes)?;
    let raw_interpretation: ProviderInterpretation = parse_json(provider_response_bytes)?;
    if raw_interpretation != draft.interpretation {
        return Err(ContractError::Provenance(
            "typed interpretation does not match the hashed raw provider response".into(),
        ));
    }
    require_schema(
        &draft.interpretation.schema_version,
        PROVIDER_INTERPRETATION_SCHEMA,
    )?;
    if draft.source_bundle_id != source_bundle.source_bundle_id
        || draft.provider.request_source_bundle_id != source_bundle.source_bundle_id
        || draft.interpretation.source_bundle_id != draft.source_bundle_id
    {
        return Err(ContractError::Provenance(
            "provider response is bound to another source bundle".into(),
        ));
    }
    if draft.interpretation.claims.is_empty() {
        return Err(ContractError::Schema(
            "intake requires at least one typed claim".into(),
        ));
    }
    let ProviderInterpretation {
        claims,
        conflicts: draft_conflicts,
        assumptions: draft_assumptions,
        ..
    } = draft.interpretation;
    let sources_by_id: BTreeMap<_, _> = source_bundle
        .sources
        .iter()
        .map(|source| (source.source_id.as_str(), source))
        .collect();

    let mut claims_by_ref = BTreeMap::<String, (EpistemicKind, SemanticClaim)>::new();
    let mut claim_ids = BTreeSet::new();
    for claim in claims {
        require_text(&claim.claim_ref, "claim.claim_ref", 64)?;
        require_text(&claim.statement, "claim.statement", 4096)?;
        validate_confidence(claim.confidence, "claim.confidence")?;
        if claim.evidence.is_empty() {
            return Err(ContractError::Provenance(format!(
                "claim {} has no source evidence",
                claim.claim_ref
            )));
        }
        let mut evidence = Vec::with_capacity(claim.evidence.len());
        let mut evidence_keys = BTreeSet::new();
        for link in claim.evidence {
            let source = sources_by_id.get(link.source_id.as_str()).ok_or_else(|| {
                ContractError::Provenance(format!(
                    "claim {} references unknown source {}",
                    claim.claim_ref, link.source_id
                ))
            })?;
            if let Some(region) = &link.region {
                validate_region(region, source.kind, source.byte_length)?;
            }
            if claim.epistemic_kind == EpistemicKind::Observation && link.region.is_none() {
                return Err(ContractError::Provenance(format!(
                    "observation {} requires a source region for each evidence link",
                    claim.claim_ref
                )));
            }
            let canonical_link = EvidenceLink {
                source_id: source.source_id.clone(),
                region: link.region,
            };
            let key = canonical_digest(&canonical_link)?;
            if !evidence_keys.insert(key) {
                return Err(ContractError::Schema(format!(
                    "claim {} repeats an evidence link",
                    claim.claim_ref
                )));
            }
            evidence.push(canonical_link);
        }
        evidence.sort_by(|a, b| {
            (
                &a.source_id,
                canonical_json(&serde_json::to_value(&a.region).unwrap_or(Value::Null)),
            )
                .cmp(&(
                    &b.source_id,
                    canonical_json(&serde_json::to_value(&b.region).unwrap_or(Value::Null)),
                ))
        });
        let mut canonical_claim = SemanticClaim {
            claim_id: String::new(),
            domain: claim.domain,
            statement: claim.statement,
            confidence: claim.confidence,
            evidence,
        };
        // The epistemic kind participates in identity without being confused
        // with provider-local refs. The prefix makes it recoverable for hashing.
        let raw_id = identity(
            "claim",
            &canonical_digest(&ClaimIdentityBody {
                epistemic_kind: claim.epistemic_kind,
                domain: canonical_claim.domain,
                statement: &canonical_claim.statement,
                confidence: canonical_claim.confidence,
                evidence: &canonical_claim.evidence,
            })?,
        );
        canonical_claim.claim_id = format!(
            "{}:{raw_id}",
            match claim.epistemic_kind {
                EpistemicKind::Observation => "observation",
                EpistemicKind::Inference => "inference",
            }
        );
        if !claim_ids.insert(canonical_claim.claim_id.clone()) {
            return Err(ContractError::Identity("duplicate claim identity".into()));
        }
        if claims_by_ref
            .insert(claim.claim_ref, (claim.epistemic_kind, canonical_claim))
            .is_some()
        {
            return Err(ContractError::Schema("duplicate claim_ref".into()));
        }
    }

    let source_bundle_id = source_bundle.source_bundle_id.clone();

    let claim_ref_to_id: BTreeMap<_, _> = claims_by_ref
        .iter()
        .map(|(reference, (_, claim))| (reference.clone(), claim.claim_id.clone()))
        .collect();
    let mut conflicts = Vec::new();
    let mut conflict_refs = BTreeSet::new();
    for conflict in draft_conflicts {
        require_text(&conflict.conflict_ref, "conflict.conflict_ref", 64)?;
        require_text(&conflict.explanation, "conflict.explanation", 2048)?;
        if !conflict_refs.insert(conflict.conflict_ref) {
            return Err(ContractError::Schema("duplicate conflict_ref".into()));
        }
        let left = claim_ref_to_id
            .get(&conflict.left_claim_ref)
            .ok_or_else(|| {
                ContractError::Provenance("conflict references unknown left claim".into())
            })?;
        let right = claim_ref_to_id
            .get(&conflict.right_claim_ref)
            .ok_or_else(|| {
                ContractError::Provenance("conflict references unknown right claim".into())
            })?;
        if left == right {
            return Err(ContractError::Schema(
                "conflict endpoints must be distinct claims".into(),
            ));
        }
        let mut record = SemanticConflict {
            conflict_id: String::new(),
            left_claim_id: left.clone(),
            right_claim_id: right.clone(),
            explanation: conflict.explanation,
        };
        record.conflict_id = conflict_identity(&record)?;
        conflicts.push(record);
    }

    let mut assumptions = Vec::new();
    let mut assumption_refs = BTreeSet::new();
    for assumption in draft_assumptions {
        require_text(&assumption.assumption_ref, "assumption.assumption_ref", 64)?;
        require_text(&assumption.statement, "assumption.statement", 4096)?;
        require_text(&assumption.rationale, "assumption.rationale", 2048)?;
        validate_confidence(assumption.confidence, "assumption.confidence")?;
        if !assumption_refs.insert(assumption.assumption_ref) {
            return Err(ContractError::Schema("duplicate assumption_ref".into()));
        }
        let mut related_claim_ids = Vec::new();
        for claim_ref in assumption.related_claim_refs {
            let claim_id = claim_ref_to_id.get(&claim_ref).ok_or_else(|| {
                ContractError::Provenance(format!(
                    "assumption references unknown claim {claim_ref}"
                ))
            })?;
            related_claim_ids.push(claim_id.clone());
        }
        related_claim_ids.sort();
        related_claim_ids.dedup();
        let mut record = SemanticAssumption {
            assumption_id: String::new(),
            statement: assumption.statement,
            rationale: assumption.rationale,
            confidence: assumption.confidence,
            related_claim_ids,
        };
        record.assumption_id = assumption_identity(&record)?;
        assumptions.push(record);
    }

    let mut observations = Vec::new();
    let mut inferences = Vec::new();
    for (_, (kind, claim)) in claims_by_ref {
        match kind {
            EpistemicKind::Observation => observations.push(claim),
            EpistemicKind::Inference => inferences.push(claim),
        }
    }
    let mut intake = SemanticIntake {
        schema_version: INTAKE_SCHEMA.to_owned(),
        intake_id: String::new(),
        request_id: source_bundle.request_id.clone(),
        source_bundle_id,
        provider: draft.provider,
        sources: source_bundle.sources.clone(),
        observations,
        inferences,
        conflicts,
        assumptions,
    };
    sort_intake(&mut intake);
    intake.intake_id = intake_identity(&intake)?;
    Ok(intake)
}

#[derive(Serialize)]
struct ClaimIdentityBody<'a> {
    epistemic_kind: EpistemicKind,
    domain: ClaimDomain,
    statement: &'a str,
    confidence: f64,
    evidence: &'a [EvidenceLink],
}

fn sort_intake(intake: &mut SemanticIntake) {
    intake.sources.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    intake
        .observations
        .sort_by(|a, b| a.claim_id.cmp(&b.claim_id));
    intake
        .inferences
        .sort_by(|a, b| a.claim_id.cmp(&b.claim_id));
    intake
        .conflicts
        .sort_by(|a, b| a.conflict_id.cmp(&b.conflict_id));
    intake
        .assumptions
        .sort_by(|a, b| a.assumption_id.cmp(&b.assumption_id));
}

/// Revalidate a serialized canonical intake against fresh response and source
/// bytes. This rejects forged IDs and stale source/provider payloads.
pub fn validate_intake(
    intake: &SemanticIntake,
    provider_response_bytes: &[u8],
    source_bytes: &BTreeMap<String, Vec<u8>>,
) -> Result<(), ContractError> {
    let source_bundle = SourceBundle {
        schema_version: SOURCE_BUNDLE_SCHEMA.to_owned(),
        request_id: intake.request_id.clone(),
        source_bundle_id: intake.source_bundle_id.clone(),
        sources: intake.sources.clone(),
    };
    let interpretation: ProviderInterpretation = parse_json(provider_response_bytes)?;
    let draft = IntakeDraft {
        schema_version: INTAKE_DRAFT_SCHEMA.to_owned(),
        source_bundle_id: intake.source_bundle_id.clone(),
        provider: intake.provider.clone(),
        interpretation,
    };
    let rebuilt = normalize_intake(draft, &source_bundle, provider_response_bytes, source_bytes)?;
    if &rebuilt != intake {
        return Err(ContractError::Identity(
            "canonical intake differs from source-bound provider output".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FailedLayer {
    SemanticIntake,
    WorldLayout,
    Collision,
    Navigation,
    Gameplay,
    AssetPreparation,
    VisualQuality,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum GateId {
    SemanticIntake,
    WorldLayout,
    Collision,
    Navigation,
    Traversal,
    Gameplay,
    AssetPreparation,
    VisualQuality,
}

impl FailedLayer {
    fn accepts_gate(self, gate: GateId) -> bool {
        matches!(
            (self, gate),
            (Self::SemanticIntake, GateId::SemanticIntake)
                | (Self::WorldLayout, GateId::WorldLayout)
                | (Self::Collision, GateId::Collision)
                | (Self::Navigation, GateId::Navigation | GateId::Traversal)
                | (Self::Gameplay, GateId::Gameplay)
                | (Self::AssetPreparation, GateId::AssetPreparation)
                | (Self::VisualQuality, GateId::VisualQuality)
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RepairEditClass {
    ReviseInference,
    ResolveIntakeConflict,
    RecordAssumption,
    AdjustWorldLayout,
    RelocateSpawn,
    AdjustCollision,
    ReconnectNavigation,
    WidenRoute,
    AdjustGameplayRule,
    AdjustAssetMetadata,
    AdjustCameraOrLighting,
}

fn edit_allowed(layer: FailedLayer, edit: RepairEditClass) -> bool {
    matches!(
        (layer, edit),
        (
            FailedLayer::SemanticIntake,
            RepairEditClass::ReviseInference
                | RepairEditClass::ResolveIntakeConflict
                | RepairEditClass::RecordAssumption
        ) | (
            FailedLayer::WorldLayout,
            RepairEditClass::AdjustWorldLayout | RepairEditClass::RelocateSpawn
        ) | (FailedLayer::Collision, RepairEditClass::AdjustCollision)
            | (
                FailedLayer::Navigation,
                RepairEditClass::ReconnectNavigation | RepairEditClass::WidenRoute
            )
            | (FailedLayer::Gameplay, RepairEditClass::AdjustGameplayRule)
            | (
                FailedLayer::AssetPreparation,
                RepairEditClass::AdjustAssetMetadata
            )
            | (
                FailedLayer::VisualQuality,
                RepairEditClass::AdjustCameraOrLighting
            )
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReference {
    pub validator_id: String,
    pub schema_version: String,
    pub gate_id: GateId,
    pub candidate_sha256: String,
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepairTarget {
    pub artifact_id: String,
    pub before_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepairProposalDraft {
    pub schema_version: String,
    pub candidate_before_sha256: String,
    pub failed_layer: FailedLayer,
    pub failure_evidence: EvidenceReference,
    pub edit_class: RepairEditClass,
    pub authorized_targets: Vec<RepairTarget>,
    pub max_artifact_changes: u8,
    pub diagnosis: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepairProposal {
    pub schema_version: String,
    pub proposal_id: String,
    pub candidate_before_sha256: String,
    pub failed_layer: FailedLayer,
    pub failure_evidence: EvidenceReference,
    pub edit_class: RepairEditClass,
    pub authorized_targets: Vec<RepairTarget>,
    pub max_artifact_changes: u8,
    pub diagnosis: String,
    pub rationale: String,
}

#[derive(Serialize)]
struct ProposalIdentity<'a> {
    schema_version: &'a str,
    candidate_before_sha256: &'a str,
    failed_layer: FailedLayer,
    failure_evidence: &'a EvidenceReference,
    edit_class: RepairEditClass,
    authorized_targets: &'a [RepairTarget],
    max_artifact_changes: u8,
    diagnosis: &'a str,
    rationale: &'a str,
}

fn proposal_id(proposal: &RepairProposal) -> Result<String, ContractError> {
    let body = ProposalIdentity {
        schema_version: &proposal.schema_version,
        candidate_before_sha256: &proposal.candidate_before_sha256,
        failed_layer: proposal.failed_layer,
        failure_evidence: &proposal.failure_evidence,
        edit_class: proposal.edit_class,
        authorized_targets: &proposal.authorized_targets,
        max_artifact_changes: proposal.max_artifact_changes,
        diagnosis: &proposal.diagnosis,
        rationale: &proposal.rationale,
    };
    Ok(identity("repair", &canonical_digest(&body)?))
}

fn validate_proposal_fields(proposal: &RepairProposal) -> Result<(), ContractError> {
    require_schema(&proposal.schema_version, REPAIR_PROPOSAL_SCHEMA)?;
    if !valid_digest(&proposal.candidate_before_sha256)
        || proposal.failure_evidence.candidate_sha256 != proposal.candidate_before_sha256
        || !valid_digest(&proposal.failure_evidence.receipt_sha256)
        || !proposal
            .failed_layer
            .accepts_gate(proposal.failure_evidence.gate_id)
    {
        return Err(ContractError::Repair(
            "proposal failure evidence is malformed or bound to another candidate/layer".into(),
        ));
    }
    require_text(
        &proposal.failure_evidence.validator_id,
        "failure_evidence.validator_id",
        128,
    )?;
    require_text(
        &proposal.failure_evidence.schema_version,
        "failure_evidence.schema_version",
        128,
    )?;
    require_text(&proposal.diagnosis, "diagnosis", 2048)?;
    require_text(&proposal.rationale, "rationale", 2048)?;
    if !edit_allowed(proposal.failed_layer, proposal.edit_class) {
        return Err(ContractError::Repair(
            "edit class is not authorized for failed layer".into(),
        ));
    }
    // A world-layout repair may legitimately replace the authored layout and
    // the small, deterministic set of Rust/Julia-derived runtime evidence
    // that depends on it. Keep that repair bounded, but do not force callers
    // to hide those derived changes behind an under-specified target list.
    if proposal.authorized_targets.is_empty()
        || proposal.authorized_targets.len() > 8
        || proposal.max_artifact_changes == 0
        || usize::from(proposal.max_artifact_changes) > proposal.authorized_targets.len()
    {
        return Err(ContractError::Repair(
            "repair scope must authorize 1..=8 targets and no more changes than targets".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    for target in &proposal.authorized_targets {
        require_text(&target.artifact_id, "target.artifact_id", 128)?;
        if !valid_digest(&target.before_sha256) || !ids.insert(target.artifact_id.as_str()) {
            return Err(ContractError::Repair(
                "repair target digest is malformed or target is duplicated".into(),
            ));
        }
    }
    if proposal_id(proposal)? != proposal.proposal_id {
        return Err(ContractError::Identity(
            "forged or stale proposal_id".into(),
        ));
    }
    Ok(())
}

pub fn normalize_repair_proposal(
    draft: RepairProposalDraft,
) -> Result<RepairProposal, ContractError> {
    require_schema(&draft.schema_version, REPAIR_PROPOSAL_DRAFT_SCHEMA)?;
    let mut proposal = RepairProposal {
        schema_version: REPAIR_PROPOSAL_SCHEMA.to_owned(),
        proposal_id: String::new(),
        candidate_before_sha256: draft.candidate_before_sha256,
        failed_layer: draft.failed_layer,
        failure_evidence: draft.failure_evidence,
        edit_class: draft.edit_class,
        authorized_targets: draft.authorized_targets,
        max_artifact_changes: draft.max_artifact_changes,
        diagnosis: draft.diagnosis,
        rationale: draft.rationale,
    };
    proposal
        .authorized_targets
        .sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
    validate_proposal_fields(&RepairProposal {
        proposal_id: "pending".into(),
        ..proposal.clone()
    })
    .or_else(|error| match error {
        ContractError::Identity(_) => Ok(()),
        other => Err(other),
    })?;
    proposal.proposal_id = proposal_id(&proposal)?;
    validate_proposal_fields(&proposal)?;
    Ok(proposal)
}

pub fn validate_repair_proposal(proposal: &RepairProposal) -> Result<(), ContractError> {
    validate_proposal_fields(proposal)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptOutcome {
    Fail,
    Pass,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MetricId {
    UnresolvedConflicts,
    NavigationComponents,
    TraversalBlockedSteps,
    CollisionPenetrations,
    GameplayObjectiveCompletion,
    MissingAssetFeatures,
    VisualSimilarityScore,
    ReachableObjectives,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MetricObservation {
    pub metric_id: MetricId,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeFinding {
    pub code: String,
    pub artifact_sha256: String,
    pub detail: String,
}

/// Output of a registered native validator. It is produced by Rust integration
/// code only after parsing and validating the raw receipt bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ValidatedNativeReceipt {
    pub validator_id: String,
    pub schema_version: String,
    pub gate_id: GateId,
    pub candidate_sha256: String,
    pub artifact_sha256: String,
    pub outcome: ReceiptOutcome,
    pub findings: Vec<NativeFinding>,
    pub measurements: Vec<MetricObservation>,
}

pub type NativeReceiptValidator = fn(&[u8]) -> Result<ValidatedNativeReceipt, ContractError>;

#[derive(Clone)]
struct ValidatorRegistration {
    schema_version: String,
    validate: NativeReceiptValidator,
}

/// Registry is assembled by trusted Rust integration code. No provider JSON
/// can register or override a validator.
#[derive(Default, Clone)]
pub struct NativeValidatorRegistry {
    entries: BTreeMap<String, ValidatorRegistration>,
}

impl NativeValidatorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        validator_id: impl Into<String>,
        schema_version: impl Into<String>,
        validate: NativeReceiptValidator,
    ) -> Result<(), ContractError> {
        let validator_id = validator_id.into();
        let schema_version = schema_version.into();
        require_text(&validator_id, "validator_id", 128)?;
        require_text(&schema_version, "schema_version", 128)?;
        if self.entries.contains_key(&validator_id) {
            return Err(ContractError::Registry(format!(
                "validator {validator_id} is already registered"
            )));
        }
        self.entries.insert(
            validator_id,
            ValidatorRegistration {
                schema_version,
                validate,
            },
        );
        Ok(())
    }

    fn validate_receipt(
        &self,
        reference: &EvidenceReference,
        receipt_bytes: &[u8],
    ) -> Result<ValidatedNativeReceipt, ContractError> {
        let registration = self.entries.get(&reference.validator_id).ok_or_else(|| {
            ContractError::Registry(format!(
                "unknown native validator {}",
                reference.validator_id
            ))
        })?;
        if registration.schema_version != reference.schema_version {
            return Err(ContractError::Registry(format!(
                "validator {} does not register schema {}",
                reference.validator_id, reference.schema_version
            )));
        }
        if sha256_prefixed(receipt_bytes) != reference.receipt_sha256 {
            return Err(ContractError::Provenance(
                "native receipt bytes are stale or mismatched".into(),
            ));
        }
        let receipt = (registration.validate)(receipt_bytes)?;
        if receipt.validator_id != reference.validator_id
            || receipt.schema_version != reference.schema_version
            || receipt.gate_id != reference.gate_id
            || receipt.candidate_sha256 != reference.candidate_sha256
        {
            return Err(ContractError::Registry(
                "native receipt binding does not match registered reference".into(),
            ));
        }
        if !valid_digest(&receipt.candidate_sha256) || !valid_digest(&receipt.artifact_sha256) {
            return Err(ContractError::Provenance(
                "native receipt contains malformed artifact identity".into(),
            ));
        }
        if receipt.measurements.is_empty() {
            return Err(ContractError::Registry(
                "status-only native evidence is not admissible".into(),
            ));
        }
        if receipt.measurements.len() > 64 || receipt.findings.len() > 64 {
            return Err(ContractError::Registry(
                "native receipt exceeds bounded evidence limits".into(),
            ));
        }
        let mut metric_ids = BTreeSet::new();
        for measurement in &receipt.measurements {
            if !measurement.value.is_finite() || !metric_ids.insert(measurement.metric_id) {
                return Err(ContractError::Registry(
                    "native receipt has a non-finite or duplicate measurement".into(),
                ));
            }
        }
        if receipt.outcome == ReceiptOutcome::Fail && receipt.findings.is_empty() {
            return Err(ContractError::Registry(
                "failed native receipt must include typed findings".into(),
            ));
        }
        if receipt.outcome == ReceiptOutcome::Pass && !receipt.findings.is_empty() {
            return Err(ContractError::Registry(
                "passing native receipt cannot retain blocking findings".into(),
            ));
        }
        for finding in &receipt.findings {
            require_text(&finding.code, "finding.code", 128)?;
            require_text(&finding.detail, "finding.detail", 1024)?;
            if !valid_digest(&finding.artifact_sha256)
                || finding.artifact_sha256 != receipt.artifact_sha256
            {
                return Err(ContractError::Registry(
                    "finding artifact digest is malformed".into(),
                ));
            }
        }
        Ok(receipt)
    }
}

pub fn validate_proposal_evidence(
    proposal: &RepairProposal,
    candidate_bytes: &[u8],
    receipt_bytes: &[u8],
    registry: &NativeValidatorRegistry,
) -> Result<ValidatedNativeReceipt, ContractError> {
    validate_proposal_fields(proposal)?;
    if sha256_prefixed(candidate_bytes) != proposal.candidate_before_sha256 {
        return Err(ContractError::Provenance(
            "candidate bytes do not match repair proposal".into(),
        ));
    }
    let receipt = registry.validate_receipt(&proposal.failure_evidence, receipt_bytes)?;
    if receipt.outcome != ReceiptOutcome::Fail
        || !proposal.failed_layer.accepts_gate(receipt.gate_id)
    {
        return Err(ContractError::Repair(
            "proposal is not anchored to a failing receipt for its declared layer".into(),
        ));
    }
    if !proposal
        .authorized_targets
        .iter()
        .any(|target| target.before_sha256 == receipt.artifact_sha256)
    {
        return Err(ContractError::Repair(
            "failed receipt artifact is outside the authorized repair scope".into(),
        ));
    }
    Ok(receipt)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactChange {
    pub artifact_id: String,
    pub before_sha256: String,
    pub after_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepairEvidenceDeltaDraft {
    pub schema_version: String,
    pub proposal_id: String,
    pub candidate_before_sha256: String,
    pub candidate_after_sha256: String,
    pub before_evidence: EvidenceReference,
    pub after_evidence: EvidenceReference,
    pub changed_artifacts: Vec<ArtifactChange>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairAssessment {
    GatePassed,
    ImprovedStillFailing,
    StillFailing,
    Regressed,
    Mixed,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MetricDelta {
    pub metric_id: MetricId,
    pub before: f64,
    pub after: f64,
    pub direction: MetricDirection,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MetricDirection {
    LowerIsBetter,
    HigherIsBetter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepairEvidenceDelta {
    pub schema_version: String,
    pub delta_id: String,
    pub proposal_id: String,
    pub candidate_before_sha256: String,
    pub candidate_after_sha256: String,
    pub failed_layer: FailedLayer,
    pub before_receipt_sha256: String,
    pub after_receipt_sha256: String,
    pub before_outcome: ReceiptOutcome,
    pub after_outcome: ReceiptOutcome,
    pub assessment: RepairAssessment,
    pub changed_artifacts: Vec<ArtifactChange>,
    pub metric_deltas: Vec<MetricDelta>,
    pub explanation: String,
}

fn metric_direction(metric: MetricId) -> MetricDirection {
    match metric {
        MetricId::GameplayObjectiveCompletion
        | MetricId::VisualSimilarityScore
        | MetricId::ReachableObjectives => MetricDirection::HigherIsBetter,
        MetricId::UnresolvedConflicts
        | MetricId::NavigationComponents
        | MetricId::TraversalBlockedSteps
        | MetricId::CollisionPenetrations
        | MetricId::MissingAssetFeatures => MetricDirection::LowerIsBetter,
    }
}

fn compare_metrics(
    before: &[MetricObservation],
    after: &[MetricObservation],
) -> (Vec<MetricDelta>, bool, bool, bool) {
    let before_map: BTreeMap<_, _> = before
        .iter()
        .map(|item| (item.metric_id, item.value))
        .collect();
    let after_map: BTreeMap<_, _> = after
        .iter()
        .map(|item| (item.metric_id, item.value))
        .collect();
    let mut deltas = Vec::new();
    let (mut improved, mut regressed, mut incomparable) = (false, false, false);
    for (metric, before_value) in before_map {
        let Some(after_value) = after_map.get(&metric).copied() else {
            incomparable = true;
            continue;
        };
        let direction = metric_direction(metric);
        if before_value == after_value {
            // Equal values are not evidence of progress.
        } else {
            match direction {
                MetricDirection::LowerIsBetter if after_value < before_value => improved = true,
                MetricDirection::HigherIsBetter if after_value > before_value => improved = true,
                _ => regressed = true,
            }
        }
        deltas.push(MetricDelta {
            metric_id: metric,
            before: before_value,
            after: after_value,
            direction,
        });
    }
    if after_map
        .keys()
        .any(|metric| !deltas.iter().any(|delta| delta.metric_id == *metric))
    {
        incomparable = true;
    }
    (deltas, improved, regressed, incomparable)
}

fn assess(
    before: &ValidatedNativeReceipt,
    after: &ValidatedNativeReceipt,
) -> (RepairAssessment, Vec<MetricDelta>) {
    let (deltas, improved, regressed, incomparable) =
        compare_metrics(&before.measurements, &after.measurements);
    if after.outcome == ReceiptOutcome::Pass {
        return (RepairAssessment::GatePassed, deltas);
    }
    if after.outcome == ReceiptOutcome::Indeterminate {
        return (RepairAssessment::Inconclusive, deltas);
    }
    let assessment = match (improved, regressed, incomparable) {
        (true, false, false) => RepairAssessment::ImprovedStillFailing,
        (false, true, false) => RepairAssessment::Regressed,
        (true, true, _) | (true, false, true) | (false, true, true) => RepairAssessment::Mixed,
        (false, false, true) => RepairAssessment::Inconclusive,
        (false, false, false) => RepairAssessment::StillFailing,
    };
    (assessment, deltas)
}

fn delta_id(delta: &RepairEvidenceDelta) -> Result<String, ContractError> {
    #[derive(Serialize)]
    struct Body<'a> {
        schema_version: &'a str,
        proposal_id: &'a str,
        candidate_before_sha256: &'a str,
        candidate_after_sha256: &'a str,
        failed_layer: FailedLayer,
        before_receipt_sha256: &'a str,
        after_receipt_sha256: &'a str,
        before_outcome: ReceiptOutcome,
        after_outcome: ReceiptOutcome,
        assessment: RepairAssessment,
        changed_artifacts: &'a [ArtifactChange],
        metric_deltas: &'a [MetricDelta],
        explanation: &'a str,
    }
    let body = Body {
        schema_version: &delta.schema_version,
        proposal_id: &delta.proposal_id,
        candidate_before_sha256: &delta.candidate_before_sha256,
        candidate_after_sha256: &delta.candidate_after_sha256,
        failed_layer: delta.failed_layer,
        before_receipt_sha256: &delta.before_receipt_sha256,
        after_receipt_sha256: &delta.after_receipt_sha256,
        before_outcome: delta.before_outcome,
        after_outcome: delta.after_outcome,
        assessment: delta.assessment,
        changed_artifacts: &delta.changed_artifacts,
        metric_deltas: &delta.metric_deltas,
        explanation: &delta.explanation,
    };
    Ok(identity("repair-delta", &canonical_digest(&body)?))
}

/// Independently revalidate raw candidate, artifact and native receipt bytes,
/// then derive the outcome and explanation. Caller-supplied pass/improvement
/// fields do not exist in the draft schema.
#[allow(clippy::too_many_arguments)]
pub fn validate_repair_delta(
    proposal: &RepairProposal,
    draft: RepairEvidenceDeltaDraft,
    before_candidate_bytes: &[u8],
    after_candidate_bytes: &[u8],
    before_receipt_bytes: &[u8],
    after_receipt_bytes: &[u8],
    before_artifacts: &BTreeMap<String, Vec<u8>>,
    after_artifacts: &BTreeMap<String, Vec<u8>>,
    registry: &NativeValidatorRegistry,
) -> Result<RepairEvidenceDelta, ContractError> {
    validate_proposal_fields(proposal)?;
    require_schema(&draft.schema_version, REPAIR_DELTA_DRAFT_SCHEMA)?;
    if draft.proposal_id != proposal.proposal_id
        || draft.candidate_before_sha256 != proposal.candidate_before_sha256
        || sha256_prefixed(before_candidate_bytes) != draft.candidate_before_sha256
        || sha256_prefixed(after_candidate_bytes) != draft.candidate_after_sha256
        || !valid_digest(&draft.candidate_after_sha256)
        || draft.candidate_before_sha256 == draft.candidate_after_sha256
    {
        return Err(ContractError::Provenance(
            "repair delta is stale or bound to another candidate".into(),
        ));
    }
    if draft.before_evidence != proposal.failure_evidence
        || draft.after_evidence.validator_id != draft.before_evidence.validator_id
        || draft.after_evidence.schema_version != draft.before_evidence.schema_version
        || draft.after_evidence.gate_id != draft.before_evidence.gate_id
        || draft.after_evidence.candidate_sha256 != draft.candidate_after_sha256
    {
        return Err(ContractError::Repair(
            "before/after receipt references do not bind to the proposal and same native gate"
                .into(),
        ));
    }
    let before = validate_proposal_evidence(
        proposal,
        before_candidate_bytes,
        before_receipt_bytes,
        registry,
    )?;
    let after = registry.validate_receipt(&draft.after_evidence, after_receipt_bytes)?;
    if after.candidate_sha256 != draft.candidate_after_sha256 {
        return Err(ContractError::Provenance(
            "after receipt is bound to another candidate".into(),
        ));
    }
    if draft.changed_artifacts.is_empty()
        || draft.changed_artifacts.len() > usize::from(proposal.max_artifact_changes)
    {
        return Err(ContractError::Repair(
            "changed artifact count exceeds the authorized repair bound".into(),
        ));
    }
    let targets: BTreeMap<_, _> = proposal
        .authorized_targets
        .iter()
        .map(|target| (target.artifact_id.as_str(), target))
        .collect();
    let mut seen = BTreeSet::new();
    let mut changes = draft.changed_artifacts;
    changes.sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
    for change in &changes {
        let target = targets.get(change.artifact_id.as_str()).ok_or_else(|| {
            ContractError::Repair(format!(
                "artifact {} is outside authorized scope",
                change.artifact_id
            ))
        })?;
        if !seen.insert(change.artifact_id.as_str())
            || change.before_sha256 != target.before_sha256
            || !valid_digest(&change.after_sha256)
            || change.after_sha256 == change.before_sha256
        {
            return Err(ContractError::Repair(format!(
                "invalid or duplicate change for {}",
                change.artifact_id
            )));
        }
        let before_bytes = before_artifacts.get(&change.artifact_id).ok_or_else(|| {
            ContractError::Provenance(format!("missing before bytes for {}", change.artifact_id))
        })?;
        let after_bytes = after_artifacts.get(&change.artifact_id).ok_or_else(|| {
            ContractError::Provenance(format!("missing after bytes for {}", change.artifact_id))
        })?;
        if sha256_prefixed(before_bytes) != change.before_sha256
            || sha256_prefixed(after_bytes) != change.after_sha256
        {
            return Err(ContractError::Provenance(format!(
                "stale artifact bytes for {}",
                change.artifact_id
            )));
        }
    }
    if !changes
        .iter()
        .any(|change| change.after_sha256 == after.artifact_sha256)
    {
        return Err(ContractError::Repair(
            "after receipt does not measure an artifact changed by this proposal".into(),
        ));
    }
    let (assessment, metric_deltas) = assess(&before, &after);
    let explanation = match assessment {
        RepairAssessment::GatePassed => "The registered native validator now passes the previously failing gate.".to_owned(),
        RepairAssessment::ImprovedStillFailing => "Measured failure metrics moved in the registered direction, but the native gate still fails.".to_owned(),
        RepairAssessment::StillFailing => "The native gate still fails and no measured failure metric improved.".to_owned(),
        RepairAssessment::Regressed => "The native gate still fails and at least one measured failure metric regressed.".to_owned(),
        RepairAssessment::Mixed => "Some measured failure metrics improved while others regressed; the native gate still fails.".to_owned(),
        RepairAssessment::Inconclusive => "The registered native validator could not establish comparable passing evidence.".to_owned(),
    };
    let mut delta = RepairEvidenceDelta {
        schema_version: REPAIR_DELTA_SCHEMA.to_owned(),
        delta_id: String::new(),
        proposal_id: proposal.proposal_id.clone(),
        candidate_before_sha256: draft.candidate_before_sha256,
        candidate_after_sha256: draft.candidate_after_sha256,
        failed_layer: proposal.failed_layer,
        before_receipt_sha256: draft.before_evidence.receipt_sha256,
        after_receipt_sha256: draft.after_evidence.receipt_sha256,
        before_outcome: before.outcome,
        after_outcome: after.outcome,
        assessment,
        changed_artifacts: changes,
        metric_deltas,
        explanation,
    };
    delta.delta_id = delta_id(&delta)?;
    Ok(delta)
}

/// Verify a serialized delta's own identity. Receipt and source bytes must be
/// checked with `validate_repair_delta` before any promotion decision.
pub fn validate_delta_identity(delta: &RepairEvidenceDelta) -> Result<(), ContractError> {
    require_schema(&delta.schema_version, REPAIR_DELTA_SCHEMA)?;
    for digest in [
        delta.candidate_before_sha256.as_str(),
        delta.candidate_after_sha256.as_str(),
        delta.before_receipt_sha256.as_str(),
        delta.after_receipt_sha256.as_str(),
    ] {
        if !valid_digest(digest) {
            return Err(ContractError::Identity(
                "repair delta contains malformed digest".into(),
            ));
        }
    }
    if delta.delta_id != delta_id(delta)? {
        return Err(ContractError::Identity(
            "forged or stale repair delta id".into(),
        ));
    }
    Ok(())
}

pub fn parse_json<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, ContractError> {
    serde_json::from_slice(bytes).map_err(|error| ContractError::Json(error.to_string()))
}

pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<Vec<u8>, ContractError> {
    serde_json::to_vec_pretty(value).map_err(|error| ContractError::Json(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha_digest_format_is_strict() {
        assert!(valid_digest(&sha256_prefixed(b"x")));
        assert!(!valid_digest(&format!("SHA256:{}", "a".repeat(64))));
        assert!(!valid_digest(&format!("sha256:{}", "A".repeat(64))));
    }
}
