from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path


PIPELINE = Path(__file__).resolve().parents[1] / "pipeline"
sys.path.insert(0, str(PIPELINE))

from derive_arena_annotations import derive
from asset_physical_acceptance import evaluate as evaluate_asset_physical


class CompactArenaDerivationTests(unittest.TestCase):
    def test_compact_derivation_preserves_landform_intent_and_limits_density(self) -> None:
        source = {
            "world_bounds": {"width": 2400, "length": 1600},
            "zone": {"id": "source"},
            "asset_profiles": {
                "forest": {
                    "role": "foliage",
                    "layers": [
                        {
                            "id": "canopy",
                            "role": "conifer_canopy",
                            "instances_per_feature": 420,
                            "minimum_spacing_m": 7,
                            "scale_m": [1.3, 1.9],
                        }
                    ],
                },
                "keep": {
                    "role": "faction_fortification",
                    "instances_per_feature": 1,
                    "scale_m": [1.0, 1.0],
                },
                "settlement": {
                    "role": "settlement_landmark",
                    "instances_per_feature": 1,
                    "scale_m": [1.0, 1.0],
                },
                "massif_dressing": {
                    "role": "landform_dressing",
                    "instances_per_feature": 24,
                    "minimum_spacing_m": 42,
                    "scale_m": [0.48, 0.82],
                },
                "regional_crags": {
                    "role": "landform_dressing",
                    "instances_per_feature": 8,
                    "minimum_spacing_m": 30,
                    "scale_m": [0.14, 0.32],
                },
            },
            "features": [
                {
                    "id": "lane",
                    "category": "corridor",
                    "semantic": "lane",
                    "properties": {"minimum_width_m": 32},
                },
                {
                    "id": "alps",
                    "category": "landform",
                    "semantic": "alpine_massif",
                    "generation": {"elevation_m": [90, 280]},
                },
                {
                    "id": "stream",
                    "category": "hydrology",
                    "semantic": "stream",
                    "properties": {"width_m": 12},
                },
                {
                    "id": "keep",
                    "category": "landmark",
                    "semantic": "faction_keep",
                    "properties": {},
                },
                {
                    "id": "hamlet",
                    "category": "landmark",
                    "semantic": "settlement_cluster",
                    "properties": {},
                },
            ],
            "traversal_policy": {"maximum_keep_lane_distance_m": 225},
        }
        target = {
            "world_bounds": {"width": 256, "length": 256},
            "zone": {"id": "target"},
            "source_images": [],
            "image_reconciliation": {},
            "generation_seed": 1,
        }

        result = derive(copy.deepcopy(source), target)

        self.assertEqual(result["features"][0]["properties"]["minimum_width_m"], 10.0)
        # The visible road matches the lane it represents. At 4.5 m it was
        # narrower than one 5 m-wide unit, so a character standing in its own
        # lane hung off both sides of the path art.
        self.assertEqual(
            result["features"][0]["properties"]["visual_width_m"],
            result["features"][0]["properties"]["minimum_width_m"],
        )
        self.assertEqual(result["features"][0]["properties"]["maximum_design_grade"], 0.12)
        self.assertTrue(result["features"][0]["properties"]["derive_proximity_crossings"])
        self.assertEqual(result["traversal_policy"]["maximum_lane_grade"], 0.25)
        self.assertEqual(result["traversal_policy"]["maximum_lane_p95_grade"], 0.20)
        self.assertEqual(result["traversal_policy"]["maximum_lane_cross_grade"], 0.20)
        self.assertEqual(
            result["asset_profiles"]["massif_dressing"]["instances_per_feature"], 4
        )
        # A compact boundary massif must remain an impassable visual wall
        # without consuming the full hierarchy of a 256 m battlefield.
        self.assertLess(result["features"][1]["generation"]["elevation_m"][1], 50)
        self.assertGreater(result["features"][1]["generation"]["elevation_m"][1], 40)
        self.assertEqual(result["features"][2]["properties"]["width_m"], 2.0)
        self.assertEqual(
            result["features"][2]["properties"]["channel_profile"],
            "wetland_rill",
        )
        self.assertEqual(result["features"][3]["properties"]["scatter_exclusion_radius_m"], 20.0)
        self.assertEqual(result["features"][4]["properties"]["scatter_exclusion_radius_m"], 16.0)
        canopy = result["asset_profiles"]["forest"]["layers"][0]
        self.assertEqual(canopy["instances_per_feature"], 28)
        self.assertEqual(canopy["maximum_slope_degrees"], 28.0)
        self.assertGreaterEqual(canopy["scale_m"][0], 0.4)
        self.assertLess(canopy["scale_m"][1], 0.75)
        # Compact forests need enough visual mass to read as woodland while
        # the placement solver still enforces non-overlap at tree scale.
        self.assertEqual(canopy["minimum_spacing_m"], 1.8)
        groundcover = next(
            layer
            for layer in result["asset_profiles"]["forest"]["layers"]
            if layer["id"] == "groundcover"
        )
        self.assertEqual(20, groundcover["instances_per_feature"])
        self.assertEqual(30.0, groundcover["maximum_slope_degrees"])
        self.assertEqual([0.22, 0.5], groundcover["scale_m"])
        self.assertEqual(["foliage", "undergrowth"], groundcover["required_tags"])
        self.assertEqual(result["asset_profiles"]["keep"]["instances_per_feature"], 1)
        self.assertLess(result["asset_profiles"]["keep"]["scale_m"][0], 0.2)
        self.assertGreater(result["asset_profiles"]["keep"]["scale_m"][0], 0.1)
        self.assertEqual(
            result["asset_profiles"]["settlement"]["scale_m"],
            [0.28, 0.34],
        )
        self.assertEqual(result["asset_profiles"]["massif_dressing"]["instances_per_feature"], 4)
        self.assertGreaterEqual(
            result["asset_profiles"]["massif_dressing"]["minimum_spacing_m"],
            18.0,
        )
        self.assertLess(result["asset_profiles"]["massif_dressing"]["scale_m"][1], 0.2)
        regional = result["asset_profiles"]["regional_crags"]
        self.assertEqual(regional["instances_per_feature"], 2)
        self.assertGreaterEqual(regional["minimum_spacing_m"], 10.0)
        self.assertGreater(regional["scale_m"][1], 0.07)
        self.assertEqual(result["derivation"]["source_zone_id"], "source")

    def test_physical_asset_gate_rejects_kaiju_canopy(self) -> None:
        asset = {
            "source_path": "assets/tree.glb",
            "sha256": "a" * 64,
        }
        plan = {
            "zone_id": "test",
            "assignments": [
                {
                    "feature_id": "forest",
                    "role": "foliage",
                    "layers": [
                        {
                            "id": "canopy",
                            "role": "conifer_canopy",
                            "runtime_enabled": True,
                            "scale_m": [1.3, 1.9],
                            "assets": [asset],
                        }
                    ],
                }
            ],
        }
        preflight = {
            "assets": [
                {
                    **asset,
                    "bounds_m": {"size": [8.0, 9.0, 20.0]},
                }
            ]
        }
        failed = evaluate_asset_physical(plan, preflight)
        self.assertEqual("failed", failed["status"])
        plan["assignments"][0]["layers"][0]["scale_m"] = [0.45, 0.65]
        passed = evaluate_asset_physical(plan, preflight)
        self.assertEqual("passed", passed["status"])

    def test_settlement_physical_gate_requires_house_scale_cluster(self) -> None:
        asset = {
            "source_path": "assets/settlement.glb",
            "sha256": "b" * 64,
        }
        plan = {
            "zone_id": "test",
            "assignments": [
                {
                    "feature_id": "hamlet",
                    "role": "settlement_landmark",
                    "runtime_enabled": True,
                    "scale_m": [0.28, 0.34],
                    "assets": [asset],
                }
            ],
        }
        preflight = {
            "assets": [
                {
                    **asset,
                    "bounds_m": {"size": [70.0, 70.0, 27.0]},
                }
            ]
        }
        report = evaluate_asset_physical(plan, preflight)
        self.assertEqual("passed", report["status"])
        record = report["records"][0]
        self.assertEqual(23.8, record["maximum_scaled_footprint_m"])
        self.assertEqual(9.18, record["maximum_scaled_height_m"])

        plan["assignments"][0]["scale_m"] = [0.13, 0.13]
        failed = evaluate_asset_physical(plan, preflight)
        self.assertEqual("failed", failed["status"])
        self.assertTrue(any("minimum for settlement_landmark" in item for item in failed["failures"]))


if __name__ == "__main__":
    unittest.main()
