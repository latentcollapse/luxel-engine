//! Rust-owned semantic checks for the MVP world artifacts.
//!
//! The project ledger does not attempt to become a general terrain engine. It
//! does, however, refuse to call a JSON file a connected playable world merely
//! because its bytes are present and hashed. This bounded validator checks the
//! terrain, collision, and navigation contracts used by the Gate Run slice.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::{ArtifactRef, LedgerError, ProjectSpec, sha256_prefixed, validate_relative_path};
use luxel_reference_runtime::{
    TRAVERSAL_EVIDENCE_SCHEMA, TraversalEvidence, WORLD_SCHEMA, WorldArtifact,
    validate_traversal_evidence, validate_world_artifact,
};

pub fn validate_world_bundle(root: &Path, spec: &ProjectSpec) -> Result<(), LedgerError> {
    if let Some(bundle) = &spec.world.reference_runtime {
        return validate_reference_runtime_bundle(root, spec, bundle);
    }
    let terrain = read_artifact_json(root, &spec.world.terrain, "terrain")?;
    let collision = read_artifact_json(root, &spec.world.collision, "collision")?;
    let navigation = read_artifact_json(root, &spec.world.navigation, "navigation")?;
    validate_terrain(&terrain, spec)?;
    validate_collision(&collision, spec)?;
    validate_navigation(&navigation, spec)?;
    Ok(())
}

fn validate_reference_runtime_bundle(
    root: &Path,
    spec: &ProjectSpec,
    bundle: &crate::ReferenceRuntimeWorldBundle,
) -> Result<(), LedgerError> {
    let world = read_reference_artifact::<WorldArtifact>(
        root,
        &bundle.world_artifact,
        "reference world",
        "reference_world",
        WORLD_SCHEMA,
    )?;
    if bundle.world_artifact.artifact_id != world.artifact_id {
        return Err(LedgerError::Provenance(
            "reference world artifact ID does not match the sealed runtime artifact".into(),
        ));
    }
    validate_world_artifact(&world).map_err(runtime_error)?;

    let traversal = read_reference_artifact::<TraversalEvidence>(
        root,
        &bundle.traversal_evidence,
        "reference traversal",
        "traversal_evidence",
        TRAVERSAL_EVIDENCE_SCHEMA,
    )?;
    let traversal_id = format!(
        "traversal-{}",
        traversal.evidence_sha256.trim_start_matches("sha256:")
    );
    if bundle.traversal_evidence.artifact_id != traversal_id {
        return Err(LedgerError::Provenance(
            "traversal artifact ID does not match the sealed runtime evidence".into(),
        ));
    }
    validate_traversal_evidence(&world, &traversal).map_err(runtime_error)?;

    let layout = &world.body.authored_layout;
    if spec.world.world_id != world.body.world_id
        || !same_measure(spec.world.dimensions_m[0], layout.width_m)
        || !same_measure(spec.world.dimensions_m[1], layout.length_m)
    {
        return Err(LedgerError::Provenance(
            "project world identity or dimensions do not match the reference runtime artifact"
                .into(),
        ));
    }
    if spec.world.objective.objective_id != layout.traversal.objective_id
        || spec.gameplay.objective_id != layout.traversal.objective_id
        || spec.gameplay.start_entity_id != layout.traversal.start_spawn_id
        || traversal.body.world_artifact_id != world.artifact_id
        || traversal.body.world_artifact_sha256 != world.artifact_sha256
        || traversal.body.objective_id != spec.world.objective.objective_id
    {
        return Err(LedgerError::Provenance(
            "project objective, start spawn, and traversal identities do not match the runtime world"
                .into(),
        ));
    }

    let authored_spawns = &layout.spawns;
    if authored_spawns.len() != spec.world.spawns.len() {
        return Err(LedgerError::Contract(
            "project spawn set does not match the authored reference world".into(),
        ));
    }
    for spawn in &spec.world.spawns {
        let Some(native) = authored_spawns
            .iter()
            .find(|native| native.spawn_id == spawn.spawn_id)
        else {
            return Err(LedgerError::Provenance(format!(
                "project spawn {} is absent from the reference world",
                spawn.spawn_id
            )));
        };
        let expected_team = match native.role {
            luxel_reference_runtime::SpawnRole::PlayerStart => "player",
            luxel_reference_runtime::SpawnRole::Opponent => "opponent",
        };
        if spawn.team != expected_team
            || !same_measure(spawn.position_xz_m[0], native.position_xz_m[0])
            || !same_measure(spawn.position_xz_m[1], native.position_xz_m[1])
        {
            return Err(LedgerError::Provenance(format!(
                "project spawn {} differs from its authored runtime identity",
                spawn.spawn_id
            )));
        }
    }
    Ok(())
}

