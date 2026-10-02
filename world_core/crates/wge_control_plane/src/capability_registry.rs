//! Rust-owned, read-only capability catalog for the model-facing WGE surface.
//!
//! A capability is a semantic contract with explicit schemas, preconditions,
//! dependencies, executor boundaries, validators, failure modes, repair
//! options, and cost/quality metadata. It is not an executable plugin and a
//! backend implementation is never its identity.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wge_certification_authority::{canonical_json, sha256_prefixed};

pub const CAPABILITY_REGISTRY_SCHEMA: &str = "wge.capability-registry/v1";
pub const NATIVE_CAPABILITY_REGISTRY_ID: &str = "wge.capability-registry.native/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityRegistryError(pub String);

impl std::fmt::Display for CapabilityRegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CapabilityRegistryError {}

type Result<T> = std::result::Result<T, CapabilityRegistryError>;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityClass {
    World,
    Asset,
    Character,
    Graphics,
    Gameplay,
    Runtime,
    Verification,
    Repair,
    Style,
    Provider,
}

impl CapabilityClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Asset => "asset",
            Self::Character => "character",
            Self::Graphics => "graphics",
            Self::Gameplay => "gameplay",
            Self::Runtime => "runtime",
            Self::Verification => "verification",
            Self::Repair => "repair",
            Self::Style => "style",
            Self::Provider => "provider",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Experimental,
    Candidate,
    Certified,
    Retired,
}

