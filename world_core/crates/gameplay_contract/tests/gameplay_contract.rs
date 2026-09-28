use std::collections::BTreeSet;

use serde::Deserialize;
use wge_gameplay_contract::{
    AbilityId, FailureCode, GAMEPLAY_TRACE_SCHEMA, GameOutcome, GameSnapshot, GameplayEffect,
    GameplayTag, InputEvent, MAX_REPLAY_EVENTS, NpcAction, NpcDecision, ObjectiveState,
    ReplayTrace, TargetingRule, run_replay, validate_snapshot, verify_replay,
};

#[derive(Deserialize)]
struct Fixture {
    snapshot: GameSnapshot,
    trace: ReplayTrace,
}

#[derive(Deserialize)]
struct ExpectedReceipt {
    schema_version: String,
    outcome: GameOutcome,
    event_count: usize,
    receipt_sha256: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/gameplay_contract/vertical_slice_v1.json"
    ))
    .expect("known-good gameplay fixture must deserialize")
}

fn assert_code(result: Result<(), wge_gameplay_contract::GameFailure>, expected: FailureCode) {
    let failure = result.expect_err("known-bad input must fail closed");
    assert_eq!(failure.code, expected);
}

#[test]
fn known_good_playthrough_wins_and_receipt_is_byte_stable() {
    let fixture = fixture();
    validate_snapshot(&fixture.snapshot).expect("known-good snapshot passes");

    let first = run_replay(&fixture.snapshot, &fixture.trace).expect("known-good trace completes");
    let second = run_replay(&fixture.snapshot, &fixture.trace).expect("same trace replays");
    assert_eq!(
        wge_gameplay_contract::GAMEPLAY_FIXED_TICK_RATE_HZ,
        30,
        "the contract declares its deterministic fixed simulation rate"
    );
    assert!(
        first
            .body
            .events
            .iter()
            .enumerate()
            .all(|(index, event)| event.tick == index as u64 + 1)
    );
    assert_eq!(first.body.outcome, GameOutcome::Won);
    assert_eq!(first.body.event_count, 9);
    let expected: ExpectedReceipt = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/gameplay_contract/vertical_slice_v1.expected.json"
    ))
    .expect("checked-in gameplay receipt golden is valid JSON");
    assert_eq!(first.body.schema_version, expected.schema_version);
    assert_eq!(first.body.outcome, expected.outcome);
    assert_eq!(first.body.event_count, expected.event_count);
    assert_eq!(first.receipt_sha256, expected.receipt_sha256);
    assert_eq!(first.body.final_state.objective, ObjectiveState::Secured);
    assert_eq!(
        first.body.final_state.entities[&"npc_wyrm".into()].health,
        0
    );
    assert!(first.body.final_state.entities[&"player_ash".into()].health > 0);
    assert!(first.body.final_state.entities[&"player_bram".into()].health > 0);
    assert_eq!(
        first.body.final_state.entities[&"player_ash".into()].energy,
        2
    );
    assert_eq!(
        first.body.final_state.entities[&"player_bram".into()].energy,
        2
    );
    assert!(first.body.events.iter().any(|event| matches!(
        event.transition,
        wge_gameplay_contract::Transition::AbilityActivated {
            applied_damage: 3,
            target_health_after: 3,
            ..
        }
    )));
    assert_eq!(
        first.canonical_bytes().unwrap(),
        second.canonical_bytes().unwrap()
    );
    assert_eq!(first.receipt_sha256, second.receipt_sha256);

    let has_explained_npc_turn = first.body.events.iter().any(|event| {
        matches!(
            event.npc_action,
            NpcAction::Attacked {
                decision: NpcDecision::LowestHealthRatioThenEntityId,
                ..
            }
        )
    });
    assert!(
        has_explained_npc_turn,
        "runtime receipt includes the NPC decision and response"
    );
    let expectation = first.expectation();
    let verified = verify_replay(&fixture.snapshot, &fixture.trace, &expectation)
        .expect("matching replay evidence passes");
    assert_eq!(
        verified.canonical_bytes().unwrap(),
        first.canonical_bytes().unwrap()
    );
}

#[test]
fn silent_ability_is_rejected_and_nonzero_effect_is_accepted() {
    let mut fixture = fixture();
    validate_snapshot(&fixture.snapshot).expect("nonzero known-good ability is accepted");

    fixture.snapshot.ability.effect = GameplayEffect::Damage { amount: 0 };
    assert_code(
        validate_snapshot(&fixture.snapshot),
        FailureCode::SilentAbility,
    );
}

