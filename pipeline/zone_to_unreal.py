#!/usr/bin/env python3
"""Emit an Unreal Landscape/PCG import manifest from Codeweald artifacts."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable

from asset_plan import ASSET_PLAN_VERSION
from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError
from zone_runtime_effects import RUNTIME_EFFECTS_VERSION, build_runtime_effects


UNREAL_VERSION = "codeweald.unreal-zone-import/v1"


def _read(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ZoneCompileError("Cannot read %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise ZoneCompileError("%s must contain an object" % path)
    return value


def _point_cm(point: list[float]) -> list[float]:
    # Codeweald ground is X/Z with Y up. UE ground is X/Y with Z up.
    return [round(float(point[0]) * 100.0, 4), round(float(point[1]) * 100.0, 4), 0.0]


def adapt_unreal(zone_spec: dict[str, Any], terrain: dict[str, Any], asset_plan: dict[str, Any], runtime_effects: dict[str, Any] | None = None) -> dict[str, Any]:
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        raise ZoneCompileError("Expected %s" % ZONE_SPEC_VERSION)
    if asset_plan.get("schema_version") != ASSET_PLAN_VERSION:
        raise ZoneCompileError("Expected %s" % ASSET_PLAN_VERSION)
    if asset_plan.get("zone_id") != zone_spec.get("zone", {}).get("id"):
        raise ZoneCompileError("Asset plan belongs to a different zone")
    runtime_effects = runtime_effects or build_runtime_effects(zone_spec)
    if runtime_effects.get("schema_version") != RUNTIME_EFFECTS_VERSION or runtime_effects.get("zone_id") != zone_spec.get("zone", {}).get("id"):
        raise ZoneCompileError("Runtime effects belong to a different zone or use an unsupported schema")
    bounds, height_range = terrain.get("world_bounds_m", {}), terrain.get("height_range_m", {})
    minimum, maximum = float(height_range.get("min", 0.0)), float(height_range.get("max", 0.0))
    if maximum <= minimum:
        raise ZoneCompileError("Terrain manifest has no usable height range")
    height_span_m = maximum - minimum
    unreal_artifacts = terrain.get("engine_artifacts", {}).get("unreal", {})
    if not isinstance(unreal_artifacts, dict):
        unreal_artifacts = {}
    weightmaps = unreal_artifacts.get("weightmaps", {})
    if not isinstance(weightmaps, dict):
        weightmaps = {}
    profile_by_feature = {assignment.get("feature_id"): assignment for assignment in asset_plan.get("assignments", []) if isinstance(assignment, dict)}
    pcg_features = []
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict):
            continue
        geometry = feature.get("geometry", {})
        entry = {
            "id": feature.get("id"),
            "semantic": feature.get("semantic"),
            "geometry_type": geometry.get("type"),
            "points_cm_xy": [_point_cm(point) for point in geometry.get("points", [])],
        }
        assignment = profile_by_feature.get(feature.get("id"))
        if assignment:
            entry["pcg_asset_sources"] = [
                {"path": asset.get("source_path"), "format": asset.get("format"), "sha256": asset.get("sha256")}
                for asset in assignment.get("assets", [])
                if isinstance(asset, dict)
            ]
            entry["pcg_role"] = assignment.get("role")
            entry["instance_count"] = assignment.get("instance_count")
            entry["scale_m"] = assignment.get("scale_m")
            entry["pcg_layers"] = [
                {
                    "id": layer.get("id"), "role": layer.get("role"), "instance_count": layer.get("instance_count"),
                    "minimum_spacing_m": layer.get("minimum_spacing_m", 0.0), "scale_m": layer.get("scale_m"),
                    "asset_sources": [{"path": asset.get("source_path"), "format": asset.get("format"), "sha256": asset.get("sha256")} for asset in layer.get("assets", []) if isinstance(asset, dict)],
                }
                for layer in assignment.get("layers", []) if isinstance(layer, dict)
            ]
        pcg_features.append(entry)
    return {
        "schema_version": UNREAL_VERSION,
        "zone_id": zone_spec.get("zone", {}).get("id"),
        "coordinate_contract": {"source": "right-handed-xz-up-y", "unreal": "x-y-horizontal-z-up", "meters_to_centimeters": 100},
        "landscape": {
            # Prefer the adapter's valid 1009 Landscape payload.  The
            # fallback keeps this function usable for older manifests.
            "heightmap_16": unreal_artifacts.get("heightmap_16", terrain.get("artifacts", {}).get("heightmap_16")),
            "resolution": unreal_artifacts.get("landscape_resolution", terrain.get("resolution")),
            "extent_cm": {"x": float(bounds.get("width", 0.0)) * 100.0, "y": float(bounds.get("length", 0.0)) * 100.0},
            "height_range_m": {"min": minimum, "max": maximum},
            "z_scale": round(height_span_m * 100.0 / 512.0, 8),
            "layer_sources": {
                **terrain.get("channel_convention", {}).get("splatmap", {}),
                "splatmap": terrain.get("artifacts", {}).get("splatmap"),
                "weightmaps": weightmaps,
            },
            "materials": terrain.get("terrain_materials", {}),
            "component_layout": unreal_artifacts.get("component_layout"),
        },
        "pcg_features": pcg_features,
        "runtime_effects": runtime_effects,
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Create an Unreal Landscape/PCG manifest from Codeweald artifacts")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("terrain_manifest", type=Path)
    parser.add_argument("asset_plan", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        output = adapt_unreal(_read(args.zone_spec), _read(args.terrain_manifest), _read(args.asset_plan))
    except ZoneCompileError as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Wrote Unreal zone manifest: %s" % args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
