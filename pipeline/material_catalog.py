"""Resolve provenance-pinned PBR bundles for portable terrain layers.

The texture lane is intentionally separate from visual generation.  This module
is the narrow bridge: a concept batch may name a material id, but it receives
paths only after the on-disk manifest and every declared map digest verify.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image

from texture_material_pipeline import MATERIAL_VERSION, TextureMaterialError, mean_linear_luminance


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _inside(path: Path, root: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


DEFAULT_REPEAT_METERS = {
    "grass": 8.0,
    "road": 6.0,
    "rock": 7.5,
    "wetland": 6.0,
    "snow": 5.0,
}


def resolve_terrain_materials(
    requested: dict[str, str],
    project_root: Path,
    asset_root: Path,
    repeat_meters: dict[str, float] | None = None,
) -> dict[str, dict[str, Any]]:
    """Return verified engine-neutral material references for named layers."""
    if not isinstance(requested, dict):
        raise TextureMaterialError("terrain_materials must be an object")
    project_root, asset_root = project_root.resolve(), asset_root.resolve()
    resolved: dict[str, dict[str, Any]] = {}
    for layer, material_id in requested.items():
        if layer not in DEFAULT_REPEAT_METERS:
            raise TextureMaterialError("terrain material layer %r is unsupported" % layer)
        if not isinstance(material_id, str) or not material_id:
            raise TextureMaterialError("terrain material %s must name a material id" % layer)
        directory = asset_root / "generated" / "codeweald_materials" / material_id
        manifest_path = directory / "material_manifest.json"
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise TextureMaterialError("terrain material %s has no readable accepted manifest" % material_id) from exc
        if not isinstance(manifest, dict) or manifest.get("schema_version") != MATERIAL_VERSION:
            raise TextureMaterialError("terrain material %s has an unsupported manifest" % material_id)
        if manifest.get("material_id") != material_id or manifest.get("status") not in {"accepted", "accepted_with_warnings"}:
            raise TextureMaterialError("terrain material %s is not accepted" % material_id)
        maps = manifest.get("maps")
        if not isinstance(maps, dict):
            raise TextureMaterialError("terrain material %s declares no maps" % material_id)
        map_references: dict[str, dict[str, str]] = {}
        dimensions: tuple[int, int] | None = None
        albedo_mean_linear_luminance: float | None = None
        for kind in ("albedo", "normal", "roughness"):
            entry = maps.get(kind)
            if not isinstance(entry, dict) or not isinstance(entry.get("path"), str) or not isinstance(entry.get("sha256"), str):
                raise TextureMaterialError("terrain material %s has invalid %s map metadata" % (material_id, kind))
            candidate = (directory / entry["path"]).resolve()
            if not _inside(candidate, directory) or not candidate.is_file() or _sha256(candidate) != entry["sha256"]:
                raise TextureMaterialError("terrain material %s %s map does not match its manifest" % (material_id, kind))
            try:
                with Image.open(candidate) as image:
                    candidate_dimensions = image.size
                    if kind == "albedo":
                        # The manifest hash already pins these exact pixels, so
                        # deriving luminance here at resolve time keeps every
                        # previously accepted material bundle usable without a
                        # regeneration pass or a manifest schema bump.
                        pixels = np.asarray(image.convert("RGB"), dtype=np.float32) / 255.0
                        albedo_mean_linear_luminance = mean_linear_luminance(pixels)
            except OSError as exc:
                raise TextureMaterialError(
                    "terrain material %s %s map cannot be decoded"
                    % (material_id, kind)
                ) from exc
            if dimensions is None:
                dimensions = candidate_dimensions
            elif dimensions != candidate_dimensions:
                raise TextureMaterialError(
                    "terrain material %s maps do not share one resolution"
                    % material_id
                )
            map_references[kind] = {"path": candidate.relative_to(project_root).as_posix(), "sha256": entry["sha256"]}
        scale = float(
            (repeat_meters or {}).get(layer, DEFAULT_REPEAT_METERS[layer])
        )
        if not 1.0 <= scale <= 32.0:
            raise TextureMaterialError(
                "terrain material %s repeat scale must be in [1, 32] meters"
                % layer
            )
        width, height = dimensions or (0, 0)
        resolved[layer] = {
            "material_id": material_id,
            "manifest": manifest_path.relative_to(project_root).as_posix(),
            "maps": map_references,
            "dimensions_px": {"width": width, "height": height},
            "meters_per_repeat": scale,
            "minimum_texels_per_meter": round(min(width, height) / scale, 4),
            "albedo_mean_linear_luminance": round(albedo_mean_linear_luminance, 6),
        }
    return resolved
