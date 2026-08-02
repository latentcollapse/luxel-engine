//! Deterministic, engine-neutral validation for Codeweald ZoneSpec documents.
//!
//! This crate deliberately validates the shared world authority, not Godot
//! scenes or renderer output. Engine adapters remain responsible for turning a
//! valid ZoneSpec into meshes, navigation resources, and runtime effects.

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

mod render_plan;
pub use render_plan::{RENDER_PLAN_SCHEMA, compile_render_plan_value, validate_render_plan_value};

pub const ZONE_SPEC_SCHEMA: &str = "codeweald.zone-spec/v1";
pub const PLACEMENT_PLAN_SCHEMA: &str = "codeweald.placement-plan/v1";
pub const TERRAIN_ARTIFACTS_SCHEMA: &str = "codeweald.terrain-artifacts/v1";
pub const TERRAIN_ANALYSIS_SCHEMA: &str = "codeweald.terrain-analysis/v1";

#[derive(Debug, Clone, PartialEq)]
pub struct WorldIdentity {
    pub zone_id: String,
    pub width_m: f64,
    pub length_m: f64,
    pub feature_count: usize,
    pub canonical_sha256: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerrainAnalysisIdentity {
    pub zone_id: String,
    pub resolution: usize,
    pub status: String,
    pub heightfield_sha256: String,
    pub protected_relief_mask_sha256: String,
    pub semantic_region_mask_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldSpecError {
    Json(String),
    Contract(String),
}

impl std::fmt::Display for WorldSpecError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(message) => write!(formatter, "invalid JSON: {message}"),
            Self::Contract(message) => write!(formatter, "invalid ZoneSpec: {message}"),
        }
    }
}

impl std::error::Error for WorldSpecError {}

/// Parse, validate, and give a ZoneSpec a stable content identity.
pub fn validate_json(input: &str) -> Result<WorldIdentity, WorldSpecError> {
    let document: Value =
        serde_json::from_str(input).map_err(|error| WorldSpecError::Json(error.to_string()))?;
    validate_value(&document)
}

/// Validate an already parsed ZoneSpec. Object-member order does not affect
/// the fingerprint; array order remains significant by design.
pub fn validate_value(document: &Value) -> Result<WorldIdentity, WorldSpecError> {
    let root = object(document, "root")?;
    let schema = string(
        required(root, "schema_version", "root")?,
        "root.schema_version",
    )?;
    if schema != ZONE_SPEC_SCHEMA {
        return Err(WorldSpecError::Contract(format!(
            "root.schema_version must be {ZONE_SPEC_SCHEMA:?}, got {schema:?}"
        )));
    }

    let zone = object(required(root, "zone", "root")?, "root.zone")?;
    let zone_id = string(required(zone, "id", "root.zone")?, "root.zone.id")?.to_owned();
    if zone_id.trim().is_empty() {
        return Err(WorldSpecError::Contract(
            "root.zone.id must not be empty".into(),
        ));
    }
    let bounds = object(
        required(zone, "world_bounds", "root.zone")?,
        "root.zone.world_bounds",
    )?;
    let width_m = positive_number(
        required(bounds, "width", "root.zone.world_bounds")?,
        "root.zone.world_bounds.width",
    )?;
    let length_m = positive_number(
        required(bounds, "length", "root.zone.world_bounds")?,
        "root.zone.world_bounds.length",
    )?;
    let units = string(
        required(bounds, "units", "root.zone.world_bounds")?,
        "root.zone.world_bounds.units",
    )?;
    if units != "meters" {
        return Err(WorldSpecError::Contract(
            "root.zone.world_bounds.units must be \"meters\"".into(),
        ));
    }
    let coordinate_system = string(
        required(zone, "coordinate_system", "root.zone")?,
        "root.zone.coordinate_system",
    )?;
    if coordinate_system != "right-handed-xz-up-y" {
        return Err(WorldSpecError::Contract(
            "root.zone.coordinate_system must be \"right-handed-xz-up-y\"".into(),
        ));
    }

    let source_images = array(
        required(zone, "source_images", "root.zone")?,
        "root.zone.source_images",
    )?;
    if source_images.is_empty() {
        return Err(WorldSpecError::Contract(
            "root.zone.source_images must not be empty".into(),
        ));
    }

    let features = array(required(root, "features", "root")?, "root.features")?;
    if features.is_empty() {
        return Err(WorldSpecError::Contract(
            "root.features must not be empty".into(),
        ));
    }
    let mut feature_ids = BTreeSet::new();
    for (index, feature) in features.iter().enumerate() {
        let feature = object(feature, &format!("root.features[{index}]"))?;
        let id = string(
            required(feature, "id", &format!("root.features[{index}]"))?,
            &format!("root.features[{index}].id"),
        )?;
        if id.trim().is_empty() {
            return Err(WorldSpecError::Contract(format!(
                "root.features[{index}].id must not be empty"
            )));
        }
        if !feature_ids.insert(id) {
            return Err(WorldSpecError::Contract(format!(
                "duplicate feature id {id:?}"
            )));
        }
        object(
            required(feature, "geometry", &format!("root.features[{index}]"))?,
            &format!("root.features[{index}].geometry"),
        )?;
        if let Some(composition_value) = feature
            .get("generation")
            .and_then(Value::as_object)
            .and_then(|generation| generation.get("composition"))
        {
            let path = format!("root.features[{index}].generation.composition");
            let composition = object(composition_value, &path)?;
            let pattern = string(
                required(composition, "pattern", &path)?,
                &format!("{path}.pattern"),
            )?;
            if !matches!(pattern, "ridge_network" | "clustered_ridges") {
                return Err(WorldSpecError::Contract(format!(
                    "{path}.pattern must be \"ridge_network\" or \"clustered_ridges\""
                )));
            }
            integer_at_least(
                required(composition, "spine_count", &path)?,
                &format!("{path}.spine_count"),
                1,
            )
            .and_then(|count| {
                if count <= 8 {
                    Ok(count)
                } else {
                    Err(WorldSpecError::Contract(format!(
                        "{path}.spine_count must not exceed 8"
                    )))
                }
            })?;
            for (key, upper) in [
                ("elevation_bias", 1.0),
                ("along_jitter", 0.5),
                ("cross_jitter", 0.5),
            ] {
                let value = nonnegative_number(
                    required(composition, key, &path)?,
                    &format!("{path}.{key}"),
                )?;
                if value > upper {
                    return Err(WorldSpecError::Contract(format!(
                        "{path}.{key} must not exceed {upper}"
                    )));
                }
            }
        }
    }

    Ok(WorldIdentity {
        zone_id,
        width_m,
        length_m,
        feature_count: features.len(),
        canonical_sha256: canonical_sha256(document),
    })
}

