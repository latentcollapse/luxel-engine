//! Live evidence adapter for the current supervised offscreen graphics path.
//!
//! The current backend produces promoted offscreen captures on demand. It does
//! not own a window, swapchain, or presentation loop. Tier B records therefore
//! attest to promoted capture telemetry, not real-time presentation behavior.

use wge_live_evidence_contract::{
    ContractDiagnostic, DiagnosticCode, LIVE_FRAME_SCHEMA, LiveEvidenceSession,
    LiveFrameAttestation, LiveFrameTelemetry, PacketIdentity, SamplingPolicy, SemanticEvent,
    SemanticEventKind, SessionState, TierAResult, TierASnapshotRequest,
};
use wge_reference_runtime::WorldArtifact;

use crate::{
    CaptureFormat, FrameStatus, GraphicsContractError, GraphicsReady, GraphicsScenePacket,
    GraphicsTelemetry, GraphicsWorkerError, GraphicsWorkerSupervisor, PromotedFrame,
    canonical_json, sha256_prefixed, validate_frame_receipt, validate_scene_packet,
};

/// Execution mode currently implemented by [`LiveGraphicsSession`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsSessionMode {
    OffscreenCapture,
}

/// A request for real-time presentation is explicitly unsupported by the
/// current worker. No offscreen capture is substituted for a presented frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LivePresentOutcome {
    Unsupported { reason: &'static str },
}

/// A frame independently promoted by the Rust supervisor and recorded as a
/// bounded Tier B attestation in the live evidence contract.
#[derive(Debug)]
pub struct LiveCaptureOutput {
    pub promoted: PromotedFrame,
    pub attestation: LiveFrameAttestation,
}

#[derive(Debug)]
pub enum LiveGraphicsError {
    Worker(GraphicsWorkerError),
    GraphicsContract(GraphicsContractError),
    EvidenceContract(ContractDiagnostic),
    Telemetry(String),
    Identity(String),
}

impl std::fmt::Display for LiveGraphicsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Worker(error) => write!(formatter, "{error}"),
            Self::GraphicsContract(error) => write!(formatter, "{error}"),
            Self::EvidenceContract(error) => write!(formatter, "{error}"),
            Self::Telemetry(detail) => {
                write!(formatter, "invalid promoted-frame telemetry: {detail}")
            }
            Self::Identity(detail) => write!(formatter, "live graphics identity failure: {detail}"),
        }
    }
}

impl std::error::Error for LiveGraphicsError {}

impl From<GraphicsWorkerError> for LiveGraphicsError {
    fn from(error: GraphicsWorkerError) -> Self {
        Self::Worker(error)
    }
}

impl From<GraphicsContractError> for LiveGraphicsError {
    fn from(error: GraphicsContractError) -> Self {
        Self::GraphicsContract(error)
    }
}

impl From<ContractDiagnostic> for LiveGraphicsError {
    fn from(error: ContractDiagnostic) -> Self {
        Self::EvidenceContract(error)
    }
}

/// Binds one project/session to an exact scene packet and validated graphics
/// capability set. All timestamps and frame sequence numbers are supplied by
/// the caller; this adapter never reads a wall clock.
pub struct LiveGraphicsSession {
    evidence: LiveEvidenceSession,
    project_id: String,
    session_id: String,
    active_packet: GraphicsScenePacket,
    active_capabilities: GraphicsReady,
    capabilities_sha256: String,
    last_event_sequence: u64,
}

