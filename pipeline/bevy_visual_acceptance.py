#!/usr/bin/env python3
"""Deterministic integrity gate for Codeweald's Bevy overview capture.

This gate deliberately measures renderer failures rather than claiming that
simple pixels can judge finished art. It catches clipped framing, black
terrain, missing roads, broken route ribbons, absent foliage/water, and empty
or low-contrast worlds before a model promotes a visual iteration.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import deque
from pathlib import Path
from typing import Any, Iterable

import numpy as np
from PIL import Image

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

from metric_honesty import labels as metric_labels  # noqa: E402
from style_reference import (  # noqa: E402
    DETAIL_DENSITY_VERSION,
    detail_density,
    surface_variation_coverage,
)

VERSION = "codeweald.bevy-visual-acceptance/v1"


def _read_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain an object")
    return value


def _components(mask: np.ndarray, minimum_pixels: int) -> list[int]:
    """Return 4-connected component sizes for a small boolean mask."""
    visited = np.zeros(mask.shape, dtype=bool)
    sizes: list[int] = []
    rows, columns = mask.shape
    for row, column in np.argwhere(mask):
        if visited[row, column]:
            continue
        queue = deque([(int(row), int(column))])
        visited[row, column] = True
        size = 0
        while queue:
            current_row, current_column = queue.popleft()
            size += 1
            for next_row, next_column in (
                (current_row - 1, current_column),
                (current_row + 1, current_column),
                (current_row, current_column - 1),
                (current_row, current_column + 1),
            ):
                if (
                    0 <= next_row < rows
                    and 0 <= next_column < columns
                    and mask[next_row, next_column]
                    and not visited[next_row, next_column]
                ):
                    visited[next_row, next_column] = True
                    queue.append((next_row, next_column))
        if size >= minimum_pixels:
            sizes.append(size)
    return sorted(sizes, reverse=True)


def _dilate(mask: np.ndarray, iterations: int) -> np.ndarray:
    result = mask.copy()
    for _ in range(iterations):
        padded = np.pad(result, 1, mode="constant", constant_values=False)
        result = (
            padded[1:-1, 1:-1]
            | padded[:-2, 1:-1]
            | padded[2:, 1:-1]
            | padded[1:-1, :-2]
            | padded[1:-1, 2:]
        )
    return result


def evaluate(zone_spec: dict[str, Any], capture_path: Path) -> dict[str, Any]:
    with Image.open(capture_path) as source:
        rgb = np.asarray(source.convert("RGB"), dtype=np.float32) / 255.0
    height, width = rgb.shape[:2]
    if min(height, width) < 256:
        raise ValueError("Bevy capture must be at least 256 pixels per side")

    # The status HUD occupies the upper-left of the sky. Ignore the upper strip
    # globally rather than teaching visual metrics the current UI dimensions.
    analysis = rgb[int(height * 0.14) :, :, :]
    background_samples = np.concatenate(
        (
            analysis[:12, int(width * 0.72) :, :].reshape((-1, 3)),
            analysis[:, -8:, :].reshape((-1, 3)),
        ),
        axis=0,
    )
    background = np.median(background_samples, axis=0)
    foreground = np.linalg.norm(analysis - background, axis=2) >= 0.045
    luma = (
        analysis[:, :, 0] * 0.2126
        + analysis[:, :, 1] * 0.7152
        + analysis[:, :, 2] * 0.0722
    )
    gradient_y, gradient_x = np.gradient(luma)
    edges = np.hypot(gradient_x, gradient_y) >= 0.045

    red, green, blue = (
        analysis[:, :, 0],
        analysis[:, :, 1],
        analysis[:, :, 2],
    )
    # NOTE (2026-08-02): this hue test cannot see the world's actual road.
    #
    # The alpine arena's declared road colour is (0.306, 0.278, 0.196) -- a
    # red/green ratio of 1.0987 against the 1.10 required here. It misses by
    # 0.1%. The gate passed for months only because a flat debug ribbon
    # (0.34, 0.23, 0.105 -> ratio 1.478) was drawn on top of every road and
    # supplied the hue. With the ribbon hidden so the real path art is visible,
    # this goes red on a world whose roads were just *widened* from 4.5 m to
    # 10 m -- it is measuring the overlay, not the road.
    #
    # Deriving the ratios from the declared palette was tried and is worse
    # (0.00056): ambient light shifts the rendered road away from its albedo,
    # so no fixed ratio recovers it. The real fix is to project the known road
    # geometry into the frame instead of guessing road pixels from colour.
    # Tracked as D18; thresholds left untouched rather than tuned until green.
    roads = (
        foreground
        & (red >= 0.30)
        & (red >= green * 1.10)
        & (green >= blue * 1.16)
    )
    foliage = (
        foreground
        & (green >= 0.20)
        & (green >= red * 1.08)
        & (green >= blue * 1.05)
    )
    water = (
        foreground
        & (blue >= 0.22)
        & (green >= red * 1.16)
        & (blue >= red * 1.16)
    )
    road_components = _components(
        _dilate(roads[::2, ::2], iterations=6),
        minimum_pixels=48,
    )
    road_pixels = int(roads.sum())
    largest_road_ratio = (
        min(1.0, float(road_components[0] * 4 / road_pixels))
        if road_pixels and road_components
        else 0.0
    )
    border_edges = np.concatenate(
        (
            edges[-4:, :].reshape(-1),
            edges[:, :4].reshape(-1),
            edges[:, -4:].reshape(-1),
        )
    )
    foreground_luma = luma[foreground]
    failures: list[str] = []
    foreground_fraction = float(foreground.mean())
    border_edge_fraction = float(border_edges.mean())
    dark_fraction = (
        float((foreground_luma < 0.055).mean())
        if foreground_luma.size
        else 1.0
    )
    edge_density = float(edges[foreground].mean()) if foreground.any() else 0.0
    dynamic_range = (
        float(
            np.percentile(foreground_luma, 95)
            - np.percentile(foreground_luma, 5)
        )
        if foreground_luma.size
        else 0.0
    )
    road_fraction = float(roads.mean())
    foliage_fraction = float(foliage.mean())
    water_fraction = float(water.mean())

    if not 0.18 <= foreground_fraction <= 0.88:
        failures.append("Compiled world has implausible screen coverage")
    if border_edge_fraction > 0.04:
        failures.append("Compiled world is clipped by the overview frame")
    if dark_fraction > 0.08:
        failures.append("Compiled world contains excessive black/unreadable pixels")
    if edge_density < 0.008 or dynamic_range < 0.10:
        failures.append("Compiled world lacks readable visual structure")
    if road_fraction < 0.008:
        failures.append("Compiled roads are not visually readable")
    # Warm settlement pads share the inspection road palette, so RGB component
    # count remains diagnostic rather than pretending to prove road topology.
    # Topology is certified from the renderer-independent corridor meshes; this
    # image gate only proves that a warm road surface is actually visible.
    if foliage_fraction < 0.008:
        failures.append("Compiled foliage is not visually readable")
    if water_fraction < 0.001:
        failures.append("Compiled wetland/water hierarchy is not visually readable")

    return {
        "schema_version": VERSION,
        "zone_id": zone_spec.get("zone", {}).get("id"),
        "capture": {
            "path": str(capture_path),
            "sha256": hashlib.sha256(capture_path.read_bytes()).hexdigest(),
            "width": width,
            "height": height,
        },
        "status": "failed" if failures else "passed",
        "failures": failures,
        "metrics": {
            "background_rgb": [round(float(value), 5) for value in background],
            "foreground_fraction": round(foreground_fraction, 6),
            "border_edge_fraction": round(border_edge_fraction, 6),
            "dark_foreground_fraction": round(dark_fraction, 6),
            "foreground_edge_density": round(edge_density, 6),
            # Computed by the same function as the source art's, on the
            # HUD-cropped frame, so the two are directly comparable. The
            # foreground metric above stays as this gate's own tuned signal.
            "detail_density": round(detail_density(analysis), 6),
            "detail_density_version": DETAIL_DENSITY_VERSION,
            "surface_variation_coverage": round(surface_variation_coverage(analysis), 6),
            "foreground_dynamic_range": round(dynamic_range, 6),
            "road_fraction": round(road_fraction, 6),
            "road_component_count": len(road_components),
            "largest_road_component_ratio": round(largest_road_ratio, 6),
            "foliage_fraction": round(foliage_fraction, 6),
            "water_fraction": round(water_fraction, 6),
        },
        # What each metric is actually measuring, decided by scoring it against
        # noise, shuffled pixels, and an un-filtered render (tooling item 6).
        # Three of these are coverage gates that a grain field aces -- reading
        # one as a fidelity score is how a broken render out-scored a correct
        # one. Computed from the live metric functions, so it cannot go stale
        # the way a checked-in table would.
        "metric_kinds": metric_labels(),
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Evaluate a deterministic Codeweald Bevy overview capture"
    )
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("capture", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args(argv)
    try:
        report = evaluate(_read_json(arguments.zone_spec), arguments.capture)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        parser.error(str(exc))
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        "Bevy visual acceptance %s: %s"
        % (report["zone_id"], report["status"])
    )
    return 0 if report["status"] == "passed" else 2


if __name__ == "__main__":
    raise SystemExit(main())
