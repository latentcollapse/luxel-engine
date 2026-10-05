"""Rebuild a tree's leaf geometry as cluster cards (a billboard cloud).

Used by tools/build_kit.py for CONVERGE-2 N-5 (WGE_CONVERGE2_CONTRACTS.md §2).
A scanned tree carries millions of alpha-tested leaf triangles; the packet can
carry a few thousand. Thinning the leaves would leave a sparse tree with giant
leaves, so instead:

  1. the leaf triangles are grouped by a uniform spatial grid whose cell size
     grows until at most `max_clusters` cells are occupied (deterministic: no
     random seeds, clusters ordered by cell key);
  2. each cluster gets two crossed cards through its area-weighted centre: one
     facing away from the crown centre, one turned 90 degrees about the
     vertical, both square and sized to the cluster's projected extent;
  3. each card is a tile of one atlas, rasterised here in numpy from the real
     leaves (UV-mapped albedo and alpha, front-most fragment wins, 2x2
     supersampled so the tile's alpha is real coverage), darkened toward the
     crown centre as a cheap ambient occlusion;
  4. card vertex normals lean outward from the crown centre so the canopy
     shades as a volume, not as flat planes.

`silhouette_coverage` rasterises the original leaves and the cards from a view
and reports how much of the original alpha-tested silhouette the cards cover.
"""

import numpy as np

UP = np.array([0.0, 1.0, 0.0])
ALPHA_CUTOFF = 0.5


def _frame(normal):
    """Card axes (u, v, n): u horizontal, v roughly up, n the given normal."""
    n = normal / np.linalg.norm(normal)
    u = np.cross(UP, n)
    if np.linalg.norm(u) < 1e-6:
        u = np.array([1.0, 0.0, 0.0])
    u /= np.linalg.norm(u)
    v = np.cross(n, u)
    return u, v, n


def _sample(texture, uv):
    """Nearest texel of an (H, W, C) array at glTF UVs (v = 0 is the top row), wrapping."""
    h, w = texture.shape[:2]
    cols = np.floor(np.mod(uv[:, 0], 1.0) * w).astype(np.int64).clip(0, w - 1)
    rows = np.floor(np.mod(uv[:, 1], 1.0) * h).astype(np.int64).clip(0, h - 1)
    return texture[rows, cols]