fn read_reference_artifact<T: serde::de::DeserializeOwned>(
    root: &Path,
    artifact: &ArtifactRef,
    label: &str,
    expected_kind: &str,
    expected_schema: &str,
) -> Result<T, LedgerError> {
    if artifact.kind != expected_kind
        || artifact.schema_version != expected_schema
        || artifact.producer != "luxel-reference-runtime"
    {
        return Err(LedgerError::Contract(format!(
            "{label} reference must name the registered native runtime schema, kind, and producer"
        )));
    }
    validate_relative_path(&artifact.path, &format!("{label} artifact path"))?;
    let canonical_root = root
        .canonicalize()
        .map_err(|error| LedgerError::Io(format!("{}: {error}", root.display())))?;
    let path = canonical_root.join(Path::new(&artifact.path));
    let canonical_path = path
        .canonicalize()
        .map_err(|error| LedgerError::Io(format!("{}: {error}", path.display())))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(LedgerError::Contract(format!(
            "{label} artifact path escapes the candidate root: {}",
            artifact.path
        )));
    }
    let bytes = fs::read(&canonical_path)
        .map_err(|error| LedgerError::Io(format!("{}: {error}", canonical_path.display())))?;
    if sha256_prefixed(&bytes) != artifact.sha256 {
        return Err(LedgerError::Provenance(format!(
            "{label} file digest does not match its declared artifact reference"
        )));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| LedgerError::Json(format!("{}: {error}", canonical_path.display())))
}

fn runtime_error(error: luxel_reference_runtime::ReferenceRuntimeError) -> LedgerError {
    match error.code {
        "provenance_failure" => LedgerError::Provenance(error.to_string()),
        _ => LedgerError::Contract(error.to_string()),
    }
}

fn same_measure(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-9
}

fn read_artifact_json(
    root: &Path,
    artifact: &ArtifactRef,
    label: &str,
) -> Result<Value, LedgerError> {
    let relative = Path::new(&artifact.path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(LedgerError::Contract(format!(
            "{label} artifact path escapes the candidate root: {}",
            artifact.path
        )));
    }
    let path = root.join(relative);
    let bytes =
        fs::read(&path).map_err(|error| LedgerError::Io(format!("{}: {error}", path.display())))?;
    if sha256_prefixed(&bytes) != artifact.sha256 {
        return Err(LedgerError::Provenance(format!(
            "{label} artifact digest does not match its declared identity"
        )));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| LedgerError::Json(format!("{}: {error}", path.display())))
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, LedgerError> {
    value
        .as_object()
        .ok_or_else(|| LedgerError::Contract(format!("{label} must be an object")))
}

fn exact_keys(
    value: &Map<String, Value>,
    allowed: &[&str],
    label: &str,
) -> Result<(), LedgerError> {
    if value.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(LedgerError::Contract(format!(
            "{label} contains an unknown field"
        )));
    }
    Ok(())
}

fn required<'a>(
    value: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a Value, LedgerError> {
    value
        .get(key)
        .ok_or_else(|| LedgerError::Contract(format!("{label}.{key} is required")))
}

