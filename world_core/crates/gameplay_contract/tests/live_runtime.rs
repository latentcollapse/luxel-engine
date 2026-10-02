use serde::Deserialize;
use wge_gameplay_contract::{
    AbilityId, FailureCode, GameOutcome, GameSnapshot, GameplaySession, InputEvent, NpcAction,
    ObjectiveState, ReplayTrace, Transition, run_replay,
};

#[derive(Deserialize)]
struct Fixture {
    snapshot: GameSnapshot,
    trace: ReplayTrace,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/gameplay_contract/vertical_slice_v1.json"
    ))
    .expect("checked-in v1 fixture must deserialize")
}

fn run_session(snapshot: &GameSnapshot, trace: &ReplayTrace) -> GameplaySession {
    let mut session = GameplaySession::new(snapshot).expect("fixture snapshot validates");
    for input in &trace.events {
        let tick = session.next_tick().expect("tick counter remains in range");
        session
            .step(tick, input.clone())
            .expect("fixture trace input succeeds");
    }
    session
}

fn failure_code<T: std::fmt::Debug>(
    result: Result<T, wge_gameplay_contract::GameFailure>,
) -> FailureCode {
    result.expect_err("adversarial operation must fail").code
}

#[test]
fn initialization_requires_a_validated_snapshot_and_failed_steps_are_atomic() {
    let data = fixture();
    let mut invalid_snapshot = data.snapshot.clone();
    invalid_snapshot.schema_version = "wge.gameplay-snapshot/v99".to_owned();
    assert_eq!(
        failure_code(GameplaySession::new(&invalid_snapshot)),
        FailureCode::UnsupportedSchema
    );

    let mut session = GameplaySession::new(&data.snapshot).unwrap();
    let before = session.state().clone();
    assert_eq!(
        failure_code(session.step(
            2,
            InputEvent::Move {
                destination: "bridge".into(),
            }
        )),
        FailureCode::UnexpectedTick
    );
    assert_eq!(*session.state(), before);
    assert!(session.trace().events.is_empty());

    assert_eq!(
        failure_code(session.step(
            1,
            InputEvent::Move {
                destination: "relay".into(),
            }
        )),
        FailureCode::MoveNotAdjacent
    );
    assert_eq!(*session.state(), before);
    assert!(session.trace().events.is_empty());

    let accepted = session
        .step(
            1,
            InputEvent::Move {
                destination: "bridge".into(),
            },
        )
        .unwrap();
    assert_eq!(accepted.tick, 1);
    assert_eq!(session.state().tick, 1);
    assert_eq!(session.trace().events.len(), 1);
}

