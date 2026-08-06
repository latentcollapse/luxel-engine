"""Tests for the roadmap 1.5 collision acceptance gate.

Same standard as test_navmesh_acceptance.py: "the negative case first, a
gate that cannot fail is not a gate." The real-batch test proves the gate
passes on a healthy compiled world; the deliberately-broken tests prove it
actually fires -- delete a collider, move a placement off terrain, confirm
red, matching the exact repair Opus asked for on 1.4.
"""

from __future__ import annotations

import json
import math
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from reference_content import CONTENT_ROOT  # noqa: E402

from collision_acceptance import build, evaluate  # noqa: E402

REAL_BATCH = (
    CONTENT_ROOT / "concept_batches/codeweald_alpine_arena_v1"
)

RESOLUTION = 17
EXTENT_M = 64.0


def _flat_height_m(value: float = 0.0) -> np.ndarray:
    return np.full((RESOLUTION, RESOLUTION), value, dtype=np.float64)


def _render_instance(**overrides) -> dict:
    base = {
        "id": "hut:primary:0000",
        "feature_id": "hut",
        "role": "settlement_landmark",
        "position_m": [0.0, 0.0, 0.0],
        "yaw_degrees": 0.0,
        "scale": 1.0,
    }
    base.update(overrides)
    return base


def _collider(**overrides) -> dict:
    base = {
        "id": "hut:primary:0000",
        "feature_id": "hut",
        "role": "settlement_landmark",
        "shape": "box",
        "obstructs": True,
        "centre_m": [0.0, 0.0, 0.0],
        "half_extents_m": [2.0, 2.0, 2.0],
        "yaw_degrees": 0.0,
    }
    base.update(overrides)
    return base


class HandBuiltEvaluateTests(unittest.TestCase):
    """Fast, isolated checks against small synthetic artifacts."""

    def test_a_correctly_seated_collider_passes(self) -> None:
        collision_plan = {"instance_colliders": [_collider()]}
        render_plan = {"instances": [_render_instance()]}
        result = evaluate(collision_plan, render_plan, _flat_height_m(0.0), EXTENT_M, EXTENT_M)
        self.assertEqual("passed", result["status"])
        self.assertEqual([], result["failures"])

    def test_a_missing_collider_for_a_collidable_role_fails_and_is_named(self) -> None:
        # The negative case Opus asked for on 1.4, adapted here: delete a
        # collider for an instance whose role says it should have one.
        collision_plan = {"instance_colliders": []}
        render_plan = {"instances": [_render_instance()]}
        result = evaluate(collision_plan, render_plan, _flat_height_m(0.0), EXTENT_M, EXTENT_M)
        self.assertEqual("failed", result["status"])
        self.assertEqual(1, len(result["failures"]))
        self.assertIn("hut:primary:0000", result["failures"][0])
        self.assertIn("settlement_landmark", result["failures"][0])

    def test_a_non_collidable_role_needs_no_collider(self) -> None:
        collision_plan = {"instance_colliders": []}
        render_plan = {
            "instances": [_render_instance(role="highland_groundcover")]
        }
        result = evaluate(collision_plan, render_plan, _flat_height_m(0.0), EXTENT_M, EXTENT_M)
        self.assertEqual("passed", result["status"])

    def test_a_collider_floating_above_terrain_fails_and_is_named(self) -> None:
        collision_plan = {"instance_colliders": [_collider()]}
        render_plan = {"instances": [_render_instance(position_m=[0.0, 20.0, 0.0])]}
        result = evaluate(collision_plan, render_plan, _flat_height_m(0.0), EXTENT_M, EXTENT_M)
        self.assertEqual("failed", result["status"])
        self.assertIn("hut:primary:0000", result["failures"][0])
        self.assertIn("floats", result["failures"][0])

    def test_a_collider_buried_below_terrain_fails_and_is_named(self) -> None:
        collision_plan = {"instance_colliders": [_collider()]}
        render_plan = {"instances": [_render_instance(position_m=[0.0, -20.0, 0.0])]}
        result = evaluate(collision_plan, render_plan, _flat_height_m(0.0), EXTENT_M, EXTENT_M)
        self.assertEqual("failed", result["status"])
        self.assertIn("hut:primary:0000", result["failures"][0])
        self.assertIn("buried", result["failures"][0])

    def test_a_small_grounding_offset_is_within_tolerance(self) -> None:
        # render_plan.rs clamps grounding_offset_m to at most 0.35 m for any
        # collidable role here. A gate this tight would fail every healthy
        # placement.
        collision_plan = {"instance_colliders": [_collider()]}
        render_plan = {"instances": [_render_instance(position_m=[0.0, -0.3, 0.0])]}
        result = evaluate(collision_plan, render_plan, _flat_height_m(0.0), EXTENT_M, EXTENT_M)
        self.assertEqual("passed", result["status"])

    def test_a_deck_floating_over_a_gorge_is_exempt(self) -> None:
        # Decks span above lower ground by design -- see module docstring.
        # A bridge over a real gorge must not fail this check.
        collision_plan = {
            "instance_colliders": [
                _collider(
                    id="bridge:primary:0000",
                    role="lane_crossing_structure",
                    shape="deck",
                    centre_m=[0.0, 15.0, 0.0],
                )
            ]
        }
        render_plan = {
            "instances": [
                _render_instance(
                    id="bridge:primary:0000",
                    role="lane_crossing_structure",
                    position_m=[0.0, 15.0, 0.0],
                )
            ]
        }
        result = evaluate(collision_plan, render_plan, _flat_height_m(0.0), EXTENT_M, EXTENT_M)
        self.assertEqual("passed", result["status"])


