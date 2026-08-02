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

from boundary_plan import (  # noqa: E402
    APRON_EXTENT_DIAGONALS,
    APRON_FALLOFF_GRADE,
    KEEP_SEMANTIC,
    BoundaryPlanError,
    _erode,
    build,
    standable,
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
        "semantic": KEEP_SEMANTIC,
        "geometry": {"type": "point", "points": [[x, z]]},
    }


class _World:
    """A synthetic batch just complete enough for `build` to read."""

    def __init__(self, heights: np.ndarray, keeps: list[dict]) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.path = Path(self._temporary.name)
        (self.path / "terrain").mkdir()
        heights.astype("<f4").tofile(self.path / "terrain/heightfield_f32le.bin")
        zone_spec = {
            "features": keeps,
            "traversal_policy": {
                "agent_radius_m": 2.5,
                "agent_max_climb_m": 4.0,
                "agent_max_slope_degrees": 45.0,
            },
        }
        (self.path / "zone_spec.json").write_text(
            json.dumps(zone_spec), encoding="utf-8"
        )
        manifest = {
            "zone_id": "synthetic",
            "resolution": RESOLUTION,
            "world_bounds_m": {"width": EXTENT_M, "length": EXTENT_M},
            "height_range_m": {"min": float(heights.min()), "max": float(heights.max())},
            "heightfield_sha256": hashlib.sha256(
                (self.path / "terrain/heightfield_f32le.bin").read_bytes()
            ).hexdigest(),
            "zone_spec_sha256": _canonical(zone_spec),
        }
        (self.path / "terrain/terrain_manifest.json").write_text(
            json.dumps(manifest), encoding="utf-8"
        )

    def close(self) -> None:
        self._temporary.cleanup()


def _flat() -> np.ndarray:
    return np.zeros((RESOLUTION, RESOLUTION), dtype=np.float64)


def _walled(wall_cells: int = 24, height_m: float = 60.0) -> np.ndarray:
    """A flat field ringed by a wall too tall and steep to climb."""
    heights = _flat()
    heights[:wall_cells, :] = height_m
    heights[-wall_cells:, :] = height_m
    heights[:, :wall_cells] = height_m
    heights[:, -wall_cells:] = height_m
    return heights


