//! CONVERGE-3 gates (`WGE_CONVERGE3_CONTRACTS.md`).
//!
//! W-1 still water, R-1 ruin damage. Tests that lower `converge3` need the
//! built kits, like the N-5 ones: `python3 tools/build_kit.py --set kit1` and
//! `--set kit2`, then `cargo test --test converge3 -- --include-ignored`.

use std::path::PathBuf;

use wge_native_graphics_contract::still_water::{POOL_RADII_XZ_M, TerrainSurface};
use wge_native_graphics_contract::{
    BufferPayload, Campaign2Inputs, Campaign2View, GraphicsScenePacket, KitSet, ParityContent, ParityPolicyCandidate,
    StandingWater, TerrainWetZone, canonical_json, load_kit_set, lower_campaign2_packet_inputs, seal_scene_packet,
    validate_scene_packet,
};

#[path = "support/synthetic_layers.rs"]
mod synthetic_layers;
use synthetic_layers::synthetic_set;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn reference() -> GraphicsScenePacket {
    let path =
        repo_root().join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("reference packet")).expect("parses")
}

const CONVERGE: ParityContent = ParityContent {
    hero_materials: false,
    converge0: true,
};

fn kit_set(name: &str) -> KitSet {
    load_kit_set(&repo_root().join(format!("tools/kit/{name}.lock.json")), &repo_root())
        .unwrap_or_else(|e| panic!("built {name} loads (run tools/build_kit.py --set {name}): {e}"))
}

fn kit() -> KitSet {
    kit_set("kit2")
}

fn lower(candidate: ParityPolicyCandidate, kit: &KitSet, view: Campaign2View) -> GraphicsScenePacket {
    let layers = synthetic_set();
    lower_campaign2_packet_inputs(
        &reference(),
        view,
        &Campaign2Inputs {
            candidate,
            content: CONVERGE,
            terrain_layers: Some(&layers),
            kit: Some(kit),
        },
    )
    .expect("lowers")
}

fn heights(packet: &GraphicsScenePacket) -> &[f32] {
    match &packet.body.terrain.heights_m.payload {
        BufferPayload::F32(values) => values,
        _ => panic!("f32 heights"),
    }
}

fn ellipse(center: [f32; 2], x: f32, z: f32) -> f32 {
    (((x - center[0]) / POOL_RADII_XZ_M[0]).powi(2) + ((z - center[1]) / POOL_RADII_XZ_M[1]).powi(2)).sqrt()
}

#[test]
#[ignore = "needs the built kit (python3 tools/build_kit.py)"]
fn w1_still_water_replaces_the_disc_and_its_rings() {
    let kit = kit();
    for view in [Campaign2View::Close, Campaign2View::Medium, Campaign2View::Wide] {
        let packet = lower(ParityPolicyCandidate::Converge3, &kit, view);
        validate_scene_packet(&packet).expect("valid");
        let body = &packet.body;
        assert!(
            !body
                .instances
                .iter()
                .any(|i| i.instance_id.starts_with("campaign2-wet")),
            "{view:?}: converge2 pool left"
        );
        assert!(
            !body.meshes.iter().any(|m| m.mesh_id.starts_with("campaign2-wet")),
            "{view:?}: converge2 pool mesh left"
        );
        assert!(
            !body
                .materials
                .iter()
                .any(|m| m.material_id == "campaign2-wet" || m.material_id == "campaign2-hero-glow")
        );
        assert!(
            !body.textures.iter().any(|t| t.texture_id == "wge-campaign2-wet-albedo"),
            "orphan ripple albedo"
        );
        assert!(
            !body
                .materials
                .iter()
                .any(|m| m.emissive_factor_rgb != [0.0; 3] && m.material_id.contains("wet")),
            "nothing wet glows"
        );
        let zone: &TerrainWetZone = body.terrain.wet_zone.as_ref().expect("wet zone");
        assert_eq!(zone.radii_xz_m, POOL_RADII_XZ_M);
        let water = zone.standing_water.expect("standing water");
        assert!(
            water.roughness <= 0.05 && water.albedo_rgb.iter().all(|c| *c < 0.05),
            "dark, smooth water"
        );
        // converge2's render policy, camera and terrain heights are unchanged by W-1.
        let converge2 = lower(ParityPolicyCandidate::Converge2, &kit_set("kit1"), view);
        // L-2a: converge3's policy is converge2's plus foliage coverage.
        let mut expected = converge2.body.render_policy.clone().expect("converge2 policy");
        expected.foliage_coverage = Some(wge_native_graphics_contract::FoliageCoveragePolicy { samples: 4 });
        assert_eq!(body.render_policy, Some(expected));
        assert_eq!(body.camera, converge2.body.camera);
        assert_eq!(heights(&packet), heights(&converge2), "W-1 does not move the ground");
        assert!(converge2.body.terrain.wet_zone.is_none());
        assert!(
            !String::from_utf8(canonical_json(&converge2).unwrap())
                .unwrap()
                .contains("wet_zone")
        );
    }
}

