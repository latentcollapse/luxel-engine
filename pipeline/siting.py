"""Where structures belong (systems roadmap S10).

Settlements are currently scattered by a solver that knows about terrain slope
and a keep-out radius around other landmarks, and nothing else. It does not know
what a lane is. So three villages sit across the roads they exist to serve --
`southwest_hamlet` by 9.9 m, `westcentral_hamlet` by 9.3 m, `eastcentral_hamlet`
by 7.7 m -- and no lane on the map runs end to end (D13).

**That is not a tuning problem.** A village on a road is a scoring problem: the
score never contained a term for "do not block the route". Adding one is the
fix; nudging the scatter's spacing is not.

Two roles, two rules, and the reason [S14](SYSTEMS_ROADMAP.md) had to split them
apart first:

- **settlements** are dressing. They want flat, sheltered, well-drained ground
  near a route and *off* it. Being plausible is the whole job.
- **lane guardians** are gameplay. They want to be *on* the lane, evenly spaced
  along it, on ground that reads from a distance. Being plausible is secondary
  to being fair.

While those were one baked asset neither rule could be honoured, which is why
this reads the site conditions field but the decomposition had to land first.

Pure arithmetic: no batch, no I/O, no `bpy`.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

import numpy as np


# How close a settlement may come to a lane centreline before it is obstructing
# it. The lanes declare `minimum_width_m: 10`, so half of that is the roadbed
# itself; the rest is the shoulder a body needs to pass without being funnelled
# into a wall.
LANE_KEEP_OUT_M = 14.0

# Guardians sit on the lane by design, so their limit is each other.
GUARDIAN_SPACING_M = 58.0


@dataclass(frozen=True)
class Site:
    x: float
    z: float
    score: float
    role: str


def _polyline_distance(x: np.ndarray, z: np.ndarray, points) -> np.ndarray:
    """Distance from every cell to a polyline, in metres."""
    best = np.full(x.shape, np.inf)
    for a, b in zip(points, points[1:]):
        ax, az = float(a[0]), float(a[1])
        bx, bz = float(b[0]), float(b[1])
        dx, dz = bx - ax, bz - az
        length_sq = dx * dx + dz * dz
        if length_sq <= 1e-9:
            best = np.minimum(best, np.hypot(x - ax, z - az))
            continue
        t = np.clip(((x - ax) * dx + (z - az) * dz) / length_sq, 0.0, 1.0)
        best = np.minimum(best, np.hypot(x - (ax + t * dx), z - (az + t * dz)))
    return best


def settlement_score(
    *,
    slope_degrees: np.ndarray,
    wetness: np.ndarray,
    exposure: np.ndarray,
    lane_distance: np.ndarray,
) -> np.ndarray:
    """How good a village site every cell is.

    The lane term is two-sided and that is the whole point: *near* a road is
    good, *on* it is disqualifying. A one-sided "close to the road" term is what
    the current scatter effectively has, and it produces villages in the middle
    of the carriageway.
    """
    buildable = np.clip(1.0 - slope_degrees / 12.0, 0.0, 1.0)
    drained = np.clip(1.0 - np.abs(wetness - 5.0) / 4.5, 0.0, 1.0)
    sheltered = np.clip(1.0 - exposure / 0.8, 0.0, 1.0)
    # Wants a road nearby, but not underfoot.
    access = np.clip(1.0 - (lane_distance - LANE_KEEP_OUT_M) / 46.0, 0.0, 1.0)
    clear = np.clip((lane_distance - LANE_KEEP_OUT_M) / 6.0, 0.0, 1.0)
    return buildable * drained * sheltered * access * clear


def guardian_score(
    *,
    slope_degrees: np.ndarray,
    exposure: np.ndarray,
    lane_distance: np.ndarray,
) -> np.ndarray:
    """How good a lane-guardian site every cell is.

    Inverted against the settlement rule on purpose: a guardian wants to be on
    the lane it guards, on ground level enough to build on and open enough to
    see from. It does not care about drainage; nobody lives in it.
    """
    buildable = np.clip(1.0 - slope_degrees / 16.0, 0.0, 1.0)
    commanding = np.clip(0.35 + exposure, 0.0, 1.0)
    on_lane = np.clip(1.0 - lane_distance / 11.0, 0.0, 1.0)
    return buildable * commanding * on_lane


def _harvest(
    score: np.ndarray,
    x: np.ndarray,
    z: np.ndarray,
    *,
    count: int,
    spacing_m: float,
    role: str,
    floor: float = 0.05,
) -> list[Site]:
    """Greedy best-first with a keep-out, which is what spacing means.

    Taking the top N cells outright would return N cells of the same hilltop.
    """
    taken: list[Site] = []
    working = score.copy()
    for _ in range(count):
        index = int(np.argmax(working))
        row, column = divmod(index, working.shape[1])
        value = float(working[row, column])
        if value <= floor:
            break
        site = Site(
            x=float(x[row, column]), z=float(z[row, column]), score=round(value, 4),
            role=role,
        )
        taken.append(site)
        working[np.hypot(x - site.x, z - site.z) < spacing_m] = 0.0
    return taken


def audit_existing(features, lanes) -> list[dict]:
    """Which placed structures are standing in a lane, and by how much.

    D13 is reported today as "no lane runs end to end", which names the symptom.
    This names the structure, the lane, and the metres of overlap -- the three
    things somebody needs to fix it.
    """
    findings: list[dict] = []
    for feature in features:
        if feature.get("category") != "landmark":
            continue
        points = (feature.get("geometry") or {}).get("points") or []
        if not points:
            continue
        radius = float(
            (feature.get("properties") or {}).get("scatter_exclusion_radius_m", 0.0)
        )
        px, pz = float(points[0][0]), float(points[0][1])
        for lane in lanes:
            lane_points = (lane.get("geometry") or {}).get("points") or []
            if len(lane_points) < 2:
                continue
            half_width = (
                float((lane.get("properties") or {}).get("minimum_width_m", 10.0)) * 0.5
            )
            distance = float(
                _polyline_distance(np.array([[px]]), np.array([[pz]]), lane_points)[0, 0]
            )
            overlap = (radius + half_width) - distance
            if overlap > 0.0:
                findings.append(
                    {
                        "feature_id": feature.get("id"),
                        "lane_id": lane.get("id"),
                        "centreline_distance_m": round(distance, 2),
                        "overlap_m": round(overlap, 2),
                        "repair": (
                            "move %s at least %.1f m off %s, or shrink its "
                            "exclusion radius below %.1f m"
                            % (
                                feature.get("id"),
                                overlap,
                                lane.get("id"),
                                max(distance - half_width, 0.0),
                            )
                        ),
                    }
                )
    return sorted(findings, key=lambda f: -f["overlap_m"])


def build(batch_dir) -> dict:
    """Score the world for structure siting and audit what is already placed."""
    import json
    from pathlib import Path

    batch_dir = Path(batch_dir)
    zone_spec = json.loads((batch_dir / "zone_spec.json").read_text(encoding="utf-8"))
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

    bounds = manifest["world_bounds_m"]
    width, length = float(bounds["width"]), float(bounds["length"])
    x_axis = np.linspace(-width * 0.5, width * 0.5, resolution)
    z_axis = np.linspace(length * 0.5, -length * 0.5, resolution)
    x, z = np.meshgrid(x_axis, z_axis)

    features = zone_spec.get("features", [])
    lanes = [f for f in features if f.get("category") == "corridor"]
    lane_distance = np.full(x.shape, np.inf)
    for lane in lanes:
        points = (lane.get("geometry") or {}).get("points") or []
        if len(points) >= 2:
            lane_distance = np.minimum(lane_distance, _polyline_distance(x, z, points))
    if not np.isfinite(lane_distance).all():
        lane_distance = np.full(x.shape, max(width, length))

    slope = plane("slope_degrees")
    wetness = plane("wetness_index")
    exposure = plane("exposure")

    settlements = settlement_score(
        slope_degrees=slope, wetness=wetness, exposure=exposure,
        lane_distance=lane_distance,
    )
    guardians = guardian_score(
        slope_degrees=slope, exposure=exposure, lane_distance=lane_distance
    )

    existing = [f for f in features if f.get("semantic") == "settlement_cluster"]
    proposals = _harvest(
        settlements, x, z, count=max(len(existing), 6), spacing_m=34.0,
        role="settlement_cluster",
    )
    # Two guardians per lane is the standard MOBA shape; the spacing keeps them
    # off each other rather than the count enforcing it.
    guardian_sites = _harvest(
        guardians, x, z, count=max(2 * len(lanes), 2), spacing_m=GUARDIAN_SPACING_M,
        role="lane_guardian",
    )

    conflicts = audit_existing(features, lanes)
    return {
        "schema_version": "codeweald.siting-plan/v1",
        "zone_id": manifest.get("zone_id"),
        "heightfield_sha256": manifest.get("heightfield_sha256"),
        "lane_keep_out_m": LANE_KEEP_OUT_M,
        "guardian_spacing_m": GUARDIAN_SPACING_M,
        "proposed_settlements": [
            {"x": round(s.x, 2), "z": round(s.z, 2), "score": s.score} for s in proposals
        ],
        "proposed_guardians": [
            {"x": round(s.x, 2), "z": round(s.z, 2), "score": s.score}
            for s in guardian_sites
        ],
        "obstructing_placements": conflicts,
        "obstruction_count": len(conflicts),
        "best_settlement_score": round(float(settlements.max()), 4),
        "best_guardian_score": round(float(guardians.max()), 4),
    }
