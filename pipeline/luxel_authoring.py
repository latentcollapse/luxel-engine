"""Provider-neutral, parse-only authoring request lowering for Luxel.

This module accepts declarative JSON data. It never evaluates Python, invokes a
provider, infers claims, or assigns native source identities. It prepares the
Rust source-bundle draft and preserves explicitly supplied interpretations;
``bind_prepared_bundle`` adds Rust-issued source IDs after native preparation.
Rust remains responsible for source-bundle and semantic-intake authority.
"""

from __future__ import annotations

import base64
import binascii
import argparse
import hashlib
import json
import math
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping

from pipeline.luxel_source_intake import (
    INTAKE_DRAFT_SCHEMA,
    PROVIDER_INTERPRETATION_SCHEMA,
    SOURCE_BUNDLE_DRAFT_SCHEMA,
)


AUTHORING_SOURCE_SCHEMA = "luxel.authoring-source-bundle/v1"
AUTHORING_REQUEST_SCHEMA = "luxel.authoring-request/v1"
NATIVE_INTAKE_REQUEST_SCHEMA = "luxel.native-intake-request/v1"
MAX_DOCUMENT_BYTES = 16 * 1024 * 1024
MAX_SOURCE_BYTES = 8 * 1024 * 1024
MAX_TOTAL_SOURCE_BYTES = 24 * 1024 * 1024

_DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
_SOURCE_BUNDLE_ID = re.compile(r"source-bundle:sha256:[0-9a-f]{64}\Z")
_SOURCE_ID = re.compile(r"source:sha256:[0-9a-f]{64}\Z")
_SOURCE_REF = re.compile(r"[A-Za-z0-9_.-]{1,64}\Z")
_SOURCE_KINDS = frozenset({"brief", "concept_art", "design_document"})
_DOMAINS = frozenset(
    {
        "visual",
        "gameplay",
        "world",
        "style",
        "constraint",
        "interaction",
        "asset",
        "narrative",
        "accessibility",
        "performance",
        "exclusion",
        "other",
    }
)
_EPISTEMIC_KINDS = frozenset({"observation", "inference"})
_ORIGINS = frozenset({"user_supplied", "provider_generated", "retrieved", "derived"})


@dataclass(frozen=True)
class AuthoringDiagnostic:
    code: str
    detail: str
    path: str | None = None
    line: int | None = None
    column: int | None = None
    end_line: int | None = None
    end_column: int | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"code": self.code, "detail": self.detail}
        for field in ("path", "line", "column", "end_line", "end_column"):
            value = getattr(self, field)
            if value is not None:
                result[field] = value
        return result


class AuthoringError(ValueError):
    """A stable parse or contract diagnostic; no partial request is emitted."""

    def __init__(self, diagnostic: AuthoringDiagnostic) -> None:
        self.diagnostic = diagnostic
        self.diagnostics = (diagnostic,)
        where = f" at {diagnostic.path}" if diagnostic.path else ""
        if diagnostic.line is not None:
            where += f" ({diagnostic.line}:{(diagnostic.column or 0) + 1})"
        super().__init__(f"{diagnostic.code}{where}: {diagnostic.detail}")


class _DuplicateJsonKey(ValueError):
    def __init__(self, key: str) -> None:
        self.key = key
        super().__init__(key)


class _InvalidJsonConstant(ValueError):
    pass


def _fail(
    code: str,
    detail: str,
    path: str | None = None,
    *,
    line: int | None = None,
    column: int | None = None,
    end_line: int | None = None,
    end_column: int | None = None,
) -> None:
    raise AuthoringError(
        AuthoringDiagnostic(
            code,
            detail,
            path,
            line,
            column,
            end_line,
            end_column,
        )
    )


def _pairs_without_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise _DuplicateJsonKey(key)
        result[key] = value
    return result


def _reject_json_constant(token: str) -> None:
    raise _InvalidJsonConstant(token)


def parse_authoring_source(source: str | bytes) -> dict[str, Any]:
    """Parse one UTF-8 JSON authoring bundle with stable syntax diagnostics."""
    if isinstance(source, bytes):
        if len(source) > MAX_DOCUMENT_BYTES:
            _fail("document_limit", "authoring JSON exceeds the byte limit")
        try:
            text = source.decode("utf-8", errors="strict")
        except UnicodeDecodeError as error:
            _fail(
                "invalid_utf8",
                "authoring JSON must be UTF-8",
                line=1,
                column=error.start,
                end_line=1,
                end_column=error.end,
            )
    elif isinstance(source, str):
        try:
            encoded = source.encode("utf-8", errors="strict")
        except UnicodeEncodeError as error:
            _fail("invalid_unicode", "authoring JSON contains an unpaired surrogate")
        if len(encoded) > MAX_DOCUMENT_BYTES:
            _fail("document_limit", "authoring JSON exceeds the byte limit")
        text = source
    else:
        _fail("invalid_input_type", "authoring source must be UTF-8 JSON text or bytes")
    try:
        value = json.loads(
            text,
            object_pairs_hook=_pairs_without_duplicates,
            parse_constant=_reject_json_constant,
        )
    except json.JSONDecodeError as error:
        _fail(
            "invalid_json",
            error.msg,
            line=error.lineno,
            column=max(0, error.colno - 1),
            end_line=error.lineno,
            end_column=error.colno,
        )
    except _DuplicateJsonKey as error:
        _fail("duplicate_json_key", f"JSON object repeats key {error.key!r}")
    except _InvalidJsonConstant as error:
        _fail("invalid_json_number", f"non-standard JSON number {error}")
    if not isinstance(value, dict):
        _fail("invalid_root", "authoring source root must be an object")
    return value


def _object(
    value: Any,
    *,
    path: str,
    required: set[str],
    optional: set[str] | None = None,
) -> dict[str, Any]:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        _fail("expected_object", "value must be an object", path)
    allowed = required | (optional or set())
    unknown = sorted(set(value) - allowed)
    if unknown:
        _fail("unknown_field", f"unknown field(s): {', '.join(unknown)}", path)
    missing = sorted(required - set(value))
    if missing:
        _fail("missing_field", f"required field(s) missing: {', '.join(missing)}", path)
    return value


