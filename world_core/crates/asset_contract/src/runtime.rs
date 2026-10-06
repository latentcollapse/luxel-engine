//! Deterministic runtime-asset preparation and acceptance.
//!
//! This module validates a model-authored preparation request against the
//! source GLB. It does not generate geometry, retarget animation, or repair
//! assets. A ready package is a content-addressed import manifest; the source
//! bytes remain the artifact to import.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    ASSET_RUNTIME_RECEIPT_SCHEMA, ASSET_RUNTIME_REQUEST_SCHEMA, AccessorInfo, AssetContractError,
    AssetIdentity, AssetReport, AssetUse, Axis, Bounds3, canonical_json, inspect_glb, parse_glb,
    sha256_hex,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTarget {
    Native,
    /// Compatibility-only target retained for historical fixtures. The active
    /// Luxel path uses `native`; it is never a runtime dependency of Luxel.
    Unity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationStatus {
    Ready,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationRole {
    Idle,
    Locomotion,
    Attack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocketRole {
    WeaponMount,
    ProjectileMuzzle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionShape {
    Box,
    Sphere,
    Capsule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetPreparationRequest {
    pub schema_version: String,
    pub target: RuntimeTarget,
    pub asset_use: AssetUse,
    #[serde(default)]
    pub expected_source_sha256: Option<String>,
    pub meters_per_unit: f64,
    pub vertical_axis: Axis,
    #[serde(default)]
    pub rig: Option<RigRequirements>,
    pub required_animations: Vec<AnimationRequirement>,
    pub required_sockets: Vec<SocketRequirement>,
    #[serde(default)]
    pub collision: Option<CollisionMetadata>,
    pub lods: Vec<LodMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigRequirements {
    pub expected_root_joint: String,
    pub max_influences: u8,
    pub weight_sum_tolerance: f64,
    pub require_inverse_bind_matrices: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationRequirement {
    pub role: AnimationRole,
    pub clip_name: String,
    pub minimum_duration_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocketRequirement {
    pub role: SocketRole,
    pub node_name: String,
    pub parent_joint_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionMetadata {
    pub shape: CollisionShape,
    pub center: [f64; 3],
    /// Full local-space dimensions along X/Y/Z.
    pub size: [f64; 3],
    #[serde(default)]
    pub axis: Option<Axis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LodMetadata {
    pub level: u8,
    pub mesh_name: String,
    /// Select this level when projected screen coverage is below this value.
    pub switch_below_fraction: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFindingCode {
    RequestSchemaUnsupported,
    MissingSourceDigestPin,
    SourceDigestMismatch,
    UnsupportedRequiredExtension,
    AssetUseUnspecified,
    InvalidRuntimeTransform,
    InvalidRigRequirements,
    MissingRigContract,
    UnexpectedRigContract,
    InvalidSkin,
    InvalidJoint,
    InvalidSkinHierarchy,
    InactiveSkinnedMesh,
    InvalidInverseBindMatrices,
    MissingSkinnedMesh,
    MissingJointAttributes,
    InvalidJointAccessor,
    InvalidWeightAccessor,
    JointIndexOutOfRange,
    InvalidSkinWeights,
    TooManyInfluences,
    InvalidAnimationSet,
    MissingRequiredAnimation,
    AmbiguousAnimation,
    InvalidAnimation,
    InvalidAnimationTiming,
    InvalidAnimationTarget,
    AnimationNoMotion,
    MissingRequiredSocket,
    AmbiguousSocket,
    InvalidSocketHierarchy,
    InvalidSocketTransform,
    MissingCollision,
    InvalidCollision,
    CollisionOutsideAssetBounds,
    MissingLod0,
    InvalidLodSet,
    MissingLodMesh,
    LodMeshNotSkinned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFinding {
    pub code: RuntimeFindingCode,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetPreparationReceipt {
    pub schema_version: String,
    pub status: PreparationStatus,
    pub source_identity: AssetIdentity,
    pub request_sha256: String,
    pub producer: ProducerIdentity,
    pub package: Option<RuntimeAssetPackage>,
    pub findings: Vec<RuntimeFinding>,
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerIdentity {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeAssetPackage {
    pub package_id: String,
    pub schema_version: String,
    pub target: RuntimeTarget,
    pub asset_use: AssetUse,
    pub source_identity: AssetIdentity,
    /// Every renderable mesh identity in the source package. LOD policy is a
    /// separate selection policy; a multi-part static asset must not be
    /// forced to misrepresent its parts as LOD levels.
    pub mesh_ids: Vec<String>,
    pub transform: RuntimeTransform,
    pub rig: Option<RigSummary>,
    pub animations: Vec<AnimationSummary>,
    pub sockets: Vec<SocketSummary>,
    pub collision: CollisionMetadata,
    pub lods: Vec<LodMetadata>,
    pub provenance: AssetProvenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeTransform {
    pub meters_per_unit: f64,
    pub vertical_axis: Axis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RigSummary {
    pub skin_index: usize,
    pub root_joint_name: String,
    pub root_joint_node: usize,
    pub joint_names: Vec<String>,
    pub joint_nodes: Vec<usize>,
    pub inverse_bind_matrices_sha256: Option<String>,
    pub skinned_mesh_count: usize,
    pub vertex_count: u64,
    pub max_influences_observed: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimationSummary {
    pub role: AnimationRole,
    pub clip_name: String,
    pub duration_seconds: f64,
    pub keyframe_count: u64,
    pub channel_count: u64,
    pub target_joint_names: Vec<String>,
    pub has_sampled_motion: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocketSummary {
    pub role: SocketRole,
    pub node_name: String,
    pub node_index: usize,
    pub parent_joint_name: String,
    pub translation: [f64; 3],
    pub rotation_xyzw: [f64; 4],
    pub scale: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetProvenance {
    pub producer: ProducerIdentity,
    pub source_sha256: String,
    pub source_byte_length: u64,
    pub request_sha256: String,
    pub inspection_report_sha256: String,
}

#[derive(Debug, Clone)]
struct RigValidation {
    summary: Option<RigSummary>,
    joint_nodes: BTreeSet<usize>,
    skinned_meshes: BTreeSet<usize>,
    active_nodes: BTreeSet<usize>,
}

/// Validate the source and compile a deterministic runtime import package.
///
/// A rejected receipt is still returned for structurally valid GLBs so callers
/// can inspect named findings. Invalid containers/documents remain hard errors.
pub fn prepare_asset(
    bytes: &[u8],
    request: &AssetPreparationRequest,
) -> Result<AssetPreparationReceipt, AssetContractError> {
    let report = inspect_glb(bytes)?;
    let (document, binary, _) = parse_glb(bytes)?;
    let request_value = serde_json::to_value(request)
        .map_err(|error| AssetContractError::Json(error.to_string()))?;
    let request_sha256 = sha256_hex(canonical_json(&request_value).as_bytes());
    let producer = producer_identity();
    let mut findings = Vec::new();

    validate_request(request, &report, &mut findings);
    for extension in &report.gltf.extensions_required {
        finding(
            &mut findings,
            RuntimeFindingCode::UnsupportedRequiredExtension,
            format!("extensionsRequired.{extension}"),
            "the bounded runtime profile does not implement required glTF extension semantics",
        );
    }
    let rig = validate_rig(&document, binary, request, &mut findings);
    let animations =
        validate_animations(&document, binary, request, &rig.joint_nodes, &mut findings);
    let sockets = validate_sockets(
        &document,
        request,
        &rig.joint_nodes,
        &rig.active_nodes,
        &mut findings,
    );
    let collision = validate_collision(request, report.facts.bounds_local.as_ref(), &mut findings);
    validate_lods(
        &document,
        request,
        &rig.skinned_meshes,
        &rig.active_nodes,
        &mut findings,
    );

    findings.sort_by(|left, right| {
        (&left.code, &left.subject, &left.detail).cmp(&(&right.code, &right.subject, &right.detail))
    });
    findings.dedup();

    let package = if findings.is_empty() {
        let collision = collision.expect("a ready receipt has validated collision metadata");
        let mesh_ids = runtime_mesh_ids(&document)?;
        let mut package = RuntimeAssetPackage {
            package_id: String::new(),
            schema_version: "luxel.runtime-asset-package/v1".into(),
            target: request.target,
            asset_use: request.asset_use,
            source_identity: report.identity.clone(),
            mesh_ids,
            transform: RuntimeTransform {
                meters_per_unit: request.meters_per_unit,
                vertical_axis: request.vertical_axis,
            },
            rig: rig.summary,
            animations,
            sockets,
            collision,
            lods: sorted_lods(&request.lods),
            provenance: AssetProvenance {
                producer: producer.clone(),
                source_sha256: report.identity.source_sha256.clone(),
                source_byte_length: report.identity.byte_length,
                request_sha256: request_sha256.clone(),
                inspection_report_sha256: report.canonical_report_sha256.clone(),
            },
        };
        let mut value = serde_json::to_value(&package)
            .map_err(|error| AssetContractError::Json(error.to_string()))?;
        value
            .as_object_mut()
            .expect("runtime package serializes as an object")
            .remove("package_id");
        let package_sha256 = sha256_hex(canonical_json(&value).as_bytes());
        package.package_id = format!("runtime_asset_sha256_{package_sha256}");
        Some(package)
    } else {
        None
    };

    let mut receipt = AssetPreparationReceipt {
        schema_version: ASSET_RUNTIME_RECEIPT_SCHEMA.into(),
        status: if package.is_some() {
            PreparationStatus::Ready
        } else {
            PreparationStatus::Rejected
        },
        source_identity: report.identity,
        request_sha256,
        producer,
        package,
        findings,
        receipt_sha256: String::new(),
    };
    let mut value = serde_json::to_value(&receipt)
        .map_err(|error| AssetContractError::Json(error.to_string()))?;
    value
        .as_object_mut()
        .expect("preparation receipt serializes as an object")
        .remove("receipt_sha256");
    receipt.receipt_sha256 = sha256_hex(canonical_json(&value).as_bytes());
    Ok(receipt)
}

fn producer_identity() -> ProducerIdentity {
    ProducerIdentity {
        name: "luxel-asset-contract".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

fn validate_request(
    request: &AssetPreparationRequest,
    report: &AssetReport,
    findings: &mut Vec<RuntimeFinding>,
) {
    if request.schema_version != ASSET_RUNTIME_REQUEST_SCHEMA {
        finding(
            findings,
            RuntimeFindingCode::RequestSchemaUnsupported,
            "schema_version",
            format!("expected {ASSET_RUNTIME_REQUEST_SCHEMA}"),
        );
    }
    if request.asset_use == AssetUse::Unspecified {
        finding(
            findings,
            RuntimeFindingCode::AssetUseUnspecified,
            "asset_use",
            "asset use must be explicit",
        );
    }
    match request.expected_source_sha256.as_deref() {
        None => finding(
            findings,
            RuntimeFindingCode::MissingSourceDigestPin,
            "expected_source_sha256",
            "prepare requests must pin the inspected source SHA-256",
        ),
        Some(expected) if expected != report.identity.source_sha256 => finding(
            findings,
            RuntimeFindingCode::SourceDigestMismatch,
            "expected_source_sha256",
            "pinned source digest does not match the inspected GLB",
        ),
        Some(_) => {}
    }
    if !request.meters_per_unit.is_finite() || request.meters_per_unit <= 0.0 {
        finding(
            findings,
            RuntimeFindingCode::InvalidRuntimeTransform,
            "meters_per_unit",
            "must be finite and greater than zero",
        );
    }

    match request.asset_use {
        AssetUse::Character => {
            if request.rig.is_none() {
                finding(
                    findings,
                    RuntimeFindingCode::MissingRigContract,
                    "rig",
                    "character preparation requires explicit skeleton and skin requirements",
                );
            }
            let expected_roles = [
                AnimationRole::Idle,
                AnimationRole::Locomotion,
                AnimationRole::Attack,
            ];
            let mut roles = request
                .required_animations
                .iter()
                .map(|entry| entry.role)
                .collect::<Vec<_>>();
            roles.sort_unstable();
            roles.dedup();
            if roles != expected_roles {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidAnimationSet,
                    "required_animations",
                    "character contract requires exactly one idle, locomotion, and attack role",
                );
            }
            if !request
                .required_sockets
                .iter()
                .any(|socket| socket.role == SocketRole::WeaponMount)
            {
                finding(
                    findings,
                    RuntimeFindingCode::MissingRequiredSocket,
                    "required_sockets",
                    "character contract requires a weapon_mount socket",
                );
            }
        }
        AssetUse::StaticMesh => {
            if request.rig.is_some() {
                finding(
                    findings,
                    RuntimeFindingCode::UnexpectedRigContract,
                    "rig",
                    "static mesh contract cannot declare character skin requirements",
                );
            }
            if !request.required_animations.is_empty() {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidAnimationSet,
                    "required_animations",
                    "static mesh contract cannot require character animation clips",
                );
            }
        }
        AssetUse::Unspecified => {}
    }

    if let Some(rig) = &request.rig
        && (rig.expected_root_joint.trim().is_empty()
            || !(1..=4).contains(&rig.max_influences)
            || !rig.weight_sum_tolerance.is_finite()
            || rig.weight_sum_tolerance <= 0.0
            || rig.weight_sum_tolerance > 0.1)
    {
        finding(
            findings,
            RuntimeFindingCode::InvalidRigRequirements,
            "rig",
            "root name, influence limit (1..=4), and tolerance (0, 0.1] must be valid",
        );
    }

    let mut clip_names = BTreeSet::new();
    let mut animation_roles = BTreeSet::new();
    for animation in &request.required_animations {
        if animation.clip_name.trim().is_empty()
            || !animation.minimum_duration_seconds.is_finite()
            || animation.minimum_duration_seconds <= 0.0
            || !clip_names.insert(animation.clip_name.clone())
            || !animation_roles.insert(animation.role)
        {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimationSet,
                "required_animations",
                "clip names and roles must be unique; names and minimum durations must be valid",
            );
            break;
        }
    }

    let mut socket_names = BTreeSet::new();
    for socket in &request.required_sockets {
        if socket.node_name.trim().is_empty()
            || socket.parent_joint_name.trim().is_empty()
            || !socket_names.insert(socket.node_name.clone())
        {
            finding(
                findings,
                RuntimeFindingCode::InvalidSocketTransform,
                "required_sockets",
                "socket names and parent joints must be non-empty and socket names unique",
            );
            break;
        }
    }

    if let Some(collision) = &request.collision
        && (!collision.center.iter().all(|value| value.is_finite())
            || !collision
                .size
                .iter()
                .all(|value| value.is_finite() && *value > 0.0))
    {
        finding(
            findings,
            RuntimeFindingCode::InvalidCollision,
            "collision",
            "center must be finite and every full dimension must be finite and positive",
        );
    }
    if request.lods.is_empty() {
        finding(
            findings,
            RuntimeFindingCode::MissingLod0,
            "lods",
            "runtime package requires an explicit LOD 0",
        );
    }
}

fn validate_rig(
    document: &Value,
    binary: &[u8],
    request: &AssetPreparationRequest,
    findings: &mut Vec<RuntimeFinding>,
) -> RigValidation {
    let empty = RigValidation {
        summary: None,
        joint_nodes: BTreeSet::new(),
        skinned_meshes: BTreeSet::new(),
        active_nodes: BTreeSet::new(),
    };
    if request.asset_use != AssetUse::Character {
        return empty;
    }
    let Some(requirements) = &request.rig else {
        return empty;
    };
    let nodes = match super::array_or_empty(document, "nodes") {
        Ok(value) => value,
        Err(_) => {
            finding(
                findings,
                RuntimeFindingCode::InvalidJoint,
                "nodes",
                "node array is invalid",
            );
            return empty;
        }
    };
    let skins = match super::array_or_empty(document, "skins") {
        Ok(value) => value,
        Err(_) => {
            finding(
                findings,
                RuntimeFindingCode::InvalidSkin,
                "skins",
                "skin array is invalid",
            );
            return empty;
        }
    };
    if skins.len() != 1 {
        finding(
            findings,
            RuntimeFindingCode::InvalidSkin,
            "skins",
            "character slice requires exactly one skin",
        );
        return empty;
    }
    for (node_index, node) in nodes.iter().enumerate() {
        if node.get("skin").is_some() && node.get("skin").and_then(Value::as_u64) != Some(0) {
            finding(
                findings,
                RuntimeFindingCode::InvalidSkin,
                format!("nodes[{node_index}].skin"),
                "character node references a missing or unsupported skin index",
            );
        }
    }
    let skin = &skins[0];
    let Some(joints_value) = skin.get("joints").and_then(Value::as_array) else {
        finding(
            findings,
            RuntimeFindingCode::InvalidSkin,
            "skins[0].joints",
            "joint list is missing or is not an array",
        );
        return empty;
    };
    if joints_value.is_empty() {
        finding(
            findings,
            RuntimeFindingCode::InvalidSkin,
            "skins[0].joints",
            "joint list must not be empty",
        );
        return empty;
    }

    let mut joint_nodes = BTreeSet::new();
    let mut ordered_joint_nodes = Vec::with_capacity(joints_value.len());
    let mut joint_names = Vec::with_capacity(joints_value.len());
    let mut names_seen = BTreeSet::new();
    for (joint_order, value) in joints_value.iter().enumerate() {
        let Some(index) = value.as_u64().map(|number| number as usize) else {
            finding(
                findings,
                RuntimeFindingCode::InvalidJoint,
                format!("skins[0].joints[{joint_order}]"),
                "joint node index must be a non-negative integer",
            );
            continue;
        };
        let Some(node) = nodes.get(index) else {
            finding(
                findings,
                RuntimeFindingCode::InvalidJoint,
                format!("skins[0].joints[{joint_order}]"),
                "joint node index is outside the node array",
            );
            continue;
        };
        let name = node
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if name.trim().is_empty() || !names_seen.insert(name.clone()) {
            finding(
                findings,
                RuntimeFindingCode::InvalidJoint,
                format!("nodes[{index}].name"),
                "joint names must be present, non-empty, and unique",
            );
        }
        if !joint_nodes.insert(index) {
            finding(
                findings,
                RuntimeFindingCode::InvalidJoint,
                format!("skins[0].joints[{joint_order}]"),
                "joint node appears more than once",
            );
        }
        ordered_joint_nodes.push(index);
        joint_names.push(name);
    }
    if joint_nodes.len() != joints_value.len() {
        return RigValidation {
            joint_nodes,
            ..empty
        };
    }

    let parents = match parent_map(nodes) {
        Ok(value) => value,
        Err(detail) => {
            finding(
                findings,
                RuntimeFindingCode::InvalidSkinHierarchy,
                "nodes",
                detail,
            );
            return RigValidation {
                joint_nodes,
                ..empty
            };
        }
    };
    let root_index = if let Some(skeleton) = skin.get("skeleton") {
        skeleton
            .as_u64()
            .map(|value| value as usize)
            .filter(|index| *index < nodes.len())
    } else {
        let roots = ordered_joint_nodes
            .iter()
            .copied()
            .filter(|index| !parents[*index].is_some_and(|parent| joint_nodes.contains(&parent)))
            .collect::<Vec<_>>();
        if roots.len() == 1 {
            Some(roots[0])
        } else {
            None
        }
    };
    let Some(root_index) = root_index else {
        finding(
            findings,
            RuntimeFindingCode::InvalidSkinHierarchy,
            "skins[0].skeleton",
            "skeleton root must resolve to one node",
        );
        return RigValidation {
            joint_nodes,
            ..empty
        };
    };
    let root_name = nodes[root_index]
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if !joint_nodes.contains(&root_index)
        || root_name != requirements.expected_root_joint
        || ordered_joint_nodes
            .iter()
            .any(|joint| !is_descendant_or_self(*joint, root_index, &parents))
    {
        finding(
            findings,
            RuntimeFindingCode::InvalidSkinHierarchy,
            "skins[0].skeleton",
            "declared root must match the request, be a skin joint, and contain every joint",
        );
    }
    let active_nodes = match super::active_node_indices(document, nodes) {
        Ok(active) => active,
        Err(_) => {
            finding(
                findings,
                RuntimeFindingCode::InvalidSkinHierarchy,
                "scene",
                "selected scene graph is invalid or unreachable",
            );
            BTreeSet::new()
        }
    };
    let active_scene_invalid = ordered_joint_nodes
        .iter()
        .any(|joint| !active_nodes.contains(joint));
    if active_scene_invalid {
        finding(
            findings,
            RuntimeFindingCode::InvalidSkinHierarchy,
            "scene",
            "every skin joint must be reachable from the selected scene",
        );
    }

    let inverse_bind_digest = validate_inverse_bind_matrices(
        document,
        binary,
        skin,
        ordered_joint_nodes.len(),
        requirements.require_inverse_bind_matrices,
        findings,
    );

    let mut skinned_meshes = BTreeSet::new();
    let mut vertex_count = 0u64;
    let mut max_influences_observed = 0u8;
    for (node_index, node) in nodes.iter().enumerate() {
        let uses_skin = node.get("skin").and_then(Value::as_u64) == Some(0);
        if !uses_skin {
            continue;
        }
        if !active_nodes.contains(&node_index) {
            finding(
                findings,
                RuntimeFindingCode::InactiveSkinnedMesh,
                format!("nodes[{node_index}]"),
                "skinned mesh node is not reachable from the selected scene",
            );
        }
        let Some(mesh_index) = node.get("mesh").and_then(Value::as_u64).map(|v| v as usize) else {
            finding(
                findings,
                RuntimeFindingCode::MissingSkinnedMesh,
                format!("nodes[{node_index}]"),
                "node declares the character skin without a mesh",
            );
            continue;
        };
        let meshes = match super::array_or_empty(document, "meshes") {
            Ok(value) => value,
            Err(_) => &[],
        };
        let Some(mesh) = meshes.get(mesh_index) else {
            finding(
                findings,
                RuntimeFindingCode::MissingSkinnedMesh,
                format!("nodes[{node_index}].mesh"),
                "skinned node references a missing mesh",
            );
            continue;
        };
        let mesh_name = mesh
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        skinned_meshes.insert(mesh_index);
        let Some(primitives) = mesh.get("primitives").and_then(Value::as_array) else {
            finding(
                findings,
                RuntimeFindingCode::MissingSkinnedMesh,
                format!("meshes[{mesh_index}].primitives"),
                "skinned mesh has no primitive array",
            );
            continue;
        };
        for (primitive_index, primitive) in primitives.iter().enumerate() {
            let subject = format!("{mesh_name}.primitives[{primitive_index}]");
            if let Some((vertices, influences)) = validate_skin_primitive(
                document,
                binary,
                primitive,
                joint_nodes.len(),
                requirements,
                &subject,
                findings,
            ) {
                vertex_count += vertices;
                max_influences_observed = max_influences_observed.max(influences);
            }
        }
    }
    if skinned_meshes.is_empty() {
        finding(
            findings,
            RuntimeFindingCode::MissingSkinnedMesh,
            "nodes",
            "no mesh node is bound to the required skin",
        );
    }

    let summary = Some(RigSummary {
        skin_index: 0,
        root_joint_name: root_name,
        root_joint_node: root_index,
        joint_names,
        joint_nodes: ordered_joint_nodes,
        inverse_bind_matrices_sha256: inverse_bind_digest,
        skinned_mesh_count: skinned_meshes.len(),
        vertex_count,
        max_influences_observed,
    });
    RigValidation {
        summary,
        joint_nodes,
        skinned_meshes,
        active_nodes,
    }
}

fn validate_inverse_bind_matrices(
    document: &Value,
    binary: &[u8],
    skin: &Value,
    joint_count: usize,
    required: bool,
    findings: &mut Vec<RuntimeFinding>,
) -> Option<String> {
    let Some(index) = skin
        .get("inverseBindMatrices")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
    else {
        if required {
            finding(
                findings,
                RuntimeFindingCode::InvalidInverseBindMatrices,
                "skins[0].inverseBindMatrices",
                "inverse bind matrices are required by the character runtime profile",
            );
        }
        return None;
    };
    let info = match super::accessor_info(document, binary, index) {
        Ok(info) => info,
        Err(_) => {
            finding(
                findings,
                RuntimeFindingCode::InvalidInverseBindMatrices,
                "skins[0].inverseBindMatrices",
                "accessor cannot be decoded",
            );
            return None;
        }
    };
    if info.accessor_type != "MAT4"
        || info.component_type != 5126
        || info.normalized
        || info.count != joint_count
    {
        finding(
            findings,
            RuntimeFindingCode::InvalidInverseBindMatrices,
            "skins[0].inverseBindMatrices",
            "accessor must be unnormalized FLOAT MAT4 with one matrix per joint",
        );
        return None;
    }
    let mut matrices = Vec::with_capacity(info.count * 16);
    for element in 0..info.count {
        let mut matrix = [0.0; 16];
        for (component, matrix_value) in matrix.iter_mut().enumerate() {
            let Some(value) = read_component(binary, &info, element, component) else {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidInverseBindMatrices,
                    "skins[0].inverseBindMatrices",
                    "matrix contains an unreadable component",
                );
                return None;
            };
            if !value.is_finite() {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidInverseBindMatrices,
                    "skins[0].inverseBindMatrices",
                    "matrix contains a non-finite component",
                );
                return None;
            }
            *matrix_value = value;
        }
        if determinant4(matrix).abs() < 1e-10 {
            finding(
                findings,
                RuntimeFindingCode::InvalidInverseBindMatrices,
                format!("skins[0].inverseBindMatrices[{element}]"),
                "inverse bind matrix is singular",
            );
            return None;
        }
        matrices.extend(matrix.into_iter().map(|value| (value as f32).to_bits()));
    }
    let bytes = matrices
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    Some(sha256_hex(&bytes))
}

fn validate_skin_primitive(
    document: &Value,
    binary: &[u8],
    primitive: &Value,
    joint_count: usize,
    requirements: &RigRequirements,
    subject: &str,
    findings: &mut Vec<RuntimeFinding>,
) -> Option<(u64, u8)> {
    let Some(attributes) = primitive.get("attributes").and_then(Value::as_object) else {
        finding(
            findings,
            RuntimeFindingCode::MissingJointAttributes,
            subject,
            "primitive has no attribute map",
        );
        return None;
    };
    if attributes.contains_key("JOINTS_1") || attributes.contains_key("WEIGHTS_1") {
        finding(
            findings,
            RuntimeFindingCode::InvalidJointAccessor,
            subject,
            "this bounded profile supports only JOINTS_0 and WEIGHTS_0",
        );
    }
    let joint_accessor = attributes
        .get("JOINTS_0")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let weight_accessor = attributes
        .get("WEIGHTS_0")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let (Some(joint_accessor), Some(weight_accessor)) = (joint_accessor, weight_accessor) else {
        finding(
            findings,
            RuntimeFindingCode::MissingJointAttributes,
            subject,
            "every skinned primitive requires JOINTS_0 and WEIGHTS_0",
        );
        return None;
    };
    let joints = match super::accessor_info(document, binary, joint_accessor) {
        Ok(info) => info,
        Err(_) => {
            finding(
                findings,
                RuntimeFindingCode::InvalidJointAccessor,
                subject,
                "JOINTS_0 accessor cannot be decoded",
            );
            return None;
        }
    };
    let weights = match super::accessor_info(document, binary, weight_accessor) {
        Ok(info) => info,
        Err(_) => {
            finding(
                findings,
                RuntimeFindingCode::InvalidWeightAccessor,
                subject,
                "WEIGHTS_0 accessor cannot be decoded",
            );
            return None;
        }
    };
    let valid_joint_format = joints.accessor_type == "VEC4"
        && matches!(joints.component_type, 5121 | 5123)
        && !joints.normalized;
    if !valid_joint_format {
        finding(
            findings,
            RuntimeFindingCode::InvalidJointAccessor,
            subject,
            "JOINTS_0 must be unnormalized UNSIGNED_BYTE or UNSIGNED_SHORT VEC4",
        );
        return None;
    }
    let valid_weight_format = weights.accessor_type == "VEC4"
        && (weights.component_type == 5126 && !weights.normalized
            || matches!(weights.component_type, 5121 | 5123) && weights.normalized);
    if !valid_weight_format {
        finding(
            findings,
            RuntimeFindingCode::InvalidWeightAccessor,
            subject,
            "WEIGHTS_0 must be FLOAT VEC4 or normalized unsigned integer VEC4",
        );
        return None;
    }
    if joints.count != weights.count {
        finding(
            findings,
            RuntimeFindingCode::InvalidWeightAccessor,
            subject,
            "JOINTS_0 and WEIGHTS_0 must have the same vertex count",
        );
        return None;
    }
    let position_count = attributes
        .get("POSITION")
        .and_then(Value::as_u64)
        .and_then(|index| super::accessor_info(document, binary, index as usize).ok())
        .map(|info| info.count);
    if position_count != Some(joints.count) {
        finding(
            findings,
            RuntimeFindingCode::InvalidWeightAccessor,
            subject,
            "skin attribute counts must match POSITION",
        );
        return None;
    }

    let mut maximum_influences = 0u8;
    for vertex in 0..joints.count {
        let mut sum = 0.0;
        let mut influences = 0u8;
        for component in 0..4 {
            let joint =
                read_component(binary, &joints, vertex, component).map(|value| value as usize);
            let weight = read_weight(binary, &weights, vertex, component);
            let (Some(joint), Some(weight)) = (joint, weight) else {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidWeightAccessor,
                    subject,
                    "skin accessor contains an unreadable value",
                );
                return None;
            };
            if joint >= joint_count {
                finding(
                    findings,
                    RuntimeFindingCode::JointIndexOutOfRange,
                    format!("{subject}.vertex[{vertex}]"),
                    "JOINTS_0 references an index outside skins[0].joints",
                );
                return None;
            }
            if !weight.is_finite() || weight < 0.0 {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidSkinWeights,
                    format!("{subject}.vertex[{vertex}]"),
                    "skin weights must be finite and non-negative",
                );
                return None;
            }
            if weight > 0.0 {
                influences += 1;
            }
            sum += weight;
        }
        if influences == 0 || (sum - 1.0).abs() > requirements.weight_sum_tolerance {
            finding(
                findings,
                RuntimeFindingCode::InvalidSkinWeights,
                format!("{subject}.vertex[{vertex}]"),
                "each vertex must have positive influences whose weights sum to one",
            );
            return None;
        }
        if influences > requirements.max_influences {
            finding(
                findings,
                RuntimeFindingCode::TooManyInfluences,
                format!("{subject}.vertex[{vertex}]"),
                format!(
                    "observed {influences}, limit is {}",
                    requirements.max_influences
                ),
            );
            return None;
        }
        maximum_influences = maximum_influences.max(influences);
    }
    Some((joints.count as u64, maximum_influences))
}

fn validate_animations(
    document: &Value,
    binary: &[u8],
    request: &AssetPreparationRequest,
    joint_nodes: &BTreeSet<usize>,
    findings: &mut Vec<RuntimeFinding>,
) -> Vec<AnimationSummary> {
    let animations = match super::array_or_empty(document, "animations") {
        Ok(value) => value,
        Err(_) => {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimationSet,
                "animations",
                "animation array is invalid",
            );
            return Vec::new();
        }
    };
    let nodes = super::array_or_empty(document, "nodes").unwrap_or(&[]);
    let mut output = Vec::new();
    for required in &request.required_animations {
        let matching = animations
            .iter()
            .enumerate()
            .filter(|(_, animation)| {
                animation.get("name").and_then(Value::as_str) == Some(&required.clip_name)
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            finding(
                findings,
                RuntimeFindingCode::MissingRequiredAnimation,
                &required.clip_name,
                format!("required {:?} clip is absent", required.role),
            );
            continue;
        }
        if matching.len() != 1 {
            finding(
                findings,
                RuntimeFindingCode::AmbiguousAnimation,
                &required.clip_name,
                "animation name must identify exactly one clip",
            );
            continue;
        }
        let (_, animation) = matching[0];
        if let Some(summary) = validate_animation(
            animation,
            required,
            binary,
            nodes,
            joint_nodes,
            request.asset_use == AssetUse::Character,
            document,
            findings,
        ) {
            output.push(summary);
        }
    }
    output.sort_by_key(|summary| summary.role);
    output
}

#[allow(clippy::too_many_arguments)]
fn validate_animation(
    animation: &Value,
    requirement: &AnimationRequirement,
    binary: &[u8],
    nodes: &[Value],
    joint_nodes: &BTreeSet<usize>,
    character: bool,
    document: &Value,
    findings: &mut Vec<RuntimeFinding>,
) -> Option<AnimationSummary> {
    let subject = requirement.clip_name.as_str();
    let Some(samplers) = animation.get("samplers").and_then(Value::as_array) else {
        finding(
            findings,
            RuntimeFindingCode::InvalidAnimation,
            subject,
            "clip samplers must be an array",
        );
        return None;
    };
    let Some(channels) = animation.get("channels").and_then(Value::as_array) else {
        finding(
            findings,
            RuntimeFindingCode::InvalidAnimation,
            subject,
            "clip channels must be an array",
        );
        return None;
    };
    if samplers.is_empty() || channels.is_empty() {
        finding(
            findings,
            RuntimeFindingCode::InvalidAnimation,
            subject,
            "clip must contain at least one sampler and one channel",
        );
        return None;
    }
    let mut targets = BTreeSet::new();
    let mut target_names = BTreeSet::new();
    let mut duration = 0.0f64;
    let mut keyframe_count = 0usize;
    let mut has_sampled_motion = false;
    for (channel_index, channel) in channels.iter().enumerate() {
        let sampler_index = channel
            .get("sampler")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let Some(sampler) = sampler_index.and_then(|index| samplers.get(index)) else {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimation,
                format!("{subject}.channels[{channel_index}]"),
                "channel sampler index is missing or out of range",
            );
            continue;
        };
        let Some(target) = channel.get("target") else {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimationTarget,
                format!("{subject}.channels[{channel_index}]"),
                "animation channel target is missing",
            );
            continue;
        };
        let node_index = target
            .get("node")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let path = target
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(node_index) = node_index.filter(|index| *index < nodes.len()) else {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimationTarget,
                format!("{subject}.channels[{channel_index}]"),
                "target node index is missing or out of range",
            );
            continue;
        };
        if !matches!(path, "translation" | "rotation" | "scale" | "weights") {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimationTarget,
                format!("{subject}.channels[{channel_index}]"),
                "target path is unsupported",
            );
            continue;
        }
        if character && path != "weights" && !joint_nodes.contains(&node_index) {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimationTarget,
                format!("{subject}.channels[{channel_index}]"),
                "character transform animation must target a validated skin joint",
            );
        }
        if !targets.insert((node_index, path.to_owned())) {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimationTarget,
                format!("{subject}.channels[{channel_index}]"),
                "clip repeats a node/path target",
            );
        }
        target_names.insert(
            nodes[node_index]
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        );

        let input_index = sampler
            .get("input")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let output_index = sampler
            .get("output")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let interpolation = sampler
            .get("interpolation")
            .and_then(Value::as_str)
            .unwrap_or("LINEAR");
        if !matches!(interpolation, "LINEAR" | "STEP" | "CUBICSPLINE") {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimation,
                format!("{subject}.channels[{channel_index}]"),
                "interpolation must be LINEAR, STEP, or CUBICSPLINE",
            );
            continue;
        }
        let (Some(input_index), Some(output_index)) = (input_index, output_index) else {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimation,
                format!("{subject}.channels[{channel_index}]"),
                "sampler input and output accessor indices are required",
            );
            continue;
        };
        let input = match super::accessor_info(document, binary, input_index) {
            Ok(info) => info,
            Err(_) => {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidAnimationTiming,
                    format!("{subject}.channels[{channel_index}]"),
                    "input time accessor cannot be decoded",
                );
                continue;
            }
        };
        let output = match super::accessor_info(document, binary, output_index) {
            Ok(info) => info,
            Err(_) => {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidAnimation,
                    format!("{subject}.channels[{channel_index}]"),
                    "output accessor cannot be decoded",
                );
                continue;
            }
        };
        if input.accessor_type != "SCALAR"
            || input.component_type != 5126
            || input.normalized
            || input.count < 2
        {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimationTiming,
                format!("{subject}.channels[{channel_index}]"),
                "input times must be an unnormalized FLOAT SCALAR accessor with at least two keys",
            );
            continue;
        }
        let mut times = Vec::with_capacity(input.count);
        for key in 0..input.count {
            let value = read_component(binary, &input, key, 0).unwrap_or(f64::NAN);
            times.push(value);
        }
        if times.iter().any(|time| !time.is_finite() || *time < 0.0)
            || times.windows(2).any(|pair| pair[1] <= pair[0])
        {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimationTiming,
                format!("{subject}.channels[{channel_index}]"),
                "key times must be finite, non-negative, and strictly increasing",
            );
            continue;
        }
        let interpolation_multiplier = if interpolation == "CUBICSPLINE" { 3 } else { 1 };
        let expected_output_type = match path {
            "translation" | "scale" => "VEC3",
            "rotation" => "VEC4",
            "weights" => "SCALAR",
            _ => unreachable!(),
        };
        if output.component_type != 5126
            || output.normalized
            || output.accessor_type != expected_output_type
            || output.count != input.count * interpolation_multiplier
        {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimation,
                format!("{subject}.channels[{channel_index}]"),
                "output accessor type/count does not match target path, keys, and interpolation",
            );
            continue;
        }
        let mut output_is_valid = true;
        for element in 0..output.count {
            for component in 0..output.component_count {
                let value = read_component(binary, &output, element, component).unwrap_or(f64::NAN);
                if !value.is_finite() {
                    output_is_valid = false;
                    break;
                }
            }
        }
        if !output_is_valid {
            finding(
                findings,
                RuntimeFindingCode::InvalidAnimation,
                format!("{subject}.channels[{channel_index}]"),
                "output accessor contains a non-finite value",
            );
            continue;
        }
        if path == "rotation" {
            let key_stride = interpolation_multiplier;
            for key in 0..input.count {
                let value_index = key * key_stride + usize::from(interpolation == "CUBICSPLINE");
                let q = (0..4)
                    .map(|component| read_component(binary, &output, value_index, component))
                    .collect::<Option<Vec<_>>>();
                let Some(q) = q else {
                    output_is_valid = false;
                    break;
                };
                let norm = q.iter().map(|value| value * value).sum::<f64>().sqrt();
                if (norm - 1.0).abs() > 1e-3 {
                    output_is_valid = false;
                    break;
                }
            }
            if !output_is_valid {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidAnimation,
                    format!("{subject}.channels[{channel_index}]"),
                    "rotation key quaternions must be normalized",
                );
                continue;
            }
        }
        let sample_offset = usize::from(interpolation == "CUBICSPLINE");
        let first_sample = (0..output.component_count)
            .map(|component| read_component(binary, &output, sample_offset, component))
            .collect::<Option<Vec<_>>>();
        if let Some(first) = first_sample {
            for key in 1..input.count {
                let sample_index = key * interpolation_multiplier + sample_offset;
                let sample = (0..output.component_count)
                    .map(|component| read_component(binary, &output, sample_index, component))
                    .collect::<Option<Vec<_>>>();
                if let Some(sample) = sample {
                    if path == "rotation" && first.len() == 4 && sample.len() == 4 {
                        let dot = first
                            .iter()
                            .zip(&sample)
                            .map(|(left, right)| left * right)
                            .sum::<f64>()
                            .abs();
                        has_sampled_motion |= dot < 0.9995;
                    } else {
                        let delta = first
                            .iter()
                            .zip(&sample)
                            .map(|(left, right)| (left - right) * (left - right))
                            .sum::<f64>()
                            .sqrt();
                        has_sampled_motion |= delta > 1e-5;
                    }
                }
            }
        }
        let channel_duration =
            times.last().copied().unwrap_or_default() - times.first().copied().unwrap_or_default();
        duration = duration.max(channel_duration);
        keyframe_count = keyframe_count.max(input.count);
    }
    if duration < requirement.minimum_duration_seconds {
        finding(
            findings,
            RuntimeFindingCode::InvalidAnimationTiming,
            subject,
            format!(
                "clip duration {duration:.6}s is shorter than required {:.6}s",
                requirement.minimum_duration_seconds
            ),
        );
    }
    if matches!(
        requirement.role,
        AnimationRole::Locomotion | AnimationRole::Attack
    ) && !has_sampled_motion
    {
        finding(
            findings,
            RuntimeFindingCode::AnimationNoMotion,
            subject,
            "locomotion and attack clips must change at least one sampled transform or morph curve",
        );
    }
    if channels.is_empty() || targets.is_empty() || duration <= 0.0 {
        return None;
    }
    Some(AnimationSummary {
        role: requirement.role,
        clip_name: requirement.clip_name.clone(),
        duration_seconds: duration,
        keyframe_count: keyframe_count as u64,
        channel_count: channels.len() as u64,
        target_joint_names: target_names.into_iter().collect(),
        has_sampled_motion,
    })
}

fn validate_sockets(
    document: &Value,
    request: &AssetPreparationRequest,
    joint_nodes: &BTreeSet<usize>,
    active_nodes: &BTreeSet<usize>,
    findings: &mut Vec<RuntimeFinding>,
) -> Vec<SocketSummary> {
    let nodes = match super::array_or_empty(document, "nodes") {
        Ok(value) => value,
        Err(_) => {
            return Vec::new();
        }
    };
    let parents = match parent_map(nodes) {
        Ok(value) => value,
        Err(detail) => {
            finding(
                findings,
                RuntimeFindingCode::InvalidSocketHierarchy,
                "nodes",
                detail,
            );
            return Vec::new();
        }
    };
    let scene_active_nodes = if request.asset_use == AssetUse::Character {
        active_nodes.clone()
    } else {
        super::active_node_indices(document, nodes).unwrap_or_default()
    };
    let mut output = Vec::new();
    for required in &request.required_sockets {
        let matches = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.get("name").and_then(Value::as_str) == Some(&required.node_name)
            })
            .collect::<Vec<_>>();
        if matches.is_empty() {
            finding(
                findings,
                RuntimeFindingCode::MissingRequiredSocket,
                &required.node_name,
                format!("required {:?} node is absent", required.role),
            );
            continue;
        }
        if matches.len() != 1 {
            finding(
                findings,
                RuntimeFindingCode::AmbiguousSocket,
                &required.node_name,
                "socket node name must be unique",
            );
            continue;
        }
        let (node_index, node) = matches[0];
        let parent_index = parents[node_index];
        let parent_name = parent_index
            .and_then(|index| nodes.get(index))
            .and_then(|node| node.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let parent_is_valid_joint = request.asset_use != AssetUse::Character
            || parent_index.is_some_and(|index| joint_nodes.contains(&index));
        let socket_and_parent_are_active = scene_active_nodes.contains(&node_index)
            && parent_index.is_some_and(|index| scene_active_nodes.contains(&index));
        if parent_name != required.parent_joint_name
            || !parent_is_valid_joint
            || !socket_and_parent_are_active
            || node
                .get("extras")
                .and_then(|extras| extras.get("luxel_socket"))
                .and_then(Value::as_bool)
                != Some(true)
        {
            finding(
                findings,
                RuntimeFindingCode::InvalidSocketHierarchy,
                &required.node_name,
                "socket must be tagged extras.luxel_socket=true and be a direct child of its declared joint",
            );
            continue;
        }
        let transform = match node_transform(node) {
            Ok(value) => value,
            Err(detail) => {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidSocketTransform,
                    &required.node_name,
                    detail,
                );
                continue;
            }
        };
        output.push(SocketSummary {
            role: required.role,
            node_name: required.node_name.clone(),
            node_index,
            parent_joint_name: required.parent_joint_name.clone(),
            translation: transform.0,
            rotation_xyzw: transform.1,
            scale: transform.2,
        });
    }
    output.sort_by(|left, right| left.node_name.cmp(&right.node_name));
    output
}

