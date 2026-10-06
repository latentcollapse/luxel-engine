"""Stage a fresh Luxel semantic-intake request.

This module is deliberately a transport boundary.  It copies caller-owned
brief/concept/design bytes, asks the Rust intake CLI to assign the source
bundle identity, and then binds an already-produced provider interpretation to
that identity.  It never derives a claim from a file and never decides whether
an interpretation is true; Rust performs those checks during normalization and
validation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import mimetypes
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Mapping, Sequence


SOURCE_BUNDLE_DRAFT_SCHEMA = "luxel.source-bundle-draft/v1"
PROVIDER_INTERPRETATION_SCHEMA = "luxel.provider-interpretation/v1"
INTAKE_DRAFT_SCHEMA = "luxel.semantic-intake-draft/v1"


class SourceIntakeError(RuntimeError):
    """The caller supplied an unsafe or internally inconsistent intake."""


@dataclass(frozen=True)
class StagedIntake:
    root: Path
    source_bundle: Path
    provider_response: Path
    intake_draft: Path
    source_bundle_id: str


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return "sha256:" + digest.hexdigest()


def _load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise SourceIntakeError(f"cannot read JSON input {path}: {error}") from error
    if not isinstance(value, dict):
        raise SourceIntakeError(f"JSON input {path} must contain an object")
    return value


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def _require_file(path: Path, label: str) -> Path:
    path = path.expanduser().resolve()
    if not path.is_file():
        raise SourceIntakeError(f"{label} does not exist: {path}")
    if path.stat().st_size == 0:
        raise SourceIntakeError(f"{label} is empty: {path}")
    return path


def _require_request_id(request_id: str) -> str:
    if not request_id or len(request_id) > 128 or any(char.isspace() for char in request_id):
        raise SourceIntakeError("request_id must be nonempty, <=128 characters, and contain no whitespace")
    if any(ord(char) < 32 for char in request_id):
        raise SourceIntakeError("request_id must not contain control characters")
    return request_id


def _media_type(path: Path, fallback: str) -> str:
    guessed, _encoding = mimetypes.guess_type(path.name)
    return guessed or fallback


def _copy_source(source: Path, destination: Path) -> Path:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    if _sha256(source) != _sha256(destination):
        raise SourceIntakeError(f"source copy changed bytes: {source}")
    return destination


def _run(
    argv: Sequence[str],
    *,
    cwd: Path,
    runner: Callable[..., subprocess.CompletedProcess[str]],
) -> subprocess.CompletedProcess[str]:
    try:
        result = runner(
            list(argv),
            cwd=str(cwd),
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as error:
        raise SourceIntakeError(f"cannot execute intake command {argv[0]!r}: {error}") from error
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SourceIntakeError(
            f"native source-bundle preparation failed with exit {result.returncode}"
            + (f": {detail}" if detail else "")
        )
    return result


def _source_draft(
    *,
    request_id: str,
    brief: Path,
    concept_art: Path,
    design_document: Path,
    source_root: Path,
) -> dict[str, object]:
    records = (
        ("brief.md", "brief", "text/markdown", brief, "brief"),
        (
            "concept_art" + (concept_art.suffix.lower() or ".bin"),
            "concept_art",
            _media_type(concept_art, "application/octet-stream"),
            concept_art,
            "concept_art",
        ),
        (
            "design_document" + (design_document.suffix.lower() or ".bin"),
            "design_document",
            _media_type(design_document, "application/octet-stream"),
            design_document,
            "design_document",
        ),
    )
    sources = []
    for source_ref, kind, media_type, source, origin_ref in records:
        destination = _copy_source(source, source_root / source_ref)
        sources.append(
            {
                "source_ref": source_ref,
                "kind": kind,
                "content_sha256": _sha256(destination),
                "media_type": media_type,
                "provenance": {
                    "origin": "user_supplied",
                    "origin_ref": origin_ref,
                    "provider_id": None,
                    "provider_version": None,
                },
            }
        )
    return {
        "schema_version": SOURCE_BUNDLE_DRAFT_SCHEMA,
        "request_id": request_id,
        "sources": sources,
    }


def _validate_provider_response(
    provider_response: Mapping[str, Any],
    *,
    source_bundle_id: str,
) -> None:
    if provider_response.get("schema_version") != PROVIDER_INTERPRETATION_SCHEMA:
        raise SourceIntakeError(
            f"provider response must use {PROVIDER_INTERPRETATION_SCHEMA}"
        )
    if provider_response.get("source_bundle_id") != source_bundle_id:
        raise SourceIntakeError(
            "provider response is not bound to the freshly prepared source bundle; "
            "produce a new interpretation for this source_bundle_id"
        )
    for field in ("claims", "conflicts", "assumptions"):
        if not isinstance(provider_response.get(field), list):
            raise SourceIntakeError(f"provider response field {field!r} must be an array")


def stage_intake(
    output_root: Path,
    *,
    brief: Path,
    concept_art: Path,
    design_document: Path,
    provider_response: Path,
    request_id: str,
    provider_id: str,
    provider_version: str,
    protocol: str,
    intake_cli: str,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> StagedIntake:
    """Create a fresh, source-bound intake directory.

    The provider response is read and copied byte-for-byte.  In particular,
    this function refuses a response with a placeholder or stale bundle ID
    instead of silently rewriting semantic content to fit the new request.
    """

    request_id = _require_request_id(request_id)
    for value, label in (
        (provider_id, "provider_id"),
        (provider_version, "provider_version"),
        (protocol, "protocol"),
    ):
        if not value or any(ord(char) < 32 for char in value):
            raise SourceIntakeError(f"{label} must be nonempty and contain no control characters")
    brief = _require_file(brief, "brief")
    concept_art = _require_file(concept_art, "concept art")
    design_document = _require_file(design_document, "design document")
    provider_response = _require_file(provider_response, "provider response")
    output_root = output_root.expanduser().resolve()
    if output_root.exists() and any(output_root.iterdir()):
        raise SourceIntakeError(f"output directory must be empty: {output_root}")
    output_root.mkdir(parents=True, exist_ok=True)
    source_root = output_root / "sources"
    draft_path = output_root / "source-bundle-draft.json"
    _write_json(
        draft_path,
        _source_draft(
            request_id=request_id,
            brief=brief,
            concept_art=concept_art,
            design_document=design_document,
            source_root=source_root,
        ),
    )

    bundle_path = output_root / "source-bundle.json"
    draft = _load_json(draft_path)
    bindings = [
        f"{source['source_ref']}={source_root / source['source_ref']}"
        for source in draft["sources"]
    ]
    _run(
        [intake_cli, "prepare-source-bundle", str(draft_path), str(bundle_path), *bindings],
        cwd=output_root,
        runner=runner,
    )
    bundle = _load_json(bundle_path)
    source_bundle_id = bundle.get("source_bundle_id")
    if not isinstance(source_bundle_id, str) or not source_bundle_id:
        raise SourceIntakeError("native source-bundle output has no source_bundle_id")

    provider = _load_json(provider_response)
    _validate_provider_response(provider, source_bundle_id=source_bundle_id)
    provider_path = output_root / "provider-response.json"
    _copy_source(provider_response, provider_path)
    intake_path = output_root / "intake-draft.json"
    _write_json(
        intake_path,
        {
            "schema_version": INTAKE_DRAFT_SCHEMA,
            "source_bundle_id": source_bundle_id,
            "provider": {
                "provider_id": provider_id,
                "provider_version": provider_version,
                "protocol": protocol,
                "request_source_bundle_id": source_bundle_id,
                "response_sha256": _sha256(provider_path),
            },
            "interpretation": provider,
        },
    )
    return StagedIntake(output_root, bundle_path, provider_path, intake_path, source_bundle_id)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--brief", required=True, type=Path)
    parser.add_argument("--concept-art", required=True, type=Path)
    parser.add_argument("--design-document", required=True, type=Path)
    parser.add_argument("--provider-response", required=True, type=Path)
    parser.add_argument("--request-id", required=True)
    parser.add_argument("--provider-id", required=True)
    parser.add_argument("--provider-version", required=True)
    parser.add_argument("--protocol", required=True)
    parser.add_argument("--intake-cli", default="luxel-intake-repair")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        staged = stage_intake(
            args.output,
            brief=args.brief,
            concept_art=args.concept_art,
            design_document=args.design_document,
            provider_response=args.provider_response,
            request_id=args.request_id,
            provider_id=args.provider_id,
            provider_version=args.provider_version,
            protocol=args.protocol,
            intake_cli=args.intake_cli,
        )
    except SourceIntakeError as error:
        print(str(error), file=sys.stderr)
        return 2
    print(
        json.dumps(
            {
                "status": "staged",
                "root": str(staged.root),
                "source_bundle": str(staged.source_bundle),
                "provider_response": str(staged.provider_response),
                "intake_draft": str(staged.intake_draft),
                "source_bundle_id": staged.source_bundle_id,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
