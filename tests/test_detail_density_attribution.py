"""Surface detail must be measured comparably and blamed on the right owner.

Two defects are covered here, and they compound. The metrics were not
comparable -- one side averaged over foreground pixels at one threshold, the
other over the whole frame at another -- so the shortfall had no defensible
size. And the critic attributed that shortfall to landform composition, which
was measured (three build+capture cycles, every scalar driven to its bound) to
move it by under one percent.

Together they produced a loop that could run forever: a wrong number pointed at
a knob that could not change it.
"""

import sys
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

PIPELINE = Path(__file__).resolve().parents[1] / "pipeline"
sys.path.insert(0, str(PIPELINE))

from style_reference import (
    DETAIL_DENSITY_HEIGHT,
    DETAIL_DENSITY_THRESHOLD,
    _edge_density,
    _luma,
    analyze,
    detail_density,
    surface_variation_coverage,
)
from wge_critic import _detail_pair, diagnose, merge_patches


def _ridged(height: int) -> np.ndarray:
    """A deterministic pattern defined in normalised coordinates.

    The same *content* at any pixel count, which is exactly what a comparable
    detail metric has to be insensitive to. Contrast ramps left to right so that
    only part of the frame clears the edge threshold -- a flat high-contrast
    pattern would saturate at every resolution and prove nothing.
    """
    width = int(height * 16 / 9)
    y = np.linspace(0.0, 1.0, height, dtype=np.float32)[:, None]
    x = np.linspace(0.0, 1.0, width, dtype=np.float32)[None, :]
    field = 0.5 + (0.5 * x) * np.sin(y * 2.0 * np.pi * 40.0)
    return np.clip(np.repeat(field[:, :, None], 3, axis=2), 0.0, 1.0)


def _zone_spec() -> dict:
    return {
        "features": [
            {
                "id": "western_alps",
                "category": "landform",
                "semantic": "alpine_massif",
                "generation": {
                    "composition": {
                        "pattern": "ridge_network",
                        "spine_count": 3,
                        "elevation_bias": 0.5,
                        "along_jitter": 0.08,
                        "cross_jitter": 0.10,
                    }
                },
            }
        ]
    }


class DetailDensityIsComparableTests(unittest.TestCase):
    def test_same_content_at_different_resolutions_scores_the_same(self):
        small = detail_density(_ridged(DETAIL_DENSITY_HEIGHT))
        large = detail_density(_ridged(DETAIL_DENSITY_HEIGHT * 2))
        self.assertAlmostEqual(small, large, delta=0.02)

    def test_the_legacy_metric_was_not_resolution_stable(self):
        """The property the old comparison lacked, shown rather than asserted."""
        small = _edge_density(_luma(_ridged(DETAIL_DENSITY_HEIGHT)))
        large = _edge_density(_luma(_ridged(DETAIL_DENSITY_HEIGHT * 2)))
        self.assertGreater(
            abs(small - large),
            0.05,
            "if the legacy metric were resolution-stable this fix would be moot",
        )

    def test_flat_frames_carry_no_detail(self):
        flat = np.full((480, 640, 3), 0.42, dtype=np.float32)
        self.assertEqual(0.0, detail_density(flat))

    def test_threshold_is_applied_to_luminance_gradient(self):
        height = DETAIL_DENSITY_HEIGHT
        ramp = np.zeros((height, 64, 3), dtype=np.float32)
        # A step larger than the threshold on exactly one column boundary.
        ramp[:, 32:, :] = DETAIL_DENSITY_THRESHOLD * 4.0
        self.assertGreater(detail_density(ramp), 0.0)

    def test_analyze_reports_detail_density_alongside_the_legacy_metric(self):
        # Off the canonical height the two diverge, which is the whole point:
        # the legacy metric is reading pixels, the new one is reading content.
        metrics = analyze(_ridged(DETAIL_DENSITY_HEIGHT * 2))["metrics"]
        self.assertIn("detail_density", metrics)
        self.assertIn("edge_density", metrics)
        self.assertNotEqual(metrics["detail_density"], metrics["edge_density"])

    def test_legacy_metric_is_untouched(self):
        """Re-tuning the existing gates is a separate change, not this one."""
        luma = _luma(_ridged(DETAIL_DENSITY_HEIGHT * 2))
        gradient_y, gradient_x = np.gradient(luma)
        self.assertEqual(
            float((np.hypot(gradient_x, gradient_y) >= 0.055).mean()),
            _edge_density(luma),
        )

    def test_rgb_is_required(self):
        with self.assertRaises(ValueError):
            detail_density(np.zeros((16, 16), dtype=np.float32))


