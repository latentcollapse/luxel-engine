use std::path::{Path, PathBuf};
use std::time::Duration;

use wge_native_graphics_contract::{
    CapabilityState, WINDOW_PROBE_RECEIPT_SCHEMA, WindowBackendEvidence, WindowCapabilities,
    WindowProbeReceipt, WindowProbeStage, WindowProbeStatus, WindowTargetRequest,
    probe_lava_window,
};

fn request() -> WindowTargetRequest {
    WindowTargetRequest::new("window-probe-test", 96, 64, false).unwrap()
}

fn available_capabilities() -> WindowCapabilities {
    WindowCapabilities {
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

fn present_receipt() -> WindowProbeReceipt {
    WindowProbeReceipt {
        schema_version: WINDOW_PROBE_RECEIPT_SCHEMA.to_owned(),
        request: request(),
        status: WindowProbeStatus::PresentVerified,
        stage: WindowProbeStage::Complete,
        capabilities: available_capabilities(),
        evidence: Some(WindowBackendEvidence {
            backend_id: "lava-vulkan".to_owned(),
            backend_revision: "11c7e31bdf62408d22bf379e9e59510f69d2103e".to_owned(),
            julia_version: "1.12.0".to_owned(),
            device_name: "test device".to_owned(),
            framebuffer_width_px: 96,
            framebuffer_height_px: 64,
            successful_present_count: 1,
        }),
        detail: None,
    }
}

#[test]
fn window_target_rejects_empty_identity_and_invalid_extent() {
    assert!(WindowTargetRequest::new("", 96, 64, false).is_err());
    assert!(WindowTargetRequest::new("valid", 0, 64, false).is_err());
    assert!(WindowTargetRequest::new("valid", 96, 8193, false).is_err());
}

#[test]
fn present_claim_requires_native_evidence_and_every_capability() {
    let mut receipt = present_receipt();
    assert!(receipt.is_present_verified());

    receipt.evidence = None;
    assert_eq!(
        receipt.validate().unwrap_err().code,
        "missing_present_evidence"
    );

    let mut receipt = present_receipt();
    receipt.capabilities.presentation = CapabilityState::NotExercised;
    assert_eq!(
        receipt.validate().unwrap_err().code,
        "incomplete_present_evidence"
    );
}

#[test]
fn pass_shaped_and_stale_backend_evidence_are_rejected() {
    let mut receipt = present_receipt();
    receipt.evidence.as_mut().unwrap().successful_present_count = 0;
    assert_eq!(
        receipt.validate().unwrap_err().code,
        "invalid_present_measurement"
    );

    let mut receipt = present_receipt();
    receipt.evidence.as_mut().unwrap().framebuffer_width_px = 128;
    assert_eq!(
        receipt.validate().unwrap_err().code,
        "present_extent_mismatch"
    );

    let mut receipt = present_receipt();
    receipt.evidence.as_mut().unwrap().successful_present_count = 2;
    assert_eq!(
        receipt.validate().unwrap_err().code,
        "invalid_present_measurement"
    );

    let mut receipt = present_receipt();
    receipt.evidence.as_mut().unwrap().backend_revision = "stale-lava".to_owned();
    assert_eq!(
        receipt.validate().unwrap_err().code,
        "backend_identity_mismatch"
    );
}

#[test]
fn unavailable_is_explicit_and_cannot_be_promoted_as_present() {
    let mut capabilities = WindowCapabilities {
        julia_runtime: CapabilityState::NotExercised,
        display_connection: CapabilityState::Unavailable,
        native_window: CapabilityState::NotExercised,
        vulkan_surface: CapabilityState::NotExercised,
        swapchain: CapabilityState::NotExercised,
        image_acquisition: CapabilityState::NotExercised,
        draw_recording: CapabilityState::NotExercised,
        presentation: CapabilityState::NotExercised,
    };
    let mut receipt = WindowProbeReceipt {
        schema_version: WINDOW_PROBE_RECEIPT_SCHEMA.to_owned(),
        request: request(),
        status: WindowProbeStatus::Unavailable,
        stage: WindowProbeStage::Display,
        capabilities: capabilities.clone(),
        evidence: None,
        detail: Some("headless session".to_owned()),
    };
    assert!(receipt.validate().is_ok());
    assert!(!receipt.is_present_verified());

    capabilities.presentation = CapabilityState::Available;
    receipt.capabilities = capabilities;
    assert_eq!(
        receipt.validate().unwrap_err().code,
        "invalid_unavailable_result"
    );
}

#[test]
fn receipt_rejects_unknown_fields_and_schema_drift() {
    let mut value = serde_json::to_value(present_receipt()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<WindowProbeReceipt>(value).is_err());

    let mut receipt = present_receipt();
    receipt.schema_version = "wge.window-probe-receipt/v0".to_owned();
    assert_eq!(receipt.validate().unwrap_err().code, "schema_mismatch");
}

#[cfg(target_os = "linux")]
#[test]
fn lava_window_target_opens_surface_swapchain_and_presents_a_real_frame() {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../graphics_lab");
    let project = project.canonicalize().expect("graphics_lab project exists");
    let julia = std::env::var_os("WGE_JULIA").unwrap_or_else(|| "julia".into());
    let display_available = ["WAYLAND_DISPLAY", "DISPLAY"]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    let receipt = probe_lava_window(
        &request(),
        julia.as_os_str(),
        Path::new(&project),
        Duration::from_secs(240),
    )
    .unwrap();
    receipt.validate().unwrap();

    if display_available {
        assert_eq!(
            receipt.status,
            WindowProbeStatus::PresentVerified,
            "{receipt:#?}"
        );
        assert!(receipt.is_present_verified());
        let evidence = receipt.evidence.as_ref().unwrap();
        assert_eq!(evidence.successful_present_count, 1);
        assert!(evidence.framebuffer_width_px > 0);
        assert!(evidence.framebuffer_height_px > 0);
        assert_eq!(
            receipt.capabilities.presentation,
            CapabilityState::Available
        );
    } else {
        assert_eq!(receipt.status, WindowProbeStatus::Unavailable);
        assert_eq!(receipt.stage, WindowProbeStage::Display);
        assert_eq!(
            receipt.capabilities.presentation,
            CapabilityState::NotExercised
        );
        assert!(!receipt.is_present_verified());
    }
}
