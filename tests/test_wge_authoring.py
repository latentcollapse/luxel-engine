"""Focused checks for the parse-only provider-neutral authoring boundary."""

from __future__ import annotations

import base64
import copy
import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from pipeline.wge_authoring import (
    AUTHORING_SOURCE_SCHEMA,
    AuthoringError,
    bind_prepared_bundle,
    lower_authoring_bundle,
    lower_authoring_source,
    parse_authoring_source,
    scaffold_authoring_bundle,
)


ROOT = Path(__file__).resolve().parents[1]
INTAKE_CLI = ROOT / "world_core/target/debug/wge-intake-repair"


def _sha256(content: bytes) -> str:
    return "sha256:" + hashlib.sha256(content).hexdigest()


def _good_bundle() -> dict:
    brief = b"Reach the relay before sunset.\n"
    concept = b"concept-art-control-bytes"
    design = b"The route crosses a marsh and ends at a relay.\n"
    brief_start = brief.index(b"Reach")
    design_start = design.index(b"marsh")
    return {
        "schema_version": AUTHORING_SOURCE_SCHEMA,
        "request_id": "authoring-test-001",
        "provider": {
            "provider_id": "test-provider",
            "provider_version": "2",
            "protocol": "typed-json/v1",
        },
        "sources": [
            {
                "source_ref": "brief",
                "kind": "brief",
                "media_type": "text/markdown",
                "content_sha256": _sha256(brief),
                "content_text": brief.decode(),
                "provenance": {"origin": "user_supplied", "origin_ref": "brief.md"},
            },
            {
                "source_ref": "concept",
                "kind": "concept_art",
                "media_type": "image/png",
                "content_sha256": _sha256(concept),
                "content_base64": base64.b64encode(concept).decode("ascii"),
                "provenance": {"origin": "user_supplied", "origin_ref": "relay-concept.png"},
            },
            {
                "source_ref": "design",
                "kind": "design_document",
                "media_type": "text/plain",
                "content_sha256": _sha256(design),
                "content_text": design.decode(),
                "provenance": {
                    "origin": "retrieved",
                    "origin_ref": "design/route-notes.txt",
                    "provider_id": "project-archive",
                    "provider_version": "2026-09",
                },
            },
        ],
        "claims": [
            {
                "claim_ref": "relay-goal",
                "epistemic_kind": "observation",
                "domain": "gameplay",
                "statement": "The player must reach the relay before sunset.",
                "confidence": 1.0,
                "evidence": [
                    {
                        "source_ref": "brief",
                        "region": {
                            "kind": "text_span",
                            "start_byte": brief_start,
                            "end_byte": brief_start + len(b"Reach the relay"),
                        },
                    }
                ],
                "declaration_span": {"line": 4, "column": 2, "end_line": 4, "end_column": 71},
            },
            {
                "claim_ref": "marsh-route",
                "epistemic_kind": "inference",
                "domain": "world",
                "statement": "A traversable marsh corridor is likely part of the route.",
                "confidence": 0.82,
                "evidence": [
                    {
                        "source_ref": "design",
                        "region": {
                            "kind": "text_span",
                            "start_byte": design_start,
                            "end_byte": design_start + len(b"marsh"),
                        },
                    },
                    {
                        "source_ref": "concept",
                        "region": {
                            "kind": "image_rect",
                            "x_min": 0.12,
                            "y_min": 0.20,
                            "x_max": 0.78,
                            "y_max": 0.86,
                        },
                    },
                ],
            },
        ],
        "conflicts": [],
        "assumptions": [
            {
                "assumption_ref": "walkable-player",
                "statement": "The player can traverse the marsh on foot.",
                "rationale": "No vehicle or swimming mechanic is specified.",
                "confidence": 0.64,
                "related_claim_refs": ["relay-goal", "marsh-route"],
            }
        ],
    }


def _prepared_bundle(request: dict) -> dict:
    draft = request["source_bundle_draft"]
    payloads = {item["source_ref"]: item["content_base64"] for item in request["source_payloads"]}
    records = []
    for index, source in enumerate(draft["sources"], 1):
        records.append(
            {
                "source_id": "source:sha256:" + str(index) * 64,
                "kind": source["kind"],
                "content_sha256": source["content_sha256"],
                "byte_length": len(base64.b64decode(payloads[source["source_ref"]])),
                "media_type": source["media_type"],
                "provenance": source["provenance"],
            }
        )
    return {
        "schema_version": "wge.source-bundle/v1",
        "request_id": draft["request_id"],
        "source_bundle_id": "source-bundle:sha256:" + "a" * 64,
        "sources": records,
    }


