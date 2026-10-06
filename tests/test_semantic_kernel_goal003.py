from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "pipeline"))

from luxel_semantic_ir import canonical_dumps, lower_file  # noqa: E402


REGISTRY = ROOT / "world_core" / "crates" / "semantic_kernel" / "registry_v0.json"
WORKER = ROOT / "terrain_lab" / "bin" / "lane_overlap_worker.jl"
PROJECT = ROOT / "terrain_lab"
MANIFEST = PROJECT / "Manifest.toml"
FIXTURES = ROOT / "tests" / "fixtures" / "semantic_kernel"
BINARY = ROOT / "world_core" / "target" / "debug" / "luxel-semantic-kernel"
PATH_INVALID = FIXTURES / "path_invalid.luxel"
PATH_REPAIRED = FIXTURES / "path_repaired.luxel"
PATH_NOTE = FIXTURES / "path_repaired_note.luxel"
PATH_TAMPERED = FIXTURES / "path_tampered.luxel"


def events(text: str) -> list[dict]:
    return [json.loads(line) for line in text.splitlines() if line.startswith("{")]


def run_kernel(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run([str(BINARY), *args], text=True, capture_output=True, check=False)


def write_ir(source: Path, directory: Path, name: str) -> Path:
    path = directory / name
    path.write_text(canonical_dumps(lower_file(source, REGISTRY)) + "\n", encoding="utf-8")
    return path


class Goal003SemanticKernelTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        subprocess.run(
            ["cargo", "build", "-p", "luxel-semantic-kernel", "--quiet"],
            cwd=ROOT / "world_core",
            check=True,
        )

    def path_args(self, directory: Path) -> tuple[Path, Path, Path]:
        invalid_ir = write_ir(PATH_INVALID, directory, "path-invalid.ir.json")
        repaired_ir = write_ir(PATH_REPAIRED, directory, "path-repaired.ir.json")
        tampered_ir = write_ir(PATH_TAMPERED, directory, "path-tampered.ir.json")
        return invalid_ir, repaired_ir, tampered_ir

    def certify_args(self, store: Path, source: Path, ir: Path) -> list[str]:
        return [
            "certify",
            "--store",
            str(store),
            "--source",
            str(source),
            "--ir",
            str(ir),
            "--registry",
            str(REGISTRY),
            "--worker",
            str(WORKER),
            "--project",
            str(PROJECT),
            "--manifest",
            str(MANIFEST),
        ]

    def apply_args(
        self,
        store: Path,
        invalid_ir: Path,
        repaired_ir: Path,
        tampered_ir: Path,
    ) -> list[str]:
        return [
            "apply-repair",
            "--store",
            str(store),
            "--invalid-source",
            str(PATH_INVALID),
            "--invalid-ir",
            str(invalid_ir),
            "--repaired-source",
            str(PATH_REPAIRED),
            "--repaired-ir",
            str(repaired_ir),
            "--tampered-source",
            str(PATH_TAMPERED),
            "--tampered-ir",
            str(tampered_ir),
            "--registry",
            str(REGISTRY),
            "--worker",
            str(WORKER),
            "--project",
            str(PROJECT),
            "--manifest",
            str(MANIFEST),
        ]

    def commit_path_world(self, directory: Path) -> tuple[Path, str, subprocess.CompletedProcess[str]]:
        store = directory / "store"
        store.mkdir()
        invalid_ir, repaired_ir, tampered_ir = self.path_args(directory)
        failed = run_kernel(self.certify_args(store, PATH_INVALID, invalid_ir))
        self.assertEqual(failed.returncode, 0, failed.stdout + failed.stderr)
        applied = run_kernel(self.apply_args(store, invalid_ir, repaired_ir, tampered_ir))
        self.assertEqual(applied.returncode, 0, applied.stdout + applied.stderr)
        status = run_kernel(["status", "--store", str(store)])
        self.assertEqual(status.returncode, 0, status.stdout + status.stderr)
        generation_id = events(status.stdout)[0]["generation_id"]
        self.assertNotEqual(generation_id, "G0")
        return store, generation_id, applied

    def test_path_failure_is_a_repair_and_keeps_genesis_current(self) -> None:
        directory = Path(tempfile.mkdtemp(prefix="luxel-goal003-fail-"))
        try:
            store = directory / "store"
            store.mkdir()
            invalid_ir = write_ir(PATH_INVALID, directory, "invalid.ir.json")
            result = run_kernel(self.certify_args(store, PATH_INVALID, invalid_ir))
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(events(run_kernel(["status", "--store", str(store)]).stdout)[0]["generation_id"], "G0")
            repair_event = next(row for row in events(result.stdout) if row.get("event") == "repair")
            self.assertEqual(repair_event["gate_id"], "path.within_budget")
            self.assertEqual(repair_event["repair_class"], "shorten_path")
            repairs = list((store / "repairs").glob("*.json"))
            self.assertEqual(len(repairs), 1)
            repair = json.loads(repairs[0].read_text())
            self.assertEqual(repair["allowed_repair_class"], "shorten_path")
            self.assertEqual(repair["node_id"], "long_route")
            self.assertEqual(repair["source_span"]["line"], 6)
        finally:
            shutil.rmtree(directory, ignore_errors=True)

    def test_lane_and_path_commit_through_one_generation_and_one_worker(self) -> None:
        directory = Path(tempfile.mkdtemp(prefix="luxel-goal003-commit-"))
        try:
            store, generation_id, applied = self.commit_path_world(directory)
            solver_rows = [row for row in events(applied.stdout) if row.get("event") == "solver"]
            self.assertEqual([row["julia_op"] for row in solver_rows], ["lane_overlap", "path_length"])
            self.assertEqual({row["pid"] for row in solver_rows}, {next(row for row in events(applied.stdout) if row.get("event") == "worker_ready")["pid"]})

            generation = json.loads((store / "generations" / f"{generation_id}.json").read_text())
            self.assertEqual(set(generation["artifact_index"]), {"lane_overlap", "path_length"})
            self.assertEqual(set(generation["evidence_index"]), {"lane.footprint_clear", "path.within_budget"})
            self.assertIsNone(generation["commit_receipt_id"])
            self.assertEqual(set(generation["receipt_index"]), set(generation["evidence_index"]))
            self.assertEqual(len(list(store.glob("pointer.json"))), 1)
            self.assertEqual(len(list(store.glob("evidence/*.bin"))), 3)
            proposals = [json.loads(path.read_text()) for path in (store / "proposals").glob("*.json")]
            committed = [proposal for proposal in proposals if proposal["status"] == "committed"]
            self.assertEqual(len(committed), 1)
            self.assertEqual(committed[0]["operation"], "lane_overlap+path_length")

            inspected = run_kernel(["inspect", "--store", str(store)])
            self.assertEqual(inspected.returncode, 0, inspected.stdout + inspected.stderr)
            inspect_row = events(inspected.stdout)[0]
            self.assertEqual(set(inspect_row["artifact_index"]), {"lane_overlap", "path_length"})
            self.assertEqual(set(inspect_row["receipt_index"]), {"lane.footprint_clear", "path.within_budget"})
            verified = run_kernel(["verify-current", "--store", str(store)])
            self.assertEqual(verified.returncode, 0, verified.stdout + verified.stderr)
            accepted = run_kernel(
                [
                    "accept-receipt",
                    "--store",
                    str(store),
                    "--receipt-id",
                    generation["receipt_index"]["path.within_budget"],
                ]
            )
            self.assertEqual(accepted.returncode, 0, accepted.stdout + accepted.stderr)
        finally:
            shutil.rmtree(directory, ignore_errors=True)

    def test_equivalent_source_and_fresh_run_keep_multi_gate_identity_stable(self) -> None:
        first_dir = Path(tempfile.mkdtemp(prefix="luxel-goal003-a-"))
        second_dir = Path(tempfile.mkdtemp(prefix="luxel-goal003-b-"))
        try:
            first_store, first_generation, _ = self.commit_path_world(first_dir)
            second_store, second_generation, _ = self.commit_path_world(second_dir)
            self.assertEqual(first_generation, second_generation)

            before = (first_store / "generations" / f"{first_generation}.json").read_bytes()
            note_ir = write_ir(PATH_NOTE, first_dir, "note.ir.json")
            invalid_ir = write_ir(PATH_INVALID, first_dir, "note-invalid.ir.json")
            equivalent = run_kernel(
                [
                    "submit-equivalent",
                    "--store",
                    str(first_store),
                    "--invalid-source",
                    str(PATH_INVALID),
                    "--invalid-ir",
                    str(invalid_ir),
                    "--source",
                    str(PATH_NOTE),
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
            self.assertEqual(row["generation_id"], first_generation)
            self.assertNotEqual(row["authoring_digest"], "sha256:" + "0" * 64)
            self.assertEqual((first_store / "generations" / f"{first_generation}.json").read_bytes(), before)
        finally:
            shutil.rmtree(first_dir, ignore_errors=True)
            shutil.rmtree(second_dir, ignore_errors=True)

    def test_corrupt_multi_gate_generation_fails_closed(self) -> None:
        directory = Path(tempfile.mkdtemp(prefix="luxel-goal003-corrupt-"))
        try:
            store, generation_id, _ = self.commit_path_world(directory)
            broken = directory / "broken"
            shutil.copytree(store, broken)
            record_path = broken / "generations" / f"{generation_id}.json"
            record = json.loads(record_path.read_text())
            record["evidence_index"]["path.within_budget"] = "sha256:" + "0" * 64
            record_path.write_text(json.dumps(record), encoding="utf-8")
            inspected = run_kernel(["inspect", "--store", str(broken)])
            self.assertNotEqual(inspected.returncode, 0)
            self.assertEqual(events(inspected.stdout)[0]["code"], "identity_conflict")
            self.assertEqual(events(run_kernel(["status", "--store", str(store)]).stdout)[0]["generation_id"], generation_id)
        finally:
            shutil.rmtree(directory, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
