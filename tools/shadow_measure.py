#!/usr/bin/env python3
"""Shadow consistency with the sun, by distance (CONVERGE-4 L-1).

Question: does every object that should cast a shadow toward the sun's
direction actually darken the ground where the sun puts that shadow? A shadow
field that is "unified with the sun" answers yes at every distance; a single
view-fitted shadow map answers yes only inside its fitted distance.

Method, per tree (its meshes grouped by `kit-tree-sNNNN`):
  * crown centre C = the tree's world bound centre; ground under it from the
    terrain; the sun direction L from the packet's directional light;
  * predicted shadow point P = C + L * t, where the ray from C along L meets
    the terrain (marched against the same height field the renderer uses);
  * control point Q = the trunk base mirrored through P (same distance from
    the tree, away from the sun), so P and Q see the same ground material and
    lighting except for the shadow;
  * both projected to the capture; a sample is kept only if neither point is
    covered by any instance's projected hull (we must see bare ground) and both
    are inside the frame;
  * darkening = 1 - L(P) / L(Q) on linear luminance of a 2-px disc.

Reported per camera-distance band: trees sampled, median darkening, fraction
with darkening >= 0.15 ("shadow present"). A sun-consistent shadow field has a
similar shadow-present fraction in every band.

Usage: shadow_measure.py RUN_DIR [--view wide] [--json OUT]
"""
import argparse
import json
import math
import os
import re
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from atmosphere_measure import Terrain, camera_rays, linear_luminance, mesh_mask, read_ppm  # noqa: E402

BANDS_M = [(0, 30), (30, 60), (60, 120), (120, 250), (250, 1e9)]
PRESENT = 0.15
TREE = re.compile(r"^kit-tree-(s?\d+)-\d+$")


def quat_rotate(q, v):
    x, y, z, w = q
    u = np.array([x, y, z])
    t = 2.0 * np.cross(u, v)
    return v + w * t + np.cross(u, t)


def project(points, camera, basis):
    forward, right, up, tan_x, tan_y = basis
    w, h = camera["width_px"], camera["height_px"]
    rel = points - np.array(camera["position_xyz_m"], dtype=np.float64)
    depth = rel @ forward
    sx = ((rel @ right) / (depth * tan_x) + 1.0) * 0.5 * w
    sy = (1.0 - (rel @ up) / (depth * tan_y)) * 0.5 * h
    return sx, sy, depth


def ground_hit(origin, direction, terrain, max_m=200.0):
    """First terrain intersection along a ray (coarse march, then bisection)."""
    t_prev, t = 0.0, 0.25
    while t < max_m:
        p = origin + direction * t
        g, inside = terrain.height(np.array([p[0]]), np.array([p[2]]))
        if not inside[0]:
            return None
        if p[1] <= g[0]:
            lo, hi = t_prev, t
            for _ in range(20):
                mid = 0.5 * (lo + hi)
                q = origin + direction * mid
                gq, _ = terrain.height(np.array([q[0]]), np.array([q[2]]))
                lo, hi = (lo, mid) if q[1] <= gq[0] else (mid, hi)
            return origin + direction * hi
        t_prev, t = t, t + 0.25
    return None


def disc_luminance(lum, x, y, radius=2):
    h, w = lum.shape
    xi, yi = int(round(x)), int(round(y))
    if not (radius <= xi < w - radius and radius <= yi < h - radius):
        return None
    return float(lum[yi - radius:yi + radius + 1, xi - radius:xi + radius + 1].mean())


def measure(run_dir, view):
    packet = json.load(open(os.path.join(run_dir, view, "graphics_scene_packet.json")))["body"]
    camera = packet["camera"]
    lum = linear_luminance(read_ppm(os.path.join(run_dir, view, "native_capture.ppm")))
    _, basis = camera_rays(camera)
    covered = mesh_mask(packet, camera, basis, lum.shape, margin_px=2)
    terrain = Terrain(packet["terrain"])
    light = next(l for l in packet["lights"] if l["kind"]["kind"] == "directional")
    sun = np.array(light["kind"]["direction_xyz"], dtype=np.float64)
    sun /= np.linalg.norm(sun)

    meshes = {m["mesh_id"]: np.array(m["positions_m"], dtype=np.float64) for m in packet["meshes"]}
    trees = {}
    for inst in packet["instances"]:
        match = TREE.match(inst["instance_id"])
        if not match:
            continue
        tr = inst["transform"]
        pts = meshes[inst["mesh_id"]] * np.array(tr["scale_xyz"])
        if tr["rotation_xyzw"] != [0.0, 0.0, 0.0, 1.0]:
            pts = np.array([quat_rotate(tr["rotation_xyzw"], p) for p in pts[:: max(1, len(pts) // 400)]])
        trees.setdefault(match.group(1), []).append(pts + np.array(tr["translation_xyz_m"]))

    cam = np.array(camera["position_xyz_m"], dtype=np.float64)
    rows = []
    for key, parts in trees.items():
        pts = np.concatenate(parts)
        lo, hi = pts.min(axis=0), pts.max(axis=0)
        centre = 0.5 * (lo + hi)
        base = np.array([centre[0], lo[1], centre[2]])
        hit = ground_hit(centre, sun, terrain)
        if hit is None:
            continue
        control = base + (base - hit)
        control[1] = terrain.height(np.array([control[0]]), np.array([control[2]]))[0][0]
        sx, sy, depth = project(np.stack([hit, control]), camera, basis)
        if (depth <= camera["near_plane_m"]).any():
            continue
        h, w = lum.shape
        ok = True
        for x, y in zip(sx, sy):
            xi, yi = int(round(x)), int(round(y))
            if not (0 <= xi < w and 0 <= yi < h) or covered[yi, xi]:
                ok = False
        if not ok:
            continue
        lp, lq = disc_luminance(lum, sx[0], sy[0]), disc_luminance(lum, sx[1], sy[1])
        if lp is None or lq is None or lq <= 1e-6:
            continue
        rows.append((float(np.linalg.norm(base - cam)), 1.0 - lp / lq))

    bands = []
    for low, high in BANDS_M:
        values = [d for dist, d in rows if low <= dist < high]
        bands.append({
            "band_m": [low, None if high >= 1e9 else high],
            "trees": len(values),
            "median_darkening": round(float(np.median(values)), 3) if values else None,
            "shadow_present_fraction": round(float(np.mean([v >= PRESENT for v in values])), 3) if values else None,
        })
    return {
        "run": run_dir,
        "view": view,
        "sun_elevation_deg": round(math.degrees(math.asin(-sun[1])), 1),
        "shadow_fit": packet.get("render_policy", {}).get("shadow_fit"),
        "trees_total": len(trees),
        "trees_sampled": len(rows),
        "bands": bands,
    }


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
    return 0


if __name__ == "__main__":
    sys.exit(main())
