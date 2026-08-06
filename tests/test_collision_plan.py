from __future__ import annotations

import json
import math
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from reference_content import CONTENT_ROOT  # noqa: E402

from collision_plan import (  # noqa: E402
    ROLE_COLLISION,
    TRUNK_RADIUS_FRACTION,
    CollisionPlanError,
    _rotate_y,
    build,
    instance_collider,
)

REAL_BATCH = (
    CONTENT_ROOT
    / "concept_batches/codeweald_alpine_arena_v1"
)


def _instance(**overrides) -> dict:
    base = {
        "id": "feature:primary:0000",
        "feature_id": "feature",
        "role": "faction_fortification",
        "position_m": [0.0, 0.0, 0.0],
        "yaw_degrees": 0.0,
        "scale": 1.0,
    }
    base.update(overrides)
    return base


def _bounds(minimum: list[float], maximum: list[float]) -> dict:
    return {
        "min": minimum,
        "max": maximum,
        "size": [maximum[i] - minimum[i] for i in range(3)],
    }


class RotationConventionTests(unittest.TestCase):
    """The collider must sit where the mesh sits.

    The renderer places instances with Bevy's `Quat::from_rotation_y`, which is
    right-handed about +Y: +X rotates toward -Z. A collider built on the
    opposite handedness would be mirrored across the world and look plausible
    in aggregate while being wrong for every rotated instance.
    """

    def test_ninety_degrees_sends_positive_x_toward_negative_z(self) -> None:
        x, z = _rotate_y(1.0, 0.0, 90.0)
        self.assertAlmostEqual(0.0, x, places=6)
        self.assertAlmostEqual(-1.0, z, places=6)

    def test_zero_degrees_is_identity(self) -> None:
        self.assertEqual((3.0, -4.0), _rotate_y(3.0, -4.0, 0.0))

    def test_one_eighty_negates_both_axes(self) -> None:
        x, z = _rotate_y(2.0, 5.0, 180.0)
        self.assertAlmostEqual(-2.0, x, places=6)
        self.assertAlmostEqual(-5.0, z, places=6)

    def test_rotation_preserves_distance_from_origin(self) -> None:
        for degrees in (17.0, 90.0, 166.566784, 271.3):
            x, z = _rotate_y(3.0, 4.0, degrees)
            self.assertAlmostEqual(5.0, math.hypot(x, z), places=6)


class OffOriginGeometryTests(unittest.TestCase):
    """Asset bounds are not centred on the asset origin.

    `highland_fortified_keep_b.glb` spans z=0..184 in its own space, so its
    volume centre sits +92 local units from the origin the instance is placed
    at. Ignoring that would put every keep's collider ~12 m from its building
    at production scale -- present, plausible, and wrong.
    """

    def test_off_origin_centre_is_carried_into_world_space(self) -> None:
        collider = instance_collider(
            _instance(position_m=[10.0, 0.0, 20.0]),
            _bounds([-132.0, -132.0, 0.0], [132.0, 148.0, 184.0]),
        )
        # Local centre is (0, 8, 92); unrotated it just offsets +Z.
        self.assertAlmostEqual(10.0, collider["centre_m"][0], places=4)
        self.assertAlmostEqual(8.0, collider["centre_m"][1], places=4)
        self.assertAlmostEqual(112.0, collider["centre_m"][2], places=4)

    def test_off_origin_centre_rotates_with_yaw(self) -> None:
        collider = instance_collider(
            _instance(position_m=[0.0, 0.0, 0.0], yaw_degrees=90.0),
            _bounds([-132.0, -132.0, 0.0], [132.0, 148.0, 184.0]),
        )
        # +92 on Z, rotated 90 degrees about Y, lands on +X.
        self.assertAlmostEqual(92.0, collider["centre_m"][0], places=4)
        self.assertAlmostEqual(0.0, collider["centre_m"][2], places=4)

    def test_scale_applies_to_centre_and_extents_together(self) -> None:
        collider = instance_collider(
            _instance(scale=0.1306),
            _bounds([-132.0, -132.0, 0.0], [132.0, 148.0, 184.0]),
        )
        self.assertAlmostEqual(92.0 * 0.1306, collider["centre_m"][2], places=4)
        self.assertAlmostEqual(132.0 * 0.1306, collider["half_extents_m"][0], places=4)

    def test_a_centred_asset_needs_no_offset(self) -> None:
        collider = instance_collider(
            _instance(position_m=[5.0, 1.0, -3.0]),
            _bounds([-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]),
        )
        self.assertEqual([5.0, 1.0, -3.0], collider["centre_m"])


