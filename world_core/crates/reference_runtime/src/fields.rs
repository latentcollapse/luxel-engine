use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    AuthoredLayout, JULIA_FIELD_REQUEST_SCHEMA, JULIA_FIELD_RESPONSE_SCHEMA, ReferenceRuntimeError,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NumericalFields {
    pub resolution: usize,
    pub heights_m: Vec<f64>,
    pub slope_grade: Vec<f64>,
    pub region_codes: Vec<u8>,
    pub spatial_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct JuliaFieldResponse {
    pub schema_version: String,
    pub request_sha256: String,
    pub world_id: String,
    pub resolution: usize,
    pub width_m: f64,
    pub length_m: f64,
    pub heights_f64_le_hex: String,
    pub heights_sha256: String,
    pub slope_grade_f64_le_hex: String,
    pub slope_grade_sha256: String,
    pub region_codes_hex: String,
    pub region_codes_sha256: String,
    pub spatial_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JuliaFieldProvenance {
    pub schema_version: String,
    pub worker_id: String,
    pub request_sha256: String,
    pub response_sha256: String,
    pub worker_script_sha256: String,
    pub terrain_project_sha256: String,
    pub julia_version: String,
    /// Exact protocol exchange is retained so a later authority validator can
    /// reparse the bytes instead of trusting a producer's status field.
    pub request_json: String,
    pub response_json: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct JuliaFieldRequest {
    pub schema_version: String,
    pub layout_sha256: String,
    pub world_id: String,
    pub width_m: f64,
    pub length_m: f64,
    pub resolution: usize,
    pub seed: u64,
    pub terrain: crate::TerrainIntent,
    pub regions: Vec<crate::SemanticRegion>,
}

impl JuliaFieldRequest {
    pub fn new(layout: &AuthoredLayout, layout_sha256: &str) -> Self {
        Self {
            schema_version: JULIA_FIELD_REQUEST_SCHEMA.to_owned(),
            layout_sha256: layout_sha256.to_owned(),
            world_id: layout.world_id.clone(),
            width_m: layout.width_m,
            length_m: layout.length_m,
            resolution: layout.resolution,
            seed: layout.seed,
            terrain: layout.terrain.clone(),
            regions: layout.regions.clone(),
        }
    }
}

pub fn spatial_fields_sha256(fields: &NumericalFields) -> String {
    let mut digest = Sha256::new();
    digest.update(b"wge.julia-spatial-fields/v1\0");
    digest.update((fields.resolution as u64).to_le_bytes());
    for value in &fields.heights_m {
        digest.update(value.to_le_bytes());
    }
    for value in &fields.slope_grade {
        digest.update(value.to_le_bytes());
    }
    digest.update(&fields.region_codes);
    format!("sha256:{}", hex(&digest.finalize()))
}

/// Independently recompute Julia's authored terrain height function and check
/// the returned height samples against it. Julia remains the producer and
/// numerical authority; this check protects the authority boundary from a
/// response whose arrays and digests were all resealed by its producer.
pub fn validate_height_field(
    layout: &AuthoredLayout,
    heights_m: &[f64],
) -> Result<(), ReferenceRuntimeError> {
    let expected_count = layout
        .resolution
        .checked_mul(layout.resolution)
        .ok_or_else(|| ReferenceRuntimeError::contract("field dimensions overflow".into()))?;
    if heights_m.len() != expected_count {
        return Err(ReferenceRuntimeError::contract(
            "height field shape does not match authored layout".into(),
        ));
    }

    let phase = (layout.seed % 1_000_003) as f64 / 1_000_003.0 * std::f64::consts::TAU;
    let denominator = (layout.resolution - 1) as f64;
    for row in 0..layout.resolution {
        let z = layout.length_m / 2.0 - row as f64 * layout.length_m / denominator;
        for column in 0..layout.resolution {
            let x = -layout.width_m / 2.0 + column as f64 * layout.width_m / denominator;
            let mut expected = layout.terrain.base_elevation_m;
            for feature in &layout.terrain.features {
                let dx = (x - feature.center_xz_m[0]) / feature.radius_x_m;
                let dz = (z - feature.center_xz_m[1]) / feature.radius_z_m;
                expected += feature.elevation_m * (-0.5 * (dx * dx + dz * dz)).exp();
            }
            let mut noise = (x * 0.173 + phase).sin() * (z * 0.137 - phase * 0.37).cos();
            noise += 0.25 * ((x + z) * 0.071 + phase * 0.51).sin();
            expected += layout.terrain.noise_amplitude_m * noise;

            let actual = heights_m[row * layout.resolution + column];
            // Julia and Rust may use different libm implementations. Permit
            // only rounding-scale drift, far below any meaningful terrain
            // change; producer-resealed alterations remain detectable.
            let tolerance = 64.0 * f64::EPSILON * (1.0 + expected.abs());
            if !actual.is_finite() || (actual - expected).abs() > tolerance {
                return Err(ReferenceRuntimeError::provenance(format!(
                    "height field disagrees with authored Julia terrain function at cell {} (expected {expected}, got {actual})",
                    row * layout.resolution + column
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn fields_from_response(
    request: &JuliaFieldRequest,
    request_sha256: &str,
    response: &JuliaFieldResponse,
) -> Result<NumericalFields, ReferenceRuntimeError> {
    if response.schema_version != JULIA_FIELD_RESPONSE_SCHEMA
        || response.request_sha256 != request_sha256
        || response.world_id != request.world_id
        || response.resolution != request.resolution
        || response.width_m != request.width_m
        || response.length_m != request.length_m
    {
        return Err(ReferenceRuntimeError::provenance(
            "Julia field response identity or dimensions do not match the request".into(),
        ));
    }
    let count = request
        .resolution
        .checked_mul(request.resolution)
        .ok_or_else(|| ReferenceRuntimeError::contract("field dimensions overflow".into()))?;
    let heights_m = decode_f64_le_hex(&response.heights_f64_le_hex, count, "height")?;
    let slope_grade = decode_f64_le_hex(&response.slope_grade_f64_le_hex, count, "slope")?;
    let region_codes = decode_hex(&response.region_codes_hex, count, "region")?;
    for (label, actual, expected) in [
        (
            "height",
            f64_array_sha256(&heights_m),
            response.heights_sha256.as_str(),
        ),
        (
            "slope",
            f64_array_sha256(&slope_grade),
            response.slope_grade_sha256.as_str(),
        ),
        (
            "semantic region",
            prefixed_sha256(&region_codes),
            response.region_codes_sha256.as_str(),
        ),
    ] {
        if actual != expected {
            return Err(ReferenceRuntimeError::provenance(format!(
                "Julia {label} field digest differs from returned bytes (worker={expected}, Rust={actual})"
            )));
        }
    }
    if heights_m
        .iter()
        .chain(&slope_grade)
        .any(|value| !value.is_finite())
    {
        return Err(ReferenceRuntimeError::contract(
            "Julia field arrays contain a non-finite value".into(),
        ));
    }
    let fields = NumericalFields {
        resolution: request.resolution,
        heights_m,
        slope_grade,
        region_codes,
        spatial_sha256: response.spatial_sha256.clone(),
    };
    let calculated_sha256 = spatial_fields_sha256(&fields);
    if calculated_sha256 != fields.spatial_sha256 {
        return Err(ReferenceRuntimeError::provenance(format!(
            "Julia spatial field digest does not match returned numerical data (worker={}, Rust={calculated_sha256})",
            fields.spatial_sha256
        )));
    }
    Ok(fields)
}

pub(crate) fn prefixed_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{}", hex(&digest))
}

fn f64_array_sha256(values: &[f64]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * 8);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    prefixed_sha256(&bytes)
}

fn decode_f64_le_hex(
    hex: &str,
    count: usize,
    label: &str,
) -> Result<Vec<f64>, ReferenceRuntimeError> {
    let byte_count = count.checked_mul(8).ok_or_else(|| {
        ReferenceRuntimeError::contract(format!("{label} field byte count overflow"))
    })?;
    let bytes = decode_hex(hex, byte_count, label)?;
    Ok(bytes
        .chunks_exact(8)
        .map(|chunk| f64::from_le_bytes(chunk.try_into().expect("chunk length is fixed")))
        .collect())
}

fn decode_hex(
    hex: &str,
    expected_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, ReferenceRuntimeError> {
    if hex.len() != expected_bytes.saturating_mul(2) || !hex.len().is_multiple_of(2) {
        return Err(ReferenceRuntimeError::contract(format!(
            "Julia {label} field byte length does not match its declared dimensions"
        )));
    }
    let mut bytes = Vec::with_capacity(expected_bytes);
    for pair in hex.as_bytes().chunks_exact(2) {
        let high = nibble(pair[0]).ok_or_else(|| {
            ReferenceRuntimeError::contract(format!(
                "Julia {label} field has invalid hexadecimal data"
            ))
        })?;
        let low = nibble(pair[1]).ok_or_else(|| {
            ReferenceRuntimeError::contract(format!(
                "Julia {label} field has invalid hexadecimal data"
            ))
        })?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(TABLE[(byte >> 4) as usize] as char);
        output.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(crate) fn is_sha256(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::validate_height_field;
    use crate::AuthoredLayout;

    #[test]
    fn independent_height_check_rejects_resealed_terrain_samples() {
        let mut layout: AuthoredLayout =
            serde_json::from_str(include_str!("../examples/riverwatch.layout.json"))
                .expect("authored example layout deserializes");
        layout.terrain.features.clear();
        layout.terrain.noise_amplitude_m = 0.0;
        let expected_heights = vec![layout.terrain.base_elevation_m; layout.resolution.pow(2)];

        validate_height_field(&layout, &expected_heights)
            .expect("authored flat terrain has the expected constant heights");

        let mut producer_resealed_heights = expected_heights;
        producer_resealed_heights[0] += 0.25;
        let error = validate_height_field(&layout, &producer_resealed_heights)
            .expect_err("a changed height must fail even if producer digests were resealed");
        assert!(error.message.contains("authored Julia terrain function"));
    }
}
