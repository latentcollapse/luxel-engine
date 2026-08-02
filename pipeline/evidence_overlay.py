#!/usr/bin/env python3
"""Render a concept-evidence contact sheet from a compiled ZoneSpec.

The sheet is a review aid, not a new authority.  It makes a model's semantic
claims inspectable against the exact hashed source images before any terrain or
asset output is trusted.
"""

from __future__ import annotations

import argparse
import colorsys
import hashlib
import json
from pathlib import Path
from typing import Any, Iterable

from PIL import Image, ImageDraw, ImageFont

from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError


OVERLAY_VERSION = "codeweald.concept-evidence-overlay/v1"


def _color(identifier: str) -> tuple[int, int, int]:
    hue = int(hashlib.sha256(identifier.encode("utf-8")).hexdigest()[:8], 16) / 0xFFFFFFFF
    red, green, blue = colorsys.hsv_to_rgb(hue, 0.78, 1.0)
    return round(red * 255), round(green * 255), round(blue * 255)


def render_overlay(zone_spec: dict[str, Any], source_root: Path, output: Path) -> dict[str, Any]:
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        raise ZoneCompileError("Expected %s" % ZONE_SPEC_VERSION)
    zone = zone_spec.get("zone", {})
    sources = zone.get("source_images", [])
    if not isinstance(sources, list) or not sources:
        raise ZoneCompileError("ZoneSpec has no source images")
    features = [feature for feature in zone_spec.get("features", []) if isinstance(feature, dict)]
    source_by_id = {source.get("id"): source for source in sources if isinstance(source, dict)}
    panels: list[Image.Image] = []
    evidence_count = 0
    source_evidence_counts: dict[str, int] = {}
    claim_counts: dict[str, int] = {}
    font = ImageFont.load_default()
    reconciliation = zone.get("image_reconciliation", {})
    canonical_map_image_id = reconciliation.get("canonical_map_image_id")
    for source_id, source in source_by_id.items():
        source_path = source_root / str(source.get("path", ""))
        try:
            image = Image.open(source_path).convert("RGB")
        except OSError as exc:
            raise ZoneCompileError("Cannot render source evidence %s: %s" % (source_path, exc)) from exc
        scale = min(1.0, 960.0 / image.width)
        content = image.resize((round(image.width * scale), round(image.height * scale)), Image.Resampling.LANCZOS) if scale < 1.0 else image.copy()
        header_height = 26
        panel = Image.new("RGB", (content.width, content.height + header_height), (18, 20, 24))
        panel.paste(content, (0, header_height))
        draw = ImageDraw.Draw(panel)
        canonical_label = " canonical map" if source_id == canonical_map_image_id else ""
        draw.text(
            (8, 7),
            "%s | %s%s" % (source_id, source.get("role", "reference"), canonical_label),
            fill=(230, 234, 240),
            font=font,
        )
        source_evidence_counts[str(source_id)] = 0
        for feature in features:
            for evidence in feature.get("evidence", []):
                if not isinstance(evidence, dict) or evidence.get("image_id") != source_id:
                    continue
                left, top, right, bottom = evidence["region"]
                rectangle = (
                    round(float(left) * content.width),
                    header_height + round(float(top) * content.height),
                    round(float(right) * content.width),
                    header_height + round(float(bottom) * content.height),
                )
                color = _color(str(feature.get("id", "feature")))
                draw.rectangle(rectangle, outline=color, width=max(2, round(3 * scale)))
                claim = str(evidence.get("claim", "unspecified"))
                visibility = str(evidence.get("visibility", "direct"))
                label = "%s [%s/%s]" % (
                    feature.get("id", "feature"),
                    claim,
                    visibility,
                )
                label_position = (rectangle[0] + 3, max(0, rectangle[1] - 12))
                draw.text(label_position, label, fill=color, stroke_width=1, stroke_fill=(0, 0, 0), font=font)
                evidence_count += 1
                source_evidence_counts[str(source_id)] += 1
                claim_counts[claim] = claim_counts.get(claim, 0) + 1
        panels.append(panel)
    width = max(panel.width for panel in panels)
    height = sum(panel.height for panel in panels) + 12 * max(0, len(panels) - 1)
    sheet = Image.new("RGB", (width, height), (18, 20, 24))
    cursor = 0
    for panel in panels:
        sheet.paste(panel, (0, cursor))
        cursor += panel.height + 12
    output.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(output)
    return {
        "schema_version": OVERLAY_VERSION,
        "zone_id": zone.get("id"),
        "output": output.name,
        "source_image_count": len(panels),
        "feature_count": len(features),
        "evidence_count": evidence_count,
        "canonical_map_image_id": canonical_map_image_id,
        "source_evidence_counts": source_evidence_counts,
        "claim_counts": claim_counts,
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Render Codeweald feature-evidence overlays for a concept batch")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("--source-root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        zone_spec = json.loads(args.zone_spec.read_text(encoding="utf-8"))
        report = render_overlay(zone_spec, args.source_root, args.output)
    except (OSError, json.JSONDecodeError, ZoneCompileError) as exc:
        parser.error(str(exc))
    print("Rendered %d evidence regions at %s" % (report["evidence_count"], args.output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
