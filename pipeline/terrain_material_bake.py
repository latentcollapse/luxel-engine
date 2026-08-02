#!/usr/bin/env python3
"""Bake verified terrain materials into portable world-space preview maps.

This is the inexpensive renderer-parity lane.  The splat contract remains the
authoritative material description; these images let simple backends display
the same accepted source materials without implementing a terrain shader.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image


LAYERS = ("grass", "road", "rock", "snow")


def _save_atomic(image: Image.Image, destination: Path) -> None:
    temporary = destination.with_name("." + destination.name + ".tmp")
    image.save(temporary, format="PNG")
    temporary.replace(destination)


def _load_map(project_root: Path, material: dict[str, Any], kind: str) -> np.ndarray:
    path = project_root / material["maps"][kind]["path"]
    with Image.open(path) as image:
        if kind == "roughness":
            return np.asarray(image.convert("L"), dtype=np.float32) / 255.0
        return np.asarray(image.convert("RGB"), dtype=np.float32) / 255.0


def _tile_world_space(
    source: np.ndarray,
    *,
    width_m: float,
    length_m: float,
    repeat_m: float,
    resolution: int,
) -> np.ndarray:
    """Sample one image in canonical X/Z world coordinates."""
    source_height, source_width = source.shape[:2]
    x = np.linspace(0.0, width_m, resolution, endpoint=True)
    z = np.linspace(0.0, length_m, resolution, endpoint=True)
    columns = np.floor((x / repeat_m) * source_width).astype(np.int64) % source_width
    rows = np.floor((z / repeat_m) * source_height).astype(np.int64) % source_height
    return source[rows[:, None], columns[None, :]]


def bake_material_preview(
    *,
    project_root: Path,
    output_dir: Path,
    splat_rgba: np.ndarray,
    wetland_mask: np.ndarray,
    materials: dict[str, dict[str, Any]],
    width_m: float,
    length_m: float,
) -> dict[str, str]:
    """Bake albedo and packed metallic/roughness maps from verified bundles."""
    missing = [layer for layer in (*LAYERS, "wetland") if layer not in materials]
    if missing:
        raise ValueError("terrain material bake is missing layers: " + ", ".join(missing))
    resolution = int(splat_rgba.shape[0])
    if splat_rgba.shape != (resolution, resolution, 4):
        raise ValueError("terrain splat must be one square RGBA image")
    if wetland_mask.shape != (resolution, resolution):
        raise ValueError("wetland mask must match terrain splat resolution")

    weights = splat_rgba.astype(np.float32) / 255.0
    wetland = (
        (wetland_mask.astype(np.float32) / 255.0) * weights[:, :, 0] * 0.82
    )
    weights[:, :, 0] = np.clip(weights[:, :, 0] - wetland, 0.0, 1.0)

    albedo = np.zeros((resolution, resolution, 3), dtype=np.float32)
    roughness = np.zeros((resolution, resolution), dtype=np.float32)
    for channel, layer in enumerate(LAYERS):
        contract = materials[layer]
        tiled_albedo = _tile_world_space(
            _load_map(project_root, contract, "albedo"),
            width_m=width_m,
            length_m=length_m,
            repeat_m=float(contract["meters_per_repeat"]),
            resolution=resolution,
        )
        tiled_roughness = _tile_world_space(
            _load_map(project_root, contract, "roughness"),
            width_m=width_m,
            length_m=length_m,
            repeat_m=float(contract["meters_per_repeat"]),
            resolution=resolution,
        )
        albedo += tiled_albedo * weights[:, :, channel, None]
        roughness += tiled_roughness * weights[:, :, channel]

    wetland_contract = materials["wetland"]
    wetland_albedo = _tile_world_space(
        _load_map(project_root, wetland_contract, "albedo"),
        width_m=width_m,
        length_m=length_m,
        repeat_m=float(wetland_contract["meters_per_repeat"]),
        resolution=resolution,
    )
    wetland_roughness = _tile_world_space(
        _load_map(project_root, wetland_contract, "roughness"),
        width_m=width_m,
        length_m=length_m,
        repeat_m=float(wetland_contract["meters_per_repeat"]),
        resolution=resolution,
    )
    albedo += wetland_albedo * wetland[:, :, None]
    roughness += wetland_roughness * wetland

    output_dir.mkdir(parents=True, exist_ok=True)
    albedo_path = output_dir / "terrain_material_albedo.png"
    packed_path = output_dir / "terrain_material_metallic_roughness.png"
    _save_atomic(
        Image.fromarray(
            np.clip(albedo * 255.0, 0, 255).astype(np.uint8), mode="RGB"
        ),
        albedo_path,
    )
    packed = np.zeros((resolution, resolution, 3), dtype=np.uint8)
    packed[:, :, 1] = np.clip(roughness * 255.0, 0, 255).astype(np.uint8)
    _save_atomic(Image.fromarray(packed, mode="RGB"), packed_path)
    return {
        "albedo": albedo_path.name,
        "metallic_roughness": packed_path.name,
        "projection": "canonical_xz_world_space",
    }
