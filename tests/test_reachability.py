from __future__ import annotations

import copy
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

import zone_rasterizer  # noqa: E402
from reachability import (  # noqa: E402
    DECLARATIONS,
    inconclusive,
    module_reachability,
    sweep,
    undeclared_parameters,
    unreachable,
)
from zone_rasterizer import ZONE_SPEC_VERSION  # noqa: E402

# Small and synthetic on purpose: the sweep rasterises once per
# (feature, parameter) pair, so the real 1025-resolution batch would make
# these tests cost minutes instead of a second. Reachability is a yes/no
# question about wiring, and wiring does not care how big the grid is.
TEST_RESOLUTION = 65


def _landform(
    feature_id: str, profile: str, pattern: str, points: list[list[float]]
) -> dict:
    return {
        "id": feature_id,
        "category": "landform",
        "semantic": "alpine_massif",
        "geometry": {"type": "polygon", "points": points},
        "generation": {
            "profile": profile,
            "elevation_m": [5.0, 30.0],
            "cliffness": 0.8,
            "composition": {
                "pattern": pattern,
                "spine_count": 3,
                "along_jitter": 0.08,
                "cross_jitter": 0.10,
                "elevation_bias": 0.72,
                "silhouette": "continuous_boundary_wall",
                "massing": "terrain_primary",
                "surface": "fractured_granite",
                "dressing": "none",
            },
        },
    }


def _spec(features: list[dict]) -> dict:
    return {
        "schema_version": ZONE_SPEC_VERSION,
        "generation_seed": 7,
        "zone": {"id": "test_zone", "world_bounds": {"width": 256.0, "length": 256.0}},
        "features": features,
    }


def _both_profiles() -> dict:
    # Polygon points are world metres, matching a compiled ZoneSpec -- the
    # normalized [0,1] coordinates live in annotations.json, and the compiler
    # converts them. A normalized polygon here would be sub-metre, fall
    # between sample points, and mask out to nothing.
    return _spec(
        [
            _landform(
                "massif_a",
                "alpine_jagged_massif",
                "ridge_network",
                [[-120.0, -120.0], [-10.0, -120.0], [-10.0, -10.0], [-120.0, -10.0]],
            ),
            _landform(
                "crags_a",
                "scattered_crag_field",
                "clustered_ridges",
                [[10.0, 10.0], [120.0, 10.0], [120.0, 120.0], [10.0, 120.0]],
            ),
        ]
    )


class SweepDetectsWiredParametersTests(unittest.TestCase):
    """Every declared parameter must move the artifact it claims to control.

    This is the assertion the whole module exists to make; if it ever fails
    on real code, a knob has gone decorative.
    """

    def test_all_declared_parameters_reach_the_heightfield(self) -> None:
        results = sweep(_both_profiles(), resolution=TEST_RESOLUTION)
        self.assertTrue(results, "sweep produced no observations")
        self.assertEqual([], unreachable(results))

    def test_every_declared_parameter_is_actually_exercised(self) -> None:
        # A sweep that silently skipped a parameter would report a clean
        # result for a knob it never touched.
        results = sweep(_both_profiles(), resolution=TEST_RESOLUTION)
        exercised = {result.parameter for result in results}
        self.assertEqual(
            {declaration.name for declaration in DECLARATIONS}, exercised
        )

    def test_both_profiles_are_reported_separately(self) -> None:
        results = sweep(_both_profiles(), resolution=TEST_RESOLUTION)
        profiles = {result.profile for result in results}
        self.assertEqual({"alpine_jagged_massif", "scattered_crag_field"}, profiles)