impl LiveGraphicsSession {
    /// Start an offscreen-capture evidence session against the packet and the
    /// worker's typed, validated capability response.
    pub fn new(
        supervisor: &mut GraphicsWorkerSupervisor,
        project_id: impl Into<String>,
        session_id: impl Into<String>,
        packet: GraphicsScenePacket,
        policy: SamplingPolicy,
        started_at_ms: u64,
    ) -> Result<Self, LiveGraphicsError> {
        validate_scene_packet(&packet)?;
        let capabilities = supervisor.capabilities()?.clone();
        let capabilities_sha256 = capability_digest(&capabilities)?;
        let packet_identity = packet_identity(&packet, &capabilities_sha256);
        let project_id = project_id.into();
        let session_id = session_id.into();
        let evidence = LiveEvidenceSession::new(
            project_id.clone(),
            session_id.clone(),
            packet_identity,
            policy,
            started_at_ms,
        )?;
        Ok(Self {
            evidence,
            project_id,
            session_id,
            active_packet: packet,
            active_capabilities: capabilities,
            capabilities_sha256,
            last_event_sequence: 0,
        })
    }

    pub fn mode(&self) -> GraphicsSessionMode {
        GraphicsSessionMode::OffscreenCapture
    }

    pub fn request_live_present(&self) -> LivePresentOutcome {
        LivePresentOutcome::Unsupported {
            reason: "the supervised graphics worker provides on-demand offscreen captures; it has no live window or presentation loop",
        }
    }

    pub fn state(&self) -> &SessionState {
        self.evidence.state()
    }

    pub fn active_packet(&self) -> &GraphicsScenePacket {
        &self.active_packet
    }

    pub fn capabilities_sha256(&self) -> &str {
        &self.capabilities_sha256
    }

    pub fn pending_sample(&self) -> Option<&TierASnapshotRequest> {
        self.evidence.pending_request()
    }

    /// Advance the contract's monotonic timer and return a newly scheduled
    /// Tier A request, if one became due.
    pub fn tick(&mut self, now_ms: u64) -> Result<Option<TierASnapshotRequest>, LiveGraphicsError> {
        self.evidence.tick(now_ms).map_err(Into::into)
    }

    /// Observe packet, camera, and capability changes using a fresh validated
    /// worker capability response. Each change schedules or rebinds a Tier A
    /// request through the live evidence contract.
    pub fn synchronize_binding(
        &mut self,
        supervisor: &mut GraphicsWorkerSupervisor,
        packet: &GraphicsScenePacket,
        observed_at_ms: u64,
    ) -> Result<(), LiveGraphicsError> {
        validate_scene_packet(packet)?;
        let capabilities = supervisor.capabilities()?.clone();
        self.observe_binding(packet, capabilities, observed_at_ms)
    }

    /// Render through the persistent Rust supervisor, independently retain its
    /// promoted receipt, and record Tier B only after packet, capability,
    /// capture, and telemetry bindings have been checked.
    pub fn render_capture_and_record(
        &mut self,
        supervisor: &mut GraphicsWorkerSupervisor,
        packet: &GraphicsScenePacket,
        world: &WorldArtifact,
        frame_sequence: u64,
        observed_at_ms: u64,
    ) -> Result<LiveCaptureOutput, LiveGraphicsError> {
        self.synchronize_binding(supervisor, packet, observed_at_ms)?;
        let promoted = supervisor.render_and_promote(packet, world)?;
        self.validate_promoted_frame(packet, &promoted)?;
        let telemetry = &promoted.frame.telemetry;
        let attestation = self.make_attestation(telemetry, frame_sequence, observed_at_ms)?;
        if let Err(diagnostic) = self.evidence.record_live_frame(attestation.clone()) {
            if diagnostic.code == DiagnosticCode::InvalidTelemetry {
                return Err(LiveGraphicsError::Telemetry(format!(
                    "{diagnostic}; Tier B={:?}; promoted source={telemetry:?}",
                    attestation.telemetry
                )));
            }
            return Err(diagnostic.into());
        }
        Ok(LiveCaptureOutput {
            promoted,
            attestation,
        })
    }

    /// Consume a typed Tier A result. Certification is accepted only as
    /// represented by the typed contract; callers remain responsible for
    /// independently validating any certified snapshot with native validators.
    pub fn complete_snapshot(
        &mut self,
        result: Option<TierAResult>,
        now_ms: u64,
    ) -> Result<(), LiveGraphicsError> {
        self.evidence
            .complete_snapshot(result, now_ms)
            .map_err(Into::into)
    }

