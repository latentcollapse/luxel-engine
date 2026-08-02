from __future__ import annotations

import hashlib
import json
import math
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from reference_content import CONTENT_ROOT  # noqa: E402

from navigation_plan import (  # noqa: E402
    NAV_AGENT_FITS,
    NAV_PRIMARY_COMPONENT,
    NAV_WALKABLE_SURFACE,
    NavigationPlanError,
    build,
    clearance_m,
    rasterize_colliders,
)

REAL_BATCH = (
    CONTENT_ROOT / "concept_batches/codeweald_alpine_arena_v1"
)

RESOLUTION = 257
EXTENT_M = 256.0


def _canonical(document: object) -> str:
    return hashlib.sha256(
        json.dumps(
            document, ensure_ascii=False, separators=(",", ":"), sort_keys=True
        ).encode("utf-8")
    ).hexdigest()


def _keep(identifier: str, x: float, z: float) -> dict:
    return {
        "id": identifier,
        "category": "landmark",
        "semantic": "faction_keep",
        "geometry": {"type": "point", "points": [[x, z]]},
    }


def _lane(identifier: str, points: list[list[float]]) -> dict:
    return {
        "id": identifier,
        "category": "corridor",
        "semantic": "lane",
        "geometry": {"type": "polyline", "points": points},
        "properties": {"lane_id": "mid", "minimum_width_m": 10.0},
    }


def _box(identifier: str, x: float, z: float, half: float, obstructs: bool = True) -> dict:
    return {
        "id": identifier,
        "feature_id": identifier,
        "role": "settlement_landmark" if obstructs else "lane_crossing_structure",
        "shape": "box" if obstructs else "deck",
        "obstructs": obstructs,
        "centre_m": [x, 0.0, z],
        "half_extents_m": [half, 5.0, half],
        "yaw_degrees": 0.0,
    }


class _World:
    """A synthetic batch just complete enough for `build` to read."""

    def __init__(self, heights: np.ndarray, features: list[dict], colliders: list[dict]) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.path = Path(self._temporary.name)
        (self.path / "terrain").mkdir()
        heights.astype("<f4").tofile(self.path / "terrain/heightfield_f32le.bin")
        digest = hashlib.sha256(
            (self.path / "terrain/heightfield_f32le.bin").read_bytes()
        ).hexdigest()
        zone_spec = {
            "features": features,
            "traversal_policy": {
                "agent_radius_m": 2.5,
                "agent_max_climb_m": 4.0,
                "agent_max_slope_degrees": 45.0,
            },
        }
        (self.path / "zone_spec.json").write_text(json.dumps(zone_spec), encoding="utf-8")
        (self.path / "collision_plan.json").write_text(
            json.dumps({"heightfield_sha256": digest, "instance_colliders": colliders}),
            encoding="utf-8",
        )
        (self.path / "terrain/terrain_manifest.json").write_text(
            json.dumps(
                {
                    "zone_id": "synthetic",
                    "resolution": RESOLUTION,
                    "world_bounds_m": {"width": EXTENT_M, "length": EXTENT_M},
                    "height_range_m": {
                        "min": float(heights.min()),
                        "max": float(heights.max()),
                    },
                    "heightfield_sha256": digest,
                    "zone_spec_sha256": _canonical(zone_spec),
                }
            ),
            encoding="utf-8",
        )

    def close(self) -> None:
        self._temporary.cleanup()


def _flat() -> np.ndarray:
    return np.zeros((RESOLUTION, RESOLUTION), dtype=np.float64)


class ColliderRasterTests(unittest.TestCase):
    def test_a_deck_does_not_block(self) -> None:
        # Regression: `lane_crossing_structure` was a solid box, so all 20
        # bridges walled off the lanes they exist to carry and no lane ran end
        # to end. Navigation must not subtract anything that carries traffic.
        plan = {"instance_colliders": [_box("bridge", 0.0, 0.0, 8.0, obstructs=False)]}
        self.assertFalse(rasterize_colliders(plan, 65, EXTENT_M, EXTENT_M).any())

    def test_an_obstructing_box_blocks(self) -> None:
        plan = {"instance_colliders": [_box("hall", 0.0, 0.0, 8.0)]}
        self.assertTrue(rasterize_colliders(plan, 65, EXTENT_M, EXTENT_M).any())

    def test_a_rotated_box_is_not_grown_into_an_axis_aligned_one(self) -> None:
        # A long thin hall turned 45 degrees. Its true footprint stays 40x4 m;
        # an axis-aligned box recomputed in world space would swell to roughly
        # 31x31 m and block ground beside the building that a player can walk.
        collider = _box("hall", 0.0, 0.0, 10.0)
        collider["half_extents_m"] = [20.0, 5.0, 2.0]
        collider["yaw_degrees"] = 45.0
        turned = int(
            rasterize_colliders(
                {"instance_colliders": [collider]}, 257, EXTENT_M, EXTENT_M
            ).sum()
        )
        true_area = 40.0 * 4.0
        inflated_area = (40.0 * math.sqrt(0.5) + 4.0 * math.sqrt(0.5)) ** 2
        self.assertLess(abs(turned - true_area) / true_area, 0.15, turned)
        self.assertLess(turned, inflated_area * 0.5, turned)

    def test_an_unknown_shape_is_refused_rather_than_ignored(self) -> None:
        collider = _box("mystery", 0.0, 0.0, 4.0)
        collider["shape"] = "capsule"
        with self.assertRaises(NavigationPlanError):
            rasterize_colliders({"instance_colliders": [collider]}, 65, EXTENT_M, EXTENT_M)


