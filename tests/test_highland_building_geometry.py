"""Tests for `highland_building_geometry.py`, the pure half of the
individual-building kit generator (roadmap "split village clusters",
closing D3's "untested Blender generators" for this new module before it
ships, not after).

"The negative case first: a test that cannot fail is not a test." The
roof/wall pivot test proves both directions: the fixed convention keeps the
roof over its footprint under any rotation, and a hand-reconstruction of the
*actual historical bug* (roof world offset baked into vertex data while its
pivot stays at the origin -- see `generate_highland_settlement_kit._roof`'s
docstring and roadmap D3) reliably fails the same assertion. If the negative
case did not fail, the positive case would not be proving anything.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from asset_parts import _group_of  # noqa: E402
from highland_building_geometry import (  # noqa: E402
    COTTAGE_VARIANTS,
    footprint_corners,
    historically_buggy_footprint_and_roof_centroid,
    point_in_convex_polygon,
    rigidly_placed_footprint_and_roof_centroid,
    roof_local_vertices,
    rotate_xz,
    watchtower_total_height_m,
    well_total_height_m,
)


class RotationConventionTests(unittest.TestCase):
    """The roof must sit on the walls, including after a rotation."""

    def test_the_historically_buggy_pattern_actually_fails(self) -> None:
        # Negative case first. If this assertion could not fail, the positive
        # test below would not be proving the fix does anything.
        spec = COTTAGE_VARIANTS[0]
        world_xz = (40.0, -25.0)  # far from the origin, where the bug bites
        for yaw in (30.0, 90.0, 145.0, 210.0):
            footprint, centroid = historically_buggy_footprint_and_roof_centroid(
                spec, yaw, world_xz
            )
            self.assertFalse(
                point_in_convex_polygon(centroid, footprint),
                "expected the reconstructed historical bug to swing the roof "
                "off its footprint at yaw=%.1f, but it stayed on" % yaw,
            )

    def test_rotated_building_roof_centroid_stays_within_its_footprint(self) -> None:
        for spec in COTTAGE_VARIANTS:
            for yaw in (0.0, 30.0, 90.0, 145.0, 210.0, 271.0, 359.0):
                for world_xz in ((0.0, 0.0), (18.0, -6.0), (-52.0, 71.0)):
                    footprint, centroid = rigidly_placed_footprint_and_roof_centroid(
                        spec, yaw, world_xz
                    )
                    self.assertTrue(
                        point_in_convex_polygon(centroid, footprint),
                        "cottage %s roof centroid %r left its footprint %r at "
                        "yaw=%.1f, world=%r" % (spec.key, centroid, footprint, yaw, world_xz),
                    )

    def test_roof_centroid_is_independent_of_world_offset_when_correctly_authored(
        self,
    ) -> None:
        # A rigidly-parented roof and wall keep the same *relative* position
        # under translation -- moving the whole building must not change
        # where, proportionally, the roof centroid sits over the footprint.
        spec = COTTAGE_VARIANTS[1]
        _footprint_a, centroid_a = rigidly_placed_footprint_and_roof_centroid(
            spec, 0.0, (0.0, 0.0)
        )
        _footprint_b, centroid_b = rigidly_placed_footprint_and_roof_centroid(
            spec, 0.0, (500.0, -300.0)
        )
        self.assertAlmostEqual(centroid_b[0] - centroid_a[0], 500.0, places=6)
        self.assertAlmostEqual(centroid_b[1] - centroid_a[1], -300.0, places=6)


class RoofVertexGeometryTests(unittest.TestCase):
    def test_roof_base_vertices_sit_at_the_eave_height(self) -> None:
        vertices = roof_local_vertices(8.0, 7.0, base_height_m=3.0, ridge_height_m=5.5)
        base_vertices = [v for v in vertices if v[1] == 3.0]
        ridge_vertices = [v for v in vertices if v[1] == 5.5]
        self.assertEqual(4, len(base_vertices))
        self.assertEqual(2, len(ridge_vertices))

    def test_roof_footprint_is_centred_on_the_building_origin(self) -> None:
        vertices = roof_local_vertices(8.0, 7.0, base_height_m=3.0, ridge_height_m=5.5)
        xs = [v[0] for v in vertices]
        zs = [v[2] for v in vertices]
        self.assertAlmostEqual(0.0, (min(xs) + max(xs)) / 2.0)
        self.assertAlmostEqual(0.0, (min(zs) + max(zs)) / 2.0)

    def test_ridge_must_sit_above_the_eave(self) -> None:
        for spec in COTTAGE_VARIANTS:
            self.assertGreater(
                spec.ridge_height_m,
                spec.base_height_m,
                "cottage %s has a roof pitch that does not rise" % spec.key,
            )


class RotateXZTests(unittest.TestCase):
    def test_ninety_degrees_swaps_axes(self) -> None:
        x, z = rotate_xz((1.0, 0.0), 90.0)
        self.assertAlmostEqual(0.0, x, places=6)
        self.assertAlmostEqual(1.0, z, places=6)

    def test_full_turn_is_identity(self) -> None:
        x, z = rotate_xz((3.0, -4.0), 360.0)
        self.assertAlmostEqual(3.0, x, places=6)
        self.assertAlmostEqual(-4.0, z, places=6)


class PointInConvexPolygonTests(unittest.TestCase):
    def test_centre_is_inside(self) -> None:
        square = footprint_corners(4.0, 4.0)
        self.assertTrue(point_in_convex_polygon((0.0, 0.0), square))

    def test_far_outside_is_outside(self) -> None:
        square = footprint_corners(4.0, 4.0)
        self.assertFalse(point_in_convex_polygon((100.0, 0.0), square))

    def test_works_regardless_of_winding_direction(self) -> None:
        square_ccw = footprint_corners(4.0, 4.0)
        square_cw = list(reversed(square_ccw))
        self.assertTrue(point_in_convex_polygon((0.5, 0.5), square_ccw))
        self.assertTrue(point_in_convex_polygon((0.5, 0.5), square_cw))


class CottageVariantTests(unittest.TestCase):
    """Matt's art-direction checklist: at least 5 distinct variants."""

    def test_at_least_five_variants(self) -> None:
        self.assertGreaterEqual(len(COTTAGE_VARIANTS), 5)

    def test_variant_keys_are_unique(self) -> None:
        keys = [spec.key for spec in COTTAGE_VARIANTS]
        self.assertEqual(len(keys), len(set(keys)))

    def test_footprints_are_distinct(self) -> None:
        footprints = {(spec.width_m, spec.depth_m) for spec in COTTAGE_VARIANTS}
        self.assertEqual(len(footprints), len(COTTAGE_VARIANTS))

    def test_not_all_variants_share_one_roof_pitch(self) -> None:
        pitches = {spec.roof_pitch_m for spec in COTTAGE_VARIANTS}
        self.assertGreater(len(pitches), 1)

    def test_both_roof_materials_are_represented(self) -> None:
        materials = {spec.roof_material for spec in COTTAGE_VARIANTS}
        self.assertEqual({"slate", "thatch"}, materials)

    def test_storeys_vary(self) -> None:
        storeys = {spec.storeys for spec in COTTAGE_VARIANTS}
        self.assertGreater(len(storeys), 1)

    def test_door_sides_vary(self) -> None:
        sides = {spec.door_side for spec in COTTAGE_VARIANTS}
        self.assertGreater(len(sides), 1)

    def test_chimney_placement_varies(self) -> None:
        placements = {spec.chimney_offsets for spec in COTTAGE_VARIANTS}
        self.assertGreater(len(placements), 1)

    def test_no_two_variants_are_identical_on_every_axis(self) -> None:
        seen = set()
        for spec in COTTAGE_VARIANTS:
            fingerprint = (
                spec.width_m,
                spec.depth_m,
                spec.roof_pitch_m,
                spec.roof_material,
                spec.storeys,
                spec.door_side,
                spec.chimney_offsets,
            )
            self.assertNotIn(fingerprint, seen)
            seen.add(fingerprint)