    /// Decode and consume a closed-schema Tier A wire result. Pass-shaped
    /// status fields are rejected by the evidence contract.
    pub fn complete_snapshot_json(
        &mut self,
        bytes: &[u8],
        now_ms: u64,
    ) -> Result<(), LiveGraphicsError> {
        self.evidence
            .complete_snapshot_json(bytes, now_ms)
            .map_err(Into::into)
    }

    /// Deterministic digest of the typed evidence-session snapshot. It depends
    /// on explicit caller timestamps and accepted transition order only.
    pub fn digest(&self) -> Result<String, LiveGraphicsError> {
        self.evidence.digest().map_err(Into::into)
    }

    fn observe_binding(
        &mut self,
        packet: &GraphicsScenePacket,
        capabilities: GraphicsReady,
        observed_at_ms: u64,
    ) -> Result<(), LiveGraphicsError> {
        let capabilities_sha256 = capability_digest(&capabilities)?;
        let packet_changed = packet.packet_sha256 != self.active_packet.packet_sha256;
        let camera_changed = packet.body.camera != self.active_packet.body.camera;
        let capabilities_changed = capabilities_sha256 != self.capabilities_sha256;
        if !(packet_changed || capabilities_changed) {
            return Ok(());
        }

        let identity = packet_identity(packet, &capabilities_sha256);
        let mut changes = Vec::with_capacity(3);
        if packet_changed {
            changes.push(SemanticEventKind::SceneChanged);
        }
        if camera_changed {
            changes.push(SemanticEventKind::CameraCut);
        }
        if capabilities_changed {
            changes.push(SemanticEventKind::CapabilityChanged);
        }

        for kind in changes {
            let sequence = self.last_event_sequence.checked_add(1).ok_or_else(|| {
                LiveGraphicsError::Identity("semantic event sequence overflow".into())
            })?;
            let event = SemanticEvent {
                sequence,
                occurred_at_ms: observed_at_ms,
                kind,
                packet: identity.clone(),
            };
            self.evidence.observe_event(event)?;
            self.last_event_sequence = sequence;
            // Each accepted event already binds the full final identity. Keep
            // the local binding aligned if a later coalesced event fails.
            self.active_packet = packet.clone();
            self.active_capabilities = capabilities.clone();
            self.capabilities_sha256.clone_from(&capabilities_sha256);
        }
        Ok(())
    }

    fn validate_promoted_frame(
        &self,
        packet: &GraphicsScenePacket,
        promoted: &PromotedFrame,
    ) -> Result<(), LiveGraphicsError> {
        if packet.packet_sha256 != self.active_packet.packet_sha256
            || promoted.frame.packet_sha256 != packet.packet_sha256
            || promoted.receipt.body.packet_sha256 != packet.packet_sha256
        {
            return Err(LiveGraphicsError::Identity(
                "promoted frame is stale or detached from the active packet".into(),
            ));
        }
        if promoted.frame.schema != "wge.lava-frame/v1"
            || promoted.frame.capture_id != packet.body.capture.capture_id
            || promoted.frame.width_px != packet.body.capture.width_px
            || promoted.frame.height_px != packet.body.capture.height_px
            || promoted.frame.capture_sha256
                != promoted
                    .receipt
                    .body
                    .capture_sha256
                    .as_deref()
                    .unwrap_or_default()
        {
            return Err(LiveGraphicsError::Identity(
                "promoted capture identity does not match the packet and receipt".into(),
            ));
        }
        if promoted.receipt.body.status != FrameStatus::Passed
            || promoted.receipt.body.format != CaptureFormat::Rgba8Srgb
            || promoted.receipt.body.backend_id != self.active_capabilities.backend_id
            || promoted.receipt.body.adapter_revision != self.active_capabilities.adapter_revision
            || promoted.receipt.body.lava_revision != self.active_capabilities.lava_revision
            || promoted.receipt.body.device_uuid != self.active_capabilities.device_uuid
            || promoted.frame.backend_id != self.active_capabilities.backend_id
            || promoted.frame.adapter_revision != self.active_capabilities.adapter_revision
            || promoted.frame.lava_revision != self.active_capabilities.lava_revision
            || promoted.frame.telemetry != promoted.receipt.body.telemetry
        {
            return Err(LiveGraphicsError::Identity(
                "promoted frame telemetry or backend identity is detached from validated capabilities".into(),
            ));
        }
        validate_frame_receipt(&promoted.receipt, packet, &promoted.capture_bytes)?;
        Ok(())
    }