/// Produce canonical JSON with lexical object keys. This is intentionally
/// exposed so later compilers can derive deterministic content-addressed
/// artifact keys from exactly the same bytes.
pub fn canonical_json(document: &Value) -> String {
    serde_json::to_string(&canonicalize(document)).expect("canonical JSON values are serializable")
}

pub fn canonical_sha256(document: &Value) -> String {
    let mut digest = Sha256::new();
    digest.update(canonical_json(document).as_bytes());
    format!("{:x}", digest.finalize())
}

/// Validate a placement result produced by a world solver (Julia, Python, or
/// another deterministic backend) against immutable world and asset authority.
/// The plan deliberately carries concrete transforms rather than asking Godot
/// to reroll random scatter at scene-build time.
pub fn validate_placement_plan_value(
    zone_spec: &Value,
    asset_plan: &Value,
    placement_plan: &Value,
) -> Result<usize, WorldSpecError> {
    let world = validate_value(zone_spec)?;
    let zone_root = object(zone_spec, "zone spec")?;
    let known_features: BTreeSet<&str> = array(
        required(zone_root, "features", "zone spec")?,
        "zone spec.features",
    )?
    .iter()
    .filter_map(|feature| feature.get("id").and_then(Value::as_str))
    .collect();
    let known_assets = asset_hashes(asset_plan)?;

    let root = object(placement_plan, "placement plan")?;
    let schema = string(
        required(root, "schema_version", "placement plan")?,
        "placement plan.schema_version",
    )?;
    if schema != PLACEMENT_PLAN_SCHEMA {
        return Err(WorldSpecError::Contract(format!(
            "placement plan.schema_version must be {PLACEMENT_PLAN_SCHEMA:?}"
        )));
    }
    fingerprint_matches(root, "zone_spec_sha256", &world.canonical_sha256)?;
    fingerprint_matches(root, "asset_plan_sha256", &canonical_sha256(asset_plan))?;

    let placements = array(
        required(root, "placements", "placement plan")?,
        "placement plan.placements",
    )?;
    if placements.is_empty() {
        let requests_dressing = array(
            required(zone_root, "features", "zone spec")?,
            "zone spec.features",
        )?
        .iter()
        .filter(|feature| feature.get("category").and_then(Value::as_str) == Some("landform"))
        .filter_map(|feature| feature.pointer("/generation/composition"))
        .any(|composition| composition.get("dressing").and_then(Value::as_str) != Some("none"));
        if requests_dressing {
            return Err(WorldSpecError::Contract(
                "placement plan is empty but the ZoneSpec requests landform dressing".into(),
            ));
        }
        return Ok(0);
    }
    let mut placement_ids = BTreeSet::new();
    for (index, placement) in placements.iter().enumerate() {
        let path = format!("placement plan.placements[{index}]");
        let placement = object(placement, &path)?;
        let id = string(required(placement, "id", &path)?, &format!("{path}.id"))?;
        if !placement_ids.insert(id) {
            return Err(WorldSpecError::Contract(format!(
                "duplicate placement id {id:?}"
            )));
        }
        let feature_id = string(
            required(placement, "feature_id", &path)?,
            &format!("{path}.feature_id"),
        )?;
        if !known_features.contains(feature_id) {
            return Err(WorldSpecError::Contract(format!(
                "{path}.feature_id {feature_id:?} is not in the ZoneSpec"
            )));
        }
        let asset_sha256 = string(
            required(placement, "asset_sha256", &path)?,
            &format!("{path}.asset_sha256"),
        )?;
        if !known_assets.contains(asset_sha256) {
            return Err(WorldSpecError::Contract(format!(
                "{path}.asset_sha256 is not in the asset plan"
            )));
        }
        let position = array(
            required(placement, "position_m", &path)?,
            &format!("{path}.position_m"),
        )?;
        if position.len() != 3 {
            return Err(WorldSpecError::Contract(format!(
                "{path}.position_m must be [x, y, z]"
            )));
        }
        let x = finite_number(&position[0], &format!("{path}.position_m[0]"))?;
        let y = finite_number(&position[1], &format!("{path}.position_m[1]"))?;
        let z = finite_number(&position[2], &format!("{path}.position_m[2]"))?;
        if x.abs() > world.width_m * 0.5 || z.abs() > world.length_m * 0.5 {
            return Err(WorldSpecError::Contract(format!(
                "{path}.position_m lies outside world bounds"
            )));
        }
        let ground = finite_number(
            required(placement, "ground_height_m", &path)?,
            &format!("{path}.ground_height_m"),
        )?;
        if (y - ground).abs() > 0.01 {
            return Err(WorldSpecError::Contract(format!(
                "{path}.position_m[1] must match ground_height_m"
            )));
        }
        finite_number(
            required(placement, "yaw_degrees", &path)?,
            &format!("{path}.yaw_degrees"),
        )?;
        let scale = finite_number(
            required(placement, "scale", &path)?,
            &format!("{path}.scale"),
        )?;
        if scale <= 0.0 {
            return Err(WorldSpecError::Contract(format!(
                "{path}.scale must be greater than zero"
            )));
        }
    }
    Ok(placements.len())
}

