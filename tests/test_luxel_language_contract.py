"""Executable integrity checks for the Luxel language draft 0.6 contract.

These tests validate the hand-authored closure vectors without selecting a
production compiler host. Future compiler implementations must consume the same
fixtures rather than replacing them with self-generated expectations.
"""

from __future__ import annotations

import ast
import hashlib
import json
import math
import re
import unicodedata
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CONTRACT_PATH = ROOT / "tests" / "dsl_conformance" / "closure_v06.json"
MIGRATION_PATH = ROOT / "tests" / "dsl_conformance" / "migration_worldbuilder_v1.json"
SPEC_PATH = ROOT / "docs" / "DSL docs" / "LUXEL_LANGUAGE_SPEC.md"
PROTOTYPE_TEST_PATH = ROOT / "tests" / "test_worldbuilder_dsl.py"

REQUIRED_FIXTURE_IDS = {
    "fixture.canonical.all_array_orderings",
    "fixture.digest.complete_domain_matrix",
    "fixture.identity.module_move_and_registry_binding",
    "fixture.profile.identity_and_cache_isolation",
    "fixture.syntax.mask_comparison_and_keywords",
    "fixture.numeric.unit_scale_rounding",
    "fixture.compiler.patch_compatibility",
    "fixture.collections.and_identifier_roundtrip",
    "fixture.migration.lenient_preview_requires_strict_promotion",
    "fixture.extensions.empty_set_digest",
    "fixture.cache.reuse_event_location",
    "fixture.policy.soft_promotion_label",
}
SEED_PATTERN = re.compile(r"seed256:[0-9a-f]{64}\Z")
DIGEST_PATTERN = re.compile(r"sha256:[0-9a-f]{64}\Z")


def _walk_values(value):
    if isinstance(value, dict):
        for child in value.values():
            yield from _walk_values(child)
    elif isinstance(value, list):
        for child in value:
            yield from _walk_values(child)
    else:
        yield value


def _load_contract() -> dict:
    return json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))


def _fixtures_by_id() -> dict[str, dict]:
    fixtures = _load_contract()["fixtures"]
    return {fixture["id"]: fixture for fixture in fixtures}


def _utf8_key(value: str) -> bytes:
    return unicodedata.normalize("NFC", value).encode("utf-8")


def _dependency_key(record: dict) -> tuple[bytes, str, str]:
    return (
        _utf8_key(record["module_id"]),
        record["semantic_digest"],
        record["document_digest"] or "",
    )


def _canonical_dag_order(records: list[dict]) -> list[str]:
    dependencies = {record["id"]: set(record["depends_on"]) for record in records}
    unknown = set().union(*dependencies.values()) - dependencies.keys()
    if unknown:
        raise ValueError(f"unknown dependency IDs: {sorted(unknown)}")

    ordered: list[str] = []
    while dependencies:
        ready = sorted(
            (node_id for node_id, parents in dependencies.items() if not parents),
            key=_utf8_key,
        )
        if not ready:
            raise ValueError("dependency cycle")
        selected = ready[0]
        ordered.append(selected)
        del dependencies[selected]
        for parents in dependencies.values():
            parents.discard(selected)
    return ordered


def _domain_hash(tag: str, value) -> str:
    encoded = json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return "sha256:" + hashlib.sha256(tag.encode("ascii") + b"\0" + encoded).hexdigest()


def _matrix_digests(fields: dict, fixture: dict) -> dict[str, str]:
    semantic = {name: fields[name] for name in fixture["semantic_fields"]}
    extensions = fields["ignorable_extensions"]
    document = {name: value for name, value in fields.items() if name != "document_digest"}
    return {
        "semantic_digest": _domain_hash("luxel.ir.semantic/1", semantic),
        "extension_digest": _domain_hash("luxel.ir.extensions/1", extensions),
        "document_digest": _domain_hash("luxel.ir.document/1", document),
    }


