use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use wge_native_graphics_contract::{
    GraphicsReady, GraphicsWorkerSupervisor, lower_reference_world, validate_frame_receipt,
    validate_ready, validate_scene_packet,
};
use wge_reference_runtime::build_from_layout_path;

fn gpu_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static GPU_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    GPU_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("GPU test lock is not poisoned")
}

fn julia_executable() -> PathBuf {
    std::env::var_os("WGE_JULIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("julia"))
}

#[test]
fn certified_reference_world_lowers_to_a_valid_coarse_packet() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let layout = manifest_dir
        .join("../reference_runtime/examples/riverwatch.layout.json")
        .canonicalize()
        .expect("reference layout exists");
    let terrain_lab = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace")
        .join("terrain_lab");
    let output_dir = std::env::temp_dir().join(format!(
        "wge-native-graphics-contract-{}",
        std::process::id()
    ));
    fs::create_dir_all(&output_dir).expect("test output directory is writable");
    let input = output_dir.join("layout.json");
    fs::copy(&layout, &input).expect("test layout copies");

    let build = build_from_layout_path(&input, &julia_executable(), &terrain_lab)
        .expect("reference world builds");
    let packet = lower_reference_world(&build.world).expect("world lowers to graphics packet");
    assert_eq!(packet.body.meshes.len(), 1);
    assert_eq!(
        packet.body.instances.len(),
        build.world.body.authored_layout.obstacles.len()
    );
    validate_scene_packet(&packet).expect("lowered packet validates");
    assert_eq!(packet.body.terrain.resolution, 49);
    assert_eq!(packet.body.capture.width_px, 320);
    assert!(packet.body.overlays.len() >= 4);
    assert_eq!(packet.body.terrain.heights_m.count, 49 * 49);
    assert_eq!(packet.body.terrain.region_codes.count, 49 * 49);

    fs::remove_dir_all(output_dir).expect("test output directory is removed");
}

#[test]
fn rust_packet_crosses_the_julia_protocol_boundary() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let layout = manifest_dir
        .join("../reference_runtime/examples/riverwatch.layout.json")
        .canonicalize()
        .expect("reference layout exists");
    let terrain_lab = workspace_root.join("terrain_lab");
    let graphics_lab = workspace_root.join("graphics_lab");
    let worker = graphics_lab.join("bin/wge_graphics_worker.jl");
    let output_dir = std::env::temp_dir().join(format!(
        "wge-native-graphics-protocol-{}",
        std::process::id()
    ));
    fs::create_dir_all(&output_dir).expect("test output directory is writable");
    let input = output_dir.join("layout.json");
    let packet_path = output_dir.join("packet.json");
    fs::copy(&layout, &input).expect("test layout copies");
    let build = build_from_layout_path(&input, &julia_executable(), &terrain_lab)
        .expect("reference world builds");
    let packet = lower_reference_world(&build.world).expect("world lowers to graphics packet");
    fs::write(
        &packet_path,
        serde_json::to_vec(&packet).expect("packet serializes"),
    )
    .expect("packet writes");

    let script = r#"
using JSON3
include(ENV["WGE_GRAPHICS_WORKER"])
packet = JSON3.read(read(ENV["WGE_PACKET_PATH"], String))
response = JSON3.read(handle(JSON3.write((op="validate_packet", packet=packet))))
response["kind"] == "packet_validated" || error(JSON3.write(response))
println(response["packet_sha256"])
"#;
    let output = Command::new(julia_executable())
        .arg(format!("--project={}", graphics_lab.display()))
        .arg("--startup-file=no")
        .arg("-e")
        .arg(script)
        .env("WGE_GRAPHICS_WORKER", &worker)
        .env("WGE_PACKET_PATH", &packet_path)
        .output()
        .expect("graphics Julia worker starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response_sha = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    assert_eq!(response_sha, packet.packet_sha256);
    fs::remove_dir_all(output_dir).expect("test output directory is removed");
}

