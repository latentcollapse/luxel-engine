"""Where a body may go, and what the world looks like past it.

MVP roadmap 1.2 (see Luxel/docs/archive/2026-09_roadmaps-and-audits/mvp-roadmap.md). Luxel emits a heightfield that
stops dead at the world bounds: the terrain mesh ends at the data edge and
beyond it is skybox. That is two separate defects wearing one costume.

**The player falls off.** Nothing in any emitted artifact says where a body may
stand. `world_bounds_m` is the extent of the *data*, and the compiler has been
treating it as the extent of the *game* by omission.

**The horizon ends in void.** Even a world that contains its players still has
to look like it continues, or every outward view reads as a diorama on a table.

This module refuses to invent the answer to the first. It *measures* it: flood
fill the world with the zone spec's own declared agent, from the keeps, and see
where that agent can get. The reachable set is the playable region -- not a
number somebody picked. If that set touches the data edge, the world does not
contain its players, and this says so with the spans that leak rather than
quietly fencing them.

That distinction is the whole point. A synthetic barrier over a leak would make
the map playable today and make the defect invisible forever, which is exactly
the failure mode Luxel exists to catch. Roadmap 2.5 closes these leaks with
landform -- geometry is the boundary -- and this artifact is what tells 2.5
where to put it.

The apron is a descriptor, not a mesh, matching `collision_plan`'s treatment of
terrain: the renderer and every backend build it from the same declared rule, so
there is one apron and not one per consumer. It is explicitly non-colliding --
an apron a body could walk onto would extend the very leak this module reports.
"""

from __future__ import annotations

import hashlib
import json
import math
from collections import deque
from pathlib import Path
from typing import Any

import numpy as np

SCHEMA_VERSION = "codeweald.boundary-plan/v1"

# Cell size for the reachability fill. Finer than the agent by enough that
# erosion by the agent radius is not quantised into uselessness, coarse enough
# that the fill stays cheap. This is a resolution knob, not a semantic one --
# what an agent can squeeze through is set by the erosion below, not by this.
TARGET_CELL_M = 1.0

# How far the apron extends past the world bounds, as a multiple of the world
# diagonal. One diagonal guarantees that from the far corner of the world,
# looking outward, there is still at least a full world's width of land before
# the apron itself ends.
# Overridable per world via `zone.apron_extent_diagonals`. Half a diagonal
# still puts a full half-world of land beyond the far corner before the apron
# itself ends, which is past anything a normal play camera sees, while a full
# diagonal made a 256 m map sit in the middle of a ~980 m plate.
APRON_EXTENT_DIAGONALS = 0.5

# How steeply the apron falls away, as a grade. Land receding toward a horizon
# drops a few degrees; this is ~3.4.
#
# It used to fall by the world's entire *height range*, which on a map whose
# rim sits near 0 m but whose peaks reach 51 m meant dropping a full mountain's
# height from a near-sea-level edge -- a 16.6 degree face all the way round, so
# the world read as a pyramid sitting on a plate. The bug was there at every
# extent; halving the apron is what made it visible.
APRON_FALLOFF_GRADE = 0.06

# Semantics that mark "somewhere a player is known to start". Wherever a player
# starts is by definition inside the playable region, so these seed the fill.
#
# Luxel is a general-purpose world compiler, not a MOBA tool. This list used to be
# the single value "faction_keep" and the build *refused* a world without one --
# so a dungeon, a race track, or an open-world zone could not compile at all.
# A world with no declared start is still a world; it falls back to its largest
# standable region and says so in `playable.derivation`.
KEEP_SEMANTIC = "faction_keep"
SEED_SEMANTICS = (KEEP_SEMANTIC, "spawn", "player_start", "team_spawn", "start_area")


class BoundaryPlanError(ValueError):
    """The boundary plan cannot be built from these artifacts."""