fn validate_collision(
    request: &AssetPreparationRequest,
    bounds: Option<&Bounds3>,
    findings: &mut Vec<RuntimeFinding>,
) -> Option<CollisionMetadata> {
    let Some(collision) = &request.collision else {
        finding(
            findings,
            RuntimeFindingCode::MissingCollision,
            "collision",
            "runtime package requires explicit collision metadata",
        );
        return None;
    };
    match collision.shape {
        CollisionShape::Box => {
            if collision.axis.is_some() {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidCollision,
                    "collision.axis",
                    "box collision must not declare an orientation axis",
                );
            }
        }
        CollisionShape::Sphere => {
            if collision.axis.is_some()
                || !approximately_equal(collision.size[0], collision.size[1], 1e-5)
                || !approximately_equal(collision.size[1], collision.size[2], 1e-5)
            {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidCollision,
                    "collision",
                    "sphere dimensions must be equal and must not declare an axis",
                );
            }
        }
        CollisionShape::Capsule => {
            if collision.axis != Some(request.vertical_axis) {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidCollision,
                    "collision.axis",
                    "capsule axis must equal the declared runtime vertical axis",
                );
            }
            let axis = axis_index(request.vertical_axis);
            let radial = (0..3).filter(|index| *index != axis).collect::<Vec<_>>();
            if !approximately_equal(collision.size[radial[0]], collision.size[radial[1]], 1e-5)
                || collision.size[axis] < collision.size[radial[0]]
            {
                finding(
                    findings,
                    RuntimeFindingCode::InvalidCollision,
                    "collision.size",
                    "capsule requires equal radial dimensions and height at least its diameter",
                );
            }
        }
    }
    if let Some(bounds) = bounds {
        for axis in 0..3 {
            let low = collision.center[axis] - collision.size[axis] * 0.5;
            let high = collision.center[axis] + collision.size[axis] * 0.5;
            if low < bounds.min[axis] - 1e-5 || high > bounds.max[axis] + 1e-5 {
                finding(
                    findings,
                    RuntimeFindingCode::CollisionOutsideAssetBounds,
                    "collision",
                    "collision volume extends outside the asset's local bounds",
                );
                break;
            }
        }
    } else {
        finding(
            findings,
            RuntimeFindingCode::InvalidCollision,
            "collision",
            "asset has no measured geometry bounds",
        );
    }
    Some(collision.clone())
}

