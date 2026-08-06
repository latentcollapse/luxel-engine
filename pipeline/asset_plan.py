#!/usr/bin/env python3
"""Resolve ZoneSpec asset intent against an observed Codeweald asset catalog."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable

from asset_catalog import CATALOG_VERSION
from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError


ASSET_PLAN_VERSION = "codeweald.asset-plan/v1"

# Which S6 suitability field decides where each foliage role may stand.
#
# `canopy_suitability_u8.bin` describes **canopy**, so it governs canopy roles
# and nothing else. Applying it to groundcover, understory or forest-floor rock
# would be a category error dressed as ecology: a boulder has no treeline, and
# gating one on canopy suitability would strip scree out of exactly the open
# ground it belongs on.
#
# A role absent from this table scatters as it always did -- inside its authored
# polygon, on slope and spacing alone. That is a statement that no ecological
# field describes it yet, not an oversight, and it is why the value is allowed
# to be `None` rather than defaulted to something.
ECOLOGY_FIELDS: dict[str, str] = {
    "conifer_canopy": "canopy_suitability",
}


def _read(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ZoneCompileError("Cannot read %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise ZoneCompileError("%s must contain an object" % path)
    return value


def _profile_reference(feature: dict[str, Any]) -> str | None:
    generation, properties = feature.get("generation", {}), feature.get("properties", {})
    if isinstance(generation, dict) and isinstance(generation.get("asset_profile"), str):
        return generation["asset_profile"]
    if isinstance(properties, dict) and isinstance(properties.get("asset_profile"), str):
        return properties["asset_profile"]
    return None


def _lod_family_key(asset: dict[str, Any]) -> str:
    source_path = str(asset.get("source_path", ""))
    for suffix in ("_lod0.", "_lod1.", "_lod2."):
        if suffix in source_path:
            return source_path.replace(suffix, "_lod*.", 1)
    return source_path


def _attach_lod_siblings(
    selected: list[dict[str, Any]], assets: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    """Attach only catalog-observed sibling LODs to each selected asset.

    Filename inference is used solely as a family join key. Every returned
    sibling remains a complete catalog record with its observed digest,
    readiness, and tags; the planner never invents a path.
    """
    by_family: dict[str, dict[str, dict[str, Any]]] = {}
    for asset in assets:
        if not isinstance(asset, dict):
            continue
        tags = set(asset.get("tags", []))
        tier = next((lod for lod in ("lod0", "lod1", "lod2") if lod in tags), None)
        if tier is not None:
            by_family.setdefault(_lod_family_key(asset), {})[tier] = asset
    resolved: list[dict[str, Any]] = []
    for asset in selected:
        record = dict(asset)
        family = by_family.get(_lod_family_key(asset), {})
        if "lod0" in set(asset.get("tags", [])) and len(family) > 1:
            missing = [tier for tier in ("lod0", "lod1", "lod2") if tier not in family]
            if missing:
                raise ZoneCompileError(
                    "LOD asset %s is missing catalog siblings %s"
                    % (asset.get("source_path", "<unknown>"), ", ".join(missing))
                )
            record["lod_assets"] = {tier: family[tier] for tier in ("lod0", "lod1", "lod2")}
        resolved.append(record)
    return resolved


def _select_layer(profile_id: str, layer: dict[str, Any], assets: list[dict[str, Any]]) -> dict[str, Any]:
    """Resolve one authored ecological layer against observed catalog facts."""
    required_tags = set(layer.get("required_tags", []))
    prefixes = tuple(layer.get("source_prefixes", []))
    formats = set(layer.get("formats", []))
    candidates = [
        asset
        for asset in assets
        if isinstance(asset, dict)
        and required_tags.issubset(set(asset.get("tags", [])))
        and (not prefixes or str(asset.get("source_path", "")).startswith(prefixes))
        and (not formats or str(asset.get("format", "")).lower() in formats)
    ]
    candidates.sort(key=lambda asset: (str(asset.get("source_path", "")), str(asset.get("id", ""))))
    variant_count = int(layer.get("variant_count", 1))
    selected = candidates[:variant_count]
    if len(selected) < variant_count:
        raise ZoneCompileError(
            "Asset profile %s layer %s needs %d variants tagged %s; catalog has %d"
            % (profile_id, layer.get("id", "unnamed"), variant_count, sorted(required_tags), len(selected))
        )
    role = layer.get("role", profile_id)
    return {
        "id": layer.get("id", "primary"),
        "role": role,
        # Which S6 field governs where this layer may stand, declared rather
        # than inferred. The scatter is compiled in Rust, and a Rust that
        # sniffed `role` for the substring "canopy" would be one rename away
        # from silently scattering trees by no ecology at all.
        "ecology_field": ECOLOGY_FIELDS.get(role),
        "instance_count": int(layer.get("instances_per_feature", 1)),
        "scale_m": layer.get("scale_m", [1.0, 1.0]),
        "minimum_spacing_m": float(layer.get("minimum_spacing_m", 0.0)),
        "maximum_slope_degrees": float(
            layer.get("maximum_slope_degrees", 35.0)
        ),
        "runtime_enabled": bool(layer.get("runtime_enabled", True)),
        "quality_requirements": dict(layer.get("quality_requirements", {})),
        "appearance_requirements": dict(layer.get("appearance_requirements", {})),
        "assets": _attach_lod_siblings(selected, assets),
    }


def resolve_asset_plan(zone_spec: dict[str, Any], catalog: dict[str, Any]) -> dict[str, Any]:
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        raise ZoneCompileError("Expected %s" % ZONE_SPEC_VERSION)
    if catalog.get("schema_version") != CATALOG_VERSION:
        raise ZoneCompileError("Expected %s" % CATALOG_VERSION)
    profiles = zone_spec.get("asset_profiles", {})
    assets = catalog.get("assets", [])
    if not isinstance(profiles, dict) or not isinstance(assets, list):
        raise ZoneCompileError("ZoneSpec profiles and catalog assets must be collections")
    assignments: list[dict[str, Any]] = []
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict):
            continue
        profile_id = _profile_reference(feature)
        if profile_id is None:
            continue
        profile = profiles.get(profile_id)
        if not isinstance(profile, dict):
            raise ZoneCompileError("Feature %s references an unavailable asset profile" % feature.get("id", "<unknown>"))
        authored_layers = profile.get("layers")
        if authored_layers is not None and (not isinstance(authored_layers, list) or not authored_layers):
            raise ZoneCompileError("Asset profile %s has invalid ecological layers" % profile_id)
        layers = [_select_layer(profile_id, layer, assets) for layer in authored_layers] if authored_layers else [_select_layer(profile_id, profile, assets)]
        primary = layers[0]
        assignments.append(
            {
                "feature_id": feature.get("id"),
                "profile_id": profile_id,
                "role": profile.get("role", "environment"),
                # These retained top-level fields preserve v1 consumer
                # compatibility. New consumers use every semantic layer.
                "instance_count": primary["instance_count"],
                "scale_m": primary["scale_m"],
                "minimum_spacing_m": primary["minimum_spacing_m"],
                "runtime_enabled": primary["runtime_enabled"],
                "assets": primary["assets"],
                "layers": layers,
            }
        )
    return {
        "schema_version": ASSET_PLAN_VERSION,
        "zone_id": zone_spec.get("zone", {}).get("id", "unknown"),
        "catalog_schema": CATALOG_VERSION,
        "assignments": assignments,
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Resolve ZoneSpec asset profiles against a Codeweald catalog")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("catalog", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        plan = resolve_asset_plan(_read(args.zone_spec), _read(args.catalog))
    except ZoneCompileError as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Resolved %d asset assignments at %s" % (len(plan["assignments"]), args.output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
