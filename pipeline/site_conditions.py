"""The site conditions field: what every downstream system needs to know about
every point on the map (systems roadmap S1).

Forestry wants to know where it is wet and which slopes face the sun. Surfacing
wants the same numbers to decide where peat gives way to scree. Settlement
siting wants flatness and water proximity. Scree wants slope and rockfall
shadow. Derived independently, those four systems disagree about where the wet
hollow is -- the trees say one thing, the soil another, and the villages a
third, and nothing in the pipeline can tell you which is right.

**So it is computed once, here, and read by all of them.** That is the whole
purpose: not to save time, but to make the world internally consistent by
construction.

Two classes of field, computed differently on purpose:

**Local geometry** -- slope, aspect, curvature, relative elevation, insolation --
is a stencil over the heightfield. Vectorised, exact, full terrain resolution.

**Flow-derived fields** -- depression depth, accumulation, wetness -- need a
depression-filled surface and a routing pass, which is inherently sequential.
Those run on a coarser working grid and are resampled up. This is a deliberate
accuracy trade and a safe one: wetness is a broad-scale property, a hollow that
holds water is many cells across, and the alternative is a pure-Python priority
flood over a million cells on every build. The working resolution is recorded in
the report so nobody has to infer it.

This module deliberately stops short of *interpreting* what it computes. Which
sinks are lakes, which are bogs, where channels run and how deep to carve them
is hydrology (S2), and it reads the fields emitted here rather than recomputing
them.
"""

from __future__ import annotations

import hashlib
import heapq
import json
import math
from pathlib import Path
from typing import Any

import numpy as np

SCHEMA_VERSION = "codeweald.site-conditions/v1"

# Order is contract: the raster is a stack of float32 planes in exactly this
# sequence, and `site_conditions.json` repeats it so a consumer never has to
# guess which plane it is reading.
FIELDS = (
    "slope_degrees",
    "aspect_degrees",
    "profile_curvature",
    "relative_elevation_m",
    "flow_accumulation_log",
    "wetness_index",
    "insolation",
    "exposure",
)

# Working grid for the flow pass. 257 over a 256 m world is one sample per
# metre of world, which resolves any hollow big enough to matter to vegetation
# or siting. Raising it costs roughly quadratic time in the priority flood.
FLOW_RESOLUTION = 257

# Sun arc for insolation. Northern-hemisphere upland default: the sun sits
# south of overhead, so south-facing slopes are warmer and drier and their
# treeline runs higher. Not yet authored -- it becomes a climate parameter when
# S6/S7 actually consume it, and hard-coding it now would be inventing an
# authoring surface nothing reads.
SUN_AZIMUTH_DEGREES = 180.0
SUN_ALTITUDE_DEGREES = 42.0

# Radius over which "relative elevation" and "exposure" are judged, in metres.
# Small enough to distinguish a hollow from its shoulder, large enough not to
# simply restate slope.
NEIGHBOURHOOD_M = 24.0


class SiteConditionsError(ValueError):
    """The site conditions field cannot be built from these artifacts."""


def _digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _box_blur(values: np.ndarray, radius: int) -> np.ndarray:
    """Separable mean over a (2r+1) square, edge-clamped.

    Edge-clamped rather than wrapped: a cell on the north rim must not average
    in the south rim, which would make both ends of the map claim to sit in a
    hollow that is really the seam between them.
    """
    if radius < 1:
        return values.astype(np.float64, copy=True)
    padded = np.pad(values.astype(np.float64), radius, mode="edge")
    window = 2 * radius + 1
    cumulative = np.cumsum(padded, axis=0)
    rows = np.empty((values.shape[0], padded.shape[1]))
    rows[:] = cumulative[window - 1 :, :]
    rows[1:] -= cumulative[: -window, :]
    cumulative = np.cumsum(rows, axis=1)
    result = np.empty(values.shape)
    result[:] = cumulative[:, window - 1 :]
    result[:, 1:] -= cumulative[:, : -window]
    return result / float(window * window)


def _slope_aspect(height: np.ndarray, cell_m: float) -> tuple[np.ndarray, np.ndarray]:
    """Slope in degrees and aspect in compass degrees (0 = north, clockwise)."""
    # `np.gradient` returns d/drow, d/dcol. Rows run north to south, so a
    # positive row derivative means the ground falls southward, and the
    # northward component of the downslope direction is its negation.
    dz_drow, dz_dcol = np.gradient(height.astype(np.float64), cell_m)
    slope = np.degrees(np.arctan(np.hypot(dz_dcol, dz_drow)))
    aspect = np.degrees(np.arctan2(-dz_dcol, dz_drow)) % 360.0
    # Flat ground has no aspect. Reporting an arbitrary one would let a
    # consumer read a sun preference into ground that has none.
    aspect = np.where(slope < 0.05, np.nan, aspect)
    return slope, aspect


