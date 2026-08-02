"""Tests for the 0.2 authoring diagnoses in build_zone.py.

Each diagnosis exists to translate an opaque gate failure into the authored
zone_spec field responsible and the concrete repair, following the pattern
set by `_accessibility_diagnosis`. Every test here asserts BOTH directions:
a failing report produces a diagnosis naming the right field, and a passing
report produces no diagnosis at all -- a diagnosis that fires on healthy
input is worse than none, because it sends an author chasing a problem that
does not exist.
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

PIPELINE = Path(__file__).resolve().parents[1] / "pipeline"
sys.path.insert(0, str(PIPELINE))

from build_zone import (
    _hydrology_diagnosis,
    _terrain_contract_diagnosis,
    _traversal_diagnosis,
)


def _write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value), encoding="utf-8")


class HydrologyDiagnosisTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.report_path = Path(self._tmp.name) / "terrain_analysis.json"
        self.zone_spec_path = Path(self._tmp.name) / "zone_spec.json"

    def _write(self, hydrology: list[dict], features: list[dict]):
        _write_json(
            self.report_path,
            {
                "policy": {
                    "maximum_hydrology_uphill_fraction": 0.08,
                    "maximum_hydrology_uphill_step_m": 0.35,
                },
                "hydrology": hydrology,
            },
        )
        _write_json(self.zone_spec_path, {"features": features})

    def test_passing_report_produces_no_diagnosis(self):
        self._write(
            hydrology=[
                {
                    "id": "river_a",
                    "channel_profile": "incised_stream",
                    "uphill_step_fraction": 0.0,
                    "maximum_uphill_step_m": 0.0,
                }
            ],
            features=[],
        )
        self.assertEqual(_hydrology_diagnosis(self.report_path, self.zone_spec_path), "")

    def test_surface_channel_failure_names_the_profile_repair(self):
        self._write(
            hydrology=[
                {
                    "id": "stream_a",
                    "channel_profile": "surface_channel",
                    "uphill_step_fraction": 0.20,
                    "maximum_uphill_step_m": 0.5,
                }
            ],
            features=[],
        )
        diagnosis = _hydrology_diagnosis(self.report_path, self.zone_spec_path)
        self.assertIn("stream_a", diagnosis)
        self.assertIn("channel_profile", diagnosis)
        self.assertIn("incised_stream", diagnosis)

    def test_incised_stream_failure_points_at_geometry_not_profile(self):
        self._write(
            hydrology=[
                {
                    "id": "river_b",
                    "channel_profile": "incised_stream",
                    "uphill_step_fraction": 0.20,
                    "maximum_uphill_step_m": 0.5,
                }
            ],
            features=[
                {
                    "id": "river_b",
                    "category": "hydrology",
                    "geometry": {"points": [[0, 0], [10, 10], [20, 5]]},
                }
            ],
        )
        diagnosis = _hydrology_diagnosis(self.report_path, self.zone_spec_path)
        self.assertIn("river_b", diagnosis)
        self.assertIn("3 points", diagnosis)
        self.assertIn("waypoints", diagnosis)

    def test_wetland_rill_is_exempt_even_when_thresholds_are_exceeded(self):
        self._write(
            hydrology=[
                {
                    "id": "marsh_a",
                    "channel_profile": "wetland_rill",
                    "uphill_step_fraction": 0.99,
                    "maximum_uphill_step_m": 5.0,
                }
            ],
            features=[],
        )
        self.assertEqual(_hydrology_diagnosis(self.report_path, self.zone_spec_path), "")

    def test_unreadable_report_returns_empty_string(self):
        missing = Path(self._tmp.name) / "missing.json"
        self.assertEqual(_hydrology_diagnosis(missing, self.zone_spec_path), "")


class TraversalDiagnosisTests(unittest.TestCase):
    def _policy(self, **overrides):
        base = {
            "maximum_lane_grade": 0.72,
            "maximum_lane_p95_grade": 0.32,
            "maximum_lane_cross_grade": 0.40,
            "maximum_keep_lane_distance_m": 225.0,
            "maximum_objective_lane_distance_m": 280.0,
        }
        base.update(overrides)
        return base

    def test_passing_report_produces_no_diagnosis(self):
        report = {"failures": [], "policy": self._policy(), "lanes": {}, "keeps": {}, "objectives": {}}
        self.assertEqual(_traversal_diagnosis(report, {"features": []}), "")

    def test_lane_width_failure_names_authored_and_required_width(self):
        report = {
            "failures": ["Lane north is narrower than the traversal agent contract"],
            "policy": self._policy(),
            "lanes": {
                "north": {
                    "feature_id": "lane_north",
                    "authored_width_m": 3.0,
                    "actor_clearance": {
                        "large": {"minimum_required_width_m": 9.5},
                        "standard": {"minimum_required_width_m": 7.0},
                    },
                }
            },
            "keeps": {},
            "objectives": {},
        }
        diagnosis = _traversal_diagnosis(report, {"features": []})
        self.assertIn("Lane north", diagnosis)
        self.assertIn("minimum_width_m", diagnosis)
        self.assertIn("9.5", diagnosis)  # the largest actor's requirement, not just any one

    def test_lane_grade_failure_names_policy_key_and_route_repair(self):
        report = {
            "failures": ["Lane south exceeds maximum longitudinal grade"],
            "policy": self._policy(),
            "lanes": {"south": {"feature_id": "lane_south", "maximum_longitudinal_grade": 0.9}},
            "keeps": {},
            "objectives": {},
        }
        diagnosis = _traversal_diagnosis(report, {"features": []})
        self.assertIn("Lane south", diagnosis)
        self.assertIn("traversal_policy.maximum_lane_grade", diagnosis)
        self.assertIn("0.9", diagnosis)

    def test_lane_opposing_keeps_failure_names_endpoint_assignments(self):
        report = {
            "failures": ["Lane east endpoints do not connect opposing keeps"],
            "policy": self._policy(),
            "lanes": {
                "east": {
                    "feature_id": "lane_east",
                    "endpoint_assignments": [
                        {"keep_id": "keep_a", "distance_m": 10.0},
                        {"keep_id": "keep_a", "distance_m": 12.0},
                    ],
                }
            },
            "keeps": {},
            "objectives": {},
        }
        diagnosis = _traversal_diagnosis(report, {"features": []})
        self.assertIn("Lane east", diagnosis)
        self.assertIn("keep_a", diagnosis)

    def test_lane_stream_crossing_failure_names_both_ids_and_bridge_repair(self):
        report = {
            "failures": ["Lane west crosses river_a without an authored traversal link"],
            "policy": self._policy(),
            "lanes": {},
            "keeps": {},
            "objectives": {},
        }
        diagnosis = _traversal_diagnosis(report, {"features": []})
        self.assertIn("west", diagnosis)
        self.assertIn("river_a", diagnosis)
        self.assertIn("bridge", diagnosis)
        self.assertIn("derived_from", diagnosis)

    def test_keep_slope_failure_names_grade_and_policy_key(self):
        report = {
            "failures": ["Keep keep_a spawn anchor exceeds local slope limit"],
            "policy": self._policy(),
            "lanes": {},
            "keeps": {"keep_a": {"maximum_local_grade": 0.55}},
            "objectives": {},
        }
        diagnosis = _traversal_diagnosis(report, {"features": []})
        self.assertIn("Keep keep_a", diagnosis)
        self.assertIn("traversal_policy.maximum_lane_cross_grade", diagnosis)

    def test_objective_disconnected_failure_names_policy_key(self):
        report = {
            "failures": ["Objective ruin_a is disconnected from the lane network"],
            "policy": self._policy(),
            "lanes": {},
            "keeps": {},
            "objectives": {"ruin_a": {"nearest_lane_distance_m": 500.0}},
        }
        diagnosis = _traversal_diagnosis(report, {"features": []})
        self.assertIn("Objective ruin_a", diagnosis)
        self.assertIn("maximum_objective_lane_distance_m", diagnosis)

    def test_blocking_lane_failure_names_redundancy_repair(self):
        report = {
            "failures": ["Blocking lane north leaves no opposing-keep replan route"],
            "policy": self._policy(),
            "lanes": {},
            "keeps": {},
            "objectives": {},
        }
        diagnosis = _traversal_diagnosis(report, {"features": []})
        self.assertIn("north", diagnosis)
        self.assertIn("additional lane", diagnosis)

    def test_unrecognised_failure_is_passed_through_not_dropped(self):
        report = {
            "failures": ["Lane north does something this diagnosis has never heard of"],
            "policy": self._policy(),
            "lanes": {},
            "keeps": {},
            "objectives": {},
        }
        diagnosis = _traversal_diagnosis(report, {"features": []})
        self.assertIn("Unexplained", diagnosis)
        self.assertIn("does something this diagnosis has never heard of", diagnosis)


class TerrainContractDiagnosisTests(unittest.TestCase):
    def test_hash_mismatch_triages_as_stale_artifact_not_authoring(self):
        diagnosis = _terrain_contract_diagnosis(
            "Command failed: invalid ZoneSpec: terrain manifest.zone_spec_sha256 "
            "does not match the supplied artifact"
        )
        self.assertIn("not an authoring mistake", diagnosis)
        self.assertIn("rebuild", diagnosis.lower())

    def test_zone_id_mismatch_triages_as_stale_batch(self):
        diagnosis = _terrain_contract_diagnosis(
            "Command failed: invalid ZoneSpec: terrain manifest.zone_id does not "
            "match the ZoneSpec"
        )
        self.assertIn("zone_id", diagnosis)
        self.assertIn("not an authoring mistake", diagnosis)

    def test_byte_size_mismatch_triages_as_mismatched_build(self):
        diagnosis = _terrain_contract_diagnosis(
            "Command failed: invalid ZoneSpec: heightfield has 100 bytes; expected 400"
        )
        self.assertIn("wrong size", diagnosis)

    def test_schema_version_mismatch_triages_as_pipeline_version_skew(self):
        diagnosis = _terrain_contract_diagnosis(
            'Command failed: invalid ZoneSpec: terrain analysis.schema_version '
            'must be "codeweald.terrain-analysis/v1", got "codeweald.terrain-analysis/v0"'
        )
        self.assertIn("version mismatch", diagnosis)

    def test_unrecognised_message_gets_generic_not_authoring_fallback(self):
        diagnosis = _terrain_contract_diagnosis("Command failed: something entirely new")
        self.assertIn("not an authoring gate", diagnosis)

    def test_every_triage_category_says_not_authoring(self):
        # The whole point of this gate's diagnosis is to stop an author from
        # chasing a fake DSL knob for a provenance bug. Every branch, known
        # or unknown, must say so -- this is the negative case: if any
        # category silently omitted the disclaimer, this test catches it.
        messages = [
            "x does not match the supplied artifact",
            "x does not match the ZoneSpec",
            "heightfield has 1 bytes; expected 2",
            "resolution does not match the terrain manifest",
            'schema_version must be "a", got "b"',
            "totally unrecognised failure text",
        ]
        for message in messages:
            with self.subTest(message=message):
                diagnosis = _terrain_contract_diagnosis(message)
                self.assertIn("not an authoring", diagnosis.lower())


if __name__ == "__main__":
    unittest.main()
