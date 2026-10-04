//! Typed, engine-neutral input and evidence contracts for the native WGE
//! graphics path.
//!
//! This crate owns no Vulkan handles and imports no renderer-specific types.
//! It lowers an already validated reference world into one coarse packet that
//! a supervised graphics worker can consume and independently revalidate.

use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wge_reference_runtime::{ReferenceCamera, WorldArtifact, validate_world_artifact};

pub const SCENE_PACKET_SCHEMA: &str = "wge.graphics-scene-packet/v6";
pub const READY_SCHEMA: &str = "wge.graphics-ready/v1";
pub const FRAME_RECEIPT_SCHEMA: &str = "wge.graphics-frame-receipt/v1";
pub const RENDERER_ATTESTATION_SCHEMA: &str = "wge.graphics-renderer-attestation/v1";
pub const ADAPTER_REVISION: &str = "wge.lava-adapter/v7";
pub const LAVA_BACKEND_ID: &str = "lava-vulkan";
pub const LAVA_REVISION: &str = "11c7e31bdf62408d22bf379e9e59510f69d2103e";
pub const MAX_PACKET_ELEMENTS: usize = 16 * 1024 * 1024;
pub const MAX_CAPTURE_DIMENSION: u32 = 8192;
pub const MAX_CAPTURE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_DENSE_BENCHMARK_INSTANCES: usize = 4096;
const MAX_TEXTURE_MIP_LEVELS: u32 = 32;
/// The native reference path deliberately has a bounded physical envelope.
/// This keeps all derived camera/light calculations finite in f32 and makes
/// malformed or hostile packets fail before they reach Julia/Vulkan.
pub const MAX_NATIVE_COORDINATE_M: f32 = 1_000_000.0;
const MAX_TELEMETRY_COUNTER: usize = 1 << 40;
const MAX_FRAME_TIME_US: u64 = 60_000_000;
const MIN_NATIVE_DISTANCE_M: f32 = 1.0e-4;
const MIN_NATIVE_SCALE: f32 = 1.0e-4;
const MIN_NATIVE_LUMINANCE_STDDEV: f64 = 0.01;
const MIN_NATIVE_DISTINCT_COLORS: usize = 3;
const MIN_NATIVE_NON_DOMINANT_PIXELS: usize = 16;
const MIN_VECTOR_LENGTH_SQUARED: f32 = 1.0e-8;
const UNIT_QUATERNION_TOLERANCE: f32 = 1.0e-3;
const MAX_BASIS_COSINE: f32 = 0.999;
const REFERENCE_FOG_WEIGHT_AT_CAMERA: f64 = 0.16;
const MAX_REFERENCE_FOG_DENSITY: f64 = 0.006;

pub mod asset_projection;
pub mod deformation;
pub mod input_session;
pub mod live;
pub mod material_maps;
pub mod render_policy;
pub mod scene_composition;
pub mod session;
pub mod terrain_layers;
pub mod supervisor;
pub mod visual_quality;
pub mod window;

pub use deformation::{
    deformation_error, deformation_v7_enabled, validate_deformation,
    validate_deformation_receipt, validate_schema_deformation, DeformationFamily, DeformationField,
    DeformationIntent, DeformationRejection, DeformationTelemetry, DEFORMATION_V7_ENV,
    SCENE_PACKET_SCHEMA_V7,
};

pub use render_policy::{
    validate_packet_render_policy, validate_render_policy, AtmospherePolicy, BloomPolicy,
    DitherPolicy, GradePolicy, MeshSurfacePolicy, RenderPolicy, ResolvedRenderPolicy,
    SamplerPolicy, ShadowFitPolicy, ShadowPolicy, SkyModel, SkyPolicy, TerrainSurfacePolicy,
    VignettePolicy,
    POLICY_SCALE,
};


pub use asset_projection::{
    GRAPHICS_ASSET_PROJECTION_SCHEMA, GraphicsAssetMesh, GraphicsAssetProjection,
    GraphicsAssetTexture, project_render_asset, validate_graphics_asset_projection,
};
pub use scene_composition::{compose_bound_scene, compose_bound_scene_with_camera};
pub use terrain_layers::{
    LayerCoverage, LayerSource, LayerTextureSizes, MacroRamp, SquareRgba8, TerrainLayer,
    TerrainLayerSet, TerrainLayers, apply_terrain_layers, build_terrain_layer_set,
    load_terrain_layer_set, validate_terrain_layers,
};

pub use input_session::{
    INPUT_FRAME_SCHEMA, INPUT_SAMPLE_SCHEMA, INPUT_SIM_TICK_DT_MS, INPUT_TRACE_SCHEMA,
    InputDrivenSession, InputFrame, InputSample, TraceStep, camera_intent,
    ds3_map, frame_from_gamepad, frame_from_keyboard, frame_from_sample, trace_digest,
};

pub use live::{
    GraphicsSessionMode, LiveCaptureOutput, LiveGraphicsError, LiveGraphicsSession,
    LivePresentOutcome,
};

