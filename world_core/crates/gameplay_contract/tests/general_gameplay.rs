use std::collections::{BTreeMap, BTreeSet};

use general::{
    AbilityId, AbilityRequirement, ActiveEffect, CostPayer, EffectOperation, EffectRecipient,
    EffectSpec, EntityControl, EntityId, GENERAL_RECEIPT_SCHEMA, GENERAL_SNAPSHOT_SCHEMA,
    GENERAL_TRACE_SCHEMA, GameplayTag, GeneralAbilitySpec, GeneralFailureCode, GeneralGameSpec,
    GeneralInputEvent, GeneralObjectiveSpec, GeneralTrace, LocationId, NpcPolicy, ObjectiveId,
    ObjectiveRequirement, ObjectiveStatus, ResourceCost, ResourceId, ResourceStateSpec,
    StackingPolicy, TargetingRule, run_general_replay, validate_general_snapshot,
    verify_general_receipt, verify_general_replay,
};
use sha2::Digest;
use luxel_gameplay_contract::general;

fn set(values: &[&str]) -> BTreeSet<GameplayTag> {
    values
        .iter()
        .map(|value| GameplayTag::from(*value))
        .collect()
}

fn resources(values: &[(&str, u32, u32)]) -> BTreeMap<ResourceId, ResourceStateSpec> {
    values
        .iter()
        .map(|(name, current, maximum)| {
            (
                ResourceId::from(*name),
                ResourceStateSpec {
                    current: *current,
                    maximum: *maximum,
                },
            )
        })
        .collect()
}

