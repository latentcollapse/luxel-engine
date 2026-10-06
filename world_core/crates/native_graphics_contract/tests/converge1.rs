//! CONVERGE-1 N-1 gates: the analytic sky model and the exponential,
//! height-dependent atmosphere (docs/world/converge/converge1-contracts.md §1).
//!
//! The renderer-side behaviour (sun-side vs anti-sun horizon, haze integral)
//! is gated in `graphics_lab/test/render_policy.jl`; byte identity of the
//! null / `full` / converge0 frames is proven by render, as in CONVERGE-0.

use std::path::PathBuf;

use wge_native_graphics_contract::{
    AtmospherePolicy, Campaign2View, GraphicsScenePacket, ParityContent, ParityPolicyCandidate, RenderPolicy,
    SkyModel, SkyPolicy, TerrainLayerSet, lower_campaign2_packet_with, validate_render_policy, CONVERGE1_ATMOSPHERE,
    CONVERGE1_SKY,
};

#[path = "support/synthetic_layers.rs"]
mod synthetic_layers;

fn reference() -> GraphicsScenePacket {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    let text = std::fs::read_to_string(&path).expect("committed reference packet");
    serde_json::from_str(&text).expect("reference packet parses")
}

fn converge_content() -> ParityContent {
    ParityContent { hero_materials: false, converge0: true }
}

fn lower(view: Campaign2View, candidate: ParityPolicyCandidate) -> GraphicsScenePacket {
    let set: TerrainLayerSet = synthetic_layers::synthetic_set();
    lower_campaign2_packet_with(&reference(), view, candidate, converge_content(), Some(&set)).expect("lowers")
}

fn sky(model: Option<SkyModel>) -> SkyPolicy {
    SkyPolicy { sun_disc_radius_milli_deg: 650, sun_disc_gain_bp: 120_000, sun_glow_gain_bp: 2_400, model }
}

fn atmosphere(falloff: i32, density: i32, gain: i32) -> AtmospherePolicy {
    AtmospherePolicy { height_falloff_milli_per_m: falloff, density_at_ground_bp: density, sun_scatter_gain_bp: gain }
}

#[test]
fn converge1_is_converge0_plus_sky_model_and_atmosphere() {
    let converge0 = ParityPolicyCandidate::Converge0.policy().unwrap();
    let converge1 = ParityPolicyCandidate::Converge1.policy().unwrap();
    validate_render_policy(&converge1).expect("converge1 policy is in bounds");
    assert_eq!(converge1.sky, Some(CONVERGE1_SKY));
    assert_eq!(converge1.atmosphere, Some(CONVERGE1_ATMOSPHERE));
    assert!(matches!(CONVERGE1_SKY.model, Some(SkyModel::Analytic { .. })));
    // Everything else is converge0's, so the A/B isolates sky and atmosphere.
    let stripped = RenderPolicy { sky: converge0.sky, atmosphere: None, ..converge1 };
    assert_eq!(stripped, converge0);
    // The sun disc is unchanged; only the model differs.
    let disc_only = SkyPolicy { model: None, ..CONVERGE1_SKY };
    assert_eq!(Some(disc_only), converge0.sky);
}

#[test]
fn absent_axes_do_not_serialise() {
    // converge0 and `full` packets must keep their canonical bytes: the new
    // keys may only appear when the axis is present.
    for candidate in [ParityPolicyCandidate::Full, ParityPolicyCandidate::Converge0] {
        let json = serde_json::to_string(&candidate.policy().unwrap()).unwrap();
        for key in ["atmosphere", "model", "turbidity_milli"] {
            assert!(!json.contains(key), "{candidate:?} policy JSON gained `{key}`: {json}");
        }
    }
    let json = serde_json::to_value(ParityPolicyCandidate::Converge1.policy().unwrap()).unwrap();
    assert_eq!(json["sky"]["model"], serde_json::json!({"kind": "analytic", "turbidity_milli": 3000}));
    assert_eq!(
        json["atmosphere"],
        serde_json::json!({
            "height_falloff_milli_per_m": CONVERGE1_ATMOSPHERE.height_falloff_milli_per_m,
            "density_at_ground_bp": CONVERGE1_ATMOSPHERE.density_at_ground_bp,
            "sun_scatter_gain_bp": CONVERGE1_ATMOSPHERE.sun_scatter_gain_bp
        })
    );
}

