"""Editor-side preflight and verified-prop import for Codeweald Unreal payloads.

Install this directory in a UE project's Plugins directory, enable Python Editor
Script Plugin plus the glTF importer, and call ``import_zone(manifest_path)``
from Unreal's Python console.  Terrain import stays deliberately explicit: the
payload includes a valid Landscape heightmap plus one layer source per material,
and ``preflight`` reports every value a native Landscape importer must consume.
It does not silently build a visually plausible substitute Landscape.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any


SCHEMA = "codeweald.unreal-zone-import/v1"
REQUIRED_LANDSCAPE_RESOLUTION = 1009


def _load(path: str | Path) -> tuple[dict[str, Any], Path]:
    manifest_path = Path(path).expanduser().resolve()
    value = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or value.get("schema_version") != SCHEMA:
        raise ValueError("Expected %s" % SCHEMA)
    return value, manifest_path.parent


def _existing(root: Path, relative: str | None) -> Path | None:
    if not relative:
        return None
    candidate = (root / relative).resolve()
    return candidate if candidate.is_file() else None


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _find_source(manifest_dir: Path, relative: str) -> Path | None:
    for parent in (manifest_dir, *manifest_dir.parents):
        candidate = parent / relative
        if candidate.is_file():
            return candidate
    return None


def _feature_sources(feature: dict[str, Any]) -> list[dict[str, Any]]:
    sources = [source for source in feature.get("pcg_asset_sources", []) if isinstance(source, dict)]
    for layer in feature.get("pcg_layers", []):
        if isinstance(layer, dict):
            sources.extend(source for source in layer.get("asset_sources", []) if isinstance(source, dict))
    return sources


def preflight(manifest_path: str | Path) -> dict[str, Any]:
    """Return a deterministic acceptance report without changing the UE project."""
    manifest, root = _load(manifest_path)
    landscape = manifest.get("landscape") if isinstance(manifest.get("landscape"), dict) else {}
    errors: list[str] = []
    height = _existing(root, landscape.get("heightmap_16"))
    if height is None:
        errors.append("missing Unreal Landscape heightmap")
    if landscape.get("resolution") != REQUIRED_LANDSCAPE_RESOLUTION:
        errors.append("Landscape resolution must be %d" % REQUIRED_LANDSCAPE_RESOLUTION)
    if not isinstance(landscape.get("z_scale"), (float, int)) or landscape["z_scale"] <= 0:
        errors.append("Landscape z_scale must be positive")
    layers = landscape.get("layer_sources", {}).get("weightmaps", {})
    for name in ("grass", "road", "rock", "snow"):
        if _existing(root, layers.get(name) if isinstance(layers, dict) else None) is None:
            errors.append("missing %s Landscape layer" % name)
    asset_sources = 0
    verified_assets = 0
    for feature in manifest.get("pcg_features", []):
        if not isinstance(feature, dict):
            continue
        for source in _feature_sources(feature):
            asset_sources += 1
            candidate = _find_source(root, str(source.get("path", "")))
            if candidate and source.get("sha256") == _sha256(candidate):
                verified_assets += 1
            elif candidate:
                errors.append("digest mismatch: %s" % source.get("path"))
            else:
                errors.append("missing source prop: %s" % source.get("path"))
    return {
        "schema_version": "codeweald.unreal-preflight/v1",
        "zone_id": manifest.get("zone_id"),
        "status": "passed" if not errors else "failed",
        "errors": errors,
        "heightmap": str(height) if height else None,
        "z_scale": landscape.get("z_scale"),
        "asset_sources": asset_sources,
        "verified_assets": verified_assets,
    }


def import_zone(manifest_path: str | Path, destination: str = "/Game/CodewealdGenerated") -> dict[str, Any]:
    """Preflight then import only verified GLB/FBX props into native UE assets.

    Landscape files remain in the report for the project Landscape importer.
    This is intentional until a compiled LandscapeEditor module owns the final
    Landscape and material-layer creation transaction.
    """
    report = preflight(manifest_path)
    if report["status"] != "passed":
        raise RuntimeError("Codeweald preflight failed: " + "; ".join(report["errors"]))
    import unreal  # Available only inside Unreal Editor.

    manifest, root = _load(manifest_path)
    seen: set[str] = set()
    imported: list[str] = []
    for feature in manifest.get("pcg_features", []):
        for source in _feature_sources(feature) if isinstance(feature, dict) else []:
            path = _find_source(root, str(source.get("path", "")))
            if path is None or str(path) in seen:
                continue
            seen.add(str(path))
            task = unreal.AssetImportTask()
            task.filename = str(path)
            task.destination_path = destination.rstrip("/") + "/SourceProps"
            task.automated = True
            task.save = True
            unreal.AssetToolsHelpers.get_asset_tools().import_asset_tasks([task])
            imported.extend(str(value) for value in task.imported_object_paths)
    report["imported_object_paths"] = imported
    unreal.log("Codeweald imported verified source props; Landscape payload retained at " + str(report["heightmap"]))
    return report
