"""The Gaea seam: heightfield import and `.terrain` graph editing.

Both modules shipped untested, and both carried defects that only show up in the
consumer rather than at the point they happen:

- Previews at Gaea-native resolutions cannot be meshed. The viewer decimates by
  `TERRAIN_STRIDE` and refuses any grid where `(resolution - 1) % 4`; Gaea only
  ever builds powers of two, and `2**k - 1` is never divisible by 4. Every
  import was writing a file no consumer could open.
- Min/max normalisation stretched whatever the build happened to occupy to the
  full `--relief-m`, so two builds of one graph differing threefold in height
  imported to the same world -- the exact silent rescale the module's own
  docstring says it exists to prevent.

The cases below are written against those two failures specifically.
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

import gaea_terrain  # noqa: E402
import import_heightfield as importer  # noqa: E402


def ramp(side: int, peak_fraction: float) -> np.ndarray:
    """A ramp occupying `peak_fraction` of the 16-bit range."""
    unit = np.linspace(0.0, 1.0, side * side).reshape(side, side)
    return unit * peak_fraction * 65535.0


class HeightfieldImportTest(unittest.TestCase):
    def setUp(self) -> None:
        self._directory = tempfile.TemporaryDirectory()
        self.root = Path(self._directory.name)
        self.addCleanup(self._directory.cleanup)

    def png16(self, name: str, data: np.ndarray) -> Path:
        path = self.root / name
        Image.fromarray(data.astype("<u2")).save(path)
        return path

    def manifest_at(self, output: Path) -> dict:
        return json.loads((output / "terrain" / "terrain_manifest.json").read_text())

    # --- resolution -------------------------------------------------------

    def test_stride_compatible_lifts_powers_of_two(self) -> None:
        expected = {512: 513, 1024: 1025, 2048: 2049, 4096: 4097, 1025: 1025, 513: 513}
        for source, target in expected.items():
            with self.subTest(source=source):
                self.assertEqual(importer.stride_compatible(source), target)

    def test_gaea_native_resolutions_import_to_a_meshable_grid(self) -> None:
        for side in (512, 1024):
            with self.subTest(side=side):
                source = self.png16(f"height_{side}.png", ramp(side, 1.0))
                output = self.root / f"out_{side}"
                importer.import_heightfield(
                    source, output, world_m=800.0, relief_m=300.0
                )

                manifest = self.manifest_at(output)
                resolution = manifest["resolution"]
                self.assertEqual(
                    (resolution - 1) % importer.VIEWER_TERRAIN_STRIDE,
                    0,
                    f"{resolution} is not meshable by the viewer",
                )
                self.assertEqual(manifest["imported_from"]["source_resolution"], side)
                self.assertTrue(manifest["imported_from"]["resampled"])

                raw = (output / "terrain" / "heightfield_f32le.bin").read_bytes()
                self.assertEqual(len(raw), resolution * resolution * 4)

    # --- vertical mapping -------------------------------------------------

    def test_full_scale_mapping_preserves_relative_height(self) -> None:
        tall, _ = importer.read_heightfield(
            self.png16("tall.png", ramp(64, 0.9)), None, normalize=False
        )
        short, _ = importer.read_heightfield(
            self.png16("short.png", ramp(64, 0.3)), None, normalize=False
        )
        self.assertAlmostEqual(float(tall.max()), 0.9, places=3)
        self.assertAlmostEqual(float(short.max()), 0.3, places=3)
        self.assertLess(short.max(), tall.max())

    def test_normalize_is_opt_in_and_recorded(self) -> None:
        source = self.png16("short.png", ramp(64, 0.3))

        stretched, mapping = importer.read_heightfield(source, None, normalize=True)
        self.assertEqual(mapping, "min_max")
        self.assertAlmostEqual(float(stretched.max()), 1.0, places=6)

        _, default_mapping = importer.read_heightfield(source, None, normalize=False)
        self.assertEqual(default_mapping, "full_scale")

    def test_manifest_declares_every_input_that_moves_the_geometry(self) -> None:
        """Determinism is graded over equal *declared* inputs, so the mapping
        and the resample belong in the manifest or the digest is a half-truth:
        one image under the two rules is two different worlds."""
        source = self.png16("height.png", ramp(512, 0.8))
        output = self.root / "out"
        importer.import_heightfield(source, output, world_m=800.0, relief_m=300.0)

        declared = self.manifest_at(output)["imported_from"]
        for field in (
            "sha256",
            "relief_m",
            "world_m",
            "vertical_mapping",
            "source_resolution",
            "resampled",
            "importer_version",
        ):
            with self.subTest(field=field):
                self.assertIn(field, declared)

    def test_import_is_reproducible(self) -> None:
        source = self.png16("height.png", ramp(512, 0.8))
        digests = []
        for run in ("a", "b"):
            output = self.root / run
            importer.import_heightfield(source, output, world_m=800.0, relief_m=300.0)
            digests.append(self.manifest_at(output)["heightfield_sha256"])
        self.assertEqual(digests[0], digests[1])

    def test_relief_assertion_sets_the_height_range(self) -> None:
        source = self.png16("height.png", ramp(512, 1.0))
        output = self.root / "out"
        importer.import_heightfield(source, output, world_m=800.0, relief_m=250.0)
        self.assertAlmostEqual(
            self.manifest_at(output)["height_range_m"]["max"], 250.0, places=1
        )

    # --- input validation -------------------------------------------------

    def test_non_square_input_is_refused(self) -> None:
        """A 32x64 image wrote a 32^2 manifest over a 2048-float buffer: wrong
        at write time, surfacing much later as a byte-count error."""
        source = self.png16("wide.png", ramp(64, 1.0)[:32])
        with self.assertRaisesRegex(SystemExit, "square"):
            importer.read_heightfield(source, None, normalize=False)

    def test_flat_input_is_refused(self) -> None:
        source = self.png16("flat.png", np.full((64, 64), 8192.0))
        with self.assertRaisesRegex(SystemExit, "flat"):
            importer.read_heightfield(source, None, normalize=False)

    def test_preview_is_marked_and_carries_no_certification(self) -> None:
        """A preview must stay refusable by the compiled-world path. The viewer
        binds a terrain manifest to a ZoneSpec digest; a preview has none and so
        cannot impersonate a certified world."""
        source = self.png16("height.png", ramp(512, 0.8))
        output = self.root / "out"
        importer.import_heightfield(source, output, world_m=800.0, relief_m=300.0)

        manifest = self.manifest_at(output)
        self.assertIs(manifest["preview"], True)
        self.assertNotIn("zone_spec_sha256", manifest)


class GaeaGraphTest(unittest.TestCase):
    def graph(self) -> dict:
        return {
            "$id": "1",
            "Nodes": {
                "$id": "2",
                "$values": [
                    {"$id": "3", "Id": 720, "Name": "Erosion2", "Position": {"X": 0}},
                    {"$id": "4", "Id": 834, "Name": "Rivers", "Position": {"X": 1}},
                ],
            },
        }

    def test_add_save_definition_marks_the_requested_node(self) -> None:
        marked = gaea_terrain.add_save_definition(self.graph(), 834, "Height")
        self.assertEqual(marked["Name"], "Rivers")
        self.assertEqual(marked["SaveDefinition"]["Format"], gaea_terrain.HEIGHT_FORMAT)
        self.assertIs(marked["SaveDefinition"]["IsEnabled"], True)

    def test_inserted_reference_ids_do_not_collide(self) -> None:
        """Newtonsoft resolves `$ref` by exact string match against `$id`, so a
        duplicate silently aliases two different objects."""
        graph = self.graph()
        gaea_terrain.add_save_definition(graph, 720, "Height")
        gaea_terrain.add_save_definition(graph, 834, "Height")

        identifiers = [
            node["$id"]
            for node in gaea_terrain.walk(graph)
            if isinstance(node.get("$id"), str)
        ]
        self.assertEqual(len(identifiers), len(set(identifiers)))

    def test_unknown_node_is_refused(self) -> None:
        with self.assertRaisesRegex(SystemExit, "no node with Id"):
            gaea_terrain.add_save_definition(self.graph(), 999, "Height")

    def test_nodes_are_found_without_relying_on_type_strings(self) -> None:
        """Gaea's `$type` values carry assembly-qualified names that differ
        between builds, so identification has to be structural."""
        self.assertEqual(
            {node["Id"] for node in gaea_terrain.nodes(self.graph())}, {720, 834}
        )


if __name__ == "__main__":
    unittest.main()
