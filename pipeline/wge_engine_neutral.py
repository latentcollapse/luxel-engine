"""Model-facing transport seam for the WGE engine-neutral certification path.

All semantic normalization, runtime measurements, receipt sealing, repair
assessment, candidate identity, and promotion decisions belong to native Rust
CLIs. This module only stages caller-owned inputs and transports their outputs.

The native CLIs own source intake, runtime measurement, candidate identity,
receipt sealing, repair evidence, and certification. Python only stages files,
constructs typed request envelopes, and transports native outputs.

Input directory contract:
  source-bundle-draft.json   Rust SourceBundleDraft
  intake-draft.json          Rust IntakeDraft, already bound to the prepared
                             source bundle ID
  provider-response.json     Rust ProviderInterpretation bytes
  layout.json                repaired/current AuthoredLayout
  before-layout.json         candidate layout with an expected visual failure
  static-mesh-source.json    typed static mesh source
  asset-package.json         typed static asset package
  repair-proposal-draft.json typed Rust RepairProposalDraft
  sources/<source_ref>       raw caller sources named by source-bundle draft

The source bundle must include a design_document source whose bytes are exactly
``layout.json``. Provider output is supplied by the caller; this seam does not
call a model or edit semantic claims.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Mapping, Sequence


ROOT = Path(__file__).resolve().parents[1]


class OrchestrationError(RuntimeError):
    """A native stage rejected input or produced an unexpected result."""


@dataclass(frozen=True)
class NativeCommands:
    intake: str
    runtime: str
    authority: str
    julia: str | None = None
    asset: str | None = None
    ledger: str | None = None


@dataclass(frozen=True)
class SmokeResult:
    output_dir: Path
    source_bundle: Path
    semantic_intake: Path
    before_bundle: Path
    current_bundle: Path
    proposal: Path
    certification_request: Path
    certification_report: Mapping[str, object]
    handoff_snapshot: Path


REQUIRED_INPUTS = (
    "source-bundle-draft.json",
    "intake-draft.json",
    "provider-response.json",
    "layout.json",
    "before-layout.json",
    "static-mesh-source.json",
    "asset-package.json",
    "repair-proposal-draft.json",
    "repair-delta-draft.json",
)

ENVELOPE_SCHEMA = "wge.certification-receipt-envelope/v1"
REQUEST_SCHEMA = "wge.certification-request/v1"
BAD_GLB_SHA256 = "sha256:858fa104880822d081405579fb5b39d533d3b3b341d38aa1490a44b634f5e2b4"
RUNTIME_ARTIFACTS = (
    ("world-artifact", "world_artifact", "world_artifact.json"),
    ("traversal-evidence", "traversal_evidence", "traversal_evidence.json"),
    ("gameplay-binding", "gameplay_world_binding", "gameplay_world_binding.json"),
    ("gameplay-trace", "gameplay_trace", "gameplay_trace.json"),
    ("reference-capture", "reference_capture_ppm", "reference_capture.ppm"),
    ("visual-evidence", "visual_evidence", "visual_evidence.json"),
)
REPAIR_ARTIFACT_KINDS = {"repair_proposal", "repair_delta", "repair_record"}


def _run(
    argv: Sequence[str],
    *,
    cwd: Path | None = None,
    expected_codes: Sequence[int] = (0,),
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> subprocess.CompletedProcess[str]:
    try:
        result = runner(
            list(argv),
            cwd=str(cwd) if cwd else None,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
    except OSError as error:
        raise OrchestrationError(f"cannot execute native command {argv[0]!r}: {error}") from error
    if result.returncode not in expected_codes:
        output = (result.stderr or result.stdout).strip()
        raise OrchestrationError(
            f"native command exited {result.returncode}: {' '.join(map(str, argv))}"
            + (f"\n{output}" if output else "")
        )
    return result


def _read_json(path: Path) -> object:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise OrchestrationError(f"cannot read native JSON output {path}: {error}") from error


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + "\n", encoding="utf-8")


def _write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def _digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def _stdout_json(result: subprocess.CompletedProcess[str], command: str) -> object:
    try:
        return json.loads(result.stdout)
    except (TypeError, json.JSONDecodeError) as error:
        raise OrchestrationError(f"native {command} did not emit JSON on stdout: {error}") from error


def _candidate_file(
    *, project_id: str, snapshot_id: str, artifact_root: str,
    artifact_dir: Path, authorized: Sequence[str], candidate_sha256: str = "",
) -> dict[str, object]:
    artifacts = []
    for item in sorted(artifact_dir.iterdir(), key=lambda path: path.name):
        if not item.is_file():
            continue
        artifact_id, separator, kind = item.name.partition("--")
        if not separator or not artifact_id or not kind:
            raise OrchestrationError(f"candidate artifact filename lacks id--kind: {item.name}")
        raw = item.read_bytes()
        artifacts.append({
            "artifact_id": artifact_id,
            "kind": kind,
            "path": item.name,
            "sha256": _digest(raw),
        })
    return {
        "project_id": project_id,
        "snapshot_id": snapshot_id,
        "candidate_sha256": candidate_sha256,
        "artifact_root": artifact_root,
        "authorized_repair_artifact_ids": sorted(set(authorized)),
        "artifacts": artifacts,
    }


def _candidate_id(
    commands: NativeCommands, candidate_path: Path, output_root: Path,
    runner: Callable[..., subprocess.CompletedProcess[str]],
) -> str:
    result = _run(
        [commands.authority, "candidate-id", str(candidate_path), "--artifact-root", str(output_root)],
        runner=runner,
    )
    if result.stdout.strip().startswith("{"):
        value = _stdout_json(result, "candidate-id")
        digest = value.get("candidate_sha256") if isinstance(value, dict) else None
    else:
        digest = result.stdout.strip()
    if not isinstance(digest, str) or not digest.startswith("sha256:"):
        raise OrchestrationError("native candidate-id did not return candidate_sha256")
    return digest


def _native_profile(
    commands: NativeCommands,
    profile: str,
    runner: Callable[..., subprocess.CompletedProcess[str]],
) -> tuple[list[dict[str, object]], dict[str, dict[str, str]]]:
    result = _run([commands.authority, "profile", profile], runner=runner)
    value = _stdout_json(result, "profile")
    if isinstance(value, list):
        gates = value
    elif isinstance(value, dict):
        gates = value.get("gates", value.get("profile"))
    else:
        gates = None
    if not isinstance(gates, list):
        raise OrchestrationError("native profile did not return a gate list")
    descriptors: dict[str, dict[str, str]] = {}
    for gate in gates:
        if not isinstance(gate, dict):
            raise OrchestrationError("native gate profile contains a malformed entry")
        gate_id = gate.get("gate_id")
        validator = gate.get("validator_id")
        schema = gate.get("receipt_schema")
        if not all(isinstance(item, str) and item for item in (gate_id, validator, schema)):
            raise OrchestrationError("native gate profile entry lacks registered receipt fields")
        descriptors[gate_id] = {"validator_id": validator, "receipt_schema": schema}
    if len(descriptors) != len(gates):
        raise OrchestrationError("native gate profile contains duplicate gate ids")
    return gates, descriptors


def _seal_receipt(
    commands: NativeCommands,
    output_root: Path,
    candidate: Mapping[str, object],
    descriptors: Mapping[str, Mapping[str, str]],
    gate_id: str,
    status: str,
    evidence_ids: Sequence[str],
    payload: Mapping[str, object],
    producer: str,
    runner: Callable[..., subprocess.CompletedProcess[str]],
) -> dict[str, object]:
    descriptor = descriptors.get(gate_id)
    if descriptor is None:
        raise OrchestrationError(f"native registry has no gate {gate_id!r}")
    artifacts = {item["artifact_id"]: item for item in candidate["artifacts"]}
    evidence = []
    for artifact_id in sorted(set(evidence_ids)):
        artifact = artifacts.get(artifact_id)
        if artifact is None:
            raise OrchestrationError(f"receipt evidence artifact is absent: {artifact_id}")
        evidence.append({
            "artifact_id": artifact_id,
            "kind": artifact["kind"],
            "sha256": artifact["sha256"],
        })
    envelope = {
        "schema_version": ENVELOPE_SCHEMA,
        "receipt_id": "",
        "project_id": candidate["project_id"],
        "snapshot_id": candidate["snapshot_id"],
        "candidate_sha256": candidate["candidate_sha256"],
        "gate_id": gate_id,
        "validator_id": descriptor["validator_id"],
        "receipt_schema": descriptor["receipt_schema"],
        "status": status,
        "producer": producer,
        "observed_input_sha256": "",
        "evidence": evidence,
        "payload": dict(payload),
    }
    draft = output_root / "receipt-drafts" / f"{candidate['snapshot_id']}--{gate_id}.json"
    sealed = output_root / "receipts" / f"{candidate['snapshot_id']}--{gate_id}.json"
    _write_json(draft, envelope)
    sealed.parent.mkdir(parents=True, exist_ok=True)
    _run([commands.authority, "seal", str(draft), str(sealed)], runner=runner)
    value = _read_json(sealed)
    if not isinstance(value, dict) or not value.get("receipt_id"):
        raise OrchestrationError(f"native seal did not return a typed receipt for {gate_id}")
    return value


def _repair_reference(
    commands: NativeCommands,
    receipt: Mapping[str, object],
    candidate_path: Path,
    output_root: Path,
    label: str,
    runner: Callable[..., subprocess.CompletedProcess[str]],
) -> tuple[dict[str, object], Path]:
    reference_path = output_root / "repair" / f"{label}-reference.json"
    bridge_path = output_root / "repair" / f"{label}-native-bridge.json"
    _write_json(reference_path, receipt)
    result = _run(
        [
            commands.authority,
            "repair-reference",
            str(reference_path),
            "--candidate",
            str(candidate_path),
            "--artifact-root",
            str(output_root),
            "--output-bridge",
            str(bridge_path),
        ],
        runner=runner,
    )
    value = _stdout_json(result, "repair-reference")
    if not isinstance(value, dict) or not bridge_path.is_file():
        raise OrchestrationError("native repair-reference did not emit a reference and bridge")
    return value, bridge_path


def _source_records(
    source_dir: Path,
    bundle: Mapping[str, object],
    draft: Mapping[str, object],
) -> list[tuple[str, Path]]:
    draft_by_ref = {
        source["source_ref"]: source
        for source in draft["sources"]
        if isinstance(source, dict) and isinstance(source.get("source_ref"), str)
    }
    result = []
    used_refs: set[str] = set()
    for record in bundle.get("sources", []):
        matches = []
        for ref, source in draft_by_ref.items():
            if ref in used_refs:
                continue
            raw_path = source_dir / "sources" / ref
            if (
                source.get("kind") == record.get("kind")
                and source.get("content_sha256") == record.get("content_sha256")
                and source.get("media_type") == record.get("media_type")
                and source.get("provenance") == record.get("provenance")
                and _digest(raw_path.read_bytes()) == record.get("content_sha256")
            ):
                matches.append((ref, raw_path))
        if len(matches) != 1:
            raise OrchestrationError(
                f"cannot uniquely bind native source record {record.get('source_id')!r} to caller bytes"
            )
        ref, path = matches[0]
        used_refs.add(ref)
        result.append((record["source_id"], path))
    if len(used_refs) != len(draft_by_ref):
        raise OrchestrationError("native source bundle omitted one or more caller source records")
    return sorted(result)


def _stage_candidate(
    source_dir: Path,
    source_bindings: Sequence[tuple[str, Path]],
    bundle_path: Path,
    intake_path: Path,
    runtime_dir: Path,
    layout_path: Path,
    artifact_dir: Path,
    extra_artifacts: Sequence[tuple[str, str, Path]] = (),
) -> None:
    artifact_dir.mkdir(parents=True, exist_ok=True)
    fixed = (
        ("source-bundle", "source_bundle", bundle_path),
        ("semantic-intake", "semantic_intake", intake_path),
        ("provider-response", "provider_response", source_dir / "provider-response.json"),
        ("static-mesh-source", "static_mesh_source", source_dir / "static-mesh-source.json"),
        ("asset-package", "asset_package", source_dir / "asset-package.json"),
        ("authored-layout", "authored_layout", layout_path),
    )
    for artifact_id, kind, path in fixed:
        _write_bytes(artifact_dir / f"{artifact_id}--{kind}", path.read_bytes())
    for index, (source_id, path) in enumerate(source_bindings):
        _write_bytes(artifact_dir / f"source-{index:03d}--source_bytes", path.read_bytes())
    for artifact_id, kind, filename in RUNTIME_ARTIFACTS:
        source = runtime_dir / filename
        if not source.is_file():
            raise OrchestrationError(f"reference runtime omitted required artifact {filename}")
        _write_bytes(artifact_dir / f"{artifact_id}--{kind}", source.read_bytes())
    for artifact_id, kind, source in extra_artifacts:
        if not source.is_file():
            raise OrchestrationError(f"additional candidate artifact is missing: {source}")
        _write_bytes(artifact_dir / f"{artifact_id}--{kind}", source.read_bytes())


def _candidate_path(
    output_root: Path,
    candidate: Mapping[str, object],
    name: str,
) -> Path:
    path = output_root / f"{name}-candidate.json"
    _write_json(path, candidate)
    return path


def _handoff_snapshot(output_root: Path, source_dir: Path, report: Mapping[str, object]) -> Path:
    snapshot = output_root / "handoff_snapshot"
    snapshot.mkdir(parents=True, exist_ok=True)
    source_target = snapshot / "source"
    evidence_target = snapshot / "evidence"
    for root, target, skip_output in (
        (source_dir, source_target, None),
        (output_root, evidence_target, snapshot),
    ):
        for path in sorted(root.rglob("*")):
            if not path.is_file():
                continue
            if skip_output is not None and (path == skip_output or skip_output in path.parents):
                continue
            relative = path.relative_to(root)
            _write_bytes(target / relative, path.read_bytes())
    files = []
    for path in sorted(snapshot.rglob("*")):
        if path.is_file() and path.name != "handoff-manifest.json":
            files.append({
                "path": path.relative_to(snapshot).as_posix(),
                "sha256": _digest(path.read_bytes()),
                "byte_length": path.stat().st_size,
            })
    manifest = {
        "schema_version": "wge.engine-neutral-handoff/v1",
        "project_id": report.get("project_id"),
        "snapshot_id": report.get("snapshot_id"),
        "candidate_sha256": report.get("candidate_sha256"),
        "certification_report_id": report.get("report_id"),
        "certification_status": report.get("status"),
        "runnable_bundle": "evidence/current",
        "files": files,
    }
    _write_json(snapshot / "handoff-manifest.json", manifest)
    archive = output_root / "handoff_snapshot.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for path in sorted(snapshot.rglob("*")):
            if not path.is_file():
                continue
            relative = path.relative_to(snapshot.parent).as_posix()
            info = zipfile.ZipInfo(relative, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            zf.writestr(info, path.read_bytes())
    return snapshot


def _source_bindings(source_dir: Path, draft_path: Path) -> list[str]:
    draft = _read_json(draft_path)
    if not isinstance(draft, dict) or not isinstance(draft.get("sources"), list):
        raise OrchestrationError("source-bundle-draft.json is not a typed source bundle draft")
    bindings: list[str] = []
    refs: set[str] = set()
    for source in draft["sources"]:
        if not isinstance(source, dict) or not isinstance(source.get("source_ref"), str):
            raise OrchestrationError("source bundle contains a malformed source reference")
        ref = source["source_ref"]
        if not ref or ref in refs or Path(ref).name != ref or ref in {".", ".."}:
            raise OrchestrationError(f"unsafe or duplicate source_ref: {ref!r}")
        refs.add(ref)
        source_path = source_dir / "sources" / ref
        if not source_path.is_file():
            raise OrchestrationError(f"caller source file is missing: {source_path}")
        bindings.append(f"{ref}={source_path}")
    if not bindings:
        raise OrchestrationError("source bundle must contain at least one caller source")
    return bindings


def _validate_inputs(source_dir: Path) -> None:
    missing = [name for name in REQUIRED_INPUTS if not (source_dir / name).is_file()]
    if missing:
        raise OrchestrationError("source directory is missing: " + ", ".join(missing))
    for name in REQUIRED_INPUTS:
        if (source_dir / name).stat().st_size == 0:
            raise OrchestrationError(f"caller input is empty: {name}")
    # The layout is itself an authored design source, so Rust intake can bind
    # the semantic specification to the exact world that is built.
    draft = _read_json(source_dir / "source-bundle-draft.json")
    layout_raw = (source_dir / "layout.json").read_bytes()
    design_sources = [
        source for source in draft["sources"]
        if source.get("kind") == "design_document" and source.get("source_ref")
    ]
    if not any(
        (source_dir / "sources" / source["source_ref"]).is_file()
        and (source_dir / "sources" / source["source_ref"]).read_bytes() == layout_raw
        for source in design_sources
    ):
        raise OrchestrationError(
            "source bundle must bind layout.json byte-for-byte as a design_document source"
        )


def _runtime_build(
    commands: NativeCommands,
    layout: Path,
    output: Path,
    *,
    expect_visual_failure: bool,
    runner: Callable[..., subprocess.CompletedProcess[str]],
) -> Mapping[str, object]:
    args = [commands.runtime, "build", "--layout", str(layout), "--output-dir", str(output)]
    if commands.julia:
        args.extend(("--julia", commands.julia))
    result = _run(args, expected_codes=(2,) if expect_visual_failure else (0,), runner=runner)
    report_path = output / "candidate_report.json"
    report = _read_json(report_path)
    if not isinstance(report, dict) or report.get("schema_version") != "wge.reference-runtime-candidate/v1":
        raise OrchestrationError(f"runtime emitted a malformed candidate report: {report_path}")
    expected_visual = "failed" if expect_visual_failure else "passed"
    if report.get("visual_status") != expected_visual:
        raise OrchestrationError(
            f"runtime visual status is {report.get('visual_status')!r}; expected {expected_visual!r}"
        )
    if not expect_visual_failure and report.get("reference_gates_passed") is not True:
        raise OrchestrationError("current reference-runtime candidate did not pass its native gates")
    # Preserve stdout/stderr only as transport diagnostics; they are not evidence.
    _ = result
    return report


def run_smoke(
    source_dir: Path,
    output_dir: Path,
    commands: NativeCommands,
    *,
    bad_glb: Path | None = None,
    profile: str = "engine-neutral",
    rigging_glb: Path | None = None,
    rigging_request: Path | None = None,
    project_id: str | None = None,
    project_template: Path | None = None,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> SmokeResult:
    """Run one complete source-to-certified engine-neutral vertical slice."""
    source_dir = source_dir.resolve(strict=True)
    output_dir = output_dir.resolve()
    if not source_dir.is_dir():
        raise OrchestrationError(f"source path is not a directory: {source_dir}")
    if output_dir == source_dir or source_dir in output_dir.parents:
        raise OrchestrationError("output directory must not be inside the caller source directory")
    if output_dir.exists() and any(output_dir.iterdir()):
        raise OrchestrationError(f"output directory must be empty for a reproducible run: {output_dir}")
    _validate_inputs(source_dir)
    bindings = _source_bindings(source_dir, source_dir / "source-bundle-draft.json")
    output_dir.mkdir(parents=True, exist_ok=True)

    if profile not in {"engine-neutral", "native-mvp"}:
        raise OrchestrationError(f"unsupported certification profile: {profile}")
    if profile == "native-mvp" and (rigging_glb is None or rigging_request is None):
        raise OrchestrationError("native-mvp certification requires a pinned rigging GLB and request")
    if project_id is None:
        project_id = "wge-native-mvp" if profile == "native-mvp" else "wge-engine-neutral"
    if not project_id.strip():
        raise OrchestrationError("certification project_id must not be empty")
    if project_template is not None:
        project_template = project_template.expanduser().resolve(strict=True)
        if commands.ledger is None:
            raise OrchestrationError("project-spec compilation requires the Rust project ledger CLI")
    gates, descriptors = _native_profile(commands, profile, runner)

    extra_artifacts: list[tuple[str, str, Path]] = []
    if rigging_glb is not None or rigging_request is not None:
        if rigging_glb is None or rigging_request is None:
            raise OrchestrationError("rigging GLB and preparation request must be supplied together")
        asset_cli = commands.asset or os.environ.get("WGE_ASSET_CONTRACT", "")
        if not asset_cli:
            asset_cli = str(ROOT / "world_core" / "target" / "debug" / "wge-asset-contract")
        rigging_glb = rigging_glb.expanduser().resolve(strict=True)
        rigging_request = rigging_request.expanduser().resolve(strict=True)
        rigging_dir = output_dir / "rigging"
        rigging_dir.mkdir(parents=True, exist_ok=True)
        pinned_request = rigging_dir / "request.json"
        _write_bytes(pinned_request, rigging_request.read_bytes())
        rigging_receipt_path = rigging_dir / "preparation-receipt.json"
        preparation = _run(
            [asset_cli, "prepare", str(rigging_glb), str(pinned_request)],
            expected_codes=(0,),
            runner=runner,
        )
        if not preparation.stdout.strip():
            raise OrchestrationError("asset contract returned no typed rigging receipt")
        _write_bytes(rigging_receipt_path, preparation.stdout.encode("utf-8"))
        receipt = _read_json(rigging_receipt_path)
        if not isinstance(receipt, dict) or receipt.get("status") != "ready":
            raise OrchestrationError("native-mvp rigging input did not produce a ready runtime package")
        extra_artifacts = [
            ("hero-glb", "rigging_glb", rigging_glb),
            ("hero-request", "rigging_request", pinned_request),
            ("hero-preparation", "rigging_preparation_receipt", rigging_receipt_path),
        ]

    intake_dir = output_dir / "intake"
    intake_dir.mkdir(parents=True, exist_ok=True)
    bundle_path = intake_dir / "source-bundle.json"
    _run(
        [commands.intake, "prepare-source-bundle", str(source_dir / "source-bundle-draft.json"), str(bundle_path), *bindings],
        runner=runner,
    )
    bundle_for_bindings = _read_json(bundle_path)
    source_draft_for_bindings = _read_json(source_dir / "source-bundle-draft.json")
    if not isinstance(bundle_for_bindings, dict) or not isinstance(source_draft_for_bindings, dict):
        raise OrchestrationError("native source bundle inputs are not JSON objects")
    draft_by_ref = {
        item["source_ref"]: item
        for item in source_draft_for_bindings.get("sources", [])
        if isinstance(item, dict) and isinstance(item.get("source_ref"), str)
    }
    source_id_bindings: list[str] = []
    for record in bundle_for_bindings.get("sources", []):
        if not isinstance(record, dict):
            raise OrchestrationError("native source bundle contains a malformed source record")
        matches = [
            ref
            for ref, draft_item in draft_by_ref.items()
            if draft_item.get("content_sha256") == record.get("content_sha256")
            and draft_item.get("kind") == record.get("kind")
            and draft_item.get("media_type") == record.get("media_type")
            and draft_item.get("provenance") == record.get("provenance")
        ]
        if len(matches) != 1:
            raise OrchestrationError(
                f"cannot bind native source record {record.get('source_id')!r} to one source_ref"
            )
        source_id_bindings.append(
            f"{record['source_id']}={source_dir / 'sources' / matches[0]}"
        )
    _run(
        [commands.intake, "validate-source-bundle", str(bundle_path), *source_id_bindings],
        runner=runner,
    )
    intake_path = intake_dir / "semantic-intake.json"
    _run(
        [
            commands.intake,
            "normalize-intake",
            str(source_dir / "intake-draft.json"),
            str(bundle_path),
            str(source_dir / "provider-response.json"),
            str(intake_path),
            *source_id_bindings,
        ],
        runner=runner,
    )
    _run(
        [commands.intake, "validate-intake", str(intake_path), str(source_dir / "provider-response.json"), *source_id_bindings],
        runner=runner,
    )

    project_spec_path: Path | None = None
    if project_template is not None:
        project_spec_path = output_dir / "project_spec.json"
        compile_result = _run(
            [
                commands.ledger,
                "compile-spec",
                str(intake_path),
                str(project_template),
                "--output",
                str(project_spec_path),
            ],
            runner=runner,
        )
        try:
            compile_summary = json.loads(compile_result.stdout)
        except json.JSONDecodeError as error:
            raise OrchestrationError(f"project ledger emitted non-JSON compile output: {error}") from error
        if not isinstance(compile_summary, dict) or compile_summary.get("status") != "compiled":
            raise OrchestrationError("project ledger did not report a compiled project spec")
        extra_artifacts.extend(
            [
                ("project-spec", "project_spec", project_spec_path),
                ("project-template", "project_template", project_template),
            ]
        )

    bundle = _read_json(bundle_path)
    draft = _read_json(source_dir / "source-bundle-draft.json")
    if not isinstance(bundle, dict) or not isinstance(draft, dict):
        raise OrchestrationError("native intake outputs are not JSON objects")
    source_records = _source_records(source_dir, bundle, draft)

    before_dir = output_dir / "before"
    current_dir = output_dir / "current"
    before_report = _runtime_build(
        commands,
        source_dir / "before-layout.json",
        before_dir,
        expect_visual_failure=True,
        runner=runner,
    )
    current_report = _runtime_build(
        commands,
        source_dir / "layout.json",
        current_dir,
        expect_visual_failure=False,
        runner=runner,
    )
    _run([commands.runtime, "verify", "--bundle", str(current_dir)], runner=runner)

    deferred_gate_ids = (
        ["unity_import", "unity_build", "unity_playthrough"]
        if profile == "native-mvp"
        else ["rigging", "unity_import", "unity_build", "unity_playthrough"]
    )
    project_manifest = {
        "schema_version": "wge.project-manifest/v1",
        "project_id": project_id,
        "source_bundle_id": bundle.get("source_bundle_id"),
        "semantic_intake_id": _read_json(intake_path).get("intake_id"),
        "scope": "native_mvp_vertical_slice" if profile == "native-mvp" else "engine_neutral_vertical_slice",
        "deferred_gates": deferred_gate_ids,
    }
    manifest_path = output_dir / "project-manifest.json"
    _write_json(manifest_path, project_manifest)

    before_artifact_dir = before_dir / "artifacts"
    current_artifact_dir = current_dir / "artifacts"
    _stage_candidate(
        source_dir,
        source_records,
        bundle_path,
        intake_path,
        before_dir,
        source_dir / "before-layout.json",
        before_artifact_dir,
        extra_artifacts,
    )
    _stage_candidate(
        source_dir,
        source_records,
        bundle_path,
        intake_path,
        current_dir,
        source_dir / "layout.json",
        current_artifact_dir,
        extra_artifacts,
    )
    for artifact_dir in (before_artifact_dir, current_artifact_dir):
        _write_bytes(
            artifact_dir / "project-manifest--project_manifest",
            manifest_path.read_bytes(),
        )

    proposal_template = _read_json(source_dir / "repair-proposal-draft.json")
    if not isinstance(proposal_template, dict):
        raise OrchestrationError("repair-proposal-draft.json must be a JSON object")
    target_items = proposal_template.get("authorized_targets")
    if not isinstance(target_items, list) or not target_items:
        raise OrchestrationError(
            "repair-proposal-draft.json must explicitly authorize its changed artifact IDs"
        )
    target_ids: list[str] = []
    for item in target_items:
        if not isinstance(item, dict) or not isinstance(item.get("artifact_id"), str):
            raise OrchestrationError("repair proposal contains a malformed authorized target")
        target_ids.append(item["artifact_id"])
    if len(set(target_ids)) != len(target_ids):
        raise OrchestrationError("repair proposal contains duplicate authorized targets")
    target_ids = sorted(target_ids)

    before_candidate = _candidate_file(
        project_id=str(project_manifest.get("project_id", "wge-engine-neutral")),
        snapshot_id="before-visual-repair",
        artifact_root="before/artifacts",
        artifact_dir=before_artifact_dir,
        authorized=(),
    )
    current_candidate = _candidate_file(
        project_id=str(project_manifest.get("project_id", "wge-engine-neutral")),
        snapshot_id="current-certified",
        artifact_root="current/artifacts",
        artifact_dir=current_artifact_dir,
        authorized=target_ids,
    )
    before_candidate_path = _candidate_path(output_dir, before_candidate, "before")
    before_candidate["candidate_sha256"] = _candidate_id(
        commands, before_candidate_path, output_dir, runner
    )
    before_candidate_path = _candidate_path(output_dir, before_candidate, "before")
    current_candidate_path = _candidate_path(output_dir, current_candidate, "current")
    current_candidate["candidate_sha256"] = _candidate_id(
        commands, current_candidate_path, output_dir, runner
    )
    current_candidate_path = _candidate_path(output_dir, current_candidate, "current")

    def artifact_map(candidate: Mapping[str, object]) -> dict[str, Mapping[str, object]]:
        return {item["artifact_id"]: item for item in candidate["artifacts"]}

    before_map = artifact_map(before_candidate)
    current_map = artifact_map(current_candidate)
    changed_ids = sorted(
        artifact_id
        for artifact_id in set(before_map) | set(current_map)
        if artifact_id in before_map
        and artifact_id in current_map
        and (
            before_map[artifact_id]["sha256"] != current_map[artifact_id]["sha256"]
            or before_map[artifact_id]["kind"] != current_map[artifact_id]["kind"]
        )
    )
    if changed_ids != target_ids:
        raise OrchestrationError(
            "repair proposal target set does not match the actual before/after runtime artifact change set: "
            f"declared={target_ids!r} actual={changed_ids!r}"
        )

    def runtime_ids(candidate: Mapping[str, object]) -> dict[str, str]:
        available = artifact_map(candidate)
        expected = {
            "world": "world-artifact",
            "traversal": "traversal-evidence",
            "gameplay": "gameplay-binding",
            "capture": "reference-capture",
            "visual": "visual-evidence",
        }
        for artifact_id in expected.values():
            if artifact_id not in available:
                raise OrchestrationError(f"candidate is missing runtime artifact {artifact_id}")
        return expected

    before_runtime = runtime_ids(before_candidate)
    current_runtime = runtime_ids(current_candidate)

    receipt_producer = (
        "wge-native-mvp-orchestrator"
        if profile == "native-mvp"
        else "wge-engine-neutral-orchestrator"
    )

    before_visual_payload = {
        "world_artifact_id": before_runtime["world"],
        "capture_artifact_id": before_runtime["capture"],
        "visual_evidence_artifact_id": before_runtime["visual"],
    }
    before_visual = _seal_receipt(
        commands,
        output_dir,
        before_candidate,
        descriptors,
        "visual",
        "fail",
        list(before_visual_payload.values()),
        before_visual_payload,
        receipt_producer,
        runner,
    )

    semantic_payload = {
        "intake_artifact_id": "semantic-intake",
        "provider_response_artifact_id": "provider-response",
        "source_bundle_artifact_id": "source-bundle",
        "layout_artifact_id": "authored-layout",
        "project_spec_artifact_id": "project-spec" if project_spec_path is not None else None,
        "project_template_artifact_id": "project-template" if project_spec_path is not None else None,
        "source_artifacts": [
            {"source_id": source_id, "artifact_id": f"source-{index:03d}"}
            for index, (source_id, _path) in enumerate(source_records)
        ],
    }
    semantic_evidence = [
        "semantic-intake",
        "provider-response",
        "source-bundle",
        "authored-layout",
        *[f"source-{index:03d}" for index in range(len(source_records))],
    ]
    if project_spec_path is not None:
        semantic_evidence.extend(["project-spec", "project-template"])
    current_receipts: list[dict[str, object]] = []
    current_receipts.append(
        _seal_receipt(
            commands,
            output_dir,
            current_candidate,
            descriptors,
            "semantic",
            "pass",
            semantic_evidence,
            semantic_payload,
            receipt_producer,
            runner,
        )
    )
    if profile == "native-mvp":
        rigging_payload = {
            "source_glb_artifact_id": "hero-glb",
            "preparation_request_artifact_id": "hero-request",
            "preparation_receipt_artifact_id": "hero-preparation",
        }
        current_receipts.append(
            _seal_receipt(
                commands,
                output_dir,
                current_candidate,
                descriptors,
                "rigging",
                "pass",
                list(rigging_payload.values()),
                rigging_payload,
                receipt_producer,
                runner,
            )
        )
    world_payload = {
        "world_artifact_id": current_runtime["world"],
        "traversal_artifact_id": current_runtime["traversal"],
        "layout_artifact_id": "authored-layout",
    }
    current_receipts.append(
        _seal_receipt(
            commands,
            output_dir,
            current_candidate,
            descriptors,
            "world",
            "pass",
            list(world_payload.values()),
            world_payload,
            receipt_producer,
            runner,
        )
    )
    gameplay_payload = {
        "world_artifact_id": current_runtime["world"],
        "traversal_artifact_id": current_runtime["traversal"],
        "capture_artifact_id": current_runtime["capture"],
        "visual_evidence_artifact_id": current_runtime["visual"],
        "gameplay_binding_artifact_id": current_runtime["gameplay"],
    }
    current_receipts.append(
        _seal_receipt(
            commands,
            output_dir,
            current_candidate,
            descriptors,
            "gameplay",
            "pass",
            list(gameplay_payload.values()),
            gameplay_payload,
            receipt_producer,
            runner,
        )
    )
    package = _read_json(source_dir / "asset-package.json")
    if not isinstance(package, dict) or not isinstance(package.get("asset_use"), str):
        raise OrchestrationError("asset-package.json must declare a typed asset_use")
    asset_payload = {
        "source_artifact_id": "static-mesh-source",
        "package_artifact_id": "asset-package",
        "asset_use": package["asset_use"],
    }
    current_receipts.append(
        _seal_receipt(
            commands,
            output_dir,
            current_candidate,
            descriptors,
            "asset",
            "pass",
            ["static-mesh-source", "asset-package"],
            asset_payload,
            receipt_producer,
            runner,
        )
    )
    current_visual_payload = {
        "world_artifact_id": current_runtime["world"],
        "capture_artifact_id": current_runtime["capture"],
        "visual_evidence_artifact_id": current_runtime["visual"],
    }
    current_visual = _seal_receipt(
        commands,
        output_dir,
        current_candidate,
        descriptors,
        "visual",
        "pass",
        list(current_visual_payload.values()),
        current_visual_payload,
        receipt_producer,
        runner,
    )
    current_receipts.append(current_visual)
    for gate_id in deferred_gate_ids:
        deferred_payload = {
            "reason_code": "deferred_by_scope",
            "deferral_scope": gate_id,
            "detail": (
                "This native MVP scope explicitly defers Unity integration."
                if profile == "native-mvp"
                else "This engine-neutral scope explicitly defers character rigging and Unity integration."
            ),
        }
        current_receipts.append(
            _seal_receipt(
                commands,
                output_dir,
                current_candidate,
                descriptors,
                gate_id,
                "indeterminate",
                ["project-manifest"],
                deferred_payload,
                receipt_producer,
                runner,
            )
        )

    before_reference, _before_bridge = _repair_reference(
        commands, before_visual, before_candidate_path, output_dir, "before", runner
    )
    after_reference, _after_bridge = _repair_reference(
        commands, current_visual, current_candidate_path, output_dir, "after", runner
    )
    proposal_draft = dict(proposal_template)
    proposal_draft["candidate_before_sha256"] = before_candidate["candidate_sha256"]
    proposal_draft["failure_evidence"] = before_reference
    proposal_draft["authorized_targets"] = [
        {
            "artifact_id": artifact_id,
            "before_sha256": before_map[artifact_id]["sha256"],
        }
        for artifact_id in target_ids
    ]
    bound_proposal_draft_path = output_dir / "repair-proposal-draft-bound.json"
    _write_json(bound_proposal_draft_path, proposal_draft)
    proposal_path = output_dir / "repair-proposal.json"
    _run(
        [
            commands.intake,
            "normalize-repair-proposal", str(bound_proposal_draft_path),
            str(proposal_path),
        ],
        runner=runner,
    )
    _run([commands.intake, "validate-repair-proposal", str(proposal_path)], runner=runner)

    proposal = _read_json(proposal_path)
    delta_template = _read_json(source_dir / "repair-delta-draft.json")
    if not isinstance(proposal, dict) or not isinstance(delta_template, dict):
        raise OrchestrationError("native repair proposal or delta draft is not a JSON object")

    # The proposed after-layout is only a model-supplied candidate until the
    # Rust repair contract authorizes and applies it. Rebuild the runtime from
    # the bytes emitted by that native application step, then restage the
    # candidate that will feed the authority plane.
    applied_layout_path = output_dir / "repair" / "applied-layout.json"
    application_path = output_dir / "repair" / "application.json"
    _run(
        [
            commands.intake,
            "apply-repair",
            str(source_dir / "before-layout.json"),
            str(proposal_path),
            str(source_dir / "layout.json"),
            "--output",
            str(applied_layout_path),
            "--receipt",
            str(application_path),
        ],
        runner=runner,
    )
    if applied_layout_path.read_bytes() != (source_dir / "layout.json").read_bytes():
        raise OrchestrationError("native repair application changed the proposed layout bytes")
    rebuilt_dir = output_dir / "repair" / "rebuilt-current"
    rebuilt_report = _runtime_build(
        commands,
        applied_layout_path,
        rebuilt_dir,
        expect_visual_failure=False,
        runner=runner,
    )
    _run([commands.runtime, "verify", "--bundle", str(rebuilt_dir)], runner=runner)
    for path in sorted(rebuilt_dir.rglob("*")):
        if path.is_file():
            _write_bytes(current_dir / path.relative_to(rebuilt_dir), path.read_bytes())
    _stage_candidate(
        source_dir,
        source_records,
        bundle_path,
        intake_path,
        current_dir,
        applied_layout_path,
        current_artifact_dir,
        extra_artifacts,
    )
    _write_bytes(current_artifact_dir / "project-manifest--project_manifest", manifest_path.read_bytes())
    previous_current_sha256 = current_candidate["candidate_sha256"]
    current_candidate = _candidate_file(
        project_id=str(project_manifest.get("project_id", "wge-engine-neutral")),
        snapshot_id="current-certified",
        artifact_root="current/artifacts",
        artifact_dir=current_artifact_dir,
        authorized=target_ids,
    )
    current_candidate_path = _candidate_path(output_dir, current_candidate, "current")
    current_candidate["candidate_sha256"] = _candidate_id(
        commands, current_candidate_path, output_dir, runner
    )
    current_candidate_path = _candidate_path(output_dir, current_candidate, "current")
    if current_candidate["candidate_sha256"] != previous_current_sha256:
        raise OrchestrationError("native repair application changed the certified candidate unexpectedly")
    current_map = artifact_map(current_candidate)
    changed_ids = sorted(
        artifact_id
        for artifact_id in set(before_map) | set(current_map)
        if artifact_id in before_map
        and artifact_id in current_map
        and (
            before_map[artifact_id]["sha256"] != current_map[artifact_id]["sha256"]
            or before_map[artifact_id]["kind"] != current_map[artifact_id]["kind"]
        )
    )
    if changed_ids != target_ids:
        raise OrchestrationError(
            "native repair application changed an unauthorized artifact set: "
            f"declared={target_ids!r} actual={changed_ids!r}"
        )
    current_report = rebuilt_report
    delta_draft = dict(delta_template)
    delta_draft["proposal_id"] = proposal["proposal_id"]
    delta_draft["candidate_before_sha256"] = before_candidate["candidate_sha256"]
    delta_draft["candidate_after_sha256"] = current_candidate["candidate_sha256"]
    delta_draft["before_evidence"] = before_reference
    delta_draft["after_evidence"] = after_reference
    changed_template = delta_template.get("changed_artifacts")
    if not isinstance(changed_template, list) or not changed_template:
        raise OrchestrationError(
            "repair-delta-draft.json must explicitly enumerate changed artifact IDs"
        )
    changed_artifacts = []
    for change in changed_template:
        if not isinstance(change, dict) or not isinstance(change.get("artifact_id"), str):
            raise OrchestrationError("repair delta draft contains a malformed artifact change")
        artifact_id = change["artifact_id"]
        if artifact_id not in before_map or artifact_id not in current_map:
            raise OrchestrationError(f"repair delta names unknown artifact {artifact_id}")
        changed_artifacts.append({
            "artifact_id": artifact_id,
            "before_sha256": before_map[artifact_id]["sha256"],
            "after_sha256": current_map[artifact_id]["sha256"],
        })
    delta_draft["changed_artifacts"] = changed_artifacts
    delta_draft_path = output_dir / "repair-delta-draft-bound.json"
    _write_json(delta_draft_path, delta_draft)
    delta_request = {
        "schema_version": "wge.repair-delta-request/v1",
        "before_candidate": before_candidate_path.name,
        "after_candidate": current_candidate_path.name,
        "before_receipt": str(Path("repair") / "before-reference.json"),
        "after_receipt": str(Path("repair") / "after-reference.json"),
        "proposal": proposal_path.name,
        "delta_draft": delta_draft_path.name,
    }
    # The native repair-reference command consumes the sealed receipt, while
    # repair-delta consumes the receipt envelope itself. Keep those JSON files
    # separate from the native bridge artifacts.
    _write_json(output_dir / "repair" / "before-reference.json", before_visual)
    _write_json(output_dir / "repair" / "after-reference.json", current_visual)
    delta_request_path = output_dir / "repair-delta-request.json"
    _write_json(delta_request_path, delta_request)
    delta_path = output_dir / "repair-record.json"
    _run(
        [
            commands.authority,
            "repair-delta",
            str(delta_request_path),
            "--artifact-root",
            str(output_dir),
            "--output",
            str(delta_path),
        ],
        runner=runner,
    )
    delta = _read_json(delta_path)

    _write_bytes(current_artifact_dir / "repair-proposal--repair_proposal", proposal_path.read_bytes())
    _write_bytes(current_artifact_dir / "repair-delta-draft--repair_delta", delta_draft_path.read_bytes())
    _write_bytes(current_artifact_dir / "repair-record--repair_record", delta_path.read_bytes())
    current_candidate = _candidate_file(
        project_id=current_candidate["project_id"],
        snapshot_id=current_candidate["snapshot_id"],
        artifact_root=current_candidate["artifact_root"],
        artifact_dir=current_artifact_dir,
        authorized=target_ids,
        candidate_sha256=current_candidate["candidate_sha256"],
    )
    current_candidate_path = _candidate_path(output_dir, current_candidate, "current")
    if _candidate_id(commands, current_candidate_path, output_dir, runner) != current_candidate["candidate_sha256"]:
        raise OrchestrationError("repair metadata changed the content candidate identity")

    repair_payload = {
        "proposal_artifact_id": "repair-proposal",
        "delta_draft_artifact_id": "repair-delta-draft",
        "delta_artifact_id": "repair-record",
        "before_snapshot_id": before_candidate["snapshot_id"],
        "before_receipt_id": before_visual["receipt_id"],
        "after_receipt_id": current_visual["receipt_id"],
        "proposal": proposal,
        "delta_draft": delta_draft,
        "delta": delta,
    }
    repair_receipt = _seal_receipt(
        commands,
        output_dir,
        current_candidate,
        descriptors,
        "repair",
        "pass",
        ["repair-proposal", "repair-delta-draft", "repair-record"],
        repair_payload,
        receipt_producer,
        runner,
    )

    if bad_glb is not None:
        bad_glb = bad_glb.expanduser().resolve()
        if not bad_glb.is_file():
            raise OrchestrationError(f"negative-control GLB is missing: {bad_glb}")
        bad_bytes = bad_glb.read_bytes()
        if _digest(bad_bytes) != BAD_GLB_SHA256:
            raise OrchestrationError("supplied negative-control GLB bytes do not match the pinned rejection control")
        _write_bytes(output_dir / "negative_controls" / bad_glb.name, bad_bytes)

    request = {
        "schema_version": REQUEST_SCHEMA,
        "current_snapshot_id": current_candidate["snapshot_id"],
        "candidates": [before_candidate, current_candidate],
        "gates": gates,
        "receipts": [before_visual, *current_receipts, repair_receipt],
    }
    request_path = output_dir / "certification-request.json"
    _write_json(request_path, request)
    validate_result = _run(
        [
            commands.authority,
            "validate",
            str(request_path),
            "--artifact-root",
            str(output_dir),
            "--profile",
            profile,
        ],
        runner=runner,
    )
    report = _stdout_json(validate_result, "certification validate")
    if not isinstance(report, dict):
        raise OrchestrationError("native certification report is not a JSON object")
    report_path = output_dir / "certification-report.json"
    _write_json(report_path, report)
    expected_status = "native_mvp_certified" if profile == "native-mvp" else "engine_neutral_certified"
    if report.get("status") != expected_status:
        raise OrchestrationError(f"native certification did not pass: {report.get('reasons')}")

    stage_summary = {
        "schema_version": "wge.engine-neutral-orchestration-stages/v1",
        "completed_native_stages": [
            "source_bundle_prepare_and_validate",
            "semantic_intake_normalize_and_validate",
            "before_reference_runtime_visual_failure",
            "current_reference_runtime_and_verify",
            "candidate_identity_native_revalidation",
            "native_receipt_sealing",
            "native_typed_repair_reference_and_delta",
            "native_typed_repair_application_and_rebuild",
            "native_certification_request_and_revalidation",
            *(["native_mvp_rigging_revalidation"] if profile == "native-mvp" else []),
        ],
        "before_visual_status": before_report.get("visual_status"),
        "current_visual_status": current_report.get("visual_status"),
        "bad_glb_sha256": _digest((output_dir / "negative_controls" / bad_glb.name).read_bytes()) if bad_glb is not None else None,
        "certification_profile": profile,
        "certification_status": report.get("status"),
        "certification_report_id": report.get("report_id"),
    }
    _write_json(output_dir / "native-stage-summary.json", stage_summary)
    handoff = _handoff_snapshot(output_dir, source_dir, report)
    return SmokeResult(
        output_dir,
        bundle_path,
        intake_path,
        before_dir,
        current_dir,
        proposal_path,
        request_path,
        report,
        handoff,
    )

def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source_dir", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("--intake-cli", default=os.environ.get("WGE_INTAKE_CLI", "wge-intake-repair"))
    parser.add_argument("--runtime-cli", default=os.environ.get("WGE_RUNTIME_CLI", "wge-reference-runtime"))
    parser.add_argument("--authority-cli", default=os.environ.get("WGE_AUTHORITY_CLI", "wge-certification-authority"))
    parser.add_argument("--julia", default=os.environ.get("WGE_JULIA"))
    parser.add_argument(
        "--bad-glb",
        type=Path,
        default=Path(
            os.environ.get(
                "WGE_NEGATIVE_CONTROL_GLB",
                "/home/mattc/Pictures/Generated 2D Images/sample_2026-09-26T091412.074.glb",
            )
        ),
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        run_smoke(
            args.source_dir,
            args.output_dir,
            NativeCommands(args.intake_cli, args.runtime_cli, args.authority_cli, args.julia),
            bad_glb=args.bad_glb,
        )
    except OrchestrationError as error:
        print(str(error), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
