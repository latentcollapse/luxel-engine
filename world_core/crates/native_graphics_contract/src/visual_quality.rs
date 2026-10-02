//! Versioned, engine-neutral technical visual-quality evidence.
//!
//! This gate measures deterministic properties of projected terrain/content
//! pixels. It does not attempt to score artistic taste or replace the native
//! capture-integrity gate.

use serde::{Deserialize, Serialize};

use crate::{
    ADAPTER_REVISION, Axis, CaptureFormat, FRAME_RECEIPT_SCHEMA, FrameStatus,
    GraphicsContractError, GraphicsFrameReceipt, GraphicsScenePacket, LAVA_BACKEND_ID,
    LAVA_REVISION, MAX_CAPTURE_BYTES, MAX_CAPTURE_DIMENSION, SemanticOverlay, canonical_json,
    measure_frame_capture, project_screen_point, sha256_prefixed, validate_frame_receipt,
    validate_scene_packet,
};

pub const VISUAL_QUALITY_EVIDENCE_SCHEMA: &str = "wge.visual-quality-evidence/v1";
pub const VISUAL_QUALITY_PROFILE_SCHEMA: &str = "wge.visual-quality-profile/v1";
pub const CAMPAIGN2_VISUAL_EVIDENCE_SCHEMA: &str = "wge.campaign2-visual-evidence/v1";

const MAX_TERRAIN_GRID_SIDE: u16 = 257;
const MAX_QUALITY_OVERLAYS: usize = 8192;
const MAX_QUALITY_RASTER_VISITS: u64 = 64 * 1024 * 1024;
const LUMINANCE_BIN_COUNT: usize = 32;
const RGB_BIN_COUNT: usize = 4096;

/// An inspectable deterministic technical floor for terrain-rich captures.
/// The thresholds are fixed-point integers so profile hashing and decisions
/// do not depend on floating-point serialization or rounding modes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VisualQualityProfile {
    pub schema_version: String,
    pub profile_id: String,
    pub profile_version: u32,
    pub capture_width_px: u32,
    pub capture_height_px: u32,
    pub capture_format: CaptureFormat,
    pub region: QualityRegion,
    /// Maximum side length of the sampled terrain mesh used to derive the
    /// projected terrain coverage mask.
    pub maximum_terrain_grid_side: u16,
    pub minimum_region_coverage_bp: u16,
    pub minimum_sampled_pixels: u32,
    pub maximum_overlay_coverage_bp: u16,
    pub minimum_luminance_p95_p05_span: u8,
    pub minimum_distinct_luminance_bins: u8,
    pub minimum_distinct_rgb_bins: u16,
    pub minimum_edge_pair_fraction_bp: u16,
    pub minimum_edge_luminance_delta: u8,
    pub tile_columns: u8,
    pub tile_rows: u8,
    pub minimum_tile_luminance_span: u8,
    pub minimum_tile_samples: u16,
    pub minimum_varied_tile_fraction_bp: u16,
}

impl VisualQualityProfile {
    /// A conservative engine-neutral technical reference profile. Passing it
    /// proves measurable spatial/color structure in the declared world region;
    /// it is not an aesthetic or production-readiness certification.
    pub fn terrain_reference_v1(width_px: u32, height_px: u32) -> Self {
        Self {
            schema_version: VISUAL_QUALITY_PROFILE_SCHEMA.into(),
            profile_id: "terrain-technical-reference".into(),
            profile_version: 1,
            capture_width_px: width_px,
            capture_height_px: height_px,
            capture_format: CaptureFormat::Rgba8Srgb,
            region: QualityRegion::ProjectedTerrain,
            maximum_terrain_grid_side: 129,
            minimum_region_coverage_bp: 2_000,
            minimum_sampled_pixels: 1_024,
            maximum_overlay_coverage_bp: 2_500,
            minimum_luminance_p95_p05_span: 32,
            minimum_distinct_luminance_bins: 6,
            minimum_distinct_rgb_bins: 12,
            minimum_edge_pair_fraction_bp: 500,
            minimum_edge_luminance_delta: 12,
            tile_columns: 8,
            tile_rows: 8,
            minimum_tile_luminance_span: 10,
            minimum_tile_samples: 8,
            minimum_varied_tile_fraction_bp: 2_500,
        }
    }

