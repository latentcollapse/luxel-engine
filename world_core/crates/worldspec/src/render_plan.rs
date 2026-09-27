use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{WorldSpecError, canonical_sha256, validate_placement_plan_value, validate_value};

pub const RENDER_PLAN_SCHEMA: &str = "codeweald.render-plan/v1";

#[derive(Clone)]
struct AssetRef {
    sha256: String,
    lod_assets: Map<String, Value>,
    vertical_size_m: f64,
}

#[derive(Clone, Copy)]
struct Terrain<'a> {
    heights: &'a [f32],
    /// S6's canopy suitability, one byte per cell, row-major, in the same
    /// resolution and orientation as `heights` -- so it is sampled by the
    /// coordinate maths that already exists rather than introducing a second
    /// convention for the two to disagree about.
    suitability: &'a [u8],
    resolution: usize,
    width: f64,
    length: f64,
}

impl Terrain<'_> {
    /// Suitability at a world position, 0..=1.
    fn suitability_at(self, x: f64, z: f64) -> f64 {
        let u = (x / self.width + 0.5).clamp(0.0, 1.0);
        let v = (0.5 - z / self.length).clamp(0.0, 1.0);
        let column = (u * (self.resolution - 1) as f64).round() as usize;
        let row = (v * (self.resolution - 1) as f64).round() as usize;
        self.suitability[row * self.resolution + column] as f64 / 255.0
    }

    /// The world position of a cell centre; the inverse of `sample`'s indexing.
    fn cell_position(self, row: usize, column: usize) -> [f64; 2] {
        let last = (self.resolution - 1) as f64;
        [
            (column as f64 / last - 0.5) * self.width,
            (0.5 - row as f64 / last) * self.length,
        ]
    }

    fn sample(self, x: f64, z: f64) -> f64 {
        let u = (x / self.width + 0.5).clamp(0.0, 1.0);
        let v = (0.5 - z / self.length).clamp(0.0, 1.0);
        let column = (u * (self.resolution - 1) as f64).round() as usize;
        let row = (v * (self.resolution - 1) as f64).round() as usize;
        self.heights[row * self.resolution + column] as f64
    }

    fn slope_degrees(self, x: f64, z: f64) -> f64 {
        let step_x = self.width / (self.resolution - 1) as f64;
        let step_z = self.length / (self.resolution - 1) as f64;
        let gradient_x = (self.sample(x + step_x, z) - self.sample(x - step_x, z)) / (step_x * 2.0);
        let gradient_z = (self.sample(x, z + step_z) - self.sample(x, z - step_z)) / (step_z * 2.0);
        gradient_x.hypot(gradient_z).atan().to_degrees()
    }
}

struct StableRng(u64);

impl StableRng {
    fn from_parts(seed: u64, parts: &[&str]) -> Self {
        let mut digest = Sha256::new();
        digest.update(seed.to_le_bytes());
        for part in parts {
            digest.update((part.len() as u64).to_le_bytes());
            digest.update(part.as_bytes());
        }
        let bytes: [u8; 8] = digest.finalize()[..8].try_into().expect("sha256 prefix");
        Self(u64::from_le_bytes(bytes))
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / ((1_u64 << 53) as f64))
    }

    fn range(&mut self, minimum: f64, maximum: f64) -> f64 {
        minimum + self.unit() * (maximum - minimum)
    }

    fn index(&mut self, length: usize) -> usize {
        (self.next_u64() % length as u64) as usize
    }
}

#[derive(Clone)]
struct Exclusion {
    points: Vec<[f64; 2]>,
    radius: f64,
    filled: bool,
}

pub fn compile_render_plan_value(
    zone: &Value,
    asset_plan: &Value,
    asset_preflight: &Value,
    terrain_manifest: &Value,
    heightfield_bytes: &[u8],
    suitability_bytes: &[u8],
    landform_plan: &Value,
) -> Result<Value, WorldSpecError> {
    let world = validate_value(zone)?;
    validate_placement_plan_value(zone, asset_plan, landform_plan)?;
    let resolution = usize_at(terrain_manifest, "/resolution")?;
    let expected_bytes = resolution
        .checked_mul(resolution)
        .and_then(|samples| samples.checked_mul(4))
        .ok_or_else(|| contract("terrain resolution overflows"))?;
    if heightfield_bytes.len() != expected_bytes {
        return Err(contract(format!(
            "heightfield has {} bytes; expected {expected_bytes}",
            heightfield_bytes.len()
        )));
    }
    let heights = heightfield_bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
        .collect::<Vec<_>>();
    if heights.iter().any(|height| !height.is_finite()) {
        return Err(contract("heightfield contains non-finite samples"));
    }
    if suitability_bytes.len() != resolution * resolution {
        return Err(contract(format!(
            "canopy suitability has {} bytes; expected {} for a {resolution}^2 field",
            suitability_bytes.len(),
            resolution * resolution
        )));
    }
    let terrain = Terrain {
        heights: &heights,
        suitability: suitability_bytes,
        resolution,
        width: number_at(terrain_manifest, "/world_bounds_m/width")?,
        length: number_at(terrain_manifest, "/world_bounds_m/length")?,
    };
    if (terrain.width - world.width_m).abs() > 1e-6
        || (terrain.length - world.length_m).abs() > 1e-6
    {
        return Err(contract("terrain bounds do not match ZoneSpec"));
    }

    let seed = zone
        .get("generation_seed")
        .and_then(Value::as_u64)
        .ok_or_else(|| contract("ZoneSpec generation_seed is missing"))?;
    let features = indexed_objects(zone, "features", "ZoneSpec")?;
    let assignments = indexed_objects(asset_plan, "assignments", "asset plan")?;
    let asset_bounds = asset_vertical_sizes(asset_plan, asset_preflight)?;
    let exclusions = build_exclusions(features.values().copied())?;
    let mut instances = Vec::new();
    let mut ids = BTreeSet::new();
    let mut shortfalls: Vec<Value> = Vec::new();

    for placement in array_at(landform_plan, "/placements")? {
        let mut record = placement
            .as_object()
            .ok_or_else(|| contract("landform placement is not an object"))?
            .clone();
        apply_grounding(&mut record, "landform_dressing", &asset_bounds)?;
        record.insert("source".into(), json!("julia_landform"));
        record.insert("role".into(), json!("landform_dressing"));
        push_unique(&mut instances, &mut ids, record)?;
    }

    for (feature_id, assignment) in assignments {
        let feature = features
            .get(&feature_id)
            .ok_or_else(|| contract(format!("asset assignment references {feature_id:?}")))?;
        let role = string_field(assignment, "role", "asset assignment")?;
        if role == "landform_dressing" || !bool_field(assignment, "runtime_enabled", true) {
            continue;
        }
        if role == "foliage" {
            compile_foliage(
                seed,
                feature,
                assignment,
                &asset_bounds,
                terrain,
                &exclusions,
                &mut instances,
                &mut ids,
                &mut shortfalls,
            )?;
        } else {
            compile_anchored(
                seed,
                feature,
                assignment,
                &asset_bounds,
                terrain,
                &mut instances,
                &mut ids,
            )?;
        }
    }

    instances.sort_by(|left, right| {
        left.get("id")
            .and_then(Value::as_str)
            .cmp(&right.get("id").and_then(Value::as_str))
    });
    let mut counts = BTreeMap::<String, usize>::new();
    for instance in &instances {
        let role = instance
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        *counts.entry(role.to_owned()).or_default() += 1;
    }
    let corridors = compile_corridors(features.values().copied(), terrain)?;
    let plan = json!({
        "schema_version": RENDER_PLAN_SCHEMA,
        "zone_spec_sha256": world.canonical_sha256,
        "asset_plan_sha256": canonical_sha256(asset_plan),
        "asset_preflight_sha256": canonical_sha256(asset_preflight),
        "terrain_manifest_sha256": canonical_sha256(terrain_manifest),
        "heightfield_sha256": bytes_sha256(heightfield_bytes),
        "canopy_suitability_sha256": bytes_sha256(suitability_bytes),
        "landform_placement_plan_sha256": canonical_sha256(landform_plan),
        "generator": {
            "name": "codeweald-worldspec",
            "scope": "renderer_independent_composition",
            "version": env!("CARGO_PKG_VERSION"),
            "generation_seed": seed,
        },
        "counts_by_role": counts,
        // Where the ecology gave less than the spec asked for. Empty is the
        // normal case; a non-empty list is the terrain disagreeing with the
        // author, in its own words, rather than a build failure.
        "ecology_shortfalls": shortfalls,
        "corridors": corridors,
        "instances": instances,
    });
    validate_render_plan_value(
        zone,
        asset_plan,
        asset_preflight,
        terrain_manifest,
        heightfield_bytes,
        suitability_bytes,
        landform_plan,
        &plan,
    )?;
    Ok(plan)
}