pub use supervisor::{
    BoundSceneRenderAuthorization, GraphicsFrameOutput, GraphicsWorkerError,
    GraphicsWorkerSupervisor, PromotedFrame, WORKER_SCHEMA,
};
pub use visual_quality::{
    CAMPAIGN2_VISUAL_EVIDENCE_SCHEMA, Campaign2VisualEvidence, Campaign2VisualEvidenceBody,
    Campaign2VisualMeasurements, QualityOutcome, QualityReason, QualityReasonCode, QualityRegion,
    VISUAL_QUALITY_EVIDENCE_SCHEMA, VISUAL_QUALITY_PROFILE_SCHEMA, VisualObservation,
    VisualObservationStatus, VisualQualityEvidence, VisualQualityEvidenceBody,
    VisualQualityMeasurements, VisualQualityProfile, assess_campaign2_visual_evidence,
    assess_visual_quality, validate_campaign2_visual_evidence,
    validate_registered_visual_quality_profile, validate_visual_quality_evidence,
};
pub use window::{
    CapabilityState, WINDOW_PROBE_RECEIPT_SCHEMA, WINDOW_TARGET_REQUEST_SCHEMA,
    WindowBackendEvidence, WindowCapabilities, WindowContractError, WindowProbeReceipt,
    WindowProbeStage, WindowProbeStatus, WindowTargetRequest, probe_lava_window,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_artifact_sha256: Option<String>,
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
    /// TetCage deformation binding (scene packet v7 ONLY). `Option` +
    /// `skip_serializing_if` is what makes flag-off output byte-identical to
    /// v6 by construction rather than by test: absent means the key is absent
    /// from the canonical JSON, so the sealed digest is unchanged. Presence
    /// REQUIRES `schema_version == SCENE_PACKET_SCHEMA_V7` and is enforced in
    /// both directions by `deformation::validate_schema_deformation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deformation: Option<DeformationIntent>,
    /// Typed render policy: the execution channel from style intent to renderer
    /// state (graphical parity audit GP-01). `Option` +
    /// `skip_serializing_if` for the same reason as `deformation`: an absent
    /// section means "the renderer used its declared defaults", so a v6
    /// packet's canonical bytes are unchanged by the section existing at all.
    /// Presence is honoured in BOTH v6 and v7 — unlike `deformation`, a policy
    /// does not change what a packet MEANS, only how it is presented, so it
    /// does not warrant a schema split.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_policy: Option<RenderPolicy>,
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
    /// N-4 layered surface. Absent = the single-material terrain, and the
    /// canonical bytes of every existing packet are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<TerrainLayers>,
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
    pub clearcoat: f32,
    pub clearcoat_roughness: f32,
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
    Rgba8MipChain { levels: Vec<TextureMipLevel> },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TextureMipLevel {
    pub width_px: u32,
    pub height_px: u32,
    pub base64: String,
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
    /// Optional conditioned tangents. Empty means the backend may derive a
    /// deterministic fallback from positions, normals, UVs, and triangle
    /// order. Imported assets retain their Rust-conditioned stream here so a
    /// backend does not silently replace authored tangent provenance.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tangents: Vec<[f32; 4]>,
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

/// Source and capability identity captured by the Rust supervisor after it has
/// validated the launched worker and renderer sources. The authority plane
/// revalidates this typed bundle against the promoted frame receipt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsRendererAttestationBody {
    pub schema_version: String,
    pub backend_id: String,
    pub adapter_revision: String,
    pub lava_revision: String,
    pub worker_ready_message: Value,
    pub ready: GraphicsReady,
    pub source_digests: BTreeMap<String, String>,
    pub source_identity_sha256: String,
    pub renderer_identity_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsRendererAttestation {
    pub body: GraphicsRendererAttestationBody,
    pub attestation_sha256: String,
}

pub fn seal_renderer_attestation(
    body: GraphicsRendererAttestationBody,
) -> Result<GraphicsRendererAttestation, GraphicsContractError> {
    validate_renderer_attestation_body(&body)?;
    let attestation_sha256 = sha256_prefixed(&canonical_json(&body)?);
    Ok(GraphicsRendererAttestation {
        body,
        attestation_sha256,
    })
}

pub fn validate_renderer_attestation(
    attestation: &GraphicsRendererAttestation,
    frame: &GraphicsFrameReceipt,
) -> Result<(), GraphicsContractError> {
    let expected = sha256_prefixed(&canonical_json(&attestation.body)?);
    if attestation.attestation_sha256 != expected {
        return Err(GraphicsContractError::provenance(
            "renderer attestation digest does not match its canonical body",
        ));
    }
    validate_renderer_attestation_body(&attestation.body)?;
    let body = &attestation.body;
    let frame_body = &frame.body;
    if frame_body.backend_id != body.backend_id
        || frame_body.adapter_revision != body.adapter_revision
        || frame_body.lava_revision != body.lava_revision
        || frame_body.device_uuid != body.ready.device_uuid
        || frame_body.worker_script_sha256
            != body
                .source_digests
                .get("worker_script")
                .map(String::as_str)
                .unwrap_or_default()
        || frame_body.renderer_identity_sha256 != body.renderer_identity_sha256
    {
        return Err(GraphicsContractError::provenance(
            "promoted frame receipt is detached from renderer attestation",
        ));
    }
    Ok(())
}

fn validate_renderer_attestation_body(
    body: &GraphicsRendererAttestationBody,
) -> Result<(), GraphicsContractError> {
    if body.schema_version != RENDERER_ATTESTATION_SCHEMA
        || body.backend_id != LAVA_BACKEND_ID
        || body.adapter_revision != ADAPTER_REVISION
        || body.lava_revision != LAVA_REVISION
    {
        return Err(GraphicsContractError::provenance(
            "renderer attestation is not bound to the audited Lava contract",
        ));
    }
    validate_ready(&body.ready)?;
    let required_sources = [
        "graphics_contract",
        "lava_adapter",
        "manifest",
        "project",
        "worker_script",
    ];
    if body.source_digests.len() != required_sources.len()
        || required_sources
            .iter()
            .any(|source| !body.source_digests.contains_key(*source))
    {
        return Err(GraphicsContractError::provenance(
            "renderer attestation source manifest is incomplete",
        ));
    }
    for (source, digest) in &body.source_digests {
        valid_sha(digest, source)?;
    }
    let worker_script = body
        .worker_ready_message
        .get("script_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            GraphicsContractError::provenance(
                "renderer attestation worker-ready message has no script identity",
            )
        })?;
    if worker_script != body.source_digests["worker_script"]
        || body
            .worker_ready_message
            .get("kind")
            .and_then(Value::as_str)
            != Some("ready")
    {
        return Err(GraphicsContractError::provenance(
            "renderer attestation worker identity is inconsistent",
        ));
    }
    let source_identity = sha256_prefixed(&canonical_json(&body.source_digests)?);
    if body.source_identity_sha256 != source_identity {
        return Err(GraphicsContractError::provenance(
            "renderer attestation source identity does not match its manifest",
        ));
    }
    let renderer_identity = sha256_prefixed(&canonical_json(&json!({
        "capabilities": &body.ready,
        "worker_ready": &body.worker_ready_message,
        "sources": &body.source_digests,
    }))?);
    if body.renderer_identity_sha256 != renderer_identity {
        return Err(GraphicsContractError::provenance(
            "renderer attestation identity does not match its capabilities or sources",
        ));
    }
    Ok(())
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
pub struct GraphicsTextureResidencyTelemetry {
    /// Number of distinct packet texture identities whose payloads were
    /// materialized for this frame. This intentionally excludes adapter-owned
    /// default textures.
    pub texture_count: usize,
    /// Sum of resident mip levels across those distinct packet textures.
    pub mip_levels: usize,
    /// Sum of decoded RGBA8 bytes across those resident levels.
    pub payload_bytes: usize,
    /// Highest sampler LOD made resident for any packet texture.
    pub max_sampler_lod: u32,
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
    /// Optional for backward-compatible single-level receipts. A frame that
    /// carries a multi-level packet texture must provide this evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture_residency: Option<GraphicsTextureResidencyTelemetry>,
    pub frame_time_us: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_frame_time_us: Option<u64>,
    /// TetCage deformation evidence (scene packet v7 ONLY). Same
    /// byte-identity argument as `GraphicsScenePacketBody::deformation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deformation: Option<DeformationTelemetry>,
    pub pass_timings: GraphicsPassTimings,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsPassTimings {
    pub prepare_us: u64,
    pub scene_raster_us: u64,
    pub resolve_us: u64,
    pub overlay_us: u64,
    pub flush_readback_us: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_prepare_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_scene_raster_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_resolve_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_overlay_us: Option<u64>,
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
    if packet.body.schema_version != SCENE_PACKET_SCHEMA
        && packet.body.schema_version != deformation::SCENE_PACKET_SCHEMA_V7
    {
        return Err(GraphicsContractError::unsupported(format!(
            "unsupported scene packet schema {}",
            packet.body.schema_version
        )));
    }
    // v6/v7 mutual exclusion, before any content validation: a packet whose
    // schema and contents disagree about whether it carries a deformation has
    // no coherent meaning, and diagnosing that first keeps every later error
    // message about deformation actually meaning something.
    deformation::validate_schema_deformation(&packet.body)?;
    if let Some(intent) = &packet.body.deformation {
        deformation::validate_deformation(&packet.body, intent)?;
    }
    render_policy::validate_packet_render_policy(&packet.body)?;
    valid_id(&packet.body.packet_id, "packet_id")?;
    match (
        &packet.body.scene_artifact_id,
        &packet.body.scene_artifact_sha256,
    ) {
        (Some(scene_id), Some(scene_sha256)) => {
            valid_id(scene_id, "scene_artifact_id")?;
            valid_sha(scene_sha256, "scene_artifact_sha256")?;
        }
        (None, None) => {}
        _ => {
            return Err(GraphicsContractError::provenance(
                "scene artifact identity must include both id and digest",
            ));
        }
    }
    valid_id(&packet.body.world_artifact_id, "world_artifact_id")?;
    valid_sha(&packet.body.world_artifact_sha256, "world_artifact_sha256")?;
    valid_sha(&packet.body.spatial_fields_sha256, "spatial_fields_sha256")?;
    validate_coordinate_system(&packet.body.coordinate_system)?;
    validate_camera(&packet.body.camera)?;
    validate_terrain(&packet.body.terrain, &packet.body.materials, &packet.body.textures)?;

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

fn seal_frame_receipt(
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

fn seal_frame_receipt_with_capture(
    packet: &GraphicsScenePacket,
    body: GraphicsFrameReceiptBody,
    capture_bytes: &[u8],
) -> Result<GraphicsFrameReceipt, GraphicsContractError> {
    let receipt = seal_frame_receipt(body)?;
    validate_frame_receipt(&receipt, packet, capture_bytes)?;
    Ok(receipt)
}

/// Produce the deterministic receipt projection used by certification
/// snapshots. Runtime timing telemetry is inherently host- and load-dependent;
/// benchmark commands retain it in their ordinary receipts, while the
/// certification artifact keeps structural telemetry and explicitly zeros the
/// timing fields so identical scene/capture bytes produce identical evidence.
pub fn deterministic_certification_frame_receipt(
    packet: &GraphicsScenePacket,
    receipt: &GraphicsFrameReceipt,
    capture_bytes: &[u8],
) -> Result<GraphicsFrameReceipt, GraphicsContractError> {
    let mut body = receipt.body.clone();
    body.telemetry.frame_time_us = 0;
    body.telemetry.gpu_frame_time_us = None;
    body.telemetry.pass_timings = GraphicsPassTimings {
        prepare_us: 0,
        scene_raster_us: 0,
        resolve_us: 0,
        overlay_us: 0,
        flush_readback_us: 0,
        gpu_prepare_us: None,
        gpu_scene_raster_us: None,
        gpu_resolve_us: None,
        gpu_overlay_us: None,
    };
    body.detail =
        "Rust-promoted Lava frame; timing telemetry omitted from deterministic certification identity"
            .into();
    seal_frame_receipt_with_capture(packet, body, capture_bytes)
}

pub(crate) fn validate_frame_receipt(
    receipt: &GraphicsFrameReceipt,
    packet: &GraphicsScenePacket,
    capture_bytes: &[u8],
) -> Result<(), GraphicsContractError> {
    validate_scene_packet(packet)?;
    validate_frame_receipt_shape(receipt)?;
    validate_texture_residency_telemetry(packet, &receipt.body.telemetry)?;
    if receipt.body.packet_sha256 != packet.packet_sha256 {
        return Err(GraphicsContractError::provenance(
            "frame receipt is bound to a different scene packet",
        ));
    }
    if receipt.body.capture_id != packet.body.capture.capture_id
        || receipt.body.width_px != packet.body.capture.width_px
        || receipt.body.height_px != packet.body.capture.height_px
        || receipt.body.format != packet.body.capture.format
    {
        return Err(GraphicsContractError::provenance(
            "frame receipt is detached from the packet capture request",
        ));
    }
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
            let authoritative_measurements = measure_frame_capture(packet, capture_bytes)?;
            if receipt.body.measurements != authoritative_measurements {
                return Err(GraphicsContractError::provenance(
                    "frame receipt measurements do not match independently measured capture bytes",
                ));
            }
            validate_native_visual_gate(packet, &authoritative_measurements, capture_bytes)?;
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
    let mut role_samples: [Vec<ScreenSample>; 5] = std::array::from_fn(|_| Vec::new());
    for overlay in &packet.body.overlays {
        let index = marker_role_index(overlay_role(overlay));
        role_colors[index].insert(rgba8(overlay_color(overlay))?);
        role_samples[index].extend(overlay_samples(&packet.body.camera, overlay));
    }

    let mut role_pixels = [0usize; 5];
    let mut mean = 0.0f64;
    let mut sum_squared_delta = 0.0f64;
    let mut sample_count = 0.0f64;
    for (pixel_index, pixel) in capture_bytes.chunks_exact(4).enumerate() {
        let rgba = [pixel[0], pixel[1], pixel[2], pixel[3]];
        let x = pixel_index % width;
        let y = pixel_index / width;
        distinct_colors.insert((rgba[0], rgba[1], rgba[2]));
        let luminance = 0.2126 * f64::from(rgba[0]) / 255.0
            + 0.7152 * f64::from(rgba[1]) / 255.0
            + 0.0722 * f64::from(rgba[2]) / 255.0;
        sample_count += 1.0;
        let delta = luminance - mean;
        mean += delta / sample_count;
        sum_squared_delta += delta * (luminance - mean);
        for (index, colors) in role_colors.iter().enumerate() {
            if colors.contains(&rgba)
                && role_samples[index]
                    .iter()
                    .copied()
                    .any(|sample| sample_contains(sample, x, y))
            {
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
    capture_bytes: &[u8],
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
    if capture_bytes.len() != pixel_count * 4 {
        return Err(GraphicsContractError::provenance(
            "native visual gate cannot inspect a capture with detached dimensions",
        ));
    }
    let non_dominant_pixels = non_dominant_pixel_count(capture_bytes);
    if non_dominant_pixels < MIN_NATIVE_NON_DOMINANT_PIXELS {
        return Err(GraphicsContractError::provenance(
            "native visual gate failed: capture has no spatially supported terrain variation",
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

#[derive(Clone, Copy, Debug)]
struct ScreenSample {
    x: f32,
    y: f32,
    radius_px: f32,
}

fn normalize3(vector: [f32; 3]) -> [f32; 3] {
    let length = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    [vector[0] / length, vector[1] / length, vector[2] / length]
}

fn dot3(first: [f32; 3], second: [f32; 3]) -> f32 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn cross3(first: [f32; 3], second: [f32; 3]) -> [f32; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

/// Fraction of sampled pixels, in basis points, whose camera ray points BELOW
/// the horizon yet never meets the terrain before the far plane: the visible
/// "world ends here" void (audit item 1, CONVERGE-0 step 6).
///
/// Deterministic CPU geometry, not a capture statistic: each sampled pixel's
/// ray is marched over the bilinear terrain heightfield (step 1% of distance,
/// at least 0.25 m). Instances are ignored, so the measure is conservative —
/// a prop can only hide void, never create it. Rays above the horizon are sky
/// and never count.
pub fn world_void_fraction_bp(packet: &GraphicsScenePacket, stride_px: u32) -> u32 {
    let camera = &packet.body.camera;
    let terrain = &packet.body.terrain;
    let BufferPayload::F32(heights) = &terrain.heights_m.payload else {
        return 10_000;
    };
    let CameraProjection::Perspective { fov_y_degrees } = camera.projection else {
        return 0;
    };
    let (forward, right, up) = camera_basis(camera);
    let tan_y = (fov_y_degrees.to_radians() * 0.5).tan();
    let tan_x = tan_y * camera.width_px as f32 / camera.height_px as f32;
    let n = terrain.resolution;
    let half_width = terrain.width_m * 0.5;
    let half_length = terrain.length_m * 0.5;
    let height_at = |x: f32, z: f32| -> Option<f32> {
        if x.abs() > half_width || z.abs() > half_length {
            return None;
        }
        let column = (x / terrain.width_m + 0.5) * (n - 1) as f32;
        let row = (0.5 - z / terrain.length_m) * (n - 1) as f32;
        let c0 = (column.floor() as usize).min(n - 2);
        let r0 = (row.floor() as usize).min(n - 2);
        let tc = column - c0 as f32;
        let tr = row - r0 as f32;
        let sample = |r: usize, c: usize| heights[r * n + c];
        let top = sample(r0, c0) * (1.0 - tc) + sample(r0, c0 + 1) * tc;
        let bottom = sample(r0 + 1, c0) * (1.0 - tc) + sample(r0 + 1, c0 + 1) * tc;
        Some(top * (1.0 - tr) + bottom * tr)
    };
    let stride = stride_px.max(1) as usize;
    let (mut sampled, mut void) = (0u64, 0u64);
    for py in (0..camera.height_px as usize).step_by(stride) {
        for px in (0..camera.width_px as usize).step_by(stride) {
            sampled += 1;
            let ndc_x = (px as f32 + 0.5) / camera.width_px as f32 * 2.0 - 1.0;
            let ndc_y = 1.0 - (py as f32 + 0.5) / camera.height_px as f32 * 2.0;
            let mut ray = [0.0f32; 3];
            for axis in 0..3 {
                ray[axis] = forward[axis] + ndc_x * tan_x * right[axis] + ndc_y * tan_y * up[axis];
            }
            let length = (ray[0] * ray[0] + ray[1] * ray[1] + ray[2] * ray[2]).sqrt();
            let ray = ray.map(|value| value / length);
            if ray[1] >= 0.0 {
                continue;
            }
            let mut distance = camera.near_plane_m;
            let mut hit = false;
            while distance < camera.far_plane_m {
                let point = [
                    camera.position_xyz_m[0] + ray[0] * distance,
                    camera.position_xyz_m[1] + ray[1] * distance,
                    camera.position_xyz_m[2] + ray[2] * distance,
                ];
                if let Some(ground) = height_at(point[0], point[2])
                    && point[1] <= ground
                {
                    hit = true;
                    break;
                }
                distance += (distance * 0.01).max(0.25);
            }
            if !hit {
                void += 1;
            }
        }
    }
    if sampled == 0 { 0 } else { (void * 10_000 / sampled) as u32 }
}

fn camera_basis(camera: &GraphicsCamera) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let forward = normalize3(camera.forward_xyz);
    let right = normalize3(cross3(forward, normalize3(camera.up_xyz)));
    let up = normalize3(cross3(right, forward));
    (forward, right, up)
}

fn project_screen_point(camera: &GraphicsCamera, position: [f32; 3]) -> Option<(f32, f32, f32)> {
    let (forward, right, up) = camera_basis(camera);
    let relative = [
        position[0] - camera.position_xyz_m[0],
        position[1] - camera.position_xyz_m[1],
        position[2] - camera.position_xyz_m[2],
    ];
    let depth = dot3(forward, relative);
    if depth < camera.near_plane_m || depth > camera.far_plane_m {
        return None;
    }
    let horizontal = dot3(right, relative);
    let vertical = dot3(up, relative);
    let aspect = camera.width_px as f32 / camera.height_px as f32;
    let (ndc_x, ndc_y, pixels_per_meter) = match camera.projection {
        CameraProjection::Orthographic { span_m } => {
            let half_height = span_m * 0.5;
            let half_width = half_height * aspect;
            (
                horizontal / half_width,
                vertical / half_height,
                camera.height_px as f32 / span_m,
            )
        }
        CameraProjection::Perspective { fov_y_degrees } => {
            let half_fov = (fov_y_degrees.to_radians() * 0.5).tan();
            let half_height = depth * half_fov;
            let half_width = half_height * aspect;
            (
                horizontal / half_width,
                vertical / half_height,
                camera.height_px as f32 / (2.0 * half_height),
            )
        }
    };
    if !ndc_x.is_finite() || !ndc_y.is_finite() || !pixels_per_meter.is_finite() {
        return None;
    }
    Some((
        (ndc_x * 0.5 + 0.5) * camera.width_px as f32,
        // The packet's camera basis is semantic. The Lava/Vulkan lowering
        // uses a positive-height viewport, whose positive NDC Y lands in the
        // lower framebuffer rows; keep Rust-owned masks and overlay sampling
        // in the same top-to-bottom screen convention as the capture bytes.
        (-ndc_y * 0.5 + 0.5) * camera.height_px as f32,
        pixels_per_meter,
    ))
}

fn clip_overlay_segment(
    camera: &GraphicsCamera,
    first: [f32; 3],
    last: [f32; 3],
) -> Option<([f32; 3], [f32; 3])> {
    let (forward, _, _) = camera_basis(camera);
    let first_relative = [
        first[0] - camera.position_xyz_m[0],
        first[1] - camera.position_xyz_m[1],
        first[2] - camera.position_xyz_m[2],
    ];
    let last_relative = [
        last[0] - camera.position_xyz_m[0],
        last[1] - camera.position_xyz_m[1],
        last[2] - camera.position_xyz_m[2],
    ];
    let first_depth = dot3(forward, first_relative);
    let last_depth = dot3(forward, last_relative);
    let depth_delta = last_depth - first_depth;
    if depth_delta == 0.0 {
        return (camera.near_plane_m..=camera.far_plane_m)
            .contains(&first_depth)
            .then_some((first, last));
    }
    let mut lower = (camera.near_plane_m - first_depth) / depth_delta;
    let mut upper = (camera.far_plane_m - first_depth) / depth_delta;
    if lower > upper {
        std::mem::swap(&mut lower, &mut upper);
    }
    let start = lower.max(0.0);
    let end = upper.min(1.0);
    (start <= end).then(|| {
        let interpolate = |amount: f32| {
            [
                first[0] + (last[0] - first[0]) * amount,
                first[1] + (last[1] - first[1]) * amount,
                first[2] + (last[2] - first[2]) * amount,
            ]
        };
        (interpolate(start), interpolate(end))
    })
}

fn overlay_samples(camera: &GraphicsCamera, overlay: &SemanticOverlay) -> Vec<ScreenSample> {
    let mut samples = Vec::new();
    match overlay {
        SemanticOverlay::Point {
            position_xyz_m,
            radius_m,
            ..
        } => {
            if let Some((x, y, pixels_per_meter)) = project_screen_point(camera, *position_xyz_m) {
                samples.push(ScreenSample {
                    x,
                    y,
                    radius_px: (*radius_m * pixels_per_meter + 3.0).max(4.0),
                });
            }
        }
        SemanticOverlay::Circle {
            center_xyz_m,
            radius_m,
            ..
        } => {
            for step in 0..16 {
                let angle = std::f32::consts::TAU * step as f32 / 16.0;
                let position = [
                    center_xyz_m[0] + *radius_m * angle.cos(),
                    center_xyz_m[1],
                    center_xyz_m[2] + *radius_m * angle.sin(),
                ];
                if let Some((x, y, pixels_per_meter)) = project_screen_point(camera, position) {
                    samples.push(ScreenSample {
                        x,
                        y,
                        radius_px: (4.0 + pixels_per_meter * 0.08).max(4.0),
                    });
                }
            }
        }
        SemanticOverlay::Polyline {
            points_xyz_m,
            thickness_m,
            ..
        } => {
            for pair in points_xyz_m.windows(2) {
                let Some((first, last)) = clip_overlay_segment(camera, pair[0], pair[1]) else {
                    continue;
                };
                for step in 0..=8 {
                    let amount = step as f32 / 8.0;
                    let position = [
                        first[0] + (last[0] - first[0]) * amount,
                        first[1] + (last[1] - first[1]) * amount,
                        first[2] + (last[2] - first[2]) * amount,
                    ];
                    if let Some((x, y, pixels_per_meter)) = project_screen_point(camera, position) {
                        samples.push(ScreenSample {
                            x,
                            y,
                            radius_px: (*thickness_m * pixels_per_meter + 3.0).max(4.0),
                        });
                    }
                }
            }
        }
    }
    samples
}

fn sample_contains(sample: ScreenSample, x: usize, y: usize) -> bool {
    let dx = x as f32 + 0.5 - sample.x;
    let dy = y as f32 + 0.5 - sample.y;
    dx * dx + dy * dy <= sample.radius_px * sample.radius_px
}

fn non_dominant_pixel_count(capture_bytes: &[u8]) -> usize {
    let mut colors = BTreeMap::<[u8; 3], usize>::new();
    for pixel in capture_bytes.chunks_exact(4) {
        *colors.entry([pixel[0], pixel[1], pixel[2]]).or_default() += 1;
    }
    let dominant = colors.values().copied().max().unwrap_or(0);
    capture_bytes.len() / 4 - dominant
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
    if body.backend_id != LAVA_BACKEND_ID {
        return Err(GraphicsContractError::provenance(
            "frame receipt backend is not the audited Lava backend",
        ));
    }
    if body.adapter_revision != ADAPTER_REVISION {
        return Err(GraphicsContractError::provenance(
            "frame receipt adapter revision is not the audited native adapter",
        ));
    }
    if body.lava_revision != LAVA_REVISION {
        return Err(GraphicsContractError::provenance(
            "frame receipt Lava revision is not the audited revision",
        ));
    }
    valid_id(&body.adapter_revision, "adapter_revision")?;
    valid_id(&body.lava_revision, "lava_revision")?;
    valid_id(&body.device_uuid, "device_uuid")?;
    valid_sha(&body.worker_script_sha256, "worker_script_sha256")?;
    valid_sha(&body.renderer_identity_sha256, "renderer_identity_sha256")?;
    if body.format != CaptureFormat::Rgba8Srgb {
        return Err(GraphicsContractError::unsupported(
            "native frame receipts require RGBA8 sRGB captures",
        ));
    }
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
        tangents: Vec::new(),
    }
}

fn block_mesh(mesh_id: &str, material_id: &str) -> MeshPacket {
    let mut mesh = obstacle_mesh();
    mesh.mesh_id = mesh_id.into();
    mesh.material_id = material_id.into();
    mesh
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
        tangents: Vec::new(),
    }
}

fn objective_beacon_mesh() -> MeshPacket {
    const SEGMENTS: usize = 8;
    let mut mesh = BeaconMeshBuffers::with_capacity(SEGMENTS);

    mesh.append_band(0.0, 0.45, 1.8, 1.6);
    mesh.append_band(0.45, 3.8, 0.72, 0.72);
    mesh.append_band(3.8, 4.35, 1.2, 0.9);
    mesh.append_cap(4.35, 0.9, true);
    mesh.append_cap(0.0, 1.8, false);

    mesh.into_mesh("objective-beacon", "objective-beacon")
}

fn radial_mesh(
    mesh_id: &str,
    material_id: &str,
    segments: usize,
    bands: &[(f32, f32, f32, f32)],
    bottom_cap: Option<f32>,
    top_cap: Option<f32>,
) -> MeshPacket {
    radial_mesh_with(
        BeaconMeshBuffers::with_capacity(segments),
        mesh_id,
        material_id,
        bands,
        bottom_cap,
        top_cap,
    )
}

/// `radial_mesh` with metric UVs: one texture repeat per `metres_per_repeat`
/// along the circumference and the profile (audit MD-4).
fn radial_mesh_metric(
    mesh_id: &str,
    material_id: &str,
    segments: usize,
    bands: &[(f32, f32, f32, f32)],
    bottom_cap: Option<f32>,
    top_cap: Option<f32>,
    metres_per_repeat: f32,
) -> MeshPacket {
    radial_mesh_with(
        BeaconMeshBuffers::with_metric_uv(segments, metres_per_repeat),
        mesh_id,
        material_id,
        bands,
        bottom_cap,
        top_cap,
    )
}

fn radial_mesh_with(
    mut mesh: BeaconMeshBuffers,
    mesh_id: &str,
    material_id: &str,
    bands: &[(f32, f32, f32, f32)],
    bottom_cap: Option<f32>,
    top_cap: Option<f32>,
) -> MeshPacket {
    for &(lower_y, upper_y, lower_radius, upper_radius) in bands {
        mesh.append_band(lower_y, upper_y, lower_radius, upper_radius);
    }
    if let Some(radius) = top_cap {
        let y = bands
            .last()
            .map(|(_, upper_y, _, _)| *upper_y)
            .unwrap_or(0.0);
        mesh.append_cap(y, radius, true);
    }
    if let Some(radius) = bottom_cap {
        let y = bands
            .first()
            .map(|(lower_y, _, _, _)| *lower_y)
            .unwrap_or(0.0);
        mesh.append_cap(y, radius, false);
    }
    mesh.into_mesh(mesh_id, material_id)
}

fn torus_mesh(
    mesh_id: &str,
    material_id: &str,
    major_radius: f32,
    minor_radius: f32,
    major_segments: usize,
    minor_segments: usize,
) -> MeshPacket {
    let mut positions = Vec::with_capacity(major_segments * minor_segments);
    let mut normals = Vec::with_capacity(major_segments * minor_segments);
    let mut uv0 = Vec::with_capacity(major_segments * minor_segments);
    let mut indices = Vec::with_capacity(major_segments * minor_segments * 6);
    for major in 0..major_segments {
        let major_angle = std::f32::consts::TAU * major as f32 / major_segments as f32;
        let major_cos = major_angle.cos();
        let major_sin = major_angle.sin();
        for minor in 0..minor_segments {
            let minor_angle = std::f32::consts::TAU * minor as f32 / minor_segments as f32;
            let minor_cos = minor_angle.cos();
            let minor_sin = minor_angle.sin();
            let radius = major_radius + minor_radius * minor_cos;
            positions.push([
                radius * major_cos,
                radius * major_sin,
                minor_radius * minor_sin,
            ]);
            normals.push([minor_cos * major_cos, minor_cos * major_sin, minor_sin]);
            uv0.push([
                major as f32 / major_segments as f32,
                minor as f32 / minor_segments as f32,
            ]);
        }
    }
    for major in 0..major_segments {
        let next_major = (major + 1) % major_segments;
        for minor in 0..minor_segments {
            let next_minor = (minor + 1) % minor_segments;
            let first = (major * minor_segments + minor) as u32;
            let second = (next_major * minor_segments + minor) as u32;
            let third = (next_major * minor_segments + next_minor) as u32;
            let fourth = (major * minor_segments + next_minor) as u32;
            indices.extend([first, second, third, first, third, fourth]);
        }
    }
    MeshPacket {
        mesh_id: mesh_id.into(),
        positions_m: positions,
        normals,
        uv0,
        indices,
        material_id: material_id.into(),
        tangents: Vec::new(),
    }
}

/// Torus with metric UVs and a duplicated seam column/row.
///
/// The parametric `torus_mesh` shares the seam vertices through `% segments`,
/// so its last quad interpolates U from 63/64 back to 0 — the whole texture
/// squeezed backwards into one quad. That is invisible under 0..1 clamp and
/// a hard seam under repeat, so the metric variant duplicates the seam instead.
fn torus_mesh_metric(
    mesh_id: &str,
    material_id: &str,
    major_radius: f32,
    minor_radius: f32,
    major_segments: usize,
    minor_segments: usize,
    metres_per_repeat: f32,
) -> MeshPacket {
    let mut positions = Vec::with_capacity((major_segments + 1) * (minor_segments + 1));
    let mut normals = Vec::with_capacity((major_segments + 1) * (minor_segments + 1));
    let mut uv0 = Vec::with_capacity((major_segments + 1) * (minor_segments + 1));
    let mut indices = Vec::with_capacity(major_segments * minor_segments * 6);
    for major in 0..=major_segments {
        let major_angle = std::f32::consts::TAU * major as f32 / major_segments as f32;
        let (major_sin, major_cos) = major_angle.sin_cos();
        for minor in 0..=minor_segments {
            let minor_angle = std::f32::consts::TAU * minor as f32 / minor_segments as f32;
            let (minor_sin, minor_cos) = minor_angle.sin_cos();
            let radius = major_radius + minor_radius * minor_cos;
            positions.push([
                radius * major_cos,
                radius * major_sin,
                minor_radius * minor_sin,
            ]);
            normals.push([minor_cos * major_cos, minor_cos * major_sin, minor_sin]);
            // U along the tube's centre line, V around the tube.
            uv0.push([
                major_angle * major_radius / metres_per_repeat,
                minor_angle * minor_radius / metres_per_repeat,
            ]);
        }
    }
    let row = minor_segments + 1;
    for major in 0..major_segments {
        for minor in 0..minor_segments {
            let first = (major * row + minor) as u32;
            let second = ((major + 1) * row + minor) as u32;
            let third = ((major + 1) * row + minor + 1) as u32;
            let fourth = (major * row + minor + 1) as u32;
            indices.extend([first, second, third, first, third, fourth]);
        }
    }
    MeshPacket {
        mesh_id: mesh_id.into(),
        positions_m: positions,
        normals,
        uv0,
        indices,
        material_id: material_id.into(),
        tangents: Vec::new(),
    }
}

/// Planar metric UVs for a mesh whose instance is scaled by `scale_xyz`.
///
/// Used for meshes whose faces own their vertices (disc fans, block faces), so
/// a per-vertex projection cannot tear a shared vertex. The projection plane is
/// chosen per vertex from its dominant normal axis, and coordinates are scaled
/// into WORLD metres first: a unit block stretched to 0.30 x 1.75 x 0.62 m must
/// not carry a 5.8:1 texel stretch just because its UVs were authored in
/// object space.
fn with_planar_metric_uv(mut mesh: MeshPacket, scale_xyz: [f32; 3], metres_per_repeat: f32) -> MeshPacket {
    mesh.uv0 = mesh
        .positions_m
        .iter()
        .zip(&mesh.normals)
        .map(|(position, normal)| {
            let world = [
                position[0] * scale_xyz[0],
                position[1] * scale_xyz[1],
                position[2] * scale_xyz[2],
            ];
            let [nx, ny, nz] = normal.map(f32::abs);
            let (a, b) = if ny >= nx && ny >= nz {
                (world[0], world[2])
            } else if nx >= nz {
                (world[2], world[1])
            } else {
                (world[0], world[1])
            };
            [a / metres_per_repeat, b / metres_per_repeat]
        })
        .collect();
    mesh
}

/// Ellipsoid with metric UVs: U along the equator, V along the meridian, one
/// repeat per `metres_per_repeat`. Latitude/longitude topology still pinches
/// at the poles (analytic area-weighted p90 anisotropy 2.29 for a sphere); a
/// cube-sphere would remove that, but the crowns are placeholder geometry
/// scheduled for replacement by imported trees (audit N-5/N-6).
fn ellipsoid_mesh_metric(
    mesh_id: &str,
    material_id: &str,
    radii: [f32; 3],
    longitude_segments: usize,
    latitude_segments: usize,
    metres_per_repeat: f32,
) -> MeshPacket {
    let mut mesh = ellipsoid_mesh(mesh_id, material_id, radii, longitude_segments, latitude_segments);
    let equator = std::f32::consts::TAU * 0.5 * (radii[0] + radii[2]);
    // Meridian length of an ellipse with semi-axes (horizontal, vertical),
    // Ramanujan's approximation halved (pole to pole).
    let (a, b) = (0.5 * (radii[0] + radii[2]), radii[1]);
    let h = ((a - b) * (a - b)) / ((a + b) * (a + b));
    let half_perimeter =
        0.5 * std::f32::consts::PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()));
    for uv in &mut mesh.uv0 {
        *uv = [uv[0] * equator / metres_per_repeat, uv[1] * half_perimeter / metres_per_repeat];
    }
    mesh
}

fn disc_mesh(mesh_id: &str, material_id: &str, segments: usize) -> MeshPacket {
    let mut positions = Vec::with_capacity(segments * 3);
    let mut normals = Vec::with_capacity(segments * 3);
    let mut uv0 = Vec::with_capacity(segments * 3);
    let mut indices = Vec::with_capacity(segments * 3);
    let segments_f = segments as f32;
    for segment in 0..segments {
        let first_angle = std::f32::consts::TAU * segment as f32 / segments_f;
        let second_angle = std::f32::consts::TAU * (segment + 1) as f32 / segments_f;
        let first = [first_angle.cos(), 0.0, first_angle.sin()];
        let second = [second_angle.cos(), 0.0, second_angle.sin()];
        let first_index = positions.len() as u32;
        positions.extend([[0.0, 0.0, 0.0], first, second]);
        normals.extend([[0.0, 1.0, 0.0]; 3]);
        uv0.extend([
            [0.5, 0.5],
            [0.5 + first[0] * 0.5, 0.5 + first[2] * 0.5],
            [0.5 + second[0] * 0.5, 0.5 + second[2] * 0.5],
        ]);
        indices.extend([first_index, first_index + 1, first_index + 2]);
    }
    MeshPacket {
        mesh_id: mesh_id.into(),
        positions_m: positions,
        normals,
        uv0,
        indices,
        material_id: material_id.into(),
        tangents: Vec::new(),
    }
}

fn ring_mesh(
    mesh_id: &str,
    material_id: &str,
    inner_radius: f32,
    outer_radius: f32,
    segments: usize,
) -> MeshPacket {
    let mut positions = Vec::with_capacity(segments * 4);
    let mut normals = Vec::with_capacity(segments * 4);
    let mut uv0 = Vec::with_capacity(segments * 4);
    let mut indices = Vec::with_capacity(segments * 6);
    let segments_f = segments as f32;
    for segment in 0..segments {
        let first_angle = std::f32::consts::TAU * segment as f32 / segments_f;
        let second_angle = std::f32::consts::TAU * (segment + 1) as f32 / segments_f;
        let vertices = [
            [
                outer_radius * first_angle.cos(),
                0.0,
                outer_radius * first_angle.sin(),
            ],
            [
                inner_radius * first_angle.cos(),
                0.0,
                inner_radius * first_angle.sin(),
            ],
            [
                inner_radius * second_angle.cos(),
                0.0,
                inner_radius * second_angle.sin(),
            ],
            [
                outer_radius * second_angle.cos(),
                0.0,
                outer_radius * second_angle.sin(),
            ],
        ];
        let first_index = positions.len() as u32;
        positions.extend(vertices);
        normals.extend([[0.0, 1.0, 0.0]; 4]);
        uv0.extend([[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
        indices.extend([
            first_index,
            first_index + 2,
            first_index + 1,
            first_index,
            first_index + 3,
            first_index + 2,
        ]);
    }
    MeshPacket {
        mesh_id: mesh_id.into(),
        positions_m: positions,
        normals,
        uv0,
        indices,
        material_id: material_id.into(),
        tangents: Vec::new(),
    }
}

fn ellipsoid_mesh(
    mesh_id: &str,
    material_id: &str,
    radii: [f32; 3],
    longitude_segments: usize,
    latitude_segments: usize,
) -> MeshPacket {
    let mut positions = Vec::with_capacity((longitude_segments + 1) * (latitude_segments + 1));
    let mut normals = Vec::with_capacity((longitude_segments + 1) * (latitude_segments + 1));
    let mut uv0 = Vec::with_capacity((longitude_segments + 1) * (latitude_segments + 1));
    let mut indices = Vec::with_capacity(longitude_segments * latitude_segments * 6);
    for latitude in 0..=latitude_segments {
        let v = latitude as f32 / latitude_segments as f32;
        let polar = std::f32::consts::PI * v;
        let sin_polar = polar.sin();
        let cos_polar = polar.cos();
        for longitude in 0..=longitude_segments {
            let u = longitude as f32 / longitude_segments as f32;
            let azimuth = std::f32::consts::TAU * u;
            let sin_azimuth = azimuth.sin();
            let cos_azimuth = azimuth.cos();
            let unit = [sin_polar * cos_azimuth, cos_polar, sin_polar * sin_azimuth];
            positions.push([radii[0] * unit[0], radii[1] * unit[1], radii[2] * unit[2]]);
            let unnormalized_normal = [
                unit[0] / radii[0].max(0.0001),
                unit[1] / radii[1].max(0.0001),
                unit[2] / radii[2].max(0.0001),
            ];
            let normal_length = unnormalized_normal
                .iter()
                .map(|value| value * value)
                .sum::<f32>()
                .sqrt()
                .max(0.0001);
            normals.push([
                unnormalized_normal[0] / normal_length,
                unnormalized_normal[1] / normal_length,
                unnormalized_normal[2] / normal_length,
            ]);
            uv0.push([u, v]);
        }
    }
    for latitude in 0..latitude_segments {
        for longitude in 0..longitude_segments {
            let row = latitude * (longitude_segments + 1);
            let next_row = (latitude + 1) * (longitude_segments + 1);
            let first = (row + longitude) as u32;
            let second = (row + longitude + 1) as u32;
            let third = (next_row + longitude + 1) as u32;
            let fourth = (next_row + longitude) as u32;
            indices.extend([first, second, third, first, third, fourth]);
        }
    }
    MeshPacket {
        mesh_id: mesh_id.into(),
        positions_m: positions,
        normals,
        uv0,
        indices,
        material_id: material_id.into(),
        tangents: Vec::new(),
    }
}

fn campaign2_terrain_height(packet: &GraphicsScenePacket, x: f64, z: f64) -> f32 {
    let cell = nearest_cell(
        f64::from(packet.body.terrain.width_m),
        f64::from(packet.body.terrain.length_m),
        packet.body.terrain.resolution,
        [x, z],
    );
    match &packet.body.terrain.heights_m.payload {
        BufferPayload::F32(values) => values.get(cell).copied().unwrap_or(0.0),
        BufferPayload::U8(_) | BufferPayload::U32(_) => 0.0,
    }
}

fn campaign2_view_name(view: Campaign2View) -> &'static str {
    match view {
        Campaign2View::Close => "close",
        Campaign2View::Medium => "medium",
        Campaign2View::Wide => "wide",
    }
}

/// The three fixed inspection cuts used by Campaign 2.  They are intentionally
/// a small closed set rather than a free-form camera API: the resulting
/// evidence remains reproducible and each cut has a known perceptual job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Campaign2View {
    Close,
    Medium,
    Wide,
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

struct BeaconMeshBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uv0: Vec<[f32; 2]>,
    indices: Vec<u32>,
    segments: usize,
    /// `None` = the historical parametric UVs (each band V 0..1, U 0..1 around
    /// the circumference). `Some(m)` = metric UVs, one texture repeat per `m`
    /// metres along both the circumference and the profile (audit MD-4).
    metric_repeat_m: Option<f32>,
    /// Profile arc length already consumed by earlier bands, metres. Metric V
    /// continues across bands so stacked bands share one texture field.
    profile_length_m: f32,
}

impl BeaconMeshBuffers {
    fn with_capacity(segments: usize) -> Self {
        Self {
            positions: Vec::with_capacity(segments * 4 * 3 + segments * 6),
            normals: Vec::with_capacity(segments * 4 * 3 + segments * 6),
            uv0: Vec::with_capacity(segments * 4 * 3 + segments * 6),
            indices: Vec::with_capacity(segments * 6 * 4),
            segments,
            metric_repeat_m: None,
            profile_length_m: 0.0,
        }
    }

    fn with_metric_uv(segments: usize, metres_per_repeat: f32) -> Self {
        let mut buffers = Self::with_capacity(segments);
        buffers.metric_repeat_m = Some(metres_per_repeat);
        buffers
    }

    fn into_mesh(self, mesh_id: &str, material_id: &str) -> MeshPacket {
        MeshPacket {
            mesh_id: mesh_id.into(),
            positions_m: self.positions,
            normals: self.normals,
            uv0: self.uv0,
            indices: self.indices,
            material_id: material_id.into(),
            tangents: Vec::new(),
        }
    }

    fn append_band(&mut self, lower_y: f32, upper_y: f32, lower_radius: f32, upper_radius: f32) {
        let segments_f = self.segments as f32;
        for segment in 0..self.segments {
            let first_angle = std::f32::consts::TAU * segment as f32 / segments_f;
            let second_angle = std::f32::consts::TAU * (segment + 1) as f32 / segments_f;
            let first_u = segment as f32 / segments_f;
            let second_u = (segment + 1) as f32 / segments_f;
            let radial_delta = upper_radius - lower_radius;
            let vertical_delta = upper_y - lower_y;
            let smooth_normal = |angle: f32| {
                let candidate = [
                    vertical_delta * angle.cos(),
                    -radial_delta,
                    vertical_delta * angle.sin(),
                ];
                let length = candidate
                    .iter()
                    .map(|value| value * value)
                    .sum::<f32>()
                    .sqrt()
                    .max(0.0001);
                [
                    candidate[0] / length,
                    candidate[1] / length,
                    candidate[2] / length,
                ]
            };
            let face = [
                [
                    lower_radius * first_angle.cos(),
                    lower_y,
                    lower_radius * first_angle.sin(),
                ],
                [
                    lower_radius * second_angle.cos(),
                    lower_y,
                    lower_radius * second_angle.sin(),
                ],
                [
                    upper_radius * second_angle.cos(),
                    upper_y,
                    upper_radius * second_angle.sin(),
                ],
                [
                    upper_radius * first_angle.cos(),
                    upper_y,
                    upper_radius * first_angle.sin(),
                ],
            ];
            let first = self.positions.len() as u32;
            self.positions.extend(face);
            self.normals.extend([
                smooth_normal(first_angle),
                smooth_normal(second_angle),
                smooth_normal(second_angle),
                smooth_normal(first_angle),
            ]);
            if let Some(repeat) = self.metric_repeat_m {
                // Both rings are measured at the band's MEAN radius. Measuring
                // each ring at its own radius looks more exact but shears: the
                // U offset between the rings grows as angle x (r_lower -
                // r_upper), reaching 2*pi*dr at the seam (measured p90
                // anisotropy 7.5 on the hero core). The mean radius has no
                // shear and a density error bounded by r_edge / r_mean.
                let slant = (vertical_delta * vertical_delta + radial_delta * radial_delta).sqrt();
                let lower_v = self.profile_length_m / repeat;
                let upper_v = (self.profile_length_m + slant) / repeat;
                let mean_radius = 0.5 * (lower_radius + upper_radius);
                self.uv0.extend([
                    [first_angle * mean_radius / repeat, lower_v],
                    [second_angle * mean_radius / repeat, lower_v],
                    [second_angle * mean_radius / repeat, upper_v],
                    [first_angle * mean_radius / repeat, upper_v],
                ]);
            } else {
                self.uv0.extend([
                    [first_u, 0.0],
                    [second_u, 0.0],
                    [second_u, 1.0],
                    [first_u, 1.0],
                ]);
            }
            self.indices
                .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
        }
        let radial_delta = upper_radius - lower_radius;
        let vertical_delta = upper_y - lower_y;
        self.profile_length_m +=
            (vertical_delta * vertical_delta + radial_delta * radial_delta).sqrt();
    }

    fn append_cap(&mut self, y: f32, radius: f32, top: bool) {
        let normal = if top {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, -1.0, 0.0]
        };
        let center = [0.0, y, 0.0];
        let segments_f = self.segments as f32;
        for segment in 0..self.segments {
            let first_angle = std::f32::consts::TAU * segment as f32 / segments_f;
            let second_angle = std::f32::consts::TAU * (segment + 1) as f32 / segments_f;
            let first = [radius * first_angle.cos(), y, radius * first_angle.sin()];
            let second = [radius * second_angle.cos(), y, radius * second_angle.sin()];
            let vertices = if top {
                [center, first, second]
            } else {
                [center, second, first]
            };
            let uvs = if let Some(repeat) = self.metric_repeat_m {
                // Planar metric mapping; the order follows `vertices`.
                vertices.map(|vertex| [vertex[0] / repeat, vertex[2] / repeat])
            } else {
                let first_uv = [
                    0.5 + first[0] / (radius * 2.0),
                    0.5 + first[2] / (radius * 2.0),
                ];
                let second_uv = [
                    0.5 + second[0] / (radius * 2.0),
                    0.5 + second[2] / (radius * 2.0),
                ];
                [[0.5, 0.5], first_uv, second_uv]
            };
            self.append_triangle(vertices, normal, uvs);
        }
    }

    fn append_triangle(&mut self, vertices: [[f32; 3]; 3], normal: [f32; 3], uvs: [[f32; 2]; 3]) {
        let first = self.positions.len() as u32;
        self.positions.extend(vertices);
        self.normals.extend([normal; 3]);
        self.uv0.extend(uvs);
        self.indices.extend([first, first + 1, first + 2]);
    }
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
    const WIDTH: usize = 128;
    const HEIGHT: usize = 128;

    let mut bytes = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for row in 0..HEIGHT {
        for column in 0..WIDTH {
            let u = column as f32 / WIDTH as f32;
            let v = row as f32 / HEIGHT as f32;
            let broad = terrain_value_noise(u, v, 4);
            let medium = terrain_value_noise(u, v, 13);
            let fine = terrain_value_noise(u, v, 37);
            let grain = terrain_hash(column as u32, row as u32);
            let mineral_cell = terrain_hash((column / 2) as u32, (row / 2) as u32);
            let mineral_fleck = if mineral_cell > 0.82 {
                -0.30
            } else if mineral_cell < 0.04 {
                0.19
            } else {
                0.0
            };
            let tone = (0.5
                + (broad - 0.5) * 0.72
                + (medium - 0.5) * 0.44
                + (fine - 0.5) * 0.58
                + (grain - 0.5) * 0.30
                + mineral_fleck)
                .clamp(0.0, 1.0);

            // A restrained earth-and-moss albedo gives the existing neutral
            // terrain surface visible macro, micro, and sparse mineral-fleck
            // structure. It encodes no biome, region, or gameplay meaning;
            // those remain in the independently bound world fields.
            let channels = [
                (78.0 + tone * 90.0).round() as u8,
                (88.0 + tone * 76.0).round() as u8,
                (58.0 + tone * 54.0).round() as u8,
            ];
            bytes.extend([channels[0], channels[1], channels[2], 255]);
        }
    }

    procedural_texture(
        "riverwatch-terrain-albedo-v4",
        "procedural-riverwatch-terrain-albedo-v4",
        TextureColorSpace::Srgb,
        WIDTH as u32,
        HEIGHT as u32,
        bytes,
    )
}

fn terrain_hash(x: u32, y: u32) -> f32 {
    let mut value = x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b) ^ 0xc2b2_ae35;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    value as f32 / u32::MAX as f32
}

fn terrain_value_noise(u: f32, v: f32, cells: u32) -> f32 {
    let x = u * cells as f32;
    let y = v * cells as f32;
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let tx = x - x.floor();
    let ty = y - y.floor();
    let tx = tx * tx * (3.0 - 2.0 * tx);
    let ty = ty * ty * (3.0 - 2.0 * ty);
    let sample = |sample_x: u32, sample_y: u32| terrain_hash(sample_x % cells, sample_y % cells);
    let top = sample(x0, y0) * (1.0 - tx) + sample(x0 + 1, y0) * tx;
    let bottom = sample(x0, y0 + 1) * (1.0 - tx) + sample(x0 + 1, y0 + 1) * tx;
    top * (1.0 - ty) + bottom * ty
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
        "procedural-riverwatch-foliage-albedo-v2",
        [74, 148, 44],
        [30, 90, 20],
    )
}

fn procedural_beacon_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "riverwatch-beacon-albedo",
        "procedural-riverwatch-beacon-albedo-v1",
        [72, 86, 96],
        [132, 116, 78],
    )
}

fn procedural_showcase_stone_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "riverwatch-showcase-stone-albedo",
        "procedural-riverwatch-showcase-stone-albedo-v1",
        [150, 162, 172],
        [220, 230, 236],
    )
}

fn procedural_showcase_metal_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "riverwatch-showcase-metal-albedo",
        "procedural-riverwatch-showcase-metal-albedo-v1",
        [172, 84, 25],
        [240, 170, 70],
    )
}