def _digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _canonical_digest(document: Any) -> str:
    """The terrain manifest's notion of a zone-spec digest: canonicalised JSON."""
    return hashlib.sha256(
        json.dumps(
            document, ensure_ascii=False, separators=(",", ":"), sort_keys=True
        ).encode("utf-8")
    ).hexdigest()


def _disc_offsets(radius_cells: float) -> list[tuple[int, int]]:
    """Integer offsets whose centres lie within `radius_cells` of the origin."""
    limit = int(math.floor(radius_cells))
    return [
        (dr, dc)
        for dr in range(-limit, limit + 1)
        for dc in range(-limit, limit + 1)
        if math.hypot(dr, dc) <= radius_cells
    ]


def _erode(mask: np.ndarray, radius_cells: float) -> np.ndarray:
    """Cells whose whole agent-radius disc is inside `mask`.

    Off-grid counts as outside, so this also keeps a body's centre at least one
    agent radius clear of the data edge -- a body standing exactly on the last
    row is already half off the world.
    """
    eroded = mask.copy()
    for dr, dc in _disc_offsets(radius_cells):
        if dr == 0 and dc == 0:
            continue
        shifted = np.zeros_like(mask)
        rows, columns = mask.shape
        source_rows = slice(max(0, dr), rows + min(0, dr))
        target_rows = slice(max(0, -dr), rows + min(0, -dr))
        source_columns = slice(max(0, dc), columns + min(0, dc))
        target_columns = slice(max(0, -dc), columns + min(0, -dc))
        shifted[target_rows, target_columns] = mask[source_rows, source_columns]
        eroded &= shifted
    return eroded


def _shift(values: np.ndarray, dr: int, dc: int) -> np.ndarray:
    """Shift by (dr, dc), repeating the edge rather than wrapping around it.

    `np.roll` would make a cell on the north edge sample the *south* edge of the
    world for its slope ring -- a cliff on one border silently making the
    opposite border unstandable.
    """
    rows, columns = values.shape
    row_index = np.clip(np.arange(rows) + dr, 0, rows - 1)
    column_index = np.clip(np.arange(columns) + dc, 0, columns - 1)
    return values[np.ix_(row_index, column_index)]


def standable(
    heights: np.ndarray, cell_m: float, radius_m: float, max_slope_degrees: float
) -> np.ndarray:
    """Cells flat enough to stand on, by the same measure `traversal_probe` uses.

    Local grade is the largest height difference to a ring of samples one agent
    radius away, divided by that radius -- an agent is stopped by the steepest
    thing under its feet, not by the average.
    """
    ring_cells = max(1, int(round(radius_m / cell_m)))
    span = ring_cells * cell_m
    grade = np.zeros_like(heights)
    for index in range(8):
        angle = math.tau * index / 8.0
        dr = int(round(-math.sin(angle) * ring_cells))
        dc = int(round(math.cos(angle) * ring_cells))
        grade = np.maximum(grade, np.abs(_shift(heights, dr, dc) - heights) / span)
    return grade <= math.tan(math.radians(max_slope_degrees))


def _flood(
    passable: np.ndarray,
    heights: np.ndarray,
    seeds: list[tuple[int, int]],
    max_climb_m: float,
) -> np.ndarray:
    reached = np.zeros_like(passable)
    queue: deque[tuple[int, int]] = deque()
    rows, columns = passable.shape
    for seed in seeds:
        if passable[seed] and not reached[seed]:
            reached[seed] = True
            queue.append(seed)
    while queue:
        row, column = queue.popleft()
        here = heights[row, column]
        for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            next_row, next_column = row + dr, column + dc
            if not (0 <= next_row < rows and 0 <= next_column < columns):
                continue
            if reached[next_row, next_column] or not passable[next_row, next_column]:
                continue
            # A step taller than the agent can climb is a wall, however flat the
            # ground is on either side of it.
            if abs(heights[next_row, next_column] - here) > max_climb_m:
                continue
            reached[next_row, next_column] = True
            queue.append((next_row, next_column))
    return reached