#[allow(clippy::too_many_arguments)]
pub fn validate_render_plan_value(
    zone: &Value,
    asset_plan: &Value,
    asset_preflight: &Value,
    terrain_manifest: &Value,
    heightfield_bytes: &[u8],
    suitability_bytes: &[u8],
    landform_plan: &Value,
    render_plan: &Value,
) -> Result<usize, WorldSpecError> {
    let world = validate_value(zone)?;
    let root = render_plan
        .as_object()
        .ok_or_else(|| contract("render plan root is not an object"))?;
    if root.get("schema_version").and_then(Value::as_str) != Some(RENDER_PLAN_SCHEMA) {
        return Err(contract(format!(
            "render plan schema_version must be {RENDER_PLAN_SCHEMA:?}"
        )));
    }
    fingerprint(root, "zone_spec_sha256", &world.canonical_sha256)?;
    fingerprint(root, "asset_plan_sha256", &canonical_sha256(asset_plan))?;
    fingerprint(
        root,
        "asset_preflight_sha256",
        &canonical_sha256(asset_preflight),
    )?;
    fingerprint(
        root,
        "terrain_manifest_sha256",
        &canonical_sha256(terrain_manifest),
    )?;
    fingerprint(root, "heightfield_sha256", &bytes_sha256(heightfield_bytes))?;
    // The ecology now decides where things stand, so it is an input the plan is
    // bound to. Without this a stale suitability field would validate against a
    // plan compiled from a different one, which is the exact failure the
    // heightfield binding above exists to prevent.
    fingerprint(
        root,
        "canopy_suitability_sha256",
        &bytes_sha256(suitability_bytes),
    )?;
    fingerprint(
        root,
        "landform_placement_plan_sha256",
        &canonical_sha256(landform_plan),
    )?;

    let known_assets = collect_asset_digests(asset_plan);
    let foliage_slope_limits = collect_foliage_slope_limits(asset_plan)?;
    let protected_landforms = protected_landform_polygons(zone)?;
    let resolution = usize_at(terrain_manifest, "/resolution")?;
    let heights = heightfield_bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
        .collect::<Vec<_>>();
    if suitability_bytes.len() != resolution * resolution {
        return Err(contract(format!(
            "canopy suitability has {} bytes; expected {} for a {resolution}^2 field",
            suitability_bytes.len(),
            resolution * resolution
        )));
    }
    let terrain = Terrain {
        heights: &heights,
        suitability: suitability_bytes,
        resolution,
        width: number_at(terrain_manifest, "/world_bounds_m/width")?,
        length: number_at(terrain_manifest, "/world_bounds_m/length")?,
    };
    let instances = root
        .get("instances")
        .and_then(Value::as_array)
        .ok_or_else(|| contract("render plan instances are missing"))?;
    let mut ids = BTreeSet::new();
    let mut landform_count = 0;
    for (index, instance) in instances.iter().enumerate() {
        let record = instance
            .as_object()
            .ok_or_else(|| contract(format!("render plan instances[{index}] is not an object")))?;
        let id = record
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| contract(format!("render plan instances[{index}].id is missing")))?;
        if !ids.insert(id) {
            return Err(contract(format!(
                "duplicate render-plan instance id {id:?}"
            )));
        }
        let digest = record
            .get("asset_sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| contract(format!("{id}.asset_sha256 is missing")))?;
        if !known_assets.contains(digest) {
            return Err(contract(format!("{id} references unknown asset {digest}")));
        }
        let position = numeric_array(record.get("position_m"), 3, &format!("{id}.position_m"))?;
        let grounding_offset = record
            .get("grounding_offset_m")
            .and_then(Value::as_f64)
            .ok_or_else(|| contract(format!("{id}.grounding_offset_m is missing")))?;
        if !grounding_offset.is_finite() || !(0.0..=2.5).contains(&grounding_offset) {
            return Err(contract(format!(
                "{id} has invalid grounding penetration {grounding_offset}"
            )));
        }
        if position[0].abs() > world.width_m * 0.5 + 1e-6
            || position[2].abs() > world.length_m * 0.5 + 1e-6
        {
            return Err(contract(format!("{id} lies outside world bounds")));
        }
        if record.get("source").and_then(Value::as_str) == Some("rust_scatter") {
            let feature_id = string_field(record, "feature_id", id)?;
            let layer_id = string_field(record, "layer_id", id)?;
            let promised = foliage_slope_limits
                .get(&(feature_id.to_owned(), layer_id.to_owned()))
                .ok_or_else(|| {
                    contract(format!(
                        "{id} has no foliage slope contract for {feature_id}:{layer_id}"
                    ))
                })?;
            let limit = promised.slope_limit;
            let slope = terrain.slope_degrees(position[0], position[2]);
            if slope > limit + 1e-6 {
                return Err(contract(format!(
                    "{id} stands on {slope:.3} degree terrain; limit is {limit:.3}"
                )));
            }
            // **A floor, not a re-derivation.** The compiler draws in
            // proportion to suitability, so there is no single value the
            // validator could demand without reimplementing the RNG and
            // guaranteeing the two drift. What *is* checkable, and is the thing
            // that actually matters, is the boundary: nothing may stand where
            // its ecology says nothing grows. Compiler and validator agree
            // exactly there, which is where agreement is load-bearing.
            if let Some(field) = promised.ecology_field.as_deref() {
                if field != "canopy_suitability" {
                    return Err(contract(format!(
                        "{id} declares unknown ecology field {field:?}"
                    )));
                }
                if terrain.suitability_at(position[0], position[2]) <= 0.0 {
                    return Err(contract(format!(
                        "{id} stands where {field} is zero: the scatter ignored \
                         the ecology it declares"
                    )));
                }
            }
            if protected_landforms
                .iter()
                .any(|polygon| point_in_polygon([position[0], position[2]], polygon))
            {
                return Err(contract(format!(
                    "{id} lies inside protected non-traversable relief"
                )));
            }
        }
        if record.get("source").and_then(Value::as_str) == Some("julia_landform") {
            landform_count += 1;
        }
    }
    let expected_landforms = array_at(landform_plan, "/placements")?.len();
    if landform_count != expected_landforms {
        return Err(contract(format!(
            "render plan has {landform_count} Julia landforms; expected {expected_landforms}"
        )));
    }
    validate_corridors(root.get("corridors"), world.width_m, world.length_m)?;
    Ok(instances.len())
}

/// What the asset plan promised about one foliage layer.
///
/// Slope and ecology travel together because the validator has to re-check both
/// against the same layer, and two parallel maps keyed by the same pair is how
/// they end up disagreeing about which layers exist.
struct FoliageContract {
    slope_limit: f64,
    ecology_field: Option<String>,
}

fn collect_foliage_slope_limits(
    asset_plan: &Value,
) -> Result<BTreeMap<(String, String), FoliageContract>, WorldSpecError> {
    let mut limits = BTreeMap::new();
    for assignment in array_at(asset_plan, "/assignments")? {
        let assignment = assignment
            .as_object()
            .ok_or_else(|| contract("asset assignment is not an object"))?;
        if string_field(assignment, "role", "asset assignment")? != "foliage" {
            continue;
        }
        let feature_id = string_field(assignment, "feature_id", "asset assignment")?;
        for layer in assignment
            .get("layers")
            .and_then(Value::as_array)
            .ok_or_else(|| contract(format!("{feature_id} foliage layers are missing")))?
        {
            let layer = layer
                .as_object()
                .ok_or_else(|| contract(format!("{feature_id} foliage layer is invalid")))?;
            let layer_id = string_field(layer, "id", "foliage layer")?;
            let limit = layer
                .get("maximum_slope_degrees")
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    contract(format!(
                        "{feature_id}:{layer_id}.maximum_slope_degrees is missing"
                    ))
                })?;
            if !limit.is_finite() || !(0.0..=60.0).contains(&limit) {
                return Err(contract(format!(
                    "{feature_id}:{layer_id} has invalid slope limit {limit}"
                )));
            }
            limits.insert(
                (feature_id.to_owned(), layer_id.to_owned()),
                FoliageContract {
                    slope_limit: limit,
                    ecology_field: layer
                        .get("ecology_field")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
            );
        }
    }
    Ok(limits)
}

