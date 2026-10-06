"""First implementation tests for the frozen Luxel 0.6 source kernel."""

from __future__ import annotations

import sys
import tempfile
import unittest
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CLOSURE_PATH = ROOT / "tests" / "dsl_conformance" / "closure_v06.json"
sys.path.insert(0, str(ROOT / "pipeline"))

from luxel_language_kernel import (  # noqa: E402
    BuildManifest,
    FrontendError,
    RegistrySymbolPin,
    parse_module,
    parse_module_file,
    source_digest,
)


HASH_A = "sha256:" + "a" * 64
HASH_B = "sha256:" + "b" * 64
HASH_C = "sha256:" + "c" * 64
SEED_1 = "seed256:" + "0" * 63 + "1"
SEED_2 = "seed256:" + "0" * 63 + "2"


def pin(namespace: str, symbol: str, *, kind: str = "constructor", positional: int = 0) -> RegistrySymbolPin:
    return RegistrySymbolPin(
        namespace=namespace,
        symbol=symbol,
        module_id=f"registry.{namespace}",
        semantic_digest=HASH_A,
        kind=kind,
        positional_identity_slots=positional,
    )


DEFAULT_IMPORTS = (
    pin("luxel.core", "seed"),
    pin("luxel.asset", "asset"),
    pin("luxel.geometry", "connected"),
    pin("luxel.units", "mm", kind="unit"),
)


def manifest_for(
    source: str,
    *,
    request_seed: str | None = None,
    imports: tuple[RegistrySymbolPin, ...] = DEFAULT_IMPORTS,
) -> BuildManifest:
    return BuildManifest(
        package_id="demo.package",
        root_module_id="demo.asset.sword",
        root_source_digest=source_digest(source),
        package_seed=SEED_1,
        target="asset.glb",
        product_profile_id="asset.gameplay@1",
        product_profile_digest=HASH_B,
        registry_digest=HASH_C,
        imports=imports,
        request_module_seed=request_seed,
    )


def error_code(source: str, **manifest_kwargs) -> str:
    with unittest.TestCase().assertRaises(FrontendError) as caught:
        parse_module(source, manifest_for(source, **manifest_kwargs))
    return caught.exception.code


