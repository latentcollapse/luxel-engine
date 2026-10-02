//! Engine-neutral window target contracts and a bounded Lava presentation probe.
//!
//! The probe opens a real GLFW/Vulkan window, renders one tiny diagnostic
//! triangle to Lava's `WindowTarget`, and calls Lava's swapchain submit/present
//! API. It never substitutes an offscreen capture for presentation evidence.

use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{LAVA_BACKEND_ID, LAVA_REVISION, MAX_CAPTURE_DIMENSION};

pub const WINDOW_TARGET_REQUEST_SCHEMA: &str = "wge.window-target-request/v1";
pub const WINDOW_PROBE_RECEIPT_SCHEMA: &str = "wge.window-probe-receipt/v1";
const JULIA_PROBE_PROTOCOL: &str = "wge.lava-window-probe/v1";
const PROBE_OUTPUT_PREFIX: &str = "WGE_WINDOW_PROBE=";
const MAX_PROBE_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_PROCESS_OUTPUT_BYTES: usize = 64 * 1024;

const LAVA_WINDOW_PROBE: &str = r#"
using Lava
using GeometryBasics
using JSON3

const WGE_WINDOW_PROBE_PROTOCOL = "wge.lava-window-probe/v1"
const WGE_LAVA_REVISION = "11c7e31bdf62408d22bf379e9e59510f69d2103e"

function _wge_window_probe_vertex()
    index = Lava.vertex_index()
    x = index == Int32(1) ? -0.8f0 : index == Int32(2) ? 0.8f0 : 0.0f0
    y = index == Int32(3) ? 0.8f0 : -0.8f0
    Lava.set_position!(GeometryBasics.Vec4f(x, y, 0.5f0, 1.0f0))
    return nothing
end

function _wge_window_probe_fragment()
    Lava.gfx_output(0, GeometryBasics.Vec4f(0.15f0, 0.75f0, 1.0f0, 1.0f0))
    return nothing
end

function _wge_window_probe_payload(outcome, phase, detail, successful_present_count,
                                   framebuffer_width, framebuffer_height, device_name)
    return (
        protocol=WGE_WINDOW_PROBE_PROTOCOL,
        outcome=outcome,
        phase=phase,
        detail=detail,
        backend_id="lava-vulkan",
        backend_revision=WGE_LAVA_REVISION,
        runtime_version=string(VERSION),
        device_name=device_name,
        framebuffer_width_px=framebuffer_width,
        framebuffer_height_px=framebuffer_height,
        successful_present_count=successful_present_count,
    )
end

function _wge_run_window_probe()
    width = parse(Int, ARGS[1])
    height = parse(Int, ARGS[2])
    vsync = ARGS[3] == "true"
    phase = "runtime"
    win = nothing
    context = nothing
    device_name = ""
    framebuffer_width = 0
    framebuffer_height = 0
    successful_present_count = 0
    result = nothing

    try
        context = Lava.vk_context()
        phase = "window_surface_swapchain"
        win = Lava.RenderWindow(width, height;
            title="WGE window capability probe", vsync, ctx=context)
        framebuffer_width, framebuffer_height = size(win)
        device_name = context.device_name
        bq = context.default_bq
        pipeline = Lava.Rasterizer(
            vertex=_wge_window_probe_vertex,
            fragment=_wge_window_probe_fragment,
            cull=Lava.NoCull(),
            depth=Lava.DepthOff(),
        )

        for _ in 1:3
            phase = "image_acquisition"
            Lava.acquire_next_image!(win)
            phase = "draw_recording"
            Lava.draw!(bq, pipeline, Lava.WindowTarget(win), 3;
                clear_color=(0.02f0, 0.03f0, 0.05f0, 1.0f0))

            frame_slots = length(win.in_flight)
            if frame_slots < 2
                phase = "presentation"
                error("Lava frame rotation cannot independently prove presentation with fewer than two in-flight slots")
            end
            prior_frame = win.current_frame
            phase = "presentation"
            Lava.present_frame!(bq, win)
            if win.current_frame == mod1(prior_frame + 1, frame_slots)
                successful_present_count = 1
                result = _wge_window_probe_payload(
                    "present_verified", "complete", nothing,
                    successful_present_count, framebuffer_width,
                    framebuffer_height, device_name)
                break
            end
        end

        if result === nothing
            result = _wge_window_probe_payload(
                "failed", "presentation",
                "Lava did not advance its frame slot after three present attempts; no successful present is claimed",
                successful_present_count, framebuffer_width,
                framebuffer_height, device_name)
        end
    catch error
        result = _wge_window_probe_payload(
            "failed", phase, sprint(showerror, error),
            successful_present_count, framebuffer_width,
            framebuffer_height, device_name)
    finally
        if win !== nothing
            try
                close(win)
            catch error
                result = _wge_window_probe_payload(
                    "failed", "cleanup", sprint(showerror, error),
                    successful_present_count, framebuffer_width,
                    framebuffer_height, device_name)
            end
        end
    end

    result === nothing && error("window probe produced no typed result")
    println("WGE_WINDOW_PROBE=", JSON3.write(result))
    flush(stdout)
    return nothing
