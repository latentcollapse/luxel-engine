"""Tests for `asset_affordance.py`, the re-measurable half of an asset's claim.

The failure this exists to prevent is specific: an asset that advertises a way
in, and a mesh that no longer has one. Every check below is paired with the
defect it catches, because an acceptance gate that only ever passes is
indistinguishable from no gate at all -- which is what the pipeline had while
both keeps were sealed (D17).

Threshold measurement is exercised against synthetic part tables rather than
exported GLBs so the sealed cases can be constructed exactly; the contract
round-trip is exercised against the real shipped keep.
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from reference_content import CONTENT_ROOT  # noqa: E402

import asset_affordance  # noqa: E402
from asset_affordance import (  # noqa: E402
    AffordanceError,
    ROLE_REQUIRED_AFFORDANCES,
    load_contract,
    measure_threshold_m,
    verify,
)

SCALE = 0.1306
CLIMB_M = 4.0
RADIUS_M = 2.5
KEEP = (
    CONTENT_ROOT
    / "assets/generated/codeweald_highland_keep/highland_fortified_keep_b.glb"
)
# The gate faces -Z in the shipped asset; the band is the wall crossing and its
# approach, in metres along that outward bearing.
BEARING = 180.0
BAND = (7.9666, 19.0676)


def _part(name: str, centre_x: float, centre_z: float, width: float, depth: float, top: float = 40.0):
    """One part table entry in asset units, as `asset_parts` reports them."""
    return {
        "name": name,
        "min": [centre_x - width / 2, 0.0, centre_z - depth / 2],
        "max": [centre_x + width / 2, top, centre_z + depth / 2],
    }


def _open_gateway(half_width: float = 34.0):
    """Two piers flanking a clear corridor, at the declared band's depth."""
    return [
        _part("Gate_pier_left", -(half_width + 7.0), -104.0, 14.0, 40.0),
        _part("Gate_pier_right", half_width + 7.0, -104.0, 14.0, 40.0),
    ]


class ThresholdMeasurementTest(unittest.TestCase):
    def test_an_open_gateway_measures_its_clear_width(self):
        clear = measure_threshold_m(
            _open_gateway(), bearing_degrees=BEARING, band_m=BAND, scale=SCALE, max_climb_m=CLIMB_M
        )
        self.assertAlmostEqual(68.0 * SCALE, clear, places=4)

    def test_a_part_spanning_the_centreline_reads_as_sealed(self):
        """The D17 defect exactly: one solid cube centred on the gate."""
        parts = _open_gateway() + [_part("Gatehouse", 0.0, -104.0, 54.0, 38.0)]
        self.assertEqual(
            0.0,
            measure_threshold_m(parts, bearing_degrees=BEARING, band_m=BAND, scale=SCALE, max_climb_m=CLIMB_M),
        )

    def test_a_closed_leaf_reads_as_sealed(self):
        """And its smaller sibling: a shut door across the threshold."""
        parts = _open_gateway() + [_part("Gate_shadow", 0.0, -123.5, 19.0, 2.0)]
        self.assertEqual(
            0.0,
            measure_threshold_m(parts, bearing_degrees=BEARING, band_m=BAND, scale=SCALE, max_climb_m=CLIMB_M),
        )

    def test_a_low_slab_across_the_threshold_is_a_floor(self):
        """A drawbridge deck crosses the doorway and must not close it.

        This is the check that keeps the measurement agreeing with
        `rasterize_colliders`, which steps a body onto anything under its climb
        height. Without it, every asset with a doorstep would read as sealed.
        """
        parts = _open_gateway() + [_part("Drawbridge", 0.0, -133.0, 20.0, 30.0, top=1.0)]
        clear = measure_threshold_m(
            parts, bearing_degrees=BEARING, band_m=BAND, scale=SCALE, max_climb_m=CLIMB_M
        )
        self.assertAlmostEqual(68.0 * SCALE, clear, places=4)

    def test_a_raised_arch_still_reads_as_sealed(self):
        """Height is deliberately not a reprieve.

        `rasterize_colliders` subtracts a part's whole ground footprint
        regardless of its underside, so an arch blocks the ground beneath it. A
        measurement that forgave overhead geometry would certify gateways the
        game cannot actually route through.
        """
        arch = _part("Gate_arch", 0.0, -104.0, 68.0, 20.0, top=70.0)
        arch["min"][1] = 50.0  # springs well above head height, and still blocks
        parts = _open_gateway() + [arch]
        self.assertEqual(
            0.0,
            measure_threshold_m(parts, bearing_degrees=BEARING, band_m=BAND, scale=SCALE, max_climb_m=CLIMB_M),
        )

    def test_geometry_outside_the_band_does_not_narrow_the_threshold(self):
        """The far wall of an enclosed building is not its door being shut."""
        parts = _open_gateway() + [_part("Great_hall", 0.0, 0.0, 44.0, 40.0)]
        clear = measure_threshold_m(
            parts, bearing_degrees=BEARING, band_m=BAND, scale=SCALE, max_climb_m=CLIMB_M
        )
        self.assertAlmostEqual(68.0 * SCALE, clear, places=4)

    def test_a_band_naming_empty_space_is_refused(self):
        """Otherwise a mis-declared band passes by measuring nothing."""
        with self.assertRaises(AffordanceError):
            measure_threshold_m(
                _open_gateway(), bearing_degrees=BEARING, band_m=(400.0, 500.0),
                scale=SCALE, max_climb_m=CLIMB_M,
            )


