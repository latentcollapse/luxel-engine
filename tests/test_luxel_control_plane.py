"""Focused CLI integration checks for the native Luxel transaction surface."""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_SPEC = (
    ROOT
    / "world_core/crates/luxel_control_plane/tests/fixtures/control_project_spec.json"
)
TARGET = ROOT / "world_core/target/debug"
CONTROL_PLANE = TARGET / "luxel-control-plane"
REFERENCE_RUNTIME = TARGET / "luxel-reference-runtime"
LAYOUT = ROOT / "world_core/crates/reference_runtime/examples/riverwatch.layout.json"


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def candidate_identity(
    project_id: str,
    snapshot_id: str,
    artifacts: list[dict[str, str]],
    authorized_repair_artifact_ids: list[str],
) -> str:
    identity = {
        "schema_version": "luxel.candidate-identity/v1",
        "project_id": project_id,
        "snapshot_id": snapshot_id,
        "artifacts": [
            {
                "artifact_id": artifact["artifact_id"],
                "kind": artifact["kind"],
                "sha256": artifact["sha256"],
            }
            for artifact in sorted(artifacts, key=lambda item: item["artifact_id"])
            if artifact["kind"] not in {"repair_proposal", "repair_delta", "repair_record"}
        ],
        "authorized_repair_artifact_ids": sorted(authorized_repair_artifact_ids),
    }
    return digest(canonical_json(identity))


class ControlPlaneCliTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        result = subprocess.run(
            [
                "cargo",
                "build",
                "--offline",
                "--manifest-path",
                str(ROOT / "world_core/Cargo.toml"),
                "-p",
                "luxel-control-plane",
                "-p",
                "luxel-reference-runtime",
            ],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        if result.returncode:
            raise AssertionError(result.stdout + result.stderr)

    def run_native(self, command: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            command,
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        if check and result.returncode:
            raise AssertionError(f"command failed: {command!r}\n{result.stdout}\n{result.stderr}")
        return result

    def create_store(self, root: Path) -> Path:
        store = root / "project"
        self.run_native(
            [
                str(CONTROL_PLANE),
                "create",
                str(store),
                "--spec",
                str(FIXTURE_SPEC),
                "--profile",
                "engine-neutral",
            ]
        )
        return store

    def test_profile_create_open_and_current_pointer_are_typed(self) -> None:
        with tempfile.TemporaryDirectory(prefix="luxel-control-cli-") as temporary:
            root = Path(temporary)
            profile = self.run_native([str(CONTROL_PLANE), "profile", "engine-neutral"])
            profile_value = json.loads(profile.stdout)
            self.assertEqual(profile_value["profile_id"], "engine-neutral")
            self.assertEqual(len(profile_value["gates"]), 8)

            store = self.create_store(root)
            opened = json.loads(self.run_native([str(CONTROL_PLANE), "open", str(store)]).stdout)
            self.assertEqual(opened["project_id"], "control-test")
            current = json.loads(
                self.run_native([str(CONTROL_PLANE), "inspect-current", str(store)]).stdout
            )
            self.assertEqual(current["status"], "uncommitted")

            invalid = self.run_native(
                [str(CONTROL_PLANE), "profile", "unity"], check=False
            )
            self.assertNotEqual(invalid.returncode, 0)

    def test_world_candidate_playtest_and_capture_are_reproducible(self) -> None:
        with tempfile.TemporaryDirectory(prefix="luxel-control-world-") as temporary:
            root = Path(temporary)
            store = self.create_store(root)
            build_dir = root / "runtime-build"
            self.run_native(
                [
                    str(REFERENCE_RUNTIME),
                    "build",
                    "--layout",
                    str(LAYOUT),
                    "--output-dir",
                    str(build_dir),
                ]
            )

            world_bytes = (build_dir / "world_artifact.json").read_bytes()
            world_artifact_sha256 = json.loads(world_bytes)["artifact_sha256"]
            artifact_root = root / "candidate-artifacts"
            artifact_root.mkdir()
            (artifact_root / "world.json").write_bytes(world_bytes)
            artifacts = [
                {
                    "artifact_id": "world-artifact",
                    "kind": "world_artifact",
                    "path": "world.json",
                    "sha256": digest(world_bytes),
                }
            ]
            candidate = {
                "project_id": "control-test",
                "snapshot_id": "walk-candidate",
                "candidate_sha256": candidate_identity(
                    "control-test", "walk-candidate", artifacts, []
                ),
                "artifact_root": ".",
                "authorized_repair_artifact_ids": [],
                "artifacts": artifacts,
            }
            manifest = root / "candidate.json"
            manifest.write_text(json.dumps(candidate), encoding="utf-8")
            self.run_native(
                [
                    str(CONTROL_PLANE),
                    "create-candidate",
                    str(store),
                    "--manifest",
                    str(manifest),
                    "--artifact-root",
                    str(artifact_root),
                ]
            )

            playtest = json.loads(
                self.run_native(
                    [
                        str(CONTROL_PLANE),
                        "run-playtest",
                        str(store),
                        "--candidate",
                        "walk-candidate",
                        "--world-artifact",
                        "world-artifact",
                    ]
                ).stdout
            )
            self.assertEqual(playtest["schema_version"], "luxel.reference-playtest/v1")
            self.assertEqual(playtest["outcome"], "completed")
            self.assertGreater(playtest["steps"], 0)
            self.assertTrue(playtest["authority_revalidated"])

            captures = []
            for label in ("first", "second"):
                capture = json.loads(
                    self.run_native(
                        [
                            str(CONTROL_PLANE),
                            "capture-evidence",
                            str(store),
                            "--candidate",
                            "walk-candidate",
                            "--world-artifact",
                            "world-artifact",
                            "--output",
                            f"captures/{label}",
                        ]
                    ).stdout
                )
                captures.append(capture)
                capture_file = store / "captures" / label / "reference_capture.ppm"
                visual_file = store / "captures" / label / "visual_evidence.json"
                self.assertEqual(digest(capture_file.read_bytes()), capture["capture_sha256"])
                visual = json.loads(visual_file.read_text(encoding="utf-8"))
                self.assertEqual(visual["evidence_sha256"], capture["visual_evidence_sha256"])
                self.assertEqual(capture["world_artifact_sha256"], world_artifact_sha256)

            self.assertEqual(captures[0]["capture_sha256"], captures[1]["capture_sha256"])
            self.assertEqual(
                captures[0]["visual_evidence_sha256"],
                captures[1]["visual_evidence_sha256"],
            )
            self.assertEqual(
                (store / "captures/first/reference_capture.ppm").read_bytes(),
                (store / "captures/second/reference_capture.ppm").read_bytes(),
            )

    def test_forged_candidate_identity_is_rejected_before_publication(self) -> None:
        with tempfile.TemporaryDirectory(prefix="luxel-control-forged-") as temporary:
            root = Path(temporary)
            store = self.create_store(root)
            artifact_root = root / "artifacts"
            artifact_root.mkdir()
            raw = b"candidate payload"
            (artifact_root / "payload.bin").write_bytes(raw)
            artifacts = [
                {
                    "artifact_id": "payload",
                    "kind": "control_payload",
                    "path": "payload.bin",
                    "sha256": digest(raw),
                }
            ]
            candidate = {
                "project_id": "control-test",
                "snapshot_id": "forged-candidate",
                "candidate_sha256": digest(b"forged identity"),
                "artifact_root": ".",
                "authorized_repair_artifact_ids": [],
                "artifacts": artifacts,
            }
            manifest = root / "forged-candidate.json"
            manifest.write_text(json.dumps(candidate), encoding="utf-8")
            result = self.run_native(
                [
                    str(CONTROL_PLANE),
                    "create-candidate",
                    str(store),
                    "--manifest",
                    str(manifest),
                    "--artifact-root",
                    str(artifact_root),
                ],
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((store / "candidates/forged-candidate").exists())
            self.assertEqual(list((store / "candidates").iterdir()), [])


if __name__ == "__main__":
    unittest.main()