end

_wge_run_window_probe()
"#;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WindowTargetRequest {
    pub schema_version: String,
    pub target_id: String,
    pub width_px: u32,
    pub height_px: u32,
    pub vsync: bool,
}

impl WindowTargetRequest {
    pub fn new(
        target_id: impl Into<String>,
        width_px: u32,
        height_px: u32,
        vsync: bool,
    ) -> Result<Self, WindowContractError> {
        let request = Self {
            schema_version: WINDOW_TARGET_REQUEST_SCHEMA.to_owned(),
            target_id: target_id.into(),
            width_px,
            height_px,
            vsync,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), WindowContractError> {
        if self.schema_version != WINDOW_TARGET_REQUEST_SCHEMA {
            return Err(WindowContractError::new(
                "schema_mismatch",
                "window target request schema is not supported",
            ));
        }
        if self.target_id.trim().is_empty() || self.target_id.len() > 128 {
            return Err(WindowContractError::new(
                "invalid_target_id",
                "target_id must contain 1 to 128 non-whitespace bytes",
            ));
        }
        if !(1..=MAX_CAPTURE_DIMENSION).contains(&self.width_px)
            || !(1..=MAX_CAPTURE_DIMENSION).contains(&self.height_px)
        {
            return Err(WindowContractError::new(
                "invalid_dimensions",
                format!("window dimensions must be in 1..={MAX_CAPTURE_DIMENSION}"),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WindowProbeStatus {
    PresentVerified,
    Unavailable,
    Failed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WindowProbeStage {
    Runtime,
    Display,
    WindowSurfaceSwapchain,
    ImageAcquisition,
    DrawRecording,
    Presentation,
    Cleanup,
    Complete,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Available,
    Unavailable,
    Failed,
    NotExercised,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WindowCapabilities {
    pub julia_runtime: CapabilityState,
    pub display_connection: CapabilityState,
    pub native_window: CapabilityState,
    pub vulkan_surface: CapabilityState,
    pub swapchain: CapabilityState,
    pub image_acquisition: CapabilityState,
    pub draw_recording: CapabilityState,
    pub presentation: CapabilityState,
}

impl WindowCapabilities {
    fn not_exercised() -> Self {
        Self {
            julia_runtime: CapabilityState::NotExercised,
            display_connection: CapabilityState::NotExercised,
            native_window: CapabilityState::NotExercised,
            vulkan_surface: CapabilityState::NotExercised,
            swapchain: CapabilityState::NotExercised,
            image_acquisition: CapabilityState::NotExercised,
            draw_recording: CapabilityState::NotExercised,
            presentation: CapabilityState::NotExercised,
        }
    }

    fn all_available() -> Self {
        Self {
            julia_runtime: CapabilityState::Available,
            display_connection: CapabilityState::Available,
            native_window: CapabilityState::Available,
            vulkan_surface: CapabilityState::Available,
            swapchain: CapabilityState::Available,
            image_acquisition: CapabilityState::Available,
            draw_recording: CapabilityState::Available,
            presentation: CapabilityState::Available,
        }
    }

    fn all_are(&self, expected: CapabilityState) -> bool {
        [
            self.julia_runtime,
            self.display_connection,
            self.native_window,
            self.vulkan_surface,
            self.swapchain,
            self.image_acquisition,
            self.draw_recording,
            self.presentation,
        ]
        .into_iter()
        .all(|state| state == expected)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WindowBackendEvidence {
    pub backend_id: String,
    pub backend_revision: String,
    pub julia_version: String,
    pub device_name: String,
    pub framebuffer_width_px: u32,
    pub framebuffer_height_px: u32,
    pub successful_present_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WindowProbeReceipt {
    pub schema_version: String,
    pub request: WindowTargetRequest,
    pub status: WindowProbeStatus,
    pub stage: WindowProbeStage,
    pub capabilities: WindowCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<WindowBackendEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl WindowProbeReceipt {
    pub fn validate(&self) -> Result<(), WindowContractError> {
        self.request.validate()?;
        if self.schema_version != WINDOW_PROBE_RECEIPT_SCHEMA {
            return Err(WindowContractError::new(
                "schema_mismatch",
                "window probe receipt schema is not supported",
            ));
        }
        if self
            .detail
            .as_ref()
            .is_some_and(|detail| detail.len() > 4096)
        {
            return Err(WindowContractError::new(
                "detail_too_long",
                "window probe detail exceeds the receipt limit",
            ));
        }
        match self.status {
            WindowProbeStatus::PresentVerified => {
                if self.stage != WindowProbeStage::Complete
                    || !self.capabilities.all_are(CapabilityState::Available)
                    || self.detail.is_some()
                {
                    return Err(WindowContractError::new(
                        "incomplete_present_evidence",
                        "verified presentation requires every capability and the complete stage",
                    ));
                }
                let evidence = self.evidence.as_ref().ok_or_else(|| {
                    WindowContractError::new(
                        "missing_present_evidence",
                        "verified presentation requires native backend evidence",
                    )
                })?;
                validate_backend_evidence(evidence)?;
                if evidence.framebuffer_width_px != self.request.width_px
                    || evidence.framebuffer_height_px != self.request.height_px
                {
                    return Err(WindowContractError::new(
                        "present_extent_mismatch",
                        "native framebuffer extent does not match the requested window extent",
                    ));
                }
            }
            WindowProbeStatus::Unavailable => {
                let expected = match self.stage {
                    WindowProbeStage::Runtime => Some(WindowCapabilities {
                        julia_runtime: CapabilityState::Unavailable,
                        ..WindowCapabilities::not_exercised()
                    }),
                    WindowProbeStage::Display => Some(WindowCapabilities {
                        display_connection: CapabilityState::Unavailable,
                        ..WindowCapabilities::not_exercised()
                    }),
                    _ => None,
                };
                if expected
                    .as_ref()
                    .is_none_or(|expected| self.capabilities != *expected)
                    || self.evidence.is_some()
                    || self.detail.as_ref().is_none_or(String::is_empty)
                {
                    return Err(WindowContractError::new(
                        "invalid_unavailable_result",
                        "unavailable must identify a runtime/display limitation and cannot carry a present claim",
                    ));
                }
            }
            WindowProbeStatus::Failed => {
                if self.stage == WindowProbeStage::Complete
                    || self.detail.as_ref().is_none_or(String::is_empty)
                {
                    return Err(WindowContractError::new(
                        "invalid_failed_result",
                        "failed probes require a non-complete stage and diagnostic detail",
                    ));
                }
                if let Some(evidence) = &self.evidence {
                    validate_backend_evidence(evidence)?;
                    if self.stage != WindowProbeStage::Cleanup
                        || self.capabilities != WindowCapabilities::all_available()
                    {
                        return Err(WindowContractError::new(
                            "unexpected_partial_present",
                            "partial present evidence is allowed only when cleanup failed after a verified present",
                        ));
                    }
                } else {
                    let expected = if self.stage == WindowProbeStage::Cleanup {
                        WindowCapabilities::not_exercised()
                    } else {
                        capabilities_through(self.stage, true)
                    };
                    if self.capabilities != expected {
                        return Err(WindowContractError::new(
                            "inconsistent_failure_capabilities",
                            "failure capability states do not match the stage reached",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn is_present_verified(&self) -> bool {
        self.validate().is_ok() && self.status == WindowProbeStatus::PresentVerified
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowContractError {
    pub code: &'static str,
    pub message: String,
}

impl WindowContractError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for WindowContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WindowContractError {}

/// Exercise a real Lava `RenderWindow`/Vulkan swapchain and present one frame.
///
/// This is an isolated capability smoke test. It does not turn the existing
/// offscreen worker into a window renderer and does not produce a certification
/// receipt. The returned status is `PresentVerified` only after the native
/// `present_frame!` call advances Lava's in-flight frame slot.
pub fn probe_lava_window(
    request: &WindowTargetRequest,
    julia_executable: &OsStr,
    julia_project: &Path,
    timeout: Duration,
) -> Result<WindowProbeReceipt, WindowContractError> {
    request.validate()?;
    if timeout.is_zero() || timeout > MAX_PROBE_TIMEOUT {
        return Err(WindowContractError::new(
            "invalid_timeout",
            "probe timeout must be greater than zero and at most 600 seconds",
        ));
    }

    #[cfg(target_os = "linux")]
    if !has_linux_display() {
        return Ok(unavailable_receipt(
            request,
            WindowProbeStage::Display,
            "neither WAYLAND_DISPLAY nor DISPLAY is set; a native window cannot be opened in this session",
        ));
    }

    if !julia_project.is_dir() {
        return Ok(unavailable_receipt(
            request,
            WindowProbeStage::Runtime,
            "the supplied Julia project directory does not exist",
        ));
    }

    let mut child = match Command::new(julia_executable)
        .arg(format!("--project={}", julia_project.display()))
        .arg("--startup-file=no")
        .arg("-e")
        .arg(LAVA_WINDOW_PROBE)
        .arg("--")
        .arg(request.width_px.to_string())
        .arg(request.height_px.to_string())
        .arg(if request.vsync { "true" } else { "false" })
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(unavailable_receipt(
                request,
                WindowProbeStage::Runtime,
                format!("Julia executable is unavailable: {error}"),
            ));
        }
        Err(error) => {
            return Ok(failed_receipt(
                request,
                WindowProbeStage::Runtime,
                format!("could not start Julia: {error}"),
                WindowCapabilities {
                    julia_runtime: CapabilityState::Failed,
                    ..WindowCapabilities::not_exercised()
                },
                None,
            ));
        }
    };

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(failed_receipt(
            request,
            WindowProbeStage::Runtime,
            "Julia stdout pipe was not created",
            WindowCapabilities {
                julia_runtime: CapabilityState::Failed,
                ..WindowCapabilities::not_exercised()
            },
            None,
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(failed_receipt(
            request,
            WindowProbeStage::Runtime,
            "Julia stderr pipe was not created",
            WindowCapabilities {
                julia_runtime: CapabilityState::Failed,
                ..WindowCapabilities::not_exercised()
            },
            None,
        ));
    };
    let stdout_reader = thread::spawn(move || read_bounded(stdout));
    let stderr_reader = thread::spawn(move || read_bounded(stderr));
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            Err(_) => {
                let _ = child.kill();
                break child.wait().ok();
            }
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    if timed_out {
        return Ok(failed_receipt(
            request,
            WindowProbeStage::Runtime,
            format!("Julia window probe exceeded its {timeout:?} time limit"),
            WindowCapabilities {
                julia_runtime: CapabilityState::Failed,
                ..WindowCapabilities::not_exercised()
            },
            None,
        ));
    }

    let mut receipt = parse_probe_output(request, &stdout, &stderr);
    if !exit_status.is_some_and(|status| status.success()) {
        let detail = format!(
            "Julia exited unsuccessfully (status {:?}); {}",
            exit_status.and_then(|status| status.code()),
            diagnostic_text(&stderr)
        );
        if receipt.status == WindowProbeStatus::PresentVerified {
            receipt = failed_receipt(
                request,
                WindowProbeStage::Cleanup,
                detail,
                WindowCapabilities::all_available(),
                receipt.evidence,
            );
        } else {
            receipt.status = WindowProbeStatus::Failed;
            receipt.detail = Some(detail);
        }
    }
    receipt.validate()?;
    Ok(receipt)
}

fn parse_probe_output(
    request: &WindowTargetRequest,
    stdout: &[u8],
    stderr: &[u8],
) -> WindowProbeReceipt {
    let output = String::from_utf8_lossy(stdout);
    let Some(line) = output
        .lines()
        .find_map(|line| line.strip_prefix(PROBE_OUTPUT_PREFIX))
    else {
        return failed_receipt(
            request,
            WindowProbeStage::Runtime,
            format!(
                "Julia did not emit the typed probe protocol; {}",
                diagnostic_text(stderr)
            ),
            WindowCapabilities {
                julia_runtime: CapabilityState::Failed,
                ..WindowCapabilities::not_exercised()
            },
            None,
        );
    };
    let payload = match serde_json::from_str::<NativeProbePayload>(line) {
        Ok(payload) => payload,
        Err(error) => {
            return failed_receipt(
                request,
                WindowProbeStage::Runtime,
                format!("Julia probe emitted malformed JSON: {error}"),
                WindowCapabilities {
                    julia_runtime: CapabilityState::Failed,
                    ..WindowCapabilities::not_exercised()
                },
                None,
            );
        }
    };
    if payload.protocol != JULIA_PROBE_PROTOCOL {
        return failed_receipt(
            request,
            WindowProbeStage::Runtime,
            "Julia probe protocol identifier is stale or unknown",
            WindowCapabilities {
                julia_runtime: CapabilityState::Failed,
                ..WindowCapabilities::not_exercised()
            },
            None,
        );
    }

    let evidence = payload.to_evidence();
    match payload.outcome.as_str() {
        "present_verified" => {
            let Some(evidence) = evidence else {
                return failed_receipt(
                    request,
                    WindowProbeStage::Presentation,
                    "Julia reported presentation without complete backend evidence",
                    capabilities_through(WindowProbeStage::Presentation, true),
                    None,
                );
            };
            let receipt = WindowProbeReceipt {
                schema_version: WINDOW_PROBE_RECEIPT_SCHEMA.to_owned(),
                request: request.clone(),
                status: WindowProbeStatus::PresentVerified,
                stage: WindowProbeStage::Complete,
                capabilities: WindowCapabilities::all_available(),
                evidence: Some(evidence),
                detail: None,
            };
            if let Err(error) = receipt.validate() {
                return failed_receipt(
                    request,
                    WindowProbeStage::Presentation,
                    format!("Rust rejected the native present claim: {error}"),
                    capabilities_through(WindowProbeStage::Presentation, true),
                    None,
                );
            }
            receipt
        }
        "failed" => {
            let Some(stage) = parse_stage(&payload.phase) else {
                return failed_receipt(
                    request,
                    WindowProbeStage::Runtime,
                    "Julia probe reported an unknown failure stage",
                    WindowCapabilities {
                        julia_runtime: CapabilityState::Failed,
                        ..WindowCapabilities::not_exercised()
                    },
                    None,
                );
            };
            let capabilities = capabilities_through(stage, true);
            let partial_evidence = if stage == WindowProbeStage::Cleanup {
                evidence
            } else {
                None
            };
            failed_receipt(
                request,
                stage,
                payload
                    .detail
                    .unwrap_or_else(|| "Lava probe failed without diagnostic detail".to_owned()),
                if partial_evidence.is_some() {
                    WindowCapabilities::all_available()
                } else if stage == WindowProbeStage::Cleanup {
                    WindowCapabilities::not_exercised()
                } else {
                    capabilities
                },
                partial_evidence,
            )
        }
        _ => failed_receipt(
            request,
            WindowProbeStage::Runtime,
            "Julia probe returned an unsupported outcome tag",
            WindowCapabilities {
                julia_runtime: CapabilityState::Failed,
                ..WindowCapabilities::not_exercised()
            },
            None,
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeProbePayload {
    protocol: String,
    outcome: String,
    phase: String,
    detail: Option<String>,
    backend_id: Option<String>,
    backend_revision: Option<String>,
    runtime_version: Option<String>,
    device_name: Option<String>,
    framebuffer_width_px: Option<u32>,
    framebuffer_height_px: Option<u32>,
    successful_present_count: Option<u32>,
}

impl NativeProbePayload {
    fn to_evidence(&self) -> Option<WindowBackendEvidence> {
        Some(WindowBackendEvidence {
            backend_id: self.backend_id.clone()?,
            backend_revision: self.backend_revision.clone()?,
            julia_version: self.runtime_version.clone()?,
            device_name: self.device_name.clone()?,
            framebuffer_width_px: self.framebuffer_width_px?,
            framebuffer_height_px: self.framebuffer_height_px?,
            successful_present_count: self.successful_present_count?,
        })
    }
}

fn validate_backend_evidence(evidence: &WindowBackendEvidence) -> Result<(), WindowContractError> {
    if evidence.backend_id != LAVA_BACKEND_ID || evidence.backend_revision != LAVA_REVISION {
        return Err(WindowContractError::new(
            "backend_identity_mismatch",
            "native evidence does not match the pinned Lava backend identity",
        ));
    }
    if evidence.julia_version.trim().is_empty() || evidence.device_name.trim().is_empty() {
        return Err(WindowContractError::new(
            "incomplete_backend_identity",
            "native evidence must identify the Julia runtime and Vulkan device",
        ));
    }
    if !(1..=MAX_CAPTURE_DIMENSION).contains(&evidence.framebuffer_width_px)
        || !(1..=MAX_CAPTURE_DIMENSION).contains(&evidence.framebuffer_height_px)
        || evidence.successful_present_count != 1
    {
        return Err(WindowContractError::new(
            "invalid_present_measurement",
            "present evidence has invalid framebuffer dimensions or does not prove exactly one successful present",
        ));
    }
    Ok(())
}

fn capabilities_through(stage: WindowProbeStage, failed: bool) -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::not_exercised();
    match stage {
        WindowProbeStage::Runtime => {
            capabilities.julia_runtime = if failed {
                CapabilityState::Failed
            } else {
                CapabilityState::Available
            };
        }
        WindowProbeStage::Display => {
            capabilities.julia_runtime = CapabilityState::Available;
            capabilities.display_connection = if failed {
                CapabilityState::Failed
            } else {
                CapabilityState::Available
            };
        }
        WindowProbeStage::WindowSurfaceSwapchain => {
            capabilities.julia_runtime = CapabilityState::Available;
            // Lava's RenderWindow constructor creates all three native objects
            // in one call; on constructor failure the sub-capability is unknown.
        }
        WindowProbeStage::ImageAcquisition => {
            capabilities.julia_runtime = CapabilityState::Available;
            capabilities.display_connection = CapabilityState::Available;
            capabilities.native_window = CapabilityState::Available;
            capabilities.vulkan_surface = CapabilityState::Available;
            capabilities.swapchain = CapabilityState::Available;
            capabilities.image_acquisition = if failed {
                CapabilityState::Failed
            } else {
                CapabilityState::Available
            };
        }
        WindowProbeStage::DrawRecording => {
            capabilities = capabilities_through(WindowProbeStage::ImageAcquisition, false);
            capabilities.draw_recording = if failed {
                CapabilityState::Failed
            } else {
                CapabilityState::Available
            };
        }
        WindowProbeStage::Presentation => {
            capabilities = capabilities_through(WindowProbeStage::DrawRecording, false);
            capabilities.presentation = if failed {
                CapabilityState::Failed
            } else {
                CapabilityState::Available
            };
        }
        WindowProbeStage::Cleanup | WindowProbeStage::Complete => {
            capabilities = WindowCapabilities::all_available();
        }
    }
    capabilities
}

fn parse_stage(phase: &str) -> Option<WindowProbeStage> {
    match phase {
        "runtime" => Some(WindowProbeStage::Runtime),
        "window_surface_swapchain" => Some(WindowProbeStage::WindowSurfaceSwapchain),
        "image_acquisition" => Some(WindowProbeStage::ImageAcquisition),
        "draw_recording" => Some(WindowProbeStage::DrawRecording),
        "presentation" => Some(WindowProbeStage::Presentation),
        "cleanup" => Some(WindowProbeStage::Cleanup),
        "complete" => Some(WindowProbeStage::Complete),
        _ => None,
    }
}

fn unavailable_receipt(
    request: &WindowTargetRequest,
    stage: WindowProbeStage,
    detail: impl Into<String>,
) -> WindowProbeReceipt {
    let mut capabilities = WindowCapabilities::not_exercised();
    match stage {
        WindowProbeStage::Runtime => capabilities.julia_runtime = CapabilityState::Unavailable,
        WindowProbeStage::Display => {
            capabilities.display_connection = CapabilityState::Unavailable;
        }
        _ => unreachable!("only runtime or display preflight can be unavailable"),
    }
    WindowProbeReceipt {
        schema_version: WINDOW_PROBE_RECEIPT_SCHEMA.to_owned(),
        request: request.clone(),
        status: WindowProbeStatus::Unavailable,
        stage,
        capabilities,
        evidence: None,
        detail: Some(detail.into()),
    }
}

fn failed_receipt(
    request: &WindowTargetRequest,
    stage: WindowProbeStage,
    detail: impl Into<String>,
    capabilities: WindowCapabilities,
    evidence: Option<WindowBackendEvidence>,
) -> WindowProbeReceipt {
    WindowProbeReceipt {
        schema_version: WINDOW_PROBE_RECEIPT_SCHEMA.to_owned(),
        request: request.clone(),
        status: WindowProbeStatus::Failed,
        stage,
        capabilities,
        evidence,
        detail: Some(detail.into()),
    }
}

fn read_bounded(mut reader: impl Read) -> Vec<u8> {
    let mut output = Vec::with_capacity(MAX_PROCESS_OUTPUT_BYTES);
    let mut buffer = [0_u8; 4096];
    while let Ok(bytes_read) = reader.read(&mut buffer) {
        if bytes_read == 0 {
            break;
        }
        let available = MAX_PROCESS_OUTPUT_BYTES.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..bytes_read.min(available)]);
    }
    output
}

fn diagnostic_text(stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr).trim().to_owned();
    if detail.is_empty() {
        "Julia emitted no stderr detail".to_owned()
    } else {
        detail.chars().take(2048).collect()
    }
}

#[cfg(target_os = "linux")]
fn has_linux_display() -> bool {
    ["WAYLAND_DISPLAY", "DISPLAY"]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}
