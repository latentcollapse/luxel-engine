"""Cutting mountains out of a heightfield.

The load-bearing case here is `test_mesh_is_a_closed_manifold`. A crop is buried
under the world skirt and may carry collision, and an open sheet is see-through
from below and from any grazing angle -- the same class of defect as terrain
that stops at the data edge. Every other property of a crop is negotiable; that
one is not.

The taper cases matter for a subtler reason. The first crops cut from real Gaea
terrain rendered as rectangular blocks with sheer sides, because a square window
leaves the perimeter at whatever height the terrain happened to be. Burying the
base does not help, since the cut faces are *above* ground. These pin the fix.
"""

from __future__ import annotations

import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

import gaea_crop  # noqa: E402


def cone(side: int, height: float = 100.0, radius: float = 0.35) -> np.ndarray:
    """A compact summit standing on a flat plain.

    The plain matters. On a surface of *uniform* slope -- a cone filling the
    whole grid -- `height - min(height within window)` is the same everywhere on
    the flank, so prominence is flat and the summit is not distinguishable from
    the slope below it. Real terrain is not uniformly sloped, but the fixture
    has to avoid that degenerate case or it tests nothing.
    """
    axis = np.linspace(-1.0, 1.0, side, dtype=np.float32)
    grid_x, grid_z = np.meshgrid(axis, axis)
    return (
        height * np.clip(1.0 - np.hypot(grid_x, grid_z) / radius, 0.0, 1.0)
    ).astype(np.float32)


def read_glb(path: Path) -> tuple[dict, np.ndarray, np.ndarray, np.ndarray]:
    raw = path.read_bytes()
    magic, version, total = struct.unpack("<III", raw[:12])
    assert magic == 0x46546C67 and version == 2
    assert total == len(raw)
    offset, chunks = 12, {}
    while offset < len(raw):
        length, kind = struct.unpack("<II", raw[offset : offset + 8])
        offset += 8
        chunks[kind] = raw[offset : offset + length]
        offset += length
    document = json.loads(chunks[0x4E4F534A])
    binary = chunks[0x004E4942]

    def accessor(index: int, dtype: str, components: int) -> np.ndarray:
        spec = document["accessors"][index]
        view = document["bufferViews"][spec["bufferView"]]
        values = np.frombuffer(
            binary,
            dtype=dtype,
            count=spec["count"] * components,
            offset=view["byteOffset"],
        )
        return values.reshape(-1, components) if components > 1 else values

    return (
        document,
        accessor(0, "<u4", 1),
        accessor(1, "<f4", 3),
        accessor(2, "<f4", 3),
    )


