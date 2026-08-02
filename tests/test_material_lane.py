from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

import material_lane
import material_catalog
from material_lane import MaterialRequest, ensure_material
from texture_material_pipeline import TextureMaterialError


def _seamless_noise_image(path: Path, seed: int = 42, size: int = 256) -> Path:
    # Band-limited noise built from an FFT low-pass is periodic by
    # construction, so it tiles cleanly and has broad, low-frequency
    # variation -- exactly what assess() wants: real tonal spread without the
    # spiky per-pixel energy that its line/grid and dominant-spectral-mode
    # gates are built to reject.
    rng = np.random.default_rng(seed)
    white = rng.normal(0.0, 1.0, (size, size))
    frequency_y = np.fft.fftfreq(size)[:, None]
    frequency_x = np.fft.fftfreq(size)[None, :]
    radius_squared = frequency_x**2 + frequency_y**2
    low_pass = np.exp(-0.5 * radius_squared * (20.0**2))
    field = np.fft.ifft2(np.fft.fft2(white) * low_pass).real
    field = (field - field.mean()) / max(field.std(), 1e-6)
    value = np.clip(0.5 + field * 0.18, 0.05, 0.95)
    rgb = np.stack([value, value * 0.95, value * 0.85], axis=-1)
    pixels = np.clip(rgb * 255.0, 0, 255).astype(np.uint8)
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(pixels, mode="RGB").save(path)
    return path


class FileProviderTests(unittest.TestCase):
    def test_file_provider_produces_expected_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = _seamless_noise_image(root / "source.png")
            request = MaterialRequest(material_id="test_stone", provider="file", provider_args={"path": str(source)})
            manifest = ensure_material(request, root)
            self.assertIn(manifest["status"], {"accepted", "accepted_with_warnings"})
            bundle_dir = root / "generated" / "codeweald_materials" / "test_stone"
            for name in ("material_manifest.json", "albedo.png", "normal.png", "roughness.png", "provenance.json"):
                self.assertTrue((bundle_dir / name).is_file(), name)
            provenance = json.loads((bundle_dir / "provenance.json").read_text(encoding="utf-8"))
            self.assertEqual(provenance["provider"], "file")
            self.assertEqual(manifest["provenance"]["digest"], provenance["digest"])

    def test_second_call_with_same_request_does_not_regenerate(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = _seamless_noise_image(root / "source.png")
            request = MaterialRequest(material_id="test_stone", provider="file", provider_args={"path": str(source)})
            with patch.object(material_lane, "materialize", wraps=material_lane.materialize) as spy:
                ensure_material(request, root)
                ensure_material(request, root)
                self.assertEqual(spy.call_count, 1)

    def test_changing_provider_args_changes_digest_and_regenerates(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source_a = _seamless_noise_image(root / "a.png", seed=1)
            source_b = _seamless_noise_image(root / "b.png", seed=2)
            request_a = MaterialRequest(material_id="test_stone", provider="file", provider_args={"path": str(source_a)})
            request_b = MaterialRequest(material_id="test_stone", provider="file", provider_args={"path": str(source_b)})
            ensure_material(request_a, root)
            with patch.object(material_lane, "materialize", wraps=material_lane.materialize) as spy:
                ensure_material(request_b, root)
                self.assertEqual(spy.call_count, 1)
            bundle_dir = root / "generated" / "codeweald_materials" / "test_stone"
            provenance = json.loads((bundle_dir / "provenance.json").read_text(encoding="utf-8"))
            self.assertEqual(provenance["provider_args"]["path"], str(source_b))

    def test_force_regenerates_even_when_digest_matches(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = _seamless_noise_image(root / "source.png")
            request = MaterialRequest(material_id="test_stone", provider="file", provider_args={"path": str(source)})
            ensure_material(request, root)
            with patch.object(material_lane, "materialize", wraps=material_lane.materialize) as spy:
                ensure_material(request, root, force=True)
                self.assertEqual(spy.call_count, 1)

    def test_bundle_is_accepted_by_material_catalog(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = _seamless_noise_image(root / "source.png")
            request = MaterialRequest(material_id="test_stone", provider="file", provider_args={"path": str(source)})
            ensure_material(request, root)
            resolved = material_catalog.resolve_terrain_materials({"rock": "test_stone"}, project_root=root, asset_root=root)
            self.assertEqual(resolved["rock"]["material_id"], "test_stone")


class ProviderDispatchTests(unittest.TestCase):
    def test_unknown_provider_raises_with_valid_provider_names(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            request = MaterialRequest(material_id="test_stone", provider="paintbrush", provider_args={})
            with self.assertRaises(TextureMaterialError) as context:
                ensure_material(request, root)
            message = str(context.exception)
            self.assertIn("procedural", message)
            self.assertIn("comfyui", message)
            self.assertIn("file", message)

    def test_procedural_provider_calls_granite_generator(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pixels = np.zeros((256, 256, 3), dtype=np.uint8)
            pixels[:, :] = 128
            with patch("material_lane.generate_granite_material.generate", return_value=pixels) as spy:
                request = MaterialRequest(
                    material_id="test_procedural",
                    provider="procedural",
                    provider_args={"seed": 7, "size": 256},
                )
                ensure_material(request, root)
                spy.assert_called_once_with(seed=7, size=256)

    def test_comfyui_provider_calls_comfy_generate_texture(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)

            def fake_generate(server, output, prompt, negative, seed, size, checkpoint):
                _seamless_noise_image(output, seed=seed, size=256)
                return output

            with patch("material_lane.comfy_generate_texture.generate", side_effect=fake_generate) as spy:
                request = MaterialRequest(
                    material_id="test_comfy",
                    provider="comfyui",
                    provider_args={
                        "server": "http://127.0.0.1:8188",
                        "prompt": "granite cliff",
                        "negative": "text, logo",
                        "seed": 3,
                        "size": 256,
                        "checkpoint": "sd_xl_base_1.0.safetensors",
                    },
                )
                ensure_material(request, root)
                spy.assert_called_once()


if __name__ == "__main__":
    unittest.main()
