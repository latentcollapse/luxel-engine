"""Fracture the kit ruin's pieces and lay what broke off (CONVERGE-3 R-1).

`tools/build_kit.py --set kit2` calls `damage_piece` for each ruin piece in
`RUIN_DAMAGE`. The top of the piece is filled with a seeded Voronoi
tessellation, squashed vertically so its cells sit like courses; the cells
whose sites lie above an uneven break line (high at one end, descending toward
the other) break off. Headless Blender (`tools/blender_ruin_damage.py`)
subtracts them and returns each one intersected with the stone: that is the
debris, so every fallen piece is the shape of what it left behind.

Everything else happens here, in numpy:

* Faces of the source surface (on the standing wall and on the debris alike)
  keep their authored UVs and normals, transferred barycentrically from the
  source triangle they lie on, before anything moves; a fragment shows the
  masonry it carried.
* Fracture faces ("CUT") become stone: UVs box-projected at the wall's measured
  texel density, face normals.
* Debris lands outward from where it broke off, resting on its flattest axis,
  larger pieces nearer the wall, without overlapping. (`kit.rs` grounds each
  piece on the terrain under it.)

Deterministic: a seeded generator for sites, line and placement, and Blender's
exact boolean on the same inputs.
"""

import json
import os
import subprocess

import numpy as np

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Per piece: where sites go, how the break line runs and the seed. The line is
# a fraction of the piece's height along `axis_deg` (xz plane, from +x toward
# +z): `top` at the start of the axis, `bottom` at its end, curved by `shape`,
# with per-site noise. Spacing is the site grid (x, y, z metres); `squash`
# makes cells wider than tall. Authored against previews: tilted step boxes
# read as a saw blade and grid chunks as boxes (first review, 2026-10-05).
RUIN_DAMAGE = {
    "modular_fort_01_wall_thick_end_01": {
        "axis_deg": 30, "top": 1.02, "bottom": 0.57, "shape": 1.3, "noise_m": 0.25,
        "sites_y": [0.25, 0.98], "spacing": [1.3, 0.75, 1.3], "squash": [1.0, 1.6, 1.0],
        "seed": 7, "close_shell": True,
        # The gate stands 8 m along -z (ruin layout): nothing falls that way.
        "avoid_deg": [270, 60],
    },
    "modular_fort_01_wall_thin_gate_01": {
        # The far merlon and the parapet beside it gone; the near end intact.
        # The gate is not closed first: filling its open boundaries would seal
        # the arch passage, and its cells touch only the closed merlon/parapet.
        "axis_deg": 90, "top": 1.05, "bottom": 0.80, "shape": 3.0, "noise_m": 0.15,
        "sites_y": [0.66, 0.99], "spacing": [0.9, 0.55, 0.9], "squash": [1.0, 1.5, 1.0],
        "seed": 17, "close_shell": False,
        # The wall end stands 8 m along +z: the gate's debris falls to its sides.
        "avoid_deg": [90, 50],
    },
}
MIN_DEBRIS_VOLUME_M3 = 0.01
# A surviving face whose corners lie farther than this (metres, plus the
# outside-triangle penalty) from every source triangle is not source surface.
TRANSFER_TOLERANCE = 0.01
# |cross| (twice the area). Render conditioning refuses |cross| <= f32::EPSILON
# (1.2e-7) evaluated in f32 after placement; 2e-6 (a sub-millimetre sliver)
# keeps a margin for the rotation and the f32 rounding still to come.
MIN_TRIANGLE_CROSS_M2 = 2e-6
CUT = "CUT"


def write_obj(path, primitives, names):
    """Positions and UVs per source material. The UVs travel only so Blender's
    coplanar merge can respect UV islands (it would otherwise fuse quads that
    map to different atlas regions into one triangle: streaked trim); the
    final UVs are still transferred from the source by position."""
    with open(path, "w") as handle:
        base = 0
        for primitive in primitives:
            name = names[primitive["material"]]
            handle.write(f"o {name}\nusemtl {name}\n")
            for p in primitive["positions"]:
                handle.write("v %.6f %.6f %.6f\n" % tuple(p))
            for t in primitive["uvs"]:
                handle.write("vt %.6f %.6f\n" % (t[0], 1.0 - t[1]))
            for a, b, c in primitive["indices"].reshape(-1, 3) + 1 + base:
                handle.write(f"f {a}/{a} {b}/{b} {c}/{c}\n")
            base += len(primitive["positions"])