impl CapabilityStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Experimental => "experimental",
            Self::Candidate => "candidate",
            Self::Certified => "certified",
            Self::Retired => "retired",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDependency {
    pub id: String,
    pub version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityExecutor {
    pub kind: String,
    pub entrypoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet_schema: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeterminismContract {
    pub mode: String,
    pub seed_fields: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityCostModel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityProvenance {
    pub source: String,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDescriptor {
    pub id: String,
    pub version: u32,
    pub class: CapabilityClass,
    pub status: CapabilityStatus,
    pub owner: String,
    pub intent_schema: String,
    pub output_schema: String,
    pub preconditions: Vec<String>,
    pub dependencies: Vec<CapabilityDependency>,
    pub executor: CapabilityExecutor,
    pub determinism: DeterminismContract,
    pub quality_axes: Vec<String>,
    pub cost_model: CapabilityCostModel,
    pub failure_modes: Vec<String>,
    pub validators: Vec<String>,
    pub repair_strategies: Vec<String>,
    pub provenance: CapabilityProvenance,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityCatalog {
    pub schema_version: String,
    pub registry_id: String,
    pub registry_sha256: String,
    pub capabilities: Vec<CapabilityDescriptor>,
}

impl CapabilityCatalog {
    pub fn new(
        registry_id: impl Into<String>,
        mut capabilities: Vec<CapabilityDescriptor>,
    ) -> Result<Self> {
        capabilities.sort_by(|left, right| left.id.cmp(&right.id));
        let mut catalog = Self {
            schema_version: CAPABILITY_REGISTRY_SCHEMA.to_owned(),
            registry_id: registry_id.into(),
            registry_sha256: String::new(),
            capabilities,
        };
        catalog.validate_structure()?;
        catalog.registry_sha256 = catalog.compute_digest()?;
        Ok(catalog)
    }

    /// The initial catalog is deliberately a catalog of existing machinery,
    /// not a promise that every listed capability is already production
    /// complete. Status and unsupported cases stay explicit.
    pub fn native_v1() -> Self {
        let capabilities = vec![
            descriptor(
                "world.terrain.fields",
                1,
                CapabilityClass::World,
                CapabilityStatus::Certified,
                "wge.world-intent/v1",
                "wge.terrain-fields/v1",
                &[],
                "rust_julia_packet",
                "terrain.fields.lower",
                Some("wge.terrain-fields/v1"),
                &[
                    "world_identity_available",
                    "heightfield_source_digest_pinned",
                ],
                &[
                    "terrain_shape",
                    "traversal_support",
                    "hydrology_consistency",
                ],
                &["stale_source_digest", "non_finite_field", "uphill_flow"],
                &["wge.validator.world-traversal/v1"],
                &["world.terrain.remeasure"],
                "WGE_NATIVE_CONVERGENCE_REPORT.md",
            ),
            descriptor(
                "world.navigation.traversal",
                1,
                CapabilityClass::World,
                CapabilityStatus::Certified,
                "wge.world-intent/v1",
                "wge.traversal-evidence/v1",
                &["world.terrain.fields"],
                "rust_native",
                "reference_runtime.traversal",
                None,
                &["terrain_identity_available", "spawn_and_encounter_contract"],
                &["traversal", "grounding", "restartability"],
                &["disconnected_route", "slope_violation", "stale_world"],
                &["wge.validator.world-traversal/v1"],
                &["world.navigation.repair_route"],
                "AUTHORITY_RECLAMATION_001.md",
            ),
            descriptor(
                "world.layout.lowering",
                1,
                CapabilityClass::World,
                CapabilityStatus::Candidate,
                "wge.layout-intent/v1",
                "wge.world-artifact/v1",
                &["world.terrain.fields", "world.navigation.traversal"],
                "rust_native",
                "project_ledger.compile_world",
                None,
                &["typed_layout", "terrain_fields_ready"],
                &["semantic_faithfulness", "determinism", "traversal"],
                &["unknown_region", "stale_julia_fields", "invalid_spawn"],
                &[
                    "wge.validator.semantic-spec/v1",
                    "wge.validator.world-traversal/v1",
                ],
                &["world.layout.repair"],
                "WGE_REPOSITORY_TOPOLOGY.md",
            ),
            descriptor(
                "asset.intake.validate",
                1,
                CapabilityClass::Asset,
                CapabilityStatus::Certified,
                "wge.asset-intent/v1",
                "wge.runtime-asset-package/v1",
                &[],
                "rust_native",
                "asset_contract.inspect_and_prepare",
                None,
                &["source_bytes_digest_pinned", "supported_container"],
                &[
                    "source_identity",
                    "geometry_integrity",
                    "material_integrity",
                ],
                &[
                    "malformed_container",
                    "unsupported_extension",
                    "stale_source",
                ],
                &["wge.validator.asset-preparation/v1"],
                &["asset.reimport_from_source"],
                "WGE_NATIVE_CAPABILITY_MATRIX.md",
            ),
            descriptor(
                "asset.negative-control.reject",
                1,
                CapabilityClass::Verification,
                CapabilityStatus::Certified,
                "wge.asset-control/v1",
                "wge.rejection-evidence/v1",
                &["asset.intake.validate"],
                "rust_native",
                "asset_contract.known_bad_control",
                None,
                &["known_bad_source_available"],
                &["fail_closed_behavior", "negative_control_integrity"],
                &["bad_control_accepted", "control_digest_changed"],
                &["wge.validator.asset-preparation/v1"],
                &["asset.restore_known_bad_control"],
                "AUTHORITY_RECLAMATION_001.md",
            ),
            descriptor(
                "gameplay.kit.compose",
                1,
                CapabilityClass::Gameplay,
                CapabilityStatus::Certified,
                "wge.gameplay-intent/v1",
                "wge.gameplay-kit/v1",
                &[],
                "rust_native",
                "gameplay_contract.resolve_kit",
                None,
                &["closed_capability_registry", "acyclic_dependencies"],
                &[
                    "mechanical_correctness",
                    "replay_determinism",
                    "resource_gating",
                ],
                &["unknown_ability", "dependency_cycle", "silent_effect"],
                &["wge.validator.gameplay-replay/v1"],
                &["gameplay.kit.repair"],
                "docs/WGE_gameplay_kit_architecture.md",
            ),
            descriptor(
                "graphics.scene.packet",
                1,
                CapabilityClass::Graphics,
                CapabilityStatus::Candidate,
                "wge.scene-intent/v1",
                "wge.graphics-scene-packet/v6",
                &["asset.intake.validate", "world.layout.lowering"],
                "julia_lava_packet",
                "native_graphics.lower_packet",
                Some("wge.graphics-scene-packet/v6"),
                &["world_artifact_valid", "asset_identity_available"],
                &[
                    "material_separation",
                    "grounding",
                    "frame_cost",
                    "artifact_rate",
                ],
                &[
                    "packet_identity_mismatch",
                    "unsupported_material",
                    "worker_failure",
                ],
                &[
                    "wge.validator.visual-reference/v1",
                    "wge.validator.visual-quality/v1",
                ],
                &[
                    "graphics.scene.reduce_budget",
                    "graphics.scene.rebuild_packet",
                ],
                "WGE_NATIVE_GRAPHICS_ARCHITECTURE.md",
            ),
            descriptor(
                "graphics.authored-frame.campaign2",
                1,
                CapabilityClass::Graphics,
                CapabilityStatus::Candidate,
                "wge.authored-frame-intent/v1",
                "wge.graphics-scene-packet/v6",
                &["graphics.scene.packet"],
                "julia_lava_packet",
                "native_graphics.campaign2_projection",
                Some("wge.graphics-scene-packet/v6"),
                &["fixed_camera", "bounded_scene_budget"],
                &["composition", "material_separation", "atmospheric_depth"],
                &["flat_surface", "camera_out_of_bounds", "budget_exceeded"],
                &["wge.validator.visual-quality/v1"],
                &["graphics.authored-frame.recompose"],
                "WGE_GRAPHICS_CAMPAIGN_2_REPORT.md",
            ),
            descriptor(
                "graphics.capture.reference",
                1,
                CapabilityClass::Verification,
                CapabilityStatus::Certified,
                "wge.capture-intent/v1",
                "wge.reference-capture/v1",
                &["graphics.scene.packet"],
                "julia_lava_packet",
                "native_graphics.capture_reference",
                Some("wge.native-rgba8-capture/v1"),
                &["fixed_camera", "capture_dimensions_declared"],
                &["provenance", "determinism", "camera_binding"],
                &["truncated_capture", "stale_packet", "camera_mismatch"],
                &["wge.validator.visual-reference/v1"],
                &["graphics.capture.rebind_camera"],
                "WGE_NATIVE_GRAPHICS_BENCHMARK.md",
            ),
            descriptor(
                "graphics.visual-quality.evidence",
                1,
                CapabilityClass::Verification,
                CapabilityStatus::Candidate,
                "wge.visual-quality-intent/v1",
                "wge.native-visual-quality-evidence/v1",
                &["graphics.capture.reference"],
                "rust_native",
                "certification_authority.measure_visual_quality",
                None,
                &["capture_bytes_bound", "registered_thresholds"],
                &[
                    "silhouette_readability",
                    "material_separation",
                    "grounding_contact",
                    "lighting_consistency",
                    "atmospheric_depth",
                    "texture_frequency",
                    "composition",
                    "density",
                    "artifact_rate",
                    "frame_cost",
                    "memory_cost",
                ],
                &[
                    "flat_capture",
                    "overlay_heavy_capture",
                    "indeterminate_axis",
                ],
                &["wge.validator.visual-quality/v1"],
                &["graphics.visual-quality.diagnose"],
                "WGE_NATIVE_QUALITY_GAPS.md",
            ),
            descriptor(
                "runtime.reference.deterministic",
                1,
                CapabilityClass::Runtime,
                CapabilityStatus::Certified,
                "wge.runtime-intent/v1",
                "wge.reference-playthrough/v1",
                &["world.layout.lowering", "gameplay.kit.compose"],
                "rust_native",
                "reference_runtime.fixed_step",
                None,
                &[
                    "validated_world",
                    "validated_gameplay_kit",
                    "fixed_tick_schedule",
                ],
                &["replay_determinism", "collision", "objective_completion"],
                &["stale_tick", "collision_penetration", "trace_divergence"],
                &[
                    "wge.validator.gameplay-replay/v1",
                    "wge.validator.world-traversal/v1",
                ],
                &["runtime.replay_from_checkpoint"],
                "WGE_NATIVE_CONVERGENCE_REPORT.md",
            ),
            descriptor(
                "repair.evidence-bounded",
                1,
                CapabilityClass::Repair,
                CapabilityStatus::Certified,
                "wge.repair-intent/v1",
                "wge.repair-receipt/v1",
                &[
                    "graphics.visual-quality.evidence",
                    "runtime.reference.deterministic",
                ],
                "rust_native",
                "control_plane.bounded_repair",
                None,
                &["failed_receipt", "authorized_artifact_scope"],
                &["before_after_improvement", "scope_integrity", "replay"],
                &["unauthorized_edit", "unchanged_candidate", "stale_receipt"],
                &["wge.validator.evidence-repair/v1"],
                &["repair.propose_bounded_delta"],
                "AUTHORITY_RECLAMATION_001.md",
            ),
            descriptor(
                "style.profile.compile",
                1,
                CapabilityClass::Style,
                CapabilityStatus::Experimental,
                "wge.style-profile/v1",
                "wge.style-plan/v1",
                &["graphics.scene.packet"],
                "rust_native",
                "style_profile.compile",
                None,
                &[
                    "typed_observations",
                    "explicit_conflicts",
                    "budget_constraints",
                ],
                &["style_coherence", "material_separation", "composition"],
                &["unsupported_axis", "unresolved_conflict", "budget_exceeded"],
                &["wge.validator.semantic-spec/v1"],
                &["style.profile.resolve_conflict"],
                "WGE_STYLE_PROFILE_CONTRACT.md",
            ),
            descriptor(
                "project.construction-plan",
                1,
                CapabilityClass::Runtime,
                CapabilityStatus::Candidate,
                "wge.construction-intent/v1",
                "wge.construction-plan/v1",
                &[
                    "world.layout.lowering",
                    "asset.intake.validate",
                    "gameplay.kit.compose",
                    "style.profile.compile",
                ],
                "rust_native",
                "control_plane.construction_plan.compile",
                None,
                &[
                    "typed_brief_available",
                    "style_plan_bound",
                    "validator_registry_available",
                ],
                &[
                    "plan_completeness",
                    "requirement_localization",
                    "determinism",
                ],
                &[
                    "unknown_capability",
                    "stale_style_plan",
                    "runtime_provider_leak",
                ],
                &["wge.validator.semantic-spec/v1"],
                &["project.plan.reconcile"],
                "WGE_CONSTRUCTION_PLAN_CONTRACT.md",
            ),
            descriptor(
                "provider.character.blender-boundary",
                1,
                CapabilityClass::Provider,
                CapabilityStatus::Candidate,
                "wge.character-intent/v1",
                "wge.runtime-asset-package/v1",
                &["asset.intake.validate"],
                "bounded_provider_job",
                "provider.blender.prepare_character",
                None,
                &["provider_output_digest_pinned", "runtime_import_validation"],
                &["source_identity", "rig_integrity", "animation_integrity"],
                &["provider_timeout", "missing_skinning", "bad_joint_index"],
                &["wge.validator.asset-preparation/v1"],
                &["provider.character.reimport"],
                "WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md",
            ),
        ];
        Self::new(NATIVE_CAPABILITY_REGISTRY_ID, capabilities)
            .expect("the checked-in native capability catalog must be valid")
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_structure()?;
        let expected = self.compute_digest()?;
        if self.registry_sha256 != expected {
            return Err(CapabilityRegistryError(
                "capability registry digest does not match its descriptors".into(),
            ));
        }
        Ok(())
    }

    pub fn descriptor(&self, id: &str) -> Option<&CapabilityDescriptor> {
        self.capabilities
            .iter()
            .find(|capability| capability.id == id)
    }

    pub fn list(
        &self,
        class: Option<CapabilityClass>,
        status: Option<CapabilityStatus>,
    ) -> Vec<CapabilityDescriptor> {
        self.capabilities
            .iter()
            .filter(|capability| class.is_none_or(|value| capability.class == value))
            .filter(|capability| status.is_none_or(|value| capability.status == value))
            .cloned()
            .collect()
    }

    pub fn compute_digest(&self) -> Result<String> {
        let mut value = serde_json::to_value(self).map_err(|error| {
            CapabilityRegistryError(format!("cannot serialize catalog: {error}"))
        })?;
        value
            .as_object_mut()
            .ok_or_else(|| CapabilityRegistryError("catalog is not a JSON object".into()))?
            .remove("registry_sha256");
        Ok(sha256_prefixed(canonical_json(&value).as_bytes()))
    }

    fn validate_structure(&self) -> Result<()> {
        if self.schema_version != CAPABILITY_REGISTRY_SCHEMA {
            return Err(CapabilityRegistryError(format!(
                "unsupported capability registry schema {}",
                self.schema_version
            )));
        }
        valid_identifier(&self.registry_id, "registry_id")?;
        if self.capabilities.is_empty() {
            return Err(CapabilityRegistryError(
                "capability registry cannot be empty".into(),
            ));
        }
        let mut ids = BTreeSet::new();
        for capability in &self.capabilities {
            valid_identifier(&capability.id, "capability id")?;
            if capability.version == 0 {
                return Err(CapabilityRegistryError(format!(
                    "capability {} has an invalid zero version",
                    capability.id
                )));
            }
            if !ids.insert(capability.id.as_str()) {
                return Err(CapabilityRegistryError(format!(
                    "duplicate capability id {}",
                    capability.id
                )));
            }
            valid_text(&capability.owner, "owner")?;
            valid_text(&capability.intent_schema, "intent_schema")?;
            valid_text(&capability.output_schema, "output_schema")?;
            valid_text(&capability.executor.kind, "executor.kind")?;
            valid_text(&capability.executor.entrypoint, "executor.entrypoint")?;
            valid_text(&capability.determinism.mode, "determinism.mode")?;
            if capability.validators.is_empty() {
                return Err(CapabilityRegistryError(format!(
                    "capability {} has no registered validator",
                    capability.id
                )));
            }
            for dependency in &capability.dependencies {
                valid_identifier(&dependency.id, "dependency id")?;
                if dependency.id == capability.id {
                    return Err(CapabilityRegistryError(format!(
                        "capability {} depends on itself",
                        capability.id
                    )));
                }
                if dependency.version == 0 {
                    return Err(CapabilityRegistryError(format!(
                        "capability {} has a zero-version dependency {}",
                        capability.id, dependency.id
                    )));
                }
            }
        }
        for capability in &self.capabilities {
            for dependency in &capability.dependencies {
                let Some(target) = self.descriptor(&dependency.id) else {
                    return Err(CapabilityRegistryError(format!(
                        "capability {} depends on unknown capability {}",
                        capability.id, dependency.id
                    )));
                };
                if target.version != dependency.version {
                    return Err(CapabilityRegistryError(format!(
                        "capability {} requires {} version {}, catalog has version {}",
                        capability.id, dependency.id, dependency.version, target.version
                    )));
                }
            }
        }
        Ok(())
    }
}

fn valid_identifier(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
    {
        return Err(CapabilityRegistryError(format!(
            "{label} is not a safe identifier: {value:?}"
        )));
    }
    Ok(())
}

fn valid_text(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(CapabilityRegistryError(format!("{label} cannot be empty")));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn descriptor(
    id: &str,
    version: u32,
    class: CapabilityClass,
    status: CapabilityStatus,
    intent_schema: &str,
    output_schema: &str,
    dependencies: &[&str],
    executor_kind: &str,
    entrypoint: &str,
    packet_schema: Option<&str>,
    preconditions: &[&str],
    quality_axes: &[&str],
    failure_modes: &[&str],
    validators: &[&str],
    repair_strategies: &[&str],
    provenance_source: &str,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: format!("{id}/v{version}"),
        version,
        class,
        status,
        owner: "rust-authority".into(),
        intent_schema: intent_schema.into(),
        output_schema: output_schema.into(),
        preconditions: preconditions.iter().map(|value| (*value).into()).collect(),
        dependencies: dependencies
            .iter()
            .map(|dependency| CapabilityDependency {
                id: format!("{dependency}/v{version}"),
                version,
            })
            .collect(),
        executor: CapabilityExecutor {
            kind: executor_kind.into(),
            entrypoint: entrypoint.into(),
            packet_schema: packet_schema.map(Into::into),
        },
        determinism: DeterminismContract {
            mode: "deterministic_when_inputs_and_seed_are_bound".into(),
            seed_fields: vec!["project_seed".into(), "input_identities".into()],
        },
        quality_axes: quality_axes.iter().map(|value| (*value).into()).collect(),
        cost_model: CapabilityCostModel {
            cpu_us: None,
            gpu_us: None,
            memory_bytes: None,
        },
        failure_modes: failure_modes.iter().map(|value| (*value).into()).collect(),
        validators: validators.iter().map(|value| (*value).into()).collect(),
        repair_strategies: repair_strategies
            .iter()
            .map(|value| format!("{value}/v1"))
            .collect(),
        provenance: CapabilityProvenance {
            source: provenance_source.into(),
            evidence: vec!["wge.native-capability-catalog/v1".into()],
        },
    }
}

pub fn class_from_str(value: &str) -> Option<CapabilityClass> {
    Some(match value {
        "world" => CapabilityClass::World,
        "asset" => CapabilityClass::Asset,
        "character" => CapabilityClass::Character,
        "graphics" => CapabilityClass::Graphics,
        "gameplay" => CapabilityClass::Gameplay,
        "runtime" => CapabilityClass::Runtime,
        "verification" => CapabilityClass::Verification,
        "repair" => CapabilityClass::Repair,
        "style" => CapabilityClass::Style,
        "provider" => CapabilityClass::Provider,
        _ => return None,
    })
}

pub fn status_from_str(value: &str) -> Option<CapabilityStatus> {
    Some(match value {
        "experimental" => CapabilityStatus::Experimental,
        "candidate" => CapabilityStatus::Candidate,
        "certified" => CapabilityStatus::Certified,
        "retired" => CapabilityStatus::Retired,
        _ => return None,
    })
}

pub fn descriptor_json(catalog: &CapabilityCatalog, id: &str) -> Result<Value> {
    let descriptor = catalog
        .descriptor(id)
        .ok_or_else(|| CapabilityRegistryError(format!("unknown capability {id}")))?;
    let mut value = serde_json::to_value(descriptor).map_err(|error| {
        CapabilityRegistryError(format!("cannot serialize descriptor: {error}"))
    })?;
    value
        .as_object_mut()
        .ok_or_else(|| CapabilityRegistryError("descriptor is not a JSON object".into()))?
        .extend([
            (
                "registry_id".into(),
                Value::String(catalog.registry_id.clone()),
            ),
            (
                "registry_sha256".into(),
                Value::String(catalog.registry_sha256.clone()),
            ),
        ]);
    Ok(value)
}
