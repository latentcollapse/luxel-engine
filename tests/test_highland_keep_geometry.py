"""Tests for `highland_keep_geometry.py`, the pure half of the keep kit
generator (D3's "untested Blender generators", and the regression net under the
D17 sealed-keep fix).

Every assertion here is paired with a negative case, following
`test_highland_building_geometry.py`: the sealed keep passed every gate the
pipeline had for weeks precisely because nothing could fail, so a test that
proves the gate is open is worth only as much as the matching proof that it
would notice a shut one.

The two measurements under test are the ones that decide whether a keep is a
building or a wall:

- `gate_clearance` -- a body can get through the curtain
- `courtyard_ring` -- once through, there is somewhere to stand

Both were live defects. The curtain had no opening at all, and once it did, the
courtyard was still too cramped to hold a navigable cell.
"""

from __future__ import annotations

import math
import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from asset_parts import _group_of  # noqa: E402
import highland_keep_geometry as geometry  # noqa: E402
from highland_keep_geometry import (  # noqa: E402
    DONJON_RADIUS,
    GATE_BAND,
    GATE_LOCAL_BEARING_DEGREES,
    VARIANTS,
    KeepSpec,
    Part,
    affordances,
    courtyard_ring,
    gate_clearance,
    keep_parts,
    verify,
)

AGENT_RADIUS_M = 2.5
MAX_CLIMB_M = 4.0
SCALE = 0.1306


def _measure(spec: KeepSpec, **overrides):
    settings = {"scale": SCALE, "agent_radius_m": AGENT_RADIUS_M, "max_climb_m": MAX_CLIMB_M}
    settings.update(overrides)
    return settings


class GatewayTest(unittest.TestCase):
    def test_every_variant_is_enterable_by_the_default_agent(self):
        for spec in VARIANTS:
            with self.subTest(variant=spec.key):
                self.assertEqual([], verify(spec, **_measure(spec)))

    def test_gateway_clears_more_than_one_agent_diameter(self):
        for spec in VARIANTS:
            with self.subTest(variant=spec.key):
                clear = gate_clearance(keep_parts(spec), scale=SCALE, max_climb_m=MAX_CLIMB_M)
                self.assertGreater(clear * 2.0 * SCALE, 2.0 * AGENT_RADIUS_M)

    def test_no_obstructing_part_straddles_the_threshold(self):
        """The plan-view check, stated independently of `gate_clearance`.

        `rasterize_colliders` subtracts a part's whole ground footprint whenever
        its top is above the climb limit -- it never reads the underside -- so
        an arch over the gate blocks exactly as a solid block does. Nothing
        solid may cross x=0 inside the gate band, at any height.
        """
        for spec in VARIANTS:
            for part in keep_parts(spec):
                if not part.obstructs(scale=SCALE, max_climb_m=MAX_CLIMB_M):
                    continue
                half_z = part.half_extent_z(rotated=True)
                if part.centre[2] + half_z < GATE_BAND[0] or part.centre[2] - half_z > GATE_BAND[1]:
                    continue
                half_x = part.half_extent_x(rotated=True)
                with self.subTest(variant=spec.key, part=part.name):
                    self.assertGreaterEqual(
                        abs(part.centre[0]) - half_x, 0.0,
                        "%s spans the gate centreline" % part.name,
                    )

    def test_a_closed_aperture_is_detected(self):
        """The negative case: with no aperture carved, the curtain seals.

        Reconstructs the actual historical defect -- a complete ring of curtain
        segments -- by collapsing the aperture the layout carves out, and
        asserts `verify` says so rather than passing on geometry nobody can
        walk through.
        """
        with mock.patch.object(geometry, "GATE_HALF_WIDTH", 0.0):
            for spec in VARIANTS:
                with self.subTest(variant=spec.key):
                    clear = gate_clearance(keep_parts(spec), scale=SCALE, max_climb_m=MAX_CLIMB_M)
                    self.assertEqual(0.0, clear)
                    problems = verify(spec, **_measure(spec))
                    self.assertTrue(problems)
                    self.assertIn("closes its own gateway", problems[0])

    def test_a_solid_gatehouse_reseals_the_keep(self):
        """The specific part that caused D17: one cube centred on the gate.

        A 54-unit block at the wall line is what the kit used to build, and it
        must be caught even though every other part is correctly placed.
        """
        for spec in VARIANTS:
            parts = keep_parts(spec)
            parts.append(
                Part("Gatehouse", "box", (0.0, 39.0, spec.wall_radius + 3.0), (54.0, 62.0, 38.0), "stone")
            )
            with self.subTest(variant=spec.key):
                self.assertEqual(0.0, gate_clearance(parts, scale=SCALE, max_climb_m=MAX_CLIMB_M))

    def test_a_closed_gate_leaf_reseals_the_keep(self):
        """And the smaller version of the same defect: a shut door.

        `Gate_shadow` was a 19-unit iron leaf drawn across the threshold. A
        closed door is a wall that looks like a gate.
        """
        for spec in VARIANTS:
            parts = keep_parts(spec)
            parts.append(
                Part("Gate_shadow", "box", (0.0, 23.0, spec.wall_radius + 22.5), (19.0, 25.0, 2.0), "iron")
            )
            with self.subTest(variant=spec.key):
                self.assertEqual(0.0, gate_clearance(parts, scale=SCALE, max_climb_m=MAX_CLIMB_M))

    def test_collider_reading_is_measured_as_well_as_rendered(self):
        """`asset_parts` ignores node rotation, so both readings must clear.

        A curtain segment is rotated tangentially on its node but collides as an
        axis-aligned 29x12. Measuring only the rendered box would open the gate
        in the viewer and leave it shut in the navigation surface, which is the
        exact shape of a defect that survives a screenshot review.
        """
        spec = VARIANTS[1]
        rotated = [part for part in keep_parts(spec) if part.yaw_radians]
        self.assertTrue(rotated, "curtain segments should carry a yaw")
        differing = [
            part for part in rotated
            if abs(part.half_extent_x(rotated=True) - part.half_extent_x(rotated=False)) > 1e-9
        ]
        self.assertTrue(differing, "rotation should change the measured extent for some segment")