#[test]
#[ignore = "needs the built kit (python3 tools/build_kit.py)"]
fn w1_the_kit_stands_on_the_rasterised_surface_and_out_of_the_water() {
    let kit = kit();
    let packet = lower(ParityPolicyCandidate::Converge3, &kit, Campaign2View::Close);
    let surface = TerrainSurface::of(&packet.body.terrain).unwrap();
    let zone = packet.body.terrain.wet_zone.unwrap();
    let wobble = zone.standing_water.unwrap().edge_wobble;
    // Fallen blocks are grounded block by block (`r1_fallen_blocks_rest_on_the_ground`).
    for instance in packet
        .body
        .instances
        .iter()
        .filter(|i| i.instance_id.starts_with("kit-") && !i.mesh_id.contains("fallen"))
    {
        let [x, y, z] = instance.transform.translation_xyz_m;
        let ground = surface.height(x, z);
        // Rocks are sunk by design (18 cm x scale); everything else stands on the ground.
        let id = instance.instance_id.as_str();
        let expected = if id.starts_with("kit-rock") {
            ground - 0.18 * instance.transform.scale_xyz[0]
        } else {
            ground
        };
        assert!((y - expected).abs() < 1e-4, "{id}: y {y} vs ground {expected}");
        // Rocks may sit on the shore, half in the water; plants stand dry.
        let e = ellipse(zone.center_xz_m, x, z);
        let dry_from = if id.starts_with("kit-rock") {
            1.0 - wobble
        } else {
            1.0 + wobble
        };
        if !id.starts_with("kit-ruin") {
            assert!(e > dry_from, "{id} stands in the water (e = {e})");
        }
    }
}

#[test]
fn w1_wet_zone_needs_layers_and_sane_values() {
    let base = reference();
    let zone = TerrainWetZone {
        center_xz_m: [0.0, 0.0],
        radii_xz_m: [3.0, 2.0],
        falloff_m: 0.5,
        albedo_scale: 0.6,
        roughness_scale: 0.25,
        normal_scale: 0.4,
        standing_water: None,
    };
    let with = |zone: TerrainWetZone| {
        let mut body = base.body.clone();
        body.terrain.wet_zone = Some(zone);
        seal_scene_packet(body).and_then(|packet| validate_scene_packet(&packet).map(|_| packet))
    };
    let error = with(zone).expect_err("the baseline terrain has no layers");
    assert!(error.to_string().contains("layered terrain"), "{error}");
    // With layers present the zone's values are checked.
    let layers = synthetic_set();
    let layered = lower_campaign2_packet_inputs(
        &base,
        Campaign2View::Close,
        &Campaign2Inputs {
            candidate: ParityPolicyCandidate::Converge1,
            content: CONVERGE,
            terrain_layers: Some(&layers),
            kit: None,
        },
    )
    .unwrap();
    for bad in [
        TerrainWetZone {
            albedo_scale: 1.2,
            ..zone
        },
        TerrainWetZone {
            roughness_scale: 0.0,
            ..zone
        },
        TerrainWetZone {
            radii_xz_m: [0.0, 2.0],
            ..zone
        },
        TerrainWetZone {
            falloff_m: f32::NAN,
            ..zone
        },
        TerrainWetZone {
            standing_water: Some(StandingWater {
                albedo_rgb: [0.02; 3],
                roughness: 0.02,
                edge_wobble: 0.8,
                wobble_cycles_per_m: 1.0,
                surface_y_m: 0.0,
            }),
            ..zone
        },
    ] {
        let mut body = layered.body.clone();
        body.terrain.wet_zone = Some(bad);
        let error = seal_scene_packet(body)
            .and_then(|p| validate_scene_packet(&p))
            .expect_err("refused");
        assert!(error.to_string().contains("wet_zone"), "{error}");
    }
    let mut body = layered.body.clone();
    body.terrain.wet_zone = Some(zone);
    validate_scene_packet(&seal_scene_packet(body).unwrap()).expect("a sane zone on layered terrain is valid");
}

