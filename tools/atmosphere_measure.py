#!/usr/bin/env python3
"""Atmospheric-depth measurement for CONVERGE-1 N-1 (WGE_CONVERGE1_CONTRACTS.md §1).

Acceptance (measured): "in the wide view, the backdrop ridge's luminance
contrast against the sky <= 40% of the foreground's contrast against terrain".

Method. The renderer's capture has no depth, so depth is reconstructed from the
packet itself: every pixel's camera ray (the same basis and NDC convention as
`LavaAdapter._sky_view_vertex`) is marched against the terrain height grid with
the renderer's own triangulation. Mesh instances are masked out by their
projected bounding spheres, so only terrain and sky pixels are measured.

An OCCLUSION EDGE is a vertical pixel pair where the upper pixel's surface is
>= 1.5x farther than the lower one's (sky counts as infinite). Contrast across
an edge is Michelson, |Ln - Lf| / (Ln + Lf), on linear luminance of a 4-px band
on each side (1-px gap, each band restricted to its own surface). Edges are
classed by the near side's distance:

  backdrop    near side >= 150 m, far side sky      (ridge against sky)
  foreground  near side <= 100 m, far side terrain  (a near hill against land)

The foreground cut was 60 m when first written; the converge0 wide view has no
terrain occlusion edge nearer than ~80 m (its nearest is the left hill at
80-100 m; everything closer is a mesh), so it was set to 100 m from the scene
geometry alone, before any converge1 frame was measured.

The same measurement on both classes means distance is the only variable.
Ratio = median(backdrop) / median(foreground). Contract: ratio <= 0.40.

CONVERGE-4 N-3: the km-scale backdrop is mesh instances (`backdrop-NN`), but
it is landform, and its skyline is exactly the "ridge against sky" this gate is
about. So backdrop instances are not masked: their world-space vertices are
splatted (max height per 40 m cell) into a second height grid, and rays march
against the higher of it and the terrain. A packet without a backdrop measures
exactly as before.

Usage: atmosphere_measure.py RUN_DIR [--view wide] [--json OUT]
"""
import argparse
import json
import math
import os
import sys

import numpy as np

NEAR_MAX_M = 100.0
FAR_MIN_M = 150.0
EDGE_RATIO = 1.5
GAP, BAND = 1, 4


def read_ppm(path):
    with open(path, "rb") as f:
        data = f.read()
    parts, pos = [], 0
    while len(parts) < 4:
        while data[pos:pos + 1].isspace():
            pos += 1
        if data[pos:pos + 1] == b"#":
            pos = data.index(b"\n", pos) + 1
            continue
        end = pos
        while not data[end:end + 1].isspace():
            end += 1
        parts.append(data[pos:end])
        pos = end
    pos += 1
    width, height = int(parts[1]), int(parts[2])
    return np.frombuffer(data[pos:pos + width * height * 3], dtype=np.uint8).reshape(height, width, 3)


def linear_luminance(rgb8):
    c = rgb8.astype(np.float64) / 255.0
    lin = np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)
    return 0.2126 * lin[..., 0] + 0.7152 * lin[..., 1] + 0.0722 * lin[..., 2]


def normalize(v):
    return v / np.linalg.norm(v)


def camera_rays(camera):
    forward = normalize(np.array(camera["forward_xyz"], dtype=np.float64))
    up_hint = normalize(np.array(camera["up_xyz"], dtype=np.float64))
    right = normalize(np.cross(forward, up_hint))
    up = normalize(np.cross(right, forward))
    w, h = camera["width_px"], camera["height_px"]
    tan_y = math.tan(math.radians(camera["projection"]["fov_y_degrees"]) * 0.5)
    tan_x = tan_y * w / h
    x = (np.arange(w) + 0.5) / w * 2.0 - 1.0
    y = (np.arange(h) + 0.5) / h * 2.0 - 1.0  # NDC y; top row is -1
    X, Y = np.meshgrid(x, y)
    rays = forward + X[..., None] * (right * tan_x) - Y[..., None] * (up * tan_y)
    rays /= np.linalg.norm(rays, axis=-1, keepdims=True)
    return rays, (forward, right, up, tan_x, tan_y)


