//! GCS foundation: typed capability metadata and deterministic kit composition.
//!
//! This module describes gameplay capabilities. It does not implement them or
//! select a runtime. Profiles are recipes over capability IDs, and resolution
//! only validates and closes their declared dependency graph.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const GCS_MANIFEST_SCHEMA: &str = "luxel.gcs-kit-manifest/v1";
pub const GCS_RESOLVED_KIT_SCHEMA: &str = "luxel.gcs-resolved-kit/v1";
pub const REFERENCE_VERTICAL_SLICE_KIT_ID: &str = "luxel.reference.vertical-slice";
const MAX_ID_BYTES: usize = 128;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

string_id!(CapabilityId);
string_id!(CapabilityFeatureId);
string_id!(ValidationSuiteId);
string_id!(KitId);
string_id!(ProfileId);

/// Owner of the authoritative state for a capability when it is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateAuthority {
    Local,
    Server,
    Client,
    PeerDeterministic,
}

/// Semantic estimate only; this is not a measured frame-time promise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCostClass {
    Negligible,
    Low,
    Moderate,
    High,
    Variable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkRequirement {
    None,
    Optional,
    Required,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceRequirement {
    None,
    Optional,
    Required,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceScope {
    Session,
    Character,
    World,
    Account,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitySpec {
    pub id: CapabilityId,
    pub version: u32,
    pub requires: BTreeSet<CapabilityId>,
    pub provides: BTreeSet<CapabilityFeatureId>,
    pub conflicts_with: BTreeSet<CapabilityId>,
    /// Optional edges are reported as active only when both capabilities are selected.
    /// They never cause implicit capability selection.
    pub optional_integrations: BTreeSet<CapabilityId>,
    pub authority: StateAuthority,
    pub runtime_cost: RuntimeCostClass,
    pub network: NetworkRequirement,
    pub persistence: PersistenceRequirement,
    pub persistence_scope: Option<PersistenceScope>,
    pub validation_suites: BTreeSet<ValidationSuiteId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KitProfileSpec {
    pub id: ProfileId,
    pub includes: BTreeSet<ProfileId>,
    pub required_capabilities: BTreeSet<CapabilityId>,
}

/// A model-authored request. Vector order is accepted as input syntax; the
/// resolver canonicalizes it into ordered sets before producing a kit digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KitManifest {
    pub schema_version: String,
    pub id: KitId,
    pub profiles: Vec<ProfileId>,
    pub capabilities: Vec<CapabilityId>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalIntegration {
    pub source: CapabilityId,
    pub target: CapabilityId,
}

/// Fully expanded semantic kit. BTree collections make its JSON representation
/// independent of manifest ordering and hash-map iteration behavior.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedKit {
    pub schema_version: String,
    pub id: KitId,
    pub profiles: BTreeMap<ProfileId, KitProfileSpec>,
    pub capabilities: BTreeMap<CapabilityId, CapabilitySpec>,
    pub active_optional_integrations: BTreeSet<OptionalIntegration>,
}

impl ResolvedKit {
    /// Compact canonical JSON, matching the gameplay receipt convention.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn sha256(&self) -> Result<String, serde_json::Error> {
        let bytes = self.canonical_bytes()?;
        Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionCode {
    UnsupportedManifestSchema,
    InvalidIdentifier,
    InvalidCapabilityVersion,
    MissingProvidedFeature,
    MissingValidationSuite,
    InvalidPersistenceMetadata,
    DuplicateCapabilityId,
    DuplicateProfileId,
    DuplicateCapabilitySelection,
    DuplicateProfileSelection,
    EmptyManifest,
    UnknownCapability,
    UnknownProfile,
    MissingDependency,
    CapabilityCycle,
    ProfileCycle,
    CapabilityConflict,
    OptionalIntegrationUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionDiagnostic {
    pub code: ResolutionCode,
    pub severity: DiagnosticSeverity,
    pub subject: Option<String>,
    pub related: Vec<String>,
    pub path: Vec<String>,
    pub detail: String,
}

impl ResolutionDiagnostic {
    fn error(
        code: ResolutionCode,
        subject: Option<String>,
        related: Vec<String>,
        path: Vec<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Error,
            subject,
            related,
            path,
            detail: detail.into(),
        }
    }

    fn warning(
        code: ResolutionCode,
        subject: Option<String>,
        related: Vec<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Warning,
            subject,
            related,
            path: Vec::new(),
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionReport {
    pub resolved: Option<ResolvedKit>,
    pub diagnostics: Vec<ResolutionDiagnostic>,
}

impl ResolutionReport {
    pub fn is_success(&self) -> bool {
        self.resolved.is_some()
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }
}

#[derive(Clone, Debug, Default)]
pub struct CapabilityRegistry {
    capabilities: BTreeMap<CapabilityId, CapabilitySpec>,
    profiles: BTreeMap<ProfileId, KitProfileSpec>,
}

impl CapabilityRegistry {
    /// Build a registry while rejecting duplicate IDs and malformed specs.
    /// Missing dependency references are diagnosed during resolution so the
    /// diagnostic can include the selected path that made them relevant.
    pub fn new(
        capabilities: Vec<CapabilitySpec>,
        profiles: Vec<KitProfileSpec>,
    ) -> Result<Self, Vec<ResolutionDiagnostic>> {
        let mut registry = Self::default();
        let mut diagnostics = Vec::new();

        for capability in capabilities {
            validate_capability(&capability, &mut diagnostics);
            if registry.capabilities.contains_key(&capability.id) {
                diagnostics.push(ResolutionDiagnostic::error(
                    ResolutionCode::DuplicateCapabilityId,
                    Some(capability.id.as_str().to_owned()),
                    Vec::new(),
                    Vec::new(),
                    "registry contains more than one specification for this capability ID",
                ));
            } else {
                registry
                    .capabilities
                    .insert(capability.id.clone(), capability);
            }
        }

        for profile in profiles {
            validate_profile(&profile, &mut diagnostics);
            if registry.profiles.contains_key(&profile.id) {
                diagnostics.push(ResolutionDiagnostic::error(
                    ResolutionCode::DuplicateProfileId,
                    Some(profile.id.as_str().to_owned()),
                    Vec::new(),
                    Vec::new(),
                    "registry contains more than one specification for this profile ID",
                ));
            } else {
                registry.profiles.insert(profile.id.clone(), profile);
            }
        }

        sort_diagnostics(&mut diagnostics);
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        {
            Err(diagnostics)
        } else {
            Ok(registry)
        }
    }

    pub fn capability(&self, id: &CapabilityId) -> Option<&CapabilitySpec> {
        self.capabilities.get(id)
    }

    pub fn profile(&self, id: &ProfileId) -> Option<&KitProfileSpec> {
        self.profiles.get(id)
    }

    pub fn capabilities(&self) -> &BTreeMap<CapabilityId, CapabilitySpec> {
        &self.capabilities
    }

    pub fn profiles(&self) -> &BTreeMap<ProfileId, KitProfileSpec> {
        &self.profiles
    }
}

/// Expand profile includes and transitive required capabilities, then report
/// all reachable dependency, cycle, and conflict failures in stable order.
pub fn resolve_kit(registry: &CapabilityRegistry, manifest: &KitManifest) -> ResolutionReport {
    let mut diagnostics = Vec::new();
    if manifest.schema_version != GCS_MANIFEST_SCHEMA {
        diagnostics.push(ResolutionDiagnostic::error(
            ResolutionCode::UnsupportedManifestSchema,
            Some(manifest.id.as_str().to_owned()),
            Vec::new(),
            Vec::new(),
            format!("expected manifest schema {GCS_MANIFEST_SCHEMA:?}"),
        ));
    }
    validate_identifier(
        manifest.id.as_str(),
        "kit",
        Some(manifest.id.as_str()),
        &mut diagnostics,
    );
    for id in &manifest.profiles {
        validate_identifier(
            id.as_str(),
            "selected profile",
            Some(manifest.id.as_str()),
            &mut diagnostics,
        );
    }
    for id in &manifest.capabilities {
        validate_identifier(
            id.as_str(),
            "selected capability",
            Some(manifest.id.as_str()),
            &mut diagnostics,
        );
    }

    let selected_profiles: BTreeSet<_> = manifest.profiles.iter().cloned().collect();
    if selected_profiles.len() != manifest.profiles.len() {
        add_duplicate_selections(
            &manifest.profiles,
            ResolutionCode::DuplicateProfileSelection,
            "profile",
            &mut diagnostics,
        );
    }
    let explicit_capabilities: BTreeSet<_> = manifest.capabilities.iter().cloned().collect();
    if explicit_capabilities.len() != manifest.capabilities.len() {
        add_duplicate_selections(
            &manifest.capabilities,
            ResolutionCode::DuplicateCapabilitySelection,
            "capability",
            &mut diagnostics,
        );
    }
    if manifest.profiles.is_empty() && manifest.capabilities.is_empty() {
        diagnostics.push(ResolutionDiagnostic::error(
            ResolutionCode::EmptyManifest,
            Some(manifest.id.as_str().to_owned()),
            Vec::new(),
            Vec::new(),
            "a kit must select at least one profile or capability",
        ));
    }

    let mut expanded_profiles = BTreeMap::new();
    let mut profile_state = BTreeMap::new();
    let mut profile_stack = Vec::new();
    let mut profile_capabilities = BTreeSet::new();
    for profile_id in selected_profiles {
        expand_profile(
            profile_id,
            registry,
            &mut profile_state,
            &mut profile_stack,
            &mut expanded_profiles,
            &mut profile_capabilities,
            &mut diagnostics,
        );
    }

    let mut roots = profile_capabilities;
    roots.extend(explicit_capabilities.iter().cloned());
    let mut capability_state = BTreeMap::new();
    let mut capability_stack = Vec::new();
    let mut expanded_capabilities = BTreeSet::new();
    for capability_id in roots {
        expand_capability(
            capability_id,
            None,
            registry,
            &mut capability_state,
            &mut capability_stack,
            &mut expanded_capabilities,
            &mut diagnostics,
        );
    }

    let mut conflicts = BTreeSet::new();
    for capability_id in &expanded_capabilities {
        let Some(spec) = registry.capabilities.get(capability_id) else {
            continue;
        };
        for other_id in &spec.conflicts_with {
            if expanded_capabilities.contains(other_id) && other_id != capability_id {
                let pair = if capability_id < other_id {
                    (capability_id.clone(), other_id.clone())
                } else {
                    (other_id.clone(), capability_id.clone())
                };
                conflicts.insert(pair);
            }
        }
    }
    for (left, right) in conflicts {
        diagnostics.push(ResolutionDiagnostic::error(
            ResolutionCode::CapabilityConflict,
            Some(left.as_str().to_owned()),
            vec![right.as_str().to_owned()],
            Vec::new(),
            format!("capabilities {left} and {right} declare an incompatible combination"),
        ));
    }

    let mut active_optional_integrations = BTreeSet::new();
    for capability_id in &expanded_capabilities {
        let Some(spec) = registry.capabilities.get(capability_id) else {
            continue;
        };
        for target in &spec.optional_integrations {
            if expanded_capabilities.contains(target) {
                active_optional_integrations.insert(OptionalIntegration {
                    source: capability_id.clone(),
                    target: target.clone(),
                });
            } else {
                let availability = if registry.capabilities.contains_key(target) {
                    "registered but not selected"
                } else {
                    "not present in the registry"
                };
                diagnostics.push(ResolutionDiagnostic::warning(
                    ResolutionCode::OptionalIntegrationUnavailable,
                    Some(capability_id.as_str().to_owned()),
                    vec![target.as_str().to_owned()],
                    format!(
                        "optional integration {target} is {availability}; resolution continues"
                    ),
                ));
            }
        }
    }

    sort_diagnostics(&mut diagnostics);
    let has_errors = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error);
    let resolved = if has_errors {
        None
    } else {
        let capabilities = expanded_capabilities
            .into_iter()
            .filter_map(|id| {
                registry
                    .capabilities
                    .get(&id)
                    .cloned()
                    .map(|spec| (id, spec))
            })
            .collect();
        Some(ResolvedKit {
            schema_version: GCS_RESOLVED_KIT_SCHEMA.to_owned(),
            id: manifest.id.clone(),
            profiles: expanded_profiles,
            capabilities,
            active_optional_integrations,
        })
    };
    ResolutionReport {
        resolved,
        diagnostics,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Complete,
}

fn expand_profile(
    id: ProfileId,
    registry: &CapabilityRegistry,
    state: &mut BTreeMap<ProfileId, VisitState>,
    stack: &mut Vec<ProfileId>,
    expanded: &mut BTreeMap<ProfileId, KitProfileSpec>,
    capabilities: &mut BTreeSet<CapabilityId>,
    diagnostics: &mut Vec<ResolutionDiagnostic>,
) {
    match state.get(&id) {
        Some(VisitState::Complete) => return,
        Some(VisitState::Visiting) => {
            let path = cycle_path(stack, &id);
            diagnostics.push(ResolutionDiagnostic::error(
                ResolutionCode::ProfileCycle,
                Some(id.as_str().to_owned()),
                Vec::new(),
                path.iter().map(|part| part.as_str().to_owned()).collect(),
                format!("profile inclusion cycle: {}", format_path(&path)),
            ));
            return;
        }
        None => {}
    }

    let Some(profile) = registry.profiles.get(&id) else {
        diagnostics.push(ResolutionDiagnostic::error(
            ResolutionCode::UnknownProfile,
            Some(id.as_str().to_owned()),
            Vec::new(),
            stack
                .iter()
                .map(|part| part.as_str().to_owned())
                .chain(std::iter::once(id.as_str().to_owned()))
                .collect(),
            format!("profile {id} is not registered"),
        ));
        return;
    };

    state.insert(id.clone(), VisitState::Visiting);
    stack.push(id.clone());
    for included in &profile.includes {
        expand_profile(
            included.clone(),
            registry,
            state,
            stack,
            expanded,
            capabilities,
            diagnostics,
        );
    }
    for capability in &profile.required_capabilities {
        if registry.capabilities.contains_key(capability) {
            capabilities.insert(capability.clone());
        } else {
            diagnostics.push(ResolutionDiagnostic::error(
                ResolutionCode::MissingDependency,
                Some(id.as_str().to_owned()),
                vec![capability.as_str().to_owned()],
                vec![id.as_str().to_owned(), capability.as_str().to_owned()],
                format!("profile {id} requires unregistered capability {capability}"),
            ));
        }
    }
    stack.pop();
    state.insert(id.clone(), VisitState::Complete);
    expanded.insert(id, profile.clone());
}

fn expand_capability(
    id: CapabilityId,
    parent: Option<&CapabilityId>,
    registry: &CapabilityRegistry,
    state: &mut BTreeMap<CapabilityId, VisitState>,
    stack: &mut Vec<CapabilityId>,
    expanded: &mut BTreeSet<CapabilityId>,
    diagnostics: &mut Vec<ResolutionDiagnostic>,
) {
    match state.get(&id) {
        Some(VisitState::Complete) => return,
        Some(VisitState::Visiting) => {
            let path = cycle_path(stack, &id);
            diagnostics.push(ResolutionDiagnostic::error(
                ResolutionCode::CapabilityCycle,
                Some(id.as_str().to_owned()),
                Vec::new(),
                path.iter().map(|part| part.as_str().to_owned()).collect(),
                format!("capability dependency cycle: {}", format_path(&path)),
            ));
            return;
        }
        None => {}
    }

    let Some(capability) = registry.capabilities.get(&id) else {
        let (code, subject, detail) = if let Some(parent) = parent {
            (
                ResolutionCode::MissingDependency,
                parent.as_str().to_owned(),
                format!("capability {parent} requires unregistered capability {id}"),
            )
        } else {
            (
                ResolutionCode::UnknownCapability,
                id.as_str().to_owned(),
                format!("capability {id} is not registered"),
            )
        };
        diagnostics.push(ResolutionDiagnostic::error(
            code,
            Some(subject),
            vec![id.as_str().to_owned()],
            stack
                .iter()
                .map(|part| part.as_str().to_owned())
                .chain(std::iter::once(id.as_str().to_owned()))
                .collect(),
            detail,
        ));
        return;
    };

    state.insert(id.clone(), VisitState::Visiting);
    stack.push(id.clone());
    expanded.insert(id.clone());
    for dependency in &capability.requires {
        expand_capability(
            dependency.clone(),
            Some(&id),
            registry,
            state,
            stack,
            expanded,
            diagnostics,
        );
    }
    stack.pop();
    state.insert(id, VisitState::Complete);
}

fn cycle_path<T: Clone + PartialEq>(stack: &[T], repeated: &T) -> Vec<T> {
    let start = stack.iter().position(|item| item == repeated).unwrap_or(0);
    let mut path = stack[start..].to_vec();
    path.push(repeated.clone());
    path
}

fn format_path<T: AsRef<str>>(path: &[T]) -> String {
    path.iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn validate_capability(capability: &CapabilitySpec, diagnostics: &mut Vec<ResolutionDiagnostic>) {
    validate_identifier(
        capability.id.as_str(),
        "capability",
        Some(capability.id.as_str()),
        diagnostics,
    );
    if capability.version == 0 {
        diagnostics.push(ResolutionDiagnostic::error(
            ResolutionCode::InvalidCapabilityVersion,
            Some(capability.id.as_str().to_owned()),
            Vec::new(),
            Vec::new(),
            "capability version must be greater than zero",
        ));
    }
    if capability.provides.is_empty() {
        diagnostics.push(ResolutionDiagnostic::error(
            ResolutionCode::MissingProvidedFeature,
            Some(capability.id.as_str().to_owned()),
            Vec::new(),
            Vec::new(),
            "every capability must declare at least one provided feature",
        ));
    }
    if capability.validation_suites.is_empty() {
        diagnostics.push(ResolutionDiagnostic::error(
            ResolutionCode::MissingValidationSuite,
            Some(capability.id.as_str().to_owned()),
            Vec::new(),
            Vec::new(),
            "every registered capability must name at least one validation suite",
        ));
    }
    let expected_scope = capability.persistence != PersistenceRequirement::None;
    if expected_scope != capability.persistence_scope.is_some() {
        diagnostics.push(ResolutionDiagnostic::error(
            ResolutionCode::InvalidPersistenceMetadata,
            Some(capability.id.as_str().to_owned()),
            Vec::new(),
            Vec::new(),
            "persistence scope must be present exactly when persistence is optional or required",
        ));
    }

    for id in &capability.requires {
        validate_identifier(
            id.as_str(),
            "required capability",
            Some(capability.id.as_str()),
            diagnostics,
        );
    }
    for id in &capability.conflicts_with {
        validate_identifier(
            id.as_str(),
            "conflicting capability",
            Some(capability.id.as_str()),
            diagnostics,
        );
    }
    for id in &capability.optional_integrations {
        validate_identifier(
            id.as_str(),
            "optional integration",
            Some(capability.id.as_str()),
            diagnostics,
        );
    }
    for feature in &capability.provides {
        validate_identifier(
            feature.as_str(),
            "provided feature",
            Some(capability.id.as_str()),
            diagnostics,
        );
    }
    for suite in &capability.validation_suites {
        validate_identifier(
            suite.as_str(),
            "validation suite",
            Some(capability.id.as_str()),
            diagnostics,
        );
    }
}

fn validate_profile(profile: &KitProfileSpec, diagnostics: &mut Vec<ResolutionDiagnostic>) {
    validate_identifier(
        profile.id.as_str(),
        "profile",
        Some(profile.id.as_str()),
        diagnostics,
    );
    for id in &profile.includes {
        validate_identifier(
            id.as_str(),
            "included profile",
            Some(profile.id.as_str()),
            diagnostics,
        );
    }
    for id in &profile.required_capabilities {
        validate_identifier(
            id.as_str(),
            "profile capability",
            Some(profile.id.as_str()),
            diagnostics,
        );
    }
}

fn validate_identifier(
    value: &str,
    kind: &str,
    subject: Option<&str>,
    diagnostics: &mut Vec<ResolutionDiagnostic>,
) {
    let mut chars = value.chars();
    let first = chars.next();
    let valid = value.len() <= MAX_ID_BYTES
        && first.is_some_and(|character| character.is_ascii_lowercase())
        && chars.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-' | '.')
        });
    if !valid {
        diagnostics.push(ResolutionDiagnostic::error(
            ResolutionCode::InvalidIdentifier,
            subject.map(str::to_owned),
            vec![value.to_owned()],
            Vec::new(),
            format!("{kind} ID must be 1..={MAX_ID_BYTES} bytes, start with lowercase ASCII, and contain only lowercase ASCII, digits, '.', '_' or '-'"),
        ));
    }
}

fn add_duplicate_selections<T: Ord + AsRef<str>>(
    values: &[T],
    code: ResolutionCode,
    kind: &str,
    diagnostics: &mut Vec<ResolutionDiagnostic>,
) {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value.as_ref()) {
            diagnostics.push(ResolutionDiagnostic::error(
                code,
                Some(value.as_ref().to_owned()),
                Vec::new(),
                Vec::new(),
                format!("manifest selects {kind} {} more than once", value.as_ref()),
            ));
        }
    }
}

fn sort_diagnostics(diagnostics: &mut Vec<ResolutionDiagnostic>) {
    diagnostics.sort();
    diagnostics.dedup();
}

fn id_set<T: From<&'static str> + Ord>(values: &[&'static str]) -> BTreeSet<T> {
    values.iter().map(|value| T::from(*value)).collect()
}

// This private factory deliberately mirrors every field of `CapabilitySpec` so
// the checked-in catalog remains a compact, auditable table. The public API
// still accepts the typed struct, and the lint exception is confined to this
// declarative catalog constructor.
#[allow(clippy::too_many_arguments)]
fn capability(
    id: &'static str,
    requires: &[&'static str],
    provides: &[&'static str],
    conflicts_with: &[&'static str],
    optional_integrations: &[&'static str],
    authority: StateAuthority,
    runtime_cost: RuntimeCostClass,
    network: NetworkRequirement,
    persistence: PersistenceRequirement,
    persistence_scope: Option<PersistenceScope>,
    validation_suite: &'static str,
) -> CapabilitySpec {
    CapabilitySpec {
        id: CapabilityId::from(id),
        version: 1,
        requires: id_set(requires),
        provides: id_set(provides),
        conflicts_with: id_set(conflicts_with),
        optional_integrations: id_set(optional_integrations),
        authority,
        runtime_cost,
        network,
        persistence,
        persistence_scope,
        validation_suites: id_set(&[validation_suite]),
    }
}

fn profile(
    id: &'static str,
    includes: &[&'static str],
    required_capabilities: &[&'static str],
) -> KitProfileSpec {
    KitProfileSpec {
        id: ProfileId::from(id),
        includes: id_set(includes),
        required_capabilities: id_set(required_capabilities),
    }
}

/// Small reference catalog proving profile reuse without attempting to define
/// a broad genre catalog or provide gameplay runtime machinery.
pub fn foundation_registry() -> Result<CapabilityRegistry, Vec<ResolutionDiagnostic>> {
    use NetworkRequirement::{
        None as NoNetwork, Optional as OptionalNetwork, Required as RequiredNetwork,
    };
    use PersistenceRequirement::{
        None as NoPersistence, Optional as OptionalPersistence, Required as RequiredPersistence,
    };
    use RuntimeCostClass::{High, Low, Moderate, Negligible};
    use StateAuthority::{Local, Server};

    let caps = vec![
        capability(
            "character.movement",
            &[],
            &["character.movement"],
            &[],
            &[],
            Server,
            Moderate,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.character.movement",
        ),
        capability(
            "state.attributes",
            &[],
            &["state.attributes"],
            &[],
            &[],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.state.attributes",
        ),
        capability(
            "abilities.core",
            &["state.attributes"],
            &["abilities.actions"],
            &[],
            &["network.session"],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.abilities.core",
        ),
        capability(
            "combat.melee",
            &["abilities.core", "state.attributes"],
            &["combat.melee"],
            &[],
            &[],
            Server,
            Moderate,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.combat.melee",
        ),
        capability(
            "combat.stamina",
            &["state.attributes"],
            &["combat.stamina"],
            &[],
            &[],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.combat.stamina",
        ),
        capability(
            "combat.dodge",
            &["character.movement", "state.attributes"],
            &["combat.dodge"],
            &[],
            &[],
            Server,
            Moderate,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.combat.dodge",
        ),
        capability(
            "world.persistence",
            &[],
            &["world.persistence"],
            &[],
            &[],
            Server,
            High,
            OptionalNetwork,
            RequiredPersistence,
            Some(PersistenceScope::World),
            "gcs.world.persistence",
        ),
        capability(
            "world.checkpoints",
            &["encounter.reset", "world.persistence"],
            &["world.checkpoints"],
            &[],
            &[],
            Server,
            Moderate,
            OptionalNetwork,
            RequiredPersistence,
            Some(PersistenceScope::World),
            "gcs.world.checkpoints",
        ),
        capability(
            "encounter.reset",
            &["world.persistence"],
            &["encounter.reset"],
            &[],
            &[],
            Server,
            Moderate,
            OptionalNetwork,
            RequiredPersistence,
            Some(PersistenceScope::World),
            "gcs.encounter.reset",
        ),
        capability(
            "objectives.core",
            &[],
            &["objectives.state"],
            &[],
            &[],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::World),
            "gcs.objectives.core",
        ),
        capability(
            "inventory.core",
            &[],
            &["inventory.items"],
            &[],
            &[],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.inventory.core",
        ),
        capability(
            "equipment.core",
            &["inventory.core", "state.attributes"],
            &["equipment.slots"],
            &[],
            &[],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.equipment.core",
        ),
        capability(
            "progression.core",
            &["state.attributes"],
            &["progression.character"],
            &[],
            &[],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.progression.core",
        ),
        capability(
            "combat.shooter",
            &["character.movement", "state.attributes"],
            &["combat.ranged"],
            &[],
            &["network.session"],
            Server,
            Moderate,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.combat.shooter",
        ),
        capability(
            "weapons.core",
            &["inventory.core", "state.attributes"],
            &["weapons.handling"],
            &[],
            &[],
            Server,
            Moderate,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.weapons.core",
        ),
        capability(
            "ai.perception",
            &["character.movement"],
            &["ai.perception"],
            &[],
            &[],
            Server,
            Moderate,
            OptionalNetwork,
            NoPersistence,
            None,
            "gcs.ai.perception",
        ),
        capability(
            "loot.tables",
            &["inventory.core"],
            &["loot.tables"],
            &[],
            &["network.session"],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::World),
            "gcs.loot.tables",
        ),
        capability(
            "loot.affixes",
            &["equipment.core", "loot.tables", "progression.core"],
            &["loot.affixes"],
            &[],
            &[],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.loot.affixes",
        ),
        capability(
            "survival.gathering",
            &["character.movement"],
            &["survival.gathering"],
            &[],
            &[],
            Server,
            Low,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::World),
            "gcs.survival.gathering",
        ),
        capability(
            "survival.crafting",
            &["inventory.core", "survival.gathering"],
            &["survival.crafting"],
            &[],
            &[],
            Server,
            Moderate,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.survival.crafting",
        ),
        capability(
            "survival.needs",
            &["state.attributes"],
            &["survival.needs"],
            &[],
            &[],
            Server,
            Moderate,
            OptionalNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Character),
            "gcs.survival.needs",
        ),
        capability(
            "network.offline",
            &[],
            &["network.offline"],
            &["network.session"],
            &[],
            Local,
            Negligible,
            NoNetwork,
            NoPersistence,
            None,
            "gcs.network.offline",
        ),
        capability(
            "network.session",
            &[],
            &["network.session"],
            &["network.offline"],
            &[],
            Server,
            High,
            RequiredNetwork,
            OptionalPersistence,
            Some(PersistenceScope::Session),
            "gcs.network.session",
        ),
    ];
    let profiles = vec![
        profile(
            "action_rpg",
            &[],
            &[
                "abilities.core",
                "character.movement",
                "combat.melee",
                "equipment.core",
                "inventory.core",
                "objectives.core",
                "progression.core",
                "state.attributes",
            ],
        ),
        profile(
            "soulslike",
            &["action_rpg"],
            &[
                "combat.dodge",
                "combat.stamina",
                "encounter.reset",
                "world.checkpoints",
            ],
        ),
        profile(
            "shooter",
            &[],
            &[
                "ai.perception",
                "character.movement",
                "combat.shooter",
                "objectives.core",
                "state.attributes",
                "weapons.core",
            ],
        ),
        profile(
            "looter_shooter",
            &["shooter"],
            &[
                "equipment.core",
                "inventory.core",
                "loot.affixes",
                "loot.tables",
                "progression.core",
            ],
        ),
        profile(
            "survival",
            &[],
            &[
                "character.movement",
                "inventory.core",
                "state.attributes",
                "survival.crafting",
                "survival.needs",
                "world.persistence",
            ],
        ),
    ];
    CapabilityRegistry::new(caps, profiles)
}

/// The minimal certified reference runtime kit. It is deliberately expressed
/// as a normal profile selection plus an offline transport capability so the
/// runtime exercises the same resolution path a model-authored project uses.
pub fn reference_vertical_slice_manifest() -> KitManifest {
    KitManifest {
        schema_version: GCS_MANIFEST_SCHEMA.into(),
        id: KitId::new(REFERENCE_VERTICAL_SLICE_KIT_ID),
        profiles: vec![ProfileId::new("action_rpg")],
        capabilities: vec![CapabilityId::new("network.offline")],
    }
}

pub fn reference_vertical_slice_kit() -> Result<ResolvedKit, Vec<ResolutionDiagnostic>> {
    let registry = foundation_registry()?;
    let report = resolve_kit(&registry, &reference_vertical_slice_manifest());
    report.resolved.ok_or(report.diagnostics)
}
