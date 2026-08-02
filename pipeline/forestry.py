"""Where vegetation grows, and why (systems roadmap S6).

Foliage is currently scattered into an authored polygon at a declared spacing.
That is placement, not ecology, and it cannot produce the things that make a
wooded landscape read as one: a treeline, a riparian fringe along the water, a
bog that looks like a bog, denser growth on the shaded flank than the baked one,
clearings where the soil is too thin to hold anything.

**Species by niche, density by suitability.** Each family declares the
conditions it tolerates -- an elevation band, a slope limit, a wetness range, a
preference for sun or shade, a tolerance for exposure -- and the solver scores
every cell of the map against every family. Nobody paints a treeline; a treeline
is what happens when the elevation term of the score crosses zero.

Every input comes from the site conditions field (S1), which is the point of
computing it once: the trees, the soil and the settlements all read the same
numbers, so they agree about where the wet hollow is instead of each deciding
separately.

Pure arithmetic over arrays -- no batch, no I/O, no `bpy`.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class Niche:
    """The conditions one plant family tolerates.

    Every bound is a *soft* one: suitability falls off across a band rather than
    switching at a threshold, because a treeline is a hundred metres of thinning
    and stunting, not a line where forest stops. Hard thresholds are what make
    procedural vegetation look stamped.
    """

    key: str
    # Fractions of the world's relief, not metres, so a niche transfers between
    # a 60 m map and a 600 m one without being re-authored.
    elevation_band: tuple[float, float]
    elevation_falloff: float
    maximum_slope_degrees: float
    wetness_band: tuple[float, float]
    # 0 wants deep shade, 1 wants full sun, 0.5 is indifferent.
    insolation_preference: float
    insolation_strength: float
    exposure_tolerance: float
    canopy: bool = True
    notes: str = ""


HIGHLAND_NICHES: tuple[Niche, ...] = (
    Niche(
        key="montane_conifer",
        elevation_band=(0.08, 0.62),
        elevation_falloff=0.14,
        maximum_slope_degrees=34.0,
        wetness_band=(3.0, 8.0),
        insolation_preference=0.42,
        insolation_strength=0.45,
        exposure_tolerance=0.55,
        notes="the main forest; thins into the treeline rather than stopping at it",
    ),
    Niche(
        key="subalpine_krummholz",
        elevation_band=(0.52, 0.86),
        elevation_falloff=0.10,
        maximum_slope_degrees=42.0,
        wetness_band=(2.0, 7.5),
        insolation_preference=0.55,
        insolation_strength=0.30,
        exposure_tolerance=0.95,
        notes="stunted wind-flagged growth above the forest, below the rock",
    ),
    Niche(
        key="riparian_broadleaf",
        elevation_band=(0.0, 0.40),
        elevation_falloff=0.12,
        maximum_slope_degrees=22.0,
        wetness_band=(7.0, 14.0),
        insolation_preference=0.55,
        insolation_strength=0.25,
        exposure_tolerance=0.30,
        notes="follows the water; the fringe that makes a stream read as a stream",
    ),
    Niche(
        key="bog_sedge",
        elevation_band=(0.0, 0.35),
        elevation_falloff=0.10,
        maximum_slope_degrees=8.0,
        wetness_band=(8.5, 16.0),
        insolation_preference=0.6,
        insolation_strength=0.20,
        exposure_tolerance=0.45,
        canopy=False,
        notes="standing-water margins; what makes a bog look like a bog",
    ),
    Niche(
        key="heath_scrub",
        elevation_band=(0.05, 0.70),
        elevation_falloff=0.18,
        maximum_slope_degrees=38.0,
        wetness_band=(1.5, 7.0),
        insolation_preference=0.72,
        insolation_strength=0.35,
        exposure_tolerance=0.85,
        canopy=False,
        notes="fills the exposed and the thin-soiled, where the forest gives up",
    ),
)


def _band(values: np.ndarray, low: float, high: float, falloff: float) -> np.ndarray:
    """1 inside the band, falling smoothly to 0 over `falloff` either side."""
    falloff = max(falloff, 1e-6)
    rising = np.clip((values - (low - falloff)) / falloff, 0.0, 1.0)
    falling = np.clip(((high + falloff) - values) / falloff, 0.0, 1.0)
    return np.minimum(rising, falling)


def suitability(
    niche: Niche,
    *,
    relative_elevation: np.ndarray,
    slope_degrees: np.ndarray,
    wetness: np.ndarray,
    insolation: np.ndarray,
    exposure: np.ndarray,
) -> np.ndarray:
    """How well one family does at every cell, in [0, 1].

    The terms multiply rather than average. A plant that cannot stand the wet
    does not partly grow in a bog because the elevation happened to suit it --
    any single condition being intolerable is fatal, which is what a product
    expresses and a mean does not.
    """
    score = _band(
        relative_elevation, niche.elevation_band[0], niche.elevation_band[1],
        niche.elevation_falloff,
    )
    # Slope is one-sided: nothing minds flat ground, everything minds a cliff.
    score = score * np.clip(
        1.0 - (slope_degrees - niche.maximum_slope_degrees) / 9.0, 0.0, 1.0
    )
    score = score * _band(wetness, niche.wetness_band[0], niche.wetness_band[1], 1.4)
    # Preference, not requirement: strength decides how much being on the wrong
    # aspect actually costs.
    aspect_fit = 1.0 - np.abs(insolation - niche.insolation_preference)
    score = score * (1.0 - niche.insolation_strength + niche.insolation_strength * aspect_fit)
    score = score * np.clip(
        1.0 - (exposure - niche.exposure_tolerance) / 0.35, 0.0, 1.0
    )
    return np.clip(score, 0.0, 1.0)


def resolve(
    niches: tuple[Niche, ...],
    *,
    relative_elevation: np.ndarray,
    slope_degrees: np.ndarray,
    wetness: np.ndarray,
    insolation: np.ndarray,
    exposure: np.ndarray,
) -> tuple[np.ndarray, dict[str, np.ndarray]]:
    """Score every family, then let them compete.

    Returns the winning family index per cell (-1 where nothing grows) and the
    raw suitability of each. Competition is winner-takes-cell: real stands are
    dominated by one species rather than evenly blended, and blending is what
    makes procedural forests look like soup.
    """
    scores = {
        niche.key: suitability(
            niche,
            relative_elevation=relative_elevation,
            slope_degrees=slope_degrees,
            wetness=wetness,
            insolation=insolation,
            exposure=exposure,
        )
        for niche in niches
    }
    stack = np.stack([scores[niche.key] for niche in niches])
    best = np.argmax(stack, axis=0)
    strongest = np.max(stack, axis=0)
    # Below this nothing has a foothold: bare ground, and bare ground is a
    # feature. A map with no clearings reads as a carpet.
    best = np.where(strongest >= 0.12, best, -1)
    return best, scores


def treeline_m(
    niches: tuple[Niche, ...],
    height: np.ndarray,
    canopy_mask: np.ndarray,
) -> float | None:
    """The elevation the canopy actually gives up at, measured not declared.

    Emitted so a reviewer can check the treeline against the snowline rather
    than taking the vegetation's word for it -- if the trees are growing above
    the snow, one of the two is wrong.
    """
    if not canopy_mask.any():
        return None
    return float(np.percentile(height[canopy_mask], 97.0))


def build(batch_dir) -> dict:
    """Derive the vegetation plan for a compiled world from its site conditions."""
    import json
    from pathlib import Path

    batch_dir = Path(batch_dir)
    site = json.loads((batch_dir / "site_conditions.json").read_text(encoding="utf-8"))
    manifest = json.loads(
        (batch_dir / "terrain/terrain_manifest.json").read_text(encoding="utf-8")
    )
    resolution = int(site["resolution"])
    fields = list(site["fields"])
    stack = np.fromfile(
        batch_dir / "terrain/site_conditions_f32le.bin", dtype="<f4"
    ).reshape(len(fields), resolution, resolution).astype(np.float64)

    def plane(name: str) -> np.ndarray:
        return stack[fields.index(name)]

    height = (
        np.fromfile(batch_dir / "terrain/heightfield_f32le.bin", dtype="<f4")
        .reshape(resolution, resolution)
        .astype(np.float64)
    )
    floor, ceiling = float(height.min()), float(height.max())
    relief = max(ceiling - floor, 1e-6)
    # Niches are declared in fractions of relief so they transfer between maps.
    relative = (height - floor) / relief

    best, scores = resolve(
        HIGHLAND_NICHES,
        relative_elevation=relative,
        slope_degrees=plane("slope_degrees"),
        wetness=plane("wetness_index"),
        insolation=plane("insolation"),
        exposure=plane("exposure"),
    )

    width = float(manifest["world_bounds_m"]["width"])
    cell_m = width / (resolution - 1)
    cell_area = cell_m * cell_m
    canopy_keys = {n.key for n in HIGHLAND_NICHES if n.canopy}
    canopy_mask = np.zeros(best.shape, dtype=bool)
    families = []
    for index, niche in enumerate(HIGHLAND_NICHES):
        holds = best == index
        if niche.canopy:
            canopy_mask |= holds
        families.append(
            {
                "key": niche.key,
                "canopy": niche.canopy,
                "area_m2": round(float(holds.sum()) * cell_area, 1),
                "world_fraction": round(float(holds.mean()), 5),
                "mean_suitability": round(float(scores[niche.key][holds].mean()), 4)
                if holds.any()
                else 0.0,
                "elevation_band_m": [
                    round(floor + niche.elevation_band[0] * relief, 2),
                    round(floor + niche.elevation_band[1] * relief, 2),
                ],
                "notes": niche.notes,
            }
        )

    bare = best == -1
    line = treeline_m(HIGHLAND_NICHES, height, canopy_mask)
    return {
        "schema_version": "codeweald.vegetation-plan/v1",
        "zone_id": manifest.get("zone_id"),
        "heightfield_sha256": manifest.get("heightfield_sha256"),
        "site_conditions_sha256": site.get("field_sha256"),
        "resolution": resolution,
        "cell_m": round(cell_m, 6),
        "families": families,
        "canopy_area_m2": round(float(canopy_mask.sum()) * cell_area, 1),
        "bare_area_m2": round(float(bare.sum()) * cell_area, 1),
        "bare_fraction": round(float(bare.mean()), 5),
        # Measured, so a reviewer can check it against the snowline instead of
        # taking the vegetation's word for it.
        "measured_treeline_m": round(line, 2) if line is not None else None,
        "relief_m": round(relief, 2),
    }
