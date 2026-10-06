#!/usr/bin/env python3
"""Turn a Gaea build into the km-scale backdrop (CONVERGE-4 N-3).

Landscape parity plan P2/P4 (`docs/world/landscape-parity-plan.md`).

Input: a committed spec (`tools/backdrop/<set>.json`) naming a Gaea graph, its
build settings and how the field is seated around the world. The Gaea outputs
are built once with `pipeline/gaea_build.py` into `artifacts/backdrop/<set>/gaea/`
and pinned by **pixel** digest in the lock: an eroded Gaea field is a source
artifact (Erosion2 is not reproducible between builds), so it is kept, not
rebuilt on demand.

Output: `artifacts/backdrop/<set>/backdrop.glb`, one mesh per tile, each with
its own baked albedo, and `backdrop.build.json`. The GLB digest and the Gaea
pixel digests are pinned in the committed `tools/backdrop/<set>.lock.json`; a
build that differs from the lock is refused unless run with `--pin`.

THE FIELD IS GENERATED WITHOUT REFERENCE TO THE PLAY SPACE (docs/integrations/gaea-programme.md
section 1), THEN THE PLAY SPACE IS SEATED IN IT. The seat is a point of the
field, chosen in the spec; it becomes the mesh origin, at the field's own
ground height there. Around the seat the field is pushed down (`sink_m` inside
`clear_radius_m`, easing back to natural height by `rise_radius_m`) so it can
never surface inside the near world, whose own extension and ridge hide it.
The field's outer margin eases down to the seat floor so no cut edge stands
against the sky.

Albedo is a material classification of the field, not lighting: forest below
the treeline on moderate slopes, alpine meadow and scree above it, rock on
cliffs, snow from Gaea's snow mask (when the graph exports one) and above the
snowline where the slope can hold it. The renderer lights it and the
atmosphere hazes it.

Usage:
    python3 tools/build_backdrop.py --set backdrop1 --gaea          # build the Gaea field (once)
    python3 tools/build_backdrop.py --set backdrop1                 # build and verify against the lock
    python3 tools/build_backdrop.py --set backdrop1 --pin           # build and (re)write the lock
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from types import SimpleNamespace

import numpy as np
from PIL import Image

TOOLS = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(TOOLS)
sys.path.insert(0, TOOLS)
sys.path.insert(0, os.path.join(REPO, "pipeline"))
from build_calibration_glb import Gltf, box_reduce, linear_to_srgb, png_bytes  # noqa: E402

SPEC_SCHEMA = "wge.backdrop-spec/v1"
LOCK_SCHEMA = "wge.backdrop-lock/v1"
BUILDER_VERSION = 1

# Linear albedo of each surface class. Chosen against scanned references of the
# same substances at distance: conifer canopy is very dark; alpine meadow is a
# dull yellow-green; limestone/gneiss rock a mid grey-brown; fresh snow ~0.8.
ALBEDO = {
    "forest": (0.030, 0.046, 0.026),
    "meadow": (0.105, 0.125, 0.060),
    "scree": (0.200, 0.190, 0.170),
    "rock": (0.150, 0.140, 0.128),
    "snow": (0.780, 0.800, 0.840),
}
ROUGHNESS = 0.95


def smoothstep(edge0, edge1, x):
    t = np.clip((x - edge0) / (edge1 - edge0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def sha256_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def load_spec(set_id: str) -> dict:
    path = os.path.join(TOOLS, "backdrop", f"{set_id}.json")
    spec = json.load(open(path))
    if spec.get("schema_version") != SPEC_SCHEMA:
        raise SystemExit(f"{path}: schema {spec.get('schema_version')} is not {SPEC_SCHEMA}")
    if spec.get("set_id") != set_id:
        raise SystemExit(f"{path}: set_id {spec.get('set_id')} does not match {set_id}")
    return spec


def gaea_dir(set_id: str) -> str:
    return os.path.join(REPO, "artifacts", "backdrop", set_id, "gaea")


def build_gaea(spec: dict) -> dict:
    """Build the spec's graph once. The outputs are a source artifact."""
    import gaea_build

    gaea = spec["gaea"]
    request = gaea_build.BuildRequest(
        graph=gaea_build.Path(os.path.join(REPO, gaea["graph"])),
        output_dir=gaea_build.Path(gaea_dir(spec["set_id"])),
        resolution=int(gaea["resolution"]),
        seed=gaea.get("seed"),
        timeout_s=float(gaea.get("timeout_s", 3600)),
    )
    return gaea_build.build(request)


