"""Transport contract tests for the canonical native transaction adapter."""

from __future__ import annotations

import subprocess
import unittest
from pathlib import Path

from pipeline.wge_native_transaction import (
    NativeTransactionCommands,
    NativeTransactionError,
    _run_json,
)


class NativeTransactionTransportTest(unittest.TestCase):
    def test_native_errors_are_not_reinterpreted(self) -> None:
        def runner(*args, **kwargs):
            return subprocess.CompletedProcess(
                args[0],
                2,
                stdout="",
                stderr="native authority rejected the candidate",
            )

        with self.assertRaisesRegex(NativeTransactionError, "native authority rejected"):
            _run_json(
                NativeTransactionCommands(Path("/tmp/wge-control-plane")),
                ["inspect-project", "/tmp/project"],
                runner=runner,
            )

    def test_non_object_native_result_is_rejected(self) -> None:
        def runner(*args, **kwargs):
            return subprocess.CompletedProcess(args[0], 0, stdout="[]", stderr="")

        with self.assertRaisesRegex(NativeTransactionError, "must be an object"):
            _run_json(
                NativeTransactionCommands(Path("/tmp/wge-control-plane")),
                ["profile", "engine-neutral"],
                runner=runner,
            )


if __name__ == "__main__":
    unittest.main()
