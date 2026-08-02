#!/usr/bin/env python3
"""Derive a compact, provenance-safe arena annotation set from a reviewed zone.

This is intentionally a semantic transformation, not image resizing.  It keeps
source geometry and mountain intent, adopts the target batch's intake authority,
and makes physical choices that must change for a smaller playable world: lane
widths, traversal distance budgets, and foliage/rock instance counts.
"""

from __future__ import annotations

import argparse
import copy
import json
import math
from pathlib import Path
from typing import Any


def _read(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("%s must contain a JSON object" % path)
    return value


def _write(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _area(bounds: dict[str, Any]) -> float:
    width = float(bounds["width"])
    length = float(bounds["length"])
    if width <= 0.0 or length <= 0.0:
        raise ValueError("world bounds must be positive")
    return width * length


def _compact_instances(layer: dict[str, Any], ratio: float) -> None:
    count = layer.get("instances_per_feature")
    if not isinstance(count, int) or isinstance(count, bool):
        return
    layer_id = str(layer.get("id", ""))
    # Compacting area must not erase the source's ecological read. Physical
    # scaling already fixed the former 20–39 m "kaiju pine" failure, so retain
    # enough instances for forest masses rather than isolated ornaments.
    # The reviewed concept is a dense Highland battlefield, not sparse parkland.
    # These are screen-contribution floors for the compact overview. They must
    # remain feasible after the physical slope gate: demanding 80 canopy
    # trunks from a narrow polygon forced the scatterer to colonize cliffs.
    floors = {
        # Terrain-native interior crags reduce the actually plantable area.
        # Dense source groves need more than isolated overview ornaments.
        # Compact trees are only 0.47–0.69 source scale, so a tighter center
        # spacing remains physically plausible while preserving the concept's
        # forest masses.
        "canopy": 28,
        "undergrowth": 20,
        "field_rock": 8,
        "groundcover": 20,
    }
    layer["instances_per_feature"] = max(floors.get(layer_id, 1), round(count * ratio))


def _compact_scale_pair(profile: dict[str, Any], linear_scale: float) -> None:
    scale = profile.get("scale_m")
    if not isinstance(scale, list) or len(scale) != 2:
        return
    if not all(isinstance(value, (int, float)) and not isinstance(value, bool) for value in scale):
        return
    profile["scale_m"] = [
        round(float(scale[0]) * linear_scale, 4),
        round(float(scale[1]) * linear_scale, 4),
    ]


def _compact_ecological_layer(layer: dict[str, Any], ratio: float) -> None:
    """Partially scale authored vegetation for a compact gameplay camera.

    Ecological assets remain recognizably physical objects, so scaling them by
    the full world ratio would make shrubs. Preserving source scalar values,
    however, produced 20–39 m canopy meshes in the 256 m arena. The fourth root
    of area is the geometric middle ground between those failure modes.
    """
    _compact_instances(layer, ratio)
    compact_scale = ratio**0.25
    scale = layer.get("scale_m")
    if not isinstance(scale, list) or len(scale) != 2:
        return
    role = str(layer.get("role", ""))
    minimum_scale = {
        "conifer_canopy": 0.40,
        "highland_understory": 0.25,
        "forest_floor_rock": 0.30,
    }.get(role, 0.20)
    layer["scale_m"] = [
        round(max(minimum_scale, float(scale[0]) * compact_scale), 4),
        round(max(minimum_scale, float(scale[1]) * compact_scale), 4),
    ]
    spacing = layer.get("minimum_spacing_m")
    if isinstance(spacing, (int, float)) and not isinstance(spacing, bool):
        minimum_spacing = {
            # The compact arena contains narrow forest polygons cut by lanes,
            # streams, keeps, and villages. A 3 m centre spacing made the
            # reviewed 80-tree screen-contribution floor geometrically
            # infeasible in two source regions despite correctly sized trees.
            "conifer_canopy": 1.80,
            "highland_understory": 1.25,
            "forest_floor_rock": 2.5,
        }.get(role, 1.0)
        layer["minimum_spacing_m"] = round(
            max(minimum_spacing, float(spacing) * math.sqrt(ratio)), 3
        )


def _compact_asset_profiles(profiles: dict[str, Any], ratio: float) -> dict[str, Any]:
    result = copy.deepcopy(profiles)
    macro_landmark_roles = {
        "faction_fortification",
        "objective_landmark",
        "lane_crossing_structure",
        "settlement_landmark",
    }
    for profile in result.values():
        if not isinstance(profile, dict):
            continue
        # The selected meshes are authored in world metres (the fortification
        # kit is 264 m across at scale 1).  A compact arena needs compact
        # macro landmarks; gameplay dimensions such as lane widths remain in
        # metres and are handled separately below.
        if profile.get("role") in macro_landmark_roles:
            _compact_scale_pair(profile, math.sqrt(ratio))
            if profile.get("role") == "settlement_landmark":
                # A single map-scale multiplier is not a physical contract:
                # the keep kit is 264 m wide while the authored village kit
                # is only 70 m. Linear compaction made each house toy-sized.
                # Preserve a bounded village envelope in which doors, houses,
                # and the watchtower remain plausible gameplay geometry.
                scale = profile.get("scale_m", [0.28, 0.34])
                lower = max(0.28, float(scale[0]))
                upper = max(lower, 0.34, float(scale[1]))
                profile["scale_m"] = [round(lower, 4), round(upper, 4)]
            continue
        layers = profile.get("layers")
        if isinstance(layers, list):
            for layer in layers:
                if isinstance(layer, dict):
                    _compact_ecological_layer(layer, ratio)
                    layer["maximum_slope_degrees"] = {
                        "canopy": 28.0,
                        "undergrowth": 32.0,
                        "field_rock": 38.0,
                        "groundcover": 30.0,
                    }.get(str(layer.get("id")), 32.0)
            if (
                profile.get("role") == "foliage"
                and not any(
                    isinstance(layer, dict)
                    and layer.get("id") == "groundcover"
                    for layer in layers
                )
            ):
                # Source-scale annotations describe canopy masses, but a
                # player-height compact map also needs a cheap near-ground
                # layer. This uses existing verified nature assets and remains
                # independent of canopy LOD accounting.
                layers.append(
                    {
                        "id": "groundcover",
                        "role": "highland_groundcover",
                        "instances_per_feature": 20,
                        "minimum_spacing_m": 0.65,
                        "maximum_slope_degrees": 30.0,
                        # Quaternius groundcover variants are authored at
                        # surprisingly different native scales (the fern is
                        # 2.83 m wide and Flower_4 is 2.49 m tall). Keep the
                        # shared range inside the 1.5 m physical envelope.
                        "scale_m": [0.22, 0.5],
                        "required_tags": ["foliage", "undergrowth"],
                        "source_prefixes": [
                            "assets/models/quaternius_nature/glTF/"
                        ],
                        "variant_count": 13,
                    }
                )
        else:
            if profile.get("role") == "landform_dressing":
                original_count = int(profile.get("instances_per_feature", 0))
                dense_massif = original_count >= 16
                # Continuous Alpine silhouettes belong to the terrain
                # grammar. Repeating dozens of mesh formations turns a ridge
                # wall into a PS1 cone forest, regardless of placement quality.
                # Props are a sparse secondary breakup layer only.
                profile["instances_per_feature"] = 4 if dense_massif else 2
                original_spacing = float(profile.get("minimum_spacing_m", 0.0))
                compact_spacing_scale = math.sqrt(ratio) * (
                    1.0 if dense_massif else 0.72
                )
                profile["minimum_spacing_m"] = round(
                    max(
                        18.0 if dense_massif else 10.0,
                        original_spacing * compact_spacing_scale,
                    ),
                    3,
                )
                # Interior source crags are compositional ridges, not pebbles.
                # Preserve a larger fraction of their vertical read than the
                # enclosing massif kit while keeping both in physical metres.
                _compact_scale_pair(
                    profile,
                    math.sqrt(ratio) * (1.0 if dense_massif else 1.85),
                )
            else:
                _compact_instances(profile, ratio)
    return result


def _compact_landform_relief(features: list[dict[str, Any]], ratio: float) -> dict[str, float]:
    """Keep compact arenas dramatic without preserving kilometre-map cliffs.

    Grade is approximately vertical relief divided by horizontal distance. A
    A modest reduction below linear scaling leaves room for lane grading and
    channel incision while retaining the reviewed Alpine silhouette.
    """
    default_vertical_scale = math.sqrt(ratio) * 0.88
    # Boundary mountains are compositional walls, not traversable hills. Full
    # linear compaction made a 280 m Alpine massif only 32 m tall in the
    # 256 m arena, so even sparse dressing meshes became the silhouette. Keep
    # a stronger sublinear relief scale for the terrain-primary massifs while
    # retaining conservative compaction for interior ridges and crags.
    # A 65–75 m near-vertical face consumed the visual hierarchy of the
    # complete 256 m arena. Preserve a clearly impassable Alpine border, but
    # keep it proportional to the battlefield and its 15 m interior crags.
    massif_vertical_scale = max(default_vertical_scale, ratio**0.40 * 0.78)
    for feature in features:
        if feature.get("category") != "landform":
            continue
        generation = feature.get("generation")
        if not isinstance(generation, dict):
            continue
        vertical_scale = (
            massif_vertical_scale
            if feature.get("semantic") == "alpine_massif"
            else default_vertical_scale
        )
        elevation = generation.get("elevation_m")
        if isinstance(elevation, list) and len(elevation) == 2:
            generation["elevation_m"] = [
                round(float(elevation[0]) * vertical_scale, 3),
                round(float(elevation[1]) * vertical_scale, 3),
            ]
        for key in ("snowline_m", "depth_m"):
            if isinstance(generation.get(key), (int, float)):
                generation[key] = round(float(generation[key]) * vertical_scale, 3)
    return {
        "default": default_vertical_scale,
        "alpine_massif": massif_vertical_scale,
    }


def _compact_waterways(features: list[dict[str, Any]], ratio: float) -> None:
    """Scale water footprints while preserving the source wetland morphology.

    The reviewed concept uses broad saturated ground and shallow braided rills,
    not deeply incised mountain streams.  Recording that distinction prevents
    the terrain compiler from treating every blue centerline as a trench.
    """
    linear_scale = math.sqrt(ratio)
    for feature in features:
        if feature.get("category") != "hydrology":
            continue
        properties = feature.get("properties")
        if not isinstance(properties, dict):
            continue
        width = properties.get("width_m")
        if isinstance(width, (int, float)) and not isinstance(width, bool):
            properties["width_m"] = round(max(2.0, float(width) * linear_scale), 3)
        properties.setdefault("channel_profile", "wetland_rill")


def _compact_scatter_exclusions(features: list[dict[str, Any]], ratio: float) -> None:
    """Scale landmark foliage clearances without making a small arena barren."""
    linear_scale = math.sqrt(ratio)
    defaults = {
        "faction_keep": (150.0, 20.0),
        "arcane_ruin": (58.0, 12.0),
        "settlement_cluster": (44.0, 16.0),
    }
    for feature in features:
        semantic = str(feature.get("semantic", ""))
        if semantic not in defaults:
            continue
        source_radius, compact_floor = defaults[semantic]
        properties = feature.setdefault("properties", {})
        properties["scatter_exclusion_radius_m"] = round(
            max(compact_floor, source_radius * linear_scale), 3
        )


def derive(source: dict[str, Any], target_draft: dict[str, Any]) -> dict[str, Any]:
    source_bounds = source.get("world_bounds")
    target_bounds = target_draft.get("world_bounds")
    if not isinstance(source_bounds, dict) or not isinstance(target_bounds, dict):
        raise ValueError("source and target need world_bounds")
    ratio = _area(target_bounds) / _area(source_bounds)
    if not 0.0 < ratio <= 1.0:
        raise ValueError("target must be a positive compact world, not a larger rescale")

    result = copy.deepcopy(target_draft)
    result["asset_profiles"] = _compact_asset_profiles(
        source.get("asset_profiles", {}), ratio
    )
    result["terrain_materials"] = copy.deepcopy(source.get("terrain_materials", {}))
    result["terrain_material_scale_m"] = copy.deepcopy(
        source.get("terrain_material_scale_m", {})
    )
    result["acceptance_policy"] = copy.deepcopy(
        source.get("acceptance_policy", target_draft.get("acceptance_policy", {}))
    )
    result["features"] = copy.deepcopy(source.get("features", []))
    relief_scales = _compact_landform_relief(result["features"], ratio)
    _compact_waterways(result["features"], ratio)
    _compact_scatter_exclusions(result["features"], ratio)

    policy = copy.deepcopy(source.get("traversal_policy", {}))
    policy["maximum_keep_lane_distance_m"] = min(
        float(policy.get("maximum_keep_lane_distance_m", 225.0)), 75.0
    )
    policy["maximum_objective_lane_distance_m"] = min(
        float(policy.get("maximum_objective_lane_distance_m", 280.0)), 96.0
    )
    policy["sample_spacing_m"] = min(float(policy.get("sample_spacing_m", 8.0)), 4.0)
    policy["maximum_lane_grade"] = min(
        float(policy.get("maximum_lane_grade", 0.72)), 0.25
    )
    policy["maximum_lane_p95_grade"] = min(
        float(policy.get("maximum_lane_p95_grade", 0.32)), 0.20
    )
    policy["maximum_lane_cross_grade"] = min(
        float(policy.get("maximum_lane_cross_grade", 0.40)), 0.20
    )
    result["traversal_policy"] = policy

    # Keep meters meaningful: entities do not become smaller just because the
    # arena is compact. A lane must still safely pass a 2.5 m-radius unit.
    compact_lane_width_m = 10.0
    # The visible road matches the lane it represents. It was 4.5 m against a
    # 10 m lane -- narrower than one 5 m-wide unit, so a character standing in
    # its own lane hung off both sides of the path art. The debug ribbon drew
    # at the declared width and hid that for as long as it was rendered.
    compact_lane_visual_width_m = compact_lane_width_m
    for feature in result["features"]:
        if feature.get("category") == "corridor" and feature.get("semantic") == "lane":
            properties = feature.setdefault("properties", {})
            properties["minimum_width_m"] = compact_lane_width_m
            properties["visual_width_m"] = compact_lane_visual_width_m
            # Curved splines consume part of the cross-grade budget because
            # their inner and outer edges project to slightly different
            # longitudinal stations. Keep the authored centre profile at 12%
            # so the complete 10 m roadbed remains comfortably traversable.
            properties["maximum_design_grade"] = 0.12
            properties["derive_proximity_crossings"] = True

    # Explicitly record the semantic transformation for downstream reports.
    result["derivation"] = {
        "schema_version": "codeweald.compact-arena-derivation/v1",
        "source_zone_id": source.get("zone", {}).get("id"),
        "source_world_bounds_m": source_bounds,
        "area_ratio": round(ratio, 8),
        "lane_width_m": compact_lane_width_m,
        "lane_visual_width_m": compact_lane_visual_width_m,
        "lane_design_grade_policy": "12-percent-profile; 25-percent-hard-acceptance",
        "bridge_socket_policy": "derive-near-contact-sockets-for-compact-lane-water-footprints",
        "mountain_elevation_policy": "semantic-sublinear-boundary-massif-relief",
        "mountain_vertical_scale": round(relief_scales["default"], 8),
        "alpine_massif_vertical_scale": round(
            relief_scales["alpine_massif"], 8
        ),
        "waterway_width_policy": "compact-linear-width-with-2m-floor",
        "waterway_morphology_policy": "shallow-wetland-rill-with-0.25m-maximum-incision",
        "landmark_clearance_policy": "compact-linear-radius-with-gameplay-floors",
        "dressing_density_policy": "area_scaled_with_compact-readability-floors",
        "ecological_scale_policy": "fourth-root-area-scale-with-physical-floors",
        "ecological_spacing_policy": "linear-world-scale-with-physical-floors",
        "groundcover_policy": "compact-player-height-layer-from-verified-undergrowth-assets",
        "macro_landform_policy": "terrain-primary-ridge-mass-with-sparse-secondary-mesh-dressing",
        "macro_landmark_scale_policy": "linear-world-scale-for-authored-environment-meshes",
    }
    return result


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Derive compact Codeweald arena annotations from a reviewed source"
    )
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--target-draft", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    result = derive(_read(arguments.source), _read(arguments.target_draft))
    _write(arguments.output, result)
    print("Wrote compact arena annotations to %s" % arguments.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
