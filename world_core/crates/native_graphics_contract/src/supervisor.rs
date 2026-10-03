use std::collections::BTreeMap;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use wge_asset_contract::{AssetPreparationReceipt, RenderAssetPackage};
use wge_project_ledger::{SceneArtifact, validate_scene_against_asset_receipts_and_render_assets};

use crate::{
    ADAPTER_REVISION, Campaign2View, CaptureFormat, FRAME_RECEIPT_SCHEMA, FrameStatus,
    GraphicsAssetProjection, GraphicsCamera, GraphicsContractError, GraphicsFrameMeasurements,
    GraphicsFrameReceipt, GraphicsFrameReceiptBody, GraphicsReady, GraphicsRendererAttestation,
    GraphicsRendererAttestationBody, GraphicsScenePacket, GraphicsTelemetry, LAVA_BACKEND_ID,
    LAVA_REVISION, MAX_DENSE_BENCHMARK_INSTANCES, canonical_json, compose_bound_scene_with_camera,
    lower_campaign2_packet, lower_campaign2_packet_with, lower_dense_benchmark_packet,
    ParityContent, ParityPolicyCandidate, TerrainLayerSet, lower_objective_close_packet,
    lower_reference_world, lower_showcase_packet, lower_world_showcase_packet,
    measure_frame_capture, seal_frame_receipt_with_capture, seal_renderer_attestation,
    seal_scene_packet, sha256_prefixed, validate_native_visual_gate, validate_ready,
    validate_scene_packet, validate_texture_residency_telemetry,
};
use wge_reference_runtime::{WorldArtifact, validate_world_artifact};

pub const WORKER_SCHEMA: &str = "wge.graphics-worker/v1";
const MAX_WORKER_FRAME_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_WORKER_RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
/// The ready handshake covers Julia package loading, which on a cold or
/// invalidated depot includes precompiling Lava and its GPU stack (measured:
/// Lava alone 38 s, the full chain over 120 s). Bounding startup by the
/// per-request response timeout made a fresh depot look like a hung worker
/// (CONVERGE-0 ledger, turn 5), so startup has its own, longer bound.
const DEFAULT_WORKER_STARTUP_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphicsWorkerError {
    pub code: &'static str,
    pub message: String,
}

impl GraphicsWorkerError {
    pub(crate) fn io(message: impl Into<String>) -> Self {
        Self {
            code: "worker_io",
            message: message.into(),
        }
    }

    pub(crate) fn timeout(message: impl Into<String>) -> Self {
        Self {
            code: "worker_timeout",
            message: message.into(),
        }
    }

    pub(crate) fn restart_required(message: impl Into<String>) -> Self {
        Self {
            code: "worker_restart_required",
            message: message.into(),
        }
    }

    pub(crate) fn protocol(message: impl Into<String>) -> Self {
        Self {
            code: "worker_protocol",
            message: message.into(),
        }
    }

    pub(crate) fn provenance(message: impl Into<String>) -> Self {
        Self {
            code: "provenance",
            message: message.into(),
        }
    }

    pub(crate) fn contract(error: GraphicsContractError) -> Self {
        Self {
            code: error.code,
            message: error.message,
        }
    }
}

impl std::fmt::Display for GraphicsWorkerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for GraphicsWorkerError {}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsFrameOutput {
    pub schema: String,
    pub backend_id: String,
    pub adapter_revision: String,
    pub lava_revision: String,
    pub packet_sha256: String,
    pub capture_id: String,
    pub width_px: u32,
    pub height_px: u32,
    pub capture_sha256: String,
    pub capture_base64: String,
    pub measurements: GraphicsFrameMeasurements,
    pub telemetry: GraphicsTelemetry,
}

#[derive(Debug)]
pub struct PromotedFrame {
    pub frame: GraphicsFrameOutput,
    pub receipt: GraphicsFrameReceipt,
    pub renderer_attestation: GraphicsRendererAttestation,
    pub capture_bytes: Vec<u8>,
}

/// Canonical inputs required to independently authorize a scene-bound packet
/// before it crosses into the Julia/Lava execution boundary.
pub struct BoundSceneRenderAuthorization<'a> {
    pub base_packet: &'a GraphicsScenePacket,
    pub scene: &'a SceneArtifact,
    pub asset_receipts: &'a [AssetPreparationReceipt],
    pub render_packages: &'a [RenderAssetPackage],
    pub assets: &'a [GraphicsAssetProjection],
    /// Optional derived inspection view. It is re-applied during independent
    /// Rust recomposition and therefore cannot be changed after promotion.
    pub camera: Option<&'a GraphicsCamera>,
}

