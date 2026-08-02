#!/usr/bin/env python3
"""Deterministic gameplay traversal probes for a compiled Codeweald terrain.

The report is engine-neutral evidence that reviewed lanes are more than drawn
splines: their full authored width has a bounded longitudinal and cross grade,
both faction keeps connect to each lane, and the central objective remains
reachable from the lane network. Godot separately compiles and probes its
native NavigationMesh from the same terrain.
"""

from __future__ import annotations

import argparse
import copy
import json
import math
from pathlib import Path
from typing import Any, Iterable

import numpy as np
from PIL import Image

from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError
from zone_rasterizer import _catmull_rom_route


TRAVERSAL_PROBE_VERSION = "codeweald.traversal-probe/v1"


def _actor_profiles(policy: dict[str, Any]) -> list[dict[str, float | str]]:
    """Derive a deterministic clearance matrix around the authored base actor."""
    radius = float(policy.get("agent_radius_m", 2.5))
    height = float(policy.get("agent_height_m", 8.0))
    return [
        {
            "id": "scout",
            "radius_m": round(radius * 0.5, 4),
            "height_m": round(height * 0.75, 4),
        },
        {"id": "standard", "radius_m": radius, "height_m": height},
        {
            "id": "large",
            "radius_m": round(radius * 1.5, 4),
            "height_m": round(height * 1.25, 4),
        },
    ]


def _sample_height(
    height_m: np.ndarray, width: float, length: float, x: float, z: float
) -> float:
    rows, columns = height_m.shape
    u = np.clip(x / width + 0.5, 0.0, 1.0) * (columns - 1)
    v = np.clip(0.5 - z / length, 0.0, 1.0) * (rows - 1)
    x0, z0 = int(math.floor(u)), int(math.floor(v))
    x1, z1 = min(x0 + 1, columns - 1), min(z0 + 1, rows - 1)
    tx, tz = u - x0, v - z0
    return float(
        height_m[z0, x0] * (1.0 - tx) * (1.0 - tz)
        + height_m[z0, x1] * tx * (1.0 - tz)
        + height_m[z1, x0] * (1.0 - tx) * tz
        + height_m[z1, x1] * tx * tz
    )


def _polyline_samples(points: list[list[float]], spacing: float) -> list[tuple[float, float]]:
    samples: list[tuple[float, float]] = []
    for start, finish in zip(points, points[1:]):
        dx, dz = float(finish[0]) - float(start[0]), float(finish[1]) - float(start[1])
        distance = math.hypot(dx, dz)
        steps = max(1, int(math.ceil(distance / spacing)))
        for index in range(steps):
            amount = index / steps
            samples.append((float(start[0]) + dx * amount, float(start[1]) + dz * amount))
    samples.append((float(points[-1][0]), float(points[-1][1])))
    return samples


def _point_segment_distance(
    point: tuple[float, float], start: tuple[float, float], finish: tuple[float, float]
) -> float:
    px, pz = point
    ax, az = start
    bx, bz = finish
    dx, dz = bx - ax, bz - az
    denominator = dx * dx + dz * dz
    amount = (
        0.0
        if denominator <= 1e-12
        else np.clip(((px - ax) * dx + (pz - az) * dz) / denominator, 0.0, 1.0)
    )
    return math.hypot(px - (ax + float(amount) * dx), pz - (az + float(amount) * dz))


def _point_polyline_distance(point: tuple[float, float], points: list[list[float]]) -> float:
    return min(
        _point_segment_distance(
            point,
            (float(start[0]), float(start[1])),
            (float(finish[0]), float(finish[1])),
        )
        for start, finish in zip(points, points[1:])
    )


def _local_grade(
    height_m: np.ndarray,
    width: float,
    length: float,
    point: tuple[float, float],
    radius: float,
) -> float:
    center = _sample_height(height_m, width, length, *point)
    grades = []
    for index in range(8):
        angle = math.tau * index / 8.0
        sample = (
            point[0] + math.cos(angle) * radius,
            point[1] + math.sin(angle) * radius,
        )
        grades.append(
            abs(_sample_height(height_m, width, length, *sample) - center) / radius
        )
    return max(grades, default=0.0)