def load_field(set_id: str, name: str) -> tuple[np.ndarray, dict] | None:
    """A Gaea output as float64 in [0, 1], with its pixel digest; None if absent."""
    import gaea_build

    path = os.path.join(gaea_dir(set_id), name)
    if not os.path.exists(path):
        return None
    digest = gaea_build.pixel_digest(gaea_build.Path(path))
    with Image.open(path) as image:
        array = np.asarray(image).astype(np.float64)
    full = 65535.0 if digest["mode"].startswith("I") else 255.0
    if array.ndim != 2 or array.shape[0] != array.shape[1]:
        raise SystemExit(f"{name} is not a square single-channel image")
    return array / full, digest


def to_odd_grid(field: np.ndarray) -> np.ndarray:
    """Bilinear resample a 2**k field to 2**k + 1, so it splits into tiles.

    Gaea builds at powers of two, and a 2**k-pixel field has 2**k - 1 cells,
    which no power-of-two tiling divides (the same trap docs/integrations/gaea-programme.md 3.1
    records for the viewer's stride). A one-row bilinear resample is cheap and
    auditable; asking Gaea for 2**k + 1 is not supported.
    """
    n = field.shape[0]
    if (n - 1) & (n - 2) == 0:  # already 2**k + 1
        return field
    target = n + 1
    coordinates = np.linspace(0.0, n - 1.0, target)
    low = np.floor(coordinates).astype(int).clip(0, n - 2)
    t = coordinates - low
    rows = field[low, :] * (1.0 - t)[:, None] + field[low + 1, :] * t[:, None]
    return rows[:, low] * (1.0 - t)[None, :] + rows[:, low + 1] * t[None, :]


def seat_height(height_m: np.ndarray, seat_rc: tuple[float, float], cell_m: float, radius_m: float = 250.0) -> float:
    """The field's ground at the seat: 10th percentile within `radius_m`.

    A percentile rather than the single cell, so one gully or boulder under the
    exact seat point cannot move the whole backdrop up or down.
    """
    row, col = seat_rc
    radius = max(int(round(radius_m / cell_m)), 1)
    r0, r1 = max(int(row) - radius, 0), min(int(row) + radius + 1, height_m.shape[0])
    c0, c1 = max(int(col) - radius, 0), min(int(col) + radius + 1, height_m.shape[1])
    return float(np.percentile(height_m[r0:r1, c0:c1], 10))


def seated_heights(spec: dict, height01: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, dict]:
    """Field heights in metres relative to the seat, with the seat-clearing and
    edge easing applied; the same heights without them (what the surface is
    classified on, so the easing's artificial slope never reads as cliff); the
    field-frame x/z of every cell; and facts.

    Mapping is full scale (`raw / 65535 * relief_m`), the same rule as
    `import_heightfield.py` (docs/integrations/gaea-programme.md 3.2): the denominator is the
    encoding's, so a graph edit that lowers the mountains lowers them here.
    """
    n = height01.shape[0]
    extent = float(spec["extent_m"])
    cell = extent / (n - 1)
    natural = height01 * float(spec["relief_m"])
    su, sv = spec["seat_uv"]
    seat_rc = (sv * (n - 1), su * (n - 1))
    floor = seat_height(natural, seat_rc, cell)

    axis = np.arange(n, dtype=np.float64) * cell
    x = axis[None, :] - su * extent
    z = axis[:, None] - sv * extent
    x, z = np.broadcast_to(x, (n, n)), np.broadcast_to(z, (n, n))
    radius = np.hypot(x, z)

    relative = natural - floor
    # Edge easing: within `edge_margin` of the field border, ease to the floor.
    margin = float(spec["edge_margin"])
    u = axis / extent
    edge_distance = np.minimum.outer(np.minimum(u, 1.0 - u), np.minimum(u, 1.0 - u))
    edge_weight = smoothstep(0.0, margin, edge_distance)
    relative = relative * edge_weight
    # Seat clearing: sunk under the near world, easing back out.
    sink = float(spec["sink_m"]) * (1.0 - smoothstep(float(spec["clear_radius_m"]), float(spec["rise_radius_m"]), radius))
    seated = relative - sink
    facts = {
        "cell_m": round(cell, 4),
        "seat_floor_m": round(floor, 3),
        "peak_above_seat_m": round(float(seated.max()), 1),
    }
    return seated, natural - floor, x, z, facts