def _seed_semantics(zone_spec: dict[str, Any]) -> tuple[str, ...]:
    """Which semantics count as a player start, authorable per world."""
    declared = (zone_spec.get("traversal_policy") or {}).get("playable_seed_semantics")
    if isinstance(declared, (list, tuple)) and declared:
        return tuple(str(value) for value in declared)
    return SEED_SEMANTICS


def _label_regions(
    passable: np.ndarray, heights: np.ndarray, max_climb_m: float
) -> tuple[np.ndarray, int]:
    """Connected standable regions, for worlds that declare no player start."""
    labels = np.zeros(passable.shape, dtype=np.int32)
    rows, columns = passable.shape
    current = 0
    for row in range(rows):
        for column in range(columns):
            if not passable[row, column] or labels[row, column]:
                continue
            current += 1
            labels[row, column] = current
            queue: deque[tuple[int, int]] = deque([(row, column)])
            while queue:
                r, c = queue.popleft()
                here = heights[r, c]
                for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nr, nc = r + dr, c + dc
                    if not (0 <= nr < rows and 0 <= nc < columns):
                        continue
                    if labels[nr, nc] or not passable[nr, nc]:
                        continue
                    if abs(heights[nr, nc] - here) > max_climb_m:
                        continue
                    labels[nr, nc] = current
                    queue.append((nr, nc))
    return labels, current


def _keep_anchors(zone_spec: dict[str, Any]) -> list[tuple[str, float, float]]:
    seed_semantics = _seed_semantics(zone_spec)
    anchors: list[tuple[str, float, float]] = []
    for feature in zone_spec.get("features", []):
        # Keyed on the declared semantic rather than on the id reading like a
        # keep -- a map whose keeps are named "citadel" is still a map.
        if feature.get("semantic") not in seed_semantics:
            continue
        points = (feature.get("geometry") or {}).get("points") or []
        if not points:
            continue
        anchors.append((str(feature["id"]), float(points[0][0]), float(points[0][1])))
    return anchors


def _edge_strips(escaped: np.ndarray, pad: int, rows: int, columns: int) -> dict[str, np.ndarray]:
    """The certified world's outermost ring, as reached on the padded grid.

    Two things have to be true at once and neither is obvious.

    The fill runs on a world padded outward with a flat continuation of its own
    edge -- the same continuation the apron renders. Without that padding this
    ring is unreachable by construction: reachability is computed for a body's
    *centre*, which erosion by the agent radius has already lifted off the last
    row, so the test would pronounce every world enclosed, including this one.

    But the answer is read off the *world's* last row, not off the padding. An
    agent that escapes anywhere can then walk right around the outside through
    the flat padding, so asking "did it reach the padding" reports all four
    edges leaking no matter where the single hole is -- true, useless, and it
    throws away the localisation roadmap 2.5 needs to know where to build.
    """
    return {
        "north": escaped[pad, pad : pad + columns],
        "south": escaped[pad + rows - 1, pad : pad + columns],
        "west": escaped[pad : pad + rows, pad],
        "east": escaped[pad : pad + rows, pad + columns - 1],
    }


def _edge_leaks(
    escaped: np.ndarray, width: float, length: float, pad: int, rows: int, columns: int
) -> list[dict[str, Any]]:
    """Contiguous spans of world edge the agent can stand on, in world metres."""
    edges = _edge_strips(escaped, pad, rows, columns)
    spans: list[dict[str, Any]] = []
    for name, strip in edges.items():
        horizontal = name in ("north", "south")
        extent = width if horizontal else length
        count = strip.size
        start: int | None = None
        for index in range(count + 1):
            live = index < count and bool(strip[index])
            if live and start is None:
                start = index
            elif not live and start is not None:
                # North/south run west->east in +X; west/east run north->south,
                # which is -Z, so the axis reverses.
                low = start / (count - 1) * extent - extent * 0.5
                high = (index - 1) / (count - 1) * extent - extent * 0.5
                spans.append(
                    {
                        "edge": name,
                        "axis": "x" if horizontal else "z",
                        "from_m": round(low if horizontal else -high, 3),
                        "to_m": round(high if horizontal else -low, 3),
                        "length_m": round(high - low, 3),
                    }
                )
                start = None
    spans.sort(key=lambda span: (span["edge"], span["from_m"]))
    return spans


