#!/usr/bin/env python3
"""Godot adapter for ZoneSpec v1.

The canonical spec stays engine-neutral. This adapter writes only the JSON shape
currently consumed by CaledoniaMapDefinition, so the existing Godot builders can be
incrementally modernised without making Unity or Unreal second-class citizens.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable

from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError


def _read(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as source:
        value = json.load(source)
    if not isinstance(value, dict):
        raise ZoneCompileError("ZoneSpec must be a JSON object")
    return value


def _write(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as destination:
        json.dump(value, destination, indent=2, sort_keys=True)
        destination.write("\n")


def _points(feature: dict[str, Any]) -> list[list[float]]:
    return feature["geometry"]["points"]


def adapt_zone_spec(zone_spec: dict[str, Any]) -> dict[str, Any]:
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        raise ZoneCompileError("Expected %s" % ZONE_SPEC_VERSION)
    zone = zone_spec.get("zone", {})
    bounds = zone.get("world_bounds", {})
    features = zone_spec.get("features", [])
    if not isinstance(features, list):
        raise ZoneCompileError("features must be an array")

    lanes: dict[str, list[list[float]]] = {}
    mountain_polygons: list[list[list[float]]] = []
    forest_polygons: list[list[list[float]]] = []
    marsh_polygons: list[list[list[float]]] = []
    river_splines: list[list[list[float]]] = []
    bridge_sockets: list[dict[str, Any]] = []
    bases: dict[str, dict[str, Any]] = {}

    for feature in features:
        category, semantic = feature.get("category"), feature.get("semantic")
        points = _points(feature)
        props = feature.get("properties", {})
        if category == "corridor" and semantic == "lane":
            lane_id = props.get("lane_id")
            if lane_id in {"top", "mid", "bottom"}:
                lanes[lane_id] = points
        elif category == "landform":
            mountain_polygons.append(points)
        elif category == "biome" and semantic == "forest":
            forest_polygons.append(points)
        elif category == "biome" and semantic == "marsh":
            marsh_polygons.append(points)
        elif category == "hydrology" and semantic in {"river", "stream"}:
            river_splines.append(points)
        elif category == "structure" and semantic == "bridge":
            bridge_sockets.append({
                "id": feature["id"],
                "pos": points[0],
                "rotation": props.get("rotation_degrees", 0.0),
                "type": props.get("bridge_type", "stone_arch"),
            })
        elif category == "landmark" and semantic == "faction_keep":
            team = props.get("team")
            realm = props.get("realm", team)
            if isinstance(team, str) and team and isinstance(realm, str) and realm in {"albion", "midgard", "hibernia"}:
                bases[team] = {"center": points[0], "realm": props.get("realm", team), "name": props.get("name", team)}

    missing_lanes = {"top", "mid", "bottom"} - set(lanes)
    if missing_lanes:
        raise ZoneCompileError("Godot MOBA adapter requires top/mid/bottom lanes; missing %s" % sorted(missing_lanes))
    if {"team_a", "team_b"} - set(bases):
        raise ZoneCompileError("Godot MOBA adapter requires team_a and team_b faction keeps")

    return {
        "map_id": zone["id"],
        "map_name": zone.get("name", zone["id"]),
        "map_bounds": {"width": bounds["width"], "length": bounds["length"]},
        "team_a_base": bases["team_a"],
        "team_b_base": bases["team_b"],
        "faction_bases": bases,
        "lanes": {
            "top_lane_spline": lanes["top"],
            "mid_lane_spline": lanes["mid"],
            "bottom_lane_spline": lanes["bottom"],
        },
        "jungle_path_splines": [],
        "mountain_polygons": mountain_polygons,
        "landforms": [feature for feature in features if feature.get("category") == "landform"],
        "forest_polygons": forest_polygons,
        "marsh_polygons": marsh_polygons,
        "river_splines": river_splines,
        "bridge_sockets": bridge_sockets,
        "boss_pit": {},
        "neutral_camps": [],
        "tower_sockets": [],
        "codeweald_provenance": {"zone_spec": ZONE_SPEC_VERSION, "generation_seed": zone_spec["generation_seed"]},
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Adapt a portable ZoneSpec to the current Godot map-definition JSON")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        _write(args.output, adapt_zone_spec(_read(args.zone_spec)))
    except (OSError, json.JSONDecodeError, ZoneCompileError) as exc:
        parser.error(str(exc))
    print("Wrote Godot map definition: %s" % args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