fn string_field<'a>(
    value: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str, LedgerError> {
    required(value, key, label)?
        .as_str()
        .ok_or_else(|| LedgerError::Contract(format!("{label}.{key} must be a string")))
}

fn number_field(value: &Map<String, Value>, key: &str, label: &str) -> Result<f64, LedgerError> {
    let number = required(value, key, label)?
        .as_f64()
        .ok_or_else(|| LedgerError::Contract(format!("{label}.{key} must be numeric")))?;
    if number.is_finite() {
        Ok(number)
    } else {
        Err(LedgerError::Contract(format!(
            "{label}.{key} must be finite"
        )))
    }
}

fn positive(value: f64, label: &str) -> Result<(), LedgerError> {
    if value > 0.0 {
        Ok(())
    } else {
        Err(LedgerError::Contract(format!("{label} must be positive")))
    }
}

fn array_of_numbers(value: &Value, label: &str, expected: usize) -> Result<Vec<f64>, LedgerError> {
    let array = value
        .as_array()
        .ok_or_else(|| LedgerError::Contract(format!("{label} must be an array")))?;
    if array.len() != expected {
        return Err(LedgerError::Contract(format!(
            "{label} must contain exactly {expected} values"
        )));
    }
    array
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let number = item.as_f64().ok_or_else(|| {
                LedgerError::Contract(format!("{label}[{index}] must be numeric"))
            })?;
            if number.is_finite() {
                Ok(number)
            } else {
                Err(LedgerError::Contract(format!(
                    "{label}[{index}] must be finite"
                )))
            }
        })
        .collect()
}

fn validate_terrain(value: &Value, spec: &ProjectSpec) -> Result<(), LedgerError> {
    let terrain = object(value, "terrain")?;
    exact_keys(
        terrain,
        &[
            "schema_version",
            "world_id",
            "dimensions_m",
            "heightfield",
            "heightfield_sha256",
            "resolution",
            "world_bounds_m",
            "spawn_surface_y_m",
        ],
        "terrain",
    )?;
    if string_field(terrain, "schema_version", "terrain")? != "luxel.terrain-manifest/v1"
        || string_field(terrain, "world_id", "terrain")? != spec.world.world_id
    {
        return Err(LedgerError::Contract(
            "terrain schema or world identity does not match the project".into(),
        ));
    }
    let dimensions = array_of_numbers(
        required(terrain, "dimensions_m", "terrain")?,
        "terrain.dimensions_m",
        2,
    )?;
    if dimensions != spec.world.dimensions_m {
        return Err(LedgerError::Contract(
            "terrain dimensions do not match the project world".into(),
        ));
    }
    let bounds = object(
        required(terrain, "world_bounds_m", "terrain")?,
        "terrain.world_bounds_m",
    )?;
    exact_keys(bounds, &["width", "length"], "terrain.world_bounds_m")?;
    if number_field(bounds, "width", "terrain.world_bounds_m")? != dimensions[0]
        || number_field(bounds, "length", "terrain.world_bounds_m")? != dimensions[1]
    {
        return Err(LedgerError::Contract(
            "terrain world bounds do not match its dimensions".into(),
        ));
    }
    let resolution = required(terrain, "resolution", "terrain")?
        .as_u64()
        .ok_or_else(|| LedgerError::Contract("terrain.resolution must be an integer".into()))?;
    if resolution < 2 {
        return Err(LedgerError::Contract(
            "terrain.resolution must be at least 2".into(),
        ));
    }
    let heightfield_digest = string_field(terrain, "heightfield_sha256", "terrain")?;
    if !heightfield_digest.starts_with("sha256:") {
        return Err(LedgerError::Provenance(
            "terrain.heightfield_sha256 must be content addressed".into(),
        ));
    }
    number_field(terrain, "spawn_surface_y_m", "terrain")?;
    Ok(())
}