def evaluate_traversal(
    zone_spec: dict[str, Any], height_m: np.ndarray
) -> dict[str, Any]:
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        raise ZoneCompileError("Expected %s" % ZONE_SPEC_VERSION)
    if height_m.ndim != 2 or min(height_m.shape) < 33:
        raise ZoneCompileError("Traversal probe requires a two-dimensional terrain raster")
    bounds = zone_spec.get("zone", {}).get("world_bounds", {})
    width, length = float(bounds.get("width", 0.0)), float(bounds.get("length", 0.0))
    if width <= 0.0 or length <= 0.0:
        raise ZoneCompileError("ZoneSpec has invalid world bounds")
    policy = zone_spec.get("traversal_policy", {})
    spacing = float(policy.get("sample_spacing_m", 8.0))
    maximum_grade = float(policy.get("maximum_lane_grade", 0.72))
    maximum_p95_grade = float(policy.get("maximum_lane_p95_grade", 0.32))
    maximum_cross_grade = float(policy.get("maximum_lane_cross_grade", 0.40))
    maximum_keep_distance = float(policy.get("maximum_keep_lane_distance_m", 225.0))
    maximum_objective_distance = float(
        policy.get("maximum_objective_lane_distance_m", 280.0)
    )
    agent_radius = float(policy.get("agent_radius_m", 2.5))
    actor_profiles = _actor_profiles(policy)

    features = [item for item in zone_spec.get("features", []) if isinstance(item, dict)]
    keeps = [
        feature
        for feature in features
        if feature.get("category") == "landmark"
        and feature.get("semantic") == "faction_keep"
    ]
    lanes = [
        copy.deepcopy(feature)
        for feature in features
        if feature.get("category") == "corridor" and feature.get("semantic") == "lane"
    ]
    for lane in lanes:
        geometry = lane.get("geometry", {})
        geometry["points"] = _catmull_rom_route(geometry.get("points", []))
    objectives = [
        feature
        for feature in features
        if feature.get("category") == "landmark"
        and feature.get("semantic") == "arcane_ruin"
    ]
    streams = [
        feature
        for feature in features
        if feature.get("category") == "hydrology"
        and feature.get("semantic") in {"stream", "river"}
    ]
    bridges = [
        feature
        for feature in features
        if feature.get("category") == "structure"
        and feature.get("semantic") == "bridge"
    ]
    failures: list[str] = []
    lane_reports: dict[str, Any] = {}
    keep_points = {
        str(feature.get("id")): tuple(
            float(value) for value in feature.get("geometry", {}).get("points", [[0, 0]])[0]
        )
        for feature in keeps
    }
    maximum_actor_radius = max(
        float(profile["radius_m"]) for profile in actor_profiles
    )
    minimum_usable_width = maximum_actor_radius * 2.0 + 2.0

    for lane in lanes:
        lane_id = str(lane.get("properties", {}).get("lane_id", lane.get("id", "lane")))
        points = lane.get("geometry", {}).get("points", [])
        samples = _polyline_samples(points, spacing)
        heights = [
            _sample_height(height_m, width, length, x, z) for x, z in samples
        ]
        lane_bridges = [
            bridge
            for bridge in bridges
            if lane.get("id")
            in bridge.get("properties", {}).get("derived_from", [])
        ]
        bridge_spans = [
            (
                tuple(
                    float(value)
                    for value in bridge.get("geometry", {}).get(
                        "points", [[0.0, 0.0]]
                    )[0]
                ),
                max(
                    8.0,
                    float(
                        bridge.get("properties", {}).get("lane_width_m", 0.0)
                    )
                    * 0.75,
                    max(
                        6.0,
                        float(
                            bridge.get("properties", {}).get(
                                "stream_width_m", 0.0
                            )
                        )
                        * 0.5
                        + 5.0,
                    )
                    + float(
                        bridge.get("properties", {}).get("lane_width_m", 0.0)
                    )
                    * 0.42,
                    float(
                        bridge.get("properties", {}).get("stream_width_m", 0.0)
                    )
                    * 1.5
                    + agent_radius,
                ),
            )
            for bridge in lane_bridges
        ]
        bridge_samples = [
            any(math.dist(sample, point) <= radius for point, radius in bridge_spans)
            for sample in samples
        ]
        grades = [
            abs(next_height - height) / max(math.dist(start, finish), 1e-6)
            for index, (start, finish, height, next_height) in enumerate(zip(
                samples, samples[1:], heights, heights[1:]
            ))
            if not bridge_samples[index]
            and not bridge_samples[index + 1]
        ]
        lane_width = float(lane.get("properties", {}).get("minimum_width_m", 0.0))
        edge_offset = max(agent_radius, lane_width * 0.42)
        cross_grades: list[float] = []
        for index, center in enumerate(samples):
            if bridge_samples[index]:
                continue
            before = samples[max(0, index - 1)]
            after = samples[min(len(samples) - 1, index + 1)]
            tangent_x, tangent_z = after[0] - before[0], after[1] - before[1]
            tangent_length = max(math.hypot(tangent_x, tangent_z), 1e-6)
            side_x, side_z = -tangent_z / tangent_length, tangent_x / tangent_length
            left = (center[0] + side_x * edge_offset, center[1] + side_z * edge_offset)
            right = (center[0] - side_x * edge_offset, center[1] - side_z * edge_offset)
            cross_grades.append(
                abs(
                    _sample_height(height_m, width, length, *left)
                    - _sample_height(height_m, width, length, *right)
                )
                / (edge_offset * 2.0)
            )
        endpoint_assignments = []
        for endpoint in (samples[0], samples[-1]):
            nearest_id, nearest_distance = min(
                (
                    (feature_id, math.dist(endpoint, keep_point))
                    for feature_id, keep_point in keep_points.items()
                ),
                key=lambda item: item[1],
                default=("<missing>", float("inf")),
            )
            endpoint_assignments.append(
                {"keep_id": nearest_id, "distance_m": round(nearest_distance, 3)}
            )
        crossed_streams = [
            str(stream.get("id"))
            for stream in streams
            if any(
                _point_polyline_distance(
                    sample, stream.get("geometry", {}).get("points", [])
                )
                <= float(stream.get("properties", {}).get("width_m", 8.0)) * 0.5
                + agent_radius
                for sample in samples
            )
        ]
        bridge_links = [
            str(bridge.get("id"))
            for bridge in bridges
            if lane.get("id")
            in bridge.get("properties", {}).get("derived_from", [])
            and any(
                stream_id
                in bridge.get("properties", {}).get("derived_from", [])
                for stream_id in crossed_streams
            )
        ]
        maximum_observed_grade = max(grades, default=0.0)
        p95_grade = float(np.percentile(grades, 95)) if grades else 0.0
        maximum_observed_cross_grade = max(cross_grades, default=0.0)
        if lane_width < minimum_usable_width:
            failures.append("Lane %s is narrower than the traversal agent contract" % lane_id)
        if maximum_observed_grade > maximum_grade:
            failures.append("Lane %s exceeds maximum longitudinal grade" % lane_id)
        if p95_grade > maximum_p95_grade:
            failures.append("Lane %s exceeds p95 longitudinal grade" % lane_id)
        if maximum_observed_cross_grade > maximum_cross_grade:
            failures.append("Lane %s exceeds maximum cross grade" % lane_id)
        if any(
            float(assignment["distance_m"]) > maximum_keep_distance
            for assignment in endpoint_assignments
        ):
            failures.append("Lane %s does not connect both faction keep approaches" % lane_id)
        if (
            len(endpoint_assignments) != 2
            or endpoint_assignments[0]["keep_id"] == endpoint_assignments[1]["keep_id"]
        ):
            failures.append("Lane %s endpoints do not connect opposing keeps" % lane_id)
        for stream_id in crossed_streams:
            if not any(
                lane.get("id")
                in bridge.get("properties", {}).get("derived_from", [])
                and stream_id
                in bridge.get("properties", {}).get("derived_from", [])
                for bridge in bridges
            ):
                failures.append(
                    "Lane %s crosses %s without an authored traversal link"
                    % (lane_id, stream_id)
                )
        lane_reports[lane_id] = {
            "feature_id": lane.get("id"),
            "sample_count": len(samples),
            "authored_width_m": lane_width,
            "minimum_height_m": round(min(heights), 3),
            "maximum_height_m": round(max(heights), 3),
            "maximum_longitudinal_grade": round(maximum_observed_grade, 6),
            "p95_longitudinal_grade": round(p95_grade, 6),
            "maximum_cross_grade": round(maximum_observed_cross_grade, 6),
            "endpoint_assignments": endpoint_assignments,
            "crossed_stream_ids": crossed_streams,
            "off_mesh_bridge_link_ids": bridge_links,
            "bridge_excluded_sample_count": sum(bridge_samples),
            "actor_clearance": {
                str(profile["id"]): {
                    "radius_m": profile["radius_m"],
                    "height_m": profile["height_m"],
                    "minimum_required_width_m": round(
                        float(profile["radius_m"]) * 2.0 + 2.0, 4
                    ),
                    "passed": lane_width
                    >= float(profile["radius_m"]) * 2.0 + 2.0,
                }
                for profile in actor_profiles
            },
        }

    keep_reports: dict[str, Any] = {}
    spawn_probe_radius = max(agent_radius * 2.0, 5.0)
    for keep_id, point in keep_points.items():
        grade = _local_grade(
            height_m, width, length, point, spawn_probe_radius
        )
        nearest_lane = min(
            (
                _point_polyline_distance(
                    point, lane.get("geometry", {}).get("points", [])
                )
                for lane in lanes
            ),
            default=float("inf"),
        )
        if nearest_lane > maximum_keep_distance:
            failures.append("Keep %s has no safe lane approach" % keep_id)
        if grade > maximum_cross_grade:
            failures.append("Keep %s spawn anchor exceeds local slope limit" % keep_id)
        keep_reports[keep_id] = {
            "height_m": round(_sample_height(height_m, width, length, *point), 3),
            "maximum_local_grade": round(grade, 6),
            "nearest_lane_distance_m": round(nearest_lane, 3),
        }

    objective_reports: dict[str, Any] = {}
    for objective in objectives:
        objective_id = str(objective.get("id"))
        raw_point = objective.get("geometry", {}).get("points", [[0, 0]])[0]
        point = (float(raw_point[0]), float(raw_point[1]))
        nearest_lane = min(
            (
                _point_polyline_distance(
                    point, lane.get("geometry", {}).get("points", [])
                )
                for lane in lanes
            ),
            default=float("inf"),
        )
        grade = _local_grade(height_m, width, length, point, spawn_probe_radius)
        if nearest_lane > maximum_objective_distance:
            failures.append("Objective %s is disconnected from the lane network" % objective_id)
        if grade > maximum_cross_grade:
            failures.append("Objective %s anchor exceeds local slope limit" % objective_id)
        objective_reports[objective_id] = {
            "height_m": round(_sample_height(height_m, width, length, *point), 3),
            "maximum_local_grade": round(grade, 6),
            "nearest_lane_distance_m": round(nearest_lane, 3),
        }

    viable_lane_ids = [
        lane_id
        for lane_id, report in lane_reports.items()
        if report["maximum_longitudinal_grade"] <= maximum_grade
        and report["p95_longitudinal_grade"] <= maximum_p95_grade
        and report["maximum_cross_grade"] <= maximum_cross_grade
        and all(
            clearance["passed"]
            for clearance in report["actor_clearance"].values()
        )
        and len(
            {
                assignment["keep_id"]
                for assignment in report["endpoint_assignments"]
            }
        )
        >= 2
    ]
    blocker_scenarios = []
    for blocked_lane_id in sorted(lane_reports):
        alternatives = sorted(
            lane_id
            for lane_id in viable_lane_ids
            if lane_id != blocked_lane_id
        )
        passed = bool(alternatives)
        if not passed:
            failures.append(
                "Blocking lane %s leaves no opposing-keep replan route"
                % blocked_lane_id
            )
        blocker_scenarios.append(
            {
                "blocked_lane_id": blocked_lane_id,
                "alternative_lane_ids": alternatives,
                "passed": passed,
            }
        )

    return {
        "schema_version": TRAVERSAL_PROBE_VERSION,
        "zone_id": zone_spec.get("zone", {}).get("id", "unknown"),
        "status": "failed" if failures else "passed",
        "failures": failures,
        "policy": {
            "agent_radius_m": agent_radius,
            "agent_height_m": float(policy.get("agent_height_m", 8.0)),
            "sample_spacing_m": spacing,
            "maximum_lane_grade": maximum_grade,
            "maximum_lane_p95_grade": maximum_p95_grade,
            "maximum_lane_cross_grade": maximum_cross_grade,
            "maximum_keep_lane_distance_m": maximum_keep_distance,
            "maximum_objective_lane_distance_m": maximum_objective_distance,
        },
        "lanes": lane_reports,
        "keeps": keep_reports,
        "objectives": objective_reports,
        "actor_profiles": actor_profiles,
        "off_mesh_link_count": sum(
            len(report["off_mesh_bridge_link_ids"])
            for report in lane_reports.values()
        ),
        "blocker_scenarios": blocker_scenarios,
    }


def _load_height(terrain_manifest: Path) -> np.ndarray:
    manifest = json.loads(terrain_manifest.read_text(encoding="utf-8"))
    artifact = terrain_manifest.parent / manifest.get("artifacts", {}).get(
        "heightmap_16", "heightmap_16.png"
    )
    with Image.open(artifact) as image:
        normalized = np.asarray(image, dtype=np.float32)
    if normalized.ndim == 3:
        normalized = normalized[:, :, 0]
    normalized /= 65535.0
    height_range = manifest.get("height_range_m", {})
    return float(height_range.get("min", 0.0)) + normalized * (
        float(height_range.get("max", 0.0)) - float(height_range.get("min", 0.0))
    )


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Probe lane, spawn, and objective traversal over compiled terrain"
    )
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("terrain_manifest", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        zone_spec = json.loads(args.zone_spec.read_text(encoding="utf-8"))
        report = evaluate_traversal(zone_spec, _load_height(args.terrain_manifest))
    except (OSError, ValueError, json.JSONDecodeError, ZoneCompileError) as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Traversal probe %s: %s" % (report["zone_id"], report["status"]))
    return 0 if report["status"] == "passed" else 2


if __name__ == "__main__":
    raise SystemExit(main())
