//! Closed receipt payloads. Domain artifacts themselves are owned and
//! validated by `reference_runtime`, `intake_repair_contract`, and the asset
//! contract; this module only identifies their raw candidate-bound bytes.

use serde::{Deserialize, Serialize};
use wge_intake_repair_contract::{RepairEvidenceDelta, RepairEvidenceDeltaDraft, RepairProposal};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceArtifactBinding {
    pub source_id: String,
    pub artifact_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticReceiptPayload {
    pub intake_artifact_id: String,
    pub provider_response_artifact_id: String,
    pub source_bundle_artifact_id: String,
    /// Typed authored layout, also represented by one provider-bound design
    /// source in `source_artifacts` so the semantic and world gates cannot drift.
    pub layout_artifact_id: String,
    pub source_artifacts: Vec<SourceArtifactBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorldReceiptPayload {
    pub world_artifact_id: String,
    pub traversal_artifact_id: String,
    pub layout_artifact_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GameplayReceiptPayload {
    pub world_artifact_id: String,
    pub traversal_artifact_id: String,
    pub capture_artifact_id: String,
    pub visual_evidence_artifact_id: String,
    pub gameplay_binding_artifact_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetUse {
    StaticEnvironment,
    Character,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StaticMeshSource {
    pub schema_version: String,
    pub asset_id: String,
    pub positions_m: Vec<[f64; 3]>,
    pub triangle_indices: Vec<u32>,
    pub material_slots: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StaticAssetPackage {
    pub schema_version: String,
    pub asset_id: String,
    pub source_sha256: String,
    pub asset_use: AssetUse,
    pub bounds_min_m: [f64; 3],
    pub bounds_max_m: [f64; 3],
    pub pivot_m: [f64; 3],
    pub collision_bounds_min_m: [f64; 3],
    pub collision_bounds_max_m: [f64; 3],
    pub material_slots: Vec<String>,
    pub lod_triangle_counts: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetReceiptPayload {
    pub source_artifact_id: String,
    pub package_artifact_id: String,
    pub asset_use: AssetUse,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VisualReceiptPayload {
    pub world_artifact_id: String,
    pub capture_artifact_id: String,
    pub visual_evidence_artifact_id: String,
}

/// The payload carries canonical intake/repair contract types directly.
/// Rust recomputes `delta` from `proposal`, `delta_draft`, and raw receipts;
/// none of these caller-provided outcomes are trusted.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepairReceiptPayload {
    pub proposal_artifact_id: String,
    pub delta_draft_artifact_id: String,
    pub delta_artifact_id: String,
    pub before_snapshot_id: String,
    pub before_receipt_id: String,
    pub after_receipt_id: String,
    pub proposal: RepairProposal,
    pub delta_draft: RepairEvidenceDeltaDraft,
    pub delta: RepairEvidenceDelta,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeferredReceiptPayload {
    pub reason_code: String,
    pub deferral_scope: String,
    pub detail: String,
}
