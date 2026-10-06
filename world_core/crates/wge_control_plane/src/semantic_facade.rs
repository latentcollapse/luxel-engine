//! Rust-owned discovery catalog for the model-facing semantic facade.
//!
//! The facade catalog is intentionally descriptive. It tells a model which
//! semantic verbs exist, which are partial or planned, and what a legal next
//! step looks like. It does not turn planned verbs into executable commands.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wge_certification_authority::{canonical_json, sha256_prefixed};

use crate::capability_registry::CapabilityCatalog;

pub const SEMANTIC_FACADE_SCHEMA: &str = "wge.semantic-facade/v1";
pub const NATIVE_SEMANTIC_FACADE_ID: &str = "wge.semantic-facade.native/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticFacadeError(pub String);

impl std::fmt::Display for SemanticFacadeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SemanticFacadeError {}

type Result<T> = std::result::Result<T, SemanticFacadeError>;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FacadeStatus {
    Available,
    Partial,
    Planned,
    Deferred,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FacadeOperationDescriptor {
    pub id: String,
    pub version: u32,
    pub status: FacadeStatus,
    pub purpose: String,
    pub input_schema: String,
    pub output_schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_operation: Option<String>,
    pub capability_ids: Vec<String>,
    pub legal_next_steps: Vec<String>,
    pub failure_modes: Vec<String>,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticFacadeCatalog {
    pub schema_version: String,
    pub facade_id: String,
    pub operations: Vec<FacadeOperationDescriptor>,
    pub facade_sha256: String,
}

impl SemanticFacadeCatalog {
    pub fn native_v1() -> Self {
        let operations = vec![
            operation(
                "project.intake",
                FacadeStatus::Planned,
                "Interpret a brief, concept references, constraints, observations, and provenance into a typed project intake.",
                "wge.project-intake-request/v1",
                "wge.project-intake/v1",
                None,
                &[],
                &["project.plan/v1"],
                &[
                    "ambiguous_reference",
                    "conflicting_constraint",
                    "missing_provenance",
                ],
                "docs/content-sdk/generality-bridge.md",
            ),
            operation(
                "project.plan",
                FacadeStatus::Partial,
                "Resolve a typed construction draft against current capabilities, providers, validators, and style policy.",
                "wge.construction-intent/v1",
                "wge.construction-plan/v1",
                Some("project_plan"),
                &["project.construction-plan/v1"],
                &["construction_validate/v1", "world.construct/v1"],
                &[
                    "unknown_capability",
                    "stale_style_plan",
                    "runtime_provider_leak",
                ],
                "docs/content-sdk/construction-plan-contract.md",
            ),
            operation(
                "capability.list",
                FacadeStatus::Available,
                "List registered capabilities with schemas, status, dependencies, validators, cost, and repair metadata.",
                "wge.capability-query/v1",
                "wge.capability-registry/v1",
                Some("capability_list"),
                &[],
                &["capability.explain/v1", "capability.plan/v1"],
                &["unknown_class", "unknown_status", "registry_tamper"],
                "docs/platform/capability-registry-design.md",
            ),
            operation(
                "capability.explain",
                FacadeStatus::Available,
                "Explain one registered capability without exposing backend implementation details as its identity.",
                "wge.capability-id/v1",
                "wge.capability-descriptor/v1",
                Some("capability_explain"),
                &[],
                &["capability.plan/v1"],
                &["unknown_capability", "registry_tamper"],
                "docs/platform/capability-registry-design.md",
            ),
            operation(
                "capability.plan",
                FacadeStatus::Planned,
                "Select a dependency-closed capability set for a declared construction requirement.",
                "wge.capability-plan-request/v1",
                "wge.capability-plan/v1",
                None,
                &["project.construction-plan/v1"],
                &["style.compile/v1", "project.plan/v1"],
                &[
                    "dependency_cycle",
                    "missing_validator",
                    "unsupported_requirement",
                ],
                "docs/platform/capability-registry-design.md",
            ),
            operation(
                "style.compile",
                FacadeStatus::Partial,
                "Validate a provenance-bound StyleProfile and lower it into a backend-neutral StylePlan with explicit unsupported axes.",
                "wge.style-profile/v1",
                "wge.style-plan/v1",
                Some("style_compile"),
                &["style.profile.compile/v1"],
                &["project.plan/v1", "quality.inspect/v1"],
                &["unsupported_axis", "unresolved_conflict", "budget_exceeded"],
                "docs/world/style-profile-contract.md",
            ),
            operation(
                "world.construct",
                FacadeStatus::Planned,
                "Lower an approved world intent into a Rust-owned world artifact with Julia spatial fields, collision, navigation, and traversal semantics.",
                "wge.world-construction-request/v1",
                "wge.world-artifact/v1",
                None,
                &["world.terrain.fields/v1", "world.navigation.traversal/v1"],
                &["scene.compose/v1", "runtime.launch/v1"],
                &[
                    "stale_spatial_fields",
                    "invalid_spawn",
                    "disconnected_route",
                ],
                "docs/archive/2026-09_native-graphics-checkpoints/native-convergence-report.md",
            ),
            operation(
                "scene.compose",
                FacadeStatus::Planned,
                "Bind validated assets to semantic SceneObjects, transforms, roles, collision, materials, LOD, visibility, and provenance.",
                "wge.scene-construction-request/v1",
                "wge.scene-artifact/v1",
                None,
                &[],
                &["asset.prepare/v1", "runtime.launch/v1"],
                &[
                    "unknown_asset",
                    "identity_mismatch",
                    "collision_policy_missing",
                ],
                "docs/archive/2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md",
            ),
            operation(
                "asset.prepare",
                FacadeStatus::Partial,
                "Condition a source asset into a validated runtime package while retaining source identity and negative controls.",
                "wge.asset-intent/v1",
                "wge.runtime-asset-package/v1",
                None,
                &["asset.intake.validate/v1"],
                &["scene.compose/v1", "character.prepare/v1"],
                &[
                    "malformed_container",
                    "unsupported_extension",
                    "missing_collision",
                ],
                "docs/platform/native-capability-matrix.md",
            ),
            operation(
                "character.prepare",
                FacadeStatus::Planned,
                "Prepare a bounded character asset through an optional provider boundary, then validate rig, skinning, animation, and sockets natively.",
                "wge.character-intent/v1",
                "wge.runtime-asset-package/v1",
                None,
                &["provider.character.blender-boundary/v1"],
                &["scene.compose/v1", "runtime.launch/v1"],
                &["missing_skinning", "bad_joint_index", "provider_timeout"],
                "docs/archive/2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md",
            ),
            operation(
                "gameplay.compose",
                FacadeStatus::Planned,
                "Compose a bounded gameplay kit, objective, encounter, ability/effect set, and deterministic runtime binding.",
                "wge.gameplay-intent/v1",
                "wge.gameplay-kit/v1",
                None,
                &["gameplay.kit.compose/v1"],
                &["runtime.launch/v1", "project.verify/v1"],
                &["unknown_ability", "dependency_cycle", "silent_effect"],
                "docs/gameplay/gameplay-kit-architecture.md",
            ),
            operation(
                "runtime.launch",
                FacadeStatus::Planned,
                "Launch a native WGE session with fixed-step simulation, input, collision, gameplay, present, restart, and telemetry.",
                "wge.runtime-launch-request/v1",
                "wge.native-game-session/v1",
                None,
                &["runtime.reference.deterministic/v1"],
                &["quality.inspect/v1", "project.verify/v1"],
                &[
                    "stale_snapshot",
                    "input_unavailable",
                    "frame_budget_exceeded",
                ],
                "docs/archive/2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md",
            ),
            operation(
                "quality.inspect",
                FacadeStatus::Partial,
                "Collect semantic, mechanical, visual, runtime, cost, and memory evidence without collapsing quality into one scalar.",
                "wge.quality-inspection-request/v1",
                "wge.quality-evidence/v1",
                None,
                &[
                    "graphics.visual-quality.evidence/v1",
                    "graphics.capture.reference/v1",
                ],
                &["repair.propose/v1", "project.verify/v1"],
                &["indeterminate_axis", "stale_capture", "artifact_rate"],
                "docs/world/native-quality-gaps.md",
            ),
            operation(
                "repair.propose",
                FacadeStatus::Partial,
                "Diagnose the failed semantic layer and emit a bounded, authorized repair proposal tied to before/after evidence.",
                "wge.repair-request/v1",
                "wge.repair-proposal-request/v1",
                Some("propose_repair"),
                &["repair.evidence-bounded/v1"],
                &["repair.apply/v1", "project.verify/v1"],
                &["stale_receipt", "unauthorized_edit", "unchanged_candidate"],
                "docs/archive/2026-09_roadmaps-and-audits/authority-reclamation-001.md",
            ),
            operation(
                "repair.apply",
                FacadeStatus::Partial,
                "Submit only an authorized bounded repair result for independent Rust validation and rebuild.",
                "wge.repair-work-result/v1",
                "wge.repair-receipt/v1",
                Some("apply_repair"),
                &["repair.evidence-bounded/v1"],
                &["quality.inspect/v1", "project.verify/v1"],
                &["scope_violation", "failed_remeasure", "stale_candidate"],
                "docs/archive/2026-09_roadmaps-and-audits/authority-reclamation-001.md",
            ),
            operation(
                "project.verify",
                FacadeStatus::Partial,
                "Run the registered authority validators and expose a promotion-ready or repair-required result.",
                "wge.project-verification-request/v1",
                "wge.certification-report/v1",
                Some("verify_candidate"),
                &[],
                &["project.package/v1", "repair.propose/v1"],
                &["failed_gate", "stale_evidence", "status_only_evidence"],
                "docs/archive/2026-09_native-graphics-checkpoints/native-convergence-report.md",
            ),
            operation(
                "project.package",
                FacadeStatus::Planned,
                "Produce a deterministic, inspectable, runnable WGE snapshot with provenance and a native handoff.",
                "wge.project-package-request/v1",
                "wge.runnable-snapshot/v1",
                None,
                &[],
                &[],
                &[
                    "uncertified_snapshot",
                    "runtime_dependency_leak",
                    "missing_provenance",
                ],
                "docs/archive/2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md",
            ),
        ];
        let mut catalog = Self {
            schema_version: SEMANTIC_FACADE_SCHEMA.into(),
            facade_id: NATIVE_SEMANTIC_FACADE_ID.into(),
            operations,
            facade_sha256: String::new(),
        };
        catalog.facade_sha256 = catalog
            .compute_digest()
            .expect("native facade is serializable");
        catalog
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SEMANTIC_FACADE_SCHEMA {
            return Err(SemanticFacadeError(format!(
                "unsupported semantic facade schema {}",
                self.schema_version
            )));
        }
        if self.facade_id != NATIVE_SEMANTIC_FACADE_ID {
            return Err(SemanticFacadeError(format!(
                "unknown semantic facade id {}",
                self.facade_id
            )));
        }
        let capabilities = CapabilityCatalog::native_v1();
        capabilities
            .validate()
            .map_err(|error| SemanticFacadeError(format!("capability catalog: {error}")))?;
        let mut ids = BTreeSet::new();
        for operation in &self.operations {
            valid_id(&operation.id, "operation id")?;
            if operation.version == 0 {
                return Err(SemanticFacadeError(format!(
                    "operation {} has zero version",
                    operation.id
                )));
            }
            valid_text(&operation.purpose, "operation purpose")?;
            valid_text(&operation.input_schema, "operation input schema")?;
            valid_text(&operation.output_schema, "operation output schema")?;
            valid_text(&operation.source, "operation source")?;
            if !ids.insert(operation.id.as_str()) {
                return Err(SemanticFacadeError(format!(
                    "duplicate facade operation {}",
                    operation.id
                )));
            }
            if let Some(transport) = &operation.transport_operation {
                valid_id(transport, "transport operation")?;
                if matches!(
                    operation.status,
                    FacadeStatus::Planned | FacadeStatus::Deferred
                ) {
                    return Err(SemanticFacadeError(format!(
                        "planned/deferred operation {} cannot expose transport {}",
                        operation.id, transport
                    )));
                }
            } else if operation.status == FacadeStatus::Available {
                return Err(SemanticFacadeError(format!(
                    "available operation {} has no transport mapping",
                    operation.id
                )));
            }
            validate_unique(&operation.capability_ids, "capability id")?;
            for capability in &operation.capability_ids {
                if capabilities.descriptor(capability).is_none() {
                    return Err(SemanticFacadeError(format!(
                        "operation {} names unknown capability {}",
                        operation.id, capability
                    )));
                }
            }
            validate_unique(&operation.legal_next_steps, "legal next step")?;
            validate_unique(&operation.failure_modes, "failure mode")?;
        }
        valid_digest(&self.facade_sha256, "facade_sha256")?;
        if self.facade_sha256 != self.compute_digest()? {
            return Err(SemanticFacadeError(
                "semantic facade digest does not match its descriptors".into(),
            ));
        }
        Ok(())
    }

    pub fn compute_digest(&self) -> Result<String> {
        let mut value = serde_json::to_value(self)
            .map_err(|error| SemanticFacadeError(format!("cannot serialize facade: {error}")))?;
        value
            .as_object_mut()
            .ok_or_else(|| SemanticFacadeError("semantic facade is not an object".into()))?
            .remove("facade_sha256");
        Ok(sha256_prefixed(canonical_json(&value).as_bytes()))
    }

    pub fn list(&self, status: Option<FacadeStatus>) -> Vec<&FacadeOperationDescriptor> {
        self.operations
            .iter()
            .filter(|operation| status.is_none_or(|value| operation.status == value))
            .collect()
    }

    pub fn explain(&self, id: &str) -> Result<Value> {
        let operation = self
            .operations
            .iter()
            .find(|operation| operation.id == id)
            .ok_or_else(|| SemanticFacadeError(format!("unknown facade operation {id}")))?;
        serde_json::to_value(operation).map_err(|error| {
            SemanticFacadeError(format!("cannot serialize facade operation: {error}"))
        })
    }
}

fn operation(
    id: &str,
    status: FacadeStatus,
    purpose: &str,
    input_schema: &str,
    output_schema: &str,
    transport_operation: Option<&str>,
    capability_ids: &[&str],
    legal_next_steps: &[&str],
    failure_modes: &[&str],
    source: &str,
) -> FacadeOperationDescriptor {
    FacadeOperationDescriptor {
        id: format!("{id}/v1"),
        version: 1,
        status,
        purpose: purpose.into(),
        input_schema: input_schema.into(),
        output_schema: output_schema.into(),
        transport_operation: transport_operation.map(Into::into),
        capability_ids: capability_ids.iter().map(|value| (*value).into()).collect(),
        legal_next_steps: legal_next_steps
            .iter()
            .map(|value| (*value).into())
            .collect(),
        failure_modes: failure_modes.iter().map(|value| (*value).into()).collect(),
        source: source.into(),
    }
}

fn validate_unique(values: &[String], label: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        valid_text(value, label)?;
        if !seen.insert(value.as_str()) {
            return Err(SemanticFacadeError(format!("duplicate {label} {value}")));
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
        return Err(SemanticFacadeError(format!(
            "{label} is not a safe identifier: {value:?}"
        )));
    }
    Ok(())
}

fn valid_text(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(SemanticFacadeError(format!("{label} cannot be empty")));
    }
    Ok(())
}

fn valid_digest(value: &str, label: &str) -> Result<()> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(SemanticFacadeError(format!(
            "{label} is not a sha256 digest"
        )));
    }
    Ok(())
}
