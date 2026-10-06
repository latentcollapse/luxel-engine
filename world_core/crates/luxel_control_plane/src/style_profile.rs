//! Typed, backend-neutral visual style intent and lowering.
//!
//! `StyleProfile` is semantic input. `StylePlan` is an authority-validated
//! lowering over registered capabilities. Neither contains shader source,
//! GPU handles, provider paths, or renderer-owned canonical state.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use luxel_certification_authority::{canonical_json, sha256_prefixed};

use crate::capability_registry::{CapabilityCatalog, CapabilityStatus};

pub const STYLE_PROFILE_SCHEMA: &str = "luxel.style-profile/v1";
pub const STYLE_PLAN_SCHEMA: &str = "luxel.style-plan/v1";
const CONFIDENCE_MAX: u16 = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleContractError(pub String);

impl std::fmt::Display for StyleContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for StyleContractError {}

type Result<T> = std::result::Result<T, StyleContractError>;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StyleSourceKind {
    Observed,
    Inferred,
    Assumed,
    Requested,
    Repaired,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StyleSignal {
    Scalar {
        basis_points: u16,
        source_kind: StyleSourceKind,
        source_refs: Vec<String>,
        confidence_basis_points: u16,
    },
    Category {
        value: String,
        source_kind: StyleSourceKind,
        source_refs: Vec<String>,
        confidence_basis_points: u16,
    },
    Boolean {
        value: bool,
        source_kind: StyleSourceKind,
        source_refs: Vec<String>,
        confidence_basis_points: u16,
    },
}

impl StyleSignal {
    pub fn confidence_basis_points(&self) -> u16 {
        match self {
            Self::Scalar {
                confidence_basis_points,
                ..
            }
            | Self::Category {
                confidence_basis_points,
                ..
            }
            | Self::Boolean {
                confidence_basis_points,
                ..
            } => *confidence_basis_points,
        }
    }

    pub fn source_kind(&self) -> StyleSourceKind {
        match self {
            Self::Scalar { source_kind, .. }
            | Self::Category { source_kind, .. }
            | Self::Boolean { source_kind, .. } => *source_kind,
        }
    }

    pub fn source_refs(&self) -> &[String] {
        match self {
            Self::Scalar { source_refs, .. }
            | Self::Category { source_refs, .. }
            | Self::Boolean { source_refs, .. } => source_refs,
        }
    }

    fn validate(&self, field: &str, known_refs: &BTreeSet<String>) -> Result<()> {
        if self.confidence_basis_points() > CONFIDENCE_MAX {
            return Err(StyleContractError(format!(
                "style signal {field} has confidence above 10000 basis points"
            )));
        }
        if matches!(self, Self::Category { value, .. } if value.trim().is_empty()) {
            return Err(StyleContractError(format!(
                "style category {field} cannot be empty"
            )));
        }
        if matches!(self, Self::Scalar { basis_points, .. } if *basis_points > CONFIDENCE_MAX) {
            return Err(StyleContractError(format!(
                "style scalar {field} is outside the [0, 10000] domain"
            )));
        }
        if matches!(
            self.source_kind(),
            StyleSourceKind::Observed | StyleSourceKind::Inferred
        ) && self.source_refs().is_empty()
        {
            return Err(StyleContractError(format!(
                "observed/inferred style signal {field} needs source_refs"
            )));
        }
        for source_ref in self.source_refs() {
            valid_text(source_ref, "style source reference")?;
            if !known_refs.is_empty() && !known_refs.contains(source_ref) {
                return Err(StyleContractError(format!(
                    "style signal {field} references unknown source {source_ref}"
                )));
            }
        }
        Ok(())
    }

    fn is_explicitly_unavailable(&self) -> bool {
        matches!(self, Self::Category { value, .. } if matches!(value.as_str(), "unsupported" | "unavailable"))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silhouette_complexity: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proportion_exaggeration: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_softness: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faceting: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape_frequency: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lod_simplification: Option<StyleSignal>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SurfacePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physically_based: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture_detail: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saturation: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_separation: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness_range: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic_usage: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_detail_frequency: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wetness: Option<StyleSignal>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LightingPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_fill_ratio: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_softness: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_shadow: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambient_strength: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_temperature: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_intensity: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<StyleSignal>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog_density: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aerial_perspective: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weather: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflection_expectation: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volumetrics: Option<StyleSignal>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CameraPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_of_view: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_height: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_placement: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_mapping: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_priority: Option<StyleSignal>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AnimationPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpolation: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose_exaggeration: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_expectation: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responsiveness: Option<StyleSignal>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EffectsPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bloom: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posterization: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpening: Option<StyleSignal>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompositionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_space: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landmark_readability: Option<StyleSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground_edge_density: Option<StyleSignal>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleBudgets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_width_px: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_height_px: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_frame_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_texture_bytes: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleIntent {
    #[serde(default)]
    pub geometry: GeometryPolicy,
    #[serde(default)]
    pub surfaces: SurfacePolicy,
    #[serde(default)]
    pub lighting: LightingPolicy,
    #[serde(default)]
    pub environment: EnvironmentPolicy,
    #[serde(default)]
    pub camera: CameraPolicy,
    #[serde(default)]
    pub animation: AnimationPolicy,
    #[serde(default)]
    pub effects: EffectsPolicy,
    #[serde(default)]
    pub composition: CompositionPolicy,
    #[serde(default)]
    pub budgets: StyleBudgets,
}

impl StyleIntent {
    pub fn signals(&self) -> Vec<(&'static str, &StyleSignal)> {
        let mut signals = Vec::new();
        macro_rules! collect {
            ($prefix:literal, $group:expr, $( $field:ident ),+ $(,)?) => {
                $(
                    if let Some(signal) = &$group.$field {
                        signals.push((concat!($prefix, ".", stringify!($field)), signal));
                    }
                )+
            };
        }
        collect!(
            "geometry",
            self.geometry,
            silhouette_complexity,
            proportion_exaggeration,
            edge_softness,
            faceting,
            shape_frequency,
            lod_simplification
        );
        collect!(
            "surfaces",
            self.surfaces,
            physically_based,
            texture_detail,
            saturation,
            value_separation,
            roughness_range,
            metallic_usage,
            normal_detail_frequency,
            wetness
        );
        collect!(
            "lighting",
            self.lighting,
            key_fill_ratio,
            shadow_softness,
            contact_shadow,
            ambient_strength,
            color_temperature,
            environment_intensity,
            exposure
        );
        collect!(
            "environment",
            self.environment,
            fog_density,
            aerial_perspective,
            weather,
            reflection_expectation,
            volumetrics
        );
        collect!(
            "camera",
            self.camera,
            projection,
            field_of_view,
            camera_height,
            horizon_placement,
            exposure,
            tone_mapping,
            subject_priority
        );
        collect!(
            "animation",
            self.animation,
            timing,
            interpolation,
            pose_exaggeration,
            contact_expectation,
            responsiveness
        );
        collect!(
            "effects",
            self.effects,
            bloom,
            outline,
            posterization,
            sharpening
        );
        collect!(
            "composition",
            self.composition,
            density,
            negative_space,
            landmark_readability,
            foreground_edge_density
        );
        signals
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleClaim {
    pub id: String,
    pub field: String,
    pub value: String,
    pub source_refs: Vec<String>,
    pub confidence_basis_points: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleConstraint {
    pub id: String,
    pub field: String,
    pub requirement: String,
    pub hard: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleConflict {
    pub id: String,
    pub fields: Vec<String>,
    pub detail: String,
    pub resolved: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleRegion {
    pub id: String,
    pub artifact_id: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleProvenance {
    pub artifact_id: String,
    pub sha256: String,
    pub locator: String,
    pub extraction_method: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleConfidence {
    pub field: String,
    pub basis_points: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleProfile {
    pub schema_version: String,
    pub profile_id: String,
    pub source_project_id: String,
    pub intent: StyleIntent,
    pub constraints: Vec<StyleConstraint>,
    pub observations: Vec<StyleClaim>,
    pub inferences: Vec<StyleClaim>,
    pub assumptions: Vec<StyleClaim>,
    pub conflicts: Vec<StyleConflict>,
    pub regions: Vec<StyleRegion>,
    pub provenance: Vec<StyleProvenance>,
    pub confidence: Vec<StyleConfidence>,
    pub profile_sha256: String,
}

impl StyleProfile {
    pub fn new(
        profile_id: impl Into<String>,
        source_project_id: impl Into<String>,
        intent: StyleIntent,
    ) -> Result<Self> {
        let mut profile = Self {
            schema_version: STYLE_PROFILE_SCHEMA.into(),
            profile_id: profile_id.into(),
            source_project_id: source_project_id.into(),
            intent,
            constraints: Vec::new(),
            observations: Vec::new(),
            inferences: Vec::new(),
            assumptions: Vec::new(),
            conflicts: Vec::new(),
            regions: Vec::new(),
            provenance: Vec::new(),
            confidence: Vec::new(),
            profile_sha256: String::new(),
        };
        profile.reseal()?;
        Ok(profile)
    }

    pub fn reseal(&mut self) -> Result<()> {
        self.validate_structure()?;
        self.profile_sha256 = self.compute_digest()?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_structure()?;
        if self.profile_sha256 != self.compute_digest()? {
            return Err(StyleContractError(
                "style profile digest does not match its typed content".into(),
            ));
        }
        Ok(())
    }

    pub fn compute_digest(&self) -> Result<String> {
        let mut value = serde_json::to_value(self).map_err(|error| {
            StyleContractError(format!("cannot serialize style profile: {error}"))
        })?;
        value
            .as_object_mut()
            .ok_or_else(|| StyleContractError("style profile is not a JSON object".into()))?
            .remove("profile_sha256");
        Ok(sha256_prefixed(canonical_json(&value).as_bytes()))
    }

    fn validate_structure(&self) -> Result<()> {
        if self.schema_version != STYLE_PROFILE_SCHEMA {
            return Err(StyleContractError(format!(
                "unsupported style profile schema {}",
                self.schema_version
            )));
        }
        valid_id(&self.profile_id, "profile_id")?;
        valid_text(&self.source_project_id, "source_project_id")?;
        let known_refs = self.known_refs()?;
        for (field, signal) in self.intent.signals() {
            signal.validate(field, &known_refs)?;
        }
        validate_claims("observation", &self.observations, &known_refs)?;
        validate_claims("inference", &self.inferences, &known_refs)?;
        validate_claims("assumption", &self.assumptions, &known_refs)?;
        validate_constraints(&self.constraints)?;
        validate_conflicts(&self.conflicts)?;
        validate_regions(&self.regions, &self.provenance)?;
        for provenance in &self.provenance {
            valid_id(&provenance.artifact_id, "provenance.artifact_id")?;
            valid_digest(&provenance.sha256, "provenance.sha256")?;
            valid_text(&provenance.locator, "provenance.locator")?;
            valid_text(
                &provenance.extraction_method,
                "provenance.extraction_method",
            )?;
        }
        for confidence in &self.confidence {
            valid_text(&confidence.field, "confidence.field")?;
            if confidence.basis_points > CONFIDENCE_MAX {
                return Err(StyleContractError(format!(
                    "confidence for {} exceeds 10000 basis points",
                    confidence.field
                )));
            }
        }
        validate_budgets(&self.intent.budgets)
    }

    fn known_refs(&self) -> Result<BTreeSet<String>> {
        let mut refs = BTreeSet::new();
        for provenance in &self.provenance {
            if !refs.insert(provenance.artifact_id.clone()) {
                return Err(StyleContractError(format!(
                    "duplicate provenance artifact {}",
                    provenance.artifact_id
                )));
            }
        }
        for region in &self.regions {
            if !refs.insert(region.id.clone()) {
                return Err(StyleContractError(format!(
                    "duplicate style reference {}",
                    region.id
                )));
            }
        }
        Ok(refs)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StylePlanStatus {
    Complete,
    Partial,
    Indeterminate,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LoweredStylePolicy {
    pub field: String,
    pub capability_id: String,
    pub signal: StyleSignal,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UnsupportedStyleAxis {
    pub field: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StylePlan {
    pub schema_version: String,
    pub plan_id: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub registry_id: String,
    pub registry_sha256: String,
    pub status: StylePlanStatus,
    pub selected_capabilities: Vec<String>,
    pub lowered_policies: Vec<LoweredStylePolicy>,
    pub unsupported_axes: Vec<UnsupportedStyleAxis>,
    pub indeterminate_axes: Vec<String>,
    pub budgets: StyleBudgets,
    pub plan_sha256: String,
}

impl StylePlan {
    pub fn lower(profile: &StyleProfile, catalog: &CapabilityCatalog) -> Result<Self> {
        profile.validate()?;
        catalog
            .validate()
            .map_err(|error| StyleContractError(format!("capability catalog: {error}")))?;
        let required = [
            "style.profile.compile/v1",
            "graphics.scene.packet/v1",
            "graphics.visual-quality.evidence/v1",
        ];
        for capability in required {
            let descriptor = catalog.descriptor(capability).ok_or_else(|| {
                StyleContractError(format!(
                    "style lowering requires missing capability {capability}"
                ))
            })?;
            if descriptor.status == CapabilityStatus::Retired {
                return Err(StyleContractError(format!(
                    "style lowering requires retired capability {capability}"
                )));
            }
        }
        let mut lowered_policies = Vec::new();
        let mut unsupported_axes = Vec::new();
        let mut indeterminate_axes = Vec::new();
        for (field, signal) in profile.intent.signals() {
            if signal.is_explicitly_unavailable() || !style_axis_supported(field) {
                unsupported_axes.push(UnsupportedStyleAxis {
                    field: field.into(),
                    reason: if signal.is_explicitly_unavailable() {
                        "request explicitly marks this axis unavailable".into()
                    } else {
                        "current native style lowering has no certified realization".into()
                    },
                    fallback: None,
                });
                continue;
            }
            if signal.confidence_basis_points() < 5_000 {
                indeterminate_axes.push(field.into());
            }
            lowered_policies.push(LoweredStylePolicy {
                field: field.into(),
                capability_id: capability_for_axis(field).into(),
                signal: signal.clone(),
            });
        }
        for conflict in &profile.conflicts {
            if !conflict.resolved {
                indeterminate_axes.push(format!("conflict:{}", conflict.id));
            }
        }
        lowered_policies.sort_by(|left, right| left.field.cmp(&right.field));
        unsupported_axes.sort_by(|left, right| left.field.cmp(&right.field));
        indeterminate_axes.sort();
        indeterminate_axes.dedup();
        let status = if !unsupported_axes.is_empty() {
            StylePlanStatus::Partial
        } else if !indeterminate_axes.is_empty() {
            StylePlanStatus::Indeterminate
        } else {
            StylePlanStatus::Complete
        };
        let mut plan = Self {
            schema_version: STYLE_PLAN_SCHEMA.into(),
            plan_id: format!("{}:style-plan/v1", profile.profile_id),
            profile_id: profile.profile_id.clone(),
            profile_sha256: profile.profile_sha256.clone(),
            registry_id: catalog.registry_id.clone(),
            registry_sha256: catalog.registry_sha256.clone(),
            status,
            selected_capabilities: required.iter().map(|value| (*value).into()).collect(),
            lowered_policies,
            unsupported_axes,
            indeterminate_axes,
            budgets: profile.intent.budgets.clone(),
            plan_sha256: String::new(),
        };
        plan.plan_sha256 = plan.compute_digest()?;
        plan.validate(profile, catalog)?;
        Ok(plan)
    }

    pub fn validate(&self, profile: &StyleProfile, catalog: &CapabilityCatalog) -> Result<()> {
        if self.schema_version != STYLE_PLAN_SCHEMA {
            return Err(StyleContractError(format!(
                "unsupported style plan schema {}",
                self.schema_version
            )));
        }
        profile.validate()?;
        catalog
            .validate()
            .map_err(|error| StyleContractError(format!("capability catalog: {error}")))?;
        if self.profile_id != profile.profile_id
            || self.profile_sha256 != profile.profile_sha256
            || self.registry_id != catalog.registry_id
            || self.registry_sha256 != catalog.registry_sha256
        {
            return Err(StyleContractError(
                "style plan is bound to a different profile or capability registry".into(),
            ));
        }
        if self.plan_sha256 != self.compute_digest()? {
            return Err(StyleContractError(
                "style plan digest does not match its typed content".into(),
            ));
        }
        let mut selected = BTreeSet::new();
        for capability in &self.selected_capabilities {
            if !selected.insert(capability.as_str()) {
                return Err(StyleContractError(format!(
                    "style plan selects duplicate capability {capability}"
                )));
            }
            let descriptor = catalog.descriptor(capability).ok_or_else(|| {
                StyleContractError(format!(
                    "style plan selects unknown capability {capability}"
                ))
            })?;
            if descriptor.status == CapabilityStatus::Retired {
                return Err(StyleContractError(format!(
                    "style plan selects retired capability {capability}"
                )));
            }
        }
        let mut fields = BTreeSet::new();
        for policy in &self.lowered_policies {
            if !fields.insert(policy.field.as_str()) {
                return Err(StyleContractError(format!(
                    "style plan lowers duplicate field {}",
                    policy.field
                )));
            }
            if !selected.contains(policy.capability_id.as_str()) {
                return Err(StyleContractError(format!(
                    "style policy {} names an unselected capability {}",
                    policy.field, policy.capability_id
                )));
            }
        }
        let mut unsupported = BTreeSet::new();
        for axis in &self.unsupported_axes {
            if !unsupported.insert(axis.field.as_str()) {
                return Err(StyleContractError(format!(
                    "style plan repeats unsupported axis {}",
                    axis.field
                )));
            }
            if fields.contains(axis.field.as_str()) {
                return Err(StyleContractError(format!(
                    "style axis {} is both lowered and unsupported",
                    axis.field
                )));
            }
        }
        Ok(())
    }

    pub fn compute_digest(&self) -> Result<String> {
        let mut value = serde_json::to_value(self)
            .map_err(|error| StyleContractError(format!("cannot serialize style plan: {error}")))?;
        value
            .as_object_mut()
            .ok_or_else(|| StyleContractError("style plan is not a JSON object".into()))?
            .remove("plan_sha256");
        Ok(sha256_prefixed(canonical_json(&value).as_bytes()))
    }
}

fn style_axis_supported(field: &str) -> bool {
    !matches!(
        field,
        "environment.reflection_expectation"
            | "environment.volumetrics"
            | "effects.outline"
            | "effects.posterization"
    )
}

fn capability_for_axis(field: &str) -> &'static str {
    if field.starts_with("composition.") || field.starts_with("camera.") {
        "graphics.scene.packet/v1"
    } else if field.starts_with("effects.") || field.starts_with("animation.") {
        "graphics.scene.packet/v1"
    } else {
        "style.profile.compile/v1"
    }
}

fn validate_claims(kind: &str, claims: &[StyleClaim], known_refs: &BTreeSet<String>) -> Result<()> {
    let mut ids = BTreeSet::new();
    for claim in claims {
        valid_id(&claim.id, &format!("{kind}.id"))?;
        valid_text(&claim.field, &format!("{kind}.field"))?;
        valid_text(&claim.value, &format!("{kind}.value"))?;
        if !ids.insert(claim.id.as_str()) {
            return Err(StyleContractError(format!(
                "duplicate {kind} id {}",
                claim.id
            )));
        }
        if claim.confidence_basis_points > CONFIDENCE_MAX {
            return Err(StyleContractError(format!(
                "{kind} {} has confidence above 10000 basis points",
                claim.id
            )));
        }
        for source_ref in &claim.source_refs {
            valid_text(source_ref, &format!("{kind}.source_ref"))?;
            if !known_refs.is_empty() && !known_refs.contains(source_ref) {
                return Err(StyleContractError(format!(
                    "{kind} {} references unknown source {}",
                    claim.id, source_ref
                )));
            }
        }
    }
    Ok(())
}

fn validate_constraints(constraints: &[StyleConstraint]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for constraint in constraints {
        valid_id(&constraint.id, "constraint.id")?;
        valid_text(&constraint.field, "constraint.field")?;
        valid_text(&constraint.requirement, "constraint.requirement")?;
        if !ids.insert(constraint.id.as_str()) {
            return Err(StyleContractError(format!(
                "duplicate style constraint {}",
                constraint.id
            )));
        }
    }
    Ok(())
}

fn validate_conflicts(conflicts: &[StyleConflict]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for conflict in conflicts {
        valid_id(&conflict.id, "conflict.id")?;
        if conflict.fields.is_empty() || conflict.fields.iter().any(|field| field.trim().is_empty())
        {
            return Err(StyleContractError(format!(
                "style conflict {} needs non-empty fields",
                conflict.id
            )));
        }
        valid_text(&conflict.detail, "conflict.detail")?;
        if !ids.insert(conflict.id.as_str()) {
            return Err(StyleContractError(format!(
                "duplicate style conflict {}",
                conflict.id
            )));
        }
    }
    Ok(())
}

fn validate_regions(regions: &[StyleRegion], provenance: &[StyleProvenance]) -> Result<()> {
    let known_artifacts = provenance
        .iter()
        .map(|record| record.artifact_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut ids = BTreeSet::new();
    for region in regions {
        valid_id(&region.id, "region.id")?;
        valid_id(&region.artifact_id, "region.artifact_id")?;
        if region.width == 0 || region.height == 0 {
            return Err(StyleContractError(format!(
                "style region {} must have non-zero dimensions",
                region.id
            )));
        }
        if !known_artifacts.is_empty() && !known_artifacts.contains(region.artifact_id.as_str()) {
            return Err(StyleContractError(format!(
                "style region {} references unknown artifact {}",
                region.id, region.artifact_id
            )));
        }
        if !ids.insert(region.id.as_str()) {
            return Err(StyleContractError(format!(
                "duplicate style region {}",
                region.id
            )));
        }
    }
    Ok(())
}

fn validate_budgets(budgets: &StyleBudgets) -> Result<()> {
    if budgets.target_width_px == Some(0) || budgets.target_height_px == Some(0) {
        return Err(StyleContractError(
            "style target dimensions must be non-zero".into(),
        ));
    }
    Ok(())
}

fn valid_id(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
    {
        return Err(StyleContractError(format!(
            "{label} is not a safe identifier: {value:?}"
        )));
    }
    Ok(())
}

fn valid_text(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(StyleContractError(format!("{label} cannot be empty")));
    }
    Ok(())
}

fn valid_digest(value: &str, label: &str) -> Result<()> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(StyleContractError(format!(
            "{label} is not a sha256 digest"
        )));
    }
    Ok(())
}
