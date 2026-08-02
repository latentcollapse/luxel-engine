#!/usr/bin/env python3
"""Acceptance gates for a compiled Codeweald zone.

This is intentionally a semantic gate, not an aesthetic assertion based on a
single screenshot. A scene cannot pass if it loses reviewed source evidence,
changes a landform profile into flat terrain, drops required corridors/POIs, or
claims assets that the engine did not place.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable

from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError


def _read(path: Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as source:
            value = json.load(source)
    except (OSError, json.JSONDecodeError) as exc:
        raise ZoneCompileError("Cannot read %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise ZoneCompileError("%s must contain an object" % path)
    return value


def _profile_instance_count(profile: dict[str, Any]) -> int:
    if not bool(profile.get("runtime_enabled", True)):
        return 0
    layers = profile.get("layers")
    if isinstance(layers, list):
        return sum(
            max(0, int(layer.get("instances_per_feature", 0)))
            for layer in layers
            if isinstance(layer, dict) and bool(layer.get("runtime_enabled", True))
        )
    return max(0, int(profile.get("instances_per_feature", 0)))


def _profile_lod_instance_count(profile: dict[str, Any]) -> int:
    if not bool(profile.get("runtime_enabled", True)):
        return 0
    layers = profile.get("layers")
    if not isinstance(layers, list):
        layers = [profile]
    return sum(
        max(0, int(layer.get("instances_per_feature", 0)))
        for layer in layers
        if isinstance(layer, dict)
        and bool(layer.get("runtime_enabled", True))
        and "lod0" in layer.get("required_tags", [])
    )


def _relief_floor_scale(zone_spec: dict[str, Any]) -> float:
    """Scale kilometre-world relief floors for a provenance-linked compact map."""
    derivation = zone_spec.get("derivation", {})
    source = (
        derivation.get("source_world_bounds_m", {})
        if isinstance(derivation, dict)
        else {}
    )
    target = zone_spec.get("zone", {}).get("world_bounds", {})
    try:
        source_area = float(source["width"]) * float(source["length"])
        target_area = float(target["width"]) * float(target["length"])
    except (KeyError, TypeError, ValueError):
        return 1.0
    if source_area <= 0.0 or target_area <= 0.0 or target_area > source_area:
        return 1.0
    return (target_area / source_area) ** 0.5


def evaluate_zone(
    zone_spec: dict[str, Any], terrain: dict[str, Any], godot_build: dict[str, Any] | None = None,
    visual_report: dict[str, Any] | None = None, asset_preflight: dict[str, Any] | None = None,
    perspective_report: dict[str, Any] | None = None,
    overview_projection_report: dict[str, Any] | None = None,
    traversal_report: dict[str, Any] | None = None,
    navigation_report: dict[str, Any] | None = None,
    runtime_effects_report: dict[str, Any] | None = None,
) -> dict[str, Any]:
    failures: list[str] = []
    warnings: list[str] = []
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        failures.append("ZoneSpec schema is not supported")
    features = zone_spec.get("features", [])
    if not isinstance(features, list) or not features:
        failures.append("ZoneSpec has no semantic features")
        features = []
    feature_ids = {feature.get("id") for feature in features if isinstance(feature, dict)}
    if len(feature_ids) != len(features):
        failures.append("ZoneSpec feature identifiers are not unique")
    for feature in features:
        if not feature.get("evidence"):
            failures.append("Feature %s has no concept-batch evidence" % feature.get("id", "<unknown>"))
        if feature.get("confidence", 0.0) < 0.75 and feature.get("review_state") != "reviewed":
            failures.append("Low-confidence feature %s is not reviewed" % feature.get("id", "<unknown>"))

    lanes = {feature.get("properties", {}).get("lane_id") for feature in features if feature.get("category") == "corridor" and feature.get("semantic") == "lane"}
    missing_lanes = {"top", "mid", "bottom"} - lanes
    if missing_lanes:
        failures.append("MOBA topology is missing lanes: %s" % ", ".join(sorted(missing_lanes)))
    keeps = [feature for feature in features if feature.get("category") == "landmark" and feature.get("semantic") == "faction_keep"]
    teams = {feature.get("properties", {}).get("team") for feature in keeps}
    if {"team_a", "team_b"} - teams:
        failures.append("MOBA topology is missing one or more faction keeps")
    expected_asset_assignments = sum(
        1
        for feature in features
        if isinstance(feature.get("generation"), dict) and feature["generation"].get("asset_profile")
        or isinstance(feature.get("properties"), dict) and feature["properties"].get("asset_profile")
    )
    asset_profiles = zone_spec.get("asset_profiles", {})
    expected_crag_instances = sum(
        int(asset_profiles.get(feature.get("generation", {}).get("asset_profile"), {}).get("instances_per_feature", 0))
        for feature in features
        if feature.get("category") == "landform"
        and feature.get("generation", {}).get("asset_profile")
    )
    expected_foliage_instances = sum(
        _profile_instance_count(
            asset_profiles.get(feature.get("properties", {}).get("asset_profile"), {})
        )
        for feature in features
        if feature.get("category") == "biome" and feature.get("semantic") == "forest"
    )
    expected_lod_instances = sum(
        _profile_lod_instance_count(
            asset_profiles.get(feature.get("properties", {}).get("asset_profile"), {})
        )
        for feature in features
        if feature.get("category") == "biome" and feature.get("semantic") == "forest"
    )
    expected_settlements = sum(
        1
        for feature in features
        if feature.get("category") == "landmark"
        and feature.get("semantic") == "settlement_cluster"
    )
    expected_waterways = sum(
        1
        for feature in features
        if feature.get("category") == "hydrology"
        and feature.get("semantic") == "stream"
    )
    expected_bridges = sum(
        1
        for feature in features
        if feature.get("category") == "structure"
        and feature.get("semantic") == "bridge"
    )
    expected_ruins = sum(
        1
        for feature in features
        if feature.get("category") == "landmark"
        and feature.get("semantic") == "arcane_ruin"
    )
    profile_feature_counts: dict[str, int] = {}
    for feature in features:
        if not isinstance(feature, dict):
            continue
        profile_id = feature.get("generation", {}).get(
            "asset_profile"
        ) or feature.get("properties", {}).get("asset_profile")
        if isinstance(profile_id, str):
            profile_feature_counts[profile_id] = (
                profile_feature_counts.get(profile_id, 0) + 1
            )

    terrain_landforms = {entry.get("id"): entry for entry in terrain.get("landforms", []) if isinstance(entry, dict)}
    relief_floor_scale = _relief_floor_scale(zone_spec)
    for feature in features:
        if feature.get("category") != "landform":
            continue
        feature_id = feature.get("id")
        result = terrain_landforms.get(feature_id)
        if result is None:
            failures.append("Terrain output omitted landform %s" % feature_id)
            continue
        expected_profile = feature.get("generation", {}).get("profile")
        if result.get("profile") != expected_profile:
            failures.append("Landform %s lost generation profile %s" % (feature_id, expected_profile))
            continue
        elevation = feature.get("generation", {}).get("elevation_m", [0.0, 0.0])
        expected_relief = max(0.0, float(elevation[1]) - float(elevation[0])) if isinstance(elevation, list) and len(elevation) == 2 else 0.0
        if expected_profile == "alpine_jagged_massif":
            if float(result.get("relief_m", 0.0)) < max(70.0 * relief_floor_scale, expected_relief * 0.42):
                failures.append("Alpine massif %s lacks required relief" % feature_id)
            if float(result.get("mean_slope", 0.0)) < 0.45:
                failures.append("Alpine massif %s lacks required steepness" % feature_id)
            expected_art = feature.get("generation", {}).get("composition", {})
            result_art = result.get("art_profile", {})
            for key in ("silhouette", "massing", "surface", "dressing"):
                if result_art.get(key) != expected_art.get(key):
                    failures.append(
                        "Alpine massif %s lost art-profile %s=%r"
                        % (feature_id, key, expected_art.get(key))
                    )
            if expected_art.get("silhouette") == "continuous_boundary_wall":
                profile_count = int(result.get("sidewall_profile_count", 0))
                corrugation = float(
                    result.get("sidewall_corrugation_ratio", 1.0)
                )
                world_width = float(
                    terrain.get("world_bounds_m", {}).get("width", 0.0)
                )
                elevation_bias = float(expected_art.get("elevation_bias", 0.5))
                required_relief = world_width * (
                    0.12 + 0.14 * elevation_bias
                )
                if float(result.get("relief_m", 0.0)) < required_relief:
                    failures.append(
                        "Alpine massif %s lacks boundary-wall silhouette relief"
                        % feature_id
                    )
                if profile_count < 4:
                    failures.append(
                        "Alpine massif %s lacks measurable sidewall profiles"
                        % feature_id
                    )
                elif corrugation > 0.04:
                    failures.append(
                        "Alpine massif %s has repeated sidewall corrugation"
                        % feature_id
                    )
        elif expected_profile == "alpine_sawtooth_ridge":
            if float(result.get("relief_m", 0.0)) < max(45.0 * relief_floor_scale, expected_relief * 0.28):
                failures.append("Alpine ridge %s lacks required relief" % feature_id)
            if float(result.get("p95_slope", 0.0)) < 0.45:
                failures.append("Alpine ridge %s lacks sawtooth steepness" % feature_id)
        elif expected_profile == "scattered_crag_field":
            if float(result.get("rock_coverage", 0.0)) < 0.25:
                failures.append("Crag field %s lacks exposed rock coverage" % feature_id)
            if float(result.get("relief_m", 0.0)) < max(18.0 * relief_floor_scale, expected_relief * 0.18):
                failures.append("Crag field %s lacks localized relief" % feature_id)
        elif expected_profile == "cliff_escarpment":
            if float(result.get("p95_slope", 0.0)) < 0.55:
                failures.append("Cliff escarpment %s lacks a steep face" % feature_id)
            if float(result.get("rock_coverage", 0.0)) < 0.35:
                failures.append("Cliff escarpment %s lacks exposed rock" % feature_id)
        elif expected_profile == "glacial_valley_floor":
            if float(result.get("relief_m", 0.0)) < max(10.0 * relief_floor_scale, expected_relief * 0.10):
                failures.append("Glacial valley %s lacks a readable trough and shoulders" % feature_id)

    palette = terrain.get("style_palette_srgb", {})
    if float(terrain.get("steep_surface_water_fraction", 0.0)) > 0.0001:
        failures.append("Terrain paints standing water onto steep faces")
    if set(palette) != {"grass", "road", "rock", "snow", "water"}:
        failures.append("Terrain output lacks a complete concept-derived style palette")
    requested_materials = zone_spec.get("terrain_materials", {})
    resolved_materials = terrain.get("terrain_materials", {})
    material_scales = zone_spec.get("terrain_material_scale_m", {})
    if not isinstance(requested_materials, dict):
        requested_materials = {}
    if not isinstance(resolved_materials, dict):
        resolved_materials = {}
    if not isinstance(material_scales, dict):
        material_scales = {}
    for layer, material_id in requested_materials.items():
        contract = resolved_materials.get(layer)
        if not isinstance(contract, dict):
            failures.append("Terrain output omitted reviewed %s material" % layer)
            continue
        if contract.get("material_id") != material_id:
            failures.append("Terrain output changed reviewed %s material" % layer)
        maps = contract.get("maps", {})
        if not isinstance(maps, dict) or set(maps) != {
            "albedo", "normal", "roughness"
        }:
            failures.append("Terrain %s material lacks a complete PBR map set" % layer)
        if float(contract.get("minimum_texels_per_meter", 0.0)) < 48.0:
            failures.append(
                "Terrain %s material falls below 48 texels per meter" % layer
            )
        expected_scale = material_scales.get(layer)
        if expected_scale is not None and abs(
            float(contract.get("meters_per_repeat", 0.0))
            - float(expected_scale)
        ) > 1e-4:
            failures.append("Terrain %s material lost its authored world scale" % layer)
    if "wetland" in requested_materials:
        artifacts = terrain.get("artifacts", {})
        if not isinstance(artifacts, dict) or not artifacts.get("wetland_mask"):
            failures.append("Terrain output omitted the wetland material mask")
        if float(terrain.get("wetland_coverage_fraction", 0.0)) <= 0.0:
            failures.append("Terrain wetland material mask has no selected coverage")
    if godot_build is None:
        warnings.append("No engine build report was supplied")
    else:
        if godot_build.get("zone_id") != zone_spec.get("zone", {}).get("id"):
            failures.append("Godot build report belongs to a different zone")
        if not godot_build.get("scene_written", False):
            failures.append("Godot scene was not written")
        if int(godot_build.get("keep_count", 0)) < len(keeps):
            failures.append("Godot scene omitted one or more faction keeps")
        if int(godot_build.get("lane_path_count", 0)) < len(lanes):
            failures.append("Godot scene omitted one or more lane navigation paths")
        if int(godot_build.get("lane_surface_count", 0)) < len(lanes):
            failures.append("Godot scene omitted one or more visible lane surfaces")
        if int(godot_build.get("navigation_vertex_count", 0)) <= 0:
            failures.append("Godot scene omitted native navigation vertices")
        if int(godot_build.get("navigation_polygon_count", 0)) <= 0:
            failures.append("Godot scene omitted native walkable polygons")
        for field in (
            "navigation_off_mesh_link_count",
            "navigation_enabled_off_mesh_link_count",
            "navigation_full_span_off_mesh_link_count",
        ):
            if int(godot_build.get(field, 0)) < expected_bridges:
                failures.append(
                    "Godot scene retained %d of %d required bridge navigation links in %s"
                    % (
                        int(godot_build.get(field, 0)),
                        expected_bridges,
                        field,
                    )
                )
        if int(godot_build.get("waterway_count", 0)) < expected_waterways:
            failures.append(
                "Godot scene placed %d of %d planned waterways"
                % (int(godot_build.get("waterway_count", 0)), expected_waterways)
            )
        if int(godot_build.get("bridge_count", 0)) < expected_bridges:
            failures.append(
                "Godot scene placed %d of %d planned lane-water bridges"
                % (int(godot_build.get("bridge_count", 0)), expected_bridges)
            )
        if int(godot_build.get("ruin_count", 0)) < expected_ruins:
            failures.append(
                "Godot scene placed %d of %d planned objective ruins"
                % (int(godot_build.get("ruin_count", 0)), expected_ruins)
            )
        if int(godot_build.get("settlement_count", 0)) < expected_settlements:
            failures.append(
                "Godot scene placed %d of %d planned settlement landmarks"
                % (
                    int(godot_build.get("settlement_count", 0)),
                    expected_settlements,
                )
            )
        minimum_grounded_components = expected_settlements * 6
        grounded_components = int(
            godot_build.get("settlement_grounded_component_count", 0)
        )
        if grounded_components < minimum_grounded_components:
            failures.append(
                "Godot scene grounded %d of at least %d required settlement components"
                % (grounded_components, minimum_grounded_components)
            )
        grounding_residual_m = float(
            godot_build.get("settlement_grounding_max_residual_m", float("inf"))
        )
        if expected_settlements and grounding_residual_m > 0.08:
            failures.append(
                "Godot settlement grounding residual %.3f m exceeds 0.08 m"
                % grounding_residual_m
            )
        actual_foliage_instances = int(godot_build.get("foliage_instances", 0))
        if actual_foliage_instances < expected_foliage_instances:
            failures.append(
                "Godot scene placed %d of %d planned foliage assets"
                % (actual_foliage_instances, expected_foliage_instances)
            )
        actual_crag_instances = int(godot_build.get("crag_instances", 0))
        if actual_crag_instances < expected_crag_instances:
            failures.append(
                "Godot scene placed %d of %d planned landform dressing assets"
                % (actual_crag_instances, expected_crag_instances)
            )
        expected_multimesh_instances = (
            expected_foliage_instances + expected_crag_instances
        )
        actual_multimesh_instances = int(
            godot_build.get("multimesh_instance_count", 0)
        )
        if actual_multimesh_instances < expected_multimesh_instances:
            failures.append(
                "Godot scene batched %d of %d repeated environment transforms"
                % (actual_multimesh_instances, expected_multimesh_instances)
            )
        if expected_multimesh_instances > 0 and int(
            godot_build.get("multimesh_batch_count", 0)
        ) <= 0:
            failures.append("Godot scene omitted native MultiMesh batches")
        serialized_multimesh_instances = int(
            godot_build.get("multimesh_serialized_instance_count", 0)
        )
        if serialized_multimesh_instances < expected_multimesh_instances:
            failures.append(
                "Godot scene serialized %d of %d repeated environment transforms"
                % (serialized_multimesh_instances, expected_multimesh_instances)
            )
        if expected_multimesh_instances > 0 and not godot_build.get(
            "multimesh_buffers_valid", False
        ):
            failures.append(
                "Godot scene has incomplete or malformed serialized MultiMesh buffers"
            )
        spatial_batches = int(
            godot_build.get("multimesh_spatial_chunk_batch_count", 0)
        )
        total_batches = int(godot_build.get("multimesh_batch_count", 0))
        if total_batches > 0 and spatial_batches != total_batches:
            failures.append(
                "Godot scene spatially chunked %d of %d MultiMesh batches"
                % (spatial_batches, total_batches)
            )
        if int(godot_build.get("multimesh_max_instances_per_chunk", 0)) > 768:
            failures.append("Godot scene has an oversized spatial MultiMesh chunk")
        lod_counts = godot_build.get("multimesh_lod_instance_counts", {})
        if expected_lod_instances > 0:
            for tier in ("lod0", "lod1", "lod2"):
                actual_lod_instances = int(
                    lod_counts.get(tier, 0)
                    if isinstance(lod_counts, dict)
                    else 0
                )
                if actual_lod_instances < expected_lod_instances:
                    failures.append(
                        "Godot scene emitted %d of %d planned %s render instances"
                        % (actual_lod_instances, expected_lod_instances, tier)
                    )
        if not godot_build.get("terrain_mesh_external", False):
            failures.append("Godot scene embedded the generated terrain mesh")
        if int(godot_build.get("terrain_mesh_resource_bytes", 0)) <= 0:
            failures.append("Godot scene has no serialized external terrain resource")
        if not godot_build.get("terrain_collision_external", False):
            failures.append("Godot scene embedded or omitted terrain collision")
        if int(godot_build.get("terrain_collision_sample_count", 0)) <= 0:
            failures.append("Godot scene has no heightmap collision samples")
        if int(godot_build.get("terrain_collision_resource_bytes", 0)) <= 0:
            failures.append("Godot scene has no serialized terrain collision resource")
        requested_layer_count = len(requested_materials)
        if int(godot_build.get(
            "terrain_material_bound_layer_count", 0
        )) != requested_layer_count:
            failures.append(
                "Godot scene bound %d of %d reviewed terrain material layers"
                % (
                    int(godot_build.get(
                        "terrain_material_bound_layer_count", 0
                    )),
                    requested_layer_count,
                )
            )
        if int(godot_build.get(
            "terrain_material_pbr_layer_count", 0
        )) != requested_layer_count:
            failures.append(
                "Godot scene did not retain complete PBR bindings for every terrain layer"
            )
        if requested_layer_count and float(godot_build.get(
            "terrain_material_minimum_texels_per_meter", 0.0
        )) < 48.0:
            failures.append("Godot terrain material density falls below 48 texels per meter")
        if "wetland" in requested_materials and not godot_build.get(
            "terrain_wetland_mask_bound", False
        ):
            failures.append("Godot scene did not bind the wetland material mask")
        if int(godot_build.get("scene_bytes", 0)) > 32 * 1024 * 1024:
            failures.append("Godot scene exceeds the 32 MiB generated-scene budget")
        if int(godot_build.get("asset_assignment_count", 0)) < expected_asset_assignments:
            failures.append("Godot build did not resolve every semantic asset profile")
        missing_assets = godot_build.get("missing_assets", [])
        if missing_assets:
            failures.append("Godot build selected unavailable assets: %s" % ", ".join(missing_assets))
        observed_variants = godot_build.get("landmark_asset_variants", {})
        if not isinstance(observed_variants, dict):
            observed_variants = {}
        variant_roles = {
            "faction_fortification",
            "objective_landmark",
            "lane_crossing_structure",
            "settlement_landmark",
        }
        for profile_id, feature_count in profile_feature_counts.items():
            profile = asset_profiles.get(profile_id, {})
            if not isinstance(profile, dict) or profile.get("role") not in variant_roles:
                continue
            expected_variant_count = min(
                int(profile.get("variant_count", 1)), feature_count
            )
            actual_variant_count = len(
                set(observed_variants.get(profile_id, []))
                if isinstance(observed_variants.get(profile_id, []), list)
                else set()
            )
            if actual_variant_count < expected_variant_count:
                failures.append(
                    "Godot scene used %d of %d required %s asset variants"
                    % (
                        actual_variant_count,
                        expected_variant_count,
                        profile_id,
                    )
                )
    if visual_report is None:
        warnings.append("No rendered visual acceptance report was supplied")
    else:
        if visual_report.get("zone_id") != zone_spec.get("zone", {}).get("id"):
            failures.append("Visual acceptance report belongs to a different zone")
        if visual_report.get("status") == "failed":
            failures.append("Rendered visual acceptance failed")
    if (
        runtime_effects_report is not None
        and runtime_effects_report.get("status") != "passed"
    ):
        failures.append("Serialized Godot runtime effects did not animate")

    if asset_preflight is not None:
        if asset_preflight.get("zone_id") != zone_spec.get("zone", {}).get("id"):
            failures.append("Asset visual preflight report belongs to a different zone")
        if asset_preflight.get("status") != "passed":
            failures.append("Selected assets failed Blender visual preflight")
        if int(asset_preflight.get("asset_count", 0)) <= 0:
            failures.append("Asset visual preflight has no selected renderable assets")
    if perspective_report is not None:
        if perspective_report.get("zone_id") != zone_spec.get("zone", {}).get("id"):
            failures.append("Perspective acceptance report belongs to a different zone")
        if perspective_report.get("status") != "passed":
            failures.append("Player-scale perspective acceptance failed")
    if overview_projection_report is None:
        warnings.append("No runtime overview projection report was supplied")
    else:
        if overview_projection_report.get("zone_id") != zone_spec.get("zone", {}).get("id"):
            failures.append("Overview projection acceptance belongs to a different zone")
        if overview_projection_report.get("status") != "passed":
            failures.append("Runtime overview projection acceptance failed")
    if traversal_report is None:
        warnings.append("No deterministic traversal probe report was supplied")
    else:
        if traversal_report.get("zone_id") != zone_spec.get("zone", {}).get("id"):
            failures.append("Traversal probe report belongs to a different zone")
        if traversal_report.get("status") != "passed":
            failures.append("Deterministic terrain traversal probes failed")
    if navigation_report is None:
        warnings.append("No native Godot navigation acceptance report was supplied")
    else:
        if navigation_report.get("zone_id") != zone_spec.get("zone", {}).get("id"):
            failures.append("Navigation acceptance report belongs to a different zone")
        if navigation_report.get("status") != "passed":
            failures.append("Native Godot navigation acceptance failed")

    return {
        "schema_version": "codeweald.zone-acceptance/v1",
        "zone_id": zone_spec.get("zone", {}).get("id", "unknown"),
        "status": "failed" if failures else ("warnings" if warnings else "passed"),
        "failures": failures,
        "warnings": warnings,
        "evidence": {
            "feature_count": len(features),
            "terrain_landform_count": len(terrain_landforms),
            "has_engine_build_report": godot_build is not None,
            "has_visual_acceptance_report": visual_report is not None,
            "has_asset_visual_preflight": asset_preflight is not None,
            "has_perspective_acceptance_report": perspective_report is not None,
            "has_overview_projection_report": overview_projection_report is not None,
            "has_traversal_probe_report": traversal_report is not None,
            "has_navigation_acceptance_report": navigation_report is not None,
            "has_runtime_effects_acceptance_report": runtime_effects_report is not None,
            "expected_asset_assignments": expected_asset_assignments,
            "expected_foliage_instances": expected_foliage_instances,
            "expected_crag_instances": expected_crag_instances,
            "expected_multimesh_instances": (
                expected_foliage_instances + expected_crag_instances
            ),
            "expected_lod_instances_per_tier": expected_lod_instances,
            "expected_settlement_count": expected_settlements,
            "expected_waterway_count": expected_waterways,
            "expected_bridge_count": expected_bridges,
            "expected_ruin_count": expected_ruins,
            "expected_terrain_material_layers": len(requested_materials),
            "minimum_terrain_texels_per_meter": min(
                (
                    float(contract.get("minimum_texels_per_meter", 0.0))
                    for contract in resolved_materials.values()
                    if isinstance(contract, dict)
                ),
                default=0.0,
            ),
        },
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Validate a ZoneSpec, terrain artifacts, and Godot build report")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("terrain_manifest", type=Path)
    parser.add_argument("--godot-build", type=Path)
    parser.add_argument("--visual-report", type=Path)
    parser.add_argument("--asset-preflight", type=Path)
    parser.add_argument("--perspective-report", type=Path)
    parser.add_argument("--overview-projection-report", type=Path)
    parser.add_argument("--traversal-report", type=Path)
    parser.add_argument("--navigation-report", type=Path)
    parser.add_argument("--runtime-effects-report", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        report = evaluate_zone(
            _read(args.zone_spec), _read(args.terrain_manifest), _read(args.godot_build) if args.godot_build else None,
            _read(args.visual_report) if args.visual_report else None,
            _read(args.asset_preflight) if args.asset_preflight else None,
            _read(args.perspective_report) if args.perspective_report else None,
            overview_projection_report=_read(args.overview_projection_report) if args.overview_projection_report else None,
            traversal_report=_read(args.traversal_report) if args.traversal_report else None,
            navigation_report=_read(args.navigation_report) if args.navigation_report else None,
            runtime_effects_report=_read(args.runtime_effects_report) if args.runtime_effects_report else None,
        )
    except ZoneCompileError as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Zone acceptance %s: %s" % (report["zone_id"], report["status"]))
    return 1 if report["status"] == "failed" else 0


if __name__ == "__main__":
    raise SystemExit(main())