#[test]
#[ignore = "needs the built kits (python3 tools/build_kit.py --set kit1|kit2)"]
fn r1_each_kit_arm_renders_its_own_kit_set() {
    let layers = synthetic_set();
    let lower_with = |candidate, kit: &KitSet| {
        lower_campaign2_packet_inputs(
            &reference(),
            Campaign2View::Close,
            &Campaign2Inputs {
                candidate,
                content: CONVERGE,
                terrain_layers: Some(&layers),
                kit: Some(kit),
            },
        )
    };
    let error = lower_with(ParityPolicyCandidate::Converge3, &kit_set("kit1")).expect_err("converge3 refuses kit1");
    assert!(error.to_string().contains("kit2"), "{error}");
    let error = lower_with(ParityPolicyCandidate::Converge2, &kit_set("kit2")).expect_err("converge2 refuses kit2");
    assert!(error.to_string().contains("kit1"), "{error}");
}

#[test]
#[ignore = "needs the built kits (python3 tools/build_kit.py --set kit1|kit2)"]
fn r1_kit2_differs_from_kit1_only_in_the_ruin() {
    let lock = |name: &str| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(repo_root().join(format!("tools/kit/{name}.lock.json"))).unwrap())
            .unwrap()
    };
    let (kit1, kit2) = (lock("kit1"), lock("kit2"));
    for asset in ["rock_a", "rock_b", "tree", "fern"] {
        assert_eq!(
            kit1["assets"][asset]["sha256"], kit2["assets"][asset]["sha256"],
            "{asset} changed"
        );
    }
    assert_ne!(kit1["assets"]["ruin"]["sha256"], kit2["assets"]["ruin"]["sha256"]);
    // Budget: the damaged ruin grows by <= 0.5 MB.
    let grow = kit2["assets"]["ruin"]["bytes"].as_i64().unwrap() - kit1["assets"]["ruin"]["bytes"].as_i64().unwrap();
    assert!(grow <= 500_000, "ruin grew {grow} bytes");
}

