#!/usr/bin/env python3
"""Provider-neutral contract for vision models proposing Codeweald annotations.

This is intentionally not an LLM wrapper. A model/provider receives a bounded
packet containing the exact source image identities, brief, and response schema;
it returns only semantic content. The adapter restores immutable batch metadata,
compiles the result immediately, and records diagnostics on failure.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable

from evidence_overlay import render_overlay
from zone_compiler import ANNOTATION_VERSION, ZoneCompileError, compile_annotations


VISION_RESPONSE_VERSION = "codeweald.vision-annotation-response/v1"
PACKET_VERSION = "codeweald.vision-annotation-packet/v1"
_ALLOWED_RESPONSE_KEYS = {
    "schema_version",
    "zone_name",
    "asset_profiles",
    "terrain_materials",
    "terrain_material_scale_m",
    "features",
}


def _read(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ZoneCompileError("Cannot read %s: %s" % (path, exc)) from exc
    if not isinstance(value, dict):
        raise ZoneCompileError("%s must contain one object" % path)
    return value


def _write(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def response_schema() -> dict[str, Any]:
    """Small JSON-shaped schema deliberately readable by models and validators."""
    return {
        "schema_version": VISION_RESPONSE_VERSION,
        "type": "object",
        "additional_properties": False,
        "required": ["schema_version", "features"],
        "properties": {
            "schema_version": {"const": VISION_RESPONSE_VERSION},
            "zone_name": {"type": "string", "description": "optional display name only"},
            "asset_profiles": {"type": "object", "description": "optional ecological/landform asset profiles"},
            "terrain_materials": {"type": "object", "description": "optional reviewed material ids by layer"},
            "terrain_material_scale_m": {
                "type": "object",
                "description": "optional world-space repeat size in meters by terrain layer",
            },
            "features": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "required": ["id", "category", "semantic", "geometry", "evidence", "confidence", "review_state"],
                    "description": "Codeweald feature. Geometry uses only the canonical map source_image_id. Evidence may cite any source but must obey that image's allowed claims.",
                    "properties": {
                        "geometry": {
                            "type": "object",
                            "required": ["type", "points", "source_image_id"],
                            "description": "Normalized geometry on the canonical map, never on a perspective/detail image.",
                        },
                        "evidence": {
                            "type": "array",
                            "minItems": 1,
                            "items": {
                                "type": "object",
                                "required": ["image_id", "region", "note"],
                                "properties": {
                                    "claim": {
                                        "enum": [
                                            "topology", "placement", "biome",
                                            "silhouette", "elevation", "material",
                                            "asset_scale", "style", "occlusion",
                                        ]
                                    },
                                    "visibility": {
                                        "enum": ["direct", "partial", "inferred"]
                                    },
                                },
                            },
                        },
                    },
                },
            },
        },
    }


def write_packet(batch_dir: Path, *, output: Path | None = None) -> dict[str, Any]:
    """Write an immutable-source packet a vision provider can consume."""
    batch_dir = batch_dir.resolve()
    intake = _read(batch_dir / "intake_manifest.json")
    draft = _read(batch_dir / "annotations.draft.json")
    packet = {
        "schema_version": PACKET_VERSION,
        "task": "Inspect every listed source image and return exactly one response matching response_schema. Reconcile every secondary view to the same named features, but author geometry only in the canonical map's normalized canvas. Cite every source image at least once in a multi-image batch. Do not include source_images, image_reconciliation, world_bounds, generation_seed, paths, hashes, or engine artifacts. Preserve uncertainty: anything below 0.75 confidence must be review_state proposed, and inferred visibility must be reviewed before build.",
        "brief": intake.get("brief"),
        "target_engines": intake.get("target_engines", []),
        "world_bounds_m": intake.get("world_bounds_m", draft.get("world_bounds", {})),
        "source_images": intake.get("source_images", []),
        "image_reconciliation": intake.get(
            "image_reconciliation", draft.get("image_reconciliation", {})
        ),
        "response_schema": response_schema(),
        "terrain_grammar": {
            "alpine_massif": "alpine_jagged_massif with elevation_m [min,max], snowline_m, cliffness",
            "alpine_ridge": "alpine_sawtooth_ridge with elevation_m [min,max], cliffness",
            "foothills": "rolling_foothills with elevation_m [min,max]",
            "crag_field": "scattered_crag_field with elevation_m [min,max]",
            "cliff_band": "cliff_escarpment with elevation_m [min,max], cliffness",
            "valley_floor": "glacial_valley_floor with elevation_m [min,max], optional positive depth_m",
        },
    }
    _write(output or batch_dir / "vision_annotation_packet.json", packet)
    _write(batch_dir / "model_response.template.json", {"schema_version": VISION_RESPONSE_VERSION, "features": []})
    return packet


def _merge(draft: dict[str, Any], response: dict[str, Any]) -> dict[str, Any]:
    unknown = set(response) - _ALLOWED_RESPONSE_KEYS
    if unknown:
        raise ZoneCompileError("Vision response contains forbidden keys: %s" % ", ".join(sorted(unknown)))
    if response.get("schema_version") != VISION_RESPONSE_VERSION:
        raise ZoneCompileError("Vision response schema_version must be %s" % VISION_RESPONSE_VERSION)
    features = response.get("features")
    if not isinstance(features, list) or not features:
        raise ZoneCompileError("Vision response.features must be a non-empty array")
    merged = dict(draft)
    merged["schema_version"] = ANNOTATION_VERSION
    merged["features"] = features
    for key in ("asset_profiles", "terrain_materials", "terrain_material_scale_m"):
        if key in response:
            merged[key] = response[key]
    if isinstance(response.get("zone_name"), str) and response["zone_name"].strip():
        zone = dict(merged.get("zone", {}))
        zone["name"] = response["zone_name"].strip()
        merged["zone"] = zone
    return merged


def ingest_response(batch_dir: Path, response_path: Path, *, output: Path | None = None) -> dict[str, Any]:
    """Compile a model response into a proposal or write a durable diagnostic."""
    batch_dir, response_path = batch_dir.resolve(), response_path.resolve()
    draft = _read(batch_dir / "annotations.draft.json")
    response = _read(response_path)
    report_path = batch_dir / "vision_annotation_diagnostics.json"
    try:
        merged = _merge(draft, response)
        compiled = compile_annotations(merged, source_root=batch_dir)
        destination = output or batch_dir / "annotations.proposed.json"
        _write(destination, merged)
        overlay = render_overlay(compiled.zone_spec, batch_dir, batch_dir / "vision_proposal_evidence_overlay.png")
        report = {"schema_version": PACKET_VERSION, "status": "accepted_for_review", "proposal": destination.name, "validation": compiled.report, "evidence_overlay": overlay}
    except (ZoneCompileError, OSError, ValueError) as exc:
        report = {"schema_version": PACKET_VERSION, "status": "rejected", "response": str(response_path), "diagnostic": str(exc)}
    _write(report_path, report)
    if report["status"] == "rejected":
        raise ZoneCompileError("Vision annotation response rejected: " + str(report["diagnostic"]))
    return report


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Emit or ingest the constrained Codeweald vision-annotation contract")
    parser.add_argument("batch", type=Path)
    parser.add_argument("--emit-packet", action="store_true")
    parser.add_argument("--response", type=Path, help="provider response conforming to vision-annotation-response/v1")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    if args.emit_packet == bool(args.response):
        parser.error("Supply exactly one of --emit-packet or --response")
    try:
        if args.emit_packet:
            packet = write_packet(args.batch, output=args.output)
            print("Wrote vision annotation packet for %d source image(s)" % len(packet["source_images"]))
        else:
            report = ingest_response(args.batch, args.response, output=args.output)
            print("Vision annotation proposal: %s" % report["status"])
    except ZoneCompileError as exc:
        parser.error(str(exc))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
