"""Model-facing WGE native MVP transaction.

This is deliberately a small orchestration surface.  Rust owns source intake,
project-spec compilation, world/gameplay/visual measurement, receipt
promotion, repair validation, candidate identity, and certification.  Python
only transports caller-owned inputs and packages the native outputs into a
deterministic runnable handoff.

The transaction is intentionally two-phase at the semantic boundary:

1. a model/provider supplies a fresh source-bound interpretation and the
   typed world/assets inputs consumed by the native runtime;
2. after the native candidate exists, a typed project template binds the
   candidate artifacts and design decisions into the canonical project spec.

The template is never rewritten to fit the intake.  The Rust compiler rejects
stale intake pins, missing sources, dropped evidence, and malformed artifact
graphs.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path
from pathlib import PurePosixPath
from typing import Mapping, Sequence

from pipeline.wge_engine_neutral import BAD_GLB_SHA256, NativeCommands, OrchestrationError, run_smoke


ROOT = Path(__file__).resolve().parents[1]
WORLD_CORE = ROOT / "world_core"
DEFAULT_BAD_GLB = Path(
    os.environ.get(
        "WGE_NEGATIVE_CONTROL_GLB",
        "/home/mattc/Pictures/Generated 2D Images/sample_2026-09-26T091412.074.glb",
    )
)


@dataclass(frozen=True)
class NativeMvpCommands:
    intake: str
    runtime: str
    authority: str
    ledger: str
    julia: str | None = None
    asset: str | None = None

    @classmethod
    def from_environment(cls) -> "NativeMvpCommands":
        target = WORLD_CORE / "target" / "debug"
        return cls(
            os.environ.get("WGE_INTAKE_CLI", str(target / "wge-intake-repair")),
            os.environ.get("WGE_RUNTIME_CLI", str(target / "wge-reference-runtime")),
            os.environ.get("WGE_AUTHORITY_CLI", str(target / "wge-certification-authority")),
            os.environ.get("WGE_LEDGER_CLI", str(target / "wge-project-ledger")),
            os.environ.get("WGE_JULIA"),
            os.environ.get("WGE_ASSET_CONTRACT", str(target / "wge-asset-contract")),
        )

    def smoke(self) -> NativeCommands:
        return NativeCommands(self.intake, self.runtime, self.authority, self.julia, self.asset, self.ledger)


@dataclass(frozen=True)
class NativeMvpResult:
    output_dir: Path
    spec_path: Path
    template_path: Path
    snapshot_dir: Path
    snapshot_zip: Path
    snapshot_sha256: str
    certification_report: Mapping[str, object]


def _digest_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def _digest_file(path: Path) -> str:
    return _digest_bytes(path.read_bytes())


def _canonical(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n",
        encoding="utf-8",
    )


def _run(command: Sequence[str], *, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(
            list(command),
            cwd=str(cwd) if cwd else None,
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as error:
        raise OrchestrationError(f"cannot execute native command {command[0]!r}: {error}") from error
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise OrchestrationError(
            f"native command exited {result.returncode}: {' '.join(command)}"
            + (f"\n{detail}" if detail else "")
        )
    return result


def _copy_tree(source: Path, destination: Path) -> None:
    if not source.is_dir():
        raise OrchestrationError(f"handoff source directory is missing: {source}")
    for path in sorted(source.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(source)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)


def _copy_file(source: Path, destination: Path) -> None:
    if not source.is_file():
        raise OrchestrationError(f"handoff file is missing: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)


def _compile_spec(
    commands: NativeMvpCommands,
    intake_path: Path,
    template_path: Path,
    output_path: Path,
) -> dict[str, object]:
    result = _run(
        [
            commands.ledger,
            "compile-spec",
            str(intake_path),
            str(template_path),
            "--output",
            str(output_path),
        ]
    )
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise OrchestrationError(f"project ledger emitted non-JSON compile output: {error}") from error
    if not isinstance(value, dict) or value.get("status") != "compiled":
        raise OrchestrationError("project ledger did not report a compiled project spec")
    return value


def _validate_spec(commands: NativeMvpCommands, spec_path: Path) -> dict[str, object]:
    result = _run([commands.ledger, "validate-spec", str(spec_path)])
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise OrchestrationError(f"project ledger emitted non-JSON validation output: {error}") from error
    if not isinstance(value, dict) or value.get("status") != "valid":
        raise OrchestrationError("project ledger did not independently validate the project spec")
    return value


def _snapshot_manifest(snapshot_dir: Path, *, project_spec: Path, report: Mapping[str, object]) -> dict[str, object]:
    files = []
    for path in sorted(snapshot_dir.rglob("*")):
        if not path.is_file() or path.name == "snapshot-manifest.json":
            continue
        files.append(
            {
                "path": path.relative_to(snapshot_dir).as_posix(),
                "sha256": _digest_file(path),
                "byte_length": path.stat().st_size,
            }
        )
    return {
        "schema_version": "wge.native-mvp-snapshot/v1",
        "project_id": report.get("project_id"),
        "snapshot_id": report.get("snapshot_id"),
        "candidate_sha256": report.get("candidate_sha256"),
        "certification_report_id": report.get("report_id"),
        "certification_status": report.get("status"),
        "project_spec_sha256": _digest_file(project_spec),
        "runnable": {
            "runtime": "wge-reference-runtime",
            "operation": "verify",
            "bundle": "native/current",
        },
        "deferred_gates": report.get("deferred_gates", []),
        "files": files,
    }


def _seal_snapshot_manifest(snapshot_dir: Path, *, project_spec: Path, report: Mapping[str, object]) -> str:
    manifest = _snapshot_manifest(snapshot_dir, project_spec=project_spec, report=report)
    manifest_without_digest = dict(manifest)
    manifest_without_digest["snapshot_sha256"] = ""
    snapshot_sha256 = _digest_bytes(_canonical(manifest_without_digest))
    manifest["snapshot_sha256"] = snapshot_sha256
    _write_json(snapshot_dir / "snapshot-manifest.json", manifest)
    return snapshot_sha256


def _zip_snapshot(snapshot_dir: Path, output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path in sorted(snapshot_dir.rglob("*")):
            if not path.is_file():
                continue
            info = zipfile.ZipInfo(
                path.relative_to(snapshot_dir).as_posix(),
                date_time=(1980, 1, 1, 0, 0, 0),
            )
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, path.read_bytes())


def _archive_member_path(name: str) -> PurePosixPath:
    """Return a strictly relative archive path or reject it."""

    if not name or "\x00" in name or "\\" in name or name.endswith("/"):
        raise OrchestrationError(f"native MVP snapshot archive member path is unsafe: {name!r}")
    path = PurePosixPath(name)
    if (
        path.is_absolute()
        or path.as_posix() != name
        or any(part in {"", ".", ".."} for part in path.parts)
        or (len(path.parts[0]) >= 2 and path.parts[0][1] == ":")
    ):
        raise OrchestrationError(f"native MVP snapshot archive member path is unsafe: {name!r}")
    return path


def _extract_snapshot_archive(archive_path: Path, destination: Path) -> Path:
    """Extract a generated snapshot without delegating path policy to zipfile."""

    if not archive_path.is_file():
        raise OrchestrationError(f"native MVP snapshot archive is missing: {archive_path}")
    destination.mkdir(parents=True, exist_ok=True)
    try:
        with zipfile.ZipFile(archive_path) as archive:
            infos = archive.infolist()
            by_name: dict[str, zipfile.ZipInfo] = {}
            for info in infos:
                path = _archive_member_path(info.filename)
                if info.is_dir():
                    raise OrchestrationError(
                        f"native MVP snapshot archive contains a directory member: {info.filename!r}"
                    )
                file_type = (info.external_attr >> 16) & 0o170000
                if file_type == 0o120000:
                    raise OrchestrationError(
                        f"native MVP snapshot archive contains a symlink member: {info.filename!r}"
                    )
                if info.filename in by_name:
                    raise OrchestrationError(
                        f"native MVP snapshot archive contains a duplicate member: {info.filename!r}"
                    )
                by_name[info.filename] = info
                # Keep the normalized path alive through the validation pass;
                # the actual write below uses only these already-validated parts.
                _ = path
            manifest_info = by_name.get("snapshot-manifest.json")
            if manifest_info is None:
                raise OrchestrationError("native MVP snapshot archive has no snapshot-manifest.json")
            try:
                manifest = json.loads(archive.read(manifest_info).decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                raise OrchestrationError(
                    f"native MVP snapshot archive manifest is not valid JSON: {error}"
                ) from error
            if not isinstance(manifest, dict) or not isinstance(manifest.get("files"), list):
                raise OrchestrationError("native MVP snapshot archive manifest is malformed")
            expected = {
                item["path"]
                for item in manifest["files"]
                if isinstance(item, dict) and isinstance(item.get("path"), str)
            }
            expected.add("snapshot-manifest.json")
            if set(by_name) != expected:
                raise OrchestrationError(
                    "native MVP snapshot archive members do not match its manifest"
                )
            for name, info in sorted(by_name.items()):
                relative = _archive_member_path(name)
                target = destination.joinpath(*relative.parts)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.read(info))
    except zipfile.BadZipFile as error:
        raise OrchestrationError(f"native MVP snapshot archive is invalid: {error}") from error
    return destination


def _validate_packaged_intake(
    commands: NativeMvpCommands,
    snapshot_dir: Path,
    project_spec_path: Path,
) -> None:
    """Revalidate packaged source bytes and typed intake, not just their hashes."""

    evidence_root = snapshot_dir / "native" / "handoff_snapshot" / "evidence"
    bundle_path = evidence_root / "intake" / "source-bundle.json"
    intake_path = evidence_root / "intake" / "semantic-intake.json"
    provider_path = evidence_root.parent / "source" / "provider-response.json"
    try:
        bundle = json.loads(bundle_path.read_text(encoding="utf-8"))
        spec = json.loads(project_spec_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise OrchestrationError(f"packaged intake cannot be read: {error}") from error
    if not isinstance(bundle, dict) or not isinstance(bundle.get("sources"), list):
        raise OrchestrationError("packaged source bundle is malformed")
    if not isinstance(spec, dict) or not isinstance(spec.get("brief"), dict):
        raise OrchestrationError("packaged project spec is malformed")
    references = spec["brief"].get("sources")
    if not isinstance(references, list):
        raise OrchestrationError("packaged project spec has no typed source references")
    paths_by_source_id = {
        item.get("source_id"): item.get("path")
        for item in references
        if isinstance(item, dict)
    }
    bindings: list[str] = []
    for source in bundle["sources"]:
        if not isinstance(source, dict) or not isinstance(source.get("source_id"), str):
            raise OrchestrationError("packaged source bundle has a malformed source")
        source_id = source["source_id"]
        relative = paths_by_source_id.get(source_id)
        if not isinstance(relative, str) or not relative:
            raise OrchestrationError(f"packaged source {source_id!r} is not bound by the project spec")
        source_path = snapshot_dir / relative
        if not source_path.is_file() or source_path.is_symlink():
            raise OrchestrationError(f"packaged source bytes are missing: {source_path}")
        bindings.append(f"{source_id}={source_path}")
    if len(bindings) != len(paths_by_source_id):
        raise OrchestrationError("project spec source references do not match the packaged source bundle")
    _run([commands.intake, "validate-source-bundle", str(bundle_path), *bindings])
    _run([commands.intake, "validate-intake", str(intake_path), str(provider_path), *bindings])


def _recompile_packaged_spec(
    commands: NativeMvpCommands,
    snapshot_dir: Path,
    project_spec_path: Path,
) -> None:
    """Ensure the delivered intake/template still compile to the delivered spec."""

    intake_path = snapshot_dir / "native" / "handoff_snapshot" / "evidence" / "intake" / "semantic-intake.json"
    template_path = snapshot_dir / "project-template.json"
    with tempfile.TemporaryDirectory(prefix="wge-packaged-spec-") as temporary:
        recompiled_path = Path(temporary) / "project_spec.json"
        _compile_spec(commands, intake_path, template_path, recompiled_path)
        if recompiled_path.read_bytes() != project_spec_path.read_bytes():
            raise OrchestrationError(
                "packaged project spec differs from the packaged intake/template compilation"
            )


def verify_native_snapshot(
    result: NativeMvpResult,
    commands: NativeMvpCommands,
) -> dict[str, object]:
    """Re-open the handoff and re-run native byte/runtime checks."""

    manifest = json.loads((result.snapshot_dir / "snapshot-manifest.json").read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or manifest.get("schema_version") != "wge.native-mvp-snapshot/v1":
        raise OrchestrationError("native MVP snapshot manifest is malformed")
    digest = manifest.get("snapshot_sha256")
    unsigned = dict(manifest)
    unsigned["snapshot_sha256"] = ""
    if digest != _digest_bytes(_canonical(unsigned)):
        raise OrchestrationError("native MVP snapshot manifest digest is stale")
    manifest_files = manifest.get("files")
    if not isinstance(manifest_files, list) or not manifest_files:
        raise OrchestrationError("native MVP snapshot file list is malformed")
    listed_paths: set[str] = set()
    for item in manifest_files:
        if not isinstance(item, dict):
            raise OrchestrationError("native MVP snapshot file entry is malformed")
        relative_text = item.get("path")
        if not isinstance(relative_text, str) or not relative_text:
            raise OrchestrationError("native MVP snapshot file path is malformed")
        relative = Path(relative_text)
        if (
            relative.is_absolute()
            or "\\" in relative_text
            or any(part in {"", ".", ".."} for part in relative.parts)
            or relative_text in listed_paths
        ):
            raise OrchestrationError(f"native MVP snapshot file path is unsafe or duplicated: {relative_text}")
        listed_paths.add(relative_text)
        path = result.snapshot_dir / relative
        if path.is_symlink() or not path.is_file():
            raise OrchestrationError(f"native MVP snapshot file is stale: {path}")
        if path.stat().st_size != item.get("byte_length") or _digest_file(path) != item.get("sha256"):
            raise OrchestrationError(f"native MVP snapshot file is stale: {path}")
    if any(path.is_symlink() for path in result.snapshot_dir.rglob("*")):
        raise OrchestrationError("native MVP snapshot contains a symlink")
    actual_paths = {
        path.relative_to(result.snapshot_dir).as_posix()
        for path in result.snapshot_dir.rglob("*")
        if path.is_file() and path.name != "snapshot-manifest.json"
    }
    if actual_paths != listed_paths:
        raise OrchestrationError("native MVP snapshot file list does not cover the snapshot exactly")
    project_spec_path = result.snapshot_dir / "project_spec.json"
    if manifest.get("project_spec_sha256") != _digest_file(project_spec_path):
        raise OrchestrationError("native MVP snapshot project spec digest is stale")
    runnable = manifest.get("runnable")
    if runnable != {
        "runtime": "wge-reference-runtime",
        "operation": "verify",
        "bundle": "native/current",
    }:
        raise OrchestrationError("native MVP snapshot runnable descriptor is malformed")
    _validate_spec(commands, project_spec_path)
    _validate_packaged_intake(commands, result.snapshot_dir, project_spec_path)
    _recompile_packaged_spec(commands, result.snapshot_dir, project_spec_path)
    _run(
        [
            commands.runtime,
            "verify",
            "--bundle",
            str(result.snapshot_dir / "native" / "current"),
        ]
    )
    evidence_root = result.snapshot_dir / "native" / "handoff_snapshot" / "evidence"
    authority_request = evidence_root / "certification-request.json"
    authority_result = _run(
        [
            commands.authority,
            "validate",
            str(authority_request),
            "--artifact-root",
            str(evidence_root),
            "--profile",
            "native-mvp",
        ]
    )
    try:
        authority_report = json.loads(authority_result.stdout)
    except json.JSONDecodeError as error:
        raise OrchestrationError(f"native authority emitted non-JSON snapshot output: {error}") from error
    if not isinstance(authority_report, dict) or authority_report.get("status") != "native_mvp_certified":
        raise OrchestrationError("native authority did not revalidate the certified snapshot")
    if authority_report.get("report_id") != manifest.get("certification_report_id"):
        raise OrchestrationError("native authority report identity differs from snapshot manifest")
    for key in ("project_id", "snapshot_id", "candidate_sha256", "status"):
        if authority_report.get(key) != manifest.get(key if key != "status" else "certification_status"):
            raise OrchestrationError(f"native authority {key} differs from snapshot manifest")
    return {
        "schema_version": "wge.native-mvp-snapshot-verification/v1",
        "status": "verified",
        "snapshot_sha256": digest,
        "independent_native_revalidation": True,
        "independent_authority_revalidation": True,
    }


def run_native_mvp(
    source_dir: Path,
    output_dir: Path,
    commands: NativeMvpCommands,
    *,
    project_template: Path,
    rigging_glb: Path,
    rigging_request: Path,
    bad_glb: Path | None = DEFAULT_BAD_GLB,
) -> NativeMvpResult:
    """Run the native WGE vertical slice and package a runnable snapshot."""

    source_dir = source_dir.expanduser().resolve(strict=True)
    output_dir = output_dir.expanduser().resolve()
    project_template = project_template.expanduser().resolve(strict=True)
    try:
        template_value = json.loads(project_template.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise OrchestrationError(f"cannot read project template: {error}") from error
    if not isinstance(template_value, dict) or not isinstance(template_value.get("project_id"), str):
        raise OrchestrationError("project template must declare a typed project_id")
    if not template_value["project_id"].strip():
        raise OrchestrationError("project template project_id must not be empty")
    if output_dir.exists() and any(output_dir.iterdir()):
        raise OrchestrationError(f"output directory must be empty: {output_dir}")
    if output_dir == source_dir or source_dir in output_dir.parents:
        raise OrchestrationError("output directory must not be inside caller source")
    output_dir.mkdir(parents=True, exist_ok=True)

    template_copy = output_dir / "project-template.json"
    _copy_file(project_template, template_copy)
    native_root = output_dir / "native"
    smoke = run_smoke(
        source_dir,
        native_root,
        commands.smoke(),
        bad_glb=bad_glb,
        profile="native-mvp",
        rigging_glb=rigging_glb,
        rigging_request=rigging_request,
        project_id=template_value["project_id"],
        project_template=template_copy,
    )

    spec_path = output_dir / "project_spec.json"
    _copy_file(native_root / "project_spec.json", spec_path)
    _validate_spec(commands, spec_path)

    report = dict(smoke.certification_report)
    if report.get("project_id") != template_value["project_id"]:
        raise OrchestrationError("native certification project_id does not match the project template")
    snapshot_dir = output_dir / "snapshot"
    snapshot_dir.mkdir(parents=True, exist_ok=True)
    _copy_file(spec_path, snapshot_dir / "project_spec.json")
    _copy_file(template_copy, snapshot_dir / "project-template.json")
    # Preserve the same relative root used by the compiled ProjectSpec.  A
    # spec that validates in the build directory must validate byte-for-byte
    # after handoff extraction as well.
    _copy_tree(native_root / "handoff_snapshot", snapshot_dir / "native" / "handoff_snapshot")
    _copy_tree(native_root / "current", snapshot_dir / "native" / "current")
    for name in ("certification-report.json", "native-stage-summary.json", "project-manifest.json"):
        _copy_file(native_root / name, snapshot_dir / "native" / name)
    _write_json(
        snapshot_dir / "runnable.json",
        {
            "schema_version": "wge.runnable-handoff/v1",
            "runtime": "wge-reference-runtime",
            "operation": "verify",
            "bundle": "native/current",
            "native_revalidation_required": True,
        },
    )
    snapshot_sha256 = _seal_snapshot_manifest(snapshot_dir, project_spec=spec_path, report=report)
    snapshot_zip = output_dir / "native-mvp-snapshot.zip"
    _zip_snapshot(snapshot_dir, snapshot_zip)
    result = NativeMvpResult(
        output_dir,
        spec_path,
        template_copy,
        snapshot_dir,
        snapshot_zip,
        snapshot_sha256,
        report,
    )
    verification = verify_native_snapshot(result, commands)
    with tempfile.TemporaryDirectory(prefix="wge-native-mvp-archive-") as temporary:
        extracted_dir = _extract_snapshot_archive(snapshot_zip, Path(temporary) / "snapshot")
        extracted_result = NativeMvpResult(
            output_dir=Path(temporary),
            spec_path=extracted_dir / "project_spec.json",
            template_path=extracted_dir / "project-template.json",
            snapshot_dir=extracted_dir,
            snapshot_zip=snapshot_zip,
            snapshot_sha256=json.loads(
                (extracted_dir / "snapshot-manifest.json").read_text(encoding="utf-8")
            )["snapshot_sha256"],
            certification_report=report,
        )
        verification["archive_revalidation"] = verify_native_snapshot(extracted_result, commands)
    _write_json(output_dir / "snapshot-verification.json", verification)
    if bad_glb is not None:
        bad_glb = bad_glb.expanduser().resolve()
        if not bad_glb.is_file() or _digest_file(bad_glb) != BAD_GLB_SHA256:
            raise OrchestrationError("permanent negative-control GLB is missing or changed")
    return result


def _parser():
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--source-dir", required=True, type=Path)
    parser.add_argument("--project-template", required=True, type=Path)
    parser.add_argument("--rigging-glb", required=True, type=Path)
    parser.add_argument("--rigging-request", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--bad-glb", type=Path, default=DEFAULT_BAD_GLB)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = run_native_mvp(
            args.source_dir,
            args.output_dir,
            NativeMvpCommands.from_environment(),
            project_template=args.project_template,
            rigging_glb=args.rigging_glb,
            rigging_request=args.rigging_request,
            bad_glb=args.bad_glb,
        )
    except (OSError, OrchestrationError, ValueError, json.JSONDecodeError) as error:
        print(f"wge-native-mvp: {error}", file=os.sys.stderr)
        return 2
    print(
        json.dumps(
            {
                "status": "verified",
                "output_dir": str(result.output_dir),
                "project_spec": str(result.spec_path),
                "snapshot": str(result.snapshot_dir),
                "snapshot_zip": str(result.snapshot_zip),
                "snapshot_sha256": result.snapshot_sha256,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
