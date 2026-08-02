from __future__ import annotations

import sys
import unittest
from pathlib import Path


PIPELINE = Path(__file__).resolve().parents[1] / "pipeline"
sys.path.insert(0, str(PIPELINE))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from reference_content import CONTENT_ROOT  # noqa: E402

import json

from worldbuilder_dsl import (
    WorldBuilderError,
    apply_intent,
    compile_intent,
    scaffold_intent,
)

BATCH = (
    CONTENT_ROOT
    / "concept_batches"
    / "codeweald_alpine_arena_v1"
    / "zone_spec.json"
)


SOURCE = """
from worldbuilder import World, ridge_network
w = World("test_world")
w.landform(
    "west",
    composition=ridge_network(
        spines=3,
        elevation_bias=0.72,
        along_jitter=0.08,
        cross_jitter=0.10,
    ),
)
"""


class WorldBuilderDslTests(unittest.TestCase):
    def test_python_shaped_intent_compiles_without_execution(self) -> None:
        intent = compile_intent(SOURCE, source_name="map.py")
        self.assertEqual(intent["world_id"], "test_world")
        self.assertEqual(
            intent["patches"][0]["generation"]["composition"]["pattern"],
            "ridge_network",
        )
        self.assertEqual(
            intent["patches"][0]["generation"]["composition"]["silhouette"],
            "continuous_boundary_wall",
        )
        self.assertEqual(
            intent["patches"][0]["generation"]["composition"]["massing"],
            "terrain_primary",
        )

    def test_intent_applies_to_matching_typed_landform(self) -> None:
        zone = {
            "zone": {"id": "test_world"},
            "features": [
                {
                    "id": "west",
                    "category": "landform",
                    "semantic": "alpine_massif",
                    "generation": {},
                }
            ],
        }
        result = apply_intent(zone, compile_intent(SOURCE))
        self.assertEqual(
            result["features"][0]["generation"]["composition"]["spine_count"], 3
        )
        self.assertNotIn("composition", zone["features"][0]["generation"])

    def test_host_access_and_control_flow_are_rejected(self) -> None:
        for source in (
            "import os\nw = World('test_world')\n",
            "from worldbuilder import World\nw = World('test_world')\nopen('/tmp/pwn')\n",
            "from worldbuilder import World\nw = World('test_world')\nwhile True: pass\n",
        ):
            with self.subTest(source=source):
                with self.assertRaises(WorldBuilderError):
                    compile_intent(source)

    def test_unknown_or_mistyped_features_are_rejected(self) -> None:
        intent = compile_intent(SOURCE)
        with self.assertRaises(WorldBuilderError):
            apply_intent(
                {"zone": {"id": "test_world"}, "features": []}, intent
            )


def _zone() -> dict:
    return {
        "zone": {"id": "test_world"},
        "features": [
            {
                "id": "eastern_massif",
                "category": "landform",
                "semantic": "alpine_massif",
                "generation": {},
            },
            {
                "id": "crag_belt",
                "category": "landform",
                "semantic": "crag_field",
                "generation": {},
            },
            {
                "id": "north_keep",
                "category": "structure",
                "semantic": "keep",
                "generation": {},
            },
        ],
    }


def _intent_source(feature: str, call: str) -> str:
    return (
        "from worldbuilder import World, ridge_network, clustered_ridges\n"
        'w = World("test_world")\n'
        f'w.landform("{feature}", composition={call})\n'
    )


_RIDGE = (
    "ridge_network(spines=3, elevation_bias=0.72, "
    "along_jitter=0.08, cross_jitter=0.10)"
)
_CLUSTER = (
    "clustered_ridges(spines=4, elevation_bias=0.55, "
    "along_jitter=0.18, cross_jitter=0.32)"
)


