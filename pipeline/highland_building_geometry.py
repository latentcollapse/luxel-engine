"""Pure geometry for the Highland individual-building kit. No `bpy` import.

`generate_highland_settlement_kit.py` baked five houses, a well, a terrace and
a watchtower into one glb per village, so a village placed one instance at a
time and every one of the three variants looked identical everywhere it was
used (see roadmap "split village clusters" task). This module is the
building-block half of the fix: it carries the variant specs and the vertex
math for `generate_highland_building_kit.py`, kept free of `bpy` so it is
importable and testable without Blender (roadmap D3's fix direction for the
whole generator family).

**The historical bug this module exists to not repeat** (D3): a gable roof's
mesh vertices were once baked with the house's *world* position folded into
them, while the roof object's own `obj.location` was left at the origin.
Rotating that object (`obj.rotation_euler[2] = ...`) rotates around
`obj.location`, so a roof authored that way spins around the wrong pivot the
moment anything upstream applies a rotation, and swings away from its own
walls. The fix, and the convention every function below follows, is that a
part's mesh data is always centred on the part's own local origin and the
world offset is carried exclusively by the placement transform (`obj.location`
in Blender; the instance transform in the render plan once this ships) --
never both.
"""

from __future__ import annotations

import math
from dataclasses import dataclass


@dataclass(frozen=True)
class CottageSpec:
    """One Cotswold-style cottage variant, in the shared engine convention
    (X/Z ground, Y up), all lengths in metres."""

    key: str
    width_m: float
    depth_m: float
    wall_height_m: float
    roof_pitch_m: float
    roof_material: str  # "slate" or "thatch"
    storeys: int
    door_side: str  # "front_left", "front_center", "front_right", "side"
    chimney_offsets: tuple[tuple[float, float], ...]  # local (x, z) metres

    @property
    def base_height_m(self) -> float:
        """Wall top / roof eave height above the foundation top."""
        return self.wall_height_m

    @property
    def ridge_height_m(self) -> float:
        return self.wall_height_m + self.roof_pitch_m

    @property
    def roof_suffix(self) -> str:
        return "slate_roof" if self.roof_material == "slate" else "thatch_roof"


# Five distinct variants, each differing in footprint, roof pitch, storeys,
# chimney placement, and door side -- Matt's art-direction checklist. No two
# variants share the same combination of (footprint, roof_material, storeys,
# door_side, chimney layout).
COTTAGE_VARIANTS: tuple[CottageSpec, ...] = (
    CottageSpec(
        key="A",
        width_m=8.0,
        depth_m=7.0,
        wall_height_m=3.1,
        roof_pitch_m=2.6,
        roof_material="slate",
        storeys=1,
        door_side="front_left",
        chimney_offsets=((2.2, -2.1),),
    ),
    CottageSpec(
        key="B",
        width_m=11.0,
        depth_m=8.4,
        wall_height_m=3.3,
        roof_pitch_m=3.4,
        roof_material="thatch",
        storeys=1,
        door_side="front_center",
        chimney_offsets=((-3.4, 0.0),),
    ),
    CottageSpec(
        key="C",
        width_m=9.0,
        depth_m=8.0,
        wall_height_m=4.6,
        roof_pitch_m=2.9,
        roof_material="slate",
        storeys=2,
        door_side="side",
        chimney_offsets=((0.0, 2.6),),
    ),
    CottageSpec(
        key="D",
        width_m=6.6,
        depth_m=10.2,
        wall_height_m=3.1,
        roof_pitch_m=3.6,
        roof_material="thatch",
        storeys=1,
        door_side="front_center",
        chimney_offsets=((1.9, 4.5),),
    ),
    CottageSpec(
        key="E",
        width_m=10.4,
        depth_m=9.0,
        wall_height_m=3.5,
        roof_pitch_m=2.5,
        roof_material="slate",
        storeys=1,
        door_side="front_left",
        chimney_offsets=((-3.1, -3.6), (3.1, 3.6)),
    ),
)


def roof_local_vertices(
    width_m: float, depth_m: float, base_height_m: float, ridge_height_m: float
) -> list[tuple[float, float, float]]:
    """Local gable-roof prism vertices, engine frame (x, y=height, z=depth).

    Horizontal coordinates are centred on the part's own origin; vertical
    coordinates are absolute heights above the building's own ground plane
    (the caller places the mesh with the building's ground at local y=0, the
    same convention `generate_highland_settlement_kit._roof` already uses).
    Never fold a world offset into these numbers -- that is the D3 bug.
    """
    half_width, half_depth = width_m * 0.5, depth_m * 0.5
    return [
        (-half_width, base_height_m, -half_depth),
        (half_width, base_height_m, -half_depth),
        (0.0, ridge_height_m, -half_depth),
        (-half_width, base_height_m, half_depth),
        (half_width, base_height_m, half_depth),
        (0.0, ridge_height_m, half_depth),
    ]


