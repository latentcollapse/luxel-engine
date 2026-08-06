"""Tests for the vegetation solver (systems roadmap S6).

What this replaces: foliage scattered into an authored polygon at a declared
spacing, which cannot produce a treeline, a riparian fringe, or a bog that looks
like a bog no matter how it is tuned.

Each case asserts an *ecological* property rather than a number, and is paired
with the arrangement that would break it -- the same species must be present or
absent depending only on the ground it is offered.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from forestry import HIGHLAND_NICHES, Niche, resolve, suitability, treeline_m  # noqa: E402

SIDE = 48


def _uniform(value: float) -> np.ndarray:
    return np.full((SIDE, SIDE), value, dtype=np.float64)


def _conditions(**overrides):
    base = {
        "relative_elevation": _uniform(0.30),
        "slope_degrees": _uniform(10.0),
        "wetness": _uniform(5.0),
        "insolation": _uniform(0.5),
        "exposure": _uniform(0.4),
    }
    base.update(overrides)
    return base


def _niche(key: str) -> Niche:
    return next(n for n in HIGHLAND_NICHES if n.key == key)


class NicheTest(unittest.TestCase):
    def test_a_family_grows_where_it_belongs(self):
        conifer = _niche("montane_conifer")
        score = suitability(conifer, **_conditions())
        self.assertGreater(float(score.mean()), 0.5)

    def test_a_cliff_supports_nothing(self):
        """One intolerable condition is fatal, whatever else is perfect."""
        for niche in HIGHLAND_NICHES:
            score = suitability(niche, **_conditions(slope_degrees=_uniform(78.0)))
            with self.subTest(niche=niche.key):
                self.assertEqual(0.0, float(score.max()))

    def test_conditions_multiply_rather_than_average(self):
        """A mean would let good elevation excuse a drowned root system.

        The conifer's wetness band tops out at 8; bog conditions are 12. If the
        terms averaged, it would still score respectably on elevation, slope,
        light and shelter. It must score zero.
        """
        conifer = _niche("montane_conifer")
        drowned = suitability(conifer, **_conditions(wetness=_uniform(12.0)))
        self.assertEqual(0.0, float(drowned.max()))

    def test_the_treeline_is_an_elevation_band_not_a_cliff_edge(self):
        """Suitability must decay across the band, or the forest is stamped."""
        conifer = _niche("montane_conifer")
        band = np.linspace(0.0, 1.0, SIDE)
        elevation = np.tile(band[:, None], (1, SIDE))
        score = suitability(conifer, **_conditions(relative_elevation=elevation))
        column = score[:, 0]
        # Somewhere it thins rather than switching: values strictly between.
        partial = (column > 0.05) & (column < 0.95)
        self.assertGreater(int(partial.sum()), 3, "treeline is a step, not a gradient")


class CompetitionTest(unittest.TestCase):
    def _winner(self, **overrides) -> str:
        best, _ = resolve(HIGHLAND_NICHES, **_conditions(**overrides))
        index = int(np.bincount(best[best >= 0].ravel()).argmax())
        return HIGHLAND_NICHES[index].key

    def test_wet_flat_ground_goes_to_bog_sedge(self):
        """The distinction that makes a bog look like a bog."""
        self.assertEqual(
            "bog_sedge",
            self._winner(wetness=_uniform(11.0), slope_degrees=_uniform(2.0),
                         relative_elevation=_uniform(0.10)),
        )

    def test_damp_low_ground_goes_to_riparian_broadleaf(self):
        """The fringe that makes a watercourse read as a watercourse."""
        self.assertEqual(
            "riparian_broadleaf",
            self._winner(wetness=_uniform(9.0), slope_degrees=_uniform(6.0),
                         relative_elevation=_uniform(0.12), exposure=_uniform(0.2)),
        )

    def test_high_exposed_ground_goes_to_krummholz(self):
        """Above the forest, below the rock."""
        self.assertEqual(
            "subalpine_krummholz",
            self._winner(relative_elevation=_uniform(0.72), exposure=_uniform(0.9),
                         wetness=_uniform(4.0)),
        )

    def test_bare_ground_exists(self):
        """A map with no clearings reads as a carpet.

        Ground that suits nothing must resolve to nothing rather than to
        whichever family scored least badly.
        """
        best, _ = resolve(
            HIGHLAND_NICHES,
            **_conditions(slope_degrees=_uniform(70.0), wetness=_uniform(0.2)),
        )
        self.assertTrue(bool((best == -1).all()))

    def test_aspect_changes_the_outcome(self):
        """Shade and sun are different places, and must resolve differently.

        Aspect asymmetry is the signature that distinguishes a modelled range
        from a textured one; if insolation moved nothing, it would not be worth
        computing.
        """
        shaded, _ = resolve(HIGHLAND_NICHES, **_conditions(insolation=_uniform(0.12)))
        sunlit, _ = resolve(HIGHLAND_NICHES, **_conditions(insolation=_uniform(0.95)))
        self.assertFalse(bool(np.array_equal(shaded, sunlit)))


class TreelineTest(unittest.TestCase):
    def test_the_treeline_is_measured_from_the_canopy(self):
        height = np.tile(np.linspace(0.0, 100.0, SIDE)[:, None], (1, SIDE))
        canopy = height < 60.0
        line = treeline_m(HIGHLAND_NICHES, height, canopy)
        self.assertIsNotNone(line)
        self.assertLess(line, 62.0)
        self.assertGreater(line, 45.0)

    def test_no_canopy_reports_no_treeline(self):
        """Rather than reporting zero, which would read as a treeline at sea level."""
        height = np.zeros((SIDE, SIDE))
        self.assertIsNone(treeline_m(HIGHLAND_NICHES, height, np.zeros((SIDE, SIDE), bool)))


if __name__ == "__main__":
    unittest.main()
