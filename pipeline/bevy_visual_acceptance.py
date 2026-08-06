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


# **One 8-bit code value.** A region whose local luma range falls below this has
# no recoverable detail *in the data* -- the difference between its neighbouring
# pixels is at or under the quantisation step of the file itself, so no exposure
# or grade recovers structure that was never encoded. That is what makes it a
# derived floor rather than a tuned one: it is a property of an 8-bit PNG, not a
# number chosen to make a particular world pass. A shadowed cliff sits far above
# it, because a shadow attenuates a gradient rather than deleting it.
QUANTISATION_STEP = 1.0 / 255.0
CRUSHED_LOCAL_RANGE = 2.0 * QUANTISATION_STEP

# **The one re-baseline D25 authorises, with both sides recorded.**
#
# The old gate was `dark_foreground_fraction > 0.08`. This one measures a
# different and much rarer quantity, so the old number carries no meaning here
# and reusing it would have been a coincidence dressed as continuity. Measured
# 2026-08-04 across every capture in the workspace:
#
# | capture                          | dark   | unreadable | structured |
# |----------------------------------|--------|------------|------------|
# | arena, `bevy_overview` (this gate)| 0.0027 | 0.0007     | 0.744      |
# | arena, `bevy_player`             | 0.0114 | 0.0068     | 0.404      |
# | arena, `bevy_border`             | 0.0270 | 0.0190     | 0.297      |
# | arena, runtime frame             | 0.0156 | 0.0015     | 0.904      |
# | arena, zone vista                | 0.0518 | 0.0172     | 0.667      |
# | caledonia (failed build)         | 0.2260 | 0.1565     | 0.308      |
#
# The separation is the point, and it is widest on the capture this gate
# actually runs on. The limit sits at roughly twice the worst good capture
# (`bevy_border`, 0.0190) and a quarter of the bad one, so it has room for a
# darker world without ceasing to be able to fail --
# `tests/test_bevy_visual_acceptance.py` pins both directions.
#
# **Not yet in `metric_kinds`.** That table records which metrics were scored
# against noise, shuffled pixels and an unfiltered render (tooling item 6), and
# `unreadable_fraction` has not been through it -- nor had `dark_foreground_fraction`
# before it. The evidence above is a comparison of real captures, which is a
# weaker thing, and the label should not be claimed until the scoring is run.
UNREADABLE_LIMIT = 0.04


def _local_range(values: np.ndarray) -> np.ndarray:
    """Max-minus-min in each pixel's 3x3 neighbourhood, per channel.

    Range rather than standard deviation on purpose: std over a 3x3 window is
    dominated by how many neighbours differ, so a region with one bright speck
    scores the same as one with a genuine gradient across it. What decides
    whether detail is recoverable is simply whether *any* difference survived
    quantisation, which is what a range measures.

    **Per channel, then the widest, and that is not a detail.** Quantisation
    happens per channel, so the threshold only means "one code value" if it is
    applied where the code values are. Measured on luma instead, two pixels
    differing by a full step in blue alone come out 0.00028 apart -- because
    blue carries a 0.0722 luma weight -- and would be called crushed while
    holding real encoded detail. In a dark region that is exactly the case that
    arises: shadow detail is often chroma before it is luminance.
    """
    if values.ndim == 2:
        values = values[:, :, None]
    padded = np.pad(values, ((1, 1), (1, 1), (0, 0)), mode="edge")
    stack = np.stack(
        [
            padded[row : row + values.shape[0], column : column + values.shape[1], :]
            for row in range(3)
            for column in range(3)
        ]
    )
    return (stack.max(axis=0) - stack.min(axis=0)).max(axis=2)


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
    # **D25: dark is not the same thing as unreadable.**
    #
    # This gate used to fail on `dark_foreground_fraction` alone, and that made
    # it an argument against mountains: the alpine arena went 0.0751 -> 0.0940
    # purely by gaining the relief the art direction asks for, and the cheapest
    # way to satisfy the gate was to build flatter mountains. Same standing
    # incentive as D16 -- a critic that rewards a worse world.
    #
    # The two things it conflated are separable by measurement. A shadowed cliff
    # is dark *and structured*: attenuating a surface scales its gradient down
    # but leaves it encoded. A crushed region is dark and *flat* -- the detail is
    # not dim, it is absent, and no grade recovers it. So the gate now fails on
    # the part that is actually a render defect and merely reports the rest.
    dark = foreground & (luma < 0.055)
    structure = _local_range(luma)
    crushed = dark & (structure < CRUSHED_LOCAL_RANGE)
    unreadable_fraction = (
        float(crushed.sum() / foreground.sum()) if foreground.any() else 1.0
    )
    dark_structured_fraction = (
        float((dark & ~crushed).sum() / dark.sum()) if dark.any() else 0.0
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
    # Threshold on the *crushed* fraction, not on darkness. It is set an order of
    # magnitude below the old one because it is now measuring a different and
    # much rarer thing -- see `UNREADABLE_LIMIT`.
    if unreadable_fraction > UNREADABLE_LIMIT:
        failures.append("Compiled world contains crushed, unreadable pixels")
    if edge_density < 0.008 or dynamic_range < 0.10:
        failures.append("Compiled world lacks readable visual structure")
    if road_fraction < 0.008:
        failures.append("Compiled roads are not visually readable")
    # Warm settlement pads share the inspection road palette, so RGB component
    # count remains diagnostic rather than pretending to prove road topology.
    # Topology is certified from the renderer-independent corridor meshes; this
    # image gate only proves that a warm road surface is actually visible.
    #
    # **D28: `foliage_fraction` no longer gates, for the same reason
    # `dark_foreground_fraction` stopped gating under D25 — it was measuring the
    # wrong quantity.**
    #
    # It is a green-pixel share of the whole frame, and the frame's composition
    # is a function of terrain height. Two builds of the same world went 275 ->
    # 301 render-plan instances while this fell 0.006820 -> 0.005207: more
    # foliage, lower metric. No threshold fixes an instrument that moves the
    # wrong way.
    #
    # Normalising by `foreground_fraction` (D24's recorded fix direction) is not
    # enough here. It removes the sky's share of the effect and leaves the
    # rock's: a taller massif means proportionally more rock among the world's
    # own pixels, so green-share-of-world still falls with no tree removed.
    #
    # The replacement is `foliage_projection_acceptance`, whose denominator is
    # the instance count rather than an area. This number is kept and reported
    # because it is still the right thing to watch when judging legibility, and
    # keeping it visible is what lets the replacement be checked against it
    # rather than taken on trust.
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
            # Kept, and deliberately no longer a gate. It is still the right
            # number to watch when judging whether a world is legible, and
            # keeping it visible is what lets the re-baseline be checked later
            # rather than taken on trust.
            "dark_foreground_fraction": round(dark_fraction, 6),
            "unreadable_fraction": round(unreadable_fraction, 6),
            "unreadable_limit": UNREADABLE_LIMIT,
            # Of the dark pixels, how many still carry recoverable detail. A
            # world of real mountains should sit near 1.0; a crushed render
            # collapses toward 0. This is the number that proves the split is
            # measuring what it claims to.
            "dark_structured_fraction": round(dark_structured_fraction, 6),
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
