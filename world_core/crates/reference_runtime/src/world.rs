use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fields::{
    JuliaFieldProvenance, JuliaFieldRequest, JuliaFieldResponse, NumericalFields,
    fields_from_response, is_sha256, prefixed_sha256, spatial_fields_sha256, validate_height_field,
};
use crate::model::distance;
use crate::{
    AuthoredLayout, ReferenceRuntimeError, SemanticRegion, SpawnRole, WORLD_SCHEMA, validate_layout,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CollisionArtifact {
    pub schema_version: String,
    pub world_id: String,
    pub spatial_fields_sha256: String,
    pub world_bounds_m: [f64; 2],
    pub agent_radius_m: f64,
    pub obstacles: Vec<crate::ObstacleSpec>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaypointKind {
    Encounter,
    Objective,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NavigationSegment {
    pub target_kind: WaypointKind,
    pub target_id: String,
    pub path_cells: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NavigationArtifact {
    pub schema_version: String,
    pub world_id: String,
    pub spatial_fields_sha256: String,
    pub agent_radius_m: f64,
    pub maximum_grade: f64,
    pub traversable_cell_count: usize,
    pub start_cell: usize,
    pub objective_cell: usize,
    pub segments: Vec<NavigationSegment>,
    pub route_cells: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpawnArtifact {
    pub spawn_id: String,
    pub role: SpawnRole,
    pub position_xz_m: [f64; 2],
    pub ground_y_m: f64,
    pub grid_cell: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EncounterArtifact {
    pub encounter_id: String,
    pub center_xz_m: [f64; 2],
    pub radius_m: f64,
    pub required: bool,
    pub traversal_order: u32,
    pub opponent_spawn_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldArtifactBody {
    pub schema_version: String,
    pub world_id: String,
    pub title: String,
    pub authored_layout_sha256: String,
    pub authored_layout: AuthoredLayout,
    pub fields: NumericalFields,
    pub julia_provenance: JuliaFieldProvenance,
    pub collision: CollisionArtifact,
    pub navigation: NavigationArtifact,
    pub spawns: Vec<SpawnArtifact>,
    pub encounters: Vec<EncounterArtifact>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorldArtifact {
    pub schema_version: String,
    pub artifact_id: String,
    pub artifact_sha256: String,
    pub body: WorldArtifactBody,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldBuild {
    pub world: WorldArtifact,
    pub traversal: crate::TraversalEvidence,
    pub capture_bytes: Vec<u8>,
    pub visual: crate::VisualEvidence,
    pub gameplay: crate::GameplayWorldBinding,
}

impl WorldBuild {
    /// The world slice is green only when both independently replayable runtime
    /// evidence and the measurable reference visual gate pass.
    pub fn reference_gates_passed(&self) -> bool {
        self.traversal.body.outcome == crate::TraversalOutcome::Completed
            && self.visual.body.status == crate::VisualGateStatus::Passed
            && self.gameplay.body.outcome == wge_gameplay_contract::GameOutcome::Won
    }
}

#[derive(Clone, Debug)]
struct Waypoint {
    kind: WaypointKind,
    id: String,
    position: [f64; 2],
}

#[derive(Clone, Copy, Debug)]
struct QueueEntry {
    cost: f64,
    index: usize,
}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cost.to_bits() == other.cost.to_bits() && self.index == other.index
    }
}

impl Eq for QueueEntry {}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.index.cmp(&self.index))
    }
}

pub fn build_world(
    layout: AuthoredLayout,
    layout_sha256: String,
    fields: NumericalFields,
    julia_provenance: JuliaFieldProvenance,
) -> Result<WorldArtifact, ReferenceRuntimeError> {
    validate_layout(&layout)?;
    if !is_sha256(&layout_sha256) || canonical_layout_sha(&layout)? != layout_sha256 {
        return Err(ReferenceRuntimeError::provenance(
            "authored layout digest does not match its typed contents".into(),
        ));
    }
    validate_fields_and_provenance(&layout, &layout_sha256, &fields, &julia_provenance)?;
    let collision = collision_artifact(&layout, &fields);
    let (spawns, encounters) = spawn_and_encounter_artifacts(&layout, &fields)?;
    let navigation = navigation_artifact(&layout, &fields)?;
    let body = WorldArtifactBody {
        schema_version: WORLD_SCHEMA.into(),
        world_id: layout.world_id.clone(),
        title: layout.title.clone(),
        authored_layout_sha256: layout_sha256,
        authored_layout: layout,
        fields,
        julia_provenance,
        collision,
        navigation,
        spawns,
        encounters,
    };
    let artifact = seal_world(body)?;
    validate_world_artifact(&artifact)?;
    Ok(artifact)
}

pub fn validate_world_artifact(artifact: &WorldArtifact) -> Result<(), ReferenceRuntimeError> {
    if artifact.schema_version != WORLD_SCHEMA || artifact.body.schema_version != WORLD_SCHEMA {
        return Err(ReferenceRuntimeError::contract(
            "world artifact schema version is unsupported".into(),
        ));
    }
    let digest = prefixed_sha256(&serde_json::to_vec(&artifact.body).map_err(|error| {
        ReferenceRuntimeError::contract(format!("world artifact serialization failed: {error}"))
    })?);
    if digest != artifact.artifact_sha256
        || artifact.artifact_id != format!("world-{}", digest.trim_start_matches("sha256:"))
    {
        return Err(ReferenceRuntimeError::provenance(
            "world artifact identity does not match its canonical body".into(),
        ));
    }
    let layout = &artifact.body.authored_layout;
    validate_layout(layout)?;
    if artifact.body.world_id != layout.world_id
        || artifact.body.title != layout.title
        || canonical_layout_sha(layout)? != artifact.body.authored_layout_sha256
    {
        return Err(ReferenceRuntimeError::provenance(
            "embedded authored layout does not match world identity".into(),
        ));
    }
    validate_fields_and_provenance(
        layout,
        &artifact.body.authored_layout_sha256,
        &artifact.body.fields,
        &artifact.body.julia_provenance,
    )?;
    let expected_collision = collision_artifact(layout, &artifact.body.fields);
    let (expected_spawns, expected_encounters) =
        spawn_and_encounter_artifacts(layout, &artifact.body.fields)?;
    let expected_navigation = navigation_artifact(layout, &artifact.body.fields)?;
    if artifact.body.collision != expected_collision {
        return Err(ReferenceRuntimeError::contract(
            "collision artifact does not match authored blockers and terrain identity".into(),
        ));
    }
    if artifact.body.spawns != expected_spawns || artifact.body.encounters != expected_encounters {
        return Err(ReferenceRuntimeError::contract(
            "spawn or encounter artifact differs from its authored semantic declaration".into(),
        ));
    }
    if artifact.body.navigation != expected_navigation {
        return Err(ReferenceRuntimeError::contract(
            "navigation artifact does not match traversability of the numeric world fields".into(),
        ));
    }
    verify_worker_identity(&artifact.body.julia_provenance)?;
    Ok(())
}

pub(crate) fn canonical_layout_sha(
    layout: &AuthoredLayout,
) -> Result<String, ReferenceRuntimeError> {
    serde_json::to_vec(layout)
        .map(|bytes| prefixed_sha256(&bytes))
        .map_err(|error| {
            ReferenceRuntimeError::contract(format!("layout encoding failed: {error}"))
        })
}

fn seal_world(body: WorldArtifactBody) -> Result<WorldArtifact, ReferenceRuntimeError> {
    let digest = prefixed_sha256(&serde_json::to_vec(&body).map_err(|error| {
        ReferenceRuntimeError::contract(format!("world body serialization failed: {error}"))
    })?);
    Ok(WorldArtifact {
        schema_version: WORLD_SCHEMA.into(),
        artifact_id: format!("world-{}", digest.trim_start_matches("sha256:")),
        artifact_sha256: digest,
        body,
    })
}

fn validate_fields_and_provenance(
    layout: &AuthoredLayout,
    layout_sha256: &str,
    fields: &NumericalFields,
    provenance: &JuliaFieldProvenance,
) -> Result<(), ReferenceRuntimeError> {
    if fields.resolution != layout.resolution
        || fields.heights_m.len() != layout.resolution * layout.resolution
        || fields.slope_grade.len() != layout.resolution * layout.resolution
        || fields.region_codes.len() != layout.resolution * layout.resolution
        || fields
            .heights_m
            .iter()
            .chain(&fields.slope_grade)
            .any(|value| !value.is_finite())
    {
        return Err(ReferenceRuntimeError::contract(
            "terrain field shape or finite-value constraints are invalid".into(),
        ));
    }
    if spatial_fields_sha256(fields) != fields.spatial_sha256 {
        return Err(ReferenceRuntimeError::provenance(
            "terrain field identity does not match numerical bytes".into(),
        ));
    }
    validate_height_field(layout, &fields.heights_m)?;
    if provenance.schema_version != "wge.julia-world-fields-provenance/v1"
        || provenance.worker_id != "terrain_lab/bin/wge_reference_world_fields.jl"
        || !is_sha256(&provenance.request_sha256)
        || !is_sha256(&provenance.response_sha256)
        || !is_sha256(&provenance.worker_script_sha256)
        || !is_sha256(&provenance.terrain_project_sha256)
        || !provenance.julia_version.starts_with("julia version ")
    {
        return Err(ReferenceRuntimeError::provenance(
            "Julia field provenance has an unknown worker or malformed identity".into(),
        ));
    }
    let expected_request = JuliaFieldRequest::new(layout, layout_sha256);
    let expected_request_json = serde_json::to_vec(&expected_request).map_err(|error| {
        ReferenceRuntimeError::contract(format!("Julia request serialization failed: {error}"))
    })?;
    if provenance.request_json.as_bytes() != expected_request_json
        || prefixed_sha256(&expected_request_json) != provenance.request_sha256
    {
        return Err(ReferenceRuntimeError::provenance(
            "stored Julia request bytes do not describe this authored layout".into(),
        ));
    }
    if prefixed_sha256(provenance.response_json.as_bytes()) != provenance.response_sha256 {
        return Err(ReferenceRuntimeError::provenance(
            "stored Julia response bytes do not match their receipt digest".into(),
        ));
    }
    let response: JuliaFieldResponse =
        serde_json::from_str(&provenance.response_json).map_err(|error| {
            ReferenceRuntimeError::provenance(format!(
                "stored Julia response violates the closed schema: {error}"
            ))
        })?;
    let request: JuliaFieldRequest =
        serde_json::from_slice(&expected_request_json).map_err(|error| {
            ReferenceRuntimeError::contract(format!("generated Julia request is invalid: {error}"))
        })?;
    let response_fields = fields_from_response(&request, &provenance.request_sha256, &response)?;
    if response_fields != *fields {
        return Err(ReferenceRuntimeError::provenance(
            "Julia response fields do not match the world artifact field arrays".into(),
        ));
    }
    validate_julia_gradient(layout, fields)?;
    validate_julia_regions(layout, fields)?;
    Ok(())
}

fn validate_julia_gradient(
    layout: &AuthoredLayout,
    fields: &NumericalFields,
) -> Result<(), ReferenceRuntimeError> {
    let resolution = layout.resolution;
    let dx = layout.width_m / (resolution - 1) as f64;
    let dz = layout.length_m / (resolution - 1) as f64;
    for row in 0..resolution {
        for column in 0..resolution {
            let index = row * resolution + column;
            let height = fields.heights_m[index];
            let mut expected = 0.0_f64;
            if column > 0 {
                expected = expected.max((height - fields.heights_m[index - 1]).abs() / dx);
            }
            if column + 1 < resolution {
                expected = expected.max((height - fields.heights_m[index + 1]).abs() / dx);
            }
            if row > 0 {
                expected = expected.max((height - fields.heights_m[index - resolution]).abs() / dz);
            }
            if row + 1 < resolution {
                expected = expected.max((height - fields.heights_m[index + resolution]).abs() / dz);
            }
            if (expected - fields.slope_grade[index]).abs() > 1e-10 * (1.0 + expected.abs()) {
                return Err(ReferenceRuntimeError::provenance(format!(
                    "Julia slope field disagrees with Rust recomputation at cell {index}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_julia_regions(
    layout: &AuthoredLayout,
    fields: &NumericalFields,
) -> Result<(), ReferenceRuntimeError> {
    let resolution = layout.resolution;
    let mut regions = layout.regions.iter().collect::<Vec<_>>();
    regions.sort_by_key(|region| region.priority);
    for row in 0..resolution {
        let z = layout.length_m / 2.0 - row as f64 * layout.length_m / (resolution - 1) as f64;
        for column in 0..resolution {
            let x =
                -layout.width_m / 2.0 + column as f64 * layout.width_m / (resolution - 1) as f64;
            let expected = regions
                .iter()
                .find(|region| point_in_polygon([x, z], region))
                .map(|region| region.code)
                .unwrap_or(0);
            let index = row * resolution + column;
            if fields.region_codes[index] != expected {
                return Err(ReferenceRuntimeError::provenance(format!(
                    "Julia semantic region field disagrees with authored polygons at cell {index}"
                )));
            }
        }
    }
    Ok(())
}

fn collision_artifact(layout: &AuthoredLayout, fields: &NumericalFields) -> CollisionArtifact {
    CollisionArtifact {
        schema_version: "wge.collision-world/v1".into(),
        world_id: layout.world_id.clone(),
        spatial_fields_sha256: fields.spatial_sha256.clone(),
        world_bounds_m: [layout.width_m, layout.length_m],
        agent_radius_m: layout.traversal.agent_radius_m,
        obstacles: layout.obstacles.clone(),
    }
}

fn spawn_and_encounter_artifacts(
    layout: &AuthoredLayout,
    fields: &NumericalFields,
) -> Result<(Vec<SpawnArtifact>, Vec<EncounterArtifact>), ReferenceRuntimeError> {
    let mut spawns = layout
        .spawns
        .iter()
        .map(|spawn| {
            let cell = cell_for_position(layout, spawn.position_xz_m);
            require_cell_clear(layout, fields, cell, &spawn.spawn_id)?;
            Ok(SpawnArtifact {
                spawn_id: spawn.spawn_id.clone(),
                role: spawn.role,
                position_xz_m: spawn.position_xz_m,
                ground_y_m: fields.heights_m[cell],
                grid_cell: cell,
            })
        })
        .collect::<Result<Vec<_>, ReferenceRuntimeError>>()?;
    spawns.sort_by(|left, right| left.spawn_id.cmp(&right.spawn_id));
    let mut encounters = layout
        .encounters
        .iter()
        .map(|encounter| {
            let cell = cell_for_position(layout, encounter.center_xz_m);
            require_cell_clear(layout, fields, cell, &encounter.encounter_id)?;
            Ok(EncounterArtifact {
                encounter_id: encounter.encounter_id.clone(),
                center_xz_m: encounter.center_xz_m,
                radius_m: encounter.radius_m,
                required: encounter.required,
                traversal_order: encounter.traversal_order,
                opponent_spawn_id: encounter.opponent_spawn_id.clone(),
            })
        })
        .collect::<Result<Vec<_>, ReferenceRuntimeError>>()?;
    encounters.sort_by_key(|encounter| encounter.traversal_order);
    Ok((spawns, encounters))
}

fn navigation_artifact(
    layout: &AuthoredLayout,
    fields: &NumericalFields,
) -> Result<NavigationArtifact, ReferenceRuntimeError> {
    let start_spawn = layout
        .spawns
        .iter()
        .find(|spawn| spawn.spawn_id == layout.traversal.start_spawn_id)
        .ok_or_else(|| ReferenceRuntimeError::contract("start spawn disappeared".into()))?;
    let start_cell = cell_for_position(layout, start_spawn.position_xz_m);
    let objective_cell = cell_for_position(layout, layout.traversal.objective_position_xz_m);
    require_cell_clear(layout, fields, start_cell, &layout.traversal.start_spawn_id)?;
    require_cell_clear(
        layout,
        fields,
        objective_cell,
        &layout.traversal.objective_id,
    )?;

    let mut waypoints = layout
        .encounters
        .iter()
        .map(|encounter| {
            let actor_position = layout
                .spawns
                .iter()
                .find(|spawn| spawn.spawn_id == encounter.opponent_spawn_id)
                .map(|spawn| spawn.position_xz_m)
                .unwrap_or(encounter.center_xz_m);
            Waypoint {
                kind: WaypointKind::Encounter,
                id: encounter.encounter_id.clone(),
                position: actor_position,
            }
        })
        .collect::<Vec<_>>();
    waypoints.sort_by_key(|waypoint| {
        layout
            .encounters
            .iter()
            .find(|encounter| encounter.encounter_id == waypoint.id)
            .map(|encounter| encounter.traversal_order)
            .unwrap_or(u32::MAX)
    });
    waypoints.push(Waypoint {
        kind: WaypointKind::Objective,
        id: layout.traversal.objective_id.clone(),
        position: layout.traversal.objective_position_xz_m,
    });

    let traversable_cell_count = (0..fields.heights_m.len())
        .filter(|cell| cell_is_clear(layout, fields, *cell))
        .count();
    let mut segments = Vec::new();
    let mut route_cells = Vec::new();
    let mut from_cell = start_cell;
    for waypoint in waypoints {
        let to_cell = cell_for_position(layout, waypoint.position);
        require_cell_clear(layout, fields, to_cell, &waypoint.id)?;
        let path = shortest_path(layout, fields, from_cell, to_cell).ok_or_else(|| {
            ReferenceRuntimeError::contract(format!(
                "no collision-clear route from cell {from_cell} to {} waypoint {}",
                match waypoint.kind {
                    WaypointKind::Encounter => "encounter",
                    WaypointKind::Objective => "objective",
                },
                waypoint.id
            ))
        })?;
        if route_cells.is_empty() {
            route_cells.extend(path.iter().copied());
        } else {
            route_cells.extend(path.iter().skip(1).copied());
        }
        segments.push(NavigationSegment {
            target_kind: waypoint.kind,
            target_id: waypoint.id,
            path_cells: path,
        });
        from_cell = to_cell;
    }
    if route_cells.is_empty() {
        return Err(ReferenceRuntimeError::contract(
            "navigation route has no cells".into(),
        ));
    }
    Ok(NavigationArtifact {
        schema_version: "wge.navigation-grid/v1".into(),
        world_id: layout.world_id.clone(),
        spatial_fields_sha256: fields.spatial_sha256.clone(),
        agent_radius_m: layout.traversal.agent_radius_m,
        maximum_grade: layout.traversal.maximum_grade,
        traversable_cell_count,
        start_cell,
        objective_cell,
        segments,
        route_cells,
    })
}

fn shortest_path(
    layout: &AuthoredLayout,
    fields: &NumericalFields,
    start: usize,
    goal: usize,
) -> Option<Vec<usize>> {
    if !cell_is_clear(layout, fields, start) || !cell_is_clear(layout, fields, goal) {
        return None;
    }
    let count = fields.heights_m.len();
    let mut distances = vec![f64::INFINITY; count];
    let mut previous = vec![None; count];
    let mut queue = BinaryHeap::new();
    distances[start] = 0.0;
    queue.push(QueueEntry {
        cost: 0.0,
        index: start,
    });
    while let Some(entry) = queue.pop() {
        if entry.cost > distances[entry.index] {
            continue;
        }
        if entry.index == goal {
            break;
        }
        let row = entry.index / layout.resolution;
        let column = entry.index % layout.resolution;
        let neighbors = [
            row.checked_sub(1)
                .map(|next| next * layout.resolution + column),
            (column + 1 < layout.resolution).then_some(row * layout.resolution + column + 1),
            (row + 1 < layout.resolution).then_some((row + 1) * layout.resolution + column),
            column
                .checked_sub(1)
                .map(|next| row * layout.resolution + next),
        ];
        for neighbor in neighbors.into_iter().flatten() {
            if !cell_is_clear(layout, fields, neighbor) {
                continue;
            }
            let grade = edge_grade(layout, fields, entry.index, neighbor);
            if grade > layout.traversal.maximum_grade {
                continue;
            }
            let step = if row == neighbor / layout.resolution {
                layout.width_m / (layout.resolution - 1) as f64
            } else {
                layout.length_m / (layout.resolution - 1) as f64
            };
            let candidate = entry.cost + step * (1.0 + grade * grade);
            if candidate + 1e-12 < distances[neighbor] {
                distances[neighbor] = candidate;
                previous[neighbor] = Some(entry.index);
                queue.push(QueueEntry {
                    cost: candidate,
                    index: neighbor,
                });
            }
        }
    }
    if !distances[goal].is_finite() {
        return None;
    }
    let mut path = vec![goal];
    let mut cursor = goal;
    while cursor != start {
        cursor = previous[cursor]?;
        path.push(cursor);
    }
    path.reverse();
    Some(path)
}

pub(crate) fn cell_is_clear(
    layout: &AuthoredLayout,
    fields: &NumericalFields,
    cell: usize,
) -> bool {
    if cell >= fields.heights_m.len() {
        return false;
    }
    let position = cell_position(layout, cell);
    let radius = layout.traversal.agent_radius_m;
    if position[0] < -layout.width_m / 2.0 + radius
        || position[0] > layout.width_m / 2.0 - radius
        || position[1] < -layout.length_m / 2.0 + radius
        || position[1] > layout.length_m / 2.0 - radius
    {
        return false;
    }
    if layout
        .regions
        .iter()
        .find(|region| region.code == fields.region_codes[cell])
        .is_some_and(|region| region.blocks_traversal)
    {
        return false;
    }
    !layout.obstacles.iter().any(|obstacle| {
        distance(position, obstacle.center_xz_m)
            < obstacle.radius_m + layout.traversal.agent_radius_m
    })
}

pub(crate) fn edge_grade(
    layout: &AuthoredLayout,
    fields: &NumericalFields,
    from: usize,
    to: usize,
) -> f64 {
    let from_row = from / layout.resolution;
    let to_row = to / layout.resolution;
    let spacing = if from_row == to_row {
        layout.width_m / (layout.resolution - 1) as f64
    } else {
        layout.length_m / (layout.resolution - 1) as f64
    };
    (fields.heights_m[from] - fields.heights_m[to]).abs() / spacing
}

pub(crate) fn cell_position(layout: &AuthoredLayout, cell: usize) -> [f64; 2] {
    let row = cell / layout.resolution;
    let column = cell % layout.resolution;
    [
        -layout.width_m / 2.0 + column as f64 * layout.width_m / (layout.resolution - 1) as f64,
        layout.length_m / 2.0 - row as f64 * layout.length_m / (layout.resolution - 1) as f64,
    ]
}

pub(crate) fn cell_for_position(layout: &AuthoredLayout, position: [f64; 2]) -> usize {
    // Authored positions were range-checked before this projection.
    let column = ((position[0] + layout.width_m / 2.0) / layout.width_m
        * (layout.resolution - 1) as f64)
        .round() as usize;
    let row = ((layout.length_m / 2.0 - position[1]) / layout.length_m
        * (layout.resolution - 1) as f64)
        .round() as usize;
    row.min(layout.resolution - 1) * layout.resolution + column.min(layout.resolution - 1)
}

pub(crate) fn require_cell_clear(
    layout: &AuthoredLayout,
    fields: &NumericalFields,
    cell: usize,
    subject: &str,
) -> Result<(), ReferenceRuntimeError> {
    if !cell_is_clear(layout, fields, cell) {
        Err(ReferenceRuntimeError::contract(format!(
            "{subject} maps to a blocked or colliding terrain cell {cell}"
        )))
    } else {
        Ok(())
    }
}

fn point_in_polygon(point: [f64; 2], region: &SemanticRegion) -> bool {
    let mut inside = false;
    let mut previous = region.polygon_xz_m.len() - 1;
    for current in 0..region.polygon_xz_m.len() {
        let [xi, zi] = region.polygon_xz_m[current];
        let [xj, zj] = region.polygon_xz_m[previous];
        if ((zi > point[1]) != (zj > point[1]))
            && (point[0] < (xj - xi) * (point[1] - zi) / (zj - zi) + xi)
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn verify_worker_identity(provenance: &JuliaFieldProvenance) -> Result<(), ReferenceRuntimeError> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or_else(|| {
            ReferenceRuntimeError::provenance("crate repository root is unavailable".into())
        })?;
    let worker = repo.join("terrain_lab/bin/wge_reference_world_fields.jl");
    let project = repo.join("terrain_lab/Project.toml");
    let manifest = repo.join("terrain_lab/Manifest.toml");
    let worker_bytes = fs::read(&worker).map_err(|error| {
        ReferenceRuntimeError::provenance(format!("trusted Julia worker is unavailable: {error}"))
    })?;
    if prefixed_sha256(&worker_bytes) != provenance.worker_script_sha256 {
        return Err(ReferenceRuntimeError::provenance(
            "Julia worker receipt does not bind to the trusted worker source".into(),
        ));
    }
    let mut project_bytes = fs::read(&project).map_err(|error| {
        ReferenceRuntimeError::provenance(format!(
            "trusted terrain project is unavailable: {error}"
        ))
    })?;
    if manifest.exists() {
        project_bytes.extend_from_slice(b"\0Manifest.toml\0");
        project_bytes.extend(fs::read(&manifest).map_err(|error| {
            ReferenceRuntimeError::provenance(format!(
                "trusted terrain manifest is unavailable: {error}"
            ))
        })?);
    }
    if prefixed_sha256(&project_bytes) != provenance.terrain_project_sha256 {
        return Err(ReferenceRuntimeError::provenance(
            "Julia worker receipt does not bind to the trusted terrain project".into(),
        ));
    }
    Ok(())
}
