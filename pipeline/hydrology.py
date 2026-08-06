"""Where water goes, and what it becomes when it stops (systems roadmap S2).

The alpine arena declared eight watercourses, every one of them a
`wetland_rill` two metres wide, carving essentially nothing -- so the map had
blue splatmap patches on flat ground and bridges crossing them (D20). Water was
authored as decoration, and decoration is not a network.

**Everything the art direction asks for is a consequence of position in a
drainage graph, not a property anyone authors:**

| Wanted | Derived from |
|---|---|
| stagnant green bog, lilypads | shallow closed sink, negligible catchment |
| still cold tarn | *deep* closed sink, negligible catchment |
| deeper darker pond | sink with upstream inflow |
| trickle from the mountains | low-order reach, high ground |
| river across the map | high-order reach |
| runs off the edge | reach that reaches an outlet |
| walkable ford vs swimmable | reach depth against the agent |

So this module does not ask where the water is. It asks where the water *goes*,
and reads the rest off the answer.

**The outlet problem.** Closing the world (S4) made the whole map a closed
basin: the interior floor sits below the lowest point of the rim, so 80% of the
world is a depression and nothing drains. That is physically honest and
gameplay-correct, but it means every drop that lands stays, and the art
direction explicitly wants water that runs off the edge. A world therefore needs
at least one **outlet**: a notch cut through the rim where the drainage network
leaves. Choosing it is a solver's job, not an author's -- the right place is
wherever the most water already wants to go, which is a measurement.

Pure arithmetic over arrays: no batch, no I/O, no `bpy`. The carving is applied
by `zone_rasterizer`; the classification is emitted by the build stage.
"""

from __future__ import annotations

import heapq
import math
from dataclasses import dataclass

import numpy as np

# A sink shallower than this is a damp patch, not a body of water. Below it the
# classification would be labelling numerical noise in the fill.
MINIMUM_WATER_DEPTH_M = 0.35

# Upslope area, in cells, above which a route counts as a channel rather than
# as diffuse runoff. Every hillside has flow; only some of it is a stream.
CHANNEL_ACCUMULATION = 60.0

# A sink whose catchment is under this many cells is fed by rain landing in it
# and nothing else -- the hydrological definition of stagnant water rather than
# a fed pond. Catchment decides whether water is *stagnant*; it does not decide
# whether the body is a bog, because depth does. See below.
BOG_CATCHMENT = 140.0

# Depth thresholds are **fractions of the agent**, never absolute metres.
#
# This world is authored at heroic scale -- `agent_height_m` is 8.0, not 1.8 --
# and the previous rule hardcoded 1.6 m as the swimming depth, which is chest
# height on a human and shin height on the actual agent. The result was output
# that contradicted itself: fifteen bodies classified `bog`, six of them also
# flagged `swimmable`. You cannot swim in a bog. That is what makes it a bog.
#
# Expressing these as proportions means they stay correct for any agent, and a
# world that changes its actor size does not silently reclassify its water.

# Knee-deep. Past this you are not walking through waterlogged ground -- which
# is what a bog *is* -- you are in open water that merely happens to be
# stagnant. A deep stagnant sink in rock is a tarn.
BOG_DEPTH_FRACTION = 0.25

# Shoulder-deep: the point where the feet leave the bottom.
SWIM_DEPTH_FRACTION = 0.75

# Only a fallback for direct callers. The build path passes the world's own
# authored value; this default matches `zone_compiler.DEFAULT_TRAVERSAL_POLICY`.
DEFAULT_AGENT_HEIGHT_M = 8.0


@dataclass(frozen=True)
class Outlet:
    """Where the world drains, in grid coordinates and metres."""

    row: int
    column: int
    spill_height_m: float
    catchment_cells: float
    edge: str


