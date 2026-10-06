#!/usr/bin/env python3
"""Cut mountains out of a Gaea heightfield and export them as placeable meshes.

**Why this exists.** The border stopped being generated (docs/integrations/gaea-programme.md
5.7a). `_border_rampart` is a swept profile -- every cross-section identical by
construction -- so it can never produce a peak where ridgelines *converge*, and
convergence is what separates a mountain from an extruded triangle. That is
topology, not tuning, which is why three attempts at generating the border all
failed.

Cropping sidesteps the whole problem by **harvesting erosion rather than
synthesising it**. A region cut from a Gaea heightfield already has drainage,
ridgelines and a back, because a hydraulic simulation put them there. Nothing
here tries to be clever about landform; the cleverness already happened
upstream.

It also obeys the rule in 1 exactly, which nothing before it has: a crop is an
**object with its own shape**, sited independently. Its geometry is not a
function of distance to the play space, so it cannot reproduce the defect.

**Peak selection is the only judgement this tool makes.** A crop is only as good
as the landform inside it, so `--auto` ranks candidate centres by local
prominence -- height above the lowest ground within a radius -- which is a cheap
proxy for "is there a real summit here rather than a shoulder". It is a proxy
and it is stated as one; the operator still looks at the result.

Output is a binary glTF (`.glb`) written directly. Blender is not in the path on
purpose: the mesh is a heightfield with walls, which needs none of Blender's
features, and a pure-Python writer stays deterministic and testable.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
from pathlib import Path

import numpy as np

CROP_SCHEMA = "codeweald.mountain-crop/v1"

# Bumped when the geometry rule changes, so a crop that disagrees with today's
# cropper is detectable rather than merely different.
CROPPER_VERSION = 2

# glTF component types.
_FLOAT = 5126
_UNSIGNED_INT = 5125


def load_heightfield(path: Path, resolution: int | None = None) -> np.ndarray:
    """Metres, as a square float32 grid.

    Accepts the float32 buffer `import_heightfield.py` writes (already in
    metres) or a 16-bit image, which needs `--relief-m` to acquire units.
    """
    if path.suffix.lower() == ".bin":
        raw = np.fromfile(path, dtype="<f4")
        side = resolution or int(round(len(raw) ** 0.5))
        if side * side != len(raw):
            raise SystemExit(
                f"{path.name} holds {len(raw)} floats, which is not a square grid; "
                "pass --resolution"
            )
        return raw.reshape(side, side).astype(np.float32)

    from PIL import Image

    with Image.open(path) as source:
        if source.mode not in {"I;16", "I", "L", "F"}:
            source = source.convert("I")
        data = np.asarray(source).astype(np.float32)
    if data.ndim != 2 or data.shape[0] != data.shape[1]:
        raise SystemExit(f"{path.name} is not a square single-channel image")
    return data


def find_peaks(
    heights: np.ndarray, window: int, count: int, separation: int
) -> list[tuple[int, int, float]]:
    """The `count` most prominent summits, as `(row, column, prominence)`.

    Prominence here is height above the lowest ground within `window` -- not
    true topographic prominence, which needs the saddle to a higher peak. The
    cheap version is enough to rank "summit" above "shoulder", and saying so
    keeps the limitation visible rather than implied.
    """
    radius = max(window // 2, 1)
    padded = np.pad(heights, radius, mode="edge")
    lowest = np.full(heights.shape, np.inf, dtype=np.float32)
    # Separable minimum over the window: two passes instead of window^2 work.
    for offset in range(-radius, radius + 1):
        shifted = padded[radius + offset : radius + offset + heights.shape[0], :]
        np.minimum(lowest, shifted[:, radius : radius + heights.shape[1]], out=lowest)
    columnwise = np.full(heights.shape, np.inf, dtype=np.float32)
    padded_rows = np.pad(lowest, ((0, 0), (radius, radius)), mode="edge")
    for offset in range(-radius, radius + 1):
        np.minimum(
            columnwise,
            padded_rows[:, radius + offset : radius + offset + heights.shape[1]],
            out=columnwise,
        )
    prominence = heights - columnwise

    # Never centre a crop where the window would run off the grid.
    margin = radius
    ranked = np.argsort(prominence, axis=None)[::-1]
    chosen: list[tuple[int, int, float]] = []
    for flat in ranked:
        row, column = divmod(int(flat), heights.shape[1])
        if not (margin <= row < heights.shape[0] - margin):
            continue
        if not (margin <= column < heights.shape[1] - margin):
            continue
        if any(
            abs(row - other_row) < separation and abs(column - other_column) < separation
            for other_row, other_column, _ in chosen
        ):
            continue
        chosen.append((row, column, float(prominence[row, column])))
        if len(chosen) >= count:
            break
    return chosen


def crop_region(
    heights: np.ndarray, row: int, column: int, window: int, stride: int = 1
) -> np.ndarray:
    """The patch around a peak, optionally decimated.

    A 150 m crop at 0.78 m cells is 193^2 vertices -- 76k triangles for one
    background mountain, and a border needs a dozen of them. Backdrop geometry
    is seen from hundreds of metres away and at that distance the source
    resolution is far past what the silhouette can carry, so stride is the
    difference between scenery that costs nothing and scenery that costs a
    frame. Decimation is plain subsampling rather than averaging: averaging
    rounds off exactly the ridgelines that make a crop worth cutting.
    """
    radius = window // 2
    patch = heights[
        row - radius : row + radius + 1, column - radius : column + radius + 1
    ]
    return patch[::stride, ::stride].copy() if stride > 1 else patch.copy()


def taper_edges(patch: np.ndarray, falloff: float) -> np.ndarray:
    """Bring the crop's perimeter down to its own floor.

    A square window cut from a heightfield leaves the perimeter at whatever
    height the terrain happened to be, so every crop is a rectangular block and
    the sheer sides read as sliced cake the moment they rise above the ground it
    is standing on. Burying the base does not help -- the cut faces are *above*
    ground, not below it.

    So the outer margin is tapered radially to the floor: the mesh meets the
    ground at its own base level and there is no cut face to see. The taper is
    flat until `1 - falloff` of the radius and then smoothsteps, because a
    falloff applied across the whole patch rounds the massif into a dome and
    throws away the ridgelines that were the reason to cut here.

    Radial rather than square so the footprint reads as a landform rather than
    as a tile, and so rotating a crop for variety does not expose a seam.
    """
    if falloff <= 0.0:
        return patch
    side = patch.shape[0]
    axis = np.linspace(-1.0, 1.0, side, dtype=np.float32)
    grid_x, grid_z = np.meshgrid(axis, axis)
    radius = np.sqrt(grid_x**2 + grid_z**2)
    inner = 1.0 - float(np.clip(falloff, 0.01, 0.99))
    ramp = np.clip((radius - inner) / max(1.0 - inner, 1e-6), 0.0, 1.0)
    weight = 1.0 - ramp * ramp * (3.0 - 2.0 * ramp)
    floor = float(patch.min())
    return (floor + (patch - floor) * weight).astype(np.float32)


def build_mesh(
    patch: np.ndarray, cell_m: float, skirt_depth_m: float
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """A closed solid: the terrain surface, vertical walls, and a flat cap.

    Closed rather than an open sheet because the base is buried under the world
    skirt and an open sheet is see-through from below and from any grazing angle
    -- the same class of defect as terrain that stops at the data edge.

    The mesh is centred on the origin in X/Z and its **lowest surface point sits
    at y = 0**, so a placement puts the base on the ground and everything above
    is mountain. Burying it is then just a negative Y offset.
    """
    side = patch.shape[0]
    floor = float(patch.min())
    surface = (patch - floor).astype(np.float32)
    base_y = -abs(skirt_depth_m)

    extent = (side - 1) * cell_m
    axis = np.linspace(-extent * 0.5, extent * 0.5, side, dtype=np.float32)
    grid_x, grid_z = np.meshgrid(axis, axis)

    positions = np.stack(
        [grid_x.ravel(), surface.ravel(), grid_z.ravel()], axis=1
    ).astype(np.float32)

    # Normals from the height gradient, matching the viewer's convention.
    gradient_z, gradient_x = np.gradient(surface, cell_m)
    normals = np.stack(
        [-gradient_x.ravel(), np.ones(side * side, np.float32), -gradient_z.ravel()],
        axis=1,
    ).astype(np.float32)
    normals /= np.maximum(np.linalg.norm(normals, axis=1, keepdims=True), 1e-6)

    index = np.arange(side * side, dtype=np.uint32).reshape(side, side)
    top_left = index[:-1, :-1].ravel()
    top_right = index[:-1, 1:].ravel()
    bottom_left = index[1:, :-1].ravel()
    bottom_right = index[1:, 1:].ravel()
    faces = [
        np.stack([top_left, bottom_left, top_right], axis=1),
        np.stack([top_right, bottom_left, bottom_right], axis=1),
    ]

    # Walls: one skirt vertex under each border vertex, wound outward.
    border = np.concatenate(
        [index[0, :], index[1:, -1], index[-1, -2::-1], index[-2:0:-1, 0]]
    )
    skirt_start = positions.shape[0]
    skirt_positions = positions[border].copy()
    skirt_positions[:, 1] = base_y
    skirt_normals = skirt_positions.copy()
    skirt_normals[:, 1] = 0.0
    lengths = np.maximum(np.linalg.norm(skirt_normals, axis=1, keepdims=True), 1e-6)
    skirt_normals = (skirt_normals / lengths).astype(np.float32)

    positions = np.concatenate([positions, skirt_positions])
    normals = np.concatenate([normals, skirt_normals])

    ring = np.arange(len(border), dtype=np.uint32)
    following = (ring + 1) % len(border)
    upper, upper_next = border[ring], border[following]
    lower, lower_next = skirt_start + ring, skirt_start + following
    faces.append(np.stack([upper, upper_next, lower], axis=1))
    faces.append(np.stack([upper_next, lower_next, lower], axis=1))

    # Cap the bottom with a fan so the solid is closed.
    centre = positions.shape[0]
    positions = np.concatenate(
        [positions, np.array([[0.0, base_y, 0.0]], dtype=np.float32)]
    )
    normals = np.concatenate(
        [normals, np.array([[0.0, -1.0, 0.0]], dtype=np.float32)]
    )
    fan_centre = np.full(len(border), centre, dtype=np.uint32)
    faces.append(np.stack([fan_centre, lower_next, lower], axis=1))

    indices = np.concatenate([face.ravel() for face in faces]).astype(np.uint32)
    return positions, normals, indices


# Rock, matching the arena's `style_palette_srgb.rock` closely enough that a
# crop does not read as a different substance from the terrain beside it.
CROP_BASE_COLOUR = (0.32, 0.31, 0.29, 1.0)
CROP_ROUGHNESS = 0.96


def write_glb(
    path: Path, positions: np.ndarray, normals: np.ndarray, indices: np.ndarray, name: str
) -> None:
    """Minimal binary glTF: one mesh, one primitive, one PBR material.

    The material belongs in the asset rather than in a consumer. A mesh with no
    material renders default white in Bevy -- and in Godot, Unity and Unreal --
    so leaving it out means every backend has to be told separately what a
    mountain is made of, and the first one that is not told renders a snowfield.
    """

    def pad(payload: bytes, filler: bytes) -> bytes:
        return payload + filler * (-len(payload) % 4)

    index_bytes = indices.tobytes()
    position_bytes = positions.tobytes()
    normal_bytes = normals.tobytes()
    buffer = pad(index_bytes, b"\0") + pad(position_bytes, b"\0") + normal_bytes
    index_offset = 0
    position_offset = len(pad(index_bytes, b"\0"))
    normal_offset = position_offset + len(pad(position_bytes, b"\0"))

    document = {
        "asset": {"version": "2.0", "generator": "luxel gaea_crop"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": name}],
        "materials": [
            {
                "name": "crop_rock",
                "doubleSided": False,
                "pbrMetallicRoughness": {
                    "baseColorFactor": list(CROP_BASE_COLOUR),
                    "metallicFactor": 0.0,
                    "roughnessFactor": CROP_ROUGHNESS,
                },
            }
        ],
        "meshes": [
            {
                "name": name,
                "primitives": [
                    {
                        "attributes": {"POSITION": 1, "NORMAL": 2},
                        "indices": 0,
                        "material": 0,
                    }
                ],
            }
        ],
        "buffers": [{"byteLength": len(buffer)}],
        "bufferViews": [
            {
                "buffer": 0,
                "byteOffset": index_offset,
                "byteLength": len(index_bytes),
                "target": 34963,
            },
            {
                "buffer": 0,
                "byteOffset": position_offset,
                "byteLength": len(position_bytes),
                "target": 34962,
            },
            {
                "buffer": 0,
                "byteOffset": normal_offset,
                "byteLength": len(normal_bytes),
                "target": 34962,
            },
        ],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": _UNSIGNED_INT,
                "count": int(indices.size),
                "type": "SCALAR",
            },
            {
                "bufferView": 1,
                "componentType": _FLOAT,
                "count": int(positions.shape[0]),
                "type": "VEC3",
                # Required by the spec for POSITION, and readers use it for
                # bounds without decoding the buffer.
                "min": [float(v) for v in positions.min(axis=0)],
                "max": [float(v) for v in positions.max(axis=0)],
            },
            {
                "bufferView": 2,
                "componentType": _FLOAT,
                "count": int(normals.shape[0]),
                "type": "VEC3",
            },
        ],
    }

    json_chunk = pad(
        json.dumps(document, separators=(",", ":"), sort_keys=True).encode("utf-8"),
        b" ",
    )
    binary_chunk = pad(buffer, b"\0")
    total = 12 + 8 + len(json_chunk) + 8 + len(binary_chunk)
    path.write_bytes(
        struct.pack("<III", 0x46546C67, 2, total)
        + struct.pack("<II", len(json_chunk), 0x4E4F534A)
        + json_chunk
        + struct.pack("<II", len(binary_chunk), 0x004E4942)
        + binary_chunk
    )


def crop_mountains(
    source: Path,
    output: Path,
    *,
    world_m: float,
    window_m: float,
    count: int,
    skirt_depth_m: float,
    stride: int = 1,
    edge_falloff: float = 0.28,
    resolution: int | None = None,
) -> dict:
    heights = load_heightfield(source, resolution)
    side = heights.shape[0]
    cell_m = world_m / (side - 1)
    window = max(int(round(window_m / cell_m)) | 1, 9)
    if window >= side:
        raise SystemExit(
            f"--window-m {window_m:g} is {window} cells, which does not fit in a "
            f"{side}^2 heightfield"
        )

    peaks = find_peaks(heights, window, count, separation=window // 2)
    output.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha256(source.read_bytes()).hexdigest()

    crops = []
    for order, (row, column, prominence) in enumerate(peaks):
        patch = taper_edges(
            crop_region(heights, row, column, window, stride), edge_falloff
        )
        positions, normals, indices = build_mesh(patch, cell_m * stride, skirt_depth_m)
        name = f"mountain_{order:02d}"
        write_glb(output / f"{name}.glb", positions, normals, indices, name)
        crops.append(
            {
                "name": name,
                "asset": f"{name}.glb",
                "source_cell": {"row": int(row), "column": int(column)},
                "prominence_m": round(prominence, 3),
                "relief_m": round(float(patch.max() - patch.min()), 3),
                "footprint_m": round((patch.shape[0] - 1) * cell_m * stride, 3),
                "vertices": int(positions.shape[0]),
                "triangles": int(indices.size // 3),
                "sha256": hashlib.sha256(
                    (output / f"{name}.glb").read_bytes()
                ).hexdigest(),
            }
        )

    manifest = {
        "schema_version": CROP_SCHEMA,
        "cropper_version": CROPPER_VERSION,
        # A crop is landform, never a certified world -- the same refusal
        # `import_heightfield.py` makes, for the same reason.
        "preview": True,
        "cropped_from": {
            "path": str(source),
            "sha256": digest,
            "world_m": world_m,
            "resolution": side,
            "cell_m": round(cell_m, 5),
        },
        "window_cells": window,
        "stride": stride,
        "skirt_depth_m": skirt_depth_m,
        "edge_falloff": edge_falloff,
        "crops": crops,
    }
    (output / "crop_manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("heightfield", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--world-m", type=float, default=1024.0, help="width of the source heightfield"
    )
    parser.add_argument(
        "--window-m", type=float, default=180.0, help="footprint of each crop"
    )
    parser.add_argument("--count", type=int, default=8, help="how many crops to cut")
    parser.add_argument(
        "--skirt-depth-m",
        type=float,
        default=40.0,
        help="how far the walls drop below the base, for burying under the skirt",
    )
    parser.add_argument(
        "--stride",
        type=int,
        default=1,
        help="decimate the crop by this factor; backdrop geometry rarely needs "
        "the source resolution",
    )
    parser.add_argument(
        "--edge-falloff",
        type=float,
        default=0.28,
        help="fraction of the radius over which the crop tapers to its floor, so "
        "no cut face is visible; 0 disables",
    )
    parser.add_argument("--resolution", type=int, help="only needed for raw .bin input")
    arguments = parser.parse_args()

    manifest = crop_mountains(
        arguments.heightfield,
        arguments.output,
        world_m=arguments.world_m,
        window_m=arguments.window_m,
        count=arguments.count,
        skirt_depth_m=arguments.skirt_depth_m,
        stride=arguments.stride,
        edge_falloff=arguments.edge_falloff,
        resolution=arguments.resolution,
    )
    print(
        "cut %d crops of %.0f m from %s"
        % (
            len(manifest["crops"]),
            manifest["crops"][0]["footprint_m"] if manifest["crops"] else 0.0,
            arguments.heightfield.name,
        )
    )
    for crop in manifest["crops"]:
        print(
            "  %s  relief %6.1f m  prominence %6.1f m  %6d tris"
            % (
                crop["name"],
                crop["relief_m"],
                crop["prominence_m"],
                crop["triangles"],
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
