import json
import hashlib
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

PIPELINE = Path(__file__).resolve().parents[1] / "pipeline"
sys.path.insert(0, str(PIPELINE))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from reference_content import CONTENT_ROOT, ENGINE_ROOT  # noqa: E402

from zone_compiler import ZoneCompileError, compile_annotations, compile_file
from zone_spec_to_godot import adapt_zone_spec
from zone_rasterizer import (
    _carve_hydrology,
    _flatten_landmark_pads,
    _style_palette,
    rasterize_zone_spec,
    write_raster,
)
from zone_acceptance import evaluate_zone
from asset_catalog import CATALOG_VERSION, _tags, build_catalog
from asset_plan import ASSET_PLAN_VERSION, _attach_lod_siblings, resolve_asset_plan
from zone_assets_to_godot import adapt_asset_plan
from zone_to_unity import UNITY_VERSION, adapt_unity
from zone_to_unreal import UNREAL_VERSION, adapt_unreal
from unreal_artifacts import UNREAL_LANDSCAPE_RESOLUTION, write_unreal_artifacts
from zone_runtime_effects import RUNTIME_EFFECTS_VERSION, build_runtime_effects
from visual_acceptance import evaluate_visual
from evidence_overlay import OVERLAY_VERSION, render_overlay
from asset_visual_preflight import _appearance_metrics, _contact_sheet
from perspective_acceptance import evaluate_perspective
from concept_batch_intake import INTAKE_VERSION, IntakeError, create_batch
from texture_material_pipeline import MATERIAL_VERSION, assess as assess_texture, materialize, mean_linear_luminance
from material_catalog import resolve_terrain_materials, TextureMaterialError
from vision_annotation_adapter import VISION_RESPONSE_VERSION, ingest_response
from style_reference import STYLE_VERSION, analyze as analyze_style, score as score_style
from style_calibration import (
    STYLE_CALIBRATION_VERSION,
    initial as initial_style_calibration,
    refine as refine_style_calibration,
    select_best_pass,
)
from build_zone import _canonical_source_path, apply_style_policy
from navigation_acceptance import evaluate_navigation
from overview_projection_acceptance import evaluate_projection
from traversal_probe import evaluate_traversal
from worldbuilder_dsl import apply_intent, compile_intent