fn procedural_showcase_glow_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "riverwatch-showcase-glow-albedo",
        "procedural-riverwatch-showcase-glow-albedo-v1",
        [30, 110, 145],
        [75, 210, 228],
    )
}

fn procedural_showcase_emissive_texture() -> TextureReference {
    let width = 16;
    let height = 16;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            let seam = row % 8 == 0 || column % 8 == 0;
            let diagonal = (row + column) % 11 == 0;
            bytes.extend(if seam || diagonal {
                [42, 188, 226, 255]
            } else {
                [0, 0, 0, 255]
            });
        }
    }
    procedural_texture(
        "riverwatch-showcase-emissive",
        "procedural-riverwatch-showcase-emissive-v1",
        TextureColorSpace::Srgb,
        width as u32,
        height as u32,
        bytes,
    )
}

fn procedural_campaign2_terrain_albedo_texture() -> TextureReference {
    const WIDTH: usize = 160;
    const HEIGHT: usize = 160;
    let mut bytes = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for row in 0..HEIGHT {
        for column in 0..WIDTH {
            let u = column as f32 / WIDTH as f32;
            let v = row as f32 / HEIGHT as f32;
            let broad = terrain_value_noise(u, v, 5);
            let medium = terrain_value_noise(u, v, 17);
            let fine = terrain_value_noise(u, v, 61);
            let ridge = (medium - 0.5).abs() * 2.0;
            let dry = ((u * 7.0 + v * 5.0 + broad * 2.2).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
            let moss = (0.62 + (broad - 0.5) * 0.58 + (fine - 0.5) * 0.22).clamp(0.0, 1.0);
            let rock = (ridge * 0.54 + dry * 0.14).clamp(0.0, 1.0);
            let base = [58.0, 86.0, 46.0];
            let soil = [100.0, 74.0, 46.0];
            let stone = [112.0, 115.0, 108.0];
            let grass_weight = moss * (1.0 - rock * 0.72);
            let soil_weight = ((1.0 - grass_weight) * (1.0 - rock) * 0.82).clamp(0.0, 1.0);
            let stone_weight = 1.0 - grass_weight - soil_weight;
            let grain = (terrain_hash(column as u32, row as u32) - 0.5) * 10.0;
            let pebble_hash = terrain_hash((column / 3) as u32, (row / 3) as u32);
            let pebble = if pebble_hash > 0.965 {
                24.0
            } else if pebble_hash < 0.035 {
                -18.0
            } else {
                0.0
            };
            let red = (base[0] * grass_weight
                + soil[0] * soil_weight
                + stone[0] * stone_weight
                + grain
                + pebble)
                .clamp(0.0, 255.0) as u8;
            let green = (base[1] * grass_weight
                + soil[1] * soil_weight
                + stone[1] * stone_weight
                + grain * 0.72)
                .clamp(0.0, 255.0) as u8;
            let blue = (base[2] * grass_weight
                + soil[2] * soil_weight
                + stone[2] * stone_weight
                + grain * 0.45)
                .clamp(0.0, 255.0) as u8;
            bytes.extend([red, green, blue, 255]);
        }
    }
    procedural_texture(
        "wge-campaign2-terrain-albedo",
        "procedural-wge-campaign2-terrain-albedo-v1",
        TextureColorSpace::Srgb,
        WIDTH as u32,
        HEIGHT as u32,
        bytes,
    )
}

fn procedural_campaign2_wet_albedo_texture() -> TextureReference {
    const WIDTH: usize = 32;
    const HEIGHT: usize = 32;
    let mut bytes = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for row in 0..HEIGHT {
        for column in 0..WIDTH {
            let u = column as f32 / WIDTH as f32;
            let v = row as f32 / HEIGHT as f32;
            let ripple = ((u * 18.0 + (v * 7.0).sin() * 1.8).sin() * 0.5 + 0.5).powf(7.0);
            let glint = if (column * 13 + row * 7) % 29 == 0 {
                0.42
            } else {
                0.0
            };
            let value = (0.34 + ripple * 0.34 + glint).clamp(0.0, 1.0);
            bytes.extend([
                (value * 110.0) as u8,
                (value * 164.0) as u8,
                (value * 214.0) as u8,
                255,
            ]);
        }
    }
    procedural_texture(
        "wge-campaign2-wet-albedo",
        "procedural-wge-campaign2-wet-albedo-v1",
        TextureColorSpace::Srgb,
        WIDTH as u32,
        HEIGHT as u32,
        bytes,
    )
}

fn procedural_campaign2_foliage_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "wge-campaign2-foliage-albedo",
        "procedural-wge-campaign2-foliage-albedo-v1",
        [52, 96, 30],
        [132, 174, 58],
    )
}

fn procedural_campaign2_bark_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "wge-campaign2-bark-albedo",
        "procedural-wge-campaign2-bark-albedo-v1",
        [76, 48, 28],
        [154, 96, 52],
    )
}

fn procedural_campaign2_hero_stone_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "wge-campaign2-hero-stone-albedo",
        "procedural-wge-campaign2-hero-stone-albedo-v1",
        [122, 134, 144],
        [208, 216, 220],
    )
}

fn procedural_campaign2_hero_metal_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "wge-campaign2-hero-metal-albedo",
        "procedural-wge-campaign2-hero-metal-albedo-v1",
        [194, 94, 28],
        [252, 196, 86],
    )
}

fn procedural_campaign2_hero_glow_albedo_texture() -> TextureReference {
    procedural_material_albedo_texture(
        "wge-campaign2-hero-glow-albedo",
        "procedural-wge-campaign2-hero-glow-albedo-v1",
        [18, 84, 116],
        [84, 220, 232],
    )
}