fn validate_lods(
    document: &Value,
    request: &AssetPreparationRequest,
    skinned_meshes: &BTreeSet<usize>,
    active_nodes: &BTreeSet<usize>,
    findings: &mut Vec<RuntimeFinding>,
) {
    if request.lods.is_empty() {
        return;
    }
    let meshes = super::array_or_empty(document, "meshes").unwrap_or(&[]);
    let mut levels = BTreeMap::new();
    let mut mesh_names = BTreeSet::new();
    let mut previous_threshold = f64::INFINITY;
    let ordered_lods = sorted_lods(&request.lods);
    for lod in &ordered_lods {
        if levels.insert(lod.level, ()).is_some()
            || lod.mesh_name.trim().is_empty()
            || !mesh_names.insert(lod.mesh_name.clone())
            || !lod.switch_below_fraction.is_finite()
            || lod.switch_below_fraction <= 0.0
            || lod.switch_below_fraction > 1.0
            || lod.switch_below_fraction >= previous_threshold
        {
            finding(
                findings,
                RuntimeFindingCode::InvalidLodSet,
                "lods",
                "levels must be unique and contiguous, names unique, and thresholds strictly descend in (0, 1]",
            );
            break;
        }
        previous_threshold = lod.switch_below_fraction;
    }
    if !levels.contains_key(&0) {
        finding(
            findings,
            RuntimeFindingCode::MissingLod0,
            "lods",
            "LOD 0 is required",
        );
    }
    if request
        .lods
        .iter()
        .any(|lod| lod.level == 0 && (lod.switch_below_fraction - 1.0).abs() > 1e-9)
    {
        finding(
            findings,
            RuntimeFindingCode::InvalidLodSet,
            "lods[0].switch_below_fraction",
            "LOD 0 must be the default level with threshold 1.0",
        );
    }
    if ordered_lods
        .iter()
        .enumerate()
        .any(|(expected, lod)| usize::from(lod.level) != expected)
    {
        finding(
            findings,
            RuntimeFindingCode::InvalidLodSet,
            "lods",
            "LOD levels must be contiguous starting at zero",
        );
    }
    for lod in &request.lods {
        let matches = meshes
            .iter()
            .enumerate()
            .filter(|(mesh_index, mesh)| mesh_name(mesh, *mesh_index) == lod.mesh_name)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            finding(
                findings,
                RuntimeFindingCode::MissingLodMesh,
                &lod.mesh_name,
                "LOD mesh name must resolve to exactly one glTF mesh",
            );
            continue;
        }
        let has_primitives = meshes[matches[0]]
            .get("primitives")
            .and_then(Value::as_array)
            .is_some_and(|primitives| !primitives.is_empty());
        if !has_primitives {
            finding(
                findings,
                RuntimeFindingCode::MissingLodMesh,
                &lod.mesh_name,
                "LOD mesh must contain at least one renderable primitive",
            );
            continue;
        }
        if request.asset_use == AssetUse::Character && !skinned_meshes.contains(&matches[0]) {
            finding(
                findings,
                RuntimeFindingCode::LodMeshNotSkinned,
                &lod.mesh_name,
                "every character LOD mesh must be bound to the validated skin",
            );
        }
        if request.asset_use == AssetUse::Character && skinned_meshes.contains(&matches[0]) {
            let nodes = super::array_or_empty(document, "nodes").unwrap_or(&[]);
            let has_active_skinned_node = nodes.iter().enumerate().any(|(node_index, node)| {
                node.get("mesh").and_then(Value::as_u64) == Some(matches[0] as u64)
                    && node.get("skin").and_then(Value::as_u64) == Some(0)
                    && active_nodes.contains(&node_index)
            });
            if !has_active_skinned_node {
                finding(
                    findings,
                    RuntimeFindingCode::InactiveSkinnedMesh,
                    &lod.mesh_name,
                    "character LOD has no skinned mesh node reachable from the selected scene",
                );
            }
        }
    }
}

