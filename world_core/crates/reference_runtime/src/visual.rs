use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::fields::prefixed_sha256;
use crate::world::{cell_position, validate_world_artifact};
use crate::{ReferenceRuntimeError, VISUAL_EVIDENCE_SCHEMA, VISUAL_VALIDATOR_ID, WorldArtifact};

const BACKGROUND: [u8; 3] = [13, 18, 26];
const ROUTE: [u8; 3] = [39, 220, 231];
const PLAYER: [u8; 3] = [72, 239, 106];
const OPPONENT: [u8; 3] = [224, 79, 120];
const OBJECTIVE: [u8; 3] = [255, 183, 60];
const ENCOUNTER: [u8; 3] = [236, 81, 71];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualGateStatus {
    Passed,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VisualMeasurements {
    pub world_coverage_ratio: f64,
    pub terrain_height_range_m: f64,
    pub terrain_luminance_stddev: f64,
    pub distinct_terrain_colors: usize,
    pub route_visible_pixels: usize,
    pub player_spawn_visible_pixels: usize,
    pub opponent_spawn_visible_pixels: usize,
    pub encounter_visible_pixels: usize,
    pub objective_visible_pixels: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VisualEvidenceBody {
    pub schema_version: String,
    pub validator_id: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub capture_sha256: String,
    pub capture_format: String,
    pub camera: crate::ReferenceCamera,
    pub measurements: VisualMeasurements,
    pub status: VisualGateStatus,
    pub failure_reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VisualEvidence {
    pub body: VisualEvidenceBody,
    pub evidence_sha256: String,
}

pub fn render_reference_capture(
    world: &WorldArtifact,
) -> Result<(Vec<u8>, VisualEvidence), ReferenceRuntimeError> {
    validate_world_artifact(world)?;
    let camera = &world.body.authored_layout.reference_camera;
    let width = camera.width_px as usize;
    let height = camera.height_px as usize;
    let vertical_span = camera.orthographic_span_m * height as f64 / width as f64;
    let min_height = world
        .body
        .fields
        .heights_m
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let max_height = world
        .body
        .fields
        .heights_m
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let height_range = max_height - min_height;
    let mut pixels = vec![BACKGROUND; width * height];
    let mut terrain_values = Vec::new();
    let mut terrain_colors = BTreeSet::new();
    for py in 0..height {
        let z = vertical_span * (0.5 - (py as f64 + 0.5) / height as f64);
        for px in 0..width {
            let x = camera.orthographic_span_m * ((px as f64 + 0.5) / width as f64 - 0.5);
            if x.abs() > world.body.authored_layout.width_m / 2.0
                || z.abs() > world.body.authored_layout.length_m / 2.0
            {
                continue;
            }
            let cell = crate::world::cell_for_position(&world.body.authored_layout, [x, z]);
            let elevation = world.body.fields.heights_m[cell];
            let region = world.body.fields.region_codes[cell];
            let color = terrain_color(elevation, min_height, max_height, region);
            pixels[py * width + px] = color;
            terrain_values.push(luminance(color));
            terrain_colors.insert(color);
        }
    }

    let background_pixel_count = pixels.iter().filter(|pixel| **pixel != BACKGROUND).count();
    let coverage = background_pixel_count as f64 / pixels.len() as f64;
    let route_world = world
        .body
        .navigation
        .route_cells
        .iter()
        .map(|cell| cell_position(&world.body.authored_layout, *cell))
        .collect::<Vec<_>>();
    let route_points = route_world
        .iter()
        .map(|point| world_to_pixel(world, *point))
        .collect::<Vec<_>>();
    for pair in route_points.windows(2) {
        draw_line(&mut pixels, width, height, pair[0], pair[1], ROUTE, 2);
    }

    for encounter in &world.body.encounters {
        let center = world_to_pixel(world, encounter.center_xz_m);
        let radius = ((encounter.radius_m / camera.orthographic_span_m) * width as f64)
            .round()
            .max(1.0) as i32;
        draw_ring(&mut pixels, width, height, center, radius, ENCOUNTER);
    }
    for spawn in &world.body.spawns {
        let point = world_to_pixel(world, spawn.position_xz_m);
        let color = match spawn.role {
            crate::SpawnRole::PlayerStart => PLAYER,
            crate::SpawnRole::Opponent => OPPONENT,
        };
        draw_disc(&mut pixels, width, height, point, 3, color);
    }
    let objective_point = world_to_pixel(
        world,
        world.body.authored_layout.traversal.objective_position_xz_m,
    );
    draw_disc(&mut pixels, width, height, objective_point, 4, OBJECTIVE);

    let route_visible_pixels = count_color(&pixels, ROUTE);
    let player_spawn_visible_pixels = count_color(&pixels, PLAYER);
    let opponent_spawn_visible_pixels = count_color(&pixels, OPPONENT);
    let encounter_visible_pixels = count_color(&pixels, ENCOUNTER);
    let objective_visible_pixels = count_color(&pixels, OBJECTIVE);
    let terrain_mean = if terrain_values.is_empty() {
        0.0
    } else {
        terrain_values.iter().sum::<f64>() / terrain_values.len() as f64
    };
    let terrain_variance = if terrain_values.is_empty() {
        0.0
    } else {
        terrain_values
            .iter()
            .map(|value| (value - terrain_mean).powi(2))
            .sum::<f64>()
            / terrain_values.len() as f64
    };
    let measurements = VisualMeasurements {
        world_coverage_ratio: coverage,
        terrain_height_range_m: height_range,
        terrain_luminance_stddev: terrain_variance.sqrt(),
        distinct_terrain_colors: terrain_colors.len(),
        route_visible_pixels,
        player_spawn_visible_pixels,
        opponent_spawn_visible_pixels,
        encounter_visible_pixels,
        objective_visible_pixels,
    };
    let failure_reasons =
        evaluate_visual_gate(&measurements, world.body.navigation.route_cells.len());
    let status = if failure_reasons.is_empty() {
        VisualGateStatus::Passed
    } else {
        VisualGateStatus::Failed
    };

    let mut capture = format!("P6\n{width} {height}\n255\n").into_bytes();
    for pixel in &pixels {
        capture.extend_from_slice(pixel);
    }
    let body = VisualEvidenceBody {
        schema_version: VISUAL_EVIDENCE_SCHEMA.into(),
        validator_id: VISUAL_VALIDATOR_ID.into(),
        world_artifact_id: world.artifact_id.clone(),
        world_artifact_sha256: world.artifact_sha256.clone(),
        capture_sha256: prefixed_sha256(&capture),
        capture_format: "image/x-portable-pixmap; magic=P6".into(),
        camera: camera.clone(),
        measurements,
        status,
        failure_reasons,
    };
    let evidence_sha256 = prefixed_sha256(&serde_json::to_vec(&body).map_err(|error| {
        ReferenceRuntimeError::contract(format!("visual evidence serialization failed: {error}"))
    })?);
    Ok((
        capture,
        VisualEvidence {
            body,
            evidence_sha256,
        },
    ))
}

pub fn validate_visual_evidence(
    world: &WorldArtifact,
    capture: &[u8],
    evidence: &VisualEvidence,
) -> Result<(), ReferenceRuntimeError> {
    let body_sha = prefixed_sha256(&serde_json::to_vec(&evidence.body).map_err(|error| {
        ReferenceRuntimeError::contract(format!("visual evidence serialization failed: {error}"))
    })?);
    if body_sha != evidence.evidence_sha256
        || prefixed_sha256(capture) != evidence.body.capture_sha256
    {
        return Err(ReferenceRuntimeError::provenance(
            "reference capture or visual evidence digest is invalid".into(),
        ));
    }
    let (expected_capture, expected_evidence) = render_reference_capture(world)?;
    if capture != expected_capture || *evidence != expected_evidence {
        return Err(ReferenceRuntimeError::provenance(
            "visual receipt does not match a fresh deterministic reference capture".into(),
        ));
    }
    Ok(())
}

fn evaluate_visual_gate(measurements: &VisualMeasurements, route_len: usize) -> Vec<String> {
    let mut failures = Vec::new();
    if measurements.world_coverage_ratio < 0.45 {
        failures.push("camera does not show enough of the authored world".into());
    }
    if measurements.terrain_height_range_m < 1.0 {
        failures.push("terrain has less than 1 m of visible vertical relief".into());
    }
    if measurements.terrain_luminance_stddev < 3.0 {
        failures.push("terrain capture has insufficient luminance variation".into());
    }
    if measurements.distinct_terrain_colors < 3 {
        failures.push("terrain capture has fewer than three measurable terrain colors".into());
    }
    if measurements.route_visible_pixels < route_len.min(8) {
        failures.push("navigation route is not sufficiently visible in the capture".into());
    }
    if measurements.player_spawn_visible_pixels < 5 {
        failures.push("player spawn marker is missing from the capture".into());
    }
    if measurements.opponent_spawn_visible_pixels < 5 {
        failures.push("opponent spawn marker is missing from the capture".into());
    }
    if measurements.encounter_visible_pixels < 4 {
        failures.push("encounter volume is missing from the capture".into());
    }
    if measurements.objective_visible_pixels < 5 {
        failures.push("objective marker is missing from the capture".into());
    }
    failures
}

fn terrain_color(height: f64, min: f64, max: f64, region_code: u8) -> [u8; 3] {
    let amount = if max - min <= f64::EPSILON {
        0.0
    } else {
        ((height - min) / (max - min)).clamp(0.0, 1.0)
    };
    if region_code != 0 {
        return [37, 79, 111];
    }
    let t = (amount * 5.0).floor() as u8;
    let sub = (amount * 5.0).fract();
    let palette = [
        ([48.0, 77.0, 65.0], [61.0, 101.0, 71.0]),
        ([61.0, 101.0, 71.0], [82.0, 120.0, 78.0]),
        ([82.0, 120.0, 78.0], [125.0, 128.0, 89.0]),
        ([125.0, 128.0, 89.0], [164.0, 151.0, 119.0]),
        ([164.0, 151.0, 119.0], [207.0, 210.0, 211.0]),
        ([207.0, 210.0, 211.0], [231.0, 235.0, 237.0]),
    ];
    let (low, high) = palette[usize::from(t.min(5))];
    [
        (low[0] + (high[0] - low[0]) * sub).round() as u8,
        (low[1] + (high[1] - low[1]) * sub).round() as u8,
        (low[2] + (high[2] - low[2]) * sub).round() as u8,
    ]
}

fn luminance(color: [u8; 3]) -> f64 {
    0.2126 * f64::from(color[0]) + 0.7152 * f64::from(color[1]) + 0.0722 * f64::from(color[2])
}

fn world_to_pixel(world: &WorldArtifact, position: [f64; 2]) -> (i32, i32) {
    let camera = &world.body.authored_layout.reference_camera;
    let width = camera.width_px as f64;
    let height = camera.height_px as f64;
    let vertical_span = camera.orthographic_span_m * height / width;
    let x = ((position[0] / camera.orthographic_span_m + 0.5) * width).round() as i32;
    let y = ((0.5 - position[1] / vertical_span) * height).round() as i32;
    (x, y)
}

fn draw_line(
    pixels: &mut [[u8; 3]],
    width: usize,
    height: usize,
    start: (i32, i32),
    end: (i32, i32),
    color: [u8; 3],
    thickness: i32,
) {
    let (mut x0, mut y0) = start;
    let (x1, y1) = end;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        draw_disc(pixels, width, height, (x0, y0), thickness, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x0 += sx;
        }
        if twice <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

fn draw_ring(
    pixels: &mut [[u8; 3]],
    width: usize,
    height: usize,
    center: (i32, i32),
    radius: i32,
    color: [u8; 3],
) {
    let inner = (radius - 1).max(0).pow(2);
    let outer = (radius + 1).pow(2);
    for y in center.1 - radius - 1..=center.1 + radius + 1 {
        for x in center.0 - radius - 1..=center.0 + radius + 1 {
            let distance = (x - center.0).pow(2) + (y - center.1).pow(2);
            if (inner..=outer).contains(&distance) {
                set_pixel(pixels, width, height, x, y, color);
            }
        }
    }
}

fn draw_disc(
    pixels: &mut [[u8; 3]],
    width: usize,
    height: usize,
    center: (i32, i32),
    radius: i32,
    color: [u8; 3],
) {
    let squared = radius * radius;
    for y in center.1 - radius..=center.1 + radius {
        for x in center.0 - radius..=center.0 + radius {
            if (x - center.0).pow(2) + (y - center.1).pow(2) <= squared {
                set_pixel(pixels, width, height, x, y, color);
            }
        }
    }
}

fn set_pixel(pixels: &mut [[u8; 3]], width: usize, height: usize, x: i32, y: i32, color: [u8; 3]) {
    if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
        pixels[y as usize * width + x as usize] = color;
    }
}

fn count_color(pixels: &[[u8; 3]], color: [u8; 3]) -> usize {
    pixels.iter().filter(|pixel| **pixel == color).count()
}