class WgeAuthoringTests(unittest.TestCase):
    def test_cli_exposes_the_same_parse_only_lowering_and_binding(self) -> None:
        request = _good_bundle()
        with tempfile.TemporaryDirectory(prefix="wge-authoring-cli-") as temporary:
            root = Path(temporary)
            source_path = root / "authoring.json"
            lowered_path = root / "lowered.json"
            bound_path = root / "bound.json"
            prepared_path = root / "prepared.json"
            source_path.write_text(json.dumps(request), encoding="utf-8")
            lowered = subprocess.run(
                [
                    "python3",
                    "-m",
                    "pipeline.wge_authoring",
                    "lower",
                    "--input",
                    str(source_path),
                    "--output",
                    str(lowered_path),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(lowered.returncode, 0, lowered.stderr)
            lowered_value = json.loads(lowered_path.read_text(encoding="utf-8"))
            prepared_path.write_text(
                json.dumps(_prepared_bundle(lowered_value)), encoding="utf-8"
            )
            bound = subprocess.run(
                [
                    "python3",
                    "-m",
                    "pipeline.wge_authoring",
                    "bind",
                    "--input",
                    str(lowered_path),
                    "--prepared-bundle",
                    str(prepared_path),
                    "--output",
                    str(bound_path),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(bound.returncode, 0, bound.stderr)
            self.assertEqual(
                json.loads(bound_path.read_text(encoding="utf-8"))["schema_version"],
                "wge.native-intake-request/v1",
            )

    def test_good_bundle_lowers_to_native_intake_shape(self) -> None:
        request = lower_authoring_bundle(_good_bundle())
        prepared = _prepared_bundle(request)
        native_request = bind_prepared_bundle(request, prepared)

        self.assertEqual(request["source_bundle_draft"]["schema_version"], "wge.source-bundle-draft/v1")
        self.assertIsNone(request["interpretation"]["source_bundle_id"])
        self.assertEqual(len(request["interpretation"]["claims"]), 2)
        self.assertEqual(native_request["intake_draft"]["schema_version"], "wge.semantic-intake-draft/v1")
        self.assertEqual(native_request["intake_draft"]["source_bundle_id"], prepared["source_bundle_id"])
        self.assertEqual(
            native_request["intake_draft"]["provider"]["response_sha256"],
            _sha256(native_request["provider_response_utf8"].encode("utf-8")),
        )
        self.assertEqual(
            {link["source_id"] for claim in native_request["provider_response"]["claims"] for link in claim["evidence"]},
            {source["source_id"] for source in prepared["sources"]},
        )

    def test_rust_intake_cli_accepts_lowered_and_bound_request(self) -> None:
        if not INTAKE_CLI.is_file():
            self.skipTest("native intake CLI has not been built")
        bundle = _good_bundle()
        bundle["claims"].append(
            {
                "claim_ref": "no-marsh",
                "epistemic_kind": "inference",
                "domain": "world",
                "statement": "The route avoids the marsh.",
                "confidence": 0.57,
                "evidence": [
                    {
                        "source_ref": "design",
                        "region": {"kind": "text_span", "start_byte": 0, "end_byte": 3},
                    }
                ],
            }
        )
        bundle["conflicts"] = [
            {
                "conflict_ref": "route-ambiguity",
                "left_claim_ref": "marsh-route",
                "right_claim_ref": "no-marsh",
                "explanation": "The authored sources support competing route interpretations.",
            }
        ]
        request = lower_authoring_bundle(bundle)
        with tempfile.TemporaryDirectory(prefix="wge-authoring-native-") as temporary:
            root = Path(temporary)
            source_root = root / "sources"
            source_root.mkdir()
            draft_path = root / "source-bundle-draft.json"
            draft_path.write_text(
                json.dumps(request["source_bundle_draft"], sort_keys=True), encoding="utf-8"
            )
            source_arguments = []
            for payload in request["source_payloads"]:
                source_path = source_root / payload["source_ref"]
                source_path.write_bytes(base64.b64decode(payload["content_base64"]))
                source_arguments.append(f"{payload['source_ref']}={source_path}")
            bundle_path = root / "source-bundle.json"
            prepared = subprocess.run(
                [
                    str(INTAKE_CLI),
                    "prepare-source-bundle",
                    str(draft_path),
                    str(bundle_path),
                    *source_arguments,
                ],
                cwd=root,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(prepared.returncode, 0, prepared.stderr)
            bound = bind_prepared_bundle(request, json.loads(bundle_path.read_text(encoding="utf-8")))
            intake_draft_path = root / "intake-draft.json"
            intake_draft_path.write_text(
                json.dumps(bound["intake_draft"], sort_keys=True), encoding="utf-8"
            )
            response_path = root / "provider-response.json"
            response_path.write_text(bound["provider_response_utf8"], encoding="utf-8")
            intake_path = root / "semantic-intake.json"
            source_id_arguments = []
            for payload in bound["source_payloads_by_id"]:
                source_path = source_root / payload["source_id"]
                source_path.write_bytes(base64.b64decode(payload["content_base64"]))
                source_id_arguments.append(f"{payload['source_id']}={source_path}")
            normalized = subprocess.run(
                [
                    str(INTAKE_CLI),
                    "normalize-intake",
                    str(intake_draft_path),
                    str(bundle_path),
                    str(response_path),
                    str(intake_path),
                    *source_id_arguments,
                ],
                cwd=root,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(normalized.returncode, 0, normalized.stderr)
            semantic_intake = json.loads(intake_path.read_text(encoding="utf-8"))
            self.assertEqual(semantic_intake["schema_version"], "wge.semantic-intake/v1")
            self.assertEqual(len(semantic_intake["observations"]), 1)
            self.assertEqual(len(semantic_intake["inferences"]), 2)
            self.assertEqual(len(semantic_intake["assumptions"]), 1)
            self.assertEqual(len(semantic_intake["conflicts"]), 1)

    def test_source_provenance_and_evidence_spans_survive_lowering_and_binding(self) -> None:
        request = lower_authoring_bundle(_good_bundle())
        by_ref = {source["source_ref"]: source for source in request["source_bundle_draft"]["sources"]}
        self.assertEqual(by_ref["design"]["provenance"]["origin"], "retrieved")
        self.assertEqual(by_ref["design"]["provenance"]["provider_id"], "project-archive")
        self.assertEqual(
            request["interpretation"]["claims"][0]["evidence"][0]["region"],
            {"kind": "text_span", "start_byte": 0, "end_byte": len(b"Reach the relay")},
        )
        self.assertEqual(
            request["authoring_spans"],
            [{"claim_ref": "relay-goal", "span": {"line": 4, "column": 2, "end_line": 4, "end_column": 71}}],
        )
        native_request = bind_prepared_bundle(request, _prepared_bundle(request))
        first_claim = next(
            claim for claim in native_request["provider_response"]["claims"] if claim["claim_ref"] == "relay-goal"
        )
        self.assertEqual(first_claim["evidence"][0]["region"]["kind"], "text_span")
        self.assertEqual(native_request["authoring_spans"][0]["span"]["line"], 4)

    def test_parser_rejects_executable_or_unknown_syntax_with_diagnostics(self) -> None:
        with self.assertRaises(AuthoringError) as unsafe:
            parse_authoring_source('__import__("os").system("touch /tmp/wge-authoring-pwned")')
        self.assertEqual(unsafe.exception.diagnostic.code, "invalid_json")
        self.assertIsNotNone(unsafe.exception.diagnostic.line)
        with self.assertRaises(AuthoringError) as unknown:
            lower_authoring_bundle({**_good_bundle(), "execute": "run this"})
        self.assertEqual(unknown.exception.diagnostic.code, "unknown_field")
        self.assertEqual(unknown.exception.diagnostic.path, "$")
        with self.assertRaises(AuthoringError) as duplicate:
            parse_authoring_source('{"schema_version":"a","schema_version":"b"}')
        self.assertEqual(duplicate.exception.diagnostic.code, "duplicate_json_key")

    def test_known_bad_bindings_fail_and_conflicts_remain_explicit(self) -> None:
        conflict_bundle = _good_bundle()
        conflict_bundle["claims"].append(
            {
                "claim_ref": "no-marsh",
                "epistemic_kind": "inference",
                "domain": "world",
                "statement": "The route avoids the marsh.",
                "confidence": 0.57,
                "evidence": [
                    {
                        "source_ref": "design",
                        "region": {"kind": "text_span", "start_byte": 0, "end_byte": 3},
                    }
                ],
            }
        )
        conflict_bundle["conflicts"] = [
            {
                "conflict_ref": "route-ambiguity",
                "left_claim_ref": "marsh-route",
                "right_claim_ref": "no-marsh",
                "explanation": "The brief image and route note support incompatible layouts.",
            }
        ]
        request = lower_authoring_bundle(conflict_bundle)
        self.assertEqual(request["interpretation"]["conflicts"][0]["conflict_ref"], "route-ambiguity")
        native = bind_prepared_bundle(request, _prepared_bundle(request))
        self.assertEqual(native["provider_response"]["conflicts"][0]["left_claim_ref"], "marsh-route")
        self.assertIsNone(native["provider_response"].get("resolution"))

        stale_bundle = _prepared_bundle(request)
        stale_bundle["request_id"] = "different-request"
        with self.assertRaises(AuthoringError) as stale:
            bind_prepared_bundle(request, stale_bundle)
        self.assertEqual(stale.exception.diagnostic.code, "stale_source_binding")

        bad_interpretation = copy.deepcopy(_good_bundle())
        bad_interpretation["claims"][0]["evidence"][0]["source_ref"] = "missing-source"
        with self.assertRaises(AuthoringError) as unknown_source:
            lower_authoring_bundle(bad_interpretation)
        self.assertEqual(unknown_source.exception.diagnostic.code, "unknown_source_ref")

        bad_hash = copy.deepcopy(_good_bundle())
        bad_hash["sources"][0]["content_sha256"] = "sha256:" + "0" * 64
        with self.assertRaises(AuthoringError) as mismatch:
            lower_authoring_bundle(bad_hash)
        self.assertEqual(mismatch.exception.diagnostic.code, "stale_source_binding")

        malformed_enum = copy.deepcopy(_good_bundle())
        malformed_enum["sources"][0]["kind"] = []
        with self.assertRaises(AuthoringError) as malformed:
            lower_authoring_bundle(malformed_enum)
        self.assertEqual(malformed.exception.diagnostic.code, "unknown_source_kind")

        tampered_request = lower_authoring_bundle(_good_bundle())
        tampered_request["interpretation"]["claims"][0]["evidence"][0]["region"]["end_byte"] = 9999
        with self.assertRaises(AuthoringError) as tampered:
            bind_prepared_bundle(tampered_request, _prepared_bundle(tampered_request))
        self.assertEqual(tampered.exception.diagnostic.code, "invalid_region")

    def test_json_source_lowering_is_deterministic_and_does_not_infer_claims(self) -> None:
        source = json.dumps(_good_bundle(), sort_keys=True, separators=(",", ":"))
        first = lower_authoring_source(source)
        second = lower_authoring_source(source)
        self.assertEqual(first, second)
        self.assertEqual(len(first["interpretation"]["claims"]), 2)
        self.assertEqual(first["diagnostics"], [])

    def test_scaffold_is_valid_editable_source_not_a_semantic_shortcut(self) -> None:
        scaffold = scaffold_authoring_bundle(
            request_id="scaffold-test-001",
            brief_text="Cross the relay marsh.\n",
            concept_art=b"png-control",
            design_document="Relay route notes.\n",
            provider={
                "provider_id": "test-provider",
                "provider_version": "2",
                "protocol": "typed-json/v1",
            },
        )
        self.assertEqual(scaffold["schema_version"], AUTHORING_SOURCE_SCHEMA)
        self.assertEqual(scaffold["claims"], [])
        self.assertEqual(len(scaffold["sources"]), 3)
        with self.assertRaises(AuthoringError) as incomplete:
            lower_authoring_bundle(scaffold)
        self.assertEqual(incomplete.exception.diagnostic.code, "missing_claims")


if __name__ == "__main__":
    unittest.main()
