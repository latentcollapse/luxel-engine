"""Tests for the siting solver (systems roadmap S10).

The defect: settlements scattered by a solver that knows terrain slope and a
keep-out radius around other landmarks, and nothing about lanes. Three villages
sit across the roads they exist to serve and no lane runs end to end (D13).

That is a scoring problem, not a tuning problem -- the score never contained a
term for "do not block the route". These tests assert the term exists and that
the two roles genuinely disagree, because a guardian and a village wanting the
same ground is the fusion S14 had to undo.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from siting import (  # noqa: E402
    LANE_KEEP_OUT_M,
    audit_existing,
    guardian_score,
    settlement_score,
)

SIDE = 41


def _uniform(value):
    return np.full((SIDE, SIDE), float(value))


def _lane_distance(profile):
    return np.tile(np.asarray(profile, dtype=float)[:, None], (1, SIDE))


def _settlement(**overrides):
    base = {
        "slope_degrees": _uniform(3.0),
        "wetness": _uniform(5.0),
        "exposure": _uniform(0.3),
        "lane_distance": _uniform(30.0),
    }
    base.update(overrides)
    return settlement_score(**base)


class SettlementTest(unittest.TestCase):
    def test_good_ground_near_a_road_scores_well(self):
        # Comparative, not absolute: what matters is that suitable ground beats
        # every disqualifying condition, not that it clears a number somebody
        # picked. An absolute floor here would just encode today's weights.
        good = float(_settlement().max())
        self.assertGreater(good, 0.2)
        self.assertGreater(good, float(_settlement(slope_degrees=_uniform(40.0)).max()))
        self.assertGreater(good, float(_settlement(wetness=_uniform(13.0)).max()))
        self.assertGreater(good, float(_settlement(lane_distance=_uniform(2.0)).max()))

    def test_ground_on_the_lane_scores_zero(self):
        """The term D13 says is missing. A village on the carriageway is not a
        slightly worse village; it is a disqualified one."""
        on_lane = _settlement(lane_distance=_uniform(2.0))
        self.assertEqual(0.0, float(on_lane.max()))

    def test_the_lane_term_is_two_sided(self):
        """*Near* a road is good, *on* it is fatal, *far* from it is pointless.

        A one-sided 'close to the road' term is effectively what the current
        scatter has, and it puts villages in the middle of the carriageway.
        """
        distances = np.linspace(0.0, 120.0, SIDE)
        scores = _settlement(lane_distance=_lane_distance(distances))[:, 0]
        best = int(np.argmax(scores))
        self.assertGreater(distances[best], LANE_KEEP_OUT_M)
        self.assertLess(distances[best], 70.0)
        self.assertLess(scores[0], scores[best])
        self.assertLess(scores[-1], scores[best])

    def test_a_cliff_is_not_a_village_site(self):
        self.assertEqual(0.0, float(_settlement(slope_degrees=_uniform(40.0)).max()))

    def test_a_bog_is_not_a_village_site(self):
        self.assertLess(float(_settlement(wetness=_uniform(13.0)).max()), 0.05)


class GuardianTest(unittest.TestCase):
    def test_a_guardian_wants_the_lane_the_village_must_avoid(self):
        """The two roles are opposed, which is why they could not be one asset.

        Same ground, same conditions: the settlement score must reject what the
        guardian score prefers.
        """
        on_lane = {"slope_degrees": _uniform(3.0), "exposure": _uniform(0.5),
                   "lane_distance": _uniform(2.0)}
        guardian = guardian_score(**on_lane)
        village = _settlement(lane_distance=_uniform(2.0))
        self.assertGreater(float(guardian.max()), 0.3)
        self.assertEqual(0.0, float(village.max()))

    def test_a_guardian_far_from_any_lane_is_worthless(self):
        far = guardian_score(
            slope_degrees=_uniform(3.0), exposure=_uniform(0.5),
            lane_distance=_uniform(60.0),
        )
        self.assertEqual(0.0, float(far.max()))

    def test_a_guardian_prefers_commanding_ground(self):
        low = guardian_score(slope_degrees=_uniform(3.0), exposure=_uniform(0.05),
                             lane_distance=_uniform(2.0))
        high = guardian_score(slope_degrees=_uniform(3.0), exposure=_uniform(0.85),
                              lane_distance=_uniform(2.0))
        self.assertGreater(float(high.max()), float(low.max()))


class AuditTest(unittest.TestCase):
    def _lane(self):
        return {
            "id": "mid",
            "category": "corridor",
            "geometry": {"points": [[-100.0, 0.0], [100.0, 0.0]]},
            "properties": {"minimum_width_m": 10.0},
        }

    def _village(self, z, radius=16.0):
        return {
            "id": "hamlet",
            "category": "landmark",
            "geometry": {"points": [[0.0, z]]},
            "properties": {"scatter_exclusion_radius_m": radius},
        }

    def test_a_village_on_the_lane_is_named_with_its_overlap(self):
        findings = audit_existing([self._village(0.0)], [self._lane()])
        self.assertEqual(1, len(findings))
        self.assertEqual("hamlet", findings[0]["feature_id"])
        self.assertEqual("mid", findings[0]["lane_id"])
        self.assertAlmostEqual(21.0, findings[0]["overlap_m"], places=2)
        self.assertIn("move hamlet", findings[0]["repair"])

    def test_a_village_clear_of_the_lane_is_not_reported(self):
        """The negative case: an audit that flags everything says nothing."""
        self.assertEqual([], audit_existing([self._village(60.0)], [self._lane()]))

    def test_findings_are_ordered_worst_first(self):
        near = dict(self._village(2.0)); near["id"] = "near"
        edge = dict(self._village(20.0)); edge["id"] = "edge"
        findings = audit_existing([edge, near], [self._lane()])
        self.assertEqual(["near", "edge"], [f["feature_id"] for f in findings])


if __name__ == "__main__":
    unittest.main()
