#!/usr/bin/env python3
"""Bring real-world elevation data in as a heightfield.

**Why this exists.** The reference images for this project are photographs of
places that exist -- a glacial valley in Waterton, a Tyrolean valley floor. Those
landforms have been laser-scanned. Trying to *synthesise* them is strictly worse
than downloading them: a generator approximates the character of a U-valley,
while a DEM is the valley.

So the split docs/integrations/gaea-programme.md 5.7a arrives at extends one step further.
**Gaea is for terrain that has to be fictional or has to satisfy gameplay
constraints. Real references come in as real data.** Everything downstream --
the importer, the metrics, the cropper, the viewer -- already consumes a
heightfield and does not care which produced it.

**No GDAL.** A GeoTIFF is a TIFF carrying georeferencing tags, and PIL reads
TIFF with libtiff. Pulling in GDAL or rasterio for tag parsing and a pixel read
would be a large dependency for a small job, and this pipeline has to stay
runnable headlessly on a machine nobody has provisioned.

**The projection caveat is the part that bites.** A DEM in a projected CRS (UTM,
state plane) has pixel scale already in metres. A DEM in a geographic CRS
(EPSG:4326) has it in *degrees*, and a degree of longitude is not a fixed
distance -- it shrinks with latitude. Treating degrees as metres yields terrain
stretched by a factor of `1/cos(latitude)`, which at 49 degrees (Waterton) is a
52% error in one axis and looks like a perfectly plausible valley. So the CRS is
read, not assumed, and a geographic DEM is converted using its own latitude.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path

import numpy as np

DEM_SCHEMA = "codeweald.terrain-artifacts/v1"
INGEST_VERSION = 1

# GeoTIFF tags. Values are the TIFF tag numbers, not indices.
MODEL_PIXEL_SCALE = 33550
MODEL_TIEPOINT = 33922
GEO_KEY_DIRECTORY = 34735

# GTModelTypeGeoKey values.
MODEL_PROJECTED = 1
MODEL_GEOGRAPHIC = 2

# WGS84 mean metres per degree of latitude. Good to a few parts per thousand,
# which is far below the vertical noise of any DEM this will read.
METRES_PER_DEGREE = 111_320.0

# Mirrors TERRAIN_STRIDE in the viewer; see import_heightfield.
VIEWER_TERRAIN_STRIDE = 4


def stride_compatible(resolution: int, stride: int = VIEWER_TERRAIN_STRIDE) -> int:
    remainder = (resolution - 1) % stride
    return resolution if remainder == 0 else resolution + (stride - remainder)


def read_geotiff(path: Path) -> tuple[np.ndarray, dict]:
    """Elevation in metres, plus what the file says about its own geometry."""
    from PIL import Image

    Image.MAX_IMAGE_PIXELS = None  # DEM tiles are legitimately enormous
    with Image.open(path) as source:
        if source.mode not in {"F", "I", "I;16", "I;16B", "L"}:
            source = source.convert("F")
        elevation = np.asarray(source).astype(np.float64)
        tags = dict(getattr(source, "tag_v2", {}) or {})

    if elevation.ndim != 2:
        raise SystemExit(f"{path.name} is not a single-band raster")

    scale = tags.get(MODEL_PIXEL_SCALE)
    if not scale or len(scale) < 2:
        raise SystemExit(
            f"{path.name} carries no ModelPixelScale tag, so its ground sample "
            "distance is unknown. Pass --cell-m to assert it."
        )
    tiepoint = tags.get(MODEL_TIEPOINT) or ()
    keys = tags.get(GEO_KEY_DIRECTORY) or ()

    # GeoKeyDirectory is a flat array of 4-shorts records after a 4-short
    # header; key 1024 is GTModelTypeGeoKey.
    model_type = None
    for index in range(4, len(keys) - 3, 4):
        if keys[index] == 1024:
            model_type = keys[index + 3]
            break

    return elevation, {
        "pixel_scale": (float(scale[0]), float(scale[1])),
        "tiepoint": tuple(float(v) for v in tiepoint),
        "model_type": model_type,
    }


def ground_sample_metres(geometry: dict, override: float | None) -> tuple[float, float, str]:
    """Metres per pixel in x and y, and how that was decided."""
    if override is not None:
        return override, override, "asserted"

    scale_x, scale_y = geometry["pixel_scale"]
    model_type = geometry["model_type"]

    if model_type == MODEL_GEOGRAPHIC:
        # Degrees. Latitude comes from the tiepoint's northing, which for a
        # geographic CRS is a latitude in degrees.
        tiepoint = geometry["tiepoint"]
        latitude = float(tiepoint[4]) if len(tiepoint) >= 5 else 0.0
        metres_x = scale_x * METRES_PER_DEGREE * math.cos(math.radians(latitude))
        metres_y = scale_y * METRES_PER_DEGREE
        return metres_x, metres_y, f"geographic at {latitude:.3f} deg"

    if model_type == MODEL_PROJECTED:
        return scale_x, scale_y, "projected"

    # Unknown model type. Refuse rather than guess: if the scale is in degrees
    # and gets read as metres, the terrain is ~100000x too small and the error
    # is obvious; if it is in metres and read as degrees it is not obvious at
    # all, and a plausible-looking wrong valley is the worst outcome here.
    raise SystemExit(
        "GeoTIFF does not declare GTModelTypeGeoKey, so its units are unknown. "
        "Pass --cell-m to assert metres per pixel."
    )


def clean_voids(elevation: np.ndarray, nodata: float | None) -> np.ndarray:
    """Replace nodata and absurd values with the local minimum.

    DEMs carry voids -- radar shadow behind ridges, water bodies, tile edges --
    usually as a large negative sentinel. Left alone, a -32768 cell becomes a
    hole several kilometres deep that dominates the vertical range and makes
    every downstream metre meaningless.
    """
    bad = ~np.isfinite(elevation)
    if nodata is not None:
        bad |= np.isclose(elevation, nodata)
    # Anything below the Dead Sea shore or above Everest is a sentinel, not
    # ground.
    bad |= (elevation < -450.0) | (elevation > 8900.0)
    if not bad.any():
        return elevation
    good = elevation[~bad]
    if good.size == 0:
        raise SystemExit("every cell in this DEM is nodata")
    filled = elevation.copy()
    filled[bad] = float(good.min())
    return filled


def crop_square(elevation: np.ndarray, row: int, column: int, side: int) -> np.ndarray:
    half = side // 2
    row = int(np.clip(row, half, elevation.shape[0] - half - 1))
    column = int(np.clip(column, half, elevation.shape[1] - half - 1))
    return elevation[row - half : row + half + 1, column - half : column + half + 1].copy()


def resample(heights: np.ndarray, target: int) -> np.ndarray:
    if heights.shape[0] == target:
        return heights
    from PIL import Image

    with Image.fromarray(heights.astype(np.float32), mode="F") as image:
        return np.asarray(
            image.resize((target, target), Image.Resampling.BILINEAR)
        ).astype(np.float32)


def import_dem(
    source: Path,
    output: Path,
    *,
    side_m: float | None = None,
    centre: tuple[int, int] | None = None,
    cell_m: float | None = None,
    nodata: float | None = None,
    vertical_exaggeration: float = 1.0,
) -> dict:
    elevation, geometry = read_geotiff(source)
    metres_x, metres_y, basis = ground_sample_metres(geometry, cell_m)
    elevation = clean_voids(elevation, nodata)

    # Non-square pixels are normal in geographic CRSs. Resolve to the finer of
    # the two so nothing is invented, then let the square crop do the rest.
    ground_sample = min(metres_x, metres_y)
    if not math.isclose(metres_x, metres_y, rel_tol=0.02):
        from PIL import Image

        target_rows = int(round(elevation.shape[0] * metres_y / ground_sample))
        target_columns = int(round(elevation.shape[1] * metres_x / ground_sample))
        with Image.fromarray(elevation.astype(np.float32), mode="F") as image:
            elevation = np.asarray(
                image.resize((target_columns, target_rows), Image.Resampling.BILINEAR)
            ).astype(np.float64)

    side_cells = elevation.shape[0]
    if side_m is not None:
        side_cells = max(int(round(side_m / ground_sample)) | 1, 9)
    side_cells = min(side_cells, min(elevation.shape) - 1 | 1)

    row, column = centre or (elevation.shape[0] // 2, elevation.shape[1] // 2)
    patch = crop_square(elevation, row, column, side_cells)

    source_resolution = patch.shape[0]
    patch = resample(patch, stride_compatible(source_resolution))
    resolution = patch.shape[0]
    world_m = (source_resolution - 1) * ground_sample

    floor = float(patch.min())
    heights = ((patch - floor) * vertical_exaggeration).astype("<f4")

    terrain = output / "terrain"
    terrain.mkdir(parents=True, exist_ok=True)
    raw = heights.tobytes()
    (terrain / "heightfield_f32le.bin").write_bytes(raw)

    manifest = {
        "schema_version": DEM_SCHEMA,
        "preview": True,
        "zone_id": output.name,
        "resolution": resolution,
        "world_bounds_m": {"width": world_m, "length": world_m},
        "height_range_m": {
            "min": round(float(heights.min()), 3),
            "max": round(float(heights.max()), 3),
        },
        "heightfield_sha256": hashlib.sha256(raw).hexdigest(),
        "artifacts": {"heightfield_f32le": "heightfield_f32le.bin"},
        # Real elevation, so the datum is a fact rather than an assertion --
        # the opposite of import_heightfield's `--relief-m`. Recorded because
        # re-basing to zero throws away where on Earth this ground was.
        "imported_from": {
            "path": str(source),
            "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "kind": "dem",
            "ground_sample_m": round(ground_sample, 4),
            "ground_sample_basis": basis,
            "base_elevation_m": round(floor, 3),
            "vertical_exaggeration": vertical_exaggeration,
            "source_resolution": source_resolution,
            "resampled": source_resolution != resolution,
            "ingest_version": INGEST_VERSION,
        },
    }
    (terrain / "terrain_manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dem", type=Path, help="a GeoTIFF elevation raster")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--side-m", type=float, help="crop this many metres square")
    parser.add_argument(
        "--centre", help="row,column of the crop centre; defaults to the raster centre"
    )
    parser.add_argument(
        "--cell-m",
        type=float,
        help="assert metres per pixel, for a DEM that does not declare its CRS",
    )
    parser.add_argument("--nodata", type=float, help="sentinel value to treat as void")
    parser.add_argument(
        "--vertical-exaggeration",
        type=float,
        default=1.0,
        help="scale relief; 1.0 is true to the source and is the default",
    )
    arguments = parser.parse_args()

    centre = None
    if arguments.centre:
        row, _, column = arguments.centre.partition(",")
        centre = (int(row), int(column))

    manifest = import_dem(
        arguments.dem,
        arguments.output,
        side_m=arguments.side_m,
        centre=centre,
        cell_m=arguments.cell_m,
        nodata=arguments.nodata,
        vertical_exaggeration=arguments.vertical_exaggeration,
    )
    imported = manifest["imported_from"]
    print(
        "dem %s: %d^2 over %.0f m (%.2f m/px, %s), relief %.1f m from %.0f m base"
        % (
            arguments.output.name,
            manifest["resolution"],
            manifest["world_bounds_m"]["width"],
            imported["ground_sample_m"],
            imported["ground_sample_basis"],
            manifest["height_range_m"]["max"],
            imported["base_elevation_m"],
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
