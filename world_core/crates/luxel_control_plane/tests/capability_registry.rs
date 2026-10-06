use luxel_control_plane::capability_registry::{
    CapabilityCatalog, CapabilityDependency, CapabilityStatus, class_from_str, descriptor_json,
};

#[test]
fn native_catalog_is_closed_deterministic_and_content_addressed() {
    let left = CapabilityCatalog::native_v1();
    let right = CapabilityCatalog::native_v1();

    left.validate().unwrap();
    assert_eq!(left, right);
    assert_eq!(left.registry_sha256, left.compute_digest().unwrap());
    assert!(left.capabilities.len() >= 10);
    assert!(left.descriptor("graphics.scene.packet/v1").is_some());
}

#[test]
fn catalog_rejects_tampered_digest_duplicate_and_unknown_dependency() {
    let mut tampered = CapabilityCatalog::native_v1();
    tampered.registry_sha256 =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".into();
    assert!(tampered.validate().is_err());

    let mut duplicate = CapabilityCatalog::native_v1();
    let first = duplicate.capabilities[0].clone();
    duplicate.capabilities.push(first);
    assert!(
        duplicate
            .validate()
            .unwrap_err()
            .0
            .contains("duplicate capability")
    );

    let mut unknown = CapabilityCatalog::native_v1();
    unknown.capabilities[0]
        .dependencies
        .push(CapabilityDependency {
            id: "missing.capability/v1".into(),
            version: 1,
        });
    unknown.registry_sha256 = unknown.compute_digest().unwrap();
    assert!(
        unknown
            .validate()
            .unwrap_err()
            .0
            .contains("unknown capability")
    );
}

#[test]
fn list_and_explain_are_discoverable_without_backend_identity() {
    let catalog = CapabilityCatalog::native_v1();
    let graphics = catalog.list(class_from_str("graphics"), None);
    assert_eq!(graphics.len(), 2);
    assert!(
        graphics
            .iter()
            .all(|capability| capability.id.contains("/v1"))
    );

    let candidates = catalog.list(None, Some(CapabilityStatus::Candidate));
    assert!(
        candidates
            .iter()
            .any(|capability| capability.id == "graphics.scene.packet/v1")
    );

    let explained = descriptor_json(&catalog, "graphics.visual-quality.evidence/v1").unwrap();
    assert_eq!(explained["registry_sha256"], catalog.registry_sha256);
    assert!(
        explained["validators"][0]
            .as_str()
            .unwrap()
            .starts_with("luxel.validator.")
    );
    assert!(descriptor_json(&catalog, "missing/v1").is_err());
}