/// Validate that a numerical terrain report is bound to this exact world and
/// to the exact canonical raster bytes that Julia analyzed. A failed quality
/// report is still a valid report; callers decide whether failed policy should
/// stop a build after this function establishes provenance and consistency.
pub fn validate_terrain_analysis_value(
    zone_spec: &Value,
    terrain_manifest: &Value,
    heightfield_bytes: &[u8],
    protected_relief_mask_bytes: &[u8],
    semantic_region_mask_bytes: &[u8],
    analysis: &Value,
) -> Result<TerrainAnalysisIdentity, WorldSpecError> {
    let world = validate_value(zone_spec)?;

    let manifest = object(terrain_manifest, "terrain manifest")?;
    schema_matches(manifest, "terrain manifest", TERRAIN_ARTIFACTS_SCHEMA)?;
    let manifest_zone_id = string(
        required(manifest, "zone_id", "terrain manifest")?,
        "terrain manifest.zone_id",
    )?;
    if manifest_zone_id != world.zone_id {
        return Err(WorldSpecError::Contract(
            "terrain manifest.zone_id does not match the ZoneSpec".into(),
        ));
    }
    exact_string(
        manifest,
        "zone_spec_sha256",
        "terrain manifest",
        &world.canonical_sha256,
    )?;
    let resolution = integer_at_least(
        required(manifest, "resolution", "terrain manifest")?,
        "terrain manifest.resolution",
        3,
    )?;
    validate_world_bounds(
        required(manifest, "world_bounds_m", "terrain manifest")?,
        "terrain manifest.world_bounds_m",
        world.width_m,
        world.length_m,
    )?;

    let sample_count = resolution
        .checked_mul(resolution)
        .ok_or_else(|| WorldSpecError::Contract("terrain resolution overflows".into()))?;
    let expected_heightfield_bytes = sample_count
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| WorldSpecError::Contract("heightfield byte size overflows".into()))?;
    if heightfield_bytes.len() != expected_heightfield_bytes {
        return Err(WorldSpecError::Contract(format!(
            "heightfield has {} bytes; expected {expected_heightfield_bytes}",
            heightfield_bytes.len()
        )));
    }
    if protected_relief_mask_bytes.len() != sample_count {
        return Err(WorldSpecError::Contract(format!(
            "protected relief mask has {} bytes; expected {sample_count}",
            protected_relief_mask_bytes.len()
        )));
    }
    if semantic_region_mask_bytes.len() != sample_count {
        return Err(WorldSpecError::Contract(format!(
            "semantic region mask has {} bytes; expected {sample_count}",
            semantic_region_mask_bytes.len()
        )));
    }

    let report = object(analysis, "terrain analysis")?;
    schema_matches(report, "terrain analysis", TERRAIN_ANALYSIS_SCHEMA)?;
    let report_zone_id = string(
        required(report, "zone_id", "terrain analysis")?,
        "terrain analysis.zone_id",
    )?;
    if report_zone_id != world.zone_id {
        return Err(WorldSpecError::Contract(
            "terrain analysis.zone_id does not match the ZoneSpec".into(),
        ));
    }
    exact_string(
        report,
        "zone_spec_sha256",
        "terrain analysis",
        &world.canonical_sha256,
    )?;
    let report_resolution = integer_at_least(
        required(report, "resolution", "terrain analysis")?,
        "terrain analysis.resolution",
        3,
    )?;
    if report_resolution != resolution {
        return Err(WorldSpecError::Contract(
            "terrain analysis.resolution does not match the terrain manifest".into(),
        ));
    }
    validate_world_bounds(
        required(report, "world_bounds_m", "terrain analysis")?,
        "terrain analysis.world_bounds_m",
        world.width_m,
        world.length_m,
    )?;

    let heightfield_sha256 = bytes_sha256(heightfield_bytes);
    exact_string(
        report,
        "heightfield_sha256",
        "terrain analysis",
        &heightfield_sha256,
    )?;
    let protected_relief_mask_sha256 = bytes_sha256(protected_relief_mask_bytes);
    exact_string(
        report,
        "protected_relief_mask_sha256",
        "terrain analysis",
        &protected_relief_mask_sha256,
    )?;
    let semantic_region_mask_sha256 = bytes_sha256(semantic_region_mask_bytes);
    exact_string(
        report,
        "semantic_region_mask_sha256",
        "terrain analysis",
        &semantic_region_mask_sha256,
    )?;

    let protected_fraction = finite_number(
        required(report, "protected_relief_fraction", "terrain analysis")?,
        "terrain analysis.protected_relief_fraction",
    )?;
    if !(0.0..=1.0).contains(&protected_fraction) {
        return Err(WorldSpecError::Contract(
            "terrain analysis.protected_relief_fraction must be between zero and one".into(),
        ));
    }

    let policy = object(
        required(report, "policy", "terrain analysis")?,
        "terrain analysis.policy",
    )?;
    let maximum_grade = positive_number(
        required(
            policy,
            "maximum_accessible_grade",
            "terrain analysis.policy",
        )?,
        "terrain analysis.policy.maximum_accessible_grade",
    )?;
    let steep_grade = positive_number(
        required(policy, "steep_grade", "terrain analysis.policy")?,
        "terrain analysis.policy.steep_grade",
    )?;
    let maximum_steep_fraction = finite_number(
        required(
            policy,
            "maximum_accessible_steep_fraction",
            "terrain analysis.policy",
        )?,
        "terrain analysis.policy.maximum_accessible_steep_fraction",
    )?;
    if !(0.0..=1.0).contains(&maximum_steep_fraction) {
        return Err(WorldSpecError::Contract(
            "terrain analysis.policy.maximum_accessible_steep_fraction must be between zero and one"
                .into(),
        ));
    }

    let accessible = validate_grade_summary(
        required(report, "accessible", "terrain analysis")?,
        "terrain analysis.accessible",
        steep_grade,
    )?;
    validate_grade_summary(
        required(report, "intentional_relief", "terrain analysis")?,
        "terrain analysis.intentional_relief",
        steep_grade,
    )?;
    let regions = object(
        required(report, "regions", "terrain analysis")?,
        "terrain analysis.regions",
    )?;
    for region in [
        "lane",
        "landmark_pad",
        "hydrology",
        "protected_relief",
        "traversable_landform",
        "background",
    ] {
        validate_grade_summary(
            required(regions, region, "terrain analysis.regions")?,
            &format!("terrain analysis.regions.{region}"),
            steep_grade,
        )?;
    }
    let failures = array(
        required(report, "failures", "terrain analysis")?,
        "terrain analysis.failures",
    )?;
    for (index, failure) in failures.iter().enumerate() {
        let failure = string(failure, &format!("terrain analysis.failures[{index}]"))?;
        if failure.trim().is_empty() {
            return Err(WorldSpecError::Contract(format!(
                "terrain analysis.failures[{index}] must not be empty"
            )));
        }
    }

    let status = string(
        required(report, "status", "terrain analysis")?,
        "terrain analysis.status",
    )?;
    let violates_policy = accessible.maximum_grade > maximum_grade
        || accessible.steep_edge_fraction > maximum_steep_fraction;
    match status {
        "passed" if !failures.is_empty() => {
            return Err(WorldSpecError::Contract(
                "passed terrain analysis must not contain failures".into(),
            ));
        }
        "passed" if violates_policy => {
            return Err(WorldSpecError::Contract(
                "passed terrain analysis violates its declared policy".into(),
            ));
        }
        "failed" if failures.is_empty() => {
            return Err(WorldSpecError::Contract(
                "failed terrain analysis must explain at least one failure".into(),
            ));
        }
        "failed" if !violates_policy => {
            return Err(WorldSpecError::Contract(
                "failed terrain analysis does not violate its declared policy".into(),
            ));
        }
        "passed" | "failed" => {}
        _ => {
            return Err(WorldSpecError::Contract(
                "terrain analysis.status must be \"passed\" or \"failed\"".into(),
            ));
        }
    }

    Ok(TerrainAnalysisIdentity {
        zone_id: world.zone_id,
        resolution,
        status: status.to_owned(),
        heightfield_sha256,
        protected_relief_mask_sha256,
        semantic_region_mask_sha256,
    })
}

