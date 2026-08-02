"""Declared, re-measurable affordances of a placed asset.

`asset_physical_acceptance` asks whether an asset is a *plausible size* -- is a
fortification under 30 m tall and 42 m across. Nothing asked whether the thing
the role exists for can actually happen. A keep's whole job is that units spawn
inside it and leave; both faction keeps were sealed inside their own curtain
walls for weeks (D17), passing every gate the pipeline had, because "there is a
way in" was not expressible let alone checkable.

**The generator is the only stage that knows design intent, so it states the
claim; this module is what stops the claim from being a decoration.** A kit
writes an affordance sidecar beside its GLB:

    {"enterable": {"threshold_clear_m": 8.23, "interior_ring_m": 7.24,
                   "local_bearing_degrees": 180.0,
                   "threshold_band_m": [7.97, 19.07]},
     "measured_against": {"agent_radius_m": 2.5, "agent_max_climb_m": 4.0,
                          "placed_scale": 0.1306}}

and this module re-derives the threshold from the shipped mesh and checks it
against the world the asset is about to enter. That catches three different
failures with one mechanism:

- a **stale GLB** whose sidecar still advertises an opening the mesh no longer
  has (exactly the un-regenerated variant this kit shipped in its candidate
  pool)
- a **sealed regeneration**, where someone changes the kit and the claim is
  simply no longer true
- a **wrong world**: a keep measured for a 2.5 m agent placed in a zone whose
  agent is 5 m, where the geometry never changed and the answer still did

Nothing here is keep-specific. The bearing and band are part of the claim
because the asset's author is the only one who knows where its threshold is;
given those, measuring the clear corridor is generic over any enterable asset.
"""

from __future__ import annotations

import json
import math
from pathlib import Path
from typing import Any

from asset_parts import AssetPartsError
from asset_parts import parts as asset_parts

SCHEMA_VERSION = "codeweald.asset-affordance/v1"

# Which affordances a placement role is required to declare. A role absent from
# this table declares nothing and is not checked -- a tree has no threshold and
# demanding one would be noise. A role present here fails acceptance if its
# asset ships without the claim, which is what makes "sealed keep"
# unrepresentable rather than merely detectable.
ROLE_REQUIRED_AFFORDANCES: dict[str, tuple[str, ...]] = {
    "faction_fortification": ("enterable",),
}

# Re-measurement is allowed to come in slightly under the declared figure: the
# generator measures declared part boxes, this measures the exported mesh, and
# a bevel modifier rounds a corner by a few millimetres. It is not allowed to
# come in *materially* under, which is the stale-asset case.
DECLARED_TOLERANCE_M = 0.25


class AffordanceError(ValueError):
    """The affordance claim cannot be read or checked."""


def sidecar_path(asset_path: Path) -> Path:
    return asset_path.with_suffix(".affordances.json")


def load_contract(asset_path: Path) -> dict[str, Any] | None:
    path = sidecar_path(asset_path)
    if not path.is_file():
        return None
    try:
        contract = json.loads(path.read_text(encoding="utf-8"))
    except ValueError as exc:
        raise AffordanceError("%s is not readable JSON: %s" % (path, exc))
    if not isinstance(contract, dict):
        raise AffordanceError("%s must be an object" % path)
    return contract


def _projection(centre: float, half_a: float, half_b: float, unit_a: float, unit_b: float) -> tuple[float, float]:
    reach = abs(half_a * unit_a) + abs(half_b * unit_b)
    return centre - reach, centre + reach


