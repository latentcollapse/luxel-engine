//! World-bound evidence for the generalized gameplay substrate.
//!
//! The legacy v1 binding remains in the certification wire contract for
//! compatibility, but a native reference build also runs this data-driven
//! scenario. It deliberately uses arbitrary entity counts, resource costs,
//! tag targeting, and objective prerequisites so the runtime path cannot be
//! certified solely by the old two-player/one-guard fixture.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wge_gameplay_contract::general::{
    AbilityId, CostPayer, EffectOperation, EffectRecipient, EffectSpec, EntityControl, EntityId,
    GENERAL_SNAPSHOT_SCHEMA, GENERAL_TRACE_SCHEMA, GameplayTag, GeneralAbilitySpec,
    GeneralGameSpec, GeneralGameplayReceipt, GeneralInputEvent, GeneralObjectiveSpec, GeneralTrace,
    LocationId, NpcPolicy, ObjectiveId, ObjectiveRequirement, ResourceCost, ResourceId,
    ResourceStateSpec, StackingPolicy, TargetingRule, run_general_replay,
    validate_general_snapshot, verify_general_receipt,
};

use crate::fields::prefixed_sha256;
use crate::{ReferenceRuntimeError, WorldArtifact};

pub const GENERAL_GAMEPLAY_EVIDENCE_SCHEMA: &str = "wge.reference-general-gameplay/v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeneralGameplayEvidenceBody {
    pub schema_version: String,
    pub scenario_id: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub entity_count: usize,
    pub ability_count: usize,
    pub objective_count: usize,
    pub gameplay_trace: GeneralTrace,
    pub gameplay_receipt: GeneralGameplayReceipt,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeneralGameplayEvidence {
    pub body: GeneralGameplayEvidenceBody,
    pub evidence_sha256: String,
}

pub fn build_general_gameplay_evidence(
    world: &WorldArtifact,
) -> Result<GeneralGameplayEvidence, ReferenceRuntimeError> {
    crate::validate_world_artifact(world)?;
    let (spec, trace) = scenario(world)?;
    validate_general_snapshot(&spec).map_err(general_error)?;
    let receipt = run_general_replay(&spec, &trace).map_err(general_error)?;
    if receipt.body.outcome != wge_gameplay_contract::general::GeneralOutcome::Won {
        return Err(ReferenceRuntimeError::contract(
            "general world-bound gameplay scenario did not win".into(),
        ));
    }
    let body = GeneralGameplayEvidenceBody {
        schema_version: GENERAL_GAMEPLAY_EVIDENCE_SCHEMA.into(),
        scenario_id: "world-bound-general-substrate".into(),
        world_artifact_id: world.artifact_id.clone(),
        world_artifact_sha256: world.artifact_sha256.clone(),
        entity_count: spec.entities.len(),
        ability_count: spec.abilities.len(),
        objective_count: spec.objectives.len(),
        gameplay_trace: trace,
        gameplay_receipt: receipt,
    };
    let evidence_sha256 = prefixed_sha256(&serde_json::to_vec(&body).map_err(|error| {
        ReferenceRuntimeError::contract(format!(
            "general gameplay evidence encoding failed: {error}"
        ))
    })?);
    Ok(GeneralGameplayEvidence {
        body,
        evidence_sha256,
    })
}

pub fn validate_general_gameplay_evidence(
    world: &WorldArtifact,
    evidence: &GeneralGameplayEvidence,
) -> Result<(), ReferenceRuntimeError> {
    crate::validate_world_artifact(world)?;
    if evidence.body.schema_version != GENERAL_GAMEPLAY_EVIDENCE_SCHEMA
        || evidence.body.world_artifact_id != world.artifact_id
        || evidence.body.world_artifact_sha256 != world.artifact_sha256
    {
        return Err(ReferenceRuntimeError::provenance(
            "general gameplay evidence is bound to a different world artifact".into(),
        ));
    }
    let digest = prefixed_sha256(&serde_json::to_vec(&evidence.body).map_err(|error| {
        ReferenceRuntimeError::contract(format!(
            "general gameplay evidence encoding failed: {error}"
        ))
    })?);
    if digest != evidence.evidence_sha256 {
        return Err(ReferenceRuntimeError::provenance(
            "general gameplay evidence digest does not match its body".into(),
        ));
    }
    let (spec, expected_trace) = scenario(world)?;
    if evidence.body.scenario_id != "world-bound-general-substrate"
        || evidence.body.gameplay_trace != expected_trace
        || evidence.body.entity_count != spec.entities.len()
        || evidence.body.ability_count != spec.abilities.len()
        || evidence.body.objective_count != spec.objectives.len()
    {
        return Err(ReferenceRuntimeError::provenance(
            "general gameplay evidence does not match the authored world scenario".into(),
        ));
    }
    let expected = run_general_replay(&spec, &expected_trace).map_err(general_error)?;
    if expected != evidence.body.gameplay_receipt {
        return Err(ReferenceRuntimeError::provenance(
            "general gameplay receipt differs from fresh deterministic replay".into(),
        ));
    }
    verify_general_receipt(&spec, &expected_trace, &evidence.body.gameplay_receipt)
        .map_err(general_error)
}