class RepairSuggestionTests(unittest.TestCase):
    """Every rejection a model can hit must hand back an executable repair.

    Syntactic correctness is free for any model that writes Python. These
    tests defend the semantic layer, where a weak model actually fails.
    """

    def _error(self, source: str) -> WorldBuilderError:
        with self.assertRaises(WorldBuilderError) as caught:
            apply_intent(_zone(), compile_intent(source, source_name="map.py"))
        return caught.exception

    def test_misspelled_feature_id_suggests_the_real_one(self) -> None:
        error = self._error(_intent_source("eastern_masif", _RIDGE))
        self.assertIn("eastern_massif", str(error))
        self.assertIsNotNone(error.suggestion)
        self.assertIn("'eastern_massif'", error.suggestion)

    def test_composition_illegal_for_semantic_suggests_the_legal_one(self) -> None:
        error = self._error(_intent_source("eastern_massif", _CLUSTER))
        self.assertIn("ridge_network", str(error))
        self.assertIn("ridge_network(", error.suggestion)

    def test_patching_a_non_landform_lists_the_shapeable_landforms(self) -> None:
        error = self._error(_intent_source("north_keep", _RIDGE))
        self.assertIn("eastern_massif", str(error))
        self.assertIn("crag_belt", str(error))

    def test_unknown_enum_value_suggests_nearest_valid_value(self) -> None:
        call = _RIDGE[:-1] + ', surface="fractured_granit")'
        with self.assertRaises(WorldBuilderError) as caught:
            compile_intent(_intent_source("eastern_massif", call))
        self.assertEqual(caught.exception.suggestion, "surface='fractured_granite'")

    def test_out_of_range_scalar_suggests_the_clamped_value(self) -> None:
        call = _RIDGE.replace("spines=3", "spines=40")
        with self.assertRaises(WorldBuilderError) as caught:
            compile_intent(_intent_source("eastern_massif", call))
        self.assertEqual(caught.exception.suggestion, "spines=8")

    def test_control_flow_error_explains_that_intent_is_declarative(self) -> None:
        source = (
            "from worldbuilder import World, ridge_network\n"
            'w = World("test_world")\n'
            "for i in range(3):\n"
            f'    w.landform("eastern_massif", composition={_RIDGE})\n'
        )
        with self.assertRaises(WorldBuilderError) as caught:
            compile_intent(source)
        self.assertIn("describes a world", str(caught.exception))
        self.assertEqual(caught.exception.line, 3)

    def test_errors_report_the_offending_line(self) -> None:
        call = _RIDGE.replace("spines=3", "spines=40")
        with self.assertRaises(WorldBuilderError) as caught:
            compile_intent(_intent_source("eastern_massif", call))
        self.assertEqual(caught.exception.line, 3)

    def test_bare_message_stays_available_for_programmatic_callers(self) -> None:
        error = self._error(_intent_source("eastern_masif", _RIDGE))
        self.assertNotIn("try:", error.args[0])
        self.assertIn("try:", str(error))


class LenientAuthoringTests(unittest.TestCase):
    """Leniency repairs near-misses while authoring; certification stays strict."""

    def test_lenient_mode_clamps_and_records_instead_of_raising(self) -> None:
        call = (
            'ridge_network(spines=40, elevation_bias=9.0, along_jitter=0.08, '
            'cross_jitter=0.10, surface="fractured_granit")'
        )
        source = _intent_source("eastern_massif", call)
        intent = compile_intent(source, lenient=True)
        composition = intent["patches"][0]["generation"]["composition"]
        self.assertEqual(composition["spine_count"], 8)
        self.assertEqual(composition["elevation_bias"], 1.0)
        self.assertEqual(composition["surface"], "fractured_granite")
        self.assertEqual(len(intent["repairs"]), 3)

    def test_strict_mode_is_the_default_and_still_rejects(self) -> None:
        call = _RIDGE.replace("spines=3", "spines=40")
        with self.assertRaises(WorldBuilderError):
            compile_intent(_intent_source("eastern_massif", call))

    def test_lenient_mode_still_rejects_structural_mistakes(self) -> None:
        source = (
            "from worldbuilder import World, ridge_network\n"
            'w = World("test_world")\n'
            "import os\n"
        )
        with self.assertRaises(WorldBuilderError):
            compile_intent(source, lenient=True)

    def test_clean_source_records_no_repairs(self) -> None:
        intent = compile_intent(SOURCE, lenient=True)
        self.assertNotIn("repairs", intent)

    def test_lenient_output_still_applies_to_a_zone(self) -> None:
        call = _RIDGE.replace("spines=3", "spines=40")
        intent = compile_intent(_intent_source("eastern_massif", call), lenient=True)
        result = apply_intent(_zone(), intent)
        self.assertEqual(
            result["features"][0]["generation"]["composition"]["spine_count"], 8
        )


