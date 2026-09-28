use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use wge_native_graphics_contract::{lower_reference_world, validate_scene_packet};
use wge_reference_runtime::build_from_layout_path;

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
fn rust_packet_renders_through_the_pinned_lava_worker() {
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
    assert!(frame["telemetry"]["draw_calls"].as_u64().unwrap() >= 2);

    fs::remove_dir_all(output_dir).expect("test output directory is removed");
}
