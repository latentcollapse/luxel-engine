#!/usr/bin/env python3
"""Rasterize a portable ZoneSpec into deterministic terrain artifacts.

Unlike the legacy image-threshold scripts, this compiler consumes reviewed semantic
features.  In particular, ``alpine_jagged_massif`` is a terrain grammar with sharp
ridges, protected high-relief cores, rock exposure, and a snowline; it is not a
synonym for a blurred radial hill.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

import numpy as np
from PIL import Image, ImageFilter

from collections import namedtuple as _namedtuple

from hydrology import carve_outlet, choose_outlet, fill_depressions, flow_accumulation

# The outlet is chosen on the coarse grid and cut on the fine one, so the carve
# takes a position rather than the coarse Outlet record it came from.
Outlet_ = _namedtuple("Outlet_", "row column")
_OutletAt = _namedtuple("_OutletAt", "row column")
from massif_character import shape_relief

from worldbuilder_dsl import _PATTERN_SCALARS
from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError


@dataclass(frozen=True)
class TerrainRaster:
    height_m: np.ndarray
    splat_rgba: np.ndarray
    normal_rgb: np.ndarray
    water_mask: np.ndarray
    wetland_mask: np.ndarray
    protected_relief_mask: np.ndarray
    semantic_region_mask: np.ndarray
    style_guidance_rgb: np.ndarray
    preview_rgb: np.ndarray
    manifest: dict[str, Any]


REGION_PROTECTED_RELIEF = np.uint8(1 << 0)
REGION_TRAVERSABLE_LANDFORM = np.uint8(1 << 1)
REGION_LANE = np.uint8(1 << 2)
REGION_HYDROLOGY = np.uint8(1 << 3)
REGION_LANDMARK_PAD = np.uint8(1 << 4)


# Landform profiles are hand-tuned against the concept art, but the amplitudes
# they were tuned to are exactly the ones the authoring DSL claims to control.
# Each profile therefore reads its composition scalars through
# ``_composition_scalars``, which returns the authored value alongside its ratio
# to the pattern default. Geometry multiplies the *ratio*: at the authored
# default it is exactly 1.0, so every calibrated constant below keeps its
# meaning, and moving a knob is what moves the heightfield.
MAX_SPINE_COUNT = 8


@dataclass(frozen=True)
class CompositionScalars:
    """Authored composition scalars, resolved against their pattern defaults."""

    spine_count: int
    along_jitter: float
    cross_jitter: float
    elevation_bias: float
    along_ratio: float
    cross_ratio: float


def _composition_scalars(
    feature_id: str, composition: dict[str, Any], pattern: str
) -> CompositionScalars:
    defaults = _PATTERN_SCALARS[pattern]
    raw_spines = composition.get("spine_count", defaults["spines"])
    if isinstance(raw_spines, bool) or not isinstance(raw_spines, (int, float)):
        raise ZoneCompileError(
            "%s.generation.composition.spine_count must be a whole number between "
            "1 and %d" % (feature_id, MAX_SPINE_COUNT)
        )
    spine_count = int(raw_spines)
    if spine_count != raw_spines or not 1 <= spine_count <= MAX_SPINE_COUNT:
        raise ZoneCompileError(
            "%s.generation.composition.spine_count must be a whole number between "
            "1 and %d (got %r)" % (feature_id, MAX_SPINE_COUNT, raw_spines)
        )
    jitter: dict[str, float] = {}
    for key in ("along_jitter", "cross_jitter"):
        raw = composition.get(key, defaults[key])
        if isinstance(raw, bool) or not isinstance(raw, (int, float)):
            raise ZoneCompileError(
                "%s.generation.composition.%s must be a number between 0.0 and 0.5"
                % (feature_id, key)
            )
        value = float(raw)
        if not 0.0 <= value <= 0.5:
            raise ZoneCompileError(
                "%s.generation.composition.%s must be between 0.0 and 0.5 (got %g)"
                % (feature_id, key, value)
            )
        jitter[key] = value
    # Resolved here rather than read ad-hoc at each use site, so it lands in
    # the manifest alongside the other scalars. It was previously read inline
    # with a bare 0.5 fallback, which meant detect_no_ops could never see it
    # go inert: the manifest recorded three of the four authored scalars, and
    # a knob absent from the manifest is invisible to a manifest diff.
    raw_bias = composition.get("elevation_bias", defaults["elevation_bias"])
    if isinstance(raw_bias, bool) or not isinstance(raw_bias, (int, float)):
        raise ZoneCompileError(
            "%s.generation.composition.elevation_bias must be a number between "
            "0.0 and 1.0" % feature_id
        )
    elevation_bias = float(raw_bias)
    if not 0.0 <= elevation_bias <= 1.0:
        raise ZoneCompileError(
            "%s.generation.composition.elevation_bias must be between 0.0 and "
            "1.0 (got %g)" % (feature_id, elevation_bias)
        )
    # Pattern defaults are non-zero by construction, so the ratios are defined.
    return CompositionScalars(
        spine_count=spine_count,
        along_jitter=jitter["along_jitter"],
        cross_jitter=jitter["cross_jitter"],
        elevation_bias=elevation_bias,
        along_ratio=jitter["along_jitter"] / float(defaults["along_jitter"]),
        cross_ratio=jitter["cross_jitter"] / float(defaults["cross_jitter"]),
    )


def _smoothstep(edge0: float, edge1: float, value: np.ndarray) -> np.ndarray:
    ratio = np.clip((value - edge0) / (edge1 - edge0), 0.0, 1.0)
    return ratio * ratio * (3.0 - 2.0 * ratio)


def _value_noise(shape: tuple[int, int], cells: int, rng: np.random.Generator) -> np.ndarray:
    """Deterministic smooth value noise without a runtime terrain dependency."""
    height, width = shape
    grid = rng.random((max(2, cells + 1), max(2, cells + 1)), dtype=np.float32)
    image = Image.fromarray((grid * 65535.0).astype(np.uint16))
    return np.asarray(image.resize((width, height), Image.Resampling.BICUBIC), dtype=np.float32) / 65535.0


def _fractal_noise(shape: tuple[int, int], rng: np.random.Generator, *, octaves: tuple[int, ...] = (3, 7, 15, 31)) -> np.ndarray:
    total = np.zeros(shape, dtype=np.float32)
    amplitude = 1.0
    normalizer = 0.0
    for cells in octaves:
        total += _value_noise(shape, cells, rng) * amplitude
        normalizer += amplitude
        amplitude *= 0.5
    return total / normalizer


def _box_blur_axis(values: np.ndarray, radius: int, axis: int) -> np.ndarray:
    if radius <= 0:
        return values
    padding = [(0, 0)] * values.ndim
    padding[axis] = (radius, radius)
    padded = np.pad(values, padding, mode="edge")
    cumulative = np.cumsum(padded, axis=axis, dtype=np.float64)
    zero_shape = list(cumulative.shape)
    zero_shape[axis] = 1
    cumulative = np.concatenate(
        [np.zeros(zero_shape, dtype=np.float64), cumulative], axis=axis
    )
    length = values.shape[axis]
    window = radius * 2 + 1
    finish = [slice(None)] * values.ndim
    start = [slice(None)] * values.ndim
    finish[axis] = slice(window, window + length)
    start[axis] = slice(0, length)
    return (
        (cumulative[tuple(finish)] - cumulative[tuple(start)]) / float(window)
    ).astype(np.float32)


def _broad_blur(values: np.ndarray, radius: int) -> np.ndarray:
    result = values.astype(np.float32, copy=False)
    # Two separable box passes approximate a broad Gaussian without adding a
    # SciPy/OpenCV dependency to the deterministic compiler.
    for _pass in range(2):
        result = _box_blur_axis(result, radius, 1)
        result = _box_blur_axis(result, radius, 0)
    return result


def _polygon_mask(x: np.ndarray, z: np.ndarray, points: list[list[float]]) -> tuple[np.ndarray, np.ndarray]:
    """Return inside mask and distance to the nearest polygon boundary in metres."""
    inside = np.zeros(x.shape, dtype=bool)
    distance_sq = np.full(x.shape, np.inf, dtype=np.float32)
    for index, point_a in enumerate(points):
        point_b = points[(index + 1) % len(points)]
        ax, az = float(point_a[0]), float(point_a[1])
        bx, bz = float(point_b[0]), float(point_b[1])
        crosses = ((az > z) != (bz > z)) & (x < (bx - ax) * (z - az) / ((bz - az) + 1e-12) + ax)
        inside ^= crosses
        segment_x, segment_z = bx - ax, bz - az
        denominator = segment_x * segment_x + segment_z * segment_z
        projection = np.clip(((x - ax) * segment_x + (z - az) * segment_z) / max(denominator, 1e-12), 0.0, 1.0)
        near_x, near_z = ax + projection * segment_x, az + projection * segment_z
        distance_sq = np.minimum(distance_sq, (x - near_x) ** 2 + (z - near_z) ** 2)
    return inside, np.sqrt(distance_sq, dtype=np.float32)


def _polyline_distance(x: np.ndarray, z: np.ndarray, points: list[list[float]]) -> np.ndarray:
    distance_sq = np.full(x.shape, np.inf, dtype=np.float32)
    for point_a, point_b in zip(points, points[1:]):
        ax, az = float(point_a[0]), float(point_a[1])
        bx, bz = float(point_b[0]), float(point_b[1])
        segment_x, segment_z = bx - ax, bz - az
        denominator = segment_x * segment_x + segment_z * segment_z
        projection = np.clip(((x - ax) * segment_x + (z - az) * segment_z) / max(denominator, 1e-12), 0.0, 1.0)
        near_x, near_z = ax + projection * segment_x, az + projection * segment_z
        distance_sq = np.minimum(distance_sq, (x - near_x) ** 2 + (z - near_z) ** 2)
    return np.sqrt(distance_sq, dtype=np.float32)


def _catmull_rom_route(
    points: list[list[float]], subdivisions: int = 8
) -> list[list[float]]:
    """Pass through authored route controls with a deterministic smooth spline."""
    if len(points) < 2 or subdivisions <= 0:
        return points
    result: list[list[float]] = []
    for index in range(len(points) - 1):
        p0 = np.asarray(points[max(0, index - 1)], dtype=np.float64)
        p1 = np.asarray(points[index], dtype=np.float64)
        p2 = np.asarray(points[index + 1], dtype=np.float64)
        p3 = np.asarray(points[min(len(points) - 1, index + 2)], dtype=np.float64)
        for step in range(subdivisions):
            t = step / subdivisions
            sample = 0.5 * (
                2.0 * p1
                + (-p0 + p2) * t
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t
            )
            result.append([float(sample[0]), float(sample[1])])
    result.append([float(points[-1][0]), float(points[-1][1])])
    return result


def _limit_profile_grade(
    heights: np.ndarray,
    points: list[list[float]],
    maximum_grade: float,
    pinned: dict[int, float] | None = None,
) -> np.ndarray:
    """Project a sampled route profile onto a bounded longitudinal grade."""
    result = np.asarray(heights, dtype=np.float64).copy()
    pinned = pinned or {}
    for index, value in pinned.items():
        result[index] = value
    for _pass in range(8):
        for index in range(1, len(result)):
            if index in pinned:
                continue
            distance = float(
                np.hypot(
                    points[index][0] - points[index - 1][0],
                    points[index][1] - points[index - 1][1],
                )
            )
            delta = maximum_grade * distance
            result[index] = np.clip(
                result[index], result[index - 1] - delta, result[index - 1] + delta
            )
        for index in range(len(result) - 2, -1, -1):
            if index in pinned:
                continue
            distance = float(
                np.hypot(
                    points[index + 1][0] - points[index][0],
                    points[index + 1][1] - points[index][1],
                )
            )
            delta = maximum_grade * distance
            result[index] = np.clip(
                result[index], result[index + 1] - delta, result[index + 1] + delta
            )
        for index, value in pinned.items():
            result[index] = value
    return result


def _grade_corridors(
    height: np.ndarray,
    x: np.ndarray,
    z: np.ndarray,
    features: list[dict[str, Any]],
    width: float,
    length: float,
) -> np.ndarray:
    """Compile smooth, bounded-grade roadbeds with eased cut/fill shoulders."""
    grading_radius = max(2, int(np.ceil(float(height.shape[0]) / 28.0)))
    broad_surface = _broad_blur(height, grading_radius)
    target_sum = np.zeros(height.shape, dtype=np.float64)
    weight_sum = np.zeros(height.shape, dtype=np.float64)
    strongest = np.zeros(height.shape, dtype=np.float64)
    roads: list[dict[str, Any]] = []
    keep_approaches: list[tuple[tuple[float, float], float, float]] = []
    for feature in features:
        if (
            feature.get("category") != "landmark"
            or feature.get("semantic") != "faction_keep"
        ):
            continue
        points = feature.get("geometry", {}).get("points", [])
        if not points:
            continue
        point = (float(points[0][0]), float(points[0][1]))
        keep_approaches.append(
            (
                point,
                _sample_height(height, width, length, point),
                max(
                    24.0,
                    float(
                        feature.get("properties", {}).get(
                            "scatter_exclusion_radius_m", 20.0
                        )
                    ),
                ),
            )
        )
    for feature in features:
        if (
            feature.get("category") != "corridor"
            or feature.get("semantic") != "lane"
            or feature.get("geometry", {}).get("type") != "polyline"
        ):
            continue
        properties = feature.get("properties", {})
        route = _catmull_rom_route(feature["geometry"].get("points", []))
        if len(route) < 2:
            continue
        profile = np.asarray(
            [_sample_height(broad_surface, width, length, point) for point in route],
            dtype=np.float64,
        )
        # A road entering a keep footprint and the keep's spawn pad are one
        # physical cut/fill problem. Make the route inherit pad elevation
        # before enforcing grade so their overlapping shoulders cannot create
        # a narrow seam that is neither safe road nor safe spawn surface.
        pinned_profile: dict[int, float] = {}
        for index, route_point in enumerate(route):
            for keep_point, keep_height, approach_radius in keep_approaches:
                if np.hypot(
                    route_point[0] - keep_point[0],
                    route_point[1] - keep_point[1],
                ) <= approach_radius:
                    profile[index] = keep_height
                    pinned_profile[index] = keep_height
                    break
        maximum_grade = float(properties.get("maximum_design_grade", 0.18))
        profile = _limit_profile_grade(
            profile,
            route,
            maximum_grade,
            pinned_profile,
        )
        roads.append(
            {
                "properties": properties,
                "route": route,
                "profile": profile,
                "pinned_profile": pinned_profile,
                "maximum_grade": maximum_grade,
            }
        )

    # Routes that converge or cross form one physical roadbed. Harmonize their
    # nearby longitudinal samples before rasterizing; averaging independent
    # road surfaces per pixel creates a warped saddle at junctions.
    for _junction_pass in range(16):
        for left_index, left in enumerate(roads):
            for right in roads[left_index + 1 :]:
                left_route = np.asarray(left["route"], dtype=np.float64)
                right_route = np.asarray(right["route"], dtype=np.float64)
                distances = np.linalg.norm(
                    left_route[:, None, :] - right_route[None, :, :], axis=2
                )
                contacts = np.argwhere(distances <= 10.0)
                for left_point, right_point in contacts:
                    common = (
                        left["profile"][left_point]
                        + right["profile"][right_point]
                    ) * 0.5
                    left["profile"][left_point] = common
                    right["profile"][right_point] = common
        for road in roads:
            road["profile"] = _limit_profile_grade(
                road["profile"],
                road["route"],
                road["maximum_grade"],
                road["pinned_profile"],
            )

    for road in roads:
        properties = road["properties"]
        route = road["route"]
        profile = road["profile"]
        nearest_sq = np.full(height.shape, np.inf, dtype=np.float64)
        target = np.zeros(height.shape, dtype=np.float64)
        for index, (point_a, point_b) in enumerate(zip(route, route[1:])):
            ax, az = point_a
            bx, bz = point_b
            segment_x, segment_z = bx - ax, bz - az
            denominator = segment_x * segment_x + segment_z * segment_z
            projection = np.clip(
                ((x - ax) * segment_x + (z - az) * segment_z)
                / max(denominator, 1e-12),
                0.0,
                1.0,
            )
            near_x = ax + projection * segment_x
            near_z = az + projection * segment_z
            distance_sq = (x - near_x) ** 2 + (z - near_z) ** 2
            nearer = distance_sq < nearest_sq
            target[nearer] = (
                profile[index]
                + projection[nearer] * (profile[index + 1] - profile[index])
            )
            nearest_sq[nearer] = distance_sq[nearer]
        distance = np.sqrt(nearest_sq)
        half_width = float(properties.get("minimum_width_m", 10.0)) * 0.5
        shoulder = half_width + max(4.0, half_width * 1.2)
        # The complete authored gameplay width is the guaranteed roadbed.
        # Cut/fill easing begins outside it, never under a large actor's feet.
        influence = 1.0 - _smoothstep(half_width, shoulder, distance)
        # A distant shoulder from a converging road must not tilt the full
        # roadbed of this lane. Concentrated blend weights preserve smooth
        # cut/fill shoulders while only averaging profiles where roadbeds
        # genuinely overlap.
        blend_weight = influence**4
        target_sum += target * blend_weight
        weight_sum += blend_weight
        strongest = np.maximum(strongest, influence)
    has_road = weight_sum > 1e-8
    roadbed = height.astype(np.float64, copy=True)
    roadbed[has_road] = target_sum[has_road] / weight_sum[has_road]
    return (
        height * (1.0 - strongest) + roadbed * strongest
    ).astype(np.float32)


def _sample_height(
    height: np.ndarray,
    width: float,
    length: float,
    point: list[float],
) -> float:
    rows, columns = height.shape
    u = np.clip(float(point[0]) / width + 0.5, 0.0, 1.0) * (columns - 1)
    v = np.clip(0.5 - float(point[1]) / length, 0.0, 1.0) * (rows - 1)
    x0, z0 = int(np.floor(u)), int(np.floor(v))
    x1, z1 = min(x0 + 1, columns - 1), min(z0 + 1, rows - 1)
    tx, tz = u - x0, v - z0
    return float(
        height[z0, x0] * (1.0 - tx) * (1.0 - tz)
        + height[z0, x1] * tx * (1.0 - tz)
        + height[z1, x0] * (1.0 - tx) * tz
        + height[z1, x1] * tx * tz
    )


def _polyline_samples(
    points: list[list[float]], spacing_m: float
) -> tuple[np.ndarray, np.ndarray]:
    samples: list[list[float]] = []
    distances: list[float] = []
    travelled = 0.0
    for point_a, point_b in zip(points, points[1:]):
        ax, az = point_a
        bx, bz = point_b
        segment_length = float(np.hypot(bx - ax, bz - az))
        steps = max(1, int(np.ceil(segment_length / spacing_m)))
        for step in range(steps):
            amount = step / steps
            samples.append(
                [ax + (bx - ax) * amount, az + (bz - az) * amount]
            )
            distances.append(travelled + segment_length * amount)
        travelled += segment_length
    samples.append(points[-1])
    distances.append(travelled)
    return np.asarray(samples, dtype=np.float64), np.asarray(
        distances, dtype=np.float64
    )


def _carve_hydrology(
    height: np.ndarray,
    x: np.ndarray,
    z: np.ndarray,
    features: list[dict[str, Any]],
    width: float,
    length: float,
    *,
    apply_channel_depth: bool = True,
) -> np.ndarray:
    """Conform each semantic centerline according to its channel morphology.

    Incised streams enforce a downhill bed and may cut terrain. Surface
    channels and wetland rills follow local relief with strictly bounded
    lowering, so a marsh centerline cannot become a slot canyon.
    """
    result = height.copy()
    original_height = height.copy()
    for feature in features:
        if (
            feature.get("category") != "hydrology"
            or feature.get("semantic") not in {"river", "stream"}
            or feature.get("geometry", {}).get("type") != "polyline"
        ):
            continue
        authored = feature.get("geometry", {}).get("points", [])
        if len(authored) < 2:
            continue
        points = [[float(point[0]), float(point[1])] for point in authored]
        properties = feature.get("properties", {})
        channel_profile = str(
            properties.get("channel_profile", "surface_channel")
        )
        if channel_profile not in {
            "wetland_rill",
            "surface_channel",
            "incised_stream",
        }:
            raise ZoneCompileError(
                "Unsupported hydrology channel_profile %s" % channel_profile
            )
        if not apply_channel_depth and channel_profile != "incised_stream":
            continue
        endpoint_heights = [
            _sample_height(result, width, length, point)
            for point in (points[0], points[-1])
        ]
        if endpoint_heights[-1] > endpoint_heights[0]:
            points.reverse()
        profile_points, profile_distances = _polyline_samples(points, 1.0)
        sampled_heights = np.asarray(
            [
                _sample_height(result, width, length, point)
                for point in profile_points
            ],
            dtype=np.float64,
        )
        profile_heights = (
            np.minimum.accumulate(sampled_heights)
            if channel_profile == "incised_stream"
            else sampled_heights
        )
        segment_lengths = [
            float(np.hypot(
                point_b[0] - point_a[0], point_b[1] - point_a[1]
            ))
            for point_a, point_b in zip(points, points[1:])
        ]
        segment_starts = np.concatenate(
            ([0.0], np.cumsum(segment_lengths[:-1], dtype=np.float64))
        )
        visible_width = float(properties.get("width_m", 8.0))
        half_width = visible_width * 0.5
        if channel_profile == "wetland_rill":
            outer_bank = max(3.0, half_width + 2.0)
            depth = float(np.clip(visible_width * 0.05, 0.05, 0.20))
            maximum_lowering = 0.25
        elif channel_profile == "surface_channel":
            outer_bank = max(4.0, half_width + 3.0)
            depth = float(np.clip(visible_width * 0.12, 0.12, 0.60))
            maximum_lowering = 0.75
        else:
            # A deliberately incised stream gets a broad eased bank and a
            # downhill bed. Reconciliation passes may lower confluences.
            outer_bank = max(6.0, half_width + 5.0)
            depth = float(np.clip(visible_width * 0.35, 0.35, 2.5))
            maximum_lowering = np.inf
        if not apply_channel_depth:
            depth = 0.0
        for index, (point_a, point_b) in enumerate(zip(points, points[1:])):
            ax, az = point_a
            bx, bz = point_b
            segment_x, segment_z = bx - ax, bz - az
            denominator = segment_x * segment_x + segment_z * segment_z
            projection = np.clip(
                ((x - ax) * segment_x + (z - az) * segment_z)
                / max(denominator, 1e-12),
                0.0,
                1.0,
            )
            near_x = ax + projection * segment_x
            near_z = az + projection * segment_z
            distance = np.sqrt((x - near_x) ** 2 + (z - near_z) ** 2)
            influence = 1.0 - _smoothstep(
                half_width * 0.75, outer_bank, distance
            )
            target = np.interp(
                segment_starts[index] + projection * segment_lengths[index],
                profile_distances,
                profile_heights,
            )
            target -= depth
            conformed = np.minimum(result, target)
            if np.isfinite(maximum_lowering):
                conformed = np.maximum(
                    conformed, original_height - maximum_lowering
                )
            lowered = result * (1.0 - influence) + conformed * influence
            result = np.minimum(result, lowered)
    return result.astype(np.float32, copy=False)


def _border_rampart(
    height: np.ndarray,
    x: np.ndarray,
    z: np.ndarray,
    features: list[dict[str, Any]],
    width: float,
    length: float,
    policy: dict[str, Any],
    rng: np.random.Generator,
) -> tuple[np.ndarray, dict[str, Any], np.ndarray]:
    """Close the world at its own edge (roadmap 2.5 / systems S4).

    A world whose terrain stops at the data edge does not contain its players:
    `boundary_plan` reports the walkable spans that leak, and the alpine arena
    leaks its entire north and south edges. This raises the landform that closes
    them.

    **Geometry is the boundary, deliberately.** An invisible wall would be a
    per-backend hack re-authored for Godot, Unity and Unreal separately, and it
    would make the leak untestable -- `boundary_plan` measures terrain, so a
    barrier it cannot see is a barrier that does not exist as far as the
    compiler is concerned.

    The rampart is a crest at the map edge falling to playable ground over
    `inner_face_m`. That face is what does the work: a body cannot stand on
    ground steeper than the agent's slope limit, so height/face must exceed it.
    Thinner is steeper, and therefore safer -- which is why encroachment is the
    thing that gets clamped when a landmark sits near the edge, not the height.
    """
    report: dict[str, Any] = {
        "enabled": bool(policy.get("enabled", True)),
        "profile": str(policy.get("profile", "escarpment")),
    }
    if not report["enabled"]:
        report["reason"] = "border_policy.enabled is false"
        return height, report, np.zeros(height.shape, dtype=bool)

    crest = float(policy.get("crest_m", 9.0))
    face = float(policy.get("inner_face_m", 16.0))
    peak = float(policy.get("height_m", 42.0))
    clearance = float(policy.get("landmark_clearance_m", 10.0))
    minimum_face = 3.0

    half_width, half_length = width * 0.5, length * 0.5
    inward = np.minimum(half_width - np.abs(x), half_length - np.abs(z))

    # **Distance from the play space, not from the map rectangle.**
    #
    # Driving the rampart off the rect edge builds a square basin, because that
    # is what "everywhere N metres from a rectangle" is. Reviewed 2026-08-02:
    # "the actual playable map is a rhomboid; anything that is not playable map
    # should be Alps and valley walls." The lanes and the woodlands they run
    # through are that rhomboid, so the wall is built outward from *them* and
    # everything the game does not use becomes mountain.
    #
    # The rect-edge term stays, unioned in below, because enclosure is not
    # negotiable: however the play space is shaped, the world still has to be
    # shut at its own boundary.
    play_distance = np.full(x.shape, np.inf, dtype=np.float64)
    envelope_sources = 0
    for feature in features:
        geometry = feature.get("geometry") or {}
        points = geometry.get("points") or []
        category = feature.get("category")
        if category == "corridor" and len(points) >= 2:
            width_m = float((feature.get("properties") or {}).get("minimum_width_m", 10.0))
            play_distance = np.minimum(
                play_distance, _polyline_distance(x, z, points) - width_m * 0.5
            )
            envelope_sources += 1
        elif category == "biome" and len(points) >= 3:
            # The background woodland spans nearly the whole map by design and
            # would swallow the envelope whole, taking the wall with it.
            span = max(
                max(p[0] for p in points) - min(p[0] for p in points),
                max(p[1] for p in points) - min(p[1] for p in points),
            )
            if span > min(width, length) * 0.8:
                continue
            inside, boundary = _polygon_mask(x, z, points)
            play_distance = np.minimum(play_distance, np.where(inside, 0.0, boundary))
            envelope_sources += 1
        elif category == "landmark" and points:
            radius = float(
                (feature.get("properties") or {}).get("scatter_exclusion_radius_m", 0.0)
            )
            for point in points:
                play_distance = np.minimum(
                    play_distance,
                    np.hypot(x - float(point[0]), z - float(point[1])) - radius,
                )
            envelope_sources += 1
    if envelope_sources:
        play_distance = np.maximum(play_distance, 0.0)
        report["envelope_sources"] = envelope_sources
    else:
        # Nothing declares a play space, so the rectangle is the only shape
        # available. Better a square basin than no border at all.
        play_distance = None
        report["envelope_sources"] = 0

    # **How far the rampart may reach is a field, not a number.** A single global
    # depth is set by whichever landmark sits closest to the edge, which on this
    # map is one watchpost -- and it would thin the entire border, on all four
    # edges, to satisfy one corner. Solving it per-cell lets the rampart run
    # full depth everywhere there is room and pinch only where it must.
    #
    # Pinching is safe in the direction that matters: the same height over a
    # shorter face is a *steeper* face, and steeper is what stops a body. The
    # thing that must never happen is burying a landmark, so that is what the
    # field protects.
    allowed = np.full(inward.shape, crest + face, dtype=np.float64)
    limiting: str | None = None
    tightest = math.inf
    for feature in features:
        if feature.get("category") != "landmark":
            continue
        points = (feature.get("geometry") or {}).get("points") or []
        if not points:
            continue
        radius = float(
            (feature.get("properties") or {}).get("scatter_exclusion_radius_m", 0.0)
        )
        for point in points:
            keep_out = radius + clearance
            reach = np.hypot(x - float(point[0]), z - float(point[1])) - keep_out
            allowed = np.minimum(allowed, reach)
            edge_distance = min(
                half_width - abs(float(point[0])), half_length - abs(float(point[1]))
            )
            if edge_distance - keep_out < tightest:
                tightest, limiting = edge_distance - keep_out, str(feature.get("id"))
    allowed = np.clip(allowed, minimum_face, crest + face)
    if tightest < crest + face:
        report["encroachment_limited_by"] = limiting
        report["tightest_landmark_room_m"] = round(float(tightest), 3)

    # Ramp completes inside whatever depth this cell was allowed.
    # **Ridged massif relief, not a smooth wall times noise** (systems S18).
    # Fractal noise has rounded maxima, so multiplying a rampart by it gives a
    # lumpy rampart. Folding the noise about its midpoint turns maxima into
    # creases that connect into ridgelines, and the character decides how sharp
    # that fold is and how hard the valleys are flattened afterwards -- which is
    # the difference between an Alpine trough and a Highland whaleback.
    character = str(policy.get("massif_character", "alps"))
    massif = shape_relief(x.shape, rng, character)
    report["massif_character"] = character

    # Relief carves *down* from `height_m`, never up. Raising summits above the
    # authored height buys a ragged skyline by spending the one budget the
    # border is constrained by -- shadow area, measured at 9.9% against an 8%
    # limit when peaks reached 1.55x.
    relief = float(policy.get("crest_relief", 0.0))
    modulation = 1.0 - relief * (1.0 - massif) if relief > 0.0 else np.ones_like(massif)
    if relief > 0.0:
        report["crest_relief"] = relief

    # **The face scales with the local height, so the wall's *angle* is constant.**
    # It did not, and that let the world leak: where the massif dips to a saddle
    # the wall stood 17 m over a 26 m face -- a 34 degree ramp a body walks
    # straight over -- while the summits beside it were sheer. Measured: 330 m
    # of edge reopened across four spans the moment the face was widened for a
    # taller wall. Scaling the ramp by the same modulation that sets the height
    # keeps height/face fixed everywhere, so saddles are lower but no gentler.
    ramp = np.maximum(np.minimum(face, allowed) * modulation, 1.0)
    fraction = np.clip((allowed - inward) / ramp, 0.0, 1.0)
    if play_distance is not None:
        # Wilderness first, then wall. The margin is the belt of open ground
        # outside the lanes that jungling still uses -- it is play space even
        # though no lane runs through it, so the wall starts beyond it.
        margin = float(policy.get("wilderness_margin_m", 26.0))
        valley = np.clip((play_distance - margin) / ramp, 0.0, 1.0)
        fraction = np.maximum(fraction, valley)
        report["wilderness_margin_m"] = margin
    if report["profile"] == "ridge":
        shape = 0.5 - 0.5 * np.cos(np.pi * fraction)
    else:
        # Smoothstep, not a linear ramp. A linear face meets flat ground at a
        # grade discontinuity, and the terrain accessibility gate measures the
        # steepness of *walkable* ground -- the toe cells are walkable, so a
        # hard corner there fails the gate (measured: p99 grade 1.69 -> 2.65).
        # A smoothstep toe leaves the face just as unwalkable while arriving at
        # the valley floor tangentially, which is also what an escarpment
        # actually looks like.
        shape = fraction * fraction * (3.0 - 2.0 * fraction)
    # **A uniform crest is a bowl rim.** Reviewed as "a square bowl with a flat
    # rim" -- which is what a constant-height wall around a square map is. Broad
    # noise along the perimeter breaks the skyline into summits and saddles, so
    # it reads as a range whose near side happens to be the map edge.
    #
    # Relief is cheap in the currency that matters. The black-pixel gate
    # measures *area* in shadow, and raising scattered summits costs far less of
    # it than lifting the whole wall by the same average -- the saddles between
    # them stay lit.
    rampart = peak * shape * modulation

    report.update(
        {
            "crest_m": round(crest, 3),
            "inner_face_m": round(face, 3),
            "height_m": round(peak, 3),
            "maximum_depth_m": round(float(allowed.max()), 3),
            "minimum_depth_m": round(float(allowed.min()), 3),
            # Steepest the face gets: smoothstep peaks at 1.5x its mean slope.
            "peak_face_degrees": round(
                math.degrees(math.atan2(peak * 1.5, max(float(allowed.min()), 1e-6))), 2
            ),
        }
    )
    # The rampart is deliberately unwalkable terrain, so it is tagged as
    # protected relief exactly like the massif. Without that it lands in
    # `background`, the accessibility gate measures its face as walkable ground
    # that happens to be a cliff, and closing the world fails the gate that
    # exists to keep the world walkable -- measured: p99 grade 1.69 -> 7.15.
    #
    # The threshold leaves the shallow toe unprotected on purpose. That part is
    # gentle, genuinely walkable, and should be held to the same standard as
    # any other ground; exempting it would be the gate excusing itself.
    # Raised *or* steep. Height alone misses the last cell or two of a thin
    # face, where the rampart has almost died out but the ground is still
    # tilted past anything a body can stand on -- measured: 3 such cells with a
    # 6 m face. Leaving them in `background` puts a handful of cliff cells into
    # the accessibility statistics, which is precisely the contamination the
    # tagging exists to prevent.
    cell_x = width / (rampart.shape[1] - 1)
    cell_z = length / (rampart.shape[0] - 1)
    rise_z, rise_x = np.gradient(rampart, cell_z, cell_x)
    footprint = (rampart > 0.5) | (np.hypot(rise_x, rise_z) > 1.0)
    report["protected_area_m2"] = round(
        float(footprint.sum()) * (width / (rampart.shape[1] - 1)) * (length / (rampart.shape[0] - 1)), 1
    )
    return height + rampart.astype(height.dtype), report, footprint


def _flatten_landmark_pads(
    height: np.ndarray, x: np.ndarray, z: np.ndarray, features: list[dict[str, Any]]
) -> np.ndarray:
    """Make authored landmark anchors physically spawnable.

    A landmark point is gameplay authority, not merely a place to instance a
    mesh.  A compact terrain can put that point on the shoulder of an otherwise
    valid massif, so flatten a small pad before slope/collision artifacts are
    derived. The eased edge retains the surrounding terrain silhouette.
    """
    result = height.copy()
    for feature in features:
        if feature.get("category") != "landmark":
            continue
        semantic = feature.get("semantic")
        if semantic not in {"faction_keep", "arcane_ruin", "settlement_cluster"}:
            continue
        points = feature.get("geometry", {}).get("points", [])
        if not points:
            continue
        anchor_x, anchor_z = float(points[0][0]), float(points[0][1])
        if semantic == "settlement_cluster":
            radius = max(
                12.0,
                float(
                    feature.get("properties", {}).get(
                        "scatter_exclusion_radius_m", 16.0
                    )
                ),
            )
        elif semantic == "faction_keep":
            radius = 17.0
        else:
            radius = 13.0
        row, column = np.unravel_index(
            np.argmin((x - anchor_x) ** 2 + (z - anchor_z) ** 2), x.shape
        )
        anchor_height = result[row, column]
        distance = np.sqrt((x - anchor_x) ** 2 + (z - anchor_z) ** 2)
        influence = 1.0 - _smoothstep(radius * 0.55, radius, distance)
        result = result * (1.0 - influence) + anchor_height * influence
    return result


def _feature_statistics(mask: np.ndarray, height: np.ndarray, slope: np.ndarray) -> dict[str, float]:
    values = height[mask]
    if values.size == 0:
        return {"area_m2": 0.0, "min_elevation_m": 0.0, "max_elevation_m": 0.0, "relief_m": 0.0, "mean_slope": 0.0}
    low, high = float(values.min()), float(values.max())
    relief = max(high - low, 1e-6)
    return {
        "area_m2": float(mask.sum()),
        "min_elevation_m": round(low, 3),
        "max_elevation_m": round(high, 3),
        "relief_m": round(relief, 3),
        "mean_slope": round(float(slope[mask].mean()), 5),
        "p95_slope": round(float(np.percentile(slope[mask], 95)), 5),
        "high_relief_coverage": round(float((values >= low + relief * 0.65).mean()), 5),
    }


def _sidewall_corrugation(
    mask: np.ndarray,
    height: np.ndarray,
    x: np.ndarray,
    z: np.ndarray,
) -> dict[str, float | int]:
    """Measure repeated high-frequency ribs along a terrain-primary wall.

    A good boundary massif may have an irregular skyline, but its lower faces
    should remain broad connected planes. This samples several longitudinal
    sidewall bands, removes their low-frequency profile, and reports residual
    corrugation as a fraction of total landform relief.
    """
    if not mask.any():
        return {"sidewall_corrugation_ratio": 0.0, "sidewall_profile_count": 0}
    long_axis, cross_axis, long_extent, cross_extent = _principal_axes(
        mask, x, z
    )
    long_normalized = long_axis / max(long_extent, 1.0)
    cross_normalized = cross_axis / max(cross_extent, 1.0)
    edges = np.linspace(-0.84, 0.84, 49)
    residuals: list[float] = []
    relief = max(float(np.ptp(height[mask])), 1e-6)
    for lower, upper in ((0.12, 0.30), (0.30, 0.48), (0.48, 0.68)):
        for side in (-1.0, 1.0):
            band = (
                mask
                & (side * cross_normalized >= lower)
                & (side * cross_normalized < upper)
            )
            profile = []
            for start, finish in zip(edges[:-1], edges[1:]):
                selection = (
                    band
                    & (long_normalized >= start)
                    & (long_normalized < finish)
                )
                profile.append(
                    float(np.median(height[selection]))
                    if selection.any()
                    else np.nan
                )
            values = np.asarray(profile, dtype=np.float64)
            valid = np.isfinite(values)
            if int(valid.sum()) < 12:
                continue
            values = np.interp(
                np.arange(len(values)), np.flatnonzero(valid), values[valid]
            )
            smooth = np.convolve(
                np.pad(values, 4, mode="edge"),
                np.ones(9, dtype=np.float64) / 9.0,
                mode="valid",
            )
            residuals.append(
                float(np.sqrt(np.mean((values - smooth) ** 2)) / relief)
            )
    return {
        "sidewall_corrugation_ratio": round(
            float(np.mean(residuals)) if residuals else 0.0, 6
        ),
        "sidewall_profile_count": len(residuals),
    }


def _principal_axes(mask: np.ndarray, x: np.ndarray, z: np.ndarray) -> tuple[np.ndarray, np.ndarray, float, float]:
    """Return deterministic long/cross coordinates for an irregular landform."""
    points = np.column_stack((x[mask], z[mask]))
    if len(points) < 3:
        axis = np.array([1.0, 0.0], dtype=np.float32)
        center = np.array([0.0, 0.0], dtype=np.float32)
    else:
        center = points.mean(axis=0)
        values, vectors = np.linalg.eigh(np.cov((points - center).T))
        axis = vectors[:, int(np.argmax(values))]
        if axis[0] < 0.0 or (abs(axis[0]) < 1e-6 and axis[1] < 0.0):
            axis = -axis
    dx, dz = x - center[0], z - center[1]
    long_axis = dx * axis[0] + dz * axis[1]
    cross_axis = -dx * axis[1] + dz * axis[0]
    long_extent = max(float(np.percentile(np.abs(long_axis[mask]), 90)) if mask.any() else 1.0, 1.0)
    cross_extent = max(float(np.percentile(np.abs(cross_axis[mask]), 90)) if mask.any() else 1.0, 1.0)
    return long_axis, cross_axis, long_extent, cross_extent


def _scattered_summits(
    mask: np.ndarray,
    edge_distance: np.ndarray,
    x: np.ndarray,
    z: np.ndarray,
    rng: np.random.Generator,
    *,
    minimum_spacing: float,
    count: int,
    scale_range: tuple[float, float],
    lobe_count: int,
    along_ratio: float,
    cross_ratio: float,
) -> np.ndarray:
    """Place separated, multi-lobed angular crag bodies.

    ``lobe_count`` is the authored ``spine_count``: how many overlapping rock
    masses make up each outcrop. ``along_ratio`` and ``cross_ratio`` spread
    those lobes apart along and across the outcrop's own axis, so a crag field
    with zero jitter is a row of single clean cones.
    """
    candidates = np.argwhere(
        mask
        & (
            edge_distance
            > max(
                5.0,
                (
                    float(np.percentile(edge_distance[mask], 15))
                    if mask.any()
                    else 5.0
                ),
            )
        )
    )
    field = np.zeros(x.shape, dtype=np.float32)
    selected: list[tuple[float, float]] = []
    if len(candidates):
        candidate_depth = edge_distance[
            candidates[:, 0], candidates[:, 1]
        ]
        candidate_score = candidate_depth + rng.uniform(
            0.0,
            max(float(candidate_depth.max()) * 0.32, 1.0),
            len(candidates),
        )
        candidate_order = np.argsort(candidate_score)[::-1]
    else:
        candidate_order = []
    for candidate_index in candidate_order:
        row, column = candidates[candidate_index]
        center_x, center_z = float(x[row, column]), float(z[row, column])
        if any((center_x - prior_x) ** 2 + (center_z - prior_z) ** 2 < minimum_spacing**2 for prior_x, prior_z in selected):
            continue
        selected.append((center_x, center_z))
        angle = rng.uniform(0.0, np.pi)
        along = rng.uniform(*scale_range)
        across = rng.uniform(scale_range[0] * 0.30, scale_range[1] * 0.52)
        cluster = np.zeros(x.shape, dtype=np.float32)
        for lobe_index in range(lobe_count):
            lobe_along = (
                0.0
                if lobe_index == 0
                else rng.uniform(
                    -along * 0.42 * along_ratio, along * 0.42 * along_ratio
                )
            )
            lobe_cross = (
                0.0
                if lobe_index == 0
                else rng.uniform(
                    -across * 0.50 * cross_ratio, across * 0.50 * cross_ratio
                )
            )
            lobe_center_x = (
                center_x
                + lobe_along * np.cos(angle)
                - lobe_cross * np.sin(angle)
            )
            lobe_center_z = (
                center_z
                + lobe_along * np.sin(angle)
                + lobe_cross * np.cos(angle)
            )
            dx, dz = x - lobe_center_x, z - lobe_center_z
            long_axis = dx * np.cos(angle) + dz * np.sin(angle)
            cross_axis = -dx * np.sin(angle) + dz * np.cos(angle)
            lobe_scale = rng.uniform(0.58, 1.0)
            radial = np.sqrt(
                (long_axis / max(along * lobe_scale, 1e-6)) ** 2
                + (
                    cross_axis
                    / max(across * rng.uniform(0.62, 1.0), 1e-6)
                )
                ** 2
            )
            # A finite angular cone gives the heightfield visible planes and a
            # crisp summit. Overlapping unequal lobes form a broken outcrop
            # cluster instead of one smooth volcanic mound.
            summit = np.power(
                np.clip(1.0 - radial, 0.0, 1.0),
                rng.uniform(0.62, 0.88),
            )
            cluster = np.maximum(
                cluster,
                summit.astype(np.float32)
                * rng.uniform(0.66, 1.0),
            )
        field = np.maximum(field, cluster)
        if len(selected) >= count:
            break
    peak = float(field[mask].max()) if mask.any() else 0.0
    return field / peak if peak > 0.0 else field


def _style_palette(zone_spec: dict[str, Any], source_root: Path | None = None) -> dict[str, list[float]]:
    """Extract terrain color tokens from source art without using pixels as topology."""
    fallback = {
        "grass": [0.33, 0.43, 0.20],
        "road": [0.47, 0.34, 0.19],
        "rock": [0.20, 0.22, 0.23],
        "snow": [0.88, 0.91, 0.95],
        "water": [0.16, 0.40, 0.54],
    }
    samples: list[np.ndarray] = []
    zone = zone_spec.get("zone", {})
    reconciliation = zone.get("image_reconciliation", {})
    canonical_id = reconciliation.get("canonical_map_image_id")
    terrain_palette_claims = {"style", "material", "biome"}
    claims_by_image: dict[str, set[str]] = {}
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict):
            continue
        for evidence in feature.get("evidence", []):
            if not isinstance(evidence, dict):
                continue
            image_id = evidence.get("image_id")
            claim = evidence.get("claim")
            if isinstance(image_id, str) and isinstance(claim, str):
                claims_by_image.setdefault(image_id, set()).add(claim)
    for source in zone_spec.get("zone", {}).get("source_images", []):
        if not isinstance(source, dict):
            continue
        source_id = source.get("id")
        if (
            source_id != canonical_id
            and not (
                claims_by_image.get(str(source_id), set())
                & terrain_palette_claims
            )
        ):
            continue
        path = source.get("path") if isinstance(source, dict) else None
        source_path = (source_root / path) if isinstance(path, str) and source_root is not None else Path(path) if isinstance(path, str) else None
        if source_path is None or not source_path.is_file():
            continue
        image = Image.open(source_path).convert("RGB").resize((256, 256), Image.Resampling.BILINEAR)
        samples.append(np.asarray(image, dtype=np.float32) / 255.0)
    if not samples:
        return fallback
    pixels = np.concatenate([sample.reshape((-1, 3)) for sample in samples], axis=0)
    red, green, blue = pixels[:, 0], pixels[:, 1], pixels[:, 2]
    groups = {
        "grass": pixels[(green > red * 1.04) & (green > blue * 1.03) & (green > 0.14)],
        "road": pixels[(red > 0.22) & (green > 0.16) & (red > blue * 1.12) & (np.abs(red - green) < 0.28)],
        "rock": pixels[(np.abs(red - green) < 0.10) & (np.abs(green - blue) < 0.10) & (red < 0.55)],
        "snow": pixels[(red > 0.62) & (green > 0.62) & (blue > 0.62)],
        # Use genuinely water-biased samples. The looser selector admitted
        # neutral granite and produced a gray median, erasing the concept's
        # blue-green wetland hierarchy.
        "water": pixels[
            (blue > red * 1.25)
            & (blue > green * 1.08)
            & (blue > 0.18)
        ],
    }
    palette = {}
    for token, candidates in groups.items():
        value = np.median(candidates, axis=0).tolist() if candidates.size >= 30 else fallback[token]
        palette[token] = [round(float(channel), 4) for channel in value]
    return palette


def _style_guidance(
    zone_spec: dict[str, Any],
    resolution: int,
    source_root: Path | None,
    fallback_rgb: list[float],
) -> tuple[np.ndarray, dict[str, Any]]:
    """Create a blurred color-script field from the registered canonical map.

    The blur deliberately removes object-scale pixels: keeps, trees, roads,
    and cliffs remain generated geometry. What survives is regional color and
    value direction that a global palette cannot express.
    """

    zone = zone_spec.get("zone", {})
    canonical_id = (
        zone.get("image_reconciliation", {}).get("canonical_map_image_id")
    )
    source = next(
        (
            item
            for item in zone.get("source_images", [])
            if isinstance(item, dict) and item.get("id") == canonical_id
        ),
        None,
    )
    path = source.get("path") if isinstance(source, dict) else None
    source_path = (
        source_root / path
        if source_root is not None and isinstance(path, str)
        else None
    )
    if source_path is None or not source_path.is_file():
        color = np.asarray(fallback_rgb, dtype=np.float32) * 255.0
        field = np.broadcast_to(color, (resolution, resolution, 3)).astype(
            np.uint8
        )
        return field, {"mode": "palette_fallback", "source_sha256": ""}
    blur_radius = max(4.0, resolution / 48.0)
    image = (
        Image.open(source_path)
        .convert("RGB")
        .resize((resolution, resolution), Image.Resampling.LANCZOS)
        .filter(ImageFilter.GaussianBlur(radius=blur_radius))
    )
    return np.asarray(image, dtype=np.uint8), {
        "mode": "registered_canonical_low_frequency",
        "source_sha256": str(source.get("sha256", "")),
        "blur_radius_px": round(blur_radius, 3),
    }


def rasterize_zone_spec(
    zone_spec: dict[str, Any], *, resolution: int = 1025, source_root: Path | None = None
) -> TerrainRaster:
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        raise ZoneCompileError("Expected %s" % ZONE_SPEC_VERSION)
    if resolution < 33 or resolution > 4097:
        raise ZoneCompileError("resolution must be between 33 and 4097")
    zone = zone_spec.get("zone", {})
    bounds = zone.get("world_bounds", {})
    width, length = float(bounds.get("width", 0)), float(bounds.get("length", 0))
    if width <= 0 or length <= 0:
        raise ZoneCompileError("ZoneSpec has invalid world bounds")
    features = zone_spec.get("features", [])
    if not isinstance(features, list):
        raise ZoneCompileError("ZoneSpec features must be an array")

    x_axis = np.linspace(-width * 0.5, width * 0.5, resolution, dtype=np.float32)
    z_axis = np.linspace(length * 0.5, -length * 0.5, resolution, dtype=np.float32)
    x, z = np.meshgrid(x_axis, z_axis)
    root_rng = np.random.default_rng(int(zone_spec.get("generation_seed", 1)))
    # Gentle, asymmetric highland floor. The landform profiles carry the dramatic relief.
    height = (_fractal_noise(x.shape, root_rng, octaves=(3, 7, 13)) - 0.5) * 8.0
    rock_weight = np.zeros(x.shape, dtype=np.float32)
    snow_weight = np.zeros(x.shape, dtype=np.float32)
    road_weight = np.zeros(x.shape, dtype=np.float32)
    water_weight = np.zeros(x.shape, dtype=np.float32)
    wetland_seed_weight = np.zeros(x.shape, dtype=np.float32)
    landform_masks: list[tuple[dict[str, Any], np.ndarray]] = []
    # Composition scalars as the geometry actually resolved them, so the
    # manifest reports what was built rather than what was merely authored.
    resolved_composition: dict[str, CompositionScalars] = {}

    for feature_index, feature in enumerate(features):
        geometry = feature.get("geometry", {})
        points = geometry.get("points", [])
        if not points:
            continue
        category, semantic = feature.get("category"), feature.get("semantic")
        properties = feature.get("properties", {})
        if category == "landform" and geometry.get("type") == "polygon":
            mask, edge_distance = _polygon_mask(x, z, points)
            landform_masks.append((feature, mask))
            generation = feature.get("generation", {})
            profile = generation.get("profile", "generic_rock")
            local_rng = np.random.default_rng(int(zone_spec.get("generation_seed", 1)) + feature_index * 7919)
            elevation = generation.get("elevation_m", [30.0, 80.0])
            if not isinstance(elevation, list) or len(elevation) != 2:
                raise ZoneCompileError("%s.generation.elevation_m must be [min, max]" % feature["id"])
            floor, peak = float(elevation[0]), float(elevation[1])
            max_edge_distance = max(20.0, float(edge_distance[mask].max()) if mask.any() else 20.0)
            core = np.clip(edge_distance / max_edge_distance, 0.0, 1.0)
            edge_envelope = _smoothstep(
                0.0,
                min(12.0, max(5.0, max_edge_distance * 0.20)),
                edge_distance,
            )
            if profile == "alpine_jagged_massif":
                composition = generation.get("composition", {})
                scalars = _composition_scalars(
                    feature["id"], composition, "ridge_network"
                )
                resolved_composition[feature["id"]] = scalars
                continuous_wall = (
                    composition.get("silhouette")
                    == "continuous_boundary_wall"
                    and composition.get("massing") == "terrain_primary"
                )
                # A terrain-primary boundary massif is one connected mountain
                # body with readable summit variation. Deep Gaussian gaps make
                # a cone collection; a distance field alone makes a mesa.
                broad = _fractal_noise(x.shape, local_rng, octaves=(3, 7, 13))
                sharp = _fractal_noise(x.shape, local_rng, octaves=(11, 25, 53))
                # Build a connected ridge network rather than sprinkling
                # randomly oriented Gaussian humps. The latter can satisfy a
                # slope statistic while reading as worms or volcanoes from an
                # oblique game camera.
                long_axis, cross_axis, long_extent, cross_extent = _principal_axes(mask, x, z)
                long_normalized = long_axis / max(long_extent, 1.0)
                # cross_jitter is how far the spine is allowed to wander off its
                # principal axis: both the noise-driven warp and the deterministic
                # meander scale with it, so 0.0 gives a ruler-straight range.
                cross_warp = scalars.cross_ratio * (
                    (broad - 0.5) * cross_extent * 0.18
                    + np.sin(long_normalized * np.pi * 2.2) * cross_extent * 0.07
                )
                warped_cross = cross_axis + cross_warp
                if continuous_wall:
                    # A boundary wall needs a few large named masses. Too many
                    # summits turn the complete face into a repeated comb when
                    # their saddle rhythm is allowed to modulate the wall.
                    summit_count = min(
                        7,
                        max(5, int(max(long_extent * 2.0, 1.0) / 145.0)),
                    )
                else:
                    summit_count = min(
                        13,
                        max(7, int(max(long_extent * 2.0, 1.0) / 105.0)),
                    )

                def summit_chain(offset: float) -> np.ndarray:
                    chain = np.zeros(x.shape, dtype=np.float32)
                    positions = np.linspace(-0.88, 0.88, summit_count)
                    # along_jitter slides each summit off its nominal station.
                    # At 0.0 the procession is metronomic, which reads as a
                    # fence rather than a mountain range.
                    positions += (
                        local_rng.uniform(-0.055, 0.055, size=summit_count)
                        * scalars.along_ratio
                    )
                    for summit_position in positions:
                        width = local_rng.uniform(0.065, 0.125)
                        amplitude = local_rng.uniform(0.74, 1.0)
                        summit = np.exp(
                            -0.5 * ((long_normalized - summit_position) / width) ** 2
                        )
                        chain = np.maximum(chain, summit.astype(np.float32) * amplitude)
                    saddle_floor = 0.58 if continuous_wall else 0.24
                    return np.clip(
                        saddle_floor + chain * (1.0 - saddle_floor) + offset,
                        0.0,
                        1.0,
                    )

                if continuous_wall:
                    # A triangular cross-section gives a connected wall a
                    # crisp crest and planar cliff faces. A broad Gaussian is
                    # continuous but reads as a chain of soft clay hills.
                    wall_cross = np.clip(
                        1.0
                        - np.abs(warped_cross)
                        / max(cross_extent * 0.92, 24.0),
                        0.0,
                        1.0,
                    )
                    primary_spine = np.power(wall_cross, 1.28)
                else:
                    primary_spine = np.exp(
                        -0.5
                        * (
                            warped_cross
                            / max(cross_extent * 0.23, 20.0)
                        )
                        ** 2
                    )
                primary_summits = summit_chain(0.0)
                if continuous_wall:
                    # Summit/saddle rhythm belongs at the skyline. Multiplying
                    # it through a full triangular spine stamps every summit
                    # into the sidewall as a transverse terrace. Keep a lower,
                    # connected impassable body, then lift only the upper
                    # cross-section into distinct Alpine masses. The previous
                    # 0.64 full-face floor made the compact boundary read as
                    # one extruded cake wall instead of a linked peak chain.
                    crest_weight = np.power(primary_spine, 1.28)
                    normalized_summits = np.clip(
                        (primary_summits - 0.58) / 0.42,
                        0.0,
                        1.0,
                    )
                    primary_chain = primary_spine * (
                        0.42 + 0.58 * crest_weight * normalized_summits
                    )
                else:
                    primary_chain = primary_spine * primary_summits

                # spine_count is the number of parallel ridge lines across the
                # massif: the crest plus its shoulders. Shoulders alternate
                # sides and step outward by rank, so spine_count=3 -- the
                # authored default -- reproduces the hand-tuned inner pair
                # exactly, and higher counts extend the same progression.
                #
                # Every shoulder reuses the authored summit procession with a
                # shifted saddle rhythm rather than recomputing full-grid
                # Gaussian fields, which was visually equivalent and far
                # cheaper for iterative concept builds.
                shoulder_profiles = (
                    {  # inner left
                        "sign": -1.0,
                        "offset": 0.30 if continuous_wall else 0.38,
                        "wobble": 0.06,
                        "wave": np.sin(long_normalized * np.pi * 1.7),
                        "width": 0.25 if continuous_wall else 0.15,
                        "summit_shift": 0.11,
                        "harmonic_delta": 0,
                        "summit_mix": (0.72, 0.28),
                        "wall_chain": (0.36, 0.12, 0.60),
                        "cluster_chain": 0.72,
                    },
                    {  # inner right
                        "sign": 1.0,
                        "offset": 0.29 if continuous_wall else 0.36,
                        "wobble": 0.07,
                        "wave": np.cos(long_normalized * np.pi * 1.9),
                        "width": 0.25 if continuous_wall else 0.16,
                        "summit_shift": -0.09,
                        "harmonic_delta": -1,
                        "summit_mix": (0.68, 0.32),
                        "wall_chain": (0.34, 0.12, 0.58),
                        "cluster_chain": 0.68,
                    },
                )
                chains = [primary_chain]
                for shoulder_index in range(1, scalars.spine_count):
                    shoulder = shoulder_profiles[(shoulder_index - 1) % 2]
                    rank = (shoulder_index + 1) // 2
                    # Outer shoulders are subordinate foothill ridges. Without
                    # the falloff a high spine_count builds a corduroy field of
                    # equal ridges instead of one massif with flanks.
                    falloff = 0.82 ** (rank - 1)
                    shoulder_offset = cross_extent * (
                        shoulder["offset"] * rank
                        + shoulder["wobble"]
                        * scalars.cross_ratio
                        * shoulder["wave"]
                    )
                    shoulder_spine = np.exp(
                        -0.5
                        * (
                            (warped_cross + shoulder["sign"] * shoulder_offset)
                            / max(cross_extent * shoulder["width"], 15.0)
                        )
                        ** 2
                    )
                    low, high = shoulder["summit_mix"]
                    harmonic = max(
                        2,
                        summit_count + shoulder["harmonic_delta"] - (rank - 1),
                    )
                    shoulder_summits = np.clip(
                        primary_summits * low
                        + (
                            1.0
                            - np.abs(
                                np.sin(
                                    (
                                        long_normalized
                                        + shoulder["summit_shift"] * rank
                                    )
                                    * np.pi
                                    * harmonic
                                )
                            )
                        )
                        * high,
                        0.18,
                        1.0,
                    )
                    if continuous_wall:
                        base, gain, scale = shoulder["wall_chain"]
                        chains.append(
                            shoulder_spine
                            * (
                                base
                                + gain
                                * np.power(shoulder_spine, 1.6)
                                * shoulder_summits
                            )
                            * scale
                            * falloff
                        )
                    else:
                        chains.append(
                            shoulder_spine
                            * shoulder_summits
                            * shoulder["cluster_chain"]
                            * falloff
                        )
                ridge_network = np.maximum.reduce(chains)
                ridge_network *= (
                    0.56 + 0.44 * np.power(core, 0.58)
                    if continuous_wall
                    else 0.24 + 0.76 * np.power(core, 0.62)
                )
                broad_aretes = 1.0 - np.abs(2.0 * broad - 1.0)
                fine_aretes = 1.0 - np.abs(2.0 * sharp - 1.0)
                ridged_detail = (
                    0.68
                    + 0.20 * broad_aretes
                    + 0.12 * fine_aretes
                )
                foothill = (
                    floor
                    * (1.0 - np.exp(-edge_distance / max(20.0, max_edge_distance * 0.18)))
                    * (0.68 if continuous_wall else 0.48)
                )
                summit_lift = (
                    (peak - floor)
                    * np.power(ridge_network, 1.32)
                    * (0.11 + 0.12 * fine_aretes)
                )
                massif = (
                    foothill
                    + (peak - floor) * ridge_network * ridged_detail
                    + summit_lift
                )
                if continuous_wall:
                    # Give the broad heightfield face named geological masses.
                    # Two low-frequency buttress/gully cycles run from talus
                    # to upper flank; unlike summit teeth they do not stamp
                    # horizontal terraces through the wall.
                    face_band = _smoothstep(
                        0.12, 0.34, primary_spine
                    ) * (
                        1.0
                        - _smoothstep(
                            0.78, 0.96, primary_spine
                        )
                    )
                    buttress_phase = (
                        long_normalized + 0.13
                    ) * np.pi * 2.0
                    buttresses = np.power(
                        0.5 + 0.5 * np.cos(buttress_phase),
                        1.65,
                    )
                    flank_structure = (
                        (buttresses - 0.34)
                        * (peak - floor)
                        * 0.065
                        * face_band
                        * (0.58 + 0.42 * np.power(core, 0.7))
                    )
                    # Broad debris fans continue the buttresses into the base
                    # without lifting the complete polygon into a shelf.
                    lower_flank = _smoothstep(
                        0.08, 0.24, primary_spine
                    ) * (
                        1.0
                        - _smoothstep(
                            0.34, 0.52, primary_spine
                        )
                    )
                    talus_fans = (
                        np.power(buttresses, 1.25)
                        * (peak - floor)
                        * 0.035
                        * lower_flank
                    )
                    massif += flank_structure + talus_fans
                    # elevation_bias is the model-facing silhouette control.
                    # Compact arenas otherwise make a reviewed Alpine border
                    # read as a low berm even when its topology is correct.
                    massif *= 0.80 + 0.70 * scalars.elevation_bias
                height += mask * edge_envelope * massif
                cliffness = float(generation.get("cliffness", 0.75))
                rock_weight = np.maximum(rock_weight, mask * np.clip(0.42 + cliffness * (1.0 - core * 0.25), 0.0, 1.0))
                # The source uses snow as a broken ridge accent.  A generic
                # height threshold paints an entire alpine polygon white when
                # viewed from above, destroying the intended granite/arête
                # read.  Keep snow only on the last part of steep high ridges.
                snowline = max(float(generation.get("snowline_m", peak * 0.72)), peak * 0.88)
                ridge_snow = _smoothstep(snowline, peak, height) ** 2.6
                snow_weight = np.maximum(
                    snow_weight,
                    mask * ridge_snow,
                )
            elif profile == "alpine_sawtooth_ridge":
                long_axis, cross_axis, long_extent, cross_extent = _principal_axes(mask, x, z)
                # A ridge is a continuous spine with repeated high points and
                # saddles along its long axis, not a row of cones or a mesa.
                spine = np.exp(-0.5 * (cross_axis / max(cross_extent * 0.38, 18.0)) ** 2)
                ridge_phase = (long_axis / max(long_extent, 1.0) + 1.0) * np.pi * local_rng.integers(3, 6)
                teeth = 0.32 + 0.68 * (1.0 - np.abs(np.sin(ridge_phase)))
                aretes = 0.50 + 0.50 * (1.0 - np.abs(2.0 * _fractal_noise(x.shape, local_rng, octaves=(9, 21, 43)) - 1.0))
                ridge = floor * (1.0 - np.exp(-edge_distance / max(18.0, cross_extent * 0.26))) * 0.42
                ridge += (peak - floor) * spine * teeth * aretes * (0.45 + 0.55 * core)
                height += mask * edge_envelope * ridge
                cliffness = float(generation.get("cliffness", 0.72))
                rock_weight = np.maximum(rock_weight, mask * np.clip(0.32 + cliffness * (0.35 + spine * 0.45), 0.0, 1.0))
                snowline = float(generation.get("snowline_m", peak * 0.84))
                snow_weight = np.maximum(snow_weight, mask * _smoothstep(snowline, peak, height) ** 2.8 * spine)
            elif profile == "rolling_foothills":
                # Foothills deliberately preserve broad undulation and open
                # traversable shoulders; using Alpine arêtes here would be a
                # semantic substitution even if the output looked dramatic.
                feature_noise = _fractal_noise(x.shape, local_rng, octaves=(5, 13, 27))
                height += (
                    mask
                    * edge_envelope
                    * (floor + (peak - floor) * core * (0.4 + 0.6 * feature_noise))
                )
                rock_weight = np.maximum(rock_weight, mask * (0.20 + (1.0 - core) * 0.30))
            elif profile == "scattered_crag_field":
                composition = generation.get("composition", {})
                scalars = _composition_scalars(
                    feature["id"], composition, "clustered_ridges"
                )
                resolved_composition[feature["id"]] = scalars
                elevation_bias = scalars.elevation_bias
                crags = _scattered_summits(
                    mask, edge_distance, x, z, local_rng,
                    minimum_spacing=10.0,
                    count=min(
                        7,
                        max(
                            3,
                            int(
                                mask.sum()
                                / max(1.0, resolution * resolution)
                                * 26.0
                            ),
                        ),
                    ),
                    scale_range=(6.0, 14.0),
                    lobe_count=scalars.spine_count,
                    along_ratio=scalars.along_ratio,
                    cross_ratio=scalars.cross_ratio,
                )
                long_axis, cross_axis, long_extent, cross_extent = (
                    _principal_axes(mask, x, z)
                )
                # Crag fields are terrain formations, not a request to scatter
                # cone meshes. Join the authored high points with broad,
                # broken shoulders so the result reads as one eroded rock
                # system from both the overview and player cameras.
                broad = _fractal_noise(
                    x.shape, local_rng, octaves=(3, 7, 15)
                )
                fractured = 0.62 + 0.38 * (
                    1.0
                    - np.abs(
                        2.0
                        * _fractal_noise(
                            x.shape, local_rng, octaves=(11, 27, 59)
                        )
                        - 1.0
                    )
                )
                ridge_phase = (
                    long_axis / max(long_extent, 1.0) * np.pi * 2.6
                    + (broad - 0.5) * np.pi * 0.7
                )
                ridge_center = (
                    cross_axis
                    - np.sin(ridge_phase)
                    * max(8.0, cross_extent * 0.18)
                    * scalars.cross_ratio
                )
                connected_shoulder = np.exp(
                    -0.5
                    * (
                        ridge_center
                        / max(5.0, cross_extent * 0.20)
                    )
                    ** 2
                )
                connected_shoulder *= (
                    0.38
                    + 0.42 * np.power(core, 0.72)
                    + 0.20 * broad
                )
                crag_edge = _smoothstep(
                    1.5,
                    min(
                        10.0,
                        max(5.0, max_edge_distance * 0.62),
                    ),
                    edge_distance,
                )
                crag_mass = np.maximum(
                    crags,
                    connected_shoulder.astype(np.float32) * 0.54,
                )
                crag_mass *= crag_edge * (
                    0.34 + 0.66 * np.power(core, 0.58)
                )
                relief_scale = 1.30 + 1.50 * np.clip(
                    elevation_bias, 0.0, 1.0
                )
                talus = (
                    floor
                    * np.power(np.clip(crag_mass, 0.0, 1.0), 0.74)
                    * 0.24
                )
                height += (
                    mask
                    * (
                        talus
                        + (peak - floor)
                        * crag_mass
                        * fractured
                        * relief_scale
                    )
                )
                rock_weight = np.maximum(
                    rock_weight,
                    mask
                    * np.clip(
                        0.38
                        + crag_mass * 0.50
                        + connected_shoulder * 0.12,
                        0.0,
                        1.0,
                    ),
                )
            elif profile == "cliff_escarpment":
                long_axis, cross_axis, _long_extent, cross_extent = _principal_axes(mask, x, z)
                # One side rises through a compressed, broken transition. This
                # makes a readable escarpment face instead of a symmetric hill.
                normalized_cross = cross_axis / max(cross_extent, 1.0)
                step = _smoothstep(-0.18, 0.20, normalized_cross)
                broken = 0.82 + 0.18 * _fractal_noise(x.shape, local_rng, octaves=(7, 17, 37))
                shelf = 0.18 + 0.82 * step
                height += (
                    mask
                    * edge_envelope
                    * (
                        floor * core * 0.22
                        + (peak - floor)
                        * shelf
                        * broken
                        * (0.58 + 0.42 * core)
                    )
                )
                cliffness = float(generation.get("cliffness", 0.80))
                face = np.exp(-0.5 * ((normalized_cross - 0.02) / 0.18) ** 2)
                rock_weight = np.maximum(rock_weight, mask * np.clip(0.30 + cliffness * (0.30 + face * 0.58), 0.0, 1.0))
            elif profile == "glacial_valley_floor":
                long_axis, cross_axis, _long_extent, cross_extent = _principal_axes(mask, x, z)
                # U-shaped trough: broad flat floor, steep-ish shoulders, and
                # no accidental needle peaks from the generic terrain noise.
                normalized_cross = np.abs(cross_axis) / max(cross_extent, 1.0)
                trough = np.exp(-0.5 * (normalized_cross / 0.46) ** 2)
                shoulder = _smoothstep(0.38, 0.92, normalized_cross)
                depth = float(generation.get("depth_m", (peak - floor) * 0.55))
                height += (
                    mask
                    * edge_envelope
                    * (
                        floor * core * 0.30
                        + (peak - floor) * shoulder * 0.42
                        - depth * trough * (0.56 + 0.44 * core)
                    )
                )
                rock_weight = np.maximum(rock_weight, mask * _smoothstep(0.42, 0.88, normalized_cross) * 0.48)
            else:
                raise ZoneCompileError("Unsupported landform profile %s" % profile)
        elif category == "corridor" and semantic == "lane" and geometry.get("type") == "polyline":
            grade_half_width = (
                float(properties.get("minimum_width_m", 20.0)) * 0.5
            )
            visual_half_width = (
                float(properties.get("visual_width_m", grade_half_width * 2.0))
                * 0.5
            )
            route = _catmull_rom_route(points)
            distance = _polyline_distance(x, z, route)
            surface_influence = 1.0 - _smoothstep(
                visual_half_width * 0.88,
                visual_half_width * 1.08,
                distance,
            )
            road_weight = np.maximum(road_weight, surface_influence)
        elif category == "hydrology" and semantic in {"river", "stream"} and geometry.get("type") == "polyline":
            half_width = float(properties.get("width_m", 8.0)) * 0.5
            distance = _polyline_distance(x, z, points)
            channel_profile = str(
                properties.get("channel_profile", "surface_channel")
            )
            if channel_profile == "wetland_rill":
                # The centerline is evidence for a saturated region, not a
                # promise of one continuous river. Break it into broad,
                # irregular shallow pools and retain a softer marsh field.
                wetland_outer = max(10.0, half_width + 9.0)
                wetland_influence = 1.0 - _smoothstep(
                    half_width, wetland_outer, distance
                )
                breakup = _fractal_noise(
                    x.shape, local_rng, octaves=(5, 13, 29)
                )
                pool_gate = _smoothstep(0.54, 0.72, breakup)
                water_weight = np.maximum(
                    water_weight, wetland_influence * pool_gate
                )
                wetland_seed_weight = np.maximum(
                    wetland_seed_weight,
                    wetland_influence * (0.52 + breakup * 0.48),
                )
            else:
                outer_bank = max(half_width * 2.5, half_width + 1.5)
                influence = 1.0 - _smoothstep(
                    half_width * 0.75, outer_bank, distance
                )
                water_weight = np.maximum(water_weight, influence)

    # Roads and water modify the surface after all landforms. A road mask must
    # be a gameplay grade, not merely a different colour painted over crags.
    # Broadly smoothing the authored terrain preserves large-scale elevation
    # while removing obstacle-scale spikes from the full lane width.
    # The world closes itself before roads and water are reconciled, so a route
    # or a channel authored to reach the edge still cuts its own notch through
    # the rampart rather than being buried by it.
    height, border_report, border_footprint = _border_rampart(
        height, x, z, features, width, length,
        zone_spec.get("border_policy") or {},
        np.random.default_rng(int(zone_spec.get("generation_seed", 1)) * 7919 + 104729),
    )
    height = _flatten_landmark_pads(height, x, z, features)
    height = _grade_corridors(height, x, z, features, width, length)
    height = _carve_hydrology(height, x, z, features, width, length)
    # Channel banks overlap at tributary confluences. Re-solving the network
    # after the first depth pass propagates the downhill invariant through
    # those shared cells without repeatedly deepening every bed.
    for _hydrology_pass in range(3):
        height = _carve_hydrology(
            height,
            x,
            z,
            features,
            width,
            length,
            apply_channel_depth=False,
        )
    # Compact wetland rills may cross the authored lanes many times. Their
    # shallow conformance pass must not reintroduce saw-tooth longitudinal or
    # cross slopes into an already certified roadbed. Reconcile roads last;
    # true incised channels retain explicit bridge/off-mesh semantics in the
    # render and navigation plans rather than relying on a terrain trench to
    # make the crossing impassable.
    # Systems S2: cut the world an outlet.
    #
    # Closing the border (S4) made the map a closed basin -- the interior floor
    # sits below the lowest point of the rim, so nothing drains and every drop
    # that lands stays. Physically honest, and directly against an art
    # direction that asks for water running off the edge.
    #
    # Where it goes is measured, not authored: the rim cell with the most
    # drainage arriving behind it per metre of rock in the way. Lowest-point
    # alone would notch wherever the rim dips even if nothing flows there;
    # wettest-alone would drive a gorge through a summit.
    #
    # Runs on the coarse grid for the same reason S1 does -- a pure-Python
    # priority flood over a million cells is not a trade worth making -- then
    # the notch is carved at full resolution.
    outlet_report: dict[str, Any] = {"enabled": False}
    if bool((zone_spec.get("border_policy") or {}).get("enabled", True)):
        coarse_side = min(257, resolution)
        step = max(1, (resolution - 1) // (coarse_side - 1))
        coarse = height[::step, ::step]
        coarse_filled = fill_depressions(coarse.astype(np.float64))
        coarse_flow = flow_accumulation(coarse_filled)
        outlet = choose_outlet(coarse.astype(np.float64), coarse_flow)
        scale = (resolution - 1) / (coarse.shape[0] - 1)
        full = Outlet_(
            row=int(round(outlet.row * scale)),
            column=int(round(outlet.column * scale)),
        )
        height = carve_outlet(
            height, full.row, full.column, cell_m=width / (resolution - 1)
        )
        outlet_report = {
            "enabled": True,
            "edge": outlet.edge,
            "world_m": [
                round(float(x[full.row, full.column]), 3),
                round(float(z[full.row, full.column]), 3),
            ],
            "spill_height_m": round(outlet.spill_height_m, 3),
            "catchment_cells": round(outlet.catchment_cells, 1),
            "derivation": "max_drainage_per_metre_of_rim",
        }

    height = _flatten_landmark_pads(height, x, z, features)
    height = _grade_corridors(height, x, z, features, width, length)
    wetland_radius = max(2, int(np.ceil(float(resolution) / 72.0)))
    wetland_source = np.maximum(water_weight, wetland_seed_weight)
    wetland_weight = np.clip(
        _broad_blur(wetland_source, wetland_radius) * 1.85
        - water_weight * 0.58,
        0.0,
        1.0,
    )
    wetland_weight *= 1.0 - road_weight * 0.92
    # A selected road surface remains road rather than inheriting the rock/snow
    # weights of the landform it intentionally traverses.
    rock_weight *= 1.0 - road_weight * 0.88
    snow_weight *= 1.0 - road_weight * 0.96
    spacing_x, spacing_z = width / (resolution - 1), length / (resolution - 1)
    gradient_z, gradient_x = np.gradient(height, spacing_z, spacing_x)
    slope = np.sqrt(gradient_x * gradient_x + gradient_z * gradient_z)
    # Standing/surface water cannot conform to a cliff face. Source-art water
    # polygons may overlap a massif in image space; retain their wetland
    # influence, but remove the renderable water surface as grade approaches
    # a 24-degree face. True waterfalls require an explicit vertical-water
    # semantic and mesh rather than blue terrain paint.
    water_weight *= 1.0 - _smoothstep(0.18, 0.45, slope)
    # Systems S7/S5: surfacing derived from site conditions rather than from
    # height and slope alone.
    #
    # **Scree collects where soil cannot.** Slope alone puts bare rock on every
    # steep face including the hollows, where in reality debris and soil gather.
    # Convex ground -- ridges, shoulders, spurs -- is scoured; concave ground
    # collects. Curvature is what separates them, and it is the same field S1
    # emits so the surfacing and the vegetation will agree about which is which.
    curvature = np.gradient(gradient_z, spacing_z, axis=0) + np.gradient(
        gradient_x, spacing_x, axis=1
    )
    exposure_boost = _smoothstep(0.0, 0.9, curvature)
    rock_weight = np.maximum(
        rock_weight,
        _smoothstep(0.55, 1.35, slope) * (0.72 + 0.28 * exposure_boost),
    )

    # **Snow keeps to the shade, and that is most of what makes a range read as
    # alpine.** The old rule put the snowline at 93% of the tallest point *and*
    # required the ground to already be rock and steep, so a world whose peak
    # was one massif had effectively no snow anywhere -- and now that every
    # flank carries Alpine relief (S18) that is the difference between a range
    # and a grey lump.
    #
    # Insolation is the same calculation S1 emits: the cosine of the angle
    # between the surface and the sun. South faces bake and clear; north faces
    # hold snow hundreds of metres lower. Aspect asymmetry is the signature.
    sun_altitude = math.radians(42.0)
    sun_azimuth = math.radians(180.0)
    slope_radians = np.arctan(slope)
    aspect_radians = np.arctan2(-gradient_x, gradient_z)
    insolation = np.clip(
        np.cos(slope_radians) * math.sin(sun_altitude)
        + np.sin(slope_radians) * math.cos(sun_altitude) * np.cos(sun_azimuth - aspect_radians),
        0.0,
        1.0,
    )
    relief = max(float(height.max()) - float(height.min()), 1e-6)
    # Shaded ground holds snow from 58% of the relief; sunlit ground not until
    # 88%. The band between them is where the asymmetry shows.
    snowline = float(height.min()) + relief * (0.58 + 0.30 * insolation)
    snow_weight = np.maximum(
        snow_weight,
        _smoothstep(snowline, snowline + relief * 0.16, height) ** 1.4,
    )
    # Snow does not cling to a vertical face; it slides off and lands below.
    snow_weight *= 1.0 - _smoothstep(1.6, 3.0, slope)
    grass_weight = np.clip(1.0 - road_weight - rock_weight * 0.9 - snow_weight, 0.0, 1.0)
    total = np.maximum(grass_weight + road_weight + rock_weight + snow_weight, 1e-6)
    splat = np.stack([grass_weight / total, road_weight / total, rock_weight / total, snow_weight / total], axis=-1)

    normal = np.stack([-gradient_x, np.ones_like(height), -gradient_z], axis=-1)
    normal /= np.maximum(np.linalg.norm(normal, axis=-1, keepdims=True), 1e-6)
    normal_rgb = ((normal * 0.5 + 0.5) * 255.0).astype(np.uint8)
    style_palette = _style_palette(zone_spec, source_root)
    style_guidance, style_guidance_contract = _style_guidance(
        zone_spec, resolution, source_root, style_palette["grass"]
    )
    palette = np.array([style_palette["grass"], style_palette["road"], style_palette["rock"], style_palette["snow"]], dtype=np.float32) * 255.0
    preview = np.tensordot(splat, palette, axes=([2], [0]))
    preview = preview * (0.62 + 0.38 * np.clip(normal[:, :, 1:2], 0.0, 1.0))
    preview = preview * (1.0 - wetland_weight[:, :, None] * 0.22)
    # Water in the reference is shallow Highland wetland: dark, peat-stained,
    # and integrated into its banks. A thresholded cyan replacement made every
    # pool read as a plastic cutout. Blend continuously so bank fragments retain
    # ground variation and only channel cores reach the cool slate-teal target.
    water_blend = _smoothstep(0.12, 0.72, water_weight)[:, :, None]
    water_color = np.array([18, 54, 61], dtype=np.float32)
    water_surface = preview * 0.22 + water_color * 0.78
    preview = preview * (1.0 - water_blend) + water_surface * water_blend

    protected_relief_mask = np.zeros(height.shape, dtype=np.uint8)
    semantic_region_mask = np.zeros(height.shape, dtype=np.uint8)
    if border_footprint.any():
        protected_relief_mask[border_footprint] = 255
        semantic_region_mask[border_footprint] |= REGION_PROTECTED_RELIEF
    for feature, mask in landform_masks:
        if bool(feature.get("properties", {}).get("traversable", False)):
            semantic_region_mask[mask] |= REGION_TRAVERSABLE_LANDFORM
        else:
            protected_relief_mask[mask] = 255
            semantic_region_mask[mask] |= REGION_PROTECTED_RELIEF
    semantic_region_mask[road_weight >= 0.50] |= REGION_LANE
    semantic_region_mask[water_weight >= 0.50] |= REGION_HYDROLOGY
    for feature in features:
        if feature.get("category") != "landmark":
            continue
        semantic = feature.get("semantic")
        if semantic not in {"faction_keep", "arcane_ruin", "settlement_cluster"}:
            continue
        points = feature.get("geometry", {}).get("points", [])
        if not points:
            continue
        anchor_x, anchor_z = float(points[0][0]), float(points[0][1])
        pad_radius = 10.0 if semantic == "settlement_cluster" else 6.0
        landmark_pad = (
            (x - anchor_x) ** 2 + (z - anchor_z) ** 2 <= pad_radius**2
        )
        semantic_region_mask[landmark_pad] |= REGION_LANDMARK_PAD

    manifest = {
        "schema_version": "codeweald.terrain-artifacts/v1",
        "zone_spec": ZONE_SPEC_VERSION,
        "zone_spec_sha256": hashlib.sha256(
            json.dumps(
                zone_spec,
                ensure_ascii=False,
                separators=(",", ":"),
                sort_keys=True,
            ).encode("utf-8")
        ).hexdigest(),
        # The identity of the terrain that was actually built. Comparing this
        # across two builds is how a caller proves an authoring change reached
        # the geometry; without it a knob can be "applied" forever with no
        # effect and nothing in the pipeline notices.
        "heightfield_sha256": hashlib.sha256(
            np.ascontiguousarray(height, dtype="<f4").tobytes()
        ).hexdigest(),
        "zone_id": zone["id"],
        "resolution": resolution,
        "world_bounds_m": {"width": width, "length": length},
        # What the border solver actually built, including whether a landmark
        # forced it to give ground. `boundary_plan` says whether it worked;
        # this says what was attempted, so a still-leaking world is diagnosable
        # without re-deriving the rampart by hand.
        "border": border_report,
        "outlet": outlet_report,
        "height_range_m": {"min": round(float(height.min()), 3), "max": round(float(height.max()), 3)},
        "artifacts": {
            "heightmap_16": "heightmap_16.png",
            "splatmap": "splatmap.png",
            "normalmap": "normalmap.png",
            "water_mask": "water_mask.png",
            "wetland_mask": "wetland_mask.png",
            "heightfield_f32le": "heightfield_f32le.bin",
            "protected_relief_mask": "protected_relief_mask.bin",
            "semantic_region_mask": "semantic_region_mask.bin",
            "style_guidance": "style_guidance.png",
            "preview": "terrain_preview.png",
        },
        "semantic_region_bits": {
            "protected_relief": int(REGION_PROTECTED_RELIEF),
            "traversable_landform": int(REGION_TRAVERSABLE_LANDFORM),
            "lane": int(REGION_LANE),
            "hydrology": int(REGION_HYDROLOGY),
            "landmark_pad": int(REGION_LANDMARK_PAD),
        },
        # Julia consumes the engine-neutral authored centerlines together with
        # the canonical heightfield. A hydrology mask alone can measure bank
        # roughness, but cannot prove that a river flows downhill along its
        # semantic route.
        "hydrology_centerlines": [
            {
                "id": str(feature.get("id", "")),
                "channel_profile": str(
                    feature.get("properties", {}).get(
                        "channel_profile", "surface_channel"
                    )
                ),
                "points": [
                    [float(point[0]), float(point[1])]
                    for point in feature.get("geometry", {}).get("points", [])
                ],
            }
            for feature in features
            if feature.get("category") == "hydrology"
            and feature.get("semantic") in {"river", "stream"}
            and feature.get("geometry", {}).get("type") == "polyline"
        ],
        "channel_convention": {"splatmap": {"r": "grass", "g": "road", "b": "rock", "a": "snow"}},
        "wetland_coverage_fraction": round(
            float((wetland_weight >= 0.20).mean()), 6
        ),
        "steep_surface_water_fraction": round(
            float(((water_weight >= 0.20) & (slope >= 0.45)).mean()), 8
        ),
        "style_palette_srgb": style_palette,
        "style_guidance": style_guidance_contract,
        "landforms": [
            {
                "id": feature["id"],
                "semantic": feature["semantic"],
                "profile": feature["generation"].get("profile"),
                "art_profile": {
                    key: feature["generation"].get("composition", {}).get(key)
                    for key in ("silhouette", "massing", "surface", "dressing")
                },
                # The scalars as the geometry resolved them, defaults included.
                # A repair that changes these without changing
                # heightfield_sha256 is a no-op, and the critic says so.
                "composition_scalars": (
                    {
                        "spine_count": resolved_composition[feature["id"]].spine_count,
                        "along_jitter": round(
                            resolved_composition[feature["id"]].along_jitter, 6
                        ),
                        "cross_jitter": round(
                            resolved_composition[feature["id"]].cross_jitter, 6
                        ),
                        "elevation_bias": round(
                            resolved_composition[feature["id"]].elevation_bias, 6
                        ),
                    }
                    if feature["id"] in resolved_composition
                    else None
                ),
                **_feature_statistics(mask, height, slope),
                **(
                    _sidewall_corrugation(mask, height, x, z)
                    if feature["generation"].get("composition", {}).get(
                        "silhouette"
                    )
                    == "continuous_boundary_wall"
                    else {}
                ),
                "rock_coverage": round(float((rock_weight[mask] >= 0.45).mean()) if mask.any() else 0.0, 5),
                "snow_coverage": round(float((snow_weight[mask] >= 0.30).mean()) if mask.any() else 0.0, 5),
            }
            for feature, mask in landform_masks
        ],
    }
    return TerrainRaster(
        height,
        (splat * 255.0).astype(np.uint8),
        normal_rgb,
        (water_weight * 255.0).astype(np.uint8),
        (wetland_weight * 255.0).astype(np.uint8),
        protected_relief_mask,
        semantic_region_mask,
        style_guidance,
        np.clip(preview, 0, 255).astype(np.uint8),
        manifest,
    )


def _read(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as source:
        return json.load(source)


def _save_png_atomic(image: Image.Image, destination: Path) -> None:
    temporary = destination.with_name("." + destination.name + ".tmp")
    image.save(temporary, format="PNG")
    temporary.replace(destination)


def write_raster(raster: TerrainRaster, output_dir: Path) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    normalized = (raster.height_m - raster.height_m.min()) / max(float(raster.height_m.max() - raster.height_m.min()), 1e-6)
    _save_png_atomic(
        Image.fromarray((normalized * 65535.0).astype(np.uint16)),
        output_dir / "heightmap_16.png",
    )
    _save_png_atomic(
        Image.fromarray(raster.splat_rgba, mode="RGBA"),
        output_dir / "splatmap.png",
    )
    _save_png_atomic(
        Image.fromarray(raster.normal_rgb, mode="RGB"),
        output_dir / "normalmap.png",
    )
    _save_png_atomic(
        Image.fromarray(raster.water_mask, mode="L"),
        output_dir / "water_mask.png",
    )
    _save_png_atomic(
        Image.fromarray(raster.wetland_mask, mode="L"),
        output_dir / "wetland_mask.png",
    )
    raster.height_m.astype("<f4", copy=False).tofile(
        output_dir / "heightfield_f32le.bin"
    )
    raster.protected_relief_mask.astype(np.uint8, copy=False).tofile(
        output_dir / "protected_relief_mask.bin"
    )
    raster.semantic_region_mask.astype(np.uint8, copy=False).tofile(
        output_dir / "semantic_region_mask.bin"
    )
    _save_png_atomic(
        Image.fromarray(raster.style_guidance_rgb, mode="RGB"),
        output_dir / "style_guidance.png",
    )
    _save_png_atomic(
        Image.fromarray(raster.preview_rgb, mode="RGB"),
        output_dir / "terrain_preview.png",
    )
    # Keep the manifest this build replaces. Two consecutive manifests are all
    # the critic needs to prove whether an authoring change reached geometry,
    # and rotating here means no caller has to remember to snapshot first.
    manifest_path = output_dir / "terrain_manifest.json"
    if manifest_path.exists():
        (output_dir / "terrain_manifest.previous.json").write_bytes(
            manifest_path.read_bytes()
        )
    with manifest_path.open("w", encoding="utf-8") as destination:
        json.dump(raster.manifest, destination, indent=2, sort_keys=True)
        destination.write("\n")


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Compile ZoneSpec terrain height, material, normal, and water artifacts")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--resolution", type=int, default=1025)
    args = parser.parse_args(argv)
    try:
        raster = rasterize_zone_spec(_read(args.zone_spec), resolution=args.resolution, source_root=args.zone_spec.parent)
        write_raster(raster, args.output_dir)
    except (OSError, json.JSONDecodeError, ZoneCompileError) as exc:
        parser.error(str(exc))
    print("Rasterized %s at %dx%d (%.1fm to %.1fm)" % (raster.manifest["zone_id"], args.resolution, args.resolution, raster.manifest["height_range_m"]["min"], raster.manifest["height_range_m"]["max"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
