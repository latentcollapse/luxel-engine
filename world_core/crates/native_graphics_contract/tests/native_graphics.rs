use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use wge_native_graphics_contract::{
    ADAPTER_REVISION, GraphicsReady, GraphicsWorkerSupervisor, LAVA_REVISION,
    lower_dense_benchmark_packet, lower_objective_close_packet, lower_reference_world,
    lower_showcase_packet, seal_scene_packet, validate_frame_receipt, validate_ready,
    validate_scene_packet,
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
    assert!(packet.body.meshes.len() >= 2);
    assert!(
        packet
            .body
            .meshes
            .iter()
            .any(|mesh| mesh.mesh_id == "foliage-cross")
    );
    assert!(
        packet
            .body
            .meshes
            .iter()
            .any(|mesh| mesh.mesh_id == "objective-beacon")
    );
    let beacon = packet
        .body
        .instances
        .iter()
        .find(|instance| instance.instance_id == "objective-beacon")
        .expect("objective beacon instance exists");
    assert_eq!(
        beacon.importance,
        wge_native_graphics_contract::InstanceImportance::Landmark
    );
    assert!(packet.body.instances.len() > build.world.body.authored_layout.obstacles.len());
    assert!(packet.body.instances.iter().any(|instance| {
        matches!(
            instance.importance,
            wge_native_graphics_contract::InstanceImportance::Background
        )
    }));
    let terrain_albedo = packet
        .body
        .materials
        .iter()
        .find(|material| material.material_id == "terrain-default")
        .expect("terrain material exists")
        .texture_ids
        .first()
        .expect("terrain albedo exists");
    let obstacle_albedo = packet
        .body
        .materials
        .iter()
        .find(|material| material.material_id == "obstacle-default")
        .expect("obstacle material exists")
        .texture_ids
        .first()
        .expect("obstacle albedo exists");
    assert_ne!(terrain_albedo, obstacle_albedo);
    assert!(packet.body.textures.len() >= 7);
    validate_scene_packet(&packet).expect("lowered packet validates");
    assert_eq!(packet.body.terrain.resolution, 49);
    assert_eq!(packet.body.capture.width_px, 320);
    assert!(packet.body.overlays.len() >= 4);
    assert_eq!(packet.body.terrain.heights_m.count, 49 * 49);
    assert_eq!(packet.body.terrain.region_codes.count, 49 * 49);

    let showcase = lower_showcase_packet(&packet).expect("showcase packet seals");
    validate_scene_packet(&showcase).expect("showcase packet validates");
    assert_eq!(
        showcase.body.world_artifact_id,
        packet.body.world_artifact_id
    );
    assert_eq!(
        showcase.body.spatial_fields_sha256,
        packet.body.spatial_fields_sha256
    );
    assert_eq!(showcase.body.camera.camera_id, "native-showcase");
    assert_eq!(showcase.body.camera.width_px, 640);
    assert_eq!(showcase.body.camera.height_px, 480);
    assert!(showcase.body.overlays.is_empty());
    assert!(
        showcase
            .body
            .meshes
            .iter()
            .any(|mesh| mesh.mesh_id == "showcase-halo")
    );
    assert!(
        showcase
            .body
            .meshes
            .iter()
            .any(|mesh| mesh.mesh_id == "showcase-beacon-inlay")
    );
    assert_eq!(
        showcase
            .body
            .instances
            .iter()
            .filter(|instance| instance.instance_id.starts_with("showcase-"))
            .count(),
        8
    );

    let dense = lower_dense_benchmark_packet(&packet, 128).expect("dense benchmark packet seals");
    assert_eq!(
        dense.body.instances.len(),
        packet.body.instances.len() + 128
    );
    assert_eq!(dense.body.world_artifact_id, packet.body.world_artifact_id);
    assert_ne!(dense.packet_sha256, packet.packet_sha256);
    assert_eq!(
        dense
            .body
            .instances
            .iter()
            .filter(|instance| instance.instance_id.starts_with("benchmark-foliage-"))
            .count(),
        128
    );
    validate_scene_packet(&dense).expect("dense benchmark packet validates");

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
fn rust_supervisor_times_out_and_recovers_from_a_stalled_worker() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let graphics_lab = workspace_root.join("graphics_lab");
    let output_dir = std::env::temp_dir().join(format!(
        "wge-native-graphics-timeout-{}",
        std::process::id()
    ));
    fs::create_dir_all(&output_dir).expect("test output directory is writable");
    let worker = output_dir.join("stalled_worker.jl");
    let script = format!(
        r#"
using SHA

function write_frame(payload)
    bytes = Vector{{UInt8}}(payload)
    length_bytes = UInt32(length(bytes))
    write(stdout, UInt8[(length_bytes >> 24) & 0xff, (length_bytes >> 16) & 0xff, (length_bytes >> 8) & 0xff, length_bytes & 0xff])
    write(stdout, bytes)
    flush(stdout)
end

script_sha256 = "sha256:" * bytes2hex(sha256(read(PROGRAM_FILE)))
write_frame("{{\"schema\":\"wge.graphics-worker/v1\",\"kind\":\"ready\",\"script_sha256\":\"" * script_sha256 * "\",\"lava_revision\":\"{}\",\"adapter_revision\":\"{}\"}}")
sleep(120.0)
"#,
        LAVA_REVISION, ADAPTER_REVISION
    );
    fs::write(&worker, script).expect("stalled worker writes");

    let mut supervisor = GraphicsWorkerSupervisor::start_with_timeout(
        julia_executable(),
        &graphics_lab,
        &worker,
        Duration::from_secs(10),
    )
    .expect("stalled worker starts and emits ready");
    supervisor.set_response_timeout(Duration::from_millis(250));
    let error = supervisor
        .request(serde_json::json!({"op": "probe_capabilities"}))
        .expect_err("stalled worker must hit the response deadline");
    assert_eq!(error.code, "worker_timeout");
    assert!(supervisor.ready().is_none());

    supervisor
        .restart()
        .expect("supervisor can revive the stalled worker");
    assert!(supervisor.ready().is_none());
    drop(supervisor);
    fs::remove_dir_all(output_dir).expect("test output directory is removed");
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
    let perspective_packet_path = output_dir.join("perspective-packet.json");
    fs::copy(&layout, &input).expect("test layout copies");
    let build = build_from_layout_path(&input, &julia_executable(), &terrain_lab)
        .expect("reference world builds");
    let packet = lower_reference_world(&build.world).expect("world lowers to graphics packet");
    let mut body = packet.body.clone();
    let mut duplicate = body
        .instances
        .first()
        .cloned()
        .expect("reference scene has an instanced obstacle");
    duplicate.instance_id = "obstacle-duplicate".into();
    duplicate.transform.translation_xyz_m[0] += 2.0;
    body.instances.push(duplicate);
    let expected_mesh_vertex_count = body
        .meshes
        .iter()
        .map(|mesh| mesh.indices.len() as u64)
        .sum::<u64>();
    let packet = seal_scene_packet(body).expect("instanced packet seals");
    let perspective_packet =
        lower_objective_close_packet(&packet).expect("objective close packet seals");
    fs::write(
        &packet_path,
        serde_json::to_vec(&packet).expect("packet serializes"),
    )
    .expect("packet writes");
    fs::write(
        &perspective_packet_path,
        serde_json::to_vec(&perspective_packet).expect("perspective packet serializes"),
    )
    .expect("perspective packet writes");

    let script = r#"
using JSON3
include(ENV["WGE_GRAPHICS_WORKER"])
packet = JSON3.read(read(ENV["WGE_PACKET_PATH"], String))
response = JSON3.read(handle(JSON3.write((op="render_packet", packet=packet))))
response["kind"] == "frame_rendered" || error(JSON3.write(response))
perspective_packet = JSON3.read(read(ENV["WGE_PERSPECTIVE_PACKET_PATH"], String))
perspective_response = JSON3.read(handle(JSON3.write((op="render_packet", packet=perspective_packet))))
perspective_response["kind"] == "frame_rendered" || error(JSON3.write(perspective_response))
println(JSON3.write((orthographic=response["frame"], perspective=perspective_response["frame"])))
"#;
    let output = Command::new(julia_executable())
        .arg(format!("--project={}", graphics_lab.display()))
        .arg("--startup-file=no")
        .arg("-e")
        .arg(script)
        .env("WGE_GRAPHICS_WORKER", &worker)
        .env("WGE_PACKET_PATH", &packet_path)
        .env("WGE_PERSPECTIVE_PACKET_PATH", &perspective_packet_path)
        .output()
        .expect("graphics Julia worker starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let frames: serde_json::Value = serde_json::from_slice(&output.stdout).expect("frame JSON");
    let frame = &frames["orthographic"];
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
    // One frame includes shadow terrain/mesh passes, sky/terrain/mesh scene
    // passes, a linear-HDR resolve, and the post-resolve overlay pass.
    assert!(frame["telemetry"]["draw_calls"].as_u64().unwrap() >= 7);
    // The same lower bound proves both shadow pipelines and the resolve
    // pipeline compiled in addition to the four main scene pipelines.
    assert!(
        frame["telemetry"]["pipeline_compilations"]
            .as_u64()
            .unwrap()
            >= 7
    );
    assert_eq!(
        frame["telemetry"]["instance_count"].as_u64().unwrap(),
        packet.body.instances.len() as u64
    );
    assert_eq!(
        frame["telemetry"]["visible_instance_count"]
            .as_u64()
            .unwrap(),
        packet.body.instances.len() as u64
    );
    assert_eq!(
        frame["telemetry"]["culled_instance_count"]
            .as_u64()
            .unwrap(),
        0
    );
    assert_eq!(
        frame["telemetry"]["gameplay_critical_visible_instance_count"]
            .as_u64()
            .unwrap(),
        packet
            .body
            .instances
            .iter()
            .filter(|instance| {
                matches!(
                    instance.importance,
                    wge_native_graphics_contract::InstanceImportance::GameplayCritical
                )
            })
            .count() as u64
    );
    assert_eq!(
        frame["telemetry"]["gameplay_critical_culled_instance_count"]
            .as_u64()
            .unwrap(),
        0
    );
    assert_eq!(
        frame["telemetry"]["background_visible_instance_count"]
            .as_u64()
            .unwrap(),
        packet
            .body
            .instances
            .iter()
            .filter(|instance| {
                matches!(
                    instance.importance,
                    wge_native_graphics_contract::InstanceImportance::Background
                )
            })
            .count() as u64
    );
    assert_eq!(
        frame["telemetry"]["landmark_visible_instance_count"]
            .as_u64()
            .unwrap(),
        packet
            .body
            .instances
            .iter()
            .filter(|instance| {
                matches!(
                    instance.importance,
                    wge_native_graphics_contract::InstanceImportance::Landmark
                )
            })
            .count() as u64
    );
    assert!(frame["telemetry"]["terrain_vertex_count"].as_u64().unwrap() > 0);
    assert!(frame["telemetry"]["mesh_vertex_count"].as_u64().unwrap() > 0);
    assert_eq!(
        frame["telemetry"]["mesh_vertex_count"].as_u64().unwrap(),
        expected_mesh_vertex_count
    );
    let perspective_frame = &frames["perspective"];
    assert_eq!(perspective_frame["schema"], "wge.lava-frame/v1");
    assert_eq!(
        perspective_frame["packet_sha256"],
        perspective_packet.packet_sha256
    );
    assert!(
        perspective_frame["capture_sha256"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert!(
        perspective_frame["measurements"]["distinct_terrain_colors"]
            .as_u64()
            .unwrap()
            > 1
    );
    assert_ne!(
        frame["capture_sha256"], perspective_frame["capture_sha256"],
        "close-range perspective capture must not alias the overview capture"
    );
    assert_eq!(
        perspective_frame["telemetry"]["landmark_visible_instance_count"]
            .as_u64()
            .unwrap(),
        1
    );

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
    let gpu_timestamps = ready.features.gpu_timestamps;

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
    if gpu_timestamps {
        assert!(
            promoted
                .receipt
                .body
                .telemetry
                .gpu_frame_time_us
                .is_some_and(|value| value > 0)
        );
    }
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
