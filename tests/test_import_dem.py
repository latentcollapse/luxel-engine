"""Real-world elevation ingest.

The case that earns its keep is `test_geographic_pixels_are_converted_by_latitude`.
A DEM in a geographic CRS has its pixel scale in *degrees*, and a degree of
longitude shrinks with latitude. Reading degrees as metres makes terrain ~100000x
too small, which is obvious. Applying the latitude conversion to only one axis --
or to neither -- yields terrain stretched by `1/cos(latitude)`, which at 49 deg
is a 52% error in one axis and produces a perfectly plausible-looking valley.
That is the failure worth a test: the one that still looks right.

The refusal cases matter for the same reason. A DEM that does not declare its
units is refused rather than guessed at, because a wrong guess in the metres
direction is invisible.
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image
from PIL.TiffImagePlugin import ImageFileDirectory_v2

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

import import_dem  # noqa: E402

ARCSECOND = 1.0 / 3600.0


def write_geotiff(
    path: Path,
    side: int = 129,
    scale: float = 10.0,
    model_type: int | None = import_dem.MODEL_PROJECTED,
    latitude: float = 49.0,
    relief: float = 400.0,
    base: float = 1200.0,
    voids: bool = False,
) -> Path:
    axis = np.linspace(-1.0, 1.0, side)
    grid_x, grid_y = np.meshgrid(axis, axis)
    # A U-valley: flat floor, steep walls, running north-south.
    wall = np.clip((np.abs(grid_x) - 0.25) / 0.75, 0.0, 1.0) ** 1.6
    dem = (base + relief * (wall + 0.12 * np.sin(grid_y * 3.0))).astype(np.float32)
    if voids:
        dem[10:14, 10:14] = -32768.0

    directory = ImageFileDirectory_v2()
    directory[import_dem.MODEL_PIXEL_SCALE] = (scale, scale, 0.0)
    directory[import_dem.MODEL_TIEPOINT] = (0.0, 0.0, 0.0, 300000.0, latitude, 0.0)
    directory.tagtype[import_dem.MODEL_PIXEL_SCALE] = 12
    directory.tagtype[import_dem.MODEL_TIEPOINT] = 12
    if model_type is not None:
        directory[import_dem.GEO_KEY_DIRECTORY] = (1, 1, 0, 1, 1024, 0, 1, model_type)
        directory.tagtype[import_dem.GEO_KEY_DIRECTORY] = 3
    Image.fromarray(dem, mode="F").save(path, tiffinfo=directory)
    return path


class GroundSampleTest(unittest.TestCase):
    def setUp(self) -> None:
        self._directory = tempfile.TemporaryDirectory()
        self.root = Path(self._directory.name)
        self.addCleanup(self._directory.cleanup)

    def test_projected_pixels_are_already_metres(self) -> None:
        source = write_geotiff(self.root / "utm.tif", scale=10.0)
        _, geometry = import_dem.read_geotiff(source)
        x, y, basis = import_dem.ground_sample_metres(geometry, None)
        self.assertAlmostEqual(x, 10.0, places=6)
        self.assertAlmostEqual(y, 10.0, places=6)
        self.assertEqual(basis, "projected")

    def test_geographic_pixels_are_converted_by_latitude(self) -> None:
        """The error that still looks plausible if you get it wrong."""
        source = write_geotiff(
            self.root / "geo.tif",
            scale=ARCSECOND,
            model_type=import_dem.MODEL_GEOGRAPHIC,
            latitude=49.0,
        )
        _, geometry = import_dem.read_geotiff(source)
        x, y, basis = import_dem.ground_sample_metres(geometry, None)

        expected_y = ARCSECOND * import_dem.METRES_PER_DEGREE
        expected_x = expected_y * np.cos(np.radians(49.0))
        self.assertAlmostEqual(y, expected_y, places=3)
        self.assertAlmostEqual(x, expected_x, places=3)
        # The whole point: the axes differ, and by the cosine of the latitude.
        self.assertAlmostEqual(x / y, np.cos(np.radians(49.0)), places=6)
        self.assertIn("geographic", basis)

    def test_latitude_actually_changes_the_answer(self) -> None:
        """A fixed conversion would pass the test above at one latitude only."""
        samples = {}
        for latitude in (0.0, 60.0):
            source = write_geotiff(
                self.root / f"geo{latitude:.0f}.tif",
                scale=ARCSECOND,
                model_type=import_dem.MODEL_GEOGRAPHIC,
                latitude=latitude,
            )
            _, geometry = import_dem.read_geotiff(source)
            samples[latitude] = import_dem.ground_sample_metres(geometry, None)[0]
        # cos(60) is exactly 0.5, so a degree of longitude is half as wide.
        self.assertAlmostEqual(samples[60.0] / samples[0.0], 0.5, places=4)

    def test_an_undeclared_crs_is_refused_rather_than_guessed(self) -> None:
        source = write_geotiff(self.root / "bare.tif", model_type=None)
        _, geometry = import_dem.read_geotiff(source)
        with self.assertRaisesRegex(SystemExit, "units are unknown"):
            import_dem.ground_sample_metres(geometry, None)

    def test_cell_size_can_be_asserted(self) -> None:
        source = write_geotiff(self.root / "bare.tif", model_type=None)
        _, geometry = import_dem.read_geotiff(source)
        x, y, basis = import_dem.ground_sample_metres(geometry, 2.5)
        self.assertEqual((x, y), (2.5, 2.5))
        self.assertEqual(basis, "asserted")

    def test_a_dem_without_pixel_scale_is_refused(self) -> None:
        plain = self.root / "plain.tif"
        Image.fromarray(np.zeros((33, 33), np.float32), mode="F").save(plain)
        with self.assertRaisesRegex(SystemExit, "ModelPixelScale"):
            import_dem.read_geotiff(plain)


class VoidTest(unittest.TestCase):
    def test_sentinels_are_replaced_with_the_local_minimum(self) -> None:
        """A -32768 cell is a hole kilometres deep that would dominate the
        vertical range and make every downstream metre meaningless."""
        elevation = np.full((16, 16), 1000.0)
        elevation[4, 4] = -32768.0
        cleaned = import_dem.clean_voids(elevation, None)
        self.assertGreaterEqual(float(cleaned.min()), 1000.0)
        self.assertEqual(float(cleaned.max()), 1000.0)

    def test_an_explicit_nodata_value_is_honoured(self) -> None:
        elevation = np.full((16, 16), 800.0)
        elevation[2, 2] = -9999.0
        cleaned = import_dem.clean_voids(elevation, -9999.0)
        self.assertEqual(float(cleaned.min()), 800.0)

    def test_plausible_ground_is_left_alone(self) -> None:
        elevation = np.linspace(-400.0, 8800.0, 256).reshape(16, 16)
        self.assertTrue(np.array_equal(import_dem.clean_voids(elevation, None), elevation))

    def test_an_all_void_dem_is_refused(self) -> None:
        with self.assertRaisesRegex(SystemExit, "every cell"):
            import_dem.clean_voids(np.full((8, 8), -32768.0), None)


class IngestTest(unittest.TestCase):
    def setUp(self) -> None:
        self._directory = tempfile.TemporaryDirectory()
        self.root = Path(self._directory.name)
        self.addCleanup(self._directory.cleanup)
        self.source = write_geotiff(self.root / "dem.tif", side=129, scale=10.0)

    def manifest(self, output: Path) -> dict:
        return json.loads((output / "terrain" / "terrain_manifest.json").read_text())

    def test_output_is_meshable_by_the_viewer(self) -> None:
        import_dem.import_dem(self.source, self.root / "out")
        manifest = self.manifest(self.root / "out")
        self.assertEqual((manifest["resolution"] - 1) % import_dem.VIEWER_TERRAIN_STRIDE, 0)
        raw = (self.root / "out" / "terrain" / "heightfield_f32le.bin").read_bytes()
        self.assertEqual(len(raw), manifest["resolution"] ** 2 * 4)

    def test_world_extent_follows_the_ground_sample(self) -> None:
        manifest = import_dem.import_dem(self.source, self.root / "out")
        # 129 cells at 10 m each spans 128 intervals.
        self.assertAlmostEqual(manifest["world_bounds_m"]["width"], 1280.0, places=3)

    def test_side_m_crops_to_the_requested_extent(self) -> None:
        manifest = import_dem.import_dem(self.source, self.root / "out", side_m=400.0)
        self.assertAlmostEqual(manifest["world_bounds_m"]["width"], 400.0, delta=20.0)

    def test_elevation_is_rebased_but_the_datum_is_recorded(self) -> None:
        """Re-basing to zero throws away where on Earth this ground was, so the
        original base elevation is kept rather than silently discarded."""
        manifest = import_dem.import_dem(self.source, self.root / "out")
        self.assertAlmostEqual(manifest["height_range_m"]["min"], 0.0, places=3)
        self.assertGreater(manifest["imported_from"]["base_elevation_m"], 1000.0)

    def test_vertical_exaggeration_scales_relief_only(self) -> None:
        plain = import_dem.import_dem(self.source, self.root / "a")
        tall = import_dem.import_dem(
            self.source, self.root / "b", vertical_exaggeration=2.0
        )
        self.assertAlmostEqual(
            tall["height_range_m"]["max"], plain["height_range_m"]["max"] * 2.0, delta=0.1
        )
        self.assertEqual(
            tall["world_bounds_m"]["width"], plain["world_bounds_m"]["width"]
        )

    def test_provenance_is_declared(self) -> None:
        declared = import_dem.import_dem(self.source, self.root / "out")["imported_from"]
        for field in (
            "sha256",
            "kind",
            "ground_sample_m",
            "ground_sample_basis",
            "base_elevation_m",
            "vertical_exaggeration",
            "source_resolution",
            "resampled",
            "ingest_version",
        ):
            self.assertIn(field, declared)
        self.assertEqual(declared["kind"], "dem")

    def test_a_dem_is_never_certified(self) -> None:
        manifest = import_dem.import_dem(self.source, self.root / "out")
        self.assertIs(manifest["preview"], True)
        self.assertNotIn("zone_spec_sha256", manifest)

    def test_ingest_is_reproducible(self) -> None:
        first = import_dem.import_dem(self.source, self.root / "a")
        second = import_dem.import_dem(self.source, self.root / "b")
        self.assertEqual(first["heightfield_sha256"], second["heightfield_sha256"])

    def test_a_geographic_dem_yields_square_ground_pixels(self) -> None:
        """Non-square pixels are normal in a geographic CRS; the output must not
        inherit the stretch."""
        source = write_geotiff(
            self.root / "geo.tif",
            side=129,
            scale=ARCSECOND,
            model_type=import_dem.MODEL_GEOGRAPHIC,
            latitude=49.0,
        )
        manifest = import_dem.import_dem(source, self.root / "geo")
        bounds = manifest["world_bounds_m"]
        self.assertEqual(bounds["width"], bounds["length"])


if __name__ == "__main__":
    unittest.main()
