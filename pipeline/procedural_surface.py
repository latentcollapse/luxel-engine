#!/usr/bin/env python3
"""Generate a deterministic seamless procedural surface for any terrain layer.

`generate_granite_material` proved the FFT band-limited-noise approach works,
but it hardcodes granite's bands and colour ramp, so it cannot express snow,
peat, grass, or dirt. This module exposes the same construction as tunable
parameters so a caller can author a new surface without touching code, while
keeping the granite baseline byte-stable by not importing from it.
"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
from PIL import Image

from texture_material_pipeline import MINIFICATION_TEXELS_PER_PIXEL, materialize


SURFACE_VERSION = "codeweald.procedural-surface/v1"


def _periodic_noise(
    size: int,
    rng: np.random.Generator,
    feature_scale_px: float,
) -> np.ndarray:
    """Return seamless, non-repeating band-limited noise.

    Copied rather than imported from `generate_granite_material`: that module
    is the accepted granite baseline and must stay byte-stable, so this
    surface generator carries its own copy instead of creating a shared
    dependency that a future edit to either module could silently perturb.
    """
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


def generate(
    size: int = 1024,
    seed: int = 0,
    *,
    macro_scale_px: float = 92.0,
    macro_contrast: float = 0.070,
    meso_scale_px: float = 30.0,
    meso_contrast: float = 0.043,
    fine_scale_px: float = 8.0,
    fine_contrast: float = 0.018,
    base_value: float = 0.34,
    value_range: tuple[float, float] = (0.10, 0.62),
    tint: tuple[float, float, float] = (1.0, 1.0, 1.0),
    tint_variation: float = 0.0,
) -> np.ndarray:
    """Compose a seamless surface from three additive band-limited noise fields.

    `macro_scale_px` is the parameter that decides whether the material is
    visible at distance at all: a screen pixel at overview distance covers
    roughly `MINIFICATION_TEXELS_PER_PIXEL` (currently
    %d) texels, so any band with a feature scale finer than that is averaged
    away by minification before it is ever drawn. A surface that puts its
    contrast budget only into `meso_scale_px`/`fine_scale_px` bands can look
    rich up close and still flatten to a single colour from an overview
    camera; `macro_scale_px` above that texel count is what survives.
    """
    rng = np.random.default_rng(seed)
    macro = _periodic_noise(size, rng, macro_scale_px)
    meso = _periodic_noise(size, rng, meso_scale_px)
    fine = _periodic_noise(size, rng, fine_scale_px)
    value = np.clip(
        base_value
        + macro * macro_contrast
        + meso * meso_contrast
        + fine * fine_contrast,
        value_range[0],
        value_range[1],
    )
    tint_array = np.asarray(tint, dtype=np.float32)
    # The macro band, not an independent noise field, drives hue drift: reusing
    # it keeps colour and brightness co-located so a surface reads as one
    # material with slow variation rather than a value pattern with unrelated
    # colour static painted over it.
    hue_shift = 1.0 + macro[..., None] * tint_variation
    rgb = value[..., None] * tint_array[None, None, :] * hue_shift
    return np.clip(rgb * 255.0, 0, 255).astype(np.uint8)


generate.__doc__ = generate.__doc__ % MINIFICATION_TEXELS_PER_PIXEL


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--material-id", default="procedural_surface")
    parser.add_argument("--size", type=int, default=1024)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--macro-scale-px", type=float, default=92.0)
    parser.add_argument("--macro-contrast", type=float, default=0.070)
    parser.add_argument("--meso-scale-px", type=float, default=30.0)
    parser.add_argument("--meso-contrast", type=float, default=0.043)
    parser.add_argument("--fine-scale-px", type=float, default=8.0)
    parser.add_argument("--fine-contrast", type=float, default=0.018)
    parser.add_argument("--base-value", type=float, default=0.34)
    parser.add_argument("--value-min", type=float, default=0.10)
    parser.add_argument("--value-max", type=float, default=0.62)
    parser.add_argument("--tint", type=float, nargs=3, default=(1.0, 1.0, 1.0))
    parser.add_argument("--tint-variation", type=float, default=0.0)
    parser.add_argument("--normal-strength", type=float, default=1.35)
    arguments = parser.parse_args()
    arguments.candidate.parent.mkdir(parents=True, exist_ok=True)
    pixels = generate(
        arguments.size,
        arguments.seed,
        macro_scale_px=arguments.macro_scale_px,
        macro_contrast=arguments.macro_contrast,
        meso_scale_px=arguments.meso_scale_px,
        meso_contrast=arguments.meso_contrast,
        fine_scale_px=arguments.fine_scale_px,
        fine_contrast=arguments.fine_contrast,
        base_value=arguments.base_value,
        value_range=(arguments.value_min, arguments.value_max),
        tint=tuple(arguments.tint),
        tint_variation=arguments.tint_variation,
    )
    Image.fromarray(pixels, mode="RGB").save(arguments.candidate)
    manifest = materialize(
        arguments.candidate,
        arguments.output_dir,
        arguments.material_id,
        normal_strength=arguments.normal_strength,
    )
    print(f"Surface material {manifest['material_id']}: {manifest['status']}")
    return 0 if manifest["status"] != "rejected" else 2


if __name__ == "__main__":
    raise SystemExit(main())
