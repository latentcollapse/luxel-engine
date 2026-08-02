"""Pure geometry for the Highland fortified keep kit. No `bpy` import.

`generate_highland_keep_kit.py` emitted a curtain wall with no opening and a
solid `Gatehouse` cube straddling the gate azimuth, so both faction keeps were
sealed inside their own walls. Per-part collision made that measurable and
`navigation_plan` reported it (D17), but nothing between the generator and the
compiled batch could have caught it: the entrance was a cube *named* Gatehouse,
and a name is not a claim anything checks.

This module is the checkable half. It carries the keep's part layout and the
two measurements that decide whether the keep is a building or a wall:

- `gate_clearance` -- can a body get *through* the curtain
- `courtyard_ring` -- once through, is there anywhere to stand

Both are pure arithmetic over part boxes, so they run as unit tests in
milliseconds instead of a 70-second Blender round trip (D3's fix direction for
the whole generator family, and the same split `highland_building_geometry.py`
already made for the settlement kit).

**Why the measurements are duplicated per part.** A part's rendered box and its
*collider* are not the same box. The curtain segments are rotated tangentially
on their Blender node, and `asset_parts` deliberately does not apply node
rotation, so collision sees an axis-aligned 29x12 where the render shows a
tangential one. A gate that clears only one of those readings opens in the
viewer and stays shut in the navigation surface, so every gate test below takes
the wider of the two.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field


# How much walkable width the gateway must leave, in asset units. The gate
# faces local +Z; that is not a parameter because the whole gatehouse is
# authored along that axis, and a symbol suggesting the azimuth were free would
# be a lie about the kit. Aiming the gate at the map is the placement's job,
# through the keep feature's `properties.rotation_degrees`.
GATE_HALF_WIDTH = 34.0

# Depth band, in local z, over which a part counts as standing in the gateway:
# the wall crossing itself plus the approach the drawbridge lands on.
GATE_BAND = (61.0, 146.0)

# The gate faces local +Z, but Blender's glTF export negates z, so in the
# shipped asset -- and therefore in every engine that loads it -- the threshold
# faces -Z. Placement has to rotate *from* this bearing to aim the gate at
# anything, so it is stated once here rather than rediscovered per consumer.
GATE_LOCAL_BEARING_DEGREES = 180.0

CURTAIN_THICKNESS = 12.0
CURTAIN_LENGTH = 29.0


@dataclass(frozen=True)
class Part:
    """One solid piece of the keep, in the shared engine convention.

    `centre` is (x, y-up, z) and `size` is full extents; a cylinder or cone
    carries its diameter in x and z. `yaw_radians` is rendered but deliberately
    *not* folded into the extents -- see the module docstring.
    """

    name: str
    shape: str  # "box" | "cylinder" | "cone"
    centre: tuple[float, float, float]
    size: tuple[float, float, float]
    material: str
    bevel: float = 0.0
    vertices: int = 16
    yaw_radians: float = 0.0

    @property
    def top(self) -> float:
        return self.centre[1] + self.size[1] * 0.5

    def half_extent_x(self, *, rotated: bool) -> float:
        """Half-width across the gate axis.

        `rotated=False` is what `asset_parts` reports and therefore what
        collision uses; `rotated=True` is what the mesh actually occupies.
        """
        half_x, half_z = self.size[0] * 0.5, self.size[2] * 0.5
        if not rotated or not self.yaw_radians:
            return half_x
        return abs(half_x * math.cos(self.yaw_radians)) + abs(
            half_z * math.sin(self.yaw_radians)
        )

    def half_extent_z(self, *, rotated: bool) -> float:
        half_x, half_z = self.size[0] * 0.5, self.size[2] * 0.5
        if not rotated or not self.yaw_radians:
            return half_z
        return abs(half_z * math.cos(self.yaw_radians)) + abs(
            half_x * math.sin(self.yaw_radians)
        )

    def obstructs(self, *, scale: float, max_climb_m: float) -> bool:
        """Whether a body walks into this part rather than onto it.

        The same physical test `navigation_plan.rasterize_colliders` applies, so
        the plinth, the courtyard slab and the drawbridge planks read as floors
        here for the same reason they do there.
        """
        return self.top * scale > max_climb_m


@dataclass(frozen=True)
class KeepSpec:
    """One keep variant. `wall_segments` and `tower_count` differ so the two
    realms read differently from the air without forking the whole kit."""

    key: str
    wall_segments: int
    tower_count: int
    hall_offset: tuple[float, float]
    outer_bastions: bool
    wall_radius: float = 101.0

    @property
    def courtyard_inner_face(self) -> float:
        return self.wall_radius - CURTAIN_THICKNESS * 0.5


DONJON_RADIUS = 24.0

VARIANTS: tuple[KeepSpec, ...] = (
    KeepSpec(key="a", wall_segments=24, tower_count=8, hall_offset=(0.0, -26.0), outer_bastions=False),
    KeepSpec(key="b", wall_segments=20, tower_count=6, hall_offset=(-26.0, 0.0), outer_bastions=True),
)


def _blocks_gateway(x_centre: float, x_half: float, z_centre: float, z_half: float) -> bool:
    return (
        z_centre + z_half >= GATE_BAND[0]
        and z_centre - z_half <= GATE_BAND[1]
        and abs(x_centre) - x_half < GATE_HALF_WIDTH
    )


def keep_parts(spec: KeepSpec) -> list[Part]:
    """Every solid part of one keep variant, in build order."""
    parts: list[Part] = []
    add = parts.append
    radius = spec.wall_radius

    add(Part("Bedrock_plinth", "cylinder", (0.0, 3.5, 0.0), (264.0, 7.0, 264.0), "dark_stone", vertices=32))
    add(Part("Inner_courtyard", "cylinder", (0.0, 8.0, 0.0), (226.0, 2.0, 226.0), "stone", vertices=32))

    for index in range(spec.wall_segments):
        angle = math.tau * index / float(spec.wall_segments)
        x, z = math.cos(angle) * radius, math.sin(angle) * radius
        yaw = angle + math.pi * 0.5
        # Widest of the two readings: a segment that reaches into the aperture
        # under either one is not built. This is what opens the gate, and it is
        # a measurement rather than an index list, so the 24-segment variant
        # opens correctly without a second rule.
        half_x = max(
            abs(CURTAIN_LENGTH * 0.5 * math.sin(angle)) + abs(CURTAIN_THICKNESS * 0.5 * math.cos(angle)),
            CURTAIN_LENGTH * 0.5,
        )
        if _blocks_gateway(x, half_x, z, CURTAIN_LENGTH * 0.5):
            continue
        add(Part("Curtain_segment_%02d" % index, "box", (x, 27.0, z), (CURTAIN_LENGTH, 38.0, CURTAIN_THICKNESS),
                 "stone" if index % 3 else "dark_stone", bevel=1.3, yaw_radians=yaw))
        for tooth in range(-1, 2):
            offset = tooth * 8.5
            bx = x + math.cos(angle + math.pi * 0.5) * offset
            bz = z + math.sin(angle + math.pi * 0.5) * offset
            add(Part("Wall_merlon_%02d_%d" % (index, tooth), "box", (bx, 48.0, bz), (6.0, 9.0, 13.5),
                     "dark_stone", bevel=0.55, yaw_radians=yaw))

    for index in range(spec.tower_count):
        angle = math.tau * index / float(spec.tower_count)
        x, z = math.cos(angle) * radius, math.sin(angle) * radius
        # Merlons ring the shaft at radius 13.5 with a 2-unit half-width, so
        # 15.5 is the tower's real footprint, not its 16-unit shaft radius. The
        # 24-segment variant puts a tower dead centre on the gate azimuth;
        # measuring rather than assuming drops that one and leaves the
        # 20-segment variant's ring untouched.
        if _blocks_gateway(x, 15.5, z, 15.5):
            continue
        add(Part("Corner_tower_%02d" % index, "cylinder", (x, 38.0, z), (32.0, 66.0, 32.0), "stone", vertices=14))
        add(Part("Tower_slate_roof_%02d" % index, "cone", (x, 86.0, z), (38.0, 30.0, 38.0), "slate", vertices=14))
        for tooth in range(8):
            tooth_angle = math.tau * tooth / 8.0
            add(Part("Tower_merlon_%02d_%02d" % (index, tooth), "box",
                     (x + math.cos(tooth_angle) * 13.5, 74.0, z + math.sin(tooth_angle) * 13.5),
                     (4.0, 8.0, 4.0), "dark_stone", bevel=0.35))

    # Gatehouse: two piers framing the aperture, where a single 54-unit cube
    # centred on the gate used to stand. That cube sealed the very opening it
    # was built to mark (D17). The piers close the wall line from the aperture
    # edge out to the nearest surviving curtain segment, so the gate reads as a
    # gatehouse rather than as a missing third of the front wall.
    pier_inner, pier_outer = GATE_HALF_WIDTH, 48.0
    pier_centre = (pier_inner + pier_outer) * 0.5
    for sign in (-1.0, 1.0):
        add(Part("Gate_pier_" + str(sign), "box", (sign * pier_centre, 33.0, radius + 3.0),
                 (pier_outer - pier_inner, 50.0, 40.0), "stone", bevel=2.0))
        for tooth in range(3):
            # `_merlon_NN` groups into the pier's own collider rather than
            # emitting three more; see `asset_parts._GROUP_PATTERN`.
            add(Part("Gate_pier_%s_merlon_%02d" % (sign, tooth), "box",
                     (sign * pier_centre + (tooth - 1) * 4.5, 62.0, radius + 3.0),
                     (5.0, 9.0, 36.0), "dark_stone", bevel=0.5))
        add(Part("Gate_flank_tower_" + str(sign), "cylinder", (sign * 48.0, 42.0, radius + 5.0),
                 (27.0, 75.0, 27.0), "stone", vertices=14))
        add(Part("Gate_flank_roof_" + str(sign), "cone", (sign * 48.0, 95.0, radius + 5.0),
                 (32.0, 31.0, 32.0), "slate", vertices=14))
        # The gates stand open, folded back beyond the piers. The old
        # `Gate_shadow` was one iron leaf drawn across the threshold: a closed
        # door is a wall that looks like a gate, which is the same defect as the
        # solid gatehouse, only smaller.
        add(Part("Gate_leaf_" + str(sign), "box", (sign * 36.5, 23.0, radius + 27.0),
                 (4.0, 25.0, 20.0), "iron", bevel=0.25))
    # Planks are 1 unit tall against a 4 m climb limit -- a floor, not an
    # obstruction, so they cross the threshold without narrowing it.
    for index in range(5):
        add(Part("Drawbridge_plank_%02d" % index, "box", ((index - 2) * 4.0, 10.5, radius + 32.0),
                 (3.4, 1.0, 30.0), "timber", bevel=0.08))

    # The central mass is budgeted against the courtyard, not composed freely.
    # A keep with an open gate and no room to stand inside it is the same defect
    # one step further in: the old 70x62 hall reached 6.9 m from centre and the
    # bastions plugged the ring at 8.3 m, against a 12.4 m inner face -- under
    # 5 m of ring for an agent that needs 5 m to fit, so the courtyard broke
    # into pockets and the keep anchor resolved into a 24-cell one.
    #
    # The hall is offset from the donjon on purpose. Budgeting the central mass
    # by radius alone is not enough to make it *read*: centring both put a
    # square hall concentric inside a round tower of smaller radius, so the
    # hall's corners burst out of the donjon as a cube stuck through a cylinder
    # -- visible in the viewer, invisible to a clearance measurement, and a
    # regression introduced by the D17 courtyard fix. Offsetting restores the
    # silhouette the kit was composed for, a keep rising from the end of its
    # hall, and `test_the_hall_reads_as_adjoining_the_donjon` holds it there.
    # Lower and narrower than the old 70x72x62 block, because it has to clear
    # the donjon's axis *and* stay inside the courtyard budget: offsetting a
    # hall of the old proportions far enough to read would have pushed it back
    # into the ring the D17 fix opened. A 6 m hall beside a 16 m donjon is also
    # simply the right silhouette -- the old one was a second keep.
    add(Part("Great_hall", "box", (spec.hall_offset[0], 31.0, spec.hall_offset[1]),
             (36.0, 46.0, 30.0), "stone", bevel=2.5))
    add(Part("Central_donjon", "cylinder", (0.0, 69.0, 0.0),
             (DONJON_RADIUS * 2, 122.0, DONJON_RADIUS * 2), "dark_stone", vertices=18))
    add(Part("Central_slate_spire", "cone", (0.0, 157.0, 0.0), (56.0, 54.0, 56.0), "slate", vertices=18))
    for index in range(12):
        angle = math.tau * index / 12.0
        add(Part("Donjon_merlon_%02d" % index, "box",
                 (math.cos(angle) * 20.5, 132.0, math.sin(angle) * 20.5), (5.0, 10.0, 5.0), "stone", bevel=0.4))

    if spec.outer_bastions:
        # Bastions belong *on* the curtain line. At radius 72 their inner
        # corners sat 8.3 m from centre, inside the courtyard they were meant to
        # defend, and closed the ring on that flank outright.
        for sign in (-1.0, 1.0):
            add(Part("Outer_bastion_%s" % sign, "box", (sign * 93.4, 31.0, -75.2),
                     (38.0, 50.0, 45.0), "stone", bevel=2.0))
            add(Part("Outer_bastion_roof_%s" % sign, "cone", (sign * 93.4, 69.0, -75.2),
                     (50.0, 26.0, 50.0), "slate", vertices=12))
    return parts


def gate_clearance(parts: list[Part], *, scale: float, max_climb_m: float) -> float:
    """Narrowest clear half-width of the gateway, in asset units.

    Measured over every built part rather than accumulated as each is placed,
    so a part nobody remembered to declare cannot quietly close the gate. That
    is not hypothetical: the flanking roof *cones* overhang further than the
    tower shafts they cap, and a per-call-site tally that only counted shafts
    reported a clear gate while the cones were the real constraint.
    """
    narrowest = math.inf
    for part in parts:
        if not part.obstructs(scale=scale, max_climb_m=max_climb_m):
            continue
        for rotated in (False, True):
            half_z = part.half_extent_z(rotated=rotated)
            if part.centre[2] + half_z < GATE_BAND[0] or part.centre[2] - half_z > GATE_BAND[1]:
                continue
            half_x = part.half_extent_x(rotated=rotated)
            if abs(part.centre[0]) < half_x:
                return 0.0  # straddles the centreline: the gateway is sealed
            narrowest = min(narrowest, abs(part.centre[0]) - half_x)
    return narrowest


def courtyard_ring(spec: KeepSpec, parts: list[Part], *, scale: float, max_climb_m: float) -> float:
    """Walkable ring between the central mass and the curtain, in asset units.

    `gate_clearance` only proves a body can get *through* the wall. This proves
    there is somewhere to arrive: `navigation_plan` erodes the walkable surface
    by the agent radius, so a courtyard ring narrower than one agent diameter
    has no navigable cell in it at all, and the keep's anchor resolves into
    whatever pocket survives instead of into the map.
    """
    inner_face = spec.courtyard_inner_face
    reach = 0.0
    for part in parts:
        if not part.obstructs(scale=scale, max_climb_m=max_climb_m):
            continue
        # Parts centred out on the wall line are the curtain, towers and gate;
        # they bound the ring rather than intrude into it.
        if math.hypot(part.centre[0], part.centre[2]) > inner_face * 0.75:
            continue
        half_x = part.half_extent_x(rotated=True)
        half_z = part.half_extent_z(rotated=True)
        reach = max(reach, math.hypot(abs(part.centre[0]) + half_x, abs(part.centre[2]) + half_z))
    return inner_face - reach


def affordances(spec: KeepSpec, *, scale: float, agent_radius_m: float, max_climb_m: float) -> dict:
    """The keep's declared, measured affordance contract.

    Emitted beside the GLB so the claim travels with the asset. Downstream this
    is what lets `asset_physical_acceptance` re-verify a shipped mesh against
    the world it is about to be placed in, and what gives placement a real gate
    bearing instead of a random yaw.
    """
    parts = keep_parts(spec)
    threshold = gate_clearance(parts, scale=scale, max_climb_m=max_climb_m) * 2.0
    ring = courtyard_ring(spec, parts, scale=scale, max_climb_m=max_climb_m)
    return {
        "enterable": {
            "threshold_clear_m": round(threshold * scale, 4),
            "interior_ring_m": round(ring * scale, 4),
            "local_bearing_degrees": GATE_LOCAL_BEARING_DEGREES,
            # Where along the outward bearing the threshold is, so the claim can
            # be re-measured from the shipped mesh by something that knows
            # nothing about keeps. Without it a verifier would have to guess
            # whether an obstruction is the gate being shut or simply the far
            # wall of the building, and would reject every enterable asset.
            "threshold_band_m": [
                round(GATE_BAND[0] * scale, 4),
                round(GATE_BAND[1] * scale, 4),
            ],
        },
        "measured_against": {
            "agent_radius_m": agent_radius_m,
            "agent_max_climb_m": max_climb_m,
            "placed_scale": scale,
        },
    }


def verify(spec: KeepSpec, *, scale: float, agent_radius_m: float, max_climb_m: float) -> list[str]:
    """Every reason this variant is not a building. Empty means it is one."""
    parts = keep_parts(spec)
    required = 2.0 * agent_radius_m
    problems: list[str] = []
    threshold = gate_clearance(parts, scale=scale, max_climb_m=max_climb_m) * 2.0 * scale
    if threshold <= required:
        problems.append(
            "variant %s closes its own gateway: %.2f m of clear width against the "
            "%.2f m an agent of radius %.1f m needs to fit. A keep nothing can walk "
            "into is the defect this kit exists to have fixed (D17)."
            % (spec.key, threshold, required, agent_radius_m)
        )
    ring = courtyard_ring(spec, parts, scale=scale, max_climb_m=max_climb_m) * scale
    if ring < required:
        problems.append(
            "variant %s has no walkable courtyard: %.2f m between the central mass "
            "and the curtain's inner face, against the %.2f m an agent of radius "
            "%.1f m needs. A keep that can be entered but not stood in strands its "
            "own anchor (D17)." % (spec.key, ring, required, agent_radius_m)
        )
    return problems
