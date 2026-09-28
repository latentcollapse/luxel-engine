"""Native WGE vertical-slice integration and handoff controls."""

from __future__ import annotations

import hashlib
import json
import struct
import subprocess
import tempfile
import unittest
import zipfile
import zlib
from pathlib import Path

from pipeline.rigging_provider import generate_rigged_character_control
from pipeline.wge_engine_neutral import BAD_GLB_SHA256, OrchestrationError, run_smoke
from pipeline.wge_native_mvp import (
    NativeMvpCommands,
    _extract_snapshot_archive,
    run_native_mvp,
    verify_native_snapshot,
)
from tests.test_wge_engine_neutral import BAD_GLB


ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "world_core" / "target" / "debug"
RIGGING_REQUEST_TEMPLATE = ROOT / "tests" / "fixtures" / "rigging" / "blender_control_request.json"


def _digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _fresh_concept_png() -> bytes:
    """Return a small deterministic concept control with real composition."""

    width, height = 32, 24
    rows = []
    for y in range(height):
        pixels = bytearray()
        for x in range(width):
            if y < 8:
                color = (47, 78, 112, 255)  # sky
            elif y < 16:
                ridge = 13 + (x - 16) * (x - 16) // 42
                color = (72, 91, 75, 255) if y >= ridge else (117, 133, 112, 255)
            else:
                color = (102, 112, 72, 255)  # relay meadow
            water_x = 7 + (y - 8) * 2
            if 8 <= y < 20 and abs(x - water_x) <= 1:
                color = (45, 106, 134, 255)
            if 22 <= x <= 24 and 12 <= y <= 15:
                color = (224, 167, 67, 255)  # objective beacon
            pixels.extend(color)
        rows.append(b"\x00" + bytes(pixels))

    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(b"".join(rows), level=9))
        + chunk(b"IEND", b"")
    )


def _commands() -> NativeMvpCommands:
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
            "-p",
            "wge-project-ledger",
            "-p",
            "wge-asset-contract",
        ],
        cwd=ROOT / "world_core",
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(result.stdout + result.stderr)
    return NativeMvpCommands(
        str(TARGET / "wge-intake-repair"),
        str(TARGET / "wge-reference-runtime"),
        str(TARGET / "wge-certification-authority"),
        str(TARGET / "wge-project-ledger"),
        "julia",
        str(TARGET / "wge-asset-contract"),
    )