fn test_spec() -> GeneralGameSpec {
    let gate = LocationId::from("gate");
    let hall = LocationId::from("hall");
    let shrine = LocationId::from("shrine");
    let mut navigation = BTreeMap::new();
    navigation.insert(gate.clone(), BTreeSet::from([hall.clone()]));
    navigation.insert(hall.clone(), BTreeSet::from([gate.clone(), shrine.clone()]));
    navigation.insert(shrine.clone(), BTreeSet::from([hall.clone()]));

    let mut entities = BTreeMap::new();
    entities.insert(
        EntityId::from("player_ash"),
        general::GeneralEntitySpec {
            control: EntityControl::Player,
            location: gate.clone(),
            resources: resources(&[("health", 12, 12), ("mana", 10, 10)]),
            vital_resource: Some(ResourceId::from("health")),
            tags: set(&["player", "caster", "healer"]),
        },
    );
    entities.insert(
        EntityId::from("player_bram"),
        general::GeneralEntitySpec {
            control: EntityControl::Player,
            location: hall.clone(),
            resources: resources(&[("health", 5, 10), ("mana", 4, 4), ("ward", 2, 2)]),
            vital_resource: Some(ResourceId::from("health")),
            tags: set(&["player", "scout"]),
        },
    );
    entities.insert(
        EntityId::from("player_cyra"),
        general::GeneralEntitySpec {
            control: EntityControl::Player,
            location: gate.clone(),
            resources: resources(&[("health", 10, 10), ("mana", 4, 4)]),
            vital_resource: Some(ResourceId::from("health")),
            tags: set(&["player", "support"]),
        },
    );
    entities.insert(
        EntityId::from("npc_beetle"),
        general::GeneralEntitySpec {
            control: EntityControl::Npc {
                policy: NpcPolicy::Passive,
            },
            location: hall.clone(),
            resources: resources(&[("health", 5, 5)]),
            vital_resource: Some(ResourceId::from("health")),
            tags: set(&["hostile", "beast"]),
        },
    );
    entities.insert(
        EntityId::from("npc_warden"),
        general::GeneralEntitySpec {
            control: EntityControl::Npc {
                policy: NpcPolicy::UseAbilities {
                    priority: vec![AbilityId::from("venom")],
                },
            },
            location: shrine.clone(),
            resources: resources(&[("health", 8, 8), ("mana", 4, 4)]),
            vital_resource: Some(ResourceId::from("health")),
            tags: set(&["hostile", "warden"]),
        },
    );

    let hostile_targeting = TargetingRule::Entity {
        required_tags: set(&["hostile"]),
        forbidden_tags: set(&["player"]),
        max_navigation_steps: Some(3),
        allow_self: false,
    };
    let player_targeting = TargetingRule::Entity {
        required_tags: set(&["player"]),
        forbidden_tags: set(&["hostile"]),
        max_navigation_steps: Some(3),
        allow_self: false,
    };
    let mut abilities = BTreeMap::new();
    abilities.insert(
        AbilityId::from("arc_bolt"),
        GeneralAbilitySpec {
            targeting: hostile_targeting,
            costs: vec![ResourceCost {
                payer: CostPayer::Actor,
                resource: ResourceId::from("mana"),
                amount: 2,
            }],
            cooldown_ticks: 2,
            requirements: vec![
                AbilityRequirement::ActorHasTags {
                    tags: set(&["caster"]),
                },
                AbilityRequirement::TargetResourceAtLeast {
                    resource: ResourceId::from("health"),
                    amount: 1,
                },
            ],
            effects: vec![
                EffectSpec {
                    stack_key: "arc_hit".to_owned(),
                    recipient: EffectRecipient::SelectedTarget,
                    operation: EffectOperation::Damage {
                        resource: ResourceId::from("health"),
                        amount: 3,
                    },
                    duration_ticks: 0,
                    period_ticks: None,
                    stacking: StackingPolicy::Replace,
                },
                EffectSpec {
                    stack_key: "scorch".to_owned(),
                    recipient: EffectRecipient::SelectedTarget,
                    operation: EffectOperation::Damage {
                        resource: ResourceId::from("health"),
                        amount: 1,
                    },
                    duration_ticks: 3,
                    period_ticks: Some(1),
                    stacking: StackingPolicy::AddStacks { max_stacks: 2 },
                },
            ],
        },
    );
    abilities.insert(
        AbilityId::from("mend"),
        GeneralAbilitySpec {
            targeting: player_targeting.clone(),
            costs: vec![ResourceCost {
                payer: CostPayer::Actor,
                resource: ResourceId::from("mana"),
                amount: 1,
            }],
            cooldown_ticks: 1,
            requirements: vec![AbilityRequirement::ActorHasTags {
                tags: set(&["healer"]),
            }],
            effects: vec![
                EffectSpec {
                    stack_key: "mend_heal".to_owned(),
                    recipient: EffectRecipient::SelectedTarget,
                    operation: EffectOperation::Heal {
                        resource: ResourceId::from("health"),
                        amount: 4,
                    },
                    duration_ticks: 0,
                    period_ticks: None,
                    stacking: StackingPolicy::Replace,
                },
                EffectSpec {
                    stack_key: "mend_focus".to_owned(),
                    recipient: EffectRecipient::Actor,
                    operation: EffectOperation::ResourceDelta {
                        resource: ResourceId::from("mana"),
                        delta: 1,
                    },
                    duration_ticks: 0,
                    period_ticks: None,
                    stacking: StackingPolicy::Replace,
                },
                EffectSpec {
                    stack_key: "mend_mark".to_owned(),
                    recipient: EffectRecipient::SelectedTarget,
                    operation: EffectOperation::SetTag {
                        tag: GameplayTag::from("mended"),
                        present: true,
                    },
                    duration_ticks: 0,
                    period_ticks: None,
                    stacking: StackingPolicy::Replace,
                },
            ],
        },
    );
    abilities.insert(
        AbilityId::from("venom"),
        GeneralAbilitySpec {
            targeting: player_targeting.clone(),
            costs: vec![ResourceCost {
                payer: CostPayer::Actor,
                resource: ResourceId::from("mana"),
                amount: 1,
            }],
            cooldown_ticks: 3,
            requirements: Vec::new(),
            effects: vec![EffectSpec {
                stack_key: "venom_hit".to_owned(),
                recipient: EffectRecipient::SelectedTarget,
                operation: EffectOperation::Damage {
                    resource: ResourceId::from("health"),
                    amount: 2,
                },
                duration_ticks: 0,
                period_ticks: None,
                stacking: StackingPolicy::Replace,
            }],
        },
    );
    abilities.insert(
        AbilityId::from("battle_cry"),
        GeneralAbilitySpec {
            targeting: TargetingRule::SelfOnly,
            costs: Vec::new(),
            cooldown_ticks: 1,
            requirements: Vec::new(),
            effects: vec![EffectSpec {
                stack_key: "rally_tag".to_owned(),
                recipient: EffectRecipient::AlliesInRange {
                    max_navigation_steps: 1,
                },
                operation: EffectOperation::SetTag {
                    tag: GameplayTag::from("rallied"),
                    present: true,
                },
                duration_ticks: 0,
                period_ticks: None,
                stacking: StackingPolicy::Replace,
            }],
        },
    );
    abilities.insert(
        AbilityId::from("shield_drain"),
        GeneralAbilitySpec {
            targeting: player_targeting,
            costs: Vec::new(),
            cooldown_ticks: 1,
            requirements: Vec::new(),
            effects: vec![EffectSpec {
                stack_key: "ward_drain".to_owned(),
                recipient: EffectRecipient::SelectedTarget,
                operation: EffectOperation::ResourceDelta {
                    resource: ResourceId::from("ward"),
                    delta: -1,
                },
                duration_ticks: 0,
                period_ticks: None,
                stacking: StackingPolicy::Replace,
            }],
        },
    );

    GeneralGameSpec {
        schema_version: GENERAL_SNAPSHOT_SCHEMA.to_owned(),
        navigation,
        entities,
        abilities,
        objectives: BTreeMap::from([
            (
                ObjectiveId::from("relay"),
                GeneralObjectiveSpec {
                    location: hall,
                    required_for_victory: true,
                    prerequisites: Vec::new(),
                },
            ),
            (
                ObjectiveId::from("sanctum"),
                GeneralObjectiveSpec {
                    location: shrine,
                    required_for_victory: true,
                    prerequisites: vec![
                        ObjectiveRequirement::ObjectiveSecured {
                            objective: ObjectiveId::from("relay"),
                        },
                        ObjectiveRequirement::AllEntitiesDefeatedWithTags {
                            tags: set(&["hostile"]),
                        },
                    ],
                },
            ),
        ]),
    }
}

