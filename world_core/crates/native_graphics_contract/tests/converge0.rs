//! CONVERGE-0 gates (WGE_GRAPHICS_CONVERGENCE_AUDIT.md §H).
//!
//! Every gate here has a CONTROL: the measurement is shown to fire on the arm
//! that has the defect before it is trusted to pass on the arm that fixes it.
//! A gate that has never been seen to fail is a gate nobody knows works.

use std::path::PathBuf;

use wge_native_graphics_contract::material_maps::mean_linear_rgb;
use wge_native_graphics_contract::{
    BufferPayload, Campaign2View, GraphicsScenePacket, ParityContent, ParityPolicyCandidate,
    RenderPolicy, ShadowFitPolicy, SkyPolicy, lower_campaign2_packet_with, validate_render_policy,
    world_void_fraction_bp, CONVERGE0_TERRAIN_SCALE,
};

const VIEWS: [Campaign2View; 3] = [Campaign2View::Close, Campaign2View::Medium, Campaign2View::Wide];

fn reference() -> GraphicsScenePacket {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    let text = std::fs::read_to_string(&path).expect("committed reference packet");
    serde_json::from_str(&text).expect("reference packet parses")
}

fn content(hero_materials: bool, converge0: bool) -> ParityContent {
    ParityContent { hero_materials, converge0 }
}

fn lower(view: Campaign2View, candidate: ParityPolicyCandidate, flags: ParityContent) -> GraphicsScenePacket {
    lower_campaign2_packet_with(&reference(), view, candidate, flags).expect("campaign2 lowers")
}

fn converge0(view: Campaign2View) -> GraphicsScenePacket {
    lower(view, ParityPolicyCandidate::Converge0, content(false, true))
}

fn null(view: Campaign2View) -> GraphicsScenePacket {
    lower(view, ParityPolicyCandidate::Null, ParityContent::default())
}

#[test]
fn content_flags_parse_and_fail_closed() {
    use ParityPolicyCandidate as P;
    assert_eq!(ParityContent::parse("", P::Null).unwrap(), ParityContent::default());
    assert_eq!(
        ParityContent::parse("hero-materials", P::Full).unwrap(),
        content(true, false),
        "MD-5: hero content must compose with the full render policy"
    );
    assert_eq!(
        ParityContent::parse("", P::HeroMaterials).unwrap(),
        content(true, false),
        "the legacy RENDER_POLICY=hero-materials spelling must still reproduce ab-hero3"
    );
    assert_eq!(ParityContent::parse("", P::Converge0).unwrap(), content(false, true));
    assert_eq!(
        ParityContent::parse(" hero-materials , converge0 ", P::Converge0).unwrap(),
        content(true, true)
    );
    assert!(ParityContent::parse("hero-material", P::Full).is_err(), "typos must not render the baseline");
    assert!(
        ParityContent::parse("converge0", P::Full).is_err(),
        "converge0 content without its policy would sample metric UVs through a clamp sampler"
    );
    assert!(
        lower_campaign2_packet_with(&reference(), Campaign2View::Close, P::Full, content(false, true)).is_err(),
        "the env-free entry point must enforce the same coupling"
    );
}

#[test]
fn converge0_policy_carries_its_axes_and_full_json_is_unchanged() {
    let full = ParityPolicyCandidate::Full.policy().unwrap();
    let converge = ParityPolicyCandidate::Converge0.policy().unwrap();
    validate_render_policy(&converge).expect("converge0 policy is in bounds");
    assert!(converge.mesh_surface.is_some_and(|mesh| mesh.wrap_repeat));
    assert!(converge.shadow_fit.is_some());
    assert!(converge.sky.is_some());
    assert_eq!(converge.dither, full.dither);
    assert_eq!(converge.shadow, full.shadow);
    // Absent axes must not serialise: `full` packets keep their digests.
    let full_json = serde_json::to_string(&full).unwrap();
    for key in ["mesh_surface", "shadow_fit", "sky"] {
        assert!(!full_json.contains(key), "`full` policy JSON gained `{key}`: {full_json}");
    }
}

#[test]
fn new_policy_axes_are_bounded() {
    let mut fit = RenderPolicy { shadow_fit: Some(ShadowFitPolicy { view_distance_m: 7 }), ..RenderPolicy::default() };
    assert!(validate_render_policy(&fit).is_err());
    fit.shadow_fit = Some(ShadowFitPolicy { view_distance_m: 2001 });
    assert!(validate_render_policy(&fit).is_err());
    fit.shadow_fit = Some(ShadowFitPolicy { view_distance_m: 60 });
    assert!(validate_render_policy(&fit).is_ok());
    let sky = |radius, disc, glow| RenderPolicy {
        sky: Some(SkyPolicy { sun_disc_radius_milli_deg: radius, sun_disc_gain_bp: disc, sun_glow_gain_bp: glow }),
        ..RenderPolicy::default()
    };
    assert!(validate_render_policy(&sky(650, 120_000, 2400)).is_ok());
    assert!(validate_render_policy(&sky(5001, 0, 0)).is_err());
    assert!(validate_render_policy(&sky(0, 400_001, 0)).is_err());
    assert!(validate_render_policy(&sky(0, 0, 20001)).is_err());
    assert!(validate_render_policy(&sky(-1, 0, 0)).is_err());
}

