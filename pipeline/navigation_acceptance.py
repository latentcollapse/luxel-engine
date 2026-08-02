#!/usr/bin/env python3
"""Validate native Godot NavigationMesh connectivity for a compiled zone."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable

from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError


GODOT_NAVIGATION_PROBE_VERSION = "codeweald.godot-navigation-probe/v1"
NAVIGATION_ACCEPTANCE_VERSION = "codeweald.navigation-acceptance/v1"


def evaluate_navigation(
    zone_spec: dict[str, Any], probe: dict[str, Any]
) -> dict[str, Any]:
    failures: list[str] = []
    zone_id = zone_spec.get("zone", {}).get("id", "unknown")
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        failures.append("ZoneSpec schema is not supported")
    if probe.get("schema_version") != GODOT_NAVIGATION_PROBE_VERSION:
        failures.append("Godot navigation probe schema is unsupported")
    if probe.get("zone_id") != zone_id:
        failures.append("Godot navigation probe belongs to a different zone")
    if not probe.get("navigation_found", False):
        failures.append("Godot scene has no native NavigationRegion3D")
    vertex_count = int(probe.get("navigation_vertex_count", 0))
    polygon_count = int(probe.get("navigation_polygon_count", 0))
    if vertex_count < 64 or polygon_count < 32:
        failures.append("Godot NavigationMesh has insufficient walkable geometry")

    keep_count = sum(
        isinstance(feature, dict)
        and feature.get("category") == "landmark"
        and feature.get("semantic") == "faction_keep"
        for feature in zone_spec.get("features", [])
    )
    objective_count = sum(
        isinstance(feature, dict)
        and feature.get("category") == "landmark"
        and feature.get("semantic") == "arcane_ruin"
        for feature in zone_spec.get("features", [])
    )
    bridge_count = sum(
        isinstance(feature, dict)
        and feature.get("category") == "structure"
        and feature.get("semantic") == "bridge"
        for feature in zone_spec.get("features", [])
    )
    off_mesh_link_count = int(probe.get("off_mesh_link_count", 0))
    enabled_off_mesh_link_count = int(
        probe.get("enabled_off_mesh_link_count", 0)
    )
    if off_mesh_link_count < bridge_count:
        failures.append(
            "Godot navigation serialized %d of %d required bridge links"
            % (off_mesh_link_count, bridge_count)
        )
    if enabled_off_mesh_link_count < bridge_count:
        failures.append(
            "Godot navigation has disabled bridge traversal links"
        )
    expected_roles = {"keep_to_keep": 1 if keep_count >= 2 else 0}
    expected_roles["keep_to_objective"] = keep_count * objective_count
    observed_roles = {"keep_to_keep": 0, "keep_to_objective": 0}
    route_evidence: list[dict[str, Any]] = []
    maximum_snap_distance = max(
        20.0,
        float(
            zone_spec.get("traversal_policy", {}).get(
                "maximum_keep_lane_distance_m", 225.0
            )
        )
        * 0.45,
    )
    for route in probe.get("routes", []):
        if not isinstance(route, dict):
            continue
        role = str(route.get("role", ""))
        if role in observed_roles:
            observed_roles[role] += 1
        found = bool(route.get("found", False))
        path_length = float(route.get("path_length_m", 0.0))
        straight = float(route.get("straight_line_distance_m", 0.0))
        start_snap = float(route.get("start_snap_distance_m", float("inf")))
        finish_snap = float(route.get("finish_snap_distance_m", float("inf")))
        if not found:
            failures.append(
                "Godot navigation cannot connect %s to %s"
                % (
                    route.get("from_feature_id", "<unknown>"),
                    route.get("to_feature_id", "<unknown>"),
                )
            )
        if found and (path_length < straight * 0.80 or path_length > straight * 4.0):
            failures.append(
                "Godot navigation route %s to %s has implausible path length"
                % (
                    route.get("from_feature_id", "<unknown>"),
                    route.get("to_feature_id", "<unknown>"),
                )
            )
        if start_snap > maximum_snap_distance or finish_snap > maximum_snap_distance:
            failures.append(
                "Godot navigation route %s to %s snaps too far from its semantic anchor"
                % (
                    route.get("from_feature_id", "<unknown>"),
                    route.get("to_feature_id", "<unknown>"),
                )
            )
        route_evidence.append(
            {
                "from_feature_id": route.get("from_feature_id"),
                "to_feature_id": route.get("to_feature_id"),
                "role": role,
                "found": found,
                "path_length_m": round(path_length, 3),
                "straight_line_distance_m": round(straight, 3),
                "start_snap_distance_m": round(start_snap, 3),
                "finish_snap_distance_m": round(finish_snap, 3),
            }
        )
    for role, expected_count in expected_roles.items():
        if observed_roles.get(role, 0) < expected_count:
            failures.append(
                "Godot navigation probe omitted required %s routes" % role
            )
    return {
        "schema_version": NAVIGATION_ACCEPTANCE_VERSION,
        "zone_id": zone_id,
        "status": "failed" if failures else "passed",
        "failures": failures,
        "evidence": {
            "navigation_vertex_count": vertex_count,
            "navigation_polygon_count": polygon_count,
            "expected_off_mesh_link_count": bridge_count,
            "off_mesh_link_count": off_mesh_link_count,
            "enabled_off_mesh_link_count": enabled_off_mesh_link_count,
            "expected_route_roles": expected_roles,
            "observed_route_roles": observed_roles,
            "routes": route_evidence,
        },
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate native Godot navigation connectivity"
    )
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("probe", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        zone_spec = json.loads(args.zone_spec.read_text(encoding="utf-8"))
        probe = json.loads(args.probe.read_text(encoding="utf-8"))
        report = evaluate_navigation(zone_spec, probe)
    except (OSError, ValueError, json.JSONDecodeError, ZoneCompileError) as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print("Godot navigation acceptance: %s" % report["status"])
    return 0 if report["status"] == "passed" else 2


if __name__ == "__main__":
    raise SystemExit(main())
