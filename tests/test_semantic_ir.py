"""Cross-language golden tests for the Rust-owned semantic IR v0."""

from __future__ import annotations

import copy
import hashlib
import json
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "pipeline"))

from luxel_semantic_ir import LowerError, canonical_dumps, lower_file, lower_source, normalize_document  # noqa: E402

GOLDEN = ROOT / "tests" / "fixtures" / "semantic_ir" / "v0_golden.json"
REGISTRY = ROOT / "world_core" / "crates" / "semantic_kernel" / "registry_v0.json"


class SemanticIrGoldenTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        subprocess.run(
            ["cargo", "build", "--quiet", "-p", "luxel-semantic-kernel"],
            cwd=ROOT / "world_core",
            check=True,
        )
        cls.fixtures = json.loads(GOLDEN.read_text(encoding="utf-8"))

    def test_model_facing_sources_match_shared_golden_documents(self) -> None:
        for vector in self.fixtures["vectors"]:
            with self.subTest(vector=vector["name"]):
                source = ROOT / "tests" / "fixtures" / vector["source_fixture"]
                actual = lower_file(source, REGISTRY)
                self.assertEqual(actual, vector["expected"])
                digest = hashlib.sha256(canonical_dumps(actual).encode("utf-8")).hexdigest()
                self.assertEqual(f"sha256:{digest}", vector["canonical_sha256"])

    def test_python_compatibility_seam_uses_rust_normalization(self) -> None:
        for vector in self.fixtures["vectors"]:
            with self.subTest(vector=vector["name"]):
                self.assertEqual(normalize_document(vector["expected"]), vector["expected"])

    def test_known_bad_documents_fail_closed(self) -> None:
        base = self.fixtures["vectors"][0]["expected"]
        cases: list[tuple[str, dict, str]] = []

        unknown_root = copy.deepcopy(base)
        unknown_root["injected"] = True
        cases.append(("unknown root field", unknown_root, "unknown_field"))

        unknown_node = copy.deepcopy(base)
        unknown_node["declarations"][0]["injected"] = True
        cases.append(("unknown declaration field", unknown_node, "unknown_field"))

        fractional = copy.deepcopy(base)
        fractional["declarations"][0]["footprint"]["x1"] = 1.5
        cases.append(("fractional coordinate", fractional, "malformed_ir"))

        out_of_range = copy.deepcopy(base)
        out_of_range["declarations"][0]["footprint"]["x1"] = 2**63
        cases.append(("signed range overflow", out_of_range, "unsupported_integer_range"))

        for label, document, code in cases:
            with self.subTest(case=label):
                with self.assertRaises(LowerError) as caught:
                    normalize_document(document)
                self.assertEqual(caught.exception.code, code)

    def test_legacy_geometry_acceptance_is_preserved_until_execution_validation(self) -> None:
        source = (
            "from luxel.world import lane, place\n"
            "from luxel.geometry import rect\n\n"
            "flat_lane = lane(id=\"flat\", footprint=rect(x0=0, y0=0, x1=0, y1=5))\n"
            "safe_place = place(id=\"safe\", footprint=rect(x0=1, y0=1, x1=2, y1=2))\n"
        )
        result = lower_source(source, REGISTRY)
        self.assertEqual(result["declarations"][0]["footprint"]["x0"], result["declarations"][0]["footprint"]["x1"])


if __name__ == "__main__":
    unittest.main()