fn compile_corridors<'a>(
    features: impl Iterator<Item = &'a Map<String, Value>>,
    terrain: Terrain<'_>,
) -> Result<Vec<Value>, WorldSpecError> {
    let mut corridors = Vec::new();
    for feature in features {
        if string_field(feature, "category", "feature")? != "corridor"
            || string_field(feature, "semantic", "feature")? != "lane"
        {
            continue;
        }
        let id = string_field(feature, "id", "feature")?;
        let properties = feature
            .get("properties")
            .and_then(Value::as_object)
            .ok_or_else(|| contract(format!("{id}.properties is missing")))?;
        let width = properties
            .get("visual_width_m")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 2.0 && *value <= 20.0)
            .ok_or_else(|| contract(format!("{id}.visual_width_m is invalid")))?;
        let authored = feature_points(feature)?;
        if authored.len() < 2 {
            return Err(contract(format!("{id} needs at least two route points")));
        }
        let centerline = catmull_rom_route(&authored, 8)
            .into_iter()
            .map(|point| {
                json!([
                    round_six(point[0]),
                    round_six(terrain.sample(point[0], point[1]) + 0.08),
                    round_six(point[1])
                ])
            })
            .collect::<Vec<_>>();
        corridors.push(json!({
            "id": id,
            "semantic": "lane",
            "surface": properties
                .get("surface")
                .and_then(Value::as_str)
                .unwrap_or("packed_dirt"),
            "width_m": round_six(width),
            "route_policy": "catmull-rom-pass-through/v1",
            "centerline_m": centerline,
        }));
    }
    Ok(corridors)
}

fn validate_corridors(
    value: Option<&Value>,
    world_width: f64,
    world_length: f64,
) -> Result<(), WorldSpecError> {
    let corridors = value
        .and_then(Value::as_array)
        .ok_or_else(|| contract("render plan corridors are missing"))?;
    let mut ids = BTreeSet::new();
    for (index, corridor) in corridors.iter().enumerate() {
        let record = corridor
            .as_object()
            .ok_or_else(|| contract(format!("corridors[{index}] is not an object")))?;
        let id = string_field(record, "id", "corridor")?;
        if !ids.insert(id) {
            return Err(contract(format!("duplicate corridor id {id:?}")));
        }
        let width = record
            .get("width_m")
            .and_then(Value::as_f64)
            .ok_or_else(|| contract(format!("{id}.width_m is missing")))?;
        if !width.is_finite() || !(2.0..=20.0).contains(&width) {
            return Err(contract(format!("{id}.width_m is invalid")));
        }
        if record.get("route_policy").and_then(Value::as_str) != Some("catmull-rom-pass-through/v1")
        {
            return Err(contract(format!("{id}.route_policy is invalid")));
        }
        let centerline = record
            .get("centerline_m")
            .and_then(Value::as_array)
            .ok_or_else(|| contract(format!("{id}.centerline_m is missing")))?;
        if centerline.len() < 2 {
            return Err(contract(format!("{id} has too few centerline points")));
        }
        for (point_index, point) in centerline.iter().enumerate() {
            let point = numeric_array(Some(point), 3, &format!("{id}[{point_index}]"))?;
            if point[0].abs() > world_width * 0.5 + 1e-6
                || point[2].abs() > world_length * 0.5 + 1e-6
            {
                return Err(contract(format!(
                    "{id}[{point_index}] lies outside world bounds"
                )));
            }
        }
    }
    Ok(())
}

fn catmull_rom_route(points: &[[f64; 2]], subdivisions: usize) -> Vec<[f64; 2]> {
    if points.len() < 2 || subdivisions == 0 {
        return points.to_vec();
    }
    let mut result = Vec::with_capacity((points.len() - 1) * subdivisions + 1);
    for index in 0..(points.len() - 1) {
        let p0 = points[index.saturating_sub(1)];
        let p1 = points[index];
        let p2 = points[index + 1];
        let p3 = points[(index + 2).min(points.len() - 1)];
        for step in 0..subdivisions {
            let t = step as f64 / subdivisions as f64;
            let t2 = t * t;
            let t3 = t2 * t;
            result.push([0, 1].map(|axis| {
                0.5 * ((2.0 * p1[axis])
                    + (-p0[axis] + p2[axis]) * t
                    + (2.0 * p0[axis] - 5.0 * p1[axis] + 4.0 * p2[axis] - p3[axis]) * t2
                    + (-p0[axis] + 3.0 * p1[axis] - 3.0 * p2[axis] + p3[axis]) * t3)
            }));
        }
    }
    result.push(*points.last().expect("route has points"));
    result
}