def fill_depressions(height: np.ndarray) -> np.ndarray:
    """Priority-flood fill (Barnes et al.); returns the drowned surface."""
    rows, columns = height.shape
    filled = np.full(height.shape, np.inf)
    closed = np.zeros(height.shape, dtype=bool)
    queue: list[tuple[float, int, int]] = []
    for row in range(rows):
        for column in (0, columns - 1):
            heapq.heappush(queue, (float(height[row, column]), row, column))
            closed[row, column] = True
            filled[row, column] = height[row, column]
    for column in range(columns):
        for row in (0, rows - 1):
            if not closed[row, column]:
                heapq.heappush(queue, (float(height[row, column]), row, column))
                closed[row, column] = True
                filled[row, column] = height[row, column]
    while queue:
        level, row, column = heapq.heappop(queue)
        for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            r, c = row + dr, column + dc
            if not (0 <= r < rows and 0 <= c < columns) or closed[r, c]:
                continue
            closed[r, c] = True
            filled[r, c] = max(float(height[r, c]), level)
            heapq.heappush(queue, (filled[r, c], r, c))
    return filled


def flow_accumulation(filled: np.ndarray) -> np.ndarray:
    """D8 upslope cell count over a depression-filled surface."""
    rows, columns = filled.shape
    order = np.argsort(filled, axis=None)[::-1]
    accumulation = np.ones(filled.shape, dtype=np.float64)
    steps = ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1))
    for index in order:
        row, column = divmod(int(index), columns)
        here = filled[row, column]
        best_drop, best = 0.0, None
        for dr, dc in steps:
            r, c = row + dr, column + dc
            if not (0 <= r < rows and 0 <= c < columns):
                continue
            drop = (here - filled[r, c]) / math.hypot(dr, dc)
            if drop > best_drop:
                best_drop, best = drop, (r, c)
        if best is not None:
            accumulation[best] += accumulation[row, column]
    return accumulation


def choose_outlet(height: np.ndarray, accumulation: np.ndarray) -> Outlet:
    """Where the world should drain.

    **Not an authored choice.** The right place for an outlet is wherever the
    most water already wants to leave, weighed against how much rock is in the
    way -- which is a measurement over the rim, not a preference. Every edge
    cell is scored on the drainage arriving at it against the height it would
    have to be cut through, and the best is taken.

    Picking the lowest point alone would put the outlet wherever the rim
    happens to dip, even if nothing drains toward it; picking the wettest alone
    would drive a gorge through a summit. The ratio is what makes it a saddle
    with a river behind it, which is what a real outlet is.
    """
    rows, columns = height.shape
    floor = float(height.min())
    candidates: list[tuple[float, int, int, str]] = []
    for column in range(1, columns - 1):
        candidates.append((0.0, 0, column, "north"))
        candidates.append((0.0, rows - 1, column, "south"))
    for row in range(1, rows - 1):
        candidates.append((0.0, row, 0, "west"))
        candidates.append((0.0, row, columns - 1, "east"))

    best: Outlet | None = None
    best_score = -math.inf
    for _, row, column, edge in candidates:
        rise = max(float(height[row, column]) - floor, 0.1)
        # Drainage arriving just inside the rim, not on the rim itself: the rim
        # cell is a summit and nothing accumulates on it.
        inner_row = min(max(row, 1), rows - 2)
        inner_column = min(max(column, 1), columns - 2)
        arriving = float(accumulation[inner_row, inner_column])
        score = arriving / rise
        if score > best_score:
            best_score = score
            best = Outlet(
                row=row,
                column=column,
                spill_height_m=float(height[row, column]),
                catchment_cells=arriving,
                edge=edge,
            )
    assert best is not None
    return best