#[derive(Debug, Clone, Copy)]
struct GradeSummary {
    maximum_grade: f64,
    steep_edge_fraction: f64,
}

fn validate_grade_summary(
    value: &Value,
    path: &str,
    expected_steep_grade: f64,
) -> Result<GradeSummary, WorldSpecError> {
    let summary = object(value, path)?;
    integer_at_least(
        required(summary, "edge_count", path)?,
        &format!("{path}.edge_count"),
        0,
    )?;
    let mut prior = 0.0;
    for key in ["p50_grade", "p95_grade", "p99_grade", "p999_grade"] {
        let grade = nonnegative_number(required(summary, key, path)?, &format!("{path}.{key}"))?;
        if grade < prior {
            return Err(WorldSpecError::Contract(format!(
                "{path} quantiles must be nondecreasing"
            )));
        }
        prior = grade;
    }
    let maximum_grade = nonnegative_number(
        required(summary, "maximum_grade", path)?,
        &format!("{path}.maximum_grade"),
    )?;
    if maximum_grade < prior {
        return Err(WorldSpecError::Contract(format!(
            "{path}.maximum_grade must be at least p999_grade"
        )));
    }
    let steep_grade = positive_number(
        required(summary, "steep_grade_threshold", path)?,
        &format!("{path}.steep_grade_threshold"),
    )?;
    if (steep_grade - expected_steep_grade).abs() > 1e-9 {
        return Err(WorldSpecError::Contract(format!(
            "{path}.steep_grade_threshold does not match terrain analysis.policy.steep_grade"
        )));
    }
    let steep_edge_fraction = finite_number(
        required(summary, "steep_edge_fraction", path)?,
        &format!("{path}.steep_edge_fraction"),
    )?;
    if !(0.0..=1.0).contains(&steep_edge_fraction) {
        return Err(WorldSpecError::Contract(format!(
            "{path}.steep_edge_fraction must be between zero and one"
        )));
    }
    Ok(GradeSummary {
        maximum_grade,
        steep_edge_fraction,
    })
}