def _profile_curvature(height: np.ndarray, cell_m: float) -> np.ndarray:
    """Convexity along the slope: positive on ridges and shoulders, negative in
    hollows and channels. This is the field that distinguishes a bench from a
    bowl, which slope alone cannot."""
    dz_drow, dz_dcol = np.gradient(height.astype(np.float64), cell_m)
    d2_drow2 = np.gradient(dz_drow, cell_m, axis=0)
    d2_dcol2 = np.gradient(dz_dcol, cell_m, axis=1)
    return d2_drow2 + d2_dcol2


def _fill_depressions(height: np.ndarray) -> np.ndarray:
    """Priority-flood depression filling (Barnes et al.).

    Water cannot route across a surface with pits in it -- every pit swallows
    its own catchment and the accumulation downstream of it is simply wrong. So
    the surface is filled to its spill level first, and the difference between
    filled and original is itself a useful field: it is how deep the standing
    water would be, which is what S2 will classify into lakes and bogs.
    """
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
            # Either the cell stands above the spill level, or it is drowned to
            # it. Both cases push at the level water would actually reach.
            filled[r, c] = max(float(height[r, c]), level)
            heapq.heappush(queue, (filled[r, c], r, c))
    return filled


def _flow_accumulation(filled: np.ndarray) -> np.ndarray:
    """D8 upslope cell count on a depression-filled surface.

    Single-direction routing: each cell donates its whole accumulation to its
    steepest downslope neighbour. Cruder than D-infinity and visibly so on
    planar hillsides, but it is the right first pass -- it produces the
    convergent network that channels, wetness and riparian vegetation all read
    from, and S2 can refine the routing where it matters without this having to
    change shape.
    """
    rows, columns = filled.shape
    order = np.argsort(filled, axis=None)[::-1]  # highest first
    accumulation = np.ones(filled.shape, dtype=np.float64)
    neighbours = ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1))
    for index in order:
        row, column = divmod(int(index), columns)
        here = filled[row, column]
        best_drop, best = 0.0, None
        for dr, dc in neighbours:
            r, c = row + dr, column + dc
            if not (0 <= r < rows and 0 <= c < columns):
                continue
            drop = (here - filled[r, c]) / math.hypot(dr, dc)
            if drop > best_drop:
                best_drop, best = drop, (r, c)
        if best is not None:
            accumulation[best] += accumulation[row, column]
    return accumulation


def _resample(values: np.ndarray, resolution: int) -> np.ndarray:
    """Bilinear resample to a square grid. Used to lift the flow fields from
    their working grid back to terrain resolution."""
    source = values.shape[0]
    if source == resolution:
        return values
    position = np.linspace(0.0, source - 1.0, resolution)
    low = np.clip(np.floor(position).astype(int), 0, source - 1)
    high = np.clip(low + 1, 0, source - 1)
    weight = position - low
    rows = values[low, :] * (1.0 - weight)[:, None] + values[high, :] * weight[:, None]
    return rows[:, low] * (1.0 - weight)[None, :] + rows[:, high] * weight[None, :]