fn scenario(
    world: &WorldArtifact,
) -> Result<(GeneralGameSpec, GeneralTrace), ReferenceRuntimeError> {
    let route = &world.body.navigation.route_cells;
    if route.len() < 2 || route.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ReferenceRuntimeError::contract(
            "general gameplay scenario requires a nontrivial unique world route".into(),
        ));
    }
    let locations = route.iter().copied().map(location).collect::<Vec<_>>();
    let mut navigation = BTreeMap::new();
    for location in &locations {
        navigation
            .entry(location.clone())
            .or_insert_with(BTreeSet::new);
    }
    for pair in locations.windows(2) {
        navigation
            .entry(pair[0].clone())
            .or_default()
            .insert(pair[1].clone());
        navigation
            .entry(pair[1].clone())
            .or_default()
            .insert(pair[0].clone());
    }

    let opponent_spawns = world
        .body
        .spawns
        .iter()
        .filter(|spawn| matches!(spawn.role, crate::SpawnRole::Opponent))
        .collect::<Vec<_>>();
    if opponent_spawns.is_empty() {
        return Err(ReferenceRuntimeError::contract(
            "general gameplay scenario requires an authored opponent spawn".into(),
        ));
    }
    let start = location(world.body.navigation.start_cell);
    let objective = location(world.body.navigation.objective_cell);
    let hostile = GameplayTag::from("hostile");
    let health = ResourceId::from("health");
    let mana = ResourceId::from("mana");
    let player = EntityId::from("general_player");
    let mut entities = BTreeMap::new();
    entities.insert(
        player.clone(),
        wge_gameplay_contract::general::GeneralEntitySpec {
            control: EntityControl::Player,
            location: start.clone(),
            resources: BTreeMap::from([
                (
                    health.clone(),
                    ResourceStateSpec {
                        current: 20,
                        maximum: 20,
                    },
                ),
                (
                    mana.clone(),
                    ResourceStateSpec {
                        current: opponent_spawns.len() as u32,
                        maximum: opponent_spawns.len() as u32,
                    },
                ),
            ]),
            vital_resource: Some(health.clone()),
            tags: BTreeSet::from([GameplayTag::from("player")]),
        },
    );
    entities.insert(
        EntityId::from("general_support"),
        wge_gameplay_contract::general::GeneralEntitySpec {
            control: EntityControl::Player,
            location: start.clone(),
            resources: BTreeMap::from([
                (
                    health.clone(),
                    ResourceStateSpec {
                        current: 12,
                        maximum: 12,
                    },
                ),
                (
                    mana.clone(),
                    ResourceStateSpec {
                        current: 0,
                        maximum: 1,
                    },
                ),
            ]),
            vital_resource: Some(health.clone()),
            tags: BTreeSet::from([GameplayTag::from("player"), GameplayTag::from("support")]),
        },
    );
    for spawn in &opponent_spawns {
        let id = EntityId::new(format!("npc_{}", spawn.spawn_id));
        entities.insert(
            id,
            wge_gameplay_contract::general::GeneralEntitySpec {
                control: EntityControl::Npc {
                    policy: NpcPolicy::Passive,
                },
                location: location(spawn.grid_cell),
                resources: BTreeMap::from([(
                    health.clone(),
                    ResourceStateSpec {
                        current: 2,
                        maximum: 2,
                    },
                )]),
                vital_resource: Some(health.clone()),
                tags: BTreeSet::from([hostile.clone()]),
            },
        );
    }

    let ability_id = AbilityId::from("general_pulse");
    let ability = GeneralAbilitySpec {
        targeting: TargetingRule::Entity {
            required_tags: BTreeSet::from([hostile.clone()]),
            forbidden_tags: BTreeSet::from([GameplayTag::from("player")]),
            max_navigation_steps: None,
            allow_self: false,
        },
        costs: vec![ResourceCost {
            payer: CostPayer::Actor,
            resource: mana,
            amount: 1,
        }],
        cooldown_ticks: 1,
        requirements: Vec::new(),
        effects: vec![EffectSpec {
            stack_key: "pulse_damage".into(),
            recipient: EffectRecipient::SelectedTarget,
            operation: EffectOperation::Damage {
                resource: health,
                amount: 2,
            },
            duration_ticks: 0,
            period_ticks: None,
            stacking: StackingPolicy::Replace,
        }],
    };
    let mut trace_events = opponent_spawns
        .iter()
        .map(|spawn| GeneralInputEvent::ActivateAbility {
            ability: ability_id.clone(),
            target: EntityId::new(format!("npc_{}", spawn.spawn_id)),
        })
        .collect::<Vec<_>>();
    trace_events.extend(
        locations
            .iter()
            .skip(1)
            .map(|destination| GeneralInputEvent::Move {
                destination: destination.clone(),
            }),
    );
    trace_events.push(GeneralInputEvent::InteractObjective {
        objective: ObjectiveId::from("world_objective"),
    });
    let spec = GeneralGameSpec {
        schema_version: GENERAL_SNAPSHOT_SCHEMA.into(),
        navigation,
        entities,
        abilities: BTreeMap::from([(ability_id, ability)]),
        objectives: BTreeMap::from([(
            ObjectiveId::from("world_objective"),
            GeneralObjectiveSpec {
                location: objective,
                required_for_victory: true,
                prerequisites: vec![ObjectiveRequirement::AllEntitiesDefeatedWithTags {
                    tags: BTreeSet::from([hostile]),
                }],
            },
        )]),
    };
    let trace = GeneralTrace {
        schema_version: GENERAL_TRACE_SCHEMA.into(),
        events: trace_events,
    };
    Ok((spec, trace))
}

fn location(cell: usize) -> LocationId {
    LocationId::new(format!("cell_{cell}"))
}

fn general_error(error: impl std::fmt::Display) -> ReferenceRuntimeError {
    ReferenceRuntimeError::contract(format!("general gameplay contract failed: {error}"))
}