fn schema_matches(
    root: &Map<String, Value>,
    path: &str,
    expected: &str,
) -> Result<(), WorldSpecError> {
    let actual = string(
        required(root, "schema_version", path)?,
        &format!("{path}.schema_version"),
    )?;
    if actual != expected {
        return Err(WorldSpecError::Contract(format!(
            "{path}.schema_version must be {expected:?}, got {actual:?}"
        )));
    }
    Ok(())
}

fn validate_world_bounds(
    value: &Value,
    path: &str,
    expected_width: f64,
    expected_length: f64,
) -> Result<(), WorldSpecError> {
    let bounds = object(value, path)?;
    let width = positive_number(required(bounds, "width", path)?, &format!("{path}.width"))?;
    let length = positive_number(required(bounds, "length", path)?, &format!("{path}.length"))?;
    if (width - expected_width).abs() > 1e-9 || (length - expected_length).abs() > 1e-9 {
        return Err(WorldSpecError::Contract(format!(
            "{path} does not match the ZoneSpec"
        )));
    }
    Ok(())
}

fn exact_string(
    root: &Map<String, Value>,
    key: &str,
    path: &str,
    expected: &str,
) -> Result<(), WorldSpecError> {
    let actual = string(required(root, key, path)?, &format!("{path}.{key}"))?;
    if actual != expected {
        return Err(WorldSpecError::Contract(format!(
            "{path}.{key} does not match the supplied artifact"
        )));
    }
    Ok(())
}

