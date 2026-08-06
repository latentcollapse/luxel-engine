"""The navigation surface: where a body can actually go, and what connects.

MVP roadmap 1.3 (see WGE/docs/MVP_ROADMAP.md). `traversal_probe` already knows
how to decide whether ground is walkable, but only ever used it to pass or fail
a build. Nothing was emitted, so nothing downstream could path, spawn, or check
that the map's required routes exist.

**This does not bake a polygonal navmesh, deliberately.** Recast, Unity, Unreal
and Godot all bake one from collision geometry, and baking a second one here
would be undifferentiated work every backend then has to undo -- the same
argument that made `collision_plan` emit a heightfield descriptor instead of a
triangle soup. What none of those bakers can do is certify that the *game's*
required connections exist: that a keep can be left, that each lane runs end to
end, that the jungle touches the lane beside it. That certification is the part
only the compiler can produce, and it is what this emits.

What it emits instead of polygons is strictly more useful to a baker and to a
gate:

- a **walkable surface** mask: standable terrain minus everything solid
- a **clearance field** in metres -- distance to the nearest thing that stops
  you. One field answers the question for *any* agent radius, so a larger unit
  does not need the whole analysis recomputed, and 1.2's single hard-coded
  erosion stops being the only body the world is measured for
- **connected components**, so "reachable" is a fact rather than an assumption
- **resolved anchors**: where a keep can actually be stood next to, which is not
  where the keep is

**The gap this closes.** `boundary_plan` (1.2) flood-fills terrain slope only.
It does not read `collision_plan`, so buildings, keeps, rocks and trees do not
block it: 7.66% of the region it calls playable is inside something solid. A
navigation surface that inherited that would send units through walls.
"""

from __future__ import annotations

import hashlib
import json
import math
from collections import deque
from pathlib import Path
from typing import Any

import numpy as np

from boundary_plan import (
    KEEP_SEMANTIC,
    _seed_semantics,
    TARGET_CELL_M,
    _canonical_digest,
    _digest,
    standable,
)

SCHEMA_VERSION = "codeweald.navigation-plan/v1"

# Mask bit layout, mirroring `semantic_region_mask`'s convention so a reader
# already fluent in one is fluent in both.
NAV_WALKABLE_SURFACE = 1  # standable terrain, nothing solid on it
NAV_AGENT_FITS = 2  # clearance >= the spec's agent radius
NAV_PRIMARY_COMPONENT = 4  # connected to the first keep

# How far from a requested anchor we will look for somewhere to actually stand.
# A keep's anchor is the centre of the keep, which is inside the keep's own
# collider -- the resolved point is necessarily off it, and how far off is a
# number worth reporting rather than hiding.
ANCHOR_SEARCH_M = 48.0

_INFINITY = 1.0e20


class NavigationPlanError(ValueError):
    """The navigation surface cannot be built from these artifacts."""


def _distance_1d(row: np.ndarray) -> np.ndarray:
    """Exact 1D squared distance transform (Felzenszwalb & Huttenlocher)."""
    count = row.size
    envelope = np.zeros(count, dtype=np.int64)
    boundary = np.empty(count + 1)
    boundary[0], boundary[1] = -_INFINITY, _INFINITY
    top = 0
    for index in range(1, count):
        while True:
            previous = envelope[top]
            intersect = (
                (row[index] + index * index) - (row[previous] + previous * previous)
            ) / (2.0 * index - 2.0 * previous)
            if intersect > boundary[top]:
                break
            top -= 1
        top += 1
        envelope[top] = index
        boundary[top] = intersect
        boundary[top + 1] = _INFINITY
    result = np.empty(count)
    top = 0
    for index in range(count):
        while boundary[top + 1] < index:
            top += 1
        offset = index - envelope[top]
        result[index] = offset * offset + row[envelope[top]]
    return result


def clearance_m(walkable: np.ndarray, cell_m: float) -> np.ndarray:
    """Distance from each cell to the nearest thing that stops a body, in metres.

    A field rather than a yes/no mask because agent radius is a *parameter of
    the game*, not of the world. 1.2 eroded by one hard-coded radius, which
    silently made that one body the only body the world was ever measured for;
    a siege unit or a mount would have needed the whole analysis rerun. Here any
    radius is a comparison against this field.
    """
    field = np.where(walkable, _INFINITY, 0.0)
    for axis in (0, 1):
        field = np.apply_along_axis(_distance_1d, axis, field)
    return np.sqrt(field) * cell_m


