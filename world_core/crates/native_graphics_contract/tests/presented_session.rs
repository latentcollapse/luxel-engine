//! End-to-end presented-session test: a persistent native window presents
//! continuous frames of a Rust-driven session while Tier-A evidence is
//! independently promoted through the offscreen authority path.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use wge_native_graphics_contract::session::{PresentedGraphicsSession, PresentedSessionRequest};
use wge_native_graphics_contract::{
    GraphicsWorkerSupervisor, lower_reference_world, lower_showcase_packet,
};
use wge_reference_runtime::build_from_layout_path;

fn julia_executable() -> PathBuf {
    std::env::var_os("WGE_JULIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("julia"))
}

fn graphics_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("graphics test lock is not poisoned")
}

#[test]
fn presented_session_presents_frames_and_samples_tier_a_evidence() {
    let _guard = graphics_test_guard();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let layout = manifest_dir
        .join("../reference_runtime/examples/riverwatch.layout.json")
        .canonicalize()
        .expect("reference layout exists");
    let graphics_lab = workspace_root.join("graphics_lab");
    let terrain_lab = workspace_root.join("terrain_lab");
    let worker = graphics_lab.join("bin/wge_graphics_worker.jl");
    let output_dir =
        std::env::temp_dir().join(format!("wge-presented-session-{}", std::process::id()));
    fs::create_dir_all(&output_dir).expect("test output directory is writable");
    let input = output_dir.join("riverwatch.layout.json");
    fs::copy(&layout, &input).expect("layout copies into temp fixture directory");

    let build = build_from_layout_path(&input, &julia_executable(), &terrain_lab)
        .expect("Julia-backed reference world builds");
    let world = build.world;
    let base_packet = lower_reference_world(&world).expect("world lowers to scene packet");
    // The showcase view is a proven authorized projection; use its camera as
    // the presented camera so the session window extent matches the packet.
    let packet = lower_showcase_packet(&base_packet).expect("showcase packet is authorized");
    let camera = &packet.body.camera;

    let request = PresentedSessionRequest::new(
        "presented-session-test",
        camera.width_px,
        camera.height_px,
        false,
    )
    .expect("typed presented-session request");
    let mut session =
        PresentedGraphicsSession::new(&request, &packet, &world).expect("typed session binds");

    let mut supervisor =
        GraphicsWorkerSupervisor::start(julia_executable(), &graphics_lab, &worker)
            .expect("Rust supervisor starts the Julia graphics worker");
    supervisor
        .capabilities()
        .expect("capabilities are validated before the session starts");

    let receipt = session
        .open_window(&mut supervisor)
        .expect("persistent window opens on the shared worker device");
    assert_eq!(receipt.window_width_px, camera.width_px);
    assert_eq!(receipt.window_height_px, camera.height_px);
    assert!(session.is_window_open());

    // Warm every pipeline once offscreen so the presented loop measures steady
    // state rather than lazy compilation.
    supervisor
        .render_and_promote(&packet, &world)
        .expect("warmup frame promotes");

    let report = session
        .present_frames(&mut supervisor, 12)
        .expect("continuous presented frames");
    assert_eq!(report.frames_presented, 12);
    assert!(report.window_presented_frames >= 12);
    assert_eq!(report.frame_times_us.len(), 12);
    let p95 = report
        .frame_time_us_at_percentile(95.0)
        .expect("p95 of a non-empty batch");
    assert!(p95 > 0);

    // Tier-A evidence stays Rust-owned: capture the CURRENT session camera
    // through the independent offscreen promotion path.
    session
        .set_camera(
            [
                camera.position_xyz_m[0] + 2.0,
                camera.position_xyz_m[1] + 2.0,
                camera.position_xyz_m[2],
            ],
            camera.forward_xyz,
        )
        .expect("session camera moves");
    let promoted = session
        .capture_and_promote(&mut supervisor, &world)
        .expect("Tier-A snapshot promotes through the offscreen authority path");
    assert_eq!(
        promoted.receipt.body.packet_sha256,
        promoted.frame.packet_sha256
    );
    assert_eq!(
        promoted.receipt.body.status,
        wge_native_graphics_contract::FrameStatus::Passed
    );

    // The presented loop continues on the same window after evidence sampling.
    let second = session
        .present_frames(&mut supervisor, 4)
        .expect("presenting continues after evidence sampling");
    assert_eq!(second.frames_presented, 4);

    let presented_total = session
        .close_window(&mut supervisor)
        .expect("window closes cleanly");
    assert!(presented_total >= 16);
    assert!(!session.is_window_open());

    // Closing twice is honest, not a fault.
    let again = session
        .close_window(&mut supervisor)
        .expect("closing an already-closed window is honest");
    assert_eq!(again, presented_total);

    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn presented_session_rejects_presenting_without_an_open_window() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let layout = manifest_dir
        .join("../reference_runtime/examples/riverwatch.layout.json")
        .canonicalize()
        .expect("reference layout exists");
    let graphics_lab = workspace_root.join("graphics_lab");
    let terrain_lab = workspace_root.join("terrain_lab");
    let worker = graphics_lab.join("bin/wge_graphics_worker.jl");
    let input = std::env::temp_dir().join(format!("wge-session-negative-{}", std::process::id()));
    fs::create_dir_all(&input).expect("fixture dir is writable");
    let layout_path = input.join("riverwatch.layout.json");
    fs::copy(&layout, &layout_path).expect("layout copies");

    let build = build_from_layout_path(&layout_path, &julia_executable(), &terrain_lab)
        .expect("Julia-backed reference world builds");
    let world = build.world;
    let base_packet = lower_reference_world(&world).expect("world lowers");
    let packet = lower_showcase_packet(&base_packet).expect("showcase packet is authorized");
    let camera = &packet.body.camera;
    let request =
        PresentedSessionRequest::new("negative-session", camera.width_px, camera.height_px, false)
            .expect("typed request");
    let mut session =
        PresentedGraphicsSession::new(&request, &packet, &world).expect("typed session");

    // A live worker transport exists, but the session still refuses to present
    // before the window is opened: the guard is the session's, not the
    // transport's.
    let mut supervisor =
        GraphicsWorkerSupervisor::start(julia_executable(), &graphics_lab, &worker)
            .expect("Rust supervisor starts the Julia graphics worker");
    let error = session
        .present_frames(&mut supervisor, 1)
        .expect_err("presenting without an open window fails closed");
    assert_eq!(error.code, "worker_protocol");
    let _ = fs::remove_dir_all(input);
}