fn bytes_sha256(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn fingerprint_matches(
    root: &Map<String, Value>,
    key: &str,
    expected: &str,
) -> Result<(), WorldSpecError> {
    let actual = string(
        required(root, key, "placement plan")?,
        &format!("placement plan.{key}"),
    )?;
    if actual != expected {
        return Err(WorldSpecError::Contract(format!(
            "placement plan.{key} does not match its authority"
        )));
    }
    Ok(())
}

fn asset_hashes(asset_plan: &Value) -> Result<BTreeSet<&str>, WorldSpecError> {
    let assignments = array(
        required(
            object(asset_plan, "asset plan")?,
            "assignments",
            "asset plan",
        )?,
        "asset plan.assignments",
    )?;
    let mut hashes = BTreeSet::new();
    for assignment in assignments {
        collect_asset_hashes(assignment, &mut hashes);
    }
    if hashes.is_empty() {
        return Err(WorldSpecError::Contract(
            "asset plan contains no hashed assets".into(),
        ));
    }
    Ok(hashes)
}

fn collect_asset_hashes<'a>(value: &'a Value, hashes: &mut BTreeSet<&'a str>) {
    match value {
        Value::Object(object) => {
            if let Some(hash) = object.get("sha256").and_then(Value::as_str) {
                hashes.insert(hash);
            }
            for value in object.values() {
                collect_asset_hashes(value, hashes);
            }
        }
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_asset_hashes(value, hashes)),
        _ => {}
    }
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut ordered = std::collections::BTreeMap::new();
            for (key, value) in object {
                ordered.insert(key.clone(), canonicalize(value));
            }
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        scalar => scalar.clone(),
    }
}

fn required<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a Value, WorldSpecError> {
    object
        .get(key)
        .ok_or_else(|| WorldSpecError::Contract(format!("{path}.{key} is required")))
}

fn object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, WorldSpecError> {
    value
        .as_object()
        .ok_or_else(|| WorldSpecError::Contract(format!("{path} must be an object")))
}

fn array<'a>(value: &'a Value, path: &str) -> Result<&'a Vec<Value>, WorldSpecError> {
    value
        .as_array()
        .ok_or_else(|| WorldSpecError::Contract(format!("{path} must be an array")))
}

fn string<'a>(value: &'a Value, path: &str) -> Result<&'a str, WorldSpecError> {
    value
        .as_str()
        .ok_or_else(|| WorldSpecError::Contract(format!("{path} must be a string")))
}

fn positive_number(value: &Value, path: &str) -> Result<f64, WorldSpecError> {
    let number = value
        .as_f64()
        .ok_or_else(|| WorldSpecError::Contract(format!("{path} must be a positive number")))?;
    if !number.is_finite() || number <= 0.0 {
        return Err(WorldSpecError::Contract(format!(
            "{path} must be greater than zero"
        )));
    }
    Ok(number)
}

fn nonnegative_number(value: &Value, path: &str) -> Result<f64, WorldSpecError> {
    let number = finite_number(value, path)?;
    if number < 0.0 {
        return Err(WorldSpecError::Contract(format!(
            "{path} must be zero or greater"
        )));
    }
    Ok(number)
}

fn integer_at_least(value: &Value, path: &str, minimum: usize) -> Result<usize, WorldSpecError> {
    let number = value
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| WorldSpecError::Contract(format!("{path} must be an integer")))?;
    if number < minimum {
        return Err(WorldSpecError::Contract(format!(
            "{path} must be at least {minimum}"
        )));
    }
    Ok(number)
}