def rasterize_colliders(
    collision_plan: dict[str, Any],
    resolution: int,
    width: float,
    length: float,
    heights: np.ndarray | None = None,
    max_climb_m: float = 0.0,
) -> np.ndarray:
    """Ground footprint of every instance collider that actually blocks a body.

    Boxes are oriented, so the world point is rotated *into* the box's frame
    rather than the box being grown into an axis-aligned one -- growing it would
    block ground beside a rotated building that a player can plainly walk on.

    **A body steps onto anything shorter than it can climb**, so a low slab is
    not an obstruction. This is a physical test, not a name list: a keep's
    bedrock plinth is a 34 m platform 0.9 m tall and its inner courtyard is
    0.26 m tall, against a 4 m climb limit. Both are floors. Deciding that by
    matching part names would be a guess dressed as a rule, wrong for the next
    kit; measuring the step is right for every kit.
    """
    columns = (np.arange(resolution) / (resolution - 1) - 0.5) * width
    rows = (0.5 - np.arange(resolution) / (resolution - 1)) * length
    x, z = np.meshgrid(columns, rows)
    blocked = np.zeros((resolution, resolution), dtype=bool)
    for collider in collision_plan.get("instance_colliders", []):
        # A deck carries traffic rather than stopping it. Subtracting one would
        # turn every bridge into a wall across the lane it exists to cross --
        # which is exactly what happened before `obstructs` existed.
        if not collider.get("obstructs", True):
            continue
        centre = collider["centre_m"]
        if heights is not None and collider["shape"] in ("box", "deck"):
            # Height of the part's top surface above the ground beneath it.
            top = centre[1] + float(collider["half_extents_m"][1])
            column = int(round((centre[0] / width + 0.5) * (resolution - 1)))
            row = int(round((0.5 - centre[2] / length) * (resolution - 1)))
            ground = float(
                heights[
                    min(max(row, 0), resolution - 1),
                    min(max(column, 0), resolution - 1),
                ]
            )
            if top - ground <= max_climb_m:
                continue
        offset_x, offset_z = x - centre[0], z - centre[2]
        shape = collider.get("shape")
        if shape == "cylinder":
            radius = float(collider["radius_m"])
            blocked |= offset_x * offset_x + offset_z * offset_z <= radius * radius
        elif shape == "box":
            # Inverse of collision_plan._rotate_y: world offset into box frame.
            angle = math.radians(float(collider["yaw_degrees"]))
            cos, sin = math.cos(angle), math.sin(angle)
            local_x = offset_x * cos - offset_z * sin
            local_z = offset_x * sin + offset_z * cos
            half = collider["half_extents_m"]
            blocked |= (np.abs(local_x) <= half[0]) & (np.abs(local_z) <= half[2])
        else:
            raise NavigationPlanError(
                "collider %s has unknown shape %r; navigation cannot guess "
                "whether it blocks" % (collider.get("id"), shape)
            )
    return blocked


def _flood(
    navigable: np.ndarray, heights: np.ndarray, seed: tuple[int, int], max_climb_m: float
) -> np.ndarray:
    reached = np.zeros_like(navigable)
    if not navigable[seed]:
        return reached
    reached[seed] = True
    queue: deque[tuple[int, int]] = deque([seed])
    rows, columns = navigable.shape
    while queue:
        row, column = queue.popleft()
        here = heights[row, column]
        for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            next_row, next_column = row + dr, column + dc
            if not (0 <= next_row < rows and 0 <= next_column < columns):
                continue
            if reached[next_row, next_column] or not navigable[next_row, next_column]:
                continue
            if abs(heights[next_row, next_column] - here) > max_climb_m:
                continue
            reached[next_row, next_column] = True
            queue.append((next_row, next_column))
    return reached


def _components(
    navigable: np.ndarray, heights: np.ndarray, max_climb_m: float
) -> tuple[np.ndarray, int]:
    labels = np.zeros(navigable.shape, dtype=np.int32)
    rows, columns = navigable.shape
    next_label = 0
    for row in range(rows):
        for column in range(columns):
            if not navigable[row, column] or labels[row, column]:
                continue
            next_label += 1
            labels[row, column] = next_label
            queue: deque[tuple[int, int]] = deque([(row, column)])
            while queue:
                r, c = queue.popleft()
                here = heights[r, c]
                for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nr, nc = r + dr, c + dc
                    if not (0 <= nr < rows and 0 <= nc < columns):
                        continue
                    if labels[nr, nc] or not navigable[nr, nc]:
                        continue
                    if abs(heights[nr, nc] - here) > max_climb_m:
                        continue
                    labels[nr, nc] = next_label
                    queue.append((nr, nc))
    return labels, next_label