fn support_objective_spec() -> GeneralGameSpec {
    let infirmary = LocationId::from("infirmary");
    let ward = LocationId::from("ward");
    let courtyard = LocationId::from("courtyard");
    let navigation = BTreeMap::from([
        (infirmary.clone(), BTreeSet::from([ward.clone()])),
        (
            ward.clone(),
            BTreeSet::from([infirmary.clone(), courtyard.clone()]),
        ),
        (courtyard.clone(), BTreeSet::from([ward.clone()])),
    ]);

    let entities = BTreeMap::from([
        (
            EntityId::from("medic"),
            general::GeneralEntitySpec {
                control: EntityControl::Player,
                location: infirmary.clone(),
                resources: resources(&[("health", 8, 8), ("charge", 3, 3)]),
                vital_resource: Some(ResourceId::from("health")),
                tags: set(&["player", "medic"]),
            },
        ),
        (
            EntityId::from("patient_ada"),
            general::GeneralEntitySpec {
                control: EntityControl::Player,
                location: infirmary.clone(),
                resources: resources(&[("health", 4, 10)]),
                vital_resource: Some(ResourceId::from("health")),
                tags: set(&["player", "injured"]),
            },
        ),
        (
            EntityId::from("patient_ben"),
            general::GeneralEntitySpec {
                control: EntityControl::Player,
                location: ward.clone(),
                resources: resources(&[("health", 3, 10)]),
                vital_resource: Some(ResourceId::from("health")),
                tags: set(&["player", "injured"]),
            },
        ),
        (
            EntityId::from("patient_cora"),
            general::GeneralEntitySpec {
                control: EntityControl::Player,
                location: courtyard,
                resources: resources(&[("health", 2, 10)]),
                vital_resource: Some(ResourceId::from("health")),
                tags: set(&["player", "injured"]),
            },
        ),
    ]);

    let abilities = BTreeMap::from([(
        AbilityId::from("field_triage"),
        GeneralAbilitySpec {
            targeting: TargetingRule::SelfOnly,
            costs: vec![ResourceCost {
                payer: CostPayer::Actor,
                resource: ResourceId::from("charge"),
                amount: 2,
            }],
            cooldown_ticks: 2,
            requirements: vec![
                AbilityRequirement::ActorHasTags {
                    tags: set(&["medic"]),
                },
                AbilityRequirement::ActorResourceAtLeast {
                    resource: ResourceId::from("charge"),
                    amount: 3,
                },
            ],
            effects: vec![
                EffectSpec {
                    stack_key: "triage_heal".to_owned(),
                    recipient: EffectRecipient::AlliesInRange {
                        max_navigation_steps: 1,
                    },
                    operation: EffectOperation::Heal {
                        resource: ResourceId::from("health"),
                        amount: 4,
                    },
                    duration_ticks: 0,
                    period_ticks: None,
                    stacking: StackingPolicy::Replace,
                },
                EffectSpec {
                    stack_key: "triage_stabilized".to_owned(),
                    recipient: EffectRecipient::AlliesInRange {
                        max_navigation_steps: 1,
                    },
                    operation: EffectOperation::SetTag {
                        tag: GameplayTag::from("stabilized"),
                        present: true,
                    },
                    duration_ticks: 0,
                    period_ticks: None,
                    stacking: StackingPolicy::Replace,
                },
            ],
        },
    )]);

    let objective = GeneralObjectiveSpec {
        location: infirmary,
        required_for_victory: true,
        prerequisites: vec![
            ObjectiveRequirement::EntityHasTag {
                entity: EntityId::from("patient_ada"),
                tag: GameplayTag::from("stabilized"),
            },
            ObjectiveRequirement::ResourceAtLeast {
                entity: EntityId::from("patient_ada"),
                resource: ResourceId::from("health"),
                amount: 7,
            },
            ObjectiveRequirement::EntityHasTag {
                entity: EntityId::from("patient_ben"),
                tag: GameplayTag::from("stabilized"),
            },
            ObjectiveRequirement::ResourceAtLeast {
                entity: EntityId::from("patient_ben"),
                resource: ResourceId::from("health"),
                amount: 6,
            },
        ],
    };

    GeneralGameSpec {
        schema_version: GENERAL_SNAPSHOT_SCHEMA.to_owned(),
        navigation,
        entities,
        abilities,
        objectives: BTreeMap::from([(ObjectiveId::from("triage_station"), objective)]),
    }
}

