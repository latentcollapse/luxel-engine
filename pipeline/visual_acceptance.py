#!/usr/bin/env python3
"""Rendered-image acceptance for a Codeweald ZoneSpec build.

This is deliberately a conservative *integrity* gate rather than a claim that
pixel similarity proves an artistic match.  It catches the failure modes a
semantic JSON report cannot see: black/unbound materials, empty renders,
clipped asset imports, missing readable lanes, feature regions with no visual
structure, and a forest profile that did not render as foliage.

The report remains evidence for an artist/model review.  Its stable numerical
signals give the autonomous pipeline a way to fail closed before replacing an
active engine scene with visibly broken output.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable

import numpy as np
from PIL import Image, ImageDraw

from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError


VISUAL_ACCEPTANCE_VERSION = "codeweald.visual-acceptance/v1"


def _read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ZoneCompileError("Cannot read %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise ZoneCompileError("%s must contain an object" % path)
    return value


def _image(path: Path) -> np.ndarray:
    try:
        with Image.open(path) as source:
            return np.asarray(source.convert("RGB"), dtype=np.float32) / 255.0
    except OSError as exc:
        raise ZoneCompileError("Cannot read rendered image %s: %s" % (path, exc)) from exc


def _luminance(rgb: np.ndarray) -> np.ndarray:
    return rgb[:, :, 0] * 0.2126 + rgb[:, :, 1] * 0.7152 + rgb[:, :, 2] * 0.0722


def _edge_density(luma: np.ndarray) -> float:
    gradient_y, gradient_x = np.gradient(luma)
    magnitude = np.hypot(gradient_x, gradient_y)
    return float((magnitude >= 0.055).mean())


def _mean_saturation(rgb: np.ndarray) -> float:
    maximum = rgb.max(axis=2)
    minimum = rgb.min(axis=2)
    return float(
        np.divide(
            maximum - minimum,
            maximum,
            out=np.zeros_like(maximum),
            where=maximum > 1e-6,
        ).mean()
    )


def _source_relative_color(
    source_region: np.ndarray, render_region: np.ndarray
) -> dict[str, Any]:
    source_mean = source_region.reshape(-1, 3).mean(axis=0)
    render_mean = render_region.reshape(-1, 3).mean(axis=0)
    source_luminance = float(np.dot(source_mean, [0.2126, 0.7152, 0.0722]))
    render_luminance = float(np.dot(render_mean, [0.2126, 0.7152, 0.0722]))
    source_saturation = _mean_saturation(source_region)
    render_saturation = _mean_saturation(render_region)
    color_distance = float(np.linalg.norm(source_mean - render_mean))
    luminance_ratio = render_luminance / max(source_luminance, 1e-6)
    saturation_ratio = render_saturation / max(source_saturation, 1e-6)
    accepted = (
        color_distance <= 0.14
        and 0.60 <= luminance_ratio <= 1.65
        and 0.55 <= saturation_ratio <= 1.50
    )
    return {
        "source_mean_rgb": [round(float(value), 5) for value in source_mean],
        "render_mean_rgb": [round(float(value), 5) for value in render_mean],
        "source_relative_rgb_distance": round(color_distance, 5),
        "source_relative_luminance_ratio": round(luminance_ratio, 5),
        "source_relative_saturation_ratio": round(saturation_ratio, 5),
        "source_relative_palette_status": "accepted" if accepted else "mismatch",
    }


def _region_from_points(
    points: list[Any], width: int, height: int, world_width: float, world_length: float
) -> tuple[slice, slice] | None:
    if not points:
        return None
    coordinates = [(float(point[0]), float(point[1])) for point in points if isinstance(point, list) and len(point) >= 2]
    if not coordinates:
        return None
    # ZoneSpec is centered X/Z; render capture is the same top-down map order.
    xs = [int(np.clip((x + world_width * 0.5) / world_width * width, 0, width - 1)) for x, _ in coordinates]
    ys = [int(np.clip((world_length * 0.5 - z) / world_length * height, 0, height - 1)) for _, z in coordinates]
    pad_x, pad_y = max(12, width // 80), max(12, height // 80)
    return (
        slice(max(0, min(ys) - pad_y), min(height, max(ys) + pad_y + 1)),
        slice(max(0, min(xs) - pad_x), min(width, max(xs) + pad_x + 1)),
    )


def _region_from_normalized_points(
    points: list[Any], width: int, height: int, padding: int = 10
) -> tuple[slice, slice] | None:
    coordinates = [
        (float(point[0]), float(point[1]))
        for point in points
        if isinstance(point, list) and len(point) == 2
    ]
    if not coordinates:
        return None
    xs = [int(np.clip(x * width, 0, width - 1)) for x, _ in coordinates]
    ys = [int(np.clip(y * height, 0, height - 1)) for _, y in coordinates]
    return (
        slice(max(0, min(ys) - padding), min(height, max(ys) + padding + 1)),
        slice(max(0, min(xs) - padding), min(width, max(xs) + padding + 1)),
    )


def _evidence_region(
    source: np.ndarray,
    feature: dict[str, Any],
    source_image_id: str | None = None,
) -> np.ndarray | None:
    evidence = feature.get("evidence", [])
    selected = next(
        (
            entry
            for entry in evidence
            if isinstance(entry, dict)
            and (
                source_image_id is None
                or entry.get("image_id") == source_image_id
            )
        ),
        None,
    )
    region = selected.get("region", []) if isinstance(selected, dict) else []
    if len(region) != 4:
        return None
    height, width = source.shape[:2]
    left, top, right, bottom = (float(value) for value in region)
    x0, x1 = int(np.clip(left * width, 0, width - 1)), int(
        np.clip(right * width, 1, width)
    )
    y0, y1 = int(np.clip(top * height, 0, height - 1)), int(
        np.clip(bottom * height, 1, height)
    )
    if x1 <= x0 or y1 <= y0:
        return None
    return source[y0:y1, x0:x1]


def _polygon_mask(
    shape: tuple[int, int], normalized_points: list[Any]
) -> np.ndarray | None:
    height, width = shape
    points = [
        (
            round(float(point[0]) * width),
            round(float(point[1]) * height),
        )
        for point in normalized_points
        if isinstance(point, list) and len(point) == 2
    ]
    if len(points) < 3:
        return None
    mask_image = Image.new("L", (width, height), 0)
    ImageDraw.Draw(mask_image).polygon(points, fill=255)
    mask = np.asarray(mask_image, dtype=np.uint8) > 0
    return mask if int(mask.sum()) >= 32 else None


def _foliage_fraction(rgb: np.ndarray) -> float:
    if rgb.size == 0:
        return 0.0
    pixels = rgb.reshape((-1, 3))
    green = (
        (pixels[:, 1] > pixels[:, 0] * 1.03)
        & (pixels[:, 1] > pixels[:, 2] * 1.03)
        & (pixels[:, 1] > 0.06)
    )
    return float(green.mean())


def _projected_corridor_visuals(
    rendered: np.ndarray, projection: dict[str, Any]
) -> dict[str, Any] | None:
    screen_points = projection.get("screen_normalized", [])
    half_widths = projection.get("half_width_pixels", [])
    if len(screen_points) < 2 or len(half_widths) != len(screen_points):
        return None
    height, width = rendered.shape[:2]
    points = np.asarray(
        [[float(point[0]) * width, float(point[1]) * height] for point in screen_points],
        dtype=np.float32,
    )
    radius = float(np.median(np.asarray(half_widths, dtype=np.float32)))
    if not np.isfinite(radius) or radius < 1.0:
        return None
    # Sample inside the authored ribbon and compare against a shoulder ring
    # outside it. This follows the real Camera3D projection rather than assuming
    # a top-down map fills the entire screenshot.
    line_points = [(round(float(point[0])), round(float(point[1]))) for point in points]
    corridor_image = Image.new("L", (width, height), 0)
    ImageDraw.Draw(corridor_image).line(
        line_points,
        fill=255,
        width=max(2, round(radius * 2.0 * 0.68)),
        joint="curve",
    )
    outer_image = Image.new("L", (width, height), 0)
    ImageDraw.Draw(outer_image).line(
        line_points,
        fill=255,
        width=max(3, round(radius * 2.0 * 2.80)),
        joint="curve",
    )
    inner_image = Image.new("L", (width, height), 0)
    ImageDraw.Draw(inner_image).line(
        line_points,
        fill=255,
        width=max(2, round(radius * 2.0 * 1.55)),
        joint="curve",
    )
    corridor_mask = np.asarray(corridor_image, dtype=np.uint8) > 0
    shoulder_mask = (
        (np.asarray(outer_image, dtype=np.uint8) > 0)
        & (np.asarray(inner_image, dtype=np.uint8) == 0)
    )
    if corridor_mask.sum() < 32 or shoulder_mask.sum() < 32:
        return None
    corridor_pixels = rendered[corridor_mask]
    shoulder_pixels = rendered[shoulder_mask]
    corridor_mean = corridor_pixels.mean(axis=0)
    shoulder_mean = shoulder_pixels.mean(axis=0)
    corridor_luminance = float(np.dot(corridor_mean, [0.2126, 0.7152, 0.0722]))
    shoulder_luminance = float(np.dot(shoulder_mean, [0.2126, 0.7152, 0.0722]))
    warm = (
        (corridor_pixels[:, 0] > corridor_pixels[:, 2] * 1.12)
        & (corridor_pixels[:, 0] > 0.10)
    )
    return {
        "projection_sample_count": len(screen_points),
        "corridor_pixel_count": int(corridor_mask.sum()),
        "median_half_width_pixels": round(radius, 4),
        "corridor_mean_rgb": [round(float(value), 5) for value in corridor_mean],
        "shoulder_mean_rgb": [round(float(value), 5) for value in shoulder_mean],
        "corridor_color_contrast": round(
            float(np.linalg.norm(corridor_mean - shoulder_mean)), 5
        ),
        "corridor_luminance_contrast": round(
            abs(corridor_luminance - shoulder_luminance), 5
        ),
        "warm_corridor_fraction": round(float(warm.mean()), 5),
    }


def _feature_visuals(
    zone_spec: dict[str, Any],
    source: np.ndarray,
    rendered: np.ndarray,
    projection_report: dict[str, Any] | None = None,
) -> tuple[list[str], dict[str, Any]]:
    failures: list[str] = []
    observations: dict[str, Any] = {}
    height, width = rendered.shape[:2]
    bounds = zone_spec.get("zone", {}).get("world_bounds", {})
    world_width, world_length = float(bounds.get("width", 0.0)), float(bounds.get("length", 0.0))
    if world_width <= 0.0 or world_length <= 0.0:
        return ["ZoneSpec has invalid world bounds for visual review"], observations
    luma = _luminance(rendered)
    projected_corridors = {
        entry.get("feature_id"): entry
        for entry in (projection_report or {}).get("corridors", [])
        if isinstance(entry, dict) and isinstance(entry.get("feature_id"), str)
    }
    projected_landforms = {
        entry.get("feature_id"): entry
        for entry in (projection_report or {}).get("landforms", [])
        if isinstance(entry, dict) and isinstance(entry.get("feature_id"), str)
    }
    projected_landmarks = {
        entry.get("feature_id"): entry
        for entry in (projection_report or {}).get("landmarks", [])
        if isinstance(entry, dict) and isinstance(entry.get("feature_id"), str)
    }
    projected_biomes = {
        entry.get("feature_id"): entry
        for entry in (projection_report or {}).get("biomes", [])
        if isinstance(entry, dict) and isinstance(entry.get("feature_id"), str)
    }
    canonical_source_image_id = (
        zone_spec.get("zone", {})
        .get("image_reconciliation", {})
        .get("canonical_map_image_id")
    )
    acceptance_policy = zone_spec.get("acceptance_policy", {})
    landmark_fill_range = acceptance_policy.get(
        "landmark_evidence_fill_ratio", [0.002, 2.5]
    )
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict):
            continue
        feature_id = str(feature.get("id", ""))
        category, semantic = feature.get("category"), feature.get("semantic")
        landform_projection = projected_landforms.get(feature_id)
        landmark_projection = projected_landmarks.get(feature_id)
        region = None
        if category == "landform" and landform_projection is not None:
            region = _region_from_normalized_points(
                landform_projection.get("screen_samples", []), width, height
            )
        elif category == "landmark" and landmark_projection is not None:
            screen_point = landmark_projection.get("screen_normalized", [])
            landmark_padding = (
                max(40, min(width, height) // 15)
                if semantic == "faction_keep"
                else max(24, min(width, height) // 25)
            )
            region = _region_from_normalized_points(
                [screen_point], width, height, padding=landmark_padding
            )
        if region is None:
            region = _region_from_points(feature.get("geometry", {}).get("points", []), width, height, world_width, world_length)
        if region is None:
            continue
        pixels = rendered[region]
        local_luma = luma[region]
        if pixels.size == 0:
            failures.append("Render has no samples for feature %s" % feature.get("id", "<unknown>"))
            continue
        item = {
            "mean_luminance": round(float(local_luma.mean()), 5),
            "luminance_stddev": round(float(local_luma.std()), 5),
            "edge_density": round(_edge_density(local_luma), 5),
        }
        source_region = _evidence_region(
            source, feature, canonical_source_image_id
        )
        if source_region is not None and source_region.size:
            item.update(_source_relative_color(source_region, pixels))
        if category == "landform":
            if item["luminance_stddev"] < 0.008 or item["edge_density"] < 0.003:
                failures.append("Rendered landform %s has no readable relief" % feature.get("id", "<unknown>"))
            profile = feature.get("generation", {}).get("profile")
            if profile == "alpine_jagged_massif" and landform_projection is not None:
                if source_region is not None and source_region.size:
                    source_luma = _luminance(source_region)
                    source_edges = _edge_density(source_luma)
                    source_contrast = float(source_luma.std())
                    item["source_edge_density"] = round(source_edges, 5)
                    item["source_luminance_stddev"] = round(source_contrast, 5)
                    item["silhouette_edge_ratio"] = round(
                        item["edge_density"] / max(source_edges, 1e-6), 5
                    )
                    item["silhouette_contrast_ratio"] = round(
                        item["luminance_stddev"] / max(source_contrast, 1e-6), 5
                    )
                    item["maximum_projected_relief_pixels"] = round(
                        float(
                            landform_projection.get(
                                "maximum_projected_relief_pixels", 0.0
                            )
                        ),
                        4,
                    )
                    if (
                        item["maximum_projected_relief_pixels"] < 39.0
                        or item["silhouette_contrast_ratio"] < 0.42
                    ):
                        # Edge density inside the projected landform rectangle
                        # is retained as diagnostic evidence, but is not a hard
                        # silhouette gate: foreground foliage legitimately
                        # changes that value as runtime LOD tiers switch. The
                        # projection-derived relief and source-relative
                        # contrast remain terrain-specific hard requirements.
                        failures.append(
                            "Rendered Alpine landform %s lacks source-relative silhouette structure"
                            % feature.get("id", "<unknown>")
                        )
        elif category == "biome" and semantic == "forest":
            biome_projection = projected_biomes.get(feature_id, {})
            biome_mask = _polygon_mask(
                (height, width),
                biome_projection.get("screen_normalized", []),
            )
            biome_pixels = (
                rendered[biome_mask]
                if biome_mask is not None
                else pixels.reshape((-1, 3))
            )
            render_coverage = _foliage_fraction(biome_pixels)
            source_coverage = (
                _foliage_fraction(source_region)
                if source_region is not None and source_region.size
                else 0.0
            )
            coverage_ratio = render_coverage / max(source_coverage, 0.01)
            item["projected_biome_pixel_count"] = (
                int(biome_mask.sum()) if biome_mask is not None else 0
            )
            item["source_foliage_coverage"] = round(source_coverage, 5)
            item["render_foliage_coverage"] = round(render_coverage, 5)
            item["biome_coverage_ratio"] = round(coverage_ratio, 5)
            item["green_foliage_fraction"] = round(render_coverage, 5)
            if (
                render_coverage < 0.003
                or coverage_ratio
                < float(
                    acceptance_policy.get(
                        "minimum_biome_coverage_ratio", 0.20
                    )
                )
            ):
                failures.append("Rendered forest %s lacks visible foliage" % feature.get("id", "<unknown>"))
        elif category == "landmark" and projection_report is not None:
            evidence_bounds = landmark_projection.get(
                "evidence_screen_bounds_normalized", []
            ) if landmark_projection is not None else []
            evidence_region = landmark_projection.get(
                "source_evidence_region", []
            ) if landmark_projection is not None else []
            if len(evidence_bounds) == 4 and len(evidence_region) == 4:
                projected_area = max(
                    0.0,
                    float(evidence_bounds[2]) - float(evidence_bounds[0]),
                ) * max(
                    0.0,
                    float(evidence_bounds[3]) - float(evidence_bounds[1]),
                )
                source_area = max(
                    0.0,
                    float(evidence_region[2]) - float(evidence_region[0]),
                ) * max(
                    0.0,
                    float(evidence_region[3]) - float(evidence_region[1]),
                )
                fill_ratio = projected_area / max(source_area, 1e-8)
                item["projected_landmark_area_normalized"] = round(
                    projected_area, 8
                )
                item["source_evidence_area_normalized"] = round(source_area, 8)
                item["landmark_evidence_fill_ratio"] = round(fill_ratio, 5)
                if not (
                    float(landmark_fill_range[0])
                    <= fill_ratio
                    <= float(landmark_fill_range[1])
                ):
                    failures.append(
                        "Rendered landmark %s has implausible source-relative scale"
                        % feature.get("id", "<unknown>")
                    )
            else:
                failures.append(
                    "Rendered landmark %s has no projected mesh bounds"
                    % feature.get("id", "<unknown>")
                )
        elif category == "corridor" and semantic == "lane":
            projected = _projected_corridor_visuals(
                rendered, projected_corridors.get(str(feature.get("id", "")), {})
            )
            if projected is not None:
                item.update(projected)
                readable = (
                    item["corridor_color_contrast"] >= 0.018
                    or item["corridor_luminance_contrast"] >= 0.010
                    or item["warm_corridor_fraction"] >= 0.08
                )
            else:
                # Backward-compatible diagnostic for a render captured without
                # runtime projection evidence. Full autonomous builds always
                # use the camera-projected path above.
                warm = (pixels[:, :, 0] > pixels[:, :, 2] * 1.12) & (pixels[:, :, 0] > 0.10)
                item["warm_corridor_fraction"] = round(float(warm.mean()), 5)
                readable = item["warm_corridor_fraction"] >= 0.002
            if not readable:
                failures.append("Rendered lane %s is not visually readable" % feature.get("id", "<unknown>"))
        observations[str(feature.get("id", "unknown"))] = item
    return failures, observations


def evaluate_visual(
    zone_spec: dict[str, Any],
    source: np.ndarray,
    rendered: np.ndarray,
    projection_report: dict[str, Any] | None = None,
) -> dict[str, Any]:
    failures: list[str] = []
    warnings: list[str] = []
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        failures.append("ZoneSpec schema is not supported")
    if min(rendered.shape[:2]) < 512:
        failures.append("Rendered preview is too small for visual review")
    render_luma = _luminance(rendered)
    source_luma = _luminance(source)
    source_mean_rgb = source.reshape(-1, 3).mean(axis=0)
    render_mean_rgb = rendered.reshape(-1, 3).mean(axis=0)
    metrics = {
        "source_size": {"width": int(source.shape[1]), "height": int(source.shape[0])},
        "render_size": {"width": int(rendered.shape[1]), "height": int(rendered.shape[0])},
        "render_mean_luminance": round(float(render_luma.mean()), 5),
        "render_luminance_stddev": round(float(render_luma.std()), 5),
        "render_edge_density": round(_edge_density(render_luma), 5),
        "render_clipped_fraction": round(float((rendered.max(axis=2) >= 0.995).mean()), 5),
        "source_mean_luminance": round(float(source_luma.mean()), 5),
        "source_mean_rgb": [round(float(value), 5) for value in source_mean_rgb],
        "render_mean_rgb": [round(float(value), 5) for value in render_mean_rgb],
        "global_mean_rgb_distance": round(
            float(np.linalg.norm(source_mean_rgb - render_mean_rgb)), 5
        ),
    }
    if metrics["render_mean_luminance"] < 0.012:
        failures.append("Rendered preview is effectively black")
    if metrics["render_luminance_stddev"] < 0.01:
        failures.append("Rendered preview lacks visual contrast")
    if metrics["render_edge_density"] < 0.002:
        failures.append("Rendered preview lacks structural detail")
    if metrics["render_clipped_fraction"] > 0.22:
        failures.append("Rendered preview has excessive clipped highlights")
    source_ratio = max(metrics["source_mean_luminance"], 1e-4)
    if metrics["render_mean_luminance"] / source_ratio < 0.14:
        warnings.append("Render is much darker than its concept reference")
    feature_failures, feature_observations = _feature_visuals(
        zone_spec, source, rendered, projection_report
    )
    failures.extend(feature_failures)
    palette_mismatches = [
        feature_id
        for feature_id, item in feature_observations.items()
        if item.get("source_relative_palette_status") == "mismatch"
    ]
    metrics["regional_palette_mismatch_count"] = len(palette_mismatches)
    metrics["regional_palette_sample_count"] = sum(
        "source_relative_palette_status" in item
        for item in feature_observations.values()
    )
    if palette_mismatches:
        warnings.append(
            "Source-relative palette mismatch in feature regions: "
            + ", ".join(palette_mismatches)
        )
    palette_sample_count = int(metrics["regional_palette_sample_count"])
    palette_mismatch_fraction = (
        len(palette_mismatches) / palette_sample_count
        if palette_sample_count > 0
        else 0.0
    )
    metrics["regional_palette_mismatch_fraction"] = round(
        palette_mismatch_fraction, 5
    )
    maximum_palette_mismatch = float(
        zone_spec.get("acceptance_policy", {}).get(
            "maximum_regional_palette_mismatch_fraction", 0.25
        )
    )
    metrics["maximum_regional_palette_mismatch_fraction"] = (
        maximum_palette_mismatch
    )
    if palette_mismatch_fraction > maximum_palette_mismatch:
        failures.append(
            "Regional palette mismatch fraction %.3f exceeds %.3f"
            % (palette_mismatch_fraction, maximum_palette_mismatch)
        )
    return {
        "schema_version": VISUAL_ACCEPTANCE_VERSION,
        "zone_id": zone_spec.get("zone", {}).get("id", "unknown"),
        "status": "failed" if failures else ("warnings" if warnings else "passed"),
        "failures": failures,
        "warnings": warnings,
        "metrics": metrics,
        "feature_observations": feature_observations,
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Validate rendered visibility of a Codeweald zone")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("source_image", type=Path)
    parser.add_argument("rendered_image", type=Path)
    parser.add_argument("--projection-report", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        report = evaluate_visual(
            _read_json(args.zone_spec),
            _image(args.source_image),
            _image(args.rendered_image),
            _read_json(args.projection_report) if args.projection_report else None,
        )
    except ZoneCompileError as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Visual acceptance %s: %s" % (report["zone_id"], report["status"]))
    return 1 if report["status"] == "failed" else 0


if __name__ == "__main__":
    raise SystemExit(main())