pub struct GraphicsWorkerSupervisor {
    child: Child,
    stdin: Option<ChildStdin>,
    responses: Receiver<Result<Vec<u8>, String>>,
    reader: Option<JoinHandle<()>>,
    response_timeout: Duration,
    startup_timeout: Duration,
    restart_required: bool,
    source_identity_sha256: String,
    ready_message: Value,
    ready: Option<GraphicsReady>,
    julia: PathBuf,
    project: PathBuf,
    worker: PathBuf,
}

impl GraphicsWorkerSupervisor {
    pub fn start(
        julia: impl AsRef<Path>,
        project: impl AsRef<Path>,
        worker: impl AsRef<Path>,
    ) -> Result<Self, GraphicsWorkerError> {
        Self::start_with_timeouts(
            julia,
            project,
            worker,
            DEFAULT_WORKER_STARTUP_TIMEOUT,
            DEFAULT_WORKER_RESPONSE_TIMEOUT,
        )
    }

    /// One bound for both the ready handshake and every response.
    pub fn start_with_timeout(
        julia: impl AsRef<Path>,
        project: impl AsRef<Path>,
        worker: impl AsRef<Path>,
        response_timeout: Duration,
    ) -> Result<Self, GraphicsWorkerError> {
        Self::start_with_timeouts(julia, project, worker, response_timeout, response_timeout)
    }