class PeakFindingTest(unittest.TestCase):
    def test_the_summit_of_a_cone_is_the_most_prominent_point(self) -> None:
        heights = cone(129)
        peaks = gaea_crop.find_peaks(heights, window=41, count=1, separation=20)
        row, column, prominence = peaks[0]
        self.assertAlmostEqual(row, 64, delta=2)
        self.assertAlmostEqual(column, 64, delta=2)
        self.assertGreater(prominence, 0.0)

    def test_crops_are_kept_clear_of_the_grid_edge(self) -> None:
        """A window centred too near the edge would run off the heightfield."""
        heights = np.random.default_rng(3).random((129, 129)).astype(np.float32)
        window = 41
        peaks = gaea_crop.find_peaks(heights, window=window, count=6, separation=8)
        for row, column, _ in peaks:
            self.assertGreaterEqual(min(row, column), window // 2)
            self.assertLess(max(row, column), 129 - window // 2)

    def test_peaks_are_separated(self) -> None:
        heights = np.random.default_rng(5).random((257, 257)).astype(np.float32)
        peaks = gaea_crop.find_peaks(heights, window=41, count=6, separation=30)
        for index, (row, column, _) in enumerate(peaks):
            for other_row, other_column, _ in peaks[index + 1 :]:
                self.assertTrue(
                    abs(row - other_row) >= 30 or abs(column - other_column) >= 30,
                    "two crops centred within the separation distance",
                )

    def test_uniform_slope_gives_no_usable_prominence(self) -> None:
        """Documents the metric's limitation rather than hiding it: on a cone
        filling the grid, every point on the flank scores the same, so crop
        placement there is arbitrary and the operator has to look."""
        axis = np.linspace(-1.0, 1.0, 129, dtype=np.float32)
        grid_x, grid_z = np.meshgrid(axis, axis)
        uniform = (100.0 * np.clip(1.0 - np.hypot(grid_x, grid_z), 0.0, 1.0)).astype(
            np.float32
        )
        peaks = gaea_crop.find_peaks(uniform, window=41, count=4, separation=10)
        spread = max(p for _, _, p in peaks) - min(p for _, _, p in peaks)
        self.assertLess(spread, 2.0, "expected prominence to be near-flat here")

    def test_peaks_are_ranked_by_prominence(self) -> None:
        heights = np.random.default_rng(7).random((257, 257)).astype(np.float32)
        peaks = gaea_crop.find_peaks(heights, window=33, count=5, separation=20)
        prominences = [prominence for _, _, prominence in peaks]
        self.assertEqual(prominences, sorted(prominences, reverse=True))


class CropAndTaperTest(unittest.TestCase):
    def test_stride_decimates_without_moving_the_centre(self) -> None:
        heights = cone(129)
        full = gaea_crop.crop_region(heights, 64, 64, 41, stride=1)
        thinned = gaea_crop.crop_region(heights, 64, 64, 41, stride=2)
        self.assertEqual(full.shape, (41, 41))
        self.assertEqual(thinned.shape, (21, 21))
        self.assertAlmostEqual(float(full[0, 0]), float(thinned[0, 0]), places=5)

    def test_taper_brings_the_perimeter_down_to_the_floor(self) -> None:
        """The whole point: no perimeter above the floor means no cut face."""
        patch = cone(65) + 20.0
        tapered = gaea_crop.taper_edges(patch, falloff=0.3)
        floor = float(patch.min())
        border = np.concatenate(
            [tapered[0, :], tapered[-1, :], tapered[:, 0], tapered[:, -1]]
        )
        self.assertLess(float(np.abs(border - floor).max()), 1e-4)

    def test_taper_leaves_the_summit_alone(self) -> None:
        """A falloff applied across the whole patch would round the massif into
        a dome and throw away the ridgelines that were the reason to cut here."""
        patch = cone(65)
        tapered = gaea_crop.taper_edges(patch, falloff=0.28)
        self.assertAlmostEqual(float(tapered.max()), float(patch.max()), places=4)

    def test_taper_can_be_disabled(self) -> None:
        patch = cone(33)
        self.assertTrue(np.array_equal(gaea_crop.taper_edges(patch, 0.0), patch))


class MeshTest(unittest.TestCase):
    def mesh(self, side: int = 33, skirt: float = 30.0):
        return gaea_crop.build_mesh(cone(side), cell_m=2.0, skirt_depth_m=skirt)

    def test_mesh_is_a_closed_manifold(self) -> None:
        """Every edge shared by exactly two triangles. An open sheet is
        see-through from below, and a crop is meant to be buried."""
        _, _, indices = self.mesh()
        triangles = indices.reshape(-1, 3)
        edges = np.sort(
            np.concatenate(
                [triangles[:, [0, 1]], triangles[:, [1, 2]], triangles[:, [2, 0]]]
            ),
            axis=1,
        )
        _, counts = np.unique(edges, axis=0, return_counts=True)
        self.assertEqual(int((counts != 2).sum()), 0, "mesh has boundary edges")

    def test_the_floor_sits_at_zero_and_the_walls_hang_below(self) -> None:
        """Placement puts the base on the ground; burial is a negative offset."""
        positions, _, _ = self.mesh(skirt=30.0)
        self.assertAlmostEqual(float(positions[:, 1].min()), -30.0, places=4)
        surface = positions[: 33 * 33, 1]
        self.assertAlmostEqual(float(surface.min()), 0.0, places=4)

    def test_the_footprint_matches_the_requested_cell_size(self) -> None:
        positions, _, _ = self.mesh(side=33)
        span = float(positions[:, 0].max() - positions[:, 0].min())
        self.assertAlmostEqual(span, 32 * 2.0, places=3)

    def test_normals_are_unit_length(self) -> None:
        _, normals, _ = self.mesh()
        lengths = np.linalg.norm(normals, axis=1)
        self.assertLess(float(np.abs(lengths - 1.0).max()), 1e-5)

    def test_indices_stay_in_range(self) -> None:
        positions, _, indices = self.mesh()
        self.assertLess(int(indices.max()), positions.shape[0])
        self.assertEqual(indices.size % 3, 0)


class GlbTest(unittest.TestCase):
    def setUp(self) -> None:
        self._directory = tempfile.TemporaryDirectory()
        self.root = Path(self._directory.name)
        self.addCleanup(self._directory.cleanup)

    def test_written_glb_parses_and_declares_true_bounds(self) -> None:
        positions, normals, indices = gaea_crop.build_mesh(cone(33), 2.0, 20.0)
        path = self.root / "m.glb"
        gaea_crop.write_glb(path, positions, normals, indices, "m")

        document, read_indices, read_positions, read_normals = read_glb(path)
        self.assertEqual(read_positions.shape, positions.shape)
        self.assertEqual(read_normals.shape, normals.shape)
        self.assertEqual(read_indices.shape[0], indices.size)
        # Readers use the declared bounds without decoding the buffer, so a
        # wrong min/max is a culling bug that only shows at certain angles.
        bounds = document["accessors"][1]
        self.assertTrue(np.allclose(bounds["min"], positions.min(axis=0)))
        self.assertTrue(np.allclose(bounds["max"], positions.max(axis=0)))

    def test_glb_length_field_matches_the_file(self) -> None:
        positions, normals, indices = gaea_crop.build_mesh(cone(17), 1.0, 5.0)
        path = self.root / "m.glb"
        gaea_crop.write_glb(path, positions, normals, indices, "m")
        _, _, declared_total = struct.unpack("<III", path.read_bytes()[:12])
        self.assertEqual(declared_total, path.stat().st_size)


class EndToEndTest(unittest.TestCase):
    def setUp(self) -> None:
        self._directory = tempfile.TemporaryDirectory()
        self.root = Path(self._directory.name)
        self.addCleanup(self._directory.cleanup)
        self.source = self.root / "height.bin"
        cone(257, height=180.0).tofile(self.source)

    def cut(self, output: str, **overrides) -> dict:
        options = {
            "world_m": 256.0,
            "window_m": 60.0,
            "count": 3,
            "skirt_depth_m": 25.0,
            "stride": 2,
        }
        options.update(overrides)
        return gaea_crop.crop_mountains(self.source, self.root / output, **options)

    def test_cuts_the_requested_crops_and_writes_them(self) -> None:
        manifest = self.cut("out")
        self.assertEqual(len(manifest["crops"]), 3)
        for crop in manifest["crops"]:
            self.assertTrue((self.root / "out" / crop["asset"]).is_file())
            self.assertGreater(crop["triangles"], 0)

    def test_manifest_pins_the_source_and_every_shaping_input(self) -> None:
        declared = self.cut("out")
        for field in ("path", "sha256", "world_m", "resolution", "cell_m"):
            self.assertIn(field, declared["cropped_from"])
        for field in ("window_cells", "stride", "edge_falloff", "skirt_depth_m"):
            self.assertIn(field, declared)

    def test_a_crop_is_never_certified(self) -> None:
        """Same refusal `import_heightfield.py` makes: landform is not a world."""
        manifest = self.cut("out")
        self.assertIs(manifest["preview"], True)
        self.assertNotIn("zone_spec_sha256", manifest)
        self.assertNotEqual(
            manifest["schema_version"], "codeweald.terrain-artifacts/v1"
        )

    def test_cutting_twice_produces_identical_bytes(self) -> None:
        first = self.cut("a")
        second = self.cut("b")
        self.assertEqual(
            [crop["sha256"] for crop in first["crops"]],
            [crop["sha256"] for crop in second["crops"]],
        )

    def test_a_window_larger_than_the_heightfield_is_refused(self) -> None:
        with self.assertRaisesRegex(SystemExit, "does not fit"):
            self.cut("out", window_m=400.0)

    def test_stride_reduces_triangles_without_changing_the_footprint(self) -> None:
        fine = self.cut("fine", stride=1)["crops"][0]
        coarse = self.cut("coarse", stride=4)["crops"][0]
        self.assertLess(coarse["triangles"], fine["triangles"])
        self.assertAlmostEqual(
            coarse["footprint_m"], fine["footprint_m"], delta=fine["footprint_m"] * 0.2
        )


if __name__ == "__main__":
    unittest.main()