    fn make_attestation(
        &self,
        telemetry: &GraphicsTelemetry,
        frame_sequence: u64,
        observed_at_ms: u64,
    ) -> Result<LiveFrameAttestation, LiveGraphicsError> {
        let draw_calls = u32::try_from(telemetry.draw_calls).map_err(|_| {
            LiveGraphicsError::Telemetry("draw_calls exceeds the Tier B integer range".into())
        })?;
        let submitted_instances = u32::try_from(telemetry.instance_count).map_err(|_| {
            LiveGraphicsError::Telemetry("instance_count exceeds the Tier B integer range".into())
        })?;
        let visible_instances = u32::try_from(telemetry.visible_instance_count).map_err(|_| {
            LiveGraphicsError::Telemetry(
                "visible_instance_count exceeds the Tier B integer range".into(),
            )
        })?;
        Ok(LiveFrameAttestation {
            schema_version: LIVE_FRAME_SCHEMA.into(),
            project_id: self.project_id.clone(),
            session_id: self.session_id.clone(),
            packet: packet_identity(&self.active_packet, &self.capabilities_sha256),
            frame_sequence,
            observed_at_ms,
            telemetry: LiveFrameTelemetry {
                frame_time_us: telemetry.frame_time_us,
                // The worker reports GPU duration only when timestamp support
                // is available. Zero means “unreported” in this offscreen
                // adapter; Tier B does not turn it into certification evidence.
                gpu_time_us: telemetry.gpu_frame_time_us.unwrap_or(0),
                draw_calls,
                submitted_instances,
                visible_instances,
                // An on-demand capture has no presentation queue to drop from.
                dropped_frames: 0,
            },
        })
    }
}

fn capability_digest(capabilities: &GraphicsReady) -> Result<String, LiveGraphicsError> {
    let bytes = canonical_json(capabilities)?;
    Ok(sha256_prefixed(&bytes))
}

fn packet_identity(packet: &GraphicsScenePacket, capabilities_sha256: &str) -> PacketIdentity {
    PacketIdentity {
        packet_sha256: packet.packet_sha256.clone(),
        capabilities_sha256: capabilities_sha256.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GraphicsFeatures, READY_SCHEMA};

    fn ready(device_uuid: &str) -> GraphicsReady {
        GraphicsReady {
            schema_version: READY_SCHEMA.into(),
            backend_id: "lava-vulkan".into(),
            adapter_revision: "wge.lava-adapter/v7".into(),
            lava_revision: "test-revision".into(),
            julia_version: "1.11.0".into(),
            vulkan_api_version: "1.3".into(),
            device_name: "test device".into(),
            device_uuid: device_uuid.into(),
            features: GraphicsFeatures {
                offscreen_raster: true,
                depth_attachment: true,
                texture_sampling: true,
                readback: true,
                hardware_ray_tracing: false,
                gpu_timestamps: true,
            },
        }
    }

    #[test]
    fn typed_capability_digest_changes_with_validated_capability_payload() {
        let first = capability_digest(&ready("device-a")).expect("first digest");
        let second = capability_digest(&ready("device-b")).expect("second digest");
        assert_ne!(first, second);
        assert!(first.starts_with("sha256:"));
    }
}
