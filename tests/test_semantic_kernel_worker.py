"""Falsifiable integration controls for the Rust-supervised Julia worker."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "world_core" / "target" / "debug" / "wge-semantic-kernel"
PROJECT = ROOT / "terrain_lab"
MANIFEST = PROJECT / "Manifest.toml"
EROSION_WORKER = PROJECT / "bin" / "erosion_worker.jl"
JULIA = os.environ.get("WGE_EROSION_JULIA") or shutil.which("julia") or "julia"


def events(text: str) -> list[dict]:
    return [json.loads(line) for line in text.splitlines() if line.startswith("{")]


def project_digest() -> str:
    hasher = hashlib.sha256()
    hasher.update(b"wge.julia-project/v0\0")
    for path in (PROJECT / "Project.toml", MANIFEST):
        data = path.read_bytes()
        hasher.update(len(data).to_bytes(8, "big"))
        hasher.update(data)
    return hasher.hexdigest()


def canonical_digest(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


class SemanticKernelWorkerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        subprocess.run(
            ["cargo", "build", "-p", "wge-semantic-kernel", "--quiet"],
            cwd=ROOT / "world_core",
            check=True,
        )

    def run_supervisor(
        self,
        input_text: str,
        *,
        julia: str = JULIA,
        env: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        command = [
            str(BINARY),
            "erosion-supervisor",
            "--julia",
            julia,
            "--project",
            str(PROJECT),
            "--manifest",
            str(MANIFEST),
            "--worker",
            str(EROSION_WORKER),
        ]
        environment = os.environ.copy()
        if env:
            environment.update(env)
        return subprocess.run(
            command,
            input=input_text,
            text=True,
            capture_output=True,
            check=False,
            env=environment,
        )

    @staticmethod
    def matrix_file(directory: Path, name: str, values: tuple[float, ...]) -> tuple[Path, str]:
        path = directory / name
        encoded = struct.pack(f"<{len(values)}d", *values)
        path.write_bytes(encoded)
        return path, hashlib.sha256(encoded).hexdigest()

    def flux_request(self, directory: Path, index: int, values: tuple[float, ...]) -> tuple[dict, Path]:
        height_path, digest = self.matrix_file(directory, f"height-{index}.f64", values)
        output_path = directory / f"flux-{index}.f64"
        return (
            {
                "schema": "codeweald.erosion-request/v1",
                "operation": "flux_field",
                "shape": [2, 3],
                "height_path": str(height_path),
                "height_sha256": digest,
                "output_path": str(output_path),
                "source_path": None,
                "source_sha256": None,
                "exponent": 1.15,
            },
            output_path,
        )

    def fake_julia(self, directory: Path, mode: str) -> Path:
        executable = directory / f"fake-julia-{mode}"
        body = r'''import json, os, re, struct, sys

if "--version" in sys.argv:
    print("julia version 1.10.0")
    raise SystemExit(0)

source = open(sys.argv[-1], encoding="utf-8").read()
script_hash = re.search(r'_WGE_SCRIPT_SHA256 = "([0-9a-f]{64})"', source).group(1)
project_hash = re.search(r'_WGE_PROJECT_SHA256 = "([0-9a-f]{64})"', source).group(1)
def write_frame(payload):
    sys.stdout.buffer.write(struct.pack(">I", len(payload)) + payload)
    sys.stdout.buffer.flush()
write_frame(json.dumps({"op": "ready", "script_sha256": script_hash,
                        "project_sha256": project_hash}).encode())
header = sys.stdin.buffer.read(4)
if len(header) != 4:
    raise SystemExit(3)
length = struct.unpack(">I", header)[0]
payload = sys.stdin.buffer.read(length)
if len(payload) != length:
    raise SystemExit(4)
mode = os.environ["WGE_TEST_FAKE_MODE"]
if mode == "exit":
    raise SystemExit(0)
if mode == "oversized-frame":
    sys.stdout.buffer.write(struct.pack(">I", 1000001))
    sys.stdout.buffer.flush()
    raise SystemExit(0)
if mode == "malformed-json":
    write_frame(b"not-json")
if mode == "forged-result-digest":
    request = json.loads(payload)
    result = bytes(request["shape"][0] * request["shape"][1] * 8)
    open(request["output_path"], "wb").write(result)
    response = {"schema": "codeweald.erosion-result/v1", "status": "ok",
                "operation": request["operation"], "shape": request["shape"],
                "dtype": "float64-le", "result_sha256": "0" * 64}
    write_frame(json.dumps(response).encode())
'''
        executable.write_text(f"#!{sys.executable}\n{body}", encoding="utf-8")
        executable.chmod(0o755)
        return executable

    def test_two_erosion_jobs_share_one_warm_julia_pid(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wge-warm-erosion-") as temporary:
            directory = Path(temporary)
            jobs = [
                self.flux_request(directory, 1, (0.0, 1.0, 2.0, 1.0, 2.0, 3.0)),
                self.flux_request(directory, 2, (4.0, 2.0, 0.0, 3.0, 1.0, -1.0)),
            ]
            input_text = "".join(json.dumps(job, separators=(",", ":")) + "\n" for job, _ in jobs)
            completed = self.run_supervisor(input_text)
            self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)

            rows = events(completed.stdout)
            ready = [row for row in rows if row.get("event") == "worker_ready"]
            results = [row for row in rows if row.get("event") == "worker_result"]
            self.assertEqual(len(ready), 1)
            self.assertEqual(len(results), 2)
            self.assertEqual({row["pid"] for row in results}, {ready[0]["pid"]})
            self.assertGreaterEqual(ready[0]["cold_ms"], 0)
            self.assertEqual(ready[0]["script_sha256"], hashlib.sha256(EROSION_WORKER.read_bytes()).hexdigest())
            self.assertEqual(ready[0]["project_sha256"], project_digest())
            self.assertEqual(ready[0]["manifest_sha256"], hashlib.sha256(MANIFEST.read_bytes()).hexdigest())
            self.assertTrue(ready[0]["solver_image"].startswith("sha256:"))

            result_digests = []
            for index, (result, (job, output_path)) in enumerate(zip(results, jobs, strict=True)):
                self.assertGreaterEqual(result["job_us"], 0)
                self.assertEqual(result["phase"], "first_job" if index == 0 else "warm")
                response = result["response"]
                self.assertEqual(response["schema"], "codeweald.erosion-result/v1")
                self.assertEqual(response["status"], "ok")
                self.assertEqual(response["operation"], "flux_field")
                self.assertEqual(response["shape"], [2, 3])
                self.assertEqual(response["dtype"], "float64-le")
                self.assertEqual(response["result_sha256"], hashlib.sha256(output_path.read_bytes()).hexdigest())
                result_digests.append(response["result_sha256"])

                receipt = result["receipt"]
                self.assertEqual(receipt["schema"], "wge.erosion-worker-receipt/v1")
                self.assertEqual(receipt["job_index"], index + 1)
                self.assertEqual(receipt["operation"], job["operation"])
                self.assertEqual(receipt["worker_pid"], ready[0]["pid"])
                self.assertEqual(receipt["script_sha256"], ready[0]["script_sha256"])
                self.assertEqual(receipt["project_sha256"], ready[0]["project_sha256"])
                self.assertEqual(receipt["request_sha256"], canonical_digest(job))
                self.assertEqual(receipt["response_sha256"], canonical_digest(response))
                self.assertEqual(receipt["result_sha256"], response["result_sha256"])
                self.assertEqual(result["receipt_sha256"], canonical_digest(receipt))
            self.assertNotEqual(result_digests[0], result_digests[1])

    def assert_fake_worker_failure(self, mode: str, expected_code: str) -> None:
        with tempfile.TemporaryDirectory(prefix=f"wge-fake-julia-{mode}-") as temporary:
            directory = Path(temporary)
            request, _output_path = self.flux_request(directory, 1, (0.0, 1.0, 2.0, 1.0, 2.0, 3.0))
            fake = self.fake_julia(directory, mode)
            completed = self.run_supervisor(
                json.dumps(request, separators=(",", ":")) + "\n",
                julia=str(fake),
                env={"WGE_TEST_FAKE_MODE": mode},
            )
            self.assertNotEqual(completed.returncode, 0, completed.stdout)
            failures = [row for row in events(completed.stdout) if row.get("event") == "failure"]
            self.assertTrue(failures, completed.stdout)
            self.assertEqual(failures[-1]["code"], expected_code)

    def test_oversized_worker_frame_fails_closed(self) -> None:
        self.assert_fake_worker_failure("oversized-frame", "malformed_protocol")

    def test_worker_exit_before_receipt_fails_closed(self) -> None:
        self.assert_fake_worker_failure("exit", "worker_exited")

    def test_malformed_worker_json_fails_closed(self) -> None:
        self.assert_fake_worker_failure("malformed-json", "malformed_protocol")

    def test_worker_cannot_receipt_a_forged_result_digest(self) -> None:
        self.assert_fake_worker_failure("forged-result-digest", "result_digest_mismatch")
