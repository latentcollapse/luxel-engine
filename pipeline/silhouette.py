"""Silhouette complexity: an honest target for the geometry knobs.

Tooling item 5 (see WGE/docs/platform/tooling-upgrades.md). `spine_count` and the jitters
reach the heightfield and demonstrably move it, but the only authority ever
*measured* for `spine_count` was over `water_fraction` -- drainage, not skyline
(+79.7%, sensitivity matrix 2026-07-31). Every metric that sounds like it should
capture landform shape turned out to be a texture metric. So the composition
knobs have had no target to optimise toward, and no automated loop can improve
the shape of a world it cannot measure.

**Measured from the heightfield, not from a render, and that is a departure from
the item as written.** The item proposed the terrain/sky boundary in a capture.
A rendered skyline is occluded by trees and buildings, depends on where the
camera happens to point, and needs sky segmentation that fails on a bright
horizon. More importantly it would mix material and foliage signal into a number
whose entire purpose is to attribute *geometry*. Tooling item 6 has just
finished establishing what happens when a metric's signal comes from somewhere
other than the thing it is named for: `detail_density` scores an un-filtered
render 8x above a correct one, and the critic inherited an incentive to remove
mipmaps. A skyline metric that trees can move would be the same mistake with a
new name.

What an observer standing in the world actually sees on the skyline is the
*horizon angle*: for each direction, the highest elevation angle any terrain
along that ray subtends. Sweeping that around a full turn gives a horizon
profile, and the shape of that profile is the silhouette.

Three numbers, because "complexity" is not one property:

- `horizon_relief_deg` -- how far the skyline rises and falls. A flat plain
  scores ~0 however many ridges are authored, because none of them break the
  horizon.
- `horizon_peaks_per_turn` -- how many distinct summits an observer can count.
  This is the one `spine_count` should move, and the test suite pins that it
  does.
- `horizon_roughness` -- the share of profile variance above the coarse scale,
  which separates a serrated ridge from a smooth dome.

Every one carries an adversarial control, per item 6: `honesty()` scores them on
a flat field, on noise, and on authored ridges. A silhouette metric that noise
can ace would be a roughness gate wearing a shape-metric name.
"""

from __future__ import annotations

import json
import math
from pathlib import Path
from typing import Any

import numpy as np

SILHOUETTE_VERSION = "codeweald.silhouette/v1"

# Directions sampled around a full turn. One per degree: finer than an observer
# can distinguish summits at, coarse enough to stay cheap.
AZIMUTH_COUNT = 360

# Samples along each ray. The ray is walked to the world edge, so this sets the
# distance resolution rather than the range.
RAY_SAMPLES = 160

# Observer eye height above the ground beneath them, in metres. From
# `traversal_policy.agent_height_m` when available; a skyline measured from
# ground level is a different skyline.
DEFAULT_EYE_M = 8.0

# Viewpoints are taken on a ring at this fraction of the world's half-extent.
# Measuring from the centre alone rewards a world with one central massif and
# nothing else; measuring from a ring is what an observer moving through the
# map experiences.
VIEWPOINT_RING_FRACTION = 0.45
VIEWPOINT_COUNT = 8

# A summit has to clear its neighbouring trough by this much to be counted, so
# that sensor noise on a smooth dome is not read as a dozen peaks.
PEAK_PROMINENCE_DEG = 0.75

# Angular scale at which two bumps stop being distinguishable as separate
# summits. The profile is smoothed to this before peaks are counted, so the
# metric measures shape rather than roughness.
ACUITY_DEGREES = 5.0

# Profile harmonics at or below this index are the "coarse" shape; everything
# above is roughness.
COARSE_HARMONICS = 6


class SilhouetteError(ValueError):
    """The silhouette cannot be measured from these artifacts."""


def _bilinear(heights: np.ndarray, x: np.ndarray, z: np.ndarray) -> np.ndarray:
    """Sample the heightfield at fractional cell coordinates."""
    rows, columns = heights.shape
    x = np.clip(x, 0.0, columns - 1.001)
    z = np.clip(z, 0.0, rows - 1.001)
    x0, z0 = np.floor(x).astype(int), np.floor(z).astype(int)
    x1, z1 = x0 + 1, z0 + 1
    tx, tz = x - x0, z - z0
    return (
        heights[z0, x0] * (1 - tx) * (1 - tz)
        + heights[z0, x1] * tx * (1 - tz)
        + heights[z1, x0] * (1 - tx) * tz
        + heights[z1, x1] * tx * tz
    )


