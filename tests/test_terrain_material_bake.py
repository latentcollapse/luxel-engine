from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from terrain_material_bake import bake_material_preview


class TerrainMaterialBakeTests(unittest.TestCase):
    def test_bake_uses_splat_and_wetland_contracts(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            materials = {}
            colors = {
                "grass": (0, 200, 0),
                "road": (180, 100, 20),
                "rock": (100, 100, 100),
                "snow": (240, 240, 240),
                "wetland": (20, 40, 10),
            }
            for layer, color in colors.items():
                directory = root / layer
                directory.mkdir()
                Image.new("RGB", (2, 2), color).save(directory / "albedo.png")
                Image.new("L", (2, 2), 204).save(directory / "roughness.png")
                materials[layer] = {
                    "meters_per_repeat": 2.0,
                    "maps": {
                        "albedo": {"path": f"{layer}/albedo.png"},
                        "roughness": {"path": f"{layer}/roughness.png"},
                    },
                }

            splat = np.zeros((3, 3, 4), dtype=np.uint8)
            splat[:, :, 0] = 255
            splat[0, 0] = [0, 255, 0, 0]
            wetland = np.zeros((3, 3), dtype=np.uint8)
            wetland[2, 2] = 255
            result = bake_material_preview(
                project_root=root,
                output_dir=root / "out",
                splat_rgba=splat,
                wetland_mask=wetland,
                materials=materials,
                width_m=4.0,
                length_m=4.0,
            )
            with Image.open(root / "out" / result["albedo"]) as image:
                pixels = np.asarray(image)
            self.assertTupleEqual(tuple(pixels[0, 0]), colors["road"])
            self.assertGreater(int(pixels[1, 1, 1]), 190)
            self.assertLess(int(pixels[2, 2, 1]), 100)
            self.assertTrue(
                (root / "out" / result["metallic_roughness"]).is_file()
            )


if __name__ == "__main__":
    unittest.main()
