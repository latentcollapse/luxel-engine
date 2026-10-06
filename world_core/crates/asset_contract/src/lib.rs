//! Rust-owned identity, structural facts, and bounded acceptance for GLB assets.
//!
//! The supported intake subset validates GLB v2 framing, reads uncompressed
//! POSITION and index accessors, computes deterministic bounds and triangle
//! topology, and extracts translation/scale-only separable part bounds.
//! Material image decoding, full glTF extension semantics, skin influence
//! validation, visual quality, and gameplay-scale inference are intentionally
//! reported as unsupported instead of guessed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const ASSET_CONTRACT_SCHEMA: &str = "luxel.asset-contract/v1";
pub const ASSET_RUNTIME_REQUEST_SCHEMA: &str = "luxel.asset-runtime-request/v1";
pub const ASSET_RUNTIME_RECEIPT_SCHEMA: &str = "luxel.asset-runtime-receipt/v1";
pub mod render;
pub mod runtime;
pub use render::*;
pub use runtime::*;
const JSON_CHUNK: u32 = 0x4e4f_534a;
const BIN_CHUNK: u32 = 0x004e_4942;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetContractError {
    Io(String),
    Container(String),
    Json(String),
    Contract(String),
    Unsupported(String),
}

impl fmt::Display for AssetContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(f, "I/O error: {message}"),
            Self::Container(message) => write!(f, "invalid GLB container: {message}"),
            Self::Json(message) => write!(f, "invalid glTF JSON: {message}"),
            Self::Contract(message) => write!(f, "invalid asset contract: {message}"),
            Self::Unsupported(message) => write!(f, "unsupported GLB feature: {message}"),
        }
    }
}