fn validate_collision(value: &Value, spec: &ProjectSpec) -> Result<(), LedgerError> {
    let collision = object(value, "collision")?;
    exact_keys(
        collision,
        &[
            "schema_version",
            "world_id",
            "terrain_collision",
            "colliders",
        ],
        "collision",
    )?;
    if string_field(collision, "schema_version", "collision")? != "luxel.collision-plan/v1"
        || string_field(collision, "world_id", "collision")? != spec.world.world_id
    {
        return Err(LedgerError::Contract(
            "collision schema or world identity does not match the project".into(),
        ));
    }
    let terrain_collision = object(
        required(collision, "terrain_collision", "collision")?,
        "collision.terrain_collision",
    )?;
    exact_keys(
        terrain_collision,
        &["kind", "source"],
        "collision.terrain_collision",
    )?;
    if string_field(terrain_collision, "kind", "collision.terrain_collision")? != "heightfield"
        || string_field(terrain_collision, "source", "collision.terrain_collision")? != "terrain"
    {
        return Err(LedgerError::Contract(
            "collision must be derived from the declared terrain".into(),
        ));
    }
    let colliders = required(collision, "colliders", "collision")?
        .as_array()
        .ok_or_else(|| LedgerError::Contract("collision.colliders must be an array".into()))?;
    let mut ids = BTreeSet::new();
    for (index, item) in colliders.iter().enumerate() {
        let label = format!("collision.colliders[{index}]");
        let collider = object(item, &label)?;
        exact_keys(
            collider,
            &["id", "shape", "radius_m", "height_m", "size_m", "blocking"],
            &label,
        )?;
        let id = string_field(collider, "id", &label)?;
        if !ids.insert(id.to_owned()) {
            return Err(LedgerError::Contract(format!("duplicate collider id {id}")));
        }
        let shape = string_field(collider, "shape", &label)?;
        match shape {
            "capsule" => {
                positive(
                    number_field(collider, "radius_m", &label)?,
                    &format!("{label}.radius_m"),
                )?;
                positive(
                    number_field(collider, "height_m", &label)?,
                    &format!("{label}.height_m"),
                )?;
            }
            "box" => {
                let size = array_of_numbers(
                    required(collider, "size_m", &label)?,
                    &format!("{label}.size_m"),
                    3,
                )?;
                for (axis, value) in size.iter().enumerate() {
                    positive(*value, &format!("{label}.size_m[{axis}]"))?;
                }
            }
            _ => {
                return Err(LedgerError::Contract(format!(
                    "{label}.shape is unsupported"
                )));
            }
        }
        required(collider, "blocking", &label)?
            .as_bool()
            .ok_or_else(|| LedgerError::Contract(format!("{label}.blocking must be boolean")))?;
    }
    if colliders.is_empty() {
        return Err(LedgerError::Contract(
            "collision must declare at least one collider".into(),
        ));
    }
    Ok(())
}