class ClearanceFieldTests(unittest.TestCase):
    """One field answers the question for any agent radius."""

    def test_clearance_is_zero_on_blocked_ground_and_grows_away_from_it(self) -> None:
        walkable = np.ones((21, 21), dtype=bool)
        walkable[10, 10] = False
        field = clearance_m(walkable, 1.0)
        self.assertAlmostEqual(0.0, field[10, 10], places=6)
        self.assertAlmostEqual(1.0, field[10, 11], places=6)
        self.assertAlmostEqual(5.0, field[10, 15], places=6)

    def test_clearance_is_euclidean_not_chebyshev(self) -> None:
        # A diagonal neighbour is sqrt(2) away, not 1. Getting this wrong would
        # quietly let agents through diagonal gaps narrower than they are.
        walkable = np.ones((21, 21), dtype=bool)
        walkable[10, 10] = False
        self.assertAlmostEqual(math.sqrt(2.0), clearance_m(walkable, 1.0)[11, 11], places=6)

    def test_clearance_scales_with_cell_size(self) -> None:
        walkable = np.ones((21, 21), dtype=bool)
        walkable[10, 10] = False
        self.assertAlmostEqual(2.0, clearance_m(walkable, 2.0)[10, 11], places=6)

    def test_a_larger_agent_is_answered_by_the_same_field(self) -> None:
        walkable = np.ones((41, 41), dtype=bool)
        walkable[20, 20] = False
        field = clearance_m(walkable, 1.0)
        self.assertGreater((field >= 2.5).sum(), (field >= 6.0).sum())


class BlockedLaneTests(unittest.TestCase):
    """The negative case first: a gate that cannot fail is not a gate."""

    def test_a_clear_lane_runs_end_to_end(self) -> None:
        world = _World(
            _flat(),
            [_keep("a", -100.0, 0.0), _lane("mid", [[-90.0, 0.0], [90.0, 0.0]])],
            [],
        )
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        self.assertTrue(plan["lanes"][0]["runs_end_to_end"])
        self.assertEqual([], plan["lanes"][0]["obstructions"])

    def test_a_building_dropped_on_the_lane_breaks_it_and_is_named(self) -> None:
        world = _World(
            _flat(),
            [_keep("a", -100.0, 0.0), _lane("mid", [[-90.0, 0.0], [90.0, 0.0]])],
            [_box("hall", 0.0, 0.0, 12.0)],
        )
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        lane = plan["lanes"][0]
        self.assertFalse(lane["runs_end_to_end"])
        self.assertGreater(lane["longest_impassable_run_m"], 20.0)
        # Naming the obstruction is the point. A bare fraction tells an author
        # the lane is broken without telling them what to move.
        self.assertEqual(["hall"], [entry["id"] for entry in lane["obstructions"]])

    def test_a_bridge_on_the_lane_does_not_break_it(self) -> None:
        world = _World(
            _flat(),
            [_keep("a", -100.0, 0.0), _lane("mid", [[-90.0, 0.0], [90.0, 0.0]])],
            [_box("bridge", 0.0, 0.0, 12.0, obstructs=False)],
        )
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        self.assertTrue(plan["lanes"][0]["runs_end_to_end"])


class ConnectivityTests(unittest.TestCase):
    def test_a_wall_across_the_world_strands_the_far_keep(self) -> None:
        heights = _flat()
        heights[:, 129:] = 40.0
        world = _World(heights, [_keep("a", -80.0, 0.0), _keep("b", 80.0, 0.0)], [])
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        self.assertFalse(plan["topology"]["keeps_share_one_component"])
        self.assertIn("b", plan["topology"]["stranded_anchors"])

    def test_two_keeps_on_open_ground_share_a_component(self) -> None:
        world = _World(_flat(), [_keep("a", -80.0, 0.0), _keep("b", 80.0, 0.0)], [])
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        self.assertTrue(plan["topology"]["keeps_share_one_component"])
        self.assertEqual([], plan["topology"]["stranded_anchors"])