#[test]
fn friendly_target_is_rejected_while_enemy_target_succeeds() {
    let fixture = fixture();
    let out_of_range = ReplayTrace {
        schema_version: fixture.trace.schema_version.clone(),
        events: vec![InputEvent::UseAbility {
            ability: AbilityId::from("arc_bolt"),
            target: "npc_wyrm".into(),
        }],
    };
    let range_failure = run_replay(&fixture.snapshot, &out_of_range)
        .expect_err("a target beyond declared navigation range must fail");
    assert_eq!(range_failure.code, FailureCode::InvalidTarget);

    let mut invalid = fixture.trace.clone();
    invalid.events.truncate(3);
    invalid.events[2] = InputEvent::UseAbility {
        ability: AbilityId::from("arc_bolt"),
        target: "player_bram".into(),
    };
    let failure = run_replay(&fixture.snapshot, &invalid).expect_err("friendly target must fail");
    assert_eq!(failure.code, FailureCode::InvalidTarget);
    assert_eq!(failure.event_index, Some(2));

    let valid = run_replay(&fixture.snapshot, &fixture.trace).expect("enemy target succeeds");
    assert!(valid.body.events.iter().any(|event| {
        matches!(
            event.transition,
            wge_gameplay_contract::Transition::AbilityActivated { .. }
        )
    }));
}

#[test]
fn disconnected_objective_is_rejected_and_connected_objective_is_accepted() {
    let mut fixture = fixture();
    validate_snapshot(&fixture.snapshot).expect("connected objective passes reachability gate");

    fixture
        .snapshot
        .navigation
        .adjacency
        .get_mut(&"arena".into())
        .unwrap()
        .remove(&"relay".into());
    fixture
        .snapshot
        .navigation
        .adjacency
        .get_mut(&"relay".into())
        .unwrap()
        .remove(&"arena".into());
    assert_code(
        validate_snapshot(&fixture.snapshot),
        FailureCode::UnreachableObjective,
    );
}

#[test]
fn early_objective_interaction_fails_but_completed_playthrough_secures_it() {
    let fixture = fixture();
    let mut early = fixture.trace.clone();
    early.events = vec![
        InputEvent::Move {
            destination: "bridge".into(),
        },
        InputEvent::Move {
            destination: "arena".into(),
        },
        InputEvent::Move {
            destination: "relay".into(),
        },
        InputEvent::InteractObjective,
    ];
    let failure = run_replay(&fixture.snapshot, &early)
        .expect_err("guarded objective cannot be secured early");
    assert_eq!(failure.code, FailureCode::ObjectivePrerequisiteUnmet);
    assert_eq!(failure.event_index, Some(3));

    let completed = run_replay(&fixture.snapshot, &fixture.trace)
        .expect("the known-good trace defeats the NPC before interaction");
    assert_eq!(completed.body.outcome, GameOutcome::Won);
    assert_eq!(
        completed.body.final_state.objective,
        ObjectiveState::Secured
    );
}

#[test]
fn ability_cost_and_cooldown_are_enforced_with_a_ready_tick_control() {
    let fixture = fixture();
    let mut too_soon = fixture.trace.clone();
    too_soon.events = vec![
        InputEvent::Move {
            destination: "bridge".into(),
        },
        InputEvent::Move {
            destination: "arena".into(),
        },
        InputEvent::UseAbility {
            ability: AbilityId::from("arc_bolt"),
            target: "npc_wyrm".into(),
        },
        InputEvent::UseAbility {
            ability: AbilityId::from("arc_bolt"),
            target: "npc_wyrm".into(),
        },
    ];
    let cooldown_failure = run_replay(&fixture.snapshot, &too_soon)
        .expect_err("recast before ready tick must be rejected");
    assert_eq!(cooldown_failure.code, FailureCode::AbilityCooldownActive);
    assert_eq!(cooldown_failure.event_index, Some(3));

    let mut ready = too_soon.clone();
    ready.events.insert(3, InputEvent::Wait);
    let ready_receipt =
        run_replay(&fixture.snapshot, &ready).expect("recast at ready tick succeeds");
    let activations = ready_receipt
        .body
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.transition,
                wge_gameplay_contract::Transition::AbilityActivated { .. }
            )
        })
        .count();
    assert_eq!(activations, 2);

    let mut insufficient = fixture.snapshot.clone();
    insufficient
        .entities
        .get_mut(&"player_ash".into())
        .unwrap()
        .attributes
        .energy = 1;
    validate_snapshot(&insufficient)
        .expect("low current energy is a runtime state, not malformed data");
    let mut spend_trace = too_soon;
    spend_trace.events.truncate(4);
    spend_trace.events[0] = InputEvent::Move {
        destination: "bridge".into(),
    };
    spend_trace.events[1] = InputEvent::Move {
        destination: "arena".into(),
    };
    spend_trace.events.truncate(3);
    let energy_failure = run_replay(&insufficient, &spend_trace)
        .expect_err("ability cost above current energy must be rejected");
    assert_eq!(energy_failure.code, FailureCode::InsufficientEnergy);
    assert_eq!(energy_failure.event_index, Some(2));
}

