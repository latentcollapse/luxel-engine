use serde::{Deserialize, Serialize};

use crate::{LAYOUT_SCHEMA, ReferenceRuntimeError};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AuthoredLayout {
    pub schema_version: String,
    pub world_id: String,
    pub title: String,
    pub width_m: f64,
    pub length_m: f64,
    pub resolution: usize,
    pub seed: u64,
    pub terrain: TerrainIntent,
    pub regions: Vec<SemanticRegion>,
    pub obstacles: Vec<ObstacleSpec>,
    pub spawns: Vec<SpawnSpec>,
    pub encounters: Vec<EncounterSpec>,
    pub traversal: TraversalIntent,
    pub reference_camera: ReferenceCamera,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TerrainIntent {
    pub base_elevation_m: f64,
    pub noise_amplitude_m: f64,
    pub features: Vec<TerrainFeature>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TerrainFeature {
    pub feature_id: String,
    pub center_xz_m: [f64; 2],
    pub radius_x_m: f64,
    pub radius_z_m: f64,
    pub elevation_m: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticRegion {
    pub region_id: String,
    pub code: u8,
    /// Lower numeric priorities take precedence where authored regions overlap.
    pub priority: i32,
    pub blocks_traversal: bool,
    pub polygon_xz_m: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ObstacleSpec {
    pub obstacle_id: String,
    pub center_xz_m: [f64; 2],
    pub radius_m: f64,
    pub height_m: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpawnRole {
    PlayerStart,
    Opponent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpawnSpec {
    pub spawn_id: String,
    pub role: SpawnRole,
    pub position_xz_m: [f64; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EncounterSpec {
    pub encounter_id: String,
    pub center_xz_m: [f64; 2],
    pub radius_m: f64,
    pub required: bool,
    pub traversal_order: u32,
    pub opponent_spawn_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraversalIntent {
    pub start_spawn_id: String,
    pub objective_id: String,
    pub objective_position_xz_m: [f64; 2],
    pub agent_radius_m: f64,
    pub maximum_grade: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReferenceCamera {
    pub projection: String,
    pub width_px: u32,
    pub height_px: u32,
    pub orthographic_span_m: f64,
    pub distance_m: f64,
}

pub fn validate_layout(layout: &AuthoredLayout) -> Result<(), ReferenceRuntimeError> {
    let invalid = |message: String| ReferenceRuntimeError::contract(message);
    if layout.schema_version != LAYOUT_SCHEMA {
        return Err(invalid(format!(
            "layout schema {:?} is not supported",
            layout.schema_version
        )));
    }
    valid_id(&layout.world_id, "world_id")?;
    if layout.title.trim().is_empty() {
        return Err(invalid("layout.title must not be empty".into()));
    }
    positive(layout.width_m, "layout.width_m")?;
    positive(layout.length_m, "layout.length_m")?;
    if !(17..=257).contains(&layout.resolution) || layout.resolution.is_multiple_of(2) {
        return Err(invalid(
            "layout.resolution must be an odd integer from 17 through 257".into(),
        ));
    }
    finite(layout.terrain.base_elevation_m, "terrain.base_elevation_m")?;
    nonnegative(
        layout.terrain.noise_amplitude_m,
        "terrain.noise_amplitude_m",
    )?;

    let mut feature_ids = std::collections::BTreeSet::new();
    for feature in &layout.terrain.features {
        valid_id(&feature.feature_id, "terrain feature id")?;
        if !feature_ids.insert(feature.feature_id.as_str()) {
            return Err(invalid(format!(
                "duplicate terrain feature id {}",
                feature.feature_id
            )));
        }
        point_in_bounds(feature.center_xz_m, layout, "terrain feature center")?;
        positive(feature.radius_x_m, "terrain feature radius_x_m")?;
        positive(feature.radius_z_m, "terrain feature radius_z_m")?;
        finite(feature.elevation_m, "terrain feature elevation_m")?;
        if feature.radius_x_m > layout.width_m * 2.0 || feature.radius_z_m > layout.length_m * 2.0 {
            return Err(invalid(format!(
                "terrain feature {} radius exceeds four world extents",
                feature.feature_id
            )));
        }
    }

    let mut region_ids = std::collections::BTreeSet::new();
    let mut region_codes = std::collections::BTreeSet::new();
    let mut priorities = std::collections::BTreeSet::new();
    for region in &layout.regions {
        valid_id(&region.region_id, "region id")?;
        if !region_ids.insert(region.region_id.as_str())
            || region.code == 0
            || !region_codes.insert(region.code)
            || !priorities.insert(region.priority)
        {
            return Err(invalid(format!(
                "region {} has a duplicate id, code, or precedence priority",
                region.region_id
            )));
        }
        if region.polygon_xz_m.len() < 3 {
            return Err(invalid(format!(
                "region {} polygon needs at least three points",
                region.region_id
            )));
        }
        for point in &region.polygon_xz_m {
            point_in_bounds(*point, layout, "region polygon point")?;
        }
    }

    let mut obstacle_ids = std::collections::BTreeSet::new();
    for obstacle in &layout.obstacles {
        valid_id(&obstacle.obstacle_id, "obstacle id")?;
        if !obstacle_ids.insert(obstacle.obstacle_id.as_str()) {
            return Err(invalid(format!(
                "duplicate obstacle id {}",
                obstacle.obstacle_id
            )));
        }
        point_in_bounds(obstacle.center_xz_m, layout, "obstacle center")?;
        positive(obstacle.radius_m, "obstacle.radius_m")?;
        positive(obstacle.height_m, "obstacle.height_m")?;
    }

    let mut spawn_ids = std::collections::BTreeSet::new();
    for spawn in &layout.spawns {
        valid_id(&spawn.spawn_id, "spawn id")?;
        if !spawn_ids.insert(spawn.spawn_id.as_str()) {
            return Err(invalid(format!("duplicate spawn id {}", spawn.spawn_id)));
        }
        point_in_bounds(spawn.position_xz_m, layout, "spawn position")?;
    }
    let start = layout
        .spawns
        .iter()
        .find(|spawn| spawn.spawn_id == layout.traversal.start_spawn_id)
        .ok_or_else(|| invalid("traversal start spawn is not declared".into()))?;
    if start.role != SpawnRole::PlayerStart {
        return Err(invalid(
            "traversal start spawn must have player_start role".into(),
        ));
    }
    if distance(
        start.position_xz_m,
        layout.traversal.objective_position_xz_m,
    ) <= layout.traversal.agent_radius_m
    {
        return Err(invalid(
            "player spawn and objective must occupy distinct world positions".into(),
        ));
    }
    point_in_bounds(
        layout.traversal.objective_position_xz_m,
        layout,
        "objective position",
    )?;
    valid_id(&layout.traversal.objective_id, "objective id")?;
    positive(layout.traversal.agent_radius_m, "traversal.agent_radius_m")?;
    nonnegative(layout.traversal.maximum_grade, "traversal.maximum_grade")?;

    let mut encounter_ids = std::collections::BTreeSet::new();
    let mut traversal_orders = std::collections::BTreeSet::new();
    for encounter in &layout.encounters {
        valid_id(&encounter.encounter_id, "encounter id")?;
        if !encounter_ids.insert(encounter.encounter_id.as_str())
            || !traversal_orders.insert(encounter.traversal_order)
        {
            return Err(invalid(format!(
                "encounter {} duplicates an id or traversal order",
                encounter.encounter_id
            )));
        }
        point_in_bounds(encounter.center_xz_m, layout, "encounter center")?;
        positive(encounter.radius_m, "encounter.radius_m")?;
        let actor = layout
            .spawns
            .iter()
            .find(|spawn| spawn.spawn_id == encounter.opponent_spawn_id)
            .ok_or_else(|| {
                invalid(format!(
                    "encounter {} references unknown opponent spawn {}",
                    encounter.encounter_id, encounter.opponent_spawn_id
                ))
            })?;
        if actor.role != SpawnRole::Opponent
            || distance(actor.position_xz_m, encounter.center_xz_m) > encounter.radius_m
        {
            return Err(invalid(format!(
                "encounter {} opponent spawn must be an opponent inside its volume",
                encounter.encounter_id
            )));
        }
    }
    if layout
        .encounters
        .iter()
        .filter(|encounter| encounter.required)
        .count()
        != 1
    {
        return Err(invalid(
            "the reference vertical slice requires exactly one required encounter".into(),
        ));
    }

    if layout.reference_camera.projection != "top_down_orthographic" {
        return Err(invalid(
            "reference camera projection must be top_down_orthographic".into(),
        ));
    }
    if !(32..=1024).contains(&layout.reference_camera.width_px)
        || !(32..=1024).contains(&layout.reference_camera.height_px)
    {
        return Err(invalid(
            "reference capture dimensions must be from 32 through 1024 pixels".into(),
        ));
    }
    positive(
        layout.reference_camera.orthographic_span_m,
        "reference_camera.orthographic_span_m",
    )?;
    positive(
        layout.reference_camera.distance_m,
        "reference_camera.distance_m",
    )?;
    if layout.reference_camera.orthographic_span_m < layout.width_m
        || layout.reference_camera.orthographic_span_m < layout.length_m
    {
        return Err(invalid(
            "reference camera span must include the full authored world".into(),
        ));
    }
    Ok(())
}

pub(crate) fn finite(value: f64, label: &str) -> Result<(), ReferenceRuntimeError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ReferenceRuntimeError::contract(format!(
            "{label} must be finite"
        )))
    }
}

pub(crate) fn positive(value: f64, label: &str) -> Result<(), ReferenceRuntimeError> {
    finite(value, label)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(ReferenceRuntimeError::contract(format!(
            "{label} must be positive"
        )))
    }
}

pub(crate) fn nonnegative(value: f64, label: &str) -> Result<(), ReferenceRuntimeError> {
    finite(value, label)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(ReferenceRuntimeError::contract(format!(
            "{label} must not be negative"
        )))
    }
}

pub(crate) fn valid_id(value: &str, label: &str) -> Result<(), ReferenceRuntimeError> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(ReferenceRuntimeError::contract(format!(
            "{label} must be a 1-96 character ASCII identifier"
        )));
    }
    Ok(())
}

pub(crate) fn point_in_bounds(
    point: [f64; 2],
    layout: &AuthoredLayout,
    label: &str,
) -> Result<(), ReferenceRuntimeError> {
    finite(point[0], label)?;
    finite(point[1], label)?;
    if point[0].abs() > layout.width_m / 2.0 || point[1].abs() > layout.length_m / 2.0 {
        return Err(ReferenceRuntimeError::contract(format!(
            "{label} lies outside authored world bounds"
        )));
    }
    Ok(())
}

pub(crate) fn distance(left: [f64; 2], right: [f64; 2]) -> f64 {
    (left[0] - right[0]).hypot(left[1] - right[1])
}