def measure_threshold_m(
    parts: list[dict[str, Any]],
    *,
    bearing_degrees: float,
    band_m: tuple[float, float],
    scale: float,
    max_climb_m: float,
) -> float:
    """Widest clear corridor across the asset's threshold, in metres.

    The corridor runs along the declared outward bearing and is measured only
    across the declared depth band -- the wall crossing and its approach, not
    the whole building, or the far wall of any enclosed structure would read as
    the door being shut.

    Applies `navigation_plan.rasterize_colliders`' own test: a body steps onto
    anything below its climb height, so floors, plinths and drawbridge decking
    do not narrow a doorway. Height above the threshold is deliberately *not* a
    reprieve -- that rasteriser subtracts a part's whole ground footprint
    regardless of its underside, so an arch blocks exactly as a wall does, and a
    measurement that disagreed would certify gates the game cannot use.
    """
    angle = math.radians(bearing_degrees)
    # Outward direction and the lateral axis across it.
    dx, dz = math.sin(angle), math.cos(angle)
    lx, lz = math.cos(angle), -math.sin(angle)
    near, far = min(band_m), max(band_m)

    left, right = -math.inf, math.inf
    for part in parts:
        minimum, maximum = part["min"], part["max"]
        if float(maximum[1]) * scale <= max_climb_m:
            continue
        centre_x = (float(minimum[0]) + float(maximum[0])) * 0.5 * scale
        centre_z = (float(minimum[2]) + float(maximum[2])) * 0.5 * scale
        half_x = (float(maximum[0]) - float(minimum[0])) * 0.5 * scale
        half_z = (float(maximum[2]) - float(minimum[2])) * 0.5 * scale

        depth_low, depth_high = _projection(centre_x * dx + centre_z * dz, half_x, half_z, dx, dz)
        if depth_high < near or depth_low > far:
            continue
        lateral_low, lateral_high = _projection(centre_x * lx + centre_z * lz, half_x, half_z, lx, lz)
        if lateral_low <= 0.0 <= lateral_high:
            return 0.0  # something spans the centreline: the threshold is shut
        if lateral_low > 0.0:
            right = min(right, lateral_low)
        else:
            left = max(left, lateral_high)
    if math.isinf(left) or math.isinf(right):
        # Nothing bounds the corridor at all, which means the band names empty
        # space rather than a threshold. Reporting "infinitely wide" would turn
        # a mis-declared band into a pass.
        raise AffordanceError(
            "no geometry bounds the declared threshold band %.2f..%.2f m; the "
            "band does not describe this asset" % (near, far)
        )
    return right - left


def verify(
    asset_path: Path,
    contract: dict[str, Any] | None,
    *,
    role: str,
    agent_radius_m: float,
    max_climb_m: float,
    placed_scale: float,
) -> list[str]:
    """Every reason this asset may not be placed in this world."""
    required = ROLE_REQUIRED_AFFORDANCES.get(role)
    if not required:
        return []
    if contract is None:
        return [
            "%s fills role %s, which must declare %s, but ships no affordance "
            "sidecar. Regenerate its kit: an asset whose usability is unstated "
            "cannot be checked, and D17 is what that costs."
            % (asset_path.name, role, "/".join(required))
        ]

    failures: list[str] = []
    needed = 2.0 * agent_radius_m
    for name in required:
        claim = contract.get(name)
        if not isinstance(claim, dict):
            failures.append("%s does not declare the %r affordance" % (asset_path.name, name))
            continue
        if name != "enterable":
            continue

        declared = float(claim.get("threshold_clear_m", 0.0))
        ring = float(claim.get("interior_ring_m", 0.0))
        bearing = float(claim.get("local_bearing_degrees", 0.0))
        band = claim.get("threshold_band_m")
        if not isinstance(band, list) or len(band) != 2:
            failures.append("%s declares no threshold_band_m to re-measure against" % asset_path.name)
            continue

        try:
            measured = measure_threshold_m(
                asset_parts(asset_path),
                bearing_degrees=bearing,
                band_m=(float(band[0]), float(band[1])),
                scale=placed_scale,
                max_climb_m=max_climb_m,
            )
        except (AffordanceError, AssetPartsError, OSError) as exc:
            failures.append("%s: %s" % (asset_path.name, exc))
            continue

        # The mesh is the authority; the sidecar is a claim about it.
        if measured < declared - DECLARED_TOLERANCE_M:
            failures.append(
                "%s claims a %.2f m threshold but its mesh measures %.2f m. The "
                "asset and its affordance sidecar disagree -- regenerate the kit "
                "rather than editing the claim."
                % (asset_path.name, declared, measured)
            )
        if measured <= needed:
            failures.append(
                "%s has a %.2f m threshold; this zone's agent needs %.2f m "
                "(radius %.1f m) to pass. The keep would be sealed to it (D17)."
                % (asset_path.name, measured, needed, agent_radius_m)
            )
        if ring < needed:
            # Not re-measurable from the mesh without knowing what the asset's
            # interior is, so this stays a declared figure. The generator's own
            # unit tests are what stand behind it.
            failures.append(
                "%s declares a %.2f m interior ring; this zone's agent needs "
                "%.2f m. It could be entered but not stood in (D17)."
                % (asset_path.name, ring, needed)
            )
        measured_scale = float((contract.get("measured_against") or {}).get("placed_scale", 0.0))
        if measured_scale and abs(measured_scale - placed_scale) > 1e-6:
            failures.append(
                "%s was measured at scale %.4f but this zone places it at %.4f; "
                "its affordances were verified for a different world."
                % (asset_path.name, measured_scale, placed_scale)
            )
    return failures