class NodeNamingGroupsCorrectlyTests(unittest.TestCase):
    """Every mesh node is `<Building>_<part>`; `asset_parts` must group each
    building's parts under one prefix, never split one building into two
    fake buildings or merge two buildings into one."""

    def test_cottage_parts_group_under_the_cottage(self) -> None:
        for spec in COTTAGE_VARIANTS:
            expected_group = "Cottage_%s" % spec.key
            nodes = [
                "Cottage_%s_foundation" % spec.key,
                "Cottage_%s_plaster" % spec.key,
                "Cottage_%s_%s" % (spec.key, spec.roof_suffix),
                "Cottage_%s_door" % spec.key,
                "Cottage_%s_window_-1" % spec.key,
                "Cottage_%s_window_1" % spec.key,
            ]
            for index in range(len(spec.chimney_offsets)):
                suffix = "_chimney" if len(spec.chimney_offsets) == 1 else "_chimney_%d" % index
                nodes.append("Cottage_%s%s" % (spec.key, suffix))
            for node in nodes:
                self.assertEqual(
                    expected_group,
                    _group_of(node),
                    "%s grouped as %r, expected %r" % (node, _group_of(node), expected_group),
                )

    def test_cottages_do_not_group_with_each_other(self) -> None:
        groups = {_group_of("Cottage_%s_door" % spec.key) for spec in COTTAGE_VARIANTS}
        self.assertEqual(len(groups), len(COTTAGE_VARIANTS))

    def test_well_parts_group_under_the_well(self) -> None:
        for node in ("Well_stone", "Well_timber_-1", "Well_timber_1", "Well_roof"):
            self.assertEqual("Well", _group_of(node))

    def test_watchtower_parts_group_under_the_watchtower(self) -> None:
        nodes = [
            "Watchtower_stone",
            "Watchtower_timber",
            "Watchtower_slate_roof",
        ] + ["Watchtower_merlon_%d" % index for index in range(8)]
        for node in nodes:
            self.assertEqual("Watchtower", _group_of(node))

    def test_thatch_roof_groups_with_its_cottage_not_as_its_own_part(self) -> None:
        # Before `thatch_roof` was added to `asset_parts._GROUP_PATTERN`,
        # this degraded to a standalone group named `Cottage_B_thatch_roof`
        # instead of `Cottage_B` -- the same failure mode `slate_roof` was
        # already special-cased for.
        self.assertEqual("Cottage_B", _group_of("Cottage_B_thatch_roof"))


class StandaloneAssetHeightTests(unittest.TestCase):
    """Sanity bounds so a generation-time regression is caught before Blender
    ever runs: these feed `asset_physical_acceptance`'s role limits."""

    def test_watchtower_height_is_plausible(self) -> None:
        height = watchtower_total_height_m()
        self.assertGreater(height, 5.0)
        self.assertLess(height, 25.0)

    def test_well_height_is_plausible(self) -> None:
        height = well_total_height_m()
        self.assertGreater(height, 0.5)
        self.assertLess(height, 4.0)


if __name__ == "__main__":
    unittest.main()
