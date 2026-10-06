//! Engine-neutral world compiler, validator, traversal runtime, and visual
//! reference evidence for Luxel.
//!
//! Julia owns terrain and region fields. Rust pins that boundary, derives the
//! world artifact identity, checks collision and navigation, runs traversal,
//! renders a deterministic reference capture, and revalidates its evidence.

mod bevy_capture;
mod fields;
mod gameplay_binding;
mod general_gameplay;
mod model;
mod physics;
mod runtime;
mod visual;
mod worker;
mod world;

pub use bevy_capture::{
    BEVY_CAPTURE_FORMAT, BEVY_CAPTURE_PROVENANCE_SCHEMA, BevyCaptureProvenance,
    BevyRendererIdentity, build_bevy_capture_provenance, validate_bevy_capture_provenance,
};
pub use fields::{JuliaFieldProvenance, JuliaFieldResponse, NumericalFields};
pub use gameplay_binding::{
    GameplayTickTelemetry, GameplayWorldBinding, GameplayWorldBindingBody, RuntimeCaptureMetadata,
    RuntimeCapturePhase, RuntimeEntityPose, build_gameplay_world_binding, validate_gameplay_kit,
    validate_gameplay_world_binding,
};
pub use general_gameplay::{
    GENERAL_GAMEPLAY_EVIDENCE_SCHEMA, GeneralGameplayEvidence, GeneralGameplayEvidenceBody,
    build_general_gameplay_evidence, validate_general_gameplay_evidence,
};
pub use model::{
    AuthoredLayout, EncounterSpec, ObstacleSpec, ReferenceCamera, SemanticRegion, SpawnRole,
    SpawnSpec, TerrainFeature, TerrainIntent, TraversalIntent, validate_layout,
};
pub use physics::{
    KinematicContact, KinematicContactKind, KinematicInput, KinematicState, KinematicStateBody,
    KinematicStepBody, KinematicStepDisposition, KinematicStepReceipt, PHYSICS_MAX_SWEEP_SAMPLES,
    PHYSICS_STATE_SCHEMA, PHYSICS_STEP_SCHEMA, PhysicsWorld,
};
pub use runtime::{
    MAX_TRAVERSAL_SESSION_STEPS, TRAVERSAL_SESSION_COMPLETION_SCHEMA,
    TRAVERSAL_SESSION_SNAPSHOT_SCHEMA, TraversalEventKind, TraversalEvidence,
    TraversalEvidenceBody, TraversalOutcome, TraversalSession, TraversalSessionCompletion,
    TraversalSessionCompletionBody, TraversalSessionInput, TraversalSessionSnapshot,
    TraversalSessionSnapshotBody, TraversalStep, run_playthrough, validate_traversal_evidence,
    validate_traversal_session_completion,
};
pub use visual::{
    VisualEvidence, VisualEvidenceBody, VisualGateStatus, VisualMeasurements,
    render_reference_capture, validate_visual_evidence,
};
pub use worker::{build_from_layout_path, julia_field_request, run_julia_field_worker};
pub use world::{
    CollisionArtifact, NavigationArtifact, NavigationSegment, SpawnArtifact, WorldArtifact,
    WorldArtifactBody, WorldBuild, build_world, validate_world_artifact,
};

pub const LAYOUT_SCHEMA: &str = "luxel.authored-world-layout/v1";
pub const WORLD_SCHEMA: &str = "luxel.world-artifact/v1";
pub const JULIA_FIELD_REQUEST_SCHEMA: &str = "luxel.julia-world-fields-request/v1";
pub const JULIA_FIELD_RESPONSE_SCHEMA: &str = "luxel.julia-world-fields-response/v1";
pub const TRAVERSAL_EVIDENCE_SCHEMA: &str = "luxel.reference-traversal-evidence/v1";
pub const VISUAL_EVIDENCE_SCHEMA: &str = "luxel.reference-visual-evidence/v1";
pub const TRAVERSAL_VALIDATOR_ID: &str = "luxel.reference-runtime.traversal/v1";
pub const VISUAL_VALIDATOR_ID: &str = "luxel.reference-runtime.visual/v1";
pub const GAMEPLAY_WORLD_BINDING_SCHEMA: &str = "luxel.gameplay-world-binding/v2";
pub const GAMEPLAY_WORLD_VALIDATOR_ID: &str = "luxel.reference-runtime.gameplay-binding/v2";
pub const GAMEPLAY_CAPTURE_METADATA_SCHEMA: &str = "luxel.reference-runtime-capture-metadata/v1";
pub const REFERENCE_TICK_RATE_HZ: u32 = luxel_gameplay_contract::GAMEPLAY_FIXED_TICK_RATE_HZ;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceRuntimeError {
    pub code: &'static str,
    pub message: String,
}

impl ReferenceRuntimeError {
    pub(crate) fn contract(message: String) -> Self {
        Self {
            code: "contract_violation",
            message,
        }
    }

    pub(crate) fn provenance(message: String) -> Self {
        Self {
            code: "provenance_failure",
            message,
        }
    }

    pub(crate) fn stale(message: String) -> Self {
        Self {
            code: "stale_runtime_step",
            message,
        }
    }

    pub(crate) fn divergence(message: String) -> Self {
        Self {
            code: "runtime_state_diverged",
            message,
        }
    }

    pub(crate) fn worker(message: String) -> Self {
        Self {
            code: "julia_worker_failure",
            message,
        }
    }
}

impl std::fmt::Display for ReferenceRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ReferenceRuntimeError {}
