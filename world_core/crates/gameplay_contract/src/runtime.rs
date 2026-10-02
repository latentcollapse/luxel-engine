use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::{is_playable, npc_attack, shortest_path_steps};
use crate::{
    AbilityId, Control, EntityId, FailureCode, GAMEPLAY_RECEIPT_SCHEMA, GAMEPLAY_TRACE_SCHEMA,
    GameFailure, GameSnapshot, GameplayEffect, GameplayTag, LocationId, MAX_REPLAY_EVENTS,
    ObjectivePrerequisite, TargetingRule, validate_snapshot,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEvent {
    SelectEntity {
        entity: EntityId,
    },
    Move {
        destination: LocationId,
    },
    UseAbility {
        ability: AbilityId,
        target: EntityId,
    },
    InteractObjective,
    Wait,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayTrace {
    pub schema_version: String,
    pub events: Vec<InputEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameOutcome {
    InProgress,
    Won,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveState {
    Available,
    Secured,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeEntity {
    pub control: Control,
    pub location: LocationId,
    pub health: u32,
    pub max_health: u32,
    pub energy: u32,
    pub max_energy: u32,
    pub tags: BTreeSet<GameplayTag>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeState {
    pub tick: u64,
    pub selected_entity: EntityId,
    pub entities: BTreeMap<EntityId, RuntimeEntity>,
    pub objective: ObjectiveState,
    pub outcome: GameOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transition {
    EntitySelected {
        entity: EntityId,
    },
    EntityMoved {
        entity: EntityId,
        from: LocationId,
        to: LocationId,
    },
    AbilityActivated {
        ability: AbilityId,
        actor: EntityId,
        target: EntityId,
        energy_spent: u32,
        requested_damage: u32,
        applied_damage: u32,
        overkill_damage: u32,
        target_health_after: u32,
        ready_at_tick: u64,
    },
    ObjectiveSecured {
        actor: EntityId,
        location: LocationId,
    },
    Waited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpcNoActionReason {
    Defeated,
    NoPlayerInRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpcDecision {
    LowestHealthRatioThenEntityId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NpcAction {
    NoAction {
        npc: EntityId,
        reason: NpcNoActionReason,
    },
    Attacked {
        npc: EntityId,
        target: EntityId,
        decision: NpcDecision,
        requested_damage: u32,
        applied_damage: u32,
        overkill_damage: u32,
        target_health_after: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventReceipt {
    pub event_index: usize,
    pub tick: u64,
    pub input: InputEvent,
    pub transition: Transition,
    pub npc_action: NpcAction,
    pub resulting_state_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameplayReceiptBody {
    pub schema_version: String,
    pub snapshot_sha256: String,
    pub trace_sha256: String,
    pub event_count: usize,
    pub outcome: GameOutcome,
    pub final_state_sha256: String,
    pub final_state: RuntimeState,
    pub events: Vec<EventReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameplayReceipt {
    pub body: GameplayReceiptBody,
    pub receipt_sha256: String,
}

impl GameplayReceipt {
    /// Compact JSON bytes for deterministic evidence storage and comparison.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, GameFailure> {
        serde_json::to_vec(self).map_err(|error| {
            GameFailure::new(
                FailureCode::ReplayDiverged,
                format!("receipt serialization failed: {error}"),
            )
        })
    }

    pub fn expectation(&self) -> ReplayExpectation {
        ReplayExpectation {
            snapshot_sha256: self.body.snapshot_sha256.clone(),
            trace_sha256: self.body.trace_sha256.clone(),
            receipt_sha256: self.receipt_sha256.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayExpectation {
    pub snapshot_sha256: String,
    pub trace_sha256: String,
    pub receipt_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayField {
    SnapshotSha256,
    TraceSha256,
    ReceiptSha256,
}

pub fn run_replay(
    snapshot: &GameSnapshot,
    trace: &ReplayTrace,
) -> Result<GameplayReceipt, GameFailure> {
    validate_snapshot(snapshot)?;
    if trace.schema_version != GAMEPLAY_TRACE_SCHEMA {
        return Err(GameFailure::new(
            FailureCode::UnsupportedSchema,
            format!(
                "trace schema {:?} is not supported; expected {GAMEPLAY_TRACE_SCHEMA:?}",
                trace.schema_version
            ),
        ));
    }
    if trace.events.len() > MAX_REPLAY_EVENTS {
        return Err(GameFailure::new(
            FailureCode::TraceTooLong,
            format!(
                "trace has {} events; maximum is {MAX_REPLAY_EVENTS}",
                trace.events.len()
            ),
        ));
    }

    let snapshot_sha256 = sha256_json(snapshot)?;
    let trace_sha256 = sha256_json(trace)?;
    let mut state = initial_state(snapshot)?;
    let mut cooldown_ready_at = CooldownReadyAt::new();
    let mut event_receipts = Vec::with_capacity(trace.events.len());

    for (index, input) in trace.events.iter().enumerate() {
        let tick = state.tick.checked_add(1).ok_or_else(|| {
            GameFailure::new(FailureCode::TickOverflow, "runtime tick counter overflowed")
                .at_event(index)
        })?;
        let (next_state, next_cooldowns, event_receipt) =
            step_runtime(snapshot, &state, &cooldown_ready_at, input, tick, index)?;
        event_receipts.push(event_receipt);
        state = next_state;
        cooldown_ready_at = next_cooldowns;
    }

    let body = GameplayReceiptBody {
        schema_version: GAMEPLAY_RECEIPT_SCHEMA.to_owned(),
        snapshot_sha256,
        trace_sha256,
        event_count: event_receipts.len(),
        outcome: state.outcome,
        final_state_sha256: sha256_json(&state)?,
        final_state: state,
        events: event_receipts,
    };
    let receipt_sha256 = sha256_json(&body)?;
    Ok(GameplayReceipt {
        body,
        receipt_sha256,
    })
}

pub fn verify_replay(
    snapshot: &GameSnapshot,
    trace: &ReplayTrace,
    expected: &ReplayExpectation,
) -> Result<GameplayReceipt, GameFailure> {
    let receipt = run_replay(snapshot, trace)?;
    for (field, expected_value, observed_value) in [
        (
            ReplayField::SnapshotSha256,
            expected.snapshot_sha256.as_str(),
            receipt.body.snapshot_sha256.as_str(),
        ),
        (
            ReplayField::TraceSha256,
            expected.trace_sha256.as_str(),
            receipt.body.trace_sha256.as_str(),
        ),
        (
            ReplayField::ReceiptSha256,
            expected.receipt_sha256.as_str(),
            receipt.receipt_sha256.as_str(),
        ),
    ] {
        if expected_value != observed_value {
            return Err(GameFailure::subject(
                FailureCode::ReplayDiverged,
                format!("{field:?}"),
                format!("expected {expected_value}, observed {observed_value}"),
            ));
        }
    }
    Ok(receipt)
}

pub(crate) type CooldownReadyAt = BTreeMap<EntityId, u64>;

pub(crate) fn initial_state(snapshot: &GameSnapshot) -> Result<RuntimeState, GameFailure> {
    let selected_entity =
        snapshot.playable_ids().next().cloned().ok_or_else(|| {
            GameFailure::new(FailureCode::PlayableEntityCount, "no playable entity")
        })?;
    let entities = snapshot
        .entities
        .iter()
        .map(|(id, spec)| {
            let mut tags = spec.tags.clone();
            tags.insert(GameplayTag::Alive);
            (
                id.clone(),
                RuntimeEntity {
                    control: spec.control.clone(),
                    location: spec.location.clone(),
                    health: spec.attributes.health,
                    max_health: spec.attributes.max_health,
                    energy: spec.attributes.energy,
                    max_energy: spec.attributes.max_energy,
                    tags,
                },
            )
        })
        .collect();
    Ok(RuntimeState {
        tick: 0,
        selected_entity,
        entities,
        objective: ObjectiveState::Available,
        outcome: GameOutcome::InProgress,
    })
}

/// Apply one validated v1 input through the same transactional transition path
/// used by batch replay and incremental sessions.
pub(crate) fn step_runtime(
    snapshot: &GameSnapshot,
    state: &RuntimeState,
    cooldown_ready_at: &CooldownReadyAt,
    input: &InputEvent,
    requested_tick: u64,
    event_index: usize,
) -> Result<(RuntimeState, CooldownReadyAt, EventReceipt), GameFailure> {
    if state.outcome != GameOutcome::InProgress {
        return Err(GameFailure::new(
            FailureCode::GameAlreadyFinished,
            format!(
                "input event {event_index} follows terminal outcome {:?}",
                state.outcome
            ),
        )
        .at_event(event_index));
    }
    let expected_tick = state.tick.checked_add(1).ok_or_else(|| {
        GameFailure::new(FailureCode::TickOverflow, "runtime tick counter overflowed")
            .at_event(event_index)
    })?;
    if requested_tick != expected_tick {
        return Err(GameFailure::subject(
            FailureCode::UnexpectedTick,
            "expected_tick",
            format!("requested tick {requested_tick}; next tick is {expected_tick}"),
        )
        .at_event(event_index));
    }

    // Stage all mutations in local copies. The caller commits them only when
    // the complete player input, NPC response, and outcome calculation succeed.
    let mut next_state = state.clone();
    let mut next_cooldowns = cooldown_ready_at.clone();
    let transition = apply_input(
        snapshot,
        &mut next_state,
        &mut next_cooldowns,
        input,
        requested_tick,
    )
    .map_err(|failure| failure.at_event(event_index))?;
    next_state.tick = requested_tick;
    let npc_action = apply_npc_turn(snapshot, &mut next_state)
        .map_err(|failure| failure.at_event(event_index))?;
    if next_state.objective == ObjectiveState::Secured {
        next_state.outcome = GameOutcome::Won;
    } else if no_playable_entity_alive(&next_state) {
        next_state.outcome = GameOutcome::Lost;
    }
    let event_receipt = EventReceipt {
        event_index,
        tick: requested_tick,
        input: input.clone(),
        transition,
        npc_action,
        resulting_state_sha256: sha256_json(&next_state)?,
    };
    Ok((next_state, next_cooldowns, event_receipt))
}

fn apply_input(
    snapshot: &GameSnapshot,
    state: &mut RuntimeState,
    cooldown_ready_at: &mut BTreeMap<EntityId, u64>,
    input: &InputEvent,
    tick: u64,
) -> Result<Transition, GameFailure> {
    match input {
        InputEvent::SelectEntity { entity } => {
            require_living_playable(snapshot, state, entity)?;
            state.selected_entity = entity.clone();
            Ok(Transition::EntitySelected {
                entity: entity.clone(),
            })
        }
        InputEvent::Move { destination } => {
            let actor = state.selected_entity.clone();
            require_living_playable(snapshot, state, &actor)?;
            let entity = state.entities.get_mut(&actor).ok_or_else(|| {
                GameFailure::subject(
                    FailureCode::UnknownEntity,
                    actor.as_str(),
                    "selected entity is absent from runtime state",
                )
            })?;
            if !snapshot.navigation.adjacency.contains_key(destination) {
                return Err(GameFailure::subject(
                    FailureCode::UnknownLocation,
                    destination.as_str(),
                    "movement destination is not present in the navigation graph",
                ));
            }
            if !snapshot
                .navigation
                .adjacency
                .get(&entity.location)
                .is_some_and(|neighbors| neighbors.contains(destination))
            {
                return Err(GameFailure::subject(
                    FailureCode::MoveNotAdjacent,
                    destination.as_str(),
                    format!(
                        "entity {:?} cannot move from {:?} to a non-adjacent location",
                        actor.as_str(),
                        entity.location.as_str()
                    ),
                ));
            }
            let from = entity.location.clone();
            entity.location = destination.clone();
            Ok(Transition::EntityMoved {
                entity: actor,
                from,
                to: destination.clone(),
            })
        }
        InputEvent::UseAbility { ability, target } => {
            let actor = state.selected_entity.clone();
            require_living_playable(snapshot, state, &actor)?;
            if ability != &snapshot.ability.id {
                return Err(GameFailure::subject(
                    FailureCode::UnknownAbility,
                    ability.as_str(),
                    "ability is not declared in the validated snapshot",
                ));
            }
            validate_target(snapshot, state, &actor, target)?;
            let actor_state = state.entities.get(&actor).ok_or_else(|| {
                GameFailure::subject(
                    FailureCode::UnknownEntity,
                    actor.as_str(),
                    "ability actor is absent from runtime state",
                )
            })?;
            if actor_state.energy < snapshot.ability.energy_cost {
                return Err(GameFailure::subject(
                    FailureCode::InsufficientEnergy,
                    actor.as_str(),
                    format!(
                        "ability costs {} energy but actor has {}",
                        snapshot.ability.energy_cost, actor_state.energy
                    ),
                ));
            }
            let ready_at_tick = cooldown_ready_at.get(&actor).copied().unwrap_or(0);
            if tick < ready_at_tick {
                return Err(GameFailure::subject(
                    FailureCode::AbilityCooldownActive,
                    actor.as_str(),
                    format!(
                        "ability is ready at tick {ready_at_tick}, current action is tick {tick}"
                    ),
                ));
            }

            let target_state = state.entities.get(target).ok_or_else(|| {
                GameFailure::subject(
                    FailureCode::UnknownEntity,
                    target.as_str(),
                    "ability target is absent from runtime state",
                )
            })?;
            let requested_damage = match snapshot.ability.effect {
                GameplayEffect::Damage { amount } => amount,
            };
            let before_health = target_state.health;
            let applied_damage = requested_damage.min(before_health);
            let overkill_damage = requested_damage - applied_damage;
            let target_health_after = before_health - applied_damage;
            let next_ready_at = tick
                .checked_add(u64::from(snapshot.ability.cooldown_ticks))
                .ok_or_else(|| {
                    GameFailure::new(
                        FailureCode::TickOverflow,
                        "ability cooldown ready tick overflowed",
                    )
                })?;

            let actor_state = state.entities.get_mut(&actor).ok_or_else(|| {
                GameFailure::subject(
                    FailureCode::UnknownEntity,
                    actor.as_str(),
                    "ability actor disappeared from runtime state",
                )
            })?;
            actor_state.energy -= snapshot.ability.energy_cost;
            let target_state = state.entities.get_mut(target).ok_or_else(|| {
                GameFailure::subject(
                    FailureCode::UnknownEntity,
                    target.as_str(),
                    "ability target disappeared from runtime state",
                )
            })?;
            target_state.health = target_health_after;
            if target_state.health == 0 {
                target_state.tags.remove(&GameplayTag::Alive);
                target_state.tags.insert(GameplayTag::Defeated);
            }
            cooldown_ready_at.insert(actor.clone(), next_ready_at);
            Ok(Transition::AbilityActivated {
                ability: ability.clone(),
                actor,
                target: target.clone(),
                energy_spent: snapshot.ability.energy_cost,
                requested_damage,
                applied_damage,
                overkill_damage,
                target_health_after,
                ready_at_tick: next_ready_at,
            })
        }
        InputEvent::InteractObjective => {
            let actor = state.selected_entity.clone();
            require_living_playable(snapshot, state, &actor)?;
            let actor_location = &state.entities[&actor].location;
            if actor_location != &snapshot.objective.location {
                return Err(GameFailure::subject(
                    FailureCode::ObjectiveNotAtLocation,
                    actor.as_str(),
                    format!(
                        "actor is at {:?}; objective is at {:?}",
                        actor_location.as_str(),
                        snapshot.objective.location.as_str()
                    ),
                ));
            }
            match snapshot.objective.prerequisite {
                ObjectivePrerequisite::AllNpcsDefeated if any_npc_alive(snapshot, state) => {
                    return Err(GameFailure::subject(
                        FailureCode::ObjectivePrerequisiteUnmet,
                        actor.as_str(),
                        "objective requires every NPC to be defeated",
                    ));
                }
                ObjectivePrerequisite::AllNpcsDefeated => {}
            }
            state.objective = ObjectiveState::Secured;
            Ok(Transition::ObjectiveSecured {
                actor,
                location: snapshot.objective.location.clone(),
            })
        }
        InputEvent::Wait => Ok(Transition::Waited),
    }
}

fn require_living_playable(
    snapshot: &GameSnapshot,
    state: &RuntimeState,
    entity: &EntityId,
) -> Result<(), GameFailure> {
    if !snapshot.entities.contains_key(entity) {
        return Err(GameFailure::subject(
            FailureCode::UnknownEntity,
            entity.as_str(),
            "input references an undeclared entity",
        ));
    }
    if !is_playable(snapshot, entity) {
        return Err(GameFailure::subject(
            FailureCode::EntityNotPlayable,
            entity.as_str(),
            "only playable entities can receive player input",
        ));
    }
    let runtime = state.entities.get(entity).ok_or_else(|| {
        GameFailure::subject(
            FailureCode::UnknownEntity,
            entity.as_str(),
            "entity is absent from runtime state",
        )
    })?;
    if runtime.health == 0 {
        return Err(GameFailure::subject(
            FailureCode::EntityDefeated,
            entity.as_str(),
            "defeated entity cannot receive player input",
        ));
    }
    Ok(())
}

fn validate_target(
    snapshot: &GameSnapshot,
    state: &RuntimeState,
    actor: &EntityId,
    target: &EntityId,
) -> Result<(), GameFailure> {
    let Some(target_spec) = snapshot.entities.get(target) else {
        return Err(GameFailure::subject(
            FailureCode::InvalidTarget,
            target.as_str(),
            "target is not a declared entity",
        ));
    };
    if !matches!(target_spec.control, Control::Npc { .. }) {
        return Err(GameFailure::subject(
            FailureCode::InvalidTarget,
            target.as_str(),
            "ability target must be an enemy NPC",
        ));
    }
    let target_state = state.entities.get(target).ok_or_else(|| {
        GameFailure::subject(
            FailureCode::InvalidTarget,
            target.as_str(),
            "target has no runtime state",
        )
    })?;
    if target_state.health == 0 {
        return Err(GameFailure::subject(
            FailureCode::InvalidTarget,
            target.as_str(),
            "defeated NPC cannot be targeted",
        ));
    }
    let actor_location = &state.entities[actor].location;
    let target_location = &target_state.location;
    let distance = shortest_path_steps(&snapshot.navigation, actor_location, target_location);
    let max_steps = match snapshot.ability.targeting {
        TargetingRule::EnemyWithinNavigationSteps { max_steps } => usize::from(max_steps),
    };
    if !distance.is_some_and(|steps| steps <= max_steps) {
        return Err(GameFailure::subject(
            FailureCode::InvalidTarget,
            target.as_str(),
            format!(
                "target is outside ability range of {max_steps} navigation steps from actor {:?}",
                actor.as_str()
            ),
        ));
    }
    Ok(())
}

fn apply_npc_turn(
    snapshot: &GameSnapshot,
    state: &mut RuntimeState,
) -> Result<NpcAction, GameFailure> {
    let Some(npc_id) = snapshot.npc_ids().next().cloned() else {
        return Err(GameFailure::new(
            FailureCode::NpcCount,
            "validated snapshot lost its required NPC during runtime",
        ));
    };
    let npc_state = &state.entities[&npc_id];
    if npc_state.health == 0 {
        return Ok(NpcAction::NoAction {
            npc: npc_id,
            reason: NpcNoActionReason::Defeated,
        });
    }
    let Some((attack_damage, attack_range_steps)) = npc_attack(snapshot, &npc_id) else {
        return Err(GameFailure::subject(
            FailureCode::InvalidNpcBehavior,
            npc_id.as_str(),
            "validated NPC has no supported behavior",
        ));
    };
    let target = choose_npc_target(snapshot, state, &npc_id, attack_range_steps);
    let Some(target) = target else {
        return Ok(NpcAction::NoAction {
            npc: npc_id,
            reason: NpcNoActionReason::NoPlayerInRange,
        });
    };
    let target_state = state.entities.get_mut(&target).ok_or_else(|| {
        GameFailure::subject(
            FailureCode::UnknownEntity,
            target.as_str(),
            "selected NPC target disappeared from runtime state",
        )
    })?;
    let applied_damage = attack_damage.min(target_state.health);
    let overkill_damage = attack_damage - applied_damage;
    target_state.health -= applied_damage;
    if target_state.health == 0 {
        target_state.tags.remove(&GameplayTag::Alive);
        target_state.tags.insert(GameplayTag::Defeated);
    }
    Ok(NpcAction::Attacked {
        npc: npc_id,
        target,
        decision: NpcDecision::LowestHealthRatioThenEntityId,
        requested_damage: attack_damage,
        applied_damage,
        overkill_damage,
        target_health_after: target_state.health,
    })
}

fn choose_npc_target(
    snapshot: &GameSnapshot,
    state: &RuntimeState,
    npc_id: &EntityId,
    attack_range_steps: u16,
) -> Option<EntityId> {
    let npc_location = &state.entities.get(npc_id)?.location;
    let mut candidates: Vec<EntityId> = snapshot
        .playable_ids()
        .filter_map(|id| {
            let entity = state.entities.get(id)?;
            if entity.health == 0 {
                return None;
            }
            let distance =
                shortest_path_steps(&snapshot.navigation, npc_location, &entity.location)?;
            (distance <= usize::from(attack_range_steps)).then_some(id.clone())
        })
        .collect();
    candidates.sort_by(|left, right| {
        let left_state = &state.entities[left];
        let right_state = &state.entities[right];
        let left_scaled = u128::from(left_state.health) * u128::from(right_state.max_health);
        let right_scaled = u128::from(right_state.health) * u128::from(left_state.max_health);
        left_scaled.cmp(&right_scaled).then_with(|| left.cmp(right))
    });
    candidates.into_iter().next()
}

fn any_npc_alive(snapshot: &GameSnapshot, state: &RuntimeState) -> bool {
    snapshot.npc_ids().any(|id| {
        state
            .entities
            .get(id)
            .is_some_and(|entity| entity.health > 0)
    })
}

fn no_playable_entity_alive(state: &RuntimeState) -> bool {
    !state
        .entities
        .values()
        .any(|entity| matches!(entity.control, Control::Playable) && entity.health > 0)
}

pub(crate) fn sha256_json<T: Serialize>(value: &T) -> Result<String, GameFailure> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        GameFailure::new(
            FailureCode::ReplayDiverged,
            format!("deterministic JSON serialization failed: {error}"),
        )
    })?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}
