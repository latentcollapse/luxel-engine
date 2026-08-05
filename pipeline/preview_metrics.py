#!/usr/bin/env python3
"""Terrain-intrinsic measurements over a heightfield preview.

**This does not certify anything, and its schema is deliberately distinct so
that no downstream tool can mistake it for an acceptance report.** Visual
acceptance gates measure compiled worlds -- foliage fractions, road fractions,
framing -- and several of them are frame-relative. Pointed at raw terrain they
produce numbers that mean nothing, wearing the costume of certification.

What is left when you remove everything that needs a plan, a placement or a
ZoneSpec is still worth measuring, because it is exactly the class of property
the last two terrain attempts got wrong while their other metrics improved:

**Radial monotonicity** is the important one (GAEA_PROGRAMME.md 5.5). The massif
carve made relief a monotonic function of distance from the lanes, so a nearer
summit could never be taller than a farther one and foothills in front of peaks
were mathematically forbidden. The Gaea programme's basin macro-shape is the same
construction in polar coordinates, and is safe only insofar as erosion is strong
enough to break the monotonicity the bias imposes. That is an empirical question.
A pure cone scores 1.0; real eroded terrain scores far below it; anything scoring
like a cone is a wall that happens to be curved.

**Symmetry residual** supports the 5.1 decision to composite the macro-shape
before erosion and merely *measure* the result, rather than blending a rotated
copy afterwards and averaging two independent drainage networks together.

**Sink density** is a cheap drainage sanity check. Eroded terrain drains: it has
few interior local minima. Noise with a nice histogram has many. It is a proxy,
not a hydrology model, and is reported as one.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np

# Distinct from `codeweald.terrain-artifacts/v1` on purpose. A consumer looking
# for certification must not find something shaped like it here.
PREVIEW_METRICS_SCHEMA = "codeweald.preview-metrics/v1"

# Height differences below this are noise in a float32 heightfield, not slope.
FLAT_EPSILON_M = 1e-4


def radial_monotonicity(
    heights: np.ndarray, rays: int = 360, epsilon_m: float = FLAT_EPSILON_M
) -> dict:
    """How cone-like the field is, measured from the centre outward.

    **Read `mean_strictly_rising_rim` first.** The other two numbers are kept
    because they are cheap and occasionally informative, but both mislead on the
    design this metric exists to judge:

    - `mean_non_descending_fraction` counts flat ground as monotonic. A broad
      flat valley floor -- precisely what a seated arena wants -- therefore reads
      as cone-like. Measured on a strong basin it returned 0.90 while the same
      field scored 0.55 on strict ascent, identical to no basin at all.
    - `fully_monotonic_ray_fraction` saturates at zero for anything carrying real
      detail, so it separates a cone from terrain and nothing else.

    The rim figure asks the question §5.5 actually cares about: **outside the
    floor, does height rise with distance in the way a ramp does?** A pure cone
    gives 1.0. Eroded terrain with no radial bias measured 0.55.
    """
    resolution = heights.shape[0]
    centre = (resolution - 1) / 2.0

    def sweep(inner: float, outer: float, strict: bool) -> np.ndarray:
        # One sample per cell along the band. Sampling finer than the grid makes
        # consecutive samples land on the same cell, and a zero delta fails a
        # strict-ascent test -- so an oversampled *perfect cone* scored 0.56
        # rather than ~0.95, and the metric was reading its own step size.
        samples = max(8, int((outer - inner) * centre))
        steps = np.linspace(inner * centre, outer * centre, samples)
        out = []
        for angle in np.linspace(0.0, 2.0 * np.pi, rays, endpoint=False):
            rows = np.clip(np.rint(centre + steps * np.sin(angle)), 0, resolution - 1)
            columns = np.clip(np.rint(centre + steps * np.cos(angle)), 0, resolution - 1)
            deltas = np.diff(heights[rows.astype(np.intp), columns.astype(np.intp)])
            out.append(
                float(np.mean(deltas > epsilon_m))
                if strict
                else float(np.mean(deltas >= -epsilon_m))
            )
        return np.asarray(out)

    non_descending = sweep(0.0, 1.0, False)
    return {
        "mean_non_descending_fraction": round(float(non_descending.mean()), 4),
        "fully_monotonic_ray_fraction": round(float(np.mean(non_descending >= 0.999)), 4),
        "mean_strictly_rising": round(float(sweep(0.0, 1.0, True).mean()), 4),
        # The rim is where a basin becomes a wall, and where the floor's
        # flatness cannot inflate the answer.
        "mean_strictly_rising_rim": round(float(sweep(0.5, 1.0, True).mean()), 4),
        "rays": rays,
    }


def symmetry_residual(heights: np.ndarray, relief_m: float) -> dict:
    """Departure from 180deg rotational symmetry, as a fraction of relief.

    `rot180` is an exact involution on the grid at both parities, so this needs
    no resolution-parity special case.
    """
    difference = np.abs(heights - heights[::-1, ::-1])
    scale = max(relief_m, FLAT_EPSILON_M)
    return {
        "max_fraction_of_relief": round(float(difference.max() / scale), 6),
        "p99_fraction_of_relief": round(
            float(np.percentile(difference, 99) / scale), 6
        ),
    }


def slope_stats(heights: np.ndarray, cell_m: float) -> dict:
    """Grade (rise over run) as a percentage, from the steeper of the two axes."""
    rows, columns = np.gradient(heights, cell_m)
    grade = np.hypot(rows, columns) * 100.0
    return {
        "p50_grade": round(float(np.percentile(grade, 50)), 3),
        "p99_grade": round(float(np.percentile(grade, 99)), 3),
        "max_grade": round(float(grade.max()), 3),
    }


def sink_density(heights: np.ndarray, epsilon_m: float = FLAT_EPSILON_M) -> dict:
    """Interior local minima per 1000 cells -- a drainage plausibility proxy.

    A cell is a sink when all eight neighbours are higher. Eroded terrain drains
    and so has few; uncorrelated noise has many.
    """
    interior = heights[1:-1, 1:-1]
    lower_than_all = np.ones_like(interior, dtype=bool)
    for row_shift in (-1, 0, 1):
        for column_shift in (-1, 0, 1):
            if row_shift == 0 and column_shift == 0:
                continue
            neighbour = heights[
                1 + row_shift : heights.shape[0] - 1 + row_shift,
                1 + column_shift : heights.shape[1] - 1 + column_shift,
            ]
            lower_than_all &= interior < neighbour - epsilon_m
    return {
        "sinks_per_1000_cells": round(
            float(lower_than_all.sum()) / max(interior.size, 1) * 1000.0, 4
        ),
        "sink_count": int(lower_than_all.sum()),
    }


def measure(heights: np.ndarray, world_m: float) -> dict:
    relief_m = float(heights.max() - heights.min())
    cell_m = world_m / max(heights.shape[0] - 1, 1)
    return {
        "schema_version": PREVIEW_METRICS_SCHEMA,
        # Said in the payload as well as the docstring, because the payload is
        # what a future tool reads.
        "certifies": False,
        "note": (
            "Terrain-intrinsic measurements over an uncertified preview. "
            "Not an acceptance result and cannot stand in for one."
        ),
        "resolution": int(heights.shape[0]),
        "world_m": world_m,
        "cell_m": round(cell_m, 4),
        "relief_m": round(relief_m, 3),
        "radial_monotonicity": radial_monotonicity(heights),
        "symmetry_residual": symmetry_residual(heights, relief_m),
        "slope": slope_stats(heights, cell_m),
        "drainage": sink_density(heights),
    }


def read_preview(batch: Path) -> tuple[np.ndarray, dict]:
    manifest = json.loads((batch / "terrain" / "terrain_manifest.json").read_text())
    resolution = int(manifest["resolution"])
    raw = (batch / "terrain" / "heightfield_f32le.bin").read_bytes()
    expected = resolution * resolution * 4
    if len(raw) != expected:
        raise SystemExit(
            f"heightfield is {len(raw)} bytes; the manifest's {resolution}^2 "
            f"implies {expected}"
        )
    heights = np.frombuffer(raw, dtype="<f4").reshape(resolution, resolution)
    return heights, manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("batch", type=Path, help="a preview batch directory")
    parser.add_argument("--output", type=Path, help="defaults to <batch>/preview_metrics.json")
    arguments = parser.parse_args()

    heights, manifest = read_preview(arguments.batch)
    world_m = float(manifest["world_bounds_m"]["width"])
    metrics = measure(heights, world_m)
    metrics["zone_id"] = manifest.get("zone_id")
    metrics["heightfield_sha256"] = manifest.get("heightfield_sha256")

    destination = arguments.output or arguments.batch / "preview_metrics.json"
    destination.write_text(
        json.dumps(metrics, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    radial = metrics["radial_monotonicity"]
    print(
        "%s: relief %.1f m, radial monotonic rays %.1f%% (mean %.2f), "
        "symmetry residual p99 %.4f, sinks/1000 %.2f"
        % (
            arguments.batch.name,
            metrics["relief_m"],
            radial["fully_monotonic_ray_fraction"] * 100.0,
            radial["mean_non_descending_fraction"],
            metrics["symmetry_residual"]["p99_fraction_of_relief"],
            metrics["drainage"]["sinks_per_1000_cells"],
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