def read_obj(path):
    """Triangles as (object name, material, (3, 3) positions)."""
    vertices, triangles = [], []
    obj, material = None, None
    with open(path) as handle:
        for line in handle:
            parts = line.split()
            if not parts:
                continue
            if parts[0] == "v":
                vertices.append([float(x) for x in parts[1:4]])
            elif parts[0] == "o":
                obj = parts[1]
            elif parts[0] == "usemtl":
                material = parts[1]
            elif parts[0] == "f":
                corners = [int(c.split("/")[0]) - 1 for c in parts[1:]]
                for k in range(1, len(corners) - 1):
                    triangles.append((obj, material, np.array([vertices[corners[0]], vertices[corners[k]], vertices[corners[k + 1]]])))
    return triangles


def face_normal(tri):
    n = np.cross(tri[1] - tri[0], tri[2] - tri[0])
    length = np.linalg.norm(n)
    return n / length if length > 1e-12 else np.array([0.0, 1.0, 0.0])


class Transfer:
    """Barycentric transfer of UVs and normals from a primitive's triangles."""

    def __init__(self, primitive):
        tri = primitive["indices"].reshape(-1, 3)
        self.p = primitive["positions"][tri]
        self.n = primitive["normals"][tri]
        self.uv = primitive["uvs"][tri]
        e1, e2 = self.p[:, 1] - self.p[:, 0], self.p[:, 2] - self.p[:, 0]
        self.normal = np.cross(e1, e2)
        self.area2 = np.linalg.norm(self.normal, axis=1)
        self.unit = self.normal / np.maximum(self.area2, 1e-12)[:, None]

    def __call__(self, q, hint_normal):
        """(uv, normal) at point q on the surface whose face normal is `hint_normal`."""
        d = np.abs(np.einsum("tj,tj->t", self.unit, q - self.p[:, 0]))
        # Plane alignment, either orientation: Blender may flip faces of open
        # shells when it makes normals consistent; the transferred normal is
        # the source's own, so the flip does not reach shading.
        facing = np.abs(self.unit @ hint_normal)
        bary = self._barycentric(q)
        inside = (bary >= -1e-3).all(axis=1)
        score = d + 10.0 * (~inside) + 0.5 * (facing < 0.5)
        best = int(np.argmin(score))
        w = np.clip(bary[best], 0.0, 1.0)
        w = w / w.sum()
        normal = w @ self.n[best]
        return w @ self.uv[best], normal / max(np.linalg.norm(normal), 1e-12), float(score[best])

    def triangle(self, tri, hint_normal):
        """(uvs, normals, score) for a whole output triangle from ONE source
        triangle: the one under the centroid. Its affine map is applied to all
        three corners (barycentrics unclamped, so a corner just outside it
        extrapolates within the same UV island). Per-corner lookup let a corner
        on a UV seam take the neighbouring island's mapping, which streaked
        the trim across the atlas."""
        centroid = tri.mean(axis=0)
        d = np.abs(np.einsum("tj,tj->t", self.unit, centroid - self.p[:, 0]))
        facing = np.abs(self.unit @ hint_normal)
        bary_c = self._barycentric(centroid)
        inside = (bary_c >= -1e-3).all(axis=1)
        score = d + 10.0 * (~inside) + 0.5 * (facing < 0.5)
        best = int(np.argmin(score))
        uvs, normals, worst = [], [], float(score[best])
        for q in tri:
            w = self._barycentric(q)[best]
            # Corners stay on the source plane; their distance off it counts.
            worst = max(worst, abs(float(self.unit[best] @ (q - self.p[best, 0]))))
            uvs.append(w @ self.uv[best])
            n = np.clip(w, 0.0, 1.0)
            n = (n / n.sum()) @ self.n[best]
            normals.append(n / max(np.linalg.norm(n), 1e-12))
        return np.array(uvs), np.array(normals), worst

    def _barycentric(self, q):
        v0, v1 = self.p[:, 1] - self.p[:, 0], self.p[:, 2] - self.p[:, 0]
        v2 = q - self.p[:, 0]
        d00, d01, d11 = (v0 * v0).sum(1), (v0 * v1).sum(1), (v1 * v1).sum(1)
        d20, d21 = (v2 * v0).sum(1), (v2 * v1).sum(1)
        denom = np.where(np.abs(d00 * d11 - d01 * d01) > 1e-18, d00 * d11 - d01 * d01, 1e-18)
        v = (d11 * d20 - d01 * d21) / denom
        w = (d00 * d21 - d01 * d20) / denom
        return np.stack([1 - v - w, v, w], axis=1)


