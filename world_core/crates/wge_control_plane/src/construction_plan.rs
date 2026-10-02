//! Rust-owned project construction plan and readiness resolver.
//!
//! This is the bridge between a model's brief-level plan and later semantic
//! operations. It resolves declared needs against registered capabilities and
//! validators, but never executes a provider, mutates a project, or promotes
//! evidence.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use wge_certification_authority::{ValidatorRegistry, canonical_json, sha256_prefixed};

use crate::capability_registry::{CapabilityCatalog, CapabilityStatus};
use crate::style_profile::{StylePlan, StylePlanStatus};

pub const CONSTRUCTION_PLAN_SCHEMA: &str = "wge.construction-plan/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructionPlanError(pub String);

impl std::fmt::Display for ConstructionPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ConstructionPlanError {}

type Result<T> = std::result::Result<T, ConstructionPlanError>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityNeed {
    pub capability_id: String,
    pub required: bool,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetSourcePolicy {
    Provided,
    Generate,
    ProviderPrepared,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetNeed {
    pub asset_id: String,
    pub role: String,
    pub source_policy: AssetSourcePolicy,
    pub validation_requirements: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderJob {
    pub job_id: String,
    pub provider_kind: String,
    pub operation: String,
    pub input_refs: Vec<String>,
    pub optional: bool,
    pub runtime_forbidden: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequirement {
    pub evidence_id: String,
    pub gate_id: String,
    pub validator_id: String,
    pub artifact_kind: String,
    pub required: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanAssumption {
    pub assumption_id: String,
    pub field: String,
    pub value: String,
    pub confidence_basis_points: u16,
    pub source_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UnsupportedRequirement {
    pub field: String,
    pub reason: String,
    pub hard: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConstructionPlanDraft {
    pub plan_id: String,
    pub project_id: String,
    pub brief: String,
    pub design_constraints: Vec<String>,
    pub capability_needs: Vec<CapabilityNeed>,
    pub assets: Vec<AssetNeed>,
    pub provider_jobs: Vec<ProviderJob>,
    pub world_systems: Vec<String>,
    pub gameplay_kits: Vec<String>,
    pub assumptions: Vec<PlanAssumption>,
    pub evidence_requirements: Vec<EvidenceRequirement>,
    pub unsupported_requirements: Vec<UnsupportedRequirement>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedCapabilityStatus {
    Certified,
    Candidate,
    Experimental,
    Retired,
    Unavailable,
}

impl From<Option<CapabilityStatus>> for ResolvedCapabilityStatus {
    fn from(status: Option<CapabilityStatus>) -> Self {
        match status {
            Some(CapabilityStatus::Certified) => Self::Certified,
            Some(CapabilityStatus::Candidate) => Self::Candidate,
            Some(CapabilityStatus::Experimental) => Self::Experimental,
            Some(CapabilityStatus::Retired) => Self::Retired,
            None => Self::Unavailable,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResolvedCapability {
    pub capability_id: String,
    pub version: u32,
    pub required: bool,
    pub reason: String,
    pub status: ResolvedCapabilityStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConstructionReadiness {
    Ready,
    Partial,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConstructionPlan {
    pub schema_version: String,
    pub plan_id: String,
    pub project_id: String,
    pub brief: String,
    pub design_constraints: Vec<String>,
    pub resolved_capabilities: Vec<ResolvedCapability>,
    pub assets: Vec<AssetNeed>,
    pub provider_jobs: Vec<ProviderJob>,
    pub world_systems: Vec<String>,
    pub gameplay_kits: Vec<String>,
    pub style_plan_id: String,
    pub style_plan_sha256: String,
    pub assumptions: Vec<PlanAssumption>,
    pub evidence_requirements: Vec<EvidenceRequirement>,
    pub unsupported_requirements: Vec<UnsupportedRequirement>,
    pub readiness: ConstructionReadiness,
    pub blockers: Vec<String>,
    pub advisories: Vec<String>,
    pub capability_registry_id: String,
    pub capability_registry_sha256: String,
    pub validator_registry_sha256: String,
    pub plan_sha256: String,
}

impl ConstructionPlan {
    pub fn compile(draft: &ConstructionPlanDraft, style_plan: &StylePlan) -> Result<Self> {
        let catalog = CapabilityCatalog::native_v1();
        Self::compile_with_catalog(draft, style_plan, &catalog)
    }

    pub fn compile_with_catalog(
        draft: &ConstructionPlanDraft,
        style_plan: &StylePlan,
        catalog: &CapabilityCatalog,
    ) -> Result<Self> {
        catalog
            .validate()
            .map_err(|error| ConstructionPlanError(format!("capability catalog: {error}")))?;
        validate_draft(draft)?;
        if style_plan.registry_id != catalog.registry_id
            || style_plan.registry_sha256 != catalog.registry_sha256
        {
            return Err(ConstructionPlanError(
                "construction plan style binding uses a different capability registry".into(),
            ));
        }
        valid_digest(&style_plan.plan_sha256, "style_plan.plan_sha256")?;
        let validator_registry = ValidatorRegistry::wge_engine_neutral_v1();
        let mut resolved_capabilities = Vec::new();
        let mut blockers = Vec::new();
        let mut advisories = Vec::new();
        for need in &draft.capability_needs {
            let descriptor = catalog.descriptor(&need.capability_id);
            let resolved = ResolvedCapability {
                capability_id: need.capability_id.clone(),
                version: descriptor.map_or(0, |value| value.version),
                required: need.required,
                reason: need.reason.clone(),
                status: descriptor.map(|value| value.status).into(),
            };
            match resolved.status {
                ResolvedCapabilityStatus::Unavailable if need.required => blockers.push(format!(
                    "required capability {} is unavailable",
                    need.capability_id
                )),
                ResolvedCapabilityStatus::Retired if need.required => blockers.push(format!(
                    "required capability {} is retired",
                    need.capability_id
                )),
                ResolvedCapabilityStatus::Candidate | ResolvedCapabilityStatus::Experimental => {
                    advisories.push(format!(
                        "capability {} is not certified ({:?})",
                        need.capability_id, resolved.status
                    ));
                }
                _ => {}
            }
            resolved_capabilities.push(resolved);
        }
        for job in &draft.provider_jobs {
            if !job.runtime_forbidden {
                blockers.push(format!(
                    "provider job {} is not fenced from runtime",
                    job.job_id
                ));
            }
        }
        for requirement in &draft.evidence_requirements {
            let descriptor = validator_registry
                .descriptor(&requirement.validator_id)
                .ok_or_else(|| {
                    ConstructionPlanError(format!(
                        "evidence {} names unknown validator {}",
                        requirement.evidence_id, requirement.validator_id
                    ))
                })?;
            if descriptor.gate_id != requirement.gate_id {
                return Err(ConstructionPlanError(format!(
                    "evidence {} gate {} disagrees with validator {} gate {}",
                    requirement.evidence_id,
                    requirement.gate_id,
                    requirement.validator_id,
                    descriptor.gate_id
                )));
            }
        }
        for requirement in &draft.unsupported_requirements {
            if requirement.hard {
                blockers.push(format!(
                    "hard unsupported requirement {}: {}",
                    requirement.field, requirement.reason
                ));
            } else {
                advisories.push(format!(
                    "unsupported requirement {}: {}",
                    requirement.field, requirement.reason
                ));
            }
        }
        if style_plan.status != StylePlanStatus::Complete {
            advisories.push(format!("style plan is {:?}", style_plan.status));
        }
        let readiness = if !blockers.is_empty() {
            ConstructionReadiness::Blocked
        } else if !advisories.is_empty() {
            ConstructionReadiness::Partial
        } else {
            ConstructionReadiness::Ready
        };
        let mut plan = Self {
            schema_version: CONSTRUCTION_PLAN_SCHEMA.into(),
            plan_id: draft.plan_id.clone(),
            project_id: draft.project_id.clone(),
            brief: draft.brief.clone(),
            design_constraints: draft.design_constraints.clone(),
            resolved_capabilities,
            assets: draft.assets.clone(),
            provider_jobs: draft.provider_jobs.clone(),
            world_systems: draft.world_systems.clone(),
            gameplay_kits: draft.gameplay_kits.clone(),
            style_plan_id: style_plan.plan_id.clone(),
            style_plan_sha256: style_plan.plan_sha256.clone(),
            assumptions: draft.assumptions.clone(),
            evidence_requirements: draft.evidence_requirements.clone(),
            unsupported_requirements: draft.unsupported_requirements.clone(),
            readiness,
            blockers,
            advisories,
            capability_registry_id: catalog.registry_id.clone(),
            capability_registry_sha256: catalog.registry_sha256.clone(),
            validator_registry_sha256: validator_registry.digest().into(),
            plan_sha256: String::new(),
        };
        plan.plan_sha256 = plan.compute_digest()?;
        plan.validate_with_catalog(style_plan, catalog)?;
        Ok(plan)
    }

    pub fn validate(&self, style_plan: &StylePlan) -> Result<()> {
        let catalog = CapabilityCatalog::native_v1();
        self.validate_with_catalog(style_plan, &catalog)
    }

    pub fn validate_with_catalog(
        &self,
        style_plan: &StylePlan,
        catalog: &CapabilityCatalog,
    ) -> Result<()> {
        if self.schema_version != CONSTRUCTION_PLAN_SCHEMA {
            return Err(ConstructionPlanError(format!(
                "unsupported construction plan schema {}",
                self.schema_version
            )));
        }
        catalog
            .validate()
            .map_err(|error| ConstructionPlanError(format!("capability catalog: {error}")))?;
        if self.capability_registry_id != catalog.registry_id
            || self.capability_registry_sha256 != catalog.registry_sha256
            || self.style_plan_id != style_plan.plan_id
            || self.style_plan_sha256 != style_plan.plan_sha256
        {
            return Err(ConstructionPlanError(
                "construction plan is bound to stale style or capability identity".into(),
            ));
        }
        let validator_registry = ValidatorRegistry::wge_engine_neutral_v1();
        if self.validator_registry_sha256 != validator_registry.digest() {
            return Err(ConstructionPlanError(
                "construction plan is bound to a stale validator registry".into(),
            ));
        }
        for resolved in &self.resolved_capabilities {
            let Some(descriptor) = catalog.descriptor(&resolved.capability_id) else {
                if resolved.status == ResolvedCapabilityStatus::Unavailable && resolved.version == 0
                {
                    // A required unknown capability is intentionally retained
                    // as an explicit blocked resolution. This is useful plan
                    // evidence; it must not be silently dropped or guessed.
                    continue;
                }
                return Err(ConstructionPlanError(format!(
                    "construction plan resolves unknown capability {}",
                    resolved.capability_id
                )));
            };
            let expected_status = ResolvedCapabilityStatus::from(Some(descriptor.status));
            if resolved.version != descriptor.version || resolved.status != expected_status {
                return Err(ConstructionPlanError(format!(
                    "construction plan capability {} is stale (recorded v{} {:?}, catalog v{} {:?})",
                    resolved.capability_id,
                    resolved.version,
                    resolved.status,
                    descriptor.version,
                    expected_status
                )));
            }
        }
        valid_digest(&self.plan_sha256, "plan_sha256")?;
        if self.plan_sha256 != self.compute_digest()? {
            return Err(ConstructionPlanError(
                "construction plan digest does not match its typed content".into(),
            ));
        }
        validate_draft(&ConstructionPlanDraft {
            plan_id: self.plan_id.clone(),
            project_id: self.project_id.clone(),
            brief: self.brief.clone(),
            design_constraints: self.design_constraints.clone(),
            capability_needs: self
                .resolved_capabilities
                .iter()
                .map(|capability| CapabilityNeed {
                    capability_id: capability.capability_id.clone(),
                    required: capability.required,
                    reason: capability.reason.clone(),
                })
                .collect(),
            assets: self.assets.clone(),
            provider_jobs: self.provider_jobs.clone(),
            world_systems: self.world_systems.clone(),
            gameplay_kits: self.gameplay_kits.clone(),
            assumptions: self.assumptions.clone(),
            evidence_requirements: self.evidence_requirements.clone(),
            unsupported_requirements: self.unsupported_requirements.clone(),
        })?;
        Ok(())
    }

    pub fn compute_digest(&self) -> Result<String> {
        let mut value = serde_json::to_value(self).map_err(|error| {
            ConstructionPlanError(format!("cannot serialize construction plan: {error}"))
        })?;
        value
            .as_object_mut()
            .ok_or_else(|| ConstructionPlanError("construction plan is not an object".into()))?
            .remove("plan_sha256");
        Ok(sha256_prefixed(canonical_json(&value).as_bytes()))
    }
}

fn validate_draft(draft: &ConstructionPlanDraft) -> Result<()> {
    valid_id(&draft.plan_id, "plan_id")?;
    valid_id(&draft.project_id, "project_id")?;
    valid_text(&draft.brief, "brief")?;
    validate_unique_texts(&draft.design_constraints, "design constraint")?;
    validate_unique_texts(&draft.world_systems, "world system")?;
    validate_unique_texts(&draft.gameplay_kits, "gameplay kit")?;
    let mut capability_ids = BTreeSet::new();
    for need in &draft.capability_needs {
        valid_id(&need.capability_id, "capability_need.capability_id")?;
        valid_text(&need.reason, "capability_need.reason")?;
        if !capability_ids.insert(need.capability_id.as_str()) {
            return Err(ConstructionPlanError(format!(
                "duplicate capability need {}",
                need.capability_id
            )));
        }
    }
    let mut asset_ids = BTreeSet::new();
    for asset in &draft.assets {
        valid_id(&asset.asset_id, "asset_id")?;
        valid_text(&asset.role, "asset.role")?;
        if !asset_ids.insert(asset.asset_id.as_str()) {
            return Err(ConstructionPlanError(format!(
                "duplicate asset {}",
                asset.asset_id
            )));
        }
        validate_unique_texts(
            &asset.validation_requirements,
            "asset validation requirement",
        )?;
    }
    let mut job_ids = BTreeSet::new();
    for job in &draft.provider_jobs {
        valid_id(&job.job_id, "provider_job.job_id")?;
        valid_text(&job.provider_kind, "provider_job.provider_kind")?;
        valid_text(&job.operation, "provider_job.operation")?;
        if !job_ids.insert(job.job_id.as_str()) {
            return Err(ConstructionPlanError(format!(
                "duplicate provider job {}",
                job.job_id
            )));
        }
        validate_unique_texts(&job.input_refs, "provider input reference")?;
    }
    let mut assumption_ids = BTreeSet::new();
    for assumption in &draft.assumptions {
        valid_id(&assumption.assumption_id, "assumption_id")?;
        valid_text(&assumption.field, "assumption.field")?;
        valid_text(&assumption.value, "assumption.value")?;
        if assumption.confidence_basis_points > 10_000 {
            return Err(ConstructionPlanError(format!(
                "assumption {} has confidence above 10000 basis points",
                assumption.assumption_id
            )));
        }
        if !assumption_ids.insert(assumption.assumption_id.as_str()) {
            return Err(ConstructionPlanError(format!(
                "duplicate assumption {}",
                assumption.assumption_id
            )));
        }
        validate_unique_texts(&assumption.source_refs, "assumption source reference")?;
    }
    let mut evidence_ids = BTreeSet::new();
    for evidence in &draft.evidence_requirements {
        valid_id(&evidence.evidence_id, "evidence_id")?;
        valid_id(&evidence.gate_id, "evidence.gate_id")?;
        valid_id(&evidence.validator_id, "evidence.validator_id")?;
        valid_text(&evidence.artifact_kind, "evidence.artifact_kind")?;
        if !evidence_ids.insert(evidence.evidence_id.as_str()) {
            return Err(ConstructionPlanError(format!(
                "duplicate evidence requirement {}",
                evidence.evidence_id
            )));
        }
    }
    let mut unsupported_fields = BTreeSet::new();
    for unsupported in &draft.unsupported_requirements {
        valid_text(&unsupported.field, "unsupported.field")?;
        valid_text(&unsupported.reason, "unsupported.reason")?;
        if !unsupported_fields.insert(unsupported.field.as_str()) {
            return Err(ConstructionPlanError(format!(
                "duplicate unsupported requirement {}",
                unsupported.field
            )));
        }
    }
    Ok(())
}

fn validate_unique_texts(values: &[String], label: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        valid_text(value, label)?;
        if !seen.insert(value.as_str()) {
            return Err(ConstructionPlanError(format!("duplicate {label} {value}")));
        }
    }
    Ok(())
}

fn valid_id(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
    {
        return Err(ConstructionPlanError(format!(
            "{label} is not a safe identifier: {value:?}"
        )));
    }
    Ok(())
}

fn valid_text(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(ConstructionPlanError(format!("{label} cannot be empty")));
    }
    Ok(())
}

fn valid_digest(value: &str, label: &str) -> Result<()> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ConstructionPlanError(format!(
            "{label} is not a sha256 digest"
        )));
    }
    Ok(())
}