class SweepDetectsUnwiredParametersTests(unittest.TestCase):
    """The detector has to fail on a real defect, not just pass on good code.

    Each test below simulates the exact shape of the incident that motivated
    tooling item 2: the DSL keeps advertising a scalar that the rasterizer has
    stopped reading.
    """

    @staticmethod
    def _freeze(name: str, frozen_value: float, only_pattern: str | None = None):
        real = zone_rasterizer._composition_scalars

        def patched(feature_id, composition, pattern):
            if only_pattern is None or pattern == only_pattern:
                composition = {**composition, name: frozen_value}
            return real(feature_id, composition, pattern)

        return patched

    def test_a_fully_unwired_scalar_is_reported_for_every_profile(self) -> None:
        with mock.patch.object(
            zone_rasterizer,
            "_composition_scalars",
            self._freeze("along_jitter", 0.08),
        ):
            results = sweep(_both_profiles(), resolution=TEST_RESOLUTION)
        dead = unreachable(results)
        self.assertEqual({"along_jitter"}, {result.parameter for result in dead})
        self.assertEqual(
            {"alpine_jagged_massif", "scattered_crag_field"},
            {result.profile for result in dead},
        )

    def test_an_unwired_scalar_does_not_implicate_the_others(self) -> None:
        with mock.patch.object(
            zone_rasterizer,
            "_composition_scalars",
            self._freeze("along_jitter", 0.08),
        ):
            results = sweep(_both_profiles(), resolution=TEST_RESOLUTION)
        still_fine = {
            result.parameter for result in results if result.reachable
        }
        self.assertIn("cross_jitter", still_fine)
        self.assertIn("spine_count", still_fine)
        self.assertIn("elevation_bias", still_fine)

    def test_a_half_wired_scalar_is_isolated_to_the_dead_profile(self) -> None:
        # The reason results are keyed by (parameter, profile): the rasterizer
        # branches on profile, so a scalar can be live for one and dead for
        # the other. A per-parameter-only verdict would report "reaches" and
        # hide half the defect behind the working half.
        with mock.patch.object(
            zone_rasterizer,
            "_composition_scalars",
            self._freeze("cross_jitter", 0.10, only_pattern="clustered_ridges"),
        ):
            results = sweep(_both_profiles(), resolution=TEST_RESOLUTION)
        dead = unreachable(results)
        self.assertEqual(1, len(dead))
        self.assertEqual("cross_jitter", dead[0].parameter)
        self.assertEqual("scattered_crag_field", dead[0].profile)

    def test_the_failing_result_names_the_landforms_it_tested(self) -> None:
        # A finding that cannot say where it looked is not actionable.
        with mock.patch.object(
            zone_rasterizer,
            "_composition_scalars",
            self._freeze("along_jitter", 0.08),
        ):
            results = sweep(_both_profiles(), resolution=TEST_RESOLUTION)
        for result in unreachable(results):
            self.assertTrue(result.features_tested)
            self.assertTrue(result.owner)


class InconclusiveSweepTests(unittest.TestCase):
    """A landform that cannot move the artifact under any value is not
    evidence that a parameter is unwired.

    Found by this fixture being malformed: a normalized-coordinate polygon
    masks out to nothing, contributes no height, and made all four correctly
    wired scalars report as dead. Without the control check the sweep would
    have sent someone hunting a broken wire in the rasterizer while the
    landform was the broken thing.
    """

    @staticmethod
    def _inert_landform_spec() -> dict:
        # Sub-metre polygon: falls between sample points, masks to nothing.
        return _spec(
            [
                _landform(
                    "inert",
                    "alpine_jagged_massif",
                    "ridge_network",
                    [[0.05, 0.05], [0.45, 0.05], [0.45, 0.45], [0.05, 0.45]],
                )
            ]
        )

    def test_an_inert_landform_reports_inconclusive_not_unreachable(self) -> None:
        results = sweep(self._inert_landform_spec(), resolution=TEST_RESOLUTION)
        self.assertTrue(results)
        self.assertEqual([], unreachable(results))
        self.assertEqual(len(results), len(inconclusive(results)))

    def test_inconclusive_results_name_the_skipped_landform(self) -> None:
        results = sweep(self._inert_landform_spec(), resolution=TEST_RESOLUTION)
        for result in results:
            self.assertEqual(["inert"], result.inconclusive_features)
            self.assertIn("INCONCLUSIVE", result.describe())

    def test_an_inert_landform_does_not_mask_a_real_defect_elsewhere(self) -> None:
        # One healthy landform and one inert one: the healthy landform still
        # has to carry the verdict rather than being averaged into silence.
        spec = _both_profiles()
        spec["features"].append(
            _landform(
                "inert",
                "alpine_jagged_massif",
                "ridge_network",
                [[0.05, 0.05], [0.45, 0.05], [0.45, 0.45], [0.05, 0.45]],
            )
        )
        with mock.patch.object(
            zone_rasterizer,
            "_composition_scalars",
            SweepDetectsUnwiredParametersTests._freeze("along_jitter", 0.08),
        ):
            results = sweep(spec, resolution=TEST_RESOLUTION)
        dead = {result.parameter for result in unreachable(results)}
        self.assertIn("along_jitter", dead)


