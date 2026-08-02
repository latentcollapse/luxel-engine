from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from sensitivity import (  # noqa: E402
    AUTHORITY_THRESHOLD,
    DSL_KNOB_NAMES,
    KNOB_BOUNDS,
    _authored_values,
    _far_bound,
    _landform_ids,
    load_matrix,
    metric_owners,
    metrics_from_report,
)
from wge_critic import Finding, apply_sensitivity_matrix  # noqa: E402


def _spec_with(compositions: list[dict]) -> dict:
    return {
        "zone": {"id": "test_zone"},
        "features": [
            {
                "id": f"lf_{index}",
                "category": "landform",
                "generation": {"composition": composition},
            }
            for index, composition in enumerate(compositions)
        ],
    }


class BoundSelectionTests(unittest.TestCase):
    """The perturbation has to be the biggest honest one available.

    A knob nudged a little proves nothing when the question is "does this have
    any authority at all" -- the original misattribution survived precisely
    because a small change produced a small, ambiguous number.
    """

    def test_low_values_drive_to_the_high_bound(self) -> None:
        self.assertEqual(0.5, _far_bound([0.08, 0.10], 0.0, 0.5))

    def test_high_values_drive_to_the_low_bound(self) -> None:
        self.assertEqual(0.0, _far_bound([0.72, 0.64], 0.0, 1.0))

    def test_no_authored_values_defaults_to_the_high_bound(self) -> None:
        self.assertEqual(0.5, _far_bound([], 0.0, 0.5))

    def test_authored_values_are_collected_across_landforms(self) -> None:
        spec = _spec_with([{"along_jitter": 0.1}, {"along_jitter": 0.3}, {}])
        self.assertEqual([0.1, 0.3], _authored_values(spec, "along_jitter"))

    def test_landform_ids_are_collected(self) -> None:
        self.assertEqual(["lf_0", "lf_1"], _landform_ids(_spec_with([{}, {}])))

    def test_every_knob_declares_bounds(self) -> None:
        # A knob measured without bounds has no defined "drive it to its
        # bound", which is the whole semantic of the matrix.
        for knob, (low, high) in KNOB_BOUNDS.items():
            self.assertLess(low, high, knob)


class MetricOwnerTests(unittest.TestCase):
    """An empty owner list is the finding, not an absence of one."""

    def test_a_knob_above_threshold_owns_the_metric(self) -> None:
        matrix = {"m": {"a": AUTHORITY_THRESHOLD * 2, "b": 0.0}}
        self.assertEqual({"m": ["a"]}, metric_owners(matrix))

    def test_a_metric_no_knob_moves_has_no_owner(self) -> None:
        # This is the 0.3%-against-a-6.9x-shortfall case that motivated the
        # whole item: real geometric authority, none over this metric.
        matrix = {"foreground_edge_density": {"along_jitter": 0.003, "spine_count": -0.001}}
        self.assertEqual({"foreground_edge_density": []}, metric_owners(matrix))

    def test_negative_authority_still_counts_as_authority(self) -> None:
        # A knob that drives a metric down owns it just as much as one that
        # drives it up; sign is direction, not strength.
        matrix = {"m": {"a": -AUTHORITY_THRESHOLD * 3}}
        self.assertEqual({"m": ["a"]}, metric_owners(matrix))

    def test_threshold_is_inclusive(self) -> None:
        matrix = {"m": {"a": AUTHORITY_THRESHOLD}}
        self.assertEqual({"m": ["a"]}, metric_owners(matrix))