fn mesh_name(mesh: &Value, mesh_index: usize) -> String {
    mesh.get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("mesh_{mesh_index}"))
}

fn runtime_mesh_ids(document: &Value) -> Result<Vec<String>, AssetContractError> {
    let meshes = super::array_or_empty(document, "meshes")?;
    let mut ids = BTreeSet::new();
    for (mesh_index, mesh) in meshes.iter().enumerate() {
        let id = mesh_name(mesh, mesh_index);
        if !ids.insert(id.clone()) {
            return Err(AssetContractError::Contract(format!(
                "duplicate runtime mesh identity {id}"
            )));
        }
    }
    Ok(ids.into_iter().collect())
}

fn sorted_lods(lods: &[LodMetadata]) -> Vec<LodMetadata> {
    let mut result = lods.to_vec();
    result.sort_by_key(|entry| entry.level);
    result
}

fn parent_map(nodes: &[Value]) -> Result<Vec<Option<usize>>, String> {
    let mut parents = vec![None; nodes.len()];
    for (parent, node) in nodes.iter().enumerate() {
        let Some(children) = node.get("children") else {
            continue;
        };
        let Some(children) = children.as_array() else {
            return Err(format!("nodes[{parent}].children must be an array"));
        };
        for child in children {
            let Some(child) = child.as_u64().map(|value| value as usize) else {
                return Err(format!("nodes[{parent}] has a non-integer child index"));
            };
            if child >= nodes.len() {
                return Err(format!("nodes[{parent}] references a missing child node"));
            }
            if parents[child].is_some_and(|existing| existing != parent) {
                return Err(format!("nodes[{child}] has more than one parent"));
            }
            parents[child] = Some(parent);
        }
    }
    for start in 0..nodes.len() {
        let mut seen = BTreeSet::new();
        let mut current = Some(start);
        while let Some(index) = current {
            if !seen.insert(index) {
                return Err(format!(
                    "node hierarchy contains a cycle through node {index}"
                ));
            }
            current = parents[index];
        }
    }
    Ok(parents)
}

