"""Fresh-source integration tests for the engine-neutral WGE harness."""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from pipeline.wge_engine_neutral import BAD_GLB_SHA256, NativeCommands, OrchestrationError, run_smoke


ROOT = Path(__file__).resolve().parents[1]
ALPINE = ROOT.parent / "Game Projects/Codeweald/godot_renderer/concept_batches/codeweald_alpine_arena_v1"
BRIEF = ALPINE / "vision_annotation_task.md"
CONCEPT = ALPINE / "source/overview.png"
LAYOUT = ROOT / "world_core/crates/reference_runtime/examples/riverwatch.layout.json"
BAD_GLB = Path("/home/mattc/Pictures/Generated 2D Images/sample_2026-09-26T091412.074.glb")


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def native_commands() -> NativeCommands:
    result = subprocess.run(
        [
            "cargo",
            "build",
            "--offline",
            "-p",
            "wge-intake-repair-contract",
            "-p",
            "wge-reference-runtime",
            "-p",
            "wge-certification-authority",
        ],
        cwd=ROOT / "world_core",
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(result.stdout + result.stderr)
    target = ROOT / "world_core/target/debug"
    return NativeCommands(
        str(target / "wge-intake-repair"),
        str(target / "wge-reference-runtime"),
        str(target / "wge-certification-authority"),
        "julia",
    )


def make_input(root: Path) -> Path:
    source = root / "input"
    (source / "sources").mkdir(parents=True)
    brief = source / "sources/brief.md"
    concept = source / "sources/overview.png"
    layout = source / "layout.json"
    shutil.copyfile(BRIEF, brief)
    shutil.copyfile(CONCEPT, concept)
    shutil.copyfile(LAYOUT, layout)
    (source / "sources/layout.json").write_bytes(layout.read_bytes())

    before_layout = json.loads(layout.read_text(encoding="utf-8"))
    before_layout["reference_camera"]["orthographic_span_m"] = 400.0
    write_json(source / "before-layout.json", before_layout)

    source_entries = [
        ("brief.md", "brief", "text/markdown", brief),
        ("overview.png", "concept_art", "image/png", concept),
        ("layout.json", "design_document", "application/json", source / "sources/layout.json"),
    ]
    write_json(
        source / "source-bundle-draft.json",
        {
            "schema_version": "wge.source-bundle-draft/v1",
            "request_id": "fresh-engine-neutral-test-001",
            "sources": [
                {
                    "source_ref": reference,
                    "kind": kind,
                    "content_sha256": digest(path.read_bytes()),
                    "media_type": media_type,
                    "provenance": {
                        "origin": "user_supplied",
                        "origin_ref": f"test://codeweald-alpine/{reference}",
                        "provider_id": None,
                        "provider_version": None,
                    },
                }
                for reference, kind, media_type, path in source_entries
            ],
        },
    )

    intake_cli = ROOT / "world_core/target/debug/wge-intake-repair"
    bundle_path = root / "bundle.json"
    command = [
        str(intake_cli),
        "prepare-source-bundle",
        str(source / "source-bundle-draft.json"),
        str(bundle_path),
    ]
    command.extend(
        f"{reference}={source / 'sources' / reference}"
        for reference, _kind, _media_type, _path in source_entries
    )
    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True, check=False)
    if result.returncode != 0:
        raise AssertionError(result.stdout + result.stderr)
    bundle = json.loads(bundle_path.read_text(encoding="utf-8"))
    records = {record["kind"]: record for record in bundle["sources"]}
    provider = {
        "schema_version": "wge.provider-interpretation/v1",
        "source_bundle_id": bundle["source_bundle_id"],
        "claims": [
            {
                "claim_ref": "brief-route",
                "epistemic_kind": "observation",
                "domain": "world",
                "statement": "The brief describes a traversable relay route.",
                "confidence": 0.96,
                "evidence": [
                    {
                        "source_id": records["brief"]["source_id"],
                        "region": {
                            "kind": "text_span",
                            "start_byte": 0,
                            "end_byte": min(64, len(brief.read_bytes())),
                        },
                    }
                ],
            },
            {
                "claim_ref": "concept-readability",
                "epistemic_kind": "observation",
                "domain": "visual",
                "statement": "The concept image is a source reference for readable composition.",
                "confidence": 0.82,
                "evidence": [
                    {
                        "source_id": records["concept_art"]["source_id"],
                        "region": {
                            "kind": "image_rect",
                            "x_min": 0.0,
                            "y_min": 0.0,
                            "x_max": 1.0,
                            "y_max": 1.0,
                        },
                    }
                ],
            },
            {
                "claim_ref": "layout-authored",
                "epistemic_kind": "observation",
                "domain": "world",
                "statement": "The authored layout is the executable world design document.",
                "confidence": 1.0,
                "evidence": [
                    {
                        "source_id": records["design_document"]["source_id"],
                        "region": {
                            "kind": "text_span",
                            "start_byte": 0,
                            "end_byte": len((source / "sources/layout.json").read_bytes()),
                        },
                    }
                ],
            },
            {
                "claim_ref": "slice-inference",
                "epistemic_kind": "inference",
                "domain": "gameplay",
                "statement": "The authored route can support a deterministic traversal and objective encounter.",
                "confidence": 0.88,
                "evidence": [
                    {"source_id": records["design_document"]["source_id"], "region": None}
                ],
            },
        ],
        "conflicts": [],
        "assumptions": [],
    }
    provider_path = source / "provider-response.json"
    write_json(provider_path, provider)
    write_json(
        source / "intake-draft.json",
        {
            "schema_version": "wge.semantic-intake-draft/v1",
            "source_bundle_id": bundle["source_bundle_id"],
            "provider": {
                "provider_id": "test-source-interpreter",
                "provider_version": "1.0",
                "protocol": "typed-provider-json",
                "request_source_bundle_id": bundle["source_bundle_id"],
                "response_sha256": digest(provider_path.read_bytes()),
            },
            "interpretation": provider,
        },
    )

    mesh = {
        "schema_version": "wge.static-mesh-source/v1",
        "asset_id": "relay_stone",
        "positions_m": [[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1], [1, 1, 1]],
        "triangle_indices": [0, 1, 2, 0, 3, 1, 0, 2, 3, 1, 3, 2],
        "material_slots": ["stone"],
    }
    mesh_path = source / "static-mesh-source.json"
    write_json(mesh_path, mesh)
    write_json(
        source / "asset-package.json",
        {
            "schema_version": "wge.asset-package/v1",
            "asset_id": "relay_stone",
            "source_sha256": digest(mesh_path.read_bytes()),
            "asset_use": "static_environment",
            "bounds_min_m": [0.0, 0.0, 0.0],
            "bounds_max_m": [1.0, 1.0, 1.0],
            "pivot_m": [0.0, 0.0, 0.0],
            "collision_bounds_min_m": [0.0, 0.0, 0.0],
            "collision_bounds_max_m": [1.0, 1.0, 1.0],
            "material_slots": ["stone"],
            "lod_triangle_counts": [4],
        },
    )

    zero = "sha256:" + "0" * 64
    one = "sha256:" + "1" * 64
    changed_ids = [
        "authored-layout",
        "world-artifact",
        "traversal-evidence",
        "gameplay-binding",
        "reference-capture",
        "visual-evidence",
    ]
    failure_evidence = {
        "validator_id": "wge.validator.visual-reference/v1",
        "schema_version": "wge.visual-receipt/v1",
        "gate_id": "visual_quality",
        "candidate_sha256": zero,
        "receipt_sha256": zero,
    }
    write_json(
        source / "repair-proposal-draft.json",
        {
            "schema_version": "wge.repair-proposal-draft/v1",
            "candidate_before_sha256": zero,
            "failed_layer": "visual_quality",
            "failure_evidence": failure_evidence,
            "edit_class": "adjust_camera_or_lighting",
            "authorized_targets": [
                {"artifact_id": artifact_id, "before_sha256": zero}
                for artifact_id in changed_ids
            ],
            "max_artifact_changes": len(changed_ids),
            "diagnosis": "The reference camera span makes the authored world visually under-covered.",
            "rationale": "Restore the specified authored camera composition and remeasure the native visual gate.",
        },
    )
    write_json(
        source / "repair-delta-draft.json",
        {
            "schema_version": "wge.repair-evidence-delta-draft/v1",
            "proposal_id": "repair:pending",
            "candidate_before_sha256": zero,
            "candidate_after_sha256": one,
            "before_evidence": failure_evidence,
            "after_evidence": failure_evidence,
            "changed_artifacts": [
                {"artifact_id": artifact_id, "before_sha256": zero, "after_sha256": one}
                for artifact_id in changed_ids
            ],
        },
    )
    return source