def horizon_profile(
    heights: np.ndarray,
    cell_m: float,
    viewpoint_cell: tuple[float, float],
    eye_m: float = DEFAULT_EYE_M,
    azimuths: int = AZIMUTH_COUNT,
    samples: int = RAY_SAMPLES,
) -> np.ndarray:
    """Highest elevation angle visible in each direction, in degrees.

    This is what actually forms a skyline: not the tallest peak, but the
    steepest angle anything subtends along a line of sight. A distant mountain
    and a nearby hillock can occupy the same place on the horizon, and an
    observer cannot tell them apart -- which is the point.
    """
    rows, columns = heights.shape
    row, column = viewpoint_cell
    eye = float(_bilinear(heights, np.array([column]), np.array([row]))[0]) + eye_m

    angle = np.linspace(0.0, 2.0 * math.pi, azimuths, endpoint=False)
    # Walk each ray to the far side of the world rather than a fixed range, so
    # the profile does not silently truncate on a large map.
    reach = math.hypot(rows, columns)
    step = np.linspace(1.0, reach, samples)[:, None]

    sample_columns = column + np.cos(angle)[None, :] * step
    sample_rows = row + np.sin(angle)[None, :] * step
    inside = (
        (sample_columns >= 0)
        & (sample_columns <= columns - 1)
        & (sample_rows >= 0)
        & (sample_rows <= rows - 1)
    )
    sampled = _bilinear(heights, sample_columns, sample_rows)
    distance = step * cell_m
    elevation = np.degrees(np.arctan2(sampled - eye, distance))
    elevation = np.where(inside, elevation, 0.0)
    # Floored at the true horizon. Ground *below* eye level does not form a
    # skyline -- it falls away to the horizon at 0 degrees however far off it
    # is. Without this floor a flat world scores 1.26 degrees of relief and
    # three peaks, because the deepest negative angle depends on how far the
    # ray travels before leaving the map: the metric measures the map's
    # rectangular boundary rather than its landform. Caught by `honesty()`.
    return np.maximum(elevation.max(axis=0), 0.0)


def _count_peaks(profile: np.ndarray, prominence_deg: float = PEAK_PROMINENCE_DEG) -> int:
    """Distinct summits on a circular profile, by prominence.

    Prominence rather than a bare local maximum: a smooth dome carries dozens of
    numerically-local maxima and an observer sees one hill.
    """
    count = len(profile)
    if count < 3:
        return 0
    peaks = 0
    for index in range(count):
        here = profile[index]
        if here < profile[index - 1] or here < profile[(index + 1) % count]:
            continue
        # Walk out both ways to the lower of the two flanking troughs.
        left = here
        for step in range(1, count):
            value = profile[(index - step) % count]
            left = min(left, value)
            if value > here:
                break
        right = here
        for step in range(1, count):
            value = profile[(index + step) % count]
            right = min(right, value)
            if value > here:
                break
        if here - max(left, right) >= prominence_deg:
            peaks += 1
    return peaks


def _angular_smooth(profile: np.ndarray, window_degrees: float = ACUITY_DEGREES) -> np.ndarray:
    """Blur the profile to the angular scale a summit is distinguishable at.

    Without this, per-cell noise is counted as summits: a noise field scored
    72.8 peaks per turn against an eight-ridge world's 16, so the metric would
    have rewarded roughness over shape -- precisely the failure tooling item 6
    found in `detail_density`. Two bumps a third of a degree apart are one
    summit to an observer, and should be one summit here.
    """
    span = max(1, int(round(window_degrees * len(profile) / 360.0)))
    if span <= 1:
        return profile
    kernel = np.ones(span) / span
    wrapped = np.concatenate([profile[-span:], profile, profile[:span]])
    return np.convolve(wrapped, kernel, mode="same")[span : span + len(profile)]


def profile_metrics(profile: np.ndarray) -> dict[str, float]:
    """The three silhouette numbers for one horizon profile."""
    profile = _angular_smooth(profile)
    spectrum = np.abs(np.fft.rfft(profile - profile.mean())) ** 2
    total = float(spectrum.sum())
    fine = float(spectrum[COARSE_HARMONICS + 1 :].sum())
    # Share of the profile's variance carried by its single strongest harmonic.
    # This is what separates authored landform from roughness: ridges put their
    # energy in one place, noise spreads it across every harmonic. Peak count
    # alone cannot make that distinction -- a noise field scores 22.5 summits
    # per turn against an eight-ridge world's 16 -- so a loop optimising peaks
    # would be rewarded for adding grain instead of shape.
    strongest = float(spectrum[1:].max()) if spectrum.size > 1 else 0.0
    return {
        "horizon_relief_deg": float(profile.std()),
        "horizon_peaks_per_turn": float(_count_peaks(profile)),
        "horizon_coherence": float(strongest / total) if total > 1e-12 else 0.0,
        "horizon_roughness": float(fine / total) if total > 1e-12 else 0.0,
        "horizon_max_deg": float(profile.max()),
    }