#[test]
fn npc_can_defeat_both_playable_entities_and_produce_a_loss() {
    let fixture = fixture();
    let mut trace = ReplayTrace {
        schema_version: fixture.trace.schema_version,
        events: vec![
            InputEvent::Move {
                destination: "bridge".into(),
            },
            InputEvent::Move {
                destination: "arena".into(),
            },
        ],
    };
    trace.events.extend((0..5).map(|_| InputEvent::Wait));
    trace.events.push(InputEvent::SelectEntity {
        entity: "player_bram".into(),
    });
    trace.events.push(InputEvent::Move {
        destination: "bridge".into(),
    });
    trace.events.push(InputEvent::Move {
        destination: "arena".into(),
    });
    trace.events.extend((0..5).map(|_| InputEvent::Wait));

    let receipt = run_replay(&fixture.snapshot, &trace).expect("loss is a valid terminal outcome");
    let replayed = run_replay(&fixture.snapshot, &trace).expect("loss replay is deterministic");
    assert_eq!(receipt.body.outcome, GameOutcome::Lost);
    assert_eq!(
        receipt.canonical_bytes().unwrap(),
        replayed.canonical_bytes().unwrap()
    );
    assert_eq!(receipt.body.final_state.tick, trace.events.len() as u64);
    assert_eq!(
        receipt.body.events.last().unwrap().resulting_state_sha256,
        receipt.body.final_state_sha256
    );
    assert!(
        receipt
            .body
            .final_state
            .entities
            .values()
            .filter(|entity| matches!(entity.control, wge_gameplay_contract::Control::Playable))
            .all(|entity| entity.health == 0)
    );
}

#[test]
fn replay_expectation_rejects_trace_divergence_and_accepts_exact_replay() {
    let fixture = fixture();
    let receipt = run_replay(&fixture.snapshot, &fixture.trace).expect("golden trace runs");
    let expectation = receipt.expectation();
    verify_replay(&fixture.snapshot, &fixture.trace, &expectation)
        .expect("same snapshot and trace match their receipt");

    let mut changed = fixture.trace.clone();
    changed.events.insert(
        0,
        InputEvent::SelectEntity {
            entity: "player_bram".into(),
        },
    );
    let divergence = verify_replay(&fixture.snapshot, &changed, &expectation)
        .expect_err("different trace must not reuse previous evidence");
    assert_eq!(divergence.code, FailureCode::ReplayDiverged);
    assert_eq!(divergence.subject.as_deref(), Some("TraceSha256"));
}

#[test]
fn damage_targeting_and_entity_tags_are_closed_and_inspectable() {
    let fixture = fixture();
    assert_eq!(
        fixture.snapshot.ability.targeting,
        TargetingRule::EnemyWithinNavigationSteps { max_steps: 0 }
    );
    assert!(
        fixture.snapshot.entities[&"player_ash".into()]
            .tags
            .contains(&GameplayTag::Scout)
    );

    let mut invalid = fixture.snapshot.clone();
    invalid
        .entities
        .get_mut(&"player_ash".into())
        .unwrap()
        .tags
        .insert(GameplayTag::Enemy);
    assert_code(validate_snapshot(&invalid), FailureCode::InvalidEntityTags);
}

#[test]
fn unknown_wire_fields_fail_closed() {
    let mut value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/gameplay_contract/vertical_slice_v1.json"
    ))
    .unwrap();
    value["trace"]["events"][0]["teleport"] = serde_json::Value::Bool(true);
    assert!(serde_json::from_value::<Fixture>(value).is_err());
}

