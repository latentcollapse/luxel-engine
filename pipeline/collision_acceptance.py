"""Roadmap 1.5: the collision acceptance gate.

`collision_plan` (1.1) emits colliders; it does not check them against the
terrain they are supposed to sit on. This module is the verdict: no walkable
surface without a collider, no collider floating off terrain.

**Seating is checked against `position_m[1]`, not the collider's own
bounding-box math, and that distinction is load-bearing.** The first version
of this gate derived "bottom of collider" from `centre_m[1] -
half_extents_m[1]`, which assumes an asset's local mesh bounds are
vertically symmetric around its placement pivot. They are not, in general --
measuring the real batch found the keep's collider apparently "buried" 17 m,
which is not what the render shows. `render_plan.rs::apply_grounding`
explains why: it nudges `position_m[1]` down from wherever the placement
solver put it by a small, role-clamped `grounding_offset_m` (0.05-0.35 m for
every collidable role here, up to 2.5 m only for `landform_dressing`, which
is not collidable) and records that nudge explicitly. `position_m[1]` is
therefore the actual grounding signal the pipeline already computes and
already keeps small by construction -- verified against terrain height across
a random sample of real placements, every diff was within 0.26 m. Seating is
checked against *that*, not a bounding-box derivation this module cannot
independently verify is vertically symmetric for every asset.

Both roadmap clauses turn out to be two signs of the same underlying
question -- is a collidable instance's grounded position actually near the
terrain beneath it -- so this evaluates one seating check per collidable
instance, plus a completeness check that every collidable-role placement
actually produced a collider.

**Decks are exempt from the seating check, deliberately.** A `deck` (a bridge
crossing) is meant to span above lower ground by design -- that is the entire
point of a bridge. Checking "is the deck floating above terrain" would either
always fail on every legitimate bridge, or have to be tuned so loose it catches
nothing. `collision_plan.py`'s own docstring already names the real limit
here: the deck's *upper surface* is not emitted, so an elevated bridge over a
gorge could leave a gap navigation does not know about. That is real,
unfixed, and out of scope for this gate -- it is a modelling gap, not a
placement defect this check can distinguish from a correct bridge.
"""

from __future__ import annotations

import json
import math
from pathlib import Path
from typing import Any

import numpy as np

from collision_plan import ROLE_COLLISION, CollisionPlanError, _digest

SCHEMA_VERSION = "codeweald.collision-acceptance/v1"

# How far a collidable instance's grounded position may sit off the terrain
# beneath it before it counts as floating (daylight underneath) or buried
# (sunk into the ground) rather than correctly seated. render_plan.rs clamps
# grounding_offset_m to 0.35 m at its loosest for any collidable role, so 1.0 m
# is generous headroom for that plus interpolation error, while still catching
# an asset placed at the wrong height entirely.
SEATING_TOLERANCE_M = 1.0


def _sample_height(height_m: np.ndarray, width: float, length: float, x: float, z: float) -> float:
    rows, columns = height_m.shape
    u = np.clip(x / width + 0.5, 0.0, 1.0) * (columns - 1)
    v = np.clip(0.5 - z / length, 0.0, 1.0) * (rows - 1)
    x0, z0 = int(math.floor(u)), int(math.floor(v))
    x1, z1 = min(x0 + 1, columns - 1), min(z0 + 1, rows - 1)
    tx, tz = u - x0, v - z0
    return float(
        height_m[z0, x0] * (1.0 - tx) * (1.0 - tz)
        + height_m[z0, x1] * tx * (1.0 - tz)
        + height_m[z1, x0] * (1.0 - tx) * tz
        + height_m[z1, x1] * tx * tz
    )