fn finite_number(value: &Value, path: &str) -> Result<f64, WorldSpecError> {
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .ok_or_else(|| WorldSpecError::Contract(format!("{path} must be a finite number")))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        WorldSpecError, bytes_sha256, canonical_sha256, validate_placement_plan_value,
        validate_terrain_analysis_value, validate_value,
    };

    fn valid_spec() -> serde_json::Value {
        json!({
            "schema_version": "codeweald.zone-spec/v1",
            "zone": {
                "id": "alpine_arena",
                "world_bounds": {"width": 256, "length": 256, "units": "meters"},
                "coordinate_system": "right-handed-xz-up-y",
                "source_images": [{"id": "overview"}]
            },
            "features": [{"id": "keep_a", "geometry": {"type": "point", "points": [[0, 0]]}}]
        })
    }

    #[test]
    fn valid_spec_gets_stable_identity() {
        let first = validate_value(&valid_spec()).expect("valid spec");
        let second = validate_value(&valid_spec()).expect("valid spec");
        assert_eq!(first.zone_id, "alpine_arena");
        assert_eq!(first.width_m, 256.0);
        assert_eq!(first.feature_count, 1);
        assert_eq!(first.canonical_sha256, second.canonical_sha256);
    }

    #[test]
    fn object_order_does_not_change_identity() {
        let a = json!({"b": 2, "a": {"y": 1, "x": 0}});
        let b = json!({"a": {"x": 0, "y": 1}, "b": 2});
        assert_eq!(canonical_sha256(&a), canonical_sha256(&b));
    }

    #[test]
    fn duplicate_feature_ids_are_rejected() {
        let mut spec = valid_spec();
        spec["features"]
            .as_array_mut()
            .expect("array")
            .push(json!({"id": "keep_a", "geometry": {}}));
        let error = validate_value(&spec).expect_err("duplicate must fail");
        assert!(
            matches!(error, WorldSpecError::Contract(message) if message.contains("duplicate feature id"))
        );
    }

    #[test]
    fn placement_plan_is_bound_to_world_and_assets() {
        let zone = valid_spec();
        let assets = json!({"assignments": [{"assets": [{"sha256": "a".repeat(64)}]}]});
        let plan = json!({
            "schema_version": "codeweald.placement-plan/v1",
            "zone_spec_sha256": canonical_sha256(&zone),
            "asset_plan_sha256": canonical_sha256(&assets),
            "placements": [{
                "id": "western_alps-0001", "feature_id": "keep_a",
                "asset_sha256": "a".repeat(64), "position_m": [0.0, 12.0, 0.0],
                "ground_height_m": 12.0, "yaw_degrees": 45.0, "scale": 1.0
            }]
        });
        assert_eq!(validate_placement_plan_value(&zone, &assets, &plan), Ok(1));
    }

    #[test]
    fn placement_plan_rejects_unknown_asset() {
        let zone = valid_spec();
        let assets = json!({"assignments": [{"assets": [{"sha256": "a".repeat(64)}]}]});
        let plan = json!({
            "schema_version": "codeweald.placement-plan/v1",
            "zone_spec_sha256": canonical_sha256(&zone),
            "asset_plan_sha256": canonical_sha256(&assets),
            "placements": [{
                "id": "western_alps-0001", "feature_id": "keep_a",
                "asset_sha256": "b".repeat(64), "position_m": [0.0, 12.0, 0.0],
                "ground_height_m": 12.0, "yaw_degrees": 45.0, "scale": 1.0
            }]
        });
        let error = validate_placement_plan_value(&zone, &assets, &plan)
            .expect_err("unknown asset must fail");
        assert!(
            matches!(error, WorldSpecError::Contract(message) if message.contains("asset_sha256"))
        );
    }

    #[test]
    fn empty_placement_plan_is_valid_for_terrain_native_landforms() {
        let mut zone = valid_spec();
        zone["features"].as_array_mut().expect("array").push(json!({
            "id": "terrain_wall",
            "category": "landform",
            "semantic": "alpine_massif",
            "geometry": {"type": "polygon", "points": [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]},
            "generation": {
                "profile": "alpine_jagged_massif",
                "composition": {
                    "pattern": "ridge_network",
                    "spine_count": 2,
                    "elevation_bias": 0.7,
                    "along_jitter": 0.1,
                    "cross_jitter": 0.1,
                    "silhouette": "continuous_boundary_wall",
                    "massing": "terrain_primary",
                    "surface": "fractured_granite",
                    "dressing": "none"
                }
            }
        }));
        let assets = json!({
            "assignments": [{
                "feature_id": "keep_a",
                "assets": [{"sha256": "a".repeat(64)}]
            }]
        });
        let plan = json!({
            "schema_version": "codeweald.placement-plan/v1",
            "zone_spec_sha256": canonical_sha256(&zone),
            "asset_plan_sha256": canonical_sha256(&assets),
            "placements": []
        });
        assert_eq!(validate_placement_plan_value(&zone, &assets, &plan), Ok(0));
    }

    #[test]
    fn empty_placement_plan_rejects_requested_dressing() {
        let mut zone = valid_spec();
        zone["features"].as_array_mut().expect("array").push(json!({
            "id": "dressed_wall",
            "category": "landform",
            "semantic": "alpine_massif",
            "geometry": {"type": "polygon", "points": [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]},
            "generation": {
                "profile": "alpine_jagged_massif",
                "composition": {
                    "pattern": "ridge_network",
                    "spine_count": 2,
                    "elevation_bias": 0.7,
                    "along_jitter": 0.1,
                    "cross_jitter": 0.1,
                    "silhouette": "continuous_boundary_wall",
                    "massing": "terrain_and_sparse_props",
                    "surface": "fractured_granite",
                    "dressing": "sparse"
                }
            }
        }));
        let assets = json!({
            "assignments": [{
                "feature_id": "keep_a",
                "assets": [{"sha256": "a".repeat(64)}]
            }]
        });
        let plan = json!({
            "schema_version": "codeweald.placement-plan/v1",
            "zone_spec_sha256": canonical_sha256(&zone),
            "asset_plan_sha256": canonical_sha256(&assets),
            "placements": []
        });
        let error = validate_placement_plan_value(&zone, &assets, &plan)
            .expect_err("requested dressing must not disappear");
        assert!(
            matches!(error, WorldSpecError::Contract(message) if message.contains("requests landform dressing"))
        );
    }

    fn grade_summary(maximum_grade: f64, steep_fraction: f64) -> serde_json::Value {
        json!({
            "edge_count": 12,
            "p50_grade": 0.0,
            "p95_grade": 0.0,
            "p99_grade": 0.0,
            "p999_grade": 0.0,
            "maximum_grade": maximum_grade,
            "steep_grade_threshold": 2.0,
            "steep_edge_fraction": steep_fraction
        })
    }

    fn valid_terrain_contract(
        status: &str,
    ) -> (
        serde_json::Value,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        serde_json::Value,
    ) {
        let heightfield = vec![0_u8; 3 * 3 * 4];
        let protected = vec![0_u8; 3 * 3];
        let regions = vec![0_u8; 3 * 3];
        let manifest = json!({
            "schema_version": "codeweald.terrain-artifacts/v1",
            "zone_id": "alpine_arena",
            "zone_spec_sha256": canonical_sha256(&valid_spec()),
            "resolution": 3,
            "world_bounds_m": {"width": 256.0, "length": 256.0}
        });
        let failed = status == "failed";
        let report = json!({
            "schema_version": "codeweald.terrain-analysis/v1",
            "status": status,
            "zone_id": "alpine_arena",
            "zone_spec_sha256": canonical_sha256(&valid_spec()),
            "resolution": 3,
            "world_bounds_m": {"width": 256.0, "length": 256.0},
            "heightfield_sha256": bytes_sha256(&heightfield),
            "protected_relief_mask_sha256": bytes_sha256(&protected),
            "semantic_region_mask_sha256": bytes_sha256(&regions),
            "protected_relief_fraction": 0.0,
            "policy": {
                "maximum_accessible_grade": 12.0,
                "steep_grade": 2.0,
                "maximum_accessible_steep_fraction": 0.01
            },
            "accessible": {
                "edge_count": 12,
                "p50_grade": 0.1,
                "p95_grade": 0.2,
                "p99_grade": 0.3,
                "p999_grade": 0.4,
                "maximum_grade": if failed { 13.0 } else { 0.5 },
                "steep_grade_threshold": 2.0,
                "steep_edge_fraction": if failed { 0.02 } else { 0.0 }
            },
            "intentional_relief": {
                "edge_count": 1,
                "p50_grade": 1.0,
                "p95_grade": 2.0,
                "p99_grade": 3.0,
                "p999_grade": 4.0,
                "maximum_grade": 5.0,
                "steep_grade_threshold": 2.0,
                "steep_edge_fraction": 0.5
            },
            "regions": {
                "lane": grade_summary(0.0, 0.0),
                "landmark_pad": grade_summary(0.0, 0.0),
                "hydrology": grade_summary(0.0, 0.0),
                "protected_relief": grade_summary(5.0, 0.5),
                "traversable_landform": grade_summary(0.0, 0.0),
                "background": grade_summary(0.0, 0.0)
            },
            "failures": if failed {
                json!(["accessible terrain exceeds policy"])
            } else {
                json!([])
            }
        });
        (manifest, heightfield, protected, regions, report)
    }

    #[test]
    fn terrain_analysis_is_bound_to_exact_raster_bytes() {
        let (manifest, heightfield, protected, regions, report) = valid_terrain_contract("passed");
        let identity = validate_terrain_analysis_value(
            &valid_spec(),
            &manifest,
            &heightfield,
            &protected,
            &regions,
            &report,
        )
        .expect("valid terrain analysis");
        assert_eq!(identity.status, "passed");
        assert_eq!(identity.resolution, 3);
    }

    #[test]
    fn terrain_analysis_rejects_stale_raster_bytes() {
        let (manifest, mut heightfield, protected, regions, report) =
            valid_terrain_contract("passed");
        heightfield[0] = 1;
        let error = validate_terrain_analysis_value(
            &valid_spec(),
            &manifest,
            &heightfield,
            &protected,
            &regions,
            &report,
        )
        .expect_err("stale report must fail");
        assert!(
            matches!(error, WorldSpecError::Contract(message) if message.contains("heightfield_sha256"))
        );
    }

    #[test]
    fn terrain_analysis_accepts_a_consistent_failed_quality_report() {
        let (manifest, heightfield, protected, regions, report) = valid_terrain_contract("failed");
        let identity = validate_terrain_analysis_value(
            &valid_spec(),
            &manifest,
            &heightfield,
            &protected,
            &regions,
            &report,
        )
        .expect("failed quality can still have valid provenance");
        assert_eq!(identity.status, "failed");
    }
}