def _fresh_native_input(root: Path) -> Path:
    """Create a non-fixture source bundle and provider interpretation."""

    source = root / "input"
    (source / "sources").mkdir(parents=True)
    brief = source / "sources/brief.md"
    concept = source / "sources/concept.png"
    layout = source / "layout.json"
    brief.write_text(
        "# Fresh Cedar Relay slice\n\n"
        "Build a readable cedar relay where the player crosses the hollow,\n"
        "survives the guard encounter, and reaches the cedar beacon.\n",
        encoding="utf-8",
    )
    # A valid deterministic composition control generated for this test run,
    # rather than a checked-in concept-art fixture.
    concept.write_bytes(_fresh_concept_png())
    # This layout is authored here from the typed contract instead of being a
    # mutation of the checked-in Riverwatch example. It exercises a distinct
    # world identity, dimensions, feature set, route, and encounter.
    layout_value = {
        "schema_version": "wge.authored-world-layout/v1",
        "world_id": "cedar_saddle_relay",
        "title": "Cedar Saddle Relay",
        "width_m": 104.0,
        "length_m": 80.0,
        "resolution": 49,
        "seed": 930211,
        "terrain": {
            "base_elevation_m": 10.0,
            "noise_amplitude_m": 0.19,
            "features": [
                {
                    "feature_id": "north_iron_ridge",
                    "center_xz_m": [-22.0, 12.0],
                    "radius_x_m": 14.0,
                    "radius_z_m": 11.0,
                    "elevation_m": 6.5,
                },
                {
                    "feature_id": "east_cedar_spur",
                    "center_xz_m": [28.0, -4.0],
                    "radius_x_m": 12.0,
                    "radius_z_m": 15.0,
                    "elevation_m": 4.2,
                },
                {
                    "feature_id": "relay_hollow",
                    "center_xz_m": [4.0, 0.0],
                    "radius_x_m": 9.0,
                    "radius_z_m": 8.0,
                    "elevation_m": -2.0,
                },
            ],
        },
        "regions": [
            {
                "region_id": "north_marsh",
                "code": 1,
                "priority": 10,
                "blocks_traversal": True,
                "polygon_xz_m": [
                    [-51.0, 30.0],
                    [-32.0, 30.0],
                    [-32.0, 38.0],
                    [-51.0, 38.0],
                ],
            }
        ],
        "obstacles": [
            {
                "obstacle_id": "fallen_cedar",
                "center_xz_m": [20.0, 16.0],
                "radius_m": 3.0,
                "height_m": 5.0,
            }
        ],
        "spawns": [
            {
                "spawn_id": "relay_start",
                "role": "player_start",
                "position_xz_m": [-45.0, -30.0],
            },
            {
                "spawn_id": "hollow_guard_spawn",
                "role": "opponent",
                "position_xz_m": [4.0, 0.0],
            },
        ],
        "encounters": [
            {
                "encounter_id": "hollow_guard",
                "center_xz_m": [4.0, 0.0],
                "radius_m": 5.0,
                "required": True,
                "traversal_order": 1,
                "opponent_spawn_id": "hollow_guard_spawn",
            }
        ],
        "traversal": {
            "start_spawn_id": "relay_start",
            "objective_id": "cedar_beacon",
            "objective_position_xz_m": [45.0, 30.0],
            "agent_radius_m": 0.6,
            "maximum_grade": 0.55,
        },
        "reference_camera": {
            "projection": "top_down_orthographic",
            "width_px": 320,
            "height_px": 240,
            "orthographic_span_m": 118.0,
            "distance_m": 175.0,
        },
    }
    _write_json(layout, layout_value)
    (source / "sources/layout.json").write_bytes(layout.read_bytes())

    source_entries = [
        ("brief.md", "brief", "text/markdown", brief),
        ("concept.png", "concept_art", "image/png", concept),
        ("layout.json", "design_document", "application/json", source / "sources/layout.json"),
    ]
    draft = {
        "schema_version": "wge.source-bundle-draft/v1",
        "request_id": "fresh-native-mvp-source-001",
        "sources": [
            {
                "source_ref": reference,
                "kind": kind,
                "content_sha256": _digest(path.read_bytes()),
                "media_type": media_type,
                "provenance": {
                    "origin": "user_supplied",
                    "origin_ref": f"test://fresh-native-mvp/{reference}",
                    "provider_id": None,
                    "provider_version": None,
                },
            }
            for reference, kind, media_type, path in source_entries
        ],
    }
    draft_path = source / "source-bundle-draft.json"
    _write_json(draft_path, draft)
    bundle_path = root / "bundle.json"
    result = subprocess.run(
        [
            str(TARGET / "wge-intake-repair"),
            "prepare-source-bundle",
            str(draft_path),
            str(bundle_path),
            *(f"{reference}={source / 'sources' / reference}" for reference, *_ in source_entries),
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(result.stdout + result.stderr)
    bundle = json.loads(bundle_path.read_text(encoding="utf-8"))
    records = {record["kind"]: record for record in bundle["sources"]}
    provider = {
        "schema_version": "wge.provider-interpretation/v1",
        "source_bundle_id": bundle["source_bundle_id"],
        "claims": [
            {
                "claim_ref": "fresh-route-observation",
                "epistemic_kind": "observation",
                "domain": "world",
                "statement": "The fresh brief requests a traversable relay route.",
                "confidence": 0.97,
                "evidence": [
                    {
                        "source_id": records["brief"]["source_id"],
                        "region": {"kind": "text_span", "start_byte": 0, "end_byte": len(brief.read_bytes())},
                    }
                ],
            },
            {
                "claim_ref": "fresh-composition-observation",
                "epistemic_kind": "observation",
                "domain": "visual",
                "statement": "The fresh concept image establishes a readable relay composition.",
                "confidence": 0.84,
                "evidence": [
                    {
                        "source_id": records["concept_art"]["source_id"],
                        "region": {"kind": "image_rect", "x_min": 0.0, "y_min": 0.0, "x_max": 1.0, "y_max": 1.0},
                    }
                ],
            },
            {
                "claim_ref": "fresh-layout-observation",
                "epistemic_kind": "observation",
                "domain": "world",
                "statement": "The fresh layout is the executable world design document.",
                "confidence": 1.0,
                "evidence": [
                    {
                        "source_id": records["design_document"]["source_id"],
                        "region": {"kind": "text_span", "start_byte": 0, "end_byte": len(layout.read_bytes())},
                    }
                ],
            },
            {
                "claim_ref": "fresh-playability-inference",
                "epistemic_kind": "inference",
                "domain": "gameplay",
                "statement": "The fresh authored route can support a deterministic encounter and objective.",
                "confidence": 0.89,
                "evidence": [{"source_id": records["design_document"]["source_id"], "region": None}],
            },
        ],
        "conflicts": [],
        "assumptions": [],
    }
    provider_path = source / "provider-response.json"
    _write_json(provider_path, provider)
    _write_json(
        source / "intake-draft.json",
        {
            "schema_version": "wge.semantic-intake-draft/v1",
            "source_bundle_id": bundle["source_bundle_id"],
            "provider": {
                "provider_id": "fresh-test-interpreter",
                "provider_version": "1.0",
                "protocol": "typed-provider-json",
                "request_source_bundle_id": bundle["source_bundle_id"],
                "response_sha256": _digest(provider_path.read_bytes()),
            },
            "interpretation": provider,
        },
    )
    mesh_path = source / "static-mesh-source.json"
    _write_json(
        mesh_path,
        {
            "schema_version": "wge.static-mesh-source/v1",
            "asset_id": "relay_stone",
            "positions_m": [[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1], [1, 1, 1]],
            "triangle_indices": [0, 1, 2, 0, 3, 1, 0, 2, 3, 1, 3, 2],
            "material_slots": ["stone"],
        },
    )
    _write_json(
        source / "asset-package.json",
        {
            "schema_version": "wge.asset-package/v1",
            "asset_id": "relay_stone",
            "source_sha256": _digest(mesh_path.read_bytes()),
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
    before_layout = dict(layout_value)
    before_layout["reference_camera"] = dict(layout_value["reference_camera"])
    before_layout["reference_camera"]["orthographic_span_m"] = 400.0
    _write_json(source / "before-layout.json", before_layout)
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
    _write_json(
        source / "repair-proposal-draft.json",
        {
            "schema_version": "wge.repair-proposal-draft/v1",
            "candidate_before_sha256": zero,
            "failed_layer": "visual_quality",
            "failure_evidence": failure_evidence,
            "edit_class": "adjust_camera_or_lighting",
            "authorized_targets": [
                {"artifact_id": artifact_id, "before_sha256": zero} for artifact_id in changed_ids
            ],
            "max_artifact_changes": len(changed_ids),
            "diagnosis": "The initial camera span under-covers the authored relay pass.",
            "rationale": "Restore the authored reference composition and remeasure the native visual gate.",
        },
    )
    _write_json(
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


def _good_rigging_inputs(root: Path) -> tuple[Path, Path]:
    glb = root / "hero-control.glb"
    generated = generate_rigged_character_control(glb)
    if generated.returncode != 0:
        raise AssertionError(generated.stdout + generated.stderr)
    request = root / "hero-control.request.json"
    request_value = json.loads(RIGGING_REQUEST_TEMPLATE.read_text(encoding="utf-8"))
    inspection = subprocess.run(
        [str(TARGET / "wge-asset-contract"), str(glb), "--kind", "character"],
        text=True,
        capture_output=True,
        check=False,
    )
    if inspection.returncode != 0:
        raise AssertionError(inspection.stdout + inspection.stderr)
    request_value["expected_source_sha256"] = json.loads(inspection.stdout)["inspection"]["identity"][
        "source_sha256"
    ]
    _write_json(request, request_value)
    return glb, request


def _artifact_ref(
    preflight: Path,
    candidate: dict[str, object],
    artifact_id: str,
    *,
    kind: str,
    schema_version: str,
    producer: str,
    reference_id: str | None = None,
) -> dict[str, str]:
    entries = {
        item["artifact_id"]: item
        for item in candidate["artifacts"]
        if isinstance(item, dict) and isinstance(item.get("artifact_id"), str)
    }
    item = entries[artifact_id]
    return {
        "artifact_id": reference_id or artifact_id,
        "kind": kind,
        "schema_version": schema_version,
        "path": f"native/current/artifacts/{item['path']}",
        "sha256": item["sha256"],
        "producer": producer,
    }


def _project_template(source: Path, preflight: Path) -> Path:
    intake = json.loads((preflight / "intake" / "semantic-intake.json").read_text(encoding="utf-8"))
    bundle = json.loads((preflight / "intake" / "source-bundle.json").read_text(encoding="utf-8"))
    draft = json.loads((source / "source-bundle-draft.json").read_text(encoding="utf-8"))
    candidate = json.loads((preflight / "current-candidate.json").read_text(encoding="utf-8"))
    world = json.loads((preflight / "current" / "world_artifact.json").read_text(encoding="utf-8"))
    traversal = json.loads((preflight / "current" / "traversal_evidence.json").read_text(encoding="utf-8"))
    gameplay = json.loads(
        (preflight / "current" / "gameplay_world_binding.json").read_text(encoding="utf-8")
    )
    trace = json.loads((preflight / "current" / "gameplay_trace.json").read_text(encoding="utf-8"))
    mesh = json.loads((source / "static-mesh-source.json").read_text(encoding="utf-8"))
    package = json.loads((source / "asset-package.json").read_text(encoding="utf-8"))
    hero_receipt = json.loads(
        (preflight / "current" / "artifacts" / "hero-preparation--rigging_preparation_receipt").read_text(
            encoding="utf-8"
        )
    )
    hero_request = json.loads((preflight / "current" / "artifacts" / "hero-request--rigging_request").read_text(encoding="utf-8"))
    layout = json.loads((source / "layout.json").read_text(encoding="utf-8"))

    records_by_kind = {record["kind"]: record for record in bundle["sources"]}
    drafts_by_identity = {
        (item["kind"], item["content_sha256"]): item["source_ref"] for item in draft["sources"]
    }
    source_paths = {}
    for record in intake["sources"]:
        source_ref = drafts_by_identity[(record["kind"], record["content_sha256"])]
        source_paths[record["source_id"]] = f"native/handoff_snapshot/source/sources/{source_ref}"
    brief_record = records_by_kind["brief"]
    brief_ref = drafts_by_identity[("brief", brief_record["content_sha256"])]

    world_ref = _artifact_ref(
        preflight,
        candidate,
        "world-artifact",
        kind="reference_world",
        schema_version=world["schema_version"],
        producer="wge-reference-runtime",
        reference_id=world["artifact_id"],
    )
    traversal_ref = _artifact_ref(
        preflight,
        candidate,
        "traversal-evidence",
        kind="traversal_evidence",
        schema_version=traversal["body"]["schema_version"],
        producer="wge-reference-runtime",
        reference_id="traversal-" + traversal["evidence_sha256"].removeprefix("sha256:"),
    )
    gameplay_ref = _artifact_ref(
        preflight,
        candidate,
        "gameplay-binding",
        kind="gameplay_runtime",
        schema_version=gameplay["body"]["schema_version"],
        producer="wge-reference-runtime",
    )
    trace_ref = _artifact_ref(
        preflight,
        candidate,
        "gameplay-trace",
        kind="gameplay_trace",
        schema_version=trace["schema_version"],
        producer="wge-reference-runtime",
    )
    mesh_ref = _artifact_ref(
        preflight,
        candidate,
        "static-mesh-source",
        kind="static_mesh_source",
        schema_version=mesh["schema_version"],
        producer="wge-source-intake",
    )
    package_ref = _artifact_ref(
        preflight,
        candidate,
        "asset-package",
        kind="asset_package",
        schema_version=package["schema_version"],
        producer="wge-asset-contract",
    )
    hero_glb_ref = _artifact_ref(
        preflight,
        candidate,
        "hero-glb",
        kind="character_model",
        schema_version="model/gltf-binary",
        producer="wge-rigging-provider",
    )
    hero_request_ref = _artifact_ref(
        preflight,
        candidate,
        "hero-request",
        kind="asset_preparation_request",
        schema_version=hero_request["schema_version"],
        producer="wge-rigging-provider",
    )
    hero_package_ref = _artifact_ref(
        preflight,
        candidate,
        "hero-preparation",
        kind="asset_runtime_package",
        schema_version=hero_receipt["schema_version"],
        producer="wge-asset-contract",
    )
    refs = [world_ref, traversal_ref, gameplay_ref, trace_ref, mesh_ref, package_ref, hero_glb_ref, hero_request_ref, hero_package_ref]
    graph = [{"artifact": ref, "dependencies": []} for ref in refs]
    artifact_ids = [ref["artifact_id"] for ref in refs]
    gates = [
        {"gate_id": "semantic", "evidence_kind": "semantic"},
        {"gate_id": "world", "evidence_kind": "world"},
        {"gate_id": "gameplay", "evidence_kind": "runtime"},
        {"gate_id": "asset", "evidence_kind": "asset"},
        {"gate_id": "rigging", "evidence_kind": "asset"},
        {"gate_id": "visual", "evidence_kind": "visual"},
        {"gate_id": "repair", "evidence_kind": "repair"},
    ]
    style_sources = [record["source_id"] for record in intake["sources"]]
    template = {
        "schema_version": "wge.project-template/v1",
        "expected_intake_id": intake["intake_id"],
        "project_id": "cedar-saddle-native-mvp",
        "title": layout["title"],
        "brief_source_id": brief_record["source_id"],
        "brief_text": (source / "sources" / brief_ref).read_text(encoding="utf-8"),
        "source_paths": source_paths,
        "style_target": {
            "visual_language": "readable stylized highland relay pass",
            "palette": ["moss green", "warm stone", "cool water"],
            "camera": "top-down orthographic reference capture",
            "reference_source_ids": style_sources,
        },
        "design_constraints": [
            {
                "constraint_id": "reachable-objective",
                "category": "mechanical",
                "statement": "The pass beacon must be reachable from the player start.",
                "required": True,
            },
            {
                "constraint_id": "fixed-runtime",
                "category": "runtime",
                "statement": "The vertical slice must replay at a fixed 30 Hz tick rate.",
                "required": True,
            },
        ],
        "target": {
            "engine": "reference",
            "engine_version": "wge.reference-runtime/v1",
            "platform": "linux-desktop",
            "coordinate_system": "right-handed-xz-up-y",
            "build_profile": "wge-native-reference",
        },
        "world": {
            "world_id": world["body"]["world_id"],
            "dimensions_m": [layout["width_m"], layout["length_m"]],
            "terrain": world_ref,
            "collision": world_ref,
            "navigation": world_ref,
            "reference_runtime": {
                "world_artifact": world_ref,
                "traversal_evidence": traversal_ref,
            },
            "spawns": [
                {
                    "spawn_id": spawn["spawn_id"],
                    "team": "player" if spawn["role"] == "player_start" else "opponent",
                    "position_xz_m": spawn["position_xz_m"],
                    "required": True,
                }
                for spawn in layout["spawns"]
            ],
            "objective": {
                "objective_id": layout["traversal"]["objective_id"],
                "kind": "reach_and_interact",
                "required_interaction_tag": "cedar_beacon",
                "win_condition": "objective_reached",
                "loss_condition": "player_defeated",
            },
        },
        "assets": [
            {
                "asset_id": "relay_stone",
                "source": mesh_ref,
                "runtime_package": package_ref,
                "role": "environment",
                "required_features": ["collision", "lod", "material_slots"],
            },
            {
                "asset_id": "hero_control",
                "source": hero_glb_ref,
                "runtime_package": hero_package_ref,
                "role": "character",
                "required_features": ["rig", "idle", "locomotion", "attack", "weapon_mount", "collision", "lod"],
            },
        ],
        "gameplay": {
            "runtime_package": gameplay_ref,
            "input_trace": trace_ref,
            "start_entity_id": layout["traversal"]["start_spawn_id"],
            "objective_id": layout["traversal"]["objective_id"],
        },
        "artifact_graph": graph,
        "work_orders": [
            {
                "schema_version": "wge.work-order/v1",
                "work_order_id": "build-native-vertical-slice",
                "operation": "build_and_verify",
                "snapshot_id": "current-certified",
                "allowed_artifacts": artifact_ids,
                "required_capabilities": ["semantic", "world", "gameplay", "asset", "rigging", "visual", "repair"],
                "required_gates": [gate["gate_id"] for gate in gates],
            }
        ],
        "required_gates": gates,
    }
    output = source.parent / "project-template.json"
    _write_json(output, template)
    return output


class NativeMvpIntegrationTest(unittest.TestCase):
    @unittest.skipUnless(BAD_GLB.is_file(), "the permanent supplied negative-control GLB is unavailable")
    def test_native_mvp_certifies_and_reopens_a_real_vertical_slice(self) -> None:
        commands = _commands()
        with tempfile.TemporaryDirectory(prefix="wge-native-mvp-test-") as temporary:
            root = Path(temporary)
            source = _fresh_native_input(root)
            glb, request = _good_rigging_inputs(root)
            preflight = root / "preflight"
            run_smoke(
                source,
                preflight,
                commands.smoke(),
                bad_glb=BAD_GLB,
                # This is a staging run used to derive the typed project
                # template.  Native-MVP certification is exercised by the
                # subsequent run_native_mvp call, after the template binds
                # the compiled spec to the staged artifacts.
                profile="engine-neutral",
                rigging_glb=glb,
                rigging_request=request,
            )
            template = _project_template(source, preflight)

            first = run_native_mvp(
                source,
                root / "first",
                commands,
                project_template=template,
                rigging_glb=glb,
                rigging_request=request,
                bad_glb=BAD_GLB,
            )
            report = first.certification_report
            self.assertEqual(report["status"], "native_mvp_certified")
            self.assertEqual(report["project_id"], "cedar-saddle-native-mvp")
            self.assertEqual(set(report["deferred_gates"]), {"unity_import", "unity_build", "unity_playthrough"})
            # The report retains the ten promotable/deferred receipts; the
            # rejected before-repair visual receipt remains in the request as
            # diagnostic evidence and is intentionally not counted as a
            # promoted receipt.
            self.assertEqual(len(report["receipts"]), 10)
            rigging_decision = next(item for item in report["receipts"] if item["gate_id"] == "rigging")
            self.assertEqual(
                rigging_decision["validator_id"],
                "wge.validator.rigging-runtime-preparation/v1",
            )
            self.assertIn("independently revalidated", rigging_decision["detail"])
            self.assertTrue((first.output_dir / "project_spec.json").is_file())
            candidate = json.loads(
                (first.output_dir / "native/current-candidate.json").read_text(encoding="utf-8")
            )
            candidate_ids = {item["artifact_id"] for item in candidate["artifacts"]}
            self.assertIn("project-spec", candidate_ids)
            self.assertIn("project-template", candidate_ids)
            self.assertEqual(candidate["candidate_sha256"], report["candidate_sha256"])
            spec = json.loads((first.output_dir / "project_spec.json").read_text(encoding="utf-8"))
            self.assertEqual(spec["brief"]["intake_provenance"]["intake_id"], json.loads((preflight / "intake/semantic-intake.json").read_text())["intake_id"])
            self.assertIsNotNone(spec["world"]["reference_runtime"])
            self.assertEqual(spec["gameplay"]["objective_id"], "cedar_beacon")
            application = json.loads(
                (first.output_dir / "native/repair/application.json").read_text(encoding="utf-8")
            )
            self.assertEqual(application["schema_version"], "wge.repair-application/v1")
            self.assertEqual(application["artifact_id"], "authored-layout")
            self.assertEqual(
                _digest(BAD_GLB.read_bytes()),
                BAD_GLB_SHA256,
            )
            self.assertTrue((first.output_dir / "native/negative_controls" / BAD_GLB.name).is_file())
            verification = json.loads((first.output_dir / "snapshot-verification.json").read_text())
            self.assertEqual(verification["status"], "verified")
            self.assertTrue(verification["independent_authority_revalidation"])
            self.assertEqual(verification["archive_revalidation"]["status"], "verified")
            self.assertTrue(verification["archive_revalidation"]["independent_authority_revalidation"])
            # The native authority must require the project definition; a
            # wrapper-supplied binding alone is not sufficient policy.
            request_value = json.loads(
                (
                    first.snapshot_dir
                    / "native/handoff_snapshot/evidence/certification-request.json"
                ).read_text(encoding="utf-8")
            )
            semantic_receipt = next(
                receipt for receipt in request_value["receipts"] if receipt["gate_id"] == "semantic"
            )
            semantic_receipt["receipt_id"] = ""
            semantic_receipt["observed_input_sha256"] = ""
            semantic_receipt["payload"].pop("project_spec_artifact_id", None)
            semantic_receipt["payload"].pop("project_template_artifact_id", None)
            semantic_draft = root / "native-semantic-without-project-definition.json"
            semantic_sealed = root / "native-semantic-without-project-definition.sealed.json"
            _write_json(semantic_draft, semantic_receipt)
            sealed = subprocess.run(
                [commands.authority, "seal", str(semantic_draft), str(semantic_sealed)],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(sealed.returncode, 0, sealed.stdout + sealed.stderr)
            request_value["receipts"] = [
                json.loads(semantic_sealed.read_text(encoding="utf-8"))
                if receipt["gate_id"] == "semantic"
                else receipt
                for receipt in request_value["receipts"]
            ]
            invalid_request = root / "native-request-without-project-definition.json"
            _write_json(invalid_request, request_value)
            rejected = subprocess.run(
                [
                    commands.authority,
                    "validate",
                    str(invalid_request),
                    "--artifact-root",
                    str(first.snapshot_dir / "native/handoff_snapshot/evidence"),
                    "--profile",
                    "native-mvp",
                ],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertNotEqual(rejected.returncode, 0, rejected.stdout + rejected.stderr)
            with zipfile.ZipFile(first.snapshot_zip) as archive:
                self.assertTrue(archive.namelist())
                self.assertTrue(all(not name.startswith("snapshot/") for name in archive.namelist()))

            original_spec = (first.snapshot_dir / "project_spec.json").read_bytes()
            (first.snapshot_dir / "project_spec.json").write_bytes(original_spec + b"\n")
            with self.assertRaisesRegex(OrchestrationError, "snapshot file is stale"):
                verify_native_snapshot(first, commands)
            (first.snapshot_dir / "project_spec.json").write_bytes(original_spec)
            self.assertEqual(verify_native_snapshot(first, commands)["status"], "verified")

            second = run_native_mvp(
                source,
                root / "second",
                commands,
                project_template=template,
                rigging_glb=glb,
                rigging_request=request,
                bad_glb=BAD_GLB,
            )
            self.assertEqual(first.snapshot_sha256, second.snapshot_sha256)
            self.assertEqual(
                _digest(first.snapshot_zip.read_bytes()),
                _digest(second.snapshot_zip.read_bytes()),
            )
            self.assertEqual(
                (first.snapshot_dir / "snapshot-manifest.json").read_bytes(),
                (second.snapshot_dir / "snapshot-manifest.json").read_bytes(),
            )

    def test_bad_glb_cannot_enter_native_rigging_path(self) -> None:
        if not BAD_GLB.is_file():
            self.skipTest("the permanent supplied negative-control GLB is unavailable")
        commands = _commands()
        with tempfile.TemporaryDirectory(prefix="wge-native-mvp-negative-") as temporary:
            root = Path(temporary)
            source = _fresh_native_input(root)
            request = json.loads(RIGGING_REQUEST_TEMPLATE.read_text(encoding="utf-8"))
            request_path = root / "bad.request.json"
            _write_json(request_path, request)
            output = root / "output"
            with self.assertRaisesRegex(OrchestrationError, "native command exited 3"):
                run_smoke(
                    source,
                    output,
                    commands.smoke(),
                    bad_glb=BAD_GLB,
                    profile="native-mvp",
                    rigging_glb=BAD_GLB,
                    rigging_request=request_path,
                )

    def test_snapshot_archive_rejects_path_traversal(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wge-native-mvp-archive-negative-") as temporary:
            root = Path(temporary)
            archive_path = root / "malicious.zip"
            with zipfile.ZipFile(archive_path, "w") as archive:
                archive.writestr("../escape.txt", b"not a snapshot")
            with self.assertRaisesRegex(OrchestrationError, "archive member path is unsafe"):
                _extract_snapshot_archive(archive_path, root / "extracted")


if __name__ == "__main__":
    unittest.main()