class OrientedBoxTests(unittest.TestCase):
    """Half-extents stay in asset space; yaw is carried separately.

    Recomputing a world-space AABB would inflate a rotated box -- a 40 m keep
    at 45 degrees would claim a 57 m footprint and block ground beside itself.
    """

    def test_half_extents_do_not_grow_with_rotation(self) -> None:
        straight = instance_collider(
            _instance(yaw_degrees=0.0), _bounds([-20.0, -5.0, -10.0], [20.0, 5.0, 10.0])
        )
        turned = instance_collider(
            _instance(yaw_degrees=45.0), _bounds([-20.0, -5.0, -10.0], [20.0, 5.0, 10.0])
        )
        self.assertEqual(straight["half_extents_m"], turned["half_extents_m"])

    def test_yaw_is_recorded_on_the_collider(self) -> None:
        collider = instance_collider(
            _instance(yaw_degrees=166.566784),
            _bounds([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]),
        )
        self.assertAlmostEqual(166.566784, collider["yaw_degrees"], places=5)


class RolePolicyTests(unittest.TestCase):
    """Which roles collide is a declared decision, not an inference."""

    def test_groundcover_does_not_collide(self) -> None:
        # 160 groundcover/understory instances stand in the lanes. Colliding
        # grass would make the map unwalkable while every render looked right.
        for role in ("highland_groundcover", "highland_understory"):
            collider = instance_collider(
                _instance(role=role), _bounds([-1.0, 0.0, -1.0], [1.0, 2.0, 1.0])
            )
            self.assertIsNone(collider, role)

    def test_structures_collide_as_boxes(self) -> None:
        for role in (
            "faction_fortification",
            "settlement_landmark",
            "objective_landmark",
            "forest_floor_rock",
        ):
            collider = instance_collider(
                _instance(role=role), _bounds([-1.0, 0.0, -1.0], [1.0, 2.0, 1.0])
            )
            self.assertEqual("box", collider["shape"], role)
            self.assertTrue(collider["obstructs"], role)

    def test_a_bridge_carries_traffic_rather_than_stopping_it(self) -> None:
        # A crossing sits *on* the lane centreline by design, so a solid box
        # makes it a wall across the route it exists to carry. Measured before
        # this changed: all 20 crossings obstructed their own lane and no lane
        # ran end to end.
        collider = instance_collider(
            _instance(role="lane_crossing_structure"),
            _bounds([-1.0, 0.0, -1.0], [1.0, 2.0, 1.0]),
        )
        self.assertEqual("deck", collider["shape"])
        self.assertFalse(collider["obstructs"])
        # Still geometry, unlike `none` -- a backend needs its extent to build
        # a walkable deck for an elevated crossing.
        self.assertEqual(3, len(collider["half_extents_m"]))

    def test_a_tree_collides_as_a_trunk_not_a_canopy(self) -> None:
        collider = instance_collider(
            _instance(role="conifer_canopy"),
            _bounds([-8.0, 0.0, -8.0], [8.0, 20.0, 8.0]),
        )
        self.assertEqual("cylinder", collider["shape"])
        self.assertAlmostEqual(8.0 * TRUNK_RADIUS_FRACTION, collider["radius_m"], places=5)
        # A canopy-radius collider would block ground a player walks under.
        self.assertLess(collider["radius_m"], 8.0)

    def test_an_undeclared_role_is_refused_rather_than_defaulted(self) -> None:
        # Defaulting would silently give a new role either phantom collision or
        # none at all, and nothing downstream would report it.
        with self.assertRaises(CollisionPlanError) as caught:
            instance_collider(
                _instance(role="siege_engine"),
                _bounds([-1.0, 0.0, -1.0], [1.0, 2.0, 1.0]),
            )
        self.assertIn("siege_engine", str(caught.exception))

    def test_every_declared_policy_is_a_known_shape(self) -> None:
        self.assertTrue(
            set(ROLE_COLLISION.values()) <= {"box", "trunk_cylinder", "deck", "none"}
        )


