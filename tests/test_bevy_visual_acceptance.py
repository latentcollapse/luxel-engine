from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

from PIL import Image, ImageDraw


PIPELINE = Path(__file__).resolve().parents[1] / "pipeline"
sys.path.insert(0, str(PIPELINE))

from bevy_visual_acceptance import evaluate


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


if __name__ == "__main__":
    unittest.main()