fn procedural_campaign2_emissive_texture() -> TextureReference {
    const WIDTH: usize = 32;
    const HEIGHT: usize = 32;
    let mut bytes = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for row in 0..HEIGHT {
        for column in 0..WIDTH {
            let band = (13..=18).contains(&row) || column % 16 == 0;
            let pulse = ((row * 5 + column * 11) % 31) == 0;
            bytes.extend(if band || pulse {
                [42, 190, 232, 255]
            } else {
                [0, 0, 0, 255]
            });
        }
    }
    procedural_texture(
        "wge-campaign2-emissive",
        "procedural-wge-campaign2-emissive-v1",
        TextureColorSpace::Srgb,
        WIDTH as u32,
        HEIGHT as u32,
        bytes,
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

fn procedural_beacon_emissive_texture() -> TextureReference {
    let width = 16;
    let height = 16;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            let band = (6..=9).contains(&row);
            let sparkle = (row * 7 + column * 11) % 19 == 0;
            bytes.extend(if band || sparkle {
                [255, 112, 22, 255]
            } else {
                [0, 0, 0, 255]
            });
        }
    }
    procedural_texture(
        "riverwatch-beacon-emissive",
        "procedural-riverwatch-beacon-emissive-v1",
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
        // Depth attachments are used internally for occlusion, but depth
        // evidence is not part of the promoted color-only receipt yet.
        include_depth: false,
        deterministic: true,
    };

    let mut overlays = Vec::new();
    let terrain_albedo_texture = procedural_terrain_albedo_texture();
    let stone_albedo_texture = procedural_stone_albedo_texture();
    let foliage_albedo_texture = procedural_foliage_albedo_texture();
    let beacon_albedo_texture = procedural_beacon_albedo_texture();
    let normal_texture = procedural_normal_texture();
    let roughness_texture = procedural_roughness_texture();
    let occlusion_texture = procedural_occlusion_texture();
    let emissive_texture = procedural_emissive_texture();
    let beacon_emissive_texture = procedural_beacon_emissive_texture();
    let normal_texture_id = Some(normal_texture.texture_id.clone());
    let roughness_texture_id = Some(roughness_texture.texture_id.clone());
    let occlusion_texture_id = Some(occlusion_texture.texture_id.clone());
    let emissive_texture_id = Some(emissive_texture.texture_id.clone());
    let beacon_emissive_texture_id = Some(beacon_emissive_texture.texture_id.clone());
    let [objective_x, objective_z] = layout.traversal.objective_position_xz_m;
    // The native adapter applies fog as a linear distance weight. A fixed
    // density of 0.006 makes the authored 160 m reference camera render at
    // almost the maximum 92% fog contribution, erasing terrain and material
    // contrast. Keep a small, bounded haze at the camera's authored distance.
    let fog_density = finite_f32(
        (REFERENCE_FOG_WEIGHT_AT_CAMERA / layout.reference_camera.distance_m)
            .min(MAX_REFERENCE_FOG_DENSITY),
        "reference fog density",
    )?;
    let objective_cell = nearest_cell(
        layout.width_m,
        layout.length_m,
        resolution,
        [objective_x, objective_z],
    );
    let mut materials = vec![MaterialIntent {
        material_id: "terrain-default".into(),
        base_color_rgba: [0.29, 0.38, 0.28, 1.0],
        metallic: 0.0,
        roughness: 0.92,
        clearcoat: 0.0,
        clearcoat_roughness: 0.5,
        alpha_mode: AlphaMode::Opaque,
        texture_ids: vec![terrain_albedo_texture.texture_id.clone()],
        normal_texture_id: normal_texture_id.clone(),
        roughness_texture_id: roughness_texture_id.clone(),
        occlusion_texture_id: occlusion_texture_id.clone(),
        emissive_texture_id: emissive_texture_id.clone(),
        normal_scale: 0.5,
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
            clearcoat: 0.12,
            clearcoat_roughness: 0.32,
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
            base_color_rgba: [0.38, 0.62, 0.18, 1.0],
            metallic: 0.0,
            roughness: 0.88,
            clearcoat: 0.0,
            clearcoat_roughness: 0.5,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![foliage_albedo_texture.texture_id.clone()],
            normal_texture_id: normal_texture_id.clone(),
            roughness_texture_id: roughness_texture_id.clone(),
            occlusion_texture_id: occlusion_texture_id.clone(),
            emissive_texture_id: emissive_texture_id.clone(),
            normal_scale: 0.22,
            occlusion_strength: 0.5,
            emissive_factor_rgb: [0.006, 0.020, 0.002],
        });
        meshes.push(foliage_mesh());
        instances.extend(foliage_instances);
    }
    materials.push(MaterialIntent {
        material_id: "objective-beacon".into(),
        base_color_rgba: [0.24, 0.29, 0.34, 1.0],
        metallic: 0.45,
        roughness: 0.48,
        clearcoat: 0.35,
        clearcoat_roughness: 0.18,
        alpha_mode: AlphaMode::Opaque,
        texture_ids: vec![beacon_albedo_texture.texture_id.clone()],
        normal_texture_id: normal_texture_id.clone(),
        roughness_texture_id: roughness_texture_id.clone(),
        occlusion_texture_id: occlusion_texture_id.clone(),
        emissive_texture_id: beacon_emissive_texture_id,
        normal_scale: 0.7,
        occlusion_strength: 0.85,
        emissive_factor_rgb: [0.85, 0.3, 0.035],
    });
    meshes.push(objective_beacon_mesh());
    instances.push(InstancePacket {
        instance_id: "objective-beacon".into(),
        mesh_id: "objective-beacon".into(),
        material_id: "objective-beacon".into(),
        importance: InstanceImportance::Landmark,
        transform: Transform3d {
            translation_xyz_m: [
                finite_f32(objective_x, "objective x")?,
                finite_f32(
                    world.body.fields.heights_m[objective_cell],
                    "objective height",
                )?,
                finite_f32(objective_z, "objective z")?,
            ],
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            scale_xyz: [1.0, 1.0, 1.0],
        },
    });
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
        deformation: None,
        render_policy: None,
        packet_id: format!(
            "graphics-packet-{}",
            world.artifact_id.trim_start_matches("world-")
        ),
        scene_artifact_id: None,
        scene_artifact_sha256: None,
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
            layers: None,
        },
        materials,
        textures: vec![
            terrain_albedo_texture,
            stone_albedo_texture,
            foliage_albedo_texture,
            beacon_albedo_texture,
            normal_texture,
            roughness_texture,
            occlusion_texture,
            emissive_texture,
            beacon_emissive_texture,
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
            fog_density,
            exposure: 1.0,
        },
        overlays,
        capture,
    };
    seal_scene_packet(body)
}

/// Derive a deterministic close-range material inspection view from a valid
/// overview packet without changing the semantic world or asset identities.
pub fn lower_objective_close_packet(
    packet: &GraphicsScenePacket,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    validate_scene_packet(packet)?;
    let beacon = packet
        .body
        .instances
        .iter()
        .find(|instance| instance.instance_id == "objective-beacon")
        .ok_or_else(|| {
            GraphicsContractError::provenance(
                "objective close view requires the lowered objective beacon instance",
            )
        })?;
    let [beacon_x, beacon_y, beacon_z] = beacon.transform.translation_xyz_m;
    let mut body = packet.body.clone();
    body.packet_id = format!("{}-objective-close", body.packet_id);
    body.camera = GraphicsCamera {
        camera_id: "objective-close".into(),
        projection: CameraProjection::Perspective {
            fov_y_degrees: 48.0,
        },
        position_xyz_m: [beacon_x, beacon_y + 5.0, beacon_z + 8.0],
        forward_xyz: [0.0, -0.3511234, -0.9363292],
        up_xyz: [0.0, 0.9363292, -0.3511234],
        near_plane_m: 0.1,
        far_plane_m: 128.0,
        width_px: packet.body.camera.width_px,
        height_px: packet.body.camera.height_px,
    };
    body.overlays.clear();
    body.capture.capture_id = format!("{}-objective-close", body.capture.capture_id);
    body.capture.camera_id = body.camera.camera_id.clone();
    seal_scene_packet(body)
}

fn normalize_vector3(vector: [f32; 3], label: &str) -> Result<[f32; 3], GraphicsContractError> {
    let length_squared = vector.iter().map(|value| value * value).sum::<f32>();
    if !length_squared.is_finite() || length_squared <= f32::EPSILON {
        return Err(GraphicsContractError::malformed(format!(
            "{label} is degenerate"
        )));
    }
    let inverse_length = length_squared.sqrt().recip();
    Ok([
        vector[0] * inverse_length,
        vector[1] * inverse_length,
        vector[2] * inverse_length,
    ])
}

fn cross_vector3(first: [f32; 3], second: [f32; 3]) -> [f32; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

/// Derive a deterministic authored showcase composition from a validated
/// semantic packet.  The extra geometry is a visual-quality probe, not new
/// gameplay/world authority: world identity, spatial fields, and the source
/// packet remain bound to the same artifact.
pub fn lower_showcase_packet(
    packet: &GraphicsScenePacket,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    validate_scene_packet(packet)?;
    let beacon = packet
        .body
        .instances
        .iter()
        .find(|instance| instance.instance_id == "objective-beacon")
        .ok_or_else(|| {
            GraphicsContractError::provenance(
                "showcase view requires the lowered objective beacon instance",
            )
        })?;
    let [beacon_x, beacon_y, beacon_z] = beacon.transform.translation_xyz_m;
    let target = [beacon_x, beacon_y + 2.5, beacon_z];
    let position = [beacon_x + 4.0, beacon_y + 7.5, beacon_z + 7.0];
    let forward = normalize_vector3(
        [
            target[0] - position[0],
            target[1] - position[1],
            target[2] - position[2],
        ],
        "showcase camera forward",
    )?;
    let right = normalize_vector3(
        cross_vector3(forward, [0.0, 1.0, 0.0]),
        "showcase camera right",
    )?;
    let up = normalize_vector3(cross_vector3(right, forward), "showcase camera up")?;

    let mut body = packet.body.clone();
    body.packet_id = format!("{}-showcase", body.packet_id);
    body.camera = GraphicsCamera {
        camera_id: "native-showcase".into(),
        projection: CameraProjection::Perspective {
            fov_y_degrees: 58.0,
        },
        position_xyz_m: position,
        forward_xyz: forward,
        up_xyz: up,
        near_plane_m: 0.1,
        far_plane_m: 256.0,
        width_px: 640,
        height_px: 480,
    };
    body.capture.capture_id = format!("{}-showcase", body.capture.capture_id);
    body.capture.camera_id = body.camera.camera_id.clone();
    body.capture.width_px = body.camera.width_px;
    body.capture.height_px = body.camera.height_px;
    body.overlays.clear();
    body.instances
        .retain(|instance| instance.instance_id == "objective-beacon");

    let showcase_stone_texture = procedural_showcase_stone_albedo_texture();
    let showcase_metal_texture = procedural_showcase_metal_albedo_texture();
    let showcase_glow_texture = procedural_showcase_glow_albedo_texture();
    let showcase_emissive_texture = procedural_showcase_emissive_texture();
    body.textures.extend([
        showcase_stone_texture.clone(),
        showcase_metal_texture.clone(),
        showcase_glow_texture.clone(),
        showcase_emissive_texture.clone(),
    ]);
    let common_normal = Some("riverwatch-normal".to_owned());
    let common_roughness = Some("riverwatch-roughness".to_owned());
    let common_occlusion = Some("riverwatch-occlusion".to_owned());
    if !body
        .textures
        .iter()
        .any(|texture| texture.texture_id == "riverwatch-normal")
        || !body
            .textures
            .iter()
            .any(|texture| texture.texture_id == "riverwatch-roughness")
        || !body
            .textures
            .iter()
            .any(|texture| texture.texture_id == "riverwatch-occlusion")
    {
        return Err(GraphicsContractError::provenance(
            "showcase requires the canonical normal, roughness, and occlusion textures",
        ));
    }
    body.materials.extend([
        MaterialIntent {
            material_id: "showcase-stone".into(),
            base_color_rgba: [0.80, 0.84, 0.88, 1.0],
            metallic: 0.08,
            roughness: 0.58,
            clearcoat: 0.18,
            clearcoat_roughness: 0.24,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![showcase_stone_texture.texture_id.clone()],
            normal_texture_id: common_normal.clone(),
            roughness_texture_id: common_roughness.clone(),
            occlusion_texture_id: common_occlusion.clone(),
            emissive_texture_id: None,
            normal_scale: 0.7,
            occlusion_strength: 0.82,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
        },
        MaterialIntent {
            material_id: "showcase-metal".into(),
            base_color_rgba: [0.82, 0.38, 0.08, 1.0],
            metallic: 0.92,
            roughness: 0.22,
            clearcoat: 0.32,
            clearcoat_roughness: 0.12,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![showcase_metal_texture.texture_id.clone()],
            normal_texture_id: common_normal.clone(),
            roughness_texture_id: common_roughness.clone(),
            occlusion_texture_id: common_occlusion.clone(),
            emissive_texture_id: None,
            normal_scale: 0.35,
            occlusion_strength: 0.66,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
        },
        MaterialIntent {
            material_id: "showcase-glow".into(),
            base_color_rgba: [0.30, 0.72, 0.85, 1.0],
            metallic: 0.28,
            roughness: 0.30,
            clearcoat: 0.08,
            clearcoat_roughness: 0.20,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![showcase_glow_texture.texture_id.clone()],
            normal_texture_id: common_normal,
            roughness_texture_id: common_roughness,
            occlusion_texture_id: common_occlusion,
            emissive_texture_id: Some(showcase_emissive_texture.texture_id.clone()),
            normal_scale: 0.45,
            occlusion_strength: 0.55,
            emissive_factor_rgb: [0.04, 0.32, 0.55],
        },
    ]);
    body.meshes.extend([
        radial_mesh(
            "showcase-plinth",
            "showcase-stone",
            12,
            &[
                (0.0, 0.18, 3.8, 3.8),
                (0.18, 0.34, 3.8, 3.35),
                (0.34, 0.55, 3.35, 3.25),
                (0.55, 0.72, 3.25, 2.75),
                (0.72, 0.92, 2.75, 2.62),
            ],
            Some(3.8),
            Some(2.62),
        ),
        radial_mesh(
            "showcase-column",
            "showcase-stone",
            8,
            &[
                (0.0, 0.22, 0.92, 0.92),
                (0.22, 0.42, 0.92, 0.70),
                (0.42, 4.85, 0.70, 0.70),
                (4.85, 5.08, 0.70, 0.92),
            ],
            Some(0.92),
            Some(0.92),
        ),
        torus_mesh("showcase-halo", "showcase-metal", 2.95, 0.20, 32, 10),
        torus_mesh("showcase-rune-ring", "showcase-glow", 2.22, 0.085, 32, 8),
        block_mesh("showcase-lintel", "showcase-stone"),
        block_mesh("showcase-beacon-inlay", "showcase-glow"),
        radial_mesh(
            "showcase-collar",
            "showcase-metal",
            12,
            &[(0.0, 0.18, 1.48, 1.55), (0.18, 0.36, 1.55, 1.38)],
            Some(1.48),
            Some(1.38),
        ),
        radial_mesh(
            "showcase-rock",
            "showcase-stone",
            8,
            &[
                (0.0, 0.18, 1.25, 1.35),
                (0.18, 0.82, 1.35, 0.92),
                (0.82, 1.12, 0.92, 0.28),
            ],
            Some(1.25),
            Some(0.28),
        ),
    ]);

    let identity = [0.0, 0.0, 0.0, 1.0];
    let landmark = InstanceImportance::Landmark;
    body.instances.extend([
        InstancePacket {
            instance_id: "showcase-plinth".into(),
            mesh_id: "showcase-plinth".into(),
            material_id: "showcase-stone".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x, beacon_y, beacon_z],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "showcase-column-left".into(),
            mesh_id: "showcase-column".into(),
            material_id: "showcase-stone".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x - 3.0, beacon_y, beacon_z - 0.55],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "showcase-column-right".into(),
            mesh_id: "showcase-column".into(),
            material_id: "showcase-stone".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x + 3.0, beacon_y, beacon_z - 0.55],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "showcase-lintel".into(),
            mesh_id: "showcase-lintel".into(),
            material_id: "showcase-stone".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x, beacon_y + 4.92, beacon_z - 0.55],
                rotation_xyzw: identity,
                scale_xyz: [3.3, 0.34, 0.55],
            },
        },
        InstancePacket {
            instance_id: "showcase-halo".into(),
            mesh_id: "showcase-halo".into(),
            material_id: "showcase-metal".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x, beacon_y + 4.65, beacon_z - 0.70],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "showcase-rune-ring".into(),
            mesh_id: "showcase-rune-ring".into(),
            material_id: "showcase-glow".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x, beacon_y + 0.95, beacon_z],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "showcase-collar".into(),
            mesh_id: "showcase-collar".into(),
            material_id: "showcase-metal".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x, beacon_y + 0.35, beacon_z],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "showcase-beacon-inlay".into(),
            mesh_id: "showcase-beacon-inlay".into(),
            material_id: "showcase-glow".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x, beacon_y + 1.42, beacon_z + 0.73],
                rotation_xyzw: identity,
                scale_xyz: [0.12, 1.25, 0.035],
            },
        },
        // A few explicitly named dressing stones give the inspection profile
        // real foreground/midground structure without changing gameplay
        // obstacles, navigation, or the authored world artifact.
        InstancePacket {
            instance_id: "showcase-rock-left".into(),
            mesh_id: "showcase-rock".into(),
            material_id: "showcase-stone".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x - 6.0, beacon_y, beacon_z - 1.5],
                rotation_xyzw: [0.0, 0.18, 0.0, 0.984],
                scale_xyz: [1.35, 0.92, 1.10],
            },
        },
        InstancePacket {
            instance_id: "showcase-rock-right".into(),
            mesh_id: "showcase-rock".into(),
            material_id: "showcase-stone".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x + 5.2, beacon_y, beacon_z + 1.0],
                rotation_xyzw: [0.0, -0.22, 0.0, 0.976],
                scale_xyz: [0.84, 0.68, 0.92],
            },
        },
        InstancePacket {
            instance_id: "showcase-rock-back".into(),
            mesh_id: "showcase-rock".into(),
            material_id: "showcase-stone".into(),
            importance: landmark,
            transform: Transform3d {
                translation_xyz_m: [beacon_x - 7.0, beacon_y, beacon_z + 4.5],
                rotation_xyzw: [0.0, 0.36, 0.0, 0.933],
                scale_xyz: [1.55, 1.05, 1.25],
            },
        },
    ]);
    if let Some(beacon_material) = body
        .materials
        .iter_mut()
        .find(|material| material.material_id == "objective-beacon")
    {
        beacon_material.base_color_rgba = [0.62, 0.52, 0.28, 1.0];
        beacon_material.texture_ids = vec![showcase_metal_texture.texture_id.clone()];
        beacon_material.metallic = 0.68;
        beacon_material.roughness = 0.32;
        beacon_material.emissive_factor_rgb = [0.30, 0.07, 0.008];
    }
    if let Some(light) = body.lights.first_mut() {
        light.intensity = 3.2;
        light.color_rgb = [1.0, 0.94, 0.86];
        if let LightKind::Directional { direction_xyz } = &mut light.kind {
            *direction_xyz = [0.20, -1.0, -0.25];
        }
    }
    body.environment = EnvironmentIntent {
        sky_top_rgb: [0.025, 0.055, 0.12],
        sky_horizon_rgb: [0.18, 0.28, 0.40],
        ground_rgb: [0.045, 0.060, 0.085],
        fog_color_rgb: [0.070, 0.105, 0.150],
        fog_density: 0.0008,
        exposure: 1.12,
    };
    seal_scene_packet(body)
}

/// Compose the authored world around the deterministic shrine probe. This is
/// a quality-proof profile, not a new semantic world: the source world
/// instances remain intact, the showcase geometry is explicitly named, and
/// the packet stays bound to the same artifact and spatial-field digests.
pub fn lower_world_showcase_packet(
    packet: &GraphicsScenePacket,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    validate_scene_packet(packet)?;
    let source_instances = packet.body.instances.clone();
    let mut composed = lower_showcase_packet(packet)?;
    let beacon = composed
        .body
        .instances
        .iter()
        .find(|instance| instance.instance_id == "objective-beacon")
        .ok_or_else(|| {
            GraphicsContractError::provenance(
                "world showcase requires the lowered objective beacon instance",
            )
        })?;
    let [beacon_x, beacon_y, beacon_z] = beacon.transform.translation_xyz_m;
    let target = [beacon_x - 2.0, beacon_y + 3.0, beacon_z - 4.0];
    // Keep the inspection composition close enough that the authored terrain
    // remains a substantial, measurable part of the frame after projected
    // authored meshes are conservatively excluded from the quality mask.
    let position = [beacon_x + 13.0, beacon_y + 8.0, beacon_z + 18.0];
    let forward = normalize_vector3(
        [
            target[0] - position[0],
            target[1] - position[1],
            target[2] - position[2],
        ],
        "world showcase camera forward",
    )?;
    let right = normalize_vector3(
        cross_vector3(forward, [0.0, 1.0, 0.0]),
        "world showcase camera right",
    )?;
    let up = normalize_vector3(cross_vector3(right, forward), "world showcase camera up")?;

    let mut authored_instances = source_instances
        .into_iter()
        .filter(|instance| instance.instance_id != "objective-beacon")
        .collect::<Vec<_>>();
    authored_instances.extend(composed.body.instances);
    composed.body.instances = authored_instances;
    composed.body.packet_id = format!("{}-world-showcase", packet.body.packet_id);
    composed.body.camera = GraphicsCamera {
        camera_id: "native-world-showcase".into(),
        projection: CameraProjection::Perspective {
            fov_y_degrees: 58.0,
        },
        position_xyz_m: position,
        forward_xyz: forward,
        up_xyz: up,
        near_plane_m: 0.1,
        far_plane_m: 256.0,
        width_px: 768,
        height_px: 512,
    };
    composed.body.capture.capture_id = format!("{}-world-showcase", packet.body.capture.capture_id);
    composed.body.capture.camera_id = composed.body.camera.camera_id.clone();
    composed.body.capture.width_px = composed.body.camera.width_px;
    composed.body.capture.height_px = composed.body.camera.height_px;
    composed.body.overlays.clear();
    seal_scene_packet(composed.body)
}

/// Lower the first Campaign 2 authored-frame slice.  This is deliberately a
/// visual composition over the validated world packet: semantic identity,
/// terrain fields, and gameplay-critical instance identity remain inherited
/// from the source packet, while all calibration geometry is explicit,
/// deterministic, and content-addressed in the derived packet.
/// Candidate render policy for the graphical parity A/B experiments.
///
/// Named after the gap it attacks rather than after a look, because a policy
/// that encodes "Demo A's art style" is exactly the failure the sprint forbids
/// (GP-01's whole point: style arrives through policy, but only policy the
/// renderer genuinely implements, and only as a *candidate* until human review
/// accepts it).
///
/// Unset = OFF = the frozen baseline, byte-identical. This is a producer flag;
/// validation does not consult it, for the same reason `deformation_v7_enabled`
/// is producer-only: a receiver must be able to check a packet it did not
/// produce.
pub const PARITY_POLICY_ENV: &str = "WGE_PARITY_RENDER_POLICY";