class ProvenanceTests(unittest.TestCase):
    """A collision plan that outlived its terrain is worse than none.

    It would put invisible walls where the world no longer has any, and every
    render would still look correct.
    """

    def setUp(self) -> None:
        if not (REAL_BATCH / "render_plan.json").is_file():
            self.skipTest("compiled batch not present")
        self._temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary.cleanup)
        self.batch = Path(self._temporary.name) / "batch"
        self.batch.mkdir()
        for name in ("zone_spec.json", "render_plan.json", "asset_plan.json"):
            shutil.copy2(REAL_BATCH / name, self.batch / name)
        shutil.copytree(
            REAL_BATCH / "asset_visual_preflight",
            self.batch / "asset_visual_preflight",
        )
        shutil.copytree(REAL_BATCH / "terrain", self.batch / "terrain")

    def test_a_healthy_batch_builds(self) -> None:
        plan = build(self.batch)
        self.assertEqual("heightfield", plan["terrain"]["shape"])
        self.assertGreater(plan["instance_collider_count"], 0)

    def test_the_plan_is_bound_to_every_input(self) -> None:
        plan = build(self.batch)
        for key in (
            "zone_spec_bytes_sha256",
            "zone_spec_canonical_sha256",
            "heightfield_sha256",
            "terrain_manifest_bytes_sha256",
            "asset_plan_bytes_sha256",
            "render_plan_sha256",
        ):
            self.assertTrue(plan.get(key), key)

    def test_a_heightfield_the_manifest_does_not_match_is_refused(self) -> None:
        heightfield = self.batch / "terrain/heightfield_f32le.bin"
        heightfield.write_bytes(heightfield.read_bytes() + b"\x00\x00\x00\x00")
        with self.assertRaises(CollisionPlanError) as caught:
            build(self.batch)
        self.assertIn("manifest", str(caught.exception))

    def test_a_render_plan_from_a_different_terrain_is_refused(self) -> None:
        plan_path = self.batch / "render_plan.json"
        document = json.loads(plan_path.read_text(encoding="utf-8"))
        document["heightfield_sha256"] = "0" * 64
        plan_path.write_text(json.dumps(document), encoding="utf-8")
        with self.assertRaises(CollisionPlanError) as caught:
            build(self.batch)
        self.assertIn("different heightfield", str(caught.exception))

    def test_a_missing_artifact_is_named(self) -> None:
        (self.batch / "render_plan.json").unlink()
        with self.assertRaises(CollisionPlanError) as caught:
            build(self.batch)
        self.assertIn("render_plan.json", str(caught.exception))

    def test_terrain_built_from_another_zone_spec_is_refused(self) -> None:
        # Colliders derived from this zone against another world's terrain
        # would not match what a player actually walks into.
        manifest_path = self.batch / "terrain/terrain_manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["zone_spec_sha256"] = "0" * 64
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaises(CollisionPlanError) as caught:
            build(self.batch)
        self.assertIn("different zone spec", str(caught.exception))

    def test_asset_plan_and_terrain_manifest_digests_are_named_as_bytes(self) -> None:
        # T7 / D10: asset_physical_acceptance.py and world_core/render_plan.rs
        # both write "asset_plan_sha256"/"terrain_manifest_sha256" as a
        # canonical-JSON hash, in different artifacts, for their own
        # self-contained contracts. This plan hashes raw file bytes instead --
        # a different value under what would have been the same bare key name.
        # Naming it explicitly here is what stops a future consumer treating
        # the two as comparable.
        import hashlib

        plan = build(self.batch)
        asset_plan_bytes = (self.batch / "asset_plan.json").read_bytes()
        asset_plan_canonical = json.dumps(
            json.loads(asset_plan_bytes),
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        ).encode("utf-8")
        self.assertEqual(
            plan["asset_plan_bytes_sha256"],
            hashlib.sha256(asset_plan_bytes).hexdigest(),
        )
        self.assertNotEqual(
            plan["asset_plan_bytes_sha256"],
            hashlib.sha256(asset_plan_canonical).hexdigest(),
        )

    def test_the_plan_records_both_zone_spec_digests(self) -> None:
        # `zone_spec_sha256` means the canonical JSON in the terrain manifest
        # and the file bytes in build_report.json (T7 / D10). Naming both
        # spellings is what lets a consumer compare against either without
        # guessing -- and a naive consumer comparing the two against each
        # other, expecting one identity, must see them differ.
        plan = build(self.batch)
        self.assertNotEqual(
            plan["zone_spec_bytes_sha256"], plan["zone_spec_canonical_sha256"]
        )