def _polyline_samples(
    points: list[list[float]], spacing_m: float
) -> list[tuple[float, float]]:
    samples: list[tuple[float, float]] = []
    for start, finish in zip(points, points[1:]):
        dx = float(finish[0]) - float(start[0])
        dz = float(finish[1]) - float(start[1])
        steps = max(1, int(math.ceil(math.hypot(dx, dz) / spacing_m)))
        for step in range(steps):
            amount = step / steps
            samples.append((float(start[0]) + dx * amount, float(start[1]) + dz * amount))
    samples.append((float(points[-1][0]), float(points[-1][1])))
    return samples


def _polygon_centroid(points: list[list[float]]) -> tuple[float, float]:
    """Area-weighted centroid of a closed polygon (shoelace formula).

    Used only as a *requested* point for `resolve()`, which searches outward
    for the nearest navigable cell -- so a centroid that lands outside a
    concave polygon still resolves correctly, it just costs one extra search
    step. Exact interior placement is not needed for that reason.
    """
    area = 0.0
    cx = 0.0
    cz = 0.0
    ring = points if points[0] == points[-1] else points + [points[0]]
    for (x0, z0), (x1, z1) in zip(ring, ring[1:]):
        cross = x0 * z1 - x1 * z0
        area += cross
        cx += (x0 + x1) * cross
        cz += (z0 + z1) * cross
    area *= 0.5
    if abs(area) < 1e-9:
        # Degenerate polygon (collinear or zero-area points): fall back to the
        # plain average rather than divide by ~zero.
        xs = [float(p[0]) for p in points]
        zs = [float(p[1]) for p in points]
        return sum(xs) / len(xs), sum(zs) / len(zs)
    cx /= 6.0 * area
    cz /= 6.0 * area
    return cx, cz


