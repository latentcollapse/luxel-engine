#!/usr/bin/env python3
"""Compile reviewed concept-art annotations into Codeweald's portable ZoneSpec.

This module deliberately does *not* infer a game world from colour thresholds.  A
vision model is useful for proposing annotations, but it is not the authority for
play-space topology.  The authority is a reviewable annotation file whose evidence
regions point back to the concept batch.  The compiler then produces deterministic,
engine-neutral world coordinates and a validation report that adapters can consume.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

from image_reconciliation import (
    EVIDENCE_CLAIMS,
    RECONCILIATION_VERSION,
    ROLE_ALLOWED_CLAIMS,
    ReconciliationError,
    build_reconciliation,
)

ZONE_SPEC_VERSION = "codeweald.zone-spec/v1"
ANNOTATION_VERSION = "codeweald.concept-annotations/v1"
SUPPORTED_GEOMETRIES = {"point", "polyline", "polygon"}
LAND_FORM_SEMANTICS = {
    "alpine_massif",
    "alpine_ridge",
    "foothills",
    "crag_field",
    "cliff_band",
    "valley_floor",
}
LAND_FORM_PROFILES = {
    "alpine_massif": {"alpine_jagged_massif"},
    "alpine_ridge": {"alpine_sawtooth_ridge"},
    "foothills": {"rolling_foothills"},
    "crag_field": {"scattered_crag_field"},
    "cliff_band": {"cliff_escarpment"},
    "valley_floor": {"glacial_valley_floor"},
}
SUPPORTED_REALMS = {"albion", "midgard", "hibernia"}
DEFAULT_ACCEPTANCE_POLICY = {
    "minimum_style_score": 0.42,
    "style_mismatch": "fail",
    "maximum_regional_palette_mismatch_fraction": 0.25,
    "minimum_biome_coverage_ratio": 0.20,
    "landmark_evidence_fill_ratio": [0.002, 2.5],
}
DEFAULT_TRAVERSAL_POLICY = {
    "agent_radius_m": 2.5,
    "agent_height_m": 8.0,
    "agent_max_climb_m": 4.0,
    "agent_max_slope_degrees": 45.0,
    "sample_spacing_m": 8.0,
    "maximum_lane_grade": 0.72,
    "maximum_lane_p95_grade": 0.32,
    "maximum_lane_cross_grade": 0.40,
    "maximum_keep_lane_distance_m": 225.0,
    "maximum_objective_lane_distance_m": 280.0,
}


def _segment_intersection(
    a: list[float],
    b: list[float],
    c: list[float],
    d: list[float],
) -> tuple[float, float, float] | None:
    """Return world X/Z plus lane-segment parameter for a proper intersection."""
    rx, rz = b[0] - a[0], b[1] - a[1]
    sx, sz = d[0] - c[0], d[1] - c[1]
    denominator = rx * sz - rz * sx
    if abs(denominator) < 1.0e-9:
        return None
    cx, cz = c[0] - a[0], c[1] - a[1]
    lane_t = (cx * sz - cz * sx) / denominator
    stream_t = (cx * rz - cz * rx) / denominator
    if not (0.0 <= lane_t <= 1.0 and 0.0 <= stream_t <= 1.0):
        return None
    return a[0] + lane_t * rx, a[1] + lane_t * rz, lane_t


def _nearest_lane_stream_contact(
    lane_points: list[list[float]], stream_points: list[list[float]]
) -> tuple[float, float, float, float, float] | None:
    """Return closest lane contact to a stream when their centre lines miss.

    Traversal works against physical lane/stream widths, not infinitesimal
    centre lines. This supplies the matching bridge socket when those authored
    footprints overlap or touch without a mathematical segment intersection.
    """
    best: tuple[float, float, float, float, float] | None = None

    def consider(
        lane_x: float, lane_z: float, other_x: float, other_z: float, dx: float, dz: float
    ) -> None:
        nonlocal best
        distance = math.hypot(lane_x - other_x, lane_z - other_z)
        candidate = (distance, lane_x, lane_z, dx, dz)
        if best is None or candidate < best:
            best = candidate

    def projection(
        point_x: float, point_z: float, start: list[float], finish: list[float]
    ) -> tuple[float, float]:
        dx, dz = finish[0] - start[0], finish[1] - start[1]
        denominator = dx * dx + dz * dz
        amount = 0.0 if denominator <= 1e-12 else max(0.0, min(1.0, ((point_x - start[0]) * dx + (point_z - start[1]) * dz) / denominator))
        return start[0] + amount * dx, start[1] + amount * dz

    for a, b in zip(lane_points, lane_points[1:]):
        lane_dx, lane_dz = b[0] - a[0], b[1] - a[1]
        for c, d in zip(stream_points, stream_points[1:]):
            for point in (a, b):
                stream_x, stream_z = projection(point[0], point[1], c, d)
                consider(point[0], point[1], stream_x, stream_z, lane_dx, lane_dz)
            for point in (c, d):
                lane_x, lane_z = projection(point[0], point[1], a, b)
                consider(lane_x, lane_z, point[0], point[1], lane_dx, lane_dz)
    return best


def _resolve_stream_landmark_clearance(
    features: list[dict[str, Any]],
    width: float,
    length: float,
    agent_radius_m: float,
) -> None:
    """Detour watercourses around protected gameplay landmarks.

    Source polylines describe visible topology, but a coarse segment can pass
    directly through a compact landmark after world scaling. Resolve that
    conflict once in ZoneSpec so terrain carving and every engine adapter share
    the same deterministic route.
    """
    landmarks = [
        feature
        for feature in features
        if feature.get("category") == "landmark"
        and feature.get("semantic") in {"arcane_ruin", "faction_keep"}
        and feature.get("geometry", {}).get("type") == "point"
        and feature.get("geometry", {}).get("points")
    ]
    for stream in features:
        if (
            stream.get("category") != "hydrology"
            or stream.get("semantic") not in {"stream", "river"}
            or stream.get("geometry", {}).get("type") != "polyline"
        ):
            continue
        points = stream["geometry"].get("points", [])
        if not isinstance(points, list) or len(points) < 2:
            continue
        resolved: list[list[float]] = [list(points[0])]
        resolved_landmarks: list[str] = []
        stream_half_width = (
            float(stream.get("properties", {}).get("width_m", 1.0)) * 0.5
        )
        for start, finish in zip(points, points[1:]):
            dx, dz = finish[0] - start[0], finish[1] - start[1]
            segment_length = math.hypot(dx, dz)
            if segment_length <= 1.0e-9:
                continue
            direction_x, direction_z = dx / segment_length, dz / segment_length
            detours: list[tuple[float, list[list[float]], str]] = []
            for landmark in landmarks:
                landmark_point = landmark["geometry"]["points"][0]
                lx, lz = float(landmark_point[0]), float(landmark_point[1])
                amount = max(
                    0.0,
                    min(
                        1.0,
                        ((lx - start[0]) * dx + (lz - start[1]) * dz)
                        / (segment_length * segment_length),
                    ),
                )
                if amount <= 0.02 or amount >= 0.98:
                    continue
                nearest_x = start[0] + amount * dx
                nearest_z = start[1] + amount * dz
                clearance = (
                    float(
                        landmark.get("properties", {}).get(
                            "scatter_exclusion_radius_m", 0.0
                        )
                    )
                    + stream_half_width
                    + agent_radius_m
                )
                if clearance <= 0.0 or math.hypot(lx - nearest_x, lz - nearest_z) >= clearance:
                    continue
                perpendicular_x, perpendicular_z = -direction_z, direction_x
                signed_side = (
                    direction_x * (lz - nearest_z)
                    - direction_z * (lx - nearest_x)
                )
                if abs(signed_side) <= 1.0e-9:
                    digest = hashlib.sha256(
                        ("%s::%s" % (stream.get("id"), landmark.get("id"))).encode()
                    ).digest()
                    side = 1.0 if digest[0] % 2 == 0 else -1.0
                else:
                    side = -1.0 if signed_side > 0.0 else 1.0
                lateral = clearance * 1.25
                along = min(clearance * 1.10, segment_length * 0.30)
                before = [
                    nearest_x - direction_x * along
                    + perpendicular_x * lateral * side,
                    nearest_z - direction_z * along
                    + perpendicular_z * lateral * side,
                ]
                after = [
                    nearest_x + direction_x * along
                    + perpendicular_x * lateral * side,
                    nearest_z + direction_z * along
                    + perpendicular_z * lateral * side,
                ]
                for point in (before, after):
                    point[0] = max(-width * 0.5, min(width * 0.5, point[0]))
                    point[1] = max(-length * 0.5, min(length * 0.5, point[1]))
                    # Generated geometry crosses the Python/Rust canonical
                    # provenance boundary. Quantize it like bridge sockets so
                    # both JSON implementations hash identical numeric text.
                    point[0] = round(point[0], 4)
                    point[1] = round(point[1], 4)
                detours.append(
                    (amount, [before, after], str(landmark.get("id", "")))
                )
            for _amount, detour_points, landmark_id in sorted(detours):
                resolved.extend(detour_points)
                if landmark_id and landmark_id not in resolved_landmarks:
                    resolved_landmarks.append(landmark_id)
            resolved.append(list(finish))
        if resolved_landmarks:
            stream["geometry"]["points"] = resolved
            properties = stream.setdefault("properties", {})
            properties["resolved_landmark_exclusions"] = resolved_landmarks


def _derive_lane_water_crossings(
    features: list[dict[str, Any]],
    asset_profiles: dict[str, dict[str, Any]],
    width: float,
    length: float,
    agent_radius_m: float,
) -> list[dict[str, Any]]:
    """Derive reviewed bridge sockets from authored lane/stream topology."""
    lanes = [
        feature
        for feature in features
        if feature.get("category") == "corridor"
        and feature.get("semantic") == "lane"
        and isinstance(
            feature.get("properties", {}).get("crossing_asset_profile"), str
        )
    ]
    streams = [
        feature
        for feature in features
        if feature.get("category") == "hydrology"
        and feature.get("semantic") in {"stream", "river"}
    ]
    crossings: list[dict[str, Any]] = []
    for lane in lanes:
        profile_id = str(lane["properties"]["crossing_asset_profile"])
        if profile_id not in asset_profiles:
            raise ZoneCompileError(
                "Lane %s references unknown crossing asset profile %s"
                % (lane["id"], profile_id)
            )
        lane_points = lane["geometry"]["points"]
        for stream in streams:
            stream_points = stream["geometry"]["points"]
            contacts: list[tuple[float, float, float, float]] = []
            ordinal = 0
            for lane_index, (a, b) in enumerate(zip(lane_points, lane_points[1:])):
                for c, d in zip(stream_points, stream_points[1:]):
                    intersection = _segment_intersection(a, b, c, d)
                    if intersection is None:
                        continue
                    x, z, _lane_t = intersection
                    dx, dz = b[0] - a[0], b[1] - a[1]
                    contacts.append((x, z, dx, dz))
            if not contacts and bool(lane["properties"].get("derive_proximity_crossings", False)):
                contact = _nearest_lane_stream_contact(lane_points, stream_points)
                lane_width = float(lane["properties"].get("minimum_width_m", 1.0))
                stream_width = float(stream.get("properties", {}).get("width_m", 1.0))
                clearance = lane_width * 0.5 + stream_width * 0.5 + agent_radius_m
                if contact is not None and contact[0] <= clearance:
                    _distance, x, z, dx, dz = contact
                    contacts.append((x, z, dx, dz))
            for x, z, dx, dz in contacts:
                    source_x = x / width + 0.5
                    source_y = 0.5 - z / length
                    bridge_id = "bridge_%s_%s_%d" % (
                        lane["id"],
                        stream["id"],
                        ordinal,
                    )
                    crossings.append(
                        {
                            "id": bridge_id,
                            "category": "structure",
                            "semantic": "bridge",
                            "geometry": {
                                "type": "point",
                                "points": [[round(x, 4), round(z, 4)]],
                                "source_points": [
                                    [round(source_x, 7), round(source_y, 7)]
                                ],
                                "source_image_id": lane["geometry"].get(
                                    "source_image_id"
                                ),
                            },
                            "generation": {"asset_profile": profile_id},
                            "properties": {
                                "bridge_type": "highland_stone_full_lane",
                                "derived_from": [lane["id"], stream["id"]],
                                "lane_id": lane["properties"].get("lane_id"),
                                "lane_width_m": float(
                                    lane["properties"].get("minimum_width_m", 1.0)
                                ),
                                "stream_width_m": float(
                                    stream.get("properties", {}).get("width_m", 1.0)
                                ),
                                "rotation_degrees": round(
                                    math.degrees(math.atan2(dx, dz)), 4
                                ),
                            },
                            "evidence": lane["evidence"] + stream["evidence"],
                            "confidence": min(
                                float(lane["confidence"]), float(stream["confidence"])
                            ),
                            "review_state": (
                                "reviewed"
                                if lane["review_state"] == "reviewed"
                                and stream["review_state"] == "reviewed"
                                else "proposed"
                            ),
                        }
                    )
                    ordinal += 1
    crossings.sort(key=lambda feature: str(feature["id"]))
    return crossings


def _asset_profile_reference(feature: dict[str, Any]) -> str | None:
    generation = feature.get("generation", {})
    properties = feature.get("properties", {})
    if isinstance(generation, dict) and isinstance(generation.get("asset_profile"), str):
        return generation["asset_profile"]
    if isinstance(properties, dict) and isinstance(properties.get("asset_profile"), str):
        return properties["asset_profile"]
    return None


class ZoneCompileError(ValueError):
    """Raised when annotations are too incomplete or ambiguous to build safely."""


@dataclass(frozen=True)
class CompileResult:
    zone_spec: dict[str, Any]
    report: dict[str, Any]


def _read_json(path: Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as source:
            value = json.load(source)
    except (OSError, json.JSONDecodeError) as exc:
        raise ZoneCompileError("Cannot read %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise ZoneCompileError("%s must contain one JSON object" % path)
    return value


def _write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as destination:
        json.dump(value, destination, indent=2, sort_keys=True)
        destination.write("\n")


def _as_number(value: Any, label: str) -> float:
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        raise ZoneCompileError("%s must be a number" % label)
    return float(value)


def _normalised_point(value: Any, label: str) -> list[float]:
    if not isinstance(value, list) or len(value) != 2:
        raise ZoneCompileError("%s must be a [x, y] point" % label)
    x, y = _as_number(value[0], label + "[0]"), _as_number(value[1], label + "[1]")
    if not (0.0 <= x <= 1.0 and 0.0 <= y <= 1.0):
        raise ZoneCompileError("%s must stay inside the normalised concept canvas" % label)
    return [x, y]


def _world_point(point: list[float], width: float, length: float) -> list[float]:
    """Map top-left image coordinates to an X/Z plane centered at world origin."""
    return [round((point[0] - 0.5) * width, 4), round((0.5 - point[1]) * length, 4)]


def _geometry_to_world(
    geometry: dict[str, Any],
    width: float,
    length: float,
    label: str,
    canonical_map_image_id: str,
    multi_image: bool,
) -> dict[str, Any]:
    geometry_type = geometry.get("type")
    if geometry_type not in SUPPORTED_GEOMETRIES:
        raise ZoneCompileError("%s.geometry.type must be one of %s" % (label, sorted(SUPPORTED_GEOMETRIES)))
    points = geometry.get("points")
    if not isinstance(points, list):
        raise ZoneCompileError("%s.geometry.points must be an array" % label)
    min_points = 1 if geometry_type == "point" else (2 if geometry_type == "polyline" else 3)
    if len(points) < min_points:
        raise ZoneCompileError("%s.geometry needs at least %d points" % (label, min_points))
    source_image_id = geometry.get("source_image_id")
    if source_image_id is None and multi_image:
        raise ZoneCompileError(
            "%s.geometry.source_image_id is required for a multi-image batch"
            % label
        )
    if source_image_id is None:
        source_image_id = canonical_map_image_id
    if source_image_id != canonical_map_image_id:
        raise ZoneCompileError(
            "%s.geometry.source_image_id must identify canonical map %s"
            % (label, canonical_map_image_id)
        )
    normalised = [_normalised_point(point, "%s.geometry.points[%d]" % (label, index)) for index, point in enumerate(points)]
    return {
        "type": geometry_type,
        "points": [_world_point(point, width, length) for point in normalised],
        "source_points": normalised,
        "source_image_id": source_image_id,
    }


def _validate_source_images(
    source_images: Any, source_root: Path | None = None
) -> dict[str, dict[str, Any]]:
    if not isinstance(source_images, list) or not source_images:
        raise ZoneCompileError("At least one source_images entry is required")
    source_ids: set[str] = set()
    for index, source in enumerate(source_images):
        label = "source_images[%d]" % index
        if not isinstance(source, dict):
            raise ZoneCompileError("%s must be an object" % label)
        source_id = source.get("id")
        relative_path = source.get("path")
        digest = source.get("sha256")
        if not isinstance(source_id, str) or not source_id:
            raise ZoneCompileError("%s.id must be a non-empty string" % label)
        if source_id in source_ids:
            raise ZoneCompileError("Duplicate source image id: %s" % source_id)
        source_ids.add(source_id)
        if not isinstance(relative_path, str) or not relative_path:
            raise ZoneCompileError("%s.path must be a non-empty relative path" % label)
        if Path(relative_path).is_absolute() or ".." in Path(relative_path).parts:
            raise ZoneCompileError("%s.path must remain inside the concept batch" % label)
        if not isinstance(digest, str) or len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest.lower()):
            raise ZoneCompileError("%s.sha256 must be a 64-character hexadecimal digest" % label)
        role = source.get("role", "reference")
        if role not in ROLE_ALLOWED_CLAIMS:
            raise ZoneCompileError(
                "%s.role must be a supported image-reconciliation role" % label
            )
        for dimension in ("width_px", "height_px"):
            if dimension in source and (
                not isinstance(source[dimension], int)
                or isinstance(source[dimension], bool)
                or int(source[dimension]) <= 0
            ):
                raise ZoneCompileError("%s.%s must be a positive integer" % (label, dimension))
        if source_root is not None:
            source_path = source_root / relative_path
            try:
                actual_digest = hashlib.sha256(source_path.read_bytes()).hexdigest()
            except OSError as exc:
                raise ZoneCompileError("%s source file is unavailable: %s" % (label, source_path)) from exc
            if actual_digest != digest.lower():
                raise ZoneCompileError("%s.sha256 does not match %s" % (label, source_path))
    return {str(source["id"]): source for source in source_images}


def _source_image_ids(source_images: list[dict[str, Any]]) -> set[str]:
    return {str(source["id"]) for source in source_images}


def _validate_reconciliation(
    annotations: dict[str, Any],
    width: float,
    length: float,
) -> dict[str, Any]:
    source_images = annotations.get("source_images", [])
    provided = annotations.get("image_reconciliation")
    if provided is None and len(source_images) > 1:
        raise ZoneCompileError(
            "Multi-image annotations require immutable image_reconciliation"
        )
    if provided is not None and not isinstance(provided, dict):
        raise ZoneCompileError("image_reconciliation must be an object")
    canonical = (
        str(provided.get("canonical_map_image_id"))
        if isinstance(provided, dict)
        and isinstance(provided.get("canonical_map_image_id"), str)
        else None
    )
    try:
        expected = build_reconciliation(source_images, canonical, width, length)
    except ReconciliationError as exc:
        raise ZoneCompileError(str(exc)) from exc
    if provided is not None:
        if provided.get("schema_version") != RECONCILIATION_VERSION:
            raise ZoneCompileError(
                "image_reconciliation.schema_version must be %s"
                % RECONCILIATION_VERSION
            )
        if provided != expected:
            raise ZoneCompileError(
                "image_reconciliation does not match immutable source roles, dimensions, and canonical map"
            )
    return expected


def _validate_evidence(
    evidence: Any,
    label: str,
    source_policies: dict[str, dict[str, Any]],
    multi_image: bool,
    review_state: str,
) -> list[dict[str, Any]]:
    if not isinstance(evidence, list) or not evidence:
        raise ZoneCompileError("%s.evidence must cite at least one concept-image region" % label)
    normalized: list[dict[str, Any]] = []
    for index, entry in enumerate(evidence):
        evidence_label = "%s.evidence[%d]" % (label, index)
        if not isinstance(entry, dict):
            raise ZoneCompileError("%s must be an object" % evidence_label)
        image_id = entry.get("image_id")
        if not isinstance(image_id, str) or image_id not in source_policies:
            raise ZoneCompileError("%s.image_id must identify a declared source image" % evidence_label)
        region = entry.get("region")
        if not isinstance(region, list) or len(region) != 4:
            raise ZoneCompileError("%s.region must be [left, top, right, bottom]" % evidence_label)
        left, top, right, bottom = (_as_number(region[value], "%s.region[%d]" % (evidence_label, value)) for value in range(4))
        if not (0.0 <= left < right <= 1.0 and 0.0 <= top < bottom <= 1.0):
            raise ZoneCompileError("%s.region must be an ordered normalized rectangle" % evidence_label)
        if not isinstance(entry.get("note"), str) or not str(entry["note"]).strip():
            raise ZoneCompileError("%s.note must be a non-empty explanation" % evidence_label)
        claim = entry.get("claim")
        if claim is None and multi_image:
            raise ZoneCompileError(
                "%s.claim is required for a multi-image batch" % evidence_label
            )
        if claim is None:
            claim = "topology"
        if claim not in EVIDENCE_CLAIMS:
            raise ZoneCompileError(
                "%s.claim must be one of %s"
                % (evidence_label, sorted(EVIDENCE_CLAIMS))
            )
        allowed_claims = source_policies[image_id]["allowed_claims"]
        if claim not in allowed_claims:
            raise ZoneCompileError(
                "%s role %s cannot support %s claims"
                % (evidence_label, source_policies[image_id]["role"], claim)
            )
        visibility = entry.get("visibility")
        if visibility is None and multi_image:
            raise ZoneCompileError(
                "%s.visibility is required for a multi-image batch"
                % evidence_label
            )
        if visibility is None:
            visibility = "direct"
        if visibility not in {"direct", "partial", "inferred"}:
            raise ZoneCompileError(
                "%s.visibility must be direct, partial, or inferred"
                % evidence_label
            )
        if visibility == "inferred" and review_state != "reviewed":
            raise ZoneCompileError(
                "%s inferred visibility requires a reviewed feature"
                % evidence_label
            )
        normalized_entry = dict(entry)
        normalized_entry["claim"] = claim
        normalized_entry["visibility"] = visibility
        normalized.append(normalized_entry)
    return normalized


def _validate_asset_profiles(value: Any) -> dict[str, dict[str, Any]]:
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise ZoneCompileError("asset_profiles must be an object")
    def validate_layer(label: str, layer: Any) -> None:
        if not isinstance(layer, dict):
            raise ZoneCompileError("%s must be an object" % label)
        required_tags = layer.get("required_tags")
        if not isinstance(required_tags, list) or not required_tags or not all(isinstance(tag, str) and tag for tag in required_tags):
            raise ZoneCompileError("%s.required_tags must be a non-empty string array" % label)
        for field in ("variant_count", "instances_per_feature"):
            if not isinstance(layer.get(field), int) or int(layer[field]) <= 0:
                raise ZoneCompileError("%s.%s must be a positive integer" % (label, field))
        scale = layer.get("scale_m")
        if not isinstance(scale, list) or len(scale) != 2:
            raise ZoneCompileError("%s.scale_m must be [min, max]" % label)
        scale_min, scale_max = _as_number(scale[0], label + ".scale_m[0]"), _as_number(scale[1], label + ".scale_m[1]")
        if scale_min <= 0.0 or scale_max < scale_min:
            raise ZoneCompileError("%s.scale_m must be positive and ordered" % label)
        if "minimum_spacing_m" in layer and _as_number(layer["minimum_spacing_m"], label + ".minimum_spacing_m") <= 0.0:
            raise ZoneCompileError("%s.minimum_spacing_m must be positive" % label)
        if "maximum_slope_degrees" in layer:
            maximum_slope = _as_number(
                layer["maximum_slope_degrees"],
                label + ".maximum_slope_degrees",
            )
            if not 0.0 < maximum_slope <= 60.0:
                raise ZoneCompileError(
                    "%s.maximum_slope_degrees must be in (0, 60]" % label
                )
        prefixes = layer.get("source_prefixes", [])
        if not isinstance(prefixes, list) or not all(isinstance(prefix, str) and prefix and not Path(prefix).is_absolute() and ".." not in Path(prefix).parts for prefix in prefixes):
            raise ZoneCompileError("%s.source_prefixes must contain safe relative prefixes" % label)
        formats = layer.get("formats", [])
        if not isinstance(formats, list) or not all(isinstance(format_name, str) and format_name in {"glb", "gltf", "fbx", "obj"} for format_name in formats):
            raise ZoneCompileError("%s.formats must contain supported portable mesh formats" % label)
        if "runtime_enabled" in layer and not isinstance(layer["runtime_enabled"], bool):
            raise ZoneCompileError("%s.runtime_enabled must be boolean" % label)
        quality = layer.get("quality_requirements", {})
        if not isinstance(quality, dict):
            raise ZoneCompileError("%s.quality_requirements must be an object" % label)
        for field in ("minimum_triangle_count", "minimum_material_slots"):
            if field in quality and (not isinstance(quality[field], int) or int(quality[field]) <= 0):
                raise ZoneCompileError("%s.quality_requirements.%s must be a positive integer" % (label, field))
        appearance = layer.get("appearance_requirements", {})
        if not isinstance(appearance, dict):
            raise ZoneCompileError("%s.appearance_requirements must be an object" % label)
        allowed_appearance = {"maximum_mean_luminance", "maximum_bright_fraction"}
        unknown_appearance = set(appearance) - allowed_appearance
        if unknown_appearance:
            raise ZoneCompileError(
                "%s.appearance_requirements has unsupported fields: %s"
                % (label, ", ".join(sorted(unknown_appearance)))
            )
        for field in allowed_appearance:
            if field in appearance and (
                not isinstance(appearance[field], (int, float))
                or isinstance(appearance[field], bool)
                or not 0.0 <= float(appearance[field]) <= 1.0
            ):
                raise ZoneCompileError("%s.appearance_requirements.%s must be between 0 and 1" % (label, field))

    profiles: dict[str, dict[str, Any]] = {}
    for profile_id, profile in value.items():
        if not isinstance(profile_id, str) or not profile_id or not isinstance(profile, dict):
            raise ZoneCompileError("asset_profiles must contain named objects")
        layers = profile.get("layers")
        if layers is None:
            validate_layer("asset_profiles.%s" % profile_id, profile)
        else:
            if not isinstance(layers, list) or not layers:
                raise ZoneCompileError("asset_profiles.%s.layers must be a non-empty array" % profile_id)
            seen_layer_ids: set[str] = set()
            for index, layer in enumerate(layers):
                label = "asset_profiles.%s.layers[%d]" % (profile_id, index)
                validate_layer(label, layer)
                layer_id = layer.get("id") if isinstance(layer, dict) else None
                if not isinstance(layer_id, str) or not layer_id or layer_id in seen_layer_ids:
                    raise ZoneCompileError("%s.id must be a unique non-empty string" % label)
                seen_layer_ids.add(layer_id)
        profiles[profile_id] = profile
    return profiles


def _validate_terrain_material_requests(value: Any) -> dict[str, str]:
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise ZoneCompileError("terrain_materials must be an object")
    valid_layers = {"grass", "road", "rock", "wetland", "snow"}
    requests: dict[str, str] = {}
    for layer, material_id in value.items():
        if layer not in valid_layers or not isinstance(material_id, str) or not material_id:
            raise ZoneCompileError("terrain_materials must map grass, road, rock, wetland, or snow to non-empty material ids")
        requests[layer] = material_id
    return requests


def _validate_terrain_material_scales(value: Any) -> dict[str, float]:
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise ZoneCompileError("terrain_material_scale_m must be an object")
    valid_layers = {"grass", "road", "rock", "wetland", "snow"}
    scales: dict[str, float] = {}
    for layer, raw_scale in value.items():
        if layer not in valid_layers:
            raise ZoneCompileError(
                "terrain_material_scale_m has unsupported layer %s" % layer
            )
        scale = _as_number(raw_scale, "terrain_material_scale_m.%s" % layer)
        if not 1.0 <= scale <= 32.0:
            raise ZoneCompileError(
                "terrain_material_scale_m.%s must be in [1, 32] meters"
                % layer
            )
        scales[layer] = scale
    return scales


def _validate_acceptance_policy(value: Any) -> dict[str, Any]:
    if value is None:
        return dict(DEFAULT_ACCEPTANCE_POLICY)
    if not isinstance(value, dict):
        raise ZoneCompileError("acceptance_policy must be an object")
    unknown = set(value) - set(DEFAULT_ACCEPTANCE_POLICY)
    if unknown:
        raise ZoneCompileError("acceptance_policy has unsupported fields: %s" % ", ".join(sorted(unknown)))
    minimum = value.get("minimum_style_score", DEFAULT_ACCEPTANCE_POLICY["minimum_style_score"])
    if not isinstance(minimum, (int, float)) or isinstance(minimum, bool) or not 0.0 <= float(minimum) <= 1.0:
        raise ZoneCompileError("acceptance_policy.minimum_style_score must be between 0 and 1")
    mismatch = value.get("style_mismatch", DEFAULT_ACCEPTANCE_POLICY["style_mismatch"])
    if mismatch not in {"fail", "warn"}:
        raise ZoneCompileError("acceptance_policy.style_mismatch must be fail or warn")
    maximum_palette_mismatch = value.get(
        "maximum_regional_palette_mismatch_fraction",
        DEFAULT_ACCEPTANCE_POLICY[
            "maximum_regional_palette_mismatch_fraction"
        ],
    )
    minimum_biome_coverage = value.get(
        "minimum_biome_coverage_ratio",
        DEFAULT_ACCEPTANCE_POLICY["minimum_biome_coverage_ratio"],
    )
    for field, field_value in (
        (
            "maximum_regional_palette_mismatch_fraction",
            maximum_palette_mismatch,
        ),
        ("minimum_biome_coverage_ratio", minimum_biome_coverage),
    ):
        if (
            not isinstance(field_value, (int, float))
            or isinstance(field_value, bool)
            or not 0.0 <= float(field_value) <= 1.0
        ):
            raise ZoneCompileError(
                "acceptance_policy.%s must be between 0 and 1" % field
            )
    landmark_fill = value.get(
        "landmark_evidence_fill_ratio",
        DEFAULT_ACCEPTANCE_POLICY["landmark_evidence_fill_ratio"],
    )
    if not isinstance(landmark_fill, list) or len(landmark_fill) != 2:
        raise ZoneCompileError(
            "acceptance_policy.landmark_evidence_fill_ratio must be [min, max]"
        )
    landmark_min = _as_number(
        landmark_fill[0],
        "acceptance_policy.landmark_evidence_fill_ratio[0]",
    )
    landmark_max = _as_number(
        landmark_fill[1],
        "acceptance_policy.landmark_evidence_fill_ratio[1]",
    )
    if landmark_min < 0.0 or landmark_max <= landmark_min:
        raise ZoneCompileError(
            "acceptance_policy.landmark_evidence_fill_ratio must be non-negative and ordered"
        )
    return {
        "minimum_style_score": float(minimum),
        "style_mismatch": mismatch,
        "maximum_regional_palette_mismatch_fraction": float(
            maximum_palette_mismatch
        ),
        "minimum_biome_coverage_ratio": float(minimum_biome_coverage),
        "landmark_evidence_fill_ratio": [landmark_min, landmark_max],
    }


def _validate_traversal_policy(value: Any) -> dict[str, float]:
    if value is None:
        return dict(DEFAULT_TRAVERSAL_POLICY)
    if not isinstance(value, dict):
        raise ZoneCompileError("traversal_policy must be an object")
    unknown = set(value) - set(DEFAULT_TRAVERSAL_POLICY)
    if unknown:
        raise ZoneCompileError(
            "traversal_policy has unsupported fields: %s"
            % ", ".join(sorted(unknown))
        )
    policy = dict(DEFAULT_TRAVERSAL_POLICY)
    for field, default in DEFAULT_TRAVERSAL_POLICY.items():
        raw = value.get(field, default)
        if not isinstance(raw, (int, float)) or isinstance(raw, bool):
            raise ZoneCompileError("traversal_policy.%s must be numeric" % field)
        policy[field] = float(raw)
    positive = set(DEFAULT_TRAVERSAL_POLICY) - {
        "maximum_lane_grade",
        "maximum_lane_p95_grade",
        "maximum_lane_cross_grade",
    }
    for field in positive:
        if policy[field] <= 0.0:
            raise ZoneCompileError("traversal_policy.%s must be positive" % field)
    for field in (
        "maximum_lane_grade",
        "maximum_lane_p95_grade",
        "maximum_lane_cross_grade",
    ):
        if not 0.0 < policy[field] <= 2.0:
            raise ZoneCompileError(
                "traversal_policy.%s must be in (0, 2]" % field
            )
    if not 1.0 <= policy["agent_max_slope_degrees"] <= 60.0:
        raise ZoneCompileError(
            "traversal_policy.agent_max_slope_degrees must be in [1, 60]"
        )
    return policy


def _validate_derivation(value: Any) -> dict[str, Any]:
    """Preserve scale provenance when an annotation is a derived world."""
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise ZoneCompileError("derivation must be an object")
    if value.get("schema_version") != "codeweald.compact-arena-derivation/v1":
        raise ZoneCompileError(
            "derivation.schema_version must be "
            "codeweald.compact-arena-derivation/v1"
        )
    source_bounds = value.get("source_world_bounds_m")
    if not isinstance(source_bounds, dict):
        raise ZoneCompileError("derivation.source_world_bounds_m is required")
    source_width = _as_number(
        source_bounds.get("width"), "derivation.source_world_bounds_m.width"
    )
    source_length = _as_number(
        source_bounds.get("length"), "derivation.source_world_bounds_m.length"
    )
    if source_width <= 0.0 or source_length <= 0.0:
        raise ZoneCompileError(
            "derivation.source_world_bounds_m dimensions must be positive"
        )
    source_zone_id = value.get("source_zone_id")
    if not isinstance(source_zone_id, str) or not source_zone_id:
        raise ZoneCompileError("derivation.source_zone_id is required")
    result = dict(value)
    result["source_world_bounds_m"] = {
        "width": source_width,
        "length": source_length,
    }
    return result


def _validate_annotation_header(
    annotations: dict[str, Any], source_root: Path | None = None
) -> tuple[
    float,
    float,
    dict[str, dict[str, Any]],
    dict[str, str],
    dict[str, float],
    dict[str, Any],
    dict[str, float],
    dict[str, Any],
    dict[str, Any],
]:
    if annotations.get("schema_version") != ANNOTATION_VERSION:
        raise ZoneCompileError("schema_version must be %s" % ANNOTATION_VERSION)
    bounds = annotations.get("world_bounds")
    if not isinstance(bounds, dict):
        raise ZoneCompileError("world_bounds is required")
    width = _as_number(bounds.get("width"), "world_bounds.width")
    length = _as_number(bounds.get("length"), "world_bounds.length")
    if width <= 0 or length <= 0:
        raise ZoneCompileError("world_bounds dimensions must be positive")
    _validate_source_images(annotations.get("source_images"), source_root)
    reconciliation = _validate_reconciliation(annotations, width, length)
    return (
        width,
        length,
        _validate_asset_profiles(annotations.get("asset_profiles")),
        _validate_terrain_material_requests(annotations.get("terrain_materials")),
        _validate_terrain_material_scales(
            annotations.get("terrain_material_scale_m")
        ),
        _validate_acceptance_policy(annotations.get("acceptance_policy")),
        _validate_traversal_policy(annotations.get("traversal_policy")),
        reconciliation,
        _validate_derivation(annotations.get("derivation")),
    )


def _validate_landform_generation(feature: dict[str, Any], label: str) -> None:
    """Keep named landforms from silently degenerating into generic noise."""
    semantic = str(feature.get("semantic", ""))
    generation = feature.get("generation", {})
    if not isinstance(generation, dict):
        raise ZoneCompileError("%s.generation must be an object" % label)
    profile = generation.get("profile")
    allowed = LAND_FORM_PROFILES.get(semantic, set())
    if profile not in allowed:
        raise ZoneCompileError("%s %s must use one of %s, not %r" % (label, semantic, sorted(allowed), profile))
    elevation = generation.get("elevation_m")
    if not isinstance(elevation, list) or len(elevation) != 2:
        raise ZoneCompileError("%s.generation.elevation_m must be [min, max]" % label)
    low, high = _as_number(elevation[0], label + ".generation.elevation_m[0]"), _as_number(elevation[1], label + ".generation.elevation_m[1]")
    if high <= low:
        raise ZoneCompileError("%s.generation.elevation_m must be ordered with non-zero relief" % label)
    if semantic in {"alpine_massif", "alpine_ridge", "cliff_band"}:
        cliffness = _as_number(generation.get("cliffness"), label + ".generation.cliffness")
        if not 0.0 <= cliffness <= 1.0:
            raise ZoneCompileError("%s.generation.cliffness must be in [0, 1]" % label)
    if semantic == "alpine_massif":
        snowline = _as_number(generation.get("snowline_m"), label + ".generation.snowline_m")
        if snowline < low or snowline > high:
            raise ZoneCompileError("%s.generation.snowline_m must stay inside elevation_m" % label)
    if semantic == "valley_floor" and "depth_m" in generation and _as_number(generation["depth_m"], label + ".generation.depth_m") <= 0.0:
        raise ZoneCompileError("%s.generation.depth_m must be positive" % label)
    composition = generation.get("composition")
    if composition is None:
        return
    if not isinstance(composition, dict):
        raise ZoneCompileError("%s.generation.composition must be an object" % label)
    pattern = composition.get("pattern")
    allowed_patterns = {
        "alpine_massif": {"ridge_network"},
        "alpine_ridge": {"clustered_ridges"},
        "crag_field": {"clustered_ridges"},
    }.get(semantic, set())
    if pattern not in allowed_patterns:
        raise ZoneCompileError(
            "%s.generation.composition.pattern must be one of %s"
            % (label, sorted(allowed_patterns))
        )
    spine_count = composition.get("spine_count")
    if (
        not isinstance(spine_count, int)
        or isinstance(spine_count, bool)
        or not 1 <= spine_count <= 8
    ):
        raise ZoneCompileError(
            "%s.generation.composition.spine_count must be an integer in [1, 8]"
            % label
        )
    for key in ("elevation_bias", "along_jitter", "cross_jitter"):
        value = _as_number(
            composition.get(key), "%s.generation.composition.%s" % (label, key)
        )
        upper = 1.0 if key == "elevation_bias" else 0.5
        if not 0.0 <= value <= upper:
            raise ZoneCompileError(
                "%s.generation.composition.%s must be in [0, %.1f]"
                % (label, key, upper)
            )
    art_contracts = {
        "silhouette": {"continuous_boundary_wall", "broken_ridge_cluster"},
        "massing": {"terrain_primary", "terrain_and_sparse_props"},
        "surface": {"fractured_granite", "weathered_highland_rock"},
        "dressing": {"none", "sparse", "moderate"},
    }
    for key, choices in art_contracts.items():
        value = composition.get(key)
        if value not in choices:
            raise ZoneCompileError(
                "%s.generation.composition.%s must be one of %s"
                % (label, key, sorted(choices))
            )


def compile_annotations(
    annotations: dict[str, Any], *, minimum_confidence: float = 0.75, source_root: Path | None = None
) -> CompileResult:
    """Compile a reviewed annotation document into the canonical ZoneSpec.

    Features below ``minimum_confidence`` are allowed only when a human explicitly
    marked them ``reviewed``. This lets the pipeline preserve uncertain model output
    as evidence without silently promoting guesses into collision or navigation.
    """
    (
        width,
        length,
        asset_profiles,
        terrain_materials,
        terrain_material_scale_m,
        acceptance_policy,
        traversal_policy,
        image_reconciliation,
        derivation,
    ) = _validate_annotation_header(annotations, source_root)
    source_policies = {
        str(policy["image_id"]): policy
        for policy in image_reconciliation["source_policies"]
    }
    canonical_map_image_id = str(
        image_reconciliation["canonical_map_image_id"]
    )
    multi_image = len(source_policies) > 1
    features = annotations.get("features")
    if not isinstance(features, list) or not features:
        raise ZoneCompileError("features must be a non-empty array")

    seen_ids: set[str] = set()
    compiled: list[dict[str, Any]] = []
    warnings: list[str] = []
    semantic_counts: dict[str, int] = {}
    source_image_usage = {source_id: 0 for source_id in source_policies}
    evidence_claim_counts = {claim: 0 for claim in sorted(EVIDENCE_CLAIMS)}
    multi_view_feature_count = 0

    for index, feature in enumerate(features):
        label = "features[%d]" % index
        if not isinstance(feature, dict):
            raise ZoneCompileError("%s must be an object" % label)
        feature_id = feature.get("id")
        if not isinstance(feature_id, str) or not feature_id:
            raise ZoneCompileError("%s.id must be a non-empty string" % label)
        if feature_id in seen_ids:
            raise ZoneCompileError("Duplicate feature id: %s" % feature_id)
        seen_ids.add(feature_id)

        category = feature.get("category")
        semantic = feature.get("semantic")
        if not isinstance(category, str) or not isinstance(semantic, str):
            raise ZoneCompileError("%s requires string category and semantic" % label)
        if category == "landform" and semantic not in LAND_FORM_SEMANTICS:
            raise ZoneCompileError("%s uses unsupported landform semantic %r" % (label, semantic))

        confidence = _as_number(feature.get("confidence", 0.0), label + ".confidence")
        if not 0.0 <= confidence <= 1.0:
            raise ZoneCompileError("%s.confidence must be between 0 and 1" % label)
        review_state = feature.get("review_state", "proposed")
        if review_state not in {"proposed", "reviewed"}:
            raise ZoneCompileError("%s.review_state must be proposed or reviewed" % label)
        if confidence < minimum_confidence and review_state != "reviewed":
            raise ZoneCompileError(
                "%s is below confidence %.2f and has not been reviewed" % (feature_id, minimum_confidence)
            )
        if confidence < minimum_confidence:
            warnings.append("Reviewed low-confidence feature retained: %s" % feature_id)

        evidence = _validate_evidence(
            feature.get("evidence"),
            label,
            source_policies,
            multi_image,
            review_state,
        )
        evidence_image_ids: set[str] = set()
        for evidence_entry in evidence:
            evidence_image_id = str(evidence_entry["image_id"])
            source_image_usage[evidence_image_id] += 1
            evidence_claim_counts[str(evidence_entry["claim"])] += 1
            evidence_image_ids.add(evidence_image_id)
        if len(evidence_image_ids) > 1:
            multi_view_feature_count += 1

        geometry = feature.get("geometry")
        if not isinstance(geometry, dict):
            raise ZoneCompileError("%s.geometry must be an object" % label)
        world_geometry = _geometry_to_world(
            geometry,
            width,
            length,
            label,
            canonical_map_image_id,
            multi_image,
        )

        generation = feature.get("generation", {})
        if not isinstance(generation, dict):
            raise ZoneCompileError("%s.generation must be an object" % label)
        if category == "landform":
            _validate_landform_generation(feature, label)
        if category == "landmark" and semantic == "faction_keep":
            properties = feature.get("properties", {})
            if not isinstance(properties, dict):
                raise ZoneCompileError("%s.properties must be an object" % label)
            realm = properties.get("realm")
            team = properties.get("team")
            if realm not in SUPPORTED_REALMS:
                raise ZoneCompileError("%s faction_keep.realm must be one of %s" % (label, sorted(SUPPORTED_REALMS)))
            if not isinstance(team, str) or not team:
                raise ZoneCompileError("%s faction_keep.team must be a non-empty string" % label)
        asset_profile = _asset_profile_reference(feature)
        if asset_profile is not None and asset_profile not in asset_profiles:
            raise ZoneCompileError("%s references unknown asset profile %s" % (feature_id, asset_profile))

        output = {
            "id": feature_id,
            "category": category,
            "semantic": semantic,
            "geometry": world_geometry,
            "generation": generation,
            "properties": feature.get("properties", {}),
            "evidence": evidence,
            "confidence": confidence,
            "review_state": review_state,
        }
        compiled.append(output)
        semantic_counts[semantic] = semantic_counts.get(semantic, 0) + 1

    if image_reconciliation["cross_view_policy"][
        "every_source_image_requires_evidence"
    ]:
        unused_source_images = sorted(
            image_id
            for image_id, use_count in source_image_usage.items()
            if use_count <= 0
        )
        if unused_source_images:
            raise ZoneCompileError(
                "Multi-image annotations did not reconcile source image(s): %s"
                % ", ".join(unused_source_images)
            )

    _resolve_stream_landmark_clearance(
        compiled,
        width,
        length,
        float(traversal_policy.get("agent_radius_m", 2.5)),
    )
    derived_crossings = _derive_lane_water_crossings(
        compiled, asset_profiles, width, length,
        float(traversal_policy.get("agent_radius_m", 2.5)),
    )
    for crossing in derived_crossings:
        if crossing["id"] in seen_ids:
            raise ZoneCompileError("Derived crossing id collides with feature: %s" % crossing["id"])
        seen_ids.add(crossing["id"])
        compiled.append(crossing)
        semantic_counts["bridge"] = semantic_counts.get("bridge", 0) + 1

    zone = annotations.get("zone", {})
    if not isinstance(zone, dict) or not isinstance(zone.get("id"), str) or not zone["id"]:
        raise ZoneCompileError("zone.id is required")
    zone_spec = {
        "schema_version": ZONE_SPEC_VERSION,
        "zone": {
            "id": zone["id"],
            "name": zone.get("name", zone["id"]),
            "world_bounds": {"width": width, "length": length, "units": "meters"},
            "coordinate_system": "right-handed-xz-up-y",
            "source_images": annotations["source_images"],
            "image_reconciliation": image_reconciliation,
        },
        "generation_seed": int(annotations.get("generation_seed", 1)),
        "asset_profiles": asset_profiles,
        "terrain_materials": terrain_materials,
        "terrain_material_scale_m": terrain_material_scale_m,
        "acceptance_policy": acceptance_policy,
        "traversal_policy": traversal_policy,
        "features": compiled,
    }
    if derivation:
        zone_spec["derivation"] = derivation
    report = {
        "schema_version": "codeweald.zone-validation-report/v1",
        "zone_id": zone["id"],
        "status": "warnings" if warnings else "passed",
        "feature_count": len(compiled),
        "semantic_counts": semantic_counts,
        "source_image_usage": source_image_usage,
        "evidence_claim_counts": {
            claim: count
            for claim, count in evidence_claim_counts.items()
            if count > 0
        },
        "multi_view_feature_count": multi_view_feature_count,
        "warnings": warnings,
    }
    return CompileResult(zone_spec=zone_spec, report=report)


def compile_file(input_path: Path, output_path: Path, report_path: Path, *, minimum_confidence: float = 0.75) -> CompileResult:
    result = compile_annotations(
        _read_json(input_path), minimum_confidence=minimum_confidence, source_root=input_path.parent
    )
    _write_json(output_path, result.zone_spec)
    _write_json(report_path, result.report)
    return result


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Compile Codeweald concept annotations into a portable ZoneSpec")
    parser.add_argument("annotations", type=Path, help="reviewed codeweald.concept-annotations/v1 JSON")
    parser.add_argument("--output", required=True, type=Path, help="output ZoneSpec JSON")
    parser.add_argument("--report", required=True, type=Path, help="output validation report JSON")
    parser.add_argument("--minimum-confidence", type=float, default=0.75)
    args = parser.parse_args(argv)
    try:
        result = compile_file(args.annotations, args.output, args.report, minimum_confidence=args.minimum_confidence)
    except ZoneCompileError as exc:
        parser.error(str(exc))
    print("Compiled %s: %d semantic features (%s)" % (result.report["zone_id"], result.report["feature_count"], result.report["status"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