#[test]
fn ability_targeting_cost_cooldown_damage_and_npc_turn_use_v1_rules() {
    let data = fixture();
    let mut session = GameplaySession::new(&data.snapshot).unwrap();
    let out_of_range = session
        .step(
            1,
            InputEvent::UseAbility {
                ability: AbilityId::from("arc_bolt"),
                target: "npc_wyrm".into(),
            },
        )
        .expect_err("NPC starts outside the zero-step ability range");
    assert_eq!(out_of_range.code, FailureCode::InvalidTarget);
    assert_eq!(session.state().tick, 0);

    session
        .step(
            1,
            InputEvent::Move {
                destination: "bridge".into(),
            },
        )
        .unwrap();
    session
        .step(
            2,
            InputEvent::Move {
                destination: "arena".into(),
            },
        )
        .unwrap();

    let mut underfunded_snapshot = data.snapshot.clone();
    underfunded_snapshot
        .entities
        .get_mut(&"player_ash".into())
        .unwrap()
        .attributes
        .energy = 1;
    let mut underfunded = GameplaySession::new(&underfunded_snapshot).unwrap();
    underfunded
        .step(
            1,
            InputEvent::Move {
                destination: "bridge".into(),
            },
        )
        .unwrap();
    underfunded
        .step(
            2,
            InputEvent::Move {
                destination: "arena".into(),
            },
        )
        .unwrap();
    let underfunded_before = underfunded.state().clone();
    assert_eq!(
        failure_code(underfunded.step(
            3,
            InputEvent::UseAbility {
                ability: AbilityId::from("arc_bolt"),
                target: "npc_wyrm".into(),
            }
        )),
        FailureCode::InsufficientEnergy
    );
    assert_eq!(*underfunded.state(), underfunded_before);

    let activation = session
        .step(
            3,
            InputEvent::UseAbility {
                ability: AbilityId::from("arc_bolt"),
                target: "npc_wyrm".into(),
            },
        )
        .unwrap();
    assert!(matches!(
        activation.transition,
        Transition::AbilityActivated {
            energy_spent: 2,
            requested_damage: 3,
            applied_damage: 3,
            target_health_after: 3,
            ready_at_tick: 5,
            ..
        }
    ));
    assert!(matches!(
        activation.npc_action,
        NpcAction::Attacked {
            requested_damage: 2,
            target_health_after: 8,
            ..
        }
    ));
    assert_eq!(
        session.state().entities[&"player_ash".into()].energy,
        2,
        "successful ability activation pays the declared energy cost"
    );
    let after_activation = session.state().clone();
    assert_eq!(
        failure_code(session.step(
            4,
            InputEvent::UseAbility {
                ability: AbilityId::from("arc_bolt"),
                target: "npc_wyrm".into(),
            }
        )),
        FailureCode::AbilityCooldownActive
    );
    assert_eq!(*session.state(), after_activation);

    session.step(4, InputEvent::Wait).unwrap();
    let finishing_hit = session
        .step(
            5,
            InputEvent::UseAbility {
                ability: AbilityId::from("arc_bolt"),
                target: "npc_wyrm".into(),
            },
        )
        .unwrap();
    assert!(matches!(
        finishing_hit.transition,
        Transition::AbilityActivated {
            applied_damage: 3,
            overkill_damage: 0,
            target_health_after: 0,
            ..
        }
    ));
    assert!(matches!(
        finishing_hit.npc_action,
        NpcAction::NoAction { .. }
    ));
    assert_eq!(session.state().entities[&"npc_wyrm".into()].health, 0);
}

#[test]
fn objective_prerequisite_failure_does_not_advance_and_later_interaction_wins() {
    let data = fixture();
    let mut session = GameplaySession::new(&data.snapshot).unwrap();
    for destination in ["bridge", "arena", "relay"] {
        let tick = session.next_tick().unwrap();
        session
            .step(
                tick,
                InputEvent::Move {
                    destination: destination.into(),
                },
            )
            .unwrap();
    }
    let before = session.state().clone();
    assert_eq!(
        failure_code(session.step(4, InputEvent::InteractObjective)),
        FailureCode::ObjectivePrerequisiteUnmet
    );
    assert_eq!(*session.state(), before);
    assert_eq!(session.trace().events.len(), 3);

    session
        .step(
            4,
            InputEvent::Move {
                destination: "arena".into(),
            },
        )
        .unwrap();
    session
        .step(
            5,
            InputEvent::UseAbility {
                ability: AbilityId::from("arc_bolt"),
                target: "npc_wyrm".into(),
            },
        )
        .unwrap();
    session.step(6, InputEvent::Wait).unwrap();
    session
        .step(
            7,
            InputEvent::UseAbility {
                ability: AbilityId::from("arc_bolt"),
                target: "npc_wyrm".into(),
            },
        )
        .unwrap();
    session
        .step(
            8,
            InputEvent::Move {
                destination: "relay".into(),
            },
        )
        .unwrap();
    let win = session.step(9, InputEvent::InteractObjective).unwrap();
    assert!(matches!(
        win.transition,
        Transition::ObjectiveSecured { .. }
    ));
    assert_eq!(session.state().objective, ObjectiveState::Secured);
    assert_eq!(session.state().outcome, GameOutcome::Won);
}

