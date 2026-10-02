//! A closed, deterministic single-process gameplay contract and reference runtime.
//!
//! The crate owns the meaning of gameplay snapshots, input events, state
//! transitions, replay checks, and receipts. It intentionally exposes no
//! scripting or arbitrary host-language execution surface.

mod live_runtime;
mod model;
mod runtime;

/// Typed capability registry, profile catalog, and deterministic kit resolver.
pub mod gcs;
/// Versioned generalized gameplay substrate. The original v1 vertical-slice
/// contract remains available for certified legacy bundles; new scenarios
/// should use this bounded, data-driven contract instead of adding another
/// fixture-specific branch to v1.
pub mod general;

pub use gcs::{
    CapabilityFeatureId, CapabilityId, CapabilityRegistry, CapabilitySpec, DiagnosticSeverity,
    KitManifest, KitProfileSpec, NetworkRequirement, OptionalIntegration, PersistenceRequirement,
    PersistenceScope, ProfileId, REFERENCE_VERTICAL_SLICE_KIT_ID, ResolutionCode,
    ResolutionDiagnostic, ResolutionReport, ResolvedKit, RuntimeCostClass, StateAuthority,
    ValidationSuiteId, foundation_registry, reference_vertical_slice_kit,
    reference_vertical_slice_manifest, resolve_kit,
};

pub use live_runtime::{GameplaySession, GameplaySessionSnapshot, GameplaySessionSnapshotBody};
pub use model::{
    AbilityId, AbilitySpec, Control, EntityAttributes, EntityId, EntitySpec, GameSnapshot,
    GameplayEffect, GameplayTag, LocationId, NavigationGraph, NpcBehavior, ObjectivePrerequisite,
    ObjectiveSpec, TargetingRule, validate_snapshot,
};
pub use runtime::{
    EventReceipt, GameOutcome, GameplayReceipt, GameplayReceiptBody, InputEvent, NpcAction,
    NpcDecision, NpcNoActionReason, ObjectiveState, ReplayExpectation, ReplayField, ReplayTrace,
    RuntimeEntity, RuntimeState, Transition, run_replay, verify_replay,
};

pub const GAMEPLAY_SNAPSHOT_SCHEMA: &str = "wge.gameplay-snapshot/v1";
pub const GAMEPLAY_TRACE_SCHEMA: &str = "wge.gameplay-trace/v1";
pub const GAMEPLAY_RECEIPT_SCHEMA: &str = "wge.gameplay-receipt/v1";
pub const GAMEPLAY_SESSION_SNAPSHOT_SCHEMA: &str = "wge.gameplay-session-snapshot/v1";
/// Every accepted gameplay input advances exactly one deterministic simulation tick.
pub const GAMEPLAY_FIXED_TICK_RATE_HZ: u32 = 30;
pub const MAX_REPLAY_EVENTS: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    UnsupportedSchema,
    InvalidIdentifier,
    PlayableEntityCount,
    NpcCount,
    InvalidAttributes,
    InvalidEntityTags,
    InvalidNavigationGraph,
    UnknownLocation,
    UnreachableObjective,
    UnreachableNpc,
    InvalidAbilityCost,
    InvalidCooldown,
    SilentAbility,
    InsufficientEncounterResources,
    InvalidNpcBehavior,
    InvalidObjective,
    TraceTooLong,
    UnknownEntity,
    EntityNotPlayable,
    EntityDefeated,
    MoveNotAdjacent,
    UnknownAbility,
    InvalidTarget,
    InsufficientEnergy,
    AbilityCooldownActive,
    ObjectiveNotAtLocation,
    ObjectivePrerequisiteUnmet,
    GameAlreadyFinished,
    TickOverflow,
    UnexpectedTick,
    ReplayDiverged,
    InputReadFailed,
    MalformedInput,
    OutputWriteFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameFailure {
    pub code: FailureCode,
    pub event_index: Option<usize>,
    pub subject: Option<String>,
    pub detail: String,
}

impl GameFailure {
    pub fn new(code: FailureCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            event_index: None,
            subject: None,
            detail: detail.into(),
        }
    }

    pub fn at_event(mut self, event_index: usize) -> Self {
        self.event_index = Some(event_index);
        self
    }

    pub fn subject(
        code: FailureCode,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            event_index: None,
            subject: Some(subject.into()),
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for GameFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.detail)?;
        if let Some(index) = self.event_index {
            write!(formatter, " (event {index})")?;
        }
        if let Some(subject) = &self.subject {
            write!(formatter, " [subject {subject}]")?;
        }
        Ok(())
    }
}

impl std::error::Error for GameFailure {}