#[test]
#[ignore = "needs the built kits (python3 tools/build_kit.py --set kit1|kit2)"]
fn r1_fallen_blocks_rest_on_the_ground() {
    let kit = kit();
    let packet = lower(ParityPolicyCandidate::Converge3, &kit, Campaign2View::Close);
    let surface = TerrainSurface::of(&packet.body.terrain).unwrap();
    let mut blocks = 0;
    for instance in packet
        .body
        .instances
        .iter()
        .filter(|i| i.instance_id.starts_with("kit-ruin"))
    {
        if !instance.mesh_id.contains("fallen") {
            continue;
        }
        blocks += 1;
        let mesh = packet
            .body
            .meshes
            .iter()
            .find(|m| m.mesh_id == instance.mesh_id)
            .unwrap();
        let t = &instance.transform;
        let [qx, qy, qz, qw] = t.rotation_xyzw;
        let rotate = |v: [f32; 3]| {
            // q v q*, for a unit quaternion.
            let u = [qx, qy, qz];
            let dot = u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let uu = dot * 2.0;
            let s = qw * qw - (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]);
            [0, 1, 2].map(|i| uu * u[i] + s * v[i] + 2.0 * qw * cross[i])
        };
        // Depth of the block's lowest point below the ground under it.
        let mut deepest_below = f32::NEG_INFINITY;
        let mut lowest_gap = f32::INFINITY;
        for p in &mesh.positions_m {
            let r = rotate([p[0] * t.scale_xyz[0], p[1] * t.scale_xyz[1], p[2] * t.scale_xyz[2]]);
            let world = [
                r[0] + t.translation_xyz_m[0],
                r[1] + t.translation_xyz_m[1],
                r[2] + t.translation_xyz_m[2],
            ];
            let gap = world[1] - surface.height(world[0], world[2]);
            lowest_gap = lowest_gap.min(gap);
            deepest_below = deepest_below.max(-gap);
        }
        assert!(
            lowest_gap <= 0.0,
            "{} floats {lowest_gap} m above the ground",
            instance.instance_id
        );
        assert!(
            deepest_below < 0.45,
            "{} is buried {deepest_below} m",
            instance.instance_id
        );
    }
    assert!(blocks >= 3, "expected at least three fallen blocks, found {blocks}");
}

#[test]
#[ignore = "needs the built kits (python3 tools/build_kit.py --set kit1|kit2)"]
fn n7_one_scattered_world_in_every_view_with_clear_sightlines() {
    let kit = kit();
    let views = [Campaign2View::Close, Campaign2View::Medium, Campaign2View::Wide];
    let packets: Vec<GraphicsScenePacket> = views
        .iter()
        .map(|v| lower(ParityPolicyCandidate::Converge3, &kit, *v))
        .collect();
    let scattered = |p: &GraphicsScenePacket| -> Vec<(String, [f32; 3])> {
        p.body
            .instances
            .iter()
            .filter(|i| i.instance_id.contains("-s0") || i.instance_id.contains("-s1"))
            .map(|i| (i.instance_id.clone(), i.transform.translation_xyz_m))
            .collect()
    };
    let world = scattered(&packets[0]);
    let trees = world.iter().filter(|(id, _)| id.starts_with("kit-tree-s")).count();
    assert!(trees > 50, "only {trees} tree instances");
    for p in &packets[1..] {
        assert_eq!(scattered(p), world, "every view renders the same scattered world");
    }
    // Every view's camera sees the ruin past no tree: no tree trunk within 3 m
    // of the camera-to-ruin segment.
    let ruin = packets[0]
        .body
        .instances
        .iter()
        .find(|i| i.instance_id == "kit-ruin-00")
        .unwrap()
        .transform
        .translation_xyz_m;
    for p in &packets {
        let c = p.body.camera.position_xyz_m;
        for (id, t) in world.iter().filter(|(id, _)| id.starts_with("kit-tree-s")) {
            let d = [ruin[0] - c[0], ruin[2] - c[2]];
            let s = (((t[0] - c[0]) * d[0] + (t[2] - c[2]) * d[1]) / (d[0] * d[0] + d[1] * d[1])).clamp(0.0, 1.0);
            let q = [c[0] + s * d[0], c[2] + s * d[1]];
            assert!(
                (t[0] - q[0]).hypot(t[2] - q[1]) >= 3.0,
                "{id} stands in {}'s sightline",
                p.body.camera.camera_id
            );
        }
    }
    // Scattered instances vary; converge2's do not.
    assert!(
        packets[0]
            .body
            .instances
            .iter()
            .filter(|i| i.instance_id.contains("-s0"))
            .all(|i| i.variation.is_some())
    );
    let converge2 = lower(ParityPolicyCandidate::Converge2, &kit_set("kit1"), Campaign2View::Close);
    assert!(converge2.body.instances.iter().all(|i| i.variation.is_none()));
    assert!(
        !String::from_utf8(canonical_json(&converge2).unwrap())
            .unwrap()
            .contains("\"variation\":")
    );
}