class LuxelLanguageKernelTests(unittest.TestCase):
    def test_frozen_seed_fixture_is_consumed_by_the_frontend(self) -> None:
        contract = json.loads(CLOSURE_PATH.read_text(encoding="utf-8"))
        fixture = next(
            item
            for item in contract["fixtures"]
            if item["id"] == "fixture.identity.module_move_and_registry_binding"
        )
        accepted = fixture["accepted_seed_source"]
        parsed = parse_module(accepted, manifest_for(accepted))
        self.assertEqual(parsed.module_seed.value, SEED_1)

        expected_codes = [
            "invalid_seed_assignment",
            "invalid_seed_value",
            "late_module_seed",
            "duplicate_module_seed",
        ]
        self.assertEqual(
            [error_code(source) for source in fixture["rejected_seed_sources"]],
            expected_codes,
        )

    def test_safe_module_parses_without_execution(self) -> None:
        source = (
            "from luxel.core import seed\n"
            "from luxel.asset import asset\n"
            "from luxel.geometry import connected\n"
            "from luxel.units import mm\n"
            f"module_seed = seed(value=\"{SEED_1}\")\n"
            "blade = asset(parts=[], thickness=52 * mm)\n"
            "guard = asset(parts=[])\n"
            "sword = asset(parts=[blade, guard], constraints=[connected(first=blade, second=guard)])\n"
        )
        parsed = parse_module(source, manifest_for(source))
        self.assertEqual(parsed.module_id, "demo.asset.sword")
        self.assertEqual(parsed.module_seed.kind, "explicit_source")
        self.assertEqual(parsed.module_seed.value, SEED_1)
        self.assertEqual([item.name for item in parsed.declarations], ["blade", "guard", "sword"])
        self.assertEqual(parsed.declarations[-1].expression["constructor"], "asset")
        self.assertEqual(parsed.source_digest, source_digest(source))

    def test_file_location_does_not_define_module_identity_and_py_is_rejected(self) -> None:
        source = "from luxel.asset import asset\nthing = asset(parts=[])\n"
        manifest = manifest_for(source)
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.luxel"
            second = Path(directory) / "moved.luxel"
            first.write_text(source, encoding="utf-8")
            second.write_text(source, encoding="utf-8")
            self.assertEqual(parse_module_file(first, manifest).module_id, parse_module_file(second, manifest).module_id)
            wrong = Path(directory) / "unsafe.py"
            wrong.write_text(source, encoding="utf-8")
            with self.assertRaisesRegex(FrontendError, "wrong_source_extension"):
                parse_module_file(wrong, manifest)

    def test_source_digest_and_registry_authority_are_hard_boundaries(self) -> None:
        source = "from luxel.asset import asset\nthing = asset(parts=[])\n"
        bad = manifest_for(source)
        object.__setattr__(bad, "root_source_digest", HASH_A)
        with self.assertRaisesRegex(FrontendError, "source_digest_mismatch"):
            parse_module(source, bad)

        unauthorized = "from luxel.asset import secret\nthing = secret(parts=[])\n"
        self.assertEqual(error_code(unauthorized), "undeclared_registry_export")
        alias = "from luxel.asset import asset as make\nthing = make(parts=[])\n"
        self.assertEqual(error_code(alias), "import_alias_forbidden")
        arbitrary = "import os\nthing = os.system(\"echo never\")\n"
        self.assertEqual(error_code(arbitrary), "unsupported_statement")

    def test_seed_precedence_conflict_and_placement(self) -> None:
        no_source_seed = "from luxel.asset import asset\nthing = asset(parts=[])\n"
        requested = parse_module(no_source_seed, manifest_for(no_source_seed, request_seed=SEED_2))
        self.assertEqual(requested.module_seed.kind, "build_request_module_seed")
        self.assertEqual(requested.module_seed.value, SEED_2)

        derived = parse_module(no_source_seed, manifest_for(no_source_seed))
        self.assertEqual(derived.module_seed.kind, "derive_from_package")
        self.assertEqual(derived.module_seed.package_seed, SEED_1)

        explicit = f"from luxel.core import seed\nmodule_seed = seed(value=\"{SEED_1}\")\n"
        self.assertEqual(error_code(explicit, request_seed=SEED_2), "seed_conflict")
        same = parse_module(explicit, manifest_for(explicit, request_seed=SEED_1))
        self.assertEqual(same.module_seed.kind, "explicit_source")

        positional = f"from luxel.core import seed\nmodule_seed = seed(\"{SEED_1}\")\n"
        self.assertEqual(error_code(positional), "invalid_seed_assignment")
        late = f"from luxel.asset import asset\nthing = asset(parts=[])\nmodule_seed = seed(value=\"{SEED_1}\")\n"
        self.assertEqual(error_code(late), "late_module_seed")
        duplicate = (
            f"from luxel.core import seed\nmodule_seed = seed(value=\"{SEED_1}\")\n"
            f"module_seed = seed(value=\"{SEED_1}\")\n"
        )
        self.assertEqual(error_code(duplicate), "duplicate_module_seed")

    def test_immutable_prior_binding_rules(self) -> None:
        forward = "from luxel.asset import asset\nfirst = asset(parts=[later])\nlater = asset(parts=[])\n"
        self.assertEqual(error_code(forward), "unbound_name")
        rebound = "from luxel.asset import asset\nthing = asset(parts=[])\nthing = asset(parts=[])\n"
        self.assertEqual(error_code(rebound), "binding_redefinition")
        shadow = "from luxel.asset import asset\nasset = asset(parts=[])\n"
        self.assertEqual(error_code(shadow), "binding_redefinition")
        late_import = "from luxel.asset import asset\nthing = asset(parts=[])\nfrom luxel.units import mm\n"
        self.assertEqual(error_code(late_import), "late_import")

    def test_keyword_only_calls_and_registered_identity_slot(self) -> None:
        rejected = "from luxel.geometry import connected\njoin = connected(1, 2)\n"
        self.assertEqual(error_code(rejected), "unexpected_positional_operand")

        source = "from luxel.asset import asset\nthing = asset(\"thing-id\", parts=[])\n"
        imports = (pin("luxel.asset", "asset", positional=1),)
        parsed = parse_module(source, manifest_for(source, imports=imports))
        positional = parsed.declarations[0].expression["positional_identity"]
        self.assertEqual(positional, [{"kind": "literal", "value": "thing-id"}])

    def test_collection_and_boolean_contract_vectors(self) -> None:
        tuple_source = "from luxel.asset import asset\nthing = asset(parts=(1, 2))\n"
        self.assertEqual(error_code(tuple_source), "tuple_literal_forbidden")
        duplicate = 'from luxel.asset import asset\nthing = asset(meta={"role": 1, "\\u0072ole": 2})\n'
        self.assertEqual(error_code(duplicate), "duplicate_record_key")
        boolean = "from luxel.asset import asset\nthing = asset(flag=True and False)\n"
        self.assertEqual(error_code(boolean), "unsupported_boolean_operator")
        negate = "from luxel.asset import asset\nthing = asset(flag=not True)\n"
        self.assertEqual(error_code(negate), "unsupported_boolean_operator")
        xor = "from luxel.asset import asset\nthing = asset(flag=True ^ False)\n"
        self.assertEqual(error_code(xor), "unsupported_boolean_operator")
        chained = "from luxel.asset import asset\nthing = asset(flag=0 <= 1 <= 2)\n"
        self.assertEqual(error_code(chained), "chained_comparison")

    def test_numeric_and_binding_limits(self) -> None:
        huge = f"from luxel.asset import asset\nthing = asset(value={2**53})\n"
        self.assertEqual(error_code(huge), "integer_out_of_range")
        infinity = "from luxel.asset import asset\nthing = asset(value=1e309)\n"
        self.assertEqual(error_code(infinity), "non_finite_number")
        non_ascii = "from luxel.asset import asset\ncafé = asset(parts=[])\n"
        self.assertEqual(error_code(non_ascii), "non_ascii_source_name")
        tabs = "from luxel.asset import asset\n\tthing = asset(parts=[])\n"
        self.assertEqual(error_code(tabs), "tab_indentation")

    def test_dangerous_python_shapes_never_reach_execution(self) -> None:
        call = 'from luxel.asset import asset\nthing = __import__("os")\n'
        self.assertEqual(error_code(call), "unregistered_call")
        function = "def exploit():\n    return 1\n"
        self.assertEqual(error_code(function), "unsupported_statement")
        comprehension = "from luxel.asset import asset\nthing = asset(parts=[x for x in []])\n"
        self.assertEqual(error_code(comprehension), "unsupported_syntax")


if __name__ == "__main__":
    unittest.main()
