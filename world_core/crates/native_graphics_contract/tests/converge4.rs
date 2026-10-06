//! CONVERGE-4 gates (`docs/world/converge/converge4-contracts.md`).
//!
//! N-3 km-scale backdrop. Tests that lower `converge4` need the built kit2,
//! like the CONVERGE-3 ones: `python3 tools/build_kit.py --set kit2`, then
//! `cargo test --test converge4 -- --include-ignored`. The backdrop itself is
//! synthetic here (`support/synthetic_backdrop.rs`); the real one is built from
//! a Gaea field by `tools/build_backdrop.py` and checked by its own test.

use std::path::{Path, PathBuf};

use wge_native_graphics_contract::backdrop::{
    BACKDROP_FAR_PLANE_M, BACKDROP_INSTANCE_PREFIX, BACKDROP_SEAT_DROP_M, apply_backdrop, backdrop_set_from_glb,
};
use wge_native_graphics_contract::{
    BackdropSet, BufferPayload, Campaign2Inputs, Campaign2View, GraphicsScenePacket, KitSet, ParityContent,
    ParityPolicyCandidate, canonical_json, load_backdrop_set, load_kit_set, lower_campaign2_packet_inputs,
    sha256_prefixed, validate_scene_packet, CONVERGE4_ATMOSPHERE, CONVERGE4_SHADOW_FIT,
};

#[path = "support/synthetic_layers.rs"]
mod synthetic_layers;
use synthetic_layers::synthetic_set;

#[path = "support/synthetic_backdrop.rs"]
mod synthetic_backdrop;
use synthetic_backdrop::backdrop_glb;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn reference() -> GraphicsScenePacket {
    let path =
        repo_root().join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("reference packet")).expect("parses")
}

const CONVERGE: ParityContent = ParityContent { hero_materials: false, converge0: true };

fn synthetic_backdrop() -> BackdropSet {
    let glb = backdrop_glb(2, 5, 4000.0, 900.0);
    backdrop_set_from_glb("synthetic", "sha256:test", &glb).expect("synthetic backdrop conditions")
}

fn kit2() -> KitSet {
    load_kit_set(&repo_root().join("tools/kit/kit2.lock.json"), &repo_root())
        .unwrap_or_else(|e| panic!("built kit2 loads (run tools/build_kit.py --set kit2): {e}"))
}

fn lower(
    candidate: ParityPolicyCandidate,
    kit: Option<&KitSet>,
    backdrop: Option<&BackdropSet>,
    view: Campaign2View,
) -> Result<GraphicsScenePacket, String> {
    let layers = synthetic_set();
    lower_campaign2_packet_inputs(
        &reference(),
        view,
        &Campaign2Inputs { candidate, content: CONVERGE, terrain_layers: Some(&layers), kit, backdrop },
    )
    .map_err(|error| error.to_string())
}

fn source_mean_height() -> f32 {
    let BufferPayload::F32(heights) = &reference().body.terrain.heights_m.payload else { panic!("f32 heights") };
    heights.iter().sum::<f32>() / heights.len() as f32
}

#[test]
fn converge4_is_converge3_policy_plus_backdrop_content() {
    let c3 = ParityPolicyCandidate::Converge3;
    let c4 = ParityPolicyCandidate::Converge4;
    let mut expected = c3.policy().expect("converge3 policy");
    expected.atmosphere = Some(CONVERGE4_ATMOSPHERE);
    expected.shadow_fit = Some(CONVERGE4_SHADOW_FIT);
    assert_eq!(c4.policy(), Some(expected), "converge4 = converge3 policy + km-scale atmosphere + L-1a shadow fit");
    assert!(c4.uses_converge0_content() && c4.uses_kit() && c4.uses_converge3_content() && c4.uses_backdrop());
    assert!(c3.uses_converge3_content() && !c3.uses_backdrop());
    for earlier in [
        ParityPolicyCandidate::Null,
        ParityPolicyCandidate::Converge0,
        ParityPolicyCandidate::Converge1,
        ParityPolicyCandidate::Converge2,
    ] {
        assert!(!earlier.uses_backdrop() && !earlier.uses_converge3_content(), "{earlier:?}");
    }
}

#[test]
fn the_backdrop_prefix_is_the_same_in_every_consumer() {
    // The renderer (reflection pass) and the N-1 metric recognise backdrop
    // tiles by this prefix; a drift would silently re-break the pool or the gate.
    for (path, needle) in [
        ("graphics_lab/src/LavaAdapter.jl", format!("const BACKDROP_INSTANCE_PREFIX = \"{BACKDROP_INSTANCE_PREFIX}\"")),
        ("tools/atmosphere_measure.py", format!("BACKDROP_PREFIX = \"{BACKDROP_INSTANCE_PREFIX}\"")),
    ] {
        let text = std::fs::read_to_string(repo_root().join(path)).expect("consumer source");
        assert!(text.contains(&needle), "{path} must declare {needle}");
    }
}

