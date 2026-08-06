from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

from PIL import Image, ImageDraw


PIPELINE = Path(__file__).resolve().parents[1] / "pipeline"
sys.path.insert(0, str(PIPELINE))

import numpy as np

from bevy_visual_acceptance import UNREADABLE_LIMIT, evaluate


class BevyVisualAcceptanceTests(unittest.TestCase):
    def test_rejects_black_or_clipped_capture_and_accepts_structured_world(self) -> None:
        zone = {"zone": {"id": "synthetic"}}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            black = root / "black.png"
            Image.new("RGB", (512, 320), (0, 0, 0)).save(black)
            self.assertEqual("failed", evaluate(zone, black)["status"])

            image = Image.new("RGB", (512, 320), (48, 63, 67))
            draw = ImageDraw.Draw(image)
            draw.polygon(
                [(70, 120), (430, 95), (455, 270), (85, 275)],
                fill=(75, 82, 65),
            )
            draw.line(
                [(95, 235), (230, 180), (420, 145)],
                fill=(145, 105, 55),
                width=14,
            )
            for x_coordinate in range(110, 410, 35):
                draw.polygon(
                    [
                        (x_coordinate, 125),
                        (x_coordinate - 8, 158),
                        (x_coordinate + 8, 158),
                    ],
                    fill=(42, 88, 47),
                )
            draw.ellipse((250, 215, 315, 248), fill=(45, 105, 125))
            structured = root / "structured.png"
            image.save(structured)
            report = evaluate(zone, structured)
            self.assertEqual([], report["failures"])
            self.assertEqual("passed", report["status"])


def _shaded_world(path: Path, *, structured: bool) -> None:
    """A world that is mostly *dark*, either with detail in the dark or without.

    Both images have the same luma distribution and the same dark fraction. The
    only difference is whether the dark region carries a gradient -- which is
    exactly the distinction D25 says the gate must make, so building them from a
    shared base is what makes this a controlled comparison rather than two
    unrelated pictures.
    """
    rows, columns = 320, 512
    rng = np.random.default_rng(20260804)
    # Sky, so there is a background for the foreground test to separate from.
    frame = np.full((rows, columns, 3), (46, 61, 66), dtype=np.uint8)
    # Inset, so the dark mass does not touch the frame edge. Running it to the
    # border trips the *clipping* gate instead, which would have made this test
    # pass or fail for a reason that has nothing to do with D25.
    body, side = slice(96, rows - 14), slice(24, columns - 24)
    span = (body.stop - body.start, side.stop - side.start)
    if structured:
        # A shadowed cliff: dark, but every neighbouring pixel still differs.
        shade = rng.integers(2, 13, size=span)
    else:
        # Crushed: the same darkness with the detail quantised out of existence.
        shade = np.full(span, 6)
    for channel in range(3):
        frame[body, side, channel] = shade
    # Both get the same readable furniture, so only the dark region varies and
    # the other gates (roads, foliage, water, contrast) cannot be what differs.
    image = Image.fromarray(frame, "RGB")
    draw = ImageDraw.Draw(image)
    draw.line([(95, 235), (230, 200), (420, 165)], fill=(145, 105, 55), width=16)
    for x_coordinate in range(110, 410, 30):
        draw.polygon(
            [(x_coordinate, 145), (x_coordinate - 9, 182), (x_coordinate + 9, 182)],
            fill=(42, 110, 47),
        )
    draw.ellipse((250, 235, 330, 275), fill=(45, 115, 135))
    draw.rectangle((60, 100, 460, 120), fill=(180, 186, 178))
    image.save(path)


class UnreadableVersusShadowedTests(unittest.TestCase):
    """D25: the gate must fail crushed blacks and pass shadowed mountains.

    The old gate fired on darkness alone, which made it an argument against
    relief -- the arena went 0.0751 to 0.0940 purely by gaining the mountains
    the art direction asks for. These two cases are the reason the replacement
    is allowed to be believed: without the negative one, a gate that never fails
    would look identical to a gate that works.
    """

    def test_a_shadowed_world_passes_and_a_crushed_one_fails(self) -> None:
        zone = {"zone": {"id": "synthetic"}}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shadowed, crushed = root / "shadowed.png", root / "crushed.png"
            _shaded_world(shadowed, structured=True)
            _shaded_world(crushed, structured=False)

            lit = evaluate(zone, shadowed)["metrics"]
            dead = evaluate(zone, crushed)["metrics"]

            # Precondition: this is only a fair test if both are genuinely dark.
            # If the shadowed world were merely brighter, passing it would prove
            # nothing about structure.
            self.assertGreater(lit["dark_foreground_fraction"], 0.5)
            self.assertGreater(dead["dark_foreground_fraction"], 0.5)

            self.assertLessEqual(lit["unreadable_fraction"], UNREADABLE_LIMIT)
            self.assertGreater(dead["unreadable_fraction"], UNREADABLE_LIMIT)
            self.assertGreater(lit["dark_structured_fraction"], 0.9)
            self.assertLess(dead["dark_structured_fraction"], 0.1)

            self.assertNotIn(
                "Compiled world contains crushed, unreadable pixels",
                evaluate(zone, shadowed)["failures"],
            )
            self.assertIn(
                "Compiled world contains crushed, unreadable pixels",
                evaluate(zone, crushed)["failures"],
            )

    def test_darkness_alone_no_longer_fails_the_gate(self) -> None:
        """The specific regression D25 exists to prevent.

        A world can now be more than 8% dark -- the old hard limit -- without
        failing, provided the dark is structured. If this test ever starts
        failing, the gate has gone back to charging worlds for having mountains.
        """
        zone = {"zone": {"id": "synthetic"}}
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "shadowed.png"
            _shaded_world(path, structured=True)
            report = evaluate(zone, path)
            self.assertGreater(report["metrics"]["dark_foreground_fraction"], 0.08)
            self.assertEqual([], report["failures"])


if __name__ == "__main__":
    unittest.main()
