//! RenderPolicy contract tests (graphical parity sprint §4).
//!
//! The load-bearing property is BYTE-IDENTITY WHEN ABSENT. Everything else in
//! the graphical parity sprint depends on being able to add a policy channel
//! without moving the frozen baseline captures, so that property is asserted
//! against a COMMITTED artifact rather than against a fixture this file
//! also built — a round-trip assertion could not catch a leak into canonical
//! JSON, but re-sealing a committed packet can.

use luxel_native_graphics_contract::{
    BloomPolicy, DitherPolicy, GradePolicy, MeshSurfacePolicy, RenderPolicy, SamplerPolicy,
    ShadowFitPolicy, ShadowPolicy, SkyModel, SkyPolicy, AtmospherePolicy, DebugPolicy, FoliageCoveragePolicy, IblPolicy, TerrainSurfacePolicy, VignettePolicy, canonical_json, seal_scene_packet, validate_render_policy,
    validate_scene_packet, GraphicsContractError, GraphicsScenePacketBody, POLICY_SCALE,
    SCENE_PACKET_SCHEMA,
};

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn body() -> GraphicsScenePacketBody {
    let path = repo_root()
        .join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "committed baseline packet must be present for render-policy byte-identity at {}: {error}",
            path.display()
        )
    });
    let value: serde_json::Value = serde_json::from_str(&raw).expect("committed packet parses");
    serde_json::from_value(value["body"].clone()).expect("committed body deserializes")
}

fn repo_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR = <repo>/world_core/crates/native_graphics_contract,
    // so the repo root is the third ancestor (0 = crate, 1 = crates,
    // 2 = world_core, 3 = repo). Matches the P1 test's own helper.
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace")
        .to_path_buf()
}

// ---------------------------------------------------------------------------
// BYTE IDENTITY
// ---------------------------------------------------------------------------

#[test]
fn committed_packet_reseals_identically_with_render_policy_module_present() {
    let path = repo_root()
        .join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    // NO SKIP. An earlier draft of this file returned early when the artifact
    // was missing, and the suite reported 4 passed / 6 failed while the
    // byte-identity gate had in fact gated nothing — FR-0016's exact shape
    // (a check that cannot fail is not a check). A missing artifact must fail
    // the build, not quietly pass it.
    assert!(
        path.exists(),
        "committed baseline artifact missing at {}: the byte-identity gate would be vacuous",
        path.display()
    );
    let raw = std::fs::read_to_string(&path).expect("committed packet reads");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("committed packet parses");
    let committed = value["packet_sha256"].as_str().expect("digest").to_owned();
    assert_eq!(value["body"]["schema_version"], "luxel.graphics-scene-packet/v6");

    let body: GraphicsScenePacketBody =
        serde_json::from_value(value["body"].clone()).expect("body deserializes");
    assert!(
        body.render_policy.is_none(),
        "the committed v6 artifact predates render_policy and must not carry one"
    );
    let resealed = seal_scene_packet(body).expect("re-seals");
    assert_eq!(
        resealed.packet_sha256, committed,
        "RENDER POLICY BYTE-IDENTITY FAILED: re-sealing a committed v6 packet moved its digest \
         — render_policy reached canonical JSON while absent"
    );
    validate_scene_packet(&resealed).expect("resealed packet still validates");
}

#[test]
fn absent_policy_emits_no_key_in_canonical_json() {
    let body = body();
    let json = String::from_utf8(canonical_json(&body).expect("canonical json")).expect("utf8");
    assert!(
        !json.contains("render_policy"),
        "an absent render_policy serialized a key: {json}"
    );
}

