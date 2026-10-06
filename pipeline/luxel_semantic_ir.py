#!/usr/bin/env python3
"""Lower a closed Luxel authoring module into canonical semantic IR.

The source is parsed by ``luxel_language_kernel`` and is never executed.
This module is the specimen lowerer for one lane and one placement.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from pathlib import Path
from typing import Any

from luxel_language_kernel import (
    BuildManifest,
    FrontendError,
    RegistrySymbolPin,
    SourceSpan,
    parse_module,
    source_digest,
)

IR_SCHEMA = "luxel.semantic-ir/v0"
MODULE_ID = "spike.lane.world"


class LowerError(ValueError):
    def __init__(self, code: str, detail: str) -> None:
        self.code = code
        self.detail = detail
        super().__init__(f"{code}: {detail}")


def file_digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def canonical_dumps(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def _normalizer_command() -> list[str]:
    """Return the Rust canonicalizer, building it through Cargo when needed."""
    configured = os.environ.get("LUXEL_SEMANTIC_KERNEL_BIN")
    if configured:
        return [configured, "normalize-ir"]
    root = Path(__file__).resolve().parents[1]
    binary = root / "world_core" / "target" / "debug" / "luxel-semantic-kernel"
    if binary.is_file():
        return [str(binary), "normalize-ir"]
    manifest = root / "world_core" / "crates" / "semantic_kernel" / "Cargo.toml"
    return ["cargo", "run", "--quiet", "--manifest-path", str(manifest), "--", "normalize-ir"]


def normalize_document(document: dict[str, Any]) -> dict[str, Any]:
    """Ask the Rust semantic kernel to validate and normalize a draft IR."""
    try:
        payload = canonical_dumps(document)
    except (TypeError, ValueError) as error:
        raise LowerError("malformed_ir", f"draft IR is not JSON-compatible: {error}") from error
    try:
        completed = subprocess.run(
            _normalizer_command(),
            cwd=Path(__file__).resolve().parents[1],
            input=payload,
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as error:
        raise LowerError("semantic_kernel_unavailable", str(error)) from error
    if completed.returncode != 0:
        try:
            failure = json.loads(completed.stdout)
        except json.JSONDecodeError:
            detail = completed.stderr.strip() or completed.stdout.strip() or "Rust normalizer failed"
            raise LowerError("semantic_kernel_failure", detail)
        raise LowerError(str(failure.get("code", "semantic_kernel_failure")), str(failure.get("detail", "")))
    try:
        normalized = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise LowerError("semantic_kernel_failure", f"Rust normalizer returned invalid JSON: {error}") from error
    if not isinstance(normalized, dict):
        raise LowerError("malformed_ir", "Rust normalizer returned a non-object document")
    return normalized


def _pin(namespace: str, symbol: str, digest: str) -> RegistrySymbolPin:
    return RegistrySymbolPin(
        namespace=namespace,
        symbol=symbol,
        module_id=f"registry.{namespace}",
        semantic_digest=digest,
        kind="constructor",
        positional_identity_slots=0,
    )


def _span(span: SourceSpan) -> dict[str, int]:
    return {
        "column": span.column,
        "end_column": span.end_column,
        "end_line": span.end_line,
        "line": span.line,
    }


def _literal_int(expr: dict[str, Any], label: str) -> int:
    if expr.get("kind") != "literal" or isinstance(expr.get("value"), bool) or not isinstance(expr.get("value"), int):
        raise LowerError("malformed_ir", f"{label} must be an integer literal")
    return int(expr["value"])


def _call(expr: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    if expr.get("kind") != "call":
        raise LowerError("unknown_constructor", "declarations must be constructor calls")
    if expr.get("positional_identity"):
        raise LowerError("unknown_field", "specimen constructors are keyword-only")
    arguments = expr.get("arguments")
    if not isinstance(arguments, list):
        raise LowerError("malformed_ir", "call arguments are missing")
    fields: dict[str, Any] = {}
    for item in arguments:
        if not isinstance(item, dict) or "name" not in item:
            raise LowerError("malformed_ir", "argument is not a named field")
        fields[str(item["name"])] = item["value"]
    constructor = expr.get("constructor")
    if not isinstance(constructor, str):
        raise LowerError("unknown_constructor", "constructor name is missing")
    return constructor, fields


def _rect(expr: dict[str, Any]) -> dict[str, int]:
    constructor, fields = _call(expr)
    if constructor != "rect":
        raise LowerError("unknown_constructor", f"{constructor} is not a footprint constructor")
    if set(fields) != {"x0", "x1", "y0", "y1"}:
        raise LowerError("unknown_field", "rect accepts only x0, y0, x1, y1")
    return {key: _literal_int(fields[key], key) for key in ("x0", "x1", "y0", "y1")}


def lower_source(source: str, registry_path: Path) -> dict[str, Any]:
    """Parse ``source`` as data and return the canonical IR object."""
    digest = file_digest(registry_path)
    manifest = BuildManifest(
        package_id="spike.lane",
        root_module_id=MODULE_ID,
        root_source_digest=source_digest(source),
        package_seed="seed256:" + "0" * 64,
        target="semantic-ir",
        product_profile_id="spike.lane@0",
        product_profile_digest=digest,
        registry_digest=digest,
        imports=(
            _pin("luxel.world", "lane", digest),
            _pin("luxel.world", "place", digest),
            _pin("luxel.geometry", "rect", digest),
            _pin("luxel.geometry", "path", digest),
        ),
    )
    try:
        parsed = parse_module(source, manifest)
    except FrontendError as error:
        raise LowerError(error.code, str(error)) from error
    if parsed.expression_statements:
        raise LowerError("unknown_constructor", "expression statements are outside this specimen")
    nodes = []
    for declaration in parsed.declarations:
        constructor, fields = _call(declaration.expression)
        if constructor in {"lane", "place"}:
            if set(fields) != {"id", "footprint"}:
                raise LowerError("unknown_field", f"{constructor} accepts only id and footprint")
            world_id = fields["id"]
            if world_id.get("kind") != "literal" or not isinstance(world_id.get("value"), str):
                raise LowerError("malformed_ir", "id must be a string literal")
            nodes.append(
                {
                    "binding": declaration.name,
                    "constructor": constructor,
                    "footprint": _rect(fields["footprint"]),
                    "id": world_id["value"],
                    "span": _span(declaration.span),
                }
            )
            continue
        if constructor == "path":
            required = {"budget", "id", "x0", "x1", "y0", "y1"}
            if set(fields) != required:
                raise LowerError("unknown_field", "path accepts only id, x0, y0, x1, y1, budget")
            world_id = fields["id"]
            if world_id.get("kind") != "literal" or not isinstance(world_id.get("value"), str):
                raise LowerError("malformed_ir", "id must be a string literal")
            nodes.append(
                {
                    "binding": declaration.name,
                    "budget": _literal_int(fields["budget"], "budget"),
                    "constructor": "path",
                    "id": world_id["value"],
                    "span": _span(declaration.span),
                    "x0": _literal_int(fields["x0"], "x0"),
                    "x1": _literal_int(fields["x1"], "x1"),
                    "y0": _literal_int(fields["y0"], "y0"),
                    "y1": _literal_int(fields["y1"], "y1"),
                }
            )
            continue
        raise LowerError("unknown_constructor", f"{constructor} is outside the specimen vocabulary")
    kinds = [node["constructor"] for node in nodes]
    if kinds.count("lane") != 1 or kinds.count("place") != 1:
        raise LowerError("malformed_ir", "specimen requires exactly one lane and one place")
    if kinds.count("path") > 1:
        raise LowerError("malformed_ir", "specimen allows at most one path")
    draft = {
        "declarations": nodes,
        "module_id": parsed.module_id,
        "registry_digest": digest,
        "schema": IR_SCHEMA,
    }
    return normalize_document(draft)


def lower_file(path: Path, registry_path: Path) -> dict[str, Any]:
    if path.suffix != ".luxel":
        raise LowerError("wrong_source_extension", "authoring source must use .luxel")
    return lower_source(path.read_text(encoding="utf-8"), registry_path)


def main() -> None:
    import argparse

    parser = argparse.ArgumentParser(description="Lower a lane specimen to canonical semantic IR")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    document = lower_file(args.source, args.registry)
    args.out.write_text(canonical_dumps(document) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