def build(batch_dir: Path) -> tuple[dict[str, Any], np.ndarray, np.ndarray]:
    """Derive the navigation surface. Returns the plan, its mask, and clearance."""
    terrain_dir = batch_dir / "terrain"
    manifest_path = terrain_dir / "terrain_manifest.json"
    heightfield_path = terrain_dir / "heightfield_f32le.bin"
    zone_spec_path = batch_dir / "zone_spec.json"
    collision_path = batch_dir / "collision_plan.json"
    for required in (manifest_path, heightfield_path, zone_spec_path, collision_path):
        if not required.is_file():
            raise NavigationPlanError(
                "%s is missing; compile the batch first" % required
            )

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    zone_spec = json.loads(zone_spec_path.read_text(encoding="utf-8"))
    collision_plan = json.loads(collision_path.read_text(encoding="utf-8"))

    heightfield_digest = _digest(heightfield_path)
    if manifest.get("heightfield_sha256") != heightfield_digest:
        raise NavigationPlanError(
            "terrain manifest does not match the heightfield on disk; the "
            "terrain was rebuilt without rewriting its manifest"
        )
    if collision_plan.get("heightfield_sha256") != heightfield_digest:
        raise NavigationPlanError(
            "collision plan was built against a different heightfield; a "
            "navigation surface derived from both would route units through "
            "walls that have since moved"
        )
    canonical = _canonical_digest(zone_spec)
    if manifest.get("zone_spec_sha256") not in (None, canonical):
        raise NavigationPlanError(
            "terrain was built from a different zone spec"
        )

    resolution = int(manifest["resolution"])
    bounds = manifest["world_bounds_m"]
    width, length = float(bounds["width"]), float(bounds["length"])
    heights = np.fromfile(heightfield_path, dtype="<f4")
    if heights.size != resolution * resolution:
        raise NavigationPlanError(
            "heightfield has %d samples, manifest declares %d"
            % (heights.size, resolution * resolution)
        )
    heights = heights.reshape(resolution, resolution).astype(np.float64)

    policy = zone_spec.get("traversal_policy") or {}
    radius_m = float(policy.get("agent_radius_m", 2.5))
    max_climb_m = float(policy.get("agent_max_climb_m", 4.0))
    max_slope_degrees = float(policy.get("agent_max_slope_degrees", 45.0))

    source_cell = width / (resolution - 1)
    stride = max(1, int(round(TARGET_CELL_M / source_cell)))
    grid = heights[::stride, ::stride]
    cell_m = source_cell * stride
    side = grid.shape[0]

    # The gap 1.2 left open: terrain slope alone says nothing about the 177
    # solid things standing on it.
    blocked = rasterize_colliders(
        collision_plan, side, width, length, grid, max_climb_m
    )
    surface = standable(grid, cell_m, radius_m, max_slope_degrees) & ~blocked
    clearance = clearance_m(surface, cell_m)
    navigable = clearance >= radius_m

    labels, component_count = _components(navigable, grid, max_climb_m)

    def to_cell(x: float, z: float) -> tuple[int, int]:
        column = int(round((x / width + 0.5) * (side - 1)))
        row = int(round((0.5 - z / length) * (side - 1)))
        return min(max(row, 0), side - 1), min(max(column, 0), side - 1)

    def to_world(row: int, column: int) -> tuple[float, float]:
        return (
            (column / (side - 1) - 0.5) * width,
            (0.5 - row / (side - 1)) * length,
        )

    search_cells = int(math.ceil(ANCHOR_SEARCH_M / cell_m))

    def resolve(x: float, z: float) -> tuple[tuple[int, int] | None, float]:
        """Nearest navigable cell to a requested point, and how far off it is."""
        origin = to_cell(x, z)
        if navigable[origin]:
            return origin, 0.0
        best: tuple[int, int] | None = None
        best_distance = math.inf
        for dr in range(-search_cells, search_cells + 1):
            for dc in range(-search_cells, search_cells + 1):
                row, column = origin[0] + dr, origin[1] + dc
                if not (0 <= row < side and 0 <= column < side):
                    continue
                if not navigable[row, column]:
                    continue
                distance = math.hypot(dr, dc) * cell_m
                if distance < best_distance:
                    best, best_distance = (row, column), distance
        return best, best_distance

    anchors: list[dict[str, Any]] = []
    for feature in zone_spec.get("features", []):
        category = feature.get("category")
        if category not in ("landmark", "corridor", "biome"):
            continue
        points = (feature.get("geometry") or {}).get("points") or []
        if not points:
            continue
        if category == "corridor":
            candidates = [("start", points[0]), ("end", points[-1])]
        elif category == "biome":
            # A biome is a region, not a point -- its centroid is the single
            # representative location "reachable from an adjacent lane" means.
            candidates = [("anchor", list(_polygon_centroid(points)))]
        else:
            candidates = [("anchor", points[0])]
        for suffix, point in candidates:
            cell, displacement = resolve(float(point[0]), float(point[1]))
            resolved = to_world(*cell) if cell else None
            anchors.append(
                {
                    "id": feature["id"] if suffix == "anchor" else "%s:%s" % (feature["id"], suffix),
                    "feature_id": feature["id"],
                    "semantic": feature.get("semantic"),
                    "category": category,
                    "requested_m": [float(point[0]), float(point[1])],
                    "resolved_m": [round(value, 3) for value in resolved] if resolved else None,
                    # A keep's anchor sits inside the keep's own collider, so a
                    # nonzero displacement here is expected and load-bearing:
                    # 2.1 must place spawns at the resolved point, never the
                    # requested one.
                    "displacement_m": round(displacement, 3) if cell else None,
                    "component": int(labels[cell]) if cell else None,
                    "reachable": bool(cell),
                }
            )

    seed_semantics = _seed_semantics(zone_spec)
    keeps = [
        anchor
        for anchor in anchors
        if anchor["semantic"] in seed_semantics and anchor["component"]
    ]
    # The largest component that contains a keep, not simply the first keep's.
    #
    # Seeding from one keep was fragile in a way that only showed once keeps
    # gained per-part collision: the anchor resolved onto the newly-walkable
    # courtyard floor *inside* the curtain wall, so the entire map measured as
    # 22 m2 of navigable ground. Taking the largest keep-bearing component
    # keeps a sealed interior from swallowing the measurement, while
    # `disconnected_keeps` below still reports any keep that is not in it --
    # so the finding survives instead of the number collapsing.
    sizes = {
        component: int((labels == component).sum())
        for component in range(1, component_count + 1)
    }
    primary_component = max(sizes, key=sizes.get) if sizes else 0
    # The map's main body, not "wherever a keep happens to sit". Keeps outside
    # it are reported by `stranded_anchors` below, loudly, instead of dragging
    # the whole measurement into a pocket with them -- which is exactly what
    # happened when per-part collision opened the keep courtyards and revealed
    # both keeps are sealed inside their own curtain walls (D17). A metric that
    # collapses when it finds a defect reports the collapse, not the defect.
    primary = labels == primary_component if primary_component else np.zeros_like(navigable)

    unreachable = [anchor["id"] for anchor in anchors if not anchor["reachable"]]
    stranded = [
        anchor["id"]
        for anchor in anchors
        if anchor["reachable"] and anchor["component"] != primary_component
    ]

    lanes: list[dict[str, Any]] = []
    for feature in zone_spec.get("features", []):
        if feature.get("category") != "corridor":
            continue
        points = (feature.get("geometry") or {}).get("points") or []
        if len(points) < 2:
            continue
        samples = _polyline_samples(points, cell_m)
        flags = [bool(primary[to_cell(x, z)]) for x, z in samples]
        longest_gap, run = 0, 0
        for flag in flags:
            run = 0 if flag else run + 1
            longest_gap = max(longest_gap, run)
        # Name what is standing in the lane. A fraction alone tells an author
        # the lane is broken without telling them what to move -- the same
        # unactionable shape as the accessibility gate before 0.1.
        obstructions: list[dict[str, Any]] = []
        for collider in collision_plan.get("instance_colliders", []):
            if not collider.get("obstructs", True):
                continue
            centre = collider["centre_m"]
            # Must apply the same step-height test the raster does, or the
            # listing accuses parts that are not blocking anything -- naming a
            # 0.9 m plinth as the reason a lane is broken is evidence that lies.
            if collider["shape"] in ("box", "deck"):
                top = centre[1] + float(collider["half_extents_m"][1])
                if top - float(
                    grid[to_cell(centre[0], centre[2])]
                ) <= max_climb_m:
                    continue
            reach = (
                float(collider["radius_m"])
                if collider["shape"] == "cylinder"
                else math.hypot(
                    float(collider["half_extents_m"][0]),
                    float(collider["half_extents_m"][2]),
                )
            )
            gap = (
                min(
                    math.hypot(centre[0] - x, centre[2] - z) for x, z in samples
                )
                - reach
            )
            if gap < radius_m:
                obstructions.append(
                    {
                        "id": collider["id"],
                        "role": collider["role"],
                        "clearance_m": round(gap, 3),
                    }
                )
        obstructions.sort(key=lambda entry: entry["clearance_m"])

        properties = feature.get("properties") or {}
        lanes.append(
            {
                "id": feature["id"],
                "lane_id": properties.get("lane_id"),
                "obstructions": obstructions,
                # A lane is not a lane if a unit cannot walk it end to end. This
                # measures the authored centreline, which is what minions follow.
                "centreline_navigable_fraction": round(sum(flags) / len(flags), 6),
                "longest_impassable_run_m": round(longest_gap * cell_m, 3),
                "runs_end_to_end": longest_gap == 0,
            }
        )

    mask = np.zeros((side, side), dtype=np.uint8)
    mask[surface] |= NAV_WALKABLE_SURFACE
    mask[navigable] |= NAV_AGENT_FITS
    mask[primary] |= NAV_PRIMARY_COMPONENT

    plan = {
        "schema_version": SCHEMA_VERSION,
        "zone_id": manifest.get("zone_id"),
        "zone_spec_canonical_sha256": canonical,
        "zone_spec_bytes_sha256": _digest(zone_spec_path),
        "heightfield_sha256": heightfield_digest,
        # _bytes_ (T7 / D10): world_core/render_plan.rs's own fingerprint
        # block uses "terrain_manifest_sha256" for a canonical-JSON hash
        # inside a *different* artifact (render_plan.json) for its own
        # self-contained contract. Same key name, unrelated value, never
        # compared against this one -- named explicitly so nobody "fixes"
        # one to match the other.
        "terrain_manifest_bytes_sha256": _digest(manifest_path),
        "collision_plan_sha256": _digest(collision_path),
        "agent": {
            "radius_m": radius_m,
            "max_climb_m": max_climb_m,
            "max_slope_degrees": max_slope_degrees,
            "source": "zone_spec.traversal_policy",
        },
        "surface": {
            "mask": "terrain/navigation_mask.bin",
            "mask_encoding": "uint8_row_major_bitfield",
            "mask_bits": {
                "walkable_surface": NAV_WALKABLE_SURFACE,
                "agent_fits": NAV_AGENT_FITS,
                "primary_component": NAV_PRIMARY_COMPONENT,
            },
            # A field, not a mask: any agent radius is a comparison against it,
            # so a larger unit does not require re-deriving the whole surface.
            "clearance_field": "terrain/navigation_clearance_f32le.bin",
            "clearance_encoding": "float32_le_row_major_metres",
            "resolution": side,
            "cell_m": round(cell_m, 6),
            "walkable_area_m2": round(float(surface.sum()) * cell_m * cell_m, 3),
            "navigable_area_m2": round(float(navigable.sum()) * cell_m * cell_m, 3),
            "primary_area_m2": round(float(primary.sum()) * cell_m * cell_m, 3),
            "blocked_by_colliders_m2": round(float(blocked.sum()) * cell_m * cell_m, 3),
            "component_count": int(component_count),
        },
        "anchors": anchors,
        "lanes": lanes,
        "topology": {
            "primary_component": int(primary_component),
            "unreachable_anchors": unreachable,
            "stranded_anchors": stranded,
            # None, not False, when the world declares no player start at all:
            # a dungeon has no keeps to share a component, and reporting that
            # as a failure would make every non-MOBA world look broken.
            "keeps_share_one_component": (
                len({anchor["component"] for anchor in keeps}) <= 1 if keeps else None
            ),
            "all_lanes_run_end_to_end": bool(lanes)
            and all(lane["runs_end_to_end"] for lane in lanes),
        },
        # Stated so a reader does not go looking for one. See the module
        # docstring: every target engine bakes its own from collision geometry.
        "polygonal_navmesh": None,
    }
    return plan, mask, clearance