def splat(tri_xyz, tri_uv, center, u, v, n, half, resolution, alpha, color=None, budget=4_000_000):
    """Rasterise triangles onto the square (center ± half·u ± half·v), seen from +n.

    Returns (covered[R, R] bool, rgb[R, R, 3] or None, world[R, R, 3]) for the
    front-most fragment whose alpha passes the cutoff."""
    r = resolution
    pixel = 2.0 * half / r
    rel = tri_xyz - center
    x = rel @ u
    y = rel @ v
    z = rel @ n
    # Pixel space: column from x, row from -y (image rows go down).
    px = (x + half) / pixel
    py = (half - y) / pixel
    best_depth = np.full(r * r, -np.inf)
    best_rgb = np.zeros((r * r, 3))
    best_world = np.zeros((r * r, 3))
    # Triangles are processed in groups of similar pixel span, each group's
    # (triangles x span x span) candidate grid kept under `budget` elements.
    spans = np.minimum(np.ceil(np.maximum(px.max(axis=1) - px.min(axis=1), py.max(axis=1) - py.min(axis=1))).astype(np.int64) + 2, r)
    by_span = np.argsort(spans, kind="stable")
    groups, start = [], 0
    while start < len(by_span):
        span = int(spans[by_span[min(start + 255, len(by_span) - 1)]])
        count = max(1, budget // (span * span))
        end = min(start + count, len(by_span))
        span = int(spans[by_span[end - 1]])
        groups.append((by_span[start:end], span))
        start = end
    for members, span in groups:
        ax, ay, az = px[members], py[members], z[members]
        lo_x = np.floor(ax.min(axis=1) - 0.5).astype(np.int64)
        lo_y = np.floor(ay.min(axis=1) - 0.5).astype(np.int64)
        offsets = np.arange(span)
        gx = lo_x[:, None, None] + offsets[None, None, :]
        gy = lo_y[:, None, None] + offsets[None, :, None]
        gx, gy = np.broadcast_arrays(gx, gy)
        cx, cy = gx + 0.5, gy + 0.5
        x0, x1, x2 = (ax[:, k, None, None] for k in range(3))
        y0, y1, y2 = (ay[:, k, None, None] for k in range(3))
        area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)
        valid = np.abs(area) > 1e-12
        safe = np.where(valid, area, 1.0)
        w1 = ((cx - x0) * (y2 - y0) - (x2 - x0) * (cy - y0)) / safe
        w2 = ((x1 - x0) * (cy - y0) - (cx - x0) * (y1 - y0)) / safe
        w0 = 1.0 - w1 - w2
        inside = valid & (w0 >= 0) & (w1 >= 0) & (w2 >= 0) & (gx >= 0) & (gy >= 0) & (gx < r) & (gy < r)
        t, iy, ix = np.nonzero(inside)
        if len(t) == 0:
            continue
        t_abs = members[t]
        b0, b1, b2 = w0[t, iy, ix], w1[t, iy, ix], w2[t, iy, ix]
        uv = (tri_uv[t_abs, 0] * b0[:, None] + tri_uv[t_abs, 1] * b1[:, None] + tri_uv[t_abs, 2] * b2[:, None])
        keep = _sample(alpha, uv) >= ALPHA_CUTOFF
        t, t_abs, uv = t[keep], t_abs[keep], uv[keep]
        b0, b1, b2 = b0[keep], b1[keep], b2[keep]
        pixels = (gy[t, iy[keep], ix[keep]] * r + gx[t, iy[keep], ix[keep]])
        depth = az[t, 0] * b0 + az[t, 1] * b1 + az[t, 2] * b2
        world = (tri_xyz[t_abs, 0] * b0[:, None] + tri_xyz[t_abs, 1] * b1[:, None] + tri_xyz[t_abs, 2] * b2[:, None])
        # Front-most per pixel within the chunk, then against the running best.
        order = np.lexsort((-depth, pixels))
        first = np.ones(len(order), bool)
        first[1:] = pixels[order][1:] != pixels[order][:-1]
        pick = order[first]
        pix = pixels[pick]
        closer = depth[pick] > best_depth[pix]
        pix, pick = pix[closer], pick[closer]
        best_depth[pix] = depth[pick]
        best_world[pix] = world[pick]
        if color is not None:
            best_rgb[pix] = _sample(color, uv[pick])
    covered = np.isfinite(best_depth).reshape(r, r)
    rgb = best_rgb.reshape(r, r, 3) if color is not None else None
    return covered, rgb, best_world.reshape(r, r, 3)


def _clusters(centroids, max_clusters):
    size = 0.25
    while True:
        keys = np.floor(centroids / size).astype(np.int64)
        unique, inverse = np.unique(keys, axis=0, return_inverse=True)
        if len(unique) <= max_clusters:
            return inverse.reshape(-1), len(unique), size
        size *= 1.1


