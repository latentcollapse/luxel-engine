"""Tests for the roadmap 1.4 navmesh acceptance gate.

"The negative case first: a gate that cannot fail is not a gate" -- the same
principle test_navigation_plan.py states for BlockedLaneTests applies here.
Every failure category gets a test proving the gate actually fires, and the
healthy case gets a test proving the gate can also pass -- a gate that always
fails is exactly as decorative as one that never does.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from navigation_plan import build  # noqa: E402
from navmesh_acceptance import evaluate  # noqa: E402

from test_navigation_plan import REAL_BATCH, _World, _box, _flat, _keep, _lane  # noqa: E402


def _biome(identifier: str, points: list[list[float]]) -> dict:
    return {
        "id": identifier,
        "category": "biome",
        "semantic": "forest",
        "geometry": {"type": "polygon", "points": points},
    }


class HandBuiltReportTests(unittest.TestCase):
    """Fast, isolated checks against a navigation_plan shape built by hand."""

    def _plan(self, **overrides) -> dict:
        base = {
            "lanes": [
                {
                    "id": "mid",
                    "runs_end_to_end": True,
                    "centreline_navigable_fraction": 1.0,
                    "longest_impassable_run_m": 0.0,
                }
            ],
            "topology": {
                "keeps_share_one_component": True,
                "primary_component": 1,
                "stranded_anchors": [],
                "unreachable_anchors": [],
            },
            "anchors": [
                {
                    "id": "forest_a",
                    "feature_id": "forest_a",
                    "category": "biome",
                    "component": 1,
                    "reachable": True,
                }
            ],
        }
        base.update(overrides)
        return base

    def test_a_fully_healthy_plan_passes(self) -> None:
        result = evaluate(self._plan())
        self.assertEqual("passed", result["status"])
        self.assertEqual([], result["failures"])

    def test_a_lane_that_does_not_run_end_to_end_fails_and_is_named(self) -> None:
        plan = self._plan(
            lanes=[
                {
                    "id": "north_lane",
                    "runs_end_to_end": False,
                    "centreline_navigable_fraction": 0.912,
                    "longest_impassable_run_m": 21.0,
                }
            ]
        )
        result = evaluate(plan)
        self.assertEqual("failed", result["status"])
        self.assertEqual(1, len(result["failures"]))
        self.assertIn("north_lane", result["failures"][0])
        self.assertIn("21.0", result["failures"][0])

    def test_keeps_not_sharing_a_component_fails(self) -> None:
        plan = self._plan(
            topology={
                "keeps_share_one_component": False,
                "primary_component": 1,
                "stranded_anchors": ["keep_b"],
                "unreachable_anchors": [],
            }
        )
        result = evaluate(plan)
        self.assertEqual("failed", result["status"])
        self.assertIn("connected component", result["failures"][0])

    def test_an_unreachable_biome_fails_and_is_named(self) -> None:
        plan = self._plan(
            topology={
                "keeps_share_one_component": True,
                "primary_component": 1,
                "stranded_anchors": [],
                "unreachable_anchors": ["forest_a"],
            },
            anchors=[
                {
                    "id": "forest_a",
                    "feature_id": "deep_forest",
                    "category": "biome",
                    "component": None,
                    "reachable": False,
                }
            ],
        )
        result = evaluate(plan)
        self.assertEqual("failed", result["status"])
        self.assertIn("deep_forest", result["failures"][0])
        self.assertIn("no navigable ground", result["failures"][0])

    def test_a_stranded_biome_fails_and_is_named(self) -> None:
        plan = self._plan(
            topology={
                "keeps_share_one_component": True,
                "primary_component": 1,
                "stranded_anchors": ["forest_a"],
                "unreachable_anchors": [],
            },
            anchors=[
                {
                    "id": "forest_a",
                    "feature_id": "island_forest",
                    "category": "biome",
                    "component": 2,
                    "reachable": True,
                }
            ],
        )
        result = evaluate(plan)
        self.assertEqual("failed", result["status"])
        self.assertIn("island_forest", result["failures"][0])
        self.assertIn("primary lane network", result["failures"][0])

    def test_a_stranded_non_biome_anchor_does_not_trip_this_gate(self) -> None:
        # A stranded landmark (a watchpost, say) is real information, but 1.4
        # is specifically lanes/keeps/biome -- it must not fail on an anchor
        # category it was not asked to gate, or a future stranded decoration
        # would block every build for an unrelated reason.
        plan = self._plan(
            topology={
                "keeps_share_one_component": True,
                "primary_component": 1,
                "stranded_anchors": ["watchpost"],
                "unreachable_anchors": [],
            },
            anchors=[
                {
                    "id": "watchpost",
                    "feature_id": "watchpost",
                    "category": "landmark",
                    "component": 2,
                    "reachable": True,
                }
            ],
        )
        result = evaluate(plan)
        self.assertEqual("passed", result["status"])


class SyntheticWorldTests(unittest.TestCase):
    """End to end through the real `navigation_plan.build`, not a hand-built shape."""

    def test_two_keeps_one_clear_lane_and_a_reachable_forest_passes(self) -> None:
        world = _World(
            _flat(),
            [
                _keep("a", -100.0, 0.0),
                _keep("b", 100.0, 0.0),
                _lane("mid", [[-90.0, 0.0], [90.0, 0.0]]),
                _biome("glade", [[-20.0, -20.0], [20.0, -20.0], [20.0, 20.0], [-20.0, 20.0]]),
            ],
            [],
        )
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        result = evaluate(plan)
        self.assertEqual("passed", result["status"], result["failures"])

    def test_a_building_dropped_on_the_lane_fails_the_gate(self) -> None:
        # The negative case: take the passing world above and break exactly
        # one thing. If this does not fail, the gate is decorative.
        world = _World(
            _flat(),
            [
                _keep("a", -100.0, 0.0),
                _keep("b", 100.0, 0.0),
                _lane("mid", [[-90.0, 0.0], [90.0, 0.0]]),
            ],
            [_box("hall", 0.0, 0.0, 12.0)],
        )
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        result = evaluate(plan)
        self.assertEqual("failed", result["status"])
        self.assertTrue(any("mid" in failure for failure in result["failures"]))

    def test_a_wall_separating_the_keeps_fails_the_gate(self) -> None:
        heights = _flat()
        heights[:, 129:] = 40.0
        world = _World(heights, [_keep("a", -80.0, 0.0), _keep("b", 80.0, 0.0)], [])
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        result = evaluate(plan)
        self.assertEqual("failed", result["status"])
        self.assertTrue(any("component" in failure for failure in result["failures"]))

    def test_an_isolated_forest_fails_the_gate(self) -> None:
        heights = _flat()
        heights[:, 129:] = 40.0  # a wall that only the forest sits behind
        world = _World(
            heights,
            [
                _keep("a", -80.0, 0.0),
                _biome("far_glade", [[70.0, -20.0], [90.0, -20.0], [90.0, 20.0], [70.0, 20.0]]),
            ],
            [],
        )
        self.addCleanup(world.close)
        plan, _, _ = build(world.path)
        result = evaluate(plan)
        self.assertEqual("failed", result["status"])
        self.assertTrue(any("far_glade" in failure for failure in result["failures"]))


class RealBatchTests(unittest.TestCase):
    """The current map, as actually compiled. Roadmap 1.4's own acceptance
    criterion: if this gate passes on the map as it stands, the gate is
    wrong. It must fail today, and must fail specifically on the lanes that
    are known broken (see Luxel/docs/archive/2026-09_roadmaps-and-audits/mvp-roadmap.md, roadmap 1.3 handoff)."""

    def setUp(self) -> None:
        if not (REAL_BATCH / "collision_plan.json").is_file():
            self.skipTest("compiled batch not present")

    def test_the_current_alpine_arena_fails_on_its_known_lane_gaps(self) -> None:
        plan, _, _ = build(REAL_BATCH)
        result = evaluate(plan)
        self.assertEqual("failed", result["status"])
        joined = " | ".join(result["failures"])
        for lane_id in ("north_lane", "central_lane", "south_lane"):
            self.assertIn(lane_id, joined, result["failures"])


if __name__ == "__main__":
    unittest.main()