def box_uv(tri, normal, density):
    """Planar UVs on the face's dominant axis, at `density` UV units per metre."""
    axis = int(np.argmax(np.abs(normal)))
    keep = [i for i in range(3) if i != axis]
    return tri[:, keep] * density


def uv_density(primitive):
    """Median UV length per metre along the primitive's edges."""
    tri = primitive["indices"].reshape(-1, 3)
    p, t = primitive["positions"], primitive["uvs"]
    edges = np.linalg.norm(p[tri[:, 1]] - p[tri[:, 0]], axis=1)
    uvs = np.linalg.norm(t[tri[:, 1]] - t[tri[:, 0]], axis=1)
    ok = edges > 0.05
    return float(np.median(uvs[ok] / edges[ok]))


def rebuild(triangles, by_name, stone, density):
    """Primitives (one per material) from Blender triangles with transferred attributes."""
    transfers = {name: Transfer(p) for name, p in by_name.items()}
    out = {}
    worst, filled = 0.0, 0
    for _, material, tri in triangles:
        # Exact booleans leave zero-area slivers along cut lines; render
        # conditioning refuses them (as it does Blender collapse's).
        if np.linalg.norm(np.cross(tri[1] - tri[0], tri[2] - tri[0])) < MIN_TRIANGLE_CROSS_M2:
            continue
        normal = face_normal(tri)
        transferred = None
        if material != CUT and material in by_name:
            transferred = transfers[material].triangle(tri, normal)
            # A face no source triangle carries (one `holes_fill` made to close
            # the shell: an open end or the bottom) is new surface, like a cut.
            if transferred[2] > TRANSFER_TOLERANCE:
                transferred, filled = None, filled + 1
        if transferred is None:
            target, uvs, normals = stone, box_uv(tri, normal, density), np.repeat(normal[None], 3, axis=0)
        else:
            target = material
            uvs, normals = transferred[0], transferred[1]
            worst = max(worst, transferred[2])
        acc = out.setdefault(target, {"positions": [], "normals": [], "uvs": []})
        acc["positions"].extend(tri)
        acc["normals"].extend(normals)
        acc["uvs"].extend(uvs)
    primitives = {}
    for name, acc in out.items():
        count = len(acc["positions"])
        primitives[name] = {"material": by_name[name]["material"], "positions": np.array(acc["positions"]),
                            "normals": np.array(acc["normals"]), "uvs": np.array(acc["uvs"]),
                            "indices": np.arange(count)}
    return primitives, worst, filled


def volume(tris):
    return abs(sum(np.dot(t[0], np.cross(t[1], t[2])) for t in tris) / 6.0)


def closed(tris, tolerance=1e-4):
    """Every edge shared by exactly two triangles (positions quantised)."""
    key = lambda p: tuple(np.round(p / tolerance).astype(np.int64))
    edges = {}
    for t in tris:
        corners = [key(p) for p in t]
        for a, b in ((0, 1), (1, 2), (2, 0)):
            edge = tuple(sorted((corners[a], corners[b])))
            edges[edge] = edges.get(edge, 0) + 1
    return bool(edges) and all(count == 2 for count in edges.values())


