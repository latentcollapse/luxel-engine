use std::path::PathBuf;

use serde_json::Value;
use wge_asset_contract::{
    AcceptanceStatus, AssetUse, evaluate_structural_acceptance, inspect_file,
};

const EXPECTED_INPUT_SHA256: &str =
    "858fa104880822d081405579fb5b39d533d3b3b341d38aa1490a44b634f5e2b4";
const EXPECTED_PYTHON_REPORT_SHA256: &str =
    "14301dc1dc20272b7d35e270a3d0221a310b3376470c0cc025f97f8d06638f71";

fn benchmark_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("WGE_ASSET_BENCHMARK") {
        return Some(PathBuf::from(path));
    }
    let default =
        PathBuf::from("/home/mattc/Pictures/Generated 2D Images/sample_2026-09-26T091412.074.glb");
    default.is_file().then_some(default)
}

#[test]
fn supplied_glb_matches_python_golden_facts_and_content_digest() {
    let Some(path) = benchmark_path() else {
        eprintln!("external supplied GLB is unavailable; golden benchmark skipped");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../");
    let golden_path = root.join("asset_intake_reports/sample_2026-09-26T091412.074.intake.json");
    let golden: Value = serde_json::from_slice(
        &std::fs::read(&golden_path).expect("checked-in intake report is readable"),
    )
    .expect("checked-in intake report is valid JSON");
    let report = inspect_file(&path).expect("provided GLB meets the supported contract");
    let second = inspect_file(&path).expect("provided GLB parses deterministically");

    assert_eq!(report, second);
    assert_eq!(report.identity.source_sha256, EXPECTED_INPUT_SHA256);
    assert_eq!(
        report.identity.source_sha256,
        golden["input"]["sha256"].as_str().unwrap()
    );
    assert_eq!(
        report.identity.byte_length,
        golden["input"]["byte_length"].as_u64().unwrap()
    );
    assert_eq!(
        report.facts.scene_count,
        golden["scene"]["scene_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        report.facts.node_count,
        golden["scene"]["node_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        report.facts.active_node_count,
        golden["scene"]["active_node_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        report.facts.mesh_count,
        golden["scene"]["mesh_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        report.facts.skin_count,
        golden["scene"]["skin_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        report.facts.animation_count,
        golden["scene"]["animation_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        report.facts.primitive_count,
        golden["geometry"]["primitive_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        report.facts.total_vertex_count,
        golden["geometry"]["total_vertex_count"].as_u64().unwrap()
    );
    assert_eq!(
        report.facts.total_index_count,
        golden["geometry"]["total_index_count"].as_u64().unwrap()
    );
    assert_eq!(
        report.facts.total_triangle_count,
        golden["geometry"]["total_triangle_count"].as_u64().unwrap()
    );
    assert_eq!(
        report.facts.attributes_present,
        golden["geometry"]["attributes_present"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    );
    let topology = report.facts.primitives[0].topology.as_ref().unwrap();
    let golden_topology = &golden["geometry"]["meshes"][0]["primitives"][0]["topology"];
    assert_eq!(
        topology.connected_component_count,
        golden_topology["connected_component_count"]
            .as_u64()
            .unwrap()
    );
    assert_eq!(
        topology.boundary_edge_count,
        golden_topology["boundary_edge_count"].as_u64().unwrap()
    );
    assert_eq!(
        topology.non_manifold_edge_count,
        golden_topology["non_manifold_edge_count"].as_u64().unwrap()
    );
    assert_eq!(
        topology.zero_area_triangles,
        golden_topology["zero_area_triangles"].as_u64().unwrap()
    );
    assert_eq!(
        golden["canonical_report_sha256"].as_str().unwrap(),
        EXPECTED_PYTHON_REPORT_SHA256
    );
    assert_ne!(
        report.canonical_report_sha256, EXPECTED_PYTHON_REPORT_SHA256,
        "Rust v1 digest is for its explicit bounded schema, not the Python v1 document"
    );
    let character = evaluate_structural_acceptance(&report, AssetUse::Character);
    assert_eq!(character.status, AcceptanceStatus::RequiresWork);
}
