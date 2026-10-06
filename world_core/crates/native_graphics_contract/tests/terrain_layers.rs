//! N-4 terrain layer gates (`docs/world/converge/converge1-contracts.md` §4).
//!
//! Every refusal here is shown to fire on a concrete bad input; the positive
//! cases use the synthetic set so the suite never needs the network.

use std::path::PathBuf;

use luxel_native_graphics_contract::{
    Campaign2View, GraphicsScenePacket, LayerCoverage, ParityContent, ParityPolicyCandidate,
    TerrainSurfacePolicy, apply_terrain_layers, build_terrain_layer_set, load_terrain_layer_set,
    lower_campaign2_packet_with, seal_scene_packet, validate_scene_packet, validate_terrain_layers,
};

#[path = "support/synthetic_layers.rs"]
mod synthetic_layers;
use synthetic_layers::{checker, sizes, source, synthetic_set};

fn reference() -> GraphicsScenePacket {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("reference packet")).expect("parses")
}

fn converge0(view: Campaign2View) -> GraphicsScenePacket {
    lower_campaign2_packet_with(
        &reference(),
        view,
        ParityPolicyCandidate::Converge0,
        ParityContent { hero_materials: false, converge0: true },
        Some(&synthetic_set()),
    )
    .expect("converge0 lowers with a layer set")
}

#[test]
fn the_set_is_deterministic_and_every_texture_is_mipped() {
    let first = synthetic_set();
    let second = synthetic_set();
    assert_eq!(first, second, "same sources must give bit-identical textures");
    assert_eq!(first.layers.set_sha256, second.layers.set_sha256);
    for texture in &first.textures {
        assert!(texture.mip_levels >= 2, "{} has no mip chain", texture.texture_id);
    }
    // Physical tiling comes from the scan, not the world extent.
    assert_eq!(first.layers.layers[0].metres_per_repeat_milli, 2000);
    assert_eq!(first.layers.layers[1].metres_per_repeat_milli, 3150);
}

#[test]
fn converge0_carries_layers_and_other_arms_do_not() {
    for view in [Campaign2View::Close, Campaign2View::Medium, Campaign2View::Wide] {
        let packet = converge0(view);
        validate_scene_packet(&packet).expect("layered converge0 packet validates end to end");
        let layers = packet.body.terrain.layers.as_ref().expect("converge0 terrain has layers");
        assert_eq!(layers.layers.len(), 3);
        let policy = packet.body.render_policy.as_ref().unwrap().terrain_surface.unwrap();
        assert!(policy.macro_variation_bp > 0 && policy.wrap_repeat);
    }
    let null = lower_campaign2_packet_with(
        &reference(),
        Campaign2View::Close,
        ParityPolicyCandidate::Null,
        ParityContent::default(),
        None,
    )
    .unwrap();
    assert!(null.body.terrain.layers.is_none());
    let json = serde_json::to_string(&null.body.terrain).unwrap();
    assert!(!json.contains("layers"), "absent layers must not serialise");
}

#[test]
fn a_layer_set_is_refused_outside_converge0_and_required_inside_it() {
    let set = synthetic_set();
    assert!(
        lower_campaign2_packet_with(&reference(), Campaign2View::Close, ParityPolicyCandidate::Full, ParityContent::default(), Some(&set))
            .is_err()
    );
    assert!(
        lower_campaign2_packet_with(
            &reference(),
            Campaign2View::Close,
            ParityPolicyCandidate::Converge0,
            ParityContent { hero_materials: false, converge0: true },
            None
        )
        .is_err()
    );
}

#[test]
fn macro_variation_without_layers_is_refused() {
    let mut body = lower_campaign2_packet_with(
        &reference(),
        Campaign2View::Close,
        ParityPolicyCandidate::Full,
        ParityContent::default(),
        None,
    )
    .unwrap()
    .body;
    let mut policy = body.render_policy.unwrap();
    policy.terrain_surface = Some(TerrainSurfacePolicy { macro_variation_bp: 1000, ..policy.terrain_surface.unwrap() });
    body.render_policy = Some(policy);
    let error = seal_scene_packet(body).expect_err("decorative macro axis must be refused");
    assert!(format!("{error:?}").contains("layered terrain"), "{error:?}");
}