#[test]
fn a_backdrop_is_refused_outside_the_backdrop_arm() {
    let backdrop = synthetic_backdrop();
    let error = lower(ParityPolicyCandidate::Converge1, None, Some(&backdrop), Campaign2View::Wide).unwrap_err();
    assert!(error.contains("only valid with the converge4 arm"), "{error}");
}

#[test]
fn apply_backdrop_seats_every_tile_and_opens_the_far_plane() {
    let backdrop = synthetic_backdrop();
    let mut body = reference().body;
    let before_instances = body.instances.len();
    let near = body.camera.near_plane_m;
    apply_backdrop(&mut body, &backdrop, 7.5).expect("applies");
    let placed: Vec<_> =
        body.instances.iter().filter(|i| i.instance_id.starts_with(BACKDROP_INSTANCE_PREFIX)).collect();
    assert_eq!(placed.len(), backdrop.projection.meshes.len());
    assert_eq!(body.instances.len(), before_instances + placed.len());
    for instance in &placed {
        assert_eq!(instance.transform.translation_xyz_m, [0.0, 7.5, 0.0]);
        assert_eq!(instance.transform.rotation_xyzw, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(instance.transform.scale_xyz, [1.0; 3]);
        assert!(body.meshes.iter().any(|m| m.mesh_id == instance.mesh_id), "mesh resource present");
        assert!(body.materials.iter().any(|m| m.material_id == instance.material_id), "material present");
    }
    assert_eq!(body.camera.far_plane_m, BACKDROP_FAR_PLANE_M);
    assert_eq!(body.camera.near_plane_m, near, "near plane untouched");
    // A second backdrop is refused, and so is a non-finite seat.
    assert!(apply_backdrop(&mut body, &backdrop, 7.5).is_err());
    let mut fresh = reference().body;
    assert!(apply_backdrop(&mut fresh, &backdrop, f32::NAN).is_err());
}

fn write_lock(dir: &Path, glb: &[u8], overrides: serde_json::Value) -> PathBuf {
    std::fs::write(dir.join("backdrop.glb"), glb).expect("write glb");
    let mut lock = serde_json::json!({
        "schema_version": "wge.backdrop-lock/v1",
        "set_id": "synthetic",
        "spec_sha256": "sha256:spec",
        "gaea_pixels": {"Height_Out.png": "0".repeat(64)},
        "glb": "backdrop.glb",
        "bytes": glb.len(),
        "sha256": sha256_prefixed(glb),
    });
    for (key, value) in overrides.as_object().expect("object") {
        if value.is_null() {
            lock.as_object_mut().expect("object").remove(key);
        } else {
            lock[key] = value.clone();
        }
    }
    let path = dir.join("synthetic.lock.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&lock).expect("json")).expect("write lock");
    path
}

#[test]
fn the_lock_pins_the_glb_and_its_gaea_source() {
    let dir = std::env::temp_dir().join(format!("wge-backdrop-lock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let glb = backdrop_glb(2, 5, 4000.0, 900.0);

    let good = write_lock(&dir, &glb, serde_json::json!({}));
    let set = load_backdrop_set(&good, &dir).expect("a matching lock loads");
    assert_eq!(set.set_id, "synthetic");
    assert_eq!(set.glb_sha256, sha256_prefixed(&glb));
    assert_eq!(set.lock_sha256, sha256_prefixed(&std::fs::read(&good).expect("lock")));

    let refusals = [
        (serde_json::json!({"sha256": sha256_prefixed(b"other")}), "the lock pins"),
        (serde_json::json!({"bytes": glb.len() + 1}), "the lock pins"),
        (serde_json::json!({"schema_version": "wge.backdrop-lock/v0"}), "unsupported"),
        (serde_json::json!({"gaea_pixels": {}}), "pixel digest"),
        (serde_json::json!({"extra": 1}), "malformed"),
        (serde_json::json!({"glb": "missing.glb"}), "build it with"),
    ];
    for (overrides, expected) in refusals {
        let path = write_lock(&dir, &glb, overrides.clone());
        let error = load_backdrop_set(&path, &dir).unwrap_err().to_string();
        assert!(error.contains(expected), "{overrides}: {error}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
#[ignore = "needs the built kit (python3 tools/build_kit.py --set kit2)"]
fn converge4_requires_its_backdrop_and_adds_only_the_backdrop() {
    let kit = kit2();
    let backdrop = synthetic_backdrop();
    let error = lower(ParityPolicyCandidate::Converge4, Some(&kit), None, Campaign2View::Wide).unwrap_err();
    assert!(error.contains("requires a backdrop"), "{error}");
    let error = lower(ParityPolicyCandidate::Converge3, Some(&kit), Some(&backdrop), Campaign2View::Wide).unwrap_err();
    assert!(error.contains("only valid with the converge4 arm"), "{error}");

    let seat = source_mean_height() - BACKDROP_SEAT_DROP_M;
    for view in [Campaign2View::Close, Campaign2View::Medium, Campaign2View::Wide] {
        let c3 = lower(ParityPolicyCandidate::Converge3, Some(&kit), None, view).expect("converge3 lowers");
        let c4 = lower(ParityPolicyCandidate::Converge4, Some(&kit), Some(&backdrop), view).expect("converge4 lowers");
        validate_scene_packet(&c4).expect("valid");
        assert!(!c3.body.instances.iter().any(|i| i.instance_id.starts_with(BACKDROP_INSTANCE_PREFIX)));
        assert_eq!(c3.body.camera.far_plane_m, 1500.0, "converge3 keeps its far plane");

        // converge4 = converge3 + backdrop: strip the backdrop and the far
        // plane and the bodies must match exactly.
        let mut stripped = c4.body.clone();
        let backdrop_meshes: Vec<String> = stripped
            .instances
            .iter()
            .filter(|i| i.instance_id.starts_with(BACKDROP_INSTANCE_PREFIX))
            .map(|i| i.mesh_id.clone())
            .collect();
        assert_eq!(backdrop_meshes.len(), backdrop.projection.meshes.len());
        for instance in stripped.instances.iter().filter(|i| i.instance_id.starts_with(BACKDROP_INSTANCE_PREFIX)) {
            assert_eq!(instance.transform.translation_xyz_m, [0.0, seat, 0.0]);
        }
        stripped.instances.retain(|i| !i.instance_id.starts_with(BACKDROP_INSTANCE_PREFIX));
        let backdrop_materials: Vec<String> =
            c4.body.instances.iter().filter(|i| i.instance_id.starts_with(BACKDROP_INSTANCE_PREFIX)).map(|i| i.material_id.clone()).collect();
        stripped.meshes.retain(|m| !backdrop_meshes.contains(&m.mesh_id));
        let backdrop_textures: Vec<String> = stripped
            .materials
            .iter()
            .filter(|m| backdrop_materials.contains(&m.material_id))
            .flat_map(|m| m.texture_ids.clone())
            .collect();
        stripped.materials.retain(|m| !backdrop_materials.contains(&m.material_id));
        stripped.textures.retain(|t| !backdrop_textures.contains(&t.texture_id));
        assert_eq!(stripped.camera.far_plane_m, BACKDROP_FAR_PLANE_M);
        stripped.camera.far_plane_m = c3.body.camera.far_plane_m;
        // The only policy differences are N-3's atmosphere and L-1a's shadow fit.
        let mut c4_policy = stripped.render_policy.clone().expect("converge4 policy");
        assert_eq!(c4_policy.atmosphere, Some(CONVERGE4_ATMOSPHERE));
        assert_eq!(c4_policy.shadow_fit, Some(CONVERGE4_SHADOW_FIT));
        c4_policy.atmosphere = c3.body.render_policy.as_ref().and_then(|p| p.atmosphere);
        c4_policy.shadow_fit = c3.body.render_policy.as_ref().and_then(|p| p.shadow_fit);
        assert_eq!(Some(c4_policy), c3.body.render_policy);
        stripped.render_policy = c3.body.render_policy.clone();
        stripped.packet_id = c3.body.packet_id.clone();
        stripped.capture = c3.body.capture.clone();
        assert_eq!(
            canonical_json(&stripped).expect("json"),
            canonical_json(&c3.body).expect("json"),
            "{view:?}: converge4 minus its backdrop is converge3"
        );
    }
}

#[test]
fn l1a_shadow_map_size_is_optional_bounded_and_absent_by_default() {
    use wge_native_graphics_contract::{RenderPolicy, ShadowFitPolicy, validate_render_policy};
    // Absent: not serialised, so converge0..3 packets keep their bytes.
    let legacy = ShadowFitPolicy { view_distance_m: 60, map_size_px: None };
    assert_eq!(serde_json::to_string(&legacy).expect("json"), r#"{"view_distance_m":60}"#);
    let parsed: ShadowFitPolicy = serde_json::from_str(r#"{"view_distance_m":60}"#).expect("parses");
    assert_eq!(parsed, legacy);
    for (size, ok) in [(512, true), (1024, true), (2048, true), (4096, true), (8192, false), (1000, false), (0, false)] {
        let policy = RenderPolicy {
            shadow_fit: Some(ShadowFitPolicy { view_distance_m: 250, map_size_px: Some(size) }),
            ..RenderPolicy::default()
        };
        assert_eq!(validate_render_policy(&policy).is_ok(), ok, "map_size_px {size}");
    }
    for earlier in [
        ParityPolicyCandidate::Converge0,
        ParityPolicyCandidate::Converge1,
        ParityPolicyCandidate::Converge2,
        ParityPolicyCandidate::Converge3,
    ] {
        let fit = earlier.policy().and_then(|p| p.shadow_fit).expect("converge arms fit the shadow");
        assert_eq!(fit.map_size_px, None, "{earlier:?} keeps the historical 512 map");
    }
}