#[test]
fn rust_supervisor_restarts_the_persistent_worker() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let graphics_lab = workspace_root.join("graphics_lab");
    let worker = graphics_lab.join("bin/wge_graphics_worker.jl");
    let mut supervisor =
        GraphicsWorkerSupervisor::start(julia_executable(), &graphics_lab, &worker)
            .expect("Rust supervisor starts the persistent Julia worker");
    let first_script = supervisor
        .worker_ready_message()
        .get("script_sha256")
        .and_then(serde_json::Value::as_str)
        .expect("worker ready includes script identity")
        .to_owned();
    supervisor
        .restart()
        .expect("Rust supervisor restarts the worker");
    let second_script = supervisor
        .worker_ready_message()
        .get("script_sha256")
        .and_then(serde_json::Value::as_str)
        .expect("restarted worker includes script identity");
    assert_eq!(first_script, second_script);
    assert!(supervisor.ready().is_none());
}

#[test]
fn rust_packet_renders_through_the_pinned_lava_worker() {
    let _gpu_guard = gpu_test_guard();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let layout = manifest_dir
        .join("../reference_runtime/examples/riverwatch.layout.json")
        .canonicalize()
        .expect("reference layout exists");
    let terrain_lab = workspace_root.join("terrain_lab");
    let graphics_lab = workspace_root.join("graphics_lab");
    let worker = graphics_lab.join("bin/wge_graphics_worker.jl");
    let output_dir = std::env::temp_dir().join(format!(
        "wge-native-graphics-lava-render-{}",
        std::process::id()
    ));
    fs::create_dir_all(&output_dir).expect("test output directory is writable");
    let input = output_dir.join("layout.json");
    let packet_path = output_dir.join("packet.json");
    fs::copy(&layout, &input).expect("test layout copies");
    let build = build_from_layout_path(&input, &julia_executable(), &terrain_lab)
        .expect("reference world builds");
    let packet = lower_reference_world(&build.world).expect("world lowers to graphics packet");
    fs::write(
        &packet_path,
        serde_json::to_vec(&packet).expect("packet serializes"),
    )
    .expect("packet writes");

    let script = r#"
using JSON3
include(ENV["WGE_GRAPHICS_WORKER"])
packet = JSON3.read(read(ENV["WGE_PACKET_PATH"], String))
response = JSON3.read(handle(JSON3.write((op="render_packet", packet=packet))))
response["kind"] == "frame_rendered" || error(JSON3.write(response))
println(JSON3.write(response["frame"]))
"#;
    let output = Command::new(julia_executable())
        .arg(format!("--project={}", graphics_lab.display()))
        .arg("--startup-file=no")
        .arg("-e")
        .arg(script)
        .env("WGE_GRAPHICS_WORKER", &worker)
        .env("WGE_PACKET_PATH", &packet_path)
        .output()
        .expect("graphics Julia worker starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let frame: serde_json::Value = serde_json::from_slice(&output.stdout).expect("frame JSON");
    assert_eq!(frame["schema"], "wge.lava-frame/v1");
    assert_eq!(frame["packet_sha256"], packet.packet_sha256);
    assert_eq!(frame["width_px"], packet.body.capture.width_px);
    assert_eq!(frame["height_px"], packet.body.capture.height_px);
    assert!(
        frame["capture_sha256"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert!(frame["capture_base64"].as_str().unwrap().len() > 1024);
    assert!(
        frame["measurements"]["distinct_terrain_colors"]
            .as_u64()
            .unwrap()
            > 1
    );
    assert!(
        frame["measurements"]["route_visible_pixels"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(frame["telemetry"]["draw_calls"].as_u64().unwrap() >= 3);

    fs::remove_dir_all(output_dir).expect("test output directory is removed");
}

#[test]
fn pinned_lava_capabilities_promote_through_rust_validation() {
    let _gpu_guard = gpu_test_guard();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let graphics_lab = workspace_root.join("graphics_lab");
    let worker = graphics_lab.join("bin/wge_graphics_worker.jl");
    let script = r#"
using JSON3
include(ENV["WGE_GRAPHICS_WORKER"])
response = JSON3.read(handle(JSON3.write((op="probe_capabilities",))))
response["kind"] == "capabilities_probed" || error(JSON3.write(response))
println(JSON3.write(response["ready"]))
"#;
    let output = Command::new(julia_executable())
        .arg(format!("--project={}", graphics_lab.display()))
        .arg("--startup-file=no")
        .arg("-e")
        .arg(script)
        .env("WGE_GRAPHICS_WORKER", &worker)
        .output()
        .expect("graphics Julia worker starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ready: GraphicsReady = serde_json::from_slice(&output.stdout).expect("ready JSON");
    validate_ready(&ready).expect("Rust independently validates native capabilities");
    assert!(ready.features.offscreen_raster);
    assert!(ready.features.depth_attachment);
    assert!(ready.features.texture_sampling);
    assert!(ready.features.readback);
    assert!(!ready.device_uuid.is_empty());
}

#[test]
fn rust_supervisor_promotes_a_bound_lava_frame() {
    let _gpu_guard = gpu_test_guard();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let layout = manifest_dir
        .join("../reference_runtime/examples/riverwatch.layout.json")
        .canonicalize()
        .expect("reference layout exists");
    let terrain_lab = workspace_root.join("terrain_lab");
    let graphics_lab = workspace_root.join("graphics_lab");
    let worker = graphics_lab.join("bin/wge_graphics_worker.jl");
    let output_dir = std::env::temp_dir().join(format!(
        "wge-native-graphics-supervisor-{}",
        std::process::id()
    ));
    fs::create_dir_all(&output_dir).expect("test output directory is writable");
    let input = output_dir.join("layout.json");
    fs::copy(&layout, &input).expect("test layout copies");

    let build = build_from_layout_path(&input, &julia_executable(), &terrain_lab)
        .expect("reference world builds");
    let packet = lower_reference_world(&build.world).expect("world lowers to graphics packet");
    let mut supervisor =
        GraphicsWorkerSupervisor::start(julia_executable(), &graphics_lab, &worker)
            .expect("Rust supervisor starts the persistent Julia worker");
    let ready = supervisor
        .capabilities()
        .expect("Rust supervisor validates capabilities");
    assert!(ready.features.offscreen_raster);
    assert!(ready.features.depth_attachment);
    assert!(ready.features.texture_sampling);
    assert!(ready.features.readback);

    let promoted = supervisor
        .render_and_promote(&packet)
        .expect("Rust supervisor promotes the Lava frame");
    validate_frame_receipt(&promoted.receipt, &promoted.capture_bytes)
        .expect("promoted receipt independently validates");
    assert_eq!(promoted.frame.packet_sha256, packet.packet_sha256);
    assert_eq!(promoted.receipt.body.packet_sha256, packet.packet_sha256);
    assert_eq!(
        promoted.receipt.body.capture_id,
        packet.body.capture.capture_id
    );
    assert_eq!(
        promoted.receipt.body.adapter_revision,
        promoted.frame.adapter_revision
    );
    assert_eq!(
        promoted.receipt.body.lava_revision,
        promoted.frame.lava_revision
    );
    assert!(!promoted.receipt.body.device_uuid.is_empty());
    assert!(
        promoted
            .receipt
            .body
            .renderer_identity_sha256
            .starts_with("sha256:")
    );
    assert!(
        promoted
            .receipt
            .body
            .worker_script_sha256
            .starts_with("sha256:")
    );
    assert_eq!(
        promoted.capture_bytes.len(),
        promoted.frame.width_px as usize * promoted.frame.height_px as usize * 4
    );
    let first_capture = promoted.capture_bytes.clone();
    let first_measurements = promoted.receipt.body.measurements.clone();
    supervisor
        .restart()
        .expect("supervisor can reset the graphics process");
    supervisor
        .capabilities()
        .expect("restarted process revalidates capabilities");
    let replayed = supervisor
        .render_and_promote(&packet)
        .expect("restarted process promotes the same frame");
    assert_eq!(replayed.capture_bytes, first_capture);
    assert_eq!(replayed.receipt.body.measurements, first_measurements);

    fs::remove_dir_all(output_dir).expect("test output directory is removed");
}
