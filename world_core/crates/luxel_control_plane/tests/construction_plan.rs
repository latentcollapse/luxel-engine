use luxel_control_plane::capability_registry::CapabilityCatalog;
use luxel_control_plane::construction_plan::{
    AssetNeed, AssetSourcePolicy, CapabilityNeed, ConstructionPlan, ConstructionPlanDraft,
    ConstructionReadiness, EvidenceRequirement, PlanAssumption, ProviderJob,
    UnsupportedRequirement,
};
use luxel_control_plane::style_profile::{StyleIntent, StylePlan, StyleProfile};

fn empty_style_plan() -> StylePlan {
    let profile = StyleProfile::new(
        "style-profile-construction-test-v1",
        "project-construction-test",
        StyleIntent::default(),
    )
    .unwrap();
    StylePlan::lower(&profile, &CapabilityCatalog::native_v1()).unwrap()
}

fn draft() -> ConstructionPlanDraft {
    ConstructionPlanDraft {
        plan_id: "construction-plan-alpine-v1".into(),
        project_id: "project-construction-test".into(),
        brief: "A short playable alpine citadel traversal slice.".into(),
        design_constraints: vec!["native runtime only".into(), "deterministic rebuild".into()],
        capability_needs: vec![
            CapabilityNeed {
                capability_id: "world.navigation.traversal/v1".into(),
                required: true,
                reason: "player must reach the fortress objective".into(),
            },
            CapabilityNeed {
                capability_id: "gameplay.kit.compose/v1".into(),
                required: true,
                reason: "the slice needs an objective and encounter loop".into(),
            },
        ],
        assets: vec![AssetNeed {
            asset_id: "hero-fortress/v1".into(),
            role: "hero environment landmark".into(),
            source_policy: AssetSourcePolicy::Provided,
            validation_requirements: vec!["source_digest".into(), "collision".into()],
        }],
        provider_jobs: vec![ProviderJob {
            job_id: "fortress-conditioning/v1".into(),
            provider_kind: "blender".into(),
            operation: "prepare_static_asset".into(),
            input_refs: vec!["hero-fortress/v1".into()],
            optional: true,
            runtime_forbidden: true,
        }],
        world_systems: vec!["terrain".into(), "navigation".into(), "spawn".into()],
        gameplay_kits: vec!["objective-traversal/v1".into()],
        assumptions: vec![PlanAssumption {
            assumption_id: "assume-third-person-camera".into(),
            field: "camera.mode".into(),
            value: "third_person".into(),
            confidence_basis_points: 8_000,
            source_refs: vec![],
        }],
        evidence_requirements: vec![EvidenceRequirement {
            evidence_id: "semantic-receipt".into(),
            gate_id: "semantic".into(),
            validator_id: "luxel.validator.semantic-spec/v1".into(),
            artifact_kind: "semantic_intake".into(),
            required: true,
        }],
        unsupported_requirements: vec![],
    }
}

#[test]
fn construction_plan_resolves_a_ready_native_slice_deterministically() {
    let draft = draft();
    let style = empty_style_plan();
    let plan = ConstructionPlan::compile(&draft, &style).unwrap();
    plan.validate(&style).unwrap();
    assert_eq!(plan.readiness, ConstructionReadiness::Ready);
    assert!(plan.blockers.is_empty());
    assert_eq!(plan.plan_sha256, plan.compute_digest().unwrap());
    assert_eq!(plan.resolved_capabilities.len(), 2);
    assert!(plan.provider_jobs.iter().all(|job| job.runtime_forbidden));
}

#[test]
fn construction_plan_keeps_candidate_and_soft_gaps_explicit() {
    let mut draft = draft();
    draft.capability_needs.push(CapabilityNeed {
        capability_id: "graphics.scene.packet/v1".into(),
        required: false,
        reason: "visual packet lowering is desired for the first authored frame".into(),
    });
    draft.unsupported_requirements.push(UnsupportedRequirement {
        field: "environment.reflection_expectation".into(),
        reason: "reflection capability is not certified in this slice".into(),
        hard: false,
    });
    let plan = ConstructionPlan::compile(&draft, &empty_style_plan()).unwrap();
    assert_eq!(plan.readiness, ConstructionReadiness::Partial);
    assert!(
        plan.advisories
            .iter()
            .any(|message| message.contains("not certified"))
    );
    assert!(
        plan.advisories
            .iter()
            .any(|message| message.contains("reflection_expectation"))
    );
}

#[test]
fn construction_plan_blocks_missing_capability_bad_provider_and_hard_gap() {
    let mut draft = draft();
    draft.capability_needs.push(CapabilityNeed {
        capability_id: "character.native-skinning/v1".into(),
        required: true,
        reason: "hero animation is requested".into(),
    });
    draft.provider_jobs[0].runtime_forbidden = false;
    draft.unsupported_requirements.push(UnsupportedRequirement {
        field: "ray_tracing".into(),
        reason: "not part of the demo critical path".into(),
        hard: true,
    });
    let plan = ConstructionPlan::compile(&draft, &empty_style_plan()).unwrap();
    assert_eq!(plan.readiness, ConstructionReadiness::Blocked);
    assert!(
        plan.blockers
            .iter()
            .any(|message| message.contains("unavailable"))
    );
    assert!(
        plan.blockers
            .iter()
            .any(|message| message.contains("runtime"))
    );
    assert!(
        plan.blockers
            .iter()
            .any(|message| message.contains("ray_tracing"))
    );
}

#[test]
fn stale_style_binding_unknown_validator_and_tampered_plan_fail_closed() {
    let style = empty_style_plan();
    let mut plan = ConstructionPlan::compile(&draft(), &style).unwrap();
    plan.plan_sha256 =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".into();
    assert!(plan.validate(&style).is_err());

    let mut invalid = draft();
    invalid.evidence_requirements[0].validator_id = "luxel.validator.unknown/v1".into();
    assert!(
        ConstructionPlan::compile(&invalid, &style)
            .unwrap_err()
            .0
            .contains("unknown validator")
    );

    let mut wrong_gate = draft();
    wrong_gate.evidence_requirements[0].gate_id = "world".into();
    assert!(
        ConstructionPlan::compile(&wrong_gate, &style)
            .unwrap_err()
            .0
            .contains("disagrees")
    );
}

#[test]
fn resealed_stale_resolution_and_validator_binding_still_fail_closed() {
    let style = empty_style_plan();
    let mut plan = ConstructionPlan::compile(&draft(), &style).unwrap();
    plan.resolved_capabilities[0].status =
        luxel_control_plane::construction_plan::ResolvedCapabilityStatus::Experimental;
    plan.plan_sha256 = plan.compute_digest().unwrap();
    assert!(plan.validate(&style).unwrap_err().0.contains("is stale"));

    let mut plan = ConstructionPlan::compile(&draft(), &style).unwrap();
    plan.validator_registry_sha256 =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".into();
    plan.plan_sha256 = plan.compute_digest().unwrap();
    assert!(
        plan.validate(&style)
            .unwrap_err()
            .0
            .contains("stale validator registry")
    );
}
