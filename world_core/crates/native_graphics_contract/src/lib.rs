//! Typed, engine-neutral input and evidence contracts for the native WGE
//! graphics path.
//!
//! This crate owns no Vulkan handles and imports no renderer-specific types.
//! It lowers an already validated reference world into one coarse packet that
//! a supervised graphics worker can consume and independently revalidate.

use std::collections::BTreeSet;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use wge_reference_runtime::{ReferenceCamera, WorldArtifact, validate_world_artifact};

pub const SCENE_PACKET_SCHEMA: &str = "wge.graphics-scene-packet/v5";
pub const READY_SCHEMA: &str = "wge.graphics-ready/v1";
pub const FRAME_RECEIPT_SCHEMA: &str = "wge.graphics-frame-receipt/v1";
pub const ADAPTER_REVISION: &str = "wge.lava-adapter/v2";
pub const LAVA_BACKEND_ID: &str = "lava-vulkan";
pub const LAVA_REVISION: &str = "11c7e31bdf62408d22bf379e9e59510f69d2103e";
pub const MAX_PACKET_ELEMENTS: usize = 16 * 1024 * 1024;
pub const MAX_CAPTURE_DIMENSION: u32 = 8192;
pub const MAX_CAPTURE_BYTES: usize = 32 * 1024 * 1024;
const MAX_TELEMETRY_COUNTER: usize = 1 << 40;
const MAX_FRAME_TIME_US: u64 = 60_000_000;
const MIN_NATIVE_LUMINANCE_STDDEV: f64 = 0.01;
const MIN_NATIVE_DISTINCT_COLORS: usize = 3;

pub mod supervisor;

