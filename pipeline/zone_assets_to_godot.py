#!/usr/bin/env python3
"""Translate portable asset assignments into Godot import paths."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable

from asset_plan import ASSET_PLAN_VERSION
from zone_compiler import ZoneCompileError


GODOT_ASSET_PLAN_VERSION = "codeweald.godot-asset-plan/v1"


def _read(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ZoneCompileError("Cannot read %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise ZoneCompileError("%s must contain an object" % path)
    return value


def adapt_asset_plan(plan: dict[str, Any]) -> dict[str, Any]:
    if plan.get("schema_version") != ASSET_PLAN_VERSION:
        raise ZoneCompileError("Expected %s" % ASSET_PLAN_VERSION)
    assignments: list[dict[str, Any]] = []
    asset_paths_by_sha256: dict[str, str] = {}
    for assignment in plan.get("assignments", []):
        if not isinstance(assignment, dict):
            continue
        unavailable: list[str] = []
        def adapt_layer(layer: dict[str, Any]) -> dict[str, Any]:
            paths: list[str] = []
            asset_variants: list[dict[str, Any]] = []
            for asset in layer.get("assets", []):
                if not isinstance(asset, dict):
                    continue
                source_path = asset.get("source_path")
                if not isinstance(source_path, str):
                    continue
                if asset.get("engine_readiness", {}).get("godot", False):
                    paths.append("res://" + source_path)
                    asset_sha256 = asset.get("sha256")
                    if isinstance(asset_sha256, str):
                        asset_paths_by_sha256[asset_sha256] = "res://" + source_path
                else:
                    unavailable.append(source_path)
                lod_assets = asset.get("lod_assets", {})
                if isinstance(lod_assets, dict):
                    levels: dict[str, str] = {}
                    for tier in ("lod0", "lod1", "lod2"):
                        sibling = lod_assets.get(tier)
                        if not isinstance(sibling, dict):
                            continue
                        sibling_path = sibling.get("source_path")
                        if not isinstance(sibling_path, str):
                            continue
                        if sibling.get("engine_readiness", {}).get("godot", False):
                            levels[tier] = "res://" + sibling_path
                        else:
                            unavailable.append(sibling_path)
                    if levels:
                        asset_variants.append(
                            {
                                "family": source_path.replace("_lod0.", "_lod*."),
                                "levels": levels,
                            }
                        )
            return {
                "id": layer.get("id", "primary"), "role": layer.get("role", assignment.get("role")),
                "instance_count": layer.get("instance_count", assignment.get("instance_count")),
                "scale_m": layer.get("scale_m", assignment.get("scale_m")),
                "minimum_spacing_m": layer.get("minimum_spacing_m", assignment.get("minimum_spacing_m", 0.0)),
                "runtime_enabled": layer.get("runtime_enabled", assignment.get("runtime_enabled", True)),
                "quality_requirements": layer.get("quality_requirements", {}),
                "assets": paths,
                "asset_variants": asset_variants,
            }
        source_layers = assignment.get("layers", [])
        layers = [adapt_layer(layer) for layer in source_layers if isinstance(layer, dict)] or [adapt_layer(assignment)]
        primary = layers[0]
        assignments.append(
            {
                "feature_id": assignment.get("feature_id"),
                "profile_id": assignment.get("profile_id"),
                "role": assignment.get("role"),
                "instance_count": primary["instance_count"],
                "scale_m": primary["scale_m"],
                "minimum_spacing_m": primary["minimum_spacing_m"],
                "runtime_enabled": primary["runtime_enabled"],
                "assets": primary["assets"],
                "layers": layers,
                "unavailable_assets": unavailable,
            }
        )
    return {
        "schema_version": GODOT_ASSET_PLAN_VERSION,
        "zone_id": plan.get("zone_id"),
        "asset_paths_by_sha256": asset_paths_by_sha256,
        "assignments": assignments,
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Adapt a portable Codeweald asset plan to Godot paths")
    parser.add_argument("asset_plan", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        output = adapt_asset_plan(_read(args.asset_plan))
    except ZoneCompileError as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Wrote Godot asset plan: %s" % args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