class CourtyardTest(unittest.TestCase):
    def test_every_variant_has_a_walkable_ring(self):
        for spec in VARIANTS:
            with self.subTest(variant=spec.key):
                ring = courtyard_ring(spec, keep_parts(spec), scale=SCALE, max_climb_m=MAX_CLIMB_M)
                self.assertGreaterEqual(ring * SCALE, 2.0 * AGENT_RADIUS_M)

    def test_an_oversized_central_mass_is_detected(self):
        """The negative case, and the second half of the real D17 defect.

        The gate can be wide open and the keep still strand its own anchor if
        nothing fits between the central mass and the wall -- which is what the
        70x62 hall and the misplaced bastions did.
        """
        spec = VARIANTS[1]
        parts = [part for part in keep_parts(spec) if part.name != "Great_hall"]
        parts.append(Part("Great_hall", "box", (0.0, 44.0, 0.0), (170.0, 72.0, 170.0), "stone"))
        ring = courtyard_ring(spec, parts, scale=SCALE, max_climb_m=MAX_CLIMB_M)
        self.assertLess(ring * SCALE, 2.0 * AGENT_RADIUS_M)

    def test_the_hall_reads_as_adjoining_the_donjon(self):
        """A clearance budget does not make a silhouette.

        The D17 courtyard fix shrank the central mass and centred it, which
        satisfied every measurement and produced a square hall concentric
        inside a narrower round tower -- its corners burst through the donjon
        as a cube stuck on a cylinder. Nothing caught it, because "does a body
        fit around this" and "does this look like a building" are different
        questions and only the first was asked.

        The hall must therefore sit far enough off the donjon's axis to read as
        adjoining it rather than embedded in it.
        """
        for spec in VARIANTS:
            hall = {p.name: p for p in keep_parts(spec)}["Great_hall"]
            offset = math.hypot(hall.centre[0], hall.centre[2])
            with self.subTest(variant=spec.key):
                self.assertGreaterEqual(
                    offset, DONJON_RADIUS,
                    "hall centre is %.1f from the donjon axis; inside its %.1f radius "
                    "the hall reads as a cube stuck through the tower" % (offset, DONJON_RADIUS),
                )

    def test_a_concentric_hall_is_detected(self):
        """The negative case: the exact regression, reconstructed."""
        concentric = KeepSpec(
            key="regression", wall_segments=20, tower_count=6,
            hall_offset=(0.0, 0.0), outer_bastions=True,
        )
        hall = {p.name: p for p in keep_parts(concentric)}["Great_hall"]
        self.assertLess(math.hypot(hall.centre[0], hall.centre[2]), DONJON_RADIUS)
        # And it passed the clearance budget, which is why it shipped.
        self.assertEqual([], verify(concentric, **_measure(concentric)))

    def test_bastions_sit_outside_the_courtyard(self):
        """They plugged the ring at 8.3 m when centred at radius 72."""
        spec = VARIANTS[1]
        self.assertTrue(spec.outer_bastions)
        bastions = [p for p in keep_parts(spec) if p.name.startswith("Outer_bastion_")]
        self.assertTrue(bastions)
        for part in bastions:
            with self.subTest(part=part.name):
                self.assertGreater(
                    math.hypot(part.centre[0], part.centre[2]), spec.courtyard_inner_face
                )