def classify(spec: dict, altitude: np.ndarray, slope: np.ndarray, snow_mask: np.ndarray | None,
             wear: np.ndarray | None, deposits: np.ndarray | None) -> np.ndarray:
    """Linear albedo for every cell (H x W x 3). See the module docstring."""
    tree = float(spec["treeline_m"])
    snowline = float(spec["snowline_m"])
    colour = {k: np.asarray(v, np.float64) for k, v in ALBEDO.items()}

    forest = (1.0 - smoothstep(tree - 120.0, tree + 120.0, altitude)) * (1.0 - smoothstep(0.55, 0.85, slope))
    # Rock where scree and soil cannot stay: from ~40 deg, fully by ~52 deg.
    rock = smoothstep(0.85, 1.3, slope)
    scree = smoothstep(tree + 100.0, snowline, altitude)
    # Alpine snow holds to ~50 deg and sheds from steeper faces, which is what
    # leaves the dark rock ribs that make a range read as Alpine.
    snow = smoothstep(snowline - 250.0, snowline + 250.0, altitude) * (1.0 - smoothstep(1.0, 1.5, slope))
    if snow_mask is not None:
        snow = np.maximum(snow, snow_mask)
    snow = snow * (1.0 - 0.6 * rock)

    albedo = np.broadcast_to(colour["meadow"], altitude.shape + (3,)).copy()
    albedo = albedo + (colour["scree"] - albedo) * scree[..., None]
    albedo = albedo + (colour["forest"] - albedo) * forest[..., None]
    albedo = albedo + (colour["rock"] - albedo) * rock[..., None]
    # Erosion's own outputs, where the graph has them: worn channels darker,
    # deposited debris fans lighter. Bounded to +-15% so they vary the surface
    # rather than repaint it.
    if wear is not None:
        albedo = albedo * (1.0 - 0.15 * np.clip(wear / max(wear.max(), 1e-6), 0, 1))[..., None]
    if deposits is not None:
        albedo = albedo * (1.0 + 0.15 * np.clip(deposits / max(deposits.max(), 1e-6), 0, 1))[..., None]
    albedo = albedo + (colour["snow"] - albedo) * snow[..., None]
    return np.clip(albedo, 0.0, 1.0)


def tile_mesh(seated: np.ndarray, x: np.ndarray, z: np.ndarray, rows: slice, cols: slice, stride: int,
              yaw_deg: float) -> SimpleNamespace:
    """One tile as a grid mesh: positions, normals, UVs (0..1 across the tile)."""
    h = seated[rows, cols][::stride, ::stride]
    gx = x[rows, cols][::stride, ::stride]
    gz = z[rows, cols][::stride, ::stride]
    side = h.shape[0]
    spacing = float(gx[0, 1] - gx[0, 0])
    dz, dx = np.gradient(h, spacing)
    normals = np.stack([-dx, np.ones_like(h), -dz], axis=-1)
    normals /= np.linalg.norm(normals, axis=-1, keepdims=True)

    yaw = np.radians(yaw_deg)
    c, s = np.cos(yaw), np.sin(yaw)

    def rotate(vx, vz):  # about +Y, matching a glTF/WGE yaw quaternion
        return c * vx + s * vz, -s * vx + c * vz

    px, pz = rotate(gx, gz)
    nx, nz = rotate(normals[..., 0], normals[..., 2])
    positions = np.stack([px, h, pz], axis=-1).reshape(-1, 3)
    normal_rows = np.stack([nx, normals[..., 1], nz], axis=-1).reshape(-1, 3)
    u, v = np.meshgrid(np.linspace(0.0, 1.0, side), np.linspace(0.0, 1.0, side))
    uvs = np.stack([u, v], axis=-1).reshape(-1, 2)

    index = np.arange(side * side, dtype=np.uint32).reshape(side, side)
    a, b = index[:-1, :-1].ravel(), index[:-1, 1:].ravel()
    cc, d = index[1:, :-1].ravel(), index[1:, 1:].ravel()
    # Wound counter-clockwise seen from +Y (rows run +z, columns +x).
    triangles = np.concatenate([np.stack([a, cc, b], 1), np.stack([b, cc, d], 1)])
    return SimpleNamespace(p=positions, n=normal_rows, uv=uvs, i=triangles.reshape(-1))


