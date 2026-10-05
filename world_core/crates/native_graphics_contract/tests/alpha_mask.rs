//! CONVERGE-2 N-6: alpha-mask and double-sided material fields.
//!
//! Byte identity when absent is covered by `render_policy.rs`, which re-seals a
//! committed packet whose materials carry neither field. These tests pin the
//! validation rules on the same committed packet.

use wge_native_graphics_contract::{
    AlphaMode, GraphicsScenePacketBody, canonical_json, seal_scene_packet, validate_scene_packet,
};

fn body() -> GraphicsScenePacketBody {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace")
        .join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("committed packet missing at {}: {error}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&raw).expect("committed packet parses");
    serde_json::from_value(value["body"].clone()).expect("committed body deserializes")
}

fn validates(body: GraphicsScenePacketBody) -> Result<(), String> {
    let packet = seal_scene_packet(body).map_err(|error| error.to_string())?;
    validate_scene_packet(&packet).map_err(|error| error.to_string())
}

#[test]
fn mask_with_cutoff_and_double_sided_validates_and_round_trips() {
    let mut body = body();
    body.materials[0].alpha_mode = AlphaMode::Mask;
    body.materials[0].alpha_cutoff = Some(0.5);
    body.materials[0].double_sided = Some(true);
    validates(body.clone()).expect("mask material validates");
    let json = String::from_utf8(canonical_json(&body).unwrap()).unwrap();
    assert!(json.contains("\"alpha_cutoff\":0.5"));
    assert!(json.contains("\"double_sided\":true"));
    let back: GraphicsScenePacketBody = serde_json::from_str(&json).expect("round trip");
    assert_eq!(back.materials[0].alpha_cutoff, Some(0.5));
    assert_eq!(back.materials[0].double_sided, Some(true));
}

#[test]
fn presence_moves_the_digest() {
    let plain = seal_scene_packet(body()).unwrap();
    let mut sided = body();
    sided.materials[0].double_sided = Some(true);
    assert_ne!(seal_scene_packet(sided).unwrap().packet_sha256, plain.packet_sha256);
}

#[test]
fn mask_requires_a_cutoff_inside_the_unit_interval() {
    for cutoff in [None, Some(0.0), Some(1.0), Some(-0.1), Some(f32::NAN)] {
        let mut body = body();
        body.materials[0].alpha_mode = AlphaMode::Mask;
        body.materials[0].alpha_cutoff = cutoff;
        assert!(validates(body).is_err(), "cutoff {cutoff:?} accepted");
    }
}

#[test]
fn cutoff_is_refused_off_mask() {
    for mode in [AlphaMode::Opaque, AlphaMode::Blend] {
        let mut body = body();
        body.materials[0].alpha_mode = mode;
        body.materials[0].alpha_cutoff = Some(0.5);
        assert!(validates(body).is_err(), "{mode:?} with a cutoff accepted");
    }
}

#[test]
fn single_sided_has_one_encoding() {
    let mut body = body();
    body.materials[0].double_sided = Some(false);
    assert!(validates(body).is_err());
}

// CONVERGE-2 N-5 channel semantics: metallic from the metallicRoughness BLUE channel.

#[test]
fn metallic_from_texture_needs_a_roughness_texture_and_has_one_encoding() {
    let mut explicit_false = body();
    explicit_false.materials[0].metallic_from_texture = Some(false);
    assert!(validates(explicit_false).is_err(), "Some(false) accepted");
    let mut untextured = body();
    untextured.materials[0].roughness_texture_id = None;
    untextured.materials[0].metallic_from_texture = Some(true);
    assert!(validates(untextured).is_err(), "flag without a roughness texture accepted");
}
