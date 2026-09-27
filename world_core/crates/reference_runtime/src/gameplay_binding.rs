use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wge_gameplay_contract::{
    AbilityId, AbilitySpec, Control, EntityAttributes, EntityId, EntitySpec,
    GAMEPLAY_SNAPSHOT_SCHEMA, GAMEPLAY_TRACE_SCHEMA, GameOutcome, GameSnapshot, GameplayEffect,
    GameplayReceipt, GameplayTag, InputEvent, LocationId, MAX_REPLAY_EVENTS, NavigationGraph,
    NpcBehavior, ObjectivePrerequisite, ObjectiveSpec, ReplayTrace, TargetingRule, run_replay,
    verify_replay,
};

use crate::fields::prefixed_sha256;
use crate::world::validate_world_artifact;
use crate::{
    GAMEPLAY_WORLD_BINDING_SCHEMA, GAMEPLAY_WORLD_VALIDATOR_ID, ReferenceRuntimeError,
    TRAVERSAL_EVIDENCE_SCHEMA, TraversalEvidence, VISUAL_EVIDENCE_SCHEMA, VisualEvidence,
    WorldArtifact, validate_traversal_evidence, validate_visual_evidence,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GameplayWorldBindingBody {
    pub schema_version: String,
    pub validator_id: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub traversal_evidence_sha256: String,
    pub visual_evidence_sha256: String,
    pub primary_encounter_id: String,
    pub objective_id: String,
    pub gameplay_snapshot: GameSnapshot,
    pub gameplay_trace: ReplayTrace,
    pub gameplay_receipt: GameplayReceipt,
    pub outcome: GameOutcome,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GameplayWorldBinding {
    pub body: GameplayWorldBindingBody,
    pub evidence_sha256: String,
}

pub fn build_gameplay_world_binding(
    world: &WorldArtifact,
    traversal: &TraversalEvidence,
    capture: &[u8],
    visual: &VisualEvidence,
) -> Result<GameplayWorldBinding, ReferenceRuntimeError> {
    validate_world_artifact(world)?;
    validate_traversal_evidence(world, traversal)?;
    validate_visual_evidence(world, capture, visual)?;
    let (encounter_id, snapshot, trace) = gameplay_case(world)?;
    let receipt = run_replay(&snapshot, &trace).map_err(|error| {
        ReferenceRuntimeError::contract(format!("world-bound gameplay replay failed: {error}"))
    })?;
    if receipt.body.outcome != GameOutcome::Won {
        return Err(ReferenceRuntimeError::contract(
            "world-bound gameplay replay did not win".into(),
        ));
    }
    let body = GameplayWorldBindingBody {
        schema_version: GAMEPLAY_WORLD_BINDING_SCHEMA.into(),
        validator_id: GAMEPLAY_WORLD_VALIDATOR_ID.into(),
        world_artifact_id: world.artifact_id.clone(),
        world_artifact_sha256: world.artifact_sha256.clone(),
        traversal_evidence_sha256: traversal.evidence_sha256.clone(),
        visual_evidence_sha256: visual.evidence_sha256.clone(),
        primary_encounter_id: encounter_id,
        objective_id: world.body.authored_layout.traversal.objective_id.clone(),
        gameplay_snapshot: snapshot,
        gameplay_trace: trace,
        outcome: receipt.body.outcome,
        gameplay_receipt: receipt,
    };
    let evidence_sha256 = prefixed_sha256(&serde_json::to_vec(&body).map_err(|error| {
        ReferenceRuntimeError::contract(format!("gameplay binding encoding failed: {error}"))
    })?);
    Ok(GameplayWorldBinding {
        body,
        evidence_sha256,
    })
}

pub fn validate_gameplay_world_binding(
    world: &WorldArtifact,
    traversal: &TraversalEvidence,
    capture: &[u8],
    visual: &VisualEvidence,
    binding: &GameplayWorldBinding,
) -> Result<(), ReferenceRuntimeError> {
    validate_world_artifact(world)?;
    validate_traversal_evidence(world, traversal)?;
    validate_visual_evidence(world, capture, visual)?;
    let digest = prefixed_sha256(&serde_json::to_vec(&binding.body).map_err(|error| {
        ReferenceRuntimeError::contract(format!("gameplay binding encoding failed: {error}"))
    })?);
    if digest != binding.evidence_sha256 {
        return Err(ReferenceRuntimeError::provenance(
            "gameplay world binding digest does not match its body".into(),
        ));
    }
    if binding.body.schema_version != GAMEPLAY_WORLD_BINDING_SCHEMA
        || binding.body.validator_id != GAMEPLAY_WORLD_VALIDATOR_ID
        || binding.body.world_artifact_id != world.artifact_id
        || binding.body.world_artifact_sha256 != world.artifact_sha256
        || binding.body.traversal_evidence_sha256 != traversal.evidence_sha256
        || binding.body.visual_evidence_sha256 != visual.evidence_sha256
        || binding.body.objective_id != world.body.authored_layout.traversal.objective_id
        || binding.body.outcome != GameOutcome::Won
    {
        return Err(ReferenceRuntimeError::provenance(
            "gameplay binding points at different world or runtime evidence".into(),
        ));
    }
    let (encounter_id, expected_snapshot, expected_trace) = gameplay_case(world)?;
    if binding.body.primary_encounter_id != encounter_id
        || binding.body.gameplay_snapshot != expected_snapshot
        || binding.body.gameplay_trace != expected_trace
    {
        return Err(ReferenceRuntimeError::contract(
            "gameplay snapshot or trace does not map to the world's actual route and encounter"
                .into(),
        ));
    }
    let replay = verify_replay(
        &binding.body.gameplay_snapshot,
        &binding.body.gameplay_trace,
        &binding.body.gameplay_receipt.expectation(),
    )
    .map_err(|error| {
        ReferenceRuntimeError::provenance(format!("gameplay receipt failed native replay: {error}"))
    })?;
    if replay != binding.body.gameplay_receipt
        || replay.body.outcome != binding.body.outcome
        || replay.body.outcome != GameOutcome::Won
    {
        return Err(ReferenceRuntimeError::provenance(
            "gameplay receipt is not an independently reproduced win".into(),
        ));
    }
    Ok(())
}

fn gameplay_case(
    world: &WorldArtifact,
) -> Result<(String, GameSnapshot, ReplayTrace), ReferenceRuntimeError> {
    if world.body.navigation.route_cells.len() < 2 {
        return Err(ReferenceRuntimeError::contract(
            "gameplay binding requires a nontrivial authored route".into(),
        ));
    }
    let encounter = world
        .body
        .encounters
        .iter()
        .find(|encounter| encounter.required)
        .ok_or_else(|| {
            ReferenceRuntimeError::contract(
                "gameplay binding requires at least one required encounter".into(),
            )
        })?;
    if world
        .body
        .encounters
        .iter()
        .filter(|item| item.required)
        .count()
        != 1
    {
        return Err(ReferenceRuntimeError::contract(
            "the existing gameplay contract binds exactly one required encounter per slice".into(),
        ));
    }
    let actor_spawn = world
        .body
        .spawns
        .iter()
        .find(|spawn| spawn.spawn_id == encounter.opponent_spawn_id)
        .ok_or_else(|| {
            ReferenceRuntimeError::contract("encounter actor spawn is missing".into())
        })?;
    let start = world.body.navigation.start_cell;
    let objective = world.body.navigation.objective_cell;
    let enemy_cell = actor_spawn.grid_cell;
    let route = &world.body.navigation.route_cells;
    let enemy_route_index = route
        .iter()
        .position(|cell| *cell == enemy_cell)
        .ok_or_else(|| {
            ReferenceRuntimeError::contract(
                "the authored navigation route does not reach its encounter actor spawn".into(),
            )
        })?;
    let start_location = location(start);
    let objective_location = location(objective);
    let enemy_location = location(enemy_cell);

    let mut adjacency = BTreeMap::<LocationId, BTreeSet<LocationId>>::new();
    for cell in route {
        adjacency.entry(location(*cell)).or_default();
    }
    for pair in route.windows(2) {
        let from = location(pair[0]);
        let to = location(pair[1]);
        adjacency
            .entry(from.clone())
            .or_default()
            .insert(to.clone());
        adjacency.entry(to).or_default().insert(from);
    }

    let mut entities = BTreeMap::new();
    entities.insert(
        EntityId::from("player_alpha"),
        EntitySpec {
            control: Control::Playable,
            location: start_location.clone(),
            attributes: EntityAttributes {
                health: 20,
                max_health: 20,
                energy: 2,
                max_energy: 2,
            },
            tags: BTreeSet::from([GameplayTag::Player, GameplayTag::Scout]),
        },
    );
    entities.insert(
        EntityId::from("player_beta"),
        EntitySpec {
            control: Control::Playable,
            location: start_location,
            attributes: EntityAttributes {
                health: 20,
                max_health: 20,
                energy: 2,
                max_energy: 2,
            },
            tags: BTreeSet::from([GameplayTag::Player, GameplayTag::Arcanist]),
        },
    );
    entities.insert(
        EntityId::from("npc_primary_guard"),
        EntitySpec {
            control: Control::Npc {
                behavior: NpcBehavior::GuardObjectiveCounterattack {
                    attack_damage: 1,
                    attack_range_steps: 0,
                },
            },
            location: enemy_location.clone(),
            attributes: EntityAttributes {
                health: 6,
                max_health: 6,
                energy: 0,
                max_energy: 0,
            },
            tags: BTreeSet::from([GameplayTag::Enemy]),
        },
    );
    let snapshot = GameSnapshot {
        schema_version: GAMEPLAY_SNAPSHOT_SCHEMA.into(),
        navigation: NavigationGraph { adjacency },
        entities,
        ability: AbilitySpec {
            id: AbilityId::from("world_bound_strike"),
            energy_cost: 2,
            cooldown_ticks: 1,
            targeting: TargetingRule::EnemyWithinNavigationSteps { max_steps: 0 },
            effect: GameplayEffect::Damage { amount: 6 },
        },
        objective: ObjectiveSpec {
            location: objective_location,
            prerequisite: ObjectivePrerequisite::AllNpcsDefeated,
            tags: BTreeSet::from([GameplayTag::Objective]),
        },
    };

    let mut events = Vec::new();
    for cell in route.iter().take(enemy_route_index + 1).skip(1) {
        events.push(InputEvent::Move {
            destination: location(*cell),
        });
    }
    events.push(InputEvent::UseAbility {
        ability: AbilityId::from("world_bound_strike"),
        target: EntityId::from("npc_primary_guard"),
    });
    for cell in route.iter().skip(enemy_route_index + 1) {
        events.push(InputEvent::Move {
            destination: location(*cell),
        });
    }
    events.push(InputEvent::InteractObjective);
    if events.len() > MAX_REPLAY_EVENTS {
        return Err(ReferenceRuntimeError::contract(
            "world route exceeds the gameplay replay event limit".into(),
        ));
    }
    let trace = ReplayTrace {
        schema_version: GAMEPLAY_TRACE_SCHEMA.into(),
        events,
    };
    let _stable_semantic_bindings = BTreeMap::<String, String>::from([
        ("start_cell".into(), start.to_string()),
        ("enemy_cell".into(), enemy_cell.to_string()),
        ("objective_cell".into(), objective.to_string()),
        (
            "traversal_contract".into(),
            TRAVERSAL_EVIDENCE_SCHEMA.into(),
        ),
        ("visual_contract".into(), VISUAL_EVIDENCE_SCHEMA.into()),
    ]);
    Ok((encounter.encounter_id.clone(), snapshot, trace))
}

fn location(cell: usize) -> LocationId {
    LocationId::new(format!("cell_{cell}"))
}
