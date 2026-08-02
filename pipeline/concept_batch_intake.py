#!/usr/bin/env python3
"""Create a provenance-preserving, model-ready Codeweald concept batch.

This is the pipeline's front door for arbitrary concept-art batches.  It does
not pretend that luminance, hue, or an unreviewed vision guess is navigation or
collision data.  Instead it fingerprints the supplied images, makes them
portable inside a batch, and writes a small, constrained annotation task for a
vision-capable model.  The resulting ``annotations.draft.json`` is the only
thing a model needs to complete before the deterministic ZoneSpec build starts.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
from pathlib import Path
from typing import Any, Iterable

from PIL import Image
from image_reconciliation import (
    ROLE_ALLOWED_CLAIMS,
    ReconciliationError,
    build_reconciliation,
)


INTAKE_VERSION = "codeweald.concept-batch-intake/v1"
ANNOTATION_VERSION = "codeweald.concept-annotations/v1"
SAFE_ID = re.compile(r"^[a-z][a-z0-9_]{1,63}$")


class IntakeError(ValueError):
    """The supplied art batch cannot become a deterministic intake package."""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _parse_assignment(value: str, label: str) -> tuple[str, str]:
    key, separator, item = value.partition("=")
    if not separator or not SAFE_ID.fullmatch(key) or not item:
        raise IntakeError("%s must be id=value, with a lowercase identifier" % label)
    return key, item


def _read_brief(value: str | None, path: Path | None) -> str:
    if bool(value) == bool(path):
        raise IntakeError("Provide exactly one of --brief or --brief-file")
    text = value if value else path.read_text(encoding="utf-8")
    if not text or not text.strip():
        raise IntakeError("The zone brief cannot be empty")
    return text.strip()


def _image_metadata(path: Path) -> tuple[int, int, str]:
    try:
        with Image.open(path) as image:
            width, height, mode = image.size[0], image.size[1], image.mode
    except (OSError, ValueError) as exc:
        raise IntakeError("Cannot decode concept image %s: %s" % (path, exc)) from exc
    if width < 32 or height < 32:
        raise IntakeError("Concept image %s must be at least 32 pixels in each direction" % path)
    return width, height, mode


def _write_json(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _task_markdown(
    batch_id: str,
    brief: str,
    images: list[dict[str, Any]],
    width_m: float,
    length_m: float,
    engines: list[str],
    reconciliation: dict[str, Any],
) -> str:
    sources = "\n".join("- `%s`: `%s` (%dx%d, role `%s`)" % (entry["id"], entry["path"], entry["width_px"], entry["height_px"], entry["role"]) for entry in images)
    return f"""# Codeweald semantic-annotation task: {batch_id}

## Brief

{brief}

## Source evidence

{sources}

## Deliverable

Copy `annotations.draft.json` to `annotations.json` and fill it. The intended
world bounds are `{width_m:g} x {length_m:g}` metres. The deterministic compiler
will target: {", ".join(engines)}.

For every feature, provide an ID, category, semantic, normalised 0..1 geometry,
confidence, a `proposed` or `reviewed` state, and at least one evidence region
`[left, top, right, bottom]` tied to a listed image. Geometry must name
`source_image_id: {reconciliation["canonical_map_image_id"]}`. Every evidence
entry in a multi-image batch must name its claim and visibility. Perspective
and detail images are evidence for silhouette, elevation, occlusion, materials,
or asset scale; they never become map coordinates.

Describe topology, not pixels: lanes, traversable valleys, rivers, bases,
objectives, forests, and landforms. Use `alpine_massif` only when the evidence
shows sharp high-relief walls; include an `alpine_jagged_massif` generation
profile, elevation range, snowline, and cliffness. Do not turn dark/bright
areas into terrain simply because of colour. Unknown or uncertain features must
remain `proposed` below 0.75 confidence; they will not enter the build until
reviewed.

Start with the semantic primitives that establish play space, then add asset
profiles and dressing. The compilation path is:

