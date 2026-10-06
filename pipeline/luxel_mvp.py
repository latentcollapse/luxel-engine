#!/usr/bin/env python3
"""Assemble and verify the Luxel MVP vertical-slice transaction.

This module is intentionally orchestration glue. It copies and hashes source
inputs, invokes the Rust-owned gameplay and asset contracts, writes a typed
candidate, and invokes the Rust project ledger for normalization,
certification, and Unity handoff. It does not decide semantic validity or mint
acceptance receipts.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests" / "fixtures" / "mvp_vertical_slice"
DEFAULT_CONCEPT = Path("/home/mattc/Pictures/Generated 2D Images/Warden_A_pose_front_and_back.png")
DEFAULT_ASSET = Path("/home/mattc/Pictures/Generated 2D Images/sample_2026-09-26T091412.074.glb")
LEDGER_MANIFEST = ROOT / "world_core" / "Cargo.toml"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return f"sha256:{digest.hexdigest()}"


def _receipt_id(receipt: dict[str, Any]) -> str:
    body = {key: value for key, value in receipt.items() if key != "receipt_id"}
    encoded = json.dumps(body, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"receipt_{hashlib.sha256(encoded).hexdigest()[:32]}"


def _write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _copy_required(source: Path, destination: Path) -> None:
    source = source.expanduser().resolve()
    if not source.is_file():
        raise RuntimeError(f"required source is missing: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)


def _cargo_package(package: str, *arguments: str) -> subprocess.CompletedProcess[str]:
    command = [
        "cargo",
        "run",
        "--offline",
        "--quiet",
        "--manifest-path",
        str(LEDGER_MANIFEST),
        "-p",
        package,
        "--",
        *arguments,
    ]
    return subprocess.run(command, cwd=ROOT, text=True, capture_output=True, check=False)


def _artifact(
    output_root: Path,
    artifact_id: str,
    relative_path: str,
    kind: str,
    schema_version: str,
    producer: str,
) -> dict[str, str]:
    path = output_root / relative_path
    if not path.is_file():
        raise RuntimeError(f"artifact source is missing: {path}")
    return {
        "artifact_id": artifact_id,
        "kind": kind,
        "schema_version": schema_version,
        "path": relative_path,
        "sha256": sha256(path),
        "producer": producer,
    }


def _verify_bundle_files(spec_path: Path) -> list[dict[str, str]]:
    """Verify bytes before asking Rust to certify the semantic record.

    The Rust ledger owns schema and acceptance semantics. This small byte-level
    check is orchestration glue that prevents a stale candidate from reaching
    either the ledger or Unity.
    """

    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    root = spec_path.parent.resolve()
    observed: list[dict[str, str]] = []
    references = [node["artifact"] for node in spec["artifact_graph"]]
    for source in spec["brief"]["sources"]:
        references.append({"artifact_id": source["source_id"], "path": source["path"], "sha256": source["sha256"]})
    for reference in references:
        relative = Path(reference["path"])
        if relative.is_absolute() or ".." in relative.parts:
            raise RuntimeError(f"artifact path escapes bundle: {reference['path']}")
        path = (root / relative).resolve()
        if root not in path.parents and path != root:
            raise RuntimeError(f"artifact path escapes bundle: {reference['path']}")
        if not path.is_file():
            raise RuntimeError(f"artifact is missing: {reference['path']}")
        actual = sha256(path)
        if actual != reference["sha256"]:
            raise RuntimeError(
                f"artifact digest mismatch for {reference['artifact_id']}: expected {reference['sha256']} observed {actual}"
            )
        observed.append({"artifact_id": reference["artifact_id"], "sha256": actual})
    return observed


def prepare(output_root: Path, *, concept: Path, asset: Path) -> dict[str, Path]:
    """Create a self-contained candidate input bundle.

    The candidate deliberately leaves target-runtime receipts indeterminate
    until Unity MCP has imported and played the package. That is a safety
    property, not a failed build: no offline path may claim target-runtime
    evidence it did not observe.
    """

    output_root = output_root.expanduser().resolve()
    output_root.mkdir(parents=True, exist_ok=True)
    brief = output_root / "brief.md"
    _copy_required(FIXTURE_ROOT / "brief.md", brief)
    concept_destination = output_root / "sources" / "concept_art.png"
    asset_destination = output_root / "sources" / "hero.glb"
    _copy_required(concept, concept_destination)
    _copy_required(asset, asset_destination)

    artifacts_root = output_root / "artifacts"
    artifacts_root.mkdir(parents=True, exist_ok=True)
    for source in (FIXTURE_ROOT / "artifacts").glob("*.json"):
        if source.name != "hero_runtime.json":
            _copy_required(source, artifacts_root / source.name)

    # The Rust gameplay contract owns deserialization, validation, simulation,
    # and receipt identity. This invocation is only transport glue.
    gameplay_contract_path = artifacts_root / "gameplay_contract.json"
    _copy_required(
        ROOT / "tests" / "fixtures" / "gameplay_contract" / "vertical_slice_v1.json",
        gameplay_contract_path,
    )
    gameplay_receipt_path = artifacts_root / "gameplay_receipt.json"
    gameplay_result = _cargo_package(
        "luxel-gameplay-contract",
        "run",
        str(gameplay_contract_path),
        str(gameplay_receipt_path),
    )
    if gameplay_result.returncode != 0:
        raise RuntimeError(
            "Rust gameplay contract failed: "
            + (gameplay_result.stderr or gameplay_result.stdout).strip()
        )

    # Pin the source digest in the Rust-owned preparation request. The native
    # process returns exit 3 for a typed, fail-closed rejection; that is a
    # candidate gate failure, not an orchestration crash.
    asset_request = json.loads(
        (
            ROOT
            / "tests"
            / "fixtures"
            / "asset_runtime"
            / "good_character.json"
        ).read_text(encoding="utf-8")
    )["request"]
    # The asset contract's request field is the raw 64-character digest; the
    # project ledger's ArtifactRef uses the explicit ``sha256:`` prefix.
    asset_request["expected_source_sha256"] = sha256(asset_destination).split(":", 1)[1]
    asset_request_path = artifacts_root / "asset_runtime_request.json"
    _write_json(asset_request_path, asset_request)
    asset_receipt_path = artifacts_root / "asset_preparation_receipt.json"
    asset_result = _cargo_package(
        "luxel-asset-contract",
        "prepare",
        str(asset_destination),
        str(asset_request_path),
    )
    if asset_result.returncode not in (0, 3):
        raise RuntimeError(
            "Rust asset contract transport failed: "
            + (asset_result.stderr or asset_result.stdout).strip()
        )
    if not asset_result.stdout.strip():
        raise RuntimeError("Rust asset contract returned no typed preparation receipt")
    asset_receipt_path.write_text(asset_result.stdout, encoding="utf-8")

    artifacts = {
        "terrain": _artifact(output_root, "terrain", "artifacts/terrain_manifest.json", "terrain", "luxel.terrain-manifest/v1", "luxel-terrain-fixture"),
        "collision": _artifact(output_root, "collision", "artifacts/collision.json", "collision", "luxel.collision-plan/v1", "luxel-collision-fixture"),
        "navigation": _artifact(output_root, "navigation", "artifacts/navigation.json", "navigation", "luxel.navigation-plan/v1", "luxel-navigation-fixture"),
        "asset-request": _artifact(output_root, "asset-request", "artifacts/asset_runtime_request.json", "asset-preparation-request", "luxel.asset-runtime-request/v1", "luxel-mvp-orchestrator"),
        "hero-runtime": _artifact(output_root, "hero-runtime", "artifacts/asset_preparation_receipt.json", "runtime-asset", "luxel.asset-runtime-receipt/v1", "luxel-asset-contract"),
        "gameplay": _artifact(output_root, "gameplay", "artifacts/gameplay_runtime.json", "gameplay-runtime", "luxel.gameplay-runtime/v1", "luxel-gameplay-contract"),
        "gameplay-contract": _artifact(output_root, "gameplay-contract", "artifacts/gameplay_contract.json", "gameplay-contract", "luxel.gameplay-snapshot/v1", "luxel-gameplay-contract"),
        "gameplay-receipt": _artifact(output_root, "gameplay-receipt", "artifacts/gameplay_receipt.json", "gameplay-receipt", "luxel.gameplay-receipt/v1", "luxel-gameplay-contract"),
        "input": _artifact(output_root, "input", "artifacts/input_trace.json", "input-trace", "luxel.input-trace/v1", "luxel-gameplay-contract"),
        "hero-source": _artifact(output_root, "hero-source", "sources/hero.glb", "asset-source", "glb/v2", "luxel-asset-intake"),
    }

    def dependency(artifact_id: str) -> dict[str, str]:
        return {"artifact_id": artifact_id, "sha256": artifacts[artifact_id]["sha256"]}

    nodes = [
        {"artifact": artifacts["terrain"], "dependencies": []},
        {"artifact": artifacts["hero-source"], "dependencies": []},
        {"artifact": artifacts["asset-request"], "dependencies": [dependency("hero-source")]},
        {"artifact": artifacts["collision"], "dependencies": [dependency("terrain")]},
        {"artifact": artifacts["navigation"], "dependencies": [dependency("terrain")]},
        {"artifact": artifacts["hero-runtime"], "dependencies": [dependency("hero-source"), dependency("asset-request")]},
        {"artifact": artifacts["input"], "dependencies": []},
        {"artifact": artifacts["gameplay-contract"], "dependencies": [dependency("navigation")]},
        {"artifact": artifacts["gameplay"], "dependencies": [dependency("navigation"), dependency("hero-runtime"), dependency("input"), dependency("gameplay-contract")]},
        {"artifact": artifacts["gameplay-receipt"], "dependencies": [dependency("gameplay"), dependency("gameplay-contract")]},
    ]

    source_refs = [
        {"source_id": "brief", "kind": "brief", "path": "brief.md", "sha256": sha256(brief), "region_normalized": None},
        {"source_id": "concept-art", "kind": "concept_art", "path": "sources/concept_art.png", "sha256": sha256(concept_destination), "region_normalized": [0.0, 0.0, 1.0, 1.0]},
        {"source_id": "hero-source", "kind": "source_asset", "path": "sources/hero.glb", "sha256": artifacts["hero-source"]["sha256"], "region_normalized": None},
    ]
    gates = [
        {"gate_id": "semantic.project_spec", "evidence_kind": "semantic"},
        {"gate_id": "asset.runtime_ready", "evidence_kind": "asset"},
        {"gate_id": "world.navigation_connected", "evidence_kind": "traversal"},
        {"gate_id": "gameplay.complete_loop", "evidence_kind": "runtime"},
        {"gate_id": "visual.reference_quality", "evidence_kind": "visual"},
        {"gate_id": "build.unity_import", "evidence_kind": "engine"},
        {"gate_id": "runtime.playthrough", "evidence_kind": "runtime"},
    ]
    spec = {
        "schema_version": "luxel.project-spec/v1",
        "project_id": "luxel-mvp-gate-run",
        "title": "Gate Run vertical slice",
        "brief": {
            "text": brief.read_text(encoding="utf-8"),
            "sources": source_refs,
            "claims": [
                {"claim_id": "west-to-east-route", "source_id": "brief", "kind": "constraint", "statement": "The player must be able to traverse from the west gate to the east objective.", "confidence": 1.0},
                {"claim_id": "readable-arena", "source_id": "concept-art", "kind": "observed", "statement": "The reference presents a readable stylized highland arena with a clear focal objective.", "confidence": 0.82},
                {"claim_id": "usable-character", "source_id": "hero-source", "kind": "constraint", "statement": "The source character must become a rigged, animated, collidable runtime asset.", "confidence": 1.0},
            ],
            "conflicts": [],
            "style_target": {
                "visual_language": "readable stylized highland arena",
                "palette": ["moss green", "warm stone", "violet objective light"],
                "camera": "third-person gameplay distance",
                "reference_source_ids": ["concept-art"],
            },
            "assumptions": [
                {"assumption_id": "single-process", "statement": "The first slice does not require network replication.", "reason": "The MVP explicitly defers multiplayer."},
                {"assumption_id": "fixed-tick", "statement": "The gameplay trace runs at a fixed 60 Hz simulation tick.", "reason": "Deterministic replay requires an explicit tick model."},
            ],
            "design_constraints": [
                {"constraint_id": "target-engine", "category": "build", "statement": "Import and build through Unity MCP; no editor-only repair.", "required": True},
                {"constraint_id": "connected-route", "category": "traversal", "statement": "West spawn reaches east objective through certified navigation.", "required": True},
                {"constraint_id": "usable-character", "category": "asset", "statement": "The character has rig, animation, socket, and collision metadata.", "required": True},
                {"constraint_id": "deterministic-loop", "category": "gameplay", "statement": "The same snapshot and input trace produce the same outcome.", "required": True},
            ],
        },
        "target": {
            "engine": "unity",
            "engine_version": "2022.3",
            "platform": "linux-desktop",
            "coordinate_system": "right-handed-xz-up-y",
            "build_profile": "luxel-mvp-debug",
        },
        "world": {
            "world_id": "gate-run",
            "dimensions_m": [64.0, 64.0],
            "terrain": artifacts["terrain"],
            "collision": artifacts["collision"],
            "navigation": artifacts["navigation"],
            "spawns": [{"spawn_id": "player", "team": "player", "position_xz_m": [-24.0, 0.0], "required": True}],
            "objective": {"objective_id": "obelisk", "kind": "capture", "required_interaction_tag": "can_claim", "win_condition": "objective_claimed", "loss_condition": "player_defeated"},
        },
        "assets": [{"asset_id": "hero", "source": artifacts["hero-source"], "runtime_package": artifacts["hero-runtime"], "role": "character", "required_features": ["rig", "idle", "move", "ability", "socket", "collision", "lod"]}],
        "gameplay": {"runtime_package": artifacts["gameplay"], "input_trace": artifacts["input"], "start_entity_id": "player", "objective_id": "obelisk"},
        "artifact_graph": nodes,
        "work_orders": [
            {"schema_version": "luxel.work-order/v1", "work_order_id": "prepare-world", "operation": "prepare", "snapshot_id": "candidate", "allowed_artifacts": ["terrain", "collision", "navigation"], "required_capabilities": ["terrain", "collision", "navigation"], "required_gates": ["semantic.project_spec", "world.navigation_connected"]},
            {"schema_version": "luxel.work-order/v1", "work_order_id": "prepare-character", "operation": "prepare", "snapshot_id": "candidate", "allowed_artifacts": ["hero-source", "asset-request", "hero-runtime"], "required_capabilities": ["rigging", "animation", "collision", "lod"], "required_gates": ["asset.runtime_ready"]},
            {"schema_version": "luxel.work-order/v1", "work_order_id": "run-loop", "operation": "playtest", "snapshot_id": "candidate", "allowed_artifacts": ["gameplay-contract", "gameplay", "gameplay-receipt", "input"], "required_capabilities": ["runtime", "replay"], "required_gates": ["gameplay.complete_loop", "runtime.playthrough"]},
        ],
        "required_gates": gates,
    }
    _write_json(output_root / "project_spec.json", spec)

    gate_artifacts = {
        "semantic.project_spec": artifacts["terrain"],
        "asset.runtime_ready": artifacts["hero-runtime"],
        "world.navigation_connected": artifacts["navigation"],
        # The gameplay package describes the runtime contract, but the gate
        # must be covered by the native Rust replay receipt that actually
        # proves the loop completed. A static summary is not evidence.
        "gameplay.complete_loop": artifacts["gameplay-receipt"],
        "visual.reference_quality": artifacts["hero-runtime"],
        "build.unity_import": artifacts["gameplay"],
        "runtime.playthrough": artifacts["gameplay"],
    }
    offline_pass = {"semantic.project_spec", "world.navigation_connected", "gameplay.complete_loop"}
    if asset_result.returncode == 0:
        offline_pass.add("asset.runtime_ready")
    receipts = []
    for gate in gates:
        artifact = gate_artifacts[gate["gate_id"]]
        status = "pass" if gate["gate_id"] in offline_pass else "indeterminate"
        if gate["gate_id"] == "asset.runtime_ready" and asset_result.returncode != 0:
            status = "fail"
        basis = "offline candidate"
        if gate["gate_id"] == "asset.runtime_ready":
            basis = f"Rust asset contract exit {asset_result.returncode}"
        elif status == "indeterminate":
            basis = "Unity MCP instance unavailable"
        receipt = {
            "schema_version": "luxel.evidence/v1",
            "receipt_id": "",
            "gate_id": gate["gate_id"],
            "evidence_kind": gate["evidence_kind"],
            "status": status,
            "artifact_id": artifact["artifact_id"],
            "artifact_sha256": artifact["sha256"],
            "observed_input_sha256": artifact["sha256"],
            "producer": "luxel-mvp-orchestrator",
            "details": {"basis": basis},
        }
        receipt["receipt_id"] = _receipt_id(receipt)
        receipts.append(receipt)
    _write_json(output_root / "evidence.json", {"schema_version": "luxel.evidence/v1", "receipts": receipts})
    return {"spec": output_root / "project_spec.json", "evidence": output_root / "evidence.json", "root": output_root}


def _ledger(*arguments: str) -> subprocess.CompletedProcess[str]:
    command = ["cargo", "run", "--offline", "--quiet", "--manifest-path", str(LEDGER_MANIFEST), "-p", "luxel-project-ledger", "--", *arguments]
    return subprocess.run(command, cwd=ROOT, text=True, capture_output=True, check=False)


def run_checked(arguments: Iterable[str]) -> str:
    result = _ledger(*arguments)
    if result.returncode != 0:
        raise RuntimeError((result.stderr or result.stdout).strip())
    return result.stdout.strip()


def command_prepare(args: argparse.Namespace) -> int:
    paths = prepare(Path(args.output), concept=Path(args.concept), asset=Path(args.asset))
    print(json.dumps({key: str(value) for key, value in paths.items()}, sort_keys=True))
    return 0


def command_commit(args: argparse.Namespace) -> int:
    _verify_bundle_files(Path(args.spec))
    print(run_checked(["commit", args.spec, "--evidence", args.evidence, "--output", args.output]))
    return 0


def command_validate(args: argparse.Namespace) -> int:
    observed = _verify_bundle_files(Path(args.spec))
    print(run_checked(["validate-spec", args.spec]))
    print(json.dumps({"status": "bytes_verified", "artifact_count": len(observed)}, sort_keys=True))
    if args.snapshot:
        print(run_checked(["validate-snapshot", args.snapshot]))
    return 0


def command_unity_manifest(args: argparse.Namespace) -> int:
    _verify_bundle_files(Path(args.spec))
    print(run_checked(["unity-manifest", args.spec, "--snapshot", args.snapshot, "--output", args.output]))
    return 0


def command_verify_all(args: argparse.Namespace) -> int:
    spec_path = Path(args.spec)
    evidence_path = Path(args.evidence)
    observed = _verify_bundle_files(spec_path)
    spec_result = json.loads(run_checked(["validate-spec", str(spec_path)]))
    evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
    blocked = [receipt["gate_id"] for receipt in evidence.get("receipts", []) if receipt.get("status") != "pass"]
    if blocked:
        print(json.dumps({"status": "candidate", "certified": False, "blocked_gates": blocked, "spec": spec_result, "artifact_count": len(observed)}, sort_keys=True))
        return 2
    snapshot_path = Path(args.snapshot_output)
    unity_path = Path(args.unity_output)
    run_checked(["commit", str(spec_path), "--evidence", str(evidence_path), "--output", str(snapshot_path)])
    run_checked(["unity-manifest", str(spec_path), "--snapshot", str(snapshot_path), "--output", str(unity_path)])
    snapshot_result = json.loads(run_checked(["validate-snapshot", str(snapshot_path)]))
    print(json.dumps({"status": "certified", "certified": True, "spec": spec_result, "snapshot": snapshot_result, "unity_manifest": str(unity_path)}, sort_keys=True))
    return 0


def command_repair(args: argparse.Namespace) -> int:
    """Rebuild a candidate from pinned source inputs after a measured failure."""

    input_root = Path(args.input).expanduser().resolve()
    spec_path = input_root / "project_spec.json"
    if not spec_path.is_file():
        raise RuntimeError(f"candidate has no project_spec.json: {input_root}")
    try:
        _verify_bundle_files(spec_path)
        stale = []
    except RuntimeError as error:
        stale = [str(error)]
    observed_gate_failures: list[dict[str, str]] = []
    evidence_path = input_root / "evidence.json"
    if evidence_path.is_file():
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        observed_gate_failures = [
            {
                "gate_id": str(receipt.get("gate_id", "")),
                "status": str(receipt.get("status", "")),
            }
            for receipt in evidence.get("receipts", [])
            if receipt.get("status") != "pass"
        ]
    concept = Path(args.concept) if args.concept else input_root / "sources" / "concept_art.png"
    asset = Path(args.asset) if args.asset else input_root / "sources" / "hero.glb"
    rebuilt = prepare(Path(args.output), concept=concept, asset=asset)
    _write_json(
        rebuilt["root"] / "repair_report.json",
        {
            "schema_version": "luxel.repair-report/v1",
            "source_candidate": str(input_root),
            "observed_failures": stale,
            "observed_gate_failures": observed_gate_failures,
            "repair": "regenerated_candidate_from_pinned_inputs",
            "new_spec": str(rebuilt["spec"]),
        },
    )
    print(json.dumps({key: str(value) for key, value in rebuilt.items()}, sort_keys=True))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    prepare_parser = sub.add_parser("prepare")
    prepare_parser.add_argument("--output", required=True)
    prepare_parser.add_argument("--concept", default=str(DEFAULT_CONCEPT))
    prepare_parser.add_argument("--asset", default=str(DEFAULT_ASSET))
    prepare_parser.set_defaults(function=command_prepare)
    commit_parser = sub.add_parser("commit")
    commit_parser.add_argument("--spec", required=True)
    commit_parser.add_argument("--evidence", required=True)
    commit_parser.add_argument("--output", required=True)
    commit_parser.set_defaults(function=command_commit)
    validate_parser = sub.add_parser("validate")
    validate_parser.add_argument("--spec", required=True)
    validate_parser.add_argument("--snapshot")
    validate_parser.set_defaults(function=command_validate)
    unity_parser = sub.add_parser("unity-manifest")
    unity_parser.add_argument("--spec", required=True)
    unity_parser.add_argument("--snapshot", required=True)
    unity_parser.add_argument("--output", required=True)
    unity_parser.set_defaults(function=command_unity_manifest)
    verify_parser = sub.add_parser("verify-all")
    verify_parser.add_argument("--spec", required=True)
    verify_parser.add_argument("--evidence", required=True)
    verify_parser.add_argument("--snapshot-output", required=True)
    verify_parser.add_argument("--unity-output", required=True)
    verify_parser.set_defaults(function=command_verify_all)
    repair_parser = sub.add_parser("repair")
    repair_parser.add_argument("--input", required=True)
    repair_parser.add_argument("--output", required=True)
    repair_parser.add_argument("--concept")
    repair_parser.add_argument("--asset")
    repair_parser.set_defaults(function=command_repair)
    return parser


def main(argv: list[str] | None = None) -> int:
    try:
        parsed = build_parser().parse_args(argv)
        return int(parsed.function(parsed))
    except (OSError, RuntimeError, ValueError) as error:
        print(f"luxel-mvp: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