def evaluate(
    collision_plan: dict[str, Any],
    render_plan: dict[str, Any],
    height_m: np.ndarray,
    width: float,
    length: float,
    *,
    tolerance_m: float = SEATING_TOLERANCE_M,
) -> dict[str, Any]:
    """Gate a collision plan against roadmap 1.5's two requirements."""
    failures: list[str] = []

    # No walkable surface without a collider: every placement whose role is
    # declared collidable must have produced a collider. `instance_collider`
    # already enforces this by construction today (it either returns a
    # collider or raises for an undeclared role), so this is regression
    # insurance against a future refactor reintroducing a silent drop, not a
    # check expected to find anything on a healthy plan.
    collider_ids = {
        # A composite asset emits one collider per solid part, so a collider's
        # own `id` is `<instance>#<Part>`. The placement it belongs to is
        # `instance_id`; falling back to `id` keeps older plans readable.
        collider.get("instance_id", collider["id"])
        for collider in collision_plan.get("instance_colliders", [])
    }
    position_by_id = {
        instance["id"]: instance.get("position_m")
        for instance in render_plan.get("instances", [])
        if isinstance(instance.get("id"), str)
    }
    for instance in render_plan.get("instances", []):
        role = instance.get("role")
        if ROLE_COLLISION.get(role) in (None, "none"):
            continue
        if instance.get("id") not in collider_ids:
            failures.append(
                "Instance %s has collidable role %r but no collider in "
                "collision_plan.json" % (instance.get("id"), role)
            )

    # No collider floating off terrain (or sunk into it) -- decks excluded,
    # see module docstring. Checked against the instance's grounded
    # position_m[1], not the collider's own bounding-box math -- see the
    # module docstring for why.
    for collider in collision_plan.get("instance_colliders", []):
        if collider.get("shape") == "deck":
            continue
        position = position_by_id.get(
            collider.get("instance_id", collider.get("id"))
        )
        if position is None:
            continue  # already reported by the completeness check above
        x, y, z = (float(v) for v in position)
        ground_m = _sample_height(height_m, width, length, x, z)
        gap_m = y - ground_m
        if gap_m > tolerance_m:
            failures.append(
                "Collider %s (role %s) floats %.2f m above terrain"
                % (collider["id"], collider.get("role"), gap_m)
            )
        elif gap_m < -tolerance_m:
            failures.append(
                "Collider %s (role %s) is buried %.2f m below terrain"
                % (collider["id"], collider.get("role"), -gap_m)
            )

    return {
        "schema_version": SCHEMA_VERSION,
        "status": "failed" if failures else "passed",
        "failures": failures,
        "policy": {"seating_tolerance_m": tolerance_m},
    }


def build(batch_dir: Path, *, tolerance_m: float = SEATING_TOLERANCE_M) -> dict[str, Any]:
    """Load a compiled batch's artifacts and evaluate the gate."""
    terrain_dir = batch_dir / "terrain"
    collision_path = batch_dir / "collision_plan.json"
    render_plan_path = batch_dir / "render_plan.json"
    manifest_path = terrain_dir / "terrain_manifest.json"
    heightfield_path = terrain_dir / "heightfield_f32le.bin"
    for required in (collision_path, render_plan_path, manifest_path, heightfield_path):
        if not required.is_file():
            raise CollisionPlanError("%s is missing; compile the batch first" % required)

    collision_plan = json.loads(collision_path.read_text(encoding="utf-8"))
    render_plan = json.loads(render_plan_path.read_text(encoding="utf-8"))
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))

    heightfield_digest = _digest(heightfield_path)
    if collision_plan.get("heightfield_sha256") != heightfield_digest:
        raise CollisionPlanError(
            "collision plan was built against a different heightfield; a "
            "seating check against the wrong terrain is not a measurement"
        )

    resolution = int(manifest["resolution"])
    bounds = manifest["world_bounds_m"]
    width, length = float(bounds["width"]), float(bounds["length"])
    heights = np.fromfile(heightfield_path, dtype="<f4")
    if heights.size != resolution * resolution:
        raise CollisionPlanError(
            "heightfield has %d samples, manifest declares %d"
            % (heights.size, resolution * resolution)
        )
    height_m = heights.reshape(resolution, resolution).astype(np.float64)

    return evaluate(
        collision_plan, render_plan, height_m, width, length, tolerance_m=tolerance_m
    )


def main(argv: list[str] | None = None) -> int:
    import argparse
    import sys

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("batch", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument(
        "--require-passed",
        action="store_true",
        help="fail if any collider is unseated or missing",
    )
    arguments = parser.parse_args(argv)

    batch_dir = arguments.batch.resolve()
    try:
        result = build(batch_dir)
    except (CollisionPlanError, OSError, ValueError) as exc:
        print("%s: %s" % (arguments.batch, exc), file=sys.stderr)
        return 2

    destination = arguments.output or (batch_dir / "collision_acceptance.json")
    destination.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    if result["status"] == "passed":
        print("collision acceptance: passed")
        return 0
    print("collision acceptance: FAILED (%d requirement(s))" % len(result["failures"]))
    for failure in result["failures"]:
        print("  %s" % failure)
    return 1 if arguments.require_passed else 0


if __name__ == "__main__":
    import sys

    sys.exit(main())