#[test]
fn runtime_tags_record_alive_and_defeated_transitions() {
    let fixture = fixture();
    let receipt = run_replay(&fixture.snapshot, &fixture.trace).unwrap();
    let enemy = &receipt.body.final_state.entities[&"npc_wyrm".into()];
    assert!(!enemy.tags.contains(&GameplayTag::Alive));
    assert!(enemy.tags.contains(&GameplayTag::Defeated));
}

#[test]
fn deterministic_npc_targeting_prefers_lowest_health_ratio() {
    let fixture = fixture();
    let mut trace = fixture.trace.clone();
    trace.events.truncate(3);
    let receipt = run_replay(&fixture.snapshot, &trace).unwrap();
    let attacked = receipt.body.events.last().unwrap();
    match &attacked.npc_action {
        NpcAction::Attacked {
            target, decision, ..
        } => {
            assert_eq!(target.as_str(), "player_ash");
            assert_eq!(*decision, NpcDecision::LowestHealthRatioThenEntityId);
        }
        action => panic!("expected the guard to act, observed {action:?}"),
    }
}

#[test]
fn receipt_contains_content_digests_for_snapshot_trace_and_final_state() {
    let fixture = fixture();
    let receipt = run_replay(&fixture.snapshot, &fixture.trace).unwrap();
    for digest in [
        &receipt.body.snapshot_sha256,
        &receipt.body.trace_sha256,
        &receipt.body.final_state_sha256,
        &receipt.receipt_sha256,
    ] {
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), 71);
    }
}

#[test]
fn schema_navigation_and_encounter_resource_gates_have_good_and_bad_controls() {
    let fixture = fixture();
    validate_snapshot(&fixture.snapshot).expect("known-good schema, graph, and resources pass");

    let mut unsupported = fixture.snapshot.clone();
    unsupported.schema_version = "wge.gameplay-snapshot/v99".to_owned();
    assert_code(
        validate_snapshot(&unsupported),
        FailureCode::UnsupportedSchema,
    );

    let mut no_cost = fixture.snapshot.clone();
    no_cost.ability.energy_cost = 0;
    assert_code(validate_snapshot(&no_cost), FailureCode::InvalidAbilityCost);
    let mut no_cooldown = fixture.snapshot.clone();
    no_cooldown.ability.cooldown_ticks = 0;
    assert_code(
        validate_snapshot(&no_cooldown),
        FailureCode::InvalidCooldown,
    );

    let mut asymmetric = fixture.snapshot.clone();
    asymmetric
        .navigation
        .adjacency
        .get_mut(&"arena".into())
        .unwrap()
        .remove(&"bridge".into());
    assert_code(
        validate_snapshot(&asymmetric),
        FailureCode::InvalidNavigationGraph,
    );

    let mut unwinnable = fixture.snapshot;
    unwinnable
        .entities
        .get_mut(&"player_ash".into())
        .unwrap()
        .attributes
        .energy = 0;
    unwinnable
        .entities
        .get_mut(&"player_bram".into())
        .unwrap()
        .attributes
        .energy = 0;
    assert_code(
        validate_snapshot(&unwinnable),
        FailureCode::InsufficientEncounterResources,
    );
}

#[test]
fn objective_and_movement_inputs_fail_when_the_contract_is_not_satisfied() {
    let fixture = fixture();
    let mut non_adjacent = fixture.trace.clone();
    non_adjacent.events[0] = InputEvent::Move {
        destination: "relay".into(),
    };
    let movement_failure = run_replay(&fixture.snapshot, &non_adjacent)
        .expect_err("movement cannot skip intermediate navigation nodes");
    assert_eq!(movement_failure.code, FailureCode::MoveNotAdjacent);

    let mut wrong_place = fixture.trace.clone();
    wrong_place.events = vec![InputEvent::InteractObjective];
    let objective_failure = run_replay(&fixture.snapshot, &wrong_place)
        .expect_err("objective cannot be used from spawn");
    assert_eq!(objective_failure.code, FailureCode::ObjectiveNotAtLocation);

    let mut trailing_event = fixture.trace.clone();
    trailing_event.events.push(InputEvent::Wait);
    let terminal_failure = run_replay(&fixture.snapshot, &trailing_event)
        .expect_err("a terminal win cannot be followed by more gameplay input");
    assert_eq!(terminal_failure.code, FailureCode::GameAlreadyFinished);
    assert_eq!(terminal_failure.event_index, Some(9));
}