class Terrain:
    """Height field with the renderer's triangulation (`_emit_terrain_vertex`):
    each cell splits along the (0,0)-(1,1) diagonal."""

    def __init__(self, terrain):
        self.n = terrain["resolution"]
        self.width = terrain["width_m"]
        self.length = terrain["length_m"]
        self.h = np.array(terrain["heights_m"]["payload"]["values"], dtype=np.float64).reshape(self.n, self.n)

    def height(self, wx, wz):
        cells = self.n - 1
        gx = (wx / self.width + 0.5) * cells
        gz = (0.5 - wz / self.length) * cells
        inside = (gx >= 0) & (gx <= cells) & (gz >= 0) & (gz <= cells)
        gx = np.clip(gx, 0, cells - 1e-6)
        gz = np.clip(gz, 0, cells - 1e-6)
        ix, iz = np.floor(gx).astype(int), np.floor(gz).astype(int)
        fx, fz = gx - ix, gz - iz
        h00 = self.h[iz, ix]
        h10 = self.h[iz, ix + 1]
        h11 = self.h[iz + 1, ix + 1]
        h01 = self.h[iz + 1, ix]
        lower = fx >= fz  # triangle (0,0),(1,0),(1,1)
        height = np.where(
            lower,
            h00 + fx * (h10 - h00) + fz * (h11 - h10),
            h00 + fz * (h01 - h00) + fx * (h11 - h01),
        )
        return height, inside


BACKDROP_PREFIX = "backdrop-"
BACKDROP_CELL_M = 40.0


def quat_rotate(q, v):
    x, y, z, w = q
    u = np.array([x, y, z])
    t = 2.0 * np.cross(u, v)
    return v + w * t + np.cross(u, t)


class BackdropField:
    """World-frame max-height grid splatted from the backdrop instances' vertices."""

    def __init__(self, packet):
        meshes = {m["mesh_id"]: m for m in packet["meshes"]}
        points = []
        for inst in packet["instances"]:
            if not inst["instance_id"].startswith(BACKDROP_PREFIX):
                continue
            pts = np.array(meshes[inst["mesh_id"]]["positions_m"], dtype=np.float64)
            tr = inst["transform"]
            pts = pts * np.array(tr["scale_xyz"])
            pts = np.array([quat_rotate(tr["rotation_xyzw"], p) for p in pts]) if tr["rotation_xyzw"] != [0.0, 0.0, 0.0, 1.0] else pts
            points.append(pts + np.array(tr["translation_xyz_m"]))
        self.empty = not points
        if self.empty:
            return
        pts = np.concatenate(points)
        self.x0, self.z0 = pts[:, 0].min(), pts[:, 2].min()
        nx = int((pts[:, 0].max() - self.x0) / BACKDROP_CELL_M) + 2
        nz = int((pts[:, 2].max() - self.z0) / BACKDROP_CELL_M) + 2
        grid = np.full((nz, nx), -np.inf)
        ix = ((pts[:, 0] - self.x0) / BACKDROP_CELL_M).round().astype(int)
        iz = ((pts[:, 2] - self.z0) / BACKDROP_CELL_M).round().astype(int)
        np.maximum.at(grid, (iz, ix), pts[:, 1])
        # Fill the few cells a rotated lattice leaves empty from their neighbours.
        for _ in range(3):
            empty = ~np.isfinite(grid)
            if not empty.any():
                break
            padded = np.pad(grid, 1, constant_values=-np.inf)
            neighbours = np.stack([padded[1 + dz:1 + dz + nz, 1 + dx:1 + dx + nx]
                                   for dz in (-1, 0, 1) for dx in (-1, 0, 1)])
            grid = np.where(empty, neighbours.max(axis=0), grid)
        self.grid = grid

    def height(self, wx, wz):
        nz, nx = self.grid.shape
        gx = (wx - self.x0) / BACKDROP_CELL_M
        gz = (wz - self.z0) / BACKDROP_CELL_M
        inside = (gx >= 0) & (gx <= nx - 1) & (gz >= 0) & (gz <= nz - 1)
        gx = np.clip(gx, 0, nx - 1 - 1e-6)
        gz = np.clip(gz, 0, nz - 1 - 1e-6)
        ix, iz = np.floor(gx).astype(int), np.floor(gz).astype(int)
        fx, fz = gx - ix, gz - iz
        h = (self.grid[iz, ix] * (1 - fx) + self.grid[iz, ix + 1] * fx) * (1 - fz) + \
            (self.grid[iz + 1, ix] * (1 - fx) + self.grid[iz + 1, ix + 1] * fx) * fz
        inside &= np.isfinite(h)
        return np.where(inside, h, -np.inf), inside