def carve_outlet(
    height: np.ndarray,
    row: int,
    column: int,
    *,
    cell_m: float,
    half_width_m: float = 8.0,
    reach_m: float = 52.0,
    depth_below_floor_m: float = 1.4,
) -> np.ndarray:
    """Cut a gorge through the rim so the basin drains.

    **A disc at the edge is a dimple, not an outlet.** The first version cut a
    9 m radius bowl centred on the rim cell, against a rim ~28 m deep -- so it
    lowered the outside of the wall and left the inside intact, and the basin
    still held 46,157 m2 of standing water. The notch has to run *inward* far
    enough to reach the ground it is draining.

    So it is carved along a segment from the rim cell toward the map interior,
    over `reach_m`, with the bed falling toward the edge -- a channel level with
    the ground it drains does not flow. The walls either side are left standing,
    so the world stays shut to a body while being open to water: a gorge, not a
    doorway.
    """
    rows, columns = height.shape
    row_index, column_index = np.ogrid[:rows, :columns]

    # Inward is whichever way the map centre lies from this rim cell.
    toward_row = (rows - 1) * 0.5 - row
    toward_column = (columns - 1) * 0.5 - column
    span = math.hypot(toward_row, toward_column) or 1.0
    unit_row, unit_column = toward_row / span, toward_column / span
    reach_cells = reach_m / max(cell_m, 1e-6)

    delta_row = row_index - row
    delta_column = column_index - column
    # Projection along the channel, clamped to the segment.
    along = np.clip(delta_row * unit_row + delta_column * unit_column, 0.0, reach_cells)
    across = np.hypot(
        (delta_row - along * unit_row) * cell_m,
        (delta_column - along * unit_column) * cell_m,
    )

    floor = float(height.min())
    # Bed falls from the interior floor at the inner end to below it at the rim,
    # so water has somewhere to go rather than pooling in the gorge.
    fraction = np.clip(along / max(reach_cells, 1e-6), 0.0, 1.0)
    bed = floor - depth_below_floor_m * (1.0 - fraction)

    blend = np.clip(1.0 - across / max(half_width_m, 1e-6), 0.0, 1.0)
    # Smooth shoulders, or the gorge meets the rim at a grade discontinuity and
    # the accessibility gate measures the corner -- the same defect the rampart
    # toe had.
    blend = blend * blend * (3.0 - 2.0 * blend)
    return np.minimum(height, height * (1.0 - blend) + bed * blend)