def footprint_corners(width_m: float, depth_m: float) -> list[tuple[float, float]]:
    """The four (x, z) corners of a rectangular footprint centred at origin."""
    half_width, half_depth = width_m * 0.5, depth_m * 0.5
    return [
        (-half_width, -half_depth),
        (half_width, -half_depth),
        (half_width, half_depth),
        (-half_width, half_depth),
    ]


def rotate_xz(point_xz: tuple[float, float], yaw_degrees: float) -> tuple[float, float]:
    """Rotate a horizontal (x, z) point about the origin by `yaw_degrees`."""
    x, z = point_xz
    angle = math.radians(yaw_degrees)
    cos_a, sin_a = math.cos(angle), math.sin(angle)
    return (x * cos_a - z * sin_a, x * sin_a + z * cos_a)


def point_in_convex_polygon(
    point_xz: tuple[float, float],
    polygon_xz: list[tuple[float, float]],
    tolerance_m: float = 1e-6,
) -> bool:
    """Whether `point_xz` lies inside (or on the boundary of) a convex polygon.

    Uses the standard same-sign-cross-product test. Vertex winding may be
    either direction, so the signs are normalised against the first nonzero
    cross product rather than assumed.
    """
    signs = []
    count = len(polygon_xz)
    for index in range(count):
        ax, az = polygon_xz[index]
        bx, bz = polygon_xz[(index + 1) % count]
        edge_x, edge_z = bx - ax, bz - az
        to_point_x, to_point_z = point_xz[0] - ax, point_xz[1] - az
        cross = edge_x * to_point_z - edge_z * to_point_x
        signs.append(cross)
    positive = any(value > tolerance_m for value in signs)
    negative = any(value < -tolerance_m for value in signs)
    return not (positive and negative)


def rigidly_placed_footprint_and_roof_centroid(
    spec: CottageSpec, yaw_degrees: float, world_xz: tuple[float, float]
) -> tuple[list[tuple[float, float]], tuple[float, float]]:
    """Simulate placing a correctly-authored cottage in the world.

    Both the wall footprint and the roof are parented under the same building
    root and share the building's own local origin as their rotation pivot --
    the fixed convention. Rotating the whole building by `yaw_degrees` and
    moving it to `world_xz` must keep the roof's horizontal centroid inside
    the (rotated, translated) wall footprint, for any yaw and any offset.
    """
    footprint_world = [
        (rotate_xz(corner, yaw_degrees)[0] + world_xz[0], rotate_xz(corner, yaw_degrees)[1] + world_xz[1])
        for corner in footprint_corners(spec.width_m, spec.depth_m)
    ]
    roof_vertices = roof_local_vertices(
        spec.width_m + 0.4, spec.depth_m + 0.4, spec.base_height_m, spec.ridge_height_m
    )
    roof_xz_world = [
        (rotate_xz((vx, vz), yaw_degrees)[0] + world_xz[0], rotate_xz((vx, vz), yaw_degrees)[1] + world_xz[1])
        for vx, _vy, vz in roof_vertices
    ]
    centroid = (
        sum(x for x, _z in roof_xz_world) / len(roof_xz_world),
        sum(z for _x, z in roof_xz_world) / len(roof_xz_world),
    )
    return footprint_world, centroid


def historically_buggy_footprint_and_roof_centroid(
    spec: CottageSpec, yaw_degrees: float, world_xz: tuple[float, float]
) -> tuple[list[tuple[float, float]], tuple[float, float]]:
    """Reproduce the D3 bug on purpose, for the negative-case test.

    The wall footprint is placed correctly (location carries the offset). The
    roof, as in the original defect, has the world offset baked into its
    vertex data while its own pivot stays at the world origin -- so rotating
    it rotates around (0, 0) instead of around `world_xz`.
    """
    footprint_world = [
        (rotate_xz(corner, yaw_degrees)[0] + world_xz[0], rotate_xz(corner, yaw_degrees)[1] + world_xz[1])
        for corner in footprint_corners(spec.width_m, spec.depth_m)
    ]
    roof_vertices = roof_local_vertices(
        spec.width_m + 0.4, spec.depth_m + 0.4, spec.base_height_m, spec.ridge_height_m
    )
    # Bug: world offset baked into the vertex data itself...
    baked_xz = [(vx + world_xz[0], vz + world_xz[1]) for vx, _vy, vz in roof_vertices]
    # ...then the object is rotated around the origin anyway, because its own
    # `obj.location` was left at (0, 0) instead of `world_xz`.
    roof_xz_world = [rotate_xz(point, yaw_degrees) for point in baked_xz]
    centroid = (
        sum(x for x, _z in roof_xz_world) / len(roof_xz_world),
        sum(z for _x, z in roof_xz_world) / len(roof_xz_world),
    )
    return footprint_world, centroid


def watchtower_total_height_m() -> float:
    """Stone base + timber tower + roof + merlons, for physical-acceptance checks."""
    return 4.5 + 8.5 + 2.6 + 0.8


def well_total_height_m() -> float:
    return 1.1 + 1.0