/// Which candidate policy a run should attach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParityPolicyCandidate {
    /// No policy section at all: the frozen baseline renderer behaviour.
    Null,
    /// Anti-banding only (sprint §3.1). The single highest perceived-gain
    /// change in the audit, so it is measured alone before anything rides
    /// along with it.
    Dither,
    /// Dither plus a restrained post chain (sprint §3.5).
    DitherPost,
    /// Shadow rework alone (sprint §5.2): removes the hard-coded 0.25
    /// direct-light visibility floor and widens the PCF tap spread. Measured on
    /// its own so a shadow judgement is never contaminated by post or dither.
    Shadow,
    /// Hero material set alone (goal step 1): gives every material its OWN
    /// 256x256 albedo/normal/roughness/occlusion derived from one coherent
    /// height field, replacing the shared 8x8 `riverwatch-*` maps that were
    /// identical across grass, stone, bark, metal, and foliage.
    HeroMaterials,
    /// Terrain material scale alone (sprint §3.3): tiles the terrain albedo 24
    /// times across the field with a repeat wrap. Isolated so a texture-scale
    /// judgement is not contaminated by post, dither, or shadows.
    Terrain,
    /// Everything that is genuinely implemented: dither, post chain, the
    /// shadow rework, and terrain tiling.
    Full,
    /// `Full` plus the axis this renderer still genuinely REFUSES
    /// (anisotropy, blocked on the Vulkan device feature). Terrain tiling used
    /// to be in this arm and was removed once the sampler-per-surface split
    /// landed (sprint F-4); leaving it would have kept a now-supported axis
    /// behind a refusal and made `full` weaker than it needs to be.
    FullUnsupported,
    /// CONVERGE-0 (WGE_GRAPHICS_CONVERGENCE_AUDIT.md §H): `Full` plus the three
    /// axes the converge0 content cannot render without — mesh repeat wrap
    /// (metric UVs), a view-relative shadow fit (extended world), and the
    /// view-direction sky. Selecting this arm also selects converge0 CONTENT;
    /// the policy and content are only meaningful together, and
    /// `ParityContent::from_env` refuses converge0 content under any other arm.
    Converge0,
    /// CONVERGE-1 N-1: `Converge0` plus the analytic (Preetham) sky and the
    /// exponential height-dependent atmosphere, on the same converge0 content,
    /// so the only difference from `Converge0` is sky and aerial perspective.
    Converge1,
}

/// Content flags for the parity experiments, orthogonal to the render policy.
///
/// Audit MD-5: the hero material set used to be a value of the RENDER POLICY
/// variable, so it could not combine with `full` and was judged on the frozen
/// baseline. Content is now its own comma-separated variable.
///
/// Unset = no content change = the frozen baseline content, byte-identical.
pub const PARITY_CONTENT_ENV: &str = "WGE_PARITY_CONTENT";

/// Which content changes a run applies on top of the authored Campaign 2 frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParityContent {
    /// Per-family coherent material maps (`material_maps::apply_hero_material_set`).
    pub hero_materials: bool,
    /// CONVERGE-0 scene content: extended world with a backdrop ridge, daylight
    /// sky and low warm sun, metric mesh UVs, physical metallic values.
    pub converge0: bool,
}

impl ParityContent {
    /// Parse `WGE_PARITY_CONTENT` together with the policy arm it must agree
    /// with. Unknown tokens are refused rather than ignored: a typo that
    /// silently rendered the baseline would be a mislabelled experiment.
    pub fn from_env(policy: ParityPolicyCandidate) -> Result<Self, GraphicsContractError> {
        let raw = std::env::var(PARITY_CONTENT_ENV).unwrap_or_default();
        Self::parse(&raw, policy)
    }

    pub fn parse(raw: &str, policy: ParityPolicyCandidate) -> Result<Self, GraphicsContractError> {
        let mut content = Self::default();
        for token in raw.split(',').map(str::trim).filter(|token| !token.is_empty()) {
            match token {
                "hero-materials" => content.hero_materials = true,
                "converge0" => content.converge0 = true,
                other => {
                    return Err(GraphicsContractError::malformed(format!(
                        "{PARITY_CONTENT_ENV} token `{other}` is unknown; expected \
                         `hero-materials` and/or `converge0`"
                    )));
                }
            }
        }
        // The legacy spelling `WGE_PARITY_RENDER_POLICY=hero-materials` still
        // means "baseline renderer + hero content", so the archived `ab-hero3`
        // run stays reproducible.
        if policy == ParityPolicyCandidate::HeroMaterials {
            content.hero_materials = true;
        }
        if policy.uses_converge0_content() {
            content.converge0 = true;
        } else if content.converge0 {
            return Err(GraphicsContractError::malformed(format!(
                "converge0 content requires {PARITY_POLICY_ENV}=converge0 or converge1: its metric \
                 UVs need mesh repeat wrap and its extended world needs the \
                 view-relative shadow fit"
            )));
        }
        Ok(content)
    }
}

/// The terrain material-scale candidate: 8 tiles across the field.
///
/// 8000 milli = 8 repeats across the extent. On the 96 m riverwatch field that
/// is one texture repeat every 12 m — a believable ground scale, versus the
/// baseline's single 96 m stretch. `wrap_repeat` is REQUIRED alongside it: with
/// CLAMP_TO_EDGE the outer 7 of 8 tiles would smear the border texel instead of
/// wrapping, which is why the contract enforces the pair.
const TERRAIN_TILING_CANDIDATE: TerrainSurfacePolicy = TerrainSurfacePolicy {
    uv_repeat_scale_milli: 8000,
    wrap_repeat: true,
    macro_variation_bp: 0,
    macro_frequency_milli: 40,
};

/// CONVERGE-0 world extension factor. The source terrain grid is embedded at
/// the centre of a grid `CONVERGE0_TERRAIN_SCALE` times larger on each axis.
/// It must be ODD so every source sample lands exactly on an extended grid
/// point: (N-1)*k+1 points over k times the extent keeps the 2.0 m / 1.5 m
/// spacing, so source heights are copied, never resampled.
pub const CONVERGE0_TERRAIN_SCALE: usize = 5;

/// converge0 terrain surface policy (superseded the 30-repeat extent tiling of
/// CONVERGE-0 once N-4 gave every layer a physical repeat size).
const CONVERGE0_TERRAIN_TILING: TerrainSurfacePolicy = TerrainSurfacePolicy {
    // Layered terrain (N-4) tiles each layer at its scan's physical size, so
    // the extent-relative repeat stays at identity; wrap is required because
    // every layer UV is metric and exceeds 1.0.
    uv_repeat_scale_milli: 1000,
    wrap_repeat: true,
    // ±20% albedo variation on a ~67 m period: large enough to break the
    // 2-3 m tile repetition at distance, small enough not to read as blotches.
    macro_variation_bp: 2000,
    macro_frequency_milli: 15,
};

/// CONVERGE-1 sky: the converge0 sun disc on the analytic (Preetham) model.
pub const CONVERGE1_SKY: SkyPolicy = SkyPolicy {
    sun_disc_radius_milli_deg: 650,
    sun_disc_gain_bp: 120_000,
    sun_glow_gain_bp: 2_400,
    model: Some(SkyModel::Analytic { turbidity_milli: 3000 }),
};

/// CONVERGE-1 atmosphere (values are tuned by measurement; see
/// WGE_CONVERGE1_CONTRACTS.md §1 implementation notes).
pub const CONVERGE1_ATMOSPHERE: AtmospherePolicy = AtmospherePolicy {
    height_falloff_milli_per_m: 10,
    density_at_ground_bp: 45,
    sun_scatter_gain_bp: 300,
};

/// Shadow distance for the converge0 view-relative fit. 60 m covers every
/// authored object in the wide view while keeping the 512² map at ~4 texels/m.
pub const CONVERGE0_SHADOW_DISTANCE_M: i32 = 60;

impl ParityPolicyCandidate {
    /// Arms that render the converge0 world (extended terrain, metric UVs,
    /// layered scanned ground). Their content and policy are only valid together.
    pub fn uses_converge0_content(self) -> bool {
        matches!(self, Self::Converge0 | Self::Converge1)
    }

    pub fn from_env() -> Self {
        match std::env::var(PARITY_POLICY_ENV).as_deref() {
            Ok("dither") => Self::Dither,
            Ok("dither-post") => Self::DitherPost,
            Ok("full") => Self::Full,
            Ok("full-unsupported") => Self::FullUnsupported,
            Ok("shadow") => Self::Shadow,
            Ok("terrain") => Self::Terrain,
            Ok("hero-materials") => Self::HeroMaterials,
            Ok("converge0") => Self::Converge0,
            Ok("converge1") => Self::Converge1,
            _ => Self::Null,
        }
    }

    /// Build the typed policy this candidate represents.
    pub fn policy(self) -> Option<RenderPolicy> {
        match self {
            // Explicitly NULL, not "defaults": the point of this arm is to
            // prove that ABSENT produces the baseline bytes. Attaching an
            // all-defaults policy would move the packet digest and stop being
            // that control.
            Self::Null => None,
            Self::Dither => Some(RenderPolicy {
                dither: Some(DitherPolicy {
                    amplitude_milli_lsb: 1000,
                }),
                ..RenderPolicy::default()
            }),
            Self::DitherPost => Some(RenderPolicy {
                dither: Some(DitherPolicy {
                    amplitude_milli_lsb: 1000,
                }),
                bloom: Some(BloomPolicy {
                    threshold_bp: 8200,
                    intensity_bp: 1200,
                }),
                vignette: Some(VignettePolicy {
                    strength_bp: 1800,
                    radius_bp: 6200,
                    softness_bp: 3600,
                }),
                ..RenderPolicy::default()
            }),
            Self::Full | Self::FullUnsupported => Some(RenderPolicy {
                dither: Some(DitherPolicy {
                    amplitude_milli_lsb: 1000,
                }),
                bloom: Some(BloomPolicy {
                    threshold_bp: 8200,
                    intensity_bp: 1200,
                }),
                vignette: Some(VignettePolicy {
                    strength_bp: 1800,
                    radius_bp: 6200,
                    softness_bp: 3600,
                }),
                shadow: Some(ShadowPolicy {
                    darkness_bp: 9600,
                    filter_radius_milli: 2400,
                }),
                // Only the refusal arm asks for an axis the renderer cannot
                // honour. `Full` must stay renderable, or "full" would be a
                // name for a configuration that always errors.
                ..if matches!(self, Self::FullUnsupported) {
                    RenderPolicy {
                        sampler: Some(SamplerPolicy { anisotropy: 8 }),
                        ..RenderPolicy::default()
                    }
                } else {
                    RenderPolicy {
                        terrain_surface: Some(TERRAIN_TILING_CANDIDATE),
                        ..RenderPolicy::default()
                    }
                }
            }),
            Self::Converge0 => {
                let full = Self::Full.policy().expect("Full always carries a policy");
                Some(RenderPolicy {
                    terrain_surface: Some(CONVERGE0_TERRAIN_TILING),
                    mesh_surface: Some(MeshSurfacePolicy { wrap_repeat: true }),
                    shadow_fit: Some(ShadowFitPolicy {
                        view_distance_m: CONVERGE0_SHADOW_DISTANCE_M,
                    }),
                    sky: Some(SkyPolicy {
                        sun_disc_radius_milli_deg: 650,
                        sun_disc_gain_bp: 120_000,
                        sun_glow_gain_bp: 2_400,
                        model: None,
                    }),
                    ..full
                })
            }
            Self::Converge1 => {
                let converge0 = Self::Converge0.policy().expect("Converge0 always carries a policy");
                Some(RenderPolicy {
                    sky: Some(CONVERGE1_SKY),
                    atmosphere: Some(CONVERGE1_ATMOSPHERE),
                    ..converge0
                })
            }
            Self::HeroMaterials => Some(RenderPolicy::default()),
            Self::Terrain => Some(RenderPolicy {
                terrain_surface: Some(TERRAIN_TILING_CANDIDATE),
                ..RenderPolicy::default()
            }),
            Self::Shadow => Some(RenderPolicy {
                shadow: Some(ShadowPolicy {
                    // 0.96 floor removal: a fully occluded sample keeps 4% of
                    // direct light instead of 25%.
                    darkness_bp: 9600,
                    // 2.4 texels of PCF spread instead of 1.0 — softer edges
                    // without pretending to be PCSS.
                    filter_radius_milli: 2400,
                }),
                ..RenderPolicy::default()
            }),
        }
    }
}

/// CONVERGE-0 scene content (WGE_GRAPHICS_CONVERGENCE_AUDIT.md §H steps 5-7).
///
/// Every change here is CONTENT, gated by `ParityContent::converge0`, so the
/// null arm keeps the frozen bytes:
///
///  * the world no longer ends inside the frame (extended terrain + backdrop
///    ridge high enough that every camera ray below the horizon meets ground);
///  * daylight sky, a low warm sun (25° elevation, same azimuth, so forms get
///    long shadows and side light), and fog whose colour IS the horizon colour
///    so distance fades into the sky instead of into a different grey;
///  * metallic is 0 or 1 (audit MD-6): water and stone are dielectrics.
fn apply_converge0_content(body: &mut GraphicsScenePacketBody) -> Result<(), GraphicsContractError> {
    extend_terrain_with_backdrop(&mut body.terrain)?;
    // The backdrop ridge sits up to ~300 m from the wide camera; 256 m clipped it.
    body.camera.far_plane_m = 1500.0;
    body.environment = EnvironmentIntent {
        sky_top_rgb: [0.16, 0.30, 0.58],
        sky_horizon_rgb: [0.62, 0.66, 0.72],
        ground_rgb: [0.11, 0.12, 0.085],
        // Slightly bluer and darker than the horizon: distance fades TOWARD the
        // sky without the backdrop silhouette dissolving into it.
        fog_color_rgb: [0.52, 0.58, 0.67],
        fog_density: 0.0011,
        exposure: 1.08,
    };
    if let Some(light) = body.lights.first_mut() {
        light.intensity = 5.0;
        light.color_rgb = [1.0, 0.82, 0.62];
        if let LightKind::Directional { direction_xyz } = &mut light.kind {
            // Same azimuth as the authored key light, horizontal magnitude
            // 1/tan(25°) = 2.1445 for a 25° elevation.
            *direction_xyz = [1.6407, -1.0, -1.3810];
        }
    }
    for material in &mut body.materials {
        let metallic = match material.material_id.as_str() {
            "campaign2-hero-metal" => 1.0,
            "campaign2-wet" | "campaign2-hero-glow" | "objective-beacon" | "obstacle-default"
            | "campaign2-hero-stone" | "campaign2-rock" => 0.0,
            _ => continue,
        };
        material.metallic = metallic;
    }
    Ok(())
}

/// Deterministic 2D value noise in world metres, independent of any texture
/// period, for the backdrop landforms.
fn backdrop_noise(x: f32, z: f32, wavelength_m: f32, seed: u32) -> f32 {
    let fx = x / wavelength_m;
    let fz = z / wavelength_m;
    let x0 = fx.floor();
    let z0 = fz.floor();
    let tx = fx - x0;
    let tz = fz - z0;
    let tx = tx * tx * (3.0 - 2.0 * tx);
    let tz = tz * tz * (3.0 - 2.0 * tz);
    let hash = |ix: i32, iz: i32| {
        let mut h = (ix as u32).wrapping_mul(0x27d4_eb2d)
            ^ (iz as u32).wrapping_mul(0x1656_67b1)
            ^ seed.wrapping_mul(0x9e37_79b9);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2c1b_3c6d);
        h ^= h >> 12;
        (h >> 8) as f32 / 16_777_216.0
    };
    let (ix, iz) = (x0 as i32, z0 as i32);
    let top = hash(ix, iz) * (1.0 - tx) + hash(ix + 1, iz) * tx;
    let bottom = hash(ix, iz + 1) * (1.0 - tx) + hash(ix + 1, iz + 1) * tx;
    top * (1.0 - tz) + bottom * tz
}