pub use supervisor::{
    GraphicsFrameOutput, GraphicsWorkerError, GraphicsWorkerSupervisor, PromotedFrame,
    WORKER_SCHEMA,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphicsContractError {
    pub code: &'static str,
    pub message: String,
}

impl GraphicsContractError {
    fn malformed(message: impl Into<String>) -> Self {
        Self {
            code: "malformed",
            message: message.into(),
        }
    }

    fn provenance(message: impl Into<String>) -> Self {
        Self {
            code: "provenance",
            message: message.into(),
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: "unsupported",
            message: message.into(),
        }
    }
}

impl std::fmt::Display for GraphicsContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for GraphicsContractError {}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsScenePacket {
    pub body: GraphicsScenePacketBody,
    pub packet_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsScenePacketBody {
    pub schema_version: String,
    pub packet_id: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub spatial_fields_sha256: String,
    pub frame_seed: u64,
    pub coordinate_system: CoordinateSystem,
    pub camera: GraphicsCamera,
    pub terrain: TerrainPacket,
    pub materials: Vec<MaterialIntent>,
    pub textures: Vec<TextureReference>,
    pub meshes: Vec<MeshPacket>,
    pub instances: Vec<InstancePacket>,
    pub lights: Vec<LightIntent>,
    pub environment: EnvironmentIntent,
    pub overlays: Vec<SemanticOverlay>,
    pub capture: GraphicsCaptureRequest,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Handedness {
    Right,
    Left,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CoordinateSystem {
    pub up_axis: Axis,
    pub handedness: Handedness,
    pub units_per_meter: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsCamera {
    pub camera_id: String,
    pub projection: CameraProjection,
    pub position_xyz_m: [f32; 3],
    pub forward_xyz: [f32; 3],
    pub up_xyz: [f32; 3],
    pub near_plane_m: f32,
    pub far_plane_m: f32,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CameraProjection {
    Perspective { fov_y_degrees: f32 },
    Orthographic { span_m: f32 },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TerrainPacket {
    pub terrain_id: String,
    pub width_m: f32,
    pub length_m: f32,
    pub resolution: usize,
    pub material_id: String,
    pub heights_m: BufferReference,
    pub slope_grade: BufferReference,
    pub region_codes: BufferReference,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BufferReference {
    pub buffer_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_artifact_id: Option<String>,
    pub byte_length: usize,
    pub count: usize,
    pub stride_bytes: usize,
    pub sha256: String,
    pub payload: BufferPayload,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "encoding", content = "values", rename_all = "snake_case")]
pub enum BufferPayload {
    F32(Vec<f32>),
    U8(Vec<u8>),
    U32(Vec<u32>),
}

impl BufferReference {
    pub fn inline_f32(buffer_id: impl Into<String>, values: Vec<f32>) -> Self {
        Self::from_payload(buffer_id.into(), BufferPayload::F32(values))
    }

    pub fn inline_u8(buffer_id: impl Into<String>, values: Vec<u8>) -> Self {
        Self::from_payload(buffer_id.into(), BufferPayload::U8(values))
    }

    pub fn inline_u32(buffer_id: impl Into<String>, values: Vec<u32>) -> Self {
        Self::from_payload(buffer_id.into(), BufferPayload::U32(values))
    }

    fn from_payload(buffer_id: String, payload: BufferPayload) -> Self {
        let (byte_length, count, stride_bytes) = payload.layout();
        let sha256 = sha256_prefixed(&payload.le_bytes());
        Self {
            buffer_id,
            source_artifact_id: None,
            byte_length,
            count,
            stride_bytes,
            sha256,
            payload,
        }
    }
}

impl BufferPayload {
    fn layout(&self) -> (usize, usize, usize) {
        match self {
            Self::F32(values) => (values.len() * 4, values.len(), 4),
            Self::U8(values) => (values.len(), values.len(), 1),
            Self::U32(values) => (values.len() * 4, values.len(), 4),
        }
    }

    fn le_bytes(&self) -> Vec<u8> {
        match self {
            Self::F32(values) => values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect(),
            Self::U8(values) => values.clone(),
            Self::U32(values) => values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect(),
        }
    }

    fn finite(&self) -> bool {
        match self {
            Self::F32(values) => values.iter().all(|value| value.is_finite()),
            Self::U8(_) | Self::U32(_) => true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MaterialIntent {
    pub material_id: String,
    pub base_color_rgba: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub alpha_mode: AlphaMode,
    pub texture_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_texture_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness_texture_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occlusion_texture_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emissive_texture_id: Option<String>,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    pub emissive_factor_rgb: [f32; 3],
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextureReference {
    pub texture_id: String,
    pub source_artifact_id: String,
    pub sha256: String,
    pub width_px: u32,
    pub height_px: u32,
    pub mip_levels: u32,
    pub color_space: TextureColorSpace,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<TexturePayload>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "encoding", content = "base64", rename_all = "snake_case")]
pub enum TexturePayload {
    Rgba8(String),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextureColorSpace {
    Srgb,
    Linear,
    NormalMap,
    Data,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MeshPacket {
    pub mesh_id: String,
    pub positions_m: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv0: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub material_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InstancePacket {
    pub instance_id: String,
    pub mesh_id: String,
    pub material_id: String,
    pub importance: InstanceImportance,
    pub transform: Transform3d,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstanceImportance {
    Background,
    Landmark,
    GameplayCritical,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Transform3d {
    pub translation_xyz_m: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub scale_xyz: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LightIntent {
    pub light_id: String,
    pub kind: LightKind,
    pub color_rgb: [f32; 3],
    pub intensity: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentIntent {
    pub sky_top_rgb: [f32; 3],
    pub sky_horizon_rgb: [f32; 3],
    pub ground_rgb: [f32; 3],
    pub fog_color_rgb: [f32; 3],
    pub fog_density: f32,
    pub exposure: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LightKind {
    Directional {
        direction_xyz: [f32; 3],
    },
    Point {
        position_xyz_m: [f32; 3],
        range_m: f32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticOverlay {
    Point {
        marker_id: String,
        role: MarkerRole,
        position_xyz_m: [f32; 3],
        radius_m: f32,
        color_rgba: [f32; 4],
    },
    Circle {
        marker_id: String,
        role: MarkerRole,
        center_xyz_m: [f32; 3],
        radius_m: f32,
        color_rgba: [f32; 4],
    },
    Polyline {
        marker_id: String,
        role: MarkerRole,
        points_xyz_m: Vec<[f32; 3]>,
        thickness_m: f32,
        color_rgba: [f32; 4],
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarkerRole {
    Route,
    PlayerSpawn,
    OpponentSpawn,
    Encounter,
    Objective,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsCaptureRequest {
    pub capture_id: String,
    pub camera_id: String,
    pub width_px: u32,
    pub height_px: u32,
    pub format: CaptureFormat,
    pub include_depth: bool,
    pub deterministic: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureFormat {
    Rgba8Srgb,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsReady {
    pub schema_version: String,
    pub backend_id: String,
    pub adapter_revision: String,
    pub lava_revision: String,
    pub julia_version: String,
    pub vulkan_api_version: String,
    pub device_name: String,
    pub device_uuid: String,
    pub features: GraphicsFeatures,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsFeatures {
    pub offscreen_raster: bool,
    pub depth_attachment: bool,
    pub texture_sampling: bool,
    pub readback: bool,
    pub hardware_ray_tracing: bool,
    pub gpu_timestamps: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsFrameReceipt {
    pub body: GraphicsFrameReceiptBody,
    pub receipt_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsFrameReceiptBody {
    pub schema_version: String,
    pub packet_sha256: String,
    pub capture_id: String,
    pub backend_id: String,
    pub adapter_revision: String,
    pub lava_revision: String,
    pub device_uuid: String,
    pub worker_script_sha256: String,
    pub renderer_identity_sha256: String,
    pub status: FrameStatus,
    pub format: CaptureFormat,
    pub width_px: u32,
    pub height_px: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_sha256: Option<String>,
    pub measurements: GraphicsFrameMeasurements,
    pub telemetry: GraphicsTelemetry,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrameStatus {
    Passed,
    Failed,
    Unsupported,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsFrameMeasurements {
    pub terrain_luminance_stddev: f64,
    pub distinct_terrain_colors: usize,
    pub route_visible_pixels: usize,
    pub player_spawn_visible_pixels: usize,
    pub opponent_spawn_visible_pixels: usize,
    pub encounter_visible_pixels: usize,
    pub objective_visible_pixels: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsTelemetry {
    pub upload_bytes: usize,
    pub readback_bytes: usize,
    pub draw_calls: usize,
    pub dispatch_calls: usize,
    pub pipeline_compilations: usize,
    pub instance_count: usize,
    pub visible_instance_count: usize,
    pub culled_instance_count: usize,
    pub background_visible_instance_count: usize,
    pub background_culled_instance_count: usize,
    pub landmark_visible_instance_count: usize,
    pub landmark_culled_instance_count: usize,
    pub gameplay_critical_visible_instance_count: usize,
    pub gameplay_critical_culled_instance_count: usize,
    pub terrain_vertex_count: usize,
    pub mesh_vertex_count: usize,
    pub frame_time_us: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_frame_time_us: Option<u64>,
}

pub fn sha256_prefixed(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    format!("sha256:{hex}")
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, GraphicsContractError> {
    serde_json::to_vec(value).map_err(|error| {
        GraphicsContractError::malformed(format!("canonical JSON failed: {error}"))
    })
}

pub fn seal_scene_packet(
    body: GraphicsScenePacketBody,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    let packet_sha256 = sha256_prefixed(&canonical_json(&body)?);
    let packet = GraphicsScenePacket {
        body,
        packet_sha256,
    };
    validate_scene_packet(&packet)?;
    Ok(packet)
}

pub fn validate_scene_packet(packet: &GraphicsScenePacket) -> Result<(), GraphicsContractError> {
    let body_digest = sha256_prefixed(&canonical_json(&packet.body)?);
    if packet.packet_sha256 != body_digest {
        return Err(GraphicsContractError::provenance(
            "scene packet digest does not match its canonical body",
        ));
    }
    if packet.body.schema_version != SCENE_PACKET_SCHEMA {
        return Err(GraphicsContractError::unsupported(format!(
            "unsupported scene packet schema {}",
            packet.body.schema_version
        )));
    }
    valid_id(&packet.body.packet_id, "packet_id")?;
    valid_id(&packet.body.world_artifact_id, "world_artifact_id")?;
    valid_sha(&packet.body.world_artifact_sha256, "world_artifact_sha256")?;
    valid_sha(&packet.body.spatial_fields_sha256, "spatial_fields_sha256")?;
    validate_coordinate_system(&packet.body.coordinate_system)?;
    validate_camera(&packet.body.camera)?;
    validate_terrain(&packet.body.terrain, &packet.body.materials)?;

    let mut material_ids = BTreeSet::new();
    for material in &packet.body.materials {
        validate_material(material)?;
        if !material_ids.insert(material.material_id.as_str()) {
            return Err(GraphicsContractError::malformed(format!(
                "duplicate material {}",
                material.material_id
            )));
        }
    }
    let mut texture_ids = BTreeSet::new();
    for texture in &packet.body.textures {
        validate_texture(texture)?;
        if !texture_ids.insert(texture.texture_id.as_str()) {
            return Err(GraphicsContractError::malformed(format!(
                "duplicate texture {}",
                texture.texture_id
            )));
        }
    }
    for material in &packet.body.materials {
        for texture_id in &material.texture_ids {
            if !texture_ids.contains(texture_id.as_str()) {
                return Err(GraphicsContractError::provenance(format!(
                    "material {} references unknown texture {}",
                    material.material_id, texture_id
                )));
            }
        }
        for (role, texture_id) in [
            ("normal_texture_id", material.normal_texture_id.as_deref()),
            (
                "roughness_texture_id",
                material.roughness_texture_id.as_deref(),
            ),
            (
                "occlusion_texture_id",
                material.occlusion_texture_id.as_deref(),
            ),
            (
                "emissive_texture_id",
                material.emissive_texture_id.as_deref(),
            ),
        ] {
            if let Some(texture_id) = texture_id {
                valid_id(texture_id, role)?;
                if !texture_ids.contains(texture_id) {
                    return Err(GraphicsContractError::provenance(format!(
                        "material {} references unknown {} {}",
                        material.material_id, role, texture_id
                    )));
                }
            }
        }
    }

    let mut mesh_ids = BTreeSet::new();
    for mesh in &packet.body.meshes {
        validate_mesh(mesh, &material_ids)?;
        if !mesh_ids.insert(mesh.mesh_id.as_str()) {
            return Err(GraphicsContractError::malformed(format!(
                "duplicate mesh {}",
                mesh.mesh_id
            )));
        }
    }
    let mut instance_ids = BTreeSet::new();
    for instance in &packet.body.instances {
        valid_id(&instance.instance_id, "instance_id")?;
        if !mesh_ids.contains(instance.mesh_id.as_str()) {
            return Err(GraphicsContractError::provenance(format!(
                "instance {} references unknown mesh {}",
                instance.instance_id, instance.mesh_id
            )));
        }
        if !material_ids.contains(instance.material_id.as_str()) {
            return Err(GraphicsContractError::provenance(format!(
                "instance {} references unknown material {}",
                instance.instance_id, instance.material_id
            )));
        }
        validate_transform(&instance.transform)?;
        if !instance_ids.insert(instance.instance_id.as_str()) {
            return Err(GraphicsContractError::malformed(format!(
                "duplicate instance {}",
                instance.instance_id
            )));
        }
    }

    if packet.body.lights.is_empty() {
        return Err(GraphicsContractError::malformed(
            "scene packet needs at least one light intent",
        ));
    }
    let mut light_ids = BTreeSet::new();
    for light in &packet.body.lights {
        validate_light(light)?;
        if !light_ids.insert(light.light_id.as_str()) {
            return Err(GraphicsContractError::malformed(format!(
                "duplicate light {}",
                light.light_id
            )));
        }
    }
    validate_environment(&packet.body.environment)?;
    let mut marker_ids = BTreeSet::new();
    for overlay in &packet.body.overlays {
        let marker_id = validate_overlay(overlay)?;
        if !marker_ids.insert(marker_id) {
            return Err(GraphicsContractError::malformed(format!(
                "duplicate semantic overlay {marker_id}"
            )));
        }
    }
    validate_capture(&packet.body.capture, &packet.body.camera)?;
    Ok(())
}

pub fn seal_frame_receipt(
    body: GraphicsFrameReceiptBody,
) -> Result<GraphicsFrameReceipt, GraphicsContractError> {
    let receipt_sha256 = sha256_prefixed(&canonical_json(&body)?);
    let receipt = GraphicsFrameReceipt {
        body,
        receipt_sha256,
    };
    validate_frame_receipt_shape(&receipt)?;
    Ok(receipt)
}

pub fn seal_frame_receipt_with_capture(
    body: GraphicsFrameReceiptBody,
    capture_bytes: &[u8],
) -> Result<GraphicsFrameReceipt, GraphicsContractError> {
    let receipt = seal_frame_receipt(body)?;
    validate_frame_receipt(&receipt, capture_bytes)?;
    Ok(receipt)
}

pub fn validate_frame_receipt(
    receipt: &GraphicsFrameReceipt,
    capture_bytes: &[u8],
) -> Result<(), GraphicsContractError> {
    validate_frame_receipt_shape(receipt)?;
    match receipt.body.status {
        FrameStatus::Passed => {
            if capture_bytes.is_empty() {
                return Err(GraphicsContractError::provenance(
                    "passed frame receipt has no capture bytes",
                ));
            }
            let declared = receipt.body.capture_sha256.as_deref().ok_or_else(|| {
                GraphicsContractError::provenance("passed frame receipt has no capture digest")
            })?;
            if declared != sha256_prefixed(capture_bytes) {
                return Err(GraphicsContractError::provenance(
                    "capture bytes do not match frame receipt digest",
                ));
            }
        }
        FrameStatus::Failed | FrameStatus::Unsupported => {
            if !capture_bytes.is_empty() {
                return Err(GraphicsContractError::provenance(
                    "failed or unsupported frame cannot carry capture bytes",
                ));
            }
        }
    }
    Ok(())
}

pub fn measure_frame_capture(
    packet: &GraphicsScenePacket,
    capture_bytes: &[u8],
) -> Result<GraphicsFrameMeasurements, GraphicsContractError> {
    validate_scene_packet(packet)?;
    let width = usize::try_from(packet.body.capture.width_px)
        .map_err(|_| GraphicsContractError::malformed("capture width overflows usize"))?;
    let height = usize::try_from(packet.body.capture.height_px)
        .map_err(|_| GraphicsContractError::malformed("capture height overflows usize"))?;
    let expected_bytes = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            GraphicsContractError::malformed("capture dimensions overflow byte length")
        })?;
    if capture_bytes.len() != expected_bytes {
        return Err(GraphicsContractError::provenance(format!(
            "capture has {} bytes, expected {expected_bytes}",
            capture_bytes.len()
        )));
    }

    let mut distinct_colors = BTreeSet::new();
    let mut role_colors: [BTreeSet<[u8; 4]>; 5] = std::array::from_fn(|_| BTreeSet::new());
    for overlay in &packet.body.overlays {
        let index = marker_role_index(overlay_role(overlay));
        role_colors[index].insert(rgba8(overlay_color(overlay))?);
    }

    let mut role_pixels = [0usize; 5];
    let mut mean = 0.0f64;
    let mut sum_squared_delta = 0.0f64;
    let mut sample_count = 0.0f64;
    for pixel in capture_bytes.chunks_exact(4) {
        let rgba = [pixel[0], pixel[1], pixel[2], pixel[3]];
        distinct_colors.insert((rgba[0], rgba[1], rgba[2]));
        let luminance = 0.2126 * f64::from(rgba[0]) / 255.0
            + 0.7152 * f64::from(rgba[1]) / 255.0
            + 0.0722 * f64::from(rgba[2]) / 255.0;
        sample_count += 1.0;
        let delta = luminance - mean;
        mean += delta / sample_count;
        sum_squared_delta += delta * (luminance - mean);
        for (index, colors) in role_colors.iter().enumerate() {
            if colors.contains(&rgba) {
                role_pixels[index] += 1;
            }
        }
    }
    let variance = sum_squared_delta / sample_count;
    let measurements = GraphicsFrameMeasurements {
        terrain_luminance_stddev: variance.sqrt(),
        distinct_terrain_colors: distinct_colors.len(),
        route_visible_pixels: role_pixels[marker_role_index(MarkerRole::Route)],
        player_spawn_visible_pixels: role_pixels[marker_role_index(MarkerRole::PlayerSpawn)],
        opponent_spawn_visible_pixels: role_pixels[marker_role_index(MarkerRole::OpponentSpawn)],
        encounter_visible_pixels: role_pixels[marker_role_index(MarkerRole::Encounter)],
        objective_visible_pixels: role_pixels[marker_role_index(MarkerRole::Objective)],
    };
    validate_measurements(&measurements)?;
    validate_measurement_counts(&measurements, expected_bytes / 4)?;
    Ok(measurements)
}

pub fn validate_native_visual_gate(
    packet: &GraphicsScenePacket,
    measurements: &GraphicsFrameMeasurements,
) -> Result<(), GraphicsContractError> {
    validate_scene_packet(packet)?;
    validate_measurements(measurements)?;
    let pixel_count = usize::try_from(packet.body.capture.width_px)
        .ok()
        .and_then(|width| {
            usize::try_from(packet.body.capture.height_px)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| {
            GraphicsContractError::malformed("capture dimensions overflow pixel count")
        })?;
    validate_measurement_counts(measurements, pixel_count)?;
    if measurements.terrain_luminance_stddev < MIN_NATIVE_LUMINANCE_STDDEV {
        return Err(GraphicsContractError::provenance(
            "native visual gate failed: terrain capture is visually flat",
        ));
    }
    if measurements.distinct_terrain_colors < MIN_NATIVE_DISTINCT_COLORS {
        return Err(GraphicsContractError::provenance(
            "native visual gate failed: terrain capture lacks useful color diversity",
        ));
    }
    for role in [
        MarkerRole::Route,
        MarkerRole::PlayerSpawn,
        MarkerRole::OpponentSpawn,
        MarkerRole::Encounter,
        MarkerRole::Objective,
    ] {
        if packet
            .body
            .overlays
            .iter()
            .any(|overlay| overlay_role(overlay) == role)
            && role_visible_pixels(measurements, role) == 0
        {
            return Err(GraphicsContractError::provenance(format!(
                "native visual gate failed: {role:?} overlay is not visible"
            )));
        }
    }
    Ok(())
}

fn marker_role_index(role: MarkerRole) -> usize {
    match role {
        MarkerRole::Route => 0,
        MarkerRole::PlayerSpawn => 1,
        MarkerRole::OpponentSpawn => 2,
        MarkerRole::Encounter => 3,
        MarkerRole::Objective => 4,
    }
}

fn role_visible_pixels(measurements: &GraphicsFrameMeasurements, role: MarkerRole) -> usize {
    match role {
        MarkerRole::Route => measurements.route_visible_pixels,
        MarkerRole::PlayerSpawn => measurements.player_spawn_visible_pixels,
        MarkerRole::OpponentSpawn => measurements.opponent_spawn_visible_pixels,
        MarkerRole::Encounter => measurements.encounter_visible_pixels,
        MarkerRole::Objective => measurements.objective_visible_pixels,
    }
}

fn overlay_role(overlay: &SemanticOverlay) -> MarkerRole {
    match overlay {
        SemanticOverlay::Point { role, .. }
        | SemanticOverlay::Circle { role, .. }
        | SemanticOverlay::Polyline { role, .. } => *role,
    }
}

fn overlay_color(overlay: &SemanticOverlay) -> &[f32; 4] {
    match overlay {
        SemanticOverlay::Point { color_rgba, .. }
        | SemanticOverlay::Circle { color_rgba, .. }
        | SemanticOverlay::Polyline { color_rgba, .. } => color_rgba,
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn rgba8(color: &[f32; 4]) -> Result<[u8; 4], GraphicsContractError> {
    color
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if !value.is_finite() || !(0.0..=1.0).contains(value) {
                return Err(GraphicsContractError::malformed(
                    "visual measurement color is outside [0, 1]",
                ));
            }
            let encoded = if index < 3 {
                linear_to_srgb(*value)
            } else {
                *value
            };
            Ok((encoded * 255.0).round() as u8)
        })
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| GraphicsContractError::malformed("visual measurement color has wrong arity"))
}

fn validate_frame_receipt_shape(
    receipt: &GraphicsFrameReceipt,
) -> Result<(), GraphicsContractError> {
    let body_digest = sha256_prefixed(&canonical_json(&receipt.body)?);
    if receipt.receipt_sha256 != body_digest {
        return Err(GraphicsContractError::provenance(
            "frame receipt digest does not match its canonical body",
        ));
    }
    let body = &receipt.body;
    if body.schema_version != FRAME_RECEIPT_SCHEMA {
        return Err(GraphicsContractError::unsupported(format!(
            "unsupported frame receipt schema {}",
            body.schema_version
        )));
    }
    valid_sha(&body.packet_sha256, "packet_sha256")?;
    valid_id(&body.capture_id, "capture_id")?;
    valid_id(&body.backend_id, "backend_id")?;
    valid_id(&body.adapter_revision, "adapter_revision")?;
    valid_id(&body.lava_revision, "lava_revision")?;
    valid_id(&body.device_uuid, "device_uuid")?;
    valid_sha(&body.worker_script_sha256, "worker_script_sha256")?;
    valid_sha(&body.renderer_identity_sha256, "renderer_identity_sha256")?;
    validate_dimensions(body.width_px, body.height_px, "frame receipt")?;
    validate_measurements(&body.measurements)?;
    let pixel_count = usize::try_from(body.width_px)
        .ok()
        .and_then(|width| {
            usize::try_from(body.height_px)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| GraphicsContractError::malformed("frame dimensions overflow pixel count"))?;
    validate_measurement_counts(&body.measurements, pixel_count)?;
    validate_telemetry(&body.telemetry)?;
    if body.detail.trim().is_empty() {
        return Err(GraphicsContractError::malformed(
            "frame receipt detail must not be empty",
        ));
    }
    match body.status {
        FrameStatus::Passed => {
            let declared = body.capture_sha256.as_deref().ok_or_else(|| {
                GraphicsContractError::provenance("passed frame receipt has no capture digest")
            })?;
            valid_sha(declared, "capture_sha256")?;
        }
        FrameStatus::Failed | FrameStatus::Unsupported => {
            if body.capture_sha256.is_some() {
                return Err(GraphicsContractError::provenance(
                    "failed or unsupported frame cannot carry a capture digest",
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_ready(ready: &GraphicsReady) -> Result<(), GraphicsContractError> {
    if ready.schema_version != READY_SCHEMA {
        return Err(GraphicsContractError::unsupported(format!(
            "unsupported ready schema {}",
            ready.schema_version
        )));
    }
    for (value, label) in [
        (&ready.backend_id, "backend_id"),
        (&ready.adapter_revision, "adapter_revision"),
        (&ready.lava_revision, "lava_revision"),
        (&ready.julia_version, "julia_version"),
        (&ready.vulkan_api_version, "vulkan_api_version"),
        (&ready.device_name, "device_name"),
        (&ready.device_uuid, "device_uuid"),
    ] {
        if value.trim().is_empty() {
            return Err(GraphicsContractError::malformed(format!(
                "ready {label} must not be empty"
            )));
        }
    }
    if !ready.features.offscreen_raster
        || !ready.features.depth_attachment
        || !ready.features.texture_sampling
        || !ready.features.readback
    {
        return Err(GraphicsContractError::unsupported(
            "ready backend lacks the required offscreen raster profile",
        ));
    }
    Ok(())
}

fn obstacle_mesh() -> MeshPacket {
    let mut positions = Vec::with_capacity(24);
    let mut normals = Vec::with_capacity(24);
    let mut uv0 = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    append_mesh_face(
        &mut positions,
        &mut normals,
        &mut uv0,
        &mut indices,
        [
            [-1.0, 0.0, -1.0],
            [1.0, 0.0, -1.0],
            [1.0, 0.0, 1.0],
            [-1.0, 0.0, 1.0],
        ],
        [0.0, -1.0, 0.0],
    );
    append_mesh_face(
        &mut positions,
        &mut normals,
        &mut uv0,
        &mut indices,
        [
            [-1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
        ],
        [0.0, 1.0, 0.0],
    );
    append_mesh_face(
        &mut positions,
        &mut normals,
        &mut uv0,
        &mut indices,
        [
            [-1.0, 0.0, -1.0],
            [-1.0, 1.0, -1.0],
            [1.0, 1.0, -1.0],
            [1.0, 0.0, -1.0],
        ],
        [0.0, 0.0, -1.0],
    );
    append_mesh_face(
        &mut positions,
        &mut normals,
        &mut uv0,
        &mut indices,
        [
            [1.0, 0.0, -1.0],
            [1.0, 1.0, -1.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
        ],
        [1.0, 0.0, 0.0],
    );
    append_mesh_face(
        &mut positions,
        &mut normals,
        &mut uv0,
        &mut indices,
        [
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
            [-1.0, 0.0, 1.0],
        ],
        [0.0, 0.0, 1.0],
    );
    append_mesh_face(
        &mut positions,
        &mut normals,
        &mut uv0,
        &mut indices,
        [
            [-1.0, 0.0, 1.0],
            [-1.0, 1.0, 1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, 0.0, -1.0],
        ],
        [-1.0, 0.0, 0.0],
    );
    MeshPacket {
        mesh_id: "obstacle-prism".into(),
        positions_m: positions,
        normals,
        uv0,
        indices,
        material_id: "obstacle-default".into(),
    }
}

fn foliage_mesh() -> MeshPacket {
    let mut positions = Vec::with_capacity(8);
    let mut normals = Vec::with_capacity(8);
    let mut uv0 = Vec::with_capacity(8);
    let mut indices = Vec::with_capacity(12);
    append_mesh_face(
        &mut positions,
        &mut normals,
        &mut uv0,
        &mut indices,
        [
            [-0.55, 0.0, 0.0],
            [0.55, 0.0, 0.0],
            [0.55, 1.8, 0.0],
            [-0.55, 1.8, 0.0],
        ],
        [0.0, 0.0, 1.0],
    );
    append_mesh_face(
        &mut positions,
        &mut normals,
        &mut uv0,
        &mut indices,
        [
            [0.0, 0.0, -0.55],
            [0.0, 0.0, 0.55],
            [0.0, 1.8, 0.55],
            [0.0, 1.8, -0.55],
        ],
        [1.0, 0.0, 0.0],
    );
    MeshPacket {
        mesh_id: "foliage-cross".into(),
        positions_m: positions,
        normals,
        uv0,
        indices,
        material_id: "foliage-default".into(),
    }
}

fn deterministic_foliage_instances(
    world: &WorldArtifact,
) -> Result<Vec<InstancePacket>, GraphicsContractError> {
    let layout = &world.body.authored_layout;
    let resolution = world.body.fields.resolution;
    let mut instances = Vec::new();
    for row in 0..5 {
        for column in 0..7 {
            let slot = row * 7 + column;
            let x = -layout.width_m * 0.42 + (column as f64 + 0.5) * layout.width_m * 0.84 / 7.0;
            let z = layout.length_m * 0.38 - (row as f64 + 0.5) * layout.length_m * 0.76 / 5.0;
            let cell = nearest_cell(layout.width_m, layout.length_m, resolution, [x, z]);
            if world.body.fields.region_codes[cell] != 0 {
                continue;
            }
            let route_clear = world.body.navigation.route_cells.iter().all(|route_cell| {
                let [route_x, route_z] =
                    cell_position(layout.width_m, layout.length_m, resolution, *route_cell);
                let dx = route_x - x;
                let dz = route_z - z;
                dx * dx + dz * dz > 16.0
            });
            if !route_clear {
                continue;
            }
            let obstacle_clear = layout.obstacles.iter().all(|obstacle| {
                let dx = obstacle.center_xz_m[0] - x;
                let dz = obstacle.center_xz_m[1] - z;
                dx * dx + dz * dz > (obstacle.radius_m + 1.5).powi(2)
            });
            if !obstacle_clear {
                continue;
            }
            let phase = (layout.seed.wrapping_add((slot as u64).wrapping_mul(37)) % 360) as f32
                * std::f32::consts::PI
                / 180.0;
            let height_scale = 0.85 + ((slot * 17) % 5) as f32 * 0.12;
            instances.push(InstancePacket {
                instance_id: format!("foliage-{slot:02}"),
                mesh_id: "foliage-cross".into(),
                material_id: "foliage-default".into(),
                importance: InstanceImportance::Background,
                transform: Transform3d {
                    translation_xyz_m: [
                        finite_f32(x, "foliage x")?,
                        finite_f32(world.body.fields.heights_m[cell], "foliage height")?,
                        finite_f32(z, "foliage z")?,
                    ],
                    rotation_xyzw: [0.0, (phase * 0.5).sin(), 0.0, (phase * 0.5).cos()],
                    scale_xyz: [
                        0.8 + height_scale * 0.15,
                        height_scale,
                        0.8 + height_scale * 0.15,
                    ],
                },
            });
        }
    }
    Ok(instances)
}

fn append_mesh_face(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uv0: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    face: [[f32; 3]; 4],
    normal: [f32; 3],
) {
    let first = positions.len() as u32;
    positions.extend(face);
    normals.extend([normal; 4]);
    uv0.extend([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);
}

fn procedural_texture(
    texture_id: &str,
    source_artifact_id: &str,
    color_space: TextureColorSpace,
    width_px: u32,
    height_px: u32,
    bytes: Vec<u8>,
) -> TextureReference {
    TextureReference {
        texture_id: texture_id.into(),
        source_artifact_id: source_artifact_id.into(),
        sha256: sha256_prefixed(&bytes),
        width_px,
        height_px,
        mip_levels: 1,
        color_space,
        payload: Some(TexturePayload::Rgba8(STANDARD.encode(bytes))),
    }
}

fn procedural_material_albedo_texture(
    texture_id: &str,
    source_artifact_id: &str,
    base: [u8; 3],
    accent: [u8; 3],
) -> TextureReference {
    let width = 8;
    let height = 8;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            let grain = ((row * 13 + column * 29 + row * column * 7) % 24) as u8;
            let vein = (row + 2 * column) % 7 == 0;
            let color = if vein {
                [
                    accent[0].saturating_add(grain / 2),
                    accent[1].saturating_add(grain / 2),
                    accent[2].saturating_add(grain / 3),
                    255,
                ]
            } else {
                [
                    base[0].saturating_add(grain),
                    base[1].saturating_add(grain / 2),
                    base[2].saturating_add(grain / 3),
                    255,
                ]
            };
            bytes.extend(color);
        }
    }
    procedural_texture(
        texture_id,
        source_artifact_id,
        TextureColorSpace::Srgb,
        width as u32,
        height as u32,
        bytes,
    )
}

fn procedural_terrain_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "riverwatch-terrain-albedo",
        "procedural-riverwatch-terrain-albedo-v1",
        [142, 126, 94],
        [112, 100, 76],
    )
}

fn procedural_stone_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "riverwatch-stone-albedo",
        "procedural-riverwatch-stone-albedo-v1",
        [128, 128, 124],
        [72, 73, 70],
    )
}

fn procedural_foliage_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "riverwatch-foliage-albedo",
        "procedural-riverwatch-foliage-albedo-v1",
        [50, 104, 28],
        [18, 53, 14],
    )
}

fn procedural_normal_texture() -> TextureReference {
    let width = 8;
    let height = 8;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            let x = 128 + (((row * 17 + column * 11) % 31) as i32 - 15);
            let y = 128 + (((row * 7 + column * 19) % 27) as i32 - 13);
            bytes.extend([x as u8, y as u8, 246, 255]);
        }
    }
    procedural_texture(
        "riverwatch-normal",
        "procedural-riverwatch-normal-v1",
        TextureColorSpace::NormalMap,
        width as u32,
        height as u32,
        bytes,
    )
}

fn procedural_roughness_texture() -> TextureReference {
    let width = 8;
    let height = 8;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            let value = 176 + ((row * 23 + column * 13) % 64) as u8;
            bytes.extend([value, value, value, 255]);
        }
    }
    procedural_texture(
        "riverwatch-roughness",
        "procedural-riverwatch-roughness-v1",
        TextureColorSpace::Data,
        width as u32,
        height as u32,
        bytes,
    )
}

fn procedural_occlusion_texture() -> TextureReference {
    let width = 8;
    let height = 8;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            let value = 208 + ((row * 11 + column * 5) % 32) as u8;
            bytes.extend([value, value, value, 255]);
        }
    }
    procedural_texture(
        "riverwatch-occlusion",
        "procedural-riverwatch-occlusion-v1",
        TextureColorSpace::Data,
        width as u32,
        height as u32,
        bytes,
    )
}

fn procedural_emissive_texture() -> TextureReference {
    let width = 8;
    let height = 8;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            let glow = (row * 5 + column * 3) % 19 == 0;
            bytes.extend(if glow {
                [220, 128, 32, 255]
            } else {
                [0, 0, 0, 255]
            });
        }
    }
    procedural_texture(
        "riverwatch-emissive",
        "procedural-riverwatch-emissive-v1",
        TextureColorSpace::Srgb,
        width as u32,
        height as u32,
        bytes,
    )
}

pub fn lower_reference_world(
    world: &WorldArtifact,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    validate_world_artifact(world)
        .map_err(|error| GraphicsContractError::provenance(error.to_string()))?;
    let layout = &world.body.authored_layout;
    let resolution = world.body.fields.resolution;
    let heights = world
        .body
        .fields
        .heights_m
        .iter()
        .map(|value| finite_f32(*value, "terrain height"))
        .collect::<Result<Vec<_>, _>>()?;
    let slope_grade = world
        .body
        .fields
        .slope_grade
        .iter()
        .map(|value| finite_f32(*value, "terrain slope"))
        .collect::<Result<Vec<_>, _>>()?;
    let terrain_material_id = "terrain-default".to_owned();
    let camera = lower_camera(&layout.reference_camera, layout.width_m, layout.length_m);
    let capture = GraphicsCaptureRequest {
        capture_id: format!("capture-{}", world.artifact_id.trim_start_matches("world-")),
        camera_id: camera.camera_id.clone(),
        width_px: camera.width_px,
        height_px: camera.height_px,
        format: CaptureFormat::Rgba8Srgb,
        include_depth: true,
        deterministic: true,
    };

    let mut overlays = Vec::new();
    let terrain_albedo_texture = procedural_terrain_albedo_texture();
    let stone_albedo_texture = procedural_stone_albedo_texture();
    let foliage_albedo_texture = procedural_foliage_albedo_texture();
    let normal_texture = procedural_normal_texture();
    let roughness_texture = procedural_roughness_texture();
    let occlusion_texture = procedural_occlusion_texture();
    let emissive_texture = procedural_emissive_texture();
    let normal_texture_id = Some(normal_texture.texture_id.clone());
    let roughness_texture_id = Some(roughness_texture.texture_id.clone());
    let occlusion_texture_id = Some(occlusion_texture.texture_id.clone());
    let emissive_texture_id = Some(emissive_texture.texture_id.clone());
    let mut materials = vec![MaterialIntent {
        material_id: "terrain-default".into(),
        base_color_rgba: [0.29, 0.38, 0.28, 1.0],
        metallic: 0.0,
        roughness: 0.92,
        alpha_mode: AlphaMode::Opaque,
        texture_ids: vec![terrain_albedo_texture.texture_id.clone()],
        normal_texture_id: normal_texture_id.clone(),
        roughness_texture_id: roughness_texture_id.clone(),
        occlusion_texture_id: occlusion_texture_id.clone(),
        emissive_texture_id: emissive_texture_id.clone(),
        normal_scale: 0.35,
        occlusion_strength: 0.65,
        emissive_factor_rgb: [0.0, 0.0, 0.0],
    }];
    let mut meshes = Vec::new();
    let mut instances = Vec::new();
    if !layout.obstacles.is_empty() {
        materials.push(MaterialIntent {
            material_id: "obstacle-default".into(),
            base_color_rgba: [0.27, 0.24, 0.20, 1.0],
            metallic: 0.35,
            roughness: 0.78,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![stone_albedo_texture.texture_id.clone()],
            normal_texture_id: normal_texture_id.clone(),
            roughness_texture_id: roughness_texture_id.clone(),
            occlusion_texture_id: occlusion_texture_id.clone(),
            emissive_texture_id: emissive_texture_id.clone(),
            normal_scale: 0.85,
            occlusion_strength: 0.8,
            emissive_factor_rgb: [0.035, 0.012, 0.002],
        });
        meshes.push(obstacle_mesh());
        for obstacle in &layout.obstacles {
            let [x, z] = obstacle.center_xz_m;
            let cell = nearest_cell(layout.width_m, layout.length_m, resolution, [x, z]);
            instances.push(InstancePacket {
                instance_id: obstacle.obstacle_id.clone(),
                mesh_id: "obstacle-prism".into(),
                material_id: "obstacle-default".into(),
                importance: InstanceImportance::GameplayCritical,
                transform: Transform3d {
                    translation_xyz_m: [
                        finite_f32(x, "obstacle x")?,
                        finite_f32(world.body.fields.heights_m[cell], "obstacle height")?,
                        finite_f32(z, "obstacle z")?,
                    ],
                    rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
                    scale_xyz: [
                        finite_f32(obstacle.radius_m, "obstacle radius")?,
                        finite_f32(obstacle.height_m, "obstacle height")?,
                        finite_f32(obstacle.radius_m, "obstacle radius")?,
                    ],
                },
            });
        }
    }
    let foliage_instances = deterministic_foliage_instances(world)?;
    if !foliage_instances.is_empty() {
        materials.push(MaterialIntent {
            material_id: "foliage-default".into(),
            base_color_rgba: [0.16, 0.34, 0.10, 1.0],
            metallic: 0.0,
            roughness: 0.88,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![foliage_albedo_texture.texture_id.clone()],
            normal_texture_id: normal_texture_id.clone(),
            roughness_texture_id: roughness_texture_id.clone(),
            occlusion_texture_id: occlusion_texture_id.clone(),
            emissive_texture_id: emissive_texture_id.clone(),
            normal_scale: 0.22,
            occlusion_strength: 0.5,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
        });
        meshes.push(foliage_mesh());
        instances.extend(foliage_instances);
    }
    let route_points = world
        .body
        .navigation
        .route_cells
        .iter()
        .map(|cell| {
            let [x, z] = cell_position(layout.width_m, layout.length_m, resolution, *cell);
            let height = world.body.fields.heights_m[*cell];
            Ok([
                finite_f32(x, "route x")?,
                finite_f32(height, "route height")?,
                finite_f32(z, "route z")?,
            ])
        })
        .collect::<Result<Vec<_>, GraphicsContractError>>()?;
    overlays.push(SemanticOverlay::Polyline {
        marker_id: "route".into(),
        role: MarkerRole::Route,
        points_xyz_m: route_points,
        thickness_m: 0.35,
        color_rgba: [0.05, 0.86, 0.91, 1.0],
    });
    for spawn in &world.body.spawns {
        let [x, z] = spawn.position_xz_m;
        let cell = spawn.grid_cell;
        let y = world.body.fields.heights_m[cell];
        let (role, color) = match spawn.role {
            wge_reference_runtime::SpawnRole::PlayerStart => {
                (MarkerRole::PlayerSpawn, [0.28, 0.94, 0.42, 1.0])
            }
            wge_reference_runtime::SpawnRole::Opponent => {
                (MarkerRole::OpponentSpawn, [0.88, 0.20, 0.47, 1.0])
            }
        };
        overlays.push(SemanticOverlay::Point {
            marker_id: spawn.spawn_id.clone(),
            role,
            position_xyz_m: [
                finite_f32(x, "spawn x")?,
                finite_f32(y, "spawn height")?,
                finite_f32(z, "spawn z")?,
            ],
            radius_m: 0.8,
            color_rgba: color,
        });
    }
    for encounter in &world.body.encounters {
        let [x, z] = encounter.center_xz_m;
        let cell = nearest_cell(layout.width_m, layout.length_m, resolution, [x, z]);
        overlays.push(SemanticOverlay::Circle {
            marker_id: encounter.encounter_id.clone(),
            role: MarkerRole::Encounter,
            center_xyz_m: [
                finite_f32(x, "encounter x")?,
                finite_f32(world.body.fields.heights_m[cell], "encounter height")?,
                finite_f32(z, "encounter z")?,
            ],
            radius_m: finite_f32(encounter.radius_m, "encounter radius")?,
            color_rgba: [0.93, 0.32, 0.28, 1.0],
        });
    }
    let [objective_x, objective_z] = layout.traversal.objective_position_xz_m;
    let objective_cell = nearest_cell(
        layout.width_m,
        layout.length_m,
        resolution,
        [objective_x, objective_z],
    );
    overlays.push(SemanticOverlay::Point {
        marker_id: layout.traversal.objective_id.clone(),
        role: MarkerRole::Objective,
        position_xyz_m: [
            finite_f32(objective_x, "objective x")?,
            finite_f32(
                world.body.fields.heights_m[objective_cell],
                "objective height",
            )?,
            finite_f32(objective_z, "objective z")?,
        ],
        radius_m: 1.0,
        color_rgba: [1.0, 0.72, 0.24, 1.0],
    });

    let body = GraphicsScenePacketBody {
        schema_version: SCENE_PACKET_SCHEMA.into(),
        packet_id: format!(
            "graphics-packet-{}",
            world.artifact_id.trim_start_matches("world-")
        ),
        world_artifact_id: world.artifact_id.clone(),
        world_artifact_sha256: world.artifact_sha256.clone(),
        spatial_fields_sha256: world.body.fields.spatial_sha256.clone(),
        frame_seed: layout.seed,
        coordinate_system: CoordinateSystem {
            up_axis: Axis::Y,
            handedness: Handedness::Right,
            units_per_meter: 1.0,
        },
        camera,
        terrain: TerrainPacket {
            terrain_id: layout.world_id.clone(),
            width_m: finite_f32(layout.width_m, "terrain width")?,
            length_m: finite_f32(layout.length_m, "terrain length")?,
            resolution,
            material_id: terrain_material_id.clone(),
            heights_m: BufferReference::inline_f32("terrain-heights", heights),
            slope_grade: BufferReference::inline_f32("terrain-slope", slope_grade),
            region_codes: BufferReference::inline_u8(
                "terrain-regions",
                world.body.fields.region_codes.clone(),
            ),
        },
        materials,
        textures: vec![
            terrain_albedo_texture,
            stone_albedo_texture,
            foliage_albedo_texture,
            normal_texture,
            roughness_texture,
            occlusion_texture,
            emissive_texture,
        ],
        meshes,
        instances,
        lights: vec![LightIntent {
            light_id: "key-directional".into(),
            kind: LightKind::Directional {
                direction_xyz: [0.35, -1.0, 0.25],
            },
            color_rgb: [1.0, 0.97, 0.92],
            intensity: 2.0,
        }],
        environment: EnvironmentIntent {
            sky_top_rgb: [0.08, 0.16, 0.30],
            sky_horizon_rgb: [0.48, 0.56, 0.62],
            ground_rgb: [0.16, 0.20, 0.16],
            fog_color_rgb: [0.46, 0.53, 0.58],
            fog_density: 0.006,
            exposure: 1.0,
        },
        overlays,
        capture,
    };
    seal_scene_packet(body)
}

fn validate_coordinate_system(
    coordinate_system: &CoordinateSystem,
) -> Result<(), GraphicsContractError> {
    if !coordinate_system.units_per_meter.is_finite()
        || coordinate_system.units_per_meter <= 0.0
        || (coordinate_system.units_per_meter - 1.0).abs() > f32::EPSILON
    {
        return Err(GraphicsContractError::unsupported(
            "native graphics requires finite one-unit-per-meter coordinates",
        ));
    }
    Ok(())
}

fn validate_camera(camera: &GraphicsCamera) -> Result<(), GraphicsContractError> {
    valid_id(&camera.camera_id, "camera_id")?;
    for (values, label) in [
        (&camera.position_xyz_m, "camera position"),
        (&camera.forward_xyz, "camera forward"),
        (&camera.up_xyz, "camera up"),
    ] {
        finite_values(values, label)?;
    }
    if camera.near_plane_m.is_nan()
        || camera.far_plane_m.is_nan()
        || camera.near_plane_m <= 0.0
        || camera.far_plane_m <= camera.near_plane_m
    {
        return Err(GraphicsContractError::malformed(
            "camera planes must be finite, positive, and ordered",
        ));
    }
    validate_dimensions(camera.width_px, camera.height_px, "camera")?;
    match camera.projection {
        CameraProjection::Perspective { fov_y_degrees } => {
            if !fov_y_degrees.is_finite() || !(1.0..=179.0).contains(&fov_y_degrees) {
                return Err(GraphicsContractError::malformed(
                    "perspective camera field of view is invalid",
                ));
            }
        }
        CameraProjection::Orthographic { span_m } => {
            if !span_m.is_finite() || span_m <= 0.0 {
                return Err(GraphicsContractError::malformed(
                    "orthographic camera span must be finite and positive",
                ));
            }
        }
    }
    Ok(())
}

fn validate_terrain(
    terrain: &TerrainPacket,
    materials: &[MaterialIntent],
) -> Result<(), GraphicsContractError> {
    valid_id(&terrain.terrain_id, "terrain_id")?;
    if !terrain.width_m.is_finite()
        || !terrain.length_m.is_finite()
        || terrain.width_m <= 0.0
        || terrain.length_m <= 0.0
    {
        return Err(GraphicsContractError::malformed(
            "terrain dimensions must be finite and positive",
        ));
    }
    if !(3..=2049).contains(&terrain.resolution)
        || terrain.resolution * terrain.resolution > MAX_PACKET_ELEMENTS
    {
        return Err(GraphicsContractError::malformed(
            "terrain resolution is outside the bounded graphics packet range",
        ));
    }
    valid_id(&terrain.material_id, "terrain material_id")?;
    if !materials
        .iter()
        .any(|material| material.material_id == terrain.material_id)
    {
        return Err(GraphicsContractError::provenance(
            "terrain references an unknown material",
        ));
    }
    let expected_count = terrain.resolution * terrain.resolution;
    validate_buffer(&terrain.heights_m, expected_count, "terrain heights")?;
    validate_buffer(&terrain.slope_grade, expected_count, "terrain slope")?;
    validate_buffer(&terrain.region_codes, expected_count, "terrain regions")?;
    Ok(())
}

fn validate_buffer(
    buffer: &BufferReference,
    expected_count: usize,
    label: &str,
) -> Result<(), GraphicsContractError> {
    valid_id(&buffer.buffer_id, label)?;
    if buffer.count != expected_count {
        return Err(GraphicsContractError::malformed(format!(
            "{label} count {} does not match expected {expected_count}",
            buffer.count
        )));
    }
    if buffer.payload.layout() != (buffer.byte_length, buffer.count, buffer.stride_bytes) {
        return Err(GraphicsContractError::provenance(format!(
            "{label} layout does not match its payload"
        )));
    }
    if buffer.byte_length == 0 || buffer.byte_length > MAX_PACKET_ELEMENTS * 4 {
        return Err(GraphicsContractError::malformed(format!(
            "{label} byte length is outside the bounded packet range"
        )));
    }
    if !buffer.payload.finite() {
        return Err(GraphicsContractError::malformed(format!(
            "{label} contains a non-finite value"
        )));
    }
    valid_sha(&buffer.sha256, &format!("{label} sha256"))?;
    if buffer.sha256 != sha256_prefixed(&buffer.payload.le_bytes()) {
        return Err(GraphicsContractError::provenance(format!(
            "{label} digest does not match its payload"
        )));
    }
    Ok(())
}

fn validate_material(material: &MaterialIntent) -> Result<(), GraphicsContractError> {
    valid_id(&material.material_id, "material_id")?;
    finite_values(&material.base_color_rgba, "material base color")?;
    if material
        .base_color_rgba
        .iter()
        .any(|value| !(0.0..=1.0).contains(value))
    {
        return Err(GraphicsContractError::malformed(
            "material base color must be in [0, 1]",
        ));
    }
    if !material.metallic.is_finite()
        || !material.roughness.is_finite()
        || !(0.0..=1.0).contains(&material.metallic)
        || !(0.0..=1.0).contains(&material.roughness)
    {
        return Err(GraphicsContractError::malformed(
            "material metallic and roughness must be in [0, 1]",
        ));
    }
    if !material.normal_scale.is_finite()
        || !(0.0..=2.0).contains(&material.normal_scale)
        || !material.occlusion_strength.is_finite()
        || !(0.0..=1.0).contains(&material.occlusion_strength)
    {
        return Err(GraphicsContractError::malformed(
            "material normal scale or occlusion strength is outside its bounds",
        ));
    }
    finite_values(&material.emissive_factor_rgb, "material emissive factor")?;
    if material
        .emissive_factor_rgb
        .iter()
        .any(|value| !(0.0..=16.0).contains(value))
    {
        return Err(GraphicsContractError::malformed(
            "material emissive factor must be in [0, 16]",
        ));
    }
    Ok(())
}

fn validate_environment(environment: &EnvironmentIntent) -> Result<(), GraphicsContractError> {
    for (color, label) in [
        (&environment.sky_top_rgb, "environment sky top"),
        (&environment.sky_horizon_rgb, "environment sky horizon"),
        (&environment.ground_rgb, "environment ground"),
        (&environment.fog_color_rgb, "environment fog color"),
    ] {
        finite_values(color, label)?;
        if color.iter().any(|value| !(0.0..=1.0).contains(value)) {
            return Err(GraphicsContractError::malformed(format!(
                "{label} must be in [0, 1]"
            )));
        }
    }
    if !environment.fog_density.is_finite()
        || !(0.0..=1.0).contains(&environment.fog_density)
        || !environment.exposure.is_finite()
        || !(0.01..=16.0).contains(&environment.exposure)
    {
        return Err(GraphicsContractError::malformed(
            "environment fog density or exposure is outside its bounds",
        ));
    }
    Ok(())
}

fn validate_texture(texture: &TextureReference) -> Result<(), GraphicsContractError> {
    valid_id(&texture.texture_id, "texture_id")?;
    valid_id(&texture.source_artifact_id, "texture source_artifact_id")?;
    valid_sha(&texture.sha256, "texture sha256")?;
    if texture.width_px == 0
        || texture.height_px == 0
        || texture.mip_levels == 0
        || texture.width_px > MAX_CAPTURE_DIMENSION
        || texture.height_px > MAX_CAPTURE_DIMENSION
    {
        return Err(GraphicsContractError::malformed(
            "texture dimensions or mip count are invalid",
        ));
    }
    if let Some(payload) = &texture.payload {
        if texture.mip_levels != 1 {
            return Err(GraphicsContractError::unsupported(
                "inline native texture payloads currently require one mip level",
            ));
        }
        let bytes = match payload {
            TexturePayload::Rgba8(encoded) => STANDARD.decode(encoded).map_err(|error| {
                GraphicsContractError::malformed(format!(
                    "texture {} payload is not valid base64: {error}",
                    texture.texture_id
                ))
            })?,
        };
        let expected_bytes = usize::try_from(texture.width_px)
            .ok()
            .and_then(|width| {
                usize::try_from(texture.height_px)
                    .ok()
                    .and_then(|height| width.checked_mul(height)?.checked_mul(4))
            })
            .ok_or_else(|| {
                GraphicsContractError::malformed(format!(
                    "texture {} dimensions overflow payload length",
                    texture.texture_id
                ))
            })?;
        if bytes.len() != expected_bytes {
            return Err(GraphicsContractError::provenance(format!(
                "texture {} payload has {} bytes, expected {expected_bytes}",
                texture.texture_id,
                bytes.len()
            )));
        }
        if texture.sha256 != sha256_prefixed(&bytes) {
            return Err(GraphicsContractError::provenance(format!(
                "texture {} payload digest does not match its metadata",
                texture.texture_id
            )));
        }
    }
    Ok(())
}

fn validate_mesh(
    mesh: &MeshPacket,
    material_ids: &BTreeSet<&str>,
) -> Result<(), GraphicsContractError> {
    valid_id(&mesh.mesh_id, "mesh_id")?;
    if mesh.positions_m.is_empty()
        || mesh.positions_m.len() != mesh.normals.len()
        || mesh.positions_m.len() != mesh.uv0.len()
    {
        return Err(GraphicsContractError::malformed(format!(
            "mesh {} needs matching non-empty position, normal, and uv0 arrays",
            mesh.mesh_id
        )));
    }
    if mesh.indices.is_empty() || !mesh.indices.len().is_multiple_of(3) {
        return Err(GraphicsContractError::malformed(format!(
            "mesh {} needs a non-empty triangle index array",
            mesh.mesh_id
        )));
    }
    if mesh
        .indices
        .iter()
        .any(|index| *index as usize >= mesh.positions_m.len())
    {
        return Err(GraphicsContractError::malformed(format!(
            "mesh {} contains an out-of-range index",
            mesh.mesh_id
        )));
    }
    for position in &mesh.positions_m {
        finite_values(position, "mesh position")?;
    }
    for normal in &mesh.normals {
        finite_values(normal, "mesh normal")?;
    }
    for uv in &mesh.uv0 {
        finite_values(uv, "mesh uv0")?;
    }
    if !material_ids.contains(mesh.material_id.as_str()) {
        return Err(GraphicsContractError::provenance(format!(
            "mesh {} references unknown material {}",
            mesh.mesh_id, mesh.material_id
        )));
    }
    Ok(())
}

fn validate_transform(transform: &Transform3d) -> Result<(), GraphicsContractError> {
    finite_values(&transform.translation_xyz_m, "instance translation")?;
    finite_values(&transform.rotation_xyzw, "instance rotation")?;
    finite_values(&transform.scale_xyz, "instance scale")?;
    let rotation_norm_squared = transform
        .rotation_xyzw
        .iter()
        .map(|value| value * value)
        .sum::<f32>();
    if rotation_norm_squared <= f32::EPSILON {
        return Err(GraphicsContractError::malformed(
            "instance rotation must be non-degenerate",
        ));
    }
    if transform.scale_xyz.iter().any(|value| *value <= 0.0) {
        return Err(GraphicsContractError::malformed(
            "instance scale must be positive",
        ));
    }
    Ok(())
}

fn validate_light(light: &LightIntent) -> Result<(), GraphicsContractError> {
    valid_id(&light.light_id, "light_id")?;
    finite_values(&light.color_rgb, "light color")?;
    if light.color_rgb.iter().any(|value| *value < 0.0)
        || !light.intensity.is_finite()
        || light.intensity < 0.0
    {
        return Err(GraphicsContractError::malformed(
            "light color and intensity must be finite and non-negative",
        ));
    }
    match &light.kind {
        LightKind::Directional { direction_xyz } => {
            finite_values(direction_xyz, "directional light direction")?;
        }
        LightKind::Point {
            position_xyz_m,
            range_m,
        } => {
            finite_values(position_xyz_m, "point light position")?;
            if !range_m.is_finite() || *range_m <= 0.0 {
                return Err(GraphicsContractError::malformed(
                    "point light range must be finite and positive",
                ));
            }
        }
    }
    Ok(())
}

fn validate_overlay(overlay: &SemanticOverlay) -> Result<&str, GraphicsContractError> {
    match overlay {
        SemanticOverlay::Point {
            marker_id,
            position_xyz_m,
            radius_m,
            color_rgba,
            ..
        } => {
            valid_id(marker_id, "marker_id")?;
            finite_values(position_xyz_m, "point marker position")?;
            validate_marker_style(*radius_m, color_rgba)?;
            Ok(marker_id)
        }
        SemanticOverlay::Circle {
            marker_id,
            center_xyz_m,
            radius_m,
            color_rgba,
            ..
        } => {
            valid_id(marker_id, "marker_id")?;
            finite_values(center_xyz_m, "circle marker center")?;
            validate_marker_style(*radius_m, color_rgba)?;
            Ok(marker_id)
        }
        SemanticOverlay::Polyline {
            marker_id,
            points_xyz_m,
            thickness_m,
            color_rgba,
            ..
        } => {
            valid_id(marker_id, "marker_id")?;
            if points_xyz_m.len() < 2 {
                return Err(GraphicsContractError::malformed(
                    "polyline marker needs at least two points",
                ));
            }
            for point in points_xyz_m {
                finite_values(point, "polyline marker point")?;
            }
            validate_marker_style(*thickness_m, color_rgba)?;
            Ok(marker_id)
        }
    }
}

fn validate_marker_style(
    radius_or_thickness: f32,
    color_rgba: &[f32; 4],
) -> Result<(), GraphicsContractError> {
    if !radius_or_thickness.is_finite() || radius_or_thickness <= 0.0 {
        return Err(GraphicsContractError::malformed(
            "marker radius/thickness must be finite and positive",
        ));
    }
    finite_values(color_rgba, "marker color")?;
    if color_rgba.iter().any(|value| !(0.0..=1.0).contains(value)) {
        return Err(GraphicsContractError::malformed(
            "marker color must be in [0, 1]",
        ));
    }
    Ok(())
}

fn validate_capture(
    capture: &GraphicsCaptureRequest,
    camera: &GraphicsCamera,
) -> Result<(), GraphicsContractError> {
    valid_id(&capture.capture_id, "capture_id")?;
    if capture.camera_id != camera.camera_id
        || capture.width_px != camera.width_px
        || capture.height_px != camera.height_px
    {
        return Err(GraphicsContractError::provenance(
            "capture request is detached from its packet camera",
        ));
    }
    validate_dimensions(capture.width_px, capture.height_px, "capture")?;
    if !capture.deterministic {
        return Err(GraphicsContractError::unsupported(
            "native certification requires deterministic captures",
        ));
    }
    let capture_bytes = usize::try_from(capture.width_px)
        .ok()
        .and_then(|width| {
            usize::try_from(capture.height_px)
                .ok()
                .and_then(|height| width.checked_mul(height)?.checked_mul(4))
        })
        .ok_or_else(|| {
            GraphicsContractError::malformed("capture dimensions overflow byte length")
        })?;
    if capture_bytes > MAX_CAPTURE_BYTES {
        return Err(GraphicsContractError::malformed(
            "capture byte length exceeds the bounded native frame size",
        ));
    }
    Ok(())
}

fn validate_measurements(
    measurements: &GraphicsFrameMeasurements,
) -> Result<(), GraphicsContractError> {
    if !measurements.terrain_luminance_stddev.is_finite()
        || !(0.0..=1.0).contains(&measurements.terrain_luminance_stddev)
    {
        return Err(GraphicsContractError::malformed(
            "frame luminance measurement must be finite and within [0, 1]",
        ));
    }
    Ok(())
}

fn validate_measurement_counts(
    measurements: &GraphicsFrameMeasurements,
    pixel_count: usize,
) -> Result<(), GraphicsContractError> {
    for (value, label) in [
        (
            measurements.distinct_terrain_colors,
            "distinct_terrain_colors",
        ),
        (measurements.route_visible_pixels, "route_visible_pixels"),
        (
            measurements.player_spawn_visible_pixels,
            "player_spawn_visible_pixels",
        ),
        (
            measurements.opponent_spawn_visible_pixels,
            "opponent_spawn_visible_pixels",
        ),
        (
            measurements.encounter_visible_pixels,
            "encounter_visible_pixels",
        ),
        (
            measurements.objective_visible_pixels,
            "objective_visible_pixels",
        ),
    ] {
        if value > pixel_count {
            return Err(GraphicsContractError::malformed(format!(
                "{label} exceeds capture pixel count"
            )));
        }
    }
    Ok(())
}

fn validate_telemetry(telemetry: &GraphicsTelemetry) -> Result<(), GraphicsContractError> {
    for (value, label) in [
        (telemetry.upload_bytes, "upload_bytes"),
        (telemetry.readback_bytes, "readback_bytes"),
        (telemetry.draw_calls, "draw_calls"),
        (telemetry.dispatch_calls, "dispatch_calls"),
        (telemetry.pipeline_compilations, "pipeline_compilations"),
        (telemetry.instance_count, "instance_count"),
        (telemetry.visible_instance_count, "visible_instance_count"),
        (telemetry.culled_instance_count, "culled_instance_count"),
        (
            telemetry.background_visible_instance_count,
            "background_visible_instance_count",
        ),
        (
            telemetry.background_culled_instance_count,
            "background_culled_instance_count",
        ),
        (
            telemetry.landmark_visible_instance_count,
            "landmark_visible_instance_count",
        ),
        (
            telemetry.landmark_culled_instance_count,
            "landmark_culled_instance_count",
        ),
        (
            telemetry.gameplay_critical_visible_instance_count,
            "gameplay_critical_visible_instance_count",
        ),
        (
            telemetry.gameplay_critical_culled_instance_count,
            "gameplay_critical_culled_instance_count",
        ),
        (telemetry.terrain_vertex_count, "terrain_vertex_count"),
        (telemetry.mesh_vertex_count, "mesh_vertex_count"),
    ] {
        if value > MAX_TELEMETRY_COUNTER {
            return Err(GraphicsContractError::malformed(format!(
                "telemetry {label} exceeds the bounded counter limit"
            )));
        }
    }
    if telemetry.frame_time_us > MAX_FRAME_TIME_US {
        return Err(GraphicsContractError::malformed(
            "telemetry frame time exceeds the bounded limit",
        ));
    }
    if telemetry
        .gpu_frame_time_us
        .is_some_and(|value| value > MAX_FRAME_TIME_US)
    {
        return Err(GraphicsContractError::malformed(
            "telemetry GPU frame time exceeds the bounded limit",
        ));
    }
    Ok(())
}

fn validate_dimensions(
    width_px: u32,
    height_px: u32,
    label: &str,
) -> Result<(), GraphicsContractError> {
    if width_px == 0
        || height_px == 0
        || width_px > MAX_CAPTURE_DIMENSION
        || height_px > MAX_CAPTURE_DIMENSION
    {
        return Err(GraphicsContractError::malformed(format!(
            "{label} dimensions are outside the bounded capture range"
        )));
    }
    Ok(())
}

fn valid_id(value: &str, label: &str) -> Result<(), GraphicsContractError> {
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(|character| character.is_whitespace())
    {
        return Err(GraphicsContractError::malformed(format!(
            "{label} is empty, too long, or contains whitespace"
        )));
    }
    Ok(())
}

fn valid_sha(value: &str, label: &str) -> Result<(), GraphicsContractError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(GraphicsContractError::malformed(format!(
            "{label} is not a sha256: digest"
        )));
    }
    Ok(())
}

fn finite_values<const N: usize>(
    values: &[f32; N],
    label: &str,
) -> Result<(), GraphicsContractError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(GraphicsContractError::malformed(format!(
            "{label} contains a non-finite value"
        )))
    }
}

fn finite_f32(value: f64, label: &str) -> Result<f32, GraphicsContractError> {
    if !value.is_finite() {
        return Err(GraphicsContractError::provenance(format!(
            "{label} is non-finite"
        )));
    }
    let value = value as f32;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(GraphicsContractError::provenance(format!(
            "{label} cannot be represented as finite f32"
        )))
    }
}

fn lower_camera(camera: &ReferenceCamera, width_m: f64, length_m: f64) -> GraphicsCamera {
    GraphicsCamera {
        camera_id: "reference-overview".into(),
        projection: CameraProjection::Orthographic {
            span_m: camera.orthographic_span_m as f32,
        },
        position_xyz_m: [0.0, camera.distance_m as f32, 0.0],
        forward_xyz: [0.0, -1.0, 0.0],
        up_xyz: [0.0, 0.0, -1.0],
        near_plane_m: 0.1,
        far_plane_m: (width_m.max(length_m) * 4.0 + camera.distance_m) as f32,
        width_px: camera.width_px,
        height_px: camera.height_px,
    }
}

fn cell_position(width_m: f64, length_m: f64, resolution: usize, cell: usize) -> [f64; 2] {
    let row = cell / resolution;
    let column = cell % resolution;
    let denominator = (resolution - 1) as f64;
    [
        -width_m / 2.0 + column as f64 * width_m / denominator,
        length_m / 2.0 - row as f64 * length_m / denominator,
    ]
}

fn nearest_cell(width_m: f64, length_m: f64, resolution: usize, point: [f64; 2]) -> usize {
    let denominator = (resolution - 1) as f64;
    let column = (((point[0] + width_m / 2.0) / width_m) * denominator)
        .round()
        .clamp(0.0, denominator) as usize;
    let row = (((length_m / 2.0 - point[1]) / length_m) * denominator)
        .round()
        .clamp(0.0, denominator) as usize;
    row * resolution + column
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet() -> GraphicsScenePacket {
        let camera = GraphicsCamera {
            camera_id: "camera".into(),
            projection: CameraProjection::Orthographic { span_m: 16.0 },
            position_xyz_m: [0.0, 10.0, 0.0],
            forward_xyz: [0.0, -1.0, 0.0],
            up_xyz: [0.0, 0.0, -1.0],
            near_plane_m: 0.1,
            far_plane_m: 100.0,
            width_px: 32,
            height_px: 32,
        };
        seal_scene_packet(GraphicsScenePacketBody {
            schema_version: SCENE_PACKET_SCHEMA.into(),
            packet_id: "packet-test".into(),
            world_artifact_id: "world-test".into(),
            world_artifact_sha256: sha256_prefixed(b"world"),
            spatial_fields_sha256: sha256_prefixed(b"fields"),
            frame_seed: 7,
            coordinate_system: CoordinateSystem {
                up_axis: Axis::Y,
                handedness: Handedness::Right,
                units_per_meter: 1.0,
            },
            camera: camera.clone(),
            terrain: TerrainPacket {
                terrain_id: "terrain".into(),
                width_m: 8.0,
                length_m: 8.0,
                resolution: 3,
                material_id: "terrain".into(),
                heights_m: BufferReference::inline_f32("heights", vec![0.0; 9]),
                slope_grade: BufferReference::inline_f32("slope", vec![0.0; 9]),
                region_codes: BufferReference::inline_u8("regions", vec![1; 9]),
            },
            materials: vec![MaterialIntent {
                material_id: "terrain".into(),
                base_color_rgba: [0.3, 0.4, 0.3, 1.0],
                metallic: 0.0,
                roughness: 0.9,
                alpha_mode: AlphaMode::Opaque,
                texture_ids: Vec::new(),
                normal_texture_id: None,
                roughness_texture_id: None,
                occlusion_texture_id: None,
                emissive_texture_id: None,
                normal_scale: 1.0,
                occlusion_strength: 1.0,
                emissive_factor_rgb: [0.0, 0.0, 0.0],
            }],
            textures: Vec::new(),
            meshes: Vec::new(),
            instances: Vec::new(),
            lights: vec![LightIntent {
                light_id: "sun".into(),
                kind: LightKind::Directional {
                    direction_xyz: [0.0, -1.0, 0.0],
                },
                color_rgb: [1.0, 1.0, 1.0],
                intensity: 1.0,
            }],
            environment: EnvironmentIntent {
                sky_top_rgb: [0.08, 0.16, 0.30],
                sky_horizon_rgb: [0.48, 0.56, 0.62],
                ground_rgb: [0.16, 0.20, 0.16],
                fog_color_rgb: [0.46, 0.53, 0.58],
                fog_density: 0.006,
                exposure: 1.0,
            },
            overlays: vec![SemanticOverlay::Polyline {
                marker_id: "route".into(),
                role: MarkerRole::Route,
                points_xyz_m: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 1.0]],
                thickness_m: 0.2,
                color_rgba: [0.0, 1.0, 1.0, 1.0],
            }],
            capture: GraphicsCaptureRequest {
                capture_id: "capture-test".into(),
                camera_id: camera.camera_id,
                width_px: 32,
                height_px: 32,
                format: CaptureFormat::Rgba8Srgb,
                include_depth: true,
                deterministic: true,
            },
        })
        .expect("test packet is valid")
    }

    #[test]
    fn packet_identity_is_content_bound() {
        let packet = packet();
        validate_scene_packet(&packet).expect("packet validates");
        let mut tampered = packet.clone();
        tampered.body.frame_seed += 1;
        assert!(validate_scene_packet(&tampered).is_err());
    }

    #[test]
    fn buffer_identity_rejects_resealed_payload() {
        let mut packet = packet();
        if let BufferPayload::F32(values) = &mut packet.body.terrain.heights_m.payload {
            values[0] = 1.0;
        }
        assert!(validate_scene_packet(&packet).is_err());
    }

    #[test]
    fn inline_texture_payload_is_digest_bound() {
        let bytes = vec![
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            255, 255, 255, 255, // white
        ];
        let texture = TextureReference {
            texture_id: "texture".into(),
            source_artifact_id: "source".into(),
            sha256: sha256_prefixed(&bytes),
            width_px: 2,
            height_px: 2,
            mip_levels: 1,
            color_space: TextureColorSpace::Srgb,
            payload: Some(TexturePayload::Rgba8(STANDARD.encode(&bytes))),
        };
        let mut packet = packet();
        packet.body.materials[0].texture_ids = vec![texture.texture_id.clone()];
        packet.body.textures = vec![texture];
        packet = seal_scene_packet(packet.body).expect("texture packet seals");
        validate_scene_packet(&packet).expect("matching texture payload validates");

        let TexturePayload::Rgba8(encoded) = packet.body.textures[0]
            .payload
            .as_mut()
            .expect("texture payload exists");
        encoded.replace_range(..4, "AAAA");
        assert!(validate_scene_packet(&packet).is_err());
    }

    #[test]
    fn material_texture_roles_reject_unknown_provenance() {
        let mut packet = packet();
        packet.body.materials[0].normal_texture_id = Some("missing-normal".into());
        assert!(validate_scene_packet(&packet).is_err());
    }

    #[test]
    fn authored_mesh_uv0_is_required_and_finite() {
        let mut body = packet().body;
        body.meshes = vec![MeshPacket {
            mesh_id: "mesh".into(),
            positions_m: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            uv0: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            material_id: "terrain".into(),
        }];
        seal_scene_packet(body.clone()).expect("authored UV channel validates");

        body.meshes[0].uv0.pop();
        assert!(seal_scene_packet(body).is_err());

        let mut body = packet().body;
        body.meshes = vec![MeshPacket {
            mesh_id: "mesh".into(),
            positions_m: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            uv0: vec![[0.0, 0.0], [f32::NAN, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            material_id: "terrain".into(),
        }];
        assert!(seal_scene_packet(body).is_err());
    }

    #[test]
    fn pass_receipt_requires_capture_bytes() {
        let body = GraphicsFrameReceiptBody {
            schema_version: FRAME_RECEIPT_SCHEMA.into(),
            packet_sha256: sha256_prefixed(b"packet"),
            capture_id: "capture".into(),
            backend_id: "wge.lava".into(),
            adapter_revision: ADAPTER_REVISION.into(),
            lava_revision: LAVA_REVISION.into(),
            device_uuid: "device".into(),
            worker_script_sha256: sha256_prefixed(b"worker"),
            renderer_identity_sha256: sha256_prefixed(b"renderer"),
            status: FrameStatus::Passed,
            format: CaptureFormat::Rgba8Srgb,
            width_px: 2,
            height_px: 2,
            capture_sha256: Some(sha256_prefixed(b"frame")),
            measurements: GraphicsFrameMeasurements {
                terrain_luminance_stddev: 0.4,
                distinct_terrain_colors: 3,
                route_visible_pixels: 2,
                player_spawn_visible_pixels: 2,
                opponent_spawn_visible_pixels: 2,
                encounter_visible_pixels: 2,
                objective_visible_pixels: 2,
            },
            telemetry: GraphicsTelemetry {
                upload_bytes: 1,
                readback_bytes: 1,
                draw_calls: 1,
                dispatch_calls: 0,
                pipeline_compilations: 1,
                instance_count: 0,
                visible_instance_count: 0,
                culled_instance_count: 0,
                background_visible_instance_count: 0,
                background_culled_instance_count: 0,
                landmark_visible_instance_count: 0,
                landmark_culled_instance_count: 0,
                gameplay_critical_visible_instance_count: 0,
                gameplay_critical_culled_instance_count: 0,
                terrain_vertex_count: 1,
                mesh_vertex_count: 0,
                frame_time_us: 1,
                gpu_frame_time_us: None,
            },
            detail: "measured".into(),
        };
        let mut receipt = GraphicsFrameReceipt {
            receipt_sha256: sha256_prefixed(&canonical_json(&body).expect("body JSON")),
            body,
        };
        assert!(validate_frame_receipt(&receipt, &[]).is_err());
        validate_frame_receipt(&receipt, b"frame").expect("matching capture validates");
        receipt.body.telemetry.frame_time_us = MAX_FRAME_TIME_US + 1;
        receipt.receipt_sha256 =
            sha256_prefixed(&canonical_json(&receipt.body).expect("tampered body JSON"));
        assert!(validate_frame_receipt(&receipt, b"frame").is_err());
    }

    #[test]
    fn rust_visual_measurement_recomputes_semantic_overlay_visibility() {
        let packet = packet();
        let mut capture = vec![0u8; 32 * 32 * 4];
        capture[..4].copy_from_slice(&[0, 255, 255, 255]);
        let measurements = measure_frame_capture(&packet, &capture).expect("capture measures");
        assert_eq!(measurements.route_visible_pixels, 1);
        assert_eq!(measurements.distinct_terrain_colors, 2);
    }

    #[test]
    fn srgb_capture_quantization_is_explicit() {
        assert_eq!(
            rgba8(&[0.5, 0.5, 0.5, 0.5]).expect("valid color"),
            [188, 188, 188, 128]
        );
    }

    #[test]
    fn rust_visual_measurement_rejects_truncated_capture() {
        let packet = packet();
        let error = measure_frame_capture(&packet, &[0u8; 4]).expect_err("truncated capture fails");
        assert_eq!(error.code, "provenance");
    }

    #[test]
    fn native_visual_gate_rejects_flat_capture_and_accepts_useful_capture() {
        let packet = packet();
        let flat = vec![0u8; 32 * 32 * 4];
        let flat_measurements =
            measure_frame_capture(&packet, &flat).expect("flat capture measures");
        assert!(validate_native_visual_gate(&packet, &flat_measurements).is_err());

        let mut useful = [50u8, 60, 70, 255].repeat(32 * 32);
        useful[..4].copy_from_slice(&[0, 255, 255, 255]);
        useful[4..8].copy_from_slice(&[200, 200, 200, 255]);
        let useful_measurements =
            measure_frame_capture(&packet, &useful).expect("useful capture measures");
        validate_native_visual_gate(&packet, &useful_measurements)
            .expect("useful capture passes the native visual gate");
    }

    #[test]
    fn capture_bound_rejects_a_worker_frame_that_would_exceed_the_protocol_budget() {
        let packet = packet();
        let mut body = packet.body.clone();
        body.camera.width_px = 8192;
        body.camera.height_px = 8192;
        body.capture.width_px = 8192;
        body.capture.height_px = 8192;
        assert!(seal_scene_packet(body).is_err());
    }
}