def build(batch_dir: Path) -> tuple[dict[str, Any], np.ndarray]:
    """Derive the site conditions field. Returns the report and the stacked raster."""
    terrain_dir = batch_dir / "terrain"
    manifest_path = terrain_dir / "terrain_manifest.json"
    heightfield_path = terrain_dir / "heightfield_f32le.bin"
    for required in (manifest_path, heightfield_path):
        if not required.is_file():
            raise SiteConditionsError("%s is missing; compile the batch first" % required)

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    heightfield_digest = _digest(heightfield_path)
    if manifest.get("heightfield_sha256") != heightfield_digest:
        raise SiteConditionsError(
            "terrain manifest does not match the heightfield on disk; site "
            "conditions derived from one would describe the other"
        )

    resolution = int(manifest["resolution"])
    bounds = manifest["world_bounds_m"]
    width, length = float(bounds["width"]), float(bounds["length"])
    height = np.fromfile(heightfield_path, dtype="<f4")
    if height.size != resolution * resolution:
        raise SiteConditionsError(
            "heightfield has %d samples, manifest declares %d"
            % (height.size, resolution * resolution)
        )
    height = height.reshape(resolution, resolution).astype(np.float64)
    cell_m = width / (resolution - 1)

    slope, aspect = _slope_aspect(height, cell_m)
    curvature = _profile_curvature(height, cell_m)

    neighbourhood = max(1, int(round(NEIGHBOURHOOD_M / cell_m)))
    smoothed = _box_blur(height, neighbourhood)
    relative = height - smoothed
    # Exposure: how far this cell stands above its surroundings, normalised by
    # the local relief so a 3 m knoll on a plain reads as exposed as a 30 m
    # shoulder in the mountains. Ridges approach 1, sheltered hollows 0.
    spread = _box_blur(np.abs(relative), neighbourhood) + 1e-6
    exposure = np.clip(0.5 + 0.5 * relative / (3.0 * spread), 0.0, 1.0)

    # Flow fields on the working grid.
    flow_resolution = min(FLOW_RESOLUTION, resolution)
    working = _resample(height, flow_resolution)
    filled = _fill_depressions(working)
    accumulation = _flow_accumulation(filled)
    sink_depth = np.maximum(filled - working, 0.0)

    working_cell = width / (flow_resolution - 1)
    working_slope, _ = _slope_aspect(filled, working_cell)
    # Topographic wetness: ln(upslope area / local gradient). High where a lot
    # of ground drains through gentle terrain, which is exactly where peat,
    # marsh and riparian species belong.
    contributing = accumulation * working_cell * working_cell
    gradient = np.maximum(np.tan(np.radians(working_slope)), 0.001)
    wetness = np.log(contributing / gradient)

    accumulation_log = _resample(np.log1p(accumulation), resolution)
    wetness_field = _resample(wetness, resolution)
    sink_depth_full = _resample(sink_depth, resolution)

    # Insolation: cosine of the angle between the surface normal and the sun,
    # clamped at zero (a slope facing away receives no direct sun). Flat ground
    # has no aspect, so it takes the horizontal case exactly.
    altitude = math.radians(SUN_ALTITUDE_DEGREES)
    azimuth = math.radians(SUN_AZIMUTH_DEGREES)
    slope_radians = np.radians(slope)
    aspect_radians = np.radians(np.where(np.isnan(aspect), 0.0, aspect))
    insolation = np.clip(
        np.cos(slope_radians) * math.sin(altitude)
        + np.sin(slope_radians) * math.cos(altitude) * np.cos(azimuth - aspect_radians),
        0.0,
        1.0,
    )

    planes = {
        "slope_degrees": slope,
        # NaN is meaningful for aspect and meaningless in a binary plane, so it
        # is encoded as -1: "this ground is flat and has no aspect".
        "aspect_degrees": np.where(np.isnan(aspect), -1.0, aspect),
        "profile_curvature": curvature,
        "relative_elevation_m": relative,
        "flow_accumulation_log": accumulation_log,
        "wetness_index": wetness_field,
        "insolation": insolation,
        "exposure": exposure,
    }
    stack = np.stack([planes[name].astype(np.float32) for name in FIELDS])

    if not np.isfinite(stack).all():
        offenders = [name for name in FIELDS if not np.isfinite(planes[name]).all()]
        raise SiteConditionsError(
            "site conditions contain non-finite values in: %s. A downstream "
            "solver reading these would place vegetation or settlements at "
            "coordinates that do not exist." % ", ".join(offenders)
        )

    report = {
        "schema_version": SCHEMA_VERSION,
        "zone_id": manifest.get("zone_id"),
        "heightfield_sha256": heightfield_digest,
        "terrain_manifest_bytes_sha256": _digest(manifest_path),
        "resolution": resolution,
        "cell_m": round(cell_m, 6),
        "world_bounds_m": {"width": width, "length": length},
        "field": "terrain/site_conditions_f32le.bin",
        "field_encoding": "float32_le_plane_major",
        "fields": list(FIELDS),
        "flow": {
            "working_resolution": flow_resolution,
            "working_cell_m": round(working_cell, 6),
            "derivation": "priority_flood_fill_then_d8_accumulation",
            "note": (
                "resampled to terrain resolution; wetness is a broad-scale "
                "property and a pure-Python flood over %d cells is not"
                % (resolution * resolution)
            ),
            "sink_area_m2": round(
                float((sink_depth_full > 0.05).sum()) * cell_m * cell_m, 1
            ),
            "deepest_sink_m": round(float(sink_depth_full.max()), 3),
        },
        "sun": {
            "azimuth_degrees": SUN_AZIMUTH_DEGREES,
            "altitude_degrees": SUN_ALTITUDE_DEGREES,
        },
        "neighbourhood_m": NEIGHBOURHOOD_M,
        "statistics": {
            name: {
                "min": round(float(np.nanmin(planes[name])), 5),
                "max": round(float(np.nanmax(planes[name])), 5),
                "mean": round(float(np.nanmean(planes[name])), 5),
            }
            for name in FIELDS
        },
    }
    return report, stack


def write(batch_dir: Path) -> dict[str, Any]:
    """Build the field and write both artifacts. Returns the report."""
    report, stack = build(batch_dir)
    destination = batch_dir / "terrain" / "site_conditions_f32le.bin"
    destination.parent.mkdir(parents=True, exist_ok=True)
    stack.astype("<f4").tofile(destination)
    report["field_sha256"] = _digest(destination)
    return report


def main(argv: list[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("batch", type=Path)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args(argv)

    try:
        report = write(arguments.batch.resolve())
    except (SiteConditionsError, OSError, ValueError) as exc:
        print("%s: %s" % (arguments.batch, exc), file=__import__("sys").stderr)
        return 2

    destination = arguments.output or (arguments.batch / "site_conditions.json")
    destination.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(
        "site conditions %s: %d fields at %dx%d, flow at %d, %.0f m2 of sink"
        % (
            report["zone_id"],
            len(report["fields"]),
            report["resolution"],
            report["resolution"],
            report["flow"]["working_resolution"],
            report["flow"]["sink_area_m2"],
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