def classify(
    height: np.ndarray,
    filled: np.ndarray,
    accumulation: np.ndarray,
    *,
    cell_m: float,
    agent_height_m: float = DEFAULT_AGENT_HEIGHT_M,
) -> dict:
    """Name every body of water the terrain implies.

    Bog versus pond is not a palette choice: a sink whose catchment is only the
    rain landing in it is stagnant and green, and one fed by a mountain stream
    is deeper, clearer and colder. That is the distinction the art direction
    asked for, and it falls out of the drainage graph for free.

    **Catchment alone is not enough**, which is the defect this function used to
    have. Feed decides whether water is stagnant. It says nothing about depth,
    so a deep closed basin in rock came out labelled `bog` and rendered stagnant
    green -- and the arena had a 7.96 m one. A stagnant body too deep to wade is
    a **tarn**: still, cold and clear, because there is no shallow warm margin
    for anything to rot in. Green scum on an eight-metre pool is the visible
    lie, and it survived precisely because nothing in the output disagreed with
    it until `swimmable` was also made honest.

    So the two axes are independent and both are needed:

    | | shallow | deep |
    |---|---|---|
    | **stagnant** | bog | tarn |
    | **fed** | pond | pond |

    Fed water is left as one kind deliberately. Splitting it would be inventing
    a distinction this world has no evidence for -- every fed body in the arena
    is shallow -- and inventing categories to fill a table is how a taxonomy
    stops tracking the terrain.
    """
    bog_ceiling_m = agent_height_m * BOG_DEPTH_FRACTION
    swim_depth_m = agent_height_m * SWIM_DEPTH_FRACTION
    depth = np.maximum(filled - height, 0.0)
    water = depth > MINIMUM_WATER_DEPTH_M
    channels = (accumulation >= CHANNEL_ACCUMULATION) & ~water

    bodies: list[dict] = []
    seen = np.zeros(height.shape, dtype=bool)
    rows, columns = height.shape
    for row in range(rows):
        for column in range(columns):
            if not water[row, column] or seen[row, column]:
                continue
            stack = [(row, column)]
            seen[row, column] = True
            cells: list[tuple[int, int]] = []
            while stack:
                r, c = stack.pop()
                cells.append((r, c))
                for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nr, nc = r + dr, c + dc
                    if (
                        0 <= nr < rows
                        and 0 <= nc < columns
                        and water[nr, nc]
                        and not seen[nr, nc]
                    ):
                        seen[nr, nc] = True
                        stack.append((nr, nc))
            area = len(cells) * cell_m * cell_m
            if area < 4.0:
                continue
            inflow = max(float(accumulation[r, c]) for r, c in cells)
            deepest = max(float(depth[r, c]) for r, c in cells)
            fed = inflow >= BOG_CATCHMENT
            wadeable = deepest <= bog_ceiling_m
            if fed:
                kind = "pond"
            elif wadeable:
                kind = "bog"
            else:
                kind = "tarn"
            bodies.append(
                {
                    "kind": kind,
                    "fed": fed,
                    "wadeable": bool(wadeable),
                    "area_m2": round(area, 1),
                    "maximum_depth_m": round(deepest, 3),
                    "inflow_cells": round(inflow, 1),
                    # What the art direction reads. Only a bog is green: it is
                    # the shallow stagnant margin that grows the scum. A tarn is
                    # stagnant too and still reads cold, so this keys off `kind`
                    # rather than off `fed` as it once did.
                    "surface": "stagnant_green" if kind == "bog" else "cold_blue",
                    "swimmable": bool(deepest >= swim_depth_m),
                }
            )

    # A bog you can swim in is not a bog. The thresholds are ordered so this is
    # unreachable -- BOG_DEPTH_FRACTION is well under SWIM_DEPTH_FRACTION -- but
    # they are two independent constants, and the whole reason this function was
    # wrong is that nothing ever compared them. Assert rather than trust.
    contradictions = [
        b for b in bodies if b["kind"] == "bog" and b["swimmable"]
    ]
    if contradictions:
        raise AssertionError(
            "hydrology classified %d swimmable bog(s); BOG_DEPTH_FRACTION=%.3f "
            "must stay below SWIM_DEPTH_FRACTION=%.3f (agent %.2f m)"
            % (
                len(contradictions),
                BOG_DEPTH_FRACTION,
                SWIM_DEPTH_FRACTION,
                agent_height_m,
            )
        )

    return {
        "water_area_m2": round(float(water.sum()) * cell_m * cell_m, 1),
        "channel_area_m2": round(float(channels.sum()) * cell_m * cell_m, 1),
        "bodies": sorted(bodies, key=lambda b: -b["area_m2"]),
        "body_count": len(bodies),
        "bog_count": sum(1 for b in bodies if b["kind"] == "bog"),
        "tarn_count": sum(1 for b in bodies if b["kind"] == "tarn"),
        "pond_count": sum(1 for b in bodies if b["kind"] == "pond"),
        # Everything a bog renders as wet walkable ground rather than open
        # water. Item 6's fifth splat channel consumes exactly this.
        "wetland_area_m2": round(
            sum(b["area_m2"] for b in bodies if b["kind"] == "bog"), 1
        ),
    }