def measure(
    heights: np.ndarray,
    cell_m: float,
    eye_m: float = DEFAULT_EYE_M,
    viewpoints: int = VIEWPOINT_COUNT,
    ring_fraction: float = VIEWPOINT_RING_FRACTION,
) -> dict[str, Any]:
    """Silhouette metrics averaged over a ring of observers.

    A single central viewpoint rewards a world with one big massif and nothing
    else. A ring is closer to what someone crossing the map actually sees.
    """
    rows, columns = heights.shape
    centre_row, centre_column = (rows - 1) / 2.0, (columns - 1) / 2.0
    radius = min(centre_row, centre_column) * ring_fraction

    per_viewpoint: list[dict[str, float]] = []
    for index in range(viewpoints):
        angle = 2.0 * math.pi * index / viewpoints
        viewpoint = (
            centre_row + math.sin(angle) * radius,
            centre_column + math.cos(angle) * radius,
        )
        profile = horizon_profile(heights, cell_m, viewpoint, eye_m=eye_m)
        per_viewpoint.append(profile_metrics(profile))

    keys = per_viewpoint[0].keys()
    return {
        key: round(float(np.mean([entry[key] for entry in per_viewpoint])), 6)
        for key in keys
    }


# --- adversarial controls (tooling item 6) --------------------------------

PROBE_SIZE = 257
PROBE_CELL_M = 1.0
PROBE_SEED = 20260801


def flat_field() -> np.ndarray:
    return np.zeros((PROBE_SIZE, PROBE_SIZE))


def noise_field(sigma_m: float = 6.0) -> np.ndarray:
    """Per-cell noise. Rough, but with no shape an observer could describe."""
    return np.random.default_rng(PROBE_SEED).normal(0.0, sigma_m, (PROBE_SIZE, PROBE_SIZE))


def single_massif_field(height_m: float = 60.0) -> np.ndarray:
    """One tall block near the observers: relief without landform.

    The cheapest way to move a "how much does the skyline rise and fall" number
    is to put a wall next to the viewer, not to author interesting terrain. So
    the guard has to probe it: a metric that ranks this above authored ridges is
    a target that rewards raising one thing rather than shaping the world.
    """
    field = np.zeros((PROBE_SIZE, PROBE_SIZE))
    centre = PROBE_SIZE // 2
    field[centre - 10 : centre + 10, centre - 10 : centre + 10] = height_m
    return field


def ridged_field(ridges: int, height_m: float = 40.0) -> np.ndarray:
    """`ridges` radial ridges around the centre: authored, countable shape."""
    axis = np.arange(PROBE_SIZE) - (PROBE_SIZE - 1) / 2.0
    x, y = np.meshgrid(axis, axis)
    angle = np.arctan2(y, x)
    radius = np.hypot(x, y) / ((PROBE_SIZE - 1) / 2.0)
    return height_m * np.cos(ridges * angle) ** 2 * np.clip(radius, 0.0, 1.0)