class LuxelLanguageContractTests(unittest.TestCase):
    def test_contract_inventory_matches_normative_spec(self) -> None:
        contract = _load_contract()
        self.assertEqual(contract["schema"], "luxel.conformance.closure/0.6")
        self.assertEqual(contract["spec_draft"], "0.6")
        fixtures = contract["fixtures"]
        ids = [fixture["id"] for fixture in fixtures]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(set(ids), REQUIRED_FIXTURE_IDS)
        for value in _walk_values(contract):
            if isinstance(value, str) and value.startswith("sha256:"):
                self.assertRegex(value, DIGEST_PATTERN)

        spec = SPEC_PATH.read_text(encoding="utf-8")
        self.assertIn("**Draft:** 0.6", spec)
        for fixture_id in REQUIRED_FIXTURE_IDS:
            self.assertIn(f"`{fixture_id}`", spec)

    def test_canonical_setlike_and_dag_order_vectors(self) -> None:
        fixture = _fixtures_by_id()["fixture.canonical.all_array_orderings"]
        canonical_orders = [
            [record["module_id"] for record in sorted(permutation, key=_dependency_key)]
            for permutation in fixture["dependency_permutations"]
        ]
        self.assertTrue(canonical_orders)
        self.assertTrue(all(order == canonical_orders[0] for order in canonical_orders))
        self.assertEqual(canonical_orders[0], fixture["expected_dependency_order"])
        self.assertEqual(
            _canonical_dag_order(fixture["declaration_dag"]),
            fixture["expected_declaration_order"],
        )
        sequence = fixture["semantic_sequence_reorder"]
        left_digest = _domain_hash("luxel.ir.semantic-sequence.test/1", sequence["left"])
        right_digest = _domain_hash("luxel.ir.semantic-sequence.test/1", sequence["right"])
        self.assertEqual(left_digest == right_digest, sequence["expected_digest_equal"])

    def test_digest_matrix_partitions_load_bearing_fields(self) -> None:
        fixture = _fixtures_by_id()["fixture.digest.complete_domain_matrix"]
        partitions = [
            set(fixture["semantic_fields"]),
            set(fixture["extension_fields"]),
            set(fixture["document_only_fields"]),
        ]
        for index, left in enumerate(partitions):
            for right in partitions[index + 1 :]:
                self.assertFalse(left & right)
        self.assertEqual(
            set(fixture["document_includes"]),
            {"semantic_digest", "extension_digest"},
        )
        self.assertEqual(
            set(fixture["self_excluded"]),
            {"semantic_digest", "extension_digest", "document_digest"},
        )

        # This explicit universe is independent of the fixture partitions. It
        # makes a new 13.3 row fail until the conformance matrix accounts for it.
        expected_projection_fields = {
            "schema_version", "language_version", "semantic_compiler_compatibility_id",
            "registry_digest", "module_id", "module_seed", "target",
            "product_profile_id", "product_profile_digest",
            "dependency_semantic_identities", "semantic_evidence_pins",
            "required_capabilities", "declarations", "constraints", "policies",
            "resolved_defaults", "critical_extensions", "ignorable_extensions",
            "compiler_version", "compiler_build_provenance", "source_digest",
            "source_map", "diagnostics", "repair_history",
            "dependency_document_identities", "evidence_manifest_digest",
            "construction_evidence_pins", "provenance",
        }
        self.assertEqual(set().union(*partitions), expected_projection_fields)

        mutations = fixture["mutation_expectations"]
        self.assertEqual(
            set(mutations),
            expected_projection_fields | {"semantic_digest", "extension_digest", "document_digest"},
        )
        baseline = {name: f"base:{name}" for name in expected_projection_fields}
        baseline["ignorable_extensions"] = ["base:ignorable_extensions"]
        baseline.update(
            {
                "semantic_digest": "sha256:" + "1" * 64,
                "extension_digest": "sha256:" + "2" * 64,
                "document_digest": "sha256:" + "3" * 64,
            }
        )
        baseline_digests = _matrix_digests(baseline, fixture)
        for field, expected_changed in mutations.items():
            mutated = dict(baseline)
            mutated[field] = [f"mutated:{field}"] if field == "ignorable_extensions" else f"mutated:{field}"
            changed = {
                digest_name
                for digest_name, digest in _matrix_digests(mutated, fixture).items()
                if digest != baseline_digests[digest_name]
            }
            self.assertEqual(changed, set(expected_changed), field)

    def test_empty_extension_digest_vector(self) -> None:
        fixture = _fixtures_by_id()["fixture.extensions.empty_set_digest"]
        preimage = bytes.fromhex(fixture["preimage_hex"])
        self.assertEqual(preimage, b"luxel.ir.extensions/1\0[]")
        actual = "sha256:" + hashlib.sha256(preimage).hexdigest()
        self.assertEqual(actual, fixture["expected"])

    def test_binary64_unit_vectors(self) -> None:
        fixture = _fixtures_by_id()["fixture.numeric.unit_scale_rounding"]
        self.assertEqual(fixture["rounding"], "nearest_ties_even")
        for vector in fixture["vectors"]:
            result = float(vector["source_magnitude"]) * float(vector["scale"])
            self.assertTrue(math.isfinite(result))
            self.assertEqual(result.hex(), vector["expected_hex"], vector["unit"])

    def test_profile_and_compiler_identity_vectors(self) -> None:
        profiles = _fixtures_by_id()["fixture.profile.identity_and_cache_isolation"]
        for case in profiles["cases"]:
            left = (case["left"]["canonical_id"], case["left"]["digest"])
            right = (case["right"]["canonical_id"], case["right"]["digest"])
            self.assertEqual(left == right, case["semantic_identity_equal"])
            left_cache_key = (profiles["cache_input_semantic_digest"], *left)
            right_cache_key = (profiles["cache_input_semantic_digest"], *right)
            self.assertEqual(left_cache_key == right_cache_key, case["semantic_identity_equal"])

        compilers = _fixtures_by_id()["fixture.compiler.patch_compatibility"]
        for case in compilers["cases"]:
            semantic_equal = case["left"]["compatibility_id"] == case["right"]["compatibility_id"]
            document_equal = case["left"] == case["right"]
            self.assertEqual(semantic_equal, case["semantic_identity_equal"])
            self.assertEqual(document_equal, case["document_identity_equal"])

    def test_reserved_module_seed_surface_and_representation(self) -> None:
        fixture = _fixtures_by_id()["fixture.identity.module_move_and_registry_binding"]
        cases = {case["change"]: case for case in fixture["cases"]}
        for name in ("source_path_only", "module_id", "explicit_module_seed"):
            case = cases[name]
            left_identity = (case["left"]["module_id"], case["left"]["module_seed"])
            right_identity = (case["right"]["module_id"], case["right"]["module_seed"])
            self.assertEqual(left_identity != right_identity, case["semantic_identity_changes"], name)

        undeclared = cases["undeclared_registry_export"]
        self.assertNotIn(undeclared["requested_export"], undeclared["declared_exports"])
        self.assertEqual(undeclared["result"], "binding_error")

        conflict = cases["source_and_request_seed_conflict"]
        self.assertNotEqual(conflict["source_seed"], conflict["request_seed"])
        self.assertEqual(conflict["result"], "seed_conflict")

        cycle = cases["registry_dependency_cycle"]
        with self.assertRaisesRegex(ValueError, "dependency cycle"):
            _canonical_dag_order(cycle["dependency_graph"])
        self.assertEqual(cycle["result"], "cycle_error")

        accepted = ast.parse(fixture["accepted_seed_source"], mode="exec")
        self.assertEqual(len(accepted.body), 1)
        assignment = accepted.body[0]
        self.assertIsInstance(assignment, ast.Assign)
        self.assertEqual(assignment.targets[0].id, "module_seed")
        call = assignment.value
        self.assertIsInstance(call, ast.Call)
        self.assertFalse(call.args)
        self.assertEqual([keyword.arg for keyword in call.keywords], ["value"])
        value = ast.literal_eval(call.keywords[0].value)
        self.assertRegex(value, SEED_PATTERN)

        rejected = fixture["rejected_seed_sources"]
        positional_call = ast.parse(rejected[0], mode="exec").body[0].value
        self.assertTrue(positional_call.args)
        bad_value_call = ast.parse(rejected[1], mode="exec").body[0].value
        self.assertNotRegex(ast.literal_eval(bad_value_call.keywords[0].value), SEED_PATTERN)
        late_seed = ast.parse(rejected[2], mode="exec").body
        self.assertGreater(len(late_seed), 1)
        self.assertEqual(late_seed[1].targets[0].id, "module_seed")
        duplicate_seed = ast.parse(rejected[3], mode="exec").body
        self.assertEqual(
            [node.targets[0].id for node in duplicate_seed if isinstance(node, ast.Assign)],
            ["module_seed", "module_seed"],
        )

    def test_source_syntax_vectors_are_structurally_independent_of_python_execution(self) -> None:
        fixture = _fixtures_by_id()["fixture.syntax.mask_comparison_and_keywords"]
        for source in fixture["accepted"]:
            tree = ast.parse(source, mode="exec")
            calls = [node for node in ast.walk(tree) if isinstance(node, ast.Call)]
            self.assertTrue(calls)
            self.assertTrue(all(not call.args for call in calls))

        expected_shapes = {
            "unexpected_positional_operand": ast.Call,
            "unsupported_boolean_operator": (ast.BoolOp, ast.UnaryOp),
            "chained_comparison": ast.Compare,
        }
        for rejected in fixture["rejected"]:
            tree = ast.parse(rejected["source"], mode="exec")
            shape = expected_shapes[rejected["code"]]
            matching = [node for node in ast.walk(tree) if isinstance(node, shape)]
            self.assertTrue(matching, rejected)
            if rejected["code"] == "unexpected_positional_operand":
                self.assertTrue(any(call.args for call in matching))
            if rejected["code"] == "chained_comparison":
                self.assertTrue(any(len(compare.ops) > 1 for compare in matching))

    def test_collection_and_external_identifier_vectors(self) -> None:
        fixture = _fixtures_by_id()["fixture.collections.and_identifier_roundtrip"]
        self.assertIs(fixture["must_never_become_source_bindings"], True)
        for rejected in fixture["rejected"]:
            tree = ast.parse(rejected["source"], mode="exec")
            if rejected["code"] == "tuple_literal_forbidden":
                self.assertTrue(any(isinstance(node, ast.Tuple) for node in ast.walk(tree)))
            else:
                dictionaries = [node for node in ast.walk(tree) if isinstance(node, ast.Dict)]
                self.assertEqual(len(dictionaries), 1)
                keys = [ast.literal_eval(key) for key in dictionaries[0].keys]
                self.assertNotEqual(len(keys), len(set(keys)))

        for identifier in fixture["external_identifier_values"]:
            normalized = unicodedata.normalize("NFC", identifier)
            encoded = json.dumps(normalized, ensure_ascii=False)
            self.assertEqual(json.loads(encoded), normalized)

    def test_migration_manifest_accounts_for_every_prototype_test(self) -> None:
        manifest = json.loads(MIGRATION_PATH.read_text(encoding="utf-8"))
        source_tree = ast.parse(PROTOTYPE_TEST_PATH.read_text(encoding="utf-8"))
        source_tests = {
            node.name
            for node in ast.walk(source_tree)
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
            and node.name.startswith("test_")
        }
        manifest_tests = [fixture["test"] for fixture in manifest["fixtures"]]
        self.assertEqual(len(manifest_tests), len(set(manifest_tests)))
        self.assertEqual(set(manifest_tests), source_tests)

        allowed = {"preserve", "replace_with_strict_promotion", "retire"}
        for fixture in manifest["fixtures"]:
            self.assertIn(fixture["disposition"], allowed)
            if fixture["disposition"] != "preserve":
                self.assertTrue(fixture.get("rationale"))
                self.assertIn(fixture.get("replacement_fixture"), REQUIRED_FIXTURE_IDS)

        by_name = {fixture["test"]: fixture for fixture in manifest["fixtures"]}
        self.assertEqual(
            by_name["test_lenient_output_still_applies_to_a_zone"]["disposition"],
            "replace_with_strict_promotion",
        )

    def test_lenient_cache_and_promotion_boundaries(self) -> None:
        fixtures = _fixtures_by_id()
        migration = fixtures["fixture.migration.lenient_preview_requires_strict_promotion"]
        self.assertEqual(migration["lenient_allowed_consumers"], ["preview_sandbox"])
        self.assertNotIn("preview_sandbox", migration["lenient_forbidden_consumers"])
        self.assertIn("certification", migration["lenient_forbidden_consumers"])
        self.assertNotEqual(migration["lenient_result_type"], migration["strict_result_type"])

        cache = fixtures["fixture.cache.reuse_event_location"]
        self.assertIs(cache["cache_entry_changes_on_hit"], False)
        self.assertEqual(cache["reuse_event_location"], "current_build_receipt")
        self.assertIs(cache["receipt_document_identity_changes"], True)

        promotion = fixtures["fixture.policy.soft_promotion_label"]
        expected_id = f"promotion:{promotion['policy_id']}:{promotion['objective_id']}"
        self.assertEqual(promotion["expected_constraint_id"], expected_id)
        self.assertEqual(promotion["expected_class"], "realized_hard")
        self.assertEqual(
            set(promotion["required_provenance_parents"]),
            {promotion["policy_id"], promotion["objective_id"]},
        )


if __name__ == "__main__":
    unittest.main()