#[test]
fn disconnected_enemy_is_rejected_even_when_objective_route_is_connected() {
    let mut fixture = fixture();
    validate_snapshot(&fixture.snapshot).expect("known-good NPC route passes");
    fixture
        .snapshot
        .navigation
        .adjacency
        .insert("crypt".into(), BTreeSet::new());
    fixture
        .snapshot
        .entities
        .get_mut(&"npc_wyrm".into())
        .unwrap()
        .location = "crypt".into();
    assert_code(
        validate_snapshot(&fixture.snapshot),
        FailureCode::UnreachableNpc,
    );
}

#[test]
fn snapshot_role_identity_location_attribute_and_objective_gates_have_controls() {
    let fixture = fixture();
    validate_snapshot(&fixture.snapshot).expect("known-good snapshot passes all shape gates");

    let mut missing_player = fixture.snapshot.clone();
    missing_player.entities.remove(&"player_bram".into());
    assert_code(
        validate_snapshot(&missing_player),
        FailureCode::PlayableEntityCount,
    );

    let mut missing_npc = fixture.snapshot.clone();
    missing_npc.entities.remove(&"npc_wyrm".into());
    assert_code(validate_snapshot(&missing_npc), FailureCode::NpcCount);

    let mut invalid_id = fixture.snapshot.clone();
    let player = invalid_id.entities.remove(&"player_bram".into()).unwrap();
    invalid_id.entities.insert("player/bram".into(), player);
    assert_code(
        validate_snapshot(&invalid_id),
        FailureCode::InvalidIdentifier,
    );

    let mut unknown_location = fixture.snapshot.clone();
    unknown_location
        .entities
        .get_mut(&"player_ash".into())
        .unwrap()
        .location = "void".into();
    assert_code(
        validate_snapshot(&unknown_location),
        FailureCode::UnknownLocation,
    );

    let mut invalid_health = fixture.snapshot.clone();
    invalid_health
        .entities
        .get_mut(&"player_ash".into())
        .unwrap()
        .attributes
        .health = 0;
    assert_code(
        validate_snapshot(&invalid_health),
        FailureCode::InvalidAttributes,
    );

    let mut invalid_objective = fixture.snapshot;
    invalid_objective.objective.tags.clear();
    assert_code(
        validate_snapshot(&invalid_objective),
        FailureCode::InvalidObjective,
    );
}

#[test]
fn unknown_ability_and_npc_player_input_are_rejected() {
    let fixture = fixture();
    let mut unknown_ability = fixture.trace.clone();
    unknown_ability.events.truncate(3);
    unknown_ability.events[2] = InputEvent::UseAbility {
        ability: AbilityId::from("undeclared_ability"),
        target: "npc_wyrm".into(),
    };
    let ability_failure = run_replay(&fixture.snapshot, &unknown_ability)
        .expect_err("only snapshot-declared abilities may execute");
    assert_eq!(ability_failure.code, FailureCode::UnknownAbility);

    let mut npc_input = fixture.trace.clone();
    npc_input.events = vec![InputEvent::SelectEntity {
        entity: "npc_wyrm".into(),
    }];
    let control_failure =
        run_replay(&fixture.snapshot, &npc_input).expect_err("NPCs cannot receive player input");
    assert_eq!(control_failure.code, FailureCode::EntityNotPlayable);
}

#[test]
fn trace_schema_and_size_bounds_reject_bad_inputs_and_accept_the_fixture() {
    let fixture = fixture();
    run_replay(&fixture.snapshot, &fixture.trace).expect("known-good bounded trace passes");

    let mut unsupported = fixture.trace.clone();
    unsupported.schema_version = "wge.gameplay-trace/v99".to_owned();
    let schema_failure = run_replay(&fixture.snapshot, &unsupported)
        .expect_err("unsupported trace schema must fail closed");
    assert_eq!(schema_failure.code, FailureCode::UnsupportedSchema);

    let oversized = ReplayTrace {
        schema_version: GAMEPLAY_TRACE_SCHEMA.to_owned(),
        events: vec![InputEvent::Wait; MAX_REPLAY_EVENTS + 1],
    };
    let size_failure = run_replay(&fixture.snapshot, &oversized)
        .expect_err("trace above the declared limit must fail before simulation");
    assert_eq!(size_failure.code, FailureCode::TraceTooLong);
}