def sites_and_removed(lo, hi, damage):
    """Jittered-grid Voronoi sites over the piece, and which break off."""
    rng = np.random.default_rng(damage["seed"])
    spacing = np.array(damage["spacing"])
    height = hi[1] - lo[1]
    y0, y1 = (lo[1] + f * height for f in damage["sites_y"])
    pad = 0.4
    sites = []
    for x in np.arange(lo[0] - pad, hi[0] + pad, spacing[0]):
        for y in np.arange(y0, y1, spacing[1]):
            for z in np.arange(lo[2] - pad, hi[2] + pad, spacing[2]):
                sites.append(np.array([x, y, z]) + rng.uniform(-0.35, 0.35, 3) * spacing)
    sites = np.array(sites)
    axis = np.radians(damage["axis_deg"])
    direction = np.array([np.cos(axis), 0.0, np.sin(axis)])
    along = (sites - (lo + hi) / 2) @ direction
    t = (along - along.min()) / max(along.max() - along.min(), 1e-9)
    line = lo[1] + height * (damage["top"] - (damage["top"] - damage["bottom"]) * t ** damage["shape"])
    line = line + rng.normal(0.0, damage["noise_m"], len(sites))
    removed = [int(i) for i in np.where(sites[:, 1] > line)[0]]
    return sites, removed, rng


def rest_pose(points, rng):
    """Rotation laying a piece on its flattest axis, with a random yaw and a
    few degrees of tilt."""
    centred = points - points.mean(axis=0)
    _, _, axes = np.linalg.svd(centred, full_matrices=False)
    thin = axes[2]                      # least extent: lie on it
    up = np.array([0.0, 1.0, 0.0])
    v = np.cross(thin, up)
    c = float(np.dot(thin, up))
    if np.linalg.norm(v) < 1e-9:
        lay = np.eye(3) if c > 0 else np.diag([1.0, -1.0, -1.0])
    else:
        k = np.array([[0, -v[2], v[1]], [v[2], 0, -v[0]], [-v[1], v[0], 0]])
        lay = np.eye(3) + k + k @ k * (1.0 / (1.0 + c))
    yaw = rng.uniform(0, 2 * np.pi)
    tilt, tilt_axis = np.radians(rng.uniform(0, 8)), rng.uniform(0, 2 * np.pi)
    a = np.array([np.cos(tilt_axis), 0.0, np.sin(tilt_axis)])
    ka = np.array([[0, -a[2], a[1]], [a[2], 0, -a[0]], [-a[1], a[0], 0]])
    tilt_m = np.eye(3) + np.sin(tilt) * ka + (1 - np.cos(tilt)) * ka @ ka
    yaw_m = np.array([[np.cos(yaw), 0, np.sin(yaw)], [0, 1, 0], [-np.sin(yaw), 0, np.cos(yaw)]])
    return yaw_m @ tilt_m @ lay


def land(pieces, lo, hi, rng, avoid=None):
    """Rigid placements (rotation, offset) for debris pieces, largest first:
    outward from where each broke off, past the foot, not overlapping."""
    centre = (lo + hi) / 2
    half = (hi - lo) / 2
    order = sorted(range(len(pieces)), key=lambda i: -pieces[i]["volume"])
    discs, placements = [], [None] * len(pieces)
    for rank, i in enumerate(order):
        points = pieces[i]["points"]
        rotation = rest_pose(points, rng)
        local = (points - points.mean(axis=0)) @ rotation.T
        radius = float(np.max(np.linalg.norm(local[:, [0, 2]], axis=1)))
        origin = points.mean(axis=0)
        outward = origin[[0, 2]] - centre[[0, 2]]
        outward = outward / max(np.linalg.norm(outward), 1e-9)
        for attempt in range(400):
            spread = np.radians(rng.normal(0.0, 25.0 + attempt * 0.2))
            c_, s_ = np.cos(spread), np.sin(spread)
            d = np.array([c_ * outward[0] - s_ * outward[1], s_ * outward[0] + c_ * outward[1]])
            if avoid is not None:
                # Directions within avoid[1] degrees of avoid[0] are blocked
                # (another piece of the ruin stands there).
                heading = np.degrees(np.arctan2(d[1], d[0])) % 360
                if abs((heading - avoid[0] + 180) % 360 - 180) < avoid[1]:
                    continue
            # From where the piece broke off (inside the footprint), along d to
            # the footprint's edge, then out: a heap at the foot below the break,
            # most pieces within a metre or so of the wall.
            start = np.clip(origin[[0, 2]], lo[[0, 2]], hi[[0, 2]])
            to_edge = min(((hi[0] if d[0] > 0 else lo[0]) - start[0]) / d[0] if abs(d[0]) > 1e-9 else np.inf,
                          ((hi[2] if d[1] > 0 else lo[2]) - start[1]) / d[1] if abs(d[1]) > 1e-9 else np.inf)
            fall = to_edge + radius * 0.6 + abs(rng.normal(0.0, 0.3 + 0.05 * (origin[1] - lo[1]))) + 0.01 * attempt
            x, z = start[0] + d[0] * fall, start[1] + d[1] * fall
            if all(np.hypot(x - dx, z - dz) > radius + dr + 0.05 for dx, dz, dr in discs):
                break
        discs.append((x, z, radius))
        offset = np.array([x, lo[1] - local[:, 1].min(), z])
        placements[i] = (rotation, origin, offset)
    return placements


