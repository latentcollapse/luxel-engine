"""Adversarial tests for the provider-neutral source staging boundary."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from pipeline.luxel_source_intake import SourceIntakeError, _source_draft, stage_intake


ROOT = Path(__file__).resolve().parents[1]
INTAKE_CLI = ROOT / "world_core/target/debug/luxel-intake-repair"


def _write(path: Path, content: bytes) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    return path


class LuxelSourceIntakeTests(unittest.TestCase):
    def test_draft_contains_only_caller_bytes_and_no_interpretation(self) -> None:
        with tempfile.TemporaryDirectory(prefix="luxel-intake-draft-") as temporary:
            root = Path(temporary)
            brief = _write(root / "brief.md", b"Reach the relay.\n")
            concept = _write(root / "concept.png", b"PNG-CONTROL")
            design = _write(root / "layout.json", b'{"route": "west-east"}\n')
            draft = _source_draft(
                request_id="request-001",
                brief=brief,
                concept_art=concept,
                design_document=design,
                source_root=root / "sources",
            )
            self.assertEqual(draft["schema_version"], "luxel.source-bundle-draft/v1")
            self.assertEqual(len(draft["sources"]), 3)
            self.assertNotIn("claims", draft)
            for source in draft["sources"]:
                self.assertTrue(source["content_sha256"].startswith("sha256:"))

    def test_bound_provider_response_is_staged_without_rewriting_bytes(self) -> None:
        if not INTAKE_CLI.is_file():
            self.skipTest("native intake CLI has not been built")
        with tempfile.TemporaryDirectory(prefix="luxel-intake-good-") as temporary:
            root = Path(temporary)
            brief = _write(root / "brief.md", b"Reach the relay.\n")
            concept = _write(root / "concept.png", b"PNG-CONTROL")
            design = _write(root / "layout.json", b'{"route": "west-east"}\n')
            # First obtain the native bundle identity, then provide an
            # interpretation explicitly bound to it, matching the real model
            # workflow rather than allowing a placeholder ID.
            staged_root = root / "staged"
            draft = _source_draft(
                request_id="request-002",
                brief=brief,
                concept_art=concept,
                design_document=design,
                source_root=staged_root / "sources",
            )
            draft_path = staged_root / "source-bundle-draft.json"
            draft_path.parent.mkdir(parents=True, exist_ok=True)
            draft_path.write_text(json.dumps(draft), encoding="utf-8")
            import subprocess

            bindings = [
                f"{source['source_ref']}={staged_root / 'sources' / source['source_ref']}"
                for source in draft["sources"]
            ]
            result = subprocess.run(
                [str(INTAKE_CLI), "prepare-source-bundle", str(draft_path), str(staged_root / "bundle.json"), *bindings],
                cwd=staged_root,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            bundle_id = json.loads((staged_root / "bundle.json").read_text())[
                "source_bundle_id"
            ]
            provider = {
                "schema_version": "luxel.provider-interpretation/v1",
                "source_bundle_id": bundle_id,
                "claims": [],
                "conflicts": [],
                "assumptions": [],
            }
            provider_path = _write(
                root / "provider.json",
                (json.dumps(provider, sort_keys=True) + "\n").encode(),
            )
            staged = stage_intake(
                root / "final",
                brief=brief,
                concept_art=concept,
                design_document=design,
                provider_response=provider_path,
                request_id="request-002",
                provider_id="test-provider",
                provider_version="1",
                protocol="typed-provider-json",
                intake_cli=str(INTAKE_CLI),
            )
            self.assertEqual(staged.source_bundle_id, bundle_id)
            self.assertEqual(staged.provider_response.read_bytes(), provider_path.read_bytes())
            intake = json.loads(staged.intake_draft.read_text())
            self.assertEqual(intake["provider"]["response_sha256"], "sha256:" + __import__("hashlib").sha256(provider_path.read_bytes()).hexdigest())

    def test_stale_provider_binding_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory(prefix="luxel-intake-bad-") as temporary:
            root = Path(temporary)
            brief = _write(root / "brief.md", b"brief")
            concept = _write(root / "concept.png", b"concept")
            design = _write(root / "layout.json", b"{}")
            provider = _write(
                root / "provider.json",
                b'{"schema_version":"luxel.provider-interpretation/v1","source_bundle_id":"sha256:stale","claims":[],"conflicts":[],"assumptions":[]}\n',
            )
            with self.assertRaisesRegex(SourceIntakeError, "not bound"):
                stage_intake(
                    root / "staged",
                    brief=brief,
                    concept_art=concept,
                    design_document=design,
                    provider_response=provider,
                    request_id="request-003",
                    provider_id="test-provider",
                    provider_version="1",
                    protocol="typed-provider-json",
                    intake_cli=str(INTAKE_CLI),
                )


if __name__ == "__main__":
    unittest.main()