def _rescale(rgb: np.ndarray, factor: float) -> np.ndarray:
    height, width = rgb.shape[:2]
    resized = Image.fromarray((rgb * 255.0).astype(np.uint8)).resize(
        (max(1, int(round(width * factor))), max(1, int(round(height * factor)))),
        Image.BILINEAR,
    )
    return np.asarray(resized, dtype=np.float32) / 255.0


class SurfaceVariationCoverageTests(unittest.TestCase):
    """A coverage gate, not a quality score.

    This counts whether a block was touched at all, at a fixed block size and
    a fixed std-dev floor. It says nothing about how much variation a block
    carries once it clears that floor, so it is trivially satisfiable by
    scattering noise across an otherwise dead frame -- that is a necessary
    condition for surface variation, not evidence of it. It exists to catch
    the specific failure of untouched flat regions, not to stand in for a
    quality judgement the way ``detail_density`` is used elsewhere.
    """

    def test_stable_under_rescaling(self):
        textured = _ridged(DETAIL_DENSITY_HEIGHT)
        full = surface_variation_coverage(textured)
        half = surface_variation_coverage(_rescale(textured, 0.5))
        third = surface_variation_coverage(_rescale(textured, 0.35))
        self.assertAlmostEqual(full, half, delta=0.05)
        self.assertAlmostEqual(full, third, delta=0.05)
        self.assertAlmostEqual(half, third, delta=0.05)

    def test_sparse_frames_are_not_scale_stable(self):
        """The limit of the stability claim, pinned rather than glossed over.

        A dense frame saturates and is stable at any size. A sparse one is not,
        and the direction depends on what the detail is made of: fine grain
        averages away under downsampling and the number falls, while structured
        detail like an edge or a shading ramp survives and smears across
        proportionally more blocks, so the number rises. The current render is
        the second kind -- 0.27 at full size, 0.39 at 0.35x.

        Either way, comparing captures of different sizes reads as a change that
        did not happen. Only compare like-sized frames.
        """
        grainy = np.full((512, 512, 3), 0.42, dtype=np.float32)
        rng = np.random.default_rng(3)
        for _ in range(40):
            row, column = rng.integers(0, 496, size=2)
            grainy[row : row + 12, column : column + 12, :] += rng.normal(
                scale=0.08, size=(12, 12, 1)
            )
        grainy = np.clip(grainy, 0.0, 1.0)

        structured = np.full((512, 512, 3), 0.42, dtype=np.float32)
        for offset in range(0, 512, 64):
            structured[:, offset : offset + 2, :] = 0.86

        for name, frame in (("grain", grainy), ("structure", structured)):
            with self.subTest(detail=name):
                full = surface_variation_coverage(frame)
                shrunk = surface_variation_coverage(_rescale(frame, 0.35))
                self.assertLess(full, 0.6, "fixture must actually be sparse")
                self.assertGreater(
                    abs(full - shrunk),
                    0.03,
                    "sparse coverage is not scale-stable; if it became stable "
                    "the like-sized-comparison caveat could be dropped",
                )

    def test_flat_frame_scores_zero(self):
        flat = np.full((480, 640, 3), 0.42, dtype=np.float32)
        self.assertEqual(0.0, surface_variation_coverage(flat))

    def test_fully_textured_frame_scores_close_to_one(self):
        rng = np.random.default_rng(0)
        noisy = np.clip(
            0.5 + rng.normal(scale=0.2, size=(480, 640)), 0.0, 1.0
        ).astype(np.float32)
        textured = np.repeat(noisy[:, :, None], 3, axis=2)
        self.assertGreater(surface_variation_coverage(textured), 0.98)

    def test_analyze_reports_the_metric(self):
        metrics = analyze(_ridged(DETAIL_DENSITY_HEIGHT))["metrics"]
        self.assertIn("surface_variation_coverage", metrics)

    def test_rgb_is_required(self):
        with self.assertRaises(ValueError):
            surface_variation_coverage(np.zeros((16, 16), dtype=np.float32))


