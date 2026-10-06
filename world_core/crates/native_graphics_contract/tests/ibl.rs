//! N-2 gates (docs/world/converge/converge1-contracts.md §2): bake determinism, cross-language
//! sky parity, white furnace, energy, and packet binding of the baked textures.

use std::path::PathBuf;

use luxel_native_graphics_contract::ibl::{
    IBL_BRDF_LUT_TEXTURE_ID, IBL_ENVIRONMENT_TEXTURE_ID, IblEnvironment, apply_ibl, bake, brdf_lut, irradiance,
    irradiance_sh, octahedral_decode, octahedral_encode, prefiltered, validate_packet_ibl,
};
use luxel_native_graphics_contract::{
    EnvironmentIntent, GraphicsScenePacket, GraphicsScenePacketBody, IblPolicy, LightIntent, LightKind, RenderPolicy,
    SkyModel, SkyPolicy, TexturePayload,
};

fn reference_body() -> GraphicsScenePacketBody {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    let packet: GraphicsScenePacket = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    packet.body
}

/// converge0's environment and key light, with the analytic T = 3 sky; the
/// glow is off so the sky equals the shaders' `_sky_environment` exactly.
fn converge_body(ibl: bool) -> GraphicsScenePacketBody {
    let mut body = reference_body();
    body.environment = EnvironmentIntent {
        sky_top_rgb: [0.16, 0.30, 0.58],
        sky_horizon_rgb: [0.62, 0.66, 0.72],
        ground_rgb: [0.11, 0.12, 0.085],
        fog_color_rgb: [0.52, 0.58, 0.67],
        fog_density: 0.0011,
        exposure: 1.08,
    };
    body.lights = vec![LightIntent {
        light_id: "key".into(),
        kind: LightKind::Directional { direction_xyz: [1.6407, -1.0, -1.381] },
        color_rgb: [1.0, 0.82, 0.62],
        intensity: 5.0,
    }];
    body.render_policy = Some(RenderPolicy {
        sky: Some(SkyPolicy {
            sun_disc_radius_milli_deg: 650,
            sun_disc_gain_bp: 120_000,
            sun_glow_gain_bp: 0,
            model: Some(SkyModel::Analytic { turbidity_milli: 3000 }),
        }),
        ibl: ibl.then_some(IblPolicy { enabled: true }),
        ..RenderPolicy::default()
    });
    body
}

#[test]
fn sky_matches_the_julia_shader_function() {
    // (direction, analytic _sky_environment, gradient _sky_environment) from
    // graphics_lab `LavaAdapter._sky_environment` (Float32) on converge0's
    // environment and key light, T = 3.
    let reference: [([f64; 3], [f64; 3], [f64; 3]); 5] = [
        ([0.0, 1.0, 0.0], [0.16, 0.3, 0.58], [0.16, 0.3, 0.58]),
        ([0.6, 0.1, -0.79], [0.38932884, 0.44815633, 0.5545939], [0.47431988, 0.54598945, 0.6756626]),
        ([-0.5, 0.3, 0.4], [1.4676816, 1.9493198, 2.8806524], [0.32037646, 0.42551202, 0.6288102]),
        ([0.3, -0.5, 0.81], [0.22233933, 0.23873872, 0.22794789], [0.25902307, 0.27778915, 0.27054834]),
        ([-0.8, 0.05, 0.6], [1.1822988, 1.3248198, 1.574359], [0.51720506, 0.5795519, 0.6887146]),
    ];
    let analytic = IblEnvironment::from_body(&converge_body(false)).unwrap();
    let mut gradient_body = converge_body(false);
    gradient_body.render_policy.as_mut().unwrap().sky.as_mut().unwrap().model = None;
    let gradient = IblEnvironment::from_body(&gradient_body).unwrap();
    for (d, a, g) in reference {
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let d = d.map(|v| v / l);
        for (env, expected, label) in [(&analytic, a, "analytic"), (&gradient, g, "gradient")] {
            let got = env.base(d);
            for k in 0..3 {
                assert!((got[k] - expected[k]).abs() <= 2e-4 * expected[k].max(1.0), "{label} {d:?}: {got:?} vs {expected:?}");
            }
        }
    }
}

#[test]
fn octahedral_map_round_trips() {
    for &d in &[[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.6, 0.1, -0.79], [-0.3, -0.5, 0.81], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]] {
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2] as f64).sqrt();
        let d: [f64; 3] = [d[0] / l, d[1] / l, d[2] / l];
        let back = octahedral_decode(octahedral_encode(d));
        assert!((0..3).all(|k| (back[k] - d[k]).abs() < 1e-9), "{d:?} -> {back:?}");
    }
}