class Ground:
    """The terrain, raised to the backdrop wherever the backdrop is higher."""

    def __init__(self, terrain, backdrop):
        self.terrain, self.backdrop = terrain, backdrop

    def height(self, wx, wz):
        h, inside = self.terrain.height(wx, wz)
        if self.backdrop.empty:
            return h, inside
        b, b_inside = self.backdrop.height(wx, wz)
        h = np.where(inside, h, -np.inf)
        return np.maximum(h, b), inside | b_inside


def march(origin, rays, terrain, far_m):
    """Distance to the first terrain hit per ray, inf for sky."""
    shape = rays.shape[:2]
    d = rays.reshape(-1, 3)
    hit = np.full(d.shape[0], np.inf)
    active = np.arange(d.shape[0])
    t_prev = np.zeros(d.shape[0])
    t = np.full(d.shape[0], 0.1)
    while active.size:
        p = origin + d[active] * t[active, None]
        ground, inside = terrain.height(p[:, 0], p[:, 2])
        below = inside & (p[:, 1] <= ground)
        if below.any():
            idx = active[below]
            lo, hi = t_prev[idx].copy(), t[idx].copy()
            for _ in range(16):
                mid = 0.5 * (lo + hi)
                q = origin + d[idx] * mid[:, None]
                g, _ = terrain.height(q[:, 0], q[:, 2])
                under = q[:, 1] <= g
                hi = np.where(under, mid, hi)
                lo = np.where(under, lo, mid)
            hit[idx] = hi
        done = below | (t[active] >= far_m)
        t_prev[active] = t[active]
        t[active] = t[active] + np.maximum(0.05, 0.004 * t[active])
        active = active[~done]
    return hit.reshape(shape)


def _hull(points):
    """Convex hull of 2-D points (monotone chain), counter-clockwise."""
    # Pixel-rounded: the hull only needs pixel precision, and rounding first
    # cuts a 10k-vertex tree to a few hundred points.
    pts = np.unique(np.round(points), axis=0)
    if len(pts) < 3:
        return pts
    pts = pts[np.lexsort((pts[:, 1], pts[:, 0]))]

    def half(seq):
        out = []
        for p in seq:
            while len(out) >= 2 and (
                (out[-1][0] - out[-2][0]) * (p[1] - out[-2][1]) - (out[-1][1] - out[-2][1]) * (p[0] - out[-2][0])
            ) <= 0:
                out.pop()
            out.append(p)
        return out

    lower, upper = half(pts), half(pts[::-1])
    return np.array(lower[:-1] + upper[:-1])


def mesh_mask(packet, camera, basis, shape, margin_px=3):
    """Pixels covered by instance meshes, as each instance's projected convex hull.

    CONVERGE-4: the projected bounding sphere this used to draw covered 77-100%
    of the converge3/converge4 frames (a tree's sphere is mostly sky around its
    crown), leaving no foreground edges to measure. The hull of the projected
    vertices is still conservative (it covers every pixel the mesh can) but
    follows the silhouette. An instance with vertices behind the camera falls
    back to its sphere.
    """
    from PIL import Image, ImageDraw

    forward, right, up, tan_x, tan_y = basis
    h, w = shape
    origin = np.array(camera["position_xyz_m"], dtype=np.float64)
    meshes = {m["mesh_id"]: np.array(m["positions_m"], dtype=np.float64) for m in packet["meshes"]}
    canvas = Image.new("L", (w, h), 0)
    draw = ImageDraw.Draw(canvas)
    for inst in packet["instances"]:
        if inst["instance_id"].startswith(BACKDROP_PREFIX):
            continue  # landform: measured as depth (BackdropField), not masked
        pts = meshes.get(inst["mesh_id"])
        if pts is None or not len(pts):
            continue
        tr = inst["transform"]
        world = pts * np.array(tr["scale_xyz"])
        q = tr["rotation_xyzw"]
        if q != [0.0, 0.0, 0.0, 1.0]:
            u = np.array(q[:3])
            t = 2.0 * np.cross(u, world)
            world = world + q[3] * t + np.cross(u, t)
        rel = world + np.array(tr["translation_xyz_m"]) - origin
        depth = rel @ forward
        if depth.max() <= 0.1:
            continue
        if depth.min() <= 0.1:
            # Straddles the camera plane: project its bounding sphere instead.
            centre = rel.mean(axis=0)
            radius = float(np.max(np.linalg.norm(rel - centre, axis=1)))
            d = max(centre @ forward, 0.1)
            cx = ((centre @ right) / (d * tan_x) + 1.0) * 0.5 * w
            cy = (1.0 - (centre @ up) / (d * tan_y)) * 0.5 * h
            pr = radius / (d * tan_y) * 0.5 * h + margin_px
            draw.ellipse([cx - pr, cy - pr, cx + pr, cy + pr], fill=255)
            continue
        sx = ((rel @ right) / (depth * tan_x) + 1.0) * 0.5 * w
        sy = (1.0 - (rel @ up) / (depth * tan_y)) * 0.5 * h
        if sx.max() < -margin_px or sx.min() > w + margin_px or sy.max() < -margin_px or sy.min() > h + margin_px:
            continue
        hull = _hull(np.stack([sx, sy], axis=1))
        polygon = [tuple(p) for p in hull]
        if len(polygon) >= 3:
            draw.polygon(polygon, fill=255, outline=255, width=2 * margin_px)
        else:
            draw.line(polygon, fill=255, width=2 * margin_px)
    return np.asarray(canvas) > 0