#[test]
fn every_validation_rule_fires() {
    let good = synthetic_set();
    let check = |layers: &luxel_native_graphics_contract::TerrainLayers,
                 materials: &[luxel_native_graphics_contract::MaterialIntent],
                 textures: &[luxel_native_graphics_contract::TextureReference]| {
        validate_terrain_layers(layers, materials, textures)
    };
    check(&good.layers, &good.materials, &good.textures).expect("good set validates");

    let mut one = good.layers.clone();
    one.layers.truncate(1);
    assert!(check(&one, &good.materials, &good.textures).is_err(), "a single layer is not a layered terrain");

    let mut base_covered = good.layers.clone();
    base_covered.layers[0].coverage = good.layers.layers[2].coverage;
    assert!(check(&base_covered, &good.materials, &good.textures).is_err(), "base layer cannot carry coverage");

    let mut empty = good.layers.clone();
    empty.layers[2].coverage = Some(LayerCoverage { slope_bp: None, height_mm: None, macro_ramp: None });
    assert!(check(&empty, &good.materials, &good.textures).is_err(), "empty coverage would cover everything");

    let mut flat_ramp = good.layers.clone();
    flat_ramp.layers[2].coverage = Some(LayerCoverage { slope_bp: Some([900, 900]), height_mm: None, macro_ramp: None });
    assert!(check(&flat_ramp, &good.materials, &good.textures).is_err(), "a zero-width ramp divides by zero");

    let mut too_fine = good.layers.clone();
    too_fine.layers[0].metres_per_repeat_milli = 50;
    assert!(check(&too_fine, &good.materials, &good.textures).is_err());

    let mut tinted = good.materials.clone();
    tinted[0].base_color_rgba = [0.8, 0.8, 0.8, 1.0];
    assert!(check(&good.layers, &tinted, &good.textures).is_err(), "a factor would hide a second colour source");

    let mut unmipped = good.textures.clone();
    let albedo = good.materials[0].texture_ids[0].clone();
    let texture = unmipped.iter_mut().find(|t| t.texture_id == albedo).unwrap();
    texture.mip_levels = 1;
    assert!(check(&good.layers, &good.materials, &unmipped).is_err(), "EDGE-1: unmipped scans alias");
}

#[test]
fn applying_a_set_twice_is_refused() {
    let mut body = converge0(Campaign2View::Close).body;
    assert!(apply_terrain_layers(&mut body, &synthetic_set()).is_err(), "id collisions must not shadow content");
}

#[test]
fn the_manifest_loader_verifies_every_pin() {
    let dir = std::env::temp_dir().join(format!("luxel-terrain-layers-{}", std::process::id()));
    let cache = dir.join("cache");
    let write_png = |layer: &str, name: &str, image: &luxel_native_graphics_contract::SquareRgba8| -> serde_json::Value {
        let folder = cache.join(layer);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(name);
        image::save_buffer(&path, &image.bytes, image.side, image.side, image::ColorType::Rgba8).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let digest = {
            use sha2::Digest;
            format!("sha256:{:x}", sha2::Sha256::digest(&bytes))
        };
        serde_json::json!({ "url": format!("https://example.invalid/{name}"), "bytes": bytes.len(), "sha256": digest })
    };
    let mut layers = Vec::new();
    for (id, tint, coverage) in [
        ("ground", [90, 120, 60, 255], serde_json::Value::Null),
        ("rock", [110, 110, 105, 255], serde_json::json!({ "slope_bp": [900, 1600], "height_mm": null, "macro": null })),
    ] {
        let s = source(id, tint, 2000, None);
        layers.push(serde_json::json!({
            "layer_id": id, "source_asset": format!("synthetic_{id}"), "dimensions_mm": [2000.0, 2000.0],
            "normal_scale_milli": 1000, "coverage": coverage,
            "maps": {
                "albedo": write_png(id, &format!("{id}_diff.png"), &s.albedo),
                "normal_gl": write_png(id, &format!("{id}_nor_gl.png"), &s.normal),
                "roughness": write_png(id, &format!("{id}_rough.png"), &s.roughness),
                "ao": write_png(id, &format!("{id}_ao.png"), &s.occlusion),
            }
        }));
    }
    let manifest = |license: &str| serde_json::json!({
        "schema_version": "luxel.terrain-layer-set-manifest/v1", "set_id": "loader-test", "provider": "test",
        "license": license, "cache_dir": "cache",
        "texture_sizes_px": { "albedo": 64, "normal_gl": 32, "roughness": 16, "ao": 16 },
        "layers": layers, "macro": { "size_px": 32, "seed": 7 }
    });
    let path = dir.join("manifest.json");
    std::fs::write(&path, serde_json::to_vec(&manifest("CC0-1.0")).unwrap()).unwrap();
    let set = load_terrain_layer_set(&path, &dir).expect("pinned files load");
    assert_eq!(set.layers.layers.len(), 2);

    std::fs::write(&path, serde_json::to_vec(&manifest("CC-BY-4.0")).unwrap()).unwrap();
    assert!(load_terrain_layer_set(&path, &dir).is_err(), "only CC0 sources are accepted");

    // Tamper with one file after pinning: the digest check must refuse it.
    std::fs::write(&path, serde_json::to_vec(&manifest("CC0-1.0")).unwrap()).unwrap();
    let victim = cache.join("rock").join("rock_rough.png");
    let alternate = checker(64, [10, 10, 10, 255], [20, 20, 20, 255]);
    image::save_buffer(&victim, &alternate.bytes, 64, 64, image::ColorType::Rgba8).unwrap();
    let error = load_terrain_layer_set(&path, &dir).expect_err("tampered file must be refused");
    assert!(format!("{error:?}").contains("manifest pin"), "{error:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_refuses_sources_that_cannot_reach_the_target_size() {
    let mut small = source("ground", [90, 120, 60, 255], 2000, None);
    small.albedo = checker(32, [1, 2, 3, 255], [4, 5, 6, 255]);
    let rock = source(
        "rock",
        [110, 110, 105, 255],
        3000,
        Some(LayerCoverage { slope_bp: Some([900, 1600]), height_mm: None, macro_ramp: None }),
    );
    assert!(build_terrain_layer_set("too-small", sizes(), &[small, rock], 32, 1).is_err());
}