fn is_descendant_or_self(node: usize, ancestor: usize, parents: &[Option<usize>]) -> bool {
    let mut current = Some(node);
    while let Some(index) = current {
        if index == ancestor {
            return true;
        }
        current = parents.get(index).copied().flatten();
    }
    false
}

type NodeTransform = ([f64; 3], [f64; 4], [f64; 3]);

fn node_transform(node: &Value) -> Result<NodeTransform, String> {
    if node.get("matrix").is_some() {
        return Err("socket matrix transforms are unsupported; provide local TRS".into());
    }
    let translation = super::vec3(node.get("translation"), [0.0; 3], "socket.translation")
        .map_err(|_| "socket translation must contain three finite numbers")?;
    let scale = super::vec3(node.get("scale"), [1.0; 3], "socket.scale")
        .map_err(|_| "socket scale must contain three finite numbers")?;
    let rotation = match node.get("rotation") {
        None => [0.0, 0.0, 0.0, 1.0],
        Some(value) => {
            let Some(values) = value.as_array() else {
                return Err("socket rotation must be an XYZW quaternion".into());
            };
            if values.len() != 4 {
                return Err("socket rotation must contain four quaternion values".into());
            }
            let mut result = [0.0; 4];
            for (index, value) in values.iter().enumerate() {
                result[index] = value
                    .as_f64()
                    .filter(|number| number.is_finite())
                    .ok_or("socket rotation must contain finite values")?;
            }
            result
        }
    };
    let norm = rotation
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if (norm - 1.0).abs() > 1e-3 {
        return Err("socket rotation quaternion must be normalized".into());
    }
    if scale
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err("socket scale components must be finite and positive".into());
    }
    Ok((translation, rotation, scale))
}