fn winning_trace() -> GeneralTrace {
    GeneralTrace {
        schema_version: GENERAL_TRACE_SCHEMA.to_owned(),
        events: vec![
            GeneralInputEvent::ActivateAbility {
                ability: AbilityId::from("arc_bolt"),
                target: EntityId::from("npc_beetle"),
            },
            GeneralInputEvent::Wait,
            GeneralInputEvent::ActivateAbility {
                ability: AbilityId::from("arc_bolt"),
                target: EntityId::from("npc_beetle"),
            },
            GeneralInputEvent::Wait,
            GeneralInputEvent::ActivateAbility {
                ability: AbilityId::from("mend"),
                target: EntityId::from("player_bram"),
            },
            GeneralInputEvent::ActivateAbility {
                ability: AbilityId::from("arc_bolt"),
                target: EntityId::from("npc_warden"),
            },
            GeneralInputEvent::Wait,
            GeneralInputEvent::ActivateAbility {
                ability: AbilityId::from("arc_bolt"),
                target: EntityId::from("npc_warden"),
            },
            GeneralInputEvent::Wait,
            GeneralInputEvent::Move {
                destination: LocationId::from("hall"),
            },
            GeneralInputEvent::InteractObjective {
                objective: ObjectiveId::from("relay"),
            },
            GeneralInputEvent::Move {
                destination: LocationId::from("shrine"),
            },
            GeneralInputEvent::InteractObjective {
                objective: ObjectiveId::from("sanctum"),
            },
        ],
    }
}