class UndeclaredParameterTests(unittest.TestCase):
    """A knob nobody declared is a knob nobody sweeps."""

    def test_an_undeclared_numeric_scalar_is_reported(self) -> None:
        spec = _both_profiles()
        spec["features"][0]["generation"]["composition"]["ridge_sharpness"] = 0.4
        self.assertEqual(["ridge_sharpness"], undeclared_parameters(spec))

    def test_declared_scalars_are_not_reported(self) -> None:
        self.assertEqual([], undeclared_parameters(_both_profiles()))

    def test_categorical_composition_keys_are_not_reported(self) -> None:
        # silhouette/massing/surface/dressing are strings controlling other
        # artifacts, not heightfield scalars; flagging them would be noise.
        spec = _both_profiles()
        self.assertNotIn("silhouette", undeclared_parameters(spec))
        self.assertNotIn("surface", undeclared_parameters(spec))


class ModuleReachabilityTests(unittest.TestCase):
    """The extension: a generator no build stage can reach is the same defect
    one level up.
    """

    def test_a_module_nobody_imports_or_names_is_unreachable(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pipeline = root / "pipeline"
            pipeline.mkdir()
            (pipeline / "orphan.py").write_text("x = 1\n", encoding="utf-8")
            (pipeline / "used.py").write_text("y = 2\n", encoding="utf-8")
            (pipeline / "caller.py").write_text(
                "import used\n", encoding="utf-8"
            )
            results = {
                entry.module: entry
                for entry in module_reachability(pipeline, search_roots=[root])
            }
            self.assertFalse(results["orphan"].reachable)
            self.assertTrue(results["used"].reachable)

    def test_a_module_named_by_a_doc_counts_as_reachable(self) -> None:
        # An entry point invoked from a runbook or a subprocess call is
        # reached, even though nothing imports it.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pipeline = root / "pipeline"
            pipeline.mkdir()
            (pipeline / "entry.py").write_text(
                "if __name__ == '__main__':\n    pass\n", encoding="utf-8"
            )
            (root / "README.md").write_text(
                "run pipeline/entry.py to rebuild\n", encoding="utf-8"
            )
            results = {
                entry.module: entry
                for entry in module_reachability(pipeline, search_roots=[root])
            }
            self.assertTrue(results["entry"].reachable)
            self.assertTrue(results["entry"].is_entry_point)

    def test_the_real_pipeline_has_no_unreachable_modules(self) -> None:
        pipeline = Path(__file__).resolve().parents[1] / "pipeline"
        dead = [
            entry.module
            for entry in module_reachability(pipeline)
            if not entry.reachable
        ]
        self.assertEqual([], dead)


class CriticIntegrationTests(unittest.TestCase):
    """Unreachable parameters must surface as non-actionable critic findings.

    Non-actionable by construction: a dead wire is fixed in the stage that
    owns it, never by editing the DSL value that is failing to reach it.
    """

    def test_unreachable_parameters_become_non_actionable_findings(self) -> None:
        from wge_critic import detect_unreachable_parameters

        spec = _both_profiles()
        with mock.patch.object(
            zone_rasterizer,
            "_composition_scalars",
            SweepDetectsUnwiredParametersTests._freeze("along_jitter", 0.08),
        ):
            with mock.patch("reachability.SWEEP_RESOLUTION", TEST_RESOLUTION):
                findings = detect_unreachable_parameters(copy.deepcopy(spec))
        self.assertTrue(findings)
        for finding in findings:
            self.assertEqual("parameter_unreachable", finding.metric)
            self.assertFalse(finding.actionable)
            self.assertIn("along_jitter", finding.diagnosis)
            self.assertTrue(finding.owner)

    def test_a_healthy_world_produces_no_reachability_findings(self) -> None:
        from wge_critic import detect_unreachable_parameters

        with mock.patch("reachability.SWEEP_RESOLUTION", TEST_RESOLUTION):
            findings = detect_unreachable_parameters(_both_profiles())
        self.assertEqual([], findings)


if __name__ == "__main__":
    unittest.main()