fn compile_anchored(
    seed: u64,
    feature: &Map<String, Value>,
    assignment: &Map<String, Value>,
    asset_bounds: &BTreeMap<String, f64>,
    terrain: Terrain<'_>,
    instances: &mut Vec<Map<String, Value>>,
    ids: &mut BTreeSet<String>,
) -> Result<(), WorldSpecError> {
    let feature_id = string_field(feature, "id", "feature")?;
    let role = string_field(assignment, "role", "asset assignment")?;
    let points = feature_points(feature)?;
    let anchor = *points
        .first()
        .ok_or_else(|| contract(format!("{feature_id} has no anchor point")))?;
    let assets = assignment_assets(assignment, asset_bounds)?;
    if assets.is_empty() {
        return Err(contract(format!("{feature_id} has no selected assets")));
    }
    let count = assignment
        .get("instance_count")
        .and_then(Value::as_u64)
        .unwrap_or(1) as usize;
    let scale_range = scale_range(assignment)?;
    let authored_yaw = feature
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get("rotation_degrees"))
        .and_then(Value::as_f64);
    let mut rng = StableRng::from_parts(seed, &[feature_id, role]);
    for index in 0..count {
        let asset = &assets[rng.index(assets.len())];
        let scale = rng.range(scale_range[0], scale_range[1]);
        let yaw = authored_yaw.unwrap_or_else(|| rng.range(0.0, 360.0));
        let id = format!("{feature_id}:primary:{index:04}");
        let record = instance_record(
            &id,
            feature_id,
            "primary",
            role,
            asset,
            [anchor[0], terrain.sample(anchor[0], anchor[1]), anchor[1]],
            yaw,
            scale,
            "rust_anchor",
        );
        push_unique(instances, ids, record)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
/// Every cell inside `polygon` whose suitability is non-zero, with a running
/// cumulative weight for proportional sampling.
///
/// **Why sample from the field instead of rejecting against it.** The obvious
/// change is to keep throwing uniform darts at the bounding box and discard
/// those that land on unsuitable ground. Measured on the alpine arena, canopy
/// suitability is non-zero on 2.7%-5.2% of each forest polygon, and the loop is
/// budgeted at 50 darts per instance: 28 conifers would need roughly 1400 darts
/// to yield 28 hits at a ~2% hit rate, which is the expected value exactly. So
/// the build would fail the `placed N of target` contract intermittently, on
/// terrain rather than on code, and the failure would move around as the map
/// changed. Sampling the field directly cannot miss.
///
/// It also gets the *shape* right, which is the actual point. Drawing in
/// proportion to suitability makes density fall off with the niche, so the
/// treeline thins instead of stopping at a contour -- the stamped edge the soft
/// niche bands in S6 exist to avoid.
/// **The admissible set, not merely the suitable one (D33).**
///
/// This used to weight cells by suitability alone and leave slope and exclusions
/// to the rejection test inside the scatter loop. That reintroduced exactly the
/// fragility the doc comment above says sampling the field avoids, because the
/// dart budget was then spent on ground no tree could ever stand on. Measured
/// 2026-08-04, share of each polygon's suitability weight that was actually
/// reachable once lanes, streams, keeps and protected landforms were accounted
/// for:
///
/// | forest | arena | gentler world |
/// |---|---|---|
/// | `central_forest` | 3.3% | 1.4% |
/// | `western_valley_woodland` | 26.0% | 8.5% |
/// | `eastern_valley_woodland` | 4.3% | **0.0%** |
///
/// At 3.3% reachable, 1400 darts yield ~46 hits, so the nine trees
/// `central_forest` placed were partly a fact about the dart budget rather than
/// about the ecology -- which means the shortfall counts reported on 2026-08-03
/// were not the pure "terrain answering" they were described as. Filtering here
/// makes every dart land on admissible ground, so the placed count is decided by
/// the ecology and the spacing and nothing else.
///
/// It also makes the empty case answerable *before* sampling, and answerable
/// precisely: a polygon with no admissible cell is a different statement from a
/// sampler that happened to miss.
fn suitable_cells(
    terrain: Terrain<'_>,
    polygon: &[[f64; 2]],
    bounds: [f64; 4],
    maximum_slope: f64,
    exclusions: &[Exclusion],
) -> (Vec<[f64; 2]>, Vec<f64>) {
    let [min_x, max_x, min_z, max_z] = bounds;
    let last = (terrain.resolution - 1) as f64;
    let column_of = |x: f64| ((x / terrain.width + 0.5).clamp(0.0, 1.0) * last).round() as usize;
    let row_of = |z: f64| ((0.5 - z / terrain.length).clamp(0.0, 1.0) * last).round() as usize;
    // z increases northward while rows increase southward, hence the swap.
    let first_row = row_of(max_z);
    let last_row = row_of(min_z);
    let first_column = column_of(min_x);
    let last_column = column_of(max_x);

    let mut positions = Vec::new();
    let mut cumulative = Vec::new();
    let mut running = 0.0_f64;
    for row in first_row..=last_row {
        for column in first_column..=last_column {
            // Cheap array lookup first: it prunes ~95% of the box before the
            // per-vertex polygon test ever runs.
            let weight = terrain.suitability[row * terrain.resolution + column] as f64;
            if weight <= 0.0 {
                continue;
            }
            let position = terrain.cell_position(row, column);
            if !point_in_polygon(position, polygon) {
                continue;
            }
            // Same three tests the scatter loop applies, applied once here
            // instead of once per dart. Spacing stays in the loop: it depends
            // on what has already been accepted and has no meaning per cell.
            if terrain.slope_degrees(position[0], position[1]) > maximum_slope {
                continue;
            }
            if exclusions
                .iter()
                .any(|exclusion| excluded(position, exclusion))
            {
                continue;
            }
            running += weight / 255.0;
            positions.push(position);
            cumulative.push(running);
        }
    }
    (positions, cumulative)
}

/// Draw one cell in proportion to its weight, then jitter inside it.
///
/// Without the jitter every instance would sit exactly on a grid node, which at
/// this resolution is a 0.25 m lattice -- invisible individually and unmistakable
/// across a hillside.
fn draw_weighted(
    positions: &[[f64; 2]],
    cumulative: &[f64],
    cell_m: [f64; 2],
    rng: &mut StableRng,
) -> [f64; 2] {
    let total = cumulative.last().copied().unwrap_or(0.0);
    let target = rng.range(0.0, total);
    let index = match cumulative
        .binary_search_by(|value| value.partial_cmp(&target).unwrap_or(Ordering::Less))
    {
        Ok(index) => index,
        Err(index) => index.min(positions.len() - 1),
    };
    // **The jitter must stay strictly inside its own cell.**
    //
    // `suitability_at` rounds a world position to the nearest cell, and Rust
    // rounds halves *away from zero*, so an offset of exactly -0.5 cells lands
    // in the previous one. `rng.range(-half, half)` returns exactly `-half`
    // whenever `unit()` returns 0.0, which is reachable. That would place an
    // instance whose suitability the validator then reads from a neighbouring
    // cell -- and if that neighbour is zero, the validator rejects a plan the
    // compiler just built. Rare enough (~2^-53 a draw) to present as a ghost
    // failure on one machine and not another, which is the worst kind.
    //
    // 0.49 keeps every draw inside the cell it was chosen from, at a cost of 2%
    // of the jitter range that nobody can see.
    const INSIDE_CELL: f64 = 0.49;
    // Per-axis, because a world is not required to be square and the two cell
    // sizes are only equal when it is.
    [
        positions[index][0] + rng.range(-cell_m[0] * INSIDE_CELL, cell_m[0] * INSIDE_CELL),
        positions[index][1] + rng.range(-cell_m[1] * INSIDE_CELL, cell_m[1] * INSIDE_CELL),
    ]
}

#[allow(clippy::too_many_arguments)]
fn compile_foliage(
    seed: u64,
    feature: &Map<String, Value>,
    assignment: &Map<String, Value>,
    asset_bounds: &BTreeMap<String, f64>,
    terrain: Terrain<'_>,
    exclusions: &[Exclusion],
    instances: &mut Vec<Map<String, Value>>,
    ids: &mut BTreeSet<String>,
    shortfalls: &mut Vec<Value>,
) -> Result<(), WorldSpecError> {
    let feature_id = string_field(feature, "id", "feature")?;
    let polygon = feature_points(feature)?;
    if polygon.len() < 3 {
        return Err(contract(format!("{feature_id} forest needs a polygon")));
    }
    let min_x = polygon
        .iter()
        .map(|point| point[0])
        .fold(f64::INFINITY, f64::min);
    let max_x = polygon
        .iter()
        .map(|point| point[0])
        .fold(f64::NEG_INFINITY, f64::max);
    let min_z = polygon
        .iter()
        .map(|point| point[1])
        .fold(f64::INFINITY, f64::min);
    let max_z = polygon
        .iter()
        .map(|point| point[1])
        .fold(f64::NEG_INFINITY, f64::max);
    let layers = assignment
        .get("layers")
        .and_then(Value::as_array)
        .ok_or_else(|| contract(format!("{feature_id} has no ecological layers")))?;
    for layer_value in layers {
        let layer = layer_value
            .as_object()
            .ok_or_else(|| contract(format!("{feature_id} layer is not an object")))?;
        if !bool_field(layer, "runtime_enabled", true) {
            continue;
        }
        let layer_id = string_field(layer, "id", "ecological layer")?;
        let role = string_field(layer, "role", "ecological layer")?;
        let target = layer
            .get("instance_count")
            .and_then(Value::as_u64)
            .ok_or_else(|| contract(format!("{feature_id}:{layer_id} count is missing")))?
            as usize;
        let spacing = layer
            .get("minimum_spacing_m")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let maximum_slope = layer
            .get("maximum_slope_degrees")
            .and_then(Value::as_f64)
            .unwrap_or(35.0);
        // Declared by the asset plan, never inferred from the role name. Absent
        // means "no ecological field describes this layer yet", which is a
        // statement rather than an omission -- see `asset_plan.ECOLOGY_FIELDS`.
        let ecology_field = layer.get("ecology_field").and_then(Value::as_str);
        let scales = scale_range(layer)?;
        let assets = assignment_assets(layer, asset_bounds)?;
        if assets.is_empty() {
            return Err(contract(format!(
                "{feature_id}:{layer_id} has no selected assets"
            )));
        }
        let ecology = match ecology_field {
            None => None,
            Some("canopy_suitability") => {
                let (positions, cumulative) = suitable_cells(
                    terrain,
                    &polygon,
                    [min_x, max_x, min_z, max_z],
                    maximum_slope,
                    exclusions,
                );
                if positions.is_empty() {
                    // **Two different statements, and they must not share an
                    // error (D33).** If the field is zero everywhere in the
                    // polygon, the author put a forest where its own ecology
                    // forbids one -- a spec defect, and hard.
                    //
                    // If the field is non-zero but every such cell is under a
                    // lane, a keep, a stream or a protected landform, the spec
                    // is fine and the world simply has no room left. That is the
                    // same "terrain answering" the ceiling rule exists for, and
                    // failing the build for it would demand the author measure
                    // an occupancy they cannot see. It is recorded as a
                    // shortfall of zero and printed, never swallowed.
                    let (unfiltered, _) = suitable_cells(
                        terrain,
                        &polygon,
                        [min_x, max_x, min_z, max_z],
                        f64::INFINITY,
                        &[],
                    );
                    if unfiltered.is_empty() {
                        return Err(contract(format!(
                            "{feature_id}:{layer_id} declares canopy_suitability but the \
                             field is zero across its whole polygon: this forest is \
                             authored somewhere its own ecology forbids"
                        )));
                    }
                    shortfalls.push(json!({
                        "feature_id": feature_id,
                        "layer_id": layer_id,
                        "requested": target,
                        "placed": 0,
                        "limited_by": "occupancy",
                        "suitable_cells": unfiltered.len(),
                        "admissible_cells": 0,
                    }));
                    continue;
                }
                Some((positions, cumulative))
            }
            Some(other) => {
                return Err(contract(format!(
                    "{feature_id}:{layer_id} declares unknown ecology field {other:?}"
                )));
            }
        };
        let cell_m = [
            terrain.width / (terrain.resolution - 1) as f64,
            terrain.length / (terrain.resolution - 1) as f64,
        ];

        let mut rng = StableRng::from_parts(seed, &[feature_id, layer_id, role]);
        let mut accepted = Vec::<[f64; 2]>::new();
        for _ in 0..target.saturating_mul(50).max(1) {
            if accepted.len() == target {
                break;
            }
            let candidate = match &ecology {
                Some((positions, cumulative)) => {
                    draw_weighted(positions, cumulative, cell_m, &mut rng)
                }
                None => [rng.range(min_x, max_x), rng.range(min_z, max_z)],
            };
            if !point_in_polygon(candidate, &polygon)
                || terrain.slope_degrees(candidate[0], candidate[1]) > maximum_slope
                || exclusions
                    .iter()
                    .any(|exclusion| excluded(candidate, exclusion))
                || accepted
                    .iter()
                    .any(|other| distance(*other, candidate) < spacing)
            {
                continue;
            }
            let asset = &assets[rng.index(assets.len())];
            let id = format!("{feature_id}:{layer_id}:{:04}", accepted.len());
            let record = instance_record(
                &id,
                feature_id,
                layer_id,
                role,
                asset,
                [
                    candidate[0],
                    terrain.sample(candidate[0], candidate[1]),
                    candidate[1],
                ],
                rng.range(0.0, 360.0),
                rng.range(scales[0], scales[1]),
                "rust_scatter",
            );
            push_unique(instances, ids, record)?;
            accepted.push(candidate);
        }
        // **Under an ecology, the authored count is a ceiling, not a quota.**
        //
        // That is the whole point of obeying the field. `central_forest` asks
        // for 28 conifers; canopy suitability is non-zero on 2.7% of its
        // polygon, and 1.8 m spacing over that much ground holds nine. The old
        // rule would call that a failed build. But the author did not measure
        // the ecology -- the solver did -- so a shortfall is the terrain
        // answering, not the spec being wrong.
        //
        // Layers with no ecological field keep the strict quota. Nothing there
        // has an opinion about density, so falling short really is a failure to
        // place, and weakening it would hide a real defect.
        if accepted.len() != target {
            if ecology.is_none() {
                return Err(contract(format!(
                    "{feature_id}:{layer_id} placed {} of {target} instances",
                    accepted.len()
                )));
            }
            if accepted.is_empty() {
                // Since D33 the candidate set is pre-filtered by slope and
                // exclusions, and an empty one is reported as a shortfall
                // before we get here. So reaching this point means every dart
                // landed on admissible ground and was still refused, which only
                // spacing can do -- and spacing cannot refuse the first dart,
                // because nothing has been accepted yet. This is therefore a
                // defect in the scatter, not a fact about the world, and it is
                // deliberately kept hard so it cannot be mistaken for one.
                return Err(contract(format!(
                    "{feature_id}:{layer_id} placed nothing from a non-empty \
                     admissible set: the scatter refused every candidate it was \
                     given, which spacing alone cannot do"
                )));
            }
            // Recorded rather than swallowed. A forest that quietly becomes
            // nine trees is exactly the kind of silent thinning that makes a
            // world look broken with every gate green.
            shortfalls.push(json!({
                "feature_id": feature_id,
                "layer_id": layer_id,
                "requested": target,
                "placed": accepted.len(),
                "limited_by": "ecology",
            }));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn instance_record(
    id: &str,
    feature_id: &str,
    layer_id: &str,
    role: &str,
    asset: &AssetRef,
    mut position: [f64; 3],
    yaw: f64,
    scale: f64,
    source: &str,
) -> Map<String, Value> {
    let grounding_offset = grounding_offset(role, asset.vertical_size_m, scale);
    position[1] -= grounding_offset;
    let mut record = Map::new();
    record.insert("id".into(), json!(id));
    record.insert("feature_id".into(), json!(feature_id));
    record.insert("layer_id".into(), json!(layer_id));
    record.insert("role".into(), json!(role));
    record.insert("asset_sha256".into(), json!(asset.sha256));
    if !asset.lod_assets.is_empty() {
        record.insert("lod_assets".into(), Value::Object(asset.lod_assets.clone()));
    }
    record.insert(
        "position_m".into(),
        json!(position.into_iter().map(round_six).collect::<Vec<_>>()),
    );
    record.insert(
        "grounding_offset_m".into(),
        json!(round_six(grounding_offset)),
    );
    record.insert(
        "grounding_policy".into(),
        json!(if role == "settlement_landmark" {
            "root_penetration_then_component_conform"
        } else {
            "measured_bounds_penetration"
        }),
    );
    record.insert("yaw_degrees".into(), json!(round_six(yaw)));
    record.insert("scale".into(), json!(round_six(scale)));
    record.insert("source".into(), json!(source));
    record
}

fn push_unique(
    instances: &mut Vec<Map<String, Value>>,
    ids: &mut BTreeSet<String>,
    record: Map<String, Value>,
) -> Result<(), WorldSpecError> {
    let id = record
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| contract("instance id is missing"))?;
    if !ids.insert(id.to_owned()) {
        return Err(contract(format!("duplicate compiled instance id {id:?}")));
    }
    instances.push(record);
    Ok(())
}

fn indexed_objects<'a>(
    root: &'a Value,
    field: &str,
    label: &str,
) -> Result<BTreeMap<String, &'a Map<String, Value>>, WorldSpecError> {
    let values = root
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| contract(format!("{label}.{field} is missing")))?;
    let mut indexed = BTreeMap::new();
    for value in values {
        let object = value
            .as_object()
            .ok_or_else(|| contract(format!("{label}.{field} entry is not an object")))?;
        let id_field = if field == "assignments" {
            "feature_id"
        } else {
            "id"
        };
        let id = string_field(object, id_field, label)?;
        if indexed.insert(id.to_owned(), object).is_some() {
            return Err(contract(format!("duplicate {label} id {id:?}")));
        }
    }
    Ok(indexed)
}

