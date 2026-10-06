"""Transport-only orchestration for the canonical Rust Luxel transaction store.

The existing Python runtime builders may stage source bytes and invoke Julia or
Rust workers. This module is the compatibility seam after those workers finish:
it passes their typed ProjectSpec, candidate manifests, and certification
request to ``luxel-control-plane``. It deliberately does not hash artifacts,
interpret receipts, or decide whether a snapshot is promotable.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Mapping, Sequence


class NativeTransactionError(RuntimeError):
    """The native transaction rejected transport input or failed closed."""


@dataclass(frozen=True)
class NativeTransactionCommands:
    control_plane: Path


@dataclass(frozen=True)
class NativeTransactionResult:
    root: Path
    project: Mapping[str, Any]
    current_snapshot: Mapping[str, Any]
    validation: Mapping[str, Any]
    committed_snapshot: Mapping[str, Any]
    rollback_snapshot: Mapping[str, Any]
    playtest: Mapping[str, Any]
    capture: Mapping[str, Any]


def _run_json(
    commands: NativeTransactionCommands,
    args: Sequence[str],
    *,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> dict[str, Any]:
    try:
        result = runner(
            [str(commands.control_plane), *args],
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as error:
        raise NativeTransactionError(f"cannot execute native transaction: {error}") from error
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "native transaction failed").strip()
        raise NativeTransactionError(detail)
    try:
        value = json.loads(result.stdout)
    except (TypeError, json.JSONDecodeError) as error:
        raise NativeTransactionError(f"native transaction returned malformed JSON: {error}") from error
    if not isinstance(value, dict):
        raise NativeTransactionError("native transaction result must be an object")
    return value


def _run_status(
    commands: NativeTransactionCommands,
    args: Sequence[str],
    *,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> None:
    _run_json(commands, args, runner=runner)


def _candidate_id(manifest: Path) -> str:
    try:
        value = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise NativeTransactionError(f"cannot read candidate manifest {manifest}: {error}") from error
    candidate_id = value.get("snapshot_id") if isinstance(value, dict) else None
    if not isinstance(candidate_id, str) or not candidate_id:
        raise NativeTransactionError(f"candidate manifest {manifest} has no snapshot_id")
    return candidate_id


def promote_reproducible_output(
    *,
    commands: NativeTransactionCommands,
    transaction_root: Path,
    project_spec: Path,
    output_root: Path,
    before_manifest: Path,
    current_manifest: Path,
    certification_request: Path,
    current_candidate_id: str,
    world_artifact_id: str,
    profile: str = "engine-neutral",
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> NativeTransactionResult:
    """Promote a prepared native output through the Rust transaction API.

    The two candidates are imported as immutable evidence contexts. The final
    candidate is then revalidated, replayed, captured, committed, reopened,
    and rolled back to its certified history entry. All decisions remain in
    the Rust binary; this function only supplies argument vectors and returns
    its typed JSON results.
    """

    transaction_root = transaction_root.expanduser().resolve()
    project_spec = project_spec.expanduser().resolve(strict=True)
    output_root = output_root.expanduser().resolve(strict=True)
    before_manifest = before_manifest.expanduser().resolve(strict=True)
    current_manifest = current_manifest.expanduser().resolve(strict=True)
    certification_request = certification_request.expanduser().resolve(strict=True)
    if not transaction_root.parent.exists():
        transaction_root.parent.mkdir(parents=True, exist_ok=True)

    project = _run_json(
        commands,
        ["create", str(transaction_root), "--spec", str(project_spec), "--profile", profile],
        runner=runner,
    )
    _run_json(
        commands,
        [
            "build-candidate",
            str(transaction_root),
            "--manifest",
            str(before_manifest),
            "--artifact-root",
            str(output_root),
        ],
        runner=runner,
    )
    _run_json(
        commands,
        [
            "build-candidate",
            str(transaction_root),
            "--manifest",
            str(current_manifest),
            "--artifact-root",
            str(output_root),
        ],
        runner=runner,
    )
    for candidate in (_candidate_id(before_manifest), _candidate_id(current_manifest)):
        _run_status(
            commands,
            [
                "attach-evidence",
                str(transaction_root),
                "--candidate",
                candidate,
                "--request",
                str(certification_request),
            ],
            runner=runner,
        )
    validation = _run_json(
        commands,
        [
            "verify-candidate",
            str(transaction_root),
            "--candidate",
            current_candidate_id,
        ],
        runner=runner,
    )
    playtest = _run_json(
        commands,
        [
            "run-playtest",
            str(transaction_root),
            "--candidate",
            current_candidate_id,
            "--world-artifact",
            world_artifact_id,
        ],
        runner=runner,
    )
    capture = _run_json(
        commands,
        [
            "capture-evidence",
            str(transaction_root),
            "--candidate",
            current_candidate_id,
            "--world-artifact",
            world_artifact_id,
            "--output",
            "reference-capture",
        ],
        runner=runner,
    )
    committed = _run_json(
        commands,
        [
            "commit-candidate",
            str(transaction_root),
            "--candidate",
            current_candidate_id,
        ],
        runner=runner,
    )
    reopened = _run_json(
        commands,
        ["inspect-current", str(transaction_root)],
        runner=runner,
    )
    snapshot_id = committed.get("snapshot_id")
    if not isinstance(snapshot_id, str) or not snapshot_id:
        raise NativeTransactionError("native commit returned no certified snapshot id")
    rollback = _run_json(
        commands,
        ["rollback", str(transaction_root), "--snapshot", snapshot_id],
        runner=runner,
    )
    return NativeTransactionResult(
        root=transaction_root,
        project=project,
        current_snapshot=reopened,
        validation=validation,
        committed_snapshot=committed,
        rollback_snapshot=rollback,
        playtest=playtest,
        capture=capture,
    )
