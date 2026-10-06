use luxel_control_plane::capability_registry::CapabilityCatalog;
use luxel_control_plane::style_profile::{
    CameraPolicy, EnvironmentPolicy, GeometryPolicy, LightingPolicy, StyleBudgets, StyleClaim,
    StyleConflict, StyleIntent, StylePlan, StylePlanStatus, StyleProfile, StyleProvenance,
    StyleRegion, StyleSignal, StyleSourceKind, SurfacePolicy,
};

fn known_profile() -> StyleProfile {
    let intent = StyleIntent {
        geometry: GeometryPolicy {
            silhouette_complexity: Some(StyleSignal::Scalar {
                basis_points: 7_200,
                source_kind: StyleSourceKind::Observed,
                source_refs: vec!["region-hero".into()],
                confidence_basis_points: 9_000,
            }),
            ..GeometryPolicy::default()
        },
        surfaces: SurfacePolicy {
            physically_based: Some(StyleSignal::Boolean {
                value: true,
                source_kind: StyleSourceKind::Requested,
                source_refs: Vec::new(),
                confidence_basis_points: 8_500,
            }),
            wetness: Some(StyleSignal::Scalar {
                basis_points: 6_500,
                source_kind: StyleSourceKind::Inferred,
                source_refs: vec!["concept-art-1".into()],
                confidence_basis_points: 4_500,
            }),
            ..SurfacePolicy::default()
        },
        lighting: LightingPolicy {
            shadow_softness: Some(StyleSignal::Scalar {
                basis_points: 5_000,
                source_kind: StyleSourceKind::Requested,
                source_refs: Vec::new(),
                confidence_basis_points: 8_000,
            }),
            ..LightingPolicy::default()
        },
        environment: EnvironmentPolicy {
            reflection_expectation: Some(StyleSignal::Category {
                value: "high".into(),
                source_kind: StyleSourceKind::Requested,
                source_refs: Vec::new(),
                confidence_basis_points: 8_000,
            }),
            ..EnvironmentPolicy::default()
        },
        camera: CameraPolicy::default(),
        budgets: StyleBudgets {
            target_width_px: Some(1920),
            target_height_px: Some(1080),
            max_frame_us: Some(16_667),
            ..StyleBudgets::default()
        },
        ..StyleIntent::default()
    };
    let mut profile =
        StyleProfile::new("style-profile-alpine-v1", "project-alpine", intent).unwrap();
    profile.provenance.push(StyleProvenance {
        artifact_id: "concept-art-1".into(),
        sha256: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        locator: "concept/alpine-citadel.png".into(),
        extraction_method: "human-reviewed-region-pass".into(),
    });
    profile.regions.push(StyleRegion {
        id: "region-hero".into(),
        artifact_id: "concept-art-1".into(),
        x: 100,
        y: 80,
        width: 640,
        height: 480,
    });
    profile.observations.push(StyleClaim {
        id: "observation-hero-silhouette".into(),
        field: "geometry.silhouette_complexity".into(),
        value: "dense hero contour".into(),
        source_refs: vec!["region-hero".into()],
        confidence_basis_points: 9_000,
    });
    profile.inferences.push(StyleClaim {
        id: "inference-wet-stone".into(),
        field: "surfaces.wetness".into(),
        value: "medium-high".into(),
        source_refs: vec!["concept-art-1".into()],
        confidence_basis_points: 4_500,
    });
    profile.reseal().unwrap();
    profile
}

#[test]
fn profile_and_plan_are_typed_deterministic_and_explicit_about_gaps() {
    let profile = known_profile();
    profile.validate().unwrap();
    assert_eq!(profile.profile_sha256, profile.compute_digest().unwrap());

    let catalog = CapabilityCatalog::native_v1();
    let plan = StylePlan::lower(&profile, &catalog).unwrap();
    plan.validate(&profile, &catalog).unwrap();
    assert_eq!(plan.status, StylePlanStatus::Partial);
    assert!(
        plan.unsupported_axes
            .iter()
            .any(|axis| axis.field == "environment.reflection_expectation")
    );
    assert!(
        plan.indeterminate_axes
            .iter()
            .any(|axis| axis == "surfaces.wetness")
    );
    assert!(
        plan.lowered_policies
            .iter()
            .any(|policy| policy.field == "geometry.silhouette_complexity")
    );
    assert_eq!(plan.plan_sha256, plan.compute_digest().unwrap());
}

#[test]
fn profile_rejects_bad_epistemics_bounds_and_digest_tampering() {
    let mut profile = known_profile();
    profile.profile_sha256 =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".into();
    assert!(profile.validate().is_err());

    let mut invalid = known_profile();
    invalid.intent.geometry.silhouette_complexity = Some(StyleSignal::Scalar {
        basis_points: 10_001,
        source_kind: StyleSourceKind::Observed,
        source_refs: vec!["region-hero".into()],
        confidence_basis_points: 9_000,
    });
    assert!(invalid.reseal().unwrap_err().0.contains("outside"));

    let mut unknown_source = known_profile();
    unknown_source.intent.geometry.silhouette_complexity = Some(StyleSignal::Scalar {
        basis_points: 7_200,
        source_kind: StyleSourceKind::Observed,
        source_refs: vec!["missing-region".into()],
        confidence_basis_points: 9_000,
    });
    assert!(
        unknown_source
            .reseal()
            .unwrap_err()
            .0
            .contains("unknown source")
    );
}

#[test]
fn unresolved_conflicts_survive_lowering_as_indeterminate_evidence() {
    let mut profile = known_profile();
    profile.conflicts.push(StyleConflict {
        id: "conflict-lighting-reference".into(),
        fields: vec![
            "lighting.shadow_softness".into(),
            "environment.fog_density".into(),
        ],
        detail: "references disagree about the key light softness".into(),
        resolved: false,
    });
    profile.reseal().unwrap();

    let catalog = CapabilityCatalog::native_v1();
    let plan = StylePlan::lower(&profile, &catalog).unwrap();
    assert_eq!(plan.status, StylePlanStatus::Partial);
    assert!(
        plan.indeterminate_axes
            .iter()
            .any(|axis| axis == "conflict:conflict-lighting-reference")
    );
}
