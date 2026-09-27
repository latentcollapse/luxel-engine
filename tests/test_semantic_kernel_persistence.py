"""Process-restart and identity tests for the semantic transaction kernel."""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "pipeline"))

from wge_semantic_ir import canonical_dumps, lower_file  # noqa: E402

REGISTRY = ROOT / "world_core" / "crates" / "semantic_kernel" / "registry_v0.json"
WORKER = ROOT / "terrain_lab" / "bin" / "lane_overlap_worker.jl"
PROJECT = ROOT / "terrain_lab"
MANIFEST = PROJECT / "Manifest.toml"
FIXTURES = ROOT / "tests" / "fixtures" / "semantic_kernel"
BINARY = ROOT / "world_core" / "target" / "debug" / "wge-semantic-kernel"
INVALID = FIXTURES / "invalid.wge"
REPAIRED = FIXTURES / "repaired.wge"
TAMPERED = FIXTURES / "tampered.wge"
NOTE = FIXTURES / "repaired_note.wge"


def events(text: str) -> list[dict]:
    return [json.loads(line) for line in text.splitlines() if line.startswith("{")]


def run_kernel(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run([str(BINARY), *args], text=True, capture_output=True, check=False)


def write_ir(source: Path, directory: Path, name: str) -> Path:
    path = directory / name
    path.write_text(canonical_dumps(lower_file(source, REGISTRY)) + "\n", encoding="utf-8")
    return path


def common(store: Path, directory: Path) -> list[str]:
    return [
        "--store",
        str(store),
        "--invalid-source",
        str(INVALID),
        "--invalid-ir",
        str(write_ir(INVALID, directory, "invalid.ir.json")),
        "--repaired-source",
        str(REPAIRED),
        "--repaired-ir",
        str(write_ir(REPAIRED, directory, "repaired.ir.json")),
        "--tampered-source",
        str(TAMPERED),
        "--tampered-ir",
        str(write_ir(TAMPERED, directory, "tampered.ir.json")),
        "--registry",
        str(REGISTRY),
        "--worker",
        str(WORKER),
        "--project",
        str(PROJECT),
        "--manifest",
        str(MANIFEST),
    ]


class PersistenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        subprocess.run(
            ["cargo", "build", "-p", "wge-semantic-kernel", "--quiet"],
            cwd=ROOT / "world_core",
            check=True,
        )

    def test_create_reopen_commit_and_equivalent_source(self) -> None:
        directory = Path(tempfile.mkdtemp(prefix="wge-persist-"))
        store = directory / "store"
        store.mkdir()
        try:
            created = run_kernel(["fail-proposal", *common(store, directory)])
            self.assertEqual(created.returncode, 0, created.stdout + created.stderr)
            self.assertEqual(self.status(store), "G0")
            self.assertTrue(any(row.get("class") == "SemanticRepair" for row in events(created.stdout)))
            pointer = (store / "pointer.json").read_bytes()
            genesis = (store / "generations" / "G0.json").read_bytes()
            reopened = run_kernel(["inspect", "--store", str(store)])
            self.assertEqual(reopened.returncode, 0, reopened.stdout)
            self.assertEqual(events(reopened.stdout)[0]["generation_id"], "G0")
            self.assertEqual((store / "pointer.json").read_bytes(), pointer)
            self.assertEqual((store / "generations" / "G0.json").read_bytes(), genesis)
            again = run_kernel(["create-store", "--store", str(store), "--registry", str(REGISTRY)])
            self.assertNotEqual(again.returncode, 0)
            self.assertEqual(events(again.stdout)[0]["code"], "store_exists")
            self.assertEqual((store / "generations" / "G0.json").read_bytes(), genesis)

            applied = run_kernel(["apply-repair", *common(store, directory)])
            self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
            committed = self.status(store)
            self.assertNotEqual(committed, "G0")
            generation = (store / "generations" / f"{committed}.json").read_bytes()
            self.assertEqual(self.status(store), committed)
            inspected = events(run_kernel(["inspect", "--store", str(store)]).stdout)[0]
            self.assertEqual(inspected["parent_id"], "G0")
            self.assertIsNotNone(inspected["measurement_sha256"])
            self.assertTrue((store / "receipts" / f"{inspected['commit_receipt_id']}.json").is_file())
            self.assertEqual(self.status(store), committed)
            self.assertEqual((store / "generations" / f"{committed}.json").read_bytes(), generation)

            stale = run_kernel(["apply-repair", *common(store, directory)])
            self.assertNotEqual(stale.returncode, 0)
            self.assertEqual(events(stale.stdout)[0]["code"], "stale_repair")
            self.assertEqual((store / "generations" / f"{committed}.json").read_bytes(), generation)

            note_ir = write_ir(NOTE, directory, "note.ir.json")
            equivalent = run_kernel(
                [
                    "submit-equivalent",
                    "--store",
                    str(store),
                    "--invalid-source",
                    str(INVALID),
                    "--invalid-ir",
                    str(directory / "invalid.ir.json"),
                    "--source",
                    str(NOTE),
                    "--ir",
                    str(note_ir),
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
            self.assertEqual(equivalent.returncode, 0, equivalent.stdout + equivalent.stderr)
            row = events(equivalent.stdout)[-1]
            self.assertEqual(row["event"], "equivalent")
            self.assertTrue(row["unchanged"])
            self.assertEqual(row["generation_id"], committed)
            self.assertEqual((store / "generations" / f"{committed}.json").read_bytes(), generation)
            proposals = list((store / "proposals").glob("*.json"))
            digests = {json.loads(path.read_text())["authoring_digest"] for path in proposals}
            self.assertGreaterEqual(len(digests), 2)
            blobs = list((store / "evidence").glob("*.bin"))
            bindings = [json.loads(path.read_text()) for path in (store / "bindings").glob("*.json")]
            pass_sha = inspected["measurement_sha256"]
            sharing = [item for item in bindings if item["evidence_sha256"] == pass_sha]
            self.assertGreaterEqual(len(sharing), 2)
            self.assertGreater(len({item["candidate_id"] for item in sharing}), 1)
            self.assertEqual(len(blobs), 2)
            foreign = run_kernel(
                [
                    "bind-evidence",
                    "--store",
                    str(store),
                    "--candidate",
                    sharing[0]["candidate_id"],
                    "--evidence-sha",
                    next(item["evidence_sha256"] for item in bindings if item["evidence_sha256"] != pass_sha),
                ]
            )
            self.assertEqual(events(foreign.stdout)[0]["code"], "stale_evidence")
        finally:
            shutil.rmtree(directory, ignore_errors=True)

    def test_two_fresh_runs_share_semantic_ids_and_second_open_is_idempotent(self) -> None:
        first = Path(tempfile.mkdtemp(prefix="wge-run-a-"))
        second = Path(tempfile.mkdtemp(prefix="wge-run-b-"))
        try:
            first_log = self.full_run(first)
            second_log = self.full_run(second)
            first_rows = events(first_log)
            second_rows = events(second_log)
            self.assertEqual(
                next(row for row in first_rows if row.get("event") == "canonical_result"),
                next(row for row in second_rows if row.get("event") == "canonical_result"),
            )
            generation = next(row for row in first_rows if row.get("event") == "commit")["generation_id"]
            before = (first / "store" / "generations" / f"{generation}.json").read_bytes()
            repeated = run_kernel(["run", *common(first / "store", first)])
            self.assertEqual(repeated.returncode, 0, repeated.stdout + repeated.stderr)
            self.assertTrue(any(row.get("event") == "idempotent" for row in events(repeated.stdout)))
            self.assertEqual((first / "store" / "generations" / f"{generation}.json").read_bytes(), before)
            self.assertEqual(self.status(first / "store"), generation)
        finally:
            shutil.rmtree(first, ignore_errors=True)
            shutil.rmtree(second, ignore_errors=True)

    def test_corruption_fails_closed_without_resetting_g0(self) -> None:
        directory = Path(tempfile.mkdtemp(prefix="wge-corrupt-"))
        try:
            self.full_run(directory)
            store = directory / "store"
            generation = self.status(store)
            genesis = (store / "generations" / "G0.json").read_bytes()
            pointer = (store / "pointer.json").read_bytes()
            broken = directory / "broken"
            shutil.copytree(store, broken)
            pointer_body = json.loads((broken / "pointer.json").read_text())
            pointer_body["generation_id"] = "G-missing"
            (broken / "pointer.json").write_text(json.dumps(pointer_body), encoding="utf-8")
            missing = run_kernel(["inspect", "--store", str(broken)])
            self.assertEqual(events(missing.stdout)[0]["code"], "missing_generation")
            self.assertFalse((broken / "generations" / "G-missing.json").exists())
            self.assertEqual((broken / "generations" / "G0.json").read_bytes(), genesis)

            shutil.rmtree(broken)
            shutil.copytree(store, broken)
            schema = json.loads((broken / "store.json").read_text())
            schema["schema"] = "wge.store/v9"
            (broken / "store.json").write_text(json.dumps(schema), encoding="utf-8")
            unsupported = run_kernel(["inspect", "--store", str(broken)])
            self.assertEqual(events(unsupported.stdout)[0]["code"], "unsupported_schema")

            shutil.rmtree(broken)
            shutil.copytree(store, broken)
            record_path = broken / "generations" / f"{generation}.json"
            record = json.loads(record_path.read_text())
            record["registry_digest"] = "sha256:" + "ab" * 32
            record_path.write_text(json.dumps(record), encoding="utf-8")
            conflict = run_kernel(["inspect", "--store", str(broken)])
            self.assertEqual(events(conflict.stdout)[0]["code"], "identity_conflict")
            self.assertEqual((store / "pointer.json").read_bytes(), pointer)

            shutil.rmtree(broken)
            shutil.copytree(store, broken)
            measurement = json.loads((broken / "generations" / f"{generation}.json").read_text())["measurement_sha256"]
            blob = broken / "evidence" / f"{measurement.split(':', 1)[1]}.bin"
            blob.write_bytes(blob.read_bytes() + b"x")
            hashed = run_kernel(["inspect", "--store", str(broken)])
            self.assertEqual(events(hashed.stdout)[0]["code"], "evidence_hash_mismatch")
            self.assertEqual(self.status(store), generation)
        finally:
            shutil.rmtree(directory, ignore_errors=True)

    def test_solver_drift_and_julia_restart_leave_the_world(self) -> None:
        directory = Path(tempfile.mkdtemp(prefix="wge-solver-"))
        try:
            log = self.full_run(directory)
            store = directory / "store"
            generation = next(row for row in events(log) if row.get("event") == "commit")["generation_id"]
            image = next(row for row in events(log) if row.get("event") == "worker_ready")["solver_image"]
            before = (store / "pointer.json").read_bytes()
            poked = run_kernel(
                ["poke-worker", "--worker", str(WORKER), "--project", str(PROJECT), "--manifest", str(MANIFEST)]
            )
            self.assertEqual(poked.returncode, 0, poked.stdout + poked.stderr)
            self.assertNotEqual(events(poked.stdout)[0]["pid"], next(row for row in events(log) if row.get("event") == "worker_ready")["pid"])
            self.assertEqual(events(poked.stdout)[0]["solver_image"], image)
            self.assertEqual((store / "pointer.json").read_bytes(), before)
            drifted = run_kernel(["check-solver", "--store", str(store), "--solver-image", "sha256:" + "00" * 32])
            self.assertEqual(events(drifted.stdout)[0]["code"], "solver_image_mismatch")
            self.assertEqual(self.status(store), generation)
        finally:
            shutil.rmtree(directory, ignore_errors=True)

    def test_same_length_inplace_edits_fail_closed(self) -> None:
        directory = Path(tempfile.mkdtemp(prefix="wge-inplace-"))
        try:
            self.full_run(directory)
            store = directory / "store"
            generation = self.status(store)
            pointer = (store / "pointer.json").read_bytes()
            record_path = store / "generations" / f"{generation}.json"
            original = record_path.read_bytes()
            self.assertIn(b"lane_overlap", original)
            edited = original.replace(b"lane_overlap", b"lane_overlaX", 1)
            self.assertEqual(len(edited), len(original))
            record_path.write_bytes(edited)
            opened = run_kernel(["inspect", "--store", str(store)])
            self.assertEqual(events(opened.stdout)[0]["code"], "identity_conflict")
            record_path.write_bytes(original)
            self.assertEqual(self.status(store), generation)

            g0 = store / "generations" / "G0.json"
            g0_bytes = bytearray(g0.read_bytes())
            digest_at = g0_bytes.rfind(b"sha256:")
            self.assertGreater(digest_at, 0)
            nibble = digest_at + len(b"sha256:") + 63
            g0_bytes[nibble] = ord("0") if g0_bytes[nibble] != ord("0") else ord("1")
            g0.write_bytes(g0_bytes)
            child = run_kernel(["inspect", "--store", str(store)])
            self.assertNotEqual(child.returncode, 0)
            self.assertEqual(events(child.stdout)[0]["code"], "identity_conflict")
            pointer_body = json.loads(pointer)
            pointer_body["generation_id"] = "G0"
            (store / "pointer.json").write_bytes(json.dumps(pointer_body, separators=(",", ":")).encode())
            genesis = run_kernel(["inspect", "--store", str(store)])
            self.assertNotEqual(genesis.returncode, 0)
            self.assertEqual(events(genesis.stdout)[0]["code"], "identity_conflict")
            self.assertFalse((store / "generations" / "G-missing.json").exists())
        finally:
            shutil.rmtree(directory, ignore_errors=True)

    def test_identity_checks_reject_rewritten_records(self) -> None:
        directory = Path(tempfile.mkdtemp(prefix="wge-identity-"))
        try:
            self.full_run(directory)
            store = directory / "store"
            generation = self.status(store)
            pointer = (store / "pointer.json").read_bytes()
            record_path = store / "generations" / f"{generation}.json"
            original = record_path.read_bytes()
            record = json.loads(original)
            record["determinism"] = "loose"
            record_path.write_text(json.dumps(record), encoding="utf-8")
            opened = run_kernel(["inspect", "--store", str(store)])
            self.assertNotEqual(opened.returncode, 0)
            self.assertEqual(events(opened.stdout)[0]["code"], "identity_conflict")
            record_path.write_bytes(original)

            receipt_id = record["commit_receipt_id"]
            receipt_path = store / "receipts" / f"{receipt_id}.json"
            receipt_bytes = receipt_path.read_bytes()
            receipt = json.loads(receipt_bytes)
            receipt["gate_result"] = "fail"
            receipt["solver_image"] = "sha256:" + "11" * 32
            receipt["ir_digest"] = "sha256:" + "22" * 32
            receipt["parent_id"] = "G-other"
            receipt["registry_digest"] = "sha256:" + "33" * 32
            receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
            accepted = run_kernel(["accept-receipt", "--store", str(store), "--receipt-id", receipt_id])
            self.assertNotEqual(accepted.returncode, 0)
            self.assertIn(events(accepted.stdout)[0]["code"], {"forged_receipt", "identity_conflict"})
            receipt_path.write_bytes(receipt_bytes)

            binding_path = next((store / "bindings").glob("*.json"))
            binding_bytes = binding_path.read_bytes()
            binding = json.loads(binding_bytes)
            real_candidate = binding["candidate_id"]
            evidence_sha = binding["evidence_sha256"]
            binding["candidate_id"] = "C-forged"
            binding_path.write_text(json.dumps(binding), encoding="utf-8")
            forged = run_kernel(
                ["bind-evidence", "--store", str(store), "--candidate", "C-forged", "--evidence-sha", evidence_sha]
            )
            self.assertNotEqual(forged.returncode, 0)
            self.assertIn(events(forged.stdout)[0]["code"], {"identity_conflict", "stale_evidence"})
            still = run_kernel(
                ["bind-evidence", "--store", str(store), "--candidate", real_candidate, "--evidence-sha", evidence_sha]
            )
            self.assertNotEqual(still.returncode, 0)
            binding_path.write_bytes(binding_bytes)
            self.assertEqual(self.status(store), generation)

            g0 = store / "generations" / "G0.json"
            g0_bytes = g0.read_bytes()
            g0.unlink()
            dangling = run_kernel(["inspect", "--store", str(store)])
            self.assertEqual(events(dangling.stdout)[0]["code"], "missing_generation")
            g0.write_bytes(g0_bytes)
            self.assertEqual((store / "pointer.json").read_bytes(), pointer)

            other = directory / "other.wge"
            other.write_text(
                "from wge.world import lane, place\n"
                "from wge.geometry import rect\n"
                "\n"
                "central_lane = lane(id=\"central\", footprint=rect(x0=0, y0=40, x1=100, y1=60))\n"
                "blocked_keep = place(id=\"keep\", footprint=rect(x0=0, y0=0, x1=10, y1=10))\n",
                encoding="utf-8",
            )
            other_ir = write_ir(other, directory, "other.ir.json")
            moved = run_kernel(
                [
                    "submit-equivalent",
                    "--store", str(store),
                    "--invalid-source", str(INVALID),
                    "--invalid-ir", str(directory / "invalid.ir.json"),
                    "--source", str(other),
                    "--ir", str(other_ir),
                    "--registry", str(REGISTRY),
                    "--worker", str(WORKER),
                    "--project", str(PROJECT),
                    "--manifest", str(MANIFEST),
                ]
            )
            self.assertNotEqual(moved.returncode, 0, moved.stdout + moved.stderr)
            self.assertEqual(events(moved.stdout)[-1]["code"], "not_equivalent")
            self.assertEqual((store / "pointer.json").read_bytes(), pointer)
            self.assertEqual(self.status(store), generation)
        finally:
            shutil.rmtree(directory, ignore_errors=True)

    def status(self, store: Path) -> str:
        result = run_kernel(["status", "--store", str(store)])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return events(result.stdout)[0]["generation_id"]

    def full_run(self, directory: Path) -> str:
        store = directory / "store"
        store.mkdir()
        result = run_kernel(["run", *common(store, directory)])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result.stdout


if __name__ == "__main__":
    unittest.main()