def bake(positions, uvs, indices, albedo_linear, alpha, crown_center, crown_radius,
         max_clusters=128, tile=32, supersample=2, atlas_size=512):
    """Cards for the leaf triangles. Returns (card_positions, card_normals,
    card_uvs, card_indices, atlas_rgba_linear[atlas, atlas, 4], report)."""
    tri_xyz = positions[indices.reshape(-1, 3)]
    tri_uv = uvs[indices.reshape(-1, 3)]
    centroids = tri_xyz.mean(axis=1)
    areas = 0.5 * np.linalg.norm(np.cross(tri_xyz[:, 1] - tri_xyz[:, 0], tri_xyz[:, 2] - tri_xyz[:, 0]), axis=1)
    cluster, count, cell = _clusters(centroids, max_clusters)
    per_row = atlas_size // tile
    if 2 * count > per_row * per_row:
        raise SystemExit(f"{2 * count} cards do not fit a {atlas_size}px atlas of {tile}px tiles")
    atlas = np.zeros((atlas_size, atlas_size, 4))
    card_p, card_n, card_uv, card_i = [], [], [], []
    order = np.argsort(cluster, kind="stable")
    bounds = np.searchsorted(cluster[order], np.arange(count + 1))
    resolution = tile * supersample
    slot = 0
    for c in range(count):
        members = order[bounds[c]:bounds[c + 1]]
        weights = areas[members]
        center = (centroids[members] * weights[:, None]).sum(axis=0) / max(weights.sum(), 1e-12)
        radial = center - crown_center
        radial = radial / np.linalg.norm(radial) if np.linalg.norm(radial) > 1e-6 else np.array([1.0, 0.0, 0.0])
        crossed = np.cross(UP, radial)
        crossed = crossed / np.linalg.norm(crossed) if np.linalg.norm(crossed) > 1e-6 else np.array([0.0, 0.0, 1.0])
        for normal in (radial, crossed):
            u, v, n = _frame(normal)
            rel = tri_xyz[members].reshape(-1, 3) - center
            half = max(np.abs(rel @ u).max(), np.abs(rel @ v).max()) * 1.02
            covered, rgb, world = splat(tri_xyz[members], tri_uv[members], center, u, v, n, half,
                                        resolution, alpha, albedo_linear)
            # Ambient occlusion toward the crown centre.
            depth_in = np.linalg.norm(world - crown_center, axis=-1) / crown_radius
            rgb = rgb * np.clip(0.65 + 0.35 * depth_in, 0.65, 1.0)[..., None]
            cov = covered.reshape(tile, supersample, tile, supersample).mean(axis=(1, 3))
            premult = (rgb * covered[..., None]).reshape(tile, supersample, tile, supersample, 3).sum(axis=(1, 3))
            hits = covered.reshape(tile, supersample, tile, supersample).sum(axis=(1, 3))
            tile_rgb = premult / np.maximum(hits, 1)[..., None]
            if hits.sum():
                # Bleed the mean leaf colour into empty texels so mips do not fringe dark.
                mean = premult.sum(axis=(0, 1)) / hits.sum()
                tile_rgb[hits == 0] = mean
            row, col = divmod(slot, per_row)
            atlas[row * tile:(row + 1) * tile, col * tile:(col + 1) * tile, :3] = tile_rgb
            atlas[row * tile:(row + 1) * tile, col * tile:(col + 1) * tile, 3] = cov
            # Card quad; UVs inset half a texel so filtering stays in the tile.
            inset = 0.5 / atlas_size
            u0, v0 = col * tile / atlas_size + inset, row * tile / atlas_size + inset
            u1, v1 = (col + 1) * tile / atlas_size - inset, (row + 1) * tile / atlas_size - inset
            corners = [center - half * u + half * v, center + half * u + half * v,
                       center + half * u - half * v, center - half * u - half * v]
            corner_uv = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)]
            base = len(card_p)
            for corner, corner_uv_ in zip(corners, corner_uv):
                outward = corner - crown_center
                outward = outward / max(np.linalg.norm(outward), 1e-6)
                shade = outward + 0.5 * n
                card_p.append(corner)
                card_n.append(shade / np.linalg.norm(shade))
                card_uv.append(corner_uv_)
            # Wind so the front face looks along +n.
            card_i.extend([base, base + 2, base + 1, base, base + 3, base + 2])
            slot += 1
    report = {"leaf_triangles": int(len(tri_xyz)), "clusters": int(count), "cards": slot,
              "cell_m": round(float(cell), 4), "tile_px": tile, "atlas_px": atlas_size}
    return (np.asarray(card_p), np.asarray(card_n), np.asarray(card_uv), np.asarray(card_i, np.int64),
            atlas, report)


def silhouette_coverage(leaf_xyz, leaf_uv, leaf_alpha, card_xyz, card_uv, card_alpha, center, half, normal, resolution=256):
    """Fraction of the original alpha-tested silhouette the cards cover, from
    one view direction, plus the cards' area relative to the original."""
    u, v, n = _frame(normal)
    original, _, _ = splat(leaf_xyz, leaf_uv, center, u, v, n, half, resolution, leaf_alpha)
    cards, _, _ = splat(card_xyz, card_uv, center, u, v, n, half, resolution, card_alpha)
    covered = (original & cards).sum() / max(original.sum(), 1)
    return float(covered), float(cards.sum() / max(original.sum(), 1))