class DetailPairTests(unittest.TestCase):
    def test_comparable_metrics_are_preferred(self):
        render = {"detail_density": 0.018, "foreground_edge_density": 0.044}
        source = {"detail_density": 0.277, "edge_density": 0.306}
        self.assertEqual((0.018, 0.277, True), _detail_pair(render, source))

    def test_legacy_capture_falls_back_and_is_flagged(self):
        render = {"foreground_edge_density": 0.044}
        source = {"edge_density": 0.306}
        detail, src_detail, comparable = _detail_pair(render, source)
        self.assertFalse(comparable)
        self.assertEqual((0.044, 0.306), (detail, src_detail))

    def test_half_a_pair_is_not_comparable(self):
        self.assertFalse(
            _detail_pair({"detail_density": 0.018}, {"edge_density": 0.306})[2]
        )


class DetailAttributionTests(unittest.TestCase):
    """The shortfall is reported against the owner that can close it."""

    def _detail_finding(self, render=None, source=None):
        render = render or {"detail_density": 0.018058, "foreground_edge_density": 0.044576}
        source = source or {"detail_density": 0.27661, "edge_density": 0.305775}
        findings = diagnose(_zone_spec(), render, source)
        matching = [
            f
            for f in findings
            if f.metric in {"detail_density", "foreground_edge_density"}
        ]
        self.assertEqual(1, len(matching))
        return matching[0]

    def test_shortfall_is_not_dsl_addressable(self):
        self.assertFalse(self._detail_finding().actionable)

    def test_shortfall_is_owned_by_materials(self):
        self.assertIn("materials", self._detail_finding().owner)

    def test_no_composition_patches_are_emitted_for_it(self):
        self.assertEqual({}, self._detail_finding().patches)

    def test_the_loop_emits_nothing_it_cannot_deliver(self):
        """The regression that matters: no jitter/spine repairs for detail."""
        findings = diagnose(
            _zone_spec(),
            {"detail_density": 0.018058, "foreground_edge_density": 0.044576},
            {"detail_density": 0.27661, "edge_density": 0.305775},
        )
        for change in merge_patches(findings).values():
            self.assertNotIn("along_jitter", change)
            self.assertNotIn("cross_jitter", change)
            self.assertNotIn("spines", change)

    def test_a_legacy_capture_says_its_number_is_approximate(self):
        finding = self._detail_finding(
            render={"foreground_edge_density": 0.044576},
            source={"edge_density": 0.305775},
        )
        self.assertIn("approximate", finding.diagnosis)
        self.assertEqual("foreground_edge_density", finding.metric)

    def test_a_matching_render_reports_nothing(self):
        findings = diagnose(
            _zone_spec(),
            {"detail_density": 0.30, "foreground_edge_density": 0.30},
            {"detail_density": 0.28, "edge_density": 0.28},
        )
        self.assertEqual(
            [], [f for f in findings if f.metric == "detail_density"]
        )


class CompositionPatchingSurvivesTests(unittest.TestCase):
    """Reattribution must not disarm the critic's real DSL lever."""

    def test_relief_depth_still_drives_elevation_bias(self):
        findings = diagnose(
            _zone_spec(),
            {"detail_density": 0.28, "foreground_dynamic_range": 0.05},
            {"detail_density": 0.28, "luminance_stddev": 0.20},
        )
        patches = merge_patches(findings)
        self.assertIn("western_alps", patches)
        self.assertIn("elevation_bias", patches["western_alps"])


if __name__ == "__main__":
    unittest.main()
