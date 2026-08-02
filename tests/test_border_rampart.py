"""Tests for the border rampart (roadmap 2.5 / systems S4).

The world used to simply stop at its data edge: 756 m of walkable edge across
seven spans, with the north and south edges open end to end. `boundary_plan` had
measured that for days without anything acting on it.

Each assertion below is paired with the case that breaks it, because a border
that cannot be shown to fail is indistinguishable from no border. Two of these
reconstruct defects found while building it:

- a linear inner face meets flat ground at a grade discontinuity, and the toe
  cells are walkable, so the terrain accessibility gate failed on the *border*
  (measured: p99 grade 1.69 -> 2.65)
- a global encroachment depth is set by whichever landmark sits nearest the
  edge, so one watchpost thinned the border on all four edges
"""

from __future__ import annotations

import math
import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from zone_compiler import (  # noqa: E402
    DEFAULT_BORDER_POLICY,
    ZoneCompileError,
    _validate_border_policy,
)
from zone_rasterizer import _border_rampart  # noqa: E402

WIDTH = LENGTH = 256.0
RESOLUTION = 257


def _grid():
    x_axis = np.linspace(-WIDTH * 0.5, WIDTH * 0.5, RESOLUTION, dtype=np.float64)
    z_axis = np.linspace(LENGTH * 0.5, -LENGTH * 0.5, RESOLUTION, dtype=np.float64)
    return np.meshgrid(x_axis, z_axis)


def _flat():
    return np.zeros((RESOLUTION, RESOLUTION), dtype=np.float64)


def _landmark(identifier: str, point, radius: float):
    return {
        "id": identifier,
        "category": "landmark",
        "geometry": {"points": [list(point)]},
        "properties": {"scatter_exclusion_radius_m": radius},
    }


def _raise(features=(), **overrides):
    policy = dict(DEFAULT_BORDER_POLICY)
    policy.update(overrides)
    x, z = _grid()
    return _border_rampart(
        _flat(), x, z, list(features), WIDTH, LENGTH, policy,
        np.random.default_rng(20260802),
    )


