"""Engine-neutral collision plan: what a body can stand on and what stops it.

MVP roadmap 1.1 (see Luxel/docs/archive/2026-09_roadmaps-and-audits/mvp-roadmap.md). Luxel compiles a world that
renders; it does not yet compile one anything can move through. `collision`
appears nowhere in any emitted artifact -- only in the intake brief, as stated
intent. This closes that gap for terrain and placed instances.

Two deliberate choices:

**Terrain is emitted as a heightfield descriptor, not a triangle mesh.** Every
target engine has a native heightfield collider (Unity `TerrainCollider`,
Unreal Landscape, Godot `HeightMapShape3D`, Rapier/Avian `HeightField`), each
faster and more numerically stable than a baked triangle soup, and each reading
the same `heightfield_f32le.bin` the renderer already consumes. Baking a mesh
here would be undifferentiated work that every backend then has to undo.

**Instance colliders are oriented boxes, not world-space AABBs.** A placed
asset carries a yaw; an axis-aligned box recomputed in world space inflates
with rotation -- a 40 m keep at 45 degrees would claim a 57 m footprint and
block ground beside itself. Centre + half-extents + yaw is exact, and every
engine supports it.

The plan is hash-bound to every artifact it derives from, matching
`render_plan.json`. A collision plan that outlived its terrain would be worse
than none: it would put invisible walls where the world no longer has any.
"""

from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path
from typing import Any

from asset_parts import AssetPartsError
from asset_parts import parts as asset_parts

from boundary_plan import _canonical_digest

SCHEMA_VERSION = "codeweald.collision-plan/v1"

# What each placement role contributes to collision.
#
# `none` is a real answer, not an omission: 160 groundcover and understory
# instances stand in the lanes, and giving grass a collider would make the map
# unwalkable while looking correct in every render. Roles are declared rather
# than inferred so that adding a role forces a decision here -- an unknown role
# is refused, not silently dropped.
ROLE_COLLISION: dict[str, str] = {
    "faction_fortification": "box",
    "settlement_landmark": "box",
    # A lane guardian is a structure a player fights past, not scenery. Its own
    # role because it is sited by gameplay spacing rather than by plausibility
    # -- the two were fused into one village asset until 2026-08-02 (S14).
    "lane_guardian": "box",
    "objective_landmark": "box",
    # A bridge sits *on* the lane centreline by design, so a solid box makes it
    # a wall across the route it exists to carry -- measured: all 20 of them
    # obstructed their own lane, and no lane ran end to end. A deck carries
    # traffic instead of stopping it.
    "lane_crossing_structure": "deck",
    "forest_floor_rock": "box",
    # A tree's bounding box is its canopy, and a canopy-sized collider blocks
    # ground a player can plainly walk under. The trunk is what stops them.
    "conifer_canopy": "trunk_cylinder",
    "highland_groundcover": "none",
    "highland_understory": "none",
}

# Fraction of a canopy's horizontal extent treated as trunk. Deliberately
# generous -- a slightly fat trunk is a fair fight, an invisible canopy wall is
# not.
TRUNK_RADIUS_FRACTION = 0.12


class CollisionPlanError(ValueError):
    """The collision plan cannot be built from these artifacts."""


def _digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _rotate_y(x: float, z: float, degrees: float) -> tuple[float, float]:
    """Rotate about +Y, matching the renderer's `Quat::from_rotation_y`.

    Right-handed, Y up: +X rotates toward -Z. The collider must sit exactly
    where the rendered mesh sits, so this convention has to match the viewer's
    rather than be independently plausible.
    """
    angle = math.radians(degrees)
    cos, sin = math.cos(angle), math.sin(angle)
    return x * cos + z * sin, -x * sin + z * cos


