"""End-to-end tests for the lane-overlap semantic transaction kernel."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "pipeline"))

from wge_semantic_ir import LowerError, canonical_dumps, file_digest, lower_file, lower_source  # noqa: E402

REGISTRY = ROOT / "world_core" / "crates" / "semantic_kernel" / "registry_v0.json"
WORKER = ROOT / "terrain_lab" / "bin" / "lane_overlap_worker.jl"
PROJECT = ROOT / "terrain_lab"
MANIFEST = PROJECT / "Manifest.toml"
FIXTURES = ROOT / "tests" / "fixtures" / "semantic_kernel"
BINARY = ROOT / "world_core" / "target" / "debug" / "wge-semantic-kernel"
INVALID = FIXTURES / "invalid.wge"
REPAIRED = FIXTURES / "repaired.wge"
TAMPERED = FIXTURES / "tampered.wge"


def events(text: str) -> list[dict]:
    rows = []
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("{"):
            rows.append(json.loads(line))
    return rows


def run_kernel(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(BINARY), *args],
        text=True,
        capture_output=True,
        check=False,
    )


class SemanticKernelTests(unittest.TestCase):
    store_a: Path
    store_b: Path
    transcript_a: str
    transcript_b: str
    events_a: list[dict]
    events_b: list[dict]

    @classmethod
    def setUpClass(cls) -> None:
        subprocess.run(
            ["cargo", "build", "-p", "wge-semantic-kernel", "--quiet"],
            cwd=ROOT / "world_core",
            check=True,
        )
        cls.store_a = Path(tempfile.mkdtemp(prefix="wge-kernel-a-"))
        cls.store_b = Path(tempfile.mkdtemp(prefix="wge-kernel-b-"))
        cls.transcript_a = cls._run_demo(cls.store_a)
        cls.transcript_b = cls._run_demo(cls.store_b)
        cls.events_a = events(cls.transcript_a)
        cls.events_b = events(cls.transcript_b)

    @classmethod
    def tearDownClass(cls) -> None:
        shutil.rmtree(cls.store_a, ignore_errors=True)
        shutil.rmtree(cls.store_b, ignore_errors=True)

    @classmethod
    def _write_ir(cls, source: Path, directory: Path, name: str) -> Path:
        document = lower_file(source, REGISTRY)
        path = directory / name
        path.write_text(canonical_dumps(document) + "\n", encoding="utf-8")
        return path

    @classmethod
    def _run_demo(cls, store: Path) -> str:
        completed = run_kernel(
            [
                "run",
                "--store",
                str(store),
                "--invalid-source",
                str(INVALID),
                "--invalid-ir",
                str(cls._write_ir(INVALID, store, "invalid.ir.json")),
                "--repaired-source",
                str(REPAIRED),
                "--repaired-ir",
                str(cls._write_ir(REPAIRED, store, "repaired.ir.json")),
                "--tampered-source",
                str(TAMPERED),
                "--tampered-ir",
                str(cls._write_ir(TAMPERED, store, "tampered.ir.json")),
                "--registry",
                str(REGISTRY),
                "--worker",
                str(WORKER),
                "--project",
                str(PROJECT),
                "--manifest",
                str(MANIFEST),
            ]
        )
        if completed.returncode != 0:
            raise AssertionError(completed.stdout + completed.stderr)
        return completed.stdout

    def _one(self, name: str, rows: list[dict] | None = None) -> dict:
        found = [row for row in (rows or self.events_a) if row.get("event") == name]
        self.assertTrue(found, name)
        return found[-1] if name == "current" and len(found) > 1 else found[0]

    def test_a_invalid_placement_fails_closed(self) -> None:
        measurement = self._one("measurement")
        predicate = self._one("predicate")
        self.assertTrue(measurement["intersects"])
        self.assertGreater(measurement["overlap_area"], 0)
        self.assertEqual(predicate["result"], "fail")
        self.assertEqual(predicate["owner"], "rust-kernel")

    def test_b_failure_is_semantic_repair(self) -> None:
        repair = self._one("repair")
        self.assertEqual(repair["class"], "SemanticRepair")
        self.assertNotIn("traceback", json.dumps(repair).lower())
        self.assertEqual(repair["gate_id"], "lane.footprint_clear")

    def test_c_repair_points_at_the_placement_span(self) -> None:
        repair = self._one("repair")
        self.assertEqual(repair["node_id"], "blocked_keep")
        self.assertEqual(repair["repair_class"], "move_placement_off_lane")
        self.assertEqual(repair["source_span"]["line"], 5)
        self.assertIn("measurement_sha256", repair)
        document = json.loads(
            (self.store_a / "repairs" / f"{repair['failure_id']}.json").read_text(encoding="utf-8")
        )
        self.assertEqual(document["rerun_predicate"], "lane.footprint_clear")
        self.assertEqual(document["editable_spans"], [repair["source_span"]])
        self.assertNotIn("eval", document["explanation"])

    def test_d_in_span_repair_commits(self) -> None:
        commit = self._one("commit")
        current = [row for row in self.events_a if row.get("event") == "current"][-1]
        self.assertEqual(current["generation_id"], commit["generation_id"])
        self.assertNotEqual(commit["generation_id"], "G0")
        self.assertTrue((self.store_a / "generations" / f"{commit['generation_id']}.json").is_file())

    def test_e_out_of_span_edit_is_rejected(self) -> None:
        rejection = self._one("span_rejection")
        self.assertEqual(rejection["class"], "ProvenanceFailure")
        self.assertEqual(rejection["code"], "unauthorized_span")

    def test_f_failed_candidate_does_not_move_the_pointer(self) -> None:
        checkpoint = json.loads(
            (self.store_a / "history" / "after_c1_pointer.json").read_text(encoding="utf-8")
        )
        self.assertEqual(checkpoint["generation_id"], "G0")
        currents = [row["generation_id"] for row in self.events_a if row.get("event") == "current"]
        self.assertEqual(currents[0], "G0")
        self.assertEqual(currents[1], "G0")

    def test_g_passing_candidate_moves_the_pointer(self) -> None:
        final = json.loads((self.store_a / "pointer.json").read_text(encoding="utf-8"))
        commit = self._one("commit")
        self.assertEqual(final["generation_id"], commit["generation_id"])
        self.assertNotEqual(final["generation_id"], "G0")

    def test_h_worker_cannot_mint_a_trusted_receipt(self) -> None:
        forged = self.store_a / "receipts" / "forged.json"
        forged.write_text(
            json.dumps(
                {
                    "minted_by": "julia-worker",
                    "operation": "generation.commit",
                    "receipt_id": "forged",
                    "schema": "wge.receipt/v0",
                }
            ),
            encoding="utf-8",
        )
        rejected = run_kernel(["accept-receipt", "--store", str(self.store_a), "--receipt-id", "forged"])
        self.assertNotEqual(rejected.returncode, 0)
        self.assertEqual(events(rejected.stdout)[0]["code"], "forged_receipt")
        verified = run_kernel(["verify-current", "--store", str(self.store_a)])
        self.assertEqual(verified.returncode, 0, verified.stdout)
        self.assertEqual(json.loads(verified.stdout)["minted_by"], "rust-kernel")

    def test_i_kernel_recomputes_the_measurement_hash(self) -> None:
        passed = [row for row in self.events_a if row.get("event") == "measurement"][-1]
        name = passed["sha256"].split(":", 1)[1]
        raw = (self.store_a / "evidence" / f"{name}.bin").read_bytes()
        self.assertEqual("sha256:" + hashlib.sha256(raw).hexdigest(), passed["sha256"])
        self.assertNotIn(b"gate_passed", raw)
        self.assertNotIn(b"receipt", raw)

    def test_j_stale_evidence_cannot_certify_the_second_candidate(self) -> None:
        measurements = [row for row in self.events_a if row.get("event") == "measurement"]
        candidates = [row for row in self.events_a if row.get("event") == "candidate"]
        stale = run_kernel(
            [
                "bind-evidence",
                "--store",
                str(self.store_a),
                "--candidate",
                candidates[1]["candidate_id"],
                "--evidence-sha",
                measurements[0]["sha256"],
            ]
        )
        self.assertNotEqual(stale.returncode, 0)
        self.assertEqual(events(stale.stdout)[0]["code"], "stale_evidence")

    def test_k_registry_mismatch_fails_before_commit(self) -> None:
        document = json.loads((self.store_a / "repaired.ir.json").read_text(encoding="utf-8"))
        document["registry_digest"] = "sha256:" + "ab" * 32
        mismatched = self.store_a / "mismatched.ir.json"
        mismatched.write_text(canonical_dumps(document) + "\n", encoding="utf-8")
        checked = run_kernel(
            [
                "check-ir",
                "--registry",
                str(REGISTRY),
                "--source",
                str(REPAIRED),
                "--ir",
                str(mismatched),
            ]
        )
        self.assertNotEqual(checked.returncode, 0)
        self.assertEqual(events(checked.stdout)[0]["code"], "registry_mismatch")
        self.assertEqual(events(checked.stdout)[0]["class"], "ProvenanceFailure")

    def test_l_solver_identity_mismatch_fails(self) -> None:
        claimed = "sha256:" + "00" * 32
        checked = run_kernel(
            ["check-solver", "--store", str(self.store_a), "--solver-image", claimed]
        )
        self.assertNotEqual(checked.returncode, 0)
        self.assertEqual(events(checked.stdout)[0]["code"], "solver_image_mismatch")
        fresh = Path(tempfile.mkdtemp(prefix="wge-kernel-solver-"))
        try:
            self._write_ir(INVALID, fresh, "invalid.ir.json")
            self._write_ir(REPAIRED, fresh, "repaired.ir.json")
            self._write_ir(TAMPERED, fresh, "tampered.ir.json")
            blocked = run_kernel(
                [
                    "run",
                    "--store",
                    str(fresh),
                    "--invalid-source",
                    str(INVALID),
                    "--invalid-ir",
                    str(fresh / "invalid.ir.json"),
                    "--repaired-source",
                    str(REPAIRED),
                    "--repaired-ir",
                    str(fresh / "repaired.ir.json"),
                    "--tampered-source",
                    str(TAMPERED),
                    "--tampered-ir",
                    str(fresh / "tampered.ir.json"),
                    "--registry",
                    str(REGISTRY),
                    "--worker",
                    str(WORKER),
                    "--project",
                    str(PROJECT),
                    "--manifest",
                    str(MANIFEST),
                    "--solver-image",
                    claimed,
                ]
            )
            self.assertNotEqual(blocked.returncode, 0)
            self.assertEqual(events(blocked.stdout)[0]["code"], "solver_image_mismatch")
            self.assertFalse((fresh / "pointer.json").exists())
        finally:
            shutil.rmtree(fresh, ignore_errors=True)

    def test_m_equivalent_runs_share_canonical_hashes(self) -> None:
        first = self._one("canonical_result", self.events_a)
        second = self._one("canonical_result", self.events_b)
        self.assertEqual(first["canonical_measurement_sha256"], second["canonical_measurement_sha256"])
        self.assertEqual(first["canonical_ir_sha256"], second["canonical_ir_sha256"])
        self.assertEqual(first["failing_measurement_sha256"], second["failing_measurement_sha256"])
        self.assertEqual(self._one("commit")["generation_id"], self._one("commit", self.events_b)["generation_id"])

    def test_n_both_jobs_use_one_warm_worker(self) -> None:
        ready = [row for row in self.events_a if row.get("event") == "worker_ready"]
        solvers = [row for row in self.events_a if row.get("event") == "solver"]
        self.assertEqual(len(ready), 1)
        self.assertEqual(len(solvers), 2)
        self.assertEqual(solvers[0]["pid"], solvers[1]["pid"])
        self.assertEqual(solvers[0]["pid"], ready[0]["pid"])
        final = [row for row in self.events_a if row.get("event") == "current"][-1]
        self.assertEqual(final["julia_jobs"], 2)

    def test_o_author_surface_rejects_arbitrary_code(self) -> None:
        marker = Path(tempfile.mkdtemp(prefix="wge-kernel-exec-")) / "marker"
        source = f"import os\nos.system('touch {marker}')\n"
        with self.assertRaises(LowerError) as caught:
            lower_source(source, REGISTRY)
        self.assertEqual(caught.exception.code, "unsupported_statement")
        self.assertFalse(marker.exists())
        self.assertNotIn("exec(", (ROOT / "pipeline" / "wge_semantic_ir.py").read_text(encoding="utf-8"))
        rejected = run_kernel(["reject-effect", "--effect", "generation.commit"])
        self.assertNotEqual(rejected.returncode, 0)
        self.assertEqual(events(rejected.stdout)[0]["code"], "effect_forbidden")
        host = run_kernel(["reject-effect", "--effect", "filesystem.write"])
        self.assertEqual(events(host.stdout)[0]["code"], "host_effect_not_semantic")
        missing = run_kernel(["commit"])
        self.assertNotEqual(missing.returncode, 0)
        self.assertEqual(events(missing.stdout)[0]["code"], "usage")

    def test_p_job_payload_cannot_register_an_operation(self) -> None:
        probed = run_kernel(
            [
                "probe-hostile",
                "--worker",
                str(WORKER),
                "--project",
                str(PROJECT),
            ]
        )
        self.assertEqual(probed.returncode, 0, probed.stdout + probed.stderr)
        rows = events(probed.stdout)
        by_payload = {row["payload"]: row for row in rows}
        self.assertEqual(by_payload["register"]["response"]["code"], "unsupported_operation")
        self.assertEqual(by_payload["eval"]["response"]["code"], "rejected_payload")
        self.assertTrue(by_payload["measure"]["response"]["intersects"])
        self.assertEqual(by_payload["register"]["pid"], by_payload["measure"]["pid"])
        self.assertNotIn("gate_passed", by_payload["measure"]["response"])

    def test_q_restarting_the_worker_does_not_move_the_pointer(self) -> None:
        before = (self.store_a / "pointer.json").read_bytes()
        pid = self._one("worker_ready")["pid"]
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)
        poked = run_kernel(
            [
                "poke-worker",
                "--worker",
                str(WORKER),
                "--project",
                str(PROJECT),
                "--manifest",
                str(MANIFEST),
            ]
        )
        self.assertEqual(poked.returncode, 0, poked.stdout + poked.stderr)
        self.assertNotEqual(events(poked.stdout)[0]["pid"], pid)
        self.assertEqual((self.store_a / "pointer.json").read_bytes(), before)

    def test_r_rejected_candidate_is_not_authoritative_state(self) -> None:
        repair = self._one("repair")
        candidate = [row for row in self.events_a if row.get("event") == "candidate"][0]
        record = json.loads(
            (self.store_a / "candidates" / f"{candidate['candidate_id']}.json").read_text(encoding="utf-8")
        )
        self.assertEqual(record["status"], "rejected")
        self.assertEqual(record["repair_id"], repair["failure_id"])
        self.assertFalse((self.store_a / "generations" / f"{candidate['candidate_id']}.json").exists())
        generations = list((self.store_a / "generations").glob("*.json"))
        self.assertEqual(len(generations), 2)

    def test_python_ir_digest_matches_the_rust_kernel(self) -> None:
        document = lower_file(REPAIRED, REGISTRY)
        python_digest = "sha256:" + hashlib.sha256(canonical_dumps(document).encode("utf-8")).hexdigest()
        checked = run_kernel(
            [
                "check-ir",
                "--registry",
                str(REGISTRY),
                "--source",
                str(REPAIRED),
                "--ir",
                str(self.store_a / "repaired.ir.json"),
            ]
        )
        self.assertEqual(checked.returncode, 0, checked.stdout)
        self.assertEqual(events(checked.stdout)[0]["ir_digest"], python_digest)
        self.assertEqual(events(checked.stdout)[0]["registry_digest"], file_digest(REGISTRY))

    def test_unknown_field_and_constructor_are_rejected(self) -> None:
        document = lower_file(INVALID, REGISTRY)
        document["declarations"][0]["surprise"] = 1
        path = self.store_a / "unknown-field.ir.json"
        path.write_text(canonical_dumps(document) + "\n", encoding="utf-8")
        checked = run_kernel(
            ["check-ir", "--registry", str(REGISTRY), "--source", str(INVALID), "--ir", str(path)]
        )
        self.assertEqual(events(checked.stdout)[0]["code"], "unknown_field")
        with self.assertRaises(LowerError) as caught:
            lower_source("from wge.world import lane\nthing = not_a_constructor()\n", REGISTRY)
        self.assertEqual(caught.exception.code, "unregistered_call")

    def test_corrupt_evidence_fails_verification_without_moving_the_pointer(self) -> None:
        before = (self.store_b / "pointer.json").read_bytes()
        passed = [row for row in self.events_b if row.get("event") == "measurement"][-1]
        blob = self.store_b / "evidence" / f"{passed['sha256'].split(':', 1)[1]}.bin"
        blob.write_bytes(blob.read_bytes() + b" ")
        verified = run_kernel(["verify-current", "--store", str(self.store_b)])
        self.assertNotEqual(verified.returncode, 0)
        self.assertEqual(events(verified.stdout)[0]["code"], "evidence_hash_mismatch")
        self.assertEqual((self.store_b / "pointer.json").read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
