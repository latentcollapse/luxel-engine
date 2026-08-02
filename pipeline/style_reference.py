#!/usr/bin/env python3
"""Compact, provenance-friendly visual-style measurements for Codeweald.

This does not claim a histogram proves an artistic match. It provides stable
signals for an autonomous pipeline: palette distribution, value/saturation,
and structural frequency can be recorded for a source batch and compared to
renders or material candidates before a reviewer/model makes the final call.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any, Iterable

import numpy as np
from PIL import Image


STYLE_VERSION = "codeweald.style-reference/v1"


def _load(path: Path) -> np.ndarray:
    with Image.open(path) as image:
        return np.asarray(image.convert("RGB"), dtype=np.float32) / 255.0


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _luma(rgb: np.ndarray) -> np.ndarray:
    return rgb[:, :, 0] * 0.2126 + rgb[:, :, 1] * 0.7152 + rgb[:, :, 2] * 0.0722


def _edge_density(luma: np.ndarray) -> float:
    gradient_y, gradient_x = np.gradient(luma)
    return float((np.hypot(gradient_x, gradient_y) >= 0.055).mean())


# ``detail_density`` is the one measurement compared directly between painted
# source art and an engine render, so both sides must compute it identically.
# Three things have to match or the number is not a comparison at all:
#
#   resolution   np.gradient is per-pixel, so a 2048px painting and a 1280px
#                render sample detail at different spatial frequencies
#   threshold    what counts as an edge in the first place
#   denominator  which pixels are in the average
#
# The frame is used whole on both sides. Masking sky out of a render is easy and
# masking it out of a painting is not, and an asymmetric denominator is exactly
# the defect this replaces: ``foreground_edge_density`` averaged over foreground
# only while ``edge_density`` averaged over everything, which silently inflated
# the render against its own target.
#
# The per-side metrics above and in bevy_visual_acceptance are deliberately left
# alone. They are tuned gates, self-consistent within one side, and re-tuning
# them is a separate change with its own regressions.
DETAIL_DENSITY_VERSION = "codeweald.detail-density/v1"
DETAIL_DENSITY_HEIGHT = 720
DETAIL_DENSITY_THRESHOLD = 0.055


def detail_density(rgb: np.ndarray) -> float:
    """Fraction of the frame carrying visible luminance detail.

    Comparable across images of different sizes and between painted and
    rendered sources. Not a fidelity judgement on its own -- it counts texture,
    material break-up, and shading as readily as silhouette, which is precisely
    why landform geometry barely moves it.
    """
    if rgb.ndim != 3 or rgb.shape[2] != 3:
        raise ValueError("detail density needs an RGB image")
    luma = _luma(rgb)
    height, width = luma.shape
    if height != DETAIL_DENSITY_HEIGHT:
        scale = DETAIL_DENSITY_HEIGHT / float(height)
        resized = Image.fromarray(luma.astype(np.float32), mode="F").resize(
            (max(1, int(round(width * scale))), DETAIL_DENSITY_HEIGHT),
            Image.BILINEAR,
        )
        luma = np.asarray(resized, dtype=np.float32)
    gradient_y, gradient_x = np.gradient(luma)
    return float(
        (np.hypot(gradient_x, gradient_y) >= DETAIL_DENSITY_THRESHOLD).mean()
    )


# ``detail_density`` answers "how much of the frame has an edge" but says
# nothing about the blank stretches in between -- a render can hit a healthy
# detail-density number while still carrying dead flat blocks that a painter
# would never leave untouched. ``surface_variation_coverage`` asks a coarser,
# complementary question at block granularity: does this patch of the frame
# carry *any* variation at all, however faint. A per-pixel gradient threshold
# would double-count the same edge across many pixels and say nothing about
# the block next to it that has none; std-dev per block is what actually
# tells you whether a region was touched.
SURFACE_VARIATION_BLOCK = 16
SURFACE_VARIATION_FLOOR = 0.012


def surface_variation_coverage(rgb: np.ndarray) -> float:
    """Fraction of blocks carrying any surface variation at all.

    A coverage gate, not a quality score: a block with a single faint gradient
    passes exactly as readily as one full of fine detail. That is deliberate --
    the failure mode this catches is untouched flat regions, not weak texture.

    Stable under rescaling only where it matters. A densely textured frame
    scores ~1.0 at any size, but a *sparse* frame drifts, and the direction
    depends on what its detail is made of: fine grain averages away when
    downsampled and the number falls, while structured detail survives and
    smears across proportionally more blocks, so it rises (the current render is
    the second kind -- 0.27 at full size, 0.39 at 0.35x). Compare like-sized
    captures only.
    """
    if rgb.ndim != 3 or rgb.shape[2] != 3:
        raise ValueError("surface variation coverage needs an RGB image")
    luma = _luma(rgb)
    block = SURFACE_VARIATION_BLOCK
    h = (luma.shape[0] // block) * block
    w = (luma.shape[1] // block) * block
    tiles = (
        luma[:h, :w]
        .reshape(h // block, block, w // block, block)
        .transpose(0, 2, 1, 3)
        .reshape(-1, block * block)
    )
    return float((tiles.std(axis=1) >= SURFACE_VARIATION_FLOOR).mean())


def _hsv(rgb: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    maximum, minimum = rgb.max(axis=2), rgb.min(axis=2)
    delta = maximum - minimum
    hue = np.zeros_like(maximum)
    nonzero = delta > 1e-6
    red = nonzero & (maximum == rgb[:, :, 0])
    green = nonzero & (maximum == rgb[:, :, 1])
    blue = nonzero & (maximum == rgb[:, :, 2])
    hue[red] = ((rgb[:, :, 1][red] - rgb[:, :, 2][red]) / delta[red]) % 6.0
    hue[green] = (rgb[:, :, 2][green] - rgb[:, :, 0][green]) / delta[green] + 2.0
    hue[blue] = (rgb[:, :, 0][blue] - rgb[:, :, 1][blue]) / delta[blue] + 4.0
    hue /= 6.0
    saturation = np.divide(delta, maximum, out=np.zeros_like(delta), where=maximum > 1e-6)
    return hue, saturation, maximum


def analyze(rgb: np.ndarray) -> dict[str, Any]:
    if rgb.ndim != 3 or rgb.shape[2] != 3:
        raise ValueError("style image must be RGB")
    luma = _luma(rgb)
    hue, saturation, value = _hsv(rgb)
    mean_rgb = rgb.reshape(-1, 3).mean(axis=0)
    # A small joint distribution is more robust than relying on a single
    # 'dominant colour' and has no model/API dependency.
    bins = np.histogramdd(np.stack([hue.ravel(), saturation.ravel(), value.ravel()], axis=1), bins=(12, 4, 4), range=((0.0, 1.0), (0.0, 1.0), (0.0, 1.0)))[0]
    distribution = (bins / max(float(bins.sum()), 1.0)).round(8).ravel().tolist()
    quantized = np.clip((rgb * 7.999).astype(np.int16), 0, 7)
    codes = quantized[:, :, 0] * 64 + quantized[:, :, 1] * 8 + quantized[:, :, 2]
    counts = np.bincount(codes.ravel(), minlength=512)
    palette = []
    for code in np.argsort(counts)[-8:][::-1]:
        if counts[code] <= 0:
            continue
        palette.append({"srgb": [round(((code // 64) + 0.5) / 8.0, 4), round((((code // 8) % 8) + 0.5) / 8.0, 4), round(((code % 8) + 0.5) / 8.0, 4)], "fraction": round(float(counts[code] / codes.size), 6)})
    return {
        "metrics": {
            "mean_luminance": round(float(luma.mean()), 6), "luminance_stddev": round(float(luma.std()), 6),
            "mean_saturation": round(float(saturation.mean()), 6), "saturation_stddev": round(float(saturation.std()), 6),
            "mean_value": round(float(value.mean()), 6), "edge_density": round(_edge_density(luma), 6),
            "detail_density": round(detail_density(rgb), 6),
            "surface_variation_coverage": round(surface_variation_coverage(rgb), 6),
            "mean_rgb": [round(float(channel), 6) for channel in mean_rgb],
        },
        "hsv_distribution": distribution,
        "dominant_palette_srgb": palette,
    }


def profile(path: Path) -> dict[str, Any]:
    path = path.resolve()
    result = analyze(_load(path))
    return {"schema_version": STYLE_VERSION, "source": {"path": str(path), "sha256": _sha256(path)}, **result}


def _crop_render_content(candidate: np.ndarray) -> tuple[np.ndarray, dict[str, Any]]:
    """Exclude engine viewport letterboxing from concept-style comparison."""
    height, width = candidate.shape[:2]
    corners = np.asarray((candidate[0, 0], candidate[0, -1], candidate[-1, 0], candidate[-1, -1]))
    background = np.median(corners, axis=0)
    difference = np.max(np.abs(candidate - background[None, None, :]), axis=2)
    content_y, content_x = np.where(difference >= 0.035)
    if content_x.size < width * height * 0.08:
        return candidate, {"applied": False, "box": [0, 0, width, height]}
    pad_x, pad_y = max(2, width // 100), max(2, height // 100)
    left, right = max(0, int(content_x.min()) - pad_x), min(width, int(content_x.max()) + pad_x + 1)
    top, bottom = max(0, int(content_y.min()) - pad_y), min(height, int(content_y.max()) + pad_y + 1)
    coverage = (right - left) * (bottom - top) / float(width * height)
    if coverage >= 0.97:
        return candidate, {"applied": False, "box": [0, 0, width, height]}
    return candidate[top:bottom, left:right], {
        "applied": True,
        "box": [left, top, right, bottom],
        "source_size": [width, height],
        "coverage": round(coverage, 6),
    }


def score(reference: dict[str, Any], candidate: np.ndarray) -> dict[str, Any]:
    if reference.get("schema_version") != STYLE_VERSION:
        raise ValueError("unsupported style reference")
    comparison, crop = _crop_render_content(candidate)
    measured = analyze(comparison)
    reference_metrics, candidate_metrics = reference["metrics"], measured["metrics"]
    metric_scales = {"mean_luminance": 0.24, "luminance_stddev": 0.16, "mean_saturation": 0.28, "saturation_stddev": 0.22, "mean_value": 0.26, "edge_density": 0.12}
    metric_error = float(np.mean([abs(float(reference_metrics[key]) - float(candidate_metrics[key])) / scale for key, scale in metric_scales.items()]))
    reference_hist = np.asarray(reference["hsv_distribution"], dtype=np.float32)
    candidate_hist = np.asarray(measured["hsv_distribution"], dtype=np.float32)
    histogram_distance = float(np.abs(reference_hist - candidate_hist).sum() * 0.5)
    # Scores are diagnostics, intentionally not a claim of semantic fidelity.
    style_score = float(np.exp(-(metric_error * 0.55 + histogram_distance * 1.45)))
    return {
        "schema_version": STYLE_VERSION,
        "score": round(style_score, 6),
        "metric_error": round(metric_error, 6),
        "hsv_distribution_distance": round(histogram_distance, 6),
        "comparison_crop": crop,
        "candidate": measured,
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Extract or compare deterministic Codeweald style-reference measurements")
    parser.add_argument("image", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--compare", type=Path, help="existing style profile JSON to score against instead of creating a profile")
    args = parser.parse_args(argv)
    try:
        result = score(json.loads(args.compare.read_text(encoding="utf-8")), _load(args.image)) if args.compare else profile(args.image)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        parser.error(str(exc))
    print("Style %s" % ("score: %.3f" % result["score"] if args.compare else "profile written"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