impl std::error::Error for AssetContractError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetReport {
    pub schema_version: String,
    pub identity: AssetIdentity,
    pub container: ContainerFacts,
    pub gltf: GltfFacts,
    pub facts: AssetFacts,
    pub unsupported_fields: Vec<String>,
    pub canonical_report_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetIdentity {
    /// Content identity; independent of filename, directory, and catalog location.
    pub asset_id: String,
    pub source_sha256: String,
    pub byte_length: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerFacts {
    pub magic: String,
    pub version: u32,
    pub declared_length: u64,
    pub json_byte_length: u64,
    pub bin_byte_length: u64,
    pub extra_chunks: Vec<ExtraChunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraChunk {
    pub chunk_type: String,
    pub byte_length: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GltfFacts {
    pub version: String,
    pub generator: Option<String>,
    pub extensions_used: Vec<String>,
    pub extensions_required: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetFacts {
    pub scene_count: usize,
    pub active_scene: Option<usize>,
    pub node_count: usize,
    pub active_node_count: usize,
    pub mesh_count: usize,
    pub primitive_count: usize,
    pub skin_count: usize,
    pub animation_count: usize,
    pub camera_count: usize,
    pub material_count: usize,
    pub texture_count: usize,
    pub image_count: usize,
    pub total_vertex_count: u64,
    pub total_index_count: u64,
    pub total_triangle_count: u64,
    pub attributes_present: Vec<String>,
    pub attributes_absent_from_all_primitives: Vec<String>,
    pub bounds_local: Option<Bounds3>,
    pub primitives: Vec<PrimitiveFacts>,
    pub parts: Vec<AssetPart>,
    pub rigging: RiggingFacts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bounds3 {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub size: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrimitiveFacts {
    pub mesh_index: usize,
    pub primitive_index: usize,
    pub mode: u32,
    pub material_index: Option<usize>,
    pub vertex_count: u64,
    pub index_count: u64,
    pub triangle_count: u64,
    pub attributes: BTreeMap<String, AttributeFacts>,
    pub bounds_local: Bounds3,
    pub topology: Option<TopologyFacts>,
    pub morph_target_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributeFacts {
    pub accessor_index: usize,
    pub count: u64,
    pub accessor_type: String,
    pub component_type: u32,
    pub normalized: bool,
    /// These bounds are copied from the glTF accessor metadata when present.
    /// POSITION bounds in PrimitiveFacts are instead decoded from source bytes.
    pub declared_min: Option<Vec<f64>>,
    pub declared_max: Option<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopologyFacts {
    pub triangle_count: u64,
    pub degenerate_index_triangles: u64,
    pub zero_area_triangles: u64,
    pub triangle_area_asset_units2_min: f64,
    pub triangle_area_asset_units2_max: f64,
    pub triangle_area_asset_units2_mean: f64,
    pub edge_count: u64,
    pub boundary_edge_count: u64,
    pub manifold_edge_count: u64,
    pub non_manifold_edge_count: u64,
    pub max_edge_uses: u64,
    pub isolated_vertex_count: u64,
    pub connected_component_count: u64,
    pub largest_component_vertices: u64,
    pub component_vertex_counts_top: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetPart {
    pub name: String,
    pub bounds_local: Bounds3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiggingFacts {
    pub has_joint_attributes: bool,
    pub has_weight_attributes: bool,
    pub has_tangent_attributes: bool,
    pub morph_target_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetUse {
    Character,
    StaticMesh,
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceStatus {
    Ready,
    RequiresWork,
    Rejected,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCode {
    AssetUseUnspecified,
    NoRenderableGeometry,
    MissingSkin,
    MissingJointAttributes,
    MissingWeightAttributes,
    MissingAnimation,
    ZeroAreaTriangles,
    NonManifoldEdges,
    OpenBoundaryEdges,
    FragmentedTopology,
    ScaleUnverified,
    BelowRoleMinimumHeight,
    AboveRoleMaximumHeight,
    AboveRoleMaximumFootprint,
    RequiredAffordanceMissing,
    AffordanceClaimMismatch,
    AffordanceClearanceInsufficient,
    AffordanceInteriorInsufficient,
    AffordanceScaleMismatch,
    AffordanceBandUnbounded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceFinding {
    pub code: FindingCode,
    pub observed_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralAcceptance {
    pub asset_use: AssetUse,
    pub status: AcceptanceStatus,
    pub findings: Vec<AcceptanceFinding>,
}

/// Validate a GLB from bytes. No input path or process data enters the report.
pub fn inspect_glb(bytes: &[u8]) -> Result<AssetReport, AssetContractError> {
    let source_sha256 = sha256_hex(bytes);
    let (document, binary, container) = parse_glb(bytes)?;

    let asset = document
        .get("asset")
        .and_then(Value::as_object)
        .ok_or_else(|| AssetContractError::Contract("root.asset must be an object".into()))?;
    let gltf_version = get_string(asset, "version")
        .ok_or_else(|| AssetContractError::Contract("asset.version is required".into()))?;
    if gltf_version != "2.0" {
        return Err(AssetContractError::Unsupported(format!(
            "glTF asset.version {gltf_version:?}; only 2.0 is supported"
        )));
    }
    let generator = get_string(asset, "generator").map(str::to_owned);
    let extensions_used = string_array(&document, "extensionsUsed")?;
    let extensions_required = string_array(&document, "extensionsRequired")?;

    validate_glb_buffers(&document, binary.len())?;
    let meshes = array_or_empty(&document, "meshes")?;
    let nodes = array_or_empty(&document, "nodes")?;
    let primitives = parse_primitives(&document, binary, meshes)?;
    let parts = parse_parts(&document, nodes, meshes, &primitives)?;
    let active_nodes = active_node_indices(&document, nodes)?;
    let attributes: BTreeSet<String> = primitives
        .iter()
        .flat_map(|primitive| primitive.attributes.keys().cloned())
        .collect();
    let absent_common = ["TANGENT", "COLOR_0", "JOINTS_0", "WEIGHTS_0"]
        .into_iter()
        .filter(|semantic| !attributes.contains(*semantic))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let total_vertex_count = primitives.iter().map(|p| p.vertex_count).sum();
    let total_index_count = primitives.iter().map(|p| p.index_count).sum();
    let total_triangle_count = primitives.iter().map(|p| p.triangle_count).sum();
    let bounds_local = union_bounds(primitives.iter().map(|primitive| &primitive.bounds_local));
    let has_joint_attributes = attributes.contains("JOINTS_0");
    let has_weight_attributes = attributes.contains("WEIGHTS_0");
    let has_tangent_attributes = primitives
        .iter()
        .all(|primitive| primitive.attributes.contains_key("TANGENT"));
    let morph_target_count: u64 = primitives
        .iter()
        .map(|primitive| primitive.morph_target_count as u64)
        .sum();

    let facts = AssetFacts {
        scene_count: array_or_empty(&document, "scenes")?.len(),
        active_scene: document
            .get("scene")
            .and_then(Value::as_u64)
            .map(|v| v as usize),
        node_count: nodes.len(),
        active_node_count: active_nodes.len(),
        mesh_count: meshes.len(),
        primitive_count: primitives.len(),
        skin_count: array_or_empty(&document, "skins")?.len(),
        animation_count: array_or_empty(&document, "animations")?.len(),
        camera_count: array_or_empty(&document, "cameras")?.len(),
        material_count: array_or_empty(&document, "materials")?.len(),
        texture_count: array_or_empty(&document, "textures")?.len(),
        image_count: array_or_empty(&document, "images")?.len(),
        total_vertex_count,
        total_index_count,
        total_triangle_count,
        attributes_present: attributes.into_iter().collect(),
        attributes_absent_from_all_primitives: absent_common,
        bounds_local,
        primitives,
        parts,
        rigging: RiggingFacts {
            has_joint_attributes,
            has_weight_attributes,
            has_tangent_attributes,
            morph_target_count,
        },
    };

    let unsupported_fields = unsupported_fields(&document, &facts, &extensions_required);
    let mut report = AssetReport {
        schema_version: ASSET_CONTRACT_SCHEMA.to_owned(),
        identity: AssetIdentity {
            asset_id: format!("asset_sha256_{source_sha256}"),
            source_sha256,
            byte_length: bytes.len() as u64,
        },
        container,
        gltf: GltfFacts {
            version: gltf_version.to_owned(),
            generator,
            extensions_used,
            extensions_required,
        },
        facts,
        unsupported_fields,
        canonical_report_sha256: String::new(),
    };
    let mut canonical_value = serde_json::to_value(&report)
        .map_err(|error| AssetContractError::Json(error.to_string()))?;
    canonical_value
        .as_object_mut()
        .expect("AssetReport serializes as an object")
        .remove("canonical_report_sha256");
    report.canonical_report_sha256 = sha256_hex(canonical_json(&canonical_value).as_bytes());
    Ok(report)
}

/// Read and inspect one GLB file.
pub fn inspect_file(path: impl AsRef<Path>) -> Result<AssetReport, AssetContractError> {
    let bytes = fs::read(path.as_ref())
        .map_err(|error| AssetContractError::Io(format!("{}: {error}", path.as_ref().display())))?;
    inspect_glb(&bytes)
}

pub fn evaluate_structural_acceptance(
    report: &AssetReport,
    asset_use: AssetUse,
) -> StructuralAcceptance {
    let mut findings = Vec::new();
    if report.facts.primitive_count == 0 || report.facts.total_vertex_count == 0 {
        findings.push(finding(FindingCode::NoRenderableGeometry, None));
        return StructuralAcceptance {
            asset_use,
            status: AcceptanceStatus::Rejected,
            findings,
        };
    }

    if asset_use == AssetUse::Unspecified {
        findings.push(finding(FindingCode::AssetUseUnspecified, None));
        return StructuralAcceptance {
            asset_use,
            status: AcceptanceStatus::Indeterminate,
            findings,
        };
    }

    if asset_use == AssetUse::Character {
        if report.facts.skin_count == 0 {
            findings.push(finding(FindingCode::MissingSkin, Some(0)));
        }
        if !report.facts.rigging.has_joint_attributes {
            findings.push(finding(FindingCode::MissingJointAttributes, None));
        }
        if !report.facts.rigging.has_weight_attributes {
            findings.push(finding(FindingCode::MissingWeightAttributes, None));
        }
        if report.facts.animation_count == 0 {
            findings.push(finding(FindingCode::MissingAnimation, Some(0)));
        }
        findings.push(finding(FindingCode::ScaleUnverified, None));
        for primitive in &report.facts.primitives {
            if let Some(topology) = &primitive.topology {
                if topology.zero_area_triangles > 0 {
                    findings.push(finding(
                        FindingCode::ZeroAreaTriangles,
                        Some(topology.zero_area_triangles),
                    ));
                }
                if topology.non_manifold_edge_count > 0 {
                    findings.push(finding(
                        FindingCode::NonManifoldEdges,
                        Some(topology.non_manifold_edge_count),
                    ));
                }
                if topology.boundary_edge_count > 0 {
                    findings.push(finding(
                        FindingCode::OpenBoundaryEdges,
                        Some(topology.boundary_edge_count),
                    ));
                }
                if topology.connected_component_count > 1 {
                    findings.push(finding(
                        FindingCode::FragmentedTopology,
                        Some(topology.connected_component_count),
                    ));
                }
            }
        }
    }

    StructuralAcceptance {
        asset_use,
        status: if findings.is_empty() {
            AcceptanceStatus::Ready
        } else {
            AcceptanceStatus::RequiresWork
        },
        findings,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PhysicalCalibration {
    /// Meters represented by one source coordinate unit.
    pub meters_per_unit: f64,
    /// Axis that represents vertical in the authored asset coordinate system.
    pub vertical_axis: Axis,
    /// Additional uniform scale used when placing this asset.
    pub placed_scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalRole {
    ConiferCanopy,
    HighlandUnderstory,
    HighlandGroundcover,
    ForestFloorRock,
    FactionFortification,
    ObjectiveLandmark,
    LaneGuardian,
    SettlementLandmark,
    LaneCrossingStructure,
    LandformDressing,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalStatus {
    Passed,
    Failed,
    Indeterminate,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalAcceptance {
    pub role: PhysicalRole,
    pub status: PhysicalStatus,
    pub height_m: Option<f64>,
    pub footprint_m: Option<f64>,
    pub findings: Vec<AcceptanceFinding>,
}

/// Apply the role limits from asset_physical_acceptance.py only when scale and
/// vertical axis are explicitly calibrated by the caller.
pub fn evaluate_physical_acceptance(
    report: &AssetReport,
    role: PhysicalRole,
    calibration: Option<PhysicalCalibration>,
) -> Result<PhysicalAcceptance, AssetContractError> {
    let Some((minimum_height, maximum_height, maximum_footprint)) = role_limits(role) else {
        return Ok(PhysicalAcceptance {
            role,
            status: PhysicalStatus::NotApplicable,
            height_m: None,
            footprint_m: None,
            findings: Vec::new(),
        });
    };
    let Some(calibration) = calibration else {
        return Ok(PhysicalAcceptance {
            role,
            status: PhysicalStatus::Indeterminate,
            height_m: None,
            footprint_m: None,
            findings: vec![finding(FindingCode::ScaleUnverified, None)],
        });
    };
    if !calibration.meters_per_unit.is_finite()
        || calibration.meters_per_unit <= 0.0
        || !calibration.placed_scale.is_finite()
        || calibration.placed_scale <= 0.0
    {
        return Err(AssetContractError::Contract(
            "physical calibration values must be finite and positive".into(),
        ));
    }
    let Some(bounds) = &report.facts.bounds_local else {
        return Ok(PhysicalAcceptance {
            role,
            status: PhysicalStatus::Indeterminate,
            height_m: None,
            footprint_m: None,
            findings: vec![finding(FindingCode::NoRenderableGeometry, None)],
        });
    };
    let factor = calibration.meters_per_unit * calibration.placed_scale;
    let size = bounds.size.map(|dimension| dimension * factor);
    let (height, footprint) = match calibration.vertical_axis {
        Axis::X => (size[0], size[1].max(size[2])),
        Axis::Y => (size[1], size[0].max(size[2])),
        Axis::Z => (size[2], size[0].max(size[1])),
    };
    let height = round_places(height, 5);
    let footprint = round_places(footprint, 5);
    let mut findings = Vec::new();
    if let Some(minimum) = minimum_height
        && height < minimum
    {
        findings.push(finding(FindingCode::BelowRoleMinimumHeight, None));
    }
    if height > maximum_height {
        findings.push(finding(FindingCode::AboveRoleMaximumHeight, None));
    }
    if footprint > maximum_footprint {
        findings.push(finding(FindingCode::AboveRoleMaximumFootprint, None));
    }
    Ok(PhysicalAcceptance {
        role,
        status: if findings.is_empty() {
            PhysicalStatus::Passed
        } else {
            PhysicalStatus::Failed
        },
        height_m: Some(height),
        footprint_m: Some(footprint),
        findings,
    })
}

fn role_limits(role: PhysicalRole) -> Option<(Option<f64>, f64, f64)> {
    match role {
        PhysicalRole::ConiferCanopy => Some((Some(3.0), 16.0, 10.0)),
        PhysicalRole::HighlandUnderstory => Some((None, 2.0, 2.0)),
        PhysicalRole::HighlandGroundcover => Some((None, 1.5, 1.5)),
        PhysicalRole::ForestFloorRock => Some((None, 1.0, 1.5)),
        PhysicalRole::FactionFortification => Some((None, 30.0, 42.0)),
        PhysicalRole::ObjectiveLandmark => Some((None, 9.0, 22.0)),
        PhysicalRole::LaneGuardian => Some((Some(8.0), 22.0, 14.0)),
        PhysicalRole::SettlementLandmark => Some((Some(5.0), 10.0, 24.0)),
        PhysicalRole::LaneCrossingStructure => Some((None, 3.0, 6.0)),
        PhysicalRole::LandformDressing => Some((None, 26.0, 20.0)),
        PhysicalRole::Other => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnterableClaim {
    pub threshold_clear_m: f64,
    pub interior_ring_m: f64,
    pub local_bearing_degrees: f64,
    pub threshold_band_m: [f64; 2],
    pub measured_against: Option<AffordanceCalibration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AffordanceCalibration {
    pub agent_radius_m: f64,
    pub agent_max_climb_m: f64,
    pub placed_scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AffordanceContext {
    pub agent_radius_m: f64,
    pub max_climb_m: f64,
    pub meters_per_unit: f64,
    pub placed_scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AffordanceStatus {
    Passed,
    Failed,
    NotRequired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AffordanceAcceptance {
    pub status: AffordanceStatus,
    pub measured_threshold_m: Option<f64>,
    pub required_clearance_m: Option<f64>,
    pub findings: Vec<AcceptanceFinding>,
}

/// Re-measure a declared entry corridor against the extracted part bounds.
/// Coordinates are scaled by meters_per_unit and placed_scale before testing.
pub fn evaluate_enterable_affordance(
    parts: &[AssetPart],
    role: PhysicalRole,
    claim: Option<&EnterableClaim>,
    context: AffordanceContext,
) -> Result<AffordanceAcceptance, AssetContractError> {
    if role != PhysicalRole::FactionFortification {
        return Ok(AffordanceAcceptance {
            status: AffordanceStatus::NotRequired,
            measured_threshold_m: None,
            required_clearance_m: None,
            findings: Vec::new(),
        });
    }
    let Some(claim) = claim else {
        return Ok(AffordanceAcceptance {
            status: AffordanceStatus::Failed,
            measured_threshold_m: None,
            required_clearance_m: Some(2.0 * context.agent_radius_m),
            findings: vec![finding(FindingCode::RequiredAffordanceMissing, None)],
        });
    };
    validate_affordance_values(claim, context)?;
    let measured = measure_threshold(parts, claim, context)?;
    let needed = 2.0 * context.agent_radius_m;
    let mut findings = Vec::new();
    if measured + 0.25 < claim.threshold_clear_m {
        findings.push(finding(FindingCode::AffordanceClaimMismatch, None));
    }
    if measured <= needed {
        findings.push(finding(FindingCode::AffordanceClearanceInsufficient, None));
    }
    if claim.interior_ring_m < needed {
        findings.push(finding(FindingCode::AffordanceInteriorInsufficient, None));
    }
    if let Some(calibrated) = claim.measured_against
        && calibrated.placed_scale > 0.0
        && (calibrated.placed_scale - context.placed_scale).abs() > 1e-6
    {
        findings.push(finding(FindingCode::AffordanceScaleMismatch, None));
    }
    Ok(AffordanceAcceptance {
        status: if findings.is_empty() {
            AffordanceStatus::Passed
        } else {
            AffordanceStatus::Failed
        },
        measured_threshold_m: Some(round_places(measured, 5)),
        required_clearance_m: Some(round_places(needed, 5)),
        findings,
    })
}

fn validate_affordance_values(
    claim: &EnterableClaim,
    context: AffordanceContext,
) -> Result<(), AssetContractError> {
    let values = [
        claim.threshold_clear_m,
        claim.interior_ring_m,
        claim.local_bearing_degrees,
        claim.threshold_band_m[0],
        claim.threshold_band_m[1],
        context.agent_radius_m,
        context.max_climb_m,
        context.meters_per_unit,
        context.placed_scale,
    ];
    if values.iter().any(|value| !value.is_finite())
        || claim.threshold_clear_m < 0.0
        || claim.interior_ring_m < 0.0
        || context.agent_radius_m <= 0.0
        || context.max_climb_m < 0.0
        || context.meters_per_unit <= 0.0
        || context.placed_scale <= 0.0
    {
        return Err(AssetContractError::Contract(
            "affordance dimensions and calibration must be finite and valid".into(),
        ));
    }
    Ok(())
}

fn measure_threshold(
    parts: &[AssetPart],
    claim: &EnterableClaim,
    context: AffordanceContext,
) -> Result<f64, AssetContractError> {
    let factor = context.meters_per_unit * context.placed_scale;
    let angle = claim.local_bearing_degrees.to_radians();
    let (dx, dz) = (angle.sin(), angle.cos());
    let (lx, lz) = (angle.cos(), -angle.sin());
    let near = claim.threshold_band_m[0].min(claim.threshold_band_m[1]);
    let far = claim.threshold_band_m[0].max(claim.threshold_band_m[1]);
    let (mut left, mut right) = (f64::NEG_INFINITY, f64::INFINITY);
    for part in parts {
        let min = part.bounds_local.min;
        let max = part.bounds_local.max;
        if max[1] * factor <= context.max_climb_m {
            continue;
        }
        let center_x = (min[0] + max[0]) * 0.5 * factor;
        let center_z = (min[2] + max[2]) * 0.5 * factor;
        let half_x = (max[0] - min[0]).abs() * 0.5 * factor;
        let half_z = (max[2] - min[2]).abs() * 0.5 * factor;
        let depth_center = center_x * dx + center_z * dz;
        let depth_reach = half_x * dx.abs() + half_z * dz.abs();
        if depth_center + depth_reach < near || depth_center - depth_reach > far {
            continue;
        }
        let lateral_center = center_x * lx + center_z * lz;
        let lateral_reach = half_x * lx.abs() + half_z * lz.abs();
        let lateral_low = lateral_center - lateral_reach;
        let lateral_high = lateral_center + lateral_reach;
        if lateral_low <= 0.0 && lateral_high >= 0.0 {
            return Ok(0.0);
        }
        if lateral_low > 0.0 {
            right = right.min(lateral_low);
        } else {
            left = left.max(lateral_high);
        }
    }
    if !left.is_finite() || !right.is_finite() {
        return Err(AssetContractError::Contract(format!(
            "no geometry bounds the declared threshold band {near:.2}..{far:.2} m"
        )));
    }
    Ok(right - left)
}

fn finding(code: FindingCode, observed_count: Option<u64>) -> AcceptanceFinding {
    AcceptanceFinding {
        code,
        observed_count,
    }
}

fn parse_glb(bytes: &[u8]) -> Result<(Value, &[u8], ContainerFacts), AssetContractError> {
    if bytes.len() < 20 {
        return Err(AssetContractError::Container(
            "shorter than the mandatory header and JSON chunk".into(),
        ));
    }
    if &bytes[0..4] != b"glTF" {
        return Err(AssetContractError::Container("magic must be glTF".into()));
    }
    let version = read_u32(bytes, 4)?;
    if version != 2 {
        return Err(AssetContractError::Unsupported(format!(
            "GLB container version {version}; only version 2 is supported"
        )));
    }
    let declared_length = read_u32(bytes, 8)? as usize;
    if declared_length != bytes.len() {
        return Err(AssetContractError::Container(format!(
            "declared length {declared_length} differs from actual length {}",
            bytes.len()
        )));
    }
    let mut offset = 12usize;
    let mut json_chunk: Option<&[u8]> = None;
    let mut binary_chunk: Option<&[u8]> = None;
    let mut extra_chunks = Vec::new();
    let mut first = true;
    while offset < bytes.len() {
        if offset + 8 > bytes.len() {
            return Err(AssetContractError::Container(
                "file ends inside a chunk header".into(),
            ));
        }
        let chunk_length = read_u32(bytes, offset)? as usize;
        let chunk_type = read_u32(bytes, offset + 4)?;
        let start = offset + 8;
        let end = start.checked_add(chunk_length).ok_or_else(|| {
            AssetContractError::Container("chunk length overflows address space".into())
        })?;
        if end > bytes.len() {
            return Err(AssetContractError::Container(format!(
                "chunk at byte {offset} extends beyond the file"
            )));
        }
        if !chunk_length.is_multiple_of(4) {
            return Err(AssetContractError::Container(format!(
                "chunk at byte {offset} is not four-byte aligned"
            )));
        }
        let chunk = &bytes[start..end];
        match chunk_type {
            JSON_CHUNK => {
                if json_chunk.is_some() {
                    return Err(AssetContractError::Container(
                        "more than one JSON chunk".into(),
                    ));
                }
                if !first {
                    return Err(AssetContractError::Container(
                        "JSON chunk must be the first chunk".into(),
                    ));
                }
                json_chunk = Some(chunk);
            }
            BIN_CHUNK => {
                if binary_chunk.is_some() {
                    return Err(AssetContractError::Container(
                        "more than one BIN chunk".into(),
                    ));
                }
                binary_chunk = Some(chunk);
            }
            other => {
                let kind = other.to_le_bytes();
                extra_chunks.push(ExtraChunk {
                    chunk_type: String::from_utf8_lossy(&kind)
                        .trim_end_matches('\0')
                        .to_owned(),
                    byte_length: chunk_length as u64,
                });
            }
        }
        first = false;
        offset = end;
    }
    let json_bytes = json_chunk
        .ok_or_else(|| AssetContractError::Container("JSON chunk is missing".into()))?
        .trim_ascii_end_matches(&[0, b' ', b'\t', b'\r', b'\n'][..]);
    let document: Value = serde_json::from_slice(json_bytes)
        .map_err(|error| AssetContractError::Json(error.to_string()))?;
    if !document.is_object() {
        return Err(AssetContractError::Json(
            "document root must be an object".into(),
        ));
    }
    let binary = binary_chunk.unwrap_or(&[]);
    let facts = ContainerFacts {
        magic: "glTF".into(),
        version,
        declared_length: declared_length as u64,
        json_byte_length: json_bytes.len() as u64,
        bin_byte_length: binary.len() as u64,
        extra_chunks,
    };
    Ok((document, binary, facts))
}

trait TrimAsciiEndMatches {
    fn trim_ascii_end_matches(&self, bytes: &[u8]) -> &[u8];
}

impl TrimAsciiEndMatches for [u8] {
    fn trim_ascii_end_matches(&self, bytes: &[u8]) -> &[u8] {
        let mut end = self.len();
        while end > 0 && bytes.contains(&self[end - 1]) {
            end -= 1;
        }
        &self[..end]
    }
}

fn validate_glb_buffers(document: &Value, binary_length: usize) -> Result<(), AssetContractError> {
    let Some(buffers) = document.get("buffers").and_then(Value::as_array) else {
        return Ok(());
    };
    if buffers.len() > 1 {
        return Err(AssetContractError::Unsupported(
            "more than one glTF buffer".into(),
        ));
    }
    if let Some(buffer) = buffers.first() {
        if buffer.get("uri").is_some() {
            return Err(AssetContractError::Unsupported(
                "external buffer URI in GLB".into(),
            ));
        }
        let declared = buffer
            .get("byteLength")
            .and_then(Value::as_u64)
            .ok_or_else(|| AssetContractError::Contract("buffer.byteLength is required".into()))?
            as usize;
        if declared > binary_length || binary_length - declared > 3 {
            return Err(AssetContractError::Container(format!(
                "buffer.byteLength {declared} is inconsistent with BIN chunk length {binary_length}"
            )));
        }
    } else if binary_length > 0 {
        return Err(AssetContractError::Contract(
            "BIN chunk exists without a glTF buffer".into(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct AccessorInfo<'a> {
    value: &'a Value,
    count: usize,
    accessor_type: &'a str,
    component_type: u32,
    normalized: bool,
    byte_offset: usize,
    byte_stride: usize,
    component_size: usize,
    component_count: usize,
}

fn accessor_info<'a>(
    document: &'a Value,
    binary: &[u8],
    accessor_index: usize,
) -> Result<AccessorInfo<'a>, AssetContractError> {
    let accessors = array_or_empty(document, "accessors")?;
    let accessor = accessors.get(accessor_index).ok_or_else(|| {
        AssetContractError::Contract(format!("accessor {accessor_index} is out of range"))
    })?;
    if accessor.get("sparse").is_some() {
        return Err(AssetContractError::Unsupported(format!(
            "sparse accessor {accessor_index}"
        )));
    }
    let view_index = accessor
        .get("bufferView")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AssetContractError::Unsupported(format!("accessor {accessor_index} has no bufferView"))
        })? as usize;
    let views = array_or_empty(document, "bufferViews")?;
    let view = views.get(view_index).ok_or_else(|| {
        AssetContractError::Contract(format!(
            "accessor {accessor_index} references missing bufferView {view_index}"
        ))
    })?;
    if view.get("buffer").and_then(Value::as_u64).unwrap_or(0) != 0 {
        return Err(AssetContractError::Unsupported(format!(
            "bufferView {view_index} references a non-GLB buffer"
        )));
    }
    let accessor_type = get_string(
        accessor.as_object().ok_or_else(|| {
            AssetContractError::Contract(format!("accessor {accessor_index} must be an object"))
        })?,
        "type",
    )
    .ok_or_else(|| {
        AssetContractError::Contract(format!("accessor {accessor_index}.type missing"))
    })?;
    let component_count = match accessor_type {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        other => {
            return Err(AssetContractError::Unsupported(format!(
                "accessor type {other:?}"
            )));
        }
    };
    let component_type = accessor
        .get("componentType")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AssetContractError::Contract(format!("accessor {accessor_index}.componentType missing"))
        })? as u32;
    let component_size = match component_type {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        5125 | 5126 => 4,
        other => {
            return Err(AssetContractError::Unsupported(format!(
                "componentType {other} in accessor {accessor_index}"
            )));
        }
    };
    let count = accessor
        .get("count")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AssetContractError::Contract(format!("accessor {accessor_index}.count missing"))
        })? as usize;
    let view_offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let accessor_offset = accessor
        .get("byteOffset")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let byte_offset = view_offset.checked_add(accessor_offset).ok_or_else(|| {
        AssetContractError::Contract(format!("accessor {accessor_index} offset overflow"))
    })?;
    let item_size = component_size * component_count;
    let byte_stride = view
        .get("byteStride")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(item_size);
    if byte_stride < item_size {
        return Err(AssetContractError::Contract(format!(
            "bufferView {view_index} stride is smaller than accessor item size"
        )));
    }
    let view_length = view
        .get("byteLength")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AssetContractError::Contract(format!("bufferView {view_index}.byteLength missing"))
        })? as usize;
    let view_end = view_offset.checked_add(view_length).ok_or_else(|| {
        AssetContractError::Contract(format!("bufferView {view_index} range overflow"))
    })?;
    let last_item = count
        .saturating_sub(1)
        .checked_mul(byte_stride)
        .and_then(|value| byte_offset.checked_add(value))
        .and_then(|value| value.checked_add(item_size))
        .ok_or_else(|| AssetContractError::Contract("accessor byte range overflow".into()))?;
    if view_end > binary.len() || last_item > view_end || last_item > binary.len() {
        return Err(AssetContractError::Contract(format!(
            "accessor {accessor_index} exceeds its bufferView or BIN chunk"
        )));
    }
    Ok(AccessorInfo {
        value: accessor,
        count,
        accessor_type,
        component_type,
        normalized: accessor
            .get("normalized")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        byte_offset,
        byte_stride,
        component_size,
        component_count,
    })
}

fn parse_primitives(
    document: &Value,
    binary: &[u8],
    meshes: &[Value],
) -> Result<Vec<PrimitiveFacts>, AssetContractError> {
    let mut result = Vec::new();
    for (mesh_index, mesh) in meshes.iter().enumerate() {
        let primitives = mesh
            .get("primitives")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AssetContractError::Contract(format!(
                    "mesh {mesh_index}.primitives must be an array"
                ))
            })?;
        for (primitive_index, primitive) in primitives.iter().enumerate() {
            let attributes = primitive
                .get("attributes")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    AssetContractError::Contract(format!(
                        "mesh {mesh_index} primitive {primitive_index} has no attributes object"
                    ))
                })?;
            let position_index = attributes
                .get("POSITION")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    AssetContractError::Contract(format!(
                        "mesh {mesh_index} primitive {primitive_index} has no POSITION attribute"
                    ))
                })? as usize;
            let position_info = accessor_info(document, binary, position_index)?;
            if position_info.accessor_type != "VEC3" || position_info.component_type != 5126 {
                return Err(AssetContractError::Unsupported(format!(
                    "POSITION accessor {position_index} must be unnormalized FLOAT VEC3"
                )));
            }
            let positions = read_positions(binary, &position_info, position_index)?;
            let bounds_local = bounds_from_positions(&positions)?;
            let mut attribute_facts = BTreeMap::new();
            for (semantic, index_value) in attributes {
                let accessor_index = index_value.as_u64().ok_or_else(|| {
                    AssetContractError::Contract(format!(
                        "attribute {semantic} accessor index is not an integer"
                    ))
                })? as usize;
                let info = accessor_info(document, binary, accessor_index)?;
                attribute_facts.insert(
                    semantic.clone(),
                    AttributeFacts {
                        accessor_index,
                        count: info.count as u64,
                        accessor_type: info.accessor_type.to_owned(),
                        component_type: info.component_type,
                        normalized: info.normalized,
                        declared_min: number_array(info.value.get("min"), "accessor.min")?,
                        declared_max: number_array(info.value.get("max"), "accessor.max")?,
                    },
                );
            }
            let mode = primitive.get("mode").and_then(Value::as_u64).unwrap_or(4) as u32;
            let indices = if let Some(index_value) = primitive.get("indices") {
                let accessor_index = index_value.as_u64().ok_or_else(|| {
                    AssetContractError::Contract(
                        "primitive indices must be an accessor index".into(),
                    )
                })? as usize;
                let info = accessor_info(document, binary, accessor_index)?;
                read_indices(binary, &info, accessor_index)?
            } else {
                (0..positions.len())
                    .map(|index| index as u32)
                    .collect::<Vec<_>>()
            };
            if mode == 4 && indices.len() % 3 != 0 {
                return Err(AssetContractError::Contract(format!(
                    "triangle primitive {mesh_index}:{primitive_index} index count is not divisible by three"
                )));
            }
            if indices
                .iter()
                .any(|index| *index as usize >= positions.len())
            {
                return Err(AssetContractError::Contract(format!(
                    "primitive {mesh_index}:{primitive_index} references a vertex outside POSITION"
                )));
            }
            let topology = if mode == 4 {
                Some(topology_facts(&positions, &indices)?)
            } else {
                None
            };
            let triangle_count = if mode == 4 {
                (indices.len() / 3) as u64
            } else {
                0
            };
            result.push(PrimitiveFacts {
                mesh_index,
                primitive_index,
                mode,
                material_index: primitive
                    .get("material")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize),
                vertex_count: positions.len() as u64,
                index_count: indices.len() as u64,
                triangle_count,
                attributes: attribute_facts,
                bounds_local,
                topology,
                morph_target_count: primitive
                    .get("targets")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
            });
        }
    }
    Ok(result)
}

fn read_positions(
    binary: &[u8],
    info: &AccessorInfo<'_>,
    accessor_index: usize,
) -> Result<Vec<[f64; 3]>, AssetContractError> {
    if info.normalized || info.component_count != 3 || info.component_size != 4 {
        return Err(AssetContractError::Unsupported(format!(
            "POSITION accessor {accessor_index} must contain unnormalized FLOAT VEC3 values"
        )));
    }
    let mut result = Vec::with_capacity(info.count);
    for index in 0..info.count {
        let base = info.byte_offset + index * info.byte_stride;
        let mut point = [0.0; 3];
        for (component, value) in point.iter_mut().enumerate() {
            let offset = base + component * 4;
            let bytes: [u8; 4] = binary[offset..offset + 4]
                .try_into()
                .expect("validated accessor range");
            let decoded = f32::from_le_bytes(bytes) as f64;
            if !decoded.is_finite() {
                return Err(AssetContractError::Contract(format!(
                    "POSITION accessor {accessor_index} contains a non-finite value"
                )));
            }
            *value = decoded;
        }
        result.push(point);
    }
    Ok(result)
}

fn read_indices(
    binary: &[u8],
    info: &AccessorInfo<'_>,
    accessor_index: usize,
) -> Result<Vec<u32>, AssetContractError> {
    if info.accessor_type != "SCALAR" || info.normalized {
        return Err(AssetContractError::Unsupported(format!(
            "indices accessor {accessor_index} must be unnormalized SCALAR"
        )));
    }
    if !matches!(info.component_type, 5121 | 5123 | 5125) {
        return Err(AssetContractError::Unsupported(format!(
            "indices accessor {accessor_index} must use unsigned byte, short, or int"
        )));
    }
    let mut result = Vec::with_capacity(info.count);
    for index in 0..info.count {
        let offset = info.byte_offset + index * info.byte_stride;
        let value = match info.component_type {
            5121 => binary[offset] as u32,
            5123 => u16::from_le_bytes([binary[offset], binary[offset + 1]]) as u32,
            5125 => u32::from_le_bytes([
                binary[offset],
                binary[offset + 1],
                binary[offset + 2],
                binary[offset + 3],
            ]),
            _ => unreachable!(),
        };
        result.push(value);
    }
    Ok(result)
}

fn bounds_from_positions(positions: &[[f64; 3]]) -> Result<Bounds3, AssetContractError> {
    let first = positions.first().ok_or_else(|| {
        AssetContractError::Contract("POSITION accessor must contain at least one vertex".into())
    })?;
    let mut min = *first;
    let mut max = *first;
    for point in positions.iter().skip(1) {
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    Ok(bounds(min, max))
}

fn topology_facts(
    positions: &[[f64; 3]],
    indices: &[u32],
) -> Result<TopologyFacts, AssetContractError> {
    let triangle_count = indices.len() / 3;
    let mut degenerate = 0u64;
    let mut zero_area = 0u64;
    let mut areas = Vec::with_capacity(triangle_count);
    let mut edge_uses: HashMap<(u32, u32), u64> = HashMap::with_capacity(triangle_count * 2);
    let mut union = UnionFind::new(positions.len());
    let mut referenced = vec![false; positions.len()];

    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
        if a == b || b == c || a == c {
            degenerate += 1;
        }
        let p0 = positions[a as usize];
        let p1 = positions[b as usize];
        let p2 = positions[c as usize];
        let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let v = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let area = 0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        if area <= 1e-12 {
            zero_area += 1;
        }
        areas.push(area);
        for index in [a, b, c] {
            referenced[index as usize] = true;
        }
        for (left, right) in [(a, b), (b, c), (c, a)] {
            let edge = if left <= right {
                (left, right)
            } else {
                (right, left)
            };
            *edge_uses.entry(edge).or_default() += 1;
            union.join(left as usize, right as usize);
        }
    }
    let edge_count = edge_uses.len() as u64;
    let boundary_edge_count = edge_uses.values().filter(|uses| **uses == 1).count() as u64;
    let manifold_edge_count = edge_uses.values().filter(|uses| **uses == 2).count() as u64;
    let non_manifold_edge_count = edge_uses.values().filter(|uses| **uses > 2).count() as u64;
    let max_edge_uses = edge_uses.values().copied().max().unwrap_or(0);
    let mut component_sizes: HashMap<usize, u64> = HashMap::new();
    for vertex in 0..positions.len() {
        let root = union.find(vertex);
        *component_sizes.entry(root).or_default() += 1;
    }
    let mut component_vertex_counts_top = component_sizes.values().copied().collect::<Vec<_>>();
    component_vertex_counts_top.sort_unstable_by(|left, right| right.cmp(left));
    component_vertex_counts_top.truncate(10);
    let largest_component_vertices = component_vertex_counts_top.first().copied().unwrap_or(0);
    let (area_min, area_max, area_mean) = if areas.is_empty() {
        (0.0, 0.0, 0.0)
    } else {
        let minimum = areas.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = areas.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mean = areas.iter().sum::<f64>() / areas.len() as f64;
        (minimum, maximum, mean)
    };
    Ok(TopologyFacts {
        triangle_count: triangle_count as u64,
        degenerate_index_triangles: degenerate,
        zero_area_triangles: zero_area,
        triangle_area_asset_units2_min: round_places(area_min, 9),
        triangle_area_asset_units2_max: round_places(area_max, 9),
        triangle_area_asset_units2_mean: round_places(area_mean, 9),
        edge_count,
        boundary_edge_count,
        manifold_edge_count,
        non_manifold_edge_count,
        max_edge_uses,
        isolated_vertex_count: referenced.iter().filter(|used| !**used).count() as u64,
        connected_component_count: component_sizes.len() as u64,
        largest_component_vertices,
        component_vertex_counts_top,
    })
}

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..count).collect(),
            rank: vec![0; count],
        }
    }

    fn find(&mut self, value: usize) -> usize {
        if self.parent[value] != value {
            let root = self.find(self.parent[value]);
            self.parent[value] = root;
        }
        self.parent[value]
    }

    fn join(&mut self, left: usize, right: usize) {
        let mut a = self.find(left);
        let mut b = self.find(right);
        if a == b {
            return;
        }
        if self.rank[a] < self.rank[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parent[b] = a;
        if self.rank[a] == self.rank[b] {
            self.rank[a] += 1;
        }
    }
}

fn parse_parts(
    document: &Value,
    nodes: &[Value],
    meshes: &[Value],
    primitives: &[PrimitiveFacts],
) -> Result<Vec<AssetPart>, AssetContractError> {
    let mut grouped: BTreeMap<String, Bounds3> = BTreeMap::new();
    for (node_index, node) in nodes.iter().enumerate() {
        let Some(mesh_index) = node.get("mesh").and_then(Value::as_u64).map(|v| v as usize) else {
            continue;
        };
        let Some(mesh) = meshes.get(mesh_index) else {
            return Err(AssetContractError::Contract(format!(
                "node {node_index} references missing mesh {mesh_index}"
            )));
        };
        if node.get("matrix").is_some() || non_identity_rotation(node)? {
            return Err(AssetContractError::Unsupported(format!(
                "part bounds for node {node_index} use a matrix or rotation; this slice only supports translation and scale"
            )));
        }
        let translation = vec3(node.get("translation"), [0.0; 3], "node.translation")?;
        let scale = vec3(node.get("scale"), [1.0; 3], "node.scale")?;
        let mesh_primitives = mesh
            .get("primitives")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AssetContractError::Contract(format!(
                    "mesh {mesh_index}.primitives must be an array"
                ))
            })?;
        for primitive_index in 0..mesh_primitives.len() {
            let primitive = primitives
                .iter()
                .find(|entry| {
                    entry.mesh_index == mesh_index && entry.primitive_index == primitive_index
                })
                .ok_or_else(|| {
                    AssetContractError::Contract(format!(
                        "missing facts for mesh {mesh_index} primitive {primitive_index}"
                    ))
                })?;
            let mut min = [0.0; 3];
            let mut max = [0.0; 3];
            for axis in 0..3 {
                let first = primitive.bounds_local.min[axis] * scale[axis] + translation[axis];
                let second = primitive.bounds_local.max[axis] * scale[axis] + translation[axis];
                min[axis] = first.min(second);
                max[axis] = first.max(second);
            }
            let part_name = node
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("node_{node_index}"));
            let group = part_group(&part_name);
            let part_bounds = bounds(min, max);
            grouped
                .entry(group)
                .and_modify(|existing| *existing = union_two_bounds(existing, &part_bounds))
                .or_insert(part_bounds);
        }
    }
    let _ = document;
    Ok(grouped
        .into_iter()
        .map(|(name, bounds_local)| AssetPart { name, bounds_local })
        .collect())
}

fn non_identity_rotation(node: &Value) -> Result<bool, AssetContractError> {
    let Some(rotation) = node.get("rotation") else {
        return Ok(false);
    };
    let values = rotation
        .as_array()
        .ok_or_else(|| AssetContractError::Contract("node.rotation must be an array".into()))?;
    if values.len() != 4 {
        return Err(AssetContractError::Contract(
            "node.rotation must contain four values".into(),
        ));
    }
    let q = values
        .iter()
        .map(|value| {
            value.as_f64().ok_or_else(|| {
                AssetContractError::Contract("node.rotation values must be numeric".into())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let norm = q.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm == 0.0 {
        return Err(AssetContractError::Contract(
            "node.rotation quaternion has zero length".into(),
        ));
    }
    Ok((q[0].abs() > 1e-12)
        || (q[1].abs() > 1e-12)
        || (q[2].abs() > 1e-12)
        || ((q[3].abs() / norm) - 1.0).abs() > 1e-12)
}

fn part_group(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let suffixes = [
        "slate_roof",
        "thatch_roof",
        "chimney",
        "door",
        "foundation",
        "plaster",
        "roof",
        "wall",
        "stone",
        "window",
        "merlon",
        "timber",
    ];
    for suffix in suffixes {
        let mut matched = lower.ends_with(&format!("_{suffix}"));
        if !matched {
            let prefix = format!("_{suffix}_");
            if let Some(index) = lower.rfind(&prefix) {
                let trailing = &lower[index + prefix.len()..];
                matched = !trailing.is_empty()
                    && trailing
                        .strip_prefix('-')
                        .unwrap_or(trailing)
                        .chars()
                        .all(|character| character.is_ascii_digit());
            }
        }
        if matched && let Some((prefix, _)) = name.rsplit_once('_') {
            if prefix.to_ascii_lowercase().ends_with(suffix)
                && let Some((base, _)) = prefix.rsplit_once('_')
            {
                return base.trim_end_matches('_').to_owned();
            }
            return prefix.trim_end_matches('_').to_owned();
        }
    }
    name.trim_end_matches('_').to_owned()
}

fn active_node_indices(
    document: &Value,
    nodes: &[Value],
) -> Result<BTreeSet<usize>, AssetContractError> {
    let scenes = array_or_empty(document, "scenes")?;
    let selected_scene = if scenes.is_empty() {
        None
    } else {
        Some(document.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize)
    };
    let roots: Vec<usize> = if let Some(scene_index) = selected_scene {
        let scene = scenes.get(scene_index).ok_or_else(|| {
            AssetContractError::Contract(format!("active scene {scene_index} is out of range"))
        })?;
        scene
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| AssetContractError::Contract("scene.nodes must be an array".into()))?
            .iter()
            .map(|value| {
                value.as_u64().map(|index| index as usize).ok_or_else(|| {
                    AssetContractError::Contract("scene root node indices must be integers".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        (0..nodes.len()).collect()
    };
    let mut active = BTreeSet::new();
    let mut stack = roots;
    while let Some(index) = stack.pop() {
        if index >= nodes.len() {
            return Err(AssetContractError::Contract(format!(
                "scene graph references missing node {index}"
            )));
        }
        if !active.insert(index) {
            continue;
        }
        if let Some(children) = nodes[index].get("children").and_then(Value::as_array) {
            for child in children {
                let child = child.as_u64().ok_or_else(|| {
                    AssetContractError::Contract(format!(
                        "node {index} child indices must be integers"
                    ))
                })? as usize;
                stack.push(child);
            }
        }
    }
    Ok(active)
}

fn unsupported_fields(
    document: &Value,
    facts: &AssetFacts,
    extensions_required: &[String],
) -> Vec<String> {
    let mut unsupported = BTreeSet::from([
        "gameplay_scale_calibration".to_owned(),
        "semantic_asset_classification".to_owned(),
        "collision_lod_socket_policy".to_owned(),
        "pbr_material_binding_validation".to_owned(),
        "embedded_image_decode_and_alpha_analysis".to_owned(),
        "skin_hierarchy_and_weight_validation".to_owned(),
        "visual_quality_assessment".to_owned(),
    ]);
    if facts.attributes_present.iter().any(|name| name == "NORMAL") {
        unsupported.insert("normal_length_and_winding_quality".into());
    }
    if facts
        .attributes_present
        .iter()
        .any(|name| name.starts_with("TEXCOORD_"))
    {
        unsupported.insert("uv_range_and_seam_quality".into());
    }
    if facts.rigging.morph_target_count > 0 {
        unsupported.insert("morph_target_delta_validation".into());
    }
    if !extensions_required.is_empty() {
        for extension in extensions_required {
            unsupported.insert(format!("required_extension_semantics:{extension}"));
        }
    }
    if document
        .get("skins")
        .and_then(Value::as_array)
        .is_some_and(|skins| !skins.is_empty())
    {
        unsupported.insert("skin_inverse_bind_and_joint_reference_validation".into());
    }
    unsupported.into_iter().collect()
}

fn string_array(document: &Value, key: &str) -> Result<Vec<String>, AssetContractError> {
    let Some(value) = document.get(key) else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| AssetContractError::Contract(format!("root.{key} must be an array")))?;
    let mut result = array
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                AssetContractError::Contract(format!("root.{key} entries must be strings"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    result.sort();
    result.dedup();
    Ok(result)
}

fn array_or_empty<'a>(document: &'a Value, key: &str) -> Result<&'a [Value], AssetContractError> {
    match document.get(key) {
        None => Ok(&[]),
        Some(value) => value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| AssetContractError::Contract(format!("root.{key} must be an array"))),
    }
}

fn number_array(value: Option<&Value>, path: &str) -> Result<Option<Vec<f64>>, AssetContractError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let array = value
        .as_array()
        .ok_or_else(|| AssetContractError::Contract(format!("{path} must be an array")))?;
    array
        .iter()
        .map(|number| {
            number
                .as_f64()
                .filter(|value| value.is_finite())
                .map(round_9)
                .ok_or_else(|| {
                    AssetContractError::Contract(format!("{path} values must be finite numbers"))
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn vec3(
    value: Option<&Value>,
    default: [f64; 3],
    path: &str,
) -> Result<[f64; 3], AssetContractError> {
    let Some(value) = value else {
        return Ok(default);
    };
    let array = value
        .as_array()
        .ok_or_else(|| AssetContractError::Contract(format!("{path} must be an array")))?;
    if array.len() != 3 {
        return Err(AssetContractError::Contract(format!(
            "{path} must contain three values"
        )));
    }
    let mut result = [0.0; 3];
    for (index, item) in array.iter().enumerate() {
        result[index] = item
            .as_f64()
            .filter(|number| number.is_finite())
            .ok_or_else(|| {
                AssetContractError::Contract(format!("{path} values must be finite numbers"))
            })?;
    }
    Ok(result)
}

fn get_string<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn union_bounds<'a>(bounds_iter: impl Iterator<Item = &'a Bounds3>) -> Option<Bounds3> {
    let mut iter = bounds_iter;
    let first = iter.next()?;
    let mut min = first.min;
    let mut max = first.max;
    for item in iter {
        for axis in 0..3 {
            min[axis] = min[axis].min(item.min[axis]);
            max[axis] = max[axis].max(item.max[axis]);
        }
    }
    Some(bounds(min, max))
}

fn union_two_bounds(left: &Bounds3, right: &Bounds3) -> Bounds3 {
    let mut min = left.min;
    let mut max = left.max;
    for axis in 0..3 {
        min[axis] = min[axis].min(right.min[axis]);
        max[axis] = max[axis].max(right.max[axis]);
    }
    bounds(min, max)
}

fn bounds(min: [f64; 3], max: [f64; 3]) -> Bounds3 {
    Bounds3 {
        min: min.map(round_9),
        max: max.map(round_9),
        size: std::array::from_fn(|axis| round_9(max[axis] - min[axis])),
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, AssetContractError> {
    let slice = bytes.get(offset..offset + 4).ok_or_else(|| {
        AssetContractError::Container(format!("missing 32-bit value at byte {offset}"))
    })?;
    Ok(u32::from_le_bytes(
        slice
            .try_into()
            .expect("slice was checked to contain four bytes"),
    ))
}

fn round_9(value: f64) -> f64 {
    if value.abs() < 5e-12 {
        0.0
    } else {
        (value * 1_000_000_000.0).round_ties_even() / 1_000_000_000.0
    }
}

fn round_places(value: f64, places: u32) -> f64 {
    let factor = 10_f64.powi(places as i32);
    (value * factor).round_ties_even() / factor
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Canonical JSON uses recursively sorted object keys and compact UTF-8 JSON.
pub fn canonical_json(value: &Value) -> String {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(object) => {
                let mut entries = object.iter().collect::<Vec<_>>();
                entries.sort_by(|left, right| left.0.cmp(right.0));
                let mut output = Map::new();
                for (key, value) in entries {
                    output.insert(key.clone(), sorted(value));
                }
                Value::Object(output)
            }
            Value::Array(values) => Value::Array(values.iter().map(sorted).collect()),
            _ => value.clone(),
        }
    }
    serde_json::to_string(&sorted(value)).expect("JSON values are serializable")
}

#[cfg(test)]
mod tests;