def build(batch_dir: Path) -> tuple[dict[str, Any], np.ndarray]:
    """Measure the playable region and describe the apron beyond it."""
    terrain_dir = batch_dir / "terrain"
    manifest_path = terrain_dir / "terrain_manifest.json"
    heightfield_path = terrain_dir / "heightfield_f32le.bin"
    zone_spec_path = batch_dir / "zone_spec.json"
    for required in (manifest_path, heightfield_path, zone_spec_path):
        if not required.is_file():
            raise BoundaryPlanError("%s is missing; compile the batch first" % required)

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    zone_spec = json.loads(zone_spec_path.read_text(encoding="utf-8"))

    heightfield_digest = _digest(heightfield_path)
    if manifest.get("heightfield_sha256") != heightfield_digest:
        raise BoundaryPlanError(
            "terrain manifest does not match the heightfield on disk; the "
            "terrain was rebuilt without rewriting its manifest"
        )
    zone_spec_digest = _digest(zone_spec_path)
    # `zone_spec_sha256` is not one identity. The terrain manifest hashes the
    # canonicalised JSON; build_report.json hashes the file bytes. Same key,
    # different values, so the two can never be compared -- logged as debt.
    # Match the manifest on its own terms rather than reporting a false mismatch.
    canonical_digest = _canonical_digest(zone_spec)
    if manifest.get("zone_spec_sha256") not in (None, canonical_digest):
        raise BoundaryPlanError(
            "terrain was built from a different zone spec; a boundary measured "
            "with one world's agent against another world's terrain is fiction"
        )

    resolution = int(manifest["resolution"])
    bounds = manifest["world_bounds_m"]
    width, length = float(bounds["width"]), float(bounds["length"])
    heights = np.fromfile(heightfield_path, dtype="<f4")
    if heights.size != resolution * resolution:
        raise BoundaryPlanError(
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
    rows, columns = grid.shape

    # Pad the world with a flat continuation of its own edge -- exactly what
    # the apron renders -- and measure on that. The padding is the control: if
    # the agent ends up standing in it, nothing in the certified world was
    # holding it in, and the border is decoration. Measuring on the unpadded
    # grid cannot answer that question, because the data edge blocks the agent
    # there for a reason that does not exist in the shipped game.
    pad = max(1, int(math.ceil(radius_m / cell_m)) * 2 + 2)
    padded = np.pad(grid, pad, mode="edge")

    flat = standable(padded, cell_m, radius_m, max_slope_degrees)
    # A body is a disc, not a point: its centre may only sit where the whole
    # disc is standable, which is also what stops the fill squeezing through a
    # gap narrower than the agent.
    passable = _erode(flat, radius_m / cell_m)

    anchors = _keep_anchors(zone_spec)

    def to_cell(x: float, z: float) -> tuple[int, int]:
        """World metres to a cell of the *padded* grid."""
        column = int(round((x / width + 0.5) * (columns - 1)))
        row = int(round((0.5 - z / length) * (rows - 1)))
        return (
            min(max(row, 0), rows - 1) + pad,
            min(max(column, 0), columns - 1) + pad,
        )

    # Seeded from *one* keep, not all of them. Seeding every keep would put
    # each one inside the reached set by construction, and the connectivity
    # check below could never fail -- the same shape of always-passing gate the
    # containment band test was written to avoid.
    seeds: list[tuple[int, int]] = []
    unstandable: list[str] = []
    for keep_id, x, z in anchors:
        cell = to_cell(x, z)
        if not passable[cell]:
            unstandable.append(keep_id)
        elif not seeds:
            seeds.append(cell)
    derivation = "flood_fill_from_declared_starts"
    if not seeds:
        # No declared start, or none of them standable. A world is still a
        # world: fall back to its largest standable region and record that this
        # is what happened, so a reader never mistakes a fallback for a
        # measurement from a real spawn.
        derivation = (
            "flood_fill_from_largest_region"
            if not anchors
            else "flood_fill_from_largest_region_no_standable_start"
        )
        labels, count = _label_regions(passable, padded, max_climb_m)
        if count == 0:
            raise BoundaryPlanError(
                "no standable ground anywhere for the declared agent "
                "(radius %.2f m, max slope %.1f deg)" % (radius_m, max_slope_degrees)
            )
        largest = max(
            range(1, count + 1), key=lambda index: int((labels == index).sum())
        )
        cell = tuple(int(v) for v in np.argwhere(labels == largest)[0])
        seeds.append(cell)

    escaped = _flood(passable, padded, seeds, max_climb_m)
    # Every keep must land in the same component, or "the playable region" is
    # two disconnected regions and the word is a lie.
    disconnected = [
        keep_id
        for keep_id, x, z in anchors
        if keep_id not in unstandable and not escaped[to_cell(x, z)]
    ]

    # The playable region is what the agent reaches *inside* the certified
    # world; anything it reached in the padding is a leak, not playable ground.
    reached = escaped[pad : pad + rows, pad : pad + columns]
    leaks = _edge_leaks(escaped, width, length, pad, rows, columns)
    ring = np.concatenate(list(_edge_strips(escaped, pad, rows, columns).values()))

    occupied_rows, occupied_columns = np.nonzero(reached)
    if occupied_rows.size == 0:
        raise BoundaryPlanError("the fill reached no cells at all")
    minimum_x = (occupied_columns.min() / (columns - 1) - 0.5) * width
    maximum_x = (occupied_columns.max() / (columns - 1) - 0.5) * width
    minimum_z = (0.5 - occupied_rows.max() / (rows - 1)) * length
    maximum_z = (0.5 - occupied_rows.min() / (rows - 1)) * length

    height_range = manifest.get("height_range_m") or {}
    # (terrain relief is no longer used for the apron; see APRON_FALLOFF_GRADE)
    diagonal = math.hypot(width, length)

    plan = {
        "schema_version": SCHEMA_VERSION,
        "zone_id": manifest.get("zone_id"),
        # Both spellings, named for what they actually hash, so a consumer
        # never has to guess which one a bare `zone_spec_sha256` meant.
        "zone_spec_bytes_sha256": zone_spec_digest,
        "zone_spec_canonical_sha256": canonical_digest,
        "heightfield_sha256": heightfield_digest,
        # _bytes_ (T7 / D10): world_core/render_plan.rs's own fingerprint
        # block uses "terrain_manifest_sha256" for a canonical-JSON hash
        # inside a *different* artifact (render_plan.json) for its own
        # self-contained contract. Same key name, unrelated value, never
        # compared against this one -- named explicitly so nobody "fixes"
        # one to match the other.
        "terrain_manifest_bytes_sha256": _digest(manifest_path),
        "agent": {
            "radius_m": radius_m,
            "max_climb_m": max_climb_m,
            "max_slope_degrees": max_slope_degrees,
            "source": "zone_spec.traversal_policy",
        },
        "playable": {
            # Measured, not declared: this is where the spec's own agent can
            # actually get, starting from the keeps.
            "derivation": derivation,
            "seeds": [keep_id for keep_id, _, _ in anchors if keep_id not in unstandable],
            "mask": "terrain/playable_mask.bin",
            "mask_encoding": "uint8_row_major_1_is_playable",
            "mask_resolution": rows,
            "cell_m": round(cell_m, 6),
            "area_m2": round(float(reached.sum()) * cell_m * cell_m, 3),
            "world_fraction": round(float(reached.mean()), 6),
            "bounds_m": {
                "min_x": round(float(minimum_x), 3),
                "max_x": round(float(maximum_x), 3),
                "min_z": round(float(minimum_z), 3),
                "max_z": round(float(maximum_z), 3),
            },
            # No simplified polygon yet. Tracing one would be inventing a
            # precision the mask does not have, and every consumer so far
            # (navmesh gate, region volumes, backends) wants the raster.
            "polygon": None,
        },
        "containment": {
            "enclosed": not bool(ring.any()) and not disconnected,
            "edge_reach_fraction": round(float(ring.mean()), 6),
            "leak_spans": leaks,
            "leak_length_m": round(sum(span["length_m"] for span in leaks), 3),
            "disconnected_keeps": disconnected,
            "unstandable_keeps": unstandable,
            "remedy": (
                "roadmap 2.5 -- close these spans with landform. Geometry is the "
                "boundary; an invisible wall would be a per-backend hack that has "
                "to be re-authored for every target engine."
            ),
        },
        "apron": {
            # A rule, not a mesh: the renderer and every backend build the same
            # apron from these numbers, so there is one apron and not one each.
            "rule": "edge_clamp_then_falloff",
            "extent_m": round(diagonal * APRON_EXTENT_DIAGONALS, 3),
            "extent_derivation": "world_diagonal * %g" % APRON_EXTENT_DIAGONALS,
            "falloff_depth_m": round(
                diagonal * APRON_EXTENT_DIAGONALS * APRON_FALLOFF_GRADE, 3
            ),
            "falloff_derivation": "apron extent * %g grade" % APRON_FALLOFF_GRADE,
            # An apron a body could stand on would extend the very leak above.
            "colliding": False,
        },
    }
    return plan, reached


def write(batch_dir: Path) -> dict[str, Any]:
    """Build the plan and write its mask, returning the plan for the caller."""
    plan, mask = build(batch_dir)
    mask.astype(np.uint8).tofile(batch_dir / "terrain/playable_mask.bin")
    return plan


def main(argv: list[str] | None = None) -> int:
    import argparse
    import sys

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("batch", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument(
        "--require-enclosed",
        action="store_true",
        help="fail if the world does not contain its players (turn this on once roadmap 2.5 lands)",
    )
    arguments = parser.parse_args(argv)

    batch_dir = arguments.batch.resolve()
    try:
        plan = write(batch_dir)
    except (BoundaryPlanError, OSError, ValueError) as exc:
        print("%s: %s" % (arguments.batch, exc), file=sys.stderr)
        return 2

    destination = arguments.output or (batch_dir / "boundary_plan.json")
    destination.write_text(
        json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    playable = plan["playable"]
    containment = plan["containment"]
    print(
        "boundary plan %s: playable %.0f m2 (%.1f%% of world), apron %.0f m"
        % (
            plan["zone_id"],
            playable["area_m2"],
            playable["world_fraction"] * 100.0,
            plan["apron"]["extent_m"],
        )
    )
    if containment["enclosed"]:
        print("  world is enclosed: no reachable cell touches the data edge")
        return 0
    print(
        "  NOT ENCLOSED: %.0f m of world edge is walkable off into void "
        "across %d spans (%.1f%% of the perimeter)"
        % (
            containment["leak_length_m"],
            len(containment["leak_spans"]),
            containment["edge_reach_fraction"] * 100.0,
        )
    )
    for span in containment["leak_spans"][:8]:
        print(
            "    %-5s %s %8.1f .. %8.1f m  (%.0f m)"
            % (span["edge"], span["axis"], span["from_m"], span["to_m"], span["length_m"])
        )
    if len(containment["leak_spans"]) > 8:
        print("    ... %d more" % (len(containment["leak_spans"]) - 8))
    for keep_id in containment["disconnected_keeps"]:
        print("  keep %s is not reachable from the other keeps" % keep_id)
    return 1 if arguments.require_enclosed else 0


if __name__ == "__main__":
    raise SystemExit(main())
