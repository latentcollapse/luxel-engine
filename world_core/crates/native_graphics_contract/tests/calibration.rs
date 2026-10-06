//! CALIBRATION-1 gates (docs/world/converge/converge1-contracts.md §3).
//!
//! Offline: the material manifest is complete, and each enumerated rig is a
//! valid, distinct policy. With the fetched GLB (`#[ignore]`, run explicitly
//! after tools/fetch_calibration_materials.py and tools/build_calibration_glb.py):
//! the GLB passes `prepare` and render conditioning with ZERO findings, and every
//! derived view frames its subject. The ±5% grey-card gate needs the GPU: it is
//! enforced by `render-calibration` itself, which writes
//! calibration_summary.json and then exits non-zero if any sun- or overcast-rig
//! view that frames the 18% card measures outside ±5% of GREY_CARD_TARGET_SRGB8.

use std::path::PathBuf;

use wge_asset_contract::{PreparationStatus, RenderPreparationStatus, condition_render_asset, prepare_asset};
use wge_native_graphics_contract::calibration::{
    CALIBRATION_COLUMNS, GREY_CARD_TARGET_SRGB8, grey_card_local_center, project_local_point, render_request,
    runtime_request,
};
use wge_native_graphics_contract::{
    CalibrationPlacement, CalibrationRig, CalibrationView, GraphicsScenePacket, LightKind, validate_render_policy,
};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[test]
fn material_manifest_pins_every_file() {
    let text = std::fs::read_to_string(repo().join("tools/calibration_materials/calibration1.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(manifest["license"], "CC0-1.0");
    let materials = manifest["materials"].as_array().unwrap();
    let assets: Vec<&str> = materials.iter().map(|m| m["source_asset"].as_str().unwrap()).collect();
    assert_eq!(
        assets,
        ["metal_plate", "rock_wall_08", "bark_willow_02", "wood_planks_grey", "painted_plaster_wall", "forrest_ground_01"],
        "contract §3 verified-inputs table"
    );
    let hex = |s: &str, n: usize| s.len() == n && s.chars().all(|c| c.is_ascii_hexdigit());
    for material in materials {
        let asset = material["source_asset"].as_str().unwrap();
        assert!(material["dimensions_mm"][0].as_u64().unwrap() > 0, "{asset}: physical size sets the UV scale");
        assert!(!material["authors"].as_array().unwrap().is_empty(), "{asset}: authors");
        let maps = material["maps"].as_object().unwrap();
        for required in ["albedo", "normal_gl", "roughness", "ao"] {
            assert!(maps.contains_key(required), "{asset} lacks {required}");
        }
        assert_eq!(maps.contains_key("metal"), asset == "metal_plate", "only metal_plate carries a metal map");
        for (name, entry) in maps {
            let url = entry["url"].as_str().unwrap_or("");
            assert!(url.starts_with("https://dl.polyhaven.org/") && url.ends_with("_1k.jpg"), "{asset}/{name} url {url}");
            assert!(entry["bytes"].as_u64().unwrap_or(0) > 0, "{asset}/{name} byte size");
            let sha = entry["sha256"].as_str().unwrap_or("");
            assert!(sha.starts_with("sha256:") && hex(&sha[7..], 64), "{asset}/{name} sha256 {sha}");
            assert!(hex(entry["provider_md5"].as_str().unwrap_or(""), 32), "{asset}/{name} provider md5");
        }
    }
}

#[test]
fn rigs_are_valid_distinct_policies() {
    let reference: GraphicsScenePacket = serde_json::from_str(
        &std::fs::read_to_string(
            repo().join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let lit = |rig: CalibrationRig| {
        let mut body = reference.body.clone();
        rig.apply(&mut body).unwrap();
        body
    };
    for rig in CalibrationRig::ALL {
        let body = lit(rig);
        validate_render_policy(body.render_policy.as_ref().unwrap()).unwrap_or_else(|e| panic!("{rig:?}: {e}"));
        let policy = body.render_policy.unwrap();
        assert!(policy.bloom.is_none() && policy.vignette.is_none() && policy.grade.is_none(), "{rig:?}: no post");
        assert!(policy.mesh_surface.is_some_and(|m| m.wrap_repeat), "{rig:?}: metric UVs need repeat");
        assert_eq!(body.environment.fog_density, 0.0, "{rig:?}: calibration is unfogged");
        assert_eq!(body.lights.len(), 1);
        let LightKind::Directional { direction_xyz } = body.lights[0].kind else { panic!("directional") };
        let elevation = (-direction_xyz[1] / direction_xyz.iter().map(|v| v * v).sum::<f32>().sqrt()).asin().to_degrees();
        match rig.lighting() {
            CalibrationRig::Sun | CalibrationRig::SunAlbedoGrey => assert!((elevation - 35.0).abs() < 0.05, "{elevation}"),
            CalibrationRig::Grazing => assert!((elevation - 10.0).abs() < 0.05, "{elevation}"),
            CalibrationRig::Overcast => assert_eq!(body.lights[0].intensity, 0.0, "overcast has no sun"),
            other => panic!("lighting() returned an IBL rig {other:?}"),
        }
        let ibl_textures = body.textures.iter().filter(|t| t.texture_id.starts_with("wge-ibl-")).count();
        assert_eq!(ibl_textures, if rig.ibl() { 2 } else { 0 }, "{rig:?}");
        assert_eq!(policy.ibl.is_some_and(|i| i.enabled), rig.ibl(), "{rig:?}");
    }
    // An IBL rig is its base rig plus the IBL axis, its textures and its exposure.
    for rig in CalibrationRig::ALL.into_iter().filter(|r| r.ibl()) {
        let mut with = lit(rig);
        let without = lit(rig.lighting());
        with.render_policy.as_mut().unwrap().ibl = None;
        with.textures.retain(|t| !t.texture_id.starts_with("wge-ibl-"));
        with.environment.exposure = without.environment.exposure;
        with.lights[0].light_id = without.lights[0].light_id.clone();
        assert_eq!(with, without, "{rig:?}");
    }
    // The albedo-grey rig is the sun rig plus the debug axis and nothing else.
    let mut grey = lit(CalibrationRig::SunAlbedoGrey);
    let sun = lit(CalibrationRig::Sun);
    assert_eq!(grey.render_policy.unwrap().debug.unwrap().albedo_override_bp, 5000);
    grey.render_policy.as_mut().unwrap().debug = None;
    grey.lights[0].light_id = sun.lights[0].light_id.clone();
    assert_eq!(grey, sun);
}

fn glb() -> Vec<u8> {
    std::fs::read(repo().join("artifacts/calibration/calibration1/calibration1.glb"))
        .expect("run tools/fetch_calibration_materials.py and tools/build_calibration_glb.py first")
}

#[test]
#[ignore = "requires the fetched calibration GLB (tools/fetch_calibration_materials.py, tools/build_calibration_glb.py)"]
fn calibration_glb_prepares_with_zero_findings_and_frames_every_view() {
    let glb = glb();
    let render = condition_render_asset(&glb, &render_request()).unwrap();
    assert_eq!(render.status, RenderPreparationStatus::Ready);
    assert!(render.findings.is_empty(), "render conditioning findings: {:?}", render.findings);
    let package = render.package.as_ref().unwrap();
    let runtime = prepare_asset(&glb, &runtime_request(&glb, package).unwrap()).unwrap();
    assert_eq!(runtime.status, PreparationStatus::Ready);
    assert!(runtime.findings.is_empty(), "prepare findings: {:?}", runtime.findings);
    assert!(package.textures.iter().all(|t| t.mip_levels > 1 || t.width_px <= 8), "every scan is mipped");
    for column in CALIBRATION_COLUMNS {
        assert!(package.meshes.iter().any(|m| m.mesh_id.starts_with(&format!("{column}_"))), "column {column}");
    }
    for id in ["ground_plane", "plinth", "card_grey_018", "card_white_085", "card_black_003", "ball_chrome", "ball_grey_018", "checker_strip", "bark_cylinder"] {
        assert!(package.meshes.iter().any(|m| m.mesh_id == id), "mesh {id}");
    }
    let placement = CalibrationPlacement { anchor_xyz_m: [-16.0, 12.21, -26.5] };
    let card = grey_card_local_center(package).unwrap();
    for view in CalibrationView::all() {
        let camera = view.camera(package, placement).unwrap_or_else(|e| panic!("{}: {e}", view.name()));
        assert!(camera.position_xyz_m[2] > -36.0, "{} camera stands outside the host world", view.name());
        if matches!(view, CalibrationView::Row) || view == CalibrationView::Close("calibration".into()) {
            assert!(project_local_point(&camera, placement, card).is_some(), "{} must frame the grey card", view.name());
        }
    }
    assert!((GREY_CARD_TARGET_SRGB8 - 141.1).abs() < 0.05);
}

#[test]
fn coverage_rigs_are_their_base_rig_plus_the_policy_and_stay_out_of_all() {
    use wge_native_graphics_contract::CalibrationRig;
    for (rig, base, ibl) in [
        (CalibrationRig::SunCoverage, CalibrationRig::Sun, false),
        (CalibrationRig::OvercastCoverage, CalibrationRig::Overcast, false),
        (CalibrationRig::SunIblCoverage, CalibrationRig::SunIbl, true),
    ] {
        assert_eq!(CalibrationRig::parse(rig.name()), Some(rig));
        assert!(!CalibrationRig::ALL.contains(&rig), "{} would change --rigs all", rig.name());
        assert!(rig.coverage() && !base.coverage());
        assert_eq!(rig.without_coverage(), base);
        assert_eq!(rig.lighting(), base.lighting());
        assert_eq!(rig.ibl(), ibl);
        assert_eq!(rig.exposure_calibrated(), base.exposure_calibrated());
    }
    assert!(CalibrationRig::ALL.iter().all(|rig| !rig.coverage()));
}