class ApplySensitivityMatrixTests(unittest.TestCase):
    """Measured authority overrides the critic's hardcoded attribution."""

    @staticmethod
    def _actionable(metric: str, patches: dict) -> Finding:
        return Finding(
            metric=metric,
            observed=0.1,
            target=1.0,
            severity="gate",
            diagnosis="test",
            actionable=True,
            patches=patches,
        )

    def test_an_unowned_metric_is_demoted_to_non_actionable(self) -> None:
        finding = self._actionable("m", {"lf_0": {"along_jitter": 0.3}})
        apply_sensitivity_matrix(
            [finding], {"metric_owners": {"m": []}, "authority_threshold": 0.02}
        )
        self.assertFalse(finding.actionable)
        self.assertEqual({}, finding.patches)
        self.assertIn("no DSL owner", finding.owner)

    def test_an_owned_metric_keeps_patches_naming_owning_knobs(self) -> None:
        finding = self._actionable("m", {"lf_0": {"elevation_bias": 0.9}})
        apply_sensitivity_matrix(
            [finding], {"metric_owners": {"m": ["elevation_bias"]}}
        )
        self.assertTrue(finding.actionable)
        self.assertEqual({"lf_0": {"elevation_bias": 0.9}}, finding.patches)

    def test_patches_naming_powerless_knobs_are_trimmed(self) -> None:
        finding = self._actionable(
            "m", {"lf_0": {"elevation_bias": 0.9, "along_jitter": 0.3}}
        )
        apply_sensitivity_matrix(
            [finding], {"metric_owners": {"m": ["elevation_bias"]}}
        )
        self.assertTrue(finding.actionable)
        self.assertEqual({"lf_0": {"elevation_bias": 0.9}}, finding.patches)

    def test_a_wholly_misattributed_finding_is_demoted(self) -> None:
        # Every proposed knob was measured powerless: the rule was pointed at
        # the wrong lever, which is exactly the incident this item exists for.
        finding = self._actionable("m", {"lf_0": {"along_jitter": 0.3}})
        apply_sensitivity_matrix(
            [finding], {"metric_owners": {"m": ["elevation_bias"]}}
        )
        self.assertFalse(finding.actionable)
        self.assertIn("misattributed", finding.owner)
        self.assertIn("elevation_bias", finding.owner)

    def test_already_non_actionable_findings_are_left_alone(self) -> None:
        finding = Finding(
            metric="m",
            observed=0.1,
            target=1.0,
            severity="gate",
            diagnosis="test",
            actionable=False,
            owner="materials",
        )
        apply_sensitivity_matrix([finding], {"metric_owners": {"m": ["along_jitter"]}})
        self.assertFalse(finding.actionable)
        self.assertEqual("materials", finding.owner)

    def test_the_matrix_never_promotes_a_finding(self) -> None:
        # Promotion would mean inventing a repair from a correlation.
        finding = Finding(
            metric="m",
            observed=0.1,
            target=1.0,
            severity="gate",
            diagnosis="test",
            actionable=False,
            owner="renderer",
        )
        apply_sensitivity_matrix([finding], {"metric_owners": {"m": ["spine_count"]}})
        self.assertFalse(finding.actionable)

    def test_a_metric_absent_from_the_matrix_is_untouched(self) -> None:
        finding = self._actionable("unmeasured", {"lf_0": {"along_jitter": 0.3}})
        apply_sensitivity_matrix([finding], {"metric_owners": {"other": []}})
        self.assertTrue(finding.actionable)
        self.assertEqual({"lf_0": {"along_jitter": 0.3}}, finding.patches)

    def test_no_matrix_leaves_every_finding_untouched(self) -> None:
        # The critic must stay usable on a batch nobody has measured yet.
        finding = self._actionable("m", {"lf_0": {"along_jitter": 0.3}})
        apply_sensitivity_matrix([finding], None)
        self.assertTrue(finding.actionable)


class ReportShapeTests(unittest.TestCase):
    """Both acceptance-report shapes must yield metrics.

    Regression: the first real matrix run compared a single-view perturbed
    report against a --suite baseline, whose metrics are nested under
    overview_acceptance. Reading only the top level made every baseline zero
    and produced a matrix of uniform +100% entries -- a fabricated result that
    looked like a measurement.
    """

    def test_single_view_report_metrics_are_found(self) -> None:
        self.assertEqual(
            {"a": 1.0}, metrics_from_report({"metrics": {"a": 1.0}})
        )

    def test_suite_report_metrics_are_found(self) -> None:
        report = {
            "schema_version": "codeweald.bevy-inspection-suite/v1",
            "overview_acceptance": {"metrics": {"a": 2.0}},
        }
        self.assertEqual({"a": 2.0}, metrics_from_report(report))

    def test_a_suite_report_with_null_top_level_metrics_still_resolves(self) -> None:
        # The exact shape that caused the bug: `metrics` present but null.
        report = {"metrics": None, "overview_acceptance": {"metrics": {"a": 3.0}}}
        self.assertEqual({"a": 3.0}, metrics_from_report(report))

    def test_a_report_with_no_metrics_yields_empty(self) -> None:
        self.assertEqual({}, metrics_from_report({"status": "passed"}))


class UnmeasurableMetricTests(unittest.TestCase):
    """A zero baseline means unmeasurable, never 'fully owned'."""

    def test_unmeasurable_metrics_are_not_reported_as_owned(self) -> None:
        # Previously a zero baseline produced relative_change=1.0, which
        # cleared the authority threshold and made every knob own everything.
        matrix = {"m": {}}
        self.assertEqual({"m": []}, metric_owners(matrix))

    def test_the_critic_does_not_demote_on_an_unmeasurable_metric(self) -> None:
        # An absent measurement is not evidence the metric has no owner;
        # demoting on it would silence a real finding for no reason.
        finding = ApplySensitivityMatrixTests._actionable(
            "m", {"lf_0": {"along_jitter": 0.3}}
        )
        apply_sensitivity_matrix(
            [finding],
            {"metric_owners": {"m": []}, "unmeasurable_metrics": ["m"]},
        )
        self.assertTrue(finding.actionable)
        self.assertEqual({"lf_0": {"along_jitter": 0.3}}, finding.patches)