#[test]
fn generalized_five_entity_scenario_runs_mechanical_abilities_and_objectives_deterministically() {
    let spec = test_spec();
    let trace = winning_trace();
    validate_general_snapshot(&spec).expect("arbitrary entity count and mixed abilities are valid");
    let first = run_general_replay(&spec, &trace).expect("scenario completes");
    let second = run_general_replay(&spec, &trace).expect("scenario replays");

    assert_eq!(first, second);
    assert_eq!(first.body.outcome, general::GeneralOutcome::Won);
    assert_eq!(first.body.schema_version, GENERAL_RECEIPT_SCHEMA);
    assert_eq!(first.body.event_count, trace.events.len());
    assert_eq!(first.body.final_state.entities.len(), 5);
    assert_eq!(
        first.body.final_state.objectives[&ObjectiveId::from("relay")],
        ObjectiveStatus::Secured
    );
    assert_eq!(
        first.body.final_state.objectives[&ObjectiveId::from("sanctum")],
        ObjectiveStatus::Secured
    );
    assert_eq!(
        first.body.final_state.entities[&EntityId::from("npc_beetle")].resources
            [&ResourceId::from("health")]
            .current,
        0
    );
    assert_eq!(
        first.body.final_state.entities[&EntityId::from("npc_warden")].resources
            [&ResourceId::from("health")]
            .current,
        0
    );
    assert!(
        first.body.final_state.entities[&EntityId::from("player_bram")]
            .tags
            .contains(&GameplayTag::from("mended"))
    );
    assert_eq!(
        first.body.final_state.entities[&EntityId::from("player_ash")].resources
            [&ResourceId::from("mana")]
            .current,
        2
    );
    let stacked = match &first.body.events[2].transition {
        general::GeneralTransition::AbilityActivated { effects, .. } => &effects[1],
        transition => panic!("expected an ability activation, got {transition:?}"),
    };
    assert_eq!(stacked.stack_key, "scorch");
    assert_eq!(stacked.stacks, 2);
    assert_eq!(
        first.body.events[1].periodic_effects[0].requested_delta,
        Some(-1)
    );
    assert_eq!(
        first.body.events[8].periodic_effects[0].requested_delta,
        Some(-2)
    );
    assert_eq!(first.body.events[8].periodic_effects[0].stacks, 2);
    assert!(first.body.events.iter().any(|event| {
        event.npc_actions.iter().any(|action| {
            matches!(action, general::NpcAction::UsedAbility { ability, .. } if ability.as_str() == "venom")
        })
    }));
    assert!(first.body.events.iter().any(|event| {
        matches!(
            event.transition,
            general::GeneralTransition::AbilityActivated { ref ability, .. }
                if ability.as_str() == "mend"
        )
    }));
    assert_eq!(
        first.canonical_bytes().unwrap(),
        second.canonical_bytes().unwrap()
    );
    verify_general_receipt(&spec, &trace, &first).expect("full receipt is independently replayed");
    verify_general_replay(&spec, &trace, &first.expectation()).expect("fresh evidence verifies");
}