#[test]
fn converge0_terrain_embeds_the_source_exactly() {
    let source = reference();
    let lowered = converge0(Campaign2View::Close);
    let (src, ext) = (&source.body.terrain, &lowered.body.terrain);
    let k = CONVERGE0_TERRAIN_SCALE;
    assert_eq!(ext.resolution, (src.resolution - 1) * k + 1);
    assert_eq!(ext.width_m, src.width_m * k as f32);
    assert_eq!(ext.length_m, src.length_m * k as f32);
    let (BufferPayload::F32(src_h), BufferPayload::F32(ext_h)) = (&src.heights_m.payload, &ext.heights_m.payload)
    else {
        panic!("f32 heights");
    };
    let offset = (src.resolution - 1) * (k - 1) / 2;
    for row in 0..src.resolution {
        for column in 0..src.resolution {
            assert_eq!(
                ext_h[(row + offset) * ext.resolution + column + offset].to_bits(),
                src_h[row * src.resolution + column].to_bits(),
                "source height ({row},{column}) was resampled, not copied"
            );
        }
    }
    // Continuity: the first ring outside the source must stay within 1.5 m of
    // the edge sample it grows from (1.5-2.0 m grid spacing, gentle blend).
    let n = src.resolution;
    for column in 0..n {
        let edge = src_h[column];
        let outside = ext_h[(offset - 1) * ext.resolution + column + offset];
        assert!((outside - edge).abs() < 1.5, "seam step {} at column {column}", outside - edge);
    }
    // The null arm must keep the source terrain verbatim.
    assert_eq!(null(Campaign2View::Close).body.terrain, source.body.terrain);
}

#[test]
fn converge0_shows_no_void_below_the_horizon_and_null_does() {
    for view in VIEWS {
        let control = world_void_fraction_bp(&null(view), 4);
        let fixed = world_void_fraction_bp(&converge0(view), 4);
        eprintln!("{view:?}: void fraction null={control} bp, converge0={fixed} bp");
        if view != Campaign2View::Close {
            // The audit observed the slab edge and void in medium and wide.
            assert!(control > 100, "{view:?}: control shows no void ({control} bp); the measure is blind");
        }
        assert_eq!(fixed, 0, "{view:?}: converge0 still shows {fixed} bp of void");
    }
}

/// Area-weighted p90 of per-triangle texel anisotropy (ratio of the singular
/// values of dP/dUV) for every instance of `material_ids`, in world metres.
fn anisotropy_p90_by_mesh(packet: &GraphicsScenePacket, material_ids: &[&str]) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    for instance in &packet.body.instances {
        if !material_ids.contains(&instance.material_id.as_str()) {
            continue;
        }
        let mesh = packet.body.meshes.iter().find(|mesh| mesh.mesh_id == instance.mesh_id).unwrap();
        let scale = instance.transform.scale_xyz;
        let world = |index: u32| {
            let p = mesh.positions_m[index as usize];
            [p[0] * scale[0], p[1] * scale[1], p[2] * scale[2]]
        };
        let mut samples: Vec<(f32, f32)> = Vec::new();
        for triangle in mesh.indices.chunks_exact(3) {
            let [p0, p1, p2] = [world(triangle[0]), world(triangle[1]), world(triangle[2])];
            let [t0, t1, t2] = [triangle[0], triangle[1], triangle[2]].map(|i| mesh.uv0[i as usize]);
            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let cross = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let area = 0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
            let (d1, d2) = ([t1[0] - t0[0], t1[1] - t0[1]], [t2[0] - t0[0], t2[1] - t0[1]]);
            let det = d1[0] * d2[1] - d2[0] * d1[1];
            if area < 1e-7 || det.abs() < 1e-9 {
                continue;
            }
            let tu: [f32; 3] = std::array::from_fn(|a| (e1[a] * d2[1] - e2[a] * d1[1]) / det);
            let tv: [f32; 3] = std::array::from_fn(|a| (e2[a] * d1[0] - e1[a] * d2[0]) / det);
            let dot = |x: [f32; 3], y: [f32; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
            let (a, b, c) = (dot(tu, tu), dot(tu, tv), dot(tv, tv));
            let mid = 0.5 * (a + c);
            let spread = (0.25 * (a - c) * (a - c) + b * b).sqrt();
            let ratio = ((mid + spread) / (mid - spread).max(1e-12)).sqrt();
            samples.push((ratio, area));
        }
        samples.sort_by(|x, y| x.0.total_cmp(&y.0));
        let total: f32 = samples.iter().map(|sample| sample.1).sum();
        let mut accumulated = 0.0;
        let mut p90 = f32::INFINITY;
        for (ratio, area) in &samples {
            accumulated += area;
            if accumulated >= 0.9 * total {
                p90 = *ratio;
                break;
            }
        }
        out.push((format!("{} [{}]", instance.instance_id, instance.mesh_id), p90));
    }
    out
}

