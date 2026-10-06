#!/usr/bin/env python3
"""Emit a Unity Terrain/Spline import manifest from portable Codeweald artifacts.

This does not claim that a GLB has already become a Unity prefab.  It gives a
Unity editor importer the exact verified source assets, normalized height range,
terrain dimensions, splat channels, and world-space feature geometry it needs.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any, Iterable

from asset_plan import ASSET_PLAN_VERSION
from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError
from zone_runtime_effects import RUNTIME_EFFECTS_VERSION, build_runtime_effects


UNITY_VERSION = "codeweald.unity-zone-import/v1"
# Where the *art* lives, which is not where this file lives. Deriving it from
# __file__ silently assumed the engine sat inside the game whose assets it was
# resolving; once Luxel moved out (D8) it pointed at the engine root, no FBX
# sidecar was ever found, and every Unity placement quietly fell back to the
# portable GLB -- a downgrade with no error. Callers pass the real root.
_FALLBACK_PROJECT_ROOT = Path(__file__).resolve().parents[1]


def _read(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ZoneCompileError("Cannot read %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise ZoneCompileError("%s must contain an object" % path)
    return value


def _feature_geometry(feature: dict[str, Any]) -> dict[str, Any]:
    geometry = feature.get("geometry", {})
    points = geometry.get("points", [])
    return {"type": geometry.get("type"), "points_m_xz": points}


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _unity_source_asset(asset: dict[str, Any], project_root: Path) -> dict[str, Any]:
    """Choose an actual Unity-importable representation without altering Godot's choice.

    FBX is natively imported by the supported Unity editor.  Generated GLBs
    therefore publish a sibling FBX under ``unity/``; the bundled Quaternius
    pack already provides its dedicated ``FBX (Unity)`` copies.  If no verified
    equivalent exists, retain the portable source so the editor importer can
    report it explicitly instead of substituting a made-up prefab.
    """
    source = str(asset.get("source_path", ""))
    source_path = Path(source)
    candidates: list[Path] = []
    if source_path.suffix.lower() in {".glb", ".gltf"}:
        candidates.append(source_path.parent / "unity" / (source_path.stem + ".fbx"))
        candidates.append(source_path.with_suffix(".fbx"))
    if "/glTF/" in source:
        candidates.insert(0, Path(source.replace("/glTF/", "/FBX (Unity)/").rsplit(".", 1)[0] + ".fbx"))
    for candidate in candidates:
        absolute = project_root / candidate
        if absolute.is_file():
            return {"path": candidate.as_posix(), "format": "fbx", "sha256": _sha256(absolute)}
    return {"path": source, "format": asset.get("format"), "sha256": asset.get("sha256")}


def adapt_unity(zone_spec: dict[str, Any], terrain: dict[str, Any], asset_plan: dict[str, Any], runtime_effects: dict[str, Any] | None = None, project_root: Path | None = None) -> dict[str, Any]:
    root = (project_root or _FALLBACK_PROJECT_ROOT).resolve()
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        raise ZoneCompileError("Expected %s" % ZONE_SPEC_VERSION)
    if asset_plan.get("schema_version") != ASSET_PLAN_VERSION:
        raise ZoneCompileError("Expected %s" % ASSET_PLAN_VERSION)
    if asset_plan.get("zone_id") != zone_spec.get("zone", {}).get("id"):
        raise ZoneCompileError("Asset plan belongs to a different zone")
    runtime_effects = runtime_effects or build_runtime_effects(zone_spec)
    if runtime_effects.get("schema_version") != RUNTIME_EFFECTS_VERSION or runtime_effects.get("zone_id") != zone_spec.get("zone", {}).get("id"):
        raise ZoneCompileError("Runtime effects belong to a different zone or use an unsupported schema")
    bounds = terrain.get("world_bounds_m", {})
    heights = terrain.get("height_range_m", {})
    minimum, maximum = float(heights.get("min", 0.0)), float(heights.get("max", 0.0))
    if maximum <= minimum:
        raise ZoneCompileError("Terrain manifest has no usable height range")
    placements = {
        assignment.get("feature_id"): {
            "profile_id": assignment.get("profile_id"),
            "role": assignment.get("role"),
            "instance_count": assignment.get("instance_count"),
            "scale_m": assignment.get("scale_m"),
            "source_assets": [_unity_source_asset(asset, root) for asset in assignment.get("assets", []) if isinstance(asset, dict)],
            "layers": [
                {
                    "id": layer.get("id"), "role": layer.get("role"), "instance_count": layer.get("instance_count"),
                    "scale_m": layer.get("scale_m"), "minimum_spacing_m": layer.get("minimum_spacing_m", 0.0),
                    "source_assets": [_unity_source_asset(asset, root) for asset in layer.get("assets", []) if isinstance(asset, dict)],
                }
                for layer in assignment.get("layers", []) if isinstance(layer, dict)
            ],
            "requires_prefab_import": True,
        }
        for assignment in asset_plan.get("assignments", [])
        if isinstance(assignment, dict)
    }
    features = []
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict):
            continue
        entry = {
            "id": feature.get("id"),
            "category": feature.get("category"),
            "semantic": feature.get("semantic"),
            "geometry": _feature_geometry(feature),
        }
        if feature.get("id") in placements:
            entry["placement"] = placements[feature["id"]]
        features.append(entry)
    return {
        "schema_version": UNITY_VERSION,
        "zone_id": zone_spec.get("zone", {}).get("id"),
        "coordinate_contract": {"source": "right-handed-xz-up-y", "unity_terrain": "x-z-horizontal-y-up", "flip_z": False},
        "terrain": {
            "heightmap_16": terrain.get("artifacts", {}).get("heightmap_16"),
            "height_normalization_m": {"min": minimum, "max": maximum, "range": maximum - minimum},
            "size_m": {"x": bounds.get("width"), "y": maximum - minimum, "z": bounds.get("length")},
            "resolution": terrain.get("resolution"),
            "splatmap": terrain.get("artifacts", {}).get("splatmap"),
            # The fifth weight rides in its own single-channel image; Unity
            # terrain already composites more layers than one control texture
            # holds, so this is an ordinary second control map rather than an
            # exception.
            "wetland_mask": terrain.get("artifacts", {}).get("wetland_mask"),
            "layers": terrain.get("channel_convention", {}).get("splatmap", {}),
            "layer_order": terrain.get("channel_convention", {}).get("layers", []),
            "materials": terrain.get("terrain_materials", {}),
        },
        "features": features,
        "runtime_effects": runtime_effects,
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Create a Unity import manifest from Codeweald ZoneSpec artifacts")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("terrain_manifest", type=Path)
    parser.add_argument("asset_plan", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--project-root", type=Path, default=None, help="Path to the project root directory")
    args = parser.parse_args(argv)
    try:
        output = adapt_unity(_read(args.zone_spec), _read(args.terrain_manifest), _read(args.asset_plan), project_root=args.project_root)
    except ZoneCompileError as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Wrote Unity zone manifest: %s" % args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
