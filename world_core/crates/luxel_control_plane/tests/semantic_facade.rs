use luxel_control_plane::semantic_facade::{
    FacadeStatus, SEMANTIC_FACADE_SCHEMA, SemanticFacadeCatalog,
};

#[test]
fn native_facade_is_content_addressed_and_status_explicit() {
    let catalog = SemanticFacadeCatalog::native_v1();
    catalog.validate().unwrap();
    assert_eq!(catalog.schema_version, SEMANTIC_FACADE_SCHEMA);
    assert_eq!(catalog.facade_sha256, catalog.compute_digest().unwrap());
    assert!(
        catalog
            .list(Some(FacadeStatus::Available))
            .iter()
            .all(|operation| operation.transport_operation.is_some())
    );
    assert!(
        catalog
            .list(Some(FacadeStatus::Planned))
            .iter()
            .all(|operation| operation.transport_operation.is_none())
    );
}

#[test]
fn facade_explain_is_discoverable_without_backend_identity() {
    let catalog = SemanticFacadeCatalog::native_v1();
    let plan = catalog.explain("project.plan/v1").unwrap();
    assert_eq!(plan["status"], "partial");
    assert_eq!(plan["transport_operation"], "project_plan");
    assert!(catalog.explain("world.construct/v1").unwrap()["transport_operation"].is_null());
    assert!(catalog.explain("not-a-real-operation/v1").is_err());
}

#[test]
fn forged_facade_descriptors_fail_closed() {
    let mut catalog = SemanticFacadeCatalog::native_v1();
    catalog.operations[0]
        .capability_ids
        .push("not-registered/v1".into());
    catalog.facade_sha256 = catalog.compute_digest().unwrap();
    assert!(
        catalog
            .validate()
            .unwrap_err()
            .0
            .contains("unknown capability")
    );

    let mut catalog = SemanticFacadeCatalog::native_v1();
    catalog.facade_sha256 =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".into();
    assert!(catalog.validate().unwrap_err().0.contains("digest"));
}
