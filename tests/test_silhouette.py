from __future__ import annotations

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from reference_content import CONTENT_ROOT  # noqa: E402

from silhouette import (  # noqa: E402
    PROBE_CELL_M,
    SilhouetteError,
    _count_peaks,
    flat_field,
    from_batch,
    honesty,
    horizon_profile,
    measure,
    noise_field,
    profile_metrics,
    ridged_field,
    single_massif_field,
)

REAL_BATCH = (
    CONTENT_ROOT / "concept_batches/codeweald_alpine_arena_v1"
)


class FlatWorldTests(unittest.TestCase):
    """The control that caught the first version of this metric.

    A flat world has no silhouette. The first implementation scored it at 1.26
    degrees of relief and three peaks, because it let rays that ran off the map
    contribute negative elevation angles -- so the number described the world's
    rectangular *boundary*, not its landform, and would have credited the
    composition knobs for the shape of the data.
    """

    def test_a_flat_world_has_no_silhouette(self) -> None:
        metrics = measure(flat_field(), PROBE_CELL_M)
        self.assertAlmostEqual(0.0, metrics["horizon_relief_deg"], places=4)
        self.assertEqual(0.0, metrics["horizon_peaks_per_turn"])

    def test_a_flat_world_stays_flat_from_an_off_centre_viewpoint(self) -> None:
        # Off-centre is where the boundary artefact was strongest: rays run
        # different distances before leaving the map.
        profile = horizon_profile(flat_field(), PROBE_CELL_M, (40.0, 40.0))
        self.assertAlmostEqual(0.0, float(profile.max()), places=4)
        self.assertAlmostEqual(0.0, float(profile.std()), places=4)

    def test_ground_below_the_eye_never_forms_a_skyline(self) -> None:
        # A pit is not a silhouette feature. It cannot be seen against the sky.
        field = flat_field()
        field[100:150, 100:150] = -30.0
        self.assertAlmostEqual(
            0.0, measure(field, PROBE_CELL_M)["horizon_relief_deg"], places=4
        )


class PeakCountTracksAuthoredShapeTests(unittest.TestCase):
    """The property that makes this a usable target for `spine_count`."""

    def test_more_authored_ridges_means_more_visible_summits(self) -> None:
        counts = [
            measure(ridged_field(n), PROBE_CELL_M)["horizon_peaks_per_turn"]
            for n in (2, 4, 8)
        ]
        self.assertLess(counts[0], counts[1])
        self.assertLess(counts[1], counts[2])

    def test_a_taller_range_breaks_the_horizon_further(self) -> None:
        low = measure(ridged_field(4, height_m=10.0), PROBE_CELL_M)
        high = measure(ridged_field(4, height_m=60.0), PROBE_CELL_M)
        self.assertGreater(high["horizon_max_deg"], low["horizon_max_deg"])

    def test_prominence_stops_a_smooth_dome_reading_as_many_summits(self) -> None:
        # One hill is one hill, however many numerically-local maxima its
        # discretisation produces.
        angle = np.linspace(0.0, 2.0 * np.pi, 360, endpoint=False)
        dome = 10.0 * np.cos(angle) + 0.01 * np.sin(40.0 * angle)
        self.assertEqual(1, _count_peaks(dome))


class NoiseIsNotShapeTests(unittest.TestCase):
    """Item 6's discipline applied to a new metric before anyone trusts it.

    A metric that a noise field can ace is not measuring shape. Peak count
    alone *is* gameable that way -- noise scores more summits per turn than an
    eight-ridge world -- which is why it is labelled and why `horizon_coherence`
    exists.
    """

    def test_noise_beats_authored_ridges_on_raw_peak_count(self) -> None:
        # Documenting the gameable direction rather than hiding it: this is
        # exactly why peak count must not be an optimisation target alone.
        self.assertGreater(
            measure(noise_field(), PROBE_CELL_M)["horizon_peaks_per_turn"],
            measure(ridged_field(8), PROBE_CELL_M)["horizon_peaks_per_turn"],
        )

    def test_coherence_separates_authored_shape_from_noise(self) -> None:
        noisy = measure(noise_field(), PROBE_CELL_M)["horizon_coherence"]
        for ridges in (2, 4, 8):
            self.assertGreater(
                measure(ridged_field(ridges), PROBE_CELL_M)["horizon_coherence"],
                noisy * 0.85,
                ridges,
            )
        self.assertGreater(measure(ridged_field(2), PROBE_CELL_M)["horizon_coherence"], noisy * 2.0)

    def test_the_honesty_report_labels_the_gameable_number(self) -> None:
        document = honesty()
        self.assertEqual("shape_metric", document["kind"])
        self.assertEqual([], document["findings"])
        self.assertEqual(
            "descriptive_count_gameable_alone",
            document["metric_kinds"]["horizon_peaks_per_turn"],
        )
        self.assertNotIn("horizon_peaks_per_turn", document["optimisation_targets"])
        self.assertIn("horizon_coherence", document["optimisation_targets"])

    def test_the_guard_can_fail(self) -> None:
        # A guard that cannot report a problem is not a guard. Peak count on an
        # unsmoothed profile is the exact failure the acuity smoothing fixed,
        # so it must still be detectable.
        rough = noise_field()
        self.assertGreater(
            profile_metrics(horizon_profile(rough, PROBE_CELL_M, (128.0, 128.0)))[
                "horizon_roughness"
            ],
            profile_metrics(horizon_profile(ridged_field(2), PROBE_CELL_M, (128.0, 128.0)))[
                "horizon_roughness"
            ],
        )


class ViewpointTests(unittest.TestCase):
    def test_one_tall_lump_wins_on_relief_but_loses_on_coherence(self) -> None:
        # Found by probing rather than assumed: a 60 m block beside the
        # observer scores *higher* relief (11.19) than four authored ridges
        # (2.93), because a nearby wall genuinely does dominate a skyline. That
        # makes relief cheaper to game by raising one thing than by shaping the
        # world, so coherence is what has to carry the shape signal.
        massif = measure(single_massif_field(), PROBE_CELL_M)
        ridges = measure(ridged_field(4), PROBE_CELL_M)
        self.assertGreater(massif["horizon_relief_deg"], ridges["horizon_relief_deg"])
        self.assertLess(massif["horizon_coherence"], ridges["horizon_coherence"])

    def test_relief_is_demoted_once_the_massif_probe_beats_it(self) -> None:
        document = honesty()
        self.assertEqual(["horizon_coherence"], document["optimisation_targets"])
        self.assertEqual(
            "descriptive_count_gameable_alone",
            document["metric_kinds"]["horizon_relief_deg"],
        )

    def test_the_measurement_is_deterministic(self) -> None:
        first = measure(ridged_field(4), PROBE_CELL_M)
        self.assertEqual(first, measure(ridged_field(4), PROBE_CELL_M))


class CompiledBatchTests(unittest.TestCase):
    def setUp(self) -> None:
        if not (REAL_BATCH / "terrain/heightfield_f32le.bin").is_file():
            self.skipTest("batch not compiled")

    def test_the_real_world_has_a_measurable_silhouette(self) -> None:
        document = from_batch(REAL_BATCH)
        metrics = document["metrics"]
        self.assertGreater(metrics["horizon_relief_deg"], 0.0)
        self.assertGreater(metrics["horizon_max_deg"], 0.0)
        self.assertTrue(document["heightfield_sha256"])

    def test_a_missing_heightfield_is_named(self) -> None:
        with self.assertRaises(SilhouetteError) as caught:
            from_batch(Path("/nonexistent/batch"))
        self.assertIn("terrain_manifest.json", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