fn assignment_assets(
    assignment: &Map<String, Value>,
    asset_bounds: &BTreeMap<String, f64>,
) -> Result<Vec<AssetRef>, WorldSpecError> {
    let values = assignment
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| contract("asset selection is missing"))?;
    values
        .iter()
        .map(|value| {
            let asset = value
                .as_object()
                .ok_or_else(|| contract("selected asset is not an object"))?;
            let sha256 = string_field(asset, "sha256", "selected asset")?.to_owned();
            let mut lod_assets = Map::new();
            if let Some(levels) = asset.get("lod_assets").and_then(Value::as_object) {
                for (level, value) in levels {
                    if let Some(digest) = value.get("sha256").and_then(Value::as_str) {
                        lod_assets.insert(level.clone(), json!(digest));
                    }
                }
            }
            let vertical_size_m = *asset_bounds.get(&sha256).ok_or_else(|| {
                contract(format!(
                    "selected asset {sha256} has no measured physical bounds"
                ))
            })?;
            Ok(AssetRef {
                sha256,
                lod_assets,
                vertical_size_m,
            })
        })
        .collect()
}

fn asset_vertical_sizes(
    asset_plan: &Value,
    asset_preflight: &Value,
) -> Result<BTreeMap<String, f64>, WorldSpecError> {
    let probes = asset_preflight
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| contract("asset preflight has no measured assets"))?;
    let mut heights_by_path = BTreeMap::<String, f64>::new();
    for probe in probes {
        let path = probe
            .get("source_path")
            .and_then(Value::as_str)
            .ok_or_else(|| contract("asset preflight source_path is missing"))?;
        let size = numeric_array(
            probe.pointer("/bounds_m/size"),
            3,
            "asset preflight bounds_m.size",
        )?;
        if size[2] <= 0.0 {
            return Err(contract(format!(
                "asset preflight has invalid vertical bounds for {path}"
            )));
        }
        heights_by_path.insert(path.to_owned(), size[2]);
    }
    let mut result = BTreeMap::new();
    fn visit(
        value: &Value,
        heights_by_path: &BTreeMap<String, f64>,
        result: &mut BTreeMap<String, f64>,
    ) {
        match value {
            Value::Object(object) => {
                if let (Some(digest), Some(path)) = (
                    object.get("sha256").and_then(Value::as_str),
                    object.get("source_path").and_then(Value::as_str),
                ) && let Some(height) = heights_by_path.get(path)
                {
                    result.insert(digest.to_owned(), *height);
                }
                for child in object.values() {
                    visit(child, heights_by_path, result);
                }
            }
            Value::Array(values) => {
                for child in values {
                    visit(child, heights_by_path, result);
                }
            }
            _ => {}
        }
    }
    visit(asset_plan, &heights_by_path, &mut result);
    Ok(result)
}