class FloorTest(unittest.TestCase):
    def test_low_slabs_are_floors_not_walls(self):
        """The plinth, the courtyard slab and the drawbridge are walked *on*.

        `rasterize_colliders` steps a body onto anything shorter than its climb
        height, so treating these as obstructions here would make the module
        disagree with the navigation surface it exists to predict.
        """
        parts = {part.name: part for part in keep_parts(VARIANTS[1])}
        for name in ("Bedrock_plinth", "Inner_courtyard", "Drawbridge_plank_02"):
            with self.subTest(part=name):
                self.assertFalse(parts[name].obstructs(scale=SCALE, max_climb_m=MAX_CLIMB_M))
        for name in ("Central_donjon", "Great_hall", "Gate_pier_1.0"):
            with self.subTest(part=name):
                self.assertTrue(parts[name].obstructs(scale=SCALE, max_climb_m=MAX_CLIMB_M))

    def test_a_raised_climb_limit_turns_walls_into_floors(self):
        """The climb test is physical, not a name list -- so it must move."""
        wall = {p.name: p for p in keep_parts(VARIANTS[1])}["Curtain_segment_00"]
        self.assertTrue(wall.obstructs(scale=SCALE, max_climb_m=MAX_CLIMB_M))
        self.assertFalse(wall.obstructs(scale=SCALE, max_climb_m=99.0))


class AgentPolicyTest(unittest.TestCase):
    def test_a_larger_agent_no_longer_fits(self):
        """The reason the agent is an argument and not a constant.

        The same geometry that is a keep for a 2.5 m body is a sealed box for a
        wide enough one. Baking the agent into the kit is how a generator
        silently stops being valid for the world that uses it.
        """
        spec = VARIANTS[1]
        self.assertEqual([], verify(spec, **_measure(spec)))
        problems = verify(spec, **_measure(spec, agent_radius_m=6.0))
        self.assertTrue(problems)

    def test_a_smaller_placed_scale_no_longer_fits(self):
        """Scale is the other half of the same question."""
        spec = VARIANTS[1]
        problems = verify(spec, **_measure(spec, scale=SCALE * 0.4))
        self.assertTrue(problems)

    def test_affordance_contract_reports_what_it_measured(self):
        spec = VARIANTS[1]
        contract = affordances(spec, **_measure(spec))
        self.assertEqual(GATE_LOCAL_BEARING_DEGREES, contract["enterable"]["local_bearing_degrees"])
        self.assertEqual(AGENT_RADIUS_M, contract["measured_against"]["agent_radius_m"])
        self.assertEqual(SCALE, contract["measured_against"]["placed_scale"])
        self.assertGreater(contract["enterable"]["threshold_clear_m"], 2.0 * AGENT_RADIUS_M)
        self.assertGreaterEqual(contract["enterable"]["interior_ring_m"], 2.0 * AGENT_RADIUS_M)


class PartNamingTest(unittest.TestCase):
    def test_part_names_are_unique(self):
        """Duplicate names silently merge into one collider box.

        `asset_parts` groups by name, so two parts sharing one would collide as
        their union -- a collider spanning ground neither part occupies.
        """
        for spec in VARIANTS:
            names = [part.name for part in keep_parts(spec)]
            with self.subTest(variant=spec.key):
                self.assertEqual(len(names), len(set(names)))

    def test_pier_merlons_group_into_their_pier(self):
        """Naming carries a collision cost, so it is asserted, not assumed."""
        self.assertEqual("Gate_pier_1.0", _group_of("Gate_pier_1.0_merlon_00"))
        self.assertEqual("Gate_pier_1.0", _group_of("Gate_pier_1.0"))

    def test_the_gate_faces_the_declared_local_bearing(self):
        """The gate is authored on +Z; glTF export negates z, so the shipped
        asset faces -Z. Placement rotates *from* this bearing, so a wrong value
        here aims every keep's gate at the wrong thing."""
        self.assertEqual(180.0, GATE_LOCAL_BEARING_DEGREES)
        spec = VARIANTS[1]
        piers = [p for p in keep_parts(spec) if p.name.startswith("Gate_pier_-")]
        self.assertTrue(piers)
        self.assertGreater(piers[0].centre[2], 0.0)


class VariantTest(unittest.TestCase):
    def test_variants_differ_in_silhouette(self):
        a, b = keep_parts(VARIANTS[0]), keep_parts(VARIANTS[1])
        self.assertNotEqual(
            len([p for p in a if p.name.startswith("Curtain_segment_")]),
            len([p for p in b if p.name.startswith("Curtain_segment_")]),
        )

    def test_a_tower_on_the_gate_azimuth_is_dropped(self):
        """Variant a's 8-tower ring puts one dead centre on the gate.

        Dropping it falls out of measuring the aperture rather than out of a
        per-variant special case, which is what keeps the rule correct for the
        next tower count someone picks.
        """
        towers = [p for p in keep_parts(VARIANTS[0]) if p.name.startswith("Corner_tower_")]
        self.assertEqual(VARIANTS[0].tower_count - 1, len(towers))
        self.assertEqual(
            VARIANTS[1].tower_count,
            len([p for p in keep_parts(VARIANTS[1]) if p.name.startswith("Corner_tower_")]),
        )


if __name__ == "__main__":
    unittest.main()