`source art -> reviewed annotations -> ZoneSpec -> terrain/assets/runtime -> Godot`.
"""


def create_batch(
    output: Path, batch_id: str, brief: str, image_specs: list[tuple[str, Path]], roles: dict[str, str],
    width_m: float, length_m: float, engines: list[str],
    canonical_map_image_id: str | None = None,
) -> dict[str, Any]:
    if not SAFE_ID.fullmatch(batch_id):
        raise IntakeError("batch_id must be a lowercase identifier")
    if width_m <= 0 or length_m <= 0:
        raise IntakeError("world dimensions must be positive")
    if not image_specs:
        raise IntakeError("At least one --image id=PATH is required")
    spec_ids = [image_id for image_id, _path in image_specs]
    unknown_roles = set(roles) - set(spec_ids)
    if unknown_roles:
        raise IntakeError("--role references unknown image id(s): %s" % ", ".join(sorted(unknown_roles)))
    unsupported_roles = {
        role for role in roles.values() if role not in ROLE_ALLOWED_CLAIMS
    }
    if unsupported_roles:
        raise IntakeError(
            "Unsupported source image role(s): %s"
            % ", ".join(sorted(unsupported_roles))
        )
    output = output.resolve()
    source_dir = output / "source"
    source_dir.mkdir(parents=True, exist_ok=True)
    images: list[dict[str, Any]] = []
    seen_ids: set[str] = set()
    for image_id, raw_path in image_specs:
        if image_id in seen_ids:
            raise IntakeError("Duplicate image id: %s" % image_id)
        seen_ids.add(image_id)
        source = raw_path.expanduser().resolve()
        if not source.is_file():
            raise IntakeError("Concept image does not exist: %s" % source)
        width, height, mode = _image_metadata(source)
        destination = source_dir / (image_id + source.suffix.lower())
        digest = _sha256(source)
        if destination.exists() and _sha256(destination) != digest:
            raise IntakeError("Refusing to overwrite different source image %s" % destination)
        if not destination.exists():
            shutil.copy2(source, destination)
        images.append({
            "id": image_id,
            "path": (Path("source") / destination.name).as_posix(),
            "role": roles.get(image_id, "reference"),
            "sha256": digest,
            "width_px": width,
            "height_px": height,
            "mode": mode,
        })
    try:
        reconciliation = build_reconciliation(
            images, canonical_map_image_id, width_m, length_m
        )
    except ReconciliationError as exc:
        raise IntakeError(str(exc)) from exc
    seed = int(hashlib.sha256((batch_id + "\n" + brief).encode("utf-8")).hexdigest()[:8], 16)
    annotations = {
        "schema_version": ANNOTATION_VERSION,
        "zone": {"id": batch_id, "name": batch_id.replace("_", " ").title()},
        "world_bounds": {"width": width_m, "length": length_m},
        "generation_seed": seed,
        "acceptance_policy": {
            "minimum_style_score": 0.42,
            "style_mismatch": "fail",
            "maximum_regional_palette_mismatch_fraction": 0.25,
            "minimum_biome_coverage_ratio": 0.20,
            "landmark_evidence_fill_ratio": [0.002, 2.5],
        },
        "traversal_policy": {
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
        },
        "source_images": [
            {
                key: value
                for key, value in entry.items()
                if key in {"id", "path", "role", "sha256", "width_px", "height_px"}
            }
            for entry in images
        ],
        "image_reconciliation": reconciliation,
        "asset_profiles": {},
        "features": [],
    }
    manifest = {
        "schema_version": INTAKE_VERSION,
        "batch_id": batch_id,
        "brief": brief,
        "target_engines": engines,
        "world_bounds_m": {"width": width_m, "length": length_m},
        "source_images": images,
        "image_reconciliation": reconciliation,
        "annotation_contract": {"path": "annotations.draft.json", "schema_version": ANNOTATION_VERSION},
    }
    _write_json(output / "intake_manifest.json", manifest)
    _write_json(output / "annotations.draft.json", annotations)
    (output / "vision_annotation_task.md").write_text(
        _task_markdown(
            batch_id,
            brief,
            images,
            width_m,
            length_m,
            engines,
            reconciliation,
        ),
        encoding="utf-8",
    )
    # Keep the prose task for human readability, but also emit a bounded JSON
    # packet that any vision provider can consume without being allowed to
    # change the hashes, world dimensions, or other batch authority.
    from vision_annotation_adapter import write_packet
    write_packet(output)
    return manifest


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Initialize a provenance-preserving Codeweald concept-art batch")
    parser.add_argument("--output", required=True, type=Path, help="new/existing concept_batches/<batch_id> directory")
    parser.add_argument("--batch-id", required=True)
    parser.add_argument("--brief")
    parser.add_argument("--brief-file", type=Path)
    parser.add_argument("--image", action="append", default=[], help="repeatable id=/absolute/or/relative/image.png")
    parser.add_argument("--role", action="append", default=[], help="repeatable image_id=painted_overview|reference|detail")
    parser.add_argument(
        "--canonical-map",
        help="source image id that owns normalized map geometry; required when multiple map candidates exist",
    )
    parser.add_argument("--width-m", type=float, default=2400.0)
    parser.add_argument("--length-m", type=float, default=1600.0)
    parser.add_argument("--engine", action="append", choices=["godot", "unity", "unreal"], default=[])
    args = parser.parse_args(argv)
    try:
        image_specs = [(key, Path(value)) for key, value in (_parse_assignment(item, "--image") for item in args.image)]
        roles = dict(_parse_assignment(item, "--role") for item in args.role)
        manifest = create_batch(
            args.output,
            args.batch_id,
            _read_brief(args.brief, args.brief_file),
            image_specs,
            roles,
            args.width_m,
            args.length_m,
            args.engine or ["godot"],
            args.canonical_map,
        )
    except (IntakeError, OSError) as exc:
        parser.error(str(exc))
    print("Initialized concept batch %s with %d image(s)" % (manifest["batch_id"], len(manifest["source_images"])))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
