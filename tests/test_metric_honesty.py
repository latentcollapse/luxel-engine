from __future__ import annotations

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from reference_content import CONTENT_ROOT  # noqa: E402

from metric_honesty import (  # noqa: E402
    METRICS,
    NOISE_SIGMAS,
    aliased_probe,
    assess,
    filtered_probe,
    flat_probe,
    labels,
    noise_probe,
    report,
    shuffled_probe,
    structured_probe,
)


class ClassifierCanReturnBothVerdictsTests(unittest.TestCase):
    """The guard must be able to say "honest" *and* "gameable".

    A classifier that labels everything a coverage gate would be as useless as
    the ungated metrics it replaces, and it would look exactly as convincing on
    this map. So both verdicts are pinned against metrics whose answer is known
    by construction.
    """

    def test_a_metric_that_only_sees_structure_is_called_a_quality_score(self) -> None:
        # Mean absolute deviation after heavy box-blurring: grain averages to
        # nothing, coarse relief survives.
        def coarse_only(rgb: np.ndarray) -> float:
            luma = rgb[:, :, 0]
            size = 32
            blocks = luma[: (luma.shape[0] // size) * size, : (luma.shape[1] // size) * size]
            blocks = blocks.reshape(
                blocks.shape[0] // size, size, blocks.shape[1] // size, size
            ).mean(axis=(1, 3))
            return float(np.abs(blocks - blocks.mean()).mean())

        self.assertEqual("quality_score", assess("coarse_only", coarse_only)["kind"])

    def test_a_metric_that_counts_grain_is_called_a_coverage_gate(self) -> None:
        def grain(rgb: np.ndarray) -> float:
            luma = rgb[:, :, 0]
            return float(np.abs(np.diff(luma, axis=1)).mean() * 20.0)

        verdict = assess("grain", grain)
        self.assertEqual("coverage_gate", verdict["kind"])
        self.assertIn("noise_scores_as_high_as_structure", verdict["findings"])

    def test_a_metric_reading_only_the_histogram_is_caught(self) -> None:
        # Shuffling pixels leaves the histogram untouched, so a metric that
        # scores shuffled the same as the original is not reading the image.
        verdict = assess("mean", lambda rgb: float(rgb[:, :, 0].mean()))
        self.assertIn("blind_to_spatial_structure", verdict["findings"])


class ProbeTests(unittest.TestCase):
    def test_a_blank_frame_carries_no_information(self) -> None:
        for name, metric in METRICS.items():
            self.assertAlmostEqual(0.0, metric(flat_probe()), places=5, msg=name)

    def test_shuffling_destroys_structure_but_not_the_histogram(self) -> None:
        original = np.sort(structured_probe()[:, :, 0].ravel())
        shuffled = np.sort(shuffled_probe()[:, :, 0].ravel())
        self.assertTrue(np.allclose(original, shuffled))

    def test_the_aliased_and_filtered_probes_differ_only_in_filtering(self) -> None:
        # Same content, one point-sampled and one averaged. Any metric that
        # separates them is responding to filtering quality, nothing else.
        self.assertFalse(np.allclose(aliased_probe(), filtered_probe()))
        self.assertAlmostEqual(
            float(filtered_probe()[:, :, 0].mean()),
            float(aliased_probe()[:, :, 0].mean()),
            places=2,
        )

    def test_probes_are_deterministic(self) -> None:
        # A guard whose verdict wobbles run to run is a guard nobody trusts.
        self.assertTrue(np.array_equal(noise_probe(0.065), noise_probe(0.065)))
        self.assertTrue(np.array_equal(shuffled_probe(), shuffled_probe()))


class RecordedIncidentTests(unittest.TestCase):
    """The guard has to reproduce the failures that motivated it.

    These are the observations from WGE/docs/platform/tooling-upgrades.md item 6. If the
    guard cannot rediscover them, it is not measuring what it claims to.
    """

    def test_surface_variation_coverage_saturates_on_faint_noise(self) -> None:
        metric = METRICS["surface_variation_coverage"]
        self.assertGreaterEqual(metric(noise_probe(0.02)), 0.99)
        # ...and the real render scores far below that, so the gate has no
        # headroom in the range that matters.
        self.assertLess(metric(structured_probe()), 0.99)

    def test_detail_density_rewards_aliasing_over_correct_filtering(self) -> None:
        metric = METRICS["detail_density"]
        self.assertGreater(metric(aliased_probe()), metric(filtered_probe()) * 4.0)

    def test_detail_density_is_a_cliff_that_grain_walks_over(self) -> None:
        metric = METRICS["detail_density"]
        self.assertGreater(metric(noise_probe(0.15)), metric(structured_probe()))

    def test_coarse_contrast_is_the_one_that_holds_up(self) -> None:
        metric = METRICS["coarse_contrast"]
        structured = metric(structured_probe())
        self.assertGreater(structured, max(metric(noise_probe(s)) for s in NOISE_SIGMAS) * 10.0)
        # Indifferent to filtering, which is the correct behaviour: filtering
        # changes detail, not coarse structure.
        self.assertAlmostEqual(metric(aliased_probe()), metric(filtered_probe()), places=3)
        self.assertGreater(structured, metric(shuffled_probe()) * 10.0)


class ReportTests(unittest.TestCase):
    def test_every_metric_is_labelled(self) -> None:
        self.assertEqual(set(METRICS), set(labels()))

    def test_the_report_separates_quality_scores_from_coverage_gates(self) -> None:
        document = report()
        self.assertIn("coarse_contrast", document["quality_scores"])
        for name in ("detail_density", "surface_variation_coverage", "edge_density"):
            self.assertIn(name, document["coverage_gates"])
        self.assertEqual(
            len(METRICS),
            len(document["quality_scores"]) + len(document["coverage_gates"]),
        )

    def test_the_acceptance_report_carries_the_labels(self) -> None:
        # Item 6's requirement: a metric a noise field can ace must be labelled
        # as such *in its own output*, not in a document beside it.
        import json

        from bevy_visual_acceptance import evaluate

        batch = (
            CONTENT_ROOT
            / "concept_batches/codeweald_alpine_arena_v1"
        )
        if not (batch / "bevy_overview.png").is_file():
            self.skipTest("no capture present")
        zone_spec = json.loads((batch / "zone_spec.json").read_text(encoding="utf-8"))
        result = evaluate(zone_spec, batch / "bevy_overview.png")
        self.assertEqual(labels(), result["metric_kinds"])
        for name in result["metric_kinds"]:
            self.assertIn(result["metric_kinds"][name], ("quality_score", "coverage_gate"))


if __name__ == "__main__":
    unittest.main()