const SURFACE_MATERIALS: [&str; 6] = [
    "campaign2-hero-stone",
    "campaign2-hero-metal",
    "campaign2-wet",
    "campaign2-bark",
    "campaign2-foliage",
    "campaign2-rock",
];

#[test]
fn converge0_surface_meshes_have_metric_texel_density() {
    let control = anisotropy_p90_by_mesh(&null(Campaign2View::Close), &["campaign2-hero-stone"]);
    let pedestal = control.iter().find(|(id, _)| id.contains("pedestal")).expect("pedestal").1;
    eprintln!("control: parametric pedestal p90 anisotropy = {pedestal:.1}");
    assert!(pedestal > 10.0, "control pedestal p90 {pedestal}: the measure cannot see the MD-4 smear");

    let measured = anisotropy_p90_by_mesh(&converge0(Campaign2View::Close), &SURFACE_MATERIALS);
    assert!(measured.len() >= 10, "expected the hero and foliage instances, got {}", measured.len());
    for (id, p90) in &measured {
        // Lat-long ellipsoids pinch at the poles: analytic p90 for a sphere is
        // 2.29. Documented in `ellipsoid_mesh_metric`; every other surface
        // mesh must meet the audit's 2:1.
        let limit = if id.contains("crown") || id.contains("lobe") { 2.5 } else { 2.0 };
        eprintln!("converge0 {id}: p90 anisotropy {p90:.2} (limit {limit})");
        assert!(*p90 <= limit, "{id}: p90 texel anisotropy {p90:.2} exceeds {limit}");
    }
}

#[test]
fn converge0_metallic_is_physical() {
    for material in &converge0(Campaign2View::Close).body.materials {
        if !material.material_id.starts_with("campaign2-") && material.material_id != "objective-beacon" {
            continue;
        }
        assert!(
            material.metallic == 0.0 || material.metallic == 1.0,
            "{} has non-physical metallic {}",
            material.material_id,
            material.metallic
        );
    }
}

#[test]
fn hero_materials_compose_with_full_and_preserve_declared_colour() {
    let plain = lower(Campaign2View::Close, ParityPolicyCandidate::Full, ParityContent::default());
    let hero = lower(Campaign2View::Close, ParityPolicyCandidate::Full, content(true, false));
    assert_eq!(hero.body.render_policy, plain.body.render_policy, "MD-5: hero content left the full policy intact");

    let chroma = |packet: &GraphicsScenePacket, id: &str| {
        let material = packet.body.materials.iter().find(|m| m.material_id == id).unwrap();
        let texture = packet
            .body
            .textures
            .iter()
            .find(|t| t.texture_id == material.texture_ids[0])
            .unwrap();
        let mean = mean_linear_rgb(texture).unwrap();
        let rgb: [f32; 3] = std::array::from_fn(|c| material.base_color_rgba[c] * mean[c]);
        let sum = rgb.iter().sum::<f32>();
        (rgb.map(|value| value / sum), 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2], material.roughness)
    };
    for id in [
        "campaign2-hero-metal",
        "campaign2-hero-stone",
        "campaign2-bark",
        "campaign2-foliage",
        "campaign2-wet",
        "terrain-default",
    ] {
        let (before, before_luma, _) = chroma(&plain, id);
        let (after, after_luma, roughness) = chroma(&hero, id);
        eprintln!(
            "{id}: chromaticity {before:.3?} -> {after:.3?}, luminance {before_luma:.3} -> {after_luma:.3}"
        );
        assert_eq!(roughness, 1.0, "MD-1: {id} still multiplies an absolute roughness map by a factor");
        for channel in 0..3 {
            assert!(
                (before[channel] - after[channel]).abs() < 0.01,
                "MD-1: {id} changed hue: {before:?} -> {after:?}"
            );
        }
    }
    let (metal, _, _) = chroma(&hero, "campaign2-hero-metal");
    assert!(metal[0] > metal[1] && metal[1] > metal[2], "hero metal is no longer copper-ordered: {metal:?}");
}