#[test]
fn terminal_win_and_loss_reject_more_steps() {
    let data = fixture();
    let won = run_session(&data.snapshot, &data.trace);
    assert_eq!(won.state().outcome, GameOutcome::Won);
    let before_win_guard = won.state().clone();
    let mut won = won;
    assert_eq!(
        failure_code(won.step(10, InputEvent::Wait)),
        FailureCode::GameAlreadyFinished
    );
    assert_eq!(*won.state(), before_win_guard);

    let mut loss_trace = ReplayTrace {
        schema_version: data.trace.schema_version,
        events: vec![
            InputEvent::Move {
                destination: "bridge".into(),
            },
            InputEvent::Move {
                destination: "arena".into(),
            },
        ],
    };
    loss_trace.events.extend((0..5).map(|_| InputEvent::Wait));
    loss_trace.events.push(InputEvent::SelectEntity {
        entity: "player_bram".into(),
    });
    loss_trace.events.extend([
        InputEvent::Move {
            destination: "bridge".into(),
        },
        InputEvent::Move {
            destination: "arena".into(),
        },
    ]);
    loss_trace.events.extend((0..5).map(|_| InputEvent::Wait));
    let lost = run_session(&data.snapshot, &loss_trace);
    assert_eq!(lost.state().outcome, GameOutcome::Lost);
    let mut lost = lost;
    assert_eq!(
        failure_code(lost.step(16, InputEvent::Wait)),
        FailureCode::GameAlreadyFinished
    );
}

#[test]
fn checkpoints_are_canonical_digest_bound_and_restored_by_replay() {
    let data = fixture();
    let mut session = GameplaySession::new(&data.snapshot).unwrap();
    for input in data.trace.events.iter().take(5) {
        session
            .step(session.next_tick().unwrap(), input.clone())
            .unwrap();
    }
    let checkpoint = session.checkpoint().unwrap();
    assert_eq!(
        checkpoint.canonical_bytes().unwrap(),
        session.checkpoint().unwrap().canonical_bytes().unwrap()
    );
    let restored = GameplaySession::restore(&data.snapshot, &checkpoint).unwrap();
    assert_eq!(restored.state(), session.state());
    assert_eq!(restored.trace(), session.trace());
    assert_eq!(restored.receipts(), session.receipts());
    assert_eq!(
        restored.checkpoint().unwrap().canonical_bytes().unwrap(),
        checkpoint.canonical_bytes().unwrap()
    );

    let mut tampered_state = checkpoint.clone();
    tampered_state.body.state.tick += 1;
    assert_eq!(
        failure_code(GameplaySession::restore(&data.snapshot, &tampered_state)),
        FailureCode::ReplayDiverged
    );

    let mut stale_snapshot = data.snapshot.clone();
    stale_snapshot
        .entities
        .get_mut(&"player_ash".into())
        .unwrap()
        .attributes
        .energy -= 1;
    assert_eq!(
        failure_code(GameplaySession::restore(&stale_snapshot, &checkpoint)),
        FailureCode::ReplayDiverged
    );

    let mut tampered_digest = checkpoint;
    tampered_digest.snapshot_sha256.push('0');
    assert_eq!(
        failure_code(GameplaySession::restore(&data.snapshot, &tampered_digest)),
        FailureCode::ReplayDiverged
    );
}

#[test]
fn stepped_session_matches_batch_receipts_for_the_same_trace() {
    let data = fixture();
    let mut session = GameplaySession::new(&data.snapshot).unwrap();
    for input in &data.trace.events {
        let tick = session.next_tick().unwrap();
        let stepped = session.step(tick, input.clone()).unwrap();
        assert_eq!(stepped, session.receipts().last().unwrap().clone());
    }

    let batch = run_replay(&data.snapshot, &data.trace).unwrap();
    let compared = session
        .verify_batch_equivalence()
        .expect("incremental state is identical to batch replay");
    assert_eq!(
        compared.canonical_bytes().unwrap(),
        batch.canonical_bytes().unwrap()
    );
    assert_eq!(
        session.receipts(),
        batch.body.events.as_slice(),
        "every incremental event receipt matches v1 batch replay"
    );
    assert_eq!(
        session.state(),
        &batch.body.final_state,
        "final stepped and batch states are identical"
    );
}
