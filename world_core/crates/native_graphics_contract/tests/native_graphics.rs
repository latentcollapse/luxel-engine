use std::fs;
use std::path::{Path, PathBuf};

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