def build(spec: dict) -> tuple[bytes, dict]:
    set_id = spec["set_id"]
    loaded = load_field(set_id, "Height_Out.png")
    if loaded is None:
        raise SystemExit(f"no Gaea build in {gaea_dir(set_id)}; run with --gaea first")
    height01, height_digest = loaded
    height01 = to_odd_grid(height01)
    sources = {"Height_Out.png": height_digest}
    optional = {}
    for name in spec.get("masks", []):
        found = load_field(set_id, name)
        if found is None:
            raise SystemExit(f"the spec uses {name}, which the Gaea build did not write")
        optional[name] = to_odd_grid(found[0])
        sources[name] = found[1]

    seated, natural, x, z, facts = seated_heights(spec, height01)
    n = seated.shape[0]
    dz, dx = np.gradient(natural, facts["cell_m"])
    slope = np.hypot(dx, dz)
    albedo = classify(spec, natural, slope, optional.get("Height_Snow.png"),
                      optional.get("Height_Wear.png"), optional.get("Height_Deposits.png"))

    tiles = int(spec["tiles"])
    cells = int(spec["cells_per_tile"])
    texture_px = int(spec["texture_px"])
    span = (n - 1) // tiles
    if span * tiles != n - 1 or span % cells:
        raise SystemExit(f"a {n}^2 field does not split into {tiles}x{tiles} tiles of {cells} cells")
    stride = span // cells
    if span % texture_px:
        raise SystemExit(f"a {span}-cell tile does not box-reduce to {texture_px} px")

    gltf = Gltf()
    gltf.doc["asset"]["generator"] = "wge build_backdrop.py"
    tile_facts = []
    for tile_row in range(tiles):
        for tile_col in range(tiles):
            rows = slice(tile_row * span, (tile_row + 1) * span + 1)
            cols = slice(tile_col * span, (tile_col + 1) * span + 1)
            name = f"backdrop_r{tile_row}c{tile_col}"
            mesh = tile_mesh(seated, x, z, rows, cols, stride, float(spec["yaw_deg"]))
            texels = albedo[tile_row * span:(tile_row + 1) * span, tile_col * span:(tile_col + 1) * span]
            texels = box_reduce(texels, texture_px)
            texture = gltf.texture(name, png_bytes(linear_to_srgb(texels)))
            material = gltf.material({
                "name": name,
                "pbrMetallicRoughness": {
                    "baseColorTexture": {"index": texture},
                    "metallicFactor": 0.0,
                    "roughnessFactor": ROUGHNESS,
                },
            })
            gltf.mesh(name, mesh, material)
            tile_facts.append({"name": name, "vertices": int(mesh.p.shape[0]), "triangles": int(mesh.i.size // 3)})

    glb = gltf.bytes()
    report = {
        "builder_version": BUILDER_VERSION,
        "set_id": set_id,
        "field": {"resolution": n, **facts},
        "tiles": tile_facts,
        "triangles": sum(t["triangles"] for t in tile_facts),
        "glb_bytes": len(glb),
        "sources": sources,
    }
    return glb, report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--set", required=True, dest="set_id")
    parser.add_argument("--gaea", action="store_true", help="build the Gaea field first (a source artifact)")
    parser.add_argument("--pin", action="store_true", help="write the lock instead of verifying against it")
    arguments = parser.parse_args()

    spec = load_spec(arguments.set_id)
    if arguments.gaea:
        receipt = build_gaea(spec)
        print(f"gaea: {len(receipt['outputs'])} outputs in {receipt['elapsed_s']} s -> {gaea_dir(spec['set_id'])}")

    glb, report = build(spec)
    out_rel = f"artifacts/backdrop/{spec['set_id']}"
    os.makedirs(os.path.join(REPO, out_rel), exist_ok=True)
    glb_rel = f"{out_rel}/backdrop.glb"
    with open(os.path.join(REPO, glb_rel), "wb") as handle:
        handle.write(glb)
    with open(os.path.join(REPO, out_rel, "backdrop.build.json"), "w") as handle:
        json.dump(report, handle, indent=2)

    spec_bytes = open(os.path.join(TOOLS, "backdrop", f"{spec['set_id']}.json"), "rb").read()
    lock = {
        "schema_version": LOCK_SCHEMA,
        "set_id": spec["set_id"],
        "spec_sha256": sha256_bytes(spec_bytes),
        "gaea_pixels": {name: digest["pixel_sha256"] for name, digest in sorted(report["sources"].items())},
        "glb": glb_rel,
        "bytes": len(glb),
        "sha256": sha256_bytes(glb),
    }
    lock_path = os.path.join(TOOLS, "backdrop", f"{spec['set_id']}.lock.json")
    if arguments.pin:
        with open(lock_path, "w") as handle:
            json.dump(lock, handle, indent=2)
            handle.write("\n")
        print("pinned", os.path.relpath(lock_path, REPO))
    else:
        if not os.path.exists(lock_path):
            raise SystemExit(f"no lock at {lock_path}; run with --pin")
        pinned = json.load(open(lock_path))
        if pinned != lock:
            changed = sorted(key for key in lock if pinned.get(key) != lock[key])
            raise SystemExit(f"backdrop build does not match the lock ({', '.join(changed)} differ); "
                             "rebuild with --pin only if the change is intended")
        print("verified against", os.path.relpath(lock_path, REPO))
    print(json.dumps({k: report[k] for k in ("field", "triangles", "glb_bytes")}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
