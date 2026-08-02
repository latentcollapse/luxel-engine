"""Tests against the real, generated Highland building kit GLBs.

`highland_building_geometry.py`'s pure math is checked in
`test_highland_building_geometry.py` without touching Blender. This file
closes the other half of D3's "untested Blender generators" complaint for
`generate_highland_building_kit.py`: it reads the *actual* exported GLBs
(`asset_parts.py` parses the glTF JSON chunk directly, no `bpy` required) and
checks the same "roof sits on the walls" and "nodes group by building"
properties against real exporter output, not just simulated Blender
semantics. Regenerate the kit with:

    blender --background --python pipeline/generate_highland_building_kit.py \\
        -- assets/generated/codeweald_highland_buildings

before touching the generator, since these tests read whatever is currently
on disk rather than invoking Blender themselves (the rest of this suite never
shells out to Blender either -- see `generate_highland_settlement_kit.py`'s
own lack of prior test coverage, roadmap D3).
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from reference_content import CONTENT_ROOT  # noqa: E402

from asset_parts import parts  # noqa: E402
from highland_building_geometry import COTTAGE_VARIANTS  # noqa: E402

KIT_DIR = (
    CONTENT_ROOT
    / "assets/generated/codeweald_highland_buildings"
)


def _cottage_path(key: str) -> Path:
    return KIT_DIR / ("highland_cottage_%s.glb" % key.lower())


@unittest.skipUnless(KIT_DIR.is_dir(), "highland building kit not generated on this machine")
class GeneratedCottageTests(unittest.TestCase):
    """Every generated cottage collapses to exactly one collision group --
    one building per glb was the entire point of splitting the cluster."""

    def test_every_variant_file_exists(self) -> None:
        for spec in COTTAGE_VARIANTS:
            self.assertTrue(
                _cottage_path(spec.key).is_file(),
                "%s was not generated" % _cottage_path(spec.key),
            )

    def test_each_cottage_glb_has_exactly_one_collision_group(self) -> None:
        for spec in COTTAGE_VARIANTS:
            groups = parts(_cottage_path(spec.key))
            names = [entry["name"] for entry in groups]
            self.assertEqual(
                ["Cottage_%s" % spec.key],
                names,
                "expected the whole cottage to collapse into one group, got %r" % names,
            )

    def test_roof_sits_directly_over_the_walls_in_real_exported_geometry(self) -> None:
        # A stricter, real-geometry version of the pivot test: read every
        # individual node's bounds (not the grouped whole-building box) and
        # confirm the roof's horizontal footprint contains the walls'.
        for spec in COTTAGE_VARIANTS:
            import json
            import struct

            data = _cottage_path(spec.key).read_bytes()
            length = struct.unpack("<I", data[12:16])[0]
            document = json.loads(data[20 : 20 + length])
            accessors = document["accessors"]
            meshes = document["meshes"]

            def bounds_of(node_name: str):
                for node in document["nodes"]:
                    if node.get("name") != node_name or node.get("mesh") is None:
                        continue
                    translation = node.get("translation", (0.0, 0.0, 0.0))
                    primitive = meshes[node["mesh"]]["primitives"][0]
                    accessor = accessors[primitive["attributes"]["POSITION"]]
                    low, high = accessor["min"], accessor["max"]
                    return (
                        (low[0] + translation[0], high[0] + translation[0]),
                        (low[2] + translation[2], high[2] + translation[2]),
                    )
                raise AssertionError("%s has no node named %r" % (spec.key, node_name))

            wall_x, wall_z = bounds_of("Cottage_%s_plaster" % spec.key)
            roof_x, roof_z = bounds_of("Cottage_%s_%s" % (spec.key, spec.roof_suffix))
            self.assertLessEqual(roof_x[0], wall_x[0] + 0.01)
            self.assertGreaterEqual(roof_x[1], wall_x[1] - 0.01)
            self.assertLessEqual(roof_z[0], wall_z[0] + 0.01)
            self.assertGreaterEqual(roof_z[1], wall_z[1] - 0.01)

    def test_no_two_cottage_variants_are_byte_identical(self) -> None:
        # A cheap guard against a variant loop silently generating the same
        # geometry for every key (e.g. a copy/paste that forgot to vary the
        # spec) -- distinct look was the whole ask.
        contents = {
            spec.key: _cottage_path(spec.key).read_bytes() for spec in COTTAGE_VARIANTS
        }
        digests = {key: hash(value) for key, value in contents.items()}
        self.assertEqual(len(digests), len(set(digests.values())))


@unittest.skipUnless(KIT_DIR.is_dir(), "highland building kit not generated on this machine")
class GeneratedWellAndWatchtowerTests(unittest.TestCase):
    def test_well_is_one_collision_group(self) -> None:
        groups = parts(KIT_DIR / "highland_village_well.glb")
        self.assertEqual(["Well"], [entry["name"] for entry in groups])

    def test_watchtower_is_one_collision_group(self) -> None:
        groups = parts(KIT_DIR / "highland_watchtower.glb")
        self.assertEqual(["Watchtower"], [entry["name"] for entry in groups])


if __name__ == "__main__":
    unittest.main()