    /// A stricter composition-oriented floor for the Campaign 2 inspection
    /// cuts.  It intentionally has a different job from the terrain-only
    /// reference profile: hero views spend more pixels on authored geometry,
    /// so coverage and edge thresholds are calibrated against the declared
    /// close/medium/wide authored-frame composition while color, luminance,
    /// and tile-variation requirements remain real gates.
    pub fn campaign2_authored_frame_v1(width_px: u32, height_px: u32) -> Self {
        Self {
            schema_version: VISUAL_QUALITY_PROFILE_SCHEMA.into(),
            profile_id: "campaign2-authored-frame".into(),
            profile_version: 1,
            capture_width_px: width_px,
            capture_height_px: height_px,
            capture_format: CaptureFormat::Rgba8Srgb,
            region: QualityRegion::ProjectedTerrain,
            maximum_terrain_grid_side: 129,
            minimum_region_coverage_bp: 2_400,
            minimum_sampled_pixels: 8_192,
            maximum_overlay_coverage_bp: 2_500,
            minimum_luminance_p95_p05_span: 48,
            minimum_distinct_luminance_bins: 8,
            minimum_distinct_rgb_bins: 96,
            minimum_edge_pair_fraction_bp: 220,
            minimum_edge_luminance_delta: 12,
            tile_columns: 8,
            tile_rows: 8,
            minimum_tile_luminance_span: 10,
            minimum_tile_samples: 8,
            minimum_varied_tile_fraction_bp: 4_500,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualityRegion {
    ProjectedTerrain,
    /// A declared pixel rectangle is intersected with projected terrain
    /// coverage, then semantic-overlay pixels are removed before measurement.
    DeclaredContentRect {
        region_id: String,
        x_px: u32,
        y_px: u32,
        width_px: u32,
        height_px: u32,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QualityOutcome {
    Good,
    Bad,
    Indeterminate,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QualityReasonCode {
    InvalidProfile,
    InvalidPacket,
    InvalidReceipt,
    CaptureBindingMismatch,
    CaptureDigestMismatch,
    ReceiptMeasurementMismatch,
    FrameNotPromoted,
    TerrainRegionUnavailable,
    AnalysisBudgetExceeded,
    InsufficientRegionCoverage,
    InsufficientSamplePixels,
    ExcessiveSemanticOverlayCoverage,
    InsufficientLuminanceRange,
    InsufficientLuminanceDiversity,
    InsufficientColorDiversity,
    InsufficientSpatialEdges,
    InsufficientTileVariation,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QualityReason {
    pub code: QualityReasonCode,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VisualQualityMeasurements {
    pub total_capture_pixels: u64,
    pub terrain_region_pixels: u64,
    pub authored_geometry_pixels: u64,
    pub content_region_pixels: u64,
    pub semantic_overlay_excluded_pixels: u64,
    pub measured_pixels: u64,
    pub region_coverage_bp: u16,
    pub content_region_coverage_bp: u16,
    pub semantic_overlay_coverage_bp: u16,
    pub luminance_p05: u8,
    pub luminance_p95: u8,
    pub luminance_p95_p05_span: u8,
    pub distinct_luminance_bins: u8,
    pub distinct_rgb_bins: u16,
    pub candidate_neighbor_pairs: u64,
    pub edge_neighbor_pairs: u64,
    pub edge_pair_fraction_bp: u16,
    pub varied_tiles: u16,
    pub total_tiles: u16,
    pub varied_tile_fraction_bp: u16,
    pub sampled_terrain_grid_side: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VisualQualityEvidenceBody {
    pub schema_version: String,
    pub profile: VisualQualityProfile,
    pub profile_sha256: String,
    pub packet_id: String,
    pub packet_sha256: String,
    pub capture_id: String,
    pub camera_id: String,
    pub frame_receipt_sha256: String,
    pub raw_capture_sha256: Option<String>,
    pub width_px: u32,
    pub height_px: u32,
    pub format: CaptureFormat,
    pub outcome: QualityOutcome,
    pub reasons: Vec<QualityReason>,
    pub measurements: Option<VisualQualityMeasurements>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VisualQualityEvidence {
    pub body: VisualQualityEvidenceBody,
    pub evidence_sha256: String,
}

/// A multidimensional observation record for the authored-frame campaign.
/// Values are deliberately named observations rather than a composite score:
/// some axes are deterministic image proxies, while axes requiring depth,
/// temporal history, or an authored reference remain explicitly indeterminate.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualObservationStatus {
    Measured,
    Indeterminate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VisualObservation {
    pub status: VisualObservationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_bp: Option<u16>,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Campaign2VisualMeasurements {
    pub silhouette_readability: VisualObservation,
    pub material_separation: VisualObservation,
    pub grounding_contact: VisualObservation,
    pub lighting_consistency: VisualObservation,
    pub atmospheric_depth: VisualObservation,
    pub texture_frequency: VisualObservation,
    pub composition: VisualObservation,
    pub density: VisualObservation,
    pub artifact_rate: VisualObservation,
    pub frame_cost_us: u64,
    pub gpu_frame_cost_us: Option<u64>,
    pub upload_memory_bytes: usize,
    pub readback_bytes: usize,
    pub visible_instance_count: usize,
    pub total_instance_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Campaign2VisualEvidenceBody {
    pub schema_version: String,
    pub view_role: String,
    pub packet_id: String,
    pub packet_sha256: String,
    pub capture_id: String,
    pub camera_id: String,
    pub frame_receipt_sha256: String,
    pub runtime_frame_receipt_sha256: String,
    pub raw_capture_sha256: String,
    pub measurements: Campaign2VisualMeasurements,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Campaign2VisualEvidence {
    pub body: Campaign2VisualEvidenceBody,
    pub evidence_sha256: String,
}

/// Independently bind a quality assessment to a validated scene packet, a
/// Rust-promoted frame receipt, and the exact RGBA8 sRGB capture bytes.
pub fn assess_visual_quality(
    packet: &GraphicsScenePacket,
    receipt: &GraphicsFrameReceipt,
    capture_bytes: &[u8],
    profile: &VisualQualityProfile,
) -> VisualQualityEvidence {
    let profile_bytes = match canonical_json(profile) {
        Ok(bytes) => bytes,
        Err(error) => {
            return evidence(
                packet,
                receipt,
                profile,
                None,
                QualityOutcome::Failed,
                vec![reason(QualityReasonCode::InvalidProfile, error.to_string())],
                None,
            );
        }
    };
    let profile_sha256 = sha256_prefixed(&profile_bytes);
    if let Err(detail) = validate_profile(profile) {
        return evidence(
            packet,
            receipt,
            profile,
            Some(profile_sha256),
            QualityOutcome::Failed,
            vec![reason(QualityReasonCode::InvalidProfile, detail)],
            None,
        );
    }
    if let Err(error) = validate_scene_packet(packet) {
        return evidence(
            packet,
            receipt,
            profile,
            Some(profile_sha256),
            QualityOutcome::Failed,
            vec![reason(QualityReasonCode::InvalidPacket, error.to_string())],
            None,
        );
    }
    if let Err(detail) = validate_receipt_binding(packet, receipt) {
        return evidence(
            packet,
            receipt,
            profile,
            Some(profile_sha256),
            QualityOutcome::Failed,
            vec![reason(QualityReasonCode::InvalidReceipt, detail)],
            None,
        );
    }
    if receipt.body.status != FrameStatus::Passed {
        return evidence(
            packet,
            receipt,
            profile,
            Some(profile_sha256),
            QualityOutcome::Indeterminate,
            vec![reason(
                QualityReasonCode::FrameNotPromoted,
                "frame receipt is not a passed Rust-promoted capture".into(),
            )],
            None,
        );
    }

    let capture = &packet.body.capture;
    if profile.capture_width_px != capture.width_px
        || profile.capture_height_px != capture.height_px
        || profile.capture_format != capture.format
        || receipt.body.width_px != profile.capture_width_px
        || receipt.body.height_px != profile.capture_height_px
        || receipt.body.format != profile.capture_format
    {
        return evidence(
            packet,
            receipt,
            profile,
            Some(profile_sha256),
            QualityOutcome::Failed,
            vec![reason(
                QualityReasonCode::CaptureBindingMismatch,
                "quality profile dimensions or format do not match the packet and promoted receipt"
                    .into(),
            )],
            None,
        );
    }

    let expected_bytes = match capture_byte_length(capture.width_px, capture.height_px) {
        Some(length) => length,
        None => {
            return evidence(
                packet,
                receipt,
                profile,
                Some(profile_sha256),
                QualityOutcome::Failed,
                vec![reason(
                    QualityReasonCode::CaptureBindingMismatch,
                    "capture dimensions exceed the bounded quality-analysis envelope".into(),
                )],
                None,
            );
        }
    };
    if capture_bytes.len() != expected_bytes {
        return evidence(
            packet,
            receipt,
            profile,
            Some(profile_sha256),
            QualityOutcome::Failed,
            vec![reason(
                QualityReasonCode::CaptureBindingMismatch,
                format!(
                    "capture has {} bytes, expected {expected_bytes}",
                    capture_bytes.len()
                ),
            )],
            None,
        );
    }
    let capture_sha256 = sha256_prefixed(capture_bytes);
    if receipt.body.capture_sha256.as_deref() != Some(capture_sha256.as_str()) {
        return evidence(
            packet,
            receipt,
            profile,
            Some(profile_sha256),
            QualityOutcome::Failed,
            vec![reason(
                QualityReasonCode::CaptureDigestMismatch,
                "raw RGBA bytes do not match the digest bound by the frame receipt".into(),
            )],
            Some(capture_sha256),
        );
    }
    if let Err(error) = validate_frame_receipt(receipt, packet, capture_bytes) {
        return evidence(
            packet,
            receipt,
            profile,
            Some(profile_sha256),
            QualityOutcome::Failed,
            vec![reason(QualityReasonCode::InvalidReceipt, error.to_string())],
            Some(capture_sha256),
        );
    }
    let measured = match measure_frame_capture(packet, capture_bytes) {
        Ok(measured) => measured,
        Err(error) => {
            return evidence(
                packet,
                receipt,
                profile,
                Some(profile_sha256),
                QualityOutcome::Failed,
                vec![reason(
                    QualityReasonCode::InvalidPacket,
                    format!("native capture remeasurement failed: {error}"),
                )],
                Some(capture_sha256),
            );
        }
    };
    if measured != receipt.body.measurements {
        return evidence(
            packet,
            receipt,
            profile,
            Some(profile_sha256),
            QualityOutcome::Failed,
            vec![reason(
                QualityReasonCode::ReceiptMeasurementMismatch,
                "receipt measurements do not match Rust measurements of the supplied capture"
                    .into(),
            )],
            Some(capture_sha256),
        );
    }

    let mut raster_work = 0u64;
    let region_mask = match terrain_region_mask(packet, profile, &mut raster_work) {
        Ok(mask) => mask,
        Err(AnalysisFailure::Budget) => {
            return evidence(
                packet,
                receipt,
                profile,
                Some(profile_sha256),
                QualityOutcome::Indeterminate,
                vec![reason(
                    QualityReasonCode::AnalysisBudgetExceeded,
                    "terrain projection exceeded the deterministic raster-work limit".into(),
                )],
                Some(capture_sha256),
            );
        }
        Err(AnalysisFailure::Unavailable(detail)) => {
            return evidence(
                packet,
                receipt,
                profile,
                Some(profile_sha256),
                QualityOutcome::Indeterminate,
                vec![reason(QualityReasonCode::TerrainRegionUnavailable, detail)],
                Some(capture_sha256),
            );
        }
    };
    let overlay_mask = match semantic_overlay_mask(packet, &mut raster_work) {
        Ok(mask) => mask,
        Err(AnalysisFailure::Budget) => {
            return evidence(
                packet,
                receipt,
                profile,
                Some(profile_sha256),
                QualityOutcome::Indeterminate,
                vec![reason(
                    QualityReasonCode::AnalysisBudgetExceeded,
                    "semantic-overlay exclusion exceeded the deterministic raster-work limit"
                        .into(),
                )],
                Some(capture_sha256),
            );
        }
        Err(AnalysisFailure::Unavailable(detail)) => {
            return evidence(
                packet,
                receipt,
                profile,
                Some(profile_sha256),
                QualityOutcome::Indeterminate,
                vec![reason(QualityReasonCode::TerrainRegionUnavailable, detail)],
                Some(capture_sha256),
            );
        }
    };
    let (measurements, mut reasons) = match measure_quality_region(QualityRegionInput {
        capture: capture_bytes,
        width: capture.width_px as usize,
        height: capture.height_px as usize,
        terrain_mask: &region_mask.terrain,
        authored_geometry_mask: &region_mask.authored_geometry,
        content_mask: &region_mask.content,
        overlay_mask: &overlay_mask,
        profile,
        sampled_grid_side: sampled_grid_side(packet, profile),
    }) {
        Ok(measurements) => (measurements, Vec::new()),
        Err(AnalysisFailure::Budget) => {
            return evidence(
                packet,
                receipt,
                profile,
                Some(profile_sha256),
                QualityOutcome::Indeterminate,
                vec![reason(
                    QualityReasonCode::AnalysisBudgetExceeded,
                    "quality measurements exceeded the deterministic pixel-work limit".into(),
                )],
                Some(capture_sha256),
            );
        }
        Err(AnalysisFailure::Unavailable(detail)) => {
            return evidence(
                packet,
                receipt,
                profile,
                Some(profile_sha256),
                QualityOutcome::Indeterminate,
                vec![reason(QualityReasonCode::TerrainRegionUnavailable, detail)],
                Some(capture_sha256),
            );
        }
    };
    evaluate_thresholds(profile, &measurements, &mut reasons);
    let outcome = if reasons.is_empty() {
        QualityOutcome::Good
    } else {
        QualityOutcome::Bad
    };
    evidence(
        packet,
        receipt,
        profile,
        Some(profile_sha256),
        outcome,
        reasons,
        Some(capture_sha256),
    )
    .with_measurements(measurements)
}

/// Recompute both the evidence digest and the full assessment from its native
/// packet, receipt, and bytes. A matching digest alone never promotes quality.
pub fn validate_visual_quality_evidence(
    evidence: &VisualQualityEvidence,
    packet: &GraphicsScenePacket,
    receipt: &GraphicsFrameReceipt,
    capture_bytes: &[u8],
) -> Result<(), GraphicsContractError> {
    validate_registered_visual_quality_profile(&evidence.body.profile)?;
    let expected_evidence_sha256 = sha256_prefixed(&canonical_json(&evidence.body)?);
    if expected_evidence_sha256 != evidence.evidence_sha256 {
        return Err(GraphicsContractError::provenance(
            "visual-quality evidence digest does not match its canonical body",
        ));
    }
    if assess_visual_quality(packet, receipt, capture_bytes, &evidence.body.profile) != *evidence {
        return Err(GraphicsContractError::provenance(
            "visual-quality evidence does not match independent Rust remeasurement",
        ));
    }
    Ok(())
}

/// Produce the campaign's visual evidence vector from Rust-owned packet,
/// receipt, quality measurements, and raw capture inputs.  This records
/// deterministic image proxies and refuses to invent values for properties
/// that the current color-only capture cannot establish.
pub fn assess_campaign2_visual_evidence(
    packet: &GraphicsScenePacket,
    certification_receipt: &GraphicsFrameReceipt,
    runtime_receipt: &GraphicsFrameReceipt,
    quality: &VisualQualityEvidence,
    capture_bytes: &[u8],
    view_role: &str,
) -> Campaign2VisualEvidence {
    let quality_measurements = quality.body.measurements.as_ref();
    let frame_measurements = quality_measurements;
    let total_pixels = packet
        .body
        .capture
        .width_px
        .checked_mul(packet.body.capture.height_px)
        .map(u64::from)
        .unwrap_or(0);
    let invalid_alpha_pixels = capture_bytes
        .chunks_exact(4)
        .filter(|pixel| pixel[3] != u8::MAX)
        .count() as u64;
    let telemetry = &runtime_receipt.body.telemetry;
    let total_instances = telemetry.instance_count;
    let visible_instances = telemetry.visible_instance_count;
    let measurements = Campaign2VisualMeasurements {
        silhouette_readability: frame_measurements
            .map(|value| measured(
                value.edge_pair_fraction_bp,
                "content edge-pair fraction is a deterministic silhouette/readability proxy",
            ))
            .unwrap_or_else(|| indeterminate("quality profile did not produce content measurements")),
        material_separation: frame_measurements
            .map(|value| {
                measured(
                    basis_points(u64::from(value.distinct_rgb_bins), 512),
                    "distinct 12-bit RGB bins are a deterministic material/color separation proxy",
                )
            })
            .unwrap_or_else(|| indeterminate("quality profile did not produce color measurements")),
        grounding_contact: indeterminate(
            "contact/grounding judgment is deferred: the current promoted capture has no depth or contact-classifier evidence",
        ),
        lighting_consistency: indeterminate(
            "lighting consistency judgment is deferred: packet light/environment intent is bound, but no reference-light comparison is promoted",
        ),
        atmospheric_depth: indeterminate(format!(
            "atmospheric appearance is not independently judged; packet declares fog_density={} and fog_color_rgb={:?}",
            packet.body.environment.fog_density, packet.body.environment.fog_color_rgb
        )),
        texture_frequency: frame_measurements
            .map(|value| {
                let combined = (u32::from(value.edge_pair_fraction_bp)
                    + u32::from(value.distinct_rgb_bins.min(512)))
                    / 2;
                measured(
                    combined.min(10_000) as u16,
                    "edge fraction plus color-bin diversity are a deterministic texture-frequency proxy",
                )
            })
            .unwrap_or_else(|| indeterminate("quality profile did not produce texture measurements")),
        composition: frame_measurements
            .map(|value| {
                measured(
                    value.content_region_coverage_bp,
                    "projected content coverage is a deterministic composition occupancy proxy",
                )
            })
            .unwrap_or_else(|| indeterminate("quality profile did not produce composition measurements")),
        density: if total_instances == 0 {
            indeterminate("packet contains no instances")
        } else {
            measured(
                basis_points(visible_instances as u64, total_instances as u64),
                "visible/total instance ratio is a deterministic population-density proxy",
            )
        },
        artifact_rate: measured(
            basis_points(invalid_alpha_pixels, total_pixels),
            "non-opaque output alpha pixels are the currently measurable capture-artifact proxy",
        ),
        frame_cost_us: telemetry.frame_time_us,
        gpu_frame_cost_us: telemetry.gpu_frame_time_us,
        upload_memory_bytes: telemetry.upload_bytes,
        readback_bytes: telemetry.readback_bytes,
        visible_instance_count: visible_instances,
        total_instance_count: total_instances,
    };
    let body = Campaign2VisualEvidenceBody {
        schema_version: CAMPAIGN2_VISUAL_EVIDENCE_SCHEMA.into(),
        view_role: view_role.into(),
        packet_id: packet.body.packet_id.clone(),
        packet_sha256: packet.packet_sha256.clone(),
        capture_id: packet.body.capture.capture_id.clone(),
        camera_id: packet.body.camera.camera_id.clone(),
        frame_receipt_sha256: certification_receipt.receipt_sha256.clone(),
        runtime_frame_receipt_sha256: runtime_receipt.receipt_sha256.clone(),
        raw_capture_sha256: sha256_prefixed(capture_bytes),
        measurements,
    };
    let evidence_sha256 =
        sha256_prefixed(&canonical_json(&body).expect("Campaign 2 visual evidence is JSON-safe"));
    Campaign2VisualEvidence {
        body,
        evidence_sha256,
    }
}

/// Revalidate the campaign evidence vector from the same Rust inputs.  This
/// is intentionally separate from the registered technical visual gate:
/// vector observations explain the frame, while the registered profile makes
/// the pass/fail decision.
pub fn validate_campaign2_visual_evidence(
    evidence: &Campaign2VisualEvidence,
    packet: &GraphicsScenePacket,
    certification_receipt: &GraphicsFrameReceipt,
    runtime_receipt: &GraphicsFrameReceipt,
    quality: &VisualQualityEvidence,
    capture_bytes: &[u8],
) -> Result<(), GraphicsContractError> {
    if evidence.body.schema_version != CAMPAIGN2_VISUAL_EVIDENCE_SCHEMA
        || evidence.body.view_role.trim().is_empty()
    {
        return Err(GraphicsContractError::provenance(
            "Campaign 2 visual evidence has an unsupported schema or empty view role",
        ));
    }
    let expected_digest = sha256_prefixed(
        &canonical_json(&evidence.body)
            .map_err(|error| GraphicsContractError::malformed(error.to_string()))?,
    );
    if expected_digest != evidence.evidence_sha256 {
        return Err(GraphicsContractError::provenance(
            "Campaign 2 visual evidence digest does not match its canonical body",
        ));
    }
    if evidence.body.packet_id != packet.body.packet_id
        || evidence.body.packet_sha256 != packet.packet_sha256
        || evidence.body.capture_id != packet.body.capture.capture_id
        || evidence.body.camera_id != packet.body.camera.camera_id
        || evidence.body.frame_receipt_sha256 != certification_receipt.receipt_sha256
        || evidence.body.runtime_frame_receipt_sha256 != runtime_receipt.receipt_sha256
        || evidence.body.raw_capture_sha256 != sha256_prefixed(capture_bytes)
    {
        return Err(GraphicsContractError::provenance(
            "Campaign 2 visual evidence is detached from packet, camera, receipt, or capture",
        ));
    }
    validate_visual_quality_evidence(quality, packet, certification_receipt, capture_bytes)?;
    validate_frame_receipt(runtime_receipt, packet, capture_bytes)?;
    let expected = assess_campaign2_visual_evidence(
        packet,
        certification_receipt,
        runtime_receipt,
        quality,
        capture_bytes,
        &evidence.body.view_role,
    );
    if expected != *evidence {
        return Err(GraphicsContractError::provenance(
            "Campaign 2 visual evidence does not match independent Rust remeasurement",
        ));
    }
    Ok(())
}

fn measured(value_bp: u16, detail: impl Into<String>) -> VisualObservation {
    VisualObservation {
        status: VisualObservationStatus::Measured,
        value_bp: Some(value_bp.min(10_000)),
        detail: detail.into(),
    }
}

fn indeterminate(detail: impl Into<String>) -> VisualObservation {
    VisualObservation {
        status: VisualObservationStatus::Indeterminate,
        value_bp: None,
        detail: detail.into(),
    }
}

/// Validate the profile against the Rust-owned registry used for promotion.
/// Experimental callers may still measure custom profiles through
/// [`assess_visual_quality`], but no custom threshold set can be revalidated
/// as certification evidence.
pub fn validate_registered_visual_quality_profile(
    profile: &VisualQualityProfile,
) -> Result<(), GraphicsContractError> {
    let registered = match profile.profile_id.as_str() {
        "terrain-technical-reference" => VisualQualityProfile::terrain_reference_v1(
            profile.capture_width_px,
            profile.capture_height_px,
        ),
        "campaign2-authored-frame" => VisualQualityProfile::campaign2_authored_frame_v1(
            profile.capture_width_px,
            profile.capture_height_px,
        ),
        _ => {
            return Err(GraphicsContractError::provenance(
                "visual-quality profile is not the registered Rust certification profile",
            ));
        }
    };
    if profile != &registered {
        return Err(GraphicsContractError::provenance(
            "visual-quality profile is not the registered Rust certification profile",
        ));
    }
    Ok(())
}

impl VisualQualityEvidence {
    fn with_measurements(mut self, measurements: VisualQualityMeasurements) -> Self {
        self.body.measurements = Some(measurements);
        self.evidence_sha256 = sha256_prefixed(
            &canonical_json(&self.body).expect("integer-only visual-quality evidence serializes"),
        );
        self
    }
}

fn validate_profile(profile: &VisualQualityProfile) -> Result<(), String> {
    if profile.schema_version != VISUAL_QUALITY_PROFILE_SCHEMA {
        return Err(format!(
            "unsupported visual-quality profile schema {}",
            profile.schema_version
        ));
    }
    if !valid_profile_id(&profile.profile_id) || profile.profile_version != 1 {
        return Err("profile id is malformed or profile version is unsupported".into());
    }
    if profile.capture_width_px == 0
        || profile.capture_height_px == 0
        || profile.capture_width_px > MAX_CAPTURE_DIMENSION
        || profile.capture_height_px > MAX_CAPTURE_DIMENSION
        || capture_byte_length(profile.capture_width_px, profile.capture_height_px).is_none()
    {
        return Err("profile dimensions are outside the bounded capture envelope".into());
    }
    if !(2..=MAX_TERRAIN_GRID_SIDE).contains(&profile.maximum_terrain_grid_side) {
        return Err("terrain sample grid side must be between 2 and 257".into());
    }
    for (value, label) in [
        (
            profile.minimum_region_coverage_bp,
            "minimum_region_coverage_bp",
        ),
        (
            profile.maximum_overlay_coverage_bp,
            "maximum_overlay_coverage_bp",
        ),
        (
            profile.minimum_edge_pair_fraction_bp,
            "minimum_edge_pair_fraction_bp",
        ),
        (
            profile.minimum_varied_tile_fraction_bp,
            "minimum_varied_tile_fraction_bp",
        ),
    ] {
        if value > 10_000 {
            return Err(format!("{label} exceeds 10000 basis points"));
        }
    }
    if profile.minimum_sampled_pixels == 0
        || profile.minimum_distinct_luminance_bins == 0
        || usize::from(profile.minimum_distinct_luminance_bins) > LUMINANCE_BIN_COUNT
        || profile.minimum_distinct_rgb_bins == 0
        || usize::from(profile.minimum_distinct_rgb_bins) > RGB_BIN_COUNT
        || profile.tile_columns < 2
        || profile.tile_rows < 2
        || profile.tile_columns > 16
        || profile.tile_rows > 16
        || profile.minimum_tile_samples == 0
    {
        return Err("profile measurement counts are outside the supported bounds".into());
    }
    match &profile.region {
        QualityRegion::ProjectedTerrain => {}
        QualityRegion::DeclaredContentRect {
            region_id,
            x_px,
            y_px,
            width_px,
            height_px,
        } => {
            if !valid_profile_id(region_id)
                || *width_px == 0
                || *height_px == 0
                || x_px.checked_add(*width_px).is_none()
                || y_px.checked_add(*height_px).is_none()
                || x_px + width_px > profile.capture_width_px
                || y_px + height_px > profile.capture_height_px
            {
                return Err(
                    "declared content rectangle is malformed or outside the capture".into(),
                );
            }
        }
    }
    Ok(())
}

fn validate_receipt_binding(
    packet: &GraphicsScenePacket,
    receipt: &GraphicsFrameReceipt,
) -> Result<(), String> {
    crate::validate_frame_receipt_shape(receipt).map_err(|error| error.to_string())?;
    let body_digest =
        sha256_prefixed(&canonical_json(&receipt.body).map_err(|error| error.to_string())?);
    if body_digest != receipt.receipt_sha256 {
        return Err("frame receipt digest does not match its canonical body".into());
    }
    let body = &receipt.body;
    if body.schema_version != FRAME_RECEIPT_SCHEMA
        || body.packet_sha256 != packet.packet_sha256
        || body.capture_id != packet.body.capture.capture_id
        || body.width_px != packet.body.capture.width_px
        || body.height_px != packet.body.capture.height_px
        || body.format != packet.body.capture.format
        || packet.body.capture.camera_id != packet.body.camera.camera_id
    {
        return Err("frame receipt is detached from packet, camera, or capture identity".into());
    }
    if body.backend_id != LAVA_BACKEND_ID
        || body.adapter_revision != ADAPTER_REVISION
        || body.lava_revision != LAVA_REVISION
        || body.detail.trim().is_empty()
    {
        return Err("frame receipt has incomplete or unsupported native provenance".into());
    }
    match body.status {
        FrameStatus::Passed => {
            let Some(capture_sha256) = body.capture_sha256.as_deref() else {
                return Err("passed frame receipt has no raw capture digest".into());
            };
            crate::valid_sha(capture_sha256, "capture_sha256")
                .map_err(|error| error.to_string())?;
        }
        FrameStatus::Failed | FrameStatus::Unsupported => {
            if body.capture_sha256.is_some() {
                return Err("failed or unsupported receipt cannot claim a capture digest".into());
            }
        }
    }
    crate::valid_id(&body.capture_id, "capture_id").map_err(|error| error.to_string())?;
    crate::valid_id(&body.device_uuid, "device_uuid").map_err(|error| error.to_string())?;
    crate::valid_sha(&body.packet_sha256, "packet_sha256").map_err(|error| error.to_string())?;
    crate::valid_sha(&body.worker_script_sha256, "worker_script_sha256")
        .map_err(|error| error.to_string())?;
    crate::valid_sha(&body.renderer_identity_sha256, "renderer_identity_sha256")
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn evidence(
    packet: &GraphicsScenePacket,
    receipt: &GraphicsFrameReceipt,
    profile: &VisualQualityProfile,
    profile_sha256: Option<String>,
    outcome: QualityOutcome,
    reasons: Vec<QualityReason>,
    raw_capture_sha256: Option<String>,
) -> VisualQualityEvidence {
    let profile_sha256 = profile_sha256
        .unwrap_or_else(|| sha256_prefixed(&canonical_json(profile).unwrap_or_default()));
    let body = VisualQualityEvidenceBody {
        schema_version: VISUAL_QUALITY_EVIDENCE_SCHEMA.into(),
        profile: profile.clone(),
        profile_sha256,
        packet_id: packet.body.packet_id.clone(),
        packet_sha256: packet.packet_sha256.clone(),
        capture_id: packet.body.capture.capture_id.clone(),
        camera_id: packet.body.camera.camera_id.clone(),
        frame_receipt_sha256: receipt.receipt_sha256.clone(),
        raw_capture_sha256,
        width_px: packet.body.capture.width_px,
        height_px: packet.body.capture.height_px,
        format: packet.body.capture.format,
        outcome,
        reasons,
        measurements: None,
    };
    let evidence_sha256 = sha256_prefixed(
        &canonical_json(&body).expect("visual-quality evidence uses JSON-safe fields"),
    );
    VisualQualityEvidence {
        body,
        evidence_sha256,
    }
}

fn reason(code: QualityReasonCode, detail: String) -> QualityReason {
    QualityReason { code, detail }
}

fn valid_profile_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'_' | b'-' | b'.'))
        })
}

fn capture_byte_length(width_px: u32, height_px: u32) -> Option<usize> {
    usize::try_from(width_px)
        .ok()?
        .checked_mul(usize::try_from(height_px).ok()?)?
        .checked_mul(4)
        .filter(|length| *length <= MAX_CAPTURE_BYTES)
}

#[derive(Debug)]
enum AnalysisFailure {
    Budget,
    Unavailable(String),
}

struct QualityMasks {
    /// Projected terrain after conservative exclusion of projected authored
    /// meshes. This mask owns the minimum terrain-coverage requirement.
    terrain: Vec<u8>,
    /// Projected authored meshes kept separate from terrain so props cannot
    /// masquerade as terrain while still contributing legitimate scene
    /// structure to content-level visual measurements.
    authored_geometry: Vec<u8>,
    /// The union used for color, spatial-edge, and tile measurements.
    content: Vec<u8>,
}

fn terrain_region_mask(
    packet: &GraphicsScenePacket,
    profile: &VisualQualityProfile,
    raster_work: &mut u64,
) -> Result<QualityMasks, AnalysisFailure> {
    let width = packet.body.capture.width_px as usize;
    let height = packet.body.capture.height_px as usize;
    let pixel_count = width * height;
    let mut terrain_mask = vec![0u8; pixel_count];
    let resolution = packet.body.terrain.resolution;
    let heights = match &packet.body.terrain.heights_m.payload {
        crate::BufferPayload::F32(heights) => heights,
        _ => {
            return Err(AnalysisFailure::Unavailable(
                "terrain heights are not available as validated f32 samples".into(),
            ));
        }
    };
    let side = resolution
        .min(usize::from(profile.maximum_terrain_grid_side))
        .max(2);
    let sample_indices = evenly_spaced_indices(resolution, side);
    let points = terrain_sample_points(packet, heights, &sample_indices);
    let sample_side = sample_indices.len();
    for row in 0..sample_side - 1 {
        for column in 0..sample_side - 1 {
            let top_left = points[row * sample_side + column];
            let top_right = points[row * sample_side + column + 1];
            let bottom_left = points[(row + 1) * sample_side + column];
            let bottom_right = points[(row + 1) * sample_side + column + 1];
            for triangle in [
                [top_left, top_right, bottom_right],
                [top_left, bottom_right, bottom_left],
            ] {
                let [Some(first), Some(second), Some(third)] = triangle else {
                    continue;
                };
                rasterize_triangle(
                    &mut terrain_mask,
                    width,
                    height,
                    [first, second, third],
                    raster_work,
                )?;
            }
        }
    }
    let terrain_pixel_count = terrain_mask.iter().filter(|pixel| **pixel != 0).count();
    if terrain_pixel_count == 0 {
        return Err(AnalysisFailure::Unavailable(
            "no terrain surface projects into the requested capture".into(),
        ));
    }
    if let QualityRegion::DeclaredContentRect {
        x_px,
        y_px,
        width_px,
        height_px,
        ..
    } = &profile.region
    {
        for (index, included) in terrain_mask.iter_mut().enumerate() {
            let x = index % width;
            let y = index / width;
            let in_rect = x >= *x_px as usize
                && x < (*x_px + *width_px) as usize
                && y >= *y_px as usize
                && y < (*y_px + *height_px) as usize;
            if !in_rect {
                *included = 0;
            }
        }
    }
    let mut geometry_mask = non_terrain_geometry_mask(packet, &mut *raster_work)?;
    if let QualityRegion::DeclaredContentRect {
        x_px,
        y_px,
        width_px,
        height_px,
        ..
    } = &profile.region
    {
        for (index, included) in geometry_mask.iter_mut().enumerate() {
            let x = index % width;
            let y = index / width;
            let in_rect = x >= *x_px as usize
                && x < (*x_px + *width_px) as usize
                && y >= *y_px as usize
                && y < (*y_px + *height_px) as usize;
            if !in_rect {
                *included = 0;
            }
        }
    }
    let mut content_mask = terrain_mask.clone();
    for ((terrain, geometry), content) in terrain_mask
        .iter_mut()
        .zip(geometry_mask.iter())
        .zip(content_mask.iter_mut())
    {
        if *geometry != 0 {
            *content = 1;
        }
        if *geometry != 0 {
            // The capture protocol carries RGBA, not a depth attachment. Be
            // conservative: any projected authored mesh is excluded from the
            // terrain sample instead of allowing props to inflate terrain
            // coverage or terrain-specific measurements.
            *terrain = 0;
        }
    }
    if terrain_mask.iter().all(|pixel| *pixel == 0) {
        return Err(AnalysisFailure::Unavailable(
            "declared region has no projected terrain pixels".into(),
        ));
    }
    Ok(QualityMasks {
        terrain: terrain_mask,
        authored_geometry: geometry_mask,
        content: content_mask,
    })
}

fn non_terrain_geometry_mask(
    packet: &GraphicsScenePacket,
    raster_work: &mut u64,
) -> Result<Vec<u8>, AnalysisFailure> {
    let width = packet.body.capture.width_px as usize;
    let height = packet.body.capture.height_px as usize;
    let mut mask = vec![0u8; width * height];
    for instance in &packet.body.instances {
        let Some(mesh) = packet
            .body
            .meshes
            .iter()
            .find(|mesh| mesh.mesh_id == instance.mesh_id)
        else {
            continue;
        };
        for triangle in mesh.indices.chunks_exact(3) {
            let points = [triangle[0], triangle[1], triangle[2]]
                .map(|index| mesh.positions_m[index as usize])
                .map(|position| transform_instance_point(&instance.transform, position))
                .map(|position| project_screen_point(&packet.body.camera, position));
            let [Some(first), Some(second), Some(third)] = points else {
                continue;
            };
            rasterize_triangle(
                &mut mask,
                width,
                height,
                [[first.0, first.1], [second.0, second.1], [third.0, third.1]],
                raster_work,
            )?;
        }
    }
    Ok(mask)
}

fn transform_instance_point(transform: &crate::Transform3d, position: [f32; 3]) -> [f32; 3] {
    let scaled = [
        position[0] * transform.scale_xyz[0],
        position[1] * transform.scale_xyz[1],
        position[2] * transform.scale_xyz[2],
    ];
    let [x, y, z, w] = transform.rotation_xyzw;
    let doubled = [x + x, y + y, z + z];
    let products = [
        x * doubled[0],
        y * doubled[1],
        z * doubled[2],
        x * doubled[1],
        x * doubled[2],
        y * doubled[2],
        w * doubled[0],
        w * doubled[1],
        w * doubled[2],
    ];
    let rotated = [
        (1.0 - products[1] - products[2]) * scaled[0]
            + (products[3] - products[8]) * scaled[1]
            + (products[4] + products[7]) * scaled[2],
        (products[3] + products[8]) * scaled[0]
            + (1.0 - products[0] - products[2]) * scaled[1]
            + (products[5] - products[6]) * scaled[2],
        (products[4] - products[7]) * scaled[0]
            + (products[5] + products[6]) * scaled[1]
            + (1.0 - products[0] - products[1]) * scaled[2],
    ];
    [
        rotated[0] + transform.translation_xyz_m[0],
        rotated[1] + transform.translation_xyz_m[1],
        rotated[2] + transform.translation_xyz_m[2],
    ]
}

fn evenly_spaced_indices(resolution: usize, side: usize) -> Vec<usize> {
    (0..side)
        .map(|index| index * (resolution - 1) / (side - 1))
        .collect()
}

fn terrain_sample_points(
    packet: &GraphicsScenePacket,
    heights: &[f32],
    indices: &[usize],
) -> Vec<Option<[f32; 2]>> {
    let resolution = packet.body.terrain.resolution;
    let terrain = &packet.body.terrain;
    let up_axis = match packet.body.coordinate_system.up_axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    };
    let plane_axes = (0..3).filter(|axis| *axis != up_axis).collect::<Vec<_>>();
    let width_axis = plane_axes[0];
    let length_axis = plane_axes[1];
    let camera = &packet.body.camera;
    let mut points = Vec::with_capacity(indices.len() * indices.len());
    for &row in indices {
        for &column in indices {
            let sample = row * resolution + column;
            let mut position = [0.0f32; 3];
            position[up_axis] = heights[sample];
            position[width_axis] =
                -terrain.width_m * 0.5 + column as f32 * terrain.width_m / (resolution - 1) as f32;
            position[length_axis] =
                terrain.length_m * 0.5 - row as f32 * terrain.length_m / (resolution - 1) as f32;
            points.push(project_screen_point(camera, position).map(|(x, y, _)| [x, y]));
        }
    }
    points
}

fn rasterize_triangle(
    mask: &mut [u8],
    width: usize,
    height: usize,
    triangle: [[f32; 2]; 3],
    raster_work: &mut u64,
) -> Result<(), AnalysisFailure> {
    let min_x = triangle
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as usize;
    let max_x = triangle
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(width as f32) as usize;
    let min_y = triangle
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as usize;
    let max_y = triangle
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(height as f32) as usize;
    if min_x >= max_x || min_y >= max_y {
        return Ok(());
    }
    let visits = ((max_x - min_x) as u64)
        .checked_mul((max_y - min_y) as u64)
        .ok_or(AnalysisFailure::Budget)?;
    add_raster_work(raster_work, visits)?;
    let area = edge(triangle[0], triangle[1], triangle[2]);
    if area.abs() < 1e-8 {
        return Ok(());
    }
    for y in min_y..max_y {
        for x in min_x..max_x {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            let first = edge(triangle[0], triangle[1], point);
            let second = edge(triangle[1], triangle[2], point);
            let third = edge(triangle[2], triangle[0], point);
            let inside = if area > 0.0 {
                first >= -1e-5 && second >= -1e-5 && third >= -1e-5
            } else {
                first <= 1e-5 && second <= 1e-5 && third <= 1e-5
            };
            if inside {
                mask[y * width + x] = 1;
            }
        }
    }
    Ok(())
}

fn edge(first: [f32; 2], second: [f32; 2], point: [f32; 2]) -> f32 {
    (point[0] - first[0]) * (second[1] - first[1]) - (point[1] - first[1]) * (second[0] - first[0])
}

fn semantic_overlay_mask(
    packet: &GraphicsScenePacket,
    raster_work: &mut u64,
) -> Result<Vec<u8>, AnalysisFailure> {
    if packet.body.overlays.len() > MAX_QUALITY_OVERLAYS {
        return Err(AnalysisFailure::Budget);
    }
    let width = packet.body.capture.width_px as usize;
    let height = packet.body.capture.height_px as usize;
    let mut mask = vec![0u8; width * height];
    for overlay in &packet.body.overlays {
        match overlay {
            SemanticOverlay::Point {
                position_xyz_m,
                radius_m,
                ..
            } => {
                if let Some((x, y, pixels_per_meter)) =
                    project_screen_point(&packet.body.camera, *position_xyz_m)
                {
                    rasterize_disc(
                        &mut mask,
                        width,
                        height,
                        [x, y],
                        (*radius_m * pixels_per_meter + 4.0).max(5.0),
                        raster_work,
                    )?;
                }
            }
            SemanticOverlay::Circle {
                center_xyz_m,
                radius_m,
                ..
            } => {
                const STEPS: usize = 128;
                for step in 0..STEPS {
                    let angle_a = std::f32::consts::TAU * step as f32 / STEPS as f32;
                    let angle_b = std::f32::consts::TAU * (step + 1) as f32 / STEPS as f32;
                    let first = [
                        center_xyz_m[0] + *radius_m * angle_a.cos(),
                        center_xyz_m[1],
                        center_xyz_m[2] + *radius_m * angle_a.sin(),
                    ];
                    let second = [
                        center_xyz_m[0] + *radius_m * angle_b.cos(),
                        center_xyz_m[1],
                        center_xyz_m[2] + *radius_m * angle_b.sin(),
                    ];
                    rasterize_projected_overlay_segment(
                        &mut mask,
                        width,
                        height,
                        &packet.body.camera,
                        first,
                        second,
                        None,
                        raster_work,
                    )?;
                }
            }
            SemanticOverlay::Polyline {
                points_xyz_m,
                thickness_m,
                ..
            } => {
                for pair in points_xyz_m.windows(2) {
                    rasterize_projected_overlay_segment(
                        &mut mask,
                        width,
                        height,
                        &packet.body.camera,
                        pair[0],
                        pair[1],
                        Some(*thickness_m),
                        raster_work,
                    )?;
                }
            }
        }
    }
    Ok(mask)
}

// The kernel keeps the bounded raster surface and camera inputs explicit at
// the call site; grouping them would hide the analysis budget and dimensions
// that are part of this security-sensitive measurement path.
#[allow(clippy::too_many_arguments)]
fn rasterize_projected_overlay_segment(
    mask: &mut [u8],
    width: usize,
    height: usize,
    camera: &crate::GraphicsCamera,
    first: [f32; 3],
    second: [f32; 3],
    stroke_width_m: Option<f32>,
    raster_work: &mut u64,
) -> Result<(), AnalysisFailure> {
    let (first, second) = match crate::clip_overlay_segment(camera, first, second) {
        Some(segment) => segment,
        None => return Ok(()),
    };
    let Some((x0, y0, scale0)) = project_screen_point(camera, first) else {
        return Ok(());
    };
    let Some((x1, y1, scale1)) = project_screen_point(camera, second) else {
        return Ok(());
    };
    let radius = stroke_width_m
        .map(|thickness| thickness * scale0.max(scale1) + 4.0)
        .unwrap_or(5.0)
        .max(4.0);
    rasterize_segment(mask, width, height, [x0, y0], [x1, y1], radius, raster_work)
}

fn rasterize_disc(
    mask: &mut [u8],
    width: usize,
    height: usize,
    center: [f32; 2],
    radius: f32,
    raster_work: &mut u64,
) -> Result<(), AnalysisFailure> {
    let min_x = (center[0] - radius).floor().max(0.0) as usize;
    let max_x = (center[0] + radius).ceil().min(width as f32) as usize;
    let min_y = (center[1] - radius).floor().max(0.0) as usize;
    let max_y = (center[1] + radius).ceil().min(height as f32) as usize;
    if min_x >= max_x || min_y >= max_y {
        return Ok(());
    }
    let visits = ((max_x - min_x) as u64)
        .checked_mul((max_y - min_y) as u64)
        .ok_or(AnalysisFailure::Budget)?;
    add_raster_work(raster_work, visits)?;
    let radius_squared = radius * radius;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let dx = x as f32 + 0.5 - center[0];
            let dy = y as f32 + 0.5 - center[1];
            if dx * dx + dy * dy <= radius_squared {
                mask[y * width + x] = 1;
            }
        }
    }
    Ok(())
}

fn rasterize_segment(
    mask: &mut [u8],
    width: usize,
    height: usize,
    first: [f32; 2],
    second: [f32; 2],
    radius: f32,
    raster_work: &mut u64,
) -> Result<(), AnalysisFailure> {
    let min_x = (first[0].min(second[0]) - radius).floor().max(0.0) as usize;
    let max_x = (first[0].max(second[0]) + radius).ceil().min(width as f32) as usize;
    let min_y = (first[1].min(second[1]) - radius).floor().max(0.0) as usize;
    let max_y = (first[1].max(second[1]) + radius).ceil().min(height as f32) as usize;
    if min_x >= max_x || min_y >= max_y {
        return Ok(());
    }
    let visits = ((max_x - min_x) as u64)
        .checked_mul((max_y - min_y) as u64)
        .ok_or(AnalysisFailure::Budget)?;
    add_raster_work(raster_work, visits)?;
    let delta = [second[0] - first[0], second[1] - first[1]];
    let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
    let radius_squared = radius * radius;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            let amount = if length_squared > f32::EPSILON {
                (((point[0] - first[0]) * delta[0] + (point[1] - first[1]) * delta[1])
                    / length_squared)
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };
            let nearest = [first[0] + delta[0] * amount, first[1] + delta[1] * amount];
            let dx = point[0] - nearest[0];
            let dy = point[1] - nearest[1];
            if dx * dx + dy * dy <= radius_squared {
                mask[y * width + x] = 1;
            }
        }
    }
    Ok(())
}

fn add_raster_work(work: &mut u64, visits: u64) -> Result<(), AnalysisFailure> {
    *work = work.checked_add(visits).ok_or(AnalysisFailure::Budget)?;
    if *work > MAX_QUALITY_RASTER_VISITS {
        Err(AnalysisFailure::Budget)
    } else {
        Ok(())
    }
}

struct QualityRegionInput<'a> {
    capture: &'a [u8],
    width: usize,
    height: usize,
    terrain_mask: &'a [u8],
    authored_geometry_mask: &'a [u8],
    content_mask: &'a [u8],
    overlay_mask: &'a [u8],
    profile: &'a VisualQualityProfile,
    sampled_grid_side: u16,
}

fn measure_quality_region(
    input: QualityRegionInput<'_>,
) -> Result<VisualQualityMeasurements, AnalysisFailure> {
    let QualityRegionInput {
        capture,
        width,
        height,
        terrain_mask,
        authored_geometry_mask,
        content_mask,
        overlay_mask,
        profile,
        sampled_grid_side,
    } = input;
    let total_pixels = width.checked_mul(height).ok_or(AnalysisFailure::Budget)?;
    if capture.len() != total_pixels * 4
        || terrain_mask.len() != total_pixels
        || authored_geometry_mask.len() != total_pixels
        || content_mask.len() != total_pixels
        || overlay_mask.len() != total_pixels
    {
        return Err(AnalysisFailure::Unavailable(
            "capture and projected quality masks have inconsistent dimensions".into(),
        ));
    }
    let mut terrain_region_pixels = 0usize;
    let mut authored_geometry_pixels = 0usize;
    let mut content_region_pixels = 0usize;
    let mut excluded_pixels = 0usize;
    let mut histogram = [0u64; 256];
    let mut luminance_bins = [false; LUMINANCE_BIN_COUNT];
    let mut rgb_bins = [false; RGB_BIN_COUNT];
    let tile_count = usize::from(profile.tile_columns) * usize::from(profile.tile_rows);
    let mut tile_min = [u8::MAX; 256];
    let mut tile_max = [u8::MIN; 256];
    let mut tile_samples = [0u32; 256];
    let mut luma = vec![0u8; total_pixels];
    let mut sampled = vec![0u8; total_pixels];

    for index in 0..total_pixels {
        if terrain_mask[index] != 0 {
            terrain_region_pixels += 1;
        }
        if authored_geometry_mask[index] != 0 {
            authored_geometry_pixels += 1;
        }
        if content_mask[index] == 0 {
            continue;
        }
        content_region_pixels += 1;
        if overlay_mask[index] != 0 {
            excluded_pixels += 1;
            continue;
        }
        sampled[index] = 1;
        let pixel = &capture[index * 4..index * 4 + 4];
        let luminance = luminance_u8(pixel[0], pixel[1], pixel[2]);
        luma[index] = luminance;
        histogram[usize::from(luminance)] += 1;
        luminance_bins[usize::from(luminance) * LUMINANCE_BIN_COUNT / 256] = true;
        let rgb_bin = (usize::from(pixel[0] >> 4) << 8)
            | (usize::from(pixel[1] >> 4) << 4)
            | usize::from(pixel[2] >> 4);
        rgb_bins[rgb_bin] = true;
        let x = index % width;
        let y = index / width;
        let tile_x = (x * usize::from(profile.tile_columns) / width)
            .min(usize::from(profile.tile_columns) - 1);
        let tile_y =
            (y * usize::from(profile.tile_rows) / height).min(usize::from(profile.tile_rows) - 1);
        let tile_index = tile_y * usize::from(profile.tile_columns) + tile_x;
        tile_min[tile_index] = tile_min[tile_index].min(luminance);
        tile_max[tile_index] = tile_max[tile_index].max(luminance);
        tile_samples[tile_index] += 1;
    }

    if terrain_region_pixels == 0 {
        return Err(AnalysisFailure::Unavailable(
            "quality region contains no projected terrain pixels".into(),
        ));
    }
    let measured_pixels = sampled.iter().filter(|pixel| **pixel != 0).count();
    if measured_pixels == 0 {
        return Err(AnalysisFailure::Unavailable(
            "semantic overlays obscure every pixel in the declared region".into(),
        ));
    }
    let p05 = histogram_quantile(&histogram, measured_pixels as u64, 500);
    let p95 = histogram_quantile(&histogram, measured_pixels as u64, 9_500);
    let mut candidate_pairs = 0u64;
    let mut edge_pairs = 0u64;
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            if sampled[index] == 0 {
                continue;
            }
            if x + 1 < width && sampled[index + 1] != 0 {
                candidate_pairs += 1;
                edge_pairs += u64::from(
                    luma[index].abs_diff(luma[index + 1]) >= profile.minimum_edge_luminance_delta,
                );
            }
            if y + 1 < height && sampled[index + width] != 0 {
                candidate_pairs += 1;
                edge_pairs += u64::from(
                    luma[index].abs_diff(luma[index + width])
                        >= profile.minimum_edge_luminance_delta,
                );
            }
        }
    }
    let mut varied_tiles = 0usize;
    for tile in 0..tile_count {
        if tile_samples[tile] >= u32::from(profile.minimum_tile_samples)
            && tile_max[tile].saturating_sub(tile_min[tile]) >= profile.minimum_tile_luminance_span
        {
            varied_tiles += 1;
        }
    }
    let region_coverage_bp = basis_points(terrain_region_pixels as u64, total_pixels as u64);
    let content_region_coverage_bp =
        basis_points(content_region_pixels as u64, total_pixels as u64);
    let overlay_coverage_bp = basis_points(excluded_pixels as u64, content_region_pixels as u64);
    Ok(VisualQualityMeasurements {
        total_capture_pixels: total_pixels as u64,
        terrain_region_pixels: terrain_region_pixels as u64,
        authored_geometry_pixels: authored_geometry_pixels as u64,
        content_region_pixels: content_region_pixels as u64,
        semantic_overlay_excluded_pixels: excluded_pixels as u64,
        measured_pixels: measured_pixels as u64,
        region_coverage_bp,
        content_region_coverage_bp,
        semantic_overlay_coverage_bp: overlay_coverage_bp,
        luminance_p05: p05,
        luminance_p95: p95,
        luminance_p95_p05_span: p95.saturating_sub(p05),
        distinct_luminance_bins: luminance_bins.iter().filter(|present| **present).count() as u8,
        distinct_rgb_bins: rgb_bins.iter().filter(|present| **present).count() as u16,
        candidate_neighbor_pairs: candidate_pairs,
        edge_neighbor_pairs: edge_pairs,
        edge_pair_fraction_bp: basis_points(edge_pairs, candidate_pairs),
        varied_tiles: varied_tiles as u16,
        total_tiles: tile_count as u16,
        varied_tile_fraction_bp: basis_points(varied_tiles as u64, tile_count as u64),
        sampled_terrain_grid_side: sampled_grid_side,
    })
}

fn luminance_u8(red: u8, green: u8, blue: u8) -> u8 {
    ((u32::from(red) * 2_126 + u32::from(green) * 7_152 + u32::from(blue) * 722 + 5_000) / 10_000)
        as u8
}

fn histogram_quantile(histogram: &[u64; 256], count: u64, quantile_bp: u64) -> u8 {
    let rank = count.saturating_sub(1) * quantile_bp / 10_000;
    let mut accumulated = 0u64;
    for (value, frequency) in histogram.iter().enumerate() {
        accumulated += frequency;
        if accumulated > rank {
            return value as u8;
        }
    }
    u8::MAX
}

fn basis_points(numerator: u64, denominator: u64) -> u16 {
    if denominator == 0 {
        return 0;
    }
    ((u128::from(numerator) * 10_000 / u128::from(denominator)).min(10_000)) as u16
}

fn evaluate_thresholds(
    profile: &VisualQualityProfile,
    measurements: &VisualQualityMeasurements,
    reasons: &mut Vec<QualityReason>,
) {
    if measurements.region_coverage_bp < profile.minimum_region_coverage_bp {
        reasons.push(reason(
            QualityReasonCode::InsufficientRegionCoverage,
            format!(
                "projected content covers {} bp; profile requires {} bp",
                measurements.region_coverage_bp, profile.minimum_region_coverage_bp
            ),
        ));
    }
    if measurements.measured_pixels < u64::from(profile.minimum_sampled_pixels) {
        reasons.push(reason(
            QualityReasonCode::InsufficientSamplePixels,
            format!(
                "overlay-free content has {} pixels; profile requires {}",
                measurements.measured_pixels, profile.minimum_sampled_pixels
            ),
        ));
    }
    if measurements.semantic_overlay_coverage_bp > profile.maximum_overlay_coverage_bp {
        reasons.push(reason(
            QualityReasonCode::ExcessiveSemanticOverlayCoverage,
            format!(
                "semantic overlays obscure {} bp of the selected region; profile allows {} bp",
                measurements.semantic_overlay_coverage_bp, profile.maximum_overlay_coverage_bp
            ),
        ));
    }
    if measurements.luminance_p95_p05_span < profile.minimum_luminance_p95_p05_span {
        reasons.push(reason(
            QualityReasonCode::InsufficientLuminanceRange,
            format!(
                "5th-to-95th percentile luminance span is {}; profile requires {}",
                measurements.luminance_p95_p05_span, profile.minimum_luminance_p95_p05_span
            ),
        ));
    }
    if measurements.distinct_luminance_bins < profile.minimum_distinct_luminance_bins {
        reasons.push(reason(
            QualityReasonCode::InsufficientLuminanceDiversity,
            format!(
                "{} of 32 luminance bins are occupied; profile requires {}",
                measurements.distinct_luminance_bins, profile.minimum_distinct_luminance_bins
            ),
        ));
    }
    if measurements.distinct_rgb_bins < profile.minimum_distinct_rgb_bins {
        reasons.push(reason(
            QualityReasonCode::InsufficientColorDiversity,
            format!(
                "{} quantized RGB bins are occupied; profile requires {}",
                measurements.distinct_rgb_bins, profile.minimum_distinct_rgb_bins
            ),
        ));
    }
    if measurements.edge_pair_fraction_bp < profile.minimum_edge_pair_fraction_bp {
        reasons.push(reason(
            QualityReasonCode::InsufficientSpatialEdges,
            format!(
                "{} bp of adjacent pixel pairs have a measurable luminance edge; profile requires {} bp",
                measurements.edge_pair_fraction_bp, profile.minimum_edge_pair_fraction_bp
            ),
        ));
    }
    if measurements.varied_tile_fraction_bp < profile.minimum_varied_tile_fraction_bp {
        reasons.push(reason(
            QualityReasonCode::InsufficientTileVariation,
            format!(
                "{} bp of spatial tiles vary by the minimum luminance span; profile requires {} bp",
                measurements.varied_tile_fraction_bp, profile.minimum_varied_tile_fraction_bp
            ),
        ));
    }
}

fn sampled_grid_side(packet: &GraphicsScenePacket, profile: &VisualQualityProfile) -> u16 {
    packet
        .body
        .terrain
        .resolution
        .min(usize::from(profile.maximum_terrain_grid_side)) as u16
}