class ZoneCompilerTests(unittest.TestCase):
    def setUp(self):
        self.annotations_path = CONTENT_ROOT / "concept_batches" / "caledonia_v1" / "annotations.json"
        self.annotations = json.loads(self.annotations_path.read_text(encoding="utf-8"))

    def test_concept_batch_compiles_and_preserves_alpine_semantics(self):
        result = compile_annotations(self.annotations)
        massifs = [f for f in result.zone_spec["features"] if f["semantic"] == "alpine_massif"]
        self.assertEqual(2, len(massifs))
        self.assertTrue(all(f["generation"]["profile"] == "alpine_jagged_massif" for f in massifs))
        self.assertEqual(
            {"overview"},
            {
                feature["geometry"]["source_image_id"]
                for feature in result.zone_spec["features"]
            },
        )
        raster = rasterize_zone_spec(result.zone_spec, resolution=193)
        alpine_records = {entry["id"]: entry for entry in raster.manifest["landforms"] if entry["profile"] == "alpine_jagged_massif"}
        self.assertTrue(all(record["p95_slope"] >= 0.60 for record in alpine_records.values()))
        self.assertEqual("alpine_granite_crags", massifs[0]["generation"]["asset_profile"])
        self.assertIn("highland_conifer_mixed", result.zone_spec["asset_profiles"])
        alpine_profile = result.zone_spec["asset_profiles"]["alpine_granite_crags"]
        self.assertEqual(24, alpine_profile["instances_per_feature"])
        self.assertEqual([0.48, 0.82], alpine_profile["scale_m"])
        bridges = [
            feature
            for feature in result.zone_spec["features"]
            if feature["category"] == "structure"
            and feature["semantic"] == "bridge"
        ]
        self.assertEqual(17, len(bridges))
        self.assertTrue(
            all(
                bridge["generation"]["asset_profile"] == "highland_stone_bridge"
                and len(bridge["properties"]["derived_from"]) == 2
                and len(bridge["evidence"]) == 2
                for bridge in bridges
            )
        )
        self.assertEqual(
            {
                "grass": "caledonia_highland_grass",
                "road": "caledonia_packed_dirt",
                "rock": "highland_granite_v2",
                "wetland": "caledonia_wetland_peat",
                "snow": "caledonia_windpacked_snow",
            },
            result.zone_spec["terrain_materials"],
        )
        self.assertEqual(
            {
                "grass": 8.0,
                "road": 6.0,
                "rock": 7.5,
                "wetland": 6.0,
                "snow": 5.0,
            },
            result.zone_spec["terrain_material_scale_m"],
        )
        self.assertEqual("warnings", result.report["status"])
        self.assertIn("Reviewed low-confidence feature retained: valley_waterways", result.report["warnings"])

    def test_compact_derivation_provenance_survives_zone_compilation(self):
        compact_path = (
            CONTENT_ROOT
            / "concept_batches"
            / "codeweald_alpine_arena_v1"
            / "annotations.json"
        )
        compact = json.loads(compact_path.read_text(encoding="utf-8"))
        zone_spec = compile_annotations(compact).zone_spec
        self.assertEqual(
            compact["derivation"],
            zone_spec["derivation"],
        )
        central_channel = next(
            feature
            for feature in zone_spec["features"]
            if feature["id"] == "central_wetland_channel"
        )
        self.assertEqual(
            ["central_ruin"],
            central_channel["properties"]["resolved_landmark_exclusions"],
        )
        northern_headwaters = next(
            feature
            for feature in zone_spec["features"]
            if feature["id"] == "northern_headwaters"
        )
        self.assertIn(
            "hibernia_keep",
            northern_headwaters["properties"]["resolved_landmark_exclusions"],
        )

    def test_overview_projection_rejects_mirrored_runtime_map(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        landmarks = []
        for feature in zone_spec["features"]:
            if feature["category"] != "landmark":
                continue
            region = feature["evidence"][0]["region"]
            landmarks.append({
                "feature_id": feature["id"],
                "found": True,
                "screen_normalized": [
                    (region[0] + region[2]) * 0.5,
                    (region[1] + region[3]) * 0.5,
                ],
                "evidence_screen_normalized": [
                    0.99,
                    0.99,
                ],
            })
        corridors = []
        for feature in zone_spec["features"]:
            if feature["category"] != "corridor" or feature["semantic"] != "lane":
                continue
            region = feature["evidence"][0]["region"]
            screen_points = [
                [
                    min(max(point[0], region[0] + 0.001), region[2] - 0.001),
                    min(max(point[1], region[1] + 0.001), region[3] - 0.001),
                ]
                for point in feature["geometry"]["source_points"]
            ]
            corridors.append({
                "feature_id": feature["id"],
                "found": True,
                "screen_normalized": screen_points,
                "half_width_pixels": [6.0] * len(screen_points),
                "evidence_screen_normalized": [[0.99, 0.99]] * len(screen_points),
                "evidence_half_width_pixels": [1.0] * len(screen_points),
            })
        landforms = []
        for feature in zone_spec["features"]:
            if feature["category"] != "landform":
                continue
            region = feature["evidence"][0]["region"]
            center = [
                (region[0] + region[2]) * 0.5,
                (region[1] + region[3]) * 0.5,
            ]
            landforms.append({
                "feature_id": feature["id"],
                "found": True,
                "screen_samples": [center] * 24,
                "evidence_screen_samples": [[0.99, 0.99]] * 24,
                "maximum_projected_relief_pixels": 64.0,
                "mean_projected_relief_pixels": 20.0,
            })
        projection = {
            "schema_version": "codeweald.godot-overview-projection/v1",
            "zone_id": "caledonia_concept_v1",
            "axis_projection": {
                "positive_x_delta": [0.05, 0.0],
                "positive_z_delta": [0.0, -0.05],
            },
            "landmarks": landmarks,
            "corridors": corridors,
            "landforms": landforms,
        }
        self.assertEqual("passed", evaluate_projection(zone_spec, projection)["status"])
        projection["axis_projection"]["positive_z_delta"] = [0.0, 0.05]
        albion = next(entry for entry in landmarks if entry["feature_id"] == "albion_keep")
        albion["screen_normalized"][1] = 0.2
        rejected = evaluate_projection(zone_spec, projection)
        self.assertEqual("failed", rejected["status"])
        self.assertIn("Godot overview does not project world +Z toward screen top", rejected["failures"])
        self.assertIn("Godot overview projects landmark albion_keep outside reviewed evidence region", rejected["failures"])

    def test_native_navigation_acceptance_requires_keep_and_objective_routes(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        probe = {
            "schema_version": "codeweald.godot-navigation-probe/v1",
            "zone_id": "caledonia_concept_v1",
            "navigation_found": True,
            "navigation_vertex_count": 2501,
            "navigation_polygon_count": 2200,
            "off_mesh_link_count": 17,
            "enabled_off_mesh_link_count": 17,
            "routes": [
                {
                    "from_feature_id": "albion_keep",
                    "to_feature_id": "hibernia_keep",
                    "role": "keep_to_keep",
                    "found": True,
                    "path_length_m": 2100.0,
                    "straight_line_distance_m": 1900.0,
                    "start_snap_distance_m": 4.0,
                    "finish_snap_distance_m": 5.0,
                }
            ]
            + [
                {
                    "from_feature_id": keep,
                    "to_feature_id": "central_ruin",
                    "role": "keep_to_objective",
                    "found": True,
                    "path_length_m": 1100.0,
                    "straight_line_distance_m": 900.0,
                    "start_snap_distance_m": 4.0,
                    "finish_snap_distance_m": 3.0,
                }
                for keep in ("albion_keep", "hibernia_keep")
            ],
        }
        self.assertEqual("passed", evaluate_navigation(zone_spec, probe)["status"])
        probe["enabled_off_mesh_link_count"] = 16
        self.assertIn(
            "Godot navigation has disabled bridge traversal links",
            evaluate_navigation(zone_spec, probe)["failures"],
        )
        probe["enabled_off_mesh_link_count"] = 17
        probe["routes"][0]["found"] = False
        rejected = evaluate_navigation(zone_spec, probe)
        self.assertEqual("failed", rejected["status"])
        self.assertTrue(
            any("cannot connect albion_keep" in failure for failure in rejected["failures"])
        )

    def test_unreviewed_low_confidence_topology_is_rejected(self):
        bad = json.loads(json.dumps(self.annotations))
        feature = next(f for f in bad["features"] if f["id"] == "valley_waterways")
        feature["review_state"] = "proposed"
        with self.assertRaisesRegex(ZoneCompileError, "below confidence"):
            compile_annotations(bad)

    def test_evidence_requires_declared_source_ordered_region_and_explanation(self):
        bad_image = json.loads(json.dumps(self.annotations))
        bad_image["features"][0]["evidence"][0]["image_id"] = "invented_frame"
        with self.assertRaisesRegex(ZoneCompileError, "declared source image"):
            compile_annotations(bad_image)
        bad_region = json.loads(json.dumps(self.annotations))
        bad_region["features"][0]["evidence"][0]["region"] = [0.80, 0.10, 0.20, 0.40]
        with self.assertRaisesRegex(ZoneCompileError, "ordered normalized rectangle"):
            compile_annotations(bad_region)
        bad_note = json.loads(json.dumps(self.annotations))
        bad_note["features"][0]["evidence"][0]["note"] = " "
        with self.assertRaisesRegex(ZoneCompileError, "non-empty explanation"):
            compile_annotations(bad_note)

    def test_evidence_overlay_renders_validated_feature_regions(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "evidence_overlay.png"
            report = render_overlay(zone_spec, self.annotations_path.parent, output)
            self.assertEqual(OVERLAY_VERSION, report["schema_version"])
            self.assertGreater(report["evidence_count"], 0)
            self.assertTrue(output.is_file())

    def test_asset_visual_contact_sheet_uses_only_successfully_probed_assets(self):
        from PIL import Image
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            Image.new("RGB", (16, 16), (80, 120, 70)).save(root / "tree.png")
            report = _contact_sheet([
                {"status": "passed", "thumbnail": "tree.png", "source_path": "assets/tree.glb", "triangle_count": 12, "material_slot_count": 1},
                {"status": "failed", "source_path": "assets/bad.glb"},
            ], root, root / "selected_assets_contact_sheet.png")
            self.assertEqual(1, report["rendered_tiles"])
            self.assertTrue((root / "selected_assets_contact_sheet.png").is_file())

    def test_perspective_acceptance_rejects_blank_objective_and_accepts_readable_views(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        vista = np.full((720, 1280, 3), 0.2, dtype=np.float32)
        vista[150:550, 320:940] = [0.46, 0.33, 0.21]
        objective = vista.copy()
        objective[300:420, 530:750] = [0.9, 0.82, 0.64]
        self.assertEqual("passed", evaluate_perspective(zone_spec, vista, objective)["status"])
        self.assertEqual("failed", evaluate_perspective(zone_spec, vista, np.zeros_like(objective))["status"])
        washed = np.full_like(vista, 0.72)
        washed[::12, :, :] = 0.28
        report = evaluate_perspective(zone_spec, washed, washed)
        self.assertEqual("failed", report["status"])
        self.assertTrue(any("washed out" in failure for failure in report["failures"]))

    def test_style_reference_scores_matching_render_above_unrelated_flat_image(self):
        height = width = 128
        y, x = np.indices((height, width))
        source = np.dstack([
            0.16 + 0.10 * np.sin(x * 0.18),
            0.22 + 0.12 * np.cos(y * 0.14),
            0.12 + 0.05 * np.sin((x + y) * 0.12),
        ]).clip(0.0, 1.0).astype(np.float32)
        reference = {"schema_version": STYLE_VERSION, **analyze_style(source)}
        matching = score_style(reference, source.copy())
        unrelated = score_style(reference, np.full_like(source, 0.92))
        self.assertGreater(matching["score"], 0.99)
        self.assertLess(unrelated["score"], matching["score"])

    def test_style_reference_ignores_engine_letterboxing(self):
        source = np.zeros((96, 144, 3), dtype=np.float32)
        source[:, :, 0] = 0.12
        source[:, :, 1] = 0.23
        source[:, :, 2] = 0.10
        source[::6, :, :] += 0.08
        reference = {"schema_version": STYLE_VERSION, **analyze_style(source)}
        framed = np.full((140, 200, 3), [0.24, 0.28, 0.30], dtype=np.float32)
        framed[22:118, 28:172] = source
        result = score_style(reference, framed)
        self.assertTrue(result["comparison_crop"]["applied"])
        self.assertGreater(result["score"], 0.85)

    def test_style_calibration_is_provenance_bound_and_bounded(self):
        reference = {
            "source": {"sha256": "a" * 64},
            "metrics": {
                "mean_luminance": 0.20,
                "luminance_stddev": 0.12,
                "mean_saturation": 0.29,
            },
        }
        baseline = initial_style_calibration(reference)
        self.assertEqual(STYLE_CALIBRATION_VERSION, baseline["schema_version"])
        self.assertEqual("a" * 64, baseline["source_sha256"])
        calibrated = refine_style_calibration(
            reference,
            {
                "score": 0.25,
                "candidate": {
                    "metrics": {
                        "mean_luminance": 0.10,
                        "luminance_stddev": 0.05,
                        "mean_saturation": 0.75,
                    }
                },
            },
        )
        self.assertEqual(1, calibrated["pass"])
        adjustment = calibrated["adjustment"]
        self.assertEqual(1.587401, adjustment["brightness_multiplier"])
        self.assertEqual(1.0, adjustment["contrast_multiplier"])
        self.assertEqual(0.530751, adjustment["saturation_multiplier"])
        self.assertEqual(
            0, select_best_pass({"score": 0.32}, {"score": 0.24})
        )
        self.assertEqual(
            1, select_best_pass({"score": 0.32}, {"score": 0.36})
        )
        self.assertEqual(
            0,
            select_best_pass(
                {
                    "score": 0.32,
                    "candidate": {"metrics": {"mean_luminance": 0.10}},
                },
                {
                    "score": 0.36,
                    "candidate": {"metrics": {"mean_luminance": 0.06}},
                },
            ),
        )

    def test_unknown_asset_profile_is_rejected(self):
        bad = json.loads(json.dumps(self.annotations))
        feature = next(f for f in bad["features"] if f["id"] == "central_forest")
        feature["properties"]["asset_profile"] = "not_a_real_profile"
        with self.assertRaisesRegex(ZoneCompileError, "unknown asset profile"):
            compile_annotations(bad)

    def test_asset_quality_requirements_must_be_positive_integers(self):
        bad = json.loads(json.dumps(self.annotations))
        bad["asset_profiles"]["alpine_granite_crags"]["quality_requirements"]["minimum_triangle_count"] = 0
        with self.assertRaisesRegex(ZoneCompileError, "minimum_triangle_count must be a positive integer"):
            compile_annotations(bad)

    def test_asset_appearance_requirements_are_bounded(self):
        bad = json.loads(json.dumps(self.annotations))
        bad["asset_profiles"]["alpine_granite_crags"]["appearance_requirements"]["maximum_mean_luminance"] = 1.5
        with self.assertRaisesRegex(ZoneCompileError, "maximum_mean_luminance must be between 0 and 1"):
            compile_annotations(bad)

    def test_asset_appearance_metrics_detect_white_material_loss(self):
        dark = Image.new("RGB", (128, 128), (20, 24, 31))
        white = dark.copy()
        for x in range(30, 98):
            for y in range(20, 112):
                dark.putpixel((x, y), (54, 66, 68))
                white.putpixel((x, y), (224, 224, 224))
        self.assertLess(_appearance_metrics(dark)["mean_luminance"], 0.58)
        self.assertGreater(_appearance_metrics(white)["bright_fraction"], 0.45)

    def test_strict_style_policy_fails_below_reviewed_threshold(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        self.assertEqual(
            {
                "minimum_style_score": 0.42,
                "style_mismatch": "fail",
                "maximum_regional_palette_mismatch_fraction": 0.25,
                "minimum_biome_coverage_ratio": 0.20,
                "landmark_evidence_fill_ratio": [0.002, 2.5],
            },
            zone_spec["acceptance_policy"],
        )
        visual = {"status": "passed", "failures": [], "warnings": []}
        apply_style_policy(visual, {"score": 0.227}, zone_spec["acceptance_policy"])
        self.assertEqual("failed", visual["status"])
        self.assertEqual("needs_art_direction", visual["fidelity_status"])
        self.assertIn("below the 0.42 reviewed fidelity threshold", visual["failures"][0])

    def test_acceptance_policy_rejects_unbounded_threshold(self):
        bad = json.loads(json.dumps(self.annotations))
        bad["acceptance_policy"]["minimum_style_score"] = -0.1
        with self.assertRaisesRegex(ZoneCompileError, "minimum_style_score must be between 0 and 1"):
            compile_annotations(bad)

    def test_traversal_policy_is_bounded_and_preserved(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        self.assertEqual(2.5, zone_spec["traversal_policy"]["agent_radius_m"])
        self.assertEqual(
            0.32, zone_spec["traversal_policy"]["maximum_lane_p95_grade"]
        )
        bad = json.loads(json.dumps(self.annotations))
        bad["traversal_policy"]["agent_max_slope_degrees"] = 80
        with self.assertRaisesRegex(
            ZoneCompileError, "agent_max_slope_degrees must be in"
        ):
            compile_annotations(bad)

    def test_landform_semantic_cannot_fall_through_to_a_wrong_profile(self):
        bad = json.loads(json.dumps(self.annotations))
        massif = next(feature for feature in bad["features"] if feature["id"] == "western_alps")
        massif["generation"]["profile"] = "rolling_foothills"
        with self.assertRaisesRegex(ZoneCompileError, "alpine_massif must use"):
            compile_annotations(bad)

    def test_faction_keep_rejects_unknown_realm_and_adapter_retains_midgard_base(self):
        bad = json.loads(json.dumps(self.annotations))
        keep = next(feature for feature in bad["features"] if feature["id"] == "albion_keep")
        keep["properties"]["realm"] = "unknown_realm"
        with self.assertRaisesRegex(ZoneCompileError, "faction_keep.realm"):
            compile_annotations(bad)
        tri_realm = json.loads(json.dumps(self.annotations))
        tri_realm["features"].append({
            "id": "midgard_keep", "category": "landmark", "semantic": "faction_keep",
            "geometry": {"type": "point", "points": [[0.50, 0.91]]},
            "properties": {"team": "team_c", "realm": "midgard", "name": "Midgard plateau keep", "facing_direction_xz": [0.0, -1.0]},
            "evidence": [{"image_id": "overview", "region": [0.35, 0.72, 0.65, 1.0], "note": "Norse faction keep"}], "confidence": 0.90, "review_state": "reviewed",
        })
        adapted = adapt_zone_spec(compile_annotations(tri_realm).zone_spec)
        self.assertEqual("midgard", adapted["faction_bases"]["team_c"]["realm"])

    def test_godot_adapter_receives_world_space_geometry(self):
        result = compile_annotations(self.annotations)
        godot = adapt_zone_spec(result.zone_spec)
        self.assertEqual(2400.0, godot["map_bounds"]["width"])
        self.assertEqual(3, len(godot["lanes"]))
        self.assertEqual(8, len(godot["mountain_polygons"]))
        self.assertEqual([-876.0, -464.0], godot["team_a_base"]["center"])

    def test_file_compilation_writes_deterministic_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first = compile_file(self.annotations_path, root / "zone.json", root / "report.json")
            second = compile_file(self.annotations_path, root / "zone2.json", root / "report2.json")
            self.assertEqual(first.zone_spec, second.zone_spec)
            self.assertEqual((root / "zone.json").read_bytes(), (root / "zone2.json").read_bytes())

    def test_file_compilation_rejects_missing_or_tampered_source_art(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            annotations = json.loads(json.dumps(self.annotations))
            annotations["source_images"][0]["path"] = "source/missing.png"
            input_path = root / "annotations.json"
            input_path.write_text(json.dumps(annotations), encoding="utf-8")
            with self.assertRaisesRegex(ZoneCompileError, "source file is unavailable"):
                compile_file(input_path, root / "zone.json", root / "report.json")

    def test_alpine_terrain_artifacts_have_relief_and_are_deterministic(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        first = rasterize_zone_spec(zone_spec, resolution=129)
        second = rasterize_zone_spec(zone_spec, resolution=129)
        self.assertTrue((first.height_m == second.height_m).all())
        self.assertEqual((129, 129, 4), first.splat_rgba.shape)
        self.assertEqual((129, 129), first.wetland_mask.shape)
        self.assertEqual((129, 129, 3), first.style_guidance_rgb.shape)
        self.assertGreater(first.manifest["wetland_coverage_fraction"], 0.0)
        self.assertGreater(first.manifest["height_range_m"]["max"], 100.0)
        self.assertEqual({"grass", "road", "rock", "snow", "water"}, set(first.manifest["style_palette_srgb"]))
        self.assertTrue(all(0.0 <= channel <= 1.0 for color in first.manifest["style_palette_srgb"].values() for channel in color))
        alpine_landforms = [
            landform
            for landform in first.manifest["landforms"]
            if landform["profile"] == "alpine_jagged_massif"
        ]
        self.assertEqual(2, len(alpine_landforms))
        for landform in alpine_landforms:
            self.assertEqual("alpine_jagged_massif", landform["profile"])
            self.assertGreater(landform["relief_m"], 70.0)
            self.assertGreater(landform["mean_slope"], 0.1)
        crag_fields = [
            landform
            for landform in first.manifest["landforms"]
            if landform["profile"] == "scattered_crag_field"
        ]
        self.assertEqual(6, len(crag_fields))
        self.assertTrue(all(landform["rock_coverage"] > 0.25 for landform in crag_fields))
        self.assertTrue(all(landform["relief_m"] > 8.0 for landform in crag_fields))
        with tempfile.TemporaryDirectory() as temporary:
            write_raster(first, Path(temporary))
            self.assertTrue((Path(temporary) / "heightmap_16.png").exists())
            self.assertTrue((Path(temporary) / "style_guidance.png").exists())
            self.assertTrue((Path(temporary) / "terrain_manifest.json").exists())

    def test_compact_crags_are_terrain_native_without_prop_dressing(self):
        compact_path = (
            CONTENT_ROOT
            / "concept_batches"
            / "codeweald_alpine_arena_v1"
            / "annotations.json"
        )
        compact = json.loads(compact_path.read_text(encoding="utf-8"))
        intent_path = compact_path.with_name("world_intent.py")
        zone_spec = apply_intent(
            compile_annotations(compact).zone_spec,
            compile_intent(
                intent_path.read_text(encoding="utf-8"),
                source_name=intent_path.as_posix(),
            ),
        )
        terrain = rasterize_zone_spec(
            zone_spec,
            resolution=129,
        )
        crag_fields = [
            landform
            for landform in terrain.manifest["landforms"]
            if landform["profile"] == "scattered_crag_field"
        ]
        self.assertEqual(6, len(crag_fields))
        self.assertTrue(all(landform["relief_m"] > 10.0 for landform in crag_fields))
        self.assertTrue(
            all(
                landform["art_profile"]["massing"] == "terrain_primary"
                and landform["art_profile"]["dressing"] == "none"
                for landform in crag_fields
            )
        )

    def test_hydrology_carves_ridge_crossing_into_downhill_channel(self):
        axis = np.linspace(-2.0, 2.0, 5, dtype=np.float32)
        x, z = np.meshgrid(axis, axis[::-1])
        height = np.zeros((5, 5), dtype=np.float32)
        height[:, 2] = 8.0
        feature = {
            "id": "ridge_crossing",
            "category": "hydrology",
            "semantic": "stream",
            "geometry": {
                "type": "polyline",
                "points": [[-2.0, 0.0], [0.0, 0.0], [2.0, 0.0]],
            },
            "properties": {
                "width_m": 1.0,
                "channel_profile": "incised_stream",
            },
        }
        carved = _carve_hydrology(height, x, z, [feature], 4.0, 4.0)
        centerline = carved[2, :]
        forward_uphill = np.count_nonzero(np.diff(centerline) > 0.05)
        reverse_uphill = np.count_nonzero(np.diff(centerline[::-1]) > 0.05)
        self.assertEqual(0, min(forward_uphill, reverse_uphill))

    def test_wetland_rill_cannot_cut_a_deep_trench(self):
        axis = np.linspace(-8.0, 8.0, 33, dtype=np.float32)
        x, z = np.meshgrid(axis, axis[::-1])
        height = (4.0 + x * 0.25).astype(np.float32)
        feature = {
            "id": "marsh_rill",
            "category": "hydrology",
            "semantic": "stream",
            "geometry": {
                "type": "polyline",
                "points": [[-8.0, 0.0], [0.0, 0.0], [8.0, 0.0]],
            },
            "properties": {
                "width_m": 2.0,
                "channel_profile": "wetland_rill",
            },
        }
        conformed = _carve_hydrology(height, x, z, [feature], 16.0, 16.0)
        lowering = height - conformed
        self.assertLessEqual(float(lowering.max()), 0.250001)

    def test_road_profile_grade_is_bounded(self):
        from zone_rasterizer import _catmull_rom_route, _limit_profile_grade

        controls = [[0.0, 0.0], [4.0, 2.0], [8.0, 0.0]]
        route = _catmull_rom_route(controls)
        profile = np.zeros(len(route), dtype=np.float64)
        profile[len(route) // 2] = 10.0
        bounded = _limit_profile_grade(profile, route, 0.18)
        grades = [
            abs(height_b - height_a)
            / np.hypot(point_b[0] - point_a[0], point_b[1] - point_a[1])
            for point_a, point_b, height_a, height_b in zip(
                route, route[1:], bounded, bounded[1:]
            )
        ]
        self.assertLessEqual(max(grades), 0.180001)

    def test_settlement_pad_is_flattened_with_an_eased_terrain_edge(self):
        coordinates = np.linspace(-24.0, 24.0, 97, dtype=np.float32)
        x, z = np.meshgrid(coordinates, coordinates)
        original = (8.0 + x * 0.12 + z * 0.04).astype(np.float32)
        feature = {
            "category": "landmark",
            "semantic": "settlement_cluster",
            "geometry": {"points": [[0.0, 0.0]]},
            "properties": {"scatter_exclusion_radius_m": 16.0},
        }
        flattened = _flatten_landmark_pads(original, x, z, [feature])
        inner = x**2 + z**2 <= 8.0**2
        outside = x**2 + z**2 >= 17.0**2
        self.assertLess(float(flattened[inner].std()), 1e-5)
        np.testing.assert_allclose(flattened[outside], original[outside])

    def test_named_landform_grammars_compile_to_distinct_terrain_records(self):
        annotations = json.loads(json.dumps(self.annotations))
        variants = [
            ("sawtooth_ridge", "alpine_ridge", "alpine_sawtooth_ridge", [[0.32, 0.08], [0.58, 0.11], [0.61, 0.19], [0.36, 0.22]], {"elevation_m": [75, 210], "cliffness": 0.78}),
            ("rolling_foothills", "foothills", "rolling_foothills", [[0.37, 0.76], [0.60, 0.75], [0.62, 0.91], [0.39, 0.93]], {"elevation_m": [18, 82]}),
            ("crag_field", "crag_field", "scattered_crag_field", [[0.57, 0.44], [0.70, 0.43], [0.69, 0.57], [0.55, 0.56]], {"elevation_m": [22, 115]}),
            ("cliff_escarpment", "cliff_band", "cliff_escarpment", [[0.08, 0.63], [0.23, 0.61], [0.25, 0.75], [0.10, 0.78]], {"elevation_m": [55, 180], "cliffness": 0.88}),
            ("glacial_valley", "valley_floor", "glacial_valley_floor", [[0.39, 0.29], [0.62, 0.31], [0.64, 0.39], [0.41, 0.40]], {"elevation_m": [18, 92], "depth_m": 42}),
        ]
        for feature_id, semantic, profile, points, generation in variants:
            annotations["features"].append({
                "id": feature_id, "category": "landform", "semantic": semantic,
                "geometry": {"type": "polygon", "points": points}, "generation": {"profile": profile, **generation},
                "properties": {"traversable": semantic in {"foothills", "valley_floor"}},
                "evidence": [{"image_id": "overview", "region": [0.0, 0.0, 1.0, 1.0], "note": profile}], "confidence": 0.92, "review_state": "reviewed",
            })
        raster = rasterize_zone_spec(compile_annotations(annotations).zone_spec, resolution=193)
        records = {entry["id"]: entry for entry in raster.manifest["landforms"]}
        self.assertEqual({profile for _id, _semantic, profile, _points, _generation in variants}, {records[feature_id]["profile"] for feature_id, _semantic, _profile, _points, _generation in variants})
        self.assertGreater(records["sawtooth_ridge"]["p95_slope"], 0.3)
        self.assertGreater(records["crag_field"]["rock_coverage"], 0.25)
        self.assertGreater(records["cliff_escarpment"]["p95_slope"], records["rolling_foothills"]["p95_slope"])
        self.assertLess(records["glacial_valley"]["min_elevation_m"], records["glacial_valley"]["max_elevation_m"])

    def test_lane_grading_passes_traversal_probe_and_policy_regression_fails(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        terrain = rasterize_zone_spec(zone_spec, resolution=257)
        report = evaluate_traversal(zone_spec, terrain.height_m)
        self.assertEqual("passed", report["status"], report["failures"])
        self.assertEqual({"top", "mid", "bottom"}, set(report["lanes"]))
        self.assertEqual({"scout", "standard", "large"}, {
            profile["id"] for profile in report["actor_profiles"]
        })
        self.assertEqual(17, report["off_mesh_link_count"])
        self.assertTrue(
            all(
                scenario["passed"]
                and len(scenario["alternative_lane_ids"]) == 2
                for scenario in report["blocker_scenarios"]
            )
        )
        self.assertTrue(
            all(
                all(
                    clearance["passed"]
                    for clearance in lane["actor_clearance"].values()
                )
                for lane in report["lanes"].values()
            )
        )
        self.assertTrue(
            all(
                lane["p95_longitudinal_grade"]
                <= zone_spec["traversal_policy"]["maximum_lane_p95_grade"]
                for lane in report["lanes"].values()
            )
        )
        too_strict = json.loads(json.dumps(zone_spec))
        too_strict["traversal_policy"]["maximum_lane_p95_grade"] = 0.001
        rejected = evaluate_traversal(too_strict, terrain.height_m)
        self.assertEqual("failed", rejected["status"])
        self.assertTrue(
            any("p95 longitudinal grade" in failure for failure in rejected["failures"])
        )
        missing_bridge = json.loads(json.dumps(zone_spec))
        missing_bridge["features"] = [
            feature
            for feature in missing_bridge["features"]
            if feature.get("id")
            != "bridge_central_lane_central_forest_tributary_0"
        ]
        missing_bridge_report = evaluate_traversal(
            missing_bridge, terrain.height_m
        )
        self.assertTrue(
            any(
                "without an authored traversal link" in failure
                for failure in missing_bridge_report["failures"]
            )
        )

    def test_acceptance_rejects_flattened_alpine_and_accepts_valid_build(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        terrain = rasterize_zone_spec(zone_spec, resolution=129).manifest
        terrain["terrain_materials"] = resolve_terrain_materials(
            zone_spec["terrain_materials"],
            CONTENT_ROOT,
            CONTENT_ROOT / "assets",
            zone_spec["terrain_material_scale_m"],
        )
        expected_crags = 96
        expected_assignments = 42
        build = {"zone_id": "caledonia_concept_v1", "scene_written": True, "keep_count": 2, "lane_path_count": 3, "lane_surface_count": 3, "navigation_vertex_count": 2501, "navigation_polygon_count": 2200, "waterway_count": 8, "bridge_count": 17, "ruin_count": 1, "foliage_instances": 4676, "crag_instances": expected_crags, "multimesh_batch_count": 30, "multimesh_instance_count": 10492, "multimesh_serialized_instance_count": 10492, "multimesh_buffers_valid": True, "multimesh_spatial_chunk_batch_count": 30, "multimesh_max_instances_per_chunk": 420, "multimesh_lod_instance_counts": {"lod0": 2860, "lod1": 2860, "lod2": 2860}, "terrain_mesh_external": True, "terrain_mesh_resource_bytes": 1024, "terrain_collision_external": True, "terrain_collision_sample_count": 263169, "terrain_collision_resource_bytes": 1024, "terrain_material_requested_layer_count": 5, "terrain_material_bound_layer_count": 5, "terrain_material_pbr_layer_count": 5, "terrain_material_minimum_texels_per_meter": 128.0, "terrain_material_meters_per_repeat": zone_spec["terrain_material_scale_m"], "terrain_wetland_mask_bound": True, "scene_bytes": 1024 * 1024, "settlement_count": 10, "settlement_grounded_component_count": 60, "settlement_grounding_max_residual_m": 0.03, "asset_assignment_count": expected_assignments, "missing_assets": []}
        build.update({"navigation_off_mesh_link_count": 17, "navigation_enabled_off_mesh_link_count": 17, "navigation_full_span_off_mesh_link_count": 17, "landmark_asset_variants": {"highland_fortified_keep": ["a", "b"], "arcane_objective_ruin": ["a"], "highland_stone_bridge": ["a", "b", "c", "d"], "highland_settlement_clusters": ["a", "b", "c"]}})
        visual = {"zone_id": "caledonia_concept_v1", "status": "passed"}
        projection = {"zone_id": "caledonia_concept_v1", "status": "passed"}
        traversal = {"zone_id": "caledonia_concept_v1", "status": "passed"}
        navigation = {"zone_id": "caledonia_concept_v1", "status": "passed"}
        runtime_effects = {"status": "passed"}
        accepted = evaluate_zone(zone_spec, terrain, build, visual, overview_projection_report=projection, traversal_report=traversal, navigation_report=navigation, runtime_effects_report=runtime_effects)
        self.assertEqual("passed", accepted["status"])
        self.assertEqual(4676, accepted["evidence"]["expected_foliage_instances"])
        self.assertEqual(10, accepted["evidence"]["expected_settlement_count"])
        self.assertEqual(8, accepted["evidence"]["expected_waterway_count"])
        self.assertEqual(17, accepted["evidence"]["expected_bridge_count"])
        self.assertEqual(4772, accepted["evidence"]["expected_multimesh_instances"])
        self.assertTrue(accepted["evidence"]["has_runtime_effects_acceptance_report"])
        bad_grounding = evaluate_zone(
            zone_spec,
            terrain,
            dict(build, settlement_grounding_max_residual_m=0.4),
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
            runtime_effects_report=runtime_effects,
        )
        self.assertIn(
            "Godot settlement grounding residual 0.400 m exceeds 0.08 m",
            bad_grounding["failures"],
        )
        missing_surface = evaluate_zone(
            zone_spec,
            terrain,
            dict(build, lane_surface_count=2),
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
        )
        self.assertIn(
            "Godot scene omitted one or more visible lane surfaces",
            missing_surface["failures"],
        )
        failed_traversal = evaluate_zone(
            zone_spec,
            terrain,
            build,
            visual,
            overview_projection_report=projection,
            traversal_report=dict(traversal, status="failed"),
            navigation_report=navigation,
        )
        self.assertIn(
            "Deterministic terrain traversal probes failed",
            failed_traversal["failures"],
        )
        failed_navigation = evaluate_zone(
            zone_spec,
            terrain,
            build,
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=dict(navigation, status="failed"),
        )
        self.assertIn(
            "Native Godot navigation acceptance failed",
            failed_navigation["failures"],
        )
        missing_navigation_mesh = evaluate_zone(
            zone_spec,
            terrain,
            dict(build, navigation_polygon_count=0),
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
        )
        self.assertIn(
            "Godot scene omitted native walkable polygons",
            missing_navigation_mesh["failures"],
        )
        missing_multimesh = evaluate_zone(
            zone_spec,
            terrain,
            dict(build, multimesh_instance_count=4771),
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
        )
        self.assertIn(
            "Godot scene batched 4771 of 4772 repeated environment transforms",
            missing_multimesh["failures"],
        )
        unserialized_multimesh = evaluate_zone(
            zone_spec,
            terrain,
            dict(build, multimesh_serialized_instance_count=4771),
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
        )
        self.assertIn(
            "Godot scene serialized 4771 of 4772 repeated environment transforms",
            unserialized_multimesh["failures"],
        )
        malformed_multimesh = evaluate_zone(
            zone_spec,
            terrain,
            dict(build, multimesh_buffers_valid=False),
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
        )
        self.assertIn(
            "Godot scene has incomplete or malformed serialized MultiMesh buffers",
            malformed_multimesh["failures"],
        )
        unchunked_multimesh = evaluate_zone(
            zone_spec,
            terrain,
            dict(build, multimesh_spatial_chunk_batch_count=29),
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
        )
        self.assertIn(
            "Godot scene spatially chunked 29 of 30 MultiMesh batches",
            unchunked_multimesh["failures"],
        )
        missing_lod = evaluate_zone(
            zone_spec,
            terrain,
            dict(
                build,
                multimesh_lod_instance_counts={
                    "lod0": 2860,
                    "lod1": 2859,
                    "lod2": 2860,
                },
            ),
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
        )
        self.assertIn(
            "Godot scene emitted 2859 of 2860 planned lod1 render instances",
            missing_lod["failures"],
        )
        embedded_terrain = evaluate_zone(
            zone_spec,
            terrain,
            dict(build, terrain_mesh_external=False),
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
        )
        self.assertIn(
            "Godot scene embedded the generated terrain mesh",
            embedded_terrain["failures"],
        )
        low_density = json.loads(json.dumps(terrain))
        low_density["terrain_materials"]["snow"][
            "minimum_texels_per_meter"
        ] = 32.0
        low_density_report = evaluate_zone(
            zone_spec,
            low_density,
            build,
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
        )
        self.assertIn(
            "Terrain snow material falls below 48 texels per meter",
            low_density_report["failures"],
        )
        stopped_runtime_effects = evaluate_zone(
            zone_spec,
            terrain,
            build,
            visual,
            overview_projection_report=projection,
            traversal_report=traversal,
            navigation_report=navigation,
            runtime_effects_report={"status": "failed"},
        )
        self.assertIn(
            "Serialized Godot runtime effects did not animate",
            stopped_runtime_effects["failures"],
        )
        disabled = json.loads(json.dumps(zone_spec))
        disabled["asset_profiles"]["highland_open_woodland"]["runtime_enabled"] = False
        disabled_build = dict(build, foliage_instances=2196)
        self.assertEqual("passed", evaluate_zone(disabled, terrain, disabled_build, visual, overview_projection_report=projection, traversal_report=traversal, navigation_report=navigation)["status"])
        build["foliage_instances"] = 4675
        rejected = evaluate_zone(zone_spec, terrain, build, visual, overview_projection_report=projection, traversal_report=traversal, navigation_report=navigation)
        self.assertIn("Godot scene placed 4675 of 4676 planned foliage assets", rejected["failures"])
        build["foliage_instances"] = 4676
        build["crag_instances"] = expected_crags - 1
        rejected = evaluate_zone(zone_spec, terrain, build, visual, overview_projection_report=projection, traversal_report=traversal, navigation_report=navigation)
        self.assertEqual("failed", rejected["status"])
        self.assertIn("Godot scene placed 95 of 96 planned landform dressing assets", rejected["failures"])
        build["crag_instances"] = expected_crags
        terrain["landforms"][0]["relief_m"] = 1.0
        self.assertEqual("failed", evaluate_zone(zone_spec, terrain, build, visual, overview_projection_report=projection, traversal_report=traversal, navigation_report=navigation)["status"])

    def test_acceptance_rejects_flattened_sawtooth_ridge(self):
        annotations = json.loads(json.dumps(self.annotations))
        annotations["features"].append({
            "id": "acceptance_ridge", "category": "landform", "semantic": "alpine_ridge",
            "geometry": {"type": "polygon", "points": [[0.32, 0.08], [0.58, 0.11], [0.61, 0.19], [0.36, 0.22]]},
            "generation": {"profile": "alpine_sawtooth_ridge", "elevation_m": [75, 210], "cliffness": 0.78},
            "properties": {}, "evidence": [{"image_id": "overview", "region": [0.0, 0.0, 1.0, 1.0], "note": "Acceptance-test sawtooth ridge"}], "confidence": 0.92, "review_state": "reviewed",
        })
        zone_spec = compile_annotations(annotations).zone_spec
        terrain = rasterize_zone_spec(zone_spec, resolution=193).manifest
        terrain["terrain_materials"] = resolve_terrain_materials(
            zone_spec["terrain_materials"],
            CONTENT_ROOT,
            CONTENT_ROOT / "assets",
            zone_spec["terrain_material_scale_m"],
        )
        build = {"zone_id": "caledonia_concept_v1", "scene_written": True, "keep_count": 2, "lane_path_count": 3, "lane_surface_count": 3, "navigation_vertex_count": 2501, "navigation_polygon_count": 2200, "waterway_count": 8, "bridge_count": 17, "ruin_count": 1, "foliage_instances": 4676, "crag_instances": 96, "multimesh_batch_count": 30, "multimesh_instance_count": 10492, "multimesh_serialized_instance_count": 10492, "multimesh_buffers_valid": True, "multimesh_spatial_chunk_batch_count": 30, "multimesh_max_instances_per_chunk": 420, "multimesh_lod_instance_counts": {"lod0": 2860, "lod1": 2860, "lod2": 2860}, "terrain_mesh_external": True, "terrain_mesh_resource_bytes": 1024, "terrain_collision_external": True, "terrain_collision_sample_count": 263169, "terrain_collision_resource_bytes": 1024, "terrain_material_requested_layer_count": 5, "terrain_material_bound_layer_count": 5, "terrain_material_pbr_layer_count": 5, "terrain_material_minimum_texels_per_meter": 128.0, "terrain_material_meters_per_repeat": zone_spec["terrain_material_scale_m"], "terrain_wetland_mask_bound": True, "scene_bytes": 1024 * 1024, "settlement_count": 10, "settlement_grounded_component_count": 60, "settlement_grounding_max_residual_m": 0.03, "asset_assignment_count": 42, "missing_assets": []}
        build.update({"navigation_off_mesh_link_count": 17, "navigation_enabled_off_mesh_link_count": 17, "navigation_full_span_off_mesh_link_count": 17, "landmark_asset_variants": {"highland_fortified_keep": ["a", "b"], "arcane_objective_ruin": ["a"], "highland_stone_bridge": ["a", "b", "c", "d"], "highland_settlement_clusters": ["a", "b", "c"]}})
        visual = {"zone_id": "caledonia_concept_v1", "status": "passed"}
        projection = {"zone_id": "caledonia_concept_v1", "status": "passed"}
        traversal = {"zone_id": "caledonia_concept_v1", "status": "passed"}
        navigation = {"zone_id": "caledonia_concept_v1", "status": "passed"}
        self.assertEqual("passed", evaluate_zone(zone_spec, terrain, build, visual, overview_projection_report=projection, traversal_report=traversal, navigation_report=navigation)["status"])
        ridge = next(entry for entry in terrain["landforms"] if entry["id"] == "acceptance_ridge")
        ridge["p95_slope"] = 0.01
        self.assertEqual("failed", evaluate_zone(zone_spec, terrain, build, visual, overview_projection_report=projection, traversal_report=traversal, navigation_report=navigation)["status"])

    def test_acceptance_rejects_corrugated_boundary_massif(self):
        annotations = json.loads(
            (
                CONTENT_ROOT
                / "concept_batches"
                / "codeweald_alpine_arena_v1"
                / "annotations.json"
            ).read_text(encoding="utf-8")
        )
        zone_spec = compile_annotations(annotations).zone_spec
        for feature in zone_spec["features"]:
            if feature.get("generation", {}).get("profile") == "alpine_jagged_massif":
                feature["generation"]["composition"] = {
                    "silhouette": "continuous_boundary_wall",
                    "massing": "terrain_primary",
                    "surface": "fractured_granite",
                    "dressing": "sparse",
                }
        terrain = rasterize_zone_spec(zone_spec, resolution=193).manifest
        massif = next(
            entry
            for entry in terrain["landforms"]
            if entry["profile"] == "alpine_jagged_massif"
            and entry["art_profile"]["silhouette"]
            == "continuous_boundary_wall"
        )
        self.assertGreaterEqual(massif["sidewall_profile_count"], 4)
        self.assertLessEqual(massif["sidewall_corrugation_ratio"], 0.04)
        massif["sidewall_corrugation_ratio"] = 0.08
        build = {
            "zone_id": zone_spec["zone"]["id"],
            "scene_written": True,
            "keep_count": 2,
            "lane_path_count": 3,
            "lane_surface_count": 3,
            "navigation_vertex_count": 2501,
            "navigation_polygon_count": 2200,
            "waterway_count": 8,
            "bridge_count": 17,
            "ruin_count": 1,
            "foliage_instances": 4676,
            "crag_instances": 96,
            "multimesh_batch_count": 30,
            "multimesh_instance_count": 10492,
            "multimesh_serialized_instance_count": 10492,
            "multimesh_buffers_valid": True,
            "multimesh_spatial_chunk_batch_count": 30,
            "multimesh_max_instances_per_chunk": 420,
            "multimesh_lod_instance_counts": {
                "lod0": 2860,
                "lod1": 2860,
                "lod2": 2860,
            },
            "terrain_mesh_external": True,
            "terrain_mesh_resource_bytes": 1024,
            "terrain_collision_external": True,
            "terrain_collision_sample_count": 263169,
            "terrain_collision_resource_bytes": 1024,
            "terrain_material_requested_layer_count": 5,
            "terrain_material_bound_layer_count": 5,
            "terrain_material_pbr_layer_count": 5,
            "terrain_material_minimum_texels_per_meter": 128.0,
            "terrain_material_meters_per_repeat": zone_spec[
                "terrain_material_scale_m"
            ],
            "terrain_wetland_mask_bound": True,
            "scene_bytes": 1024 * 1024,
            "settlement_count": 10,
            "settlement_grounded_component_count": 60,
            "settlement_grounding_max_residual_m": 0.03,
            "asset_assignment_count": 42,
            "missing_assets": [],
        }
        report = evaluate_zone(
            zone_spec,
            terrain,
            build,
            {"zone_id": zone_spec["zone"]["id"], "status": "passed"},
        )
        self.assertIn(
            "has repeated sidewall corrugation", " ".join(report["failures"])
        )

    def test_asset_profiles_resolve_to_portable_then_godot_paths(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        catalog = {
            "schema_version": CATALOG_VERSION,
            "assets": [
                {
                    "id": "pine_%d" % index,
                    "source_path": "assets/generated/codeweald_highland_foliage/pine_%d_lod0.glb" % index,
                    "format": "glb",
                    "tags": ["foliage", "tree", "conifer", "lod0"],
                    "engine_readiness": {"godot": True},
                }
                for index in range(6)
            ]
            + [
                {
                    "id": "cliff_%d" % index,
                    "source_path": "assets/generated/codeweald_alpine_granite/cliff_%d.glb" % index,
                    "tags": ["rock", "cliff", "alpine"],
                    "engine_readiness": {"godot": index != 5},
                }
                for index in range(6)
            ]
            + [
                {"id": "shrub_%d" % index, "source_path": "assets/models/quaternius_nature/glTF/Bush_%d.gltf" % index, "tags": ["foliage", "shrub"], "engine_readiness": {"godot": True}}
                for index in range(2)
            ]
            + [
                {"id": "rock_%d" % index, "source_path": "assets/models/quaternius_nature/glTF/Rock_%d.gltf" % index, "tags": ["rock"], "engine_readiness": {"godot": True}}
                for index in range(4)
            ]
            + [
                {"id": "ruin_%d" % index, "source_path": "assets/generated/codeweald_arcane/ruin_%d.glb" % index, "tags": ["structure", "ruin"], "engine_readiness": {"godot": True}}
                for index in range(3)
            ]
            + [
                {"id": "keep_%d" % index, "source_path": "assets/generated/codeweald_highland_keep/highland_fortified_keep_%d.glb" % index, "format": "glb", "tags": ["structure", "fortification"], "engine_readiness": {"godot": True}}
                for index in range(3)
            ]
            + [
                {"id": "bridge_%d" % index, "source_path": "assets/generated/codeweald_highland_bridge/highland_stone_bridge_%d.glb" % index, "format": "glb", "tags": ["structure", "bridge"], "engine_readiness": {"godot": True}}
                for index in range(4)
            ]
            + [
                {
                    "id": "settlement_%d" % index,
                    "source_path": "assets/generated/codeweald_highland_settlement/highland_settlement_cluster_%s.glb" % chr(ord("a") + index),
                    "format": "glb",
                    "tags": ["structure", "settlement"],
                    "engine_readiness": {"godot": True},
                }
                for index in range(3)
            ],
        }
        plan = resolve_asset_plan(zone_spec, catalog)
        self.assertEqual(ASSET_PLAN_VERSION, plan["schema_version"])
        self.assertEqual(42, len(plan["assignments"]))
        godot = adapt_asset_plan(plan)
        west = next(entry for entry in godot["assignments"] if entry["feature_id"] == "western_alps")
        self.assertEqual(5, len(west["assets"]))
        self.assertEqual(["assets/generated/codeweald_alpine_granite/cliff_5.glb"], west["unavailable_assets"])
        portable_west = next(entry for entry in plan["assignments"] if entry["feature_id"] == "western_alps")
        self.assertEqual(0.58, portable_west["layers"][0]["appearance_requirements"]["maximum_mean_luminance"])
        forest = next(entry for entry in godot["assignments"] if entry["feature_id"] == "central_forest")
        self.assertEqual(["canopy", "undergrowth", "field_rock"], [layer["id"] for layer in forest["layers"]])
        keep = next(entry for entry in godot["assignments"] if entry["feature_id"] == "albion_keep")
        self.assertEqual("faction_fortification", keep["role"])
        self.assertEqual(3, len(keep["assets"]))
        ruin = next(entry for entry in godot["assignments"] if entry["feature_id"] == "central_ruin")
        self.assertEqual("objective_landmark", ruin["role"])
        self.assertEqual(3, len(ruin["assets"]))

    def test_asset_catalog_does_not_misclassify_alpine_as_pine(self):
        tags = _tags(Path("assets/generated/codeweald_alpine/alpine_cliff_ridge_a.glb"))
        self.assertTrue({"alpine", "cliff", "rock"}.issubset(tags))
        self.assertFalse({"foliage", "tree", "conifer"}.intersection(tags))

    def test_asset_catalog_marks_generated_conifer_lod_variants(self):
        tags = _tags(Path("assets/generated/codeweald_highland_foliage/windswept_spruce_lod0.glb"))
        self.assertTrue({"foliage", "tree", "conifer", "lod0"}.issubset(tags))
        self.assertIn("lod2", _tags(Path("assets/generated/codeweald_highland_foliage/mountain_fir_lod2.glb")))
        self.assertTrue({"alpine", "cliff", "rock"}.issubset(_tags(Path("assets/generated/codeweald_alpine_granite/alpine_granite_formation_a.glb"))))
        self.assertTrue(
            {"structure", "settlement"}.issubset(
                _tags(
                    Path(
                        "assets/generated/codeweald_highland_settlement/"
                        "highland_settlement_cluster_a.glb"
                    )
                )
            )
        )

    def test_asset_plan_attaches_only_complete_observed_lod_families(self):
        assets = [
            {
                "id": tier,
                "source_path": "assets/forest/pine_%s.glb" % tier,
                "tags": ["foliage", "tree", "conifer", tier],
                "engine_readiness": {"godot": True},
            }
            for tier in ("lod0", "lod1", "lod2")
        ]
        resolved = _attach_lod_siblings([assets[0]], assets)
        self.assertEqual(
            {"lod0", "lod1", "lod2"}, set(resolved[0]["lod_assets"])
        )
        with self.assertRaisesRegex(ZoneCompileError, "missing catalog siblings"):
            _attach_lod_siblings([assets[0]], assets[:2])

    def test_visual_acceptance_rejects_black_render_and_accepts_readable_structure(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        # This test isolates minimum render integrity. Regional palette failure
        # behavior is asserted separately below.
        zone_spec["acceptance_policy"][
            "maximum_regional_palette_mismatch_fraction"
        ] = 1.0
        source = np.full((720, 1280, 3), 0.2, dtype=np.float32)
        black = np.zeros((720, 1280, 3), dtype=np.float32)
        self.assertEqual("failed", evaluate_visual(zone_spec, source, black)["status"])
        y, x = np.indices((720, 1280))
        checker = ((x // 16 + y // 16) % 2).astype(np.float32)
        readable = np.empty((720, 1280, 3), dtype=np.float32)
        readable[:, :, 0] = 0.20 + checker * 0.12
        readable[:, :, 1] = 0.30 + checker * 0.16
        readable[:, :, 2] = 0.08 + checker * 0.05
        self.assertNotEqual("failed", evaluate_visual(zone_spec, source, readable)["status"])
        rendered_image = Image.fromarray(
            np.clip(readable * 255.0, 0, 255).astype(np.uint8), mode="RGB"
        )
        draw = ImageDraw.Draw(rendered_image)
        corridors = []
        for feature in zone_spec["features"]:
            if feature["category"] != "corridor" or feature["semantic"] != "lane":
                continue
            screen_points = feature["geometry"]["source_points"]
            pixel_points = [
                (round(point[0] * 1280), round(point[1] * 720))
                for point in screen_points
            ]
            draw.line(pixel_points, fill=(150, 105, 48), width=12)
            corridors.append({
                "feature_id": feature["id"],
                "found": True,
                "screen_normalized": screen_points,
                "half_width_pixels": [6.0] * len(screen_points),
            })
        projected_render = np.asarray(rendered_image, dtype=np.float32) / 255.0
        projected_report = evaluate_visual(
            zone_spec,
            source,
            projected_render,
            {"corridors": corridors},
        )
        self.assertFalse(
            any("lane" in failure for failure in projected_report["failures"])
        )
        self.assertIn(
            "corridor_color_contrast",
            projected_report["feature_observations"]["central_lane"],
        )
        self.assertGreater(
            projected_report["metrics"]["regional_palette_sample_count"], 0
        )
        strict_palette = json.loads(json.dumps(zone_spec))
        strict_palette["acceptance_policy"][
            "maximum_regional_palette_mismatch_fraction"
        ] = 0.0
        rejected_palette = evaluate_visual(
            strict_palette, source, projected_render
        )
        self.assertIn(
            "Regional palette mismatch fraction",
            " ".join(rejected_palette["failures"]),
        )

        landmarks = []
        for feature in zone_spec["features"]:
            if feature["category"] != "landmark":
                continue
            region = feature["evidence"][0]["region"]
            center_x = (region[0] + region[2]) * 0.5
            center_y = (region[1] + region[3]) * 0.5
            width = (region[2] - region[0]) * 0.25
            height = (region[3] - region[1]) * 0.25
            landmarks.append(
                {
                    "feature_id": feature["id"],
                    "screen_normalized": [center_x, center_y],
                    "source_evidence_region": region,
                    "evidence_screen_bounds_normalized": [
                        center_x - width * 0.5,
                        center_y - height * 0.5,
                        center_x + width * 0.5,
                        center_y + height * 0.5,
                    ],
                }
            )
        landmarks[0]["evidence_screen_bounds_normalized"] = [
            0.5,
            0.5,
            0.50001,
            0.50001,
        ]
        rejected_scale = evaluate_visual(
            zone_spec,
            source,
            projected_render,
            {"corridors": corridors, "landmarks": landmarks},
        )
        self.assertIn(
            "implausible source-relative scale",
            " ".join(rejected_scale["failures"]),
        )
        self.assertIn(
            "source_relative_palette_status",
            projected_report["feature_observations"]["central_lane"],
        )

    def test_runtime_effects_are_semantic_and_portable(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        runtime = build_runtime_effects(zone_spec)
        self.assertEqual(RUNTIME_EFFECTS_VERSION, runtime["schema_version"])
        self.assertEqual({"water_flow", "objective_pulse", "foliage_wind", "banner_wave"}, {effect["kind"] for effect in runtime["effects"]})
        albion_banner = next(effect for effect in runtime["effects"] if effect["feature_id"] == "albion_keep")
        self.assertEqual("banner_wave", albion_banner["kind"])
        self.assertEqual([0.10, 0.32, 0.86], albion_banner["parameters"]["color_srgb"])
        ruin = next(effect for effect in runtime["effects"] if effect["feature_id"] == "central_ruin")
        self.assertLessEqual(ruin["parameters"]["light_energy_max"], 1.6)
        self.assertEqual([0.32, 0.03, 0.58], ruin["parameters"]["emission_color_srgb"])

        compact = json.loads(json.dumps(zone_spec))
        compact["derivation"] = {"area_ratio": 0.01706667}
        compact_banner = next(
            effect
            for effect in build_runtime_effects(compact)["effects"]
            if effect["feature_id"] == "albion_keep"
        )
        self.assertAlmostEqual(
            16.9827,
            compact_banner["parameters"]["banner_height_m"],
            places=3,
        )
        self.assertEqual(
            [1.829, 1.1758],
            compact_banner["parameters"]["banner_size_m"],
        )

    def test_unity_adapter_selects_verified_fbx_sidecars(self):
        root = CONTENT_ROOT
        zone_spec = compile_annotations(self.annotations).zone_spec
        terrain = rasterize_zone_spec(zone_spec, resolution=129).manifest
        plan = resolve_asset_plan(zone_spec, build_catalog(root, root / "assets"))
        unity = adapt_unity(zone_spec, terrain, plan, project_root=root)
        alps = next(feature for feature in unity["features"] if feature["id"] == "western_alps")
        ruin = next(feature for feature in unity["features"] if feature["id"] == "central_ruin")
        alpine_paths = [asset["path"] for asset in alps["placement"]["layers"][0]["source_assets"]]
        self.assertTrue(all(path.endswith(".fbx") and "/unity/" in path for path in alpine_paths))
        self.assertEqual("assets/generated/codeweald_arcane/unity/arcane_objective_ruin.fbx", ruin["placement"]["layers"][0]["source_assets"][0]["path"])

    def test_concept_batch_intake_preserves_source_evidence_and_writes_model_contract(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "painted_overview.png"
            from PIL import Image
            Image.new("RGB", (96, 64), (45, 70, 30)).save(source)
            batch = root / "jerall_v1"
            manifest = create_batch(
                batch, "jerall_v1", "A cold mountain pass with a fortified valley.",
                [("overview", source)], {"overview": "painted_overview"}, 3200.0, 2200.0, ["godot", "unity", "unreal"],
            )
            self.assertEqual(INTAKE_VERSION, manifest["schema_version"])
            self.assertTrue((batch / "source" / "overview.png").is_file())
            self.assertIn("alpine_massif", (batch / "vision_annotation_task.md").read_text(encoding="utf-8"))
            self.assertTrue((batch / "vision_annotation_packet.json").is_file())
            self.assertTrue((batch / "model_response.template.json").is_file())
            draft = json.loads((batch / "annotations.draft.json").read_text(encoding="utf-8"))
            self.assertEqual("painted_overview", draft["source_images"][0]["role"])
            with self.assertRaisesRegex(ZoneCompileError, "features must be a non-empty array"):
                compile_annotations(draft, source_root=batch)

    def test_multi_image_batch_reconciles_views_without_using_vistas_as_map_geometry(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            overview = root / "overview.png"
            vista = root / "western_alps_vista.png"
            Image.new("RGB", (120, 80), (42, 61, 31)).save(overview)
            Image.new("RGB", (96, 96), (70, 78, 82)).save(vista)
            batch = root / "multi_view_zone"
            manifest = create_batch(
                batch,
                "multi_view_zone",
                "A three-lane Highland valley with close alpine reference art.",
                [("western_vista", vista), ("overview", overview)],
                {"overview": "painted_overview", "western_vista": "perspective"},
                2400.0,
                1600.0,
                ["godot"],
                "overview",
            )
            reconciliation = manifest["image_reconciliation"]
            self.assertEqual(
                "overview", reconciliation["canonical_map_image_id"]
            )
            policies = {
                policy["image_id"]: policy
                for policy in reconciliation["source_policies"]
            }
            self.assertEqual(
                "canonical_world_rect",
                policies["overview"]["registration"]["type"],
            )
            self.assertEqual(
                "evidence_only",
                policies["western_vista"]["registration"]["type"],
            )
            self.assertNotIn(
                "topology", policies["western_vista"]["allowed_claims"]
            )

            features = json.loads(json.dumps(self.annotations["features"]))
            for feature in features:
                feature["geometry"]["source_image_id"] = "overview"
                for evidence in feature["evidence"]:
                    evidence["claim"] = "topology"
                    evidence["visibility"] = "direct"
            features[0]["evidence"].append(
                {
                    "image_id": "western_vista",
                    "region": [0.05, 0.08, 0.95, 0.92],
                    "claim": "silhouette",
                    "visibility": "partial",
                    "note": "Perspective silhouette evidence for the same named landform.",
                }
            )
            response = {
                "schema_version": VISION_RESPONSE_VERSION,
                "zone_name": "Reconciled Highland",
                "asset_profiles": self.annotations["asset_profiles"],
                "terrain_materials": self.annotations["terrain_materials"],
                "terrain_material_scale_m": self.annotations[
                    "terrain_material_scale_m"
                ],
                "features": features,
            }
            response_path = batch / "model_response.json"
            response_path.write_text(json.dumps(response), encoding="utf-8")
            report = ingest_response(batch, response_path)
            self.assertEqual("accepted_for_review", report["status"])
            validation = report["validation"]
            self.assertGreater(validation["source_image_usage"]["overview"], 0)
            self.assertEqual(
                1, validation["source_image_usage"]["western_vista"]
            )
            self.assertEqual(1, validation["multi_view_feature_count"])
            proposal = json.loads(
                (batch / "annotations.proposed.json").read_text(encoding="utf-8")
            )
            self.assertEqual(
                reconciliation, proposal["image_reconciliation"]
            )
            compiled = compile_annotations(proposal, source_root=batch).zone_spec
            self.assertEqual(
                (batch / "source" / "overview.png").resolve(),
                _canonical_source_path(compiled, batch).resolve(),
            )
            self.assertEqual(
                [0.2, 0.22, 0.23],
                _style_palette(compiled, batch)["rock"],
            )

            bad_geometry = json.loads(json.dumps(response))
            del bad_geometry["features"][0]["geometry"]["source_image_id"]
            response_path.write_text(json.dumps(bad_geometry), encoding="utf-8")
            with self.assertRaisesRegex(
                ZoneCompileError, "geometry.source_image_id is required"
            ):
                ingest_response(batch, response_path)

            bad_claim = json.loads(json.dumps(response))
            bad_claim["features"][0]["evidence"][-1]["claim"] = "topology"
            response_path.write_text(json.dumps(bad_claim), encoding="utf-8")
            with self.assertRaisesRegex(
                ZoneCompileError, "perspective cannot support topology"
            ):
                ingest_response(batch, response_path)

            unused_vista = json.loads(json.dumps(response))
            unused_vista["features"][0]["evidence"].pop()
            response_path.write_text(json.dumps(unused_vista), encoding="utf-8")
            with self.assertRaisesRegex(
                ZoneCompileError, "did not reconcile source image"
            ):
                ingest_response(batch, response_path)

    def test_multi_image_intake_rejects_ambiguous_map_authority(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first = root / "first.png"
            second = root / "second.png"
            Image.new("RGB", (64, 64), (30, 40, 50)).save(first)
            Image.new("RGB", (64, 64), (35, 45, 55)).save(second)
            with self.assertRaisesRegex(
                IntakeError, "exactly one explicit canonical map"
            ):
                create_batch(
                    root / "ambiguous",
                    "ambiguous_zone",
                    "Two map candidates need an explicit authority.",
                    [("first", first), ("second", second)],
                    {"first": "painted_overview", "second": "minimap"},
                    1000.0,
                    1000.0,
                    ["godot"],
                )

    def test_vision_response_adapter_preserves_batch_authority_and_compiles_proposal(self):
        from PIL import Image
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "overview.png"
            Image.new("RGB", (96, 64), (48, 68, 33)).save(source)
            batch = root / "zone"
            create_batch(batch, "vision_zone", "A fortified alpine valley.", [("overview", source)], {"overview": "painted_overview"}, 2400.0, 1600.0, ["godot", "unity", "unreal"])
            response = {
                "schema_version": VISION_RESPONSE_VERSION,
                "zone_name": "Vision Valley",
                "asset_profiles": self.annotations["asset_profiles"],
                "terrain_materials": self.annotations["terrain_materials"],
                "terrain_material_scale_m": self.annotations[
                    "terrain_material_scale_m"
                ],
                "features": self.annotations["features"],
            }
            response_path = batch / "model_response.json"
            response_path.write_text(json.dumps(response), encoding="utf-8")
            report = ingest_response(batch, response_path)
            self.assertEqual("accepted_for_review", report["status"])
            proposal = json.loads((batch / "annotations.proposed.json").read_text(encoding="utf-8"))
            self.assertEqual("vision_zone", proposal["zone"]["id"])
            self.assertEqual("Vision Valley", proposal["zone"]["name"])
            self.assertEqual(
                self.annotations["terrain_material_scale_m"],
                proposal["terrain_material_scale_m"],
            )
            self.assertEqual(self.annotations["source_images"][0]["id"], proposal["source_images"][0]["id"])
            self.assertTrue((batch / "vision_proposal_evidence_overlay.png").is_file())
            response["source_images"] = []
            response_path.write_text(json.dumps(response), encoding="utf-8")
            with self.assertRaisesRegex(ZoneCompileError, "forbidden keys"):
                ingest_response(batch, response_path)

    def test_texture_material_lane_rejects_frames_and_writes_pbr_maps_for_tileable_candidate(self):
        height = width = 256
        rng = np.random.default_rng(2112)
        noise = rng.normal(0.0, 1.0, (height, width))
        tile = sum(
            np.roll(np.roll(noise, y, axis=0), x, axis=1)
            for y in (-2, 0, 2)
            for x in (-2, 0, 2)
        ) / 9.0
        tile = 0.52 + tile / tile.std() * 0.12
        good = np.dstack([tile * 0.72, tile * 0.88, tile * 0.42]).clip(0.0, 1.0).astype(np.float32)
        self.assertNotEqual("failed", assess_texture(good)["status"])
        axis = np.arange(width, dtype=np.float32) / width
        x, y = np.meshgrid(axis, axis)
        repeated = 0.48 + 0.18 * np.sin(
            (5.0 * x + 4.0 * y + 0.2 * np.sin(3.0 * x * np.pi * 2.0))
            * np.pi
            * 2.0
        )
        repeated = np.dstack([repeated] * 3).astype(np.float32)
        self.assertIn(
            "repeated spectral motif",
            " ".join(assess_texture(repeated)["failures"]),
        )
        framed = good.copy()
        framed[:, 0] = 0.0
        framed[:, -1] = 1.0
        self.assertEqual("failed", assess_texture(framed)["status"])
        gridded = good.copy()
        gridded[:, ::16] = 0.0
        gridded[::16, :] = 1.0
        self.assertIn("line/grid artifacts", " ".join(assess_texture(gridded)["failures"]))
        # Fine grain and coarse structure are indistinguishable to the existing
        # tonal check, and a terrain material is almost never seen at one texel
        # per pixel: the reference overview averages 36 or more texels into each
        # screen pixel, so a texture made entirely of grain arrives as a flat
        # colour no matter how varied it looks in isolation.
        grain = rng.normal(0.0, 1.0, (height, width))
        grain = 0.5 + grain / grain.std() * 0.12
        grain = np.dstack([grain] * 3).clip(0.0, 1.0).astype(np.float32)
        from PIL import Image as _Image
        low = rng.normal(0.0, 1.0, (max(height // 48, 4), max(width // 48, 4)))
        blobs = np.asarray(
            _Image.fromarray(low.astype(np.float32), mode="F").resize(
                (width, height), _Image.BICUBIC
            ),
            dtype=np.float32,
        )
        blobs = 0.5 + blobs / blobs.std() * 0.12
        blobs = np.dstack([blobs] * 3).clip(0.0, 1.0).astype(np.float32)
        grain_report, blob_report = assess_texture(grain), assess_texture(blobs)
        self.assertAlmostEqual(
            grain_report["metrics"]["luminance_stddev"],
            blob_report["metrics"]["luminance_stddev"],
            delta=0.02,
            msg="fixtures must be indistinguishable to the existing check",
        )
        self.assertLess(
            grain_report["metrics"]["coarse_contrast"],
            blob_report["metrics"]["coarse_contrast"],
        )
        self.assertIn(
            "flattens to a single colour",
            " ".join(grain_report["warnings"]),
        )
        self.assertNotIn(
            "flattens to a single colour",
            " ".join(blob_report["warnings"]),
        )
        # A warning, never a failure: a material may be authored for close
        # inspection, and the distance it flattens at depends on a repeat scale
        # this check cannot see.
        self.assertNotEqual("failed", grain_report["status"])
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "peat_grass_candidate.png"
            from PIL import Image
            Image.fromarray((good * 255.0).astype(np.uint8), mode="RGB").save(candidate)
            manifest = materialize(candidate, root / "peat_grass", "peat_grass")
            self.assertEqual(MATERIAL_VERSION, manifest["schema_version"])
            self.assertIn(manifest["status"], {"accepted", "accepted_with_warnings"})
            self.assertTrue((root / "peat_grass" / "normal.png").is_file())
            self.assertTrue((root / "peat_grass" / "roughness.png").is_file())

    def test_texture_material_seam_repair_is_narrow_and_auditable(self):
        height = width = 256
        rng = np.random.default_rng(8675309)
        noise = rng.normal(0.0, 1.0, (height, width))
        smooth = sum(
            np.roll(np.roll(noise, y, axis=0), x, axis=1)
            for y in (-2, 0, 2)
            for x in (-2, 0, 2)
        ) / 9.0
        value = 0.52 + smooth / smooth.std() * 0.12
        tile = np.dstack([value * 0.72, value * 0.88, value * 0.42]).clip(0.0, 1.0).astype(np.float32)
        seamed = tile.copy()
        seamed[:, -1] = np.clip(seamed[:, -1] + 0.35, 0.0, 1.0)
        self.assertEqual(["opposite texture edges have a visible tiling discontinuity"], assess_texture(seamed)["failures"])
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            from PIL import Image
            source = root / "seamed_granite.png"
            Image.fromarray((seamed * 255.0).astype(np.uint8), mode="RGB").save(source)
            manifest = materialize(source, root / "seamed_granite", "seamed_granite", repair_seams=True)
            self.assertEqual("accepted", manifest["status"])
            self.assertTrue(manifest["processing"]["seam_repair"])
            report = json.loads((root / "seamed_granite" / "quality_report.json").read_text(encoding="utf-8"))
            self.assertEqual("passed", report["status"])
            self.assertAlmostEqual(0.0, report["metrics"]["edge_discontinuity_ratio"])

    def test_mean_linear_luminance_decodes_srgb_before_averaging(self):
        mid_grey = np.full((4, 4, 3), 0.5, dtype=np.float32)
        # 0.5 sRGB decodes to roughly 0.214 in linear light; averaging the
        # encoded values first (giving 0.5) is exactly the mistake this test
        # is here to catch.
        self.assertAlmostEqual(0.2140, mean_linear_luminance(mid_grey), places=3)
        black = np.zeros((4, 4, 3), dtype=np.float32)
        self.assertEqual(0.0, mean_linear_luminance(black))
        white = np.ones((4, 4, 3), dtype=np.float32)
        self.assertAlmostEqual(1.0, mean_linear_luminance(white))
        with self.assertRaises(TextureMaterialError):
            mean_linear_luminance(np.zeros((4, 4), dtype=np.float32))

    def test_terrain_material_contract_requires_an_accepted_hash_verified_bundle(self):
        with tempfile.TemporaryDirectory() as temporary:
            project = Path(temporary)
            directory = project / "assets" / "generated" / "codeweald_materials" / "highland_granite"
            directory.mkdir(parents=True)
            maps = {}
            for kind in ("albedo", "normal", "roughness"):
                path = directory / (kind + ".png")
                Image.new("RGB", (256, 256), (96, 112, 128)).save(path)
                maps[kind] = {"path": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
            manifest = {"schema_version": MATERIAL_VERSION, "material_id": "highland_granite", "status": "accepted", "maps": maps}
            (directory / "material_manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            resolved = resolve_terrain_materials({"rock": "highland_granite"}, project, project / "assets")
            self.assertEqual("assets/generated/codeweald_materials/highland_granite/albedo.png", resolved["rock"]["maps"]["albedo"]["path"])
            self.assertIn("albedo_mean_linear_luminance", resolved["rock"])
            self.assertGreater(resolved["rock"]["albedo_mean_linear_luminance"], 0.0)
            self.assertLess(resolved["rock"]["albedo_mean_linear_luminance"], 1.0)
            (directory / "normal.png").write_bytes(b"tampered")
            with self.assertRaisesRegex(TextureMaterialError, "does not match"):
                resolve_terrain_materials({"rock": "highland_granite"}, project, project / "assets")

    def test_reviewed_batch_resolves_all_regional_pbr_contracts_at_world_scale(self):
        project = CONTENT_ROOT
        resolved = resolve_terrain_materials(
            self.annotations["terrain_materials"],
            project,
            project / "assets",
            self.annotations["terrain_material_scale_m"],
        )
        self.assertEqual({"grass", "road", "rock", "wetland", "snow"}, set(resolved))
        for layer, contract in resolved.items():
            self.assertEqual({"albedo", "normal", "roughness"}, set(contract["maps"]))
            self.assertEqual(
                self.annotations["terrain_material_scale_m"][layer],
                contract["meters_per_repeat"],
            )
            self.assertGreaterEqual(contract["minimum_texels_per_meter"], 48.0)
            self.assertIn("albedo_mean_linear_luminance", contract)
            self.assertGreater(contract["albedo_mean_linear_luminance"], 0.0)
            self.assertLess(contract["albedo_mean_linear_luminance"], 1.0)

    def test_unity_and_unreal_manifests_preserve_terrain_and_asset_intent(self):
        zone_spec = compile_annotations(self.annotations).zone_spec
        raster = rasterize_zone_spec(zone_spec, resolution=129)
        with tempfile.TemporaryDirectory() as temporary:
            write_raster(raster, Path(temporary))
            raster.manifest.setdefault("engine_artifacts", {})["unreal"] = write_unreal_artifacts(Path(temporary))
            self.assertTrue((Path(temporary) / "unreal" / "landscape_height_16.png").exists())
            self.assertTrue((Path(temporary) / "unreal" / "snow_weight.png").exists())
        terrain = raster.manifest
        catalog = {
            "schema_version": CATALOG_VERSION,
            "assets": [
                {"id": "pine_%d" % index, "source_path": "assets/generated/codeweald_highland_foliage/pine_%d_lod0.glb" % index, "format": "glb", "sha256": "a" * 64, "tags": ["foliage", "tree", "conifer", "lod0"], "engine_readiness": {"godot": True}}
                for index in range(6)
            ]
            + [
                {"id": "cliff_%d" % index, "source_path": "assets/generated/codeweald_alpine_granite/cliff_%d.glb" % index, "format": "glb", "sha256": "b" * 64, "tags": ["rock", "cliff", "alpine"], "engine_readiness": {"godot": True}}
                for index in range(6)
            ]
            + [
                {"id": "shrub_%d" % index, "source_path": "assets/models/quaternius_nature/glTF/Bush_%d.gltf" % index, "format": "gltf", "sha256": "c" * 64, "tags": ["foliage", "shrub"], "engine_readiness": {"godot": True}}
                for index in range(2)
            ]
            + [
                {"id": "rock_%d" % index, "source_path": "assets/models/quaternius_nature/glTF/Rock_%d.gltf" % index, "format": "gltf", "sha256": "d" * 64, "tags": ["rock"], "engine_readiness": {"godot": True}}
                for index in range(4)
            ]
            + [
                {"id": "ruin_%d" % index, "source_path": "assets/generated/codeweald_arcane/ruin_%d.glb" % index, "format": "glb", "sha256": "e" * 64, "tags": ["structure", "ruin"], "engine_readiness": {"godot": True}}
                for index in range(3)
            ]
            + [
                {"id": "keep_%d" % index, "source_path": "assets/generated/codeweald_highland_keep/highland_fortified_keep_%d.glb" % index, "format": "glb", "sha256": "f" * 64, "tags": ["structure", "fortification"], "engine_readiness": {"godot": True}}
                for index in range(3)
            ]
            + [
                {"id": "bridge_%d" % index, "source_path": "assets/generated/codeweald_highland_bridge/highland_stone_bridge_%d.glb" % index, "format": "glb", "sha256": "8" * 64, "tags": ["structure", "bridge"], "engine_readiness": {"godot": True}}
                for index in range(4)
            ]
            + [
                {
                    "id": "settlement_%d" % index,
                    "source_path": "assets/generated/codeweald_highland_settlement/highland_settlement_cluster_%s.glb" % chr(ord("a") + index),
                    "format": "glb",
                    "sha256": "9" * 64,
                    "tags": ["structure", "settlement"],
                    "engine_readiness": {"godot": True},
                }
                for index in range(3)
            ],
        }
        plan = resolve_asset_plan(zone_spec, catalog)
        runtime = build_runtime_effects(zone_spec)
        unity = adapt_unity(zone_spec, terrain, plan, runtime, project_root=CONTENT_ROOT)
        unreal = adapt_unreal(zone_spec, terrain, plan, runtime)
        self.assertEqual(UNITY_VERSION, unity["schema_version"])
        self.assertEqual(2400.0, unity["terrain"]["size_m"]["x"])
        self.assertTrue(any(feature["id"] == "central_forest" and feature["placement"]["requires_prefab_import"] for feature in unity["features"]))
        forest = next(feature for feature in unity["features"] if feature["id"] == "central_forest")
        self.assertEqual(3, len(forest["placement"]["layers"]))
        self.assertEqual(runtime, unity["runtime_effects"])
        self.assertEqual(UNREAL_VERSION, unreal["schema_version"])
        self.assertGreater(unreal["landscape"]["z_scale"], 0.0)
        self.assertEqual(UNREAL_LANDSCAPE_RESOLUTION, unreal["landscape"]["resolution"])
        self.assertEqual("unreal/landscape_height_16.png", unreal["landscape"]["heightmap_16"])
        self.assertTrue(any(feature["id"] == "north_lane" for feature in unreal["pcg_features"]))
        self.assertEqual(3, len(next(feature for feature in unreal["pcg_features"] if feature["id"] == "central_forest")["pcg_layers"]))
        self.assertEqual(runtime, unreal["runtime_effects"])

    def test_unreal_adapter_is_a_compiled_editor_plugin_with_an_honest_preflight_boundary(self):
        root = ENGINE_ROOT
        plugin = root / "engine_adapters" / "unreal" / "CodewealdZoneImporter"
        descriptor = json.loads((plugin / "CodewealdZoneImporter.uplugin").read_text(encoding="utf-8"))
        modules = descriptor.get("Modules", [])
        self.assertEqual("CodewealdZoneImporter", modules[0]["Name"])
        self.assertEqual("Editor", modules[0]["Type"])
        source = (plugin / "Source" / "CodewealdZoneImporter" / "Private" / "CodewealdZoneImporterModule.cpp").read_text(encoding="utf-8")
        self.assertIn("LevelEditor.MainMenu.Tools", source)
        self.assertIn("codeweald.unreal-native-preflight/v1", source)
        self.assertIn("No Unreal assets were created", source)


if __name__ == "__main__":
    unittest.main()