    /// Separate bounds for the ready handshake (package load and precompile)
    /// and for each request afterwards.
    pub fn start_with_timeouts(
        julia: impl AsRef<Path>,
        project: impl AsRef<Path>,
        worker: impl AsRef<Path>,
        startup_timeout: Duration,
        response_timeout: Duration,
    ) -> Result<Self, GraphicsWorkerError> {
        let julia = julia.as_ref().to_owned();
        let project = project.as_ref().to_owned();
        let worker = worker.as_ref().to_owned();
        let mut child = Command::new(&julia)
            .arg(format!("--project={}", project.display()))
            .arg("--startup-file=no")
            .arg(&worker)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                GraphicsWorkerError::io(format!("failed to spawn Julia worker: {error}"))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| GraphicsWorkerError::io("Julia worker stdin was not captured"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GraphicsWorkerError::io("Julia worker stdout was not captured"))?;
        let (response_sender, responses) = mpsc::channel();
        let reader = thread::Builder::new()
            .name("wge-lava-worker-reader".into())
            .spawn(move || {
                let mut stdout = BufReader::new(stdout);
                loop {
                    match read_worker_frame(&mut stdout) {
                        Ok(payload) => {
                            if response_sender.send(Ok(payload)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = response_sender.send(Err(error));
                            break;
                        }
                    }
                }
            })
            .map_err(|error| {
                let _ = child.kill();
                let _ = child.wait();
                GraphicsWorkerError::io(format!("failed to start worker reader: {error}"))
            })?;
        let mut supervisor = Self {
            child,
            stdin: Some(stdin),
            responses,
            reader: Some(reader),
            response_timeout,
            startup_timeout,
            restart_required: false,
            source_identity_sha256: String::new(),
            ready_message: Value::Null,
            ready: None,
            julia,
            project,
            worker,
        };
        let ready = supervisor.read_frame_json_within(startup_timeout)?;
        supervisor.validate_worker_ready(&ready)?;
        supervisor.ready_message = ready;
        supervisor.source_identity_sha256 = supervisor.renderer_source_identity_sha256()?;
        Ok(supervisor)
    }

    pub fn worker_ready_message(&self) -> &Value {
        &self.ready_message
    }

    pub fn ready(&self) -> Option<&GraphicsReady> {
        self.ready.as_ref()
    }

    pub fn set_response_timeout(&mut self, response_timeout: Duration) {
        self.response_timeout = response_timeout;
    }

    pub fn restart(&mut self) -> Result<(), GraphicsWorkerError> {
        self.stop_child();
        let mut replacement = Self::start_with_timeouts(
            &self.julia,
            &self.project,
            &self.worker,
            self.startup_timeout,
            self.response_timeout,
        )?;
        replacement.response_timeout = self.response_timeout;
        let old = std::mem::replace(self, replacement);
        drop(old);
        Ok(())
    }

    pub fn capabilities(&mut self) -> Result<&GraphicsReady, GraphicsWorkerError> {
        let response = self.request(json!({"op": "probe_capabilities"}))?;
        let ready_value = response.get("ready").ok_or_else(|| {
            GraphicsWorkerError::protocol("capability response has no ready payload")
        })?;
        let ready: GraphicsReady =
            serde_json::from_value(ready_value.clone()).map_err(|error| {
                GraphicsWorkerError::protocol(format!("ready payload is not typed: {error}"))
            })?;
        validate_ready(&ready).map_err(GraphicsWorkerError::contract)?;
        if ready.backend_id != LAVA_BACKEND_ID
            || ready.adapter_revision != ADAPTER_REVISION
            || ready.lava_revision != LAVA_REVISION
        {
            return Err(GraphicsWorkerError::provenance(
                "capability identity is not the audited Lava adapter",
            ));
        }
        self.ready = Some(ready);
        Ok(self.ready.as_ref().expect("ready was just stored"))
    }

    pub fn render_and_promote(
        &mut self,
        packet: &GraphicsScenePacket,
        world: &WorldArtifact,
    ) -> Result<PromotedFrame, GraphicsWorkerError> {
        validate_scene_packet(packet).map_err(GraphicsWorkerError::contract)?;
        validate_world_artifact_binding(packet, world)?;
        validate_authorized_world_projection(packet, world, None)?;
        self.render_validated_and_promote(packet)
    }

    /// Render a packet whose terrain is surfaced by a verified layer set (N-4).
    /// The set is an explicit input, like a bound scene's packages: the
    /// supervisor re-derives the authorized projection WITH the same set and
    /// requires an exact match, so layer content cannot be smuggled into a
    /// packet the world does not authorize.
    pub fn render_and_promote_with_terrain_layers(
        &mut self,
        packet: &GraphicsScenePacket,
        world: &WorldArtifact,
        terrain_layers: &TerrainLayerSet,
    ) -> Result<PromotedFrame, GraphicsWorkerError> {
        validate_scene_packet(packet).map_err(GraphicsWorkerError::contract)?;
        validate_world_artifact_binding(packet, world)?;
        validate_authorized_world_projection(packet, world, Some(terrain_layers))?;
        self.render_validated_and_promote(packet)
    }

    /// Render a packet whose additional scene objects were produced by the
    /// typed Rust scene-composition seam. The base packet, scene artifact,
    /// runtime receipts, conditioned packages, and graphics projections are
    /// all supplied so the supervisor can independently revalidate the exact
    /// composition before any bytes reach Julia/Lava.
    pub fn render_bound_scene_and_promote(
        &mut self,
        packet: &GraphicsScenePacket,
        world: &WorldArtifact,
        authorization: BoundSceneRenderAuthorization<'_>,
    ) -> Result<PromotedFrame, GraphicsWorkerError> {
        validate_scene_packet(packet).map_err(GraphicsWorkerError::contract)?;
        validate_scene_packet(authorization.base_packet).map_err(GraphicsWorkerError::contract)?;
        validate_world_artifact_binding(packet, world)?;
        validate_world_artifact_binding(authorization.base_packet, world)?;
        validate_authorized_world_projection(authorization.base_packet, world, None)?;
        validate_scene_against_asset_receipts_and_render_assets(
            authorization.scene,
            authorization.asset_receipts,
            authorization.render_packages,
        )
        .map_err(|error| {
            GraphicsWorkerError::provenance(format!(
                "bound scene failed independent asset validation: {error}"
            ))
        })?;
        let mut base_body = authorization.base_packet.body.clone();
        let recomposed = compose_bound_scene_with_camera(
            &mut base_body,
            authorization.scene,
            authorization.assets,
            authorization.camera,
        )
        .map_err(GraphicsWorkerError::contract)?;
        if &recomposed != packet {
            return Err(GraphicsWorkerError::provenance(
                "bound scene packet does not match Rust recomposition",
            ));
        }
        self.render_validated_and_promote(packet)
    }

    fn render_validated_and_promote(
        &mut self,
        packet: &GraphicsScenePacket,
    ) -> Result<PromotedFrame, GraphicsWorkerError> {
        let ready = self.ready.clone().ok_or_else(|| {
            GraphicsWorkerError::protocol(
                "capabilities must be validated before a frame can be promoted",
            )
        })?;
        let ready_backend_id = ready.backend_id.clone();
        let ready_adapter_revision = ready.adapter_revision.clone();
        let ready_lava_revision = ready.lava_revision.clone();
        let ready_device_uuid = ready.device_uuid.clone();
        let worker_script_sha256 = self
            .ready_message
            .get("script_sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                GraphicsWorkerError::provenance("worker ready message has no script identity")
            })?
            .to_owned();
        self.ensure_renderer_sources_unchanged()?;
        let renderer_identity_sha256 =
            self.renderer_identity_sha256(&ready, &worker_script_sha256)?;
        let response = self.request(json!({
            "op": "render_packet",
            "packet": packet,
            "expected_packet_sha256": packet.packet_sha256,
        }))?;
        let frame_value = response
            .get("frame")
            .ok_or_else(|| GraphicsWorkerError::protocol("render response has no frame payload"))?;
        let frame: GraphicsFrameOutput =
            serde_json::from_value(frame_value.clone()).map_err(|error| {
                GraphicsWorkerError::protocol(format!("frame payload is not typed: {error}"))
            })?;
        if frame.schema != "wge.lava-frame/v1" {
            return Err(GraphicsWorkerError::protocol(format!(
                "unsupported frame schema {}",
                frame.schema
            )));
        }
        if frame.packet_sha256 != packet.packet_sha256 {
            return Err(GraphicsWorkerError::provenance(
                "frame is bound to a different scene packet",
            ));
        }
        if frame.capture_id != packet.body.capture.capture_id
            || frame.width_px != packet.body.capture.width_px
            || frame.height_px != packet.body.capture.height_px
        {
            return Err(GraphicsWorkerError::provenance(
                "frame capture is detached from the packet capture request",
            ));
        }
        if frame.backend_id != ready_backend_id
            || frame.adapter_revision != ready_adapter_revision
            || frame.lava_revision != ready_lava_revision
        {
            return Err(GraphicsWorkerError::provenance(
                "frame backend identity is detached from validated capabilities",
            ));
        }
        let capture_bytes = STANDARD.decode(&frame.capture_base64).map_err(|error| {
            GraphicsWorkerError::provenance(format!("frame capture is not valid base64: {error}"))
        })?;
        let expected_len = usize::try_from(frame.width_px)
            .ok()
            .and_then(|width| {
                usize::try_from(frame.height_px)
                    .ok()
                    .and_then(|height| width.checked_mul(height)?.checked_mul(4))
            })
            .ok_or_else(|| {
                GraphicsWorkerError::provenance("frame dimensions overflow capture size")
            })?;
        if capture_bytes.len() != expected_len {
            return Err(GraphicsWorkerError::provenance(format!(
                "frame capture has {} bytes, expected {expected_len}",
                capture_bytes.len()
            )));
        }
        if sha256_prefixed(&capture_bytes) != frame.capture_sha256 {
            return Err(GraphicsWorkerError::provenance(
                "frame capture digest does not match its bytes",
            ));
        }
        self.ensure_renderer_sources_unchanged()?;
        let renderer_attestation =
            self.renderer_attestation(&ready, &worker_script_sha256, &renderer_identity_sha256)?;
        validate_frame_telemetry(packet, &frame.telemetry)?;
        let authoritative_measurements =
            measure_frame_capture(packet, &capture_bytes).map_err(GraphicsWorkerError::contract)?;
        validate_native_visual_gate(packet, &authoritative_measurements, &capture_bytes)
            .map_err(GraphicsWorkerError::contract)?;
        if !measurements_agree(&frame.measurements, &authoritative_measurements) {
            return Err(GraphicsWorkerError::provenance(
                "worker visual measurements disagree with Rust capture measurements",
            ));
        }
        let body = GraphicsFrameReceiptBody {
            schema_version: FRAME_RECEIPT_SCHEMA.into(),
            packet_sha256: packet.packet_sha256.clone(),
            capture_id: frame.capture_id.clone(),
            backend_id: frame.backend_id.clone(),
            adapter_revision: frame.adapter_revision.clone(),
            lava_revision: frame.lava_revision.clone(),
            device_uuid: ready_device_uuid,
            worker_script_sha256,
            renderer_identity_sha256,
            status: FrameStatus::Passed,
            format: CaptureFormat::Rgba8Srgb,
            width_px: frame.width_px,
            height_px: frame.height_px,
            capture_sha256: Some(frame.capture_sha256.clone()),
            measurements: authoritative_measurements,
            telemetry: frame.telemetry.clone(),
            detail: "Rust-promoted Lava frame with independently verified capture bytes".into(),
        };
        let receipt = seal_frame_receipt_with_capture(packet, body, &capture_bytes)
            .map_err(GraphicsWorkerError::contract)?;
        Ok(PromotedFrame {
            frame,
            receipt,
            renderer_attestation,
            capture_bytes,
        })
    }

