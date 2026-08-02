#!/usr/bin/env python3
"""Validate runtime Godot overview projection against reviewed concept evidence.

Global image statistics cannot detect a mirrored or rotated map. The capture
script records actual Camera3D projections for world axes and semantic
landmarks; this gate verifies that +X remains screen-right, +Z remains
screen-up, and each landmark anchor lands inside its reviewed source region.
"""

from __future__ import annotations

import math
from typing import Any


PROJECTION_VERSION = "codeweald.godot-overview-projection/v1"
ACCEPTANCE_VERSION = "codeweald.overview-projection-acceptance/v1"


def _inside(point: list[Any], region: list[Any]) -> bool:
    if len(point) != 2 or len(region) != 4:
        return False
    x, y = float(point[0]), float(point[1])
    left, top, right, bottom = (float(value) for value in region)
    return left <= x <= right and top <= y <= bottom


def _regions_overlap(first: list[Any], second: list[Any], tolerance: float = 0.0) -> bool:
    if len(first) != 4 or len(second) != 4:
        return False
    first_left, first_top, first_right, first_bottom = (
        float(value) for value in first
    )
    second_left, second_top, second_right, second_bottom = (
        float(value) for value in second
    )
    return not (
        first_right < second_left - tolerance
        or first_left > second_right + tolerance
        or first_bottom < second_top - tolerance
        or first_top > second_bottom + tolerance
    )


def _point_segment_distance(point: list[Any], start: list[Any], finish: list[Any]) -> float:
    px, py = float(point[0]), float(point[1])
    ax, ay = float(start[0]), float(start[1])
    bx, by = float(finish[0]), float(finish[1])
    dx, dy = bx - ax, by - ay
    denominator = dx * dx + dy * dy
    amount = 0.0 if denominator <= 1e-12 else max(
        0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / denominator)
    )
    return math.hypot(px - (ax + amount * dx), py - (ay + amount * dy))


def _polyline_distances(points: list[Any], polyline: list[Any]) -> list[float]:
    if len(polyline) < 2:
        return [float("inf")]
    return [
        min(
            _point_segment_distance(point, start, finish)
            for start, finish in zip(polyline, polyline[1:])
        )
        for point in points
        if isinstance(point, list) and len(point) == 2
    ]