fn apply_grounding(
    record: &mut Map<String, Value>,
    role: &str,
    asset_bounds: &BTreeMap<String, f64>,
) -> Result<(), WorldSpecError> {
    let id = record
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("landform")
        .to_owned();
    let digest = record
        .get("asset_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| contract(format!("{id}.asset_sha256 is missing")))?;
    let vertical_size = *asset_bounds
        .get(digest)
        .ok_or_else(|| contract(format!("{id} has no measured physical bounds")))?;
    let scale = record
        .get("scale")
        .and_then(Value::as_f64)
        .ok_or_else(|| contract(format!("{id}.scale is missing")))?;
    let mut position = numeric_array(record.get("position_m"), 3, &format!("{id}.position_m"))?;
    let offset = grounding_offset(role, vertical_size, scale);
    position[1] -= offset;
    record.insert(
        "position_m".into(),
        json!(position.into_iter().map(round_six).collect::<Vec<_>>()),
    );
    record.insert("grounding_offset_m".into(), json!(round_six(offset)));
    record.insert(
        "grounding_policy".into(),
        json!("measured_bounds_penetration"),
    );
    Ok(())
}

fn grounding_offset(role: &str, vertical_size_m: f64, scale: f64) -> f64 {
    let visible_height = vertical_size_m * scale;
    let (ratio, minimum, maximum) = match role {
        "landform_dressing" => (0.08, 0.5, 2.5),
        "forest_floor_rock" => (0.12, 0.05, 0.35),
        "conifer_canopy" => (0.025, 0.05, 0.35),
        "highland_understory" | "highland_groundcover" => (0.03, 0.01, 0.15),
        "lane_crossing_structure" => (0.01, 0.03, 0.15),
        "faction_fortification" | "settlement_landmark" | "objective_landmark" => {
            (0.015, 0.05, 0.25)
        }
        _ => (0.01, 0.0, 0.2),
    };
    (visible_height * ratio).clamp(minimum, maximum)
}

fn collect_asset_digests(asset_plan: &Value) -> BTreeSet<String> {
    let mut digests = BTreeSet::new();
    fn visit(value: &Value, digests: &mut BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if let Some(digest) = object.get("sha256").and_then(Value::as_str)
                    && digest.len() == 64
                {
                    digests.insert(digest.to_owned());
                }
                for child in object.values() {
                    visit(child, digests);
                }
            }
            Value::Array(values) => {
                for child in values {
                    visit(child, digests);
                }
            }
            _ => {}
        }
    }
    visit(asset_plan, &mut digests);
    digests
}

fn build_exclusions<'a>(
    features: impl Iterator<Item = &'a Map<String, Value>>,
) -> Result<Vec<Exclusion>, WorldSpecError> {
    let mut exclusions = Vec::new();
    for feature in features {
        let semantic = string_field(feature, "semantic", "feature")?;
        let properties = feature.get("properties").and_then(Value::as_object);
        let protected_relief = string_field(feature, "category", "feature")? == "landform"
            && !properties
                .and_then(|value| value.get("traversable"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
        let number = |key: &str, fallback: f64| {
            properties
                .and_then(|value| value.get(key))
                .and_then(Value::as_f64)
                .unwrap_or(fallback)
        };
        let radius = match semantic {
            "lane" => number("minimum_width_m", 24.0) * 0.5 + 7.0,
            "stream" => number("width_m", 10.0) * 0.5 + 5.0,
            "faction_keep" => number("scatter_exclusion_radius_m", 150.0),
            "arcane_ruin" => number("scatter_exclusion_radius_m", 58.0),
            "settlement_cluster" => number("scatter_exclusion_radius_m", 44.0),
            _ => 0.0,
        };
        if radius > 0.0 || protected_relief {
            let points = feature_points(feature)?;
            if !points.is_empty() {
                exclusions.push(Exclusion {
                    points,
                    radius,
                    filled: protected_relief,
                });
            }
        }
    }
    Ok(exclusions)
}

fn excluded(point: [f64; 2], exclusion: &Exclusion) -> bool {
    if exclusion.filled && exclusion.points.len() >= 3 && point_in_polygon(point, &exclusion.points)
    {
        return true;
    }
    if exclusion.points.len() == 1 {
        return distance(point, exclusion.points[0]) < exclusion.radius;
    }
    exclusion
        .points
        .windows(2)
        .any(|segment| distance_to_segment(point, segment[0], segment[1]) < exclusion.radius)
}

fn protected_landform_polygons(zone: &Value) -> Result<Vec<Vec<[f64; 2]>>, WorldSpecError> {
    let mut polygons = Vec::new();
    for feature in array_at(zone, "/features")? {
        let record = feature
            .as_object()
            .ok_or_else(|| contract("ZoneSpec feature is not an object"))?;
        if string_field(record, "category", "feature")? != "landform" {
            continue;
        }
        let traversable = record
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|properties| properties.get("traversable"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !traversable {
            let points = feature_points(record)?;
            if points.len() >= 3 {
                polygons.push(points);
            }
        }
    }
    Ok(polygons)
}

fn point_in_polygon(point: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut previous = polygon.len() - 1;
    for current in 0..polygon.len() {
        let a = polygon[current];
        let b = polygon[previous];
        if ((a[1] > point[1]) != (b[1] > point[1]))
            && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn distance(left: [f64; 2], right: [f64; 2]) -> f64 {
    (left[0] - right[0]).hypot(left[1] - right[1])
}

fn distance_to_segment(point: [f64; 2], start: [f64; 2], finish: [f64; 2]) -> f64 {
    let segment = [finish[0] - start[0], finish[1] - start[1]];
    let length_squared = segment[0] * segment[0] + segment[1] * segment[1];
    if length_squared <= 1e-9 {
        return distance(point, start);
    }
    let offset = (((point[0] - start[0]) * segment[0] + (point[1] - start[1]) * segment[1])
        / length_squared)
        .clamp(0.0, 1.0);
    distance(
        point,
        [
            start[0] + segment[0] * offset,
            start[1] + segment[1] * offset,
        ],
    )
}

fn feature_points(feature: &Map<String, Value>) -> Result<Vec<[f64; 2]>, WorldSpecError> {
    let points = feature
        .get("geometry")
        .and_then(Value::as_object)
        .and_then(|geometry| geometry.get("points"))
        .and_then(Value::as_array)
        .ok_or_else(|| contract("feature geometry points are missing"))?;
    points
        .iter()
        .map(|point| {
            let values = numeric_array(Some(point), 2, "feature point")?;
            Ok([values[0], values[1]])
        })
        .collect()
}

fn scale_range(object: &Map<String, Value>) -> Result<[f64; 2], WorldSpecError> {
    let values = numeric_array(object.get("scale_m"), 2, "scale_m")?;
    if values[0] <= 0.0 || values[1] < values[0] {
        return Err(contract("scale_m must be a positive ordered range"));
    }
    Ok([values[0], values[1]])
}

fn numeric_array(
    value: Option<&Value>,
    expected: usize,
    label: &str,
) -> Result<Vec<f64>, WorldSpecError> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| contract(format!("{label} is not an array")))?;
    if values.len() != expected {
        return Err(contract(format!("{label} must have {expected} values")));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| contract(format!("{label} contains a non-finite number")))
        })
        .collect()
}

fn array_at<'a>(root: &'a Value, pointer: &str) -> Result<&'a Vec<Value>, WorldSpecError> {
    root.pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| contract(format!("{pointer} is not an array")))
}

