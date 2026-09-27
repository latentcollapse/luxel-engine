"""Thin transport adapter for Rust-owned runtime asset preparation.

This module locates and invokes ``wge-asset-contract prepare``. It does not
parse the receipt or apply any acceptance policy; callers receive the native
process result unchanged.
"""

from __future__ import annotations

import os
import subprocess
from pathlib import Path
from typing import Sequence


def prepare_asset_runtime(
    asset_glb: str | os.PathLike[str],
    request_json: str | os.PathLike[str],
    *,
    executable: str | os.PathLike[str] | None = None,
    cwd: str | os.PathLike[str] | None = None,
    timeout_seconds: float | None = None,
) -> subprocess.CompletedProcess[str]:
    """Invoke Rust preparation and return its stdout, stderr, and exit status."""

    project_root = Path(__file__).resolve().parents[1]
    native = executable or os.environ.get("WGE_ASSET_CONTRACT_BIN")
    if native is None:
        native = project_root / "world_core" / "target" / "debug" / "wge-asset-contract"

    command: Sequence[str] = (
        str(native),
        "prepare",
        str(asset_glb),
        str(request_json),
    )
    return subprocess.run(
        command,
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
        timeout=timeout_seconds,
    )