def damage_piece(primitives, names, node, workdir):
    """(damaged primitives, debris primitives, report) for one ruin piece."""
    damage = RUIN_DAMAGE[node]
    allp = np.concatenate([p["positions"] for p in primitives])
    lo, hi = allp.min(axis=0), allp.max(axis=0)
    sites, removed, rng = sites_and_removed(lo, hi, damage)
    squash = damage["squash"]
    stem = os.path.join(workdir, node)
    write_obj(stem + ".obj", primitives, names)
    spec = {"input": stem + ".obj", "output": stem + ".damaged.obj", "debris": stem + ".debris.obj",
            "seeds": sites.tolist(), "removed": removed, "squash": squash,
            "bounds": [(lo - 1.0).tolist(), (hi + np.array([1.0, 1.5, 1.0])).tolist()],
            "neighbor_radius": float(2.6 * np.max(np.array(damage["spacing"]) * squash)),
            "close_shell": damage["close_shell"]}
    with open(stem + ".spec.json", "w") as handle:
        json.dump(spec, handle)
    subprocess.run(["blender", "-b", "--factory-startup", "--python",
                    os.path.join(REPO, "tools/blender_ruin_damage.py"), "--", stem + ".spec.json"],
                   check=True, capture_output=True)
    by_name = {names[p["material"]]: p for p in primitives}
    stone = next(name for name in by_name if name.endswith("wall_1"))
    density = uv_density(by_name[stone])
    damaged, worst, filled = rebuild(read_obj(spec["output"]), by_name, stone, density)
    # Debris: one object per broken-off cell; closed pieces with volume only.
    groups = {}
    for obj, material, tri in read_obj(spec["debris"]):
        groups.setdefault(obj, []).append((obj, material, tri))
    pieces = []
    for obj, triangles in sorted(groups.items()):
        tris = [t for _, _, t in triangles]
        if not closed(tris) or volume(tris) < MIN_DEBRIS_VOLUME_M3:
            continue
        built, _, _ = rebuild(triangles, by_name, stone, density)
        pieces.append({"name": obj, "primitives": built, "volume": volume(tris),
                       "points": np.concatenate([t for t in tris])})
    placements = land(pieces, lo, hi, rng, damage.get("avoid_deg"))
    debris = []
    report_debris = []
    for piece, (rotation, origin, offset) in zip(pieces, placements):
        for primitive in piece["primitives"].values():
            moved = dict(primitive)
            moved["positions"] = (primitive["positions"] - origin) @ rotation.T + offset
            moved["normals"] = primitive["normals"] @ rotation.T
            debris.append(moved)
        report_debris.append({"cell": piece["name"], "volume_m3": round(piece["volume"], 3),
                              "xz_m": [round(float(offset[0]), 3), round(float(offset[2]), 3)]})
    report = {"sites": len(sites), "removed_cells": len(removed), "transfer_worst_score": round(worst, 5),
              "filled_faces_as_stone": filled, "stone_uv_per_m": round(density, 4),
              "debris_pieces": len(pieces), "debris": report_debris,
              "removed_volume_m3": round(sum(p["volume"] for p in pieces), 3)}
    return list(damaged.values()), debris, report
