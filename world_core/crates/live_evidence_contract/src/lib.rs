//! Narrow contract for live-session evidence and periodic Tier A snapshots.
//!
//! This crate schedules and binds evidence. It does not render, inspect pixels,
//! validate certification receipts, or grant certification authority. A future
//! supervisor must independently validate Tier A receipts with WGE's registered
//! native validators before submitting a [`TierAResult::Certified`].

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SESSION_SCHEMA: &str = "wge.live-evidence-session/v1";
pub const SNAPSHOT_REQUEST_SCHEMA: &str = "wge.live-snapshot-request/v1";
pub const SNAPSHOT_RESULT_SCHEMA: &str = "wge.live-snapshot-result/v1";
pub const LIVE_FRAME_SCHEMA: &str = "wge.live-frame-attestation/v1";

pub const MAX_SAMPLE_INTERVAL_MS: u64 = 24 * 60 * 60 * 1_000;
pub const MAX_SAMPLE_WINDOW_MS: u64 = 120_000;
pub const MAX_FRAME_TIME_US: u64 = 1_000_000;
pub const MAX_GPU_TIME_US: u64 = 1_000_000;
pub const MAX_DRAW_CALLS: u32 = 100_000;
pub const MAX_INSTANCES: u32 = 1_000_000;
pub const MAX_DROPPED_FRAMES: u32 = 1_000_000;

pub type ContractResult<T> = Result<T, ContractDiagnostic>;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    StaleOrMismatchedPacket,
    SampleWindowExpired,
    MissingSnapshot,
    ForgedPassStatus,
    InvalidTransition,
    InvalidTelemetry,
    InvalidIdentity,
    ClockRegression,
    MalformedRepresentation,
    SnapshotRejected,
    InjectedFailure,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContractDiagnostic {
    pub code: DiagnosticCode,
    pub detail: String,
}

impl ContractDiagnostic {
    fn new(code: DiagnosticCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for ContractDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.detail)
    }
}

impl std::error::Error for ContractDiagnostic {}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PacketIdentity {
    /// Identity of the exact scene packet presented by the live renderer.
    pub packet_sha256: String,
    /// Identity of the renderer/device capability set used for this session.
    pub capabilities_sha256: String,
}