fn number_at(root: &Value, pointer: &str) -> Result<f64, WorldSpecError> {
    root.pointer(pointer)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| contract(format!("{pointer} is not a positive number")))
}

fn usize_at(root: &Value, pointer: &str) -> Result<usize, WorldSpecError> {
    root.pointer(pointer)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value >= 2)
        .ok_or_else(|| contract(format!("{pointer} is not a valid resolution")))
}

fn string_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> Result<&'a str, WorldSpecError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| contract(format!("{label}.{field} is missing")))
}

fn bool_field(object: &Map<String, Value>, field: &str, fallback: bool) -> bool {
    object
        .get(field)
        .and_then(Value::as_bool)
        .unwrap_or(fallback)
}

fn fingerprint(
    root: &Map<String, Value>,
    field: &str,
    expected: &str,
) -> Result<(), WorldSpecError> {
    if root.get(field).and_then(Value::as_str) != Some(expected) {
        return Err(contract(format!(
            "{field} does not match its source artifact"
        )));
    }
    Ok(())
}

fn bytes_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn round_six(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn contract(message: impl Into<String>) -> WorldSpecError {
    WorldSpecError::Contract(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat 65^2 world 64 m square, with suitability confined to one band.
    fn ecology_fixture(suitable_columns: std::ops::Range<usize>) -> (Vec<f32>, Vec<u8>) {
        let resolution = 65;
        let heights = vec![0.0_f32; resolution * resolution];
        let mut suitability = vec![0_u8; resolution * resolution];
        for row in 0..resolution {
            for column in suitable_columns.clone() {
                suitability[row * resolution + column] = 255;
            }
        }
        (heights, suitability)
    }

    #[test]
    fn the_scatter_only_draws_where_the_ecology_permits() {
        // The crossing itself: S6 measures, and the scatter obeys. Before this,
        // four solvers were correct and unread -- the trees in the viewer were
        // wherever the polygon scatter happened to put them.
        let (heights, suitability) = ecology_fixture(48..56);
        let terrain = Terrain {
            heights: &heights,
            suitability: &suitability,
            resolution: 65,
            width: 64.0,
            length: 64.0,
        };
        let polygon = [[-32.0, -32.0], [32.0, -32.0], [32.0, 32.0], [-32.0, 32.0]];
        let (positions, cumulative) = suitable_cells(
            terrain,
            &polygon,
            [-32.0, 32.0, -32.0, 32.0],
            f64::INFINITY,
            &[],
        );
        assert!(!positions.is_empty(), "the suitable band must be found");
        assert_eq!(positions.len(), cumulative.len());
        for position in &positions {
            assert!(
                terrain.suitability_at(position[0], position[1]) > 0.0,
                "candidate at {position:?} sits where nothing grows"
            );
        }
        // And every draw lands in the band, jitter included.
        let mut rng = StableRng::from_parts(11, &["forest", "canopy"]);
        let cell_m = [64.0 / 64.0, 64.0 / 64.0];
        for _ in 0..200 {
            let drawn = draw_weighted(&positions, &cumulative, cell_m, &mut rng);
            assert!(
                terrain.suitability_at(drawn[0], drawn[1]) > 0.0,
                "jitter pushed a draw off suitable ground at {drawn:?}"
            );
        }
    }

    #[test]
    fn jitter_never_leaves_the_cell_it_was_drawn_from() {
        // Found by red-teaming the crossing, not by a failing build.
        //
        // `suitability_at` rounds to the nearest cell and Rust rounds halves
        // away from zero, so an offset of exactly -0.5 cells reads the previous
        // cell. `rng.range(-half, half)` hits its lower bound whenever `unit()`
        // returns 0.0. At ~2^-53 per draw that is a build that fails once in a
        // blue moon, on one machine, with the validator rejecting a plan the
        // compiler had just built -- the precise compile/validate divergence
        // this crossing was written carefully to avoid.
        //
        // Checked against the worst case directly rather than by sampling: no
        // number of random draws makes a 2^-53 event show up in a test.
        let positions = [[0.0, 0.0]];
        let cumulative = [1.0];
        let cell = [2.0, 2.0];
        let mut rng = StableRng::from_parts(1, &["jitter"]);
        for _ in 0..5000 {
            let drawn = draw_weighted(&positions, &cumulative, cell, &mut rng);
            assert!(
                drawn[0].abs() < cell[0] * 0.5 && drawn[1].abs() < cell[1] * 0.5,
                "jitter {drawn:?} reached or passed the cell boundary"
            );
        }
    }

    #[test]
    fn jitter_uses_each_axis_own_cell_size() {
        // The other half of the same defect: a world is not required to be
        // square, and jittering z by the width-derived cell size overshoots
        // whenever it is not.
        let positions = [[0.0, 0.0]];
        let cumulative = [1.0];
        let cell = [8.0, 1.0];
        let mut rng = StableRng::from_parts(2, &["jitter"]);
        let mut widest_z: f64 = 0.0;
        for _ in 0..5000 {
            let drawn = draw_weighted(&positions, &cumulative, cell, &mut rng);
            widest_z = widest_z.max(drawn[1].abs());
            assert!(
                drawn[1].abs() < cell[1] * 0.5,
                "z jitter used the x cell size"
            );
        }
        assert!(widest_z > cell[1] * 0.3, "z jitter collapsed to nothing");
    }

    #[test]
    fn the_polygon_still_bounds_the_ecology() {
        // Suitability is a world field and does not know about the authored
        // polygon. A forest must not spill across the map because the ground
        // happens to suit it there.
        let (heights, suitability) = ecology_fixture(0..65);
        let terrain = Terrain {
            heights: &heights,
            suitability: &suitability,
            resolution: 65,
            width: 64.0,
            length: 64.0,
        };
        let polygon = [[-8.0, -8.0], [8.0, -8.0], [8.0, 8.0], [-8.0, 8.0]];
        let (positions, _) = suitable_cells(
            terrain,
            &polygon,
            [-8.0, 8.0, -8.0, 8.0],
            f64::INFINITY,
            &[],
        );
        assert!(!positions.is_empty());
        for position in &positions {
            assert!(
                point_in_polygon(*position, &polygon),
                "candidate at {position:?} escaped the authored polygon"
            );
        }
    }

    #[test]
    fn sampling_is_proportional_to_suitability() {
        // A hard cut would stamp an edge; the treeline has to thin. Two bands,
        // one four times as suitable, must draw roughly four times as often.
        let resolution = 65;
        let heights = vec![0.0_f32; resolution * resolution];
        let mut suitability = vec![0_u8; resolution * resolution];
        for row in 0..resolution {
            suitability[row * resolution + 10] = 200;
            suitability[row * resolution + 50] = 50;
        }
        let terrain = Terrain {
            heights: &heights,
            suitability: &suitability,
            resolution,
            width: 64.0,
            length: 64.0,
        };
        let polygon = [[-32.0, -32.0], [32.0, -32.0], [32.0, 32.0], [-32.0, 32.0]];
        let (positions, cumulative) = suitable_cells(
            terrain,
            &polygon,
            [-32.0, 32.0, -32.0, 32.0],
            f64::INFINITY,
            &[],
        );
        let mut rng = StableRng::from_parts(3, &["forest", "canopy"]);
        let mut rich = 0;
        let mut poor = 0;
        for _ in 0..4000 {
            let drawn = draw_weighted(&positions, &cumulative, [0.0, 0.0], &mut rng);
            if drawn[0] < 0.0 { rich += 1 } else { poor += 1 }
        }
        let ratio = rich as f64 / poor as f64;
        assert!(
            (2.5..=6.0).contains(&ratio),
            "expected roughly 4:1 by weight, measured {ratio:.2}"
        );
    }

    #[test]
    fn a_zero_field_is_refused_rather_than_silently_emptying_a_forest() {
        let (heights, suitability) = ecology_fixture(0..0);
        let terrain = Terrain {
            heights: &heights,
            suitability: &suitability,
            resolution: 65,
            width: 64.0,
            length: 64.0,
        };
        let polygon = [[-32.0, -32.0], [32.0, -32.0], [32.0, 32.0], [-32.0, 32.0]];
        let (positions, _) = suitable_cells(
            terrain,
            &polygon,
            [-32.0, 32.0, -32.0, 32.0],
            f64::INFINITY,
            &[],
        );
        assert!(
            positions.is_empty(),
            "an all-zero field must yield no candidates, so compile_foliage errors"
        );
    }

    /// D33: the candidate set must be the *admissible* set.
    ///
    /// Before this, slope and exclusions were rejection tests applied after the
    /// draw, so the dart budget was spent on ground no tree could stand on --
    /// measured at 3.3% of `central_forest`'s weight actually reachable on the
    /// arena, and 0.0% for `eastern_valley_woodland` on the gentler world.
    #[test]
    fn exclusions_are_applied_when_the_candidate_set_is_built() {
        let (heights, suitability) = ecology_fixture(0..65);
        let terrain = Terrain {
            heights: &heights,
            suitability: &suitability,
            resolution: 65,
            width: 64.0,
            length: 64.0,
        };
        let polygon = [[-32.0, -32.0], [32.0, -32.0], [32.0, 32.0], [-32.0, 32.0]];
        // A lane straight down the middle of an otherwise wholly suitable world.
        let lane = Exclusion {
            points: vec![[0.0, -32.0], [0.0, 32.0]],
            radius: 10.0,
            filled: false,
        };

        let (unfiltered, _) = suitable_cells(
            terrain,
            &polygon,
            [-32.0, 32.0, -32.0, 32.0],
            f64::INFINITY,
            &[],
        );
        let (admissible, _) = suitable_cells(
            terrain,
            &polygon,
            [-32.0, 32.0, -32.0, 32.0],
            f64::INFINITY,
            std::slice::from_ref(&lane),
        );

        // The negative case that makes the positive one mean something: without
        // the exclusion the lane's ground is offered as candidates.
        assert!(
            unfiltered.iter().any(|p| excluded(*p, &lane)),
            "precondition: the unfiltered set must include ground under the lane"
        );
        assert!(!admissible.is_empty(), "the lane must not empty the world");
        assert!(admissible.len() < unfiltered.len());
        for position in &admissible {
            assert!(
                !excluded(*position, &lane),
                "candidate at {position:?} sits under the lane"
            );
        }
    }

    #[test]
    fn slope_is_applied_when_the_candidate_set_is_built() {
        // A world tilted steeply on one side. Suitability says yes everywhere;
        // the slope limit must remove half of it before any dart is thrown.
        let resolution = 65;
        let mut heights = vec![0.0_f32; resolution * resolution];
        for row in 0..resolution {
            for column in 0..resolution {
                // ~45 degrees on the east half, flat on the west.
                heights[row * resolution + column] = if column > 32 {
                    (column as f32 - 32.0) * 1.0
                } else {
                    0.0
                };
            }
        }
        let suitability = vec![255_u8; resolution * resolution];
        let terrain = Terrain {
            heights: &heights,
            suitability: &suitability,
            resolution,
            width: 64.0,
            length: 64.0,
        };
        let polygon = [[-32.0, -32.0], [32.0, -32.0], [32.0, 32.0], [-32.0, 32.0]];
        let (all, _) = suitable_cells(
            terrain,
            &polygon,
            [-32.0, 32.0, -32.0, 32.0],
            f64::INFINITY,
            &[],
        );
        let (gentle, _) = suitable_cells(terrain, &polygon, [-32.0, 32.0, -32.0, 32.0], 20.0, &[]);
        assert!(gentle.len() < all.len(), "the slope limit removed nothing");
        assert!(!gentle.is_empty(), "the flat half must survive");
        for position in &gentle {
            assert!(
                terrain.slope_degrees(position[0], position[1]) <= 20.0,
                "candidate at {position:?} stands on ground too steep for it"
            );
        }
    }

    #[test]
    fn a_wholly_occupied_polygon_is_a_shortfall_not_a_zero_field() {
        // The two cases D33 separates. Both produce no candidates; only one of
        // them is the author's mistake, and the build must tell them apart.
        let (heights, suitability) = ecology_fixture(0..65);
        let terrain = Terrain {
            heights: &heights,
            suitability: &suitability,
            resolution: 65,
            width: 64.0,
            length: 64.0,
        };
        let polygon = [[-8.0, -8.0], [8.0, -8.0], [8.0, 8.0], [-8.0, 8.0]];
        let blanket = Exclusion {
            points: vec![[0.0, 0.0]],
            radius: 64.0,
            filled: false,
        };
        let (occupied, _) = suitable_cells(
            terrain,
            &polygon,
            [-8.0, 8.0, -8.0, 8.0],
            f64::INFINITY,
            std::slice::from_ref(&blanket),
        );
        let (suitable, _) = suitable_cells(
            terrain,
            &polygon,
            [-8.0, 8.0, -8.0, 8.0],
            f64::INFINITY,
            &[],
        );
        assert!(occupied.is_empty(), "everything here is occupied");
        assert!(
            !suitable.is_empty(),
            "...but the ecology itself permits this ground, which is what makes \
             it a shortfall rather than a spec defect"
        );
    }

    #[test]
    fn stable_rng_is_reproducible_and_scoped() {
        let mut first = StableRng::from_parts(7, &["forest", "canopy"]);
        let mut second = StableRng::from_parts(7, &["forest", "canopy"]);
        let mut other = StableRng::from_parts(7, &["forest", "groundcover"]);
        assert_eq!(first.next_u64(), second.next_u64());
        assert_ne!(first.next_u64(), other.next_u64());
    }

    #[test]
    fn polygon_and_segment_geometry_are_bounded() {
        let polygon = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
        assert!(point_in_polygon([0.0, 0.0], &polygon));
        assert!(!point_in_polygon([2.0, 0.0], &polygon));
        assert!((distance_to_segment([0.0, 2.0], [-1.0, 0.0], [1.0, 0.0]) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn protected_relief_excludes_even_flat_interior_shelves() {
        let exclusion = Exclusion {
            points: vec![[-2.0, -2.0], [2.0, -2.0], [2.0, 2.0], [-2.0, 2.0]],
            radius: 0.0,
            filled: true,
        };
        assert!(excluded([0.0, 0.0], &exclusion));
        assert!(!excluded([3.0, 0.0], &exclusion));
    }

    #[test]
    fn measured_grounding_is_role_aware_and_bounded() {
        assert_eq!(grounding_offset("landform_dressing", 220.0, 0.09), 1.584);
        assert_eq!(grounding_offset("landform_dressing", 900.0, 1.0), 2.5);
        assert_eq!(grounding_offset("forest_floor_rock", 0.2, 0.2), 0.05);
        assert_eq!(grounding_offset("conifer_canopy", 40.0, 1.0), 0.35);
        assert_eq!(grounding_offset("lane_crossing_structure", 4.0, 1.0), 0.04);
    }

    #[test]
    fn compiled_route_is_smooth_and_passes_through_authored_points() {
        let authored = [[0.0, 0.0], [4.0, 3.0], [8.0, 0.0]];
        let route = catmull_rom_route(&authored, 8);
        assert_eq!(route.len(), 17);
        assert_eq!(route[0], authored[0]);
        assert_eq!(route[8], authored[1]);
        assert_eq!(route[16], authored[2]);
        assert!(
            route
                .windows(2)
                .all(|pair| distance(pair[0], pair[1]) < 1.0)
        );
    }
}
