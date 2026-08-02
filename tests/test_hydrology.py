"""Tests for the hydrology solver (systems roadmap S2).

The defect this replaces: eight authored `wetland_rill` centrelines, all 2 m
wide, carving nothing -- blue splatmap patches on flat ground with bridges over
them (D20). Water was decoration, and decoration is not a network.

Each case is paired with its negative, and two reconstruct defects found while
building it:

- closing the world made it a basin with no outlet, holding 46,157 m2 of
  standing water
- the first outlet was a disc centred on the rim cell, which lowered the
  outside of a 28 m wall and left the inside intact
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from hydrology import (  # noqa: E402
    BOG_CATCHMENT,
    carve_channels,
    channel_depth_field,
    carve_outlet,
    choose_outlet,
    classify,
    fill_depressions,
    flow_accumulation,
)

SIDE = 65
CELL_M = 1.0


def _basin(rim: float = 20.0, floor: float = 0.0) -> np.ndarray:
    """A world shaped exactly like the one S4 produces: a bowl with a rim."""
    axis = np.linspace(-1.0, 1.0, SIDE)
    x, z = np.meshgrid(axis, axis)
    edge = np.maximum(np.abs(x), np.abs(z))
    wall = np.clip((edge - 0.72) / 0.28, 0.0, 1.0)
    return floor + (rim - floor) * wall * wall * (3.0 - 2.0 * wall)


class FillTest(unittest.TestCase):
    def test_a_basin_fills_to_its_rim(self):
        """The closed-world case: nothing drains, so everything ponds."""
        height = _basin()
        filled = fill_depressions(height)
        depth = filled - height
        self.assertGreater(depth.max(), 10.0)
        self.assertGreater(float((depth > 0.35).mean()), 0.4)

    def test_a_tilted_plane_holds_no_water(self):
        """The negative case: ground that drains has no sink at all."""
        axis = np.linspace(0.0, 20.0, SIDE)
        height = np.tile(axis, (SIDE, 1))
        filled = fill_depressions(height)
        self.assertLess(float(np.abs(filled - height).max()), 1e-9)

    def test_accumulation_concentrates_downhill(self):
        axis = np.linspace(20.0, 0.0, SIDE)
        height = np.tile(axis[:, None], (1, SIDE))
        accumulation = flow_accumulation(fill_depressions(height))
        self.assertGreater(accumulation[-1, :].mean(), accumulation[0, :].mean())


class OutletTest(unittest.TestCase):
    def test_an_outlet_is_chosen_on_the_rim(self):
        height = _basin()
        accumulation = flow_accumulation(fill_depressions(height))
        outlet = choose_outlet(height, accumulation)
        self.assertIn(outlet.edge, {"north", "south", "east", "west"})
        on_edge = outlet.row in (0, SIDE - 1) or outlet.column in (0, SIDE - 1)
        self.assertTrue(on_edge)

    def test_carving_the_outlet_drains_the_basin(self):
        """The whole point. Before: a closed world holds its rainfall."""
        height = _basin()
        before = float((fill_depressions(height) - height > 0.35).sum())
        accumulation = flow_accumulation(fill_depressions(height))
        outlet = choose_outlet(height, accumulation)
        drained = carve_outlet(
            height, outlet.row, outlet.column, cell_m=CELL_M, reach_m=26.0
        )
        after = float((fill_depressions(drained) - drained > 0.35).sum())
        self.assertLess(after, before * 0.5, "the gorge did not drain the basin")

    def test_a_dimple_at_the_rim_does_not_drain_it(self):
        """The defect the first version had.

        A notch that does not reach inward past the wall lowers the outside of
        the rim and leaves the basin intact -- measured on the real world as
        46,157 m2 of standing water still held after 'carving' an outlet.
        """
        height = _basin()
        accumulation = flow_accumulation(fill_depressions(height))
        outlet = choose_outlet(height, accumulation)
        before = float((fill_depressions(height) - height > 0.35).sum())
        shallow = carve_outlet(
            height, outlet.row, outlet.column, cell_m=CELL_M, reach_m=2.0
        )
        after = float((fill_depressions(shallow) - shallow > 0.35).sum())
        self.assertGreater(after, before * 0.5, "a dimple should not drain a basin")

    def test_the_gorge_leaves_walls_standing(self):
        """Open to water, shut to a body: the rim either side must survive."""
        height = _basin()
        accumulation = flow_accumulation(fill_depressions(height))
        outlet = choose_outlet(height, accumulation)
        drained = carve_outlet(
            height, outlet.row, outlet.column, cell_m=CELL_M, reach_m=26.0
        )
        # The rim as a whole is still high even though one line through it is not.
        rim = np.concatenate(
            [drained[0, :], drained[-1, :], drained[:, 0], drained[:, -1]]
        )
        self.assertGreater(float(np.median(rim)), 10.0)


class ChannelTest(unittest.TestCase):
    def _accumulated(self):
        rows = np.linspace(40.0, 10.0, SIDE)[:, None]
        columns = np.abs(np.arange(SIDE) - SIDE // 2)[None, :] * 0.55
        height = rows + columns
        return height, flow_accumulation(fill_depressions(height))

    def test_depth_scales_with_the_logarithm_of_catchment(self):
        """A reach draining ten times the ground is about twice as deep, not
        ten times. A linear rule gives a scratch everywhere and one canyon."""
        _, accumulation = self._accumulated()
        depth = channel_depth_field(accumulation, cell_m=CELL_M)
        deep = float(depth.max())
        self.assertGreater(deep, 0.0)
        # The deepest reach drains far more than ten times the shallowest cut,
        # yet is nothing like ten times deeper.
        cut = depth[depth > 0.01]
        self.assertLess(deep / float(cut.min()), 10.0)

    def test_nothing_is_carved_below_the_channel_threshold(self):
        """Every hillside has flow; only some of it is a stream."""
        flat = np.ones((SIDE, SIDE))
        depth = channel_depth_field(flat * 2.0, cell_m=CELL_M)
        self.assertEqual(0.0, float(depth.max()))

    def test_the_bed_stays_within_the_agents_climb(self):
        """The declared direction is rivers shallow enough to walk, with
        bridges an aesthetic choice. A channel needing a bridge broke the map."""
        _, accumulation = self._accumulated()
        depth = channel_depth_field(accumulation, cell_m=CELL_M)
        self.assertLess(float(depth.max()), 4.0)

    def test_banks_are_smoothed_rather_than_trenched(self):
        """A one-cell trench is invisible at distance and a grade discontinuity
        the accessibility gate correctly objects to."""
        height = np.zeros((SIDE, SIDE))
        depth = np.zeros((SIDE, SIDE))
        depth[:, SIDE // 2] = 1.0
        carved = carve_channels(height, depth)
        centre = abs(float(carved[SIDE // 2, SIDE // 2]))
        shoulder = abs(float(carved[SIDE // 2, SIDE // 2 + 1]))
        self.assertGreater(centre, 0.0)
        self.assertGreater(shoulder, 0.0, "the bank is a vertical wall")
        self.assertLess(shoulder, centre)


class ClassificationTest(unittest.TestCase):
    def _bodies(self, height):
        filled = fill_depressions(height)
        accumulation = flow_accumulation(filled)
        return classify(height, filled, accumulation, cell_m=CELL_M)

    def test_an_isolated_hollow_is_a_bog(self):
        """No catchment but its own rain: stagnant, green, shallow-looking."""
        height = np.full((SIDE, SIDE), 10.0)
        height[30:36, 30:36] = 8.0
        report = self._bodies(height)
        self.assertEqual(1, report["body_count"])
        body = report["bodies"][0]
        self.assertEqual("bog", body["kind"])
        self.assertEqual("stagnant_green", body["surface"])
        self.assertFalse(body["fed"])

    def test_a_hollow_at_the_foot_of_a_slope_is_a_fed_pond(self):
        """Same hollow, given something draining into it, becomes a pond.

        This is the distinction the art direction asked for -- greenish bog
        versus darker fed water -- and it is read off the drainage graph rather
        than authored anywhere.
        """
        # A converging valley, not a set of parallel columns. Flow has to
        # concentrate for a catchment to exist at all -- on a plane of equal
        # columns each cell drains straight down its own, and the hollow at the
        # bottom sees one column's worth of water, which is a puddle.
        rows = np.linspace(40.0, 10.0, SIDE)[:, None]
        columns = np.abs(np.arange(SIDE) - SIDE // 2)[None, :] * 0.55
        height = rows + columns
        height[-9:-3, SIDE // 2 - 4 : SIDE // 2 + 5] = 4.0
        report = self._bodies(height)
        self.assertGreaterEqual(report["pond_count"], 1)
        pond = next(b for b in report["bodies"] if b["kind"] == "pond")
        self.assertTrue(pond["fed"])
        self.assertEqual("cold_blue", pond["surface"])
        self.assertGreaterEqual(pond["inflow_cells"], BOG_CATCHMENT)

    def test_flat_ground_has_no_water(self):
        report = self._bodies(np.full((SIDE, SIDE), 5.0))
        self.assertEqual(0, report["body_count"])

    def test_depth_decides_swimmable(self):
        height = np.full((SIDE, SIDE), 10.0)
        height[20:26, 20:26] = 9.4   # ankle deep
        height[40:46, 40:46] = 6.0   # over your head
        report = self._bodies(height)
        kinds = {b["swimmable"] for b in report["bodies"]}
        self.assertEqual({True, False}, kinds)


if __name__ == "__main__":
    unittest.main()