impl PacketIdentity {
    pub fn validate(&self) -> ContractResult<()> {
        validate_digest(&self.packet_sha256, "packet")?;
        validate_digest(&self.capabilities_sha256, "capabilities")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SamplingPolicy {
    /// Maximum interval between Tier A sample requests.
    pub interval_ms: u64,
    /// Maximum time allowed for a requested Tier A result.
    pub sample_window_ms: u64,
}

impl SamplingPolicy {
    pub fn validate(self) -> ContractResult<()> {
        if self.interval_ms == 0 || self.interval_ms > MAX_SAMPLE_INTERVAL_MS {
            return Err(ContractDiagnostic::new(
                DiagnosticCode::InvalidIdentity,
                format!("sample interval must be in 1..={MAX_SAMPLE_INTERVAL_MS} ms"),
            ));
        }
        if self.sample_window_ms == 0
            || self.sample_window_ms > MAX_SAMPLE_WINDOW_MS
            || self.sample_window_ms > self.interval_ms
        {
            return Err(ContractDiagnostic::new(
                DiagnosticCode::InvalidIdentity,
                format!(
                    "sample window must be in 1..={} ms and no longer than the interval",
                    self.interval_ms.min(MAX_SAMPLE_WINDOW_MS)
                ),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SampleTrigger {
    Periodic,
    SceneChanged,
    CameraCut,
    CapabilityChanged,
    Recovery,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticEventKind {
    SceneChanged,
    CameraCut,
    CapabilityChanged,
}

impl From<SemanticEventKind> for SampleTrigger {
    fn from(value: SemanticEventKind) -> Self {
        match value {
            SemanticEventKind::SceneChanged => Self::SceneChanged,
            SemanticEventKind::CameraCut => Self::CameraCut,
            SemanticEventKind::CapabilityChanged => Self::CapabilityChanged,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticEvent {
    /// Strictly increasing within one live session.
    pub sequence: u64,
    pub occurred_at_ms: u64,
    pub kind: SemanticEventKind,
    /// Packet/capability identity after the event has taken effect.
    pub packet: PacketIdentity,
}

/// Tier A request identity. `request_id` is derived from the remaining fields.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TierASnapshotRequest {
    pub schema_version: String,
    pub request_id: String,
    pub project_id: String,
    pub session_id: String,
    pub packet: PacketIdentity,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    /// Sorted and deduplicated so event coalescing has one canonical form.
    pub triggers: Vec<SampleTrigger>,
}

impl TierASnapshotRequest {
    pub fn canonical_bytes(&self) -> ContractResult<Vec<u8>> {
        canonical_bytes(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CertifiedSnapshotIdentity {
    pub project_id: String,
    pub snapshot_id: String,
    pub candidate_sha256: String,
    pub receipt_id: String,
    pub receipt_sha256: String,
    pub validator_registry_sha256: String,
    pub validator_id: String,
    pub receipt_schema: String,
}

impl CertifiedSnapshotIdentity {
    fn validate(&self, expected_project_id: &str) -> ContractResult<()> {
        validate_identifier(&self.project_id, "snapshot project")?;
        validate_identifier(&self.snapshot_id, "snapshot")?;
        validate_identifier(&self.receipt_id, "receipt")?;
        validate_identifier(&self.validator_id, "validator")?;
        validate_identifier(&self.receipt_schema, "receipt schema")?;
        validate_digest(&self.candidate_sha256, "candidate")?;
        validate_digest(&self.receipt_sha256, "receipt")?;
        validate_digest(&self.validator_registry_sha256, "validator registry")?;
        if self.project_id != expected_project_id {
            return Err(ContractDiagnostic::new(
                DiagnosticCode::StaleOrMismatchedPacket,
                "snapshot result belongs to a different project",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotFailure {
    SemanticGate,
    WorldGate,
    GameplayGate,
    VisualGate,
    RuntimeFailure,
    InjectedVisualCorruption,
}

/// Result identity returned by a future supervisor after Tier A processing.
/// There is deliberately no generic `status: "pass"` field.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum TierAResult {
    Certified {
        schema_version: String,
        request_id: String,
        project_id: String,
        session_id: String,
        packet: PacketIdentity,
        completed_at_ms: u64,
        snapshot: CertifiedSnapshotIdentity,
    },
    Rejected {
        schema_version: String,
        request_id: String,
        project_id: String,
        session_id: String,
        packet: PacketIdentity,
        completed_at_ms: u64,
        failure: SnapshotFailure,
    },
    Indeterminate {
        schema_version: String,
        request_id: String,
        project_id: String,
        session_id: String,
        packet: PacketIdentity,
        completed_at_ms: u64,
        reason: DiagnosticCode,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LiveFrameTelemetry {
    pub frame_time_us: u64,
    pub gpu_time_us: u64,
    pub draw_calls: u32,
    pub submitted_instances: u32,
    pub visible_instances: u32,
    pub dropped_frames: u32,
}

impl LiveFrameTelemetry {
    fn validate(&self) -> ContractResult<()> {
        if self.frame_time_us == 0
            || self.frame_time_us > MAX_FRAME_TIME_US
            || self.gpu_time_us > MAX_GPU_TIME_US
            || self.gpu_time_us > self.frame_time_us
            || self.draw_calls > MAX_DRAW_CALLS
            || self.submitted_instances > MAX_INSTANCES
            || self.visible_instances > self.submitted_instances
            || self.dropped_frames > MAX_DROPPED_FRAMES
        {
            return Err(ContractDiagnostic::new(
                DiagnosticCode::InvalidTelemetry,
                "live-frame telemetry is outside declared bounds or internally inconsistent",
            ));
        }
        Ok(())
    }
}

/// Tier B frame attestation. It binds the active packet and bounded counters;
/// it carries no certification status and no rendered-image digest.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LiveFrameAttestation {
    pub schema_version: String,
    pub project_id: String,
    pub session_id: String,
    pub packet: PacketIdentity,
    pub frame_sequence: u64,
    pub observed_at_ms: u64,
    pub telemetry: LiveFrameTelemetry,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "phase",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SessionState {
    Live,
    SamplePending { request: TierASnapshotRequest },
    Certified { snapshot: CertifiedSnapshotIdentity },
    Indeterminate { diagnostic: ContractDiagnostic },
    Demoted { diagnostic: ContractDiagnostic },
}

/// State holder for a single live presentation session.
///
/// Construction and transitions are typed; deserializing this struct is not
/// exposed, so saved wire state cannot be restored as trusted in-memory state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveEvidenceSession {
    project_id: String,
    session_id: String,
    packet: PacketIdentity,
    policy: SamplingPolicy,
    created_at_ms: u64,
    last_observed_at_ms: u64,
    last_sample_completed_at_ms: u64,
    last_frame_sequence: Option<u64>,
    last_event_sequence: u64,
    state: SessionState,
}

#[derive(Serialize)]
struct SessionCanonicalView<'a> {
    schema_version: &'static str,
    project_id: &'a str,
    session_id: &'a str,
    packet: &'a PacketIdentity,
    policy: SamplingPolicy,
    created_at_ms: u64,
    last_observed_at_ms: u64,
    last_sample_completed_at_ms: u64,
    last_frame_sequence: Option<u64>,
    last_event_sequence: u64,
    state: &'a SessionState,
}

impl LiveEvidenceSession {
    pub fn new(
        project_id: impl Into<String>,
        session_id: impl Into<String>,
        packet: PacketIdentity,
        policy: SamplingPolicy,
        started_at_ms: u64,
    ) -> ContractResult<Self> {
        let project_id = project_id.into();
        let session_id = session_id.into();
        validate_identifier(&project_id, "project")?;
        validate_identifier(&session_id, "session")?;
        packet.validate()?;
        policy.validate()?;
        started_at_ms
            .checked_add(policy.interval_ms)
            .ok_or_else(|| {
                ContractDiagnostic::new(
                    DiagnosticCode::InvalidIdentity,
                    "session start time overflows the sample schedule",
                )
            })?;
        Ok(Self {
            project_id,
            session_id,
            packet,
            policy,
            created_at_ms: started_at_ms,
            last_observed_at_ms: started_at_ms,
            last_sample_completed_at_ms: started_at_ms,
            last_frame_sequence: None,
            last_event_sequence: 0,
            state: SessionState::Live,
        })
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn packet(&self) -> &PacketIdentity {
        &self.packet
    }

    pub fn pending_request(&self) -> Option<&TierASnapshotRequest> {
        match &self.state {
            SessionState::SamplePending { request } => Some(request),
            _ => None,
        }
    }

    /// Advance monotonic session time, expire overdue samples, or request the
    /// next periodic/recovery snapshot once its interval elapses.
    pub fn tick(&mut self, now_ms: u64) -> ContractResult<Option<TierASnapshotRequest>> {
        self.advance_clock(now_ms)?;
        match &self.state {
            SessionState::Demoted { .. } => {
                return self.invalid_transition("cannot tick a demoted session");
            }
            SessionState::SamplePending { request } => {
                if now_ms > request.expires_at_ms {
                    let diagnostic = ContractDiagnostic::new(
                        DiagnosticCode::SampleWindowExpired,
                        "Tier A snapshot did not complete before its sample window expired",
                    );
                    self.set_indeterminate(diagnostic.clone(), now_ms);
                    return Err(diagnostic);
                }
                return Ok(None);
            }
            SessionState::Live
            | SessionState::Certified { .. }
            | SessionState::Indeterminate { .. } => {}
        }

        let Some(due_at) = self
            .last_sample_completed_at_ms
            .checked_add(self.policy.interval_ms)
        else {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::InvalidIdentity,
                "next sample time overflows the monotonic clock",
            );
            self.set_indeterminate(diagnostic.clone(), now_ms);
            return Err(diagnostic);
        };
        if now_ms < due_at {
            return Ok(None);
        }

        let trigger = if matches!(&self.state, SessionState::Indeterminate { .. }) {
            vec![SampleTrigger::Periodic, SampleTrigger::Recovery]
        } else {
            vec![SampleTrigger::Periodic]
        };
        self.start_sample(now_ms, trigger).map(Some)
    }

    /// Semantic changes request a sample immediately. A pending window is
    /// retained while its request is rebound to the newest packet identity.
    pub fn observe_event(&mut self, event: SemanticEvent) -> ContractResult<TierASnapshotRequest> {
        if matches!(&self.state, SessionState::Demoted { .. }) {
            return self.invalid_transition("cannot schedule a sample for a demoted session");
        }
        self.advance_clock(event.occurred_at_ms)?;
        if let Err(diagnostic) = event.packet.validate() {
            self.set_demoted(diagnostic.clone());
            return Err(diagnostic);
        }
        let expected_sequence = self.last_event_sequence.checked_add(1).ok_or_else(|| {
            ContractDiagnostic::new(
                DiagnosticCode::InvalidIdentity,
                "semantic event sequence overflow",
            )
        })?;
        if event.sequence != expected_sequence {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::StaleOrMismatchedPacket,
                format!(
                    "semantic event sequence {} does not follow {}",
                    event.sequence, self.last_event_sequence
                ),
            );
            self.set_indeterminate(diagnostic.clone(), event.occurred_at_ms);
            return Err(diagnostic);
        }
        if let SessionState::SamplePending { request } = &self.state
            && event.occurred_at_ms > request.expires_at_ms
        {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::SampleWindowExpired,
                "semantic event arrived after the pending sample window expired",
            );
            self.set_indeterminate(diagnostic.clone(), event.occurred_at_ms);
            return Err(diagnostic);
        }

        self.last_event_sequence = event.sequence;
        self.packet = event.packet;
        let trigger = SampleTrigger::from(event.kind);
        if let SessionState::SamplePending { request } = &self.state {
            let mut triggers = request.triggers.clone();
            triggers.push(trigger);
            triggers.sort_unstable();
            triggers.dedup();
            let rebound = self.make_request(
                request.created_at_ms,
                request.expires_at_ms,
                triggers,
                self.packet.clone(),
            )?;
            self.state = SessionState::SamplePending {
                request: rebound.clone(),
            };
            return Ok(rebound);
        }
        self.start_sample(event.occurred_at_ms, vec![trigger])
    }

    /// Record a Tier B attestation. Invalid counters demote; an old packet or
    /// regressing frame moves the session to indeterminate.
    pub fn record_live_frame(&mut self, frame: LiveFrameAttestation) -> ContractResult<()> {
        if matches!(&self.state, SessionState::Demoted { .. }) {
            return self.invalid_transition("cannot accept a live frame for a demoted session");
        }
        if frame.schema_version != LIVE_FRAME_SCHEMA
            || frame.project_id != self.project_id
            || frame.session_id != self.session_id
        {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::StaleOrMismatchedPacket,
                "live-frame session or schema identity does not match the active session",
            );
            self.set_indeterminate(
                diagnostic.clone(),
                frame.observed_at_ms.max(self.last_observed_at_ms),
            );
            return Err(diagnostic);
        }
        if let Err(diagnostic) = frame.packet.validate() {
            self.set_demoted(diagnostic.clone());
            return Err(diagnostic);
        }
        if frame.packet != self.packet {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::StaleOrMismatchedPacket,
                "live-frame packet or capabilities do not match the active session identity",
            );
            self.set_indeterminate(
                diagnostic.clone(),
                frame.observed_at_ms.max(self.last_observed_at_ms),
            );
            return Err(diagnostic);
        }
        if let Err(diagnostic) = frame.telemetry.validate() {
            self.set_demoted(diagnostic.clone());
            return Err(diagnostic);
        }
        if self
            .last_frame_sequence
            .is_some_and(|last| frame.frame_sequence <= last)
        {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::StaleOrMismatchedPacket,
                "live-frame sequence is stale or duplicated",
            );
            self.set_indeterminate(
                diagnostic.clone(),
                frame.observed_at_ms.max(self.last_observed_at_ms),
            );
            return Err(diagnostic);
        }
        self.advance_clock(frame.observed_at_ms)?;
        self.last_frame_sequence = Some(frame.frame_sequence);
        Ok(())
    }

    /// Consume one Tier A result. `None`, stale identity, or expiry never
    /// preserves a certified session state.
    pub fn complete_snapshot(
        &mut self,
        result: Option<TierAResult>,
        now_ms: u64,
    ) -> ContractResult<()> {
        self.advance_clock(now_ms)?;
        let request = match &self.state {
            SessionState::SamplePending { request } => request.clone(),
            _ => return self.invalid_transition("snapshot result requires sample_pending state"),
        };
        if now_ms > request.expires_at_ms {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::SampleWindowExpired,
                "Tier A result arrived after its sample window expired",
            );
            self.set_indeterminate(diagnostic.clone(), now_ms);
            return Err(diagnostic);
        }
        let Some(result) = result else {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::MissingSnapshot,
                "sample window completed without a Tier A snapshot result",
            );
            self.set_indeterminate(diagnostic.clone(), now_ms);
            return Err(diagnostic);
        };
        let (schema, request_id, project_id, session_id, packet, completed_at_ms) =
            result.identity();
        if schema != SNAPSHOT_RESULT_SCHEMA
            || request_id != request.request_id
            || project_id != self.project_id
            || session_id != self.session_id
            || packet != &request.packet
            || packet != &self.packet
        {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::StaleOrMismatchedPacket,
                "Tier A result does not bind to the active request, session, project, and packet",
            );
            self.set_indeterminate(diagnostic.clone(), now_ms);
            return Err(diagnostic);
        }
        if completed_at_ms < request.created_at_ms || completed_at_ms > now_ms {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::StaleOrMismatchedPacket,
                "Tier A result completion time is outside the request timeline",
            );
            self.set_indeterminate(diagnostic.clone(), now_ms);
            return Err(diagnostic);
        }
        if completed_at_ms > request.expires_at_ms {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::SampleWindowExpired,
                "Tier A result completed after the sample window expired",
            );
            self.set_indeterminate(diagnostic.clone(), now_ms);
            return Err(diagnostic);
        }

        match result {
            TierAResult::Certified { snapshot, .. } => {
                if let Err(diagnostic) = snapshot.validate(&self.project_id) {
                    self.set_indeterminate(diagnostic.clone(), now_ms);
                    return Err(diagnostic);
                }
                self.last_sample_completed_at_ms = now_ms;
                self.state = SessionState::Certified { snapshot };
                Ok(())
            }
            TierAResult::Rejected { failure, .. } => {
                let (code, detail) = if failure == SnapshotFailure::InjectedVisualCorruption {
                    (
                        DiagnosticCode::InjectedFailure,
                        "Tier A rejected the sample after injected visual corruption",
                    )
                } else {
                    (
                        DiagnosticCode::SnapshotRejected,
                        "Tier A rejected the live-session sample",
                    )
                };
                let diagnostic = ContractDiagnostic::new(code, detail);
                self.last_sample_completed_at_ms = now_ms;
                self.set_demoted(diagnostic);
                Ok(())
            }
            TierAResult::Indeterminate { reason, .. } => {
                let diagnostic = ContractDiagnostic::new(
                    reason,
                    "Tier A could not establish a determinate snapshot result",
                );
                self.set_indeterminate(diagnostic.clone(), now_ms);
                Err(diagnostic)
            }
        }
    }

    /// Decode and consume a wire result. A pass-shaped status without the
    /// explicit bound outcome is treated as attempted evidence forgery.
    pub fn complete_snapshot_json(&mut self, bytes: &[u8], now_ms: u64) -> ContractResult<()> {
        let result = match decode_tier_a_result(bytes) {
            Ok(result) => result,
            Err(diagnostic) => {
                if diagnostic.code == DiagnosticCode::ForgedPassStatus {
                    self.set_demoted_at(diagnostic.clone(), now_ms);
                } else {
                    self.set_indeterminate(
                        diagnostic.clone(),
                        now_ms.max(self.last_observed_at_ms),
                    );
                }
                return Err(diagnostic);
            }
        };
        self.complete_snapshot(Some(result), now_ms)
    }

    /// Explicit timer callback for integrations that do not drive [`tick`].
    /// Calling it outside an expired pending window is an invalid transition.
    pub fn expire_sample_window(&mut self, now_ms: u64) -> ContractResult<()> {
        self.advance_clock(now_ms)?;
        let expires_at_ms = match &self.state {
            SessionState::SamplePending { request } => request.expires_at_ms,
            _ => {
                return self
                    .invalid_transition("sample-window expiry requires sample_pending state");
            }
        };
        if now_ms <= expires_at_ms {
            return self.invalid_transition("cannot expire a sample before its deadline");
        }
        let diagnostic = ContractDiagnostic::new(
            DiagnosticCode::SampleWindowExpired,
            "Tier A sample window expired without a valid result",
        );
        self.set_indeterminate(diagnostic.clone(), now_ms);
        Err(diagnostic)
    }

    pub fn canonical_bytes(&self) -> ContractResult<Vec<u8>> {
        let view = SessionCanonicalView {
            schema_version: SESSION_SCHEMA,
            project_id: &self.project_id,
            session_id: &self.session_id,
            packet: &self.packet,
            policy: self.policy,
            created_at_ms: self.created_at_ms,
            last_observed_at_ms: self.last_observed_at_ms,
            last_sample_completed_at_ms: self.last_sample_completed_at_ms,
            last_frame_sequence: self.last_frame_sequence,
            last_event_sequence: self.last_event_sequence,
            state: &self.state,
        };
        canonical_bytes(&view)
    }

    pub fn canonical_json(&self) -> ContractResult<String> {
        String::from_utf8(self.canonical_bytes()?).map_err(|error| {
            ContractDiagnostic::new(
                DiagnosticCode::MalformedRepresentation,
                format!("canonical JSON was not UTF-8: {error}"),
            )
        })
    }

    pub fn digest(&self) -> ContractResult<String> {
        Ok(sha256_prefixed(&self.canonical_bytes()?))
    }

    fn start_sample(
        &mut self,
        now_ms: u64,
        triggers: Vec<SampleTrigger>,
    ) -> ContractResult<TierASnapshotRequest> {
        if matches!(&self.state, SessionState::Demoted { .. }) {
            return self.invalid_transition("cannot request a sample for a demoted session");
        }
        let Some(expires_at_ms) = now_ms.checked_add(self.policy.sample_window_ms) else {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::InvalidIdentity,
                "sample deadline overflows the monotonic clock",
            );
            self.set_indeterminate(diagnostic.clone(), now_ms);
            return Err(diagnostic);
        };
        let request = self.make_request(now_ms, expires_at_ms, triggers, self.packet.clone())?;
        self.state = SessionState::SamplePending {
            request: request.clone(),
        };
        Ok(request)
    }

    fn make_request(
        &self,
        created_at_ms: u64,
        expires_at_ms: u64,
        mut triggers: Vec<SampleTrigger>,
        packet: PacketIdentity,
    ) -> ContractResult<TierASnapshotRequest> {
        packet.validate()?;
        triggers.sort_unstable();
        triggers.dedup();
        if triggers.is_empty() {
            return Err(ContractDiagnostic::new(
                DiagnosticCode::InvalidIdentity,
                "snapshot request must have at least one trigger",
            ));
        }
        #[derive(Serialize)]
        struct RequestSeed<'a> {
            schema_version: &'static str,
            project_id: &'a str,
            session_id: &'a str,
            packet: &'a PacketIdentity,
            created_at_ms: u64,
            expires_at_ms: u64,
            triggers: &'a [SampleTrigger],
        }
        let seed = RequestSeed {
            schema_version: SNAPSHOT_REQUEST_SCHEMA,
            project_id: &self.project_id,
            session_id: &self.session_id,
            packet: &packet,
            created_at_ms,
            expires_at_ms,
            triggers: &triggers,
        };
        let request_id = sha256_prefixed(&canonical_bytes(&seed)?);
        Ok(TierASnapshotRequest {
            schema_version: SNAPSHOT_REQUEST_SCHEMA.to_owned(),
            request_id,
            project_id: self.project_id.clone(),
            session_id: self.session_id.clone(),
            packet,
            created_at_ms,
            expires_at_ms,
            triggers,
        })
    }

    fn advance_clock(&mut self, now_ms: u64) -> ContractResult<()> {
        if now_ms < self.last_observed_at_ms {
            let diagnostic = ContractDiagnostic::new(
                DiagnosticCode::ClockRegression,
                "monotonic session time moved backwards",
            );
            self.set_indeterminate(diagnostic.clone(), self.last_observed_at_ms);
            return Err(diagnostic);
        }
        self.last_observed_at_ms = now_ms;
        Ok(())
    }

    fn invalid_transition<T>(&mut self, detail: &str) -> ContractResult<T> {
        let diagnostic = ContractDiagnostic::new(DiagnosticCode::InvalidTransition, detail);
        self.set_demoted(diagnostic.clone());
        Err(diagnostic)
    }

    fn set_indeterminate(&mut self, diagnostic: ContractDiagnostic, now_ms: u64) {
        if !matches!(&self.state, SessionState::Demoted { .. }) {
            self.last_observed_at_ms = self.last_observed_at_ms.max(now_ms);
            self.last_sample_completed_at_ms = now_ms;
            self.state = SessionState::Indeterminate { diagnostic };
        }
    }

    fn set_demoted(&mut self, diagnostic: ContractDiagnostic) {
        self.state = SessionState::Demoted { diagnostic };
    }

    fn set_demoted_at(&mut self, diagnostic: ContractDiagnostic, now_ms: u64) {
        self.last_observed_at_ms = self.last_observed_at_ms.max(now_ms);
        self.set_demoted(diagnostic);
    }
}

impl TierAResult {
    fn identity(&self) -> (&str, &str, &str, &str, &PacketIdentity, u64) {
        match self {
            Self::Certified {
                schema_version,
                request_id,
                project_id,
                session_id,
                packet,
                completed_at_ms,
                ..
            }
            | Self::Rejected {
                schema_version,
                request_id,
                project_id,
                session_id,
                packet,
                completed_at_ms,
                ..
            }
            | Self::Indeterminate {
                schema_version,
                request_id,
                project_id,
                session_id,
                packet,
                completed_at_ms,
                ..
            } => (
                schema_version,
                request_id,
                project_id,
                session_id,
                packet,
                *completed_at_ms,
            ),
        }
    }
}

pub fn decode_tier_a_result(bytes: &[u8]) -> ContractResult<TierAResult> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
        ContractDiagnostic::new(
            DiagnosticCode::MalformedRepresentation,
            format!("Tier A result is not valid JSON: {error}"),
        )
    })?;
    if contains_pass_shaped_status(&value) {
        return Err(ContractDiagnostic::new(
            DiagnosticCode::ForgedPassStatus,
            "status=pass cannot substitute for a packet-bound Tier A outcome",
        ));
    }
    serde_json::from_value(value).map_err(|error| {
        ContractDiagnostic::new(
            DiagnosticCode::MalformedRepresentation,
            format!("Tier A result does not match the closed schema: {error}"),
        )
    })
}

/// Compact serde JSON with fixed struct field order. Contract wire types avoid
/// unordered maps and normalize set-like vectors before serialization.
pub fn canonical_bytes<T: Serialize>(value: &T) -> ContractResult<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| {
        ContractDiagnostic::new(
            DiagnosticCode::MalformedRepresentation,
            format!("cannot encode canonical contract JSON: {error}"),
        )
    })
}

