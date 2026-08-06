#!/usr/bin/env python3
"""Generate a deterministic seamless Highland-granite baseline material."""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
from PIL import Image

from texture_material_pipeline import materialize


def _periodic_noise(
    size: int,
    rng: np.random.Generator,
    feature_scale_px: float,
) -> np.ndarray:
    """Return seamless, non-repeating band-limited noise."""
    white = rng.normal(0.0, 1.0, (size, size))
    frequency_y = np.fft.fftfreq(size)[:, None]
    frequency_x = np.fft.fftfreq(size)[None, :]
    radius_squared = frequency_x**2 + frequency_y**2
    low_pass = np.exp(
        -0.5 * radius_squared * feature_scale_px**2
    )
    field = np.fft.ifft2(np.fft.fft2(white) * low_pass).real
    return ((field - field.mean()) / max(field.std(), 1e-6)).astype(
        np.float32
    )


def generate(size: int = 1024, seed: int = 3481) -> np.ndarray:
    rng = np.random.default_rng(seed)
    broad = _periodic_noise(size, rng, 92.0)
    medium = _periodic_noise(size, rng, 30.0)
    fine = _periodic_noise(size, rng, 8.0)
    mineral = _periodic_noise(size, rng, 3.5)
    quartz = np.clip((medium + fine * 0.32 - 1.35) / 1.10, 0.0, 1.0)
    dark_mineral = np.clip((-medium + mineral * 0.28 - 1.55) / 0.95, 0.0, 1.0)
    value = np.clip(
        0.34
        + broad * 0.070
        + medium * 0.043
        + fine * 0.018
        + quartz * 0.060
        - dark_mineral * 0.052,
        0.10,
        0.62,
    )
    warm = np.clip(0.5 + broad * 0.18 + medium * 0.10, 0.0, 1.0)
    rgb = np.stack(
        [
            value * (0.88 + warm * 0.05),
            value * (0.92 + warm * 0.025),
            value * (0.96 - warm * 0.04),
        ],
        axis=-1,
    )
    return np.clip(rgb * 255.0, 0, 255).astype(np.uint8)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--material-id", default="highland_granite_v2")
    arguments = parser.parse_args()
    arguments.candidate.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(generate(), mode="RGB").save(arguments.candidate)
    manifest = materialize(
        arguments.candidate,
        arguments.output_dir,
        arguments.material_id,
        normal_strength=1.35,
    )
    print(f"Granite material {manifest['material_id']}: {manifest['status']}")
    return 0 if manifest["status"] != "rejected" else 2


if __name__ == "__main__":
    raise SystemExit(main())
