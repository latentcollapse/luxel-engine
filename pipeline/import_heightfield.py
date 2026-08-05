#!/usr/bin/env python3
"""Turn an externally authored heightfield into something the viewer can fly.

Written for Gaea's `PNG16` exports, but the input is just a 16-bit greyscale
image or a raw float32 buffer, so it serves any terrain tool.

**This deliberately does NOT produce a certified world.** The viewer requires
seven artifacts for a compiled batch and runs worldspec validation across them;
that validation is what caught D32, where the render plan was bound to a stale
canopy field. Synthesising a passing `zone_spec.json` and `render_plan.json`
just to get a preview would put a lie into the one mechanism that has been
reliably catching our mistakes. So this writes a **preview**: terrain and
nothing else, marked as such, with `preview: true` in the manifest and no
provenance claims it cannot support.

The vertical mapping is the part worth understanding. A 16-bit image carries no
units -- Gaea's own height range is a property of its graph, not of the file --
so `--relief-m` is the caller asserting how tall the world is. Deriving it from
the data instead would silently rescale a world every time its noise happened to
land differently, which is exactly the class of thing this project keeps getting
bitten by.

So the mapping is **full scale**: `height_m = raw / dtype_max * relief_m`. The
denominator is a property of the encoding, not of the image's contents, which is
what makes `--relief-m` an assertion about the world rather than a caption on
whatever the noise did this time. Two builds of one graph that differ in height
stay different after import. `--normalize` opts into the old min/max stretch for
genuinely unknown-range sources, and the manifest records which was used,
because the mapping is a declared input: the same bytes under the two rules are
two different worlds and the digest alone cannot tell them apart.

Resolution is the other trap. The viewer meshes terrain on a fixed stride and
rejects any grid where `(resolution - 1) % TERRAIN_STRIDE != 0`; WGE's own
compiler emits 1025 for that reason. Gaea builds at powers of two, so **every**
Gaea-native resolution fails that check. The importer resamples up to the next
compatible grid and records both resolutions, rather than writing a file that
looks fine and cannot be opened.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import numpy as np

PREVIEW_SCHEMA = "codeweald.terrain-artifacts/v1"

# Mirrors TERRAIN_STRIDE in world_core/apps/world_viewer/src/main.rs. The viewer
# decimates the heightfield by this factor to build its mesh and refuses a grid
# it cannot decimate evenly. Duplicated deliberately and named so, because the
# alternative is emitting previews that no consumer can open.
VIEWER_TERRAIN_STRIDE = 4

# Bumped whenever the pixels-to-metres rule changes. Pinned into the manifest so
# a preview that disagrees with today's importer is detectable rather than
# merely wrong.
IMPORTER_VERSION = 2


def stride_compatible(resolution: int, stride: int = VIEWER_TERRAIN_STRIDE) -> int:
    """The smallest grid >= `resolution` the viewer will mesh.

    Gaea builds at powers of two, and `2**k - 1` is never divisible by 4, so
    this always moves 512/1024/2048 up by one to 513/1025/2049.
    """
    remainder = (resolution - 1) % stride
    return resolution if remainder == 0 else resolution + (stride - remainder)


def read_heightfield(
    path: Path, resolution: int | None, normalize: bool
) -> tuple[np.ndarray, str]:
    """Load a heightfield as float32 in 0..1, with the mapping that produced it.

    Integer images are read against their encoding's full scale, so the result
    is the graph's own height range rather than a stretch of whatever this
    particular build happened to occupy. Float inputs (Gaea's `R32`) are already
    unit-ranged and are only checked.
    """
    if path.suffix.lower() in {".r32", ".raw", ".bin"}:
        raw = np.fromfile(path, dtype="<f4")
        side = resolution or int(round(len(raw) ** 0.5))
        if side * side != len(raw):
            raise SystemExit(
                f"{path.name} holds {len(raw)} floats, which is not a square grid; "
                "pass --resolution"
            )
        data = raw.reshape(side, side).astype(np.float64)
        full_scale = 1.0
    else:
        from PIL import Image

        with Image.open(path) as source:
            if source.mode not in {"I;16", "I", "L", "F"}:
                source = source.convert("I")
            data = np.asarray(source).astype(np.float64)
            sample = np.asarray(source)
        if data.ndim != 2:
            raise SystemExit(f"{path.name} is not a single-channel image")
        # The denominator is a property of the encoding. `I` is PIL's 32-bit
        # container for what is still 16-bit data off a PNG16, so key off the
        # observed dtype only to separate 8-bit from everything else.
        full_scale = 255.0 if sample.dtype == np.uint8 else 65535.0
        if sample.dtype.kind == "f":
            full_scale = 1.0

    if data.shape[0] != data.shape[1]:
        raise SystemExit(
            f"{path.name} is {data.shape[1]}x{data.shape[0]}; heightfields must be "
            "square -- a non-square grid writes a buffer no consumer can decode"
        )

    span = float(data.max()) - float(data.min())
    if span <= 0.0:
        raise SystemExit(f"{path.name} is perfectly flat; nothing to preview")

    if normalize:
        return ((data - data.min()) / span).astype(np.float32), "min_max"

    if data.min() < -1e-6 or data.max() > full_scale * (1.0 + 1e-6):
        raise SystemExit(
            f"{path.name} holds values outside 0..{full_scale:g}; pass --normalize "
            "if the source range is genuinely unknown"
        )
    return (data / full_scale).astype(np.float32), "full_scale"


def resample(unit_heights: np.ndarray, target: int) -> np.ndarray:
    """Bilinear resample to a square `target` grid.

    Only ever a move of one row and column in practice, but it is a real edit to
    the data and so is recorded in the manifest rather than done silently.
    """
    if unit_heights.shape[0] == target:
        return unit_heights
    from PIL import Image

    with Image.fromarray(unit_heights, mode="F") as image:
        resized = image.resize((target, target), Image.Resampling.BILINEAR)
        return np.asarray(resized).astype(np.float32)


def write_preview(
    unit_heights: np.ndarray,
    destination: Path,
    world_m: float,
    relief_m: float,
    source: Path,
    source_digest: str,
    vertical_mapping: str,
    source_resolution: int,
) -> dict:
    terrain = destination / "terrain"
    terrain.mkdir(parents=True, exist_ok=True)

    resolution = unit_heights.shape[0]
    heights = (unit_heights * relief_m).astype("<f4")
    raw = heights.tobytes()
    (terrain / "heightfield_f32le.bin").write_bytes(raw)

    manifest = {
        "schema_version": PREVIEW_SCHEMA,
        # The flag the viewer keys off. A preview is terrain only -- no plans,
        # no placements, no certification -- and must never be mistaken for a
        # compiled world in a report or a capture.
        "preview": True,
        "zone_id": destination.name,
        "resolution": resolution,
        "world_bounds_m": {"width": world_m, "length": world_m},
        "height_range_m": {
            "min": round(float(heights.min()), 3),
            "max": round(float(heights.max()), 3),
        },
        "heightfield_sha256": hashlib.sha256(raw).hexdigest(),
        "artifacts": {"heightfield_f32le": "heightfield_f32le.bin"},
        # Pinned so a preview can be traced to the exact bytes it came from --
        # the same rule open decisions item 9 sets for real Gaea imports.
        #
        # Everything needed to reproduce these bytes from the source file. The
        # digest alone is not enough: the same image under `full_scale` and
        # under `min_max` is two different worlds, and the resample is a real
        # edit. Determinism is graded over *equal declared inputs* (language
        # spec section 17), so an input that is not declared here is a hole.
        "imported_from": {
            "path": str(source),
            "sha256": source_digest,
            "relief_m": relief_m,
            "world_m": world_m,
            "vertical_mapping": vertical_mapping,
            "source_resolution": source_resolution,
            "resampled": source_resolution != resolution,
            "importer_version": IMPORTER_VERSION,
        },
    }
    (terrain / "terrain_manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return manifest


def import_heightfield(
    source: Path,
    output: Path,
    *,
    world_m: float,
    relief_m: float,
    resolution: int | None = None,
    normalize: bool = False,
) -> dict:
    """Read `source`, write a preview batch under `output`, return the manifest."""
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    unit, mapping = read_heightfield(source, resolution, normalize)
    source_resolution = unit.shape[0]
    unit = resample(unit, stride_compatible(source_resolution))
    return write_preview(
        unit, output, world_m, relief_m, source, digest, mapping, source_resolution
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("heightfield", type=Path)
    parser.add_argument("--output", type=Path, required=True, help="preview directory")
    parser.add_argument(
        "--world-m", type=float, default=1024.0, help="world width/length in metres"
    )
    parser.add_argument(
        "--relief-m",
        type=float,
        default=300.0,
        help="metres between the lowest and highest point",
    )
    parser.add_argument("--resolution", type=int, help="only needed for raw .r32 input")
    parser.add_argument(
        "--normalize",
        action="store_true",
        help="stretch the observed min/max to the full relief instead of reading "
        "the encoding's full scale; only for sources whose range is unknown",
    )
    arguments = parser.parse_args()

    manifest = import_heightfield(
        arguments.heightfield,
        arguments.output,
        world_m=arguments.world_m,
        relief_m=arguments.relief_m,
        resolution=arguments.resolution,
        normalize=arguments.normalize,
    )
    imported = manifest["imported_from"]
    note = (
        " (resampled from %d^2 for the viewer's stride)" % imported["source_resolution"]
        if imported["resampled"]
        else ""
    )
    print(
        "preview %s: %d^2 over %.0f m, relief %.1f m via %s%s"
        % (
            arguments.output.name,
            manifest["resolution"],
            arguments.world_m,
            manifest["height_range_m"]["max"],
            imported["vertical_mapping"],
            note,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