fn validate_navigation(value: &Value, spec: &ProjectSpec) -> Result<(), LedgerError> {
    let navigation = object(value, "navigation")?;
    exact_keys(
        navigation,
        &[
            "schema_version",
            "world_id",
            "agent_radius_m",
            "max_climb_m",
            "start_node",
            "objective_node",
            "route",
            "connected_components",
        ],
        "navigation",
    )?;
    if string_field(navigation, "schema_version", "navigation")? != "luxel.navigation-plan/v1"
        || string_field(navigation, "world_id", "navigation")? != spec.world.world_id
    {
        return Err(LedgerError::Contract(
            "navigation schema or world identity does not match the project".into(),
        ));
    }
    positive(
        number_field(navigation, "agent_radius_m", "navigation")?,
        "navigation.agent_radius_m",
    )?;
    let max_climb = number_field(navigation, "max_climb_m", "navigation")?;
    if max_climb < 0.0 {
        return Err(LedgerError::Contract(
            "navigation.max_climb_m must not be negative".into(),
        ));
    }
    let start_name = string_field(navigation, "start_node", "navigation")?.to_owned();
    let objective_name = string_field(navigation, "objective_node", "navigation")?.to_owned();
    let route = required(navigation, "route", "navigation")?
        .as_array()
        .ok_or_else(|| LedgerError::Contract("navigation.route must be an array".into()))?;
    if route.is_empty() {
        return Err(LedgerError::Contract(
            "navigation.route must not be empty".into(),
        ));
    }
    let mut graph = BTreeMap::<String, BTreeSet<String>>::new();
    let mut positions = BTreeMap::<String, [f64; 2]>::new();
    for (index, item) in route.iter().enumerate() {
        let label = format!("navigation.route[{index}]");
        let node = object(item, &label)?;
        exact_keys(node, &["node", "position_xz_m", "neighbors"], &label)?;
        let name = string_field(node, "node", &label)?.to_owned();
        if graph.contains_key(&name) {
            return Err(LedgerError::Contract(format!(
                "duplicate navigation node {name}"
            )));
        }
        let point = array_of_numbers(
            required(node, "position_xz_m", &label)?,
            &format!("{label}.position_xz_m"),
            2,
        )?;
        if point[0].abs() > spec.world.dimensions_m[0] / 2.0
            || point[1].abs() > spec.world.dimensions_m[1] / 2.0
        {
            return Err(LedgerError::Contract(format!(
                "navigation node {name} lies outside world bounds"
            )));
        }
        let neighbors = required(node, "neighbors", &label)?
            .as_array()
            .ok_or_else(|| LedgerError::Contract(format!("{label}.neighbors must be an array")))?
            .iter()
            .map(|neighbor| {
                neighbor.as_str().map(str::to_owned).ok_or_else(|| {
                    LedgerError::Contract(format!("{label}.neighbors must contain strings"))
                })
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        graph.insert(name.clone(), neighbors);
        positions.insert(name, [point[0], point[1]]);
    }
    for (name, neighbors) in &graph {
        for neighbor in neighbors {
            let Some(reverse) = graph.get(neighbor) else {
                return Err(LedgerError::Contract(format!(
                    "navigation edge {name}->{neighbor} points to an unknown node"
                )));
            };
            if !reverse.contains(name) {
                return Err(LedgerError::Contract(format!(
                    "navigation edge {name}->{neighbor} has no reverse edge"
                )));
            }
        }
    }
    let Some(start_position) = positions.get(&start_name) else {
        return Err(LedgerError::Contract(format!(
            "navigation start_node {start_name} is not in the route"
        )));
    };
    if let Some(spawn) = spec.world.spawns.first()
        && ((spawn.position_xz_m[0] - start_position[0]).abs() > 1e-6
            || (spawn.position_xz_m[1] - start_position[1]).abs() > 1e-6)
    {
        return Err(LedgerError::Contract(
            "the required player spawn is not on navigation.start_node".into(),
        ));
    }
    if !positions.contains_key(&objective_name) {
        return Err(LedgerError::Contract(format!(
            "navigation objective_node {objective_name} is not in the route"
        )));
    }
    let mut visited = BTreeSet::from([start_name.clone()]);
    let mut queue = VecDeque::from([start_name.clone()]);
    while let Some(node) = queue.pop_front() {
        for neighbor in &graph[&node] {
            if visited.insert(neighbor.clone()) {
                queue.push_back(neighbor.clone());
            }
        }
    }
    if visited.len() != graph.len() || !visited.contains(&objective_name) {
        return Err(LedgerError::Contract(
            "navigation route is disconnected from west_spawn to east_objective".into(),
        ));
    }
    let components = required(navigation, "connected_components", "navigation")?
        .as_u64()
        .ok_or_else(|| {
            LedgerError::Contract("navigation.connected_components must be an integer".into())
        })?;
    if components != 1 {
        return Err(LedgerError::Contract(
            "navigation.connected_components must be exactly 1".into(),
        ));
    }
    Ok(())
}