def build(batch_dir, *, agent_height_m: float = DEFAULT_AGENT_HEIGHT_M) -> dict:
    """Classify the water a compiled world implies, from its own terrain.

    `agent_height_m` comes from the world's own traversal policy rather than
    from the terrain manifest, which does not carry one. Depth only means
    anything relative to whoever is standing in it.
    """
    import json
    from pathlib import Path

    batch_dir = Path(batch_dir)
    manifest = json.loads(
        (batch_dir / "terrain/terrain_manifest.json").read_text(encoding="utf-8")
    )
    resolution = int(manifest["resolution"])
    width = float(manifest["world_bounds_m"]["width"])
    height = (
        np.fromfile(batch_dir / "terrain/heightfield_f32le.bin", dtype="<f4")
        .reshape(resolution, resolution)
        .astype(np.float64)
    )
    # Same coarse working grid as S1, for the same reason.
    side = min(257, resolution)
    step = max(1, (resolution - 1) // (side - 1))
    coarse = height[::step, ::step]
    cell_m = width / (coarse.shape[0] - 1)

    filled = fill_depressions(coarse)
    accumulation = flow_accumulation(filled)
    report = classify(
        coarse,
        filled,
        accumulation,
        cell_m=cell_m,
        agent_height_m=agent_height_m,
    )
    report.update(
        {
            "schema_version": "codeweald.hydrology-plan/v1",
            "zone_id": manifest.get("zone_id"),
            "heightfield_sha256": manifest.get("heightfield_sha256"),
            "working_resolution": int(coarse.shape[0]),
            "working_cell_m": round(cell_m, 6),
            "outlet": manifest.get("outlet", {}),
            "thresholds": {
                "minimum_water_depth_m": MINIMUM_WATER_DEPTH_M,
                "channel_accumulation_cells": CHANNEL_ACCUMULATION,
                "bog_catchment_cells": BOG_CATCHMENT,
                # Recorded in metres *and* as the fractions they derive from,
                # so a reader can tell the difference between a number that was
                # chosen and a number that was computed.
                "agent_height_m": round(agent_height_m, 4),
                "bog_depth_fraction": BOG_DEPTH_FRACTION,
                "bog_maximum_depth_m": round(agent_height_m * BOG_DEPTH_FRACTION, 4),
                "swim_depth_fraction": SWIM_DEPTH_FRACTION,
                "swim_depth_m": round(agent_height_m * SWIM_DEPTH_FRACTION, 4),
            },
        }
    )
    return report


def channel_depth_field(
    accumulation: np.ndarray,
    *,
    cell_m: float,
    threshold: float = CHANNEL_ACCUMULATION,
    maximum_depth_m: float = 1.15,
) -> np.ndarray:
    """How deep the bed is at every cell, in metres.

    Depth scales with the *logarithm* of upslope area, not with area itself. A
    reach draining ten times the ground is not ten times deeper -- it is roughly
    twice -- which is why a linear rule produces a shallow scratch everywhere
    and one canyon at the outlet.

    Capped deliberately low. The declared art direction is rivers shallow enough
    to walk, with bridges as an aesthetic choice rather than a traversal
    requirement, so the bed must stay well inside the agent's climb height. A
    channel that needs a bridge is a channel that broke the map.
    """
    above = np.maximum(accumulation - threshold, 0.0)
    if not np.any(above > 0.0):
        return np.zeros_like(accumulation)
    strength = np.log1p(above) / math.log1p(float(above.max()) or 1.0)
    return np.clip(strength, 0.0, 1.0) * maximum_depth_m


def carve_channels(
    height: np.ndarray,
    depth: np.ndarray,
    *,
    smoothing: int = 2,
) -> np.ndarray:
    """Cut the derived network into the terrain.

    Banks are smoothed before subtraction so the bed has shoulders rather than
    vertical sides. A one-cell-wide trench is both invisible from any distance
    and a grade discontinuity the accessibility gate correctly objects to -- the
    same defect the rampart toe and the outlet notch each had in turn.
    """
    if smoothing < 1 or not np.any(depth > 0.0):
        return height - depth

    def box(values: np.ndarray) -> np.ndarray:
        padded = np.pad(values, smoothing, mode="edge")
        window = 2 * smoothing + 1
        total = np.zeros_like(values)
        for row_offset in range(window):
            for column_offset in range(window):
                total += padded[
                    row_offset : row_offset + values.shape[0],
                    column_offset : column_offset + values.shape[1],
                ]
        return total / float(window * window)

    # **Twice.** One box pass spreads a channel into a flat-bottomed trench with
    # vertical sides -- measured: the bed and both cells beside it identical at
    # 0.20 m, then a step to zero. That is the trench this smoothing exists to
    # avoid, just wider. Convolving two boxes gives a triangular kernel, which
    # actually tapers, so the bed has shoulders that meet the ground.
    return height - box(box(depth))
