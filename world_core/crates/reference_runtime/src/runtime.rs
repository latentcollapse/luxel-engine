use serde::{Deserialize, Serialize};

use crate::fields::prefixed_sha256;
use crate::world::{cell_is_clear, cell_position, edge_grade, validate_world_artifact};
use crate::{
    ReferenceRuntimeError, TRAVERSAL_EVIDENCE_SCHEMA, TRAVERSAL_VALIDATOR_ID, WorldArtifact,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraversalOutcome {
    Completed,
    Incomplete,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraversalEventKind {
    PlayerSpawnActivated,
    CellEntered,
    EncounterEntered,
    ObjectiveReached,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraversalStep {
    pub tick: u64,
    pub cell: usize,
    pub position_xyz_m: [f64; 3],
    pub events: Vec<TraversalEventKind>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraversalEvidenceBody {
    pub schema_version: String,
    pub validator_id: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub player_spawn_id: String,
    pub objective_id: String,
    pub objective_reached: bool,
    pub outcome: TraversalOutcome,
    pub visited_encounter_ids: Vec<String>,
    pub path_cells: Vec<usize>,
    pub steps: Vec<TraversalStep>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraversalEvidence {
    pub body: TraversalEvidenceBody,
    pub evidence_sha256: String,
}

pub fn run_playthrough(world: &WorldArtifact) -> Result<TraversalEvidence, ReferenceRuntimeError> {
    validate_world_artifact(world)?;
    let layout = &world.body.authored_layout;
    let navigation = &world.body.navigation;
    let route = &navigation.route_cells;
    if route.is_empty() || route[0] != navigation.start_cell {
        return Err(ReferenceRuntimeError::contract(
            "runtime route does not begin at the declared player spawn".into(),
        ));
    }
    if *route.last().unwrap_or(&usize::MAX) != navigation.objective_cell {
        return Err(ReferenceRuntimeError::contract(
            "runtime route does not end at the declared objective".into(),
        ));
    }

    let start_spawn = layout
        .spawns
        .iter()
        .find(|spawn| spawn.spawn_id == layout.traversal.start_spawn_id)
        .ok_or_else(|| ReferenceRuntimeError::contract("player spawn is missing".into()))?;
    let mut visited = std::collections::BTreeSet::new();
    let mut steps = Vec::with_capacity(route.len());
    let mut visited_encounter_ids = Vec::new();
    for (index, cell) in route.iter().copied().enumerate() {
        if !cell_is_clear(layout, &world.body.fields, cell) {
            return Err(ReferenceRuntimeError::contract(format!(
                "reference runtime collided at route cell {cell}"
            )));
        }
        if index > 0 {
            let previous = route[index - 1];
            let row_delta = previous / layout.resolution;
            let column_delta = previous % layout.resolution;
            let row = cell / layout.resolution;
            let column = cell % layout.resolution;
            let adjacent = row.abs_diff(row_delta) + column.abs_diff(column_delta) == 1;
            if !adjacent {
                return Err(ReferenceRuntimeError::contract(format!(
                    "runtime route contains a non-adjacent move from cell {previous} to {cell}"
                )));
            }
            let grade = edge_grade(layout, &world.body.fields, previous, cell);
            if grade > layout.traversal.maximum_grade {
                return Err(ReferenceRuntimeError::contract(format!(
                    "runtime traversal exceeds the authored slope limit at cell {cell}"
                )));
            }
        }

        let mut events = vec![TraversalEventKind::CellEntered];
        if index == 0 {
            events.push(TraversalEventKind::PlayerSpawnActivated);
        }
        let position = cell_position(layout, cell);
        for encounter in &world.body.encounters {
            if !visited.contains(&encounter.encounter_id)
                && crate::model::distance(position, encounter.center_xz_m) <= encounter.radius_m
            {
                visited.insert(encounter.encounter_id.clone());
                visited_encounter_ids.push(encounter.encounter_id.clone());
                events.push(TraversalEventKind::EncounterEntered);
            }
        }
        if index + 1 == route.len() && cell == navigation.objective_cell {
            events.push(TraversalEventKind::ObjectiveReached);
        }
        steps.push(TraversalStep {
            tick: index as u64,
            cell,
            position_xyz_m: [position[0], world.body.fields.heights_m[cell], position[1]],
            events,
        });
    }

    let objective_reached = steps.last().is_some_and(|step| {
        step.cell == navigation.objective_cell
            && step.events.contains(&TraversalEventKind::ObjectiveReached)
    });
    let required_encounters_visited = world
        .body
        .encounters
        .iter()
        .filter(|encounter| encounter.required)
        .all(|encounter| visited.contains(&encounter.encounter_id));
    let outcome = if objective_reached && required_encounters_visited {
        TraversalOutcome::Completed
    } else {
        TraversalOutcome::Incomplete
    };
    let body = TraversalEvidenceBody {
        schema_version: TRAVERSAL_EVIDENCE_SCHEMA.into(),
        validator_id: TRAVERSAL_VALIDATOR_ID.into(),
        world_artifact_id: world.artifact_id.clone(),
        world_artifact_sha256: world.artifact_sha256.clone(),
        player_spawn_id: start_spawn.spawn_id.clone(),
        objective_id: layout.traversal.objective_id.clone(),
        objective_reached,
        outcome,
        visited_encounter_ids,
        path_cells: route.clone(),
        steps,
    };
    let evidence_sha256 = prefixed_sha256(&serde_json::to_vec(&body).map_err(|error| {
        ReferenceRuntimeError::contract(format!("traversal evidence encoding failed: {error}"))
    })?);
    Ok(TraversalEvidence {
        body,
        evidence_sha256,
    })
}

pub fn validate_traversal_evidence(
    world: &WorldArtifact,
    evidence: &TraversalEvidence,
) -> Result<(), ReferenceRuntimeError> {
    let digest = prefixed_sha256(&serde_json::to_vec(&evidence.body).map_err(|error| {
        ReferenceRuntimeError::contract(format!("traversal evidence encoding failed: {error}"))
    })?);
    if digest != evidence.evidence_sha256 {
        return Err(ReferenceRuntimeError::provenance(
            "traversal evidence digest does not match its body".into(),
        ));
    }
    let expected = run_playthrough(world)?;
    if *evidence != expected {
        return Err(ReferenceRuntimeError::provenance(
            "traversal evidence differs from a fresh deterministic runtime replay".into(),
        ));
    }
    if evidence.body.outcome != TraversalOutcome::Completed {
        return Err(ReferenceRuntimeError::contract(
            "traversal runtime did not complete every required encounter and objective".into(),
        ));
    }
    Ok(())
}