class ScaffoldTests(unittest.TestCase):
    """A generated intent file must be valid, complete, and lossless.

    No model should face a blank page, and the file it is handed must not
    silently change the world just by existing.
    """

    def test_scaffold_compiles_and_applies_as_written(self) -> None:
        zone = _zone()
        result = apply_intent(zone, compile_intent(scaffold_intent(zone)))
        self.assertEqual(result["world_intent"]["patch_count"], 2)

    def test_scaffold_lists_every_shapeable_landform_and_no_others(self) -> None:
        source = scaffold_intent(_zone())
        self.assertIn("'eastern_massif'", source)
        self.assertIn("'crag_belt'", source)
        self.assertNotIn("world.landform('north_keep'", source)

    def test_scaffold_header_marks_jitter_bounds_as_documented_not_buildable(self) -> None:
        # 0.3: the header lists along_jitter/cross_jitter as 0.0-0.5, but that
        # range is only what the DSL will parse -- a legal value can still
        # fail the accessibility gate at build time. The header must say so,
        # not just list the range and leave an author to assume it is safe.
        source = scaffold_intent(_zone())
        self.assertIn("DOCUMENTED", source)
        self.assertIn("BUILDABLE", source)
        self.assertIn("accessibility", source)

    def test_scaffold_preserves_existing_composition_values(self) -> None:
        zone = _zone()
        zone["features"][0]["generation"]["composition"] = {
            "pattern": "ridge_network",
            "spine_count": 7,
            "elevation_bias": 0.31,
            "along_jitter": 0.22,
            "cross_jitter": 0.44,
            "silhouette": "continuous_boundary_wall",
            "massing": "terrain_primary",
            "surface": "fractured_granite",
            "dressing": "none",
        }
        result = apply_intent(zone, compile_intent(scaffold_intent(zone)))
        composition = result["features"][0]["generation"]["composition"]
        self.assertEqual(composition["spine_count"], 7)
        self.assertEqual(composition["elevation_bias"], 0.31)
        self.assertEqual(composition["cross_jitter"], 0.44)
        self.assertEqual(composition["dressing"], "none")

    def test_scaffold_repairs_a_composition_illegal_for_its_semantic(self) -> None:
        zone = _zone()
        zone["features"][0]["generation"]["composition"] = {
            "pattern": "clustered_ridges",
            "spine_count": 2,
        }
        source = scaffold_intent(zone)
        result = apply_intent(zone, compile_intent(source))
        self.assertEqual(
            result["features"][0]["generation"]["composition"]["pattern"],
            "ridge_network",
        )

    def test_scaffold_imports_only_the_constructors_it_uses(self) -> None:
        zone = _zone()
        zone["features"] = [zone["features"][0]]  # alpine_massif only
        source = scaffold_intent(zone)
        imports = [
            line for line in source.splitlines() if line.startswith("from worldbuilder")
        ]
        self.assertEqual(imports, ["from worldbuilder import World, ridge_network"])

    def test_scaffold_notes_landforms_it_cannot_author(self) -> None:
        zone = _zone()
        zone["features"].append(
            {
                "id": "salt_flat",
                "category": "landform",
                "semantic": "playa",
                "generation": {},
            }
        )
        source = scaffold_intent(zone)
        self.assertIn("salt_flat", source)
        self.assertNotIn("world.landform('salt_flat'", source)

    def test_scaffold_refuses_a_zone_with_nothing_to_author(self) -> None:
        zone = {"zone": {"id": "empty"}, "features": []}
        with self.assertRaises(WorldBuilderError):
            scaffold_intent(zone)

    def test_scaffold_comments_do_not_reach_the_compiler(self) -> None:
        source = scaffold_intent(_zone())
        self.assertIn("# This file describes a world", source)
        intent = compile_intent(source)
        self.assertEqual(len(intent["patches"]), 2)

    @unittest.skipUnless(BATCH.exists(), "production zone spec not present")
    def test_scaffold_round_trips_the_production_zone_without_drift(self) -> None:
        zone = json.loads(BATCH.read_text(encoding="utf-8"))
        result = apply_intent(zone, compile_intent(scaffold_intent(zone)))

        def compositions(spec: dict) -> dict:
            return {
                feature["id"]: feature.get("generation", {}).get("composition")
                for feature in spec["features"]
                if feature.get("category") == "landform"
            }

        before, after = compositions(zone), compositions(result)
        self.assertEqual(len(before), 8)
        self.assertEqual(before, after)


if __name__ == "__main__":
    unittest.main()
