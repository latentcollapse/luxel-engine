use std::collections::BTreeSet;

use wge_gameplay_contract::gcs::{
    CapabilityFeatureId, CapabilityId, CapabilityRegistry, CapabilitySpec, DiagnosticSeverity,
    GCS_MANIFEST_SCHEMA, KitId, KitManifest, KitProfileSpec, NetworkRequirement,
    PersistenceRequirement, PersistenceScope, ProfileId, ResolutionCode, RuntimeCostClass,
    StateAuthority, ValidationSuiteId, foundation_registry, resolve_kit,
};

fn capability(id: &str, requires: &[&str], conflicts: &[&str]) -> CapabilitySpec {
    CapabilitySpec {
        id: CapabilityId::from(id),
        version: 1,
        requires: requires.iter().copied().map(CapabilityId::from).collect(),
        provides: BTreeSet::from([CapabilityFeatureId::new(format!("feature.{id}"))]),
        conflicts_with: conflicts.iter().copied().map(CapabilityId::from).collect(),
        optional_integrations: BTreeSet::new(),
        authority: StateAuthority::Server,
        runtime_cost: RuntimeCostClass::Low,
        network: NetworkRequirement::Optional,
        persistence: PersistenceRequirement::Optional,
        persistence_scope: Some(PersistenceScope::Character),
        validation_suites: BTreeSet::from([ValidationSuiteId::from("suite.foundation")]),
    }
}

fn profile(id: &str, includes: &[&str], capabilities: &[&str]) -> KitProfileSpec {
    KitProfileSpec {
        id: ProfileId::from(id),
        includes: includes.iter().copied().map(ProfileId::from).collect(),
        required_capabilities: capabilities
            .iter()
            .copied()
            .map(CapabilityId::from)
            .collect(),
    }
}

fn manifest(profiles: &[&str], capabilities: &[&str]) -> KitManifest {
    KitManifest {
        schema_version: GCS_MANIFEST_SCHEMA.to_owned(),
        id: KitId::from("test.kit"),
        profiles: profiles.iter().copied().map(ProfileId::from).collect(),
        capabilities: capabilities
            .iter()
            .copied()
            .map(CapabilityId::from)
            .collect(),
    }
}

fn registry(
    capabilities: Vec<CapabilitySpec>,
    profiles: Vec<KitProfileSpec>,
) -> CapabilityRegistry {
    CapabilityRegistry::new(capabilities, profiles).expect("test registry is well-formed")
}

fn codes(report: &wge_gameplay_contract::gcs::ResolutionReport) -> BTreeSet<ResolutionCode> {
    report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn unknown_requested_capability_is_an_explicit_failure() {
    let registry = registry(vec![capability("known.capability", &[], &[])], vec![]);
    let report = resolve_kit(&registry, &manifest(&[], &["unknown.capability"]));

    assert!(!report.is_success());
    assert!(codes(&report).contains(&ResolutionCode::UnknownCapability));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == ResolutionCode::UnknownCapability
            && diagnostic.subject.as_deref() == Some("unknown.capability")
    }));
}

#[test]
fn missing_required_dependency_is_distinct_from_unknown_manifest_input() {
    let registry = registry(
        vec![capability("combat.root", &["combat.missing"], &[])],
        vec![],
    );
    let report = resolve_kit(&registry, &manifest(&[], &["combat.root"]));

    assert!(!report.is_success());
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == ResolutionCode::MissingDependency)
        .expect("missing dependency diagnostic");
    assert_eq!(diagnostic.subject.as_deref(), Some("combat.root"));
    assert_eq!(diagnostic.related, ["combat.missing"]);
    assert_eq!(diagnostic.path, ["combat.root", "combat.missing"]);
}

#[test]
fn capability_dependency_cycle_reports_a_closed_path() {
    let registry = registry(
        vec![
            capability("cycle.a", &["cycle.b"], &[]),
            capability("cycle.b", &["cycle.a"], &[]),
        ],
        vec![],
    );
    let report = resolve_kit(&registry, &manifest(&[], &["cycle.a"]));

    assert!(!report.is_success());
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == ResolutionCode::CapabilityCycle)
        .expect("capability cycle diagnostic");
    assert_eq!(diagnostic.path, ["cycle.a", "cycle.b", "cycle.a"]);
}

#[test]
fn profile_inclusion_cycle_is_rejected_with_its_path() {
    let registry = registry(
        vec![capability("profile.capability", &[], &[])],
        vec![
            profile("profile.a", &["profile.b"], &[]),
            profile("profile.b", &["profile.a"], &["profile.capability"]),
        ],
    );
    let report = resolve_kit(&registry, &manifest(&["profile.a"], &[]));

    assert!(!report.is_success());
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == ResolutionCode::ProfileCycle)
        .expect("profile cycle diagnostic");
    assert_eq!(diagnostic.path, ["profile.a", "profile.b", "profile.a"]);
}