class ContractTest(unittest.TestCase):
    def setUp(self):
        if not KEEP.is_file():
            self.skipTest("keep asset not generated")
        self.contract = load_contract(KEEP)
        self.assertIsNotNone(self.contract, "the shipped keep must carry a sidecar")

    def _verify(self, contract, **overrides):
        settings = {
            "role": "faction_fortification",
            "agent_radius_m": RADIUS_M,
            "max_climb_m": CLIMB_M,
            "placed_scale": SCALE,
        }
        settings.update(overrides)
        return verify(KEEP, contract, **settings)

    def test_the_shipped_keep_passes(self):
        self.assertEqual([], self._verify(self.contract))

    def test_a_missing_sidecar_fails(self):
        """An asset whose usability is unstated cannot be checked."""
        problems = self._verify(None)
        self.assertTrue(problems)
        self.assertIn("affordance sidecar", problems[0])

    def test_a_stale_claim_fails(self):
        """The un-regenerated-variant case: sidecar advertises an opening the
        mesh does not have. Overstate the claim and the mesh contradicts it."""
        stale = json.loads(json.dumps(self.contract))
        stale["enterable"]["threshold_clear_m"] = 40.0
        problems = self._verify(stale)
        self.assertTrue(any("disagree" in problem for problem in problems))

    def test_a_wider_agent_fails_on_unchanged_geometry(self):
        """Same mesh, same claim, different world -- and the answer changes.

        This is the check that makes the contract about a *placement* rather
        than about the asset in the abstract.
        """
        self.assertEqual([], self._verify(self.contract))
        problems = self._verify(self.contract, agent_radius_m=8.0)
        self.assertTrue(any("sealed to it" in problem for problem in problems))

    def test_a_scale_mismatch_fails(self):
        """A claim verified at one scale is not a claim at another."""
        problems = self._verify(self.contract, placed_scale=SCALE * 2)
        self.assertTrue(any("different world" in problem for problem in problems))

    def test_a_cramped_interior_fails(self):
        """Enterable but not stand-in-able is still a stranded keep."""
        cramped = json.loads(json.dumps(self.contract))
        cramped["enterable"]["interior_ring_m"] = 1.0
        problems = self._verify(cramped)
        self.assertTrue(any("stood in" in problem for problem in problems))

    def test_roles_without_declared_affordances_are_not_checked(self):
        """A tree has no threshold; demanding one would be noise."""
        self.assertNotIn("conifer_canopy", ROLE_REQUIRED_AFFORDANCES)
        self.assertEqual([], self._verify(None, role="conifer_canopy"))

    def test_unreadable_sidecar_is_reported_not_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            asset = Path(directory) / "thing.glb"
            asset.write_bytes(b"")
            asset_affordance.sidecar_path(asset).write_text("{not json", encoding="utf-8")
            with self.assertRaises(AffordanceError):
                load_contract(asset)


if __name__ == "__main__":
    unittest.main()