def honesty() -> dict[str, Any]:
    """Score the silhouette metrics against controls before trusting them.

    The failure this guards against: a "shape" metric that a noise field aces is
    a roughness gate with a misleading name, and would hand the composition
    knobs exactly the kind of dishonest target item 6 found elsewhere.
    """
    probes = {
        "flat": flat_field(),
        "noise": noise_field(),
        "single_massif": single_massif_field(),
        "ridges_2": ridged_field(2),
        "ridges_4": ridged_field(4),
        "ridges_8": ridged_field(8),
    }
    scores = {
        name: measure(field, PROBE_CELL_M) for name, field in probes.items()
    }
    findings: list[str] = []
    if scores["flat"]["horizon_relief_deg"] > 0.1:
        findings.append("nonzero_relief_on_a_flat_world")
    if scores["flat"]["horizon_peaks_per_turn"] > 0.0:
        findings.append("peaks_on_a_flat_world")
    ordered = [scores["ridges_%d" % n]["horizon_peaks_per_turn"] for n in (2, 4, 8)]
    if not (ordered[0] < ordered[1] < ordered[2]):
        findings.append("peak_count_does_not_track_authored_ridge_count")
    if scores["noise"]["horizon_coherence"] >= scores["ridges_4"]["horizon_coherence"]:
        findings.append("noise_is_as_coherent_as_authored_ridges")
    # Relief is gameable by one tall thing next to the observer, which is a
    # cheaper way to move the number than authoring terrain. Detected, labelled,
    # and kept -- it is still the right number for "does the skyline read as
    # flat", it just must not be optimised alone.
    relief_gameable = (
        scores["single_massif"]["horizon_relief_deg"]
        > scores["ridges_4"]["horizon_relief_deg"]
    )

    # Stated as a property, not hidden as a pass. `horizon_peaks_per_turn` is a
    # descriptive count and a noise field beats an eight-ridge world on it, so
    # it must never be an optimisation target on its own. `horizon_coherence`
    # is what carries the shape signal, and it is the one a repair loop should
    # drive. Reporting the gameable number without this label is how
    # `detail_density` ended up incentivising un-filtered renders.
    gameable = (
        scores["noise"]["horizon_peaks_per_turn"]
        >= scores["ridges_8"]["horizon_peaks_per_turn"]
    )
    return {
        "schema_version": SILHOUETTE_VERSION,
        "scores": scores,
        "findings": findings,
        "metric_kinds": {
            "horizon_relief_deg": (
                "descriptive_count_gameable_alone" if relief_gameable else "shape_metric"
            ),
            "horizon_coherence": "shape_metric",
            "horizon_peaks_per_turn": (
                "descriptive_count_gameable_alone" if gameable else "shape_metric"
            ),
            "horizon_roughness": "descriptive_count_gameable_alone",
            "horizon_max_deg": "shape_metric",
        },
        # Only what survived every control. Coherence is the one a repair loop
        # should drive: noise cannot fake it and a single massif does not win it.
        "optimisation_targets": (
            ["horizon_coherence"]
            if relief_gameable
            else ["horizon_relief_deg", "horizon_coherence"]
        ),
        "kind": "shape_metric" if not findings else "suspect",
    }


def from_batch(batch_dir: Path) -> dict[str, Any]:
    manifest_path = batch_dir / "terrain/terrain_manifest.json"
    heightfield_path = batch_dir / "terrain/heightfield_f32le.bin"
    for required in (manifest_path, heightfield_path):
        if not required.is_file():
            raise SilhouetteError("%s is missing; compile the batch first" % required)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    resolution = int(manifest["resolution"])
    width = float(manifest["world_bounds_m"]["width"])
    heights = np.fromfile(heightfield_path, dtype="<f4")
    if heights.size != resolution * resolution:
        raise SilhouetteError("heightfield does not match the declared resolution")
    heights = heights.reshape(resolution, resolution).astype(np.float64)
    # Decimate to roughly one sample per metre: the skyline is a coarse-scale
    # property and the full raster is 16x the work for no change in the answer.
    stride = max(1, resolution // 257)
    return {
        "schema_version": SILHOUETTE_VERSION,
        "zone_id": manifest.get("zone_id"),
        "heightfield_sha256": manifest.get("heightfield_sha256"),
        "metrics": measure(heights[::stride, ::stride], width / (resolution - 1) * stride),
    }


def main(argv: list[str] | None = None) -> int:
    import argparse
    import sys

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("batch", type=Path, nargs="?")
    parser.add_argument("--honesty", action="store_true", help="score the metric against controls")
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args(argv)

    if arguments.honesty:
        document = honesty()
        for name, score in document["scores"].items():
            print(
                "%-10s relief %6.2f deg  peaks %5.1f  roughness %.3f"
                % (
                    name,
                    score["horizon_relief_deg"],
                    score["horizon_peaks_per_turn"],
                    score["horizon_roughness"],
                )
            )
        print("verdict: %s" % document["kind"])
        for finding in document["findings"]:
            print("  %s" % finding)
    elif arguments.batch:
        document = from_batch(arguments.batch.resolve())
        metrics = document["metrics"]
        print(
            "silhouette %s: relief %.2f deg, %.1f peaks per turn, roughness %.3f, "
            "highest horizon %.2f deg"
            % (
                document["zone_id"],
                metrics["horizon_relief_deg"],
                metrics["horizon_peaks_per_turn"],
                metrics["horizon_roughness"],
                metrics["horizon_max_deg"],
            )
        )
    else:
        parser.error("give a batch or --honesty")
        return 2

    if arguments.output:
        arguments.output.write_text(
            json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
