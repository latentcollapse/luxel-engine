"""Terrain-intrinsic preview metrics.

The case that matters is `test_a_cone_scores_like_a_cone` together with
`test_eroded_terrain_is_not_monotonic`. Between them they are the measurement
the massif carve should have had to pass: relief that is a monotonic function of
distance from a point scores ~1.0, and terrain whose summits are not a function
of that distance scores far below it. Both previous terrain attempts would have
failed this on the night they were built.
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

import preview_metrics  # noqa: E402

RESOLUTION = 257
WORLD_M = 800.0


def cone(resolution: int = RESOLUTION, relief_m: float = 300.0) -> np.ndarray:
    """A basin rim: height rising monotonically with distance from the centre.

    This is the massif carve in polar coordinates -- the shape 5.5 exists to
    detect.
    """
    centre = (resolution - 1) / 2.0
    rows, columns = np.mgrid[0:resolution, 0:resolution]
    distance = np.hypot(rows - centre, columns - centre)
    return (distance / distance.max() * relief_m).astype(np.float32)


def fractal(resolution: int = RESOLUTION, relief_m: float = 300.0, seed: int = 7):
    """Value noise with a few octaves: peaks are not a function of any radius."""
    generator = np.random.default_rng(seed)
    field = np.zeros((resolution, resolution), dtype=np.float64)
    amplitude = 1.0
    for octave in (4, 8, 16, 32):
        coarse = generator.random((octave + 1, octave + 1))
        rows = np.linspace(0, octave, resolution)
        row_index = np.clip(rows.astype(int), 0, octave - 1)
        weight = rows - row_index
        interpolated = (
            coarse[row_index] * (1 - weight)[:, None] + coarse[row_index + 1] * weight[:, None]
        )
        column_index = np.clip(rows.astype(int), 0, octave - 1)
        column_weight = rows - column_index
        field += amplitude * (
            interpolated[:, column_index] * (1 - column_weight)[None, :]
            + interpolated[:, column_index + 1] * column_weight[None, :]
        )
        amplitude *= 0.5
    field -= field.min()
    return (field / field.max() * relief_m).astype(np.float32)


class RadialMonotonicityTest(unittest.TestCase):
    def test_a_cone_scores_like_a_cone(self) -> None:
        result = preview_metrics.radial_monotonicity(cone())
        self.assertGreater(result["mean_non_descending_fraction"], 0.99)
        self.assertGreater(result["fully_monotonic_ray_fraction"], 0.95)

    def test_eroded_terrain_is_not_monotonic(self) -> None:
        result = preview_metrics.radial_monotonicity(fractal())
        self.assertLess(result["mean_non_descending_fraction"], 0.8)
        self.assertLess(result["fully_monotonic_ray_fraction"], 0.05)

    def test_the_two_are_separated_by_a_wide_margin(self) -> None:
        """The gate is only useful if the classes do not overlap."""
        conic = preview_metrics.radial_monotonicity(cone())
        real = preview_metrics.radial_monotonicity(fractal())
        self.assertGreater(
            conic["mean_non_descending_fraction"]
            - real["mean_non_descending_fraction"],
            0.25,
        )

    def test_a_basin_is_detected_through_added_noise(self) -> None:
        """Weak noise over a strong radial bias is still a bowl, and the metric
        has to say so rather than being satisfied by surface roughness."""
        biased = cone() * 0.9 + fractal(relief_m=300.0) * 0.1
        result = preview_metrics.radial_monotonicity(biased)
        self.assertGreater(result["mean_non_descending_fraction"], 0.9)


class SymmetryResidualTest(unittest.TestCase):
    def test_a_symmetric_field_has_no_residual(self) -> None:
        field = fractal()
        symmetric = (0.5 * (field + field[::-1, ::-1])).astype(np.float32)
        result = preview_metrics.symmetry_residual(symmetric, 300.0)
        self.assertLess(result["max_fraction_of_relief"], 1e-5)

    def test_an_asymmetric_field_has_one(self) -> None:
        result = preview_metrics.symmetry_residual(fractal(), 300.0)
        self.assertGreater(result["max_fraction_of_relief"], 0.05)

    def test_symmetry_holds_at_both_parities(self) -> None:
        for resolution in (256, 257):
            with self.subTest(resolution=resolution):
                field = fractal(resolution=resolution)
                symmetric = (0.5 * (field + field[::-1, ::-1])).astype(np.float32)
                self.assertLess(
                    preview_metrics.symmetry_residual(symmetric, 300.0)[
                        "max_fraction_of_relief"
                    ],
                    1e-5,
                )


class DrainageAndSlopeTest(unittest.TestCase):
    def test_a_basin_has_exactly_one_sink(self) -> None:
        """Its floor. A bowl drains to one point and nowhere else, which is both
        the correct answer and a reminder that low sink counts are not on their
        own evidence of good terrain."""
        self.assertEqual(preview_metrics.sink_density(cone())["sink_count"], 1)

    def test_white_noise_is_full_of_sinks(self) -> None:
        noise = np.random.default_rng(3).random((RESOLUTION, RESOLUTION)).astype(
            np.float32
        )
        self.assertGreater(
            preview_metrics.sink_density(noise)["sinks_per_1000_cells"], 10.0
        )

    def test_slope_is_reported_as_a_percentage_grade(self) -> None:
        """`cone` normalises by the corner distance, so its 300 m rise runs over
        `128 * sqrt(2)` cells of 3.125 m -- about 566 m, a 53% grade."""
        cell_m = WORLD_M / (RESOLUTION - 1)
        run_m = ((RESOLUTION - 1) / 2.0) * np.sqrt(2.0) * cell_m
        stats = preview_metrics.slope_stats(cone(), cell_m)
        self.assertAlmostEqual(stats["p50_grade"], 300.0 / run_m * 100.0, delta=2.0)


class ReportTest(unittest.TestCase):
    def test_the_report_refuses_to_look_like_certification(self) -> None:
        metrics = preview_metrics.measure(fractal(), WORLD_M)
        self.assertEqual(
            metrics["schema_version"], preview_metrics.PREVIEW_METRICS_SCHEMA
        )
        self.assertNotEqual(metrics["schema_version"], "codeweald.terrain-artifacts/v1")
        self.assertIs(metrics["certifies"], False)
        self.assertIn("not an acceptance result", metrics["note"].lower())

    def test_report_round_trips_through_a_preview_batch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            batch = Path(directory) / "preview"
            terrain = batch / "terrain"
            terrain.mkdir(parents=True)
            heights = fractal()
            (terrain / "heightfield_f32le.bin").write_bytes(heights.tobytes())
            (terrain / "terrain_manifest.json").write_text(
                json.dumps(
                    {
                        "preview": True,
                        "zone_id": "preview",
                        "resolution": RESOLUTION,
                        "world_bounds_m": {"width": WORLD_M, "length": WORLD_M},
                        "heightfield_sha256": "deadbeef",
                    }
                )
            )
            loaded, manifest = preview_metrics.read_preview(batch)
            self.assertEqual(loaded.shape, (RESOLUTION, RESOLUTION))
            self.assertTrue(manifest["preview"])

    def test_a_manifest_disagreeing_with_the_buffer_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            batch = Path(directory) / "preview"
            terrain = batch / "terrain"
            terrain.mkdir(parents=True)
            (terrain / "heightfield_f32le.bin").write_bytes(fractal(64).tobytes())
            (terrain / "terrain_manifest.json").write_text(
                json.dumps(
                    {
                        "resolution": RESOLUTION,
                        "world_bounds_m": {"width": WORLD_M, "length": WORLD_M},
                    }
                )
            )
            with self.assertRaisesRegex(SystemExit, "implies"):
                preview_metrics.read_preview(batch)


if __name__ == "__main__":
    unittest.main()
