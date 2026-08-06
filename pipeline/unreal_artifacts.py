#!/usr/bin/env python3
"""Create Unreal-ready Landscape height/layer files from canonical terrain raster.

Codeweald's canonical 1025-square terrain is ideal for Godot/Unity but is not
an Unreal Landscape component resolution.  The adapter deliberately emits the
nearest documented valid layout (1009 = 16 x 63 quads + 1) rather than handing
UE an invalid PNG and hoping the editor guesses what to do.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from PIL import Image


UNREAL_LANDSCAPE_RESOLUTION = 1009
CHANNELS = {"grass": 0, "road": 1, "rock": 2, "snow": 3}
# The fifth layer does not live in the RGBA splat -- four channels is all an
# RGBA image has -- so it is carried by its own single-channel image and lifted
# to a weightmap here alongside the other four. Unreal composites weightmaps
# independently, so five layers is not a special case for the Landscape; it was
# only ever a special case for the file format.
WETLAND_LAYER = "wetland"


def write_unreal_artifacts(terrain_dir: Path) -> dict[str, Any]:
    terrain_dir = terrain_dir.resolve()
    height_path, splat_path = terrain_dir / "heightmap_16.png", terrain_dir / "splatmap.png"
    if not height_path.is_file() or not splat_path.is_file():
        raise FileNotFoundError("Canonical heightmap_16.png and splatmap.png are required")
    output = terrain_dir / "unreal"
    output.mkdir(parents=True, exist_ok=True)
    with Image.open(height_path) as height_source:
        # Pillow retains 16-bit grayscale mode for the Codeweald canonical input.
        height = height_source.resize((UNREAL_LANDSCAPE_RESOLUTION, UNREAL_LANDSCAPE_RESOLUTION), Image.Resampling.BILINEAR)
        if height.mode not in {"I;16", "I;16B", "I"}:
            height = height.convert("I;16")
        height.save(output / "landscape_height_16.png")
    with Image.open(splat_path) as splat_source:
        splat = splat_source.convert("RGBA").resize((UNREAL_LANDSCAPE_RESOLUTION, UNREAL_LANDSCAPE_RESOLUTION), Image.Resampling.BILINEAR)
        for name, channel in CHANNELS.items():
            splat.getchannel(channel).save(output / (name + "_weight.png"))
    wetland_path = terrain_dir / "wetland_mask.png"
    if not wetland_path.is_file():
        raise FileNotFoundError(
            "Canonical wetland_mask.png is required: it carries the fifth splat "
            "weight, and a Landscape built without it paints bog as grass"
        )
    with Image.open(wetland_path) as wetland_source:
        wetland_source.convert("L").resize(
            (UNREAL_LANDSCAPE_RESOLUTION, UNREAL_LANDSCAPE_RESOLUTION),
            Image.Resampling.BILINEAR,
        ).save(output / (WETLAND_LAYER + "_weight.png"))
    layers = (*CHANNELS, WETLAND_LAYER)
    return {
        "landscape_resolution": UNREAL_LANDSCAPE_RESOLUTION,
        "heightmap_16": "unreal/landscape_height_16.png",
        "weightmaps": {name: "unreal/%s_weight.png" % name for name in layers},
        "component_layout": {"section_size_quads": 63, "sections_per_component": 1, "components_per_axis": 16},
    }
