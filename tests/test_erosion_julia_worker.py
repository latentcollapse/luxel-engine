"""Focused contract test for the Julia-owned erosion seam."""

from __future__ import annotations

import os
import hashlib
import json
import shlex
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

import erosion  # noqa: E402
from erosion import ErosionProfile, erode, flux_field, valley_cross_section  # noqa: E402


def canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False
    ).encode("utf-8")
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


class JuliaErosionWorkerTests(unittest.TestCase):
    def test_erode_uses_the_julia_worker_and_returns_a_receipted_result(self) -> None:
        height = np.array(
            [
                [8.0, 6.0, 4.0, 6.0, 8.0],
                [7.0, 5.0, 3.0, 5.0, 7.0],
                [6.0, 4.0, 2.0, 4.0, 6.0],
                [7.0, 5.0, 3.0, 5.0, 7.0],
                [8.0, 6.0, 4.0, 6.0, 8.0],
            ],
            dtype=np.float64,
        )
        profile = ErosionProfile(
            key="worker-test",
            iterations=1,
            stream_power=0.4,
            glacial_strength=0.8,
            ice_line=0.6,
            lateral_widening=0.5,
            talus_degrees=35.0,
            cirque_strength=0.4,
            planation=0.2,
        )
        previous = os.environ.get("WGE_EROSION_BACKEND")
        os.environ["WGE_EROSION_BACKEND"] = "julia"
        try:
            eroded, report = erode(height, profile, cell_m=2.0)
            receipt = report["worker_receipt"]
            self.assertEqual(receipt["schema"], "wge.erosion-worker-receipt/v1")
            self.assertEqual(receipt["operation"], "erode")
            self.assertEqual(receipt["job_index"], 1)
            self.assertEqual(report["worker_receipt_sha256"], canonical_digest(receipt))

            section, section_receipt = valley_cross_section(eroded, 2, return_receipt=True)
            self.assertEqual(section_receipt.job_index, 2)
            self.assertEqual(section_receipt.operation, "valley_cross_section")
            self.assertEqual(section_receipt.worker_pid, receipt["worker_pid"])

            flux, flux_receipt = flux_field(eroded, return_receipt=True)
            self.assertEqual(flux.shape, height.shape)
            self.assertEqual(flux_receipt.job_index, 3)
            self.assertEqual(flux_receipt.worker_pid, receipt["worker_pid"])
        finally:
            erosion._shutdown_erosion_supervisor()
            if previous is None:
                os.environ.pop("WGE_EROSION_BACKEND", None)
            else:
                os.environ["WGE_EROSION_BACKEND"] = previous

        self.assertEqual(eroded.shape, height.shape)
        self.assertTrue(np.isfinite(eroded).all())
        self.assertEqual(report["profile"], "worker-test")
        self.assertEqual(report["iterations"], 1)
        self.assertIn(section["shape"], {"u", "v", "flat"})
        self.assertGreaterEqual(section["relief_m"], 0.0)

    def test_forged_supervisor_receipt_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wge-forged-erosion-receipt-") as temporary:
            script = Path(temporary) / "fake-supervisor.py"
            script.write_text(
                """#!/usr/bin/env python3
import hashlib, json, os, sys

def digest(value):
    raw = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    return "sha256:" + hashlib.sha256(raw).hexdigest()

pid = os.getpid()
script_sha = "1" * 64
project_sha = "2" * 64
solver_image = "sha256:" + "3" * 64
print(json.dumps({"cold_ms": 1, "event": "worker_ready", "manifest_sha256": "4" * 64,
                  "pid": pid, "project_sha256": project_sha, "script_sha256": script_sha,
                  "solver_image": solver_image, "wrapper_version": "wge.erosion-worker/v1"}), flush=True)
request = json.loads(sys.stdin.readline())
response = {"schema": "codeweald.erosion-result/v1", "status": "ok",
            "operation": request["operation"], "shape": request["shape"],
            "dtype": "float64-le", "result_sha256": "5" * 64}
receipt = {"job_index": 1, "operation": request["operation"], "project_sha256": project_sha,
           "request_sha256": digest(request), "response_sha256": digest(response),
           "result_sha256": response["result_sha256"],
           "schema": "wge.erosion-worker-receipt/v1", "script_sha256": script_sha,
           "solver_image": solver_image, "worker_pid": pid}
print(json.dumps({"event": "worker_result", "job_index": 1, "job_us": 1,
                  "phase": "first_job", "pid": pid, "receipt": receipt,
                  "receipt_sha256": "sha256:" + "0" * 64, "response": response}), flush=True)
""",
                encoding="utf-8",
            )
            script.chmod(0o755)
            previous_backend = os.environ.get("WGE_EROSION_BACKEND")
            previous_kernel = os.environ.get("WGE_SEMANTIC_KERNEL")
            os.environ["WGE_EROSION_BACKEND"] = "julia"
            os.environ["WGE_SEMANTIC_KERNEL"] = shlex.quote(sys.executable) + " " + shlex.quote(str(script))
            try:
                with self.assertRaises(erosion.ErosionWorkerError) as raised:
                    flux_field(np.arange(9, dtype=np.float64).reshape(3, 3))
                self.assertEqual(raised.exception.code, "receipt_digest_mismatch")
            finally:
                erosion._shutdown_erosion_supervisor()
                if previous_backend is None:
                    os.environ.pop("WGE_EROSION_BACKEND", None)
                else:
                    os.environ["WGE_EROSION_BACKEND"] = previous_backend
                if previous_kernel is None:
                    os.environ.pop("WGE_SEMANTIC_KERNEL", None)
                else:
                    os.environ["WGE_SEMANTIC_KERNEL"] = previous_kernel


if __name__ == "__main__":
    unittest.main()