class PatchKeyTranslationTests(unittest.TestCase):
    """The authoring surface and the ZoneSpec disagree on one name.

    Regression: the DSL calls the spine count ``spines`` while the ZoneSpec,
    manifest and rasterizer all call it ``spine_count``. A patch written with
    the ZoneSpec name matched no line in the generated intent and was dropped
    without a word, producing a sensitivity row of exactly +0.0% across every
    metric -- a fabricated "this knob controls nothing" result for a knob that
    demonstrably moves the heightfield.
    """

    def test_spine_count_is_translated_to_the_authoring_name(self) -> None:
        self.assertEqual("spines", DSL_KNOB_NAMES["spine_count"])

    def test_knobs_without_a_translation_pass_through(self) -> None:
        for knob in ("along_jitter", "cross_jitter", "elevation_bias"):
            self.assertEqual(knob, DSL_KNOB_NAMES.get(knob, knob))

    def test_every_measured_knob_resolves_to_an_authoring_name(self) -> None:
        # A knob whose authoring name is wrong measures as powerless rather
        # than failing, so this has to be checked rather than assumed.
        from wge_critic import scaffold_intent

        spec = {
            "zone": {"id": "z"},
            "features": [
                {
                    "id": "lf",
                    "category": "landform",
                    "semantic": "alpine_massif",
                    "generation": {
                        "profile": "alpine_jagged_massif",
                        "composition": {
                            "pattern": "ridge_network",
                            "spine_count": 3,
                            "along_jitter": 0.08,
                            "cross_jitter": 0.10,
                            "elevation_bias": 0.72,
                        },
                    },
                }
            ],
        }
        emitted = scaffold_intent(spec)
        for knob in KNOB_BOUNDS:
            name = DSL_KNOB_NAMES.get(knob, knob)
            self.assertIn(f"{name}=", emitted, f"{knob} -> {name}")


class ApplyToScaffoldDropTests(unittest.TestCase):
    """A repair that matches nothing must fail loudly, not vanish."""

    @staticmethod
    def _spec() -> dict:
        return {
            "zone": {"id": "z"},
            "features": [
                {
                    "id": "lf",
                    "category": "landform",
                    "semantic": "alpine_massif",
                    "generation": {
                        "profile": "alpine_jagged_massif",
                        "composition": {
                            "pattern": "ridge_network",
                            "spine_count": 3,
                            "along_jitter": 0.08,
                            "cross_jitter": 0.10,
                            "elevation_bias": 0.72,
                        },
                    },
                }
            ],
        }

    def test_an_unmatched_patch_key_raises(self) -> None:
        from wge_critic import apply_to_scaffold

        with self.assertRaises(ValueError) as caught:
            apply_to_scaffold(self._spec(), {"lf": {"spine_count": 8}})
        self.assertIn("spine_count", str(caught.exception))

    def test_an_unknown_feature_id_raises(self) -> None:
        from wge_critic import apply_to_scaffold

        with self.assertRaises(ValueError):
            apply_to_scaffold(self._spec(), {"nonexistent": {"spines": 8}})

    def test_a_matching_patch_applies_and_does_not_raise(self) -> None:
        from wge_critic import apply_to_scaffold

        emitted = apply_to_scaffold(self._spec(), {"lf": {"spines": 8}})
        self.assertIn("spines=8,", emitted)

    def test_spine_values_are_rendered_as_integers(self) -> None:
        from wge_critic import apply_to_scaffold

        emitted = apply_to_scaffold(self._spec(), {"lf": {"spines": 8.0}})
        self.assertIn("spines=8,", emitted)
        self.assertNotIn("spines=8.0,", emitted)


class LoadMatrixTests(unittest.TestCase):
    def test_a_missing_matrix_loads_as_none(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            self.assertIsNone(load_matrix(Path(temporary)))

    def test_a_corrupt_matrix_loads_as_none_rather_than_raising(self) -> None:
        # A damaged matrix must not take the critic down with it; the critic
        # still works without one.
        with tempfile.TemporaryDirectory() as temporary:
            batch = Path(temporary)
            (batch / "sensitivity_matrix.json").write_text("{not json", encoding="utf-8")
            self.assertIsNone(load_matrix(batch))

    def test_a_written_matrix_round_trips(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            batch = Path(temporary)
            document = {"metric_owners": {"m": ["a"]}}
            (batch / "sensitivity_matrix.json").write_text(
                json.dumps(document), encoding="utf-8"
            )
            self.assertEqual(document, load_matrix(batch))


if __name__ == "__main__":
    unittest.main()
