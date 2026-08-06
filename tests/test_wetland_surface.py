"""The fifth splat layer, and the defect that had to be fixed before it shipped.

Peat had been computed since S2/S7 and reached no surface: `wetland_weight` fed
a 22% darkening of a preview PNG and a bake no shipped backend reads, while
every real backend iterated four channels. A bog rendered as grass.

Promoting it exposed a second defect that had been invisible while wetland was
only a soft tint. Measured on `codeweald_alpine_arena_v1` before the fix:

| slope band | share that was strong wetland |
|---|---|
| 0-5 deg | 16.9% |
| 25-35 deg | 24.0% |
| 35-90 deg | 24.3% |

Mean slope under wetland was 21.6 deg against 17.8 deg elsewhere -- the field
was *anti*-correlated with ground that can hold water, because it came from
blurring authored image-space rills that no longer followed the terrain the
massif inversion produced. A tint that reaches too far is a smudge; a material
that reaches too far is a peat bog painted up a cliff.

These cases pin both halves: peat stays on ground that can hold it, and the five
weights remain a partition of the surface.
"""

from __future__ import annotations

import math
import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from forestry import BOG_MAXIMUM_SLOPE_DEGREES  # noqa: E402
from zone_rasterizer import _smoothstep  # noqa: E402


def _gate(slope_tangent: np.ndarray) -> np.ndarray:
    """The rule as `zone_rasterizer` applies it, kept in one place."""
    limit = math.tan(math.radians(BOG_MAXIMUM_SLOPE_DEGREES))
    return 1.0 - _smoothstep(limit, limit * 2.0, slope_tangent)


class WetlandSlopeGateTest(unittest.TestCase):
    def test_flat_ground_keeps_all_of_its_peat(self):
        self.assertAlmostEqual(1.0, float(_gate(np.array([0.0]))[0]))

    def test_a_cliff_keeps_none(self):
        # 35 deg and beyond: the band that used to carry the *most* wetland.
        steep = np.tan(np.radians(np.array([35.0, 50.0, 70.0])))
        self.assertEqual(0.0, float(_gate(steep).max()))

    def test_the_limit_is_the_ecology_layers_own(self):
        """S6 plants the sedge and S7 paints the peat; one number, not two.

        When each carried its own limit they disagreed by more than thirty
        degrees. Importing it means a change to the niche moves the surfacing
        with it, rather than leaving the two to drift until a screenshot shows
        sedge growing on bare rock.
        """
        from forestry import HIGHLAND_NICHES

        bog = next(n for n in HIGHLAND_NICHES if n.key == "bog_sedge")
        self.assertEqual(BOG_MAXIMUM_SLOPE_DEGREES, bog.maximum_slope_degrees)

    def test_the_gate_falls_monotonically(self):
        """No band may keep more peat than a flatter band below it."""
        slope = np.tan(np.radians(np.linspace(0.0, 60.0, 200)))
        kept = _gate(slope)
        self.assertTrue(bool(np.all(np.diff(kept) <= 1e-12)))


class WetlandSplatPartitionTest(unittest.TestCase):
    """Five weights describing one surface must remain a partition of it."""

    def _normalise(self, grass, road, rock, snow, wetland_influence):
        wetland_surface = np.clip(wetland_influence * grass, 0.0, 1.0)
        grass = np.clip(grass - wetland_surface, 0.0, 1.0)
        total = np.maximum(grass + road + rock + snow + wetland_surface, 1e-6)
        return (
            np.stack([grass, road, rock, snow], axis=-1) / total[..., None],
            wetland_surface / total,
        )

    def test_all_five_weights_sum_to_one(self):
        rng = np.random.default_rng(20260803)
        shape = (32, 32)
        splat, wetland = self._normalise(
            rng.uniform(0.0, 1.0, shape),
            rng.uniform(0.0, 0.4, shape),
            rng.uniform(0.0, 1.0, shape),
            rng.uniform(0.0, 0.3, shape),
            rng.uniform(0.0, 1.0, shape),
        )
        total = splat.sum(axis=-1) + wetland
        self.assertAlmostEqual(1.0, float(total.min()), places=6)
        self.assertAlmostEqual(1.0, float(total.max()), places=6)

    def test_wetland_is_taken_from_grass_alone(self):
        """Peat forms where soil already was.

        A wet hollow must not eat the rock weight of the crag above it -- the
        alternative to a bog on a cell is the meadow beside it, never the cliff.
        """
        shape = (8, 8)
        rock = np.full(shape, 0.6)
        snow = np.full(shape, 0.1)
        road = np.full(shape, 0.05)
        dry, dry_wetland = self._normalise(
            np.full(shape, 0.4), road, rock, snow, np.zeros(shape)
        )
        wet, wet_wetland = self._normalise(
            np.full(shape, 0.4), road, rock, snow, np.ones(shape)
        )
        self.assertEqual(0.0, float(dry_wetland.max()))
        self.assertGreater(float(wet_wetland.min()), 0.0)
        # Rock, snow and road keep their share; only grass pays for the bog.
        for channel, name in ((1, "road"), (2, "rock"), (3, "snow")):
            self.assertAlmostEqual(
                float(dry[..., channel].mean()),
                float(wet[..., channel].mean()),
                places=6,
                msg="%s changed when wetland appeared" % name,
            )
        self.assertLess(float(wet[..., 0].max()), float(dry[..., 0].min()))


if __name__ == "__main__":
    unittest.main()