class ContainmentCanFireTests(unittest.TestCase):
    """Regression: the leak test must be capable of reporting a leak.

    Reachability is computed for a body's *centre*, and erosion by the agent
    radius has already removed the outermost cells before the test runs. An
    earlier version asked whether any reachable cell sat on the outer row, which
    is structurally impossible -- it pronounced a world enclosed whose edge is
    half open. A gate that cannot fail is worse than no gate: it is a passing
    result nobody rechecks.
    """

    def test_an_open_plain_is_not_enclosed(self) -> None:
        world = _World(_flat(), [_keep("a", -60.0, -60.0), _keep("b", 60.0, 60.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        containment = plan["containment"]
        self.assertFalse(containment["enclosed"])
        self.assertGreater(containment["leak_length_m"], 0.0)
        # A wholly open plain leaks along its entire perimeter.
        self.assertGreater(containment["edge_reach_fraction"], 0.95)
        self.assertEqual(
            {"north", "south", "east", "west"},
            {span["edge"] for span in containment["leak_spans"]},
        )

    def test_a_walled_world_is_enclosed(self) -> None:
        world = _World(_walled(), [_keep("a", -30.0, -30.0), _keep("b", 30.0, 30.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        self.assertTrue(plan["containment"]["enclosed"])
        self.assertEqual([], plan["containment"]["leak_spans"])

    def test_a_gap_in_the_wall_is_reported_where_it_is(self) -> None:
        heights = _walled()
        # Punch a hole through the north wall around x = 0.
        heights[:24, 120:136] = 0.0
        world = _World(heights, [_keep("a", 0.0, 0.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        spans = plan["containment"]["leak_spans"]
        self.assertFalse(plan["containment"]["enclosed"])
        self.assertEqual(["north"], sorted({span["edge"] for span in spans}))
        self.assertTrue(
            any(span["from_m"] < 0.0 < span["to_m"] for span in spans), spans
        )


class AgentIsADiscTests(unittest.TestCase):
    """A body cannot squeeze through a gap narrower than it is."""

    def test_erosion_clears_the_agent_radius(self) -> None:
        mask = np.ones((21, 21), dtype=bool)
        mask[10, 10] = False
        eroded = _erode(mask, 3.0)
        self.assertFalse(eroded[10, 10])
        self.assertFalse(eroded[10, 12])
        self.assertTrue(eroded[10, 16])

    def test_erosion_keeps_a_body_clear_of_the_data_edge(self) -> None:
        # Off-grid counts as blocked: a body standing on the last row is
        # already half off the world.
        eroded = _erode(np.ones((21, 21), dtype=bool), 3.0)
        self.assertFalse(eroded[0, 10])
        self.assertTrue(eroded[10, 10])

    def test_a_gap_narrower_than_the_agent_does_not_leak(self) -> None:
        heights = _walled()
        # 2 cells wide at 1 m per cell -- far under the 5 m agent diameter.
        heights[:24, 128:130] = 0.0
        world = _World(heights, [_keep("a", 0.0, 0.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        self.assertTrue(plan["containment"]["enclosed"])


class SlopeAndClimbTests(unittest.TestCase):
    def test_flat_ground_is_standable_and_a_cliff_is_not(self) -> None:
        heights = _flat()
        heights[:, 128:] = 100.0
        flat = standable(heights, 1.0, 2.5, 45.0)
        self.assertTrue(flat[128, 60])
        self.assertFalse(flat[128, 127])

    def test_a_step_taller_than_the_agent_can_climb_is_a_wall(self) -> None:
        # Flat on both sides, so slope alone would let the agent walk across;
        # only the per-step climb limit stops it.
        heights = _flat()
        heights[:, 129:] = 20.0
        world = _World(heights, [_keep("a", -60.0, 0.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        self.assertLess(plan["playable"]["bounds_m"]["max_x"], 10.0)


class SeedingTests(unittest.TestCase):
    def test_keeps_are_found_by_semantic_not_by_name(self) -> None:
        world = _World(_walled(), [_keep("citadel_of_dawn", 0.0, 0.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        self.assertEqual(["citadel_of_dawn"], plan["playable"]["seeds"])

    def test_a_world_with_no_declared_start_still_compiles(self) -> None:
        # WGE is a general-purpose world compiler. This used to raise, so a
        # dungeon, a race track or an open-world zone -- none of which have
        # faction keeps -- could not compile at all. A world with no declared
        # start is still a world; it falls back to its largest standable region
        # and says so, rather than passing the fallback off as a measurement
        # from a real spawn.
        world = _World(_walled(), [])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        self.assertEqual(
            "flood_fill_from_largest_region", plan["playable"]["derivation"]
        )
        self.assertGreater(plan["playable"]["area_m2"], 0.0)
        self.assertEqual([], plan["playable"]["seeds"])

    def test_a_non_moba_start_semantic_seeds_the_region(self) -> None:
        # `spawn` is as valid a player start as `faction_keep`.
        start = _keep("arrival_hall", 0.0, 0.0)
        start["semantic"] = "spawn"
        world = _World(_walled(), [start])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        self.assertEqual(
            "flood_fill_from_declared_starts", plan["playable"]["derivation"]
        )
        self.assertEqual(["arrival_hall"], plan["playable"]["seeds"])

    def test_a_world_with_no_standable_ground_is_still_refused(self) -> None:
        # The fallback must not become "always succeed". A world the agent
        # cannot stand anywhere on has no playable region and must say so.
        vertical = np.random.default_rng(7).normal(
            0.0, 400.0, (RESOLUTION, RESOLUTION)
        )
        world = _World(vertical, [])
        self.addCleanup(world.close)
        with self.assertRaises(BoundaryPlanError) as caught:
            build(world.path)
        # Either guard is correct; what matters is that the fallback does not
        # become "always succeed".
        self.assertRegex(
            str(caught.exception), "no standable ground|reached no cells"
        )

    def test_a_keep_walled_off_from_the_others_is_named(self) -> None:
        heights = _walled()
        # Split the interior in two with an unclimbable ridge.
        heights[:, 126:132] = 60.0
        world = _World(heights, [_keep("a", -60.0, 0.0), _keep("b", 60.0, 0.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        self.assertEqual(["b"], plan["containment"]["disconnected_keeps"])
        # Two disconnected halves are not one playable region.
        self.assertFalse(plan["containment"]["enclosed"])


class ApronTests(unittest.TestCase):
    def test_extent_is_derived_from_the_world_not_picked(self) -> None:
        world = _World(_walled(), [_keep("a", 0.0, 0.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        expected = math.hypot(EXTENT_M, EXTENT_M) * APRON_EXTENT_DIAGONALS
        self.assertAlmostEqual(expected, plan["apron"]["extent_m"], places=2)

    def test_falloff_is_a_shallow_grade_not_the_worlds_relief(self) -> None:
        # It used to fall by the world's whole height range. On a map whose rim
        # sits near 0 m but whose peaks reach 51 m, that dropped a mountain's
        # height from a sea-level edge: a 16.6 degree face all the way round,
        # so the world read as a pyramid on a plate.
        world = _World(_walled(height_m=60.0), [_keep("a", 0.0, 0.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        apron = plan["apron"]
        grade = apron["falloff_depth_m"] / apron["extent_m"]
        self.assertAlmostEqual(APRON_FALLOFF_GRADE, grade, places=4)
        self.assertLess(math.degrees(math.atan(grade)), 6.0)
        # Independent of how tall the world is: a 60 m wall must not tilt the
        # horizon, which is exactly what the old derivation did.
        taller = _World(_walled(height_m=200.0), [_keep("a", 0.0, 0.0)])
        self.addCleanup(taller.close)
        self.assertEqual(
            apron["falloff_depth_m"], build(taller.path)[0]["apron"]["falloff_depth_m"]
        )

    def test_the_apron_never_collides(self) -> None:
        # An apron a body could stand on would extend the leak this module
        # exists to measure.
        world = _World(_walled(), [_keep("a", 0.0, 0.0)])
        self.addCleanup(world.close)
        plan, _ = build(world.path)
        self.assertFalse(plan["apron"]["colliding"])


class ProvenanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.world = _World(_walled(), [_keep("a", 0.0, 0.0)])
        self.addCleanup(self.world.close)

    def test_a_heightfield_the_manifest_does_not_match_is_refused(self) -> None:
        path = self.world.path / "terrain/heightfield_f32le.bin"
        path.write_bytes(path.read_bytes() + b"\x00\x00\x00\x00")
        with self.assertRaises(BoundaryPlanError) as caught:
            build(self.world.path)
        self.assertIn("manifest", str(caught.exception))

    def test_terrain_built_from_another_zone_spec_is_refused(self) -> None:
        # A boundary measured with one world's agent against another world's
        # terrain is fiction that looks like measurement.
        manifest_path = self.world.path / "terrain/terrain_manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["zone_spec_sha256"] = "0" * 64
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(BoundaryPlanError) as caught:
            build(self.world.path)
        self.assertIn("different zone spec", str(caught.exception))

    def test_a_missing_artifact_is_named(self) -> None:
        (self.world.path / "zone_spec.json").unlink()
        with self.assertRaises(BoundaryPlanError) as caught:
            build(self.world.path)
        self.assertIn("zone_spec.json", str(caught.exception))

    def test_the_plan_records_both_zone_spec_digests(self) -> None:
        # `zone_spec_sha256` means the canonical JSON in the terrain manifest
        # and the file bytes in build_report.json. Naming both spellings is
        # what lets a consumer compare against either without guessing.
        plan, _ = build(self.world.path)
        self.assertNotEqual(
            plan["zone_spec_bytes_sha256"], plan["zone_spec_canonical_sha256"]
        )


class CompiledBatchTests(unittest.TestCase):
    """The real world, as actually compiled."""

    def setUp(self) -> None:
        path = REAL_BATCH / "boundary_plan.json"
        if not path.is_file():
            self.skipTest("boundary plan not built")
        self.plan = json.loads(path.read_text(encoding="utf-8"))

    def test_the_alpine_arena_contains_its_players(self) -> None:
        # Measured 2026-07-31 and asserted the other way round until
        # 2026-08-02: the bordering mountain chain was mostly absent and the
        # shelf outside it was flat walkable ground ending in a cliff -- 756 m
        # of leak across seven spans, with north and south open end to end.
        # The border rampart (roadmap 2.5 / systems S4) closes it, and
        # `build_zone` now fails rather than warns, so this asserts the
        # property rather than the defect.
        containment = self.plan["containment"]
        self.assertTrue(containment["enclosed"])
        self.assertEqual(0.0, containment["leak_length_m"])
        self.assertEqual([], containment["leak_spans"])
        self.assertEqual(0.0, containment["edge_reach_fraction"])

    def test_the_playable_region_is_smaller_than_the_world(self) -> None:
        fraction = self.plan["playable"]["world_fraction"]
        self.assertGreater(fraction, 0.2)
        self.assertLess(fraction, 1.0)

    def test_the_mask_matches_the_declared_resolution(self) -> None:
        mask = np.fromfile(
            REAL_BATCH / "terrain/playable_mask.bin", dtype=np.uint8
        )
        side = self.plan["playable"]["mask_resolution"]
        self.assertEqual(side * side, mask.size)
        self.assertEqual({0, 1}, set(np.unique(mask).tolist()))

    def test_the_playable_area_agrees_with_the_mask(self) -> None:
        mask = np.fromfile(REAL_BATCH / "terrain/playable_mask.bin", dtype=np.uint8)
        cell = self.plan["playable"]["cell_m"]
        self.assertAlmostEqual(
            float(mask.sum()) * cell * cell, self.plan["playable"]["area_m2"], places=1
        )


if __name__ == "__main__":
    unittest.main()