    pub fn request(&mut self, request: Value) -> Result<Value, GraphicsWorkerError> {
        if self.restart_required {
            return Err(GraphicsWorkerError::restart_required(
                "worker transport is invalid; restart the graphics worker before issuing another request",
            ));
        }
        let payload = serde_json::to_vec(&request).map_err(|error| {
            GraphicsWorkerError::protocol(format!("request JSON serialization failed: {error}"))
        })?;
        if let Err(error) = self.write_frame(&payload) {
            self.ready = None;
            self.restart_required = true;
            self.stop_child();
            return Err(error);
        }
        let response = match self.read_frame_json() {
            Ok(response) => response,
            Err(error) => {
                self.ready = None;
                self.restart_required = true;
                self.stop_child();
                return Err(error);
            }
        };
        if response.get("schema").and_then(Value::as_str) != Some(WORKER_SCHEMA) {
            return Err(GraphicsWorkerError::protocol(
                "worker response schema is unsupported",
            ));
        }
        if response.get("kind").and_then(Value::as_str) == Some("failed") {
            let code = response
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("worker_error");
            let detail = response
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or("unknown worker failure");
            return Err(GraphicsWorkerError::protocol(format!("{code}: {detail}")));
        }
        Ok(response)
    }

    fn validate_worker_ready(&self, ready: &Value) -> Result<(), GraphicsWorkerError> {
        if ready.get("schema").and_then(Value::as_str) != Some(WORKER_SCHEMA) {
            return Err(GraphicsWorkerError::protocol(
                "worker ready schema is unsupported",
            ));
        }
        if ready.get("lava_revision").and_then(Value::as_str) != Some(LAVA_REVISION) {
            return Err(GraphicsWorkerError::provenance(
                "worker ready Lava revision is not the audited revision",
            ));
        }
        if ready.get("adapter_revision").and_then(Value::as_str) != Some(ADAPTER_REVISION) {
            return Err(GraphicsWorkerError::provenance(
                "worker ready adapter revision is not the audited revision",
            ));
        }
        let script_sha256 = ready
            .get("script_sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                GraphicsWorkerError::provenance("worker ready message has no script identity")
            })?;
        if script_sha256.len() != 71
            || !script_sha256.starts_with("sha256:")
            || !script_sha256[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(GraphicsWorkerError::provenance(
                "worker ready script identity is not a sha256 digest",
            ));
        }
        let expected_script_sha256 = sha256_file(&self.worker)?;
        if script_sha256 != expected_script_sha256 {
            return Err(GraphicsWorkerError::provenance(
                "worker ready script identity does not match the launched worker file",
            ));
        }
        if ready.get("kind").and_then(Value::as_str) != Some("ready") {
            return Err(GraphicsWorkerError::protocol(
                "worker did not send a ready message",
            ));
        }
        Ok(())
    }

    fn renderer_identity_sha256(
        &self,
        ready: &GraphicsReady,
        worker_script_sha256: &str,
    ) -> Result<String, GraphicsWorkerError> {
        let source_digests = self.renderer_source_digests(worker_script_sha256)?;
        let identity = json!({
            "capabilities": ready,
            "worker_ready": self.ready_message,
            "sources": source_digests,
        });
        Ok(sha256_prefixed(
            &canonical_json(&identity).map_err(GraphicsWorkerError::contract)?,
        ))
    }

    fn renderer_attestation(
        &self,
        ready: &GraphicsReady,
        worker_script_sha256: &str,
        renderer_identity_sha256: &str,
    ) -> Result<GraphicsRendererAttestation, GraphicsWorkerError> {
        let source_digests = self.renderer_source_digests(worker_script_sha256)?;
        let source_identity_sha256 = sha256_prefixed(
            &canonical_json(&source_digests).map_err(GraphicsWorkerError::contract)?,
        );
        seal_renderer_attestation(GraphicsRendererAttestationBody {
            schema_version: crate::RENDERER_ATTESTATION_SCHEMA.into(),
            backend_id: ready.backend_id.clone(),
            adapter_revision: ready.adapter_revision.clone(),
            lava_revision: ready.lava_revision.clone(),
            worker_ready_message: self.ready_message.clone(),
            ready: ready.clone(),
            source_digests,
            source_identity_sha256,
            renderer_identity_sha256: renderer_identity_sha256.to_owned(),
        })
        .map_err(GraphicsWorkerError::contract)
    }

    fn renderer_source_digests(
        &self,
        worker_script_sha256: &str,
    ) -> Result<BTreeMap<String, String>, GraphicsWorkerError> {
        Ok(BTreeMap::from([
            ("worker_script".into(), worker_script_sha256.to_owned()),
            (
                "lava_adapter".into(),
                sha256_file(&self.project.join("src/LavaAdapter.jl"))?,
            ),
            (
                "graphics_contract".into(),
                sha256_file(&self.project.join("src/WGEGraphics.jl"))?,
            ),
            (
                "project".into(),
                sha256_file(&self.project.join("Project.toml"))?,
            ),
            (
                "manifest".into(),
                sha256_file(&self.project.join("Manifest.toml"))?,
            ),
        ]))
    }

    fn renderer_source_identity_sha256(&self) -> Result<String, GraphicsWorkerError> {
        let source_digests = self.renderer_source_digests(&sha256_file(&self.worker)?)?;
        Ok(sha256_prefixed(
            &canonical_json(&source_digests).map_err(GraphicsWorkerError::contract)?,
        ))
    }

    fn ensure_renderer_sources_unchanged(&mut self) -> Result<(), GraphicsWorkerError> {
        let current = self.renderer_source_identity_sha256()?;
        if current == self.source_identity_sha256 {
            return Ok(());
        }
        self.ready = None;
        self.restart_required = true;
        self.stop_child();
        Err(GraphicsWorkerError::restart_required(
            "renderer source identity changed after worker startup; restart the graphics worker",
        ))
    }

    fn write_frame(&mut self, payload: &[u8]) -> Result<(), GraphicsWorkerError> {
        if payload.len() > MAX_WORKER_FRAME_BYTES {
            return Err(GraphicsWorkerError::protocol(
                "request exceeds worker frame bound",
            ));
        }
        let length = u32::try_from(payload.len())
            .map_err(|_| GraphicsWorkerError::protocol("request length overflows frame header"))?;
        let mut frame = Vec::with_capacity(4 + payload.len());
        frame.extend_from_slice(&length.to_be_bytes());
        frame.extend_from_slice(payload);
        let stdin = self
            .stdin
            .take()
            .ok_or_else(|| GraphicsWorkerError::restart_required("worker stdin is unavailable"))?;
        let timeout = self.response_timeout;
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("wge-lava-worker-writer".into())
            .spawn(move || {
                let mut stdin = stdin;
                let result = stdin
                    .write_all(&frame)
                    .and_then(|_| stdin.flush())
                    .map_err(|error| error.to_string());
                // Returning the pipe on both success and failure avoids
                // losing the live transport on the normal path.
                let _ = sender.send((stdin, result));
            })
            .map_err(|error| {
                GraphicsWorkerError::io(format!("failed to start worker writer: {error}"))
            })?;
        match receiver.recv_timeout(timeout) {
            Ok((stdin, result)) => {
                self.stdin = Some(stdin);
                result.map_err(|error| {
                    GraphicsWorkerError::io(format!("failed to write worker frame: {error}"))
                })
            }
            Err(RecvTimeoutError::Timeout) => Err(GraphicsWorkerError::timeout(format!(
                "worker did not accept a request within {} ms",
                timeout.as_millis()
            ))),
            Err(RecvTimeoutError::Disconnected) => {
                Err(GraphicsWorkerError::io("worker writer disconnected"))
            }
        }
    }

    fn read_frame_json(&mut self) -> Result<Value, GraphicsWorkerError> {
        self.read_frame_json_within(self.response_timeout)
    }

    fn read_frame_json_within(&mut self, timeout: Duration) -> Result<Value, GraphicsWorkerError> {
        let payload = match self.responses.recv_timeout(timeout) {
            Ok(Ok(payload)) => payload,
            Ok(Err(error)) => return Err(GraphicsWorkerError::io(error)),
            Err(RecvTimeoutError::Timeout) => {
                return Err(GraphicsWorkerError::timeout(format!(
                    "worker did not answer within {} ms",
                    timeout.as_millis()
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(GraphicsWorkerError::io("worker reader disconnected"));
            }
        };
        serde_json::from_slice(&payload).map_err(|error| {
            GraphicsWorkerError::protocol(format!("worker frame is not JSON: {error}"))
        })
    }

    fn stop_child(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn validate_authorized_world_projection(
    packet: &GraphicsScenePacket,
    world: &WorldArtifact,
    terrain_layers: Option<&TerrainLayerSet>,
) -> Result<(), GraphicsWorkerError> {
    let reference = lower_reference_world(world).map_err(GraphicsWorkerError::contract)?;
    let campaign2 = |view| match terrain_layers {
        None => lower_campaign2_packet(&reference, view),
        Some(set) => {
            let candidate = ParityPolicyCandidate::from_env();
            let content = ParityContent::from_env(candidate)?;
            lower_campaign2_packet_with(&reference, view, candidate, content, Some(set))
        }
    };
    let mut authorized = vec![
        reference.clone(),
        lower_objective_close_packet(&reference).map_err(GraphicsWorkerError::contract)?,
        lower_showcase_packet(&reference).map_err(GraphicsWorkerError::contract)?,
        lower_world_showcase_packet(&reference).map_err(GraphicsWorkerError::contract)?,
    ];
    // A Campaign 2 packet names its view in its packet id. Re-lowering only that
    // view (instead of all three) is the same exact-match check at a third of
    // the cost; with an N-4 layer set each lowering clones and seals ~35 MB, so
    // the difference is minutes per render. A packet whose id names no view
    // still gets all three candidates, so nothing is authorized by the shortcut.
    let named_view = [
        (Campaign2View::Close, "-campaign2-close"),
        (Campaign2View::Medium, "-campaign2-medium"),
        (Campaign2View::Wide, "-campaign2-wide"),
    ]
    .into_iter()
    .find(|(_, suffix)| packet.body.packet_id.ends_with(suffix))
    .map(|(view, _)| view);
    let views: Vec<Campaign2View> = match named_view {
        Some(view) => vec![view],
        None => vec![Campaign2View::Close, Campaign2View::Medium, Campaign2View::Wide],
    };
    for view in views {
        authorized.push(campaign2(view).map_err(GraphicsWorkerError::contract)?);
    }
    if let Some(added_instances) = packet
        .body
        .instances
        .len()
        .checked_sub(reference.body.instances.len())
        .filter(|count| (1..=MAX_DENSE_BENCHMARK_INSTANCES).contains(count))
    {
        authorized.push(
            lower_dense_benchmark_packet(&reference, added_instances)
                .map_err(GraphicsWorkerError::contract)?,
        );
    }
    for candidate in &authorized {
        if candidate == packet {
            return Ok(());
        }
        // Cameras are presentation parameters, not scene identity: an exact
        // authorized projection whose only difference is the camera is accepted
        // when the camera was produced and validated by Rust authority (the
        // packet revalidates fully, including camera basis and bounds). This is
        // the same seam C2.5 opened for Rust-owned bound-scene cameras.
        if candidate.body.camera != packet.body.camera {
            let mut body = candidate.body.clone();
            body.camera = packet.body.camera.clone();
            if let Ok(camera_variant) = seal_scene_packet(body)
                && &camera_variant == packet
            {
                return Ok(());
            }
        }
    }
    Err(GraphicsWorkerError::provenance(
        "scene packet is not an authorized projection of the validated world artifact",
    ))
}

fn validate_world_artifact_binding(
    packet: &GraphicsScenePacket,
    world: &WorldArtifact,
) -> Result<(), GraphicsWorkerError> {
    validate_world_artifact(world).map_err(|error| {
        GraphicsWorkerError::provenance(format!("world artifact failed validation: {error}"))
    })?;
    if packet.body.world_artifact_id != world.artifact_id
        || packet.body.world_artifact_sha256 != world.artifact_sha256
        || packet.body.spatial_fields_sha256 != world.body.fields.spatial_sha256
    {
        return Err(GraphicsWorkerError::provenance(
            "scene packet world provenance is detached from the validated world artifact",
        ));
    }
    Ok(())
}

fn read_worker_frame(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut header = [0u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|error| format!("failed to read worker frame header: {error}"))?;
    let length = usize::try_from(u32::from_be_bytes(header))
        .map_err(|_| "worker frame length overflows usize".to_owned())?;
    if length > MAX_WORKER_FRAME_BYTES {
        return Err("worker frame exceeds size bound".into());
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| format!("failed to read worker frame body: {error}"))?;
    Ok(payload)
}

fn sha256_file(path: &Path) -> Result<String, GraphicsWorkerError> {
    let bytes = fs::read(path).map_err(|error| {
        GraphicsWorkerError::provenance(format!(
            "renderer identity source {} is unreadable: {error}",
            path.display()
        ))
    })?;
    Ok(sha256_prefixed(&bytes))
}

fn validate_frame_telemetry(
    packet: &GraphicsScenePacket,
    telemetry: &GraphicsTelemetry,
) -> Result<(), GraphicsWorkerError> {
    validate_texture_residency_telemetry(packet, telemetry)
        .map_err(GraphicsWorkerError::contract)?;
    let expected_terrain_vertices = (packet.body.terrain.resolution - 1)
        .checked_mul(packet.body.terrain.resolution - 1)
        .and_then(|cells| cells.checked_mul(6))
        .ok_or_else(|| GraphicsWorkerError::provenance("terrain vertex count overflows"))?;
    if telemetry.terrain_vertex_count != expected_terrain_vertices {
        return Err(GraphicsWorkerError::provenance(
            "frame terrain vertex telemetry does not match the packet",
        ));
    }
    if telemetry.instance_count != packet.body.instances.len()
        || telemetry.visible_instance_count > telemetry.instance_count
        || telemetry.culled_instance_count > telemetry.instance_count
    {
        return Err(GraphicsWorkerError::provenance(
            "frame instance visibility telemetry is inconsistent with the packet",
        ));
    }
    let reported_instances = telemetry
        .visible_instance_count
        .checked_add(telemetry.culled_instance_count)
        .ok_or_else(|| {
            GraphicsWorkerError::provenance("frame instance visibility telemetry overflows")
        })?;
    if reported_instances != telemetry.instance_count {
        return Err(GraphicsWorkerError::provenance(
            "frame instance visibility telemetry does not balance",
        ));
    }
    let mut expected_importance_totals = [0usize; 3];
    for instance in &packet.body.instances {
        let index = match instance.importance {
            crate::InstanceImportance::Background => 0,
            crate::InstanceImportance::Landmark => 1,
            crate::InstanceImportance::GameplayCritical => 2,
        };
        expected_importance_totals[index] += 1;
    }
    let reported_importance = [
        (
            telemetry.background_visible_instance_count,
            telemetry.background_culled_instance_count,
        ),
        (
            telemetry.landmark_visible_instance_count,
            telemetry.landmark_culled_instance_count,
        ),
        (
            telemetry.gameplay_critical_visible_instance_count,
            telemetry.gameplay_critical_culled_instance_count,
        ),
    ];
    let mut importance_visible = 0usize;
    let mut importance_culled = 0usize;
    for (index, (visible, culled)) in reported_importance.into_iter().enumerate() {
        let total = visible.checked_add(culled).ok_or_else(|| {
            GraphicsWorkerError::provenance("frame importance telemetry overflows")
        })?;
        if total != expected_importance_totals[index] {
            return Err(GraphicsWorkerError::provenance(
                "frame importance telemetry does not match packet classes",
            ));
        }
        importance_visible = importance_visible.checked_add(visible).ok_or_else(|| {
            GraphicsWorkerError::provenance("frame importance visibility overflows")
        })?;
        importance_culled = importance_culled
            .checked_add(culled)
            .ok_or_else(|| GraphicsWorkerError::provenance("frame importance culling overflows"))?;
    }
    if importance_visible != telemetry.visible_instance_count
        || importance_culled != telemetry.culled_instance_count
    {
        return Err(GraphicsWorkerError::provenance(
            "frame importance visibility does not balance total visibility",
        ));
    }
    if packet.body.instances.is_empty() && telemetry.mesh_vertex_count != 0 {
        return Err(GraphicsWorkerError::provenance(
            "frame reports mesh vertices for a packet without instances",
        ));
    }
    Ok(())
}

fn measurements_agree(
    producer: &GraphicsFrameMeasurements,
    authority: &GraphicsFrameMeasurements,
) -> bool {
    (producer.terrain_luminance_stddev - authority.terrain_luminance_stddev).abs() <= 0.01
        && producer.distinct_terrain_colors == authority.distinct_terrain_colors
        && producer.route_visible_pixels == authority.route_visible_pixels
        && producer.player_spawn_visible_pixels == authority.player_spawn_visible_pixels
        && producer.opponent_spawn_visible_pixels == authority.opponent_spawn_visible_pixels
        && producer.encounter_visible_pixels == authority.encounter_visible_pixels
        && producer.objective_visible_pixels == authority.objective_visible_pixels
}

impl Drop for GraphicsWorkerSupervisor {
    fn drop(&mut self) {
        self.stop_child();
    }
}