fn contains_pass_shaped_status(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            (key == "status"
                && value.as_str().is_some_and(|status| {
                    status.eq_ignore_ascii_case("pass") || status.eq_ignore_ascii_case("certified")
                }))
                || contains_pass_shaped_status(value)
        }),
        serde_json::Value::Array(items) => items.iter().any(contains_pass_shaped_status),
        _ => false,
    }
}

fn validate_identifier(value: &str, label: &str) -> ContractResult<()> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(ContractDiagnostic::new(
            DiagnosticCode::InvalidIdentity,
            format!("{label} identifier is empty, too long, or contains control characters"),
        ));
    }
    Ok(())
}

fn validate_digest(value: &str, label: &str) -> ContractResult<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(ContractDiagnostic::new(
            DiagnosticCode::InvalidIdentity,
            format!("{label} digest must use canonical sha256:<64 lowercase hex> form"),
        ));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ContractDiagnostic::new(
            DiagnosticCode::InvalidIdentity,
            format!("{label} digest must use canonical sha256:<64 lowercase hex> form"),
        ));
    }
    Ok(())
}

fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    const ONE: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const TWO: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

    fn packet(packet: &str, capabilities: &str) -> PacketIdentity {
        PacketIdentity {
            packet_sha256: packet.to_owned(),
            capabilities_sha256: capabilities.to_owned(),
        }
    }

    fn session() -> LiveEvidenceSession {
        LiveEvidenceSession::new(
            "riverwatch",
            "session-1",
            packet(ZERO, ONE),
            SamplingPolicy {
                interval_ms: 100,
                sample_window_ms: 25,
            },
            0,
        )
        .unwrap()
    }

    fn snapshot_result(request: &TierASnapshotRequest, completed_at_ms: u64) -> TierAResult {
        TierAResult::Certified {
            schema_version: SNAPSHOT_RESULT_SCHEMA.to_owned(),
            request_id: request.request_id.clone(),
            project_id: request.project_id.clone(),
            session_id: request.session_id.clone(),
            packet: request.packet.clone(),
            completed_at_ms,
            snapshot: CertifiedSnapshotIdentity {
                project_id: request.project_id.clone(),
                snapshot_id: "snapshot-1".to_owned(),
                candidate_sha256: TWO.to_owned(),
                receipt_id: "receipt-1".to_owned(),
                receipt_sha256: ONE.to_owned(),
                validator_registry_sha256: ZERO.to_owned(),
                validator_id: "wge.validator.visual-reference/v1".to_owned(),
                receipt_schema: "wge.visual-receipt/v1".to_owned(),
            },
        }
    }

    fn rejected_result(
        request: &TierASnapshotRequest,
        completed_at_ms: u64,
        failure: SnapshotFailure,
    ) -> TierAResult {
        TierAResult::Rejected {
            schema_version: SNAPSHOT_RESULT_SCHEMA.to_owned(),
            request_id: request.request_id.clone(),
            project_id: request.project_id.clone(),
            session_id: request.session_id.clone(),
            packet: request.packet.clone(),
            completed_at_ms,
            failure,
        }
    }

    fn start_due_sample(session: &mut LiveEvidenceSession) -> TierASnapshotRequest {
        session.tick(100).unwrap().unwrap()
    }

    fn sample_pending() -> LiveEvidenceSession {
        let mut value = session();
        start_due_sample(&mut value);
        value
    }

    fn sample_in_state(state: SessionState) -> LiveEvidenceSession {
        let mut value = session();
        value.state = state;
        value
    }

    #[test]
    fn valid_periodic_lifecycle_binds_tier_a_and_keeps_tier_b_status_free() {
        let mut value = session();
        let frame = LiveFrameAttestation {
            schema_version: LIVE_FRAME_SCHEMA.to_owned(),
            project_id: "riverwatch".to_owned(),
            session_id: "session-1".to_owned(),
            packet: packet(ZERO, ONE),
            frame_sequence: 1,
            observed_at_ms: 50,
            telemetry: LiveFrameTelemetry {
                frame_time_us: 16_000,
                gpu_time_us: 4_000,
                draw_calls: 12,
                submitted_instances: 80,
                visible_instances: 64,
                dropped_frames: 0,
            },
        };
        value.record_live_frame(frame.clone()).unwrap();
        let frame_json = String::from_utf8(canonical_bytes(&frame).unwrap()).unwrap();
        assert!(!frame_json.contains("status"));
        assert!(frame_json.contains("packet_sha256"));
        assert!(!frame_json.contains("image_sha256"));
        assert!(value.tick(99).unwrap().is_none());

        let request = value.tick(100).unwrap().unwrap();
        assert!(matches!(value.state(), SessionState::SamplePending { .. }));
        assert_eq!(request.triggers, vec![SampleTrigger::Periodic]);
        value
            .complete_snapshot(Some(snapshot_result(&request, 110)), 110)
            .unwrap();
        assert!(matches!(value.state(), SessionState::Certified { .. }));

        let mut next_frame = frame;
        next_frame.frame_sequence = 2;
        next_frame.observed_at_ms = 120;
        value.record_live_frame(next_frame).unwrap();
        assert!(value.tick(209).unwrap().is_none());
        assert!(value.tick(210).unwrap().is_some());
    }

    #[test]
    fn each_semantic_event_triggers_and_binds_a_sample() {
        for (sequence, kind, next_packet, next_caps, trigger) in [
            (
                1,
                SemanticEventKind::SceneChanged,
                ONE,
                ONE,
                SampleTrigger::SceneChanged,
            ),
            (
                1,
                SemanticEventKind::CameraCut,
                TWO,
                ONE,
                SampleTrigger::CameraCut,
            ),
            (
                1,
                SemanticEventKind::CapabilityChanged,
                ZERO,
                TWO,
                SampleTrigger::CapabilityChanged,
            ),
        ] {
            let mut value = session();
            let request = value
                .observe_event(SemanticEvent {
                    sequence,
                    occurred_at_ms: 10,
                    kind,
                    packet: packet(next_packet, next_caps),
                })
                .unwrap();
            assert_eq!(request.triggers, vec![trigger]);
            assert_eq!(request.packet, *value.packet());
            assert!(matches!(value.state(), SessionState::SamplePending { .. }));
        }
    }

    #[test]
    fn pending_events_rebind_packet_without_extending_the_original_window() {
        let mut value = sample_pending();
        let original = value.pending_request().unwrap().clone();
        let rebound = value
            .observe_event(SemanticEvent {
                sequence: 1,
                occurred_at_ms: 110,
                kind: SemanticEventKind::CameraCut,
                packet: packet(TWO, ONE),
            })
            .unwrap();
        assert_ne!(original.request_id, rebound.request_id);
        assert_eq!(original.expires_at_ms, rebound.expires_at_ms);
        assert_eq!(
            rebound.triggers,
            vec![SampleTrigger::Periodic, SampleTrigger::CameraCut]
        );
        let error = value
            .complete_snapshot(Some(snapshot_result(&original, 115)), 115)
            .unwrap_err();
        assert_eq!(error.code, DiagnosticCode::StaleOrMismatchedPacket);
        assert!(matches!(value.state(), SessionState::Indeterminate { .. }));
    }

    #[test]
    fn sample_window_expiry_and_missing_snapshot_fail_closed() {
        let mut expired = sample_pending();
        let error = expired.tick(126).unwrap_err();
        assert_eq!(error.code, DiagnosticCode::SampleWindowExpired);
        assert!(matches!(
            expired.state(),
            SessionState::Indeterminate { .. }
        ));

        let mut missing = sample_pending();
        let error = missing.complete_snapshot(None, 110).unwrap_err();
        assert_eq!(error.code, DiagnosticCode::MissingSnapshot);
        assert!(matches!(
            missing.state(),
            SessionState::Indeterminate { .. }
        ));
    }

    #[test]
    fn mismatched_and_stale_snapshot_results_become_indeterminate() {
        let mut value = sample_pending();
        let request = value.pending_request().unwrap().clone();
        let mut stale = snapshot_result(&request, 110);
        if let TierAResult::Certified {
            packet: result_packet,
            ..
        } = &mut stale
        {
            *result_packet = packet(TWO, ONE);
        }
        let error = value.complete_snapshot(Some(stale), 110).unwrap_err();
        assert_eq!(error.code, DiagnosticCode::StaleOrMismatchedPacket);
        assert!(matches!(value.state(), SessionState::Indeterminate { .. }));
    }

    #[test]
    fn pass_shaped_status_is_diagnosed_and_demotes() {
        let mut value = sample_pending();
        let error = value
            .complete_snapshot_json(br#"{"status":"pass","snapshot_id":"snapshot-1"}"#, 110)
            .unwrap_err();
        assert_eq!(error.code, DiagnosticCode::ForgedPassStatus);
        assert!(matches!(value.state(), SessionState::Demoted { .. }));
    }

    #[test]
    fn injected_visual_failure_demotes_with_specific_diagnostic() {
        let mut value = sample_pending();
        let request = value.pending_request().unwrap().clone();
        value
            .complete_snapshot(
                Some(rejected_result(
                    &request,
                    110,
                    SnapshotFailure::InjectedVisualCorruption,
                )),
                110,
            )
            .unwrap();
        match value.state() {
            SessionState::Demoted { diagnostic } => {
                assert_eq!(diagnostic.code, DiagnosticCode::InjectedFailure)
            }
            other => panic!("expected demoted, got {other:?}"),
        }
    }

    #[test]
    fn all_illegal_snapshot_and_expiry_transitions_demote() {
        let valid_snapshot = CertifiedSnapshotIdentity {
            project_id: "riverwatch".to_owned(),
            snapshot_id: "snapshot-1".to_owned(),
            candidate_sha256: TWO.to_owned(),
            receipt_id: "receipt-1".to_owned(),
            receipt_sha256: ONE.to_owned(),
            validator_registry_sha256: ZERO.to_owned(),
            validator_id: "wge.validator.visual-reference/v1".to_owned(),
            receipt_schema: "wge.visual-receipt/v1".to_owned(),
        };
        let states = [
            SessionState::Live,
            SessionState::Certified {
                snapshot: valid_snapshot,
            },
            SessionState::Indeterminate {
                diagnostic: ContractDiagnostic::new(
                    DiagnosticCode::MissingSnapshot,
                    "test indeterminate state",
                ),
            },
            SessionState::Demoted {
                diagnostic: ContractDiagnostic::new(
                    DiagnosticCode::InjectedFailure,
                    "test demoted state",
                ),
            },
        ];
        for state in states {
            let mut completion = sample_in_state(state.clone());
            let error = completion
                .complete_snapshot(
                    Some(TierAResult::Indeterminate {
                        schema_version: SNAPSHOT_RESULT_SCHEMA.to_owned(),
                        request_id: "unbound".to_owned(),
                        project_id: "riverwatch".to_owned(),
                        session_id: "session-1".to_owned(),
                        packet: packet(ZERO, ONE),
                        completed_at_ms: 10,
                        reason: DiagnosticCode::MissingSnapshot,
                    }),
                    10,
                )
                .unwrap_err();
            assert_eq!(error.code, DiagnosticCode::InvalidTransition);
            assert!(matches!(completion.state(), SessionState::Demoted { .. }));

            let mut expiry = sample_in_state(state);
            let error = expiry.expire_sample_window(10).unwrap_err();
            assert_eq!(error.code, DiagnosticCode::InvalidTransition);
            assert!(matches!(expiry.state(), SessionState::Demoted { .. }));
        }

        let mut demoted = sample_in_state(SessionState::Demoted {
            diagnostic: ContractDiagnostic::new(
                DiagnosticCode::InjectedFailure,
                "test terminal state",
            ),
        });
        let error = demoted.tick(10).unwrap_err();
        assert_eq!(error.code, DiagnosticCode::InvalidTransition);
        let error = demoted
            .observe_event(SemanticEvent {
                sequence: 1,
                occurred_at_ms: 11,
                kind: SemanticEventKind::SceneChanged,
                packet: packet(ONE, ONE),
            })
            .unwrap_err();
        assert_eq!(error.code, DiagnosticCode::InvalidTransition);
    }

    #[test]
    fn bad_telemetry_and_stale_live_packet_are_not_accepted() {
        let mut bad = session();
        let error = bad
            .record_live_frame(LiveFrameAttestation {
                schema_version: LIVE_FRAME_SCHEMA.to_owned(),
                project_id: "riverwatch".to_owned(),
                session_id: "session-1".to_owned(),
                packet: packet(ZERO, ONE),
                frame_sequence: 1,
                observed_at_ms: 1,
                telemetry: LiveFrameTelemetry {
                    frame_time_us: 10,
                    gpu_time_us: 11,
                    draw_calls: 1,
                    submitted_instances: 1,
                    visible_instances: 1,
                    dropped_frames: 0,
                },
            })
            .unwrap_err();
        assert_eq!(error.code, DiagnosticCode::InvalidTelemetry);
        assert!(matches!(bad.state(), SessionState::Demoted { .. }));

        let mut stale = session();
        let error = stale
            .record_live_frame(LiveFrameAttestation {
                schema_version: LIVE_FRAME_SCHEMA.to_owned(),
                project_id: "riverwatch".to_owned(),
                session_id: "session-1".to_owned(),
                packet: packet(TWO, ONE),
                frame_sequence: 1,
                observed_at_ms: 1,
                telemetry: LiveFrameTelemetry {
                    frame_time_us: 10,
                    gpu_time_us: 5,
                    draw_calls: 1,
                    submitted_instances: 1,
                    visible_instances: 1,
                    dropped_frames: 0,
                },
            })
            .unwrap_err();
        assert_eq!(error.code, DiagnosticCode::StaleOrMismatchedPacket);
        assert!(matches!(stale.state(), SessionState::Indeterminate { .. }));
    }

    #[test]
    fn canonical_serialization_and_digest_are_repeatable() {
        let mut left = session();
        let mut right = session();
        let left_request = left.observe_event(SemanticEvent {
            sequence: 1,
            occurred_at_ms: 5,
            kind: SemanticEventKind::SceneChanged,
            packet: packet(ONE, ONE),
        });
        let right_request = right.observe_event(SemanticEvent {
            sequence: 1,
            occurred_at_ms: 5,
            kind: SemanticEventKind::SceneChanged,
            packet: packet(ONE, ONE),
        });
        assert_eq!(left_request.unwrap(), right_request.unwrap());
        assert_eq!(
            left.canonical_bytes().unwrap(),
            right.canonical_bytes().unwrap()
        );
        assert_eq!(
            left.canonical_json().unwrap(),
            right.canonical_json().unwrap()
        );
        assert_eq!(left.digest().unwrap(), right.digest().unwrap());
    }

    #[test]
    fn monotonic_clock_regression_fails_closed() {
        let mut value = session();
        value.tick(5).unwrap();
        let error = value.tick(4).unwrap_err();
        assert_eq!(error.code, DiagnosticCode::ClockRegression);
        assert!(matches!(value.state(), SessionState::Indeterminate { .. }));
    }

    #[test]
    fn malformed_semantic_identity_and_schedule_overflow_fail_closed() {
        let mut malformed_event = session();
        let error = malformed_event
            .observe_event(SemanticEvent {
                sequence: 1,
                occurred_at_ms: 1,
                kind: SemanticEventKind::SceneChanged,
                packet: packet("not-a-digest", ONE),
            })
            .unwrap_err();
        assert_eq!(error.code, DiagnosticCode::InvalidIdentity);
        assert!(matches!(
            malformed_event.state(),
            SessionState::Demoted { .. }
        ));

        let mut overflow = LiveEvidenceSession::new(
            "riverwatch",
            "session-overflow",
            packet(ZERO, ONE),
            SamplingPolicy {
                interval_ms: 1,
                sample_window_ms: 1,
            },
            u64::MAX - 1,
        )
        .unwrap();
        let error = overflow.tick(u64::MAX).unwrap_err();
        assert_eq!(error.code, DiagnosticCode::InvalidIdentity);
        assert!(matches!(
            overflow.state(),
            SessionState::Indeterminate { .. }
        ));
    }
}