#[test]
fn sky_model_and_atmosphere_are_bounded() {
    let policy = |sky_policy: Option<SkyPolicy>, air: Option<AtmospherePolicy>| RenderPolicy {
        sky: sky_policy,
        atmosphere: air,
        ..RenderPolicy::default()
    };
    let analytic = |t| Some(sky(Some(SkyModel::Analytic { turbidity_milli: t })));
    assert!(validate_render_policy(&policy(analytic(2000), None)).is_ok());
    assert!(validate_render_policy(&policy(analytic(10000), None)).is_ok());
    assert!(validate_render_policy(&policy(analytic(1999), None)).is_err(), "below the Preetham fit range");
    assert!(validate_render_policy(&policy(analytic(10001), None)).is_err(), "above the Preetham fit range");
    assert!(validate_render_policy(&policy(Some(sky(Some(SkyModel::Gradient {}))), None)).is_ok());

    let with_sky = |air| policy(Some(sky(None)), Some(air));
    assert!(validate_render_policy(&with_sky(atmosphere(15, 50, 300))).is_ok());
    assert!(validate_render_policy(&with_sky(atmosphere(0, 0, 0))).is_ok(), "k = 0 is a homogeneous haze");
    assert!(validate_render_policy(&with_sky(atmosphere(1000, 1000, 20000))).is_ok());
    assert!(validate_render_policy(&with_sky(atmosphere(-1, 50, 300))).is_err());
    assert!(validate_render_policy(&with_sky(atmosphere(1001, 50, 300))).is_err());
    assert!(validate_render_policy(&with_sky(atmosphere(15, -1, 300))).is_err());
    assert!(validate_render_policy(&with_sky(atmosphere(15, 1001, 300))).is_err());
    assert!(validate_render_policy(&with_sky(atmosphere(15, 50, 20001))).is_err());
    assert!(
        validate_render_policy(&policy(None, Some(atmosphere(15, 50, 300)))).is_err(),
        "atmosphere fades into the sky behind each pixel, which needs the view-direction sky"
    );
}

#[test]
fn sky_model_and_atmosphere_fail_closed_on_unknown_fields() {
    let parse = |value: serde_json::Value| serde_json::from_value::<RenderPolicy>(value);
    let base = serde_json::json!({"sun_disc_radius_milli_deg": 650, "sun_disc_gain_bp": 0, "sun_glow_gain_bp": 0});
    let with_model = |model: serde_json::Value| {
        let mut sky_value = base.clone();
        sky_value["model"] = model;
        serde_json::json!({ "sky": sky_value })
    };
    assert!(parse(with_model(serde_json::json!({"kind": "gradient"}))).is_ok());
    assert!(parse(with_model(serde_json::json!({"kind": "analytic", "turbidity_milli": 3000}))).is_ok());
    assert!(parse(with_model(serde_json::json!({"kind": "hosek", "turbidity_milli": 3000}))).is_err());
    assert!(parse(with_model(serde_json::json!({"kind": "analytic"}))).is_err());
    assert!(parse(with_model(serde_json::json!({"kind": "gradient", "turbidity_milli": 3000}))).is_err());
    assert!(parse(with_model(serde_json::json!({"kind": "analytic", "turbidity_milli": 3000, "albedo": 1})))
        .is_err());
    assert!(parse(serde_json::json!({"atmosphere": {
        "height_falloff_milli_per_m": 15, "density_at_ground_bp": 50, "sun_scatter_gain_bp": 300, "mie_g": 7600
    }}))
    .is_err());
}

#[test]
fn converge1_content_couples_like_converge0() {
    assert_eq!(ParityContent::parse("", ParityPolicyCandidate::Converge1).unwrap(), converge_content());
    assert!(ParityContent::parse("converge0", ParityPolicyCandidate::Full).is_err());
    assert!(ParityPolicyCandidate::Converge1.uses_converge0_content());
    assert!(!ParityPolicyCandidate::Full.uses_converge0_content());
}

#[test]
fn converge1_lowers_the_converge0_world_with_only_policy_changed() {
    for view in [Campaign2View::Close, Campaign2View::Medium, Campaign2View::Wide] {
        let converge0 = lower(view, ParityPolicyCandidate::Converge0);
        let converge1 = lower(view, ParityPolicyCandidate::Converge1);
        assert_ne!(converge0.packet_sha256, converge1.packet_sha256, "the policy is digest-bound");
        let mut body = converge1.body.clone();
        body.render_policy = converge0.body.render_policy;
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            serde_json::to_value(&converge0.body).unwrap(),
            "{view:?}: converge1 must change only the render policy"
        );
    }
}
