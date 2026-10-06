"""Adversarial tests for the Luxel MVP orchestration seam.

The test exercises the real Rust ledger through the Python compatibility
entrypoint. The candidate must validate semantically, while target-runtime
receipts remain indeterminate until Unity MCP supplies them; therefore a
candidate cannot be accidentally certified offline.
"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from pipeline import luxel_mvp


class LuxelMvpOrchestrationTests(unittest.TestCase):
    def test_fixture_candidate_validates_but_cannot_claim_unity(self) -> None:
        with tempfile.TemporaryDirectory(prefix="luxel-mvp-test-") as temporary:
            output = Path(temporary)
            paths = luxel_mvp.prepare(
                output,
                concept=luxel_mvp.DEFAULT_CONCEPT,
                asset=luxel_mvp.DEFAULT_ASSET,
            )
            validation = json.loads(luxel_mvp.run_checked(["validate-spec", str(paths["spec"])]))
            self.assertEqual(validation["status"], "valid")
            self.assertEqual(validation["artifact_count"], 10)
            asset_receipt = json.loads(
                (output / "artifacts" / "asset_preparation_receipt.json").read_text(
                    encoding="utf-8"
                )
            )
            self.assertEqual(asset_receipt["schema_version"], "luxel.asset-runtime-receipt/v1")
            self.assertEqual(asset_receipt["status"], "rejected")
            self.assertTrue(
                any(finding["code"] == "invalid_skin" for finding in asset_receipt["findings"])
            )
            gameplay_receipt = json.loads(
                (output / "artifacts" / "gameplay_receipt.json").read_text(encoding="utf-8")
            )
            self.assertEqual(gameplay_receipt["body"]["outcome"], "won")
            self.assertEqual(gameplay_receipt["body"]["event_count"], 9)
            evidence = json.loads(paths["evidence"].read_text(encoding="utf-8"))
            gameplay_gate = next(
                receipt
                for receipt in evidence["receipts"]
                if receipt["gate_id"] == "gameplay.complete_loop"
            )
            self.assertEqual(gameplay_gate["artifact_id"], "gameplay-receipt")
            with self.assertRaisesRegex(RuntimeError, "required gate"):
                luxel_mvp.run_checked(
                    [
                        "commit",
                        str(paths["spec"]),
                        "--evidence",
                        str(paths["evidence"]),
                        "--output",
                        str(output / "project_snapshot.json"),
                    ]
                )
            self.assertEqual(
                2,
                luxel_mvp.main(
                    [
                        "verify-all",
                        "--spec",
                        str(paths["spec"]),
                        "--evidence",
                        str(paths["evidence"]),
                        "--snapshot-output",
                        str(output / "project_snapshot.json"),
                        "--unity-output",
                        str(output / "luxel_unity_mvp_import.json"),
                    ]
                ),
            )

    def test_candidate_contains_no_absolute_artifact_paths(self) -> None:
        with tempfile.TemporaryDirectory(prefix="luxel-mvp-test-") as temporary:
            paths = luxel_mvp.prepare(
                Path(temporary),
                concept=luxel_mvp.DEFAULT_CONCEPT,
                asset=luxel_mvp.DEFAULT_ASSET,
            )
            spec = json.loads(paths["spec"].read_text(encoding="utf-8"))
            for node in spec["artifact_graph"]:
                self.assertFalse(Path(node["artifact"]["path"]).is_absolute())

    def test_tampered_artifact_fails_then_repair_rebuilds_candidate(self) -> None:
        with tempfile.TemporaryDirectory(prefix="luxel-mvp-test-") as temporary:
            output = Path(temporary) / "candidate"
            paths = luxel_mvp.prepare(
                output,
                concept=luxel_mvp.DEFAULT_CONCEPT,
                asset=luxel_mvp.DEFAULT_ASSET,
            )
            navigation = output / "artifacts" / "navigation.json"
            navigation.write_text(navigation.read_text(encoding="utf-8") + "\n", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "digest mismatch"):
                luxel_mvp._verify_bundle_files(paths["spec"])
            repaired = Path(temporary) / "repaired"
            self.assertEqual(
                0,
                luxel_mvp.main(["repair", "--input", str(output), "--output", str(repaired)]),
            )
            self.assertEqual(
                "valid",
                json.loads(luxel_mvp.run_checked(["validate-spec", str(repaired / "project_spec.json")]))["status"],
            )
            repair_report = json.loads((repaired / "repair_report.json").read_text(encoding="utf-8"))
            self.assertTrue(repair_report["observed_failures"])

    def test_disconnected_navigation_is_rejected_by_rust_world_contract(self) -> None:
        with tempfile.TemporaryDirectory(prefix="luxel-mvp-world-") as temporary:
            output = Path(temporary)
            paths = luxel_mvp.prepare(
                output,
                concept=luxel_mvp.DEFAULT_CONCEPT,
                asset=luxel_mvp.DEFAULT_ASSET,
            )
            navigation_path = output / "artifacts" / "navigation.json"
            navigation = json.loads(navigation_path.read_text(encoding="utf-8"))
            navigation["route"][-1]["neighbors"] = []
            navigation_path.write_text(
                json.dumps(navigation, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            spec = json.loads(paths["spec"].read_text(encoding="utf-8"))
            navigation_digest = luxel_mvp.sha256(navigation_path)
            for node in spec["artifact_graph"]:
                if node["artifact"]["artifact_id"] == "navigation":
                    node["artifact"]["sha256"] = navigation_digest
                for dependency in node["dependencies"]:
                    if dependency["artifact_id"] == "navigation":
                        dependency["sha256"] = navigation_digest
            spec["world"]["navigation"]["sha256"] = navigation_digest
            paths["spec"].write_text(
                json.dumps(spec, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RuntimeError, "reverse edge"):
                luxel_mvp.run_checked(["validate-spec", str(paths["spec"])])


if __name__ == "__main__":
    unittest.main()
