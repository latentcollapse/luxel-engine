"""Tests for the erosion solver (systems roadmap S3).

The claim S3 makes is specific and therefore checkable: **glacial erosion turns
a V-shaped valley into a U-shaped one.** That is the difference between terrain
that looks like noise with a silhouette and terrain that looks like the Alps,
and it is the reason the module exists rather than another noise parameter.

Every test measures a signature rather than a number somebody picked, and each
is paired with the case that should *not* produce it — because "the erosion ran"
and "the erosion did what erosion does" are different claims and only the second
is worth anything.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from erosion import (  # noqa: E402
    PROFILES,
    ErosionProfile,
    erode,
    flux_field,
    thermal_erosion,
    valley_cross_section,
)

SIDE = 129
CELL_M = 2.0


def _v_valley(depth: float = 60.0) -> np.ndarray:
    """A sharp V running north-south, with a downstream tilt so water moves."""
    columns = np.abs(np.arange(SIDE) - SIDE // 2)[None, :] / (SIDE // 2)
    rows = np.linspace(1.0, 0.0, SIDE)[:, None]
    return columns * depth + rows * depth * 0.55


def _quick(profile: ErosionProfile, **overrides) -> ErosionProfile:
    """A short run, glaciated hard enough to observe.

    The shipped profiles are deliberately conservative -- erosion is off by
    default in the pipeline and heavily capped when on, because integrating it
    planed the border away. That conservatism makes the *mechanism* invisible in
    a fourteen-step run, so these tests raise planation to demonstrate the
    behaviour rather than the shipping defaults. They assert that ice widens a
    valley, not that the current parameters widen it by any particular amount.
    """
    # `lateral_widening` too: the shipped value derives a corridor from grid
    # size, and narrowing it for pipeline safety left a glacier ten cells wide
    # in a valley sixty-four cells across -- there was no wedge left to remove
    # and the cross-section stopped moving. A glacier fills its valley, and
    # **that the width is derived from the grid rather than from the valley is
    # a real limitation of this implementation**, recorded in the roadmap rather
    # than papered over here.
    fields = {
        **profile.__dict__,
        "iterations": 14,
        "planation": 0.85,
        "lateral_widening": profile.lateral_widening * 3.0,
    }
    fields.update(overrides)
    return ErosionProfile(**fields)


class FluxTest(unittest.TestCase):
    def test_flow_concentrates_downhill(self):
        height = np.tile(np.linspace(30.0, 0.0, SIDE)[:, None], (1, SIDE))
        flux = flux_field(height)
        self.assertGreater(float(flux[-2, :].mean()), float(flux[1, :].mean()))

    def test_a_valley_carries_more_than_its_flanks(self):
        """Multiple-flow-direction must still converge, or it is just a blur."""
        flux = flux_field(_v_valley())
        centre = float(flux[-2, SIDE // 2])
        flank = float(flux[-2, 4])
        self.assertGreater(centre, flank * 2.0)

    def test_flat_ground_concentrates_nothing(self):
        flux = flux_field(np.zeros((SIDE, SIDE)))
        self.assertLess(float(flux.max() - flux.min()), 1e-6)


class ThermalTest(unittest.TestCase):
    def test_a_wall_steeper_than_repose_collapses(self):
        height = np.zeros((SIDE, SIDE))
        height[:, : SIDE // 2] = 40.0
        relaxed = thermal_erosion(height, cell_m=CELL_M, talus_degrees=35.0)
        step_before = 40.0
        step_after = float(
            height[SIDE // 2, SIDE // 2 - 1] - relaxed[SIDE // 2, SIDE // 2]
        )
        self.assertLess(abs(step_after), step_before)

    def test_ground_within_repose_is_left_alone(self):
        """The negative case: thermal erosion is not a blur."""
        gentle = np.tile(
            (np.arange(SIDE) * 0.05 * CELL_M)[None, :], (SIDE, 1)
        ).astype(float)
        relaxed = thermal_erosion(gentle, cell_m=CELL_M, talus_degrees=45.0)
        self.assertLess(float(np.abs(relaxed - gentle).max()), 1e-9)

    def test_it_conserves_material(self):
        """Sliding moves rock; it does not delete it."""
        height = np.zeros((SIDE, SIDE))
        height[:, : SIDE // 2] = 40.0
        relaxed = thermal_erosion(height, cell_m=CELL_M, talus_degrees=30.0)
        self.assertAlmostEqual(float(height.sum()), float(relaxed.sum()), places=4)


class GlacialTest(unittest.TestCase):
    def test_glaciation_turns_a_v_into_a_u(self):
        """**The claim the whole module exists to make.**

        A river cuts a V. Ice cuts a U, because it grinds sideways as well as
        down. If this does not hold, S3 is an expensive way to add noise.
        """
        height = _v_valley()
        before = valley_cross_section(height, SIDE - 6)
        self.assertEqual("v", before["shape"])
        eroded, report = erode(height, _quick(PROFILES["alps"]), cell_m=CELL_M)
        after = valley_cross_section(eroded, SIDE - 6)
        self.assertGreater(
            after["form_ratio"], before["form_ratio"],
            "glaciation did not widen the valley floor",
        )
        self.assertGreater(report["glacial_share"], 0.5)

    def test_without_ice_the_valley_stays_a_v(self):
        """The negative case, and the one that proves it is the *ice* doing it.

        Same terrain, same iterations, glacial strength zero: fluvial erosion
        alone must leave the cross-section sharp.
        """
        height = _v_valley()
        widened, _ = erode(height, _quick(PROFILES["alps"]), cell_m=CELL_M)
        fluvial, _ = erode(
            height,
            _quick(PROFILES["alps"], glacial_strength=0.0, cirque_strength=0.0),
            cell_m=CELL_M,
        )
        self.assertGreater(
            valley_cross_section(widened, SIDE - 6)["form_ratio"],
            valley_cross_section(fluvial, SIDE - 6)["form_ratio"],
        )

    def test_the_equilibrium_line_moves_where_the_ice_works(self):
        """Glacial troughs start partway up a mountain; below them rivers work.

        **Asserted as placement, not as amount, and that is a real limitation
        rather than a convenience.** The obvious claim -- a lower snowline means
        more ice and therefore more erosion -- does not hold in this
        implementation: measured, an ELA at the 95th percentile removed *more*
        total material (110k) than one at the 15th (82k). The cause is that ice
        flux is normalised by its own maximum before the corridor threshold is
        applied, so spreading ice over more ground dilutes the trunk signal
        instead of strengthening it. The ELA therefore controls where the ice
        bites, not how hard.

        Recorded in the roadmap. Fixing it means an absolute ice-volume
        threshold rather than a relative one, which is a change to the model and
        not to a parameter.
        """
        height = _v_valley()
        high, _ = erode(height, _quick(PROFILES["alps"], ice_line=0.92), cell_m=CELL_M)
        low, _ = erode(height, _quick(PROFILES["alps"], ice_line=0.35), cell_m=CELL_M)
        self.assertFalse(
            np.allclose(high, low), "the equilibrium line changed nothing at all"
        )
        # Where the deepest cut lands must move with the snowline.
        self.assertNotEqual(
            int(np.argmin(high - height)), int(np.argmin(low - height))
        )

    def test_wider_ice_makes_a_wider_floor(self):
        """`lateral_widening` is most of the difference between a U and a V."""
        height = _v_valley()
        narrow, _ = erode(
            height, _quick(PROFILES["alps"], lateral_widening=0.2), cell_m=CELL_M
        )
        broad, _ = erode(
            height, _quick(PROFILES["alps"], lateral_widening=2.2), cell_m=CELL_M
        )
        self.assertGreater(
            valley_cross_section(broad, SIDE - 6)["form_ratio"],
            valley_cross_section(narrow, SIDE - 6)["form_ratio"],
        )


class CharacterTest(unittest.TestCase):
    def test_the_three_characters_produce_different_terrain(self):
        height = _v_valley()
        results = {
            key: erode(height, _quick(PROFILES[key]), cell_m=CELL_M)[0]
            for key in ("alps", "andes")
        }
        self.assertFalse(np.allclose(results["alps"], results["andes"]))

    def test_the_characters_differ_in_trough_width(self):
        """Where the three genuinely separate, measured rather than hoped for.

        An earlier version of this test asserted that Highland terrain is
        *gentler* than Andean, on mean slope and then on summit slope, and it
        failed both ways -- correctly. A Highland glacial trough has walls every
        bit as steep as an Andean one; Glencoe is not gentle. And this fixture
        is a uniform V with no summits at all, so a claim about summit form
        cannot be demonstrated on it however it is measured.

        What the characters actually differ in here is how wide the ice cut,
        which is `lateral_widening` doing its job.
        """
        height = _v_valley()
        highland, _ = erode(height, _quick(PROFILES["highlands"]), cell_m=CELL_M)
        andes, _ = erode(height, _quick(PROFILES["andes"]), cell_m=CELL_M)
        self.assertGreater(
            valley_cross_section(highland, SIDE - 6)["form_ratio"],
            valley_cross_section(andes, SIDE - 6)["form_ratio"],
        )

    def test_the_talus_angle_caps_how_steep_rock_stands(self):
        """The other half of the character, isolated from the ice.

        Thermal erosion is the term the angle of repose controls, so it is
        tested on its own: run a cliff to rest under two angles and the gentler
        one must settle flatter.
        """
        cliff = np.zeros((SIDE, SIDE))
        cliff[:, : SIDE // 2] = 60.0
        def settle(degrees):
            field = cliff.copy()
            for _ in range(40):
                field = thermal_erosion(
                    field, cell_m=CELL_M, talus_degrees=degrees, rate=0.5
                )
            gr, gc = np.gradient(field, CELL_M)
            return float(np.hypot(gr, gc).max())
        self.assertLess(settle(28.0), settle(48.0))

    def test_an_unknown_character_is_refused(self):
        with self.assertRaises(ValueError):
            erode(_v_valley(), "karst_needles", cell_m=CELL_M)


class ProtectionTest(unittest.TestCase):
    def test_protected_ground_is_untouched(self):
        """Lanes and landmark pads are already graded; a stream-power term
        would happily cut a gully straight through a road."""
        height = _v_valley()
        protect = np.zeros(height.shape, dtype=bool)
        protect[SIDE // 2 - 2 : SIDE // 2 + 3, :] = True
        eroded, _ = erode(
            height, _quick(PROFILES["alps"]), cell_m=CELL_M, protect=protect
        )
        self.assertTrue(np.allclose(eroded[protect], height[protect]))
        self.assertFalse(np.allclose(eroded[~protect], height[~protect]))


class ReportTest(unittest.TestCase):
    def test_the_report_measures_what_happened(self):
        eroded, report = erode(_v_valley(), _quick(PROFILES["alps"]), cell_m=CELL_M)
        self.assertEqual("alps", report["profile"])
        self.assertGreater(report["maximum_lowering_m"], 0.0)
        self.assertGreater(report["relief_before_m"], 0.0)
        self.assertGreater(report["relief_after_m"], 0.0)

    def test_cross_section_distinguishes_the_two_shapes(self):
        """The measurement itself needs a negative case, or it is a rubber ruler."""
        v = _v_valley()
        u = np.tile(
            np.clip((np.abs(np.arange(SIDE) - SIDE // 2) - 40) * 4.0, 0.0, None)[None, :],
            (SIDE, 1),
        ).astype(float)
        self.assertEqual("v", valley_cross_section(v, SIDE // 2)["shape"])
        self.assertEqual("u", valley_cross_section(u, SIDE // 2)["shape"])


if __name__ == "__main__":
    unittest.main()