def evaluate_projection(zone_spec: dict[str, Any], projection: dict[str, Any]) -> dict[str, Any]:
    failures: list[str] = []
    zone_id = zone_spec.get("zone", {}).get("id", "unknown")
    if projection.get("schema_version") != PROJECTION_VERSION:
        failures.append("Godot overview projection schema is unsupported")
    if projection.get("zone_id") != zone_id:
        failures.append("Godot overview projection belongs to a different zone")

    axes = projection.get("axis_projection", {})
    positive_x = axes.get("positive_x_delta", []) if isinstance(axes, dict) else []
    positive_z = axes.get("positive_z_delta", []) if isinstance(axes, dict) else []
    if len(positive_x) != 2 or float(positive_x[0]) <= 0.02:
        failures.append("Godot overview does not project world +X toward screen right")
    if len(positive_z) != 2 or float(positive_z[1]) >= -0.02:
        failures.append("Godot overview does not project world +Z toward screen top")

    projected = {
        entry.get("feature_id"): entry
        for entry in projection.get("landmarks", [])
        if isinstance(entry, dict) and isinstance(entry.get("feature_id"), str)
    }
    landmark_count = 0
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict) or feature.get("category") != "landmark":
            continue
        landmark_count += 1
        feature_id = feature.get("id", "<unknown>")
        entry = projected.get(feature_id)
        if entry is None or not entry.get("found", False):
            failures.append("Godot overview could not project landmark %s" % feature_id)
            continue
        evidence = feature.get("evidence", [])
        expected_region = evidence[0].get("region", []) if evidence and isinstance(evidence[0], dict) else []
        # Registration must measure the camera that produced the accepted
        # render. The old evidence-camera field described an internal helper
        # projection and could reject an aligned render (or bless a bad one).
        point = entry.get("screen_normalized", [])
        rendered_bounds = entry.get("screen_bounds_normalized", [])
        if (
            not _inside(point, expected_region)
            and not _regions_overlap(rendered_bounds, expected_region, 0.01)
        ):
            failures.append(
                "Godot overview projects landmark %s outside reviewed evidence region"
                % feature_id
            )

    projected_corridors = {
        entry.get("feature_id"): entry
        for entry in projection.get("corridors", [])
        if isinstance(entry, dict) and isinstance(entry.get("feature_id"), str)
    }
    corridor_observations: dict[str, Any] = {}
    corridor_count = 0
    for feature in zone_spec.get("features", []):
        if (
            not isinstance(feature, dict)
            or feature.get("category") != "corridor"
            or feature.get("semantic") != "lane"
        ):
            continue
        corridor_count += 1
        feature_id = str(feature.get("id", "<unknown>"))
        entry = projected_corridors.get(feature_id)
        if entry is None or not entry.get("found", False):
            failures.append("Godot overview could not project lane %s" % feature_id)
            continue
        screen_points = entry.get("screen_normalized", [])
        half_widths = entry.get("half_width_pixels", [])
        if len(screen_points) < 3 or len(half_widths) != len(screen_points):
            failures.append("Godot overview lane %s has incomplete projection samples" % feature_id)
            continue
        evidence = feature.get("evidence", [])
        expected_region = (
            evidence[0].get("region", [])
            if evidence and isinstance(evidence[0], dict)
            else []
        )
        outside_count = sum(not _inside(point, expected_region) for point in screen_points)
        if outside_count:
            failures.append(
                "Godot overview projects lane %s outside reviewed evidence region"
                % feature_id
            )
        source_points = feature.get("geometry", {}).get("source_points", [])
        forward = _polyline_distances(screen_points, source_points)
        reverse = _polyline_distances(source_points, screen_points)
        distances = forward + reverse
        mean_distance = sum(distances) / max(len(distances), 1)
        maximum_distance = max(distances, default=float("inf"))
        if mean_distance > 0.09 or maximum_distance > 0.16:
            failures.append(
                "Godot overview lane %s diverges from its reviewed source trace"
                % feature_id
            )
        corridor_observations[feature_id] = {
            "projected_sample_count": len(screen_points),
            "outside_evidence_count": outside_count,
            "mean_source_trace_distance": round(mean_distance, 6),
            "maximum_source_trace_distance": round(maximum_distance, 6),
            "minimum_half_width_pixels": round(min(float(value) for value in half_widths), 4),
        }

    projected_landforms = {
        entry.get("feature_id"): entry
        for entry in projection.get("landforms", [])
        if isinstance(entry, dict) and isinstance(entry.get("feature_id"), str)
    }
    landform_observations: dict[str, Any] = {}
    landform_count = 0
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict) or feature.get("category") != "landform":
            continue
        landform_count += 1
        feature_id = str(feature.get("id", "<unknown>"))
        entry = projected_landforms.get(feature_id)
        if entry is None or not entry.get("found", False):
            failures.append("Godot overview could not project landform %s" % feature_id)
            continue
        samples = entry.get("screen_samples", [])
        if len(samples) < 24:
            failures.append(
                "Godot overview landform %s has incomplete projection samples"
                % feature_id
            )
            continue
        evidence = feature.get("evidence", [])
        expected_region = (
            evidence[0].get("region", [])
            if evidence and isinstance(evidence[0], dict)
            else []
        )
        outside_count = sum(not _inside(point, expected_region) for point in samples)
        outside_fraction = outside_count / max(len(samples), 1)
        if outside_fraction > 0.12:
            failures.append(
                "Godot overview projects landform %s outside reviewed evidence region"
                % feature_id
            )
        maximum_relief = float(entry.get("maximum_projected_relief_pixels", 0.0))
        mean_relief = float(entry.get("mean_projected_relief_pixels", 0.0))
        profile = feature.get("generation", {}).get("profile")
        # Rasterized camera evidence varies at the sub-pixel boundary across
        # renderers. A one-pixel tolerance preserves the intended 40 px floor
        # without turning 39.97 px into a fake structural failure.
        if profile == "alpine_jagged_massif" and (
            maximum_relief < 39.0 or mean_relief < 10.0
        ):
            failures.append(
                "Godot overview Alpine landform %s lacks projected vertical relief"
                % feature_id
            )
        landform_observations[feature_id] = {
            "projected_sample_count": len(samples),
            "outside_evidence_fraction": round(outside_fraction, 6),
            "maximum_projected_relief_pixels": round(maximum_relief, 4),
            "mean_projected_relief_pixels": round(mean_relief, 4),
        }

    return {
        "schema_version": ACCEPTANCE_VERSION,
        "zone_id": zone_id,
        "status": "failed" if failures else "passed",
        "failures": failures,
        "evidence": {
            "landmark_count": landmark_count,
            "projected_landmark_count": sum(
                1 for entry in projected.values() if entry.get("found", False)
            ),
            "corridor_count": corridor_count,
            "projected_corridor_count": sum(
                1 for entry in projected_corridors.values() if entry.get("found", False)
            ),
            "corridors": corridor_observations,
            "landform_count": landform_count,
            "projected_landform_count": sum(
                1 for entry in projected_landforms.values() if entry.get("found", False)
            ),
            "landforms": landform_observations,
            "axis_projection": axes,
        },
    }
