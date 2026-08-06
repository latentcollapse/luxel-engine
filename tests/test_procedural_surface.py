from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

import material_catalog
import procedural_surface
from material_lane import MaterialRequest, ensure_material
from texture_material_pipeline import MINIMUM_COARSE_CONTRAST, assess


# Total contrast budget shared by every band-placement comparison below, so a
# test failure can only be attributed to *where* the contrast sits (macro vs
# fine), never to one variant simply having more of it than the other.
_DEFAULT_TOTAL_CONTRAST = 0.070 + 0.043 + 0.018


def _luminance(pixels: np.ndarray) -> np.ndarray:
    return (pixels.astype(np.float32) / 255.0) @ np.array(
        [0.2126, 0.7152, 0.0722], dtype=np.float32
    )


class DeterminismTests(unittest.TestCase):
    def test_same_seed_is_byte_identical(self) -> None:
        first = procedural_surface.generate(size=256, seed=17)
        second = procedural_surface.generate(size=256, seed=17)
        np.testing.assert_array_equal(first, second)

    def test_different_seed_differs(self) -> None:
        first = procedural_surface.generate(size=256, seed=17)
        second = procedural_surface.generate(size=256, seed=18)
        self.assertFalse(np.array_equal(first, second))


class SeamlessTests(unittest.TestCase):
    def test_wrap_edges_are_comparable_to_interior_steps(self) -> None:
        pixels = procedural_surface.generate(size=256, seed=5).astype(np.float32) / 255.0
        interior_x = float(np.abs(pixels[:, 1:] - pixels[:, :-1]).mean())
        interior_y = float(np.abs(pixels[1:, :] - pixels[:-1, :]).mean())
        seam_x = float(np.abs(pixels[:, 0] - pixels[:, -1]).mean())
        seam_y = float(np.abs(pixels[0, :] - pixels[-1, :]).mean())
        baseline = max((interior_x + interior_y) * 0.5, 1e-5)
        # assess()'s own seam gate fails above 2.25x and warns above 1.55x; a
        # tile built on the same FFT construction as the accepted granite
        # generator should sit well inside the passing range, not merely
        # under the failure line.
        self.assertLessEqual(max(seam_x, seam_y) / baseline, 1.55)

    def test_assess_reports_no_seam_failure(self) -> None:
        pixels = procedural_surface.generate(size=256, seed=5)
        report = assess(pixels.astype(np.float32) / 255.0)
        self.assertNotIn(
            "opposite texture edges have a visible tiling discontinuity",
            report["failures"],
        )


class CoarseContrastSurvivabilityTests(unittest.TestCase):
    """The property this module exists for: only bands coarser than the
    minification texel count keep a surface visible at overview distance."""

    def test_coarse_macro_band_clears_the_floor(self) -> None:
        pixels = procedural_surface.generate(size=512, seed=11)
        report = assess(pixels.astype(np.float32) / 255.0)
        self.assertGreaterEqual(report["metrics"]["coarse_contrast"], MINIMUM_COARSE_CONTRAST)
        self.assertNotIn(
            "texture flattens to a single colour under minification; only "
            "%.4f contrast survives past 36 texels per pixel"
            % report["metrics"]["coarse_contrast"],
            report["warnings"],
        )

    def test_same_total_contrast_concentrated_in_fine_band_flattens(self) -> None:
        macro_placed = procedural_surface.generate(size=512, seed=11)
        fine_placed = procedural_surface.generate(
            size=512,
            seed=11,
            macro_contrast=0.0,
            meso_contrast=0.0,
            fine_contrast=_DEFAULT_TOTAL_CONTRAST,
            fine_scale_px=3.5,
        )
        macro_report = assess(macro_placed.astype(np.float32) / 255.0)
        fine_report = assess(fine_placed.astype(np.float32) / 255.0)

        self.assertGreaterEqual(macro_report["metrics"]["coarse_contrast"], MINIMUM_COARSE_CONTRAST)
        self.assertLess(fine_report["metrics"]["coarse_contrast"], MINIMUM_COARSE_CONTRAST)

        self.assertFalse(
            any("flattens to a single colour" in warning for warning in macro_report["warnings"])
        )
        self.assertTrue(
            any("flattens to a single colour" in warning for warning in fine_report["warnings"])
        )


class AssessStatusTests(unittest.TestCase):
    def test_default_surface_never_fails_assess(self) -> None:
        pixels = procedural_surface.generate(size=512, seed=3)
        report = assess(pixels.astype(np.float32) / 255.0)
        self.assertIn(report["status"], {"passed", "warnings"})


class MaterialLaneIntegrationTests(unittest.TestCase):
    def test_surface_provider_is_reachable_and_catalog_accepts_it(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            # A default-scale surface needs enough pixels for its 92px macro
            # band to resolve into more than a handful of Fourier modes; at
            # 256px that band is nearly the whole tile and assess() correctly
            # rejects it as a repeated spectral motif, so this uses the same
            # 1024px default the CLI and other providers materialize at.
            request = MaterialRequest(
                material_id="test_surface",
                provider="surface",
                provider_args={"seed": 9, "size": 1024},
            )
            manifest = ensure_material(request, root)
            self.assertIn(manifest["status"], {"accepted", "accepted_with_warnings"})
            resolved = material_catalog.resolve_terrain_materials(
                {"grass": "test_surface"}, project_root=root, asset_root=root
            )
            self.assertEqual(resolved["grass"]["material_id"], "test_surface")


if __name__ == "__main__":
    unittest.main()
