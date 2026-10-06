//! CONVERGE-2 N-5 hero kit gates (`docs/world/converge/converge2-contracts.md` §2).
//!
//! The always-on tests need no built kit. The `#[ignore]`d ones load the real
//! kit from `artifacts/kit/kit1/` (build it with `python3 tools/build_kit.py`)
//! and run with `cargo test --test kit -- --include-ignored`.

use std::path::PathBuf;

use luxel_native_graphics_contract::{
    Campaign2Inputs, Campaign2View, GraphicsScenePacket, KitSet, ParityContent, ParityPolicyCandidate,
    canonical_json, load_kit_set, lower_campaign2_packet_inputs, validate_scene_packet,
};

#[path = "support/synthetic_layers.rs"]
mod synthetic_layers;
use synthetic_layers::synthetic_set;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn reference() -> GraphicsScenePacket {
    let path = repo_root().join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("reference packet")).expect("parses")
}

const CONVERGE: ParityContent = ParityContent { hero_materials: false, converge0: true };

fn lower(candidate: ParityPolicyCandidate, kit: Option<&KitSet>, view: Campaign2View) -> Result<GraphicsScenePacket, String> {
    let layers = synthetic_set();
    lower_campaign2_packet_inputs(
        &reference(),
        view,
        &Campaign2Inputs { candidate, content: CONVERGE, terrain_layers: Some(&layers), kit, backdrop: None },
    )
    .map_err(|error| error.to_string())
}

#[test]
fn the_committed_lock_names_every_kit_asset() {
    let lock: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo_root().join("tools/kit/kit1.lock.json")).expect("lock")).expect("parses");
    let names: Vec<&str> = lock["assets"].as_object().expect("assets").keys().map(String::as_str).collect();
    assert_eq!(names, ["fern", "rock_a", "rock_b", "ruin", "tree"]);
    for (name, asset) in lock["assets"].as_object().unwrap() {
        assert!(asset["sha256"].as_str().unwrap().starts_with("sha256:"), "{name} digest");
        assert!(asset["bytes"].as_u64().unwrap() > 0, "{name} size");
    }
}

#[test]
fn converge2_requires_a_kit_and_no_other_arm_accepts_one() {
    let error = lower(ParityPolicyCandidate::Converge2, None, Campaign2View::Close).expect_err("kit required");
    assert!(error.contains("requires a kit"), "{error}");
    let empty = KitSet { set_id: "empty".into(), lock_sha256: "sha256:0".into(), assets: Vec::new() };
    let error = lower(ParityPolicyCandidate::Converge1, Some(&empty), Campaign2View::Close).expect_err("kit refused");
    assert!(error.contains("only valid with the converge2, converge3 or converge4 arm"), "{error}");
}

fn kit() -> KitSet {
    load_kit_set(&repo_root().join("tools/kit/kit1.lock.json"), &repo_root()).expect("built kit loads (run tools/build_kit.py)")
}

#[test]
#[ignore = "needs the built kit (python3 tools/build_kit.py)"]
fn converge2_replaces_the_shrine_and_tree_balls_with_the_kit() {
    let kit = kit();
    for view in [Campaign2View::Close, Campaign2View::Medium, Campaign2View::Wide] {
        let packet = lower(ParityPolicyCandidate::Converge2, Some(&kit), view).expect("converge2 lowers");
        validate_scene_packet(&packet).expect("valid");
        let ids: Vec<&str> = packet.body.instances.iter().map(|i| i.instance_id.as_str()).collect();
        for gone in ["campaign2-hero-", "campaign2-crown-", "campaign2-lobe-", "campaign2-trunk-"] {
            assert!(!ids.iter().any(|id| id.starts_with(gone)), "{view:?}: {gone} still present");
        }
        assert!(!packet.body.meshes.iter().any(|m| m.mesh_id.starts_with("campaign2-hero")), "orphan hero meshes");
        for kept in ["campaign2-wet-pool", "kit-ruin-00", "kit-tree-06-00", "kit-rock-04-00", "kit-fern-11-00"] {
            assert!(ids.contains(&kept), "{view:?}: {kept} missing");
        }
        // The converge1 packet differs only in content.
        let converge1 = lower(ParityPolicyCandidate::Converge1, None, view).expect("converge1 lowers");
        assert_eq!(packet.body.render_policy, converge1.body.render_policy);
        assert_eq!(packet.body.camera, converge1.body.camera);
    }
}

#[test]
#[ignore = "needs the built kit (python3 tools/build_kit.py)"]
fn the_kit_costs_at_most_24_mb_of_packet() {
    let kit = kit();
    let layers = synthetic_set();
    let inputs = |candidate, kit| Campaign2Inputs { candidate, content: CONVERGE, terrain_layers: Some(&layers), kit, backdrop: None };
    let size = |packet: &GraphicsScenePacket| canonical_json(packet).expect("json").len();
    let with = lower_campaign2_packet_inputs(&reference(), Campaign2View::Wide, &inputs(ParityPolicyCandidate::Converge2, Some(&kit))).unwrap();
    let without = lower_campaign2_packet_inputs(&reference(), Campaign2View::Wide, &inputs(ParityPolicyCandidate::Converge1, None)).unwrap();
    let added = size(&with) as i64 - size(&without) as i64;
    eprintln!("kit adds {:.1} MB ({} -> {} bytes)", added as f64 / 1e6, size(&without), size(&with));
    assert!(added <= 24_000_000, "kit adds {added} bytes");
}

#[test]
#[ignore = "needs the built kit (python3 tools/build_kit.py)"]
fn a_tampered_kit_glb_is_refused() {
    let dir = std::env::temp_dir().join(format!("luxel-kit-tamper-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("artifacts/kit/kit1")).unwrap();
    std::fs::create_dir_all(dir.join("tools/kit")).unwrap();
    for name in ["ruin", "rock_a", "rock_b", "tree", "fern"] {
        let rel = format!("artifacts/kit/kit1/{name}.glb");
        std::fs::copy(repo_root().join(&rel), dir.join(&rel)).unwrap();
    }
    std::fs::copy(repo_root().join("tools/kit/kit1.lock.json"), dir.join("tools/kit/kit1.lock.json")).unwrap();
    load_kit_set(&dir.join("tools/kit/kit1.lock.json"), &dir).expect("an untouched copy loads");
    let path = dir.join("artifacts/kit/kit1/rock_a.glb");
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(&path, bytes).unwrap();
    let error = load_kit_set(&dir.join("tools/kit/kit1.lock.json"), &dir).expect_err("tampered GLB must be refused");
    assert!(error.to_string().contains("rock_a"), "{error}");
    std::fs::remove_dir_all(dir).ok();
}