fn smoothstep_range(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Embed the source terrain in a `CONVERGE0_TERRAIN_SCALE`-times larger grid.
///
/// Inside the source rectangle every height, slope, and region value is copied
/// verbatim (the odd scale makes the grids coincide exactly). Outside it the
/// surface blends from the nearest source edge height into rolling hills over
/// the first 50 m, then rises into a backdrop ridge between 50 m and 140 m out.
/// The ridge has to stand above every camera: a finite world seen from 11 m up
/// otherwise shows a band of void between its far edge and the true horizon.
fn extend_terrain_with_backdrop(terrain: &mut TerrainPacket) -> Result<(), GraphicsContractError> {
    let source_heights = match &terrain.heights_m.payload {
        BufferPayload::F32(values) => values.clone(),
        BufferPayload::U8(_) | BufferPayload::U32(_) => {
            return Err(GraphicsContractError::unsupported(
                "converge0 terrain extension requires f32 heights",
            ));
        }
    };
    let source_slope = match &terrain.slope_grade.payload {
        BufferPayload::F32(values) => values.clone(),
        _ => {
            return Err(GraphicsContractError::unsupported(
                "converge0 terrain extension requires f32 slope grade",
            ));
        }
    };
    let source_regions = match &terrain.region_codes.payload {
        BufferPayload::U8(values) => values.clone(),
        _ => {
            return Err(GraphicsContractError::unsupported(
                "converge0 terrain extension requires u8 region codes",
            ));
        }
    };
    let n = terrain.resolution;
    let k = CONVERGE0_TERRAIN_SCALE;
    let extended = (n - 1) * k + 1;
    let offset = (n - 1) * (k - 1) / 2;
    let dx = terrain.width_m / (n - 1) as f32;
    let dz = terrain.length_m / (n - 1) as f32;
    let width_m = terrain.width_m * k as f32;
    let length_m = terrain.length_m * k as f32;
    let base_level = source_heights.iter().sum::<f32>() / source_heights.len() as f32;
    let inside = |index: usize| index >= offset && index < offset + n;

    let mut heights = vec![0.0f32; extended * extended];
    for row in 0..extended {
        for column in 0..extended {
            let index = row * extended + column;
            if inside(row) && inside(column) {
                heights[index] = source_heights[(row - offset) * n + (column - offset)];
                continue;
            }
            let clamped_row = row.clamp(offset, offset + n - 1);
            let clamped_column = column.clamp(offset, offset + n - 1);
            let edge_height = source_heights[(clamped_row - offset) * n + (clamped_column - offset)];
            let out_x = (column as f32 - clamped_column as f32) * dx;
            let out_z = (row as f32 - clamped_row as f32) * dz;
            let distance = (out_x * out_x + out_z * out_z).sqrt();
            // World position, matching the adapter's centred layout.
            let world_x = (column as f32 / (extended - 1) as f32 - 0.5) * width_m;
            let world_z = (0.5 - row as f32 / (extended - 1) as f32) * length_m;
            let hills = (backdrop_noise(world_x, world_z, 70.0, 0x51ed_2701) - 0.5) * 9.0
                + (backdrop_noise(world_x, world_z, 31.0, 0x1b87_3f2d) - 0.5) * 3.5;
            let ridge_shape = 1.0
                - (backdrop_noise(world_x, world_z, 95.0, 0x2f8b_1c77) * 2.0 - 1.0).abs();
            // 26 m: the lowest ridge that still stands above every campaign2
            // camera (eye heights 5-11 m above the hero ground) at the 0.6
            // noise floor. 55 m (first pass) filled the upper half of the wide
            // frame with a fogged wall and hid the sky.
            let ridge = 26.0 * smoothstep_range(50.0, 140.0, distance) * (0.6 + 0.4 * ridge_shape);
            let edge_weight = 1.0 - smoothstep_range(0.0, 50.0, distance);
            heights[index] =
                edge_weight * edge_height + (1.0 - edge_weight) * (base_level + hills) + ridge;
        }
    }

    // Slope: copied inside; rise/run central differences outside, which is the
    // unit the source field uses (verified against the source to within 0.1).
    let mut slope = vec![0.0f32; extended * extended];
    let mut regions = vec![0u8; extended * extended];
    for row in 0..extended {
        for column in 0..extended {
            let index = row * extended + column;
            if inside(row) && inside(column) {
                let source = (row - offset) * n + (column - offset);
                slope[index] = source_slope[source];
                regions[index] = source_regions[source];
                continue;
            }
            let left = heights[row * extended + column.saturating_sub(1)];
            let right = heights[row * extended + (column + 1).min(extended - 1)];
            let up = heights[row.saturating_sub(1) * extended + column];
            let down = heights[(row + 1).min(extended - 1) * extended + column];
            let gx = (right - left) / (2.0 * dx);
            let gz = (down - up) / (2.0 * dz);
            slope[index] = (gx * gx + gz * gz).sqrt();
        }
    }

    terrain.width_m = width_m;
    terrain.length_m = length_m;
    terrain.resolution = extended;
    terrain.heights_m = BufferReference::inline_f32("terrain-heights-converge0", heights);
    terrain.slope_grade = BufferReference::inline_f32("terrain-slope-converge0", slope);
    terrain.region_codes = BufferReference::inline_u8("terrain-regions-converge0", regions);
    Ok(())
}

/// Instance scale of both hero fins. Named because the converge0 metric fin
/// mesh folds this scale into its UVs; the two must never drift apart.
const CAMPAIGN2_FIN_SCALE: [f32; 3] = [0.30, 1.75, 0.62];

pub fn lower_campaign2_packet(
    packet: &GraphicsScenePacket,
    view: Campaign2View,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    let candidate = ParityPolicyCandidate::from_env();
    let content = ParityContent::from_env(candidate)?;
    lower_campaign2_packet_with(packet, view, candidate, content, None)
}

/// `lower_campaign2_packet` with the parity arm passed explicitly instead of
/// read from the environment, so tests can lower several arms in one process
/// without racing on process-global environment variables.
pub fn lower_campaign2_packet_with(
    packet: &GraphicsScenePacket,
    view: Campaign2View,
    parity_candidate: ParityPolicyCandidate,
    parity_content: ParityContent,
    terrain_layers: Option<&TerrainLayerSet>,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    validate_scene_packet(packet)?;
    if parity_content.converge0 != parity_candidate.uses_converge0_content() {
        return Err(GraphicsContractError::malformed(
            "converge0 content and the converge0/converge1 render policies are only valid together",
        ));
    }
    // N-4: the converge0 world is surfaced by a scanned layer set, passed in
    // explicitly (never read from disk here) so the packet stays a pure
    // function of its inputs and the supervisor can re-derive it.
    if parity_content.converge0 != terrain_layers.is_some() {
        return Err(GraphicsContractError::provenance(if parity_content.converge0 {
            "converge0 content requires a terrain layer set (tools/terrain_layers/converge0.json)"
        } else {
            "a terrain layer set is only valid with converge0 content"
        }));
    }
    let objective = packet
        .body
        .instances
        .iter()
        .find(|instance| instance.instance_id == "objective-beacon")
        .ok_or_else(|| {
            GraphicsContractError::provenance(
                "Campaign 2 requires the lowered objective beacon anchor",
            )
        })?;
    let [objective_x, _objective_y, objective_z] = objective.transform.translation_xyz_m;
    // Keep the semantic objective instance intact and place the authored
    // calibration asset a short, deterministic distance beside it.  This
    // makes the hero asset independently inspectable without changing the
    // gameplay anchor or its source-world projection.
    let hero_x = objective_x - 5.5;
    let hero_z = objective_z - 2.0;
    let hero_y = campaign2_terrain_height(packet, f64::from(hero_x), f64::from(hero_z));
    let pool_x = hero_x - 3.8;
    let pool_z = hero_z + 2.2;
    let pool_y = campaign2_terrain_height(packet, f64::from(pool_x), f64::from(pool_z)) + 0.055;
    let view_name = campaign2_view_name(view);
    let mut body = packet.body.clone();
    body.packet_id = format!("{}-campaign2-{view_name}", packet.body.packet_id);
    body.overlays.clear();
    // Candidate policy attaches here. With the flag unset this is `None`, the
    // key is omitted from canonical JSON by `skip_serializing_if`, and the
    // packet digests are exactly the frozen baseline's.
    body.render_policy = parity_candidate.policy();
    // One texture repeat every 6 m on surface materials under converge0. The
    // campaign2 albedos are still 8x8 placeholder swatches; at 2 m (first pass)
    // their 25 cm texels read as polka dots on every surface. 6 m keeps the
    // smear fixed (MD-4) without magnifying placeholder content. Real physical
    // texture sizes belong to the scanned calibration materials, not here. The
    // emissive "glow" meshes keep parametric UVs on purpose: their 32x32
    // emissive stripe pattern is authored in UV space as a decal, and tiling it
    // would be a redesign, not a fix.
    let metric_repeat_m = parity_content.converge0.then_some(6.0f32);
    // Campaign 2 is an authored-frame calibration projection, not the
    // gameplay diagnostic view.  Keep the world/spatial identities bound but
    // fence the sparse source render instances (marker foliage, cube obstacle,
    // and legacy beacon) out of the calibration composition.  The semantic
    // instances remain authoritative in `WorldArtifact` and are exercised by
    // the separate world/runtime gates.
    body.instances.clear();

    let terrain_texture = procedural_campaign2_terrain_albedo_texture();
    let wet_texture = procedural_campaign2_wet_albedo_texture();
    let foliage_texture = procedural_campaign2_foliage_albedo_texture();
    let bark_texture = procedural_campaign2_bark_albedo_texture();
    let hero_stone_texture = procedural_campaign2_hero_stone_albedo_texture();
    let hero_metal_texture = procedural_campaign2_hero_metal_albedo_texture();
    let hero_glow_texture = procedural_campaign2_hero_glow_albedo_texture();
    let hero_emissive_texture = procedural_campaign2_emissive_texture();
    body.textures.extend([
        terrain_texture.clone(),
        wet_texture.clone(),
        foliage_texture.clone(),
        bark_texture.clone(),
        hero_stone_texture.clone(),
        hero_metal_texture.clone(),
        hero_glow_texture.clone(),
        hero_emissive_texture.clone(),
    ]);

    let normal_texture = Some("riverwatch-normal".to_owned());
    let roughness_texture = Some("riverwatch-roughness".to_owned());
    let occlusion_texture = Some("riverwatch-occlusion".to_owned());
    if !body
        .textures
        .iter()
        .any(|texture| texture.texture_id == "riverwatch-normal")
        || !body
            .textures
            .iter()
            .any(|texture| texture.texture_id == "riverwatch-roughness")
        || !body
            .textures
            .iter()
            .any(|texture| texture.texture_id == "riverwatch-occlusion")
    {
        return Err(GraphicsContractError::provenance(
            "Campaign 2 requires the canonical normal, roughness, and occlusion textures",
        ));
    }

    let terrain_material_id = body.terrain.material_id.clone();
    if let Some(terrain) = body
        .materials
        .iter_mut()
        .find(|material| material.material_id == terrain_material_id)
    {
        // The Campaign 2 texture is a complete albedo, so the packet tint is
        // deliberately near-white.  A dark tint here would multiply the
        // sRGB-decoded texture a second time and erase authored surface detail.
        terrain.base_color_rgba = [0.96, 0.98, 0.92, 1.0];
        terrain.roughness = 0.88;
        terrain.normal_scale = 0.72;
        terrain.occlusion_strength = 0.58;
        terrain.texture_ids = vec![terrain_texture.texture_id.clone()];
        terrain.normal_texture_id = normal_texture.clone();
        terrain.roughness_texture_id = roughness_texture.clone();
        terrain.occlusion_texture_id = occlusion_texture.clone();
        terrain.emissive_factor_rgb = [0.0, 0.0, 0.0];
    }
    if let Some(obstacle) = body
        .materials
        .iter_mut()
        .find(|material| material.material_id == "obstacle-default")
    {
        obstacle.base_color_rgba = [0.30, 0.31, 0.29, 1.0];
        obstacle.metallic = 0.08;
        obstacle.roughness = 0.82;
        obstacle.clearcoat = 0.08;
        obstacle.texture_ids = vec![hero_stone_texture.texture_id.clone()];
        obstacle.normal_texture_id = normal_texture.clone();
        obstacle.roughness_texture_id = roughness_texture.clone();
        obstacle.occlusion_texture_id = occlusion_texture.clone();
    }
    if let Some(objective_material) = body
        .materials
        .iter_mut()
        .find(|material| material.material_id == "objective-beacon")
    {
        // The gameplay beacon remains the same source instance, but its
        // campaign presentation uses the calibrated hero family so an
        // untouched diagnostic-orange fallback cannot dominate the composed
        // frame.
        objective_material.base_color_rgba = [0.34, 0.40, 0.45, 1.0];
        objective_material.metallic = 0.42;
        objective_material.roughness = 0.30;
        objective_material.clearcoat = 0.34;
        objective_material.clearcoat_roughness = 0.12;
        objective_material.texture_ids = vec![hero_stone_texture.texture_id.clone()];
        objective_material.normal_texture_id = normal_texture.clone();
        objective_material.roughness_texture_id = roughness_texture.clone();
        objective_material.occlusion_texture_id = occlusion_texture.clone();
        objective_material.emissive_texture_id = Some(hero_emissive_texture.texture_id.clone());
        objective_material.emissive_factor_rgb = [0.015, 0.12, 0.18];
    }
    if let Some(foliage_material) = body
        .materials
        .iter_mut()
        .find(|material| material.material_id == "foliage-default")
    {
        // The source semantic foliage markers remain in the packet for
        // projection identity, but the authored composition gives them a
        // restrained forest-floor palette so they do not compete with the
        // calibrated 3D foliage cluster.
        foliage_material.base_color_rgba = [0.12, 0.27, 0.055, 1.0];
        foliage_material.roughness = 0.86;
        foliage_material.texture_ids = vec![foliage_texture.texture_id.clone()];
        foliage_material.normal_texture_id = normal_texture.clone();
        foliage_material.roughness_texture_id = roughness_texture.clone();
        foliage_material.occlusion_texture_id = occlusion_texture.clone();
        foliage_material.emissive_factor_rgb = [0.0, 0.003, 0.0];
    }
    body.materials.extend([
        MaterialIntent {
            material_id: "campaign2-wet".into(),
            base_color_rgba: [0.92, 0.96, 1.0, 1.0],
            metallic: 0.34,
            roughness: 0.06,
            clearcoat: 0.86,
            clearcoat_roughness: 0.045,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![wet_texture.texture_id.clone()],
            normal_texture_id: normal_texture.clone(),
            roughness_texture_id: roughness_texture.clone(),
            occlusion_texture_id: occlusion_texture.clone(),
            emissive_texture_id: None,
            normal_scale: 0.28,
            occlusion_strength: 0.35,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
        },
        MaterialIntent {
            material_id: "campaign2-foliage".into(),
            base_color_rgba: [0.94, 0.98, 0.90, 1.0],
            metallic: 0.0,
            roughness: 0.76,
            clearcoat: 0.06,
            clearcoat_roughness: 0.30,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![foliage_texture.texture_id.clone()],
            normal_texture_id: normal_texture.clone(),
            roughness_texture_id: roughness_texture.clone(),
            occlusion_texture_id: occlusion_texture.clone(),
            emissive_texture_id: None,
            normal_scale: 0.34,
            occlusion_strength: 0.62,
            emissive_factor_rgb: [0.004, 0.012, 0.001],
        },
        MaterialIntent {
            material_id: "campaign2-bark".into(),
            base_color_rgba: [0.92, 0.86, 0.76, 1.0],
            metallic: 0.0,
            roughness: 0.84,
            clearcoat: 0.04,
            clearcoat_roughness: 0.35,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![bark_texture.texture_id.clone()],
            normal_texture_id: normal_texture.clone(),
            roughness_texture_id: roughness_texture.clone(),
            occlusion_texture_id: occlusion_texture.clone(),
            emissive_texture_id: None,
            normal_scale: 0.45,
            occlusion_strength: 0.72,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
        },
        MaterialIntent {
            material_id: "campaign2-hero-stone".into(),
            base_color_rgba: [0.70, 0.75, 0.80, 1.0],
            metallic: 0.06,
            roughness: 0.46,
            clearcoat: 0.24,
            clearcoat_roughness: 0.18,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![hero_stone_texture.texture_id.clone()],
            normal_texture_id: normal_texture.clone(),
            roughness_texture_id: roughness_texture.clone(),
            occlusion_texture_id: occlusion_texture.clone(),
            emissive_texture_id: None,
            normal_scale: 0.82,
            occlusion_strength: 0.80,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
        },
        MaterialIntent {
            material_id: "campaign2-hero-metal".into(),
            base_color_rgba: [0.98, 0.88, 0.72, 1.0],
            metallic: 0.78,
            roughness: 0.22,
            clearcoat: 0.46,
            clearcoat_roughness: 0.08,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![hero_metal_texture.texture_id.clone()],
            normal_texture_id: normal_texture.clone(),
            roughness_texture_id: roughness_texture.clone(),
            occlusion_texture_id: occlusion_texture.clone(),
            emissive_texture_id: None,
            normal_scale: 0.36,
            occlusion_strength: 0.70,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
        },
        MaterialIntent {
            material_id: "campaign2-hero-glow".into(),
            base_color_rgba: [0.80, 0.94, 1.0, 1.0],
            metallic: 0.24,
            roughness: 0.22,
            clearcoat: 0.16,
            clearcoat_roughness: 0.12,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![hero_glow_texture.texture_id.clone()],
            normal_texture_id: normal_texture.clone(),
            roughness_texture_id: roughness_texture.clone(),
            occlusion_texture_id: occlusion_texture.clone(),
            emissive_texture_id: Some(hero_emissive_texture.texture_id.clone()),
            normal_scale: 0.42,
            occlusion_strength: 0.48,
            emissive_factor_rgb: [0.05, 0.38, 0.62],
        },
        MaterialIntent {
            material_id: "campaign2-rock".into(),
            base_color_rgba: [0.92, 0.94, 0.90, 1.0],
            metallic: 0.03,
            roughness: 0.86,
            clearcoat: 0.08,
            clearcoat_roughness: 0.28,
            alpha_mode: AlphaMode::Opaque,
            texture_ids: vec![hero_stone_texture.texture_id.clone()],
            normal_texture_id: normal_texture.clone(),
            roughness_texture_id: roughness_texture,
            occlusion_texture_id: occlusion_texture,
            emissive_texture_id: None,
            normal_scale: 0.74,
            occlusion_strength: 0.82,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
        },
    ]);
    body.meshes.extend([
        match metric_repeat_m {
            Some(repeat) => with_planar_metric_uv(
                disc_mesh("campaign2-wet-pool", "campaign2-wet", 48),
                [3.8, 1.0, 2.7],
                repeat,
            ),
            None => disc_mesh("campaign2-wet-pool", "campaign2-wet", 48),
        },
        ring_mesh("campaign2-wet-ripple", "campaign2-hero-glow", 0.94, 1.0, 48),
        match metric_repeat_m {
            Some(repeat) => radial_mesh_metric(
                "campaign2-hero-pedestal",
                "campaign2-hero-stone",
                64,
                &[
                    (0.0, 0.16, 3.9, 3.9),
                    (0.16, 0.30, 3.9, 3.55),
                    (0.30, 0.46, 3.55, 3.45),
                    (0.46, 0.62, 3.45, 2.95),
                    (0.62, 0.78, 2.95, 2.75),
                ],
                Some(3.9),
                Some(2.75),
                repeat,
            ),
            None => radial_mesh(
                "campaign2-hero-pedestal",
                "campaign2-hero-stone",
                64,
                &[
                    (0.0, 0.16, 3.9, 3.9),
                    (0.16, 0.30, 3.9, 3.55),
                    (0.30, 0.46, 3.55, 3.45),
                    (0.46, 0.62, 3.45, 2.95),
                    (0.62, 0.78, 2.95, 2.75),
                ],
                Some(3.9),
                Some(2.75),
            ),
        },
        match metric_repeat_m {
            Some(repeat) => radial_mesh_metric(
                "campaign2-hero-core",
                "campaign2-hero-metal",
                64,
                &[
                    (0.0, 0.18, 1.72, 1.72),
                    (0.18, 0.38, 1.72, 1.48),
                    (0.38, 0.66, 1.48, 1.28),
                    (0.66, 2.95, 1.28, 1.04),
                    (2.95, 3.20, 1.04, 1.34),
                    (3.20, 3.42, 1.34, 1.18),
                    (3.42, 4.72, 1.18, 0.72),
                ],
                Some(1.72),
                Some(0.72),
                repeat,
            ),
            None => radial_mesh(
                "campaign2-hero-core",
                "campaign2-hero-metal",
                64,
                &[
                    (0.0, 0.18, 1.72, 1.72),
                    (0.18, 0.38, 1.72, 1.48),
                    (0.38, 0.66, 1.48, 1.28),
                    (0.66, 2.95, 1.28, 1.04),
                    (2.95, 3.20, 1.04, 1.34),
                    (3.20, 3.42, 1.34, 1.18),
                    (3.42, 4.72, 1.18, 0.72),
                ],
                Some(1.72),
                Some(0.72),
            ),
        },
        radial_mesh(
            "campaign2-hero-spire",
            "campaign2-hero-glow",
            32,
            &[
                (0.0, 0.18, 0.72, 0.72),
                (0.18, 0.42, 0.72, 0.48),
                (0.42, 1.75, 0.48, 0.26),
                (1.75, 2.22, 0.26, 0.12),
            ],
            Some(0.72),
            Some(0.12),
        ),
        match metric_repeat_m {
            Some(repeat) => torus_mesh_metric(
                "campaign2-hero-halo",
                "campaign2-hero-metal",
                2.28,
                0.16,
                64,
                16,
                repeat,
            ),
            None => torus_mesh(
                "campaign2-hero-halo",
                "campaign2-hero-metal",
                2.28,
                0.16,
                64,
                16,
            ),
        },
        torus_mesh(
            "campaign2-hero-rune-ring",
            "campaign2-hero-glow",
            1.62,
            0.075,
            64,
            12,
        ),
        match metric_repeat_m {
            // Both fin instances share one scale, so one metric mesh serves both.
            Some(repeat) => with_planar_metric_uv(
                block_mesh("campaign2-hero-fin", "campaign2-hero-stone"),
                CAMPAIGN2_FIN_SCALE,
                repeat,
            ),
            None => block_mesh("campaign2-hero-fin", "campaign2-hero-stone"),
        },
        block_mesh("campaign2-hero-inlay", "campaign2-hero-glow"),
        match metric_repeat_m {
            Some(repeat) => radial_mesh_metric(
                "campaign2-foliage-trunk",
                "campaign2-bark",
                24,
                &[(0.0, 0.12, 0.34, 0.34), (0.12, 1.65, 0.34, 0.22)],
                Some(0.34),
                Some(0.22),
                repeat,
            ),
            None => radial_mesh(
                "campaign2-foliage-trunk",
                "campaign2-bark",
                24,
                &[(0.0, 0.12, 0.34, 0.34), (0.12, 1.65, 0.34, 0.22)],
                Some(0.34),
                Some(0.22),
            ),
        },
        match metric_repeat_m {
            Some(repeat) => ellipsoid_mesh_metric(
                "campaign2-foliage-crown",
                "campaign2-foliage",
                [1.00, 1.15, 1.00],
                32,
                16,
                repeat,
            ),
            None => ellipsoid_mesh(
                "campaign2-foliage-crown",
                "campaign2-foliage",
                [1.00, 1.15, 1.00],
                32,
                16,
            ),
        },
        match metric_repeat_m {
            Some(repeat) => ellipsoid_mesh_metric(
                "campaign2-foliage-lobe",
                "campaign2-foliage",
                [0.72, 0.76, 0.64],
                24,
                12,
                repeat,
            ),
            None => ellipsoid_mesh(
                "campaign2-foliage-lobe",
                "campaign2-foliage",
                [0.72, 0.76, 0.64],
                24,
                12,
            ),
        },
        match metric_repeat_m {
            Some(repeat) => radial_mesh_metric(
                "campaign2-rock",
                "campaign2-rock",
                24,
                &[
                    (0.0, 0.18, 1.25, 1.42),
                    (0.18, 0.72, 1.42, 1.06),
                    (0.72, 1.18, 1.06, 0.42),
                ],
                Some(1.25),
                Some(0.42),
                repeat,
            ),
            None => radial_mesh(
                "campaign2-rock",
                "campaign2-rock",
                24,
                &[
                    (0.0, 0.18, 1.25, 1.42),
                    (0.18, 0.72, 1.42, 1.06),
                    (0.72, 1.18, 1.06, 0.42),
                ],
                Some(1.25),
                Some(0.42),
            ),
        },
    ]);

    let identity = [0.0, 0.0, 0.0, 1.0];
    body.instances.extend([
        InstancePacket {
            instance_id: "campaign2-hero-core".into(),
            mesh_id: "campaign2-hero-core".into(),
            material_id: "campaign2-hero-metal".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x, hero_y + 0.76, hero_z],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "campaign2-hero-pedestal".into(),
            mesh_id: "campaign2-hero-pedestal".into(),
            material_id: "campaign2-hero-stone".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x, hero_y, hero_z],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "campaign2-hero-spire".into(),
            mesh_id: "campaign2-hero-spire".into(),
            material_id: "campaign2-hero-glow".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x, hero_y + 4.94, hero_z],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "campaign2-hero-halo".into(),
            mesh_id: "campaign2-hero-halo".into(),
            material_id: "campaign2-hero-metal".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x, hero_y + 4.15, hero_z - 0.08],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "campaign2-hero-rune-ring".into(),
            mesh_id: "campaign2-hero-rune-ring".into(),
            material_id: "campaign2-hero-glow".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x, hero_y + 1.12, hero_z + 0.02],
                rotation_xyzw: identity,
                scale_xyz: [1.0, 1.0, 1.0],
            },
        },
        InstancePacket {
            instance_id: "campaign2-hero-fin-left".into(),
            mesh_id: "campaign2-hero-fin".into(),
            material_id: "campaign2-hero-stone".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x - 1.85, hero_y + 2.10, hero_z],
                rotation_xyzw: [0.0, 0.20, 0.0, 0.98],
                scale_xyz: CAMPAIGN2_FIN_SCALE,
            },
        },
        InstancePacket {
            instance_id: "campaign2-hero-fin-right".into(),
            mesh_id: "campaign2-hero-fin".into(),
            material_id: "campaign2-hero-stone".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x + 1.85, hero_y + 2.10, hero_z],
                rotation_xyzw: [0.0, -0.20, 0.0, 0.98],
                scale_xyz: CAMPAIGN2_FIN_SCALE,
            },
        },
        InstancePacket {
            instance_id: "campaign2-hero-inlay-left".into(),
            mesh_id: "campaign2-hero-inlay".into(),
            material_id: "campaign2-hero-glow".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x - 0.56, hero_y + 2.12, hero_z + 1.18],
                rotation_xyzw: identity,
                scale_xyz: [0.10, 0.92, 0.035],
            },
        },
        InstancePacket {
            instance_id: "campaign2-hero-inlay-right".into(),
            mesh_id: "campaign2-hero-inlay".into(),
            material_id: "campaign2-hero-glow".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x + 0.56, hero_y + 2.12, hero_z + 1.18],
                rotation_xyzw: identity,
                scale_xyz: [0.10, 0.92, 0.035],
            },
        },
        InstancePacket {
            instance_id: "campaign2-hero-inlay-center".into(),
            mesh_id: "campaign2-hero-inlay".into(),
            material_id: "campaign2-hero-glow".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [hero_x, hero_y + 3.02, hero_z + 1.22],
                rotation_xyzw: identity,
                scale_xyz: [0.68, 0.055, 0.035],
            },
        },
        InstancePacket {
            instance_id: "campaign2-wet-pool".into(),
            mesh_id: "campaign2-wet-pool".into(),
            material_id: "campaign2-wet".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [pool_x, pool_y, pool_z],
                rotation_xyzw: identity,
                scale_xyz: [3.8, 1.0, 2.7],
            },
        },
        InstancePacket {
            instance_id: "campaign2-wet-ripple-outer".into(),
            mesh_id: "campaign2-wet-ripple".into(),
            material_id: "campaign2-hero-glow".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [pool_x, pool_y + 0.008, pool_z],
                rotation_xyzw: identity,
                scale_xyz: [3.8, 1.0, 2.7],
            },
        },
        InstancePacket {
            instance_id: "campaign2-wet-ripple-inner".into(),
            mesh_id: "campaign2-wet-ripple".into(),
            material_id: "campaign2-hero-glow".into(),
            importance: InstanceImportance::Landmark,
            transform: Transform3d {
                translation_xyz_m: [pool_x, pool_y + 0.012, pool_z],
                rotation_xyzw: identity,
                scale_xyz: [2.2, 1.0, 1.55],
            },
        },
    ]);

    let foliage_layout = [
        (-8.0, -5.0, 1.25),
        (-11.0, 1.0, 0.92),
        (-8.0, 8.5, 1.12),
        (6.0, -7.0, 0.88),
        (10.0, -5.0, 1.30),
        (-15.0, 11.5, 1.48),
        (13.0, 8.0, 1.04),
    ];
    for (index, (offset_x, offset_z, scale)) in foliage_layout.into_iter().enumerate() {
        let x = f64::from(hero_x) + offset_x;
        let z = f64::from(hero_z) + offset_z;
        let y = campaign2_terrain_height(packet, x, z) + 0.02;
        let trunk_angle = (index as f32 * 0.37).sin() * 0.12;
        let crown_angle = (index as f32 * 0.23).sin() * 0.18;
        body.instances.extend([
            InstancePacket {
                instance_id: format!("campaign2-trunk-{index:02}"),
                mesh_id: "campaign2-foliage-trunk".into(),
                material_id: "campaign2-bark".into(),
                importance: InstanceImportance::Background,
                transform: Transform3d {
                    translation_xyz_m: [
                        finite_f32(x, "Campaign 2 trunk x")?,
                        finite_f32(f64::from(y), "Campaign 2 trunk y")?,
                        finite_f32(z, "Campaign 2 trunk z")?,
                    ],
                    rotation_xyzw: [
                        0.0,
                        (trunk_angle * 0.5).sin(),
                        0.0,
                        (trunk_angle * 0.5).cos(),
                    ],
                    scale_xyz: [scale * 0.72, scale, scale * 0.72],
                },
            },
            InstancePacket {
                instance_id: format!("campaign2-crown-{index:02}"),
                mesh_id: "campaign2-foliage-crown".into(),
                material_id: "campaign2-foliage".into(),
                importance: InstanceImportance::Background,
                transform: Transform3d {
                    translation_xyz_m: [
                        finite_f32(x, "Campaign 2 crown x")?,
                        finite_f32(f64::from(y + 1.60 * scale), "Campaign 2 crown y")?,
                        finite_f32(z, "Campaign 2 crown z")?,
                    ],
                    rotation_xyzw: [
                        0.0,
                        (crown_angle * 0.5).sin(),
                        0.0,
                        (crown_angle * 0.5).cos(),
                    ],
                    scale_xyz: [scale, scale, scale],
                },
            },
            InstancePacket {
                instance_id: format!("campaign2-lobe-{index:02}"),
                mesh_id: "campaign2-foliage-lobe".into(),
                material_id: "campaign2-foliage".into(),
                importance: InstanceImportance::Background,
                transform: Transform3d {
                    translation_xyz_m: [
                        finite_f32(
                            x + if index % 2 == 0 {
                                0.48 * f64::from(scale)
                            } else {
                                -0.48 * f64::from(scale)
                            },
                            "Campaign 2 foliage lobe x",
                        )?,
                        finite_f32(f64::from(y + 1.82 * scale), "Campaign 2 foliage lobe y")?,
                        finite_f32(z + 0.12 * f64::from(scale), "Campaign 2 foliage lobe z")?,
                    ],
                    rotation_xyzw: [
                        0.0,
                        (crown_angle * 0.35).sin(),
                        0.0,
                        (crown_angle * 0.35).cos(),
                    ],
                    scale_xyz: [scale, scale, scale],
                },
            },
        ]);
    }

    let (target, position, fov_y_degrees, width_px, height_px) = match view {
        Campaign2View::Close => (
            [hero_x, hero_y + 2.45, hero_z],
            [hero_x + 8.0, hero_y + 5.2, hero_z + 11.0],
            46.0,
            768,
            512,
        ),
        Campaign2View::Medium => (
            [hero_x - 1.5, hero_y + 2.2, hero_z + 1.0],
            [hero_x + 14.0, hero_y + 8.0, hero_z + 18.0],
            50.0,
            960,
            640,
        ),
        Campaign2View::Wide => (
            [hero_x - 8.0, hero_y + 1.5, hero_z - 10.0],
            [hero_x + 19.0, hero_y + 11.0, hero_z + 24.0],
            50.0,
            960,
            640,
        ),
    };
    let forward = normalize_vector3(
        [
            target[0] - position[0],
            target[1] - position[1],
            target[2] - position[2],
        ],
        "Campaign 2 camera forward",
    )?;
    let right = normalize_vector3(
        cross_vector3(forward, [0.0, 1.0, 0.0]),
        "Campaign 2 camera right",
    )?;
    let up = normalize_vector3(cross_vector3(right, forward), "Campaign 2 camera up")?;
    let camera_id = format!("campaign2-{view_name}");
    body.camera = GraphicsCamera {
        camera_id: camera_id.clone(),
        projection: CameraProjection::Perspective { fov_y_degrees },
        position_xyz_m: position,
        forward_xyz: forward,
        up_xyz: up,
        near_plane_m: 0.1,
        far_plane_m: 256.0,
        width_px,
        height_px,
    };
    body.capture.capture_id = format!("{}-campaign2-{view_name}", packet.body.capture.capture_id);
    body.capture.camera_id = camera_id;
    body.capture.width_px = width_px;
    body.capture.height_px = height_px;
    if let Some(light) = body.lights.first_mut() {
        light.intensity = 3.8;
        light.color_rgb = [1.0, 0.92, 0.80];
        if let LightKind::Directional { direction_xyz } = &mut light.kind {
            *direction_xyz = [0.38, -1.0, -0.32];
        }
    }
    body.environment = EnvironmentIntent {
        sky_top_rgb: [0.006, 0.018, 0.055],
        sky_horizon_rgb: [0.22, 0.34, 0.46],
        ground_rgb: [0.035, 0.050, 0.045],
        fog_color_rgb: [0.10, 0.16, 0.22],
        fog_density: 0.0018,
        exposure: 1.08,
    };
    // Goal step 1 — coherent production-quality material set. Deliberately the
    // LAST mutation before sealing: the campaign2 materials are appended long
    // after the packet is cloned, so an earlier call rewrote them and was then
    // overwritten. That shipped 24 hero textures that no material referenced —
    // payload with no effect, which is precisely the decorative-schema failure
    // this sprint exists to prevent. Placing it here also means every texture
    // added is referenced, so packet size and residency telemetry stay honest.
    //
    // Behind the candidate flag, so the null arm keeps the frozen bytes: this
    // changes CONTENT, not renderer behaviour.
    if parity_content.converge0 {
        apply_converge0_content(&mut body)?;
        if let Some(set) = terrain_layers {
            apply_terrain_layers(&mut body, set)?;
        }
    }
    if parity_content.hero_materials {
        let remapped = material_maps::apply_hero_material_set(&mut body);
        if remapped == 0 {
            return Err(GraphicsContractError::provenance(
                "hero material set remapped no materials; refusing to report an unchanged frame",
            ));
        }
    }
    seal_scene_packet(body)
}

