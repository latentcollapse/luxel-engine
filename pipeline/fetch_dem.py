#!/usr/bin/env python3
"""Fetch real elevation for a place and write it as a GeoTIFF.

Feeds `import_dem.py`, which is deliberately unaware of where its GeoTIFF came
from -- this could equally be a USGS 3DEP tile, a Copernicus download, or a
survey. Keeping fetch and ingest separate means the ingest path stays testable
offline and the network lives in exactly one module.

**Source: AWS Terrain Tiles** (`elevation-tiles-prod`), the public
Mapzen/Tilezen terrarium set. Chosen because it needs no API key, covers the
globe, and is a documented public dataset -- a pipeline stage that requires a
personal credential is one that only works on the machine that set it up.

Terrarium encodes elevation in RGB: `(R * 256 + G + B / 256) - 32768` metres.
That is a real encoding, not a heuristic -- 1/256 m vertical quantisation, which
is far finer than the horizontal resolution justifies.

**Its resolution is what it is.** Terrain tiles are assembled from SRTM and
national datasets; at zoom 12 a pixel is roughly 25 m at 49 degrees latitude.
That is fine for a valley's *shape* and useless for a boulder. Where 1 m lidar
matters, download 3DEP and point `import_dem.py` at it instead; nothing else in
the chain changes.
"""

from __future__ import annotations

import argparse
import math
import urllib.request
from pathlib import Path

import numpy as np

TILE_URL = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png"
TILE_PIXELS = 256

# Web Mercator ground resolution at the equator, metres per pixel at zoom 0.
EQUATOR_METRES_PER_PIXEL = 156543.03392


def tile_of(latitude: float, longitude: float, zoom: int) -> tuple[int, int]:
    span = 2**zoom
    radians = math.radians(latitude)
    x = int((longitude + 180.0) / 360.0 * span)
    y = int(
        (1.0 - math.log(math.tan(radians) + 1.0 / math.cos(radians)) / math.pi)
        / 2.0
        * span
    )
    return x, y


def ground_sample_m(latitude: float, zoom: int) -> float:
    """Metres per pixel, which in Web Mercator depends on latitude."""
    return EQUATOR_METRES_PER_PIXEL * math.cos(math.radians(latitude)) / (2**zoom)


def decode_terrarium(image: np.ndarray) -> np.ndarray:
    channels = image.astype(np.float64)
    return (
        channels[:, :, 0] * 256.0 + channels[:, :, 1] + channels[:, :, 2] / 256.0
    ) - 32768.0


def fetch_tile(x: int, y: int, zoom: int, timeout: float) -> np.ndarray:
    from PIL import Image
    import io

    url = TILE_URL.format(z=zoom, x=x, y=y)
    with urllib.request.urlopen(url, timeout=timeout) as response:
        payload = response.read()
    with Image.open(io.BytesIO(payload)) as tile:
        return decode_terrarium(np.asarray(tile.convert("RGB")))


def fetch_area(
    latitude: float, longitude: float, zoom: int, tiles: int, timeout: float = 30.0
) -> np.ndarray:
    """A `tiles` x `tiles` block of terrarium tiles, centred on the coordinate."""
    centre_x, centre_y = tile_of(latitude, longitude, zoom)
    half = tiles // 2
    rows = []
    for row in range(tiles):
        columns = []
        for column in range(tiles):
            x = centre_x - half + column
            y = centre_y - half + row
            columns.append(fetch_tile(x, y, zoom, timeout))
        rows.append(np.hstack(columns))
    return np.vstack(rows)


def write_geotiff(path: Path, elevation: np.ndarray, cell_m: float) -> None:
    """A projected GeoTIFF, so `import_dem` reads metres without converting.

    Written as projected rather than geographic on purpose: the tiles are Web
    Mercator and already resolved to metres here, so declaring degrees would
    make the ingest re-apply a latitude correction that has already been made.
    """
    from PIL import Image
    from PIL.TiffImagePlugin import ImageFileDirectory_v2

    directory = ImageFileDirectory_v2()
    directory[33550] = (cell_m, cell_m, 0.0)
    directory[33922] = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
    directory[34735] = (1, 1, 0, 1, 1024, 0, 1, 1)  # GTModelType = projected
    directory.tagtype[33550] = 12
    directory.tagtype[33922] = 12
    directory.tagtype[34735] = 3
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(elevation.astype(np.float32), mode="F").save(
        path, tiffinfo=directory
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lat", type=float, required=True)
    parser.add_argument("--lon", type=float, required=True)
    parser.add_argument("--zoom", type=int, default=12, help="12 is ~25 m/px at 49 deg")
    parser.add_argument("--tiles", type=int, default=3, help="NxN tiles to stitch")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=30.0)
    arguments = parser.parse_args()

    elevation = fetch_area(
        arguments.lat, arguments.lon, arguments.zoom, arguments.tiles, arguments.timeout
    )
    cell = ground_sample_m(arguments.lat, arguments.zoom)
    write_geotiff(arguments.output, elevation, cell)
    print(
        "fetched %d^2 px at %.2f m/px (%.1f km across), elevation %.0f..%.0f m -> %s"
        % (
            elevation.shape[0],
            cell,
            elevation.shape[0] * cell / 1000.0,
            elevation.min(),
            elevation.max(),
            arguments.output.name,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