#[test]
fn presence_actually_changes_the_digest() {
    // The negative control for the test above. If adding a policy did NOT move
    // the digest, then "absent emits no key" would be vacuously true and the
    // byte-identity gate would be measuring nothing. This is FR-0016's shape:
    // a check that cannot fail is not a check.
    let mut with = body();
    with.render_policy = Some(RenderPolicy {
        dither: Some(DitherPolicy {
            amplitude_milli_lsb: 1000,
        }),
        ..RenderPolicy::default()
    });
    let without = body();
    let a = seal_scene_packet(with).expect("policy packet seals");
    let b = seal_scene_packet(without).expect("plain packet seals");
    assert_ne!(
        a.packet_sha256, b.packet_sha256,
        "adding a render policy did not move the packet digest: the policy is decorative"
    );
    validate_scene_packet(&a).expect("policy packet validates");
    let json = String::from_utf8(canonical_json(&a.body).expect("json")).expect("utf8");
    assert!(json.contains("render_policy"), "present policy omitted its key");
}

// ---------------------------------------------------------------------------
// DEFAULTS ARE AUDITABLE HERE, NOT IN THE RENDERER
// ---------------------------------------------------------------------------

#[test]
fn default_policy_reproduces_the_frozen_baseline_renderer_behaviour() {
    let resolved = RenderPolicy::default().resolved();
    assert_eq!(
        resolved.dither.amplitude_milli_lsb, 0,
        "default dither must be OFF so the frozen baseline captures stay byte-identical"
    );
    assert_eq!(
        resolved.sampler.anisotropy, 1,
        "default anisotropy must be isotropic, matching the baseline sampler"
    );
    assert_eq!(
        resolved.terrain_surface.uv_repeat_scale_milli, 1000,
        "default terrain UV scale must be 1.0 repeat/metre, i.e. the baseline whole-world 0..1 UV"
    );
    assert_eq!(
        resolved.terrain_surface.macro_variation_bp, 0,
        "default macro variation must be off"
    );
    assert_eq!(
        resolved.bloom.intensity_bp, 0,
        "default bloom must be off; the baseline post chain was a tonemap only"
    );
    assert_eq!(resolved.vignette.strength_bp, 0, "default vignette must be off");
    assert_eq!(
        resolved.shadow.darkness_bp, 7500,
        "default shadow darkness must be 0.75, i.e. the historical \
         `0.25 + 0.75 * pcf` direct-light floor, so the frozen captures stay byte-identical"
    );
    assert_eq!(
        resolved.shadow.filter_radius_milli, 1000,
        "default PCF spread must be one shadow-map texel, the baseline tap spread"
    );
    assert_eq!(
        resolved.grade.gamma_bp, POLICY_SCALE as i32,
        "default grade gamma must be identity"
    );
    for (label, actual) in [
        ("lift_r", resolved.grade.lift_r_bp),
        ("lift_g", resolved.grade.lift_g_bp),
        ("lift_b", resolved.grade.lift_b_bp),
        ("gain_r", resolved.grade.gain_r_bp),
        ("gain_g", resolved.grade.gain_g_bp),
        ("gain_b", resolved.grade.gain_b_bp),
        ("saturation", resolved.grade.saturation_bp),
    ] {
        assert_eq!(
            actual,
            if label.starts_with("lift") { 0 } else { POLICY_SCALE as i32 },
            "default grade {label} must be identity"
        );
    }
}

// ---------------------------------------------------------------------------
// BOUNDS — every rejection must be reachable and typed
// ---------------------------------------------------------------------------

/// The rejection DETAIL, which is where the offending field is named. The
/// stable `code` is `malformed` for every bounds violation by design (a caller
/// branches on the field name in the detail, not on a proliferation of codes),
/// so asserting on `code` here would assert twelve copies of "malformed".
fn code(result: Result<(), GraphicsContractError>) -> String {
    result.err().expect("expected rejection").message
}

