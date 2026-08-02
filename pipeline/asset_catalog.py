#!/usr/bin/env python3
"""Index project assets into an engine-neutral, verifiable Codeweald catalog.

The catalog is deliberately an observed fact about the project, not another place
for a model to invent paths.  It records a source-relative path, file digest,
semantic filename tags, and whether Godot has completed its import sidecar.
Other engine adapters can consume the same source paths and formats.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any, Iterable


CATALOG_VERSION = "codeweald.asset-catalog/v1"
SUPPORTED_FORMATS = {".glb", ".gltf", ".fbx", ".obj"}


def _tags(path: Path) -> list[str]:
    words = set(filter(None, re.split(r"[^a-z0-9]+", path.stem.lower())))
    value = path.stem.lower()
    tags: set[str] = set()
    if "tree" in words:
        tags.update(("foliage", "tree"))
    # Token matching is intentional: ``alpine_cliff`` is not a pine tree.
    if {"pine", "spruce", "fir"}.intersection(words):
        tags.update(("foliage", "tree", "conifer"))
    for lod in ("lod0", "lod1", "lod2"):
        if lod in words:
            tags.add(lod)
    if "bush" in words or "hedge" in words or "plant" in words:
        tags.update(("foliage", "shrub"))
    if {"fern", "grass", "flower", "clover", "mushroom"}.intersection(words):
        tags.update(("foliage", "undergrowth"))
    if "pebble" in words or "rock" in words or "stone" in words or "cliff" in words or "boulder" in words or {"alpine", "granite"}.issubset(words):
        tags.add("rock")
    if "cliff" in words or "cliff" in value or {"alpine", "granite"}.issubset(words):
        tags.update(("cliff", "alpine"))
    if "keep" in words or "castle" in words or "tower" in words:
        tags.update(("structure", "fortification"))
    if "bridge" in words:
        tags.update(("structure", "bridge"))
    if "ruin" in words or "temple" in words:
        tags.update(("structure", "ruin"))
    if {"settlement", "village", "hamlet", "house", "outpost"}.intersection(words):
        tags.update(("structure", "settlement"))
    return sorted(tags)


def build_catalog(project_root: Path, asset_root: Path) -> dict[str, Any]:
    project_root = project_root.resolve()
    root = (project_root / asset_root).resolve() if not asset_root.is_absolute() else asset_root.resolve()
    if not root.is_dir() or project_root not in root.parents and root != project_root:
        raise ValueError("Asset root must be an existing directory inside project root: %s" % root)
    assets: list[dict[str, Any]] = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.suffix.lower() not in SUPPORTED_FORMATS:
            continue
        relative = path.relative_to(project_root).as_posix()
        source_bytes = path.read_bytes()
        assets.append(
            {
                "id": "asset_" + hashlib.sha256(relative.encode("utf-8")).hexdigest()[:16],
                "source_path": relative,
                "format": path.suffix.lower().lstrip("."),
                "sha256": hashlib.sha256(source_bytes).hexdigest(),
                "size_bytes": len(source_bytes),
                "tags": _tags(path),
                "engine_readiness": {"godot": path.with_name(path.name + ".import").is_file()},
            }
        )
    return {
        "schema_version": CATALOG_VERSION,
        "asset_root": root.relative_to(project_root).as_posix(),
        "asset_count": len(assets),
        "assets": assets,
    }


def _write(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build a portable Codeweald project asset catalog")
    parser.add_argument("--project-root", type=Path, default=Path("."))
    parser.add_argument("--asset-root", type=Path, default=Path("assets/models"))
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        catalog = build_catalog(args.project_root, args.asset_root)
        _write(args.output, catalog)
    except (OSError, ValueError) as exc:
        parser.error(str(exc))
    print("Cataloged %d assets at %s" % (catalog["asset_count"], args.output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