def _text(value: Any, *, path: str, max_bytes: int, allow_controls: bool = False) -> str:
    if not isinstance(value, str):
        _fail("expected_text", "value must be a string", path)
    try:
        size = len(value.encode("utf-8", errors="strict"))
    except UnicodeEncodeError:
        _fail("invalid_unicode", "text contains an unpaired surrogate", path)
    if not value.strip() or size > max_bytes:
        _fail("invalid_text", f"value must be nonempty and at most {max_bytes} UTF-8 bytes", path)
    if not allow_controls and any(ord(char) < 32 for char in value):
        _fail("invalid_text", "control characters are forbidden", path)
    return value


def _digest(value: Any, *, path: str) -> str:
    if not isinstance(value, str) or not _DIGEST.fullmatch(value):
        _fail("malformed_digest", "expected sha256 followed by 64 lowercase hexadecimal digits", path)
    return value


def _confidence(value: Any, *, path: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        _fail("invalid_confidence", "confidence must be a finite number in [0,1]", path)
    try:
        confidence = float(value)
    except OverflowError:
        _fail("invalid_confidence", "confidence must be a finite number in [0,1]", path)
    if not math.isfinite(confidence) or not 0.0 <= confidence <= 1.0:
        _fail("invalid_confidence", "confidence must be a finite number in [0,1]", path)
    return confidence


def _span(value: Any, *, path: str) -> dict[str, int]:
    item = _object(
        value,
        path=path,
        required={"line", "column", "end_line", "end_column"},
    )
    normalized: dict[str, int] = {}
    for key in ("line", "column", "end_line", "end_column"):
        coordinate = item[key]
        if isinstance(coordinate, bool) or not isinstance(coordinate, int) or coordinate < 0:
            _fail("invalid_span", f"{key} must be a non-negative integer", f"{path}.{key}")
        normalized[key] = coordinate
    if normalized["line"] < 1 or normalized["end_line"] < normalized["line"]:
        _fail("invalid_span", "span lines must be positive and ordered", path)
    if normalized["end_line"] == normalized["line"] and normalized["end_column"] < normalized["column"]:
        _fail("invalid_span", "span columns must be ordered", path)
    return normalized


def _source_region(
    value: Any,
    *,
    path: str,
    source_kind: str,
    byte_length: int,
) -> dict[str, Any] | None:
    if value is None:
        return None
    if not isinstance(value, dict):
        _fail("invalid_region", "region must be an object or null", path)
    kind = value.get("kind")
    if kind == "text_span":
        item = _object(value, path=path, required={"kind", "start_byte", "end_byte"})
        start, end = item["start_byte"], item["end_byte"]
        if (
            source_kind not in {"brief", "design_document"}
            or isinstance(start, bool)
            or isinstance(end, bool)
            or not isinstance(start, int)
            or not isinstance(end, int)
            or start < 0
            or start >= end
            or end > byte_length
        ):
            _fail("invalid_region", "text span is empty, out of bounds, or incompatible with source kind", path)
        return {"kind": "text_span", "start_byte": start, "end_byte": end}
    if isinstance(kind, str) and kind in {"image_rect", "page_rect"}:
        fields = {"kind", "x_min", "y_min", "x_max", "y_max"}
        if kind == "page_rect":
            fields.add("page")
        item = _object(value, path=path, required=fields)
        coordinates = []
        for key in ("x_min", "y_min", "x_max", "y_max"):
            coordinate = item[key]
            if isinstance(coordinate, bool) or not isinstance(coordinate, (int, float)):
                _fail("invalid_region", f"{key} must be a finite coordinate", f"{path}.{key}")
            coordinate = float(coordinate)
            if not math.isfinite(coordinate) or not 0.0 <= coordinate <= 1.0:
                _fail("invalid_region", f"{key} must be in [0,1]", f"{path}.{key}")
            coordinates.append(coordinate)
        x_min, y_min, x_max, y_max = coordinates
        expected_kind = "concept_art" if kind == "image_rect" else "design_document"
        if source_kind != expected_kind or x_min >= x_max or y_min >= y_max:
            _fail("invalid_region", "rectangle is empty or incompatible with source kind", path)
        result: dict[str, Any] = {
            "kind": kind,
            "x_min": x_min,
            "y_min": y_min,
            "x_max": x_max,
            "y_max": y_max,
        }
        if kind == "page_rect":
            page = item["page"]
            if isinstance(page, bool) or not isinstance(page, int) or page < 1:
                _fail("invalid_region", "page must be a positive integer", f"{path}.page")
            result["page"] = page
        return result
    _fail("unknown_region_kind", f"unsupported source region kind {kind!r}", path)


def lower_authoring_bundle(value: Mapping[str, Any]) -> dict[str, Any]:
    """Lower declarative bundle data to an inspectable Rust-bound request.

    Claims and assumptions are caller/provider supplied assertions. This
    function validates their shape and source bindings but does not derive or
    grade their truth. Source IDs remain absent until Rust prepares the bundle.
    """
    if not isinstance(value, Mapping):
        _fail("expected_object", "authoring bundle must be an object")
    root = _object(
        dict(value),
        path="$",
        required={
            "schema_version",
            "request_id",
            "provider",
            "sources",
            "claims",
            "conflicts",
            "assumptions",
        },
    )
    if root["schema_version"] != AUTHORING_SOURCE_SCHEMA:
        _fail("unsupported_schema", f"expected {AUTHORING_SOURCE_SCHEMA}", "$.schema_version")
    request_id = _text(root["request_id"], path="$.request_id", max_bytes=128)
    if any(char.isspace() for char in request_id) or request_id.startswith("source-bundle:"):
        _fail("invalid_request_id", "request_id must be a fresh opaque token without whitespace", "$.request_id")

    provider = _object(
        root["provider"],
        path="$.provider",
        required={"provider_id", "provider_version", "protocol"},
    )
    provider_identity = {
        key: _text(provider[key], path=f"$.provider.{key}", max_bytes=128)
        for key in ("provider_id", "provider_version", "protocol")
    }

    if not isinstance(root["sources"], list) or not root["sources"]:
        _fail("invalid_sources", "sources must be a nonempty array", "$.sources")
    sources: list[dict[str, Any]] = []
    source_payloads: list[dict[str, str]] = []
    source_info: dict[str, dict[str, Any]] = {}
    seen_source_identities: set[str] = set()
    total_bytes = 0
    for index, raw in enumerate(root["sources"]):
        path = f"$.sources[{index}]"
        item = _object(
            raw,
            path=path,
            required={
                "source_ref",
                "kind",
                "media_type",
                "content_sha256",
                "provenance",
            },
            optional={"content_text", "content_base64"},
        )
        source_ref = _text(item["source_ref"], path=f"{path}.source_ref", max_bytes=64)
        if not _SOURCE_REF.fullmatch(source_ref):
            _fail("invalid_source_ref", "source_ref must use ASCII letters, digits, period, underscore, or hyphen", f"{path}.source_ref")
        if source_ref in source_info:
            _fail("duplicate_source_ref", f"source_ref {source_ref!r} is repeated", f"{path}.source_ref")
        kind = item["kind"]
        if not isinstance(kind, str) or kind not in _SOURCE_KINDS:
            _fail("unknown_source_kind", f"unsupported source kind {kind!r}", f"{path}.kind")
        media_type = _text(item["media_type"], path=f"{path}.media_type", max_bytes=128)
        media_parts = media_type.split("/")
        if (
            len(media_parts) != 2
            or not all(media_parts)
            or not all(char.isascii() and (char.isalnum() or char in "!#$&^_.+-/") for char in media_type)
        ):
            _fail("invalid_media_type", "media_type must be a valid type/subtype token", f"{path}.media_type")
        if ("content_text" in item) == ("content_base64" in item):
            _fail("invalid_source_payload", "provide exactly one of content_text or content_base64", path)
        if "content_text" in item:
            text_payload = item["content_text"]
            if not isinstance(text_payload, str):
                _fail("invalid_source_payload", "content_text must be a string", f"{path}.content_text")
            try:
                payload = text_payload.encode("utf-8", errors="strict")
            except UnicodeEncodeError:
                _fail("invalid_source_payload", "content_text contains an unpaired surrogate", f"{path}.content_text")
        else:
            encoded_payload = item["content_base64"]
            if not isinstance(encoded_payload, str):
                _fail("invalid_source_payload", "content_base64 must be a string", f"{path}.content_base64")
            try:
                payload = base64.b64decode(encoded_payload, validate=True)
            except (ValueError, binascii.Error):
                _fail("invalid_base64", "content_base64 is not canonical base64 data", f"{path}.content_base64")
            if base64.b64encode(payload).decode("ascii") != encoded_payload:
                _fail("invalid_base64", "content_base64 must use canonical padding and alphabet", f"{path}.content_base64")
        if not payload:
            _fail("empty_source", "source bytes must be nonempty", path)
        if len(payload) > MAX_SOURCE_BYTES:
            _fail("source_limit", f"source exceeds {MAX_SOURCE_BYTES} bytes", path)
        total_bytes += len(payload)
        if total_bytes > MAX_TOTAL_SOURCE_BYTES:
            _fail("total_source_limit", f"combined source bytes exceed {MAX_TOTAL_SOURCE_BYTES}", "$.sources")
        expected_digest = _digest(item["content_sha256"], path=f"{path}.content_sha256")
        actual_digest = "sha256:" + hashlib.sha256(payload).hexdigest()
        if actual_digest != expected_digest:
            _fail("stale_source_binding", "content_sha256 does not match supplied source bytes", f"{path}.content_sha256")

        raw_provenance = _object(
            item["provenance"],
            path=f"{path}.provenance",
            required={"origin", "origin_ref"},
            optional={"provider_id", "provider_version"},
        )
        origin = raw_provenance["origin"]
        if not isinstance(origin, str) or origin not in _ORIGINS:
            _fail("invalid_provenance", f"unsupported provenance origin {origin!r}", f"{path}.provenance.origin")
        origin_ref = _text(raw_provenance["origin_ref"], path=f"{path}.provenance.origin_ref", max_bytes=512)
        provenance = {
            "origin": origin,
            "origin_ref": origin_ref,
            "provider_id": None,
            "provider_version": None,
        }
        if origin == "user_supplied":
            if "provider_id" in raw_provenance or "provider_version" in raw_provenance:
                if raw_provenance.get("provider_id") is not None or raw_provenance.get("provider_version") is not None:
                    _fail("invalid_provenance", "user_supplied sources cannot claim provider identity", f"{path}.provenance")
        else:
            if "provider_id" not in raw_provenance or "provider_version" not in raw_provenance:
                _fail("invalid_provenance", "non-user sources require provider_id and provider_version", f"{path}.provenance")
            provenance["provider_id"] = _text(raw_provenance["provider_id"], path=f"{path}.provenance.provider_id", max_bytes=128)
            provenance["provider_version"] = _text(raw_provenance["provider_version"], path=f"{path}.provenance.provider_version", max_bytes=128)

        source_draft = {
            "source_ref": source_ref,
            "kind": kind,
            "content_sha256": expected_digest,
            "media_type": media_type,
            "provenance": provenance,
        }
        identity = json.dumps(
            {key: source_draft[key] for key in ("kind", "content_sha256", "media_type", "provenance")},
            sort_keys=True,
            separators=(",", ":"),
        )
        if identity in seen_source_identities:
            _fail("duplicate_source_identity", "two source refs would collapse to one native source ID", path)
        seen_source_identities.add(identity)
        source_info[source_ref] = {
            "kind": kind,
            "byte_length": len(payload),
            "content_sha256": expected_digest,
            "payload": payload,
        }
        sources.append(source_draft)
        source_payloads.append(
            {"source_ref": source_ref, "content_base64": base64.b64encode(payload).decode("ascii")}
        )

    counts = {kind: sum(source["kind"] == kind for source in sources) for kind in _SOURCE_KINDS}
    if counts["brief"] != 1 or counts["concept_art"] < 1 or counts["design_document"] < 1:
        _fail(
            "incomplete_source_bundle",
            "authoring requires exactly one brief, at least one concept_art, and at least one design_document source",
            "$.sources",
        )

    raw_claims = root["claims"]
    if not isinstance(raw_claims, list) or not raw_claims:
        _fail("missing_claims", "claims must be a nonempty array", "$.claims")
    claims: list[dict[str, Any]] = []
    claim_spans: list[dict[str, Any]] = []
    claim_refs: set[str] = set()
    for index, raw in enumerate(raw_claims):
        path = f"$.claims[{index}]"
        item = _object(
            raw,
            path=path,
            required={"claim_ref", "epistemic_kind", "domain", "statement", "confidence", "evidence"},
            optional={"declaration_span"},
        )
        claim_ref = _text(item["claim_ref"], path=f"{path}.claim_ref", max_bytes=64)
        if not _SOURCE_REF.fullmatch(claim_ref) or claim_ref in claim_refs:
            _fail("invalid_claim_ref", "claim_ref must be unique and use source-ref characters", f"{path}.claim_ref")
        claim_refs.add(claim_ref)
        epistemic_kind = item["epistemic_kind"]
        if not isinstance(epistemic_kind, str) or epistemic_kind not in _EPISTEMIC_KINDS:
            _fail("invalid_epistemic_kind", f"unsupported epistemic kind {epistemic_kind!r}", f"{path}.epistemic_kind")
        domain = item["domain"]
        if not isinstance(domain, str) or domain not in _DOMAINS:
            _fail("invalid_domain", f"unsupported claim domain {domain!r}", f"{path}.domain")
        statement = _text(item["statement"], path=f"{path}.statement", max_bytes=4096)
        confidence = _confidence(item["confidence"], path=f"{path}.confidence")
        if not isinstance(item["evidence"], list) or not item["evidence"]:
            _fail("missing_evidence", "each claim requires at least one evidence link", f"{path}.evidence")
        evidence: list[dict[str, Any]] = []
        evidence_keys: set[str] = set()
        for evidence_index, raw_link in enumerate(item["evidence"]):
            evidence_path = f"{path}.evidence[{evidence_index}]"
            link = _object(raw_link, path=evidence_path, required={"source_ref"}, optional={"region"})
            source_ref = _text(link["source_ref"], path=f"{evidence_path}.source_ref", max_bytes=64)
            source = source_info.get(source_ref)
            if source is None:
                _fail("unknown_source_ref", f"evidence references unknown source {source_ref!r}", f"{evidence_path}.source_ref")
            region = _source_region(
                link.get("region"),
                path=f"{evidence_path}.region",
                source_kind=source["kind"],
                byte_length=source["byte_length"],
            )
            if epistemic_kind == "observation" and region is None:
                _fail("missing_observation_region", "observations require a source region for every evidence link", evidence_path)
            normalized_link = {"source_ref": source_ref, "region": region}
            evidence_key = json.dumps(normalized_link, sort_keys=True, separators=(",", ":"))
            if evidence_key in evidence_keys:
                _fail("duplicate_evidence", "claim repeats an evidence link", evidence_path)
            evidence_keys.add(evidence_key)
            evidence.append(normalized_link)
        claim = {
            "claim_ref": claim_ref,
            "epistemic_kind": epistemic_kind,
            "domain": domain,
            "statement": statement,
            "confidence": confidence,
            "evidence": evidence,
        }
        claims.append(claim)
        if "declaration_span" in item:
            claim_spans.append(
                {"claim_ref": claim_ref, "span": _span(item["declaration_span"], path=f"{path}.declaration_span")}
            )

    raw_conflicts = root["conflicts"]
    if not isinstance(raw_conflicts, list):
        _fail("invalid_conflicts", "conflicts must be an array", "$.conflicts")
    conflicts: list[dict[str, str]] = []
    conflict_refs: set[str] = set()
    for index, raw in enumerate(raw_conflicts):
        path = f"$.conflicts[{index}]"
        item = _object(raw, path=path, required={"conflict_ref", "left_claim_ref", "right_claim_ref", "explanation"})
        conflict_ref = _text(item["conflict_ref"], path=f"{path}.conflict_ref", max_bytes=64)
        if not _SOURCE_REF.fullmatch(conflict_ref) or conflict_ref in conflict_refs:
            _fail("invalid_conflict_ref", "conflict_ref must be unique and use identifier characters", f"{path}.conflict_ref")
        conflict_refs.add(conflict_ref)
        left = _text(item["left_claim_ref"], path=f"{path}.left_claim_ref", max_bytes=64)
        right = _text(item["right_claim_ref"], path=f"{path}.right_claim_ref", max_bytes=64)
        if left not in claim_refs or right not in claim_refs:
            _fail("unknown_conflict_claim", "conflict endpoints must reference declared claims", path)
        if left == right:
            _fail("invalid_conflict", "conflict endpoints must be distinct claims", path)
        explanation = _text(item["explanation"], path=f"{path}.explanation", max_bytes=2048)
        conflicts.append(
            {
                "conflict_ref": conflict_ref,
                "left_claim_ref": left,
                "right_claim_ref": right,
                "explanation": explanation,
            }
        )

    raw_assumptions = root["assumptions"]
    if not isinstance(raw_assumptions, list):
        _fail("invalid_assumptions", "assumptions must be an array", "$.assumptions")
    assumptions: list[dict[str, Any]] = []
    assumption_refs: set[str] = set()
    for index, raw in enumerate(raw_assumptions):
        path = f"$.assumptions[{index}]"
        item = _object(
            raw,
            path=path,
            required={"assumption_ref", "statement", "rationale", "confidence", "related_claim_refs"},
        )
        assumption_ref = _text(item["assumption_ref"], path=f"{path}.assumption_ref", max_bytes=64)
        if not _SOURCE_REF.fullmatch(assumption_ref) or assumption_ref in assumption_refs:
            _fail("invalid_assumption_ref", "assumption_ref must be unique and use identifier characters", f"{path}.assumption_ref")
        assumption_refs.add(assumption_ref)
        if not isinstance(item["related_claim_refs"], list):
            _fail("invalid_assumption_links", "related_claim_refs must be an array", f"{path}.related_claim_refs")
        related = []
        for link_index, raw_ref in enumerate(item["related_claim_refs"]):
            related_ref = _text(raw_ref, path=f"{path}.related_claim_refs[{link_index}]", max_bytes=64)
            if related_ref not in claim_refs:
                _fail("unknown_assumption_claim", f"unknown related claim {related_ref!r}", f"{path}.related_claim_refs[{link_index}]")
            related.append(related_ref)
        assumptions.append(
            {
                "assumption_ref": assumption_ref,
                "statement": _text(item["statement"], path=f"{path}.statement", max_bytes=4096),
                "rationale": _text(item["rationale"], path=f"{path}.rationale", max_bytes=2048),
                "confidence": _confidence(item["confidence"], path=f"{path}.confidence"),
                "related_claim_refs": related,
            }
        )

    sources.sort(key=lambda source: source["source_ref"])
    source_payloads.sort(key=lambda source: source["source_ref"])
    return {
        "schema_version": AUTHORING_REQUEST_SCHEMA,
        "request_id": request_id,
        "provider": provider_identity,
        "source_bundle_draft": {
            "schema_version": SOURCE_BUNDLE_DRAFT_SCHEMA,
            "request_id": request_id,
            "sources": sources,
        },
        "source_payloads": source_payloads,
        "interpretation": {
            "schema_version": PROVIDER_INTERPRETATION_SCHEMA,
            "source_bundle_id": None,
            "claims": claims,
            "conflicts": conflicts,
            "assumptions": assumptions,
        },
        "authoring_spans": claim_spans,
        "diagnostics": [],
    }


def lower_authoring_source(source: str | bytes) -> dict[str, Any]:
    """Parse and lower one JSON document, retaining parser spans on errors."""
    return lower_authoring_bundle(parse_authoring_source(source))


def scaffold_authoring_bundle(
    *,
    request_id: str,
    brief_text: str,
    concept_art: bytes,
    design_document: str,
    provider: Mapping[str, str] | None = None,
) -> dict[str, Any]:
    """Create an incomplete, claim-free source bundle for model authoring.

    The result is input scaffolding, not an authoring request: it cannot be
    lowered or bound until a provider supplies explicit claims and evidence.
    This avoids manufacturing observations from the source material.
    """

    request_id = _text(request_id, path="$.request_id", max_bytes=128)
    if any(char.isspace() for char in request_id) or request_id.startswith("source-bundle:"):
        _fail("invalid_request_id", "request_id must be a fresh token without whitespace", "$.request_id")
    if not isinstance(brief_text, str) or not brief_text.strip():
        _fail("invalid_scaffold_input", "brief_text must be nonempty", "$.brief_text")
    if not isinstance(design_document, str) or not design_document.strip():
        _fail("invalid_scaffold_input", "design_document must be nonempty", "$.design_document")
    if not isinstance(concept_art, bytes) or not concept_art:
        _fail("invalid_scaffold_input", "concept_art must be nonempty bytes", "$.concept_art")
    try:
        brief_bytes = brief_text.encode("utf-8", errors="strict")
        design_bytes = design_document.encode("utf-8", errors="strict")
    except UnicodeEncodeError:
        _fail("invalid_scaffold_input", "text sources must contain valid Unicode", "$.sources")
    if len(brief_bytes) > MAX_SOURCE_BYTES or len(design_bytes) > MAX_SOURCE_BYTES or len(concept_art) > MAX_SOURCE_BYTES:
        _fail("source_limit", f"each source must be at most {MAX_SOURCE_BYTES} bytes", "$.sources")
    identity = provider
    if identity is None:
        _fail("missing_provider", "scaffold requires the actual provider identity", "$.provider")
    if not isinstance(identity, Mapping) or set(identity) != {
        "provider_id",
        "provider_version",
        "protocol",
    }:
        _fail("invalid_provider", "provider must contain provider_id, provider_version, and protocol", "$.provider")
    provider_identity = {
        key: _text(identity[key], path=f"$.provider.{key}", max_bytes=128)
        for key in ("provider_id", "provider_version", "protocol")
    }

    def source(
        source_ref: str,
        kind: str,
        media_type: str,
        payload: bytes,
        origin_ref: str,
        *,
        encoded: bool = False,
    ) -> dict[str, Any]:
        content_key = "content_base64" if encoded else "content_text"
        content: str = (
            base64.b64encode(payload).decode("ascii") if encoded else payload.decode("utf-8")
        )
        return {
            "source_ref": source_ref,
            "kind": kind,
            "media_type": media_type,
            "content_sha256": "sha256:" + hashlib.sha256(payload).hexdigest(),
            content_key: content,
            "provenance": {"origin": "user_supplied", "origin_ref": origin_ref},
        }

    return {
        "schema_version": AUTHORING_SOURCE_SCHEMA,
        "request_id": request_id,
        "provider": provider_identity,
        "sources": [
            source("brief", "brief", "text/markdown", brief_bytes, f"authoring-input:{request_id}/brief"),
            source("concept", "concept_art", "image/png", concept_art, f"authoring-input:{request_id}/concept", encoded=True),
            source("design", "design_document", "text/plain", design_bytes, f"authoring-input:{request_id}/design"),
        ],
        "claims": [],
        "conflicts": [],
        "assumptions": [],
    }


def _canonical_json_bytes(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    ).encode("utf-8")


def _revalidate_request_artifact(value: dict[str, Any]) -> dict[str, Any]:
    expected_fields = {
        "schema_version",
        "request_id",
        "provider",
        "source_bundle_draft",
        "source_payloads",
        "interpretation",
        "authoring_spans",
        "diagnostics",
    }
    if set(value) != expected_fields:
        _fail("invalid_authoring_request", "authoring request has unknown or missing fields")
    draft = value["source_bundle_draft"]
    if not isinstance(draft, dict) or set(draft) != {"schema_version", "request_id", "sources"}:
        _fail("invalid_authoring_request", "source_bundle_draft has unknown or missing fields")
    if draft["schema_version"] != SOURCE_BUNDLE_DRAFT_SCHEMA:
        _fail("invalid_authoring_request", "source_bundle_draft schema is unsupported")
    if draft["request_id"] != value["request_id"]:
        _fail("stale_source_binding", "authoring request and source draft request_id differ")
    interpretation = value["interpretation"]
    if not isinstance(interpretation, dict) or set(interpretation) != {
        "schema_version", "source_bundle_id", "claims", "conflicts", "assumptions"
    }:
        _fail("invalid_authoring_request", "interpretation has unknown or missing fields")
    if interpretation["schema_version"] != PROVIDER_INTERPRETATION_SCHEMA or interpretation["source_bundle_id"] is not None:
        _fail("stale_source_binding", "authoring interpretation must be unbound")
    if not isinstance(value["diagnostics"], list) or value["diagnostics"]:
        _fail("invalid_authoring_request", "successful authoring requests must have no diagnostics")
    if not isinstance(value["source_payloads"], list):
        _fail("invalid_authoring_request", "source_payloads must be an array")
    payload_by_ref: dict[str, str] = {}
    for index, payload in enumerate(value["source_payloads"]):
        path = f"$.source_payloads[{index}]"
        if not isinstance(payload, dict) or set(payload) != {"source_ref", "content_base64"}:
            _fail("invalid_authoring_request", "source payload has unknown or missing fields", path)
        ref, encoded = payload["source_ref"], payload["content_base64"]
        if not isinstance(ref, str) or ref in payload_by_ref or not isinstance(encoded, str):
            _fail("invalid_authoring_request", "source payload ref or bytes are malformed or duplicated", path)
        try:
            decoded = base64.b64decode(encoded, validate=True)
        except (ValueError, binascii.Error):
            _fail("invalid_authoring_request", "source payload is not valid base64", path)
        if base64.b64encode(decoded).decode("ascii") != encoded:
            _fail("invalid_authoring_request", "source payload base64 is not canonical", path)
        payload_by_ref[ref] = encoded

    span_by_claim: dict[str, dict[str, int]] = {}
    if not isinstance(value["authoring_spans"], list):
        _fail("invalid_authoring_request", "authoring_spans must be an array")
    for index, entry in enumerate(value["authoring_spans"]):
        path = f"$.authoring_spans[{index}]"
        if not isinstance(entry, dict) or set(entry) != {"claim_ref", "span"}:
            _fail("invalid_authoring_request", "authoring span has unknown or missing fields", path)
        claim_ref = entry["claim_ref"]
        if not isinstance(claim_ref, str) or claim_ref in span_by_claim:
            _fail("invalid_authoring_request", "authoring span claim ref is malformed or repeated", path)
        span_by_claim[claim_ref] = _span(entry["span"], path=f"{path}.span")

    raw_sources = draft["sources"]
    if not isinstance(raw_sources, list):
        _fail("invalid_authoring_request", "source draft sources must be an array")
    source_inputs = []
    for index, record in enumerate(raw_sources):
        path = f"$.source_bundle_draft.sources[{index}]"
        if not isinstance(record, dict) or set(record) != {
            "source_ref", "kind", "content_sha256", "media_type", "provenance"
        }:
            _fail("invalid_authoring_request", "source draft has unknown or missing fields", path)
        ref = record["source_ref"]
        encoded = payload_by_ref.get(ref) if isinstance(ref, str) else None
        if encoded is None:
            _fail("stale_source_binding", "source draft has no matching payload", path)
        source_inputs.append(
            {
                **record,
                "content_base64": encoded,
            }
        )
    if set(payload_by_ref) != {record.get("source_ref") for record in raw_sources if isinstance(record, dict)}:
        _fail("stale_source_binding", "source payload refs do not match source draft refs")

    raw_claims = interpretation["claims"]
    if not isinstance(raw_claims, list):
        _fail("invalid_authoring_request", "interpretation claims must be an array")
    claims = []
    claim_refs = set()
    for index, claim in enumerate(raw_claims):
        path = f"$.interpretation.claims[{index}]"
        if not isinstance(claim, dict) or set(claim) != {
            "claim_ref", "epistemic_kind", "domain", "statement", "confidence", "evidence"
        }:
            _fail("invalid_authoring_request", "claim has unknown or missing fields", path)
        claim_ref = claim["claim_ref"]
        if not isinstance(claim_ref, str) or claim_ref in claim_refs:
            _fail("invalid_authoring_request", "claim refs are malformed or repeated", path)
        claim_refs.add(claim_ref)
        evidence = []
        if not isinstance(claim["evidence"], list):
            _fail("invalid_authoring_request", "claim evidence must be an array", f"{path}.evidence")
        for evidence_index, link in enumerate(claim["evidence"]):
            evidence_path = f"{path}.evidence[{evidence_index}]"
            if not isinstance(link, dict) or set(link) != {"source_ref", "region"}:
                _fail("invalid_authoring_request", "evidence has unknown or missing fields", evidence_path)
            evidence.append(link)
        lowered_claim = {
            key: claim[key]
            for key in ("claim_ref", "epistemic_kind", "domain", "statement", "confidence")
        }
        lowered_claim["evidence"] = evidence
        if claim_ref in span_by_claim:
            lowered_claim["declaration_span"] = span_by_claim[claim_ref]
        claims.append(lowered_claim)
    if set(span_by_claim) - claim_refs:
        _fail("invalid_authoring_request", "authoring span references an unknown claim")

    provider = value["provider"]
    if not isinstance(provider, dict) or set(provider) != {"provider_id", "provider_version", "protocol"}:
        _fail("invalid_authoring_request", "provider identity has unknown or missing fields")
    reconstructed = lower_authoring_bundle(
        {
            "schema_version": AUTHORING_SOURCE_SCHEMA,
            "request_id": value["request_id"],
            "provider": provider,
            "sources": source_inputs,
            "claims": claims,
            "conflicts": interpretation["conflicts"],
            "assumptions": interpretation["assumptions"],
        }
    )
    if reconstructed != value:
        _fail("invalid_authoring_request", "authoring request differs from its canonical lowering")
    return reconstructed


def bind_prepared_bundle(
    request: Mapping[str, Any], source_bundle: Mapping[str, Any]
) -> dict[str, Any]:
    """Bind source refs to a Rust-prepared bundle and emit the native intake.

    The source bundle must be the exact output from Rust's
    ``prepare-source-bundle`` command. This helper checks request, metadata,
    byte lengths, payload hashes, and identity syntax. Rust still validates the
    bundle identity and the resulting intake before semantic use.
    """
    if not isinstance(request, Mapping):
        _fail("expected_object", "authoring request must be an object")
    request_value = dict(request)
    if request_value.get("schema_version") != AUTHORING_REQUEST_SCHEMA:
        _fail("unsupported_schema", f"expected {AUTHORING_REQUEST_SCHEMA}", "$.schema_version")
    request_value = _revalidate_request_artifact(request_value)
    draft = request_value.get("source_bundle_draft")
    if not isinstance(draft, dict) or draft.get("schema_version") != SOURCE_BUNDLE_DRAFT_SCHEMA:
        _fail("invalid_authoring_request", "source_bundle_draft is missing or malformed", "$.source_bundle_draft")
    request_id = request_value.get("request_id")
    if draft.get("request_id") != request_id:
        _fail("stale_source_binding", "authoring request and source draft request_id differ", "$.request_id")
    if not isinstance(source_bundle, Mapping):
        _fail("expected_object", "prepared source bundle must be an object")
    bundle = dict(source_bundle)
    required_bundle_keys = {"schema_version", "request_id", "source_bundle_id", "sources"}
    if set(bundle) != required_bundle_keys:
        _fail("invalid_prepared_bundle", "prepared source bundle fields do not match the native schema")
    if bundle["schema_version"] != "luxel.source-bundle/v1":
        _fail("invalid_prepared_bundle", "prepared source bundle has the wrong schema", "$.schema_version")
    if bundle["request_id"] != request_id:
        _fail("stale_source_binding", "prepared bundle belongs to a different request_id", "$.request_id")
    source_bundle_id = bundle["source_bundle_id"]
    if not isinstance(source_bundle_id, str) or not _SOURCE_BUNDLE_ID.fullmatch(source_bundle_id):
        _fail("malformed_source_bundle_id", "prepared source bundle ID is malformed", "$.source_bundle_id")
    if not isinstance(bundle["sources"], list):
        _fail("invalid_prepared_bundle", "prepared bundle sources must be an array", "$.sources")

    draft_sources = draft.get("sources")
    payload_records = request_value.get("source_payloads")
    if not isinstance(draft_sources, list) or not isinstance(payload_records, list):
        _fail("invalid_authoring_request", "source draft or payload list is malformed")
    payloads: dict[str, bytes] = {}
    for index, record in enumerate(payload_records):
        path = f"$.source_payloads[{index}]"
        if not isinstance(record, dict) or set(record) != {"source_ref", "content_base64"}:
            _fail("invalid_authoring_request", "source payload record has unknown or missing fields", path)
        source_ref = record["source_ref"]
        try:
            payload = base64.b64decode(record["content_base64"], validate=True)
        except (TypeError, ValueError, binascii.Error):
            _fail("invalid_authoring_request", "source payload is not valid base64", path)
        if base64.b64encode(payload).decode("ascii") != record["content_base64"] or source_ref in payloads:
            _fail("invalid_authoring_request", "source payload encoding or ref is duplicated", path)
        payloads[source_ref] = payload
    if set(payloads) != {source.get("source_ref") for source in draft_sources if isinstance(source, dict)}:
        _fail("stale_source_binding", "source payload refs do not exactly match the source draft")

    source_ids_by_ref: dict[str, str] = {}
    remaining: dict[str, dict[str, Any]] = {}
    for index, raw in enumerate(bundle["sources"]):
        path = f"$.sources[{index}]"
        if not isinstance(raw, dict) or set(raw) != {
            "source_id", "kind", "content_sha256", "byte_length", "media_type", "provenance"
        }:
            _fail("invalid_prepared_bundle", "prepared source record has unknown or missing fields", path)
        source_id = raw["source_id"]
        if not isinstance(source_id, str) or not _SOURCE_ID.fullmatch(source_id):
            _fail("malformed_source_id", "prepared source ID is malformed", f"{path}.source_id")
        if source_id in remaining:
            _fail("invalid_prepared_bundle", "prepared bundle repeats a source ID", f"{path}.source_id")
        remaining[source_id] = raw

    if len(bundle["sources"]) != len(draft_sources):
        _fail("stale_source_binding", "prepared source count differs from authoring request")
    for index, expected in enumerate(draft_sources):
        path = f"$.source_bundle_draft.sources[{index}]"
        if not isinstance(expected, dict):
            _fail("invalid_authoring_request", "source draft record must be an object", path)
        source_ref = expected.get("source_ref")
        payload = payloads.get(source_ref)
        if payload is None:
            _fail("stale_source_binding", "source draft has no corresponding bytes", path)
        digest = "sha256:" + hashlib.sha256(payload).hexdigest()
        if digest != expected.get("content_sha256"):
            _fail("stale_source_binding", "source bytes no longer match the source draft digest", path)
        matches = [
            source_id
            for source_id, record in remaining.items()
            if record.get("kind") == expected.get("kind")
            and record.get("content_sha256") == expected.get("content_sha256")
            and record.get("byte_length") == len(payload)
            and record.get("media_type") == expected.get("media_type")
            and record.get("provenance") == expected.get("provenance")
        ]
        if len(matches) != 1:
            _fail("stale_source_binding", "prepared bundle source metadata differs or is ambiguous", path)
        source_ids_by_ref[source_ref] = matches[0]
        del remaining[matches[0]]
    if remaining:
        _fail("stale_source_binding", "prepared bundle contains unbound sources")

    interpretation = request_value.get("interpretation")
    if not isinstance(interpretation, dict) or interpretation.get("schema_version") != PROVIDER_INTERPRETATION_SCHEMA:
        _fail("invalid_authoring_request", "typed interpretation is missing or malformed", "$.interpretation")
    if interpretation.get("source_bundle_id") is not None:
        _fail("stale_source_binding", "unbound authoring request already pins a source_bundle_id", "$.interpretation.source_bundle_id")
    bound_interpretation = {
        "schema_version": PROVIDER_INTERPRETATION_SCHEMA,
        "source_bundle_id": source_bundle_id,
        "claims": [],
        "conflicts": interpretation.get("conflicts"),
        "assumptions": interpretation.get("assumptions"),
    }
    if not isinstance(interpretation.get("claims"), list):
        _fail("invalid_authoring_request", "interpretation claims must be an array", "$.interpretation.claims")
    for index, claim in enumerate(interpretation["claims"]):
        if not isinstance(claim, dict) or set(claim) != {
            "claim_ref", "epistemic_kind", "domain", "statement", "confidence", "evidence"
        }:
            _fail("invalid_authoring_request", "claim has unknown or missing fields", f"$.interpretation.claims[{index}]")
        evidence = []
        for evidence_index, link in enumerate(claim["evidence"]):
            if not isinstance(link, dict) or set(link) != {"source_ref", "region"}:
                _fail("invalid_authoring_request", "evidence has unknown or missing fields", f"$.interpretation.claims[{index}].evidence[{evidence_index}]")
            source_ref = link["source_ref"]
            if source_ref not in source_ids_by_ref:
                _fail("stale_source_binding", f"claim references unbound source {source_ref!r}")
            evidence.append({"source_id": source_ids_by_ref[source_ref], "region": link["region"]})
        bound_interpretation["claims"].append(
            {
                "claim_ref": claim["claim_ref"],
                "epistemic_kind": claim["epistemic_kind"],
                "domain": claim["domain"],
                "statement": claim["statement"],
                "confidence": claim["confidence"],
                "evidence": evidence,
            }
        )

    provider = request_value.get("provider")
    if not isinstance(provider, dict) or set(provider) != {"provider_id", "provider_version", "protocol"}:
        _fail("invalid_authoring_request", "provider identity is malformed", "$.provider")
    response_bytes = _canonical_json_bytes(bound_interpretation)
    intake_draft = {
        "schema_version": INTAKE_DRAFT_SCHEMA,
        "source_bundle_id": source_bundle_id,
        "provider": {
            **provider,
            "request_source_bundle_id": source_bundle_id,
            "response_sha256": "sha256:" + hashlib.sha256(response_bytes).hexdigest(),
        },
        "interpretation": bound_interpretation,
    }
    payloads_by_id = [
        {"source_id": source_ids_by_ref[source_ref], "content_base64": base64.b64encode(payloads[source_ref]).decode("ascii")}
        for source_ref in sorted(source_ids_by_ref)
    ]
    payloads_by_id.sort(key=lambda item: item["source_id"])
    return {
        "schema_version": NATIVE_INTAKE_REQUEST_SCHEMA,
        "request_id": request_id,
        "source_bundle_id": source_bundle_id,
        "intake_draft": intake_draft,
        "provider_response": bound_interpretation,
        "provider_response_utf8": response_bytes.decode("utf-8"),
        "source_payloads_by_id": payloads_by_id,
        "authoring_spans": request_value.get("authoring_spans", []),
        "diagnostics": [],
    }


def _authoring_cli_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Parse-only provider-neutral Luxel authoring lowering"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("lower", "bind"):
        subparser = subparsers.add_parser(command)
        subparser.add_argument("--input", required=True, type=Path)
        if command == "bind":
            subparser.add_argument("--prepared-bundle", required=True, type=Path)
        subparser.add_argument("--output", required=True, type=Path)
    return parser


def _read_authoring_json(path: Path) -> dict[str, Any]:
    try:
        value = parse_authoring_source(path.read_bytes())
    except OSError as error:
        raise AuthoringError(
            AuthoringDiagnostic("input_read_error", str(error), str(path))
        ) from error
    return value


def _write_authoring_json(path: Path, value: Mapping[str, Any]) -> None:
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n",
            encoding="utf-8",
        )
    except OSError as error:
        raise AuthoringError(
            AuthoringDiagnostic("output_write_error", str(error), str(path))
        ) from error


def _authoring_cli_main(argv: list[str] | None = None) -> int:
    args = _authoring_cli_parser().parse_args(argv)
    try:
        request = lower_authoring_source(args.input.read_bytes()) if args.command == "lower" else _read_authoring_json(args.input)
        if args.command == "bind":
            request = bind_prepared_bundle(request, _read_authoring_json(args.prepared_bundle))
        _write_authoring_json(args.output, request)
    except (AuthoringError, OSError) as error:
        diagnostic = error.diagnostic.to_dict() if isinstance(error, AuthoringError) else {
            "code": "input_read_error",
            "detail": str(error),
        }
        print(json.dumps({"status": "rejected", "diagnostics": [diagnostic]}, sort_keys=True), file=sys.stderr)
        return 2
    print(
        json.dumps(
            {
                "status": "lowered" if args.command == "lower" else "bound",
                "schema_version": request["schema_version"],
                "output": str(args.output),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(_authoring_cli_main())