class RampartTest(unittest.TestCase):
    def test_the_edge_is_raised_and_the_middle_is_not(self):
        height, report, footprint = _raise()
        self.assertTrue(report["enabled"])
        centre = RESOLUTION // 2
        self.assertAlmostEqual(0.0, height[centre, centre], places=6)
        # Relief carves saddles down from `height_m`, so the rim varies between
        # the saddle floor and the authored height. What must hold everywhere is
        # that the edge is *raised* -- a saddle that reaches the valley floor is
        # a gap in the wall.
        floor = DEFAULT_BORDER_POLICY["height_m"] * (1.0 - DEFAULT_BORDER_POLICY["crest_relief"])
        edges = [height[0, :], height[-1, :], height[:, 0], height[:, -1]]
        for edge in edges:
            self.assertGreater(edge.min(), floor * 0.9)
        # Full height is reached somewhere on the rim, not on every edge --
        # broad noise does not peak on all four sides of one map.
        self.assertGreater(
            max(float(edge.max()) for edge in edges),
            DEFAULT_BORDER_POLICY["height_m"] * 0.85,
        )
        self.assertTrue(footprint.any())

    def test_a_disabled_border_changes_nothing(self):
        """The negative case for the whole feature."""
        height, report, footprint = _raise(enabled=False)
        self.assertFalse(report["enabled"])
        self.assertEqual(0.0, float(np.abs(height).max()))
        self.assertFalse(footprint.any())

    def test_the_inner_face_is_too_steep_to_stand_on(self):
        """The rampart's whole job. A face gentler than the agent's slope limit
        is scenery a player walks over, and the world stays open."""
        height, report, _ = _raise()
        self.assertGreater(report["peak_face_degrees"], 45.0)
        # Measured rather than trusted: the steepest rise along a row crossing
        # the west face must exceed a 45 degree grade at this cell size.
        cell = WIDTH / (RESOLUTION - 1)
        row = height[RESOLUTION // 2, : RESOLUTION // 2]
        self.assertGreater(np.abs(np.diff(row)).max() / cell, 1.0)

    def test_the_toe_meets_the_ground_tangentially(self):
        """A linear face fails the terrain accessibility gate at its own toe.

        The face is unwalkable by design, but the cells where it meets flat
        ground are walkable, and a grade discontinuity there is measured as
        walkable ground that happens to be a cliff. Smoothstep arrives flat.
        """
        height, _, footprint = _raise()
        row = height[RESOLUTION // 2, :]
        cell = WIDTH / (RESOLUTION - 1)
        slope = np.abs(np.diff(row)) / cell
        # Walk inward from the west edge to the first flat cell; the slope
        # approaching it must decay rather than drop off a step.
        inner = np.argmax(row[: RESOLUTION // 2] <= 0.01)
        self.assertGreater(inner, 2)
        # Measured on ground the rampart does *not* claim. Inside its own
        # footprint the face is meant to be a cliff; what must not happen is a
        # cliff cell being left in `background` for the accessibility gate to
        # measure as walkable.
        # The agent's own slope limit is the threshold that means anything here:
        # the cell where the rampart meets the ground has to be standable, or
        # the "toe" is just where the cliff happens to stop. A linear face gives
        # grade 2.63 at this cell (its constant height/face slope); smoothstep
        # gives 0.47.
        unprotected = ~footprint[RESOLUTION // 2, : RESOLUTION // 2 - 1]
        outside = np.where(unprotected)[0]
        self.assertTrue(outside.size > 0)
        self.assertLess(
            float(slope[: RESOLUTION // 2 - 1][unprotected].max()), 1.0,
            "steep ground left outside the rampart's own footprint",
        )

    def test_a_landmark_near_the_edge_pinches_only_its_own_corner(self):
        """The defect a global depth caused: one watchpost 28 m from the west
        edge thinned the border on all four edges. Depth is a field."""
        near_edge = _landmark("watchpost", (-99.84, 48.64), 16.0)
        _, report, _ = _raise([near_edge])
        self.assertEqual("watchpost", report["encroachment_limited_by"])
        self.assertLess(report["minimum_depth_m"], report["maximum_depth_m"])
        # The far edge keeps its full depth.
        self.assertAlmostEqual(
            DEFAULT_BORDER_POLICY["crest_m"] + DEFAULT_BORDER_POLICY["inner_face_m"],
            report["maximum_depth_m"],
            places=3,
        )

    def test_a_landmark_is_never_buried(self):
        """Closing the world is not worth burying a keep to do it."""
        keep = _landmark("keep", (-104.0, 0.0), 18.0)
        height, _, _ = _raise([keep])
        x, z = _grid()
        inside = np.hypot(x - (-104.0), z - 0.0) <= 18.0
        self.assertTrue(inside.any())
        self.assertLess(float(height[inside].max()), 1.0)

    def test_the_rampart_is_protected_relief_not_background(self):
        """Its footprint is intentional, unwalkable terrain.

        Left in `background` the accessibility gate measures the face as
        walkable ground and the build fails for closing the world -- measured:
        p99 grade 1.69 -> 7.15.
        """
        height, report, footprint = _raise()
        self.assertGreater(report["protected_area_m2"], 0.0)
        # Everything steep is inside the protected footprint.
        cell = WIDTH / (RESOLUTION - 1)
        gz, gx = np.gradient(height, cell)
        steep = np.hypot(gx, gz) > 1.0
        self.assertTrue(steep.any())
        self.assertFalse(
            bool((steep & ~footprint).any()),
            "%d steep cells left in background" % int((steep & ~footprint).sum()),
        )


class BorderPolicyTest(unittest.TestCase):
    def test_defaults_apply_when_unauthored(self):
        self.assertEqual(DEFAULT_BORDER_POLICY, _validate_border_policy(None))

    def test_unknown_fields_are_refused(self):
        with self.assertRaises(ZoneCompileError):
            _validate_border_policy({"height_metres": 40})

    def test_an_unsupported_profile_is_refused(self):
        with self.assertRaises(ZoneCompileError):
            _validate_border_policy({"profile": "invisible_wall"})

    def test_a_faceless_rampart_is_refused(self):
        """Zero face means zero slope for the boundary flood to stop against."""
        with self.assertRaises(ZoneCompileError):
            _validate_border_policy({"inner_face_m": 0.0})

    def test_a_disabled_border_may_omit_the_face(self):
        policy = _validate_border_policy({"enabled": False, "inner_face_m": 0.0})
        self.assertFalse(policy["enabled"])

    def test_the_ridge_profile_is_accepted_and_differs(self):
        policy = _validate_border_policy({"profile": "ridge"})
        self.assertEqual("ridge", policy["profile"])
        escarpment, _, _ = _raise()
        ridge, _, _ = _raise(profile="ridge")
        self.assertFalse(np.allclose(escarpment, ridge))


if __name__ == "__main__":
    unittest.main()