def write(batch_dir: Path) -> dict[str, Any]:
    plan, mask, clearance = build(batch_dir)
    mask.tofile(batch_dir / "terrain/navigation_mask.bin")
    clearance.astype("<f4").tofile(
        batch_dir / "terrain/navigation_clearance_f32le.bin"
    )
    return plan


def main(argv: list[str] | None = None) -> int:
    import argparse
    import sys

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("batch", type=Path)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args(argv)

    batch_dir = arguments.batch.resolve()
    try:
        plan = write(batch_dir)
    except (NavigationPlanError, OSError, ValueError) as exc:
        print("%s: %s" % (arguments.batch, exc), file=sys.stderr)
        return 2

    destination = arguments.output or (batch_dir / "navigation_plan.json")
    destination.write_text(
        json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    surface = plan["surface"]
    print(
        "navigation %s: %.0f m2 walkable, %.0f m2 fits the agent, %.0f m2 "
        "connected (%d components)"
        % (
            plan["zone_id"],
            surface["walkable_area_m2"],
            surface["navigable_area_m2"],
            surface["primary_area_m2"],
            surface["component_count"],
        )
    )
    for lane in plan["lanes"]:
        print(
            "  lane %-14s %-6s %5.1f%% navigable, longest gap %.1f m%s"
            % (
                lane["id"],
                lane["lane_id"] or "",
                lane["centreline_navigable_fraction"] * 100.0,
                lane["longest_impassable_run_m"],
                "" if lane["runs_end_to_end"] else "  <-- does not run end to end",
            )
        )
        for obstruction in lane["obstructions"][:4]:
            print(
                "      blocked by %-24s (%s) at %.1f m clearance"
                % (obstruction["id"], obstruction["role"], obstruction["clearance_m"])
            )
        if len(lane["obstructions"]) > 4:
            print("      ... and %d more" % (len(lane["obstructions"]) - 4))
    for anchor in plan["anchors"]:
        if anchor["displacement_m"]:
            print(
                "  anchor %-28s stands %.1f m off its requested point"
                % (anchor["id"], anchor["displacement_m"])
            )
    topology = plan["topology"]
    if topology["unreachable_anchors"]:
        print("  UNREACHABLE: %s" % ", ".join(topology["unreachable_anchors"]))
    if topology["stranded_anchors"]:
        print(
            "  STRANDED (not connected to the keeps): %s"
            % ", ".join(topology["stranded_anchors"])
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
