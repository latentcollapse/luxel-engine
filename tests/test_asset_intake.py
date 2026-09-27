"""Regression tests for the supplied model asset-intake benchmark."""

from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from asset_intake import (  # noqa: E402
    AssetIntakeError,
    canonical_sha256,
    inspect_asset,
)


BENCHMARK_ASSET = Path(
    os.environ.get(
        "WGE_ASSET_BENCHMARK",
        "/home/mattc/Pictures/Generated 2D Images/"
        "sample_2026-09-26T091412.074.glb",
    )
)
EXPECTED_INPUT_SHA256 = "858fa104880822d081405579fb5b39d533d3b3b341d38aa1490a44b634f5e2b4"
EXPECTED_REPORT_SHA256 = "14301dc1dc20272b7d35e270a3d0221a310b3376470c0cc025f97f8d06638f71"


@unittest.skipUnless(BENCHMARK_ASSET.is_file(), "the external GLB benchmark is unavailable")
class SuppliedAssetBenchmarkTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.report = inspect_asset(BENCHMARK_ASSET)

    def test_report_is_deterministic_and_path_independent(self):
        second = inspect_asset(BENCHMARK_ASSET)
        self.assertEqual(self.report, second)
        self.assertEqual(self.report["input"]["sha256"], EXPECTED_INPUT_SHA256)
        body = {
            key: value
            for key, value in self.report.items()
            if key != "canonical_report_sha256"
        }
        self.assertEqual(
            self.report["canonical_report_sha256"],
            canonical_sha256(body),
        )
        self.assertEqual(self.report["canonical_report_sha256"], EXPECTED_REPORT_SHA256)
        self.assertNotIn(str(BENCHMARK_ASSET), json.dumps(self.report, sort_keys=True))

    def test_report_records_the_actual_structural_contract(self):
        report = self.report
        self.assertEqual(report["input"]["byte_length"], 13980532)
        self.assertEqual(report["input"]["asset"]["generator"], "https://github.com/mikedh/trimesh")
        self.assertEqual(report["scene"]["scene_count"], 1)
        self.assertEqual(report["scene"]["node_count"], 2)
        self.assertEqual(report["scene"]["mesh_count"], 1)
        self.assertEqual(report["scene"]["skin_count"], 0)
        self.assertEqual(report["scene"]["animation_count"], 0)
        self.assertEqual(report["geometry"]["primitive_count"], 1)
        self.assertEqual(report["geometry"]["total_vertex_count"], 252602)
        self.assertEqual(report["geometry"]["total_triangle_count"], 293477)
        self.assertEqual(
            report["geometry"]["attributes_present"],
            ["NORMAL", "POSITION", "TEXCOORD_0"],
        )
        primitive = report["geometry"]["meshes"][0]["primitives"][0]
        topology = primitive["topology"]
        self.assertEqual(topology["connected_component_count"], 15941)
        self.assertEqual(topology["boundary_edge_count"], 181508)
        self.assertEqual(topology["non_manifold_edge_count"], 1)
        self.assertEqual(topology["zero_area_triangles"], 6)

    def test_report_makes_rigging_gaps_explicit(self):
        assessment = self.report["rigging_assessment"]
        self.assertEqual(assessment["status"], "unrigged_static_mesh")
        observed = assessment["observed"]
        self.assertFalse(observed["has_joint_attributes"])
        self.assertFalse(observed["has_weight_attributes"])
        self.assertFalse(observed["has_tangent_attributes"])
        repairs = {entry["id"] for entry in assessment["pre_rigging_repairs"]}
        self.assertTrue({"GEO-001", "GEO-002", "GEO-003", "GEO-004", "MAT-001"} <= repairs)
        required = {entry["id"] for entry in assessment["required_repairs_or_construction"]}
        self.assertTrue({"RIG-001", "RIG-002", "RIG-003", "RIG-005", "RIG-008"} <= required)
        unsafe = self.report["inference_boundary"]["not_safe_to_infer"]
        self.assertTrue(any("semantic identity" in claim for claim in unsafe))
        self.assertTrue(any("Real-world scale" in claim for claim in unsafe))


class ContainerValidationTest(unittest.TestCase):
    def test_truncated_input_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "broken.glb"
            path.write_bytes(b"glTF")
            with self.assertRaises(AssetIntakeError):
                inspect_asset(path)


if __name__ == "__main__":
    unittest.main()
