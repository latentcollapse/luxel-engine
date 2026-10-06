use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TraversalSessionInput {
    pub expected_tick: u64,
    pub destination_cell: usize,
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

pub const TRAVERSAL_SESSION_SNAPSHOT_SCHEMA: &str = "luxel.reference-traversal-session-snapshot/v1";
pub const TRAVERSAL_SESSION_COMPLETION_SCHEMA: &str =
    "luxel.reference-traversal-session-completion/v1";
pub const MAX_TRAVERSAL_SESSION_STEPS: usize = 257 * 257;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraversalSessionSnapshotBody {
    pub schema_version: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub steps: Vec<TraversalStep>,
    pub visited_encounter_ids: Vec<String>,
    pub objective_reached: bool,
    pub finished: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraversalSessionSnapshot {
    pub body: TraversalSessionSnapshotBody,
    pub state_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraversalSessionCompletionBody {
    pub schema_version: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub final_state_sha256: String,
    pub final_tick: u64,
    pub final_cell: usize,
    pub objective_reached: bool,
    pub outcome: TraversalOutcome,
    pub visited_encounter_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraversalSessionCompletion {
    pub body: TraversalSessionCompletionBody,
    pub completion_sha256: String,
}

/// Deterministic, engine-neutral traversal state for a single world session.
/// The session owns the validated world snapshot it was initialized from.
#[derive(Clone, Debug)]
pub struct TraversalSession {
    world: WorldArtifact,
    steps: Vec<TraversalStep>,
    visited_encounter_ids: Vec<String>,
    visited_encounter_set: BTreeSet<String>,
    objective_reached: bool,
    finished: bool,
}

impl TraversalSession {
    /// Validate and initialize at the declared player spawn (tick zero).
    pub fn initialize(world: &WorldArtifact) -> Result<Self, ReferenceRuntimeError> {
        Self::initialize_with_objective_policy(world, true)
    }

    fn initialize_with_objective_policy(
        world: &WorldArtifact,
        mark_objective_at_spawn: bool,
    ) -> Result<Self, ReferenceRuntimeError> {
        validate_world_artifact(world)?;
        validate_route_anchors(world)?;

        let start_cell = world.body.navigation.start_cell;
        let mut session = Self {
            world: world.clone(),
            steps: Vec::new(),
            visited_encounter_ids: Vec::new(),
            visited_encounter_set: BTreeSet::new(),
            objective_reached: false,
            finished: false,
        };
        if !cell_is_clear(
            &session.world.body.authored_layout,
            &session.world.body.fields,
            start_cell,
        ) {
            return Err(ReferenceRuntimeError::contract(format!(
                "reference runtime collided at route cell {start_cell}"
            )));
        }
        let step = session.make_step(
            0,
            start_cell,
            None,
            mark_objective_at_spawn && start_cell == world.body.navigation.objective_cell,
        );
        session.objective_reached = step.events.contains(&TraversalEventKind::ObjectiveReached);
        session.steps.push(step);
        Ok(session)
    }

    /// Apply one spatial input. `expected_tick` must be exactly the next tick;
    /// destinations must be clear, adjacent, and within the authored grade.
    /// Legal off-route motion is allowed, but completion still requires the
    /// objective and every required encounter.
    pub fn advance(
        &mut self,
        input: TraversalSessionInput,
    ) -> Result<&TraversalStep, ReferenceRuntimeError> {
        self.advance_with_objective_policy(input.expected_tick, input.destination_cell, true)
    }

    /// Return the current replayable state, sealed over all semantic fields.
    pub fn snapshot(&self) -> Result<TraversalSessionSnapshot, ReferenceRuntimeError> {
        let body = TraversalSessionSnapshotBody {
            schema_version: TRAVERSAL_SESSION_SNAPSHOT_SCHEMA.into(),
            world_artifact_id: self.world.artifact_id.clone(),
            world_artifact_sha256: self.world.artifact_sha256.clone(),
            steps: self.steps.clone(),
            visited_encounter_ids: self.visited_encounter_ids.clone(),
            objective_reached: self.objective_reached,
            finished: self.finished,
        };
        let state_sha256 = canonical_sha256(&body, "traversal session snapshot")?;
        Ok(TraversalSessionSnapshot { body, state_sha256 })
    }

    pub fn state_sha256(&self) -> Result<String, ReferenceRuntimeError> {
        self.snapshot().map(|snapshot| snapshot.state_sha256)
    }

    /// Restore only after replaying every step against the bound world.
    pub fn restore(
        world: &WorldArtifact,
        snapshot: &TraversalSessionSnapshot,
    ) -> Result<Self, ReferenceRuntimeError> {
        let digest = canonical_sha256(&snapshot.body, "traversal session snapshot")?;
        if digest != snapshot.state_sha256 {
            return Err(ReferenceRuntimeError::provenance(
                "traversal session state digest does not match its body".into(),
            ));
        }
        if snapshot.body.schema_version != TRAVERSAL_SESSION_SNAPSHOT_SCHEMA {
            return Err(ReferenceRuntimeError::contract(format!(
                "unsupported traversal session snapshot schema {:?}",
                snapshot.body.schema_version
            )));
        }
        if snapshot.body.world_artifact_id != world.artifact_id
            || snapshot.body.world_artifact_sha256 != world.artifact_sha256
        {
            return Err(ReferenceRuntimeError::provenance(
                "traversal session snapshot belongs to a different world artifact".into(),
            ));
        }
        if snapshot.body.steps.is_empty() || snapshot.body.steps.len() > MAX_TRAVERSAL_SESSION_STEPS
        {
            return Err(ReferenceRuntimeError::contract(format!(
                "traversal session snapshot step count must be 1..={MAX_TRAVERSAL_SESSION_STEPS}"
            )));
        }

        let mut restored = Self::initialize(world)?;
        if snapshot.body.steps[0] != restored.steps[0] {
            return Err(ReferenceRuntimeError::divergence(
                "traversal session initial step differs from the validated spawn state".into(),
            ));
        }
        for stored_step in snapshot.body.steps.iter().skip(1) {
            let replayed = restored
                .advance(TraversalSessionInput {
                    expected_tick: stored_step.tick,
                    destination_cell: stored_step.cell,
                })?
                .clone();
            if replayed != *stored_step {
                return Err(ReferenceRuntimeError::divergence(format!(
                    "traversal session replay diverged at tick {}",
                    stored_step.tick
                )));
            }
        }
        if snapshot.body.finished {
            restored.finish()?;
        }
        let replayed_snapshot = restored.snapshot()?;
        if replayed_snapshot != *snapshot {
            return Err(ReferenceRuntimeError::divergence(
                "traversal session snapshot differs from deterministic replay".into(),
            ));
        }
        Ok(restored)
    }

    /// Finish the traversal once and produce a digest-bound terminal summary.
    pub fn finish(&mut self) -> Result<TraversalSessionCompletion, ReferenceRuntimeError> {
        if self.finished {
            return Err(ReferenceRuntimeError::contract(
                "traversal session is already finished".into(),
            ));
        }
        self.finished = true;
        self.completion()
    }

    /// Recompute the terminal summary for a restored, already-finished session.
    pub fn completion(&self) -> Result<TraversalSessionCompletion, ReferenceRuntimeError> {
        if !self.finished {
            return Err(ReferenceRuntimeError::contract(
                "traversal session must be finished before requesting completion".into(),
            ));
        }
        let snapshot = self.snapshot()?;
        let final_step = self.steps.last().ok_or_else(|| {
            ReferenceRuntimeError::contract("traversal session has no initialized step".into())
        })?;
        let required_encounters_visited = self
            .world
            .body
            .encounters
            .iter()
            .filter(|encounter| encounter.required)
            .all(|encounter| self.visited_encounter_set.contains(&encounter.encounter_id));
        let outcome = if self.objective_reached && required_encounters_visited {
            TraversalOutcome::Completed
        } else {
            TraversalOutcome::Incomplete
        };
        let body = TraversalSessionCompletionBody {
            schema_version: TRAVERSAL_SESSION_COMPLETION_SCHEMA.into(),
            world_artifact_id: self.world.artifact_id.clone(),
            world_artifact_sha256: self.world.artifact_sha256.clone(),
            final_state_sha256: snapshot.state_sha256,
            final_tick: final_step.tick,
            final_cell: final_step.cell,
            objective_reached: self.objective_reached,
            outcome,
            visited_encounter_ids: self.visited_encounter_ids.clone(),
        };
        let completion_sha256 = canonical_sha256(&body, "traversal session completion")?;
        Ok(TraversalSessionCompletion {
            body,
            completion_sha256,
        })
    }

    pub fn current_cell(&self) -> Option<usize> {
        self.steps.last().map(|step| step.cell)
    }

    pub fn next_tick(&self) -> u64 {
        self.steps.len() as u64
    }

    pub fn steps(&self) -> &[TraversalStep] {
        &self.steps
    }

    pub fn visited_encounter_ids(&self) -> &[String] {
        &self.visited_encounter_ids
    }

    pub fn objective_reached(&self) -> bool {
        self.objective_reached
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn advance_with_objective_policy(
        &mut self,
        expected_tick: u64,
        destination_cell: usize,
        mark_objective_on_entry: bool,
    ) -> Result<&TraversalStep, ReferenceRuntimeError> {
        if self.finished {
            return Err(ReferenceRuntimeError::contract(
                "cannot advance a finished traversal session".into(),
            ));
        }
        let next_tick = self.next_tick();
        if expected_tick != next_tick {
            return Err(ReferenceRuntimeError::stale(format!(
                "expected traversal tick {next_tick}, received {expected_tick}"
            )));
        }
        if self.steps.len() >= MAX_TRAVERSAL_SESSION_STEPS {
            return Err(ReferenceRuntimeError::contract(format!(
                "traversal session exceeds the bounded {}-step history",
                MAX_TRAVERSAL_SESSION_STEPS
            )));
        }
        let previous = self.current_cell().ok_or_else(|| {
            ReferenceRuntimeError::contract("traversal session is not initialized".into())
        })?;
        if !cell_is_clear(
            &self.world.body.authored_layout,
            &self.world.body.fields,
            destination_cell,
        ) {
            return Err(ReferenceRuntimeError::contract(format!(
                "reference runtime collided at route cell {destination_cell}"
            )));
        }
        validate_adjacent_move(&self.world, previous, destination_cell)?;

        let mark_objective = mark_objective_on_entry
            && destination_cell == self.world.body.navigation.objective_cell
            && !self.objective_reached;
        let tick = expected_tick;
        let step = self.make_step(tick, destination_cell, Some(previous), mark_objective);
        if mark_objective {
            self.objective_reached = true;
        }
        self.steps.push(step);
        Ok(self.steps.last().expect("step was just appended"))
    }

    fn make_step(
        &mut self,
        tick: u64,
        cell: usize,
        previous: Option<usize>,
        mark_objective: bool,
    ) -> TraversalStep {
        let mut events = vec![TraversalEventKind::CellEntered];
        if previous.is_none() {
            events.push(TraversalEventKind::PlayerSpawnActivated);
        }
        let position = cell_position(&self.world.body.authored_layout, cell);
        for encounter in &self.world.body.encounters {
            if !self.visited_encounter_set.contains(&encounter.encounter_id)
                && crate::model::distance(position, encounter.center_xz_m) <= encounter.radius_m
            {
                self.visited_encounter_set
                    .insert(encounter.encounter_id.clone());
                self.visited_encounter_ids
                    .push(encounter.encounter_id.clone());
                events.push(TraversalEventKind::EncounterEntered);
            }
        }
        if mark_objective {
            events.push(TraversalEventKind::ObjectiveReached);
        }
        TraversalStep {
            tick,
            cell,
            position_xyz_m: [
                position[0],
                self.world.body.fields.heights_m[cell],
                position[1],
            ],
            events,
        }
    }
}

pub fn validate_traversal_session_completion(
    world: &WorldArtifact,
    snapshot: &TraversalSessionSnapshot,
    completion: &TraversalSessionCompletion,
) -> Result<(), ReferenceRuntimeError> {
    let completion_digest = canonical_sha256(&completion.body, "traversal session completion")?;
    if completion_digest != completion.completion_sha256 {
        return Err(ReferenceRuntimeError::provenance(
            "traversal session completion digest does not match its body".into(),
        ));
    }
    let restored = TraversalSession::restore(world, snapshot)?;
    let expected = restored.completion()?;
    if expected != *completion {
        return Err(ReferenceRuntimeError::divergence(
            "traversal session completion differs from deterministic replay".into(),
        ));
    }
    Ok(())
}

pub fn run_playthrough(world: &WorldArtifact) -> Result<TraversalEvidence, ReferenceRuntimeError> {
    let route = &world.body.navigation.route_cells;
    let mut session = TraversalSession::initialize_with_objective_policy(world, route.len() == 1)?;
    for (index, cell) in route.iter().copied().enumerate().skip(1) {
        // Preserve the batch contract: ObjectiveReached belongs to the final
        // authored route step, even if that route happens to cross the cell
        // earlier while visiting a required encounter.
        session.advance_with_objective_policy(index as u64, cell, index + 1 == route.len())?;
    }
    let completion = session.finish()?;
    let layout = &world.body.authored_layout;
    let start_spawn = layout
        .spawns
        .iter()
        .find(|spawn| spawn.spawn_id == layout.traversal.start_spawn_id)
        .ok_or_else(|| ReferenceRuntimeError::contract("player spawn is missing".into()))?;
    let body = TraversalEvidenceBody {
        schema_version: TRAVERSAL_EVIDENCE_SCHEMA.into(),
        validator_id: TRAVERSAL_VALIDATOR_ID.into(),
        world_artifact_id: world.artifact_id.clone(),
        world_artifact_sha256: world.artifact_sha256.clone(),
        player_spawn_id: start_spawn.spawn_id.clone(),
        objective_id: layout.traversal.objective_id.clone(),
        objective_reached: completion.body.objective_reached,
        outcome: completion.body.outcome,
        visited_encounter_ids: session.visited_encounter_ids.clone(),
        path_cells: route.clone(),
        steps: session.steps.clone(),
    };
    let evidence_sha256 = prefixed_sha256(&serde_json::to_vec(&body).map_err(|error| {
        ReferenceRuntimeError::contract(format!("traversal evidence encoding failed: {error}"))
    })?);
    Ok(TraversalEvidence {
        body,
        evidence_sha256,
    })
}

fn validate_route_anchors(world: &WorldArtifact) -> Result<(), ReferenceRuntimeError> {
    let route = &world.body.navigation.route_cells;
    if route.is_empty() || route[0] != world.body.navigation.start_cell {
        return Err(ReferenceRuntimeError::contract(
            "runtime route does not begin at the declared player spawn".into(),
        ));
    }
    if *route.last().unwrap_or(&usize::MAX) != world.body.navigation.objective_cell {
        return Err(ReferenceRuntimeError::contract(
            "runtime route does not end at the declared objective".into(),
        ));
    }
    Ok(())
}

fn validate_adjacent_move(
    world: &WorldArtifact,
    from: usize,
    to: usize,
) -> Result<(), ReferenceRuntimeError> {
    let resolution = world.body.authored_layout.resolution;
    let from_row = from / resolution;
    let from_column = from % resolution;
    let to_row = to / resolution;
    let to_column = to % resolution;
    let adjacent = from_row.abs_diff(to_row) + from_column.abs_diff(to_column) == 1;
    if !adjacent {
        return Err(ReferenceRuntimeError::contract(format!(
            "runtime route contains a non-adjacent move from cell {from} to {to}"
        )));
    }
    let layout = &world.body.authored_layout;
    let grade = edge_grade(layout, &world.body.fields, from, to);
    if grade > layout.traversal.maximum_grade {
        return Err(ReferenceRuntimeError::contract(format!(
            "runtime traversal exceeds the authored slope limit at cell {to}"
        )));
    }
    Ok(())
}

fn canonical_sha256<T: Serialize>(
    value: &T,
    context: &str,
) -> Result<String, ReferenceRuntimeError> {
    serde_json::to_vec(value)
        .map(|bytes| prefixed_sha256(&bytes))
        .map_err(|error| {
            ReferenceRuntimeError::contract(format!("{context} encoding failed: {error}"))
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