class CompiledBatchTests(unittest.TestCase):
    """The real world, as actually compiled."""

    def setUp(self) -> None:
        path = REAL_BATCH / "collision_plan.json"
        if not path.is_file():
            self.skipTest("collision plan not built")
        self.plan = json.loads(path.read_text(encoding="utf-8"))

    def test_terrain_references_the_heightfield_rather_than_baking_a_mesh(self) -> None:
        terrain = self.plan["terrain"]
        self.assertEqual("heightfield", terrain["shape"])
        self.assertEqual("terrain/heightfield_f32le.bin", terrain["source"])
        self.assertTrue(terrain["resolution"])

    def test_collider_and_skipped_counts_account_for_every_instance(self) -> None:
        render_plan = json.loads(
            (REAL_BATCH / "render_plan.json").read_text(encoding="utf-8")
        )
        # Colliders no longer map one-to-one onto instances: a composite asset
        # emits one per solid part. Every instance must still be accounted for
        # as either colliding or deliberately not.
        skipped = sum(self.plan["non_colliding_by_role"].values())
        covered = {
            collider.get("instance_id", collider["id"])
            for collider in self.plan["instance_colliders"]
        }
        self.assertEqual(len(render_plan["instances"]), len(covered) + skipped)
        self.assertGreaterEqual(self.plan["instance_collider_count"], len(covered))

    def test_no_collider_escapes_the_world_bounds(self) -> None:
        bounds = self.plan["terrain"]["world_bounds_m"]
        half_width = bounds["width"] * 0.5 + 50.0
        half_length = bounds["length"] * 0.5 + 50.0
        for collider in self.plan["instance_colliders"]:
            x, _, z = collider["centre_m"]
            self.assertLess(abs(x), half_width, collider["id"])
            self.assertLess(abs(z), half_length, collider["id"])

    def test_no_collider_is_degenerate(self) -> None:
        # A zero-extent collider is a hole in the world that nothing reports.
        for collider in self.plan["instance_colliders"]:
            if collider["shape"] in ("box", "deck"):
                self.assertGreater(min(collider["half_extents_m"]), 0.0, collider["id"])
            else:
                self.assertGreater(collider["radius_m"], 0.0, collider["id"])
                self.assertGreater(collider["half_height_m"], 0.0, collider["id"])


if __name__ == "__main__":
    unittest.main()