#[test]
fn conflicts_are_rejected_even_when_only_one_side_declares_them() {
    let registry = registry(
        vec![
            capability("mode.offline", &[], &["mode.online"]),
            capability("mode.online", &[], &[]),
        ],
        vec![],
    );
    let report = resolve_kit(&registry, &manifest(&[], &["mode.online", "mode.offline"]));

    assert!(!report.is_success());
    let conflicts: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == ResolutionCode::CapabilityConflict)
        .collect();
    assert_eq!(
        conflicts.len(),
        1,
        "one-sided declarations still form one conflict"
    );
    assert_eq!(conflicts[0].subject.as_deref(), Some("mode.offline"));
    assert_eq!(conflicts[0].related, ["mode.online"]);
}

#[test]
fn duplicate_registry_ids_and_manifest_selections_are_not_silently_collapsed() {
    let duplicate_registry = CapabilityRegistry::new(
        vec![
            capability("duplicate.capability", &[], &[]),
            capability("duplicate.capability", &[], &[]),
        ],
        vec![],
    )
    .expect_err("duplicate registry IDs must fail construction");
    assert!(
        duplicate_registry
            .iter()
            .any(|diagnostic| diagnostic.code == ResolutionCode::DuplicateCapabilityId)
    );

    let duplicate_profile_registry = registry(
        vec![capability("profile.capability", &[], &[])],
        vec![profile("profile.one", &[], &["profile.capability"])],
    );
    let report = resolve_kit(
        &duplicate_profile_registry,
        &manifest(
            &["profile.one", "profile.one"],
            &["profile.capability", "profile.capability"],
        ),
    );
    assert!(!report.is_success());
    assert!(codes(&report).contains(&ResolutionCode::DuplicateProfileSelection));
    assert!(codes(&report).contains(&ResolutionCode::DuplicateCapabilitySelection));
}

#[test]
fn overlapping_profiles_compose_and_close_transitive_dependencies() {
    let registry = foundation_registry().expect("checked-in foundation registry is valid");
    let report = resolve_kit(
        &registry,
        &manifest(&["soulslike", "looter_shooter", "survival"], &[]),
    );
    let resolved = report
        .resolved
        .as_ref()
        .expect("the cross-kit composition is valid");

    for profile_id in [
        "action_rpg",
        "soulslike",
        "shooter",
        "looter_shooter",
        "survival",
    ] {
        assert!(resolved.profiles.contains_key(&ProfileId::from(profile_id)));
    }
    for capability_id in [
        "character.movement",
        "state.attributes",
        "inventory.core",
        "world.persistence",
        "combat.melee",
        "combat.shooter",
        "loot.affixes",
        "survival.crafting",
    ] {
        assert!(
            resolved
                .capabilities
                .contains_key(&CapabilityId::from(capability_id))
        );
    }
    assert_eq!(
        resolved
            .capabilities
            .keys()
            .filter(|id| id.as_str() == "inventory.core")
            .count(),
        1,
        "shared capabilities are selected once"
    );
    assert!(!report.has_errors());
}

#[test]
fn manifest_order_does_not_change_canonical_bytes_or_digest() {
    let registry = foundation_registry().expect("checked-in foundation registry is valid");
    let first = resolve_kit(
        &registry,
        &manifest(
            &["soulslike", "looter_shooter", "survival"],
            &["network.session", "combat.stamina"],
        ),
    )
    .resolved
    .expect("first order resolves");
    let second = resolve_kit(
        &registry,
        &manifest(
            &["survival", "looter_shooter", "soulslike"],
            &["combat.stamina", "network.session"],
        ),
    )
    .resolved
    .expect("second order resolves");

    assert_eq!(
        first.canonical_bytes().unwrap(),
        second.canonical_bytes().unwrap()
    );
    let digest = first.sha256().unwrap();
    assert_eq!(digest, second.sha256().unwrap());
    assert!(digest.starts_with("sha256:"));
    assert_eq!(digest.len(), "sha256:".len() + 64);
    assert!(
        first
            .active_optional_integrations
            .iter()
            .any(|integration| {
                integration.source == CapabilityId::from("abilities.core")
                    && integration.target == CapabilityId::from("network.session")
            })
    );
}

#[test]
fn optional_integrations_are_reported_but_never_implicitly_selected() {
    let registry = foundation_registry().expect("checked-in foundation registry is valid");
    let report = resolve_kit(&registry, &manifest(&["action_rpg"], &[]));
    let resolved = report.resolved.as_ref().expect("base profile resolves");

    assert!(
        !resolved
            .capabilities
            .contains_key(&CapabilityId::from("network.session"))
    );
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == ResolutionCode::OptionalIntegrationUnavailable
            && diagnostic.severity == DiagnosticSeverity::Warning
            && diagnostic.related == ["network.session"]
    }));
}

#[test]
fn resolved_kit_json_round_trips_without_changing_its_digest() {
    let registry = foundation_registry().expect("checked-in foundation registry is valid");
    let resolved = resolve_kit(&registry, &manifest(&["survival"], &[]))
        .resolved
        .expect("survival profile resolves");
    let bytes = resolved.canonical_bytes().unwrap();
    let decoded: wge_gameplay_contract::gcs::ResolvedKit =
        serde_json::from_slice(&bytes).expect("resolved kit is typed JSON");

    assert_eq!(decoded.canonical_bytes().unwrap(), bytes);
    assert_eq!(decoded.sha256().unwrap(), resolved.sha256().unwrap());
}