#[test]
fn every_policy_bound_is_enforced() {
    let cases: Vec<(&str, RenderPolicy)> = vec![
        (
            "grade.lift_r_bp",
            RenderPolicy {
                grade: Some(GradePolicy {
                    lift_r_bp: -5000,
                    ..GradePolicy::default()
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "grade.gamma_bp",
            RenderPolicy {
                grade: Some(GradePolicy {
                    gamma_bp: 100,
                    ..GradePolicy::default()
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "grade.saturation_bp",
            RenderPolicy {
                grade: Some(GradePolicy {
                    saturation_bp: 30000,
                    ..GradePolicy::default()
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "bloom.intensity_bp",
            RenderPolicy {
                bloom: Some(BloomPolicy {
                    threshold_bp: 9000,
                    intensity_bp: 20000,
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "vignette.strength_bp",
            RenderPolicy {
                vignette: Some(VignettePolicy {
                    strength_bp: 9000,
                    radius_bp: 6000,
                    softness_bp: 3000,
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "vignette.radius_bp",
            RenderPolicy {
                vignette: Some(VignettePolicy {
                    strength_bp: 2000,
                    radius_bp: 500,
                    softness_bp: 3000,
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "vignette.softness_bp",
            RenderPolicy {
                vignette: Some(VignettePolicy {
                    strength_bp: 2000,
                    radius_bp: 6000,
                    softness_bp: 90000,
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "dither.amplitude_milli_lsb",
            RenderPolicy {
                dither: Some(DitherPolicy {
                    amplitude_milli_lsb: 9000,
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "terrain_surface.uv_repeat_scale_milli",
            RenderPolicy {
                terrain_surface: Some(TerrainSurfacePolicy {
                    uv_repeat_scale_milli: 0,
                    wrap_repeat: false,
                    macro_variation_bp: 0,
                    macro_frequency_milli: 40,
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "terrain_surface.macro_variation_bp",
            RenderPolicy {
                terrain_surface: Some(TerrainSurfacePolicy {
                    uv_repeat_scale_milli: 1000,
                    wrap_repeat: false,
                    macro_variation_bp: 9000,
                    macro_frequency_milli: 40,
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "terrain_surface.macro_frequency_milli",
            RenderPolicy {
                terrain_surface: Some(TerrainSurfacePolicy {
                    uv_repeat_scale_milli: 1000,
                    wrap_repeat: false,
                    macro_variation_bp: 0,
                    macro_frequency_milli: 0,
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "sampler.anisotropy",
            RenderPolicy {
                sampler: Some(SamplerPolicy { anisotropy: 0 }),
                ..RenderPolicy::default()
            },
        ),
        (
            "shadow.darkness_bp",
            RenderPolicy {
                shadow: Some(ShadowPolicy {
                    darkness_bp: POLICY_SCALE as i32 + 1,
                    filter_radius_milli: 1000,
                }),
                ..RenderPolicy::default()
            },
        ),
        (
            "shadow.filter_radius_milli",
            RenderPolicy {
                shadow: Some(ShadowPolicy {
                    darkness_bp: 7500,
                    // 99 milli-texels rounds below the one-texel baseline
                    // spread; a radius finer than the shadow map's own texel is
                    // a silent aliasing regression, not a style choice.
                    filter_radius_milli: 99,
                }),
                ..RenderPolicy::default()
            },
        ),
    ];

    let mut covered = 0usize;
    for (label, policy) in cases {
        let result = validate_render_policy(&policy);
        let message = code(result);
        assert!(
            message.contains(label),
            "rejection for {label} did not name the field: {message}"
        );
        covered += 1;
    }
    assert_eq!(
        covered, 14,
        "policy rejection coverage dropped: {covered}/14 bounds reachable"
    );
}

#[test]
fn anisotropy_above_sixteen_is_rejected() {
    let policy = RenderPolicy {
        sampler: Some(SamplerPolicy { anisotropy: 17 }),
        ..RenderPolicy::default()
    };
    assert!(code(validate_render_policy(&policy)).contains("anisotropy"));
}

#[test]
fn tiling_without_a_repeat_wrap_is_rejected() {
    // A cross-field rule, not a per-field bound. Clamped tiling smears the
    // border texel across most of the field, which looks like a texture bug and
    // reads as one. Both halves of the pair are individually legal.
    let policy = RenderPolicy {
        terrain_surface: Some(TerrainSurfacePolicy {
            uv_repeat_scale_milli: 8000,
            wrap_repeat: false,
            macro_variation_bp: 0,
            macro_frequency_milli: 40,
        }),
        ..RenderPolicy::default()
    };
    let message = code(validate_render_policy(&policy));
    assert!(
        message.contains("wrap_repeat"),
        "clamped tiling rejection did not name the field: {message}"
    );

    // The same tiling WITH a wrap is the supported configuration, so this test
    // also pins that the rule does not fire on legal input.
    let legal = RenderPolicy {
        terrain_surface: Some(TerrainSurfacePolicy {
            uv_repeat_scale_milli: 8000,
            wrap_repeat: true,
            macro_variation_bp: 0,
            macro_frequency_milli: 40,
        }),
        ..RenderPolicy::default()
    };
    assert!(validate_render_policy(&legal).is_ok());

    // And a single repeat needs no wrap, so the baseline-shaped default stays legal.
    assert!(validate_render_policy(&RenderPolicy::default()).is_ok());
}

#[test]
fn terrain_wrap_repeat_survives_serde_as_a_real_boolean() {
    // The receiver/producer agreement check: `wrap_repeat` must round-trip as a
    // JSON boolean, not as a number or a string. A receiver that coerced it
    // differently from the producer would silently disagree about sampling.
    let mut b = body();
    b.render_policy = Some(RenderPolicy {
        terrain_surface: Some(TerrainSurfacePolicy {
            uv_repeat_scale_milli: 8000,
            wrap_repeat: true,
            macro_variation_bp: 0,
            macro_frequency_milli: 40,
        }),
        ..RenderPolicy::default()
    });
    let value = serde_json::to_value(&b).expect("serialize");
    assert_eq!(
        value["render_policy"]["terrain_surface"]["wrap_repeat"],
        serde_json::Value::Bool(true),
        "wrap_repeat did not serialize as a JSON boolean"
    );
    let back: GraphicsScenePacketBody = serde_json::from_value(value).expect("deserialize");
    assert!(back.render_policy.unwrap().terrain_surface.unwrap().wrap_repeat);
}

#[test]
fn the_default_terrain_policy_is_bit_identical_to_the_frozen_baseline() {
    // 1000 milli is exactly one repeat, and clamp is the baseline sampler.
    // Together these are what make an absent policy reproduce the frozen bytes;
    // the previous unit ("repeats per world metre") would have meant 96 tiles.
    let default = TerrainSurfacePolicy::default();
    assert_eq!(default.uv_repeat_scale_milli, 1000);
    assert!(!default.wrap_repeat);
    assert_eq!(default.uv_repeat_scale_milli as f64 / 1000.0, 1.0);
}

#[test]
fn a_fully_engaged_vignette_with_no_falloff_is_rejected() {
    // A cross-field consistency rule, not a per-field bound: strength > 0 with
    // radius >= 1.0 would darken the entire frame uniformly, which is a bug in
    // a profile rather than a style choice.
    let policy = RenderPolicy {
        vignette: Some(VignettePolicy {
            strength_bp: 2000,
            radius_bp: 10000,
            softness_bp: 0,
        }),
        ..RenderPolicy::default()
    };
    assert!(code(validate_render_policy(&policy)).contains("no falloff"));
}

// ---------------------------------------------------------------------------
// ROUND TRIP + DETERMINISM
// ---------------------------------------------------------------------------

#[test]
fn a_full_policy_survives_serde_round_trip_and_revalidates() {
    let mut b = body();
    b.render_policy = Some(RenderPolicy {
        grade: Some(GradePolicy {
            lift_r_bp: 120,
            lift_g_bp: -60,
            lift_b_bp: 240,
            gamma_bp: 9800,
            gain_r_bp: 10400,
            gain_g_bp: 10200,
            gain_b_bp: 10600,
            saturation_bp: 9200,
        }),
        bloom: Some(BloomPolicy {
            threshold_bp: 8200,
            intensity_bp: 1400,
        }),
        vignette: Some(VignettePolicy {
            strength_bp: 2200,
            radius_bp: 6800,
            softness_bp: 3500,
        }),
        dither: Some(DitherPolicy {
            amplitude_milli_lsb: 1000,
        }),
        terrain_surface: Some(TerrainSurfacePolicy {
            uv_repeat_scale_milli: 8000,
            wrap_repeat: true,
            // Macro variation needs a layered terrain (N-4); this body has a
            // single-material terrain, so the round trip uses 0 and the refusal
            // of a non-zero value is asserted below.
            macro_variation_bp: 0,
            macro_frequency_milli: 55,
        }),
        sampler: Some(SamplerPolicy { anisotropy: 8 }),
        shadow: Some(ShadowPolicy {
            darkness_bp: 9600,
            filter_radius_milli: 2400,
        }),
        mesh_surface: Some(MeshSurfacePolicy { wrap_repeat: true }),
        shadow_fit: Some(ShadowFitPolicy { view_distance_m: 60, map_size_px: None }),
        sky: Some(SkyPolicy {
            sun_disc_radius_milli_deg: 650,
            sun_disc_gain_bp: 120_000,
            sun_glow_gain_bp: 2_400,
            model: Some(SkyModel::Analytic { turbidity_milli: 3000 }),
        }),
        atmosphere: Some(AtmospherePolicy {
            height_falloff_milli_per_m: 15,
            density_at_ground_bp: 50,
            sun_scatter_gain_bp: 300,
        }),
        debug: Some(DebugPolicy { albedo_override_bp: 5000 }),
        // Disabled: an enabled axis needs the baked textures (tests/ibl.rs).
        ibl: Some(IblPolicy { enabled: false }),
        foliage_coverage: Some(FoliageCoveragePolicy { samples: 4 }),
    });
    b.schema_version = SCENE_PACKET_SCHEMA.into();
    validate_render_policy(b.render_policy.as_ref().unwrap()).expect("full policy validates");
    let mut decorative = b.clone();
    if let Some(terrain) = decorative.render_policy.as_mut().unwrap().terrain_surface.as_mut() {
        terrain.macro_variation_bp = 900;
    }
    assert!(
        seal_scene_packet(decorative).is_err(),
        "macro variation on an unlayered terrain would be a decorative axis"
    );
    let packet = seal_scene_packet(b).expect("seals");
    validate_scene_packet(&packet).expect("validates end to end");

    let json = serde_json::to_value(&packet.body).expect("serialize");
    let back: GraphicsScenePacketBody = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back.render_policy, packet.body.render_policy);
    let resealed = seal_scene_packet(back).expect("re-seals");
    assert_eq!(
        resealed.packet_sha256, packet.packet_sha256,
        "policy round trip was not digest-stable"
    );
}

#[test]
fn unknown_policy_fields_fail_closed() {
    let mut b = body();
    b.render_policy = Some(RenderPolicy::default());
    let mut value = serde_json::to_value(&b).expect("serialize");
    value["render_policy"]["secret_knob"] = serde_json::json!(1);
    let parsed: Result<GraphicsScenePacketBody, _> = serde_json::from_value(value);
    assert!(
        parsed.is_err(),
        "an unknown render-policy field was accepted: the policy surface is not closed"
    );
}

#[test]
fn source_digest_is_bound_to_the_policy() {
    // Sanity that the fixture we mutate is really the committed packet.
    let b = body();
    assert!(b.world_artifact_sha256.starts_with("sha256:"));
    assert_ne!(b.world_artifact_sha256, SOURCE_SHA);
}

#[test]
fn foliage_coverage_takes_two_four_or_eight_samples() {
    for samples in [2u8, 4, 8] {
        let policy = RenderPolicy { foliage_coverage: Some(FoliageCoveragePolicy { samples }), ..RenderPolicy::default() };
        validate_render_policy(&policy).expect("valid sample count");
    }
    for samples in [0u8, 1, 3, 16] {
        let policy = RenderPolicy { foliage_coverage: Some(FoliageCoveragePolicy { samples }), ..RenderPolicy::default() };
        let error = validate_render_policy(&policy).expect_err("refused");
        assert!(error.to_string().contains("foliage_coverage"), "{error}");
    }
    // Absent serialises to nothing.
    let json = String::from_utf8(serde_json::to_vec(&RenderPolicy::default()).unwrap()).unwrap();
    assert!(!json.contains("foliage_coverage"));
}
