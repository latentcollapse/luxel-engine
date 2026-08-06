#!/usr/bin/env python3
"""Tests for the foliage projection gate.

The load-bearing test here is `test_adding_foliage_can_never_lower_the_metric`.
D28's whole finding was that the previous gate could go *down* when foliage went
*up*, so a replacement that is not proven monotonic has not addressed it — it
has only moved it. That is the negative control this gate would otherwise
lack.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "pipeline"))

from foliage_projection_acceptance import (  # noqa: E402
    ACCEPTANCE_VERSION,
    PROJECTION_VERSION,
    evaluate,
)


def instance(
    identifier: str,
    x: float = 100.0,
    y: float = 100.0,
    in_frame: bool = True,
    pixels_per_metre: float | None = 8.0,
) -> dict:
    return {
        "id": identifier,
        "world_m": [0.0, 0.0, 0.0],
        "in_frame": in_frame,
        "viewport_px": [x, y] if in_frame else None,
        "pixels_per_metre": pixels_per_metre,
    }


def projection(instances: list[dict]) -> dict:
    return {
        "schema_version": PROJECTION_VERSION,
        "viewport_px": [640.0, 480.0],
        "instance_count": len(instances),
        "in_frame_count": sum(1 for item in instances if item["in_frame"]),
        "instances": instances,
    }


def green_canvas(width: int = 640, height: int = 480) -> np.ndarray:
    """A frame that is foliage-hued everywhere."""
    canvas = np.zeros((height, width, 3), dtype=np.float32)
    canvas[:, :, 0] = 0.10
    canvas[:, :, 1] = 0.60
    canvas[:, :, 2] = 0.12
    return canvas


def bare_canvas(width: int = 640, height: int = 480) -> np.ndarray:
    """A frame with no foliage hue anywhere -- grey rock."""
    canvas = np.zeros((height, width, 3), dtype=np.float32)
    canvas[:, :, :] = 0.45
    return canvas


class FoliageProjectionAcceptanceTest(unittest.TestCase):
    def test_adding_foliage_can_never_lower_the_metric(self) -> None:
        """The exact defect D28 recorded, as a regression test.

        The old gate fell 0.006820 -> 0.005207 while instances rose 275 -> 301.
        Here the same direction of change must not be possible: every added
        in-frame instance raises the numerator and the denominator together,
        and an added *out of frame* instance may lower the fraction but can
        never lower the count.
        """
        smaller = evaluate(projection([instance(f"f{index}") for index in range(275)]))
        larger = evaluate(projection([instance(f"f{index}") for index in range(301)]))
        self.assertEqual(smaller["metrics"]["in_frame_count"], 275)
        self.assertEqual(larger["metrics"]["in_frame_count"], 301)
        self.assertGreater(
            larger["metrics"]["in_frame_count"],
            smaller["metrics"]["in_frame_count"],
        )
        # And the fraction did not fall, which is what the pixel metric did.
        self.assertGreaterEqual(
            larger["metrics"]["in_frame_fraction"],
            smaller["metrics"]["in_frame_fraction"],
        )

    def test_a_world_with_no_foliage_is_refused(self) -> None:
        report = evaluate(projection([]))
        self.assertEqual(report["status"], "failed")
        self.assertIn(
            "Compiled world placed no foliage instances", report["failures"]
        )

    def test_foliage_entirely_out_of_frame_is_refused(self) -> None:
        report = evaluate(
            projection([instance(f"f{index}", in_frame=False) for index in range(8)])
        )
        self.assertEqual(report["status"], "failed")
        self.assertIn(
            "Compiled foliage is entirely outside the capture frame",
            report["failures"],
        )
        self.assertEqual(report["metrics"]["in_frame_fraction"], 0.0)

    def test_no_foliage_and_out_of_frame_are_different_failures(self) -> None:
        """They send a reader to different files, so they must not be merged."""
        empty = evaluate(projection([]))["failures"]
        outside = evaluate(
            projection([instance("f0", in_frame=False)])
        )["failures"]
        self.assertNotEqual(empty, outside)

    def test_partial_framing_is_reported_not_failed(self) -> None:
        instances = [instance(f"in{index}") for index in range(6)]
        instances += [instance(f"out{index}", in_frame=False) for index in range(4)]
        report = evaluate(projection(instances))
        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["metrics"]["in_frame_count"], 6)
        self.assertEqual(report["metrics"]["instance_count"], 10)
        self.assertAlmostEqual(report["metrics"]["in_frame_fraction"], 0.6)

    def test_the_probe_finds_canopy_above_the_base(self) -> None:
        """The projected point is the trunk's foot, so the probe looks up."""
        report = evaluate(projection([instance("f0", x=320.0, y=300.0)]), green_canvas())
        self.assertEqual(report["metrics"]["probed_count"], 1)
        self.assertEqual(report["metrics"]["probe_hit_count"], 1)

    def test_the_probe_misses_on_bare_rock(self) -> None:
        report = evaluate(projection([instance("f0", x=320.0, y=300.0)]), bare_canvas())
        self.assertEqual(report["metrics"]["probed_count"], 1)
        self.assertEqual(report["metrics"]["probe_hit_count"], 0)

    def test_the_probe_can_fail_which_is_what_makes_a_hit_evidence(self) -> None:
        """A detector that cannot go red is not evidence of health.

        The rasterizer audit's lens, applied here: `_sidewall_corrugation` was a
        well-written gate that never once fired on the defect it was built for.
        This asserts the probe discriminates rather than always passing.
        """
        hit = evaluate(projection([instance("f0", x=320.0, y=300.0)]), green_canvas())
        miss = evaluate(projection([instance("f0", x=320.0, y=300.0)]), bare_canvas())
        self.assertEqual(hit["metrics"]["probe_hit_fraction"], 1.0)
        self.assertEqual(miss["metrics"]["probe_hit_fraction"], 0.0)

    def test_the_probe_is_not_gated(self) -> None:
        """It has never been measured on a real build, so it must not fail one."""
        report = evaluate(projection([instance("f0", x=320.0, y=300.0)]), bare_canvas())
        self.assertEqual(report["metrics"]["probe_hit_fraction"], 0.0)
        self.assertEqual(report["status"], "passed")

    def test_the_probe_is_skipped_without_a_capture(self) -> None:
        report = evaluate(projection([instance("f0")]))
        self.assertEqual(report["metrics"]["probed_count"], 0)
        self.assertEqual(report["status"], "passed")

    def test_an_instance_without_a_scale_is_not_probed(self) -> None:
        """A null `pixels_per_metre` means the projection could not size it."""
        report = evaluate(
            projection([instance("f0", pixels_per_metre=None)]), green_canvas()
        )
        self.assertEqual(report["metrics"]["probed_count"], 0)
        self.assertEqual(report["metrics"]["in_frame_count"], 1)

    def test_a_probe_at_the_frame_edge_does_not_escape_the_array(self) -> None:
        for x, y in ((0.0, 0.0), (639.0, 479.0), (0.0, 479.0), (639.0, 0.0)):
            report = evaluate(
                projection([instance("edge", x=x, y=y)]), green_canvas()
            )
            self.assertEqual(report["metrics"]["probed_count"], 1)

    def test_a_foreign_schema_is_refused_rather_than_scored(self) -> None:
        report = evaluate({"schema_version": "something.else/v1", "instances": []})
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["schema_version"], ACCEPTANCE_VERSION)
        self.assertEqual(report["metrics"], {})


if __name__ == "__main__":
    unittest.main()