#[test]
fn resource_gated_area_support_secures_a_distinct_triage_objective() {
    let spec = support_objective_spec();
    let trace = GeneralTrace {
        schema_version: GENERAL_TRACE_SCHEMA.to_owned(),
        events: vec![
            GeneralInputEvent::SelectEntity {
                entity: EntityId::from("medic"),
            },
            GeneralInputEvent::ActivateAbility {
                ability: AbilityId::from("field_triage"),
                target: EntityId::from("medic"),
            },
            GeneralInputEvent::InteractObjective {
                objective: ObjectiveId::from("triage_station"),
            },
        ],
    };

    validate_general_snapshot(&spec).expect("support scenario obeys bounded contract");
    let receipt = run_general_replay(&spec, &trace).expect("triage scenario completes");
    assert_eq!(receipt.body.outcome, general::GeneralOutcome::Won);
    assert_eq!(receipt.body.event_count, 3);
    assert_eq!(
        receipt.body.final_state.objectives[&ObjectiveId::from("triage_station")],
        ObjectiveStatus::Secured
    );

    let state = &receipt.body.final_state;
    assert_eq!(
        state.entities[&EntityId::from("medic")].resources[&ResourceId::from("charge")].current,
        1
    );
    assert_eq!(
        state.entities[&EntityId::from("patient_ada")].resources[&ResourceId::from("health")]
            .current,
        8
    );
    assert_eq!(
        state.entities[&EntityId::from("patient_ben")].resources[&ResourceId::from("health")]
            .current,
        7
    );
    for patient in ["patient_ada", "patient_ben"] {
        assert!(
            state.entities[&EntityId::from(patient)]
                .tags
                .contains(&GameplayTag::from("stabilized"))
        );
    }
    let outside = &state.entities[&EntityId::from("patient_cora")];
    assert_eq!(outside.resources[&ResourceId::from("health")].current, 2);
    assert!(!outside.tags.contains(&GameplayTag::from("stabilized")));

    let activation = match &receipt.body.events[1].transition {
        general::GeneralTransition::AbilityActivated {
            costs_paid,
            effects,
            ..
        } => {
            assert_eq!(costs_paid[0].amount, 2);
            effects
        }
        transition => panic!("expected support activation, got {transition:?}"),
    };
    assert_eq!(activation.len(), 6);
    verify_general_receipt(&spec, &trace, &receipt)
        .expect("support receipt is independently replayed");

    let mut undercharged = spec;
    undercharged
        .entities
        .get_mut(&EntityId::from("medic"))
        .unwrap()
        .resources
        .get_mut(&ResourceId::from("charge"))
        .unwrap()
        .current = 2;
    let denied = run_general_replay(&undercharged, &trace).unwrap_err();
    assert_eq!(denied.code, GeneralFailureCode::RequirementUnmet);
    assert_eq!(denied.event_index, Some(1));
    assert_eq!(denied.subject.as_deref(), Some("field_triage"));
}

#[test]
fn self_targeting_and_area_recipients_apply_tags_to_the_declared_team_and_range() {
    let trace = GeneralTrace {
        schema_version: GENERAL_TRACE_SCHEMA.to_owned(),
        events: vec![GeneralInputEvent::ActivateAbility {
            ability: AbilityId::from("battle_cry"),
            target: EntityId::from("player_ash"),
        }],
    };
    let receipt = run_general_replay(&test_spec(), &trace).unwrap();
    for player in ["player_ash", "player_bram", "player_cyra"] {
        assert!(
            receipt.body.final_state.entities[&EntityId::from(player)]
                .tags
                .contains(&GameplayTag::from("rallied"))
        );
    }
    assert!(
        !receipt.body.final_state.entities[&EntityId::from("npc_warden")]
            .tags
            .contains(&GameplayTag::from("rallied"))
    );
}