class AnchorResolutionTests(unittest.TestCase):
    def test_an_anchor_on_clear_ground_is_not_moved(self) -> None:
        world = _World(_flat(), [_keep("a", -80.0, 0.0)], [])
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        self.assertEqual(0.0, plan["anchors"][0]["displacement_m"])

    def test_an_anchor_inside_a_building_is_moved_out_and_the_offset_reported(self) -> None:
        # 2.1 must place spawns at the resolved point, never the requested one:
        # a spawn on a keep's own anchor can sit inside solid geometry.
        world = _World(_flat(), [_keep("a", 0.0, 0.0)], [_box("keep", 0.0, 0.0, 10.0)])
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        anchor = plan["anchors"][0]
        self.assertTrue(anchor["reachable"])
        self.assertGreater(anchor["displacement_m"], 10.0)
        self.assertGreater(abs(anchor["resolved_m"][0]) + abs(anchor["resolved_m"][1]), 10.0)


class ProvenanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.world = _World(_flat(), [_keep("a", 0.0, 0.0)], [])
        self.addCleanup(self.world.close)

    def test_a_collision_plan_from_another_terrain_is_refused(self) -> None:
        path = self.world.path / "collision_plan.json"
        document = json.loads(path.read_text(encoding="utf-8"))
        document["heightfield_sha256"] = "0" * 64
        path.write_text(json.dumps(document), encoding="utf-8")
        with self.assertRaises(NavigationPlanError) as caught:
            build(self.world.path)
        self.assertIn("different heightfield", str(caught.exception))

    def test_a_missing_collision_plan_is_named(self) -> None:
        (self.world.path / "collision_plan.json").unlink()
        with self.assertRaises(NavigationPlanError) as caught:
            build(self.world.path)
        self.assertIn("collision_plan.json", str(caught.exception))


class CompiledBatchTests(unittest.TestCase):
    """The real world, as actually compiled."""

    def setUp(self) -> None:
        path = REAL_BATCH / "navigation_plan.json"
        if not path.is_file():
            self.skipTest("navigation plan not built")
        self.plan = json.loads(path.read_text(encoding="utf-8"))

    def test_colliders_remove_ground_the_boundary_plan_called_playable(self) -> None:
        # boundary_plan (1.2) flood-fills terrain slope only and does not read
        # the collision plan, so it counts ground inside solid objects.
        self.assertGreater(self.plan["surface"]["blocked_by_colliders_m2"], 0.0)
        self.assertLess(
            self.plan["surface"]["navigable_area_m2"],
            self.plan["surface"]["walkable_area_m2"],
        )

    def test_no_bridge_obstructs_its_own_lane(self) -> None:
        for lane in self.plan["lanes"]:
            roles = {entry["role"] for entry in lane["obstructions"]}
            self.assertNotIn("lane_crossing_structure", roles, lane["id"])

    def test_the_mask_bits_agree_with_the_reported_areas(self) -> None:
        mask = np.fromfile(REAL_BATCH / "terrain/navigation_mask.bin", dtype=np.uint8)
        surface = self.plan["surface"]
        cell = surface["cell_m"]
        self.assertEqual(surface["resolution"] ** 2, mask.size)
        for bit, key in (
            (NAV_WALKABLE_SURFACE, "walkable_area_m2"),
            (NAV_AGENT_FITS, "navigable_area_m2"),
            (NAV_PRIMARY_COMPONENT, "primary_area_m2"),
        ):
            self.assertAlmostEqual(
                float((mask & bit > 0).sum()) * cell * cell, surface[key], places=1
            )

    def test_the_clearance_field_matches_the_agent_fits_bit(self) -> None:
        clearance = np.fromfile(
            REAL_BATCH / "terrain/navigation_clearance_f32le.bin", dtype="<f4"
        )
        mask = np.fromfile(REAL_BATCH / "terrain/navigation_mask.bin", dtype=np.uint8)
        radius = self.plan["agent"]["radius_m"]
        self.assertTrue(np.array_equal(clearance >= radius, (mask & NAV_AGENT_FITS) > 0))

    def test_no_polygonal_navmesh_is_claimed(self) -> None:
        # Stated rather than absent, so a reader does not go looking. Every
        # target engine bakes its own from collision geometry.
        self.assertIsNone(self.plan["polygonal_navmesh"])


if __name__ == "__main__":
    unittest.main()