def _asset_paths_by_digest(asset_plan: dict[str, Any]) -> dict[str, str]:
    paths: dict[str, str] = {}

    def collect(container: Any) -> None:
        if isinstance(container, dict):
            digest, source = container.get("sha256"), container.get("source_path")
            if isinstance(digest, str) and isinstance(source, str):
                paths[digest] = source
            for value in container.values():
                collect(value)
        elif isinstance(container, list):
            for value in container:
                collect(value)

    collect(asset_plan)
    return paths


def _bounds_by_path(preflight: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {
        asset["source_path"]: asset["bounds_m"]
        for asset in preflight.get("assets", [])
        if isinstance(asset, dict)
        and isinstance(asset.get("source_path"), str)
        and isinstance(asset.get("bounds_m"), dict)
    }


def instance_colliders(
    instance: dict[str, Any],
    bounds: dict[str, Any],
    asset_path: Path | None = None,
) -> list[dict[str, Any]]:
    """Every collider one placed asset contributes.

    A settlement cluster is five houses, a well, a watchtower and a 70x70
    terrace -- not one building. Collided as a single box it was a solid slab
    across a village a player can see the streets of, which is what blocked all
    three lanes (D13). When the asset separates into parts, each solid part gets
    its own collider and the gaps between them stay walkable.

    Falls back to the whole-asset box when the asset is genuinely one object, or
    when its glTF cannot be read -- a coarse collider is a worse world, an
    absent one is a broken one.
    """
    role = instance.get("role")
    shape = ROLE_COLLISION.get(role)
    if shape is None:
        raise CollisionPlanError(
            "placement role %r has no declared collision policy; add it to "
            "ROLE_COLLISION rather than letting it default" % role
        )
    if shape == "none":
        return []
    if shape != "box" or asset_path is None or not asset_path.is_file():
        collider = instance_collider(instance, bounds)
        return [collider] if collider else []

    try:
        separable = asset_parts(asset_path)
    except (AssetPartsError, OSError):
        separable = []
    if len(separable) < 2:
        collider = instance_collider(instance, bounds)
        return [collider] if collider else []

    colliders: list[dict[str, Any]] = []
    for index, part in enumerate(separable):
        collider = instance_collider(
            instance, part, suffix="%s:%02d" % (part["name"], index)
        )
        if collider is not None:
            colliders.append(collider)
    return colliders


def instance_collider(
    instance: dict[str, Any], bounds: dict[str, Any], suffix: str | None = None
) -> dict[str, Any] | None:
    """One placed asset's collider, in world space.

    Returns None for roles that deliberately do not collide.
    """
    role = instance.get("role")
    shape = ROLE_COLLISION.get(role)
    if shape is None:
        raise CollisionPlanError(
            "placement role %r has no declared collision policy; add it to "
            "ROLE_COLLISION rather than letting it default" % role
        )
    if shape == "none":
        return None

    minimum = [float(v) for v in bounds["min"]]
    maximum = [float(v) for v in bounds["max"]]
    scale = float(instance.get("scale", 1.0))
    yaw = float(instance.get("yaw_degrees", 0.0))
    position = [float(v) for v in instance["position_m"]]

    # Asset bounds are not centred on the asset origin, so the local centre
    # has to be scaled and rotated too -- dropping it would place every
    # collider at the origin of its mesh rather than the middle of its volume.
    local_centre = [(minimum[i] + maximum[i]) * 0.5 * scale for i in range(3)]
    half = [(maximum[i] - minimum[i]) * 0.5 * scale for i in range(3)]
    centre_x, centre_z = _rotate_y(local_centre[0], local_centre[2], yaw)
    centre = [
        position[0] + centre_x,
        position[1] + local_centre[1],
        position[2] + centre_z,
    ]

    identifier = instance["id"] if suffix is None else "%s#%s" % (instance["id"], suffix)

    if shape == "trunk_cylinder":
        radius = max(half[0], half[2]) * TRUNK_RADIUS_FRACTION
        return {
            "id": identifier,
            # Which placement this belongs to. A composite asset emits one
            # collider per solid part, so `id` is no longer the instance id --
            # consumers that need the placement must read this.
            "instance_id": instance["id"],
            "feature_id": instance.get("feature_id"),
            "role": role,
            "shape": "cylinder",
            "obstructs": True,
            "centre_m": [round(v, 6) for v in centre],
            "radius_m": round(radius, 6),
            "half_height_m": round(half[1], 6),
        }
    return {
        "id": identifier,
        "instance_id": instance["id"],
        "feature_id": instance.get("feature_id"),
        "role": role,
        "shape": "box" if shape == "box" else "deck",
        # A deck carries a body on its upper surface and stops nothing. It is
        # geometry, not an obstruction, so navigation must not subtract it --
        # unlike `none`, which means there is no surface here at all.
        #
        # Honest limit: the deck's *upper surface* is not yet emitted, so an
        # elevated bridge over a gorge would leave a gap the terrain does not
        # fill. Every crossing in the current world sits on walkable ground, so
        # nothing depends on it today.
        "obstructs": shape == "box",
        "centre_m": [round(v, 6) for v in centre],
        "half_extents_m": [round(v, 6) for v in half],
        "yaw_degrees": round(yaw, 6),
    }


def build(batch_dir: Path, asset_root: Path | None = None) -> dict[str, Any]:
    """Derive the collision plan from a compiled batch.

    `asset_root` is where the plan's relative asset paths resolve from. It used
    to be reconstructed as `batch_dir.parent.parent`, which silently encoded
    "every batch lives exactly two levels under the engine" -- the single
    assumption that made Luxel unable to compile a batch anywhere but inside its
    own tree (D8). Defaulted to the old behaviour so a standalone CLI run of an
    in-tree batch is unchanged.
    """
    terrain_dir = batch_dir / "terrain"
    manifest_path = terrain_dir / "terrain_manifest.json"
    heightfield_path = terrain_dir / "heightfield_f32le.bin"
    zone_spec_path = batch_dir / "zone_spec.json"
    render_plan_path = batch_dir / "render_plan.json"
    asset_plan_path = batch_dir / "asset_plan.json"
    preflight_path = (
        batch_dir / "asset_visual_preflight/asset_visual_preflight_report.json"
    )
    for required in (
        manifest_path,
        heightfield_path,
        zone_spec_path,
        render_plan_path,
        asset_plan_path,
        preflight_path,
    ):
        if not required.is_file():
            raise CollisionPlanError("%s is missing; compile the batch first" % required)

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    zone_spec = json.loads(zone_spec_path.read_text(encoding="utf-8"))
    render_plan = json.loads(render_plan_path.read_text(encoding="utf-8"))
    asset_plan = json.loads(asset_plan_path.read_text(encoding="utf-8"))
    preflight = json.loads(preflight_path.read_text(encoding="utf-8"))

    heightfield_digest = _digest(heightfield_path)
    if manifest.get("heightfield_sha256") != heightfield_digest:
        raise CollisionPlanError(
            "terrain manifest does not match the heightfield on disk; the "
            "terrain was rebuilt without rewriting its manifest"
        )
    if render_plan.get("heightfield_sha256") != heightfield_digest:
        raise CollisionPlanError(
            "render plan was built against a different heightfield; colliders "
            "derived from it would not match the world being rendered"
        )
    zone_spec_digest = _digest(zone_spec_path)
    # `zone_spec_sha256` is not one identity (T7 / D10): the terrain manifest
    # hashes the canonicalised JSON, build_report.json hashes the file bytes.
    # Match the manifest on its own terms rather than reporting a false
    # mismatch -- see boundary_plan.py, which this check mirrors.
    canonical_digest = _canonical_digest(zone_spec)
    if manifest.get("zone_spec_sha256") not in (None, canonical_digest):
        raise CollisionPlanError(
            "terrain was built from a different zone spec; colliders derived "
            "from this zone against another world's terrain would not match "
            "what a player actually walks into"
        )

    root = (asset_root or batch_dir.parent.parent).resolve()
    paths = _asset_paths_by_digest(asset_plan)
    bounds = _bounds_by_path(preflight)

    colliders: list[dict[str, Any]] = []
    skipped: dict[str, int] = {}
    for instance in render_plan.get("instances", []):
        digest = instance.get("asset_sha256")
        source = paths.get(digest)
        if source is None:
            raise CollisionPlanError(
                "render plan instance %s references asset %s, which the asset "
                "plan does not map to a source path"
                % (instance.get("id"), digest)
            )
        measured = bounds.get(source)
        if measured is None:
            raise CollisionPlanError(
                "no measured bounds for %s; asset physical acceptance must run "
                "before the collision plan" % source
            )
        # Asset paths in the plan are relative to the engine root.
        emitted = instance_colliders(instance, measured, root / source)
        if not emitted:
            role = str(instance.get("role"))
            skipped[role] = skipped.get(role, 0) + 1
            continue
        colliders.extend(emitted)

    world_bounds = manifest.get("world_bounds_m", {})
    height_range = manifest.get("height_range_m", {})
    return {
        "schema_version": SCHEMA_VERSION,
        "zone_id": manifest.get("zone_id"),
        # Bound to every artifact this is derived from. A collision plan that
        # outlives its terrain puts invisible walls where the world has none.
        # Both spellings, named for what they actually hash (T7 / D10), so a
        # consumer never has to guess which one a bare `zone_spec_sha256`
        # meant.
        "zone_spec_bytes_sha256": zone_spec_digest,
        "zone_spec_canonical_sha256": canonical_digest,
        "heightfield_sha256": heightfield_digest,
        # _bytes_ (T7 / D10): world_core/render_plan.rs's own fingerprint
        # block uses "terrain_manifest_sha256"/"asset_plan_sha256" for a
        # canonical-JSON hash inside a *different* artifact (render_plan.json)
        # for its own self-contained contract. Same key names, unrelated
        # values, never compared against each other -- named explicitly here
        # so nobody "fixes" one to match the other.
        "terrain_manifest_bytes_sha256": _digest(manifest_path),
        "asset_plan_bytes_sha256": _digest(asset_plan_path),
        "render_plan_sha256": _digest(render_plan_path),
        "terrain": {
            # A descriptor, not a mesh: every target engine builds a native
            # heightfield collider from these same bytes.
            "shape": "heightfield",
            "source": "terrain/heightfield_f32le.bin",
            "encoding": "float32_le_row_major",
            "resolution": manifest.get("resolution"),
            "world_bounds_m": world_bounds,
            "height_range_m": height_range,
        },
        "instance_collider_count": len(colliders),
        "obstructing_collider_count": sum(1 for c in colliders if c["obstructs"]),
        "instance_colliders": colliders,
        "non_colliding_by_role": dict(sorted(skipped.items())),
        "role_policy": dict(sorted(ROLE_COLLISION.items())),
    }


def main(argv: list[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("batch", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument(
        "--asset-root", type=Path,
        help="where the asset plan's relative paths resolve from; defaults to "
             "two levels above the batch, which only holds for an in-tree batch",
    )
    arguments = parser.parse_args(argv)

    try:
        plan = build(arguments.batch.resolve(), arguments.asset_root)
    except (CollisionPlanError, OSError, ValueError) as exc:
        print("%s: %s" % (arguments.batch, exc), file=__import__("sys").stderr)
        return 2

    destination = arguments.output or (arguments.batch / "collision_plan.json")
    destination.write_text(
        json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(
        "collision plan %s: %d instance colliders, terrain heightfield %dx%d"
        % (
            plan["zone_id"],
            plan["instance_collider_count"],
            plan["terrain"]["resolution"] or 0,
            plan["terrain"]["resolution"] or 0,
        )
    )
    for role, count in plan["non_colliding_by_role"].items():
        print("  %-24s %4d instances deliberately non-colliding" % (role, count))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