#[test]
fn bad_controls_reject_invalid_stack_bounds_requirements_targets_costs_and_cooldowns() {
    let mut invalid_stack = test_spec();
    invalid_stack
        .abilities
        .get_mut(&AbilityId::from("arc_bolt"))
        .unwrap()
        .effects[1]
        .stacking = StackingPolicy::AddStacks { max_stacks: 0 };
    assert_eq!(
        validate_general_snapshot(&invalid_stack).unwrap_err().code,
        GeneralFailureCode::InvalidEffect
    );

    let mut missing_cost_resource = test_spec();
    missing_cost_resource
        .entities
        .get_mut(&EntityId::from("player_bram"))
        .unwrap()
        .resources
        .remove(&ResourceId::from("mana"));
    validate_general_snapshot(&missing_cost_resource)
        .expect("entities may expose different resource sets");

    let mut unavailable_effect_target = GeneralTrace {
        schema_version: GENERAL_TRACE_SCHEMA.to_owned(),
        events: vec![GeneralInputEvent::ActivateAbility {
            ability: AbilityId::from("shield_drain"),
            target: EntityId::from("player_cyra"),
        }],
    };
    let unavailable_resource =
        run_general_replay(&test_spec(), &unavailable_effect_target).unwrap_err();
    assert_eq!(
        unavailable_resource.code,
        GeneralFailureCode::InvalidResource
    );
    unavailable_effect_target.events[0] = GeneralInputEvent::ActivateAbility {
        ability: AbilityId::from("shield_drain"),
        target: EntityId::from("player_bram"),
    };
    let declared_resource = run_general_replay(&test_spec(), &unavailable_effect_target)
        .expect("ability succeeds against an entity that owns its ward resource");
    assert_eq!(
        declared_resource.body.final_state.entities[&EntityId::from("player_bram")].resources
            [&ResourceId::from("ward")]
            .current,
        1
    );

    let mut trace = winning_trace();
    trace.events[0] = GeneralInputEvent::ActivateAbility {
        ability: AbilityId::from("arc_bolt"),
        target: EntityId::from("player_bram"),
    };
    let friendly_target = run_general_replay(&test_spec(), &trace).unwrap_err();
    assert_eq!(friendly_target.code, GeneralFailureCode::InvalidTarget);
    assert_eq!(friendly_target.event_index, Some(0));

    let mut trace = winning_trace();
    trace.events[1] = GeneralInputEvent::ActivateAbility {
        ability: AbilityId::from("arc_bolt"),
        target: EntityId::from("npc_beetle"),
    };
    let cooldown = run_general_replay(&test_spec(), &trace).unwrap_err();
    assert_eq!(cooldown.code, GeneralFailureCode::AbilityCooldownActive);
    assert_eq!(cooldown.event_index, Some(1));

    let mut spec = test_spec();
    spec.entities
        .get_mut(&EntityId::from("player_ash"))
        .unwrap()
        .tags
        .remove(&GameplayTag::from("healer"));
    let mut trace = winning_trace();
    trace.events.truncate(5);
    let requirement = run_general_replay(&spec, &trace).unwrap_err();
    assert_eq!(requirement.code, GeneralFailureCode::RequirementUnmet);
    assert_eq!(requirement.event_index, Some(4));

    let mut spec = test_spec();
    spec.entities
        .get_mut(&EntityId::from("player_ash"))
        .unwrap()
        .resources
        .get_mut(&ResourceId::from("mana"))
        .unwrap()
        .current = 0;
    let trace = GeneralTrace {
        schema_version: GENERAL_TRACE_SCHEMA.to_owned(),
        events: vec![GeneralInputEvent::ActivateAbility {
            ability: AbilityId::from("arc_bolt"),
            target: EntityId::from("npc_beetle"),
        }],
    };
    let insufficient = run_general_replay(&spec, &trace).unwrap_err();
    assert_eq!(insufficient.code, GeneralFailureCode::InsufficientResource);
    assert_eq!(insufficient.event_index, Some(0));
}

#[test]
fn stale_replay_evidence_is_rejected_after_snapshot_or_trace_changes() {
    let spec = test_spec();
    let trace = winning_trace();
    let receipt = run_general_replay(&spec, &trace).unwrap();
    let expected = receipt.expectation();

    let mut changed_trace = trace.clone();
    changed_trace.events[0] = GeneralInputEvent::Wait;
    let stale_trace = verify_general_replay(&spec, &changed_trace, &expected).unwrap_err();
    assert_eq!(stale_trace.code, GeneralFailureCode::ReplayDiverged);
    assert_eq!(stale_trace.subject.as_deref(), Some("trace_sha256"));

    let mut changed_spec = spec.clone();
    changed_spec
        .entities
        .get_mut(&EntityId::from("player_ash"))
        .unwrap()
        .tags
        .insert(GameplayTag::from("new_tag"));
    let stale_snapshot = verify_general_replay(&changed_spec, &trace, &expected).unwrap_err();
    assert_eq!(stale_snapshot.code, GeneralFailureCode::ReplayDiverged);
    assert_eq!(stale_snapshot.subject.as_deref(), Some("snapshot_sha256"));
}