#[test]
fn bake_is_deterministic_and_bound_to_the_packet() {
    let mut body = converge_body(true);
    let first = bake(&body).unwrap();
    let second = bake(&body).unwrap();
    assert_eq!(first, second, "bake must be bit-identical across runs");
    assert_eq!(first[0].texture_id, IBL_ENVIRONMENT_TEXTURE_ID);
    assert_eq!(first[1].texture_id, IBL_BRDF_LUT_TEXTURE_ID);

    // Required when enabled, refused when stale, tampered, or not enabled.
    assert!(validate_packet_ibl(&body).is_err(), "enabled IBL without its textures");
    apply_ibl(&mut body).unwrap();
    validate_packet_ibl(&body).unwrap();
    let mut stale = body.clone();
    stale.environment.sky_top_rgb = [0.2, 0.3, 0.6];
    assert!(validate_packet_ibl(&stale).is_err(), "textures baked from a different sky");
    let mut tampered = body.clone();
    let texture = tampered.textures.iter_mut().find(|t| t.texture_id == IBL_BRDF_LUT_TEXTURE_ID).unwrap();
    texture.sha256 = format!("sha256:{}", "0".repeat(64));
    assert!(validate_packet_ibl(&tampered).is_err());
    let mut smuggled = body.clone();
    smuggled.render_policy.as_mut().unwrap().ibl = None;
    assert!(validate_packet_ibl(&smuggled).is_err(), "IBL textures without the axis");
    let mut disabled = body.clone();
    disabled.render_policy.as_mut().unwrap().ibl = Some(IblPolicy { enabled: false });
    apply_ibl(&mut disabled).unwrap();
    assert!(disabled.textures.iter().all(|t| !t.texture_id.starts_with("luxel-ibl-")));
    validate_packet_ibl(&disabled).unwrap();
    let TexturePayload::Rgba8(_) = body.textures.last().unwrap().payload.as_ref().unwrap() else {
        panic!("RGBA8 payload");
    };
}

fn uniform_white() -> IblEnvironment {
    let mut body = reference_body();
    body.environment.sky_top_rgb = [1.0; 3];
    body.environment.sky_horizon_rgb = [1.0; 3];
    body.environment.ground_rgb = [1.0; 3];
    body.render_policy = None;
    IblEnvironment::from_body(&body).unwrap()
}

/// Mirrors the shader's IBL ambient (`LavaAdapter._ibl_ambient`): roughness-
/// aware Fresnel, diffuse (1 − F)(1 − metal)·albedo·irradiance, specular
/// prefiltered·(F0·A + B).
fn ibl_ambient(albedo: f64, metal: f64, roughness: f64, nv: f64, e: f64, pre: f64) -> f64 {
    let f0 = 0.04 * (1.0 - metal) + albedo * metal;
    let fr = f0 + ((1.0 - roughness).max(f0) - f0) * (1.0 - nv).powi(5);
    let (a, b) = brdf_lut(nv, roughness);
    albedo * (1.0 - fr) * (1.0 - metal) * e + pre * (f0 * a + b)
}

#[test]
fn white_furnace_returns_albedo() {
    let env = uniform_white();
    let sh = irradiance_sh(&env);
    for n in [[0.0, 1.0, 0.0], [0.6, 0.1, -0.79], [0.0, -1.0, 0.0]] {
        let e = irradiance(&sh, n);
        assert!(e.iter().all(|v| (v - 1.0).abs() < 0.01), "irradiance/π of a white sky is 1: {e:?}");
        for roughness in [0.2, 0.6, 1.0] {
            let p = prefiltered(&env, n, roughness);
            assert!(p.iter().all(|v| (v - 1.0).abs() < 1e-9), "prefiltered white is white: {p:?}");
        }
    }
    // A white rough dielectric sphere seen head-on: average over its visible
    // disc, where N·V = μ is distributed with density 2μ.
    let steps = 200;
    let mut sum = 0.0;
    for i in 0..steps {
        let mu = (f64::from(i) + 0.5) / f64::from(steps);
        sum += ibl_ambient(1.0, 0.0, 1.0, mu, 1.0, 1.0) * 2.0 * mu / f64::from(steps);
    }
    assert!((sum - 1.0).abs() <= 0.03, "white furnace sphere = {sum:.4}, expected 1 ± 3%");
}

#[test]
fn split_sum_conserves_energy() {
    for j in 0..32 {
        let roughness = (f64::from(j) + 0.5) / 32.0;
        for i in 0..32 {
            let nv = (f64::from(i) + 0.5) / 32.0;
            let (a, b) = brdf_lut(nv, roughness);
            // F0 = 1 reflects A + B: never more than the light that arrives.
            assert!(a + b <= 1.0 + 1e-6, "energy created at nv {nv}, r {roughness}: {}", a + b);
            // Near normal incidence a metal reflects ~F0 of the environment.
            // (At grazing incidence Fresnel rises toward 1, so the contract's
            // "never exceeds F0 x environment" holds only here.)
            if nv > 0.9 {
                for f0 in [0.04, 0.5, 0.95] {
                    assert!(f0 * a + b <= f0 + 0.01, "nv {nv} r {roughness} f0 {f0}: {}", f0 * a + b);
                }
            }
        }
    }
    let (a, b) = brdf_lut(0.99, 0.05);
    assert!((a + b - 1.0).abs() < 0.02, "a smooth mirror at normal incidence reflects ~all: {}", a + b);
}