class WgeEngineNeutralIntegrationTests(unittest.TestCase):
    def test_fresh_source_is_certified_with_explicit_deferred_gates_and_handoff(self) -> None:
        if not all(path.is_file() for path in (BRIEF, CONCEPT, LAYOUT, BAD_GLB)):
            self.skipTest("the shared Codeweald source corpus or supplied GLB is unavailable")
        commands = native_commands()
        with tempfile.TemporaryDirectory(prefix="wge-engine-neutral-test-") as temporary:
            root = Path(temporary)
            source = make_input(root)
            result = run_smoke(source, root / "output", commands, bad_glb=BAD_GLB)
            report = result.certification_report
            self.assertEqual(report["status"], "engine_neutral_certified")
            self.assertEqual(
                set(report["deferred_gates"]),
                {"rigging", "unity_import", "unity_build", "unity_playthrough"},
            )
            self.assertEqual(len(report["receipts"]), 10)
            self.assertTrue((result.handoff_snapshot / "handoff-manifest.json").is_file())
            self.assertTrue((result.output_dir / "negative_controls" / BAD_GLB.name).is_file())
            self.assertEqual(digest(BAD_GLB.read_bytes()), BAD_GLB_SHA256)
            summary = json.loads(
                (result.output_dir / "native-stage-summary.json").read_text(encoding="utf-8")
            )
            self.assertEqual(summary["certification_status"], "engine_neutral_certified")
            self.assertEqual(summary["bad_glb_sha256"], BAD_GLB_SHA256)

    def test_layout_source_must_be_bound_byte_for_byte(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wge-engine-neutral-input-") as temporary:
            root = Path(temporary)
            source = make_input(root)
            (source / "sources/layout.json").write_bytes(b"detached design document\n")
            with self.assertRaisesRegex(OrchestrationError, "byte-for-byte"):
                run_smoke(source, root / "output", native_commands(), bad_glb=None)


if __name__ == "__main__":
    unittest.main()