def edges(depth, lum, mask):
    """Yield (near_m, far_m_or_inf, michelson) for every clean occlusion edge."""
    h, w = depth.shape
    out = []
    for col in range(w):
        d = depth[:, col]
        for r in range(GAP + BAND, h - GAP - BAND - 1):
            above, below = d[r], d[r + 1]
            if not (above >= EDGE_RATIO * below):
                continue
            far_rows = np.arange(r - GAP - BAND + 1, r - GAP + 1)
            near_rows = np.arange(r + 1 + GAP, r + 1 + GAP + BAND)
            if mask[far_rows, col].any() or mask[near_rows, col].any() or mask[r:r + 2, col].any():
                continue
            fd, nd = d[far_rows], d[near_rows]
            # Each band must stay on its own surface.
            if math.isinf(above):
                if not np.isinf(fd).all():
                    continue
            elif not (np.isfinite(fd).all() and (np.abs(fd - above) <= 0.2 * above).all()):
                continue
            if not (np.isfinite(nd).all() and (np.abs(nd - below) <= 0.2 * below).all()):
                continue
            ln, lf = lum[near_rows, col].mean(), lum[far_rows, col].mean()
            out.append((float(below), float(above), abs(ln - lf) / max(ln + lf, 1e-9)))
    return out


def measure(run_dir, view):
    packet = json.load(open(os.path.join(run_dir, view, "graphics_scene_packet.json")))["body"]
    camera = packet["camera"]
    if camera["projection"]["kind"] != "perspective":
        raise SystemExit("perspective camera required")
    rgb = read_ppm(os.path.join(run_dir, view, "native_capture.ppm"))
    if rgb.shape[:2] != (camera["height_px"], camera["width_px"]):
        raise SystemExit(f"capture {rgb.shape[:2]} does not match camera")
    rays, basis = camera_rays(camera)
    depth = march(np.array(camera["position_xyz_m"], dtype=np.float64), rays,
                  Ground(Terrain(packet["terrain"]), BackdropField(packet)), camera["far_plane_m"])
    mask = mesh_mask(packet, camera, basis, depth.shape)
    found = edges(depth, linear_luminance(rgb), mask)
    backdrop = [c for near, far, c in found if near >= FAR_MIN_M and math.isinf(far)]
    foreground = [c for near, far, c in found if near <= NEAR_MAX_M and math.isfinite(far)]
    result = {
        "run": run_dir,
        "view": view,
        "sky_fraction": float(np.isinf(depth).mean()),
        "mesh_mask_fraction": float(mask.mean()),
        "backdrop_edges": len(backdrop),
        "foreground_edges": len(foreground),
        "backdrop_contrast_median": float(np.median(backdrop)) if backdrop else None,
        "foreground_contrast_median": float(np.median(foreground)) if foreground else None,
    }
    if backdrop and foreground:
        result["ratio"] = result["backdrop_contrast_median"] / result["foreground_contrast_median"]
        result["pass_le_0_40"] = result["ratio"] <= 0.40
    return result


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir")
    ap.add_argument("--view", default="wide")
    ap.add_argument("--json")
    args = ap.parse_args()
    result = measure(args.run_dir, args.view)
    print(json.dumps(result, indent=2))
    if args.json:
        with open(args.json, "w") as f:
            json.dump(result, f, indent=2)
    if result.get("ratio") is None:
        print("insufficient edges in one class", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