/// Derive a deterministic, explicitly synthetic dense-scene packet for
/// renderer scalability measurements. The added instances are not authored
/// world content and must never be promoted as semantic evidence.
pub fn lower_dense_benchmark_packet(
    packet: &GraphicsScenePacket,
    background_instance_count: usize,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    validate_scene_packet(packet)?;
    if !(1..=MAX_DENSE_BENCHMARK_INSTANCES).contains(&background_instance_count) {
        return Err(GraphicsContractError::malformed(format!(
            "dense benchmark instance count must be between 1 and {MAX_DENSE_BENCHMARK_INSTANCES}"
        )));
    }
    let has_foliage_mesh = packet
        .body
        .meshes
        .iter()
        .any(|mesh| mesh.mesh_id == "foliage-cross");
    let has_foliage_material = packet
        .body
        .materials
        .iter()
        .any(|material| material.material_id == "foliage-default");
    if !has_foliage_mesh || !has_foliage_material {
        return Err(GraphicsContractError::provenance(
            "dense benchmark requires the bounded foliage mesh/material profile",
        ));
    }
    let heights = match &packet.body.terrain.heights_m.payload {
        BufferPayload::F32(values) => values,
        BufferPayload::U8(_) | BufferPayload::U32(_) => {
            return Err(GraphicsContractError::unsupported(
                "dense benchmark requires f32 terrain heights",
            ));
        }
    };
    let grid_width = (background_instance_count as f64).sqrt().ceil() as usize;
    let grid_depth = background_instance_count.div_ceil(grid_width);
    let mut body = packet.body.clone();
    body.packet_id = format!("{}-dense-{background_instance_count}", body.packet_id);
    body.capture.capture_id = format!(
        "{}-dense-{background_instance_count}",
        body.capture.capture_id
    );
    body.instances.reserve(background_instance_count);
    for index in 0..background_instance_count {
        let column = index % grid_width;
        let row = index / grid_width;
        let u = (column as f64 + 0.5) / grid_width as f64;
        let v = (row as f64 + 0.5) / grid_depth as f64;
        let x =
            -0.42 * f64::from(body.terrain.width_m) + 0.84 * f64::from(body.terrain.width_m) * u;
        let z =
            -0.42 * f64::from(body.terrain.length_m) + 0.84 * f64::from(body.terrain.length_m) * v;
        let cell = nearest_cell(
            f64::from(body.terrain.width_m),
            f64::from(body.terrain.length_m),
            body.terrain.resolution,
            [x, z],
        );
        let phase = (body
            .frame_seed
            .wrapping_add((index as u64).wrapping_mul(37))
            % 360) as f32
            * std::f32::consts::PI
            / 180.0;
        let height_scale = 0.75 + ((index * 17) % 7) as f32 * 0.08;
        body.instances.push(InstancePacket {
            instance_id: format!("benchmark-foliage-{index:04}"),
            mesh_id: "foliage-cross".into(),
            material_id: "foliage-default".into(),
            importance: InstanceImportance::Background,
            transform: Transform3d {
                translation_xyz_m: [
                    finite_f32(x, "dense benchmark x")?,
                    finite_f32(f64::from(heights[cell]), "dense benchmark height")?,
                    finite_f32(z, "dense benchmark z")?,
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
    seal_scene_packet(body)
}

fn validate_coordinate_system(
    coordinate_system: &CoordinateSystem,
) -> Result<(), GraphicsContractError> {
    if coordinate_system.up_axis != Axis::Y || coordinate_system.handedness != Handedness::Right {
        return Err(GraphicsContractError::unsupported(
            "native graphics requires Y-up, right-handed coordinates",
        ));
    }
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
    bounded_values(&camera.position_xyz_m, "camera position")?;
    validate_basis(
        &camera.forward_xyz,
        &camera.up_xyz,
        "camera forward/up basis",
    )?;
    if !camera.near_plane_m.is_finite()
        || !camera.far_plane_m.is_finite()
        || camera.near_plane_m < MIN_NATIVE_DISTANCE_M
        || camera.far_plane_m <= camera.near_plane_m
        || camera.near_plane_m > MAX_NATIVE_COORDINATE_M
        || camera.far_plane_m > MAX_NATIVE_COORDINATE_M
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
            if !span_m.is_finite()
                || !(MIN_NATIVE_DISTANCE_M..=MAX_NATIVE_COORDINATE_M).contains(&span_m)
            {
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
    textures: &[TextureReference],
) -> Result<(), GraphicsContractError> {
    if let Some(layers) = &terrain.layers {
        validate_terrain_layers(layers, materials, textures)?;
    }
    valid_id(&terrain.terrain_id, "terrain_id")?;
    if !terrain.width_m.is_finite()
        || !terrain.length_m.is_finite()
        || terrain.width_m < MIN_NATIVE_DISTANCE_M
        || terrain.length_m < MIN_NATIVE_DISTANCE_M
    {
        return Err(GraphicsContractError::malformed(
            "terrain dimensions must be finite and positive",
        ));
    }
    if terrain.width_m > MAX_NATIVE_COORDINATE_M || terrain.length_m > MAX_NATIVE_COORDINATE_M {
        return Err(GraphicsContractError::malformed(
            "terrain dimensions exceed the bounded native physical envelope",
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
    if let BufferPayload::F32(values) = &buffer.payload
        && values
            .iter()
            .any(|value| value.abs() > MAX_NATIVE_COORDINATE_M)
    {
        return Err(GraphicsContractError::malformed(format!(
            "{label} exceeds the bounded native physical envelope"
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
    if !material.clearcoat.is_finite()
        || !material.clearcoat_roughness.is_finite()
        || !(0.0..=1.0).contains(&material.clearcoat)
        || !(0.045..=1.0).contains(&material.clearcoat_roughness)
    {
        return Err(GraphicsContractError::malformed(
            "material clearcoat and clearcoat roughness are outside their bounds",
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
        || texture.mip_levels > MAX_TEXTURE_MIP_LEVELS
        || texture.width_px > MAX_CAPTURE_DIMENSION
        || texture.height_px > MAX_CAPTURE_DIMENSION
    {
        return Err(GraphicsContractError::malformed(
            "texture dimensions or mip count are invalid",
        ));
    }
    if let Some(payload) = &texture.payload {
        let mut payload_bytes = Vec::new();
        match payload {
            TexturePayload::Rgba8(encoded) => {
                if texture.mip_levels != 1 {
                    return Err(GraphicsContractError::unsupported(
                        "single-level texture payload cannot claim multiple mip levels",
                    ));
                }
                payload_bytes.extend(validate_texture_level(
                    &texture.texture_id,
                    texture.width_px,
                    texture.height_px,
                    encoded,
                    0,
                )?);
            }
            TexturePayload::Rgba8MipChain { levels } => {
                if levels.len() != usize::try_from(texture.mip_levels).unwrap_or(usize::MAX) {
                    return Err(GraphicsContractError::provenance(format!(
                        "texture {} mip payload carries {} levels but declares {}",
                        texture.texture_id,
                        levels.len(),
                        texture.mip_levels
                    )));
                }
                for (level, mip) in levels.iter().enumerate() {
                    let (width, height) =
                        mip_dimensions(texture.width_px, texture.height_px, level);
                    if mip.width_px != width || mip.height_px != height {
                        return Err(GraphicsContractError::provenance(format!(
                            "texture {} mip {} dimensions are {}x{}, expected {}x{}",
                            texture.texture_id, level, mip.width_px, mip.height_px, width, height
                        )));
                    }
                    payload_bytes.extend(validate_texture_level(
                        &texture.texture_id,
                        width,
                        height,
                        &mip.base64,
                        level,
                    )?);
                }
            }
        }
        if texture.sha256 != sha256_prefixed(&payload_bytes) {
            return Err(GraphicsContractError::provenance(format!(
                "texture {} payload digest does not match its metadata",
                texture.texture_id
            )));
        }
    }
    Ok(())
}

fn validate_texture_level(
    texture_id: &str,
    width: u32,
    height: u32,
    encoded: &str,
    level: usize,
) -> Result<Vec<u8>, GraphicsContractError> {
    let bytes = STANDARD.decode(encoded).map_err(|error| {
        GraphicsContractError::malformed(format!(
            "texture {texture_id} mip {level} payload is not valid base64: {error}"
        ))
    })?;
    let expected_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height)?.checked_mul(4))
        })
        .ok_or_else(|| {
            GraphicsContractError::malformed(format!(
                "texture {texture_id} mip {level} dimensions overflow payload length"
            ))
        })?;
    if bytes.len() != expected_bytes {
        return Err(GraphicsContractError::provenance(format!(
            "texture {texture_id} mip {level} payload has {} bytes, expected {expected_bytes}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

fn mip_dimensions(width: u32, height: u32, level: usize) -> (u32, u32) {
    let mut width = width;
    let mut height = height;
    for _ in 0..level {
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    (width, height)
}

fn validate_mesh(
    mesh: &MeshPacket,
    material_ids: &BTreeSet<&str>,
) -> Result<(), GraphicsContractError> {
    valid_id(&mesh.mesh_id, "mesh_id")?;
    if mesh.positions_m.is_empty()
        || mesh.positions_m.len() != mesh.normals.len()
        || mesh.positions_m.len() != mesh.uv0.len()
        || (!mesh.tangents.is_empty() && mesh.positions_m.len() != mesh.tangents.len())
    {
        return Err(GraphicsContractError::malformed(format!(
            "mesh {} needs matching non-empty position, normal, uv0, and optional tangent arrays",
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
        bounded_values(position, "mesh position")?;
    }
    for normal in &mesh.normals {
        finite_values(normal, "mesh normal")?;
        bounded_values(normal, "mesh normal")?;
        let normal_norm_squared = squared_norm(normal);
        if !normal_norm_squared.is_finite() || normal_norm_squared <= MIN_VECTOR_LENGTH_SQUARED {
            return Err(GraphicsContractError::malformed(format!(
                "mesh {} contains a degenerate normal",
                mesh.mesh_id
            )));
        }
    }
    for uv in &mesh.uv0 {
        finite_values(uv, "mesh uv0")?;
        bounded_values(uv, "mesh uv0")?;
    }
    for tangent in &mesh.tangents {
        finite_values(tangent, "mesh tangent")?;
        bounded_values(tangent, "mesh tangent")?;
        let tangent_norm_squared = squared_norm(&[tangent[0], tangent[1], tangent[2]]);
        if !tangent_norm_squared.is_finite() || tangent_norm_squared <= MIN_VECTOR_LENGTH_SQUARED {
            return Err(GraphicsContractError::malformed(format!(
                "mesh {} contains a degenerate tangent",
                mesh.mesh_id
            )));
        }
        if (tangent[3].abs() - 1.0).abs() > 1.0e-3 {
            return Err(GraphicsContractError::malformed(format!(
                "mesh {} tangent handedness must be -1 or 1",
                mesh.mesh_id
            )));
        }
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
    bounded_values(&transform.translation_xyz_m, "instance translation")?;
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
    if (rotation_norm_squared - 1.0).abs() > UNIT_QUATERNION_TOLERANCE {
        return Err(GraphicsContractError::malformed(
            "instance rotation must be a unit quaternion",
        ));
    }
    if transform
        .scale_xyz
        .iter()
        .any(|value| *value < MIN_NATIVE_SCALE)
    {
        return Err(GraphicsContractError::malformed(
            "instance scale must be positive",
        ));
    }
    if transform
        .scale_xyz
        .iter()
        .any(|value| *value > MAX_NATIVE_COORDINATE_M)
    {
        return Err(GraphicsContractError::malformed(
            "instance scale exceeds the bounded native physical envelope",
        ));
    }
    Ok(())
}

fn validate_light(light: &LightIntent) -> Result<(), GraphicsContractError> {
    valid_id(&light.light_id, "light_id")?;
    finite_values(&light.color_rgb, "light color")?;
    if light
        .color_rgb
        .iter()
        .any(|value| *value < 0.0 || *value > MAX_NATIVE_COORDINATE_M)
        || !light.intensity.is_finite()
        || light.intensity < 0.0
        || light.intensity > MAX_NATIVE_COORDINATE_M
    {
        return Err(GraphicsContractError::malformed(
            "light color and intensity must be finite and non-negative",
        ));
    }
    match &light.kind {
        LightKind::Directional { direction_xyz } => {
            finite_values(direction_xyz, "directional light direction")?;
            let direction_norm_squared = squared_norm(direction_xyz);
            if !direction_norm_squared.is_finite()
                || direction_norm_squared <= MIN_VECTOR_LENGTH_SQUARED
            {
                return Err(GraphicsContractError::malformed(
                    "directional light direction must be non-degenerate",
                ));
            }
        }
        LightKind::Point {
            position_xyz_m,
            range_m,
        } => {
            finite_values(position_xyz_m, "point light position")?;
            bounded_values(position_xyz_m, "point light position")?;
            if !range_m.is_finite() || *range_m <= 0.0 || *range_m > MAX_NATIVE_COORDINATE_M {
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
            bounded_values(position_xyz_m, "point marker position")?;
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
            bounded_values(center_xyz_m, "circle marker center")?;
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
                bounded_values(point, "polyline marker point")?;
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
    if !radius_or_thickness.is_finite()
        || !(MIN_NATIVE_DISTANCE_M..=MAX_NATIVE_COORDINATE_M).contains(&radius_or_thickness)
    {
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
    if capture.include_depth {
        return Err(GraphicsContractError::unsupported(
            "native certification currently promotes color-only captures; depth evidence is deferred",
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
    if let Some(residency) = &telemetry.texture_residency {
        for (value, label) in [
            (residency.texture_count, "texture_residency.texture_count"),
            (residency.mip_levels, "texture_residency.mip_levels"),
            (residency.payload_bytes, "texture_residency.payload_bytes"),
        ] {
            if value > MAX_TELEMETRY_COUNTER {
                return Err(GraphicsContractError::malformed(format!(
                    "telemetry {label} exceeds the bounded counter limit"
                )));
            }
        }
        if residency.max_sampler_lod >= MAX_TEXTURE_MIP_LEVELS {
            return Err(GraphicsContractError::malformed(
                "texture residency sampler LOD exceeds the native mip bound",
            ));
        }
        if residency.texture_count == 0
            && (residency.mip_levels != 0
                || residency.payload_bytes != 0
                || residency.max_sampler_lod != 0)
        {
            return Err(GraphicsContractError::provenance(
                "empty texture residency reports non-empty resources",
            ));
        }
        if residency.mip_levels < residency.texture_count {
            return Err(GraphicsContractError::provenance(
                "texture residency has fewer mip levels than textures",
            ));
        }
        if residency.max_sampler_lod as usize >= residency.mip_levels && residency.mip_levels != 0 {
            return Err(GraphicsContractError::provenance(
                "texture residency sampler LOD exceeds resident levels",
            ));
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
    let cpu_pass_total = [
        telemetry.pass_timings.prepare_us,
        telemetry.pass_timings.scene_raster_us,
        telemetry.pass_timings.resolve_us,
        telemetry.pass_timings.overlay_us,
        telemetry.pass_timings.flush_readback_us,
    ]
    .into_iter()
    .try_fold(0u64, |total, value| total.checked_add(value))
    .ok_or_else(|| GraphicsContractError::malformed("telemetry CPU pass timings overflow"))?;
    if cpu_pass_total > telemetry.frame_time_us {
        return Err(GraphicsContractError::provenance(
            "telemetry CPU pass timings exceed total frame time",
        ));
    }
    if let (Some(frame_time), Some(values)) = (
        telemetry.gpu_frame_time_us,
        [
            telemetry.pass_timings.gpu_prepare_us,
            telemetry.pass_timings.gpu_scene_raster_us,
            telemetry.pass_timings.gpu_resolve_us,
            telemetry.pass_timings.gpu_overlay_us,
        ]
        .into_iter()
        .collect::<Option<Vec<_>>>(),
    ) {
        let gpu_pass_total = values
            .into_iter()
            .try_fold(0u64, |total, value| total.checked_add(value))
            .ok_or_else(|| {
                GraphicsContractError::malformed("telemetry GPU pass timings overflow")
            })?;
        if gpu_pass_total > frame_time.saturating_add(16) {
            return Err(GraphicsContractError::provenance(
                "telemetry GPU pass timings exceed total GPU frame time",
            ));
        }
    }
    for (value, label) in [
        (telemetry.pass_timings.prepare_us, "prepare_us"),
        (telemetry.pass_timings.scene_raster_us, "scene_raster_us"),
        (telemetry.pass_timings.resolve_us, "resolve_us"),
        (telemetry.pass_timings.overlay_us, "overlay_us"),
        (
            telemetry.pass_timings.flush_readback_us,
            "flush_readback_us",
        ),
    ] {
        if value > MAX_FRAME_TIME_US {
            return Err(GraphicsContractError::malformed(format!(
                "telemetry pass {label} exceeds the bounded limit"
            )));
        }
    }
    for (value, label) in [
        (telemetry.pass_timings.gpu_prepare_us, "gpu_prepare_us"),
        (
            telemetry.pass_timings.gpu_scene_raster_us,
            "gpu_scene_raster_us",
        ),
        (telemetry.pass_timings.gpu_resolve_us, "gpu_resolve_us"),
        (telemetry.pass_timings.gpu_overlay_us, "gpu_overlay_us"),
    ] {
        if value.is_some_and(|value| value > MAX_FRAME_TIME_US) {
            return Err(GraphicsContractError::malformed(format!(
                "telemetry GPU pass {label} exceeds the bounded limit"
            )));
        }
    }
    Ok(())
}

/// Validate the backend's packet-texture residency evidence independently of
/// producer status or aggregate byte counters. Adapter-owned default textures
/// are deliberately excluded; this measures distinct source texture identities
/// referenced by material roles in the packet.
pub(crate) fn validate_texture_residency_telemetry(
    packet: &GraphicsScenePacket,
    telemetry: &GraphicsTelemetry,
) -> Result<(), GraphicsContractError> {
    let expected = expected_texture_residency(packet)?;
    let requires_evidence = expected.mip_levels > expected.texture_count;
    match (&telemetry.texture_residency, requires_evidence) {
        (Some(actual), _) if actual == &expected => Ok(()),
        (Some(_), _) => Err(GraphicsContractError::provenance(
            "texture residency telemetry does not match the packet payloads",
        )),
        (None, true) => Err(GraphicsContractError::provenance(
            "multi-level packet textures require independent residency telemetry",
        )),
        (None, false) => Ok(()),
    }
}

fn expected_texture_residency(
    packet: &GraphicsScenePacket,
) -> Result<GraphicsTextureResidencyTelemetry, GraphicsContractError> {
    let mut referenced_ids = BTreeSet::new();
    for material in &packet.body.materials {
        referenced_ids.extend(material.texture_ids.iter().map(String::as_str));
        referenced_ids.extend(
            [
                material.normal_texture_id.as_deref(),
                material.roughness_texture_id.as_deref(),
                material.occlusion_texture_id.as_deref(),
                material.emissive_texture_id.as_deref(),
            ]
            .into_iter()
            .flatten(),
        );
    }

    let mut expected = GraphicsTextureResidencyTelemetry {
        texture_count: 0,
        mip_levels: 0,
        payload_bytes: 0,
        max_sampler_lod: 0,
    };
    for texture in &packet.body.textures {
        if !referenced_ids.contains(texture.texture_id.as_str()) {
            continue;
        }
        texture.payload.as_ref().ok_or_else(|| {
            GraphicsContractError::unsupported(format!(
                "referenced texture {} has no inline payload for native residency",
                texture.texture_id
            ))
        })?;
        expected.texture_count = expected
            .texture_count
            .checked_add(1)
            .ok_or_else(|| GraphicsContractError::malformed("texture count overflows"))?;
        expected.mip_levels = expected
            .mip_levels
            .checked_add(texture.mip_levels as usize)
            .ok_or_else(|| GraphicsContractError::malformed("texture mip count overflows"))?;
        expected.payload_bytes = expected
            .payload_bytes
            .checked_add(texture_payload_byte_length(texture)?)
            .ok_or_else(|| GraphicsContractError::malformed("texture payload bytes overflow"))?;
        expected.max_sampler_lod = expected
            .max_sampler_lod
            .max(texture.mip_levels.saturating_sub(1));
    }
    Ok(expected)
}

fn texture_payload_byte_length(texture: &TextureReference) -> Result<usize, GraphicsContractError> {
    let mut width = usize::try_from(texture.width_px)
        .map_err(|_| GraphicsContractError::malformed("texture width overflows usize"))?;
    let mut height = usize::try_from(texture.height_px)
        .map_err(|_| GraphicsContractError::malformed("texture height overflows usize"))?;
    let mut bytes = 0usize;
    for _ in 0..texture.mip_levels {
        let level_bytes = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| GraphicsContractError::malformed("texture payload size overflows"))?;
        bytes = bytes
            .checked_add(level_bytes)
            .ok_or_else(|| GraphicsContractError::malformed("texture payload bytes overflow"))?;
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    Ok(bytes)
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

fn bounded_values<const N: usize>(
    values: &[f32; N],
    label: &str,
) -> Result<(), GraphicsContractError> {
    if values
        .iter()
        .all(|value| value.abs() <= MAX_NATIVE_COORDINATE_M)
    {
        Ok(())
    } else {
        Err(GraphicsContractError::malformed(format!(
            "{label} exceeds the bounded native physical envelope"
        )))
    }
}

fn squared_norm<const N: usize>(values: &[f32; N]) -> f32 {
    values.iter().map(|value| value * value).sum()
}

fn validate_basis(
    forward: &[f32; 3],
    up: &[f32; 3],
    label: &str,
) -> Result<(), GraphicsContractError> {
    let forward_norm_squared = squared_norm(forward);
    let up_norm_squared = squared_norm(up);
    if !forward_norm_squared.is_finite()
        || !up_norm_squared.is_finite()
        || forward_norm_squared <= MIN_VECTOR_LENGTH_SQUARED
        || up_norm_squared <= MIN_VECTOR_LENGTH_SQUARED
    {
        return Err(GraphicsContractError::malformed(format!(
            "{label} contains a degenerate direction"
        )));
    }
    let forward_norm = forward_norm_squared.sqrt();
    let up_norm = up_norm_squared.sqrt();
    let cosine = forward
        .iter()
        .zip(up.iter())
        .map(|(forward, up)| (forward / forward_norm) * (up / up_norm))
        .sum::<f32>();
    if !cosine.is_finite() || cosine.abs() >= MAX_BASIS_COSINE {
        return Err(GraphicsContractError::malformed(format!(
            "{label} directions must not be collinear"
        )));
    }
    Ok(())
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
            deformation: None,
            render_policy: None,
            packet_id: "packet-test".into(),
            scene_artifact_id: None,
            scene_artifact_sha256: None,
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
                layers: None,
            },
            materials: vec![MaterialIntent {
                material_id: "terrain".into(),
                base_color_rgba: [0.3, 0.4, 0.3, 1.0],
                metallic: 0.0,
                roughness: 0.9,
                clearcoat: 0.0,
                clearcoat_roughness: 0.5,
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
                include_depth: false,
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
    fn finite_but_unsafe_native_scalars_are_rejected() {
        let mut body = packet().body;
        body.lights[0].intensity = f32::MAX;
        assert!(seal_scene_packet(body).is_err());

        let mut body = packet().body;
        body.camera.near_plane_m = f32::MIN_POSITIVE;
        assert!(seal_scene_packet(body).is_err());

        let mut body = packet().body;
        body.camera.projection = CameraProjection::Orthographic { span_m: f32::MAX };
        assert!(seal_scene_packet(body).is_err());

        let mut body = packet().body;
        body.terrain.width_m = f32::MIN_POSITIVE;
        assert!(seal_scene_packet(body).is_err());

        let mut body = packet().body;
        if let SemanticOverlay::Polyline { thickness_m, .. } = &mut body.overlays[0] {
            *thickness_m = f32::MAX;
        }
        assert!(seal_scene_packet(body).is_err());
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

        let Some(TexturePayload::Rgba8(encoded)) = packet.body.textures[0].payload.as_mut() else {
            panic!("texture payload must be the single-level RGBA8 variant");
        };
        encoded.replace_range(..4, "AAAA");
        assert!(validate_scene_packet(&packet).is_err());
    }

    #[test]
    fn multilevel_texture_requires_packet_consistent_residency_evidence() {
        let base = vec![
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            255, 255, 255, 255, // white
        ];
        let mip = vec![128, 128, 128, 255];
        let mut packet = packet();
        packet.body.materials[0].texture_ids = vec!["texture".into()];
        packet.body.textures = vec![TextureReference {
            texture_id: "texture".into(),
            source_artifact_id: "source".into(),
            sha256: sha256_prefixed(&[base.clone(), mip.clone()].concat()),
            width_px: 2,
            height_px: 2,
            mip_levels: 2,
            color_space: TextureColorSpace::Srgb,
            payload: Some(TexturePayload::Rgba8MipChain {
                levels: vec![
                    TextureMipLevel {
                        width_px: 2,
                        height_px: 2,
                        base64: STANDARD.encode(&base),
                    },
                    TextureMipLevel {
                        width_px: 1,
                        height_px: 1,
                        base64: STANDARD.encode(&mip),
                    },
                ],
            }),
        }];
        packet = seal_scene_packet(packet.body).expect("multi-level packet seals");

        let mut telemetry = GraphicsTelemetry {
            deformation: None,
            upload_bytes: 0,
            readback_bytes: 0,
            draw_calls: 0,
            dispatch_calls: 0,
            pipeline_compilations: 0,
            instance_count: 0,
            visible_instance_count: 0,
            culled_instance_count: 0,
            background_visible_instance_count: 0,
            background_culled_instance_count: 0,
            landmark_visible_instance_count: 0,
            landmark_culled_instance_count: 0,
            gameplay_critical_visible_instance_count: 0,
            gameplay_critical_culled_instance_count: 0,
            terrain_vertex_count: 0,
            mesh_vertex_count: 0,
            texture_residency: None,
            frame_time_us: 0,
            gpu_frame_time_us: None,
            pass_timings: GraphicsPassTimings {
                prepare_us: 0,
                scene_raster_us: 0,
                resolve_us: 0,
                overlay_us: 0,
                flush_readback_us: 0,
                gpu_prepare_us: None,
                gpu_scene_raster_us: None,
                gpu_resolve_us: None,
                gpu_overlay_us: None,
            },
        };
        assert!(validate_texture_residency_telemetry(&packet, &telemetry).is_err());
        telemetry.texture_residency = Some(GraphicsTextureResidencyTelemetry {
            texture_count: 1,
            mip_levels: 2,
            payload_bytes: 20,
            max_sampler_lod: 1,
        });
        validate_texture_residency_telemetry(&packet, &telemetry)
            .expect("matching residency evidence validates");
        telemetry.texture_residency.as_mut().unwrap().payload_bytes = 16;
        assert!(validate_texture_residency_telemetry(&packet, &telemetry).is_err());
    }

    #[test]
    fn material_texture_roles_reject_unknown_provenance() {
        let mut packet = packet();
        packet.body.materials[0].normal_texture_id = Some("missing-normal".into());
        assert!(validate_scene_packet(&packet).is_err());
    }

    #[test]
    fn clearcoat_contract_rejects_non_finite_and_out_of_range_values() {
        let mut body = packet().body;
        body.materials[0].clearcoat = f32::NAN;
        assert!(seal_scene_packet(body).is_err());

        let mut body = packet().body;
        body.materials[0].clearcoat = 1.01;
        assert!(seal_scene_packet(body).is_err());

        let mut body = packet().body;
        body.materials[0].clearcoat_roughness = 0.02;
        assert!(seal_scene_packet(body).is_err());
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
            tangents: Vec::new(),
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
            tangents: Vec::new(),
        }];
        assert!(seal_scene_packet(body).is_err());
    }

    #[test]
    fn camera_basis_rejects_degenerate_and_collinear_directions() {
        validate_scene_packet(&packet()).expect("known-good camera basis validates");

        let mut body = packet().body;
        body.camera.forward_xyz = [0.0, 0.0, 0.0];
        assert!(seal_scene_packet(body).is_err());

        let mut body = packet().body;
        body.camera.up_xyz = [0.0, -2.0, 0.0];
        assert!(seal_scene_packet(body).is_err());
    }

    #[test]
    fn screen_projection_lowers_semantic_camera_up_above_the_capture_center() {
        let packet = packet();
        let center = project_screen_point(&packet.body.camera, [0.0, 0.0, 0.0])
            .expect("center is inside the camera frustum");
        let semantic_up = project_screen_point(&packet.body.camera, [0.0, 0.0, -1.0])
            .expect("semantic camera-up point is inside the camera frustum");
        assert!(semantic_up.1 < center.1);
    }

    #[test]
    fn authored_mesh_normals_must_be_non_degenerate() {
        let mut body = packet().body;
        body.meshes = vec![MeshPacket {
            mesh_id: "mesh".into(),
            positions_m: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            uv0: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            material_id: "terrain".into(),
            tangents: Vec::new(),
        }];
        assert!(seal_scene_packet(body).is_err());
    }

    #[test]
    fn instance_rotations_and_directional_lights_have_valid_bases() {
        let mut body = packet().body;
        body.meshes = vec![MeshPacket {
            mesh_id: "mesh".into(),
            positions_m: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            uv0: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            material_id: "terrain".into(),
            tangents: Vec::new(),
        }];
        body.instances = vec![InstancePacket {
            instance_id: "instance".into(),
            mesh_id: "mesh".into(),
            material_id: "terrain".into(),
            importance: InstanceImportance::Background,
            transform: Transform3d {
                translation_xyz_m: [0.0, 0.0, 0.0],
                rotation_xyzw: [0.0, 0.0, 0.0, 2.0],
                scale_xyz: [1.0, 1.0, 1.0],
            },
        }];
        assert!(seal_scene_packet(body).is_err());

        let mut body = packet().body;
        body.lights[0].kind = LightKind::Directional {
            direction_xyz: [0.0, 0.0, 0.0],
        };
        assert!(seal_scene_packet(body).is_err());
    }

    #[test]
    fn pass_receipt_requires_capture_bytes() {
        let packet = packet();
        let mut capture = vec![0u8; 32 * 32 * 4];
        for (pixel_index, pixel) in capture.chunks_exact_mut(4).enumerate() {
            let value = if pixel_index.is_multiple_of(2) {
                64
            } else {
                200
            };
            pixel.copy_from_slice(&[value, value, value, 255]);
        }
        let center_pixel = (16 * 32 + 16) * 4;
        capture[center_pixel..center_pixel + 4].copy_from_slice(&[0, 255, 255, 255]);
        let measurements = measure_frame_capture(&packet, &capture).expect("capture measures");
        let body = GraphicsFrameReceiptBody {
            schema_version: FRAME_RECEIPT_SCHEMA.into(),
            packet_sha256: packet.packet_sha256.clone(),
            capture_id: packet.body.capture.capture_id.clone(),
            backend_id: LAVA_BACKEND_ID.into(),
            adapter_revision: ADAPTER_REVISION.into(),
            lava_revision: LAVA_REVISION.into(),
            device_uuid: "device".into(),
            worker_script_sha256: sha256_prefixed(b"worker"),
            renderer_identity_sha256: sha256_prefixed(b"renderer"),
            status: FrameStatus::Passed,
            format: CaptureFormat::Rgba8Srgb,
            width_px: packet.body.capture.width_px,
            height_px: packet.body.capture.height_px,
            capture_sha256: Some(sha256_prefixed(&capture)),
            measurements,
            telemetry: GraphicsTelemetry {
                deformation: None,
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
                terrain_vertex_count: 24,
                mesh_vertex_count: 0,
                texture_residency: None,
                frame_time_us: 1,
                gpu_frame_time_us: None,
                pass_timings: GraphicsPassTimings {
                    prepare_us: 0,
                    scene_raster_us: 0,
                    resolve_us: 0,
                    overlay_us: 0,
                    flush_readback_us: 0,
                    gpu_prepare_us: None,
                    gpu_scene_raster_us: None,
                    gpu_resolve_us: None,
                    gpu_overlay_us: None,
                },
            },
            detail: "measured".into(),
        };
        let mut receipt = GraphicsFrameReceipt {
            receipt_sha256: sha256_prefixed(&canonical_json(&body).expect("body JSON")),
            body,
        };
        assert!(validate_frame_receipt(&receipt, &packet, &[]).is_err());
        validate_frame_receipt(&receipt, &packet, &capture).expect("matching capture validates");
        receipt.body.telemetry.frame_time_us = MAX_FRAME_TIME_US + 1;
        receipt.receipt_sha256 =
            sha256_prefixed(&canonical_json(&receipt.body).expect("tampered body JSON"));
        assert!(validate_frame_receipt(&receipt, &packet, &capture).is_err());
        receipt.body.telemetry.frame_time_us = 10;
        receipt.body.telemetry.pass_timings.prepare_us = 11;
        receipt.receipt_sha256 =
            sha256_prefixed(&canonical_json(&receipt.body).expect("tampered pass JSON"));
        assert!(validate_frame_receipt(&receipt, &packet, &capture).is_err());
    }

    #[test]
    fn rust_visual_measurement_recomputes_semantic_overlay_visibility() {
        let packet = packet();
        let mut capture = vec![0u8; 32 * 32 * 4];
        let center_pixel = (16 * 32 + 16) * 4;
        capture[center_pixel..center_pixel + 4].copy_from_slice(&[0, 255, 255, 255]);
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
        assert_eq!(
            rgba8(&[0.0, 0.0, 0.0, 1.0 / 510.0]).expect("halfway alpha quantizes"),
            [0, 0, 0, 1]
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
        assert!(validate_native_visual_gate(&packet, &flat_measurements, &flat).is_err());

        let mut useful = vec![0u8; 32 * 32 * 4];
        for (pixel_index, pixel) in useful.chunks_exact_mut(4).enumerate() {
            let value = if pixel_index.is_multiple_of(2) {
                50
            } else {
                200
            };
            pixel.copy_from_slice(&[value, value, value, 255]);
        }
        let center_pixel = (16 * 32 + 16) * 4;
        useful[center_pixel..center_pixel + 4].copy_from_slice(&[0, 255, 255, 255]);
        let useful_measurements =
            measure_frame_capture(&packet, &useful).expect("useful capture measures");
        validate_native_visual_gate(&packet, &useful_measurements, &useful)
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
