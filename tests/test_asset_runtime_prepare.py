"""Transport-only tests for the Rust-owned asset runtime CLI adapter."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from asset_runtime_prepare import prepare_asset_runtime  # noqa: E402


class AssetRuntimeTransportTest(unittest.TestCase):
    def test_native_arguments_and_rejection_exit_are_passed_through(self):
        native_result = CompletedProcess(
            args=["/native/luxel-asset-contract", "prepare"],
            returncode=3,
            stdout='{"status":"rejected"}',
            stderr="",
        )
        with patch("asset_runtime_prepare.subprocess.run", return_value=native_result) as run:
            result = prepare_asset_runtime(
                "source.glb",
                "request.json",
                executable="/native/luxel-asset-contract",
                cwd="/project",
                timeout_seconds=9.0,
            )

        self.assertIs(result, native_result)
        run.assert_called_once_with(
            (
                "/native/luxel-asset-contract",
                "prepare",
                "source.glb",
                "request.json",
            ),
            cwd="/project",
            capture_output=True,
            text=True,
            check=False,
            timeout=9.0,
        )

    def test_configured_binary_is_transport_only(self):
        native_result = CompletedProcess(args=[], returncode=0, stdout="{}", stderr="")
        with patch.dict("os.environ", {"LUXEL_ASSET_CONTRACT_BIN": "/configured/native"}):
            with patch("asset_runtime_prepare.subprocess.run", return_value=native_result) as run:
                result = prepare_asset_runtime("a.glb", "request.json")

        self.assertEqual(result.returncode, 0)
        self.assertEqual(run.call_args.args[0][0], "/configured/native")
        self.assertEqual(run.call_args.args[0][1], "prepare")


if __name__ == "__main__":
    unittest.main()
