from __future__ import annotations

import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from pipeline.luxel_agent_surface import LocalAgentSurface, SurfaceConfig


ROOT = Path(__file__).resolve().parents[1]
CONTROL_PLANE = ROOT / "world_core" / "target" / "debug" / "luxel-control-plane"


def reseal(body: dict[str, object], field: str) -> dict[str, object]:
    payload = dict(body)
    payload.pop(field, None)
    canonical = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    payload[field] = "sha256:" + hashlib.sha256(canonical).hexdigest()
    return payload


class ConstructionSurfaceSmokeTest(unittest.TestCase):
    def test_fresh_surface_can_compile_and_revalidate_a_typed_plan(self) -> None:
        if not CONTROL_PLANE.is_file():
            self.fail("control-plane binary is missing; build the native control plane first")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            profile = reseal(
                {
                    "schema_version": "luxel.style-profile/v1",
                    "profile_id": "style-profile-smoke-v1",
                    "source_project_id": "project-smoke-v1",
                    "intent": {
                        "geometry": {},
                        "surfaces": {},
                        "lighting": {},
                        "environment": {},
                        "camera": {},
                        "animation": {},
                        "effects": {},
                        "composition": {},
                        "budgets": {},
                    },
                    "constraints": [],
                    "observations": [],
                    "inferences": [],
                    "assumptions": [],
                    "conflicts": [],
                    "regions": [],
                    "provenance": [],
                    "confidence": [],
                },
                "profile_sha256",
            )
            draft = {
                "plan_id": "construction-plan-smoke-v1",
                "project_id": "project-smoke-v1",
                "brief": "A deterministic short traversal through an authored world.",
                "design_constraints": ["native runtime", "reproducible build"],
                "capability_needs": [
                    {
                        "capability_id": "world.navigation.traversal/v1",
                        "required": True,
                        "reason": "the player must reach the objective",
                    }
                ],
                "assets": [],
                "provider_jobs": [],
                "world_systems": ["terrain", "navigation"],
                "gameplay_kits": ["objective-traversal/v1"],
                "assumptions": [],
                "evidence_requirements": [
                    {
                        "evidence_id": "semantic-receipt",
                        "gate_id": "semantic",
                        "validator_id": "luxel.validator.semantic-spec/v1",
                        "artifact_kind": "semantic_intake",
                        "required": True,
                    }
                ],
                "unsupported_requirements": [],
            }
            profile_path = root / "profile.json"
            draft_path = root / "draft.json"
            profile_path.write_text(json.dumps(profile), encoding="utf-8")
            draft_path.write_text(json.dumps(draft), encoding="utf-8")

            surface = LocalAgentSurface(SurfaceConfig(root, CONTROL_PLANE))
            style_plan = surface.dispatch("style_compile", {"profile": "profile.json"})
            style_plan_path = root / "style-plan.json"
            style_plan_path.write_text(json.dumps(style_plan), encoding="utf-8")
            plan = surface.dispatch(
                "project_plan",
                {"draft": "draft.json", "style_plan": "style-plan.json"},
            )
            plan_path = root / "plan.json"
            plan_path.write_text(json.dumps(plan), encoding="utf-8")
            validation = surface.dispatch(
                "construction_validate",
                {"plan": "plan.json", "style_plan": "style-plan.json"},
            )

            self.assertEqual(style_plan["schema_version"], "luxel.style-plan/v1")
            self.assertEqual(plan["schema_version"], "luxel.construction-plan/v1")
            self.assertEqual(plan["readiness"], "ready")
            self.assertEqual(validation["status"], "valid")


if __name__ == "__main__":
    unittest.main()