class RealBatchTests(unittest.TestCase):
    """The current map, as actually compiled: the gate must pass on healthy,
    correctly-grounded placements. A gate that always fails is exactly as
    decorative as one that never does."""

    def setUp(self) -> None:
        if not (REAL_BATCH / "render_plan.json").is_file():
            self.skipTest("compiled batch not present")

    def test_the_current_alpine_arena_passes(self) -> None:
        result = build(REAL_BATCH)
        self.assertEqual("passed", result["status"], result["failures"])


class BrokenRealBatchTests(unittest.TestCase):
    """Deliberately corrupt a copy of the real, healthy, passing batch and
    confirm the gate goes red -- the actual negative-case verification, not
    just a synthetic unit test of the logic in isolation."""

    def setUp(self) -> None:
        if not (REAL_BATCH / "render_plan.json").is_file():
            self.skipTest("compiled batch not present")
        self._temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary.cleanup)
        self.batch = Path(self._temporary.name) / "batch"
        self.batch.mkdir()
        for name in ("collision_plan.json", "render_plan.json"):
            shutil.copy2(REAL_BATCH / name, self.batch / name)
        shutil.copytree(REAL_BATCH / "terrain", self.batch / "terrain")

    def test_healthy_copy_still_passes(self) -> None:
        # Confirms the copy itself is a faithful, healthy baseline before the
        # next two tests break exactly one thing in it.
        result = build(self.batch)
        self.assertEqual("passed", result["status"], result["failures"])

    def test_deleting_an_instances_colliders_turns_the_gate_red(self) -> None:
        # Every collider for one placement, not one collider: a composite asset
        # emits one per solid part (a village is five houses, a well and a
        # watchtower), so dropping a single part leaves the instance covered.
        # The gate's contract is per-instance.
        path = self.batch / "collision_plan.json"
        plan = json.loads(path.read_text(encoding="utf-8"))
        target = plan["instance_colliders"][0].get(
            "instance_id", plan["instance_colliders"][0]["id"]
        )
        plan["instance_colliders"] = [
            collider
            for collider in plan["instance_colliders"]
            if collider.get("instance_id", collider["id"]) != target
        ]
        path.write_text(json.dumps(plan), encoding="utf-8")
        result = build(self.batch)
        self.assertEqual("failed", result["status"])
        self.assertTrue(any(target in failure for failure in result["failures"]))

    def test_losing_one_part_of_a_composite_asset_is_not_detected(self) -> None:
        # Recorded, not asserted as desirable. The gate asks "does this
        # placement have collision", so a village missing one house's collider
        # passes. Closing that needs a per-part expectation the gate does not
        # have yet -- worth knowing before trusting this gate to catch a
        # regression in `asset_parts`.
        path = self.batch / "collision_plan.json"
        plan = json.loads(path.read_text(encoding="utf-8"))
        composite = [
            collider
            for collider in plan["instance_colliders"]
            if "#" in collider["id"]
        ]
        if not composite:
            self.skipTest("no composite assets in this batch")
        plan["instance_colliders"].remove(composite[0])
        path.write_text(json.dumps(plan), encoding="utf-8")
        self.assertEqual("passed", build(self.batch)["status"])

    def test_lifting_a_placement_off_terrain_turns_the_gate_red(self) -> None:
        path = self.batch / "render_plan.json"
        plan = json.loads(path.read_text(encoding="utf-8"))
        lifted = next(
            instance
            for instance in plan["instances"]
            if instance["role"] == "faction_fortification"
        )
        lifted["position_m"][1] += 25.0
        path.write_text(json.dumps(plan), encoding="utf-8")
        result = build(self.batch)
        self.assertEqual("failed", result["status"])
        self.assertTrue(
            any(lifted["id"] in failure and "floats" in failure for failure in result["failures"])
        )


class ProvenanceTests(unittest.TestCase):
    def setUp(self) -> None:
        if not (REAL_BATCH / "render_plan.json").is_file():
            self.skipTest("compiled batch not present")
        self._temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary.cleanup)
        self.batch = Path(self._temporary.name) / "batch"
        self.batch.mkdir()
        for name in ("collision_plan.json", "render_plan.json"):
            shutil.copy2(REAL_BATCH / name, self.batch / name)
        shutil.copytree(REAL_BATCH / "terrain", self.batch / "terrain")

    def test_a_heightfield_the_collision_plan_does_not_match_is_refused(self) -> None:
        heightfield = self.batch / "terrain/heightfield_f32le.bin"
        heightfield.write_bytes(heightfield.read_bytes() + b"\x00\x00\x00\x00")
        with self.assertRaises(Exception) as caught:
            build(self.batch)
        self.assertIn("different heightfield", str(caught.exception))

    def test_a_missing_artifact_is_named(self) -> None:
        (self.batch / "render_plan.json").unlink()
        with self.assertRaises(Exception) as caught:
            build(self.batch)
        self.assertIn("render_plan.json", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