fn read_weight(
    binary: &[u8],
    info: &AccessorInfo<'_>,
    element: usize,
    component: usize,
) -> Option<f64> {
    let value = read_component(binary, info, element, component)?;
    Some(match info.component_type {
        5121 if info.normalized => value / u8::MAX as f64,
        5123 if info.normalized => value / u16::MAX as f64,
        _ => value,
    })
}

fn read_component(
    binary: &[u8],
    info: &AccessorInfo<'_>,
    element: usize,
    component: usize,
) -> Option<f64> {
    if element >= info.count || component >= info.component_count {
        return None;
    }
    let offset = info
        .byte_offset
        .checked_add(element.checked_mul(info.byte_stride)?)?
        .checked_add(component.checked_mul(info.component_size)?)?;
    let bytes = binary.get(offset..offset.checked_add(info.component_size)?)?;
    Some(match info.component_type {
        5121 => bytes[0] as f64,
        5123 => u16::from_le_bytes(bytes.try_into().ok()?) as f64,
        5126 => f32::from_le_bytes(bytes.try_into().ok()?) as f64,
        _ => return None,
    })
}

fn determinant4(values: [f64; 16]) -> f64 {
    let mut matrix = [[0.0; 4]; 4];
    for row in 0..4 {
        for column in 0..4 {
            matrix[row][column] = values[column * 4 + row];
        }
    }
    let mut determinant = 1.0;
    for column in 0..4 {
        let pivot = (column..4).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        });
        let Some(pivot) = pivot else {
            return 0.0;
        };
        if matrix[pivot][column].abs() < 1e-12 {
            return 0.0;
        }
        if pivot != column {
            matrix.swap(pivot, column);
            determinant = -determinant;
        }
        let value = matrix[column][column];
        determinant *= value;
        let pivot_row = matrix[column];
        for row in matrix.iter_mut().skip(column + 1) {
            let factor = row[column] / value;
            for (current, entry) in row.iter_mut().enumerate().skip(column + 1) {
                *entry -= factor * pivot_row[current];
            }
        }
    }
    determinant
}

fn approximately_equal(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() <= tolerance * left.abs().max(right.abs()).max(1.0)
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

fn finding(
    findings: &mut Vec<RuntimeFinding>,
    code: RuntimeFindingCode,
    subject: impl Into<String>,
    detail: impl Into<String>,
) {
    findings.push(RuntimeFinding {
        code,
        subject: subject.into(),
        detail: detail.into(),
    });
}