#[test]
fn malformed_unknown_fields_and_unsupported_versions_fail_closed() {
    let mut value: serde_json::Value = serde_json::to_value(winning_trace()).unwrap();
    value["events"][0]["untrusted_passthrough"] = serde_json::Value::Bool(true);
    assert!(serde_json::from_value::<GeneralTrace>(value).is_err());

    let mut trace = winning_trace();
    trace.schema_version = "luxel.gameplay-trace/v99".to_owned();
    let unsupported = run_general_replay(&test_spec(), &trace).unwrap_err();
    assert_eq!(unsupported.code, GeneralFailureCode::UnsupportedSchema);

    let mut spec = test_spec();
    spec.navigation
        .get_mut(&LocationId::from("gate"))
        .unwrap()
        .clear();
    let malformed_graph = validate_general_snapshot(&spec).unwrap_err();
    assert_eq!(malformed_graph.code, GeneralFailureCode::InvalidNavigation);

    let mut too_many = test_spec();
    too_many.entities = (0..=general::GENERAL_MAX_ENTITIES)
        .map(|index| {
            (
                EntityId::new(format!("entity_{index}")),
                general::GeneralEntitySpec {
                    control: EntityControl::Player,
                    location: LocationId::from("gate"),
                    resources: BTreeMap::new(),
                    vital_resource: None,
                    tags: BTreeSet::new(),
                },
            )
        })
        .collect();
    assert_eq!(
        validate_general_snapshot(&too_many).unwrap_err().code,
        GeneralFailureCode::LimitExceeded
    );
}

#[test]
fn forged_receipts_fail_even_when_the_claimant_rehashes_the_forgery() {
    let spec = test_spec();
    let trace = winning_trace();
    let mut forged = run_general_replay(&spec, &trace).unwrap();
    forged.body.events[0].resulting_state_sha256 = "00".repeat(32);
    let bytes = serde_json::to_vec(&forged.body).unwrap();
    forged.receipt_sha256 = format!("{:x}", sha2::Sha256::digest(bytes));

    let error = verify_general_receipt(&spec, &trace, &forged).unwrap_err();
    assert_eq!(error.code, GeneralFailureCode::ReplayDiverged);
    let rehashed_expectation = forged.expectation();
    let error = verify_general_replay(&spec, &trace, &rehashed_expectation).unwrap_err();
    assert_eq!(error.code, GeneralFailureCode::ReplayDiverged);
}

#[test]
fn objective_dependencies_must_be_well_formed_and_acyclic() {
    let mut cyclic = test_spec();
    cyclic
        .objectives
        .get_mut(&ObjectiveId::from("relay"))
        .unwrap()
        .prerequisites = vec![ObjectiveRequirement::ObjectiveSecured {
        objective: ObjectiveId::from("sanctum"),
    }];
    assert_eq!(
        validate_general_snapshot(&cyclic).unwrap_err().code,
        GeneralFailureCode::InvalidObjective
    );
}

#[test]
fn serialized_runtime_exposes_effect_duration_stack_and_entity_control_provenance() {
    let receipt = run_general_replay(&test_spec(), &winning_trace()).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&receipt.canonical_bytes().unwrap()).unwrap();
    assert_eq!(value["body"]["final_state"]["tick"], 13);
    assert!(value["body"]["final_state"]["active_effects"].is_object());
    assert!(value["body"]["events"][0]["npc_actions"].is_array());
    let _typed_effects: Option<&ActiveEffect> = receipt
        .body
        .final_state
        .active_effects
        .values()
        .flat_map(BTreeMap::values)
        .next();
}
