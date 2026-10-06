"""The authoring DSL's composition scalars must reach terrain geometry.

These are regression tests for a specific class of failure rather than for a
specific bug: an authoring knob that validates, serializes, survives the whole
build, and changes nothing. That failure is invisible to every other test in
the suite -- the build stays green, the world stays valid, and the only symptom
is a person editing a number and watching the terrain not move.

The check is therefore always the same shape: rasterize twice with one scalar
changed and assert the heightfield hash moved.
"""

import copy
import hashlib
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

PIPELINE = Path(__file__).resolve().parents[1] / "pipeline"
sys.path.insert(0, str(PIPELINE))

from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError
from zone_rasterizer import MAX_SPINE_COUNT, rasterize_zone_spec, write_raster
from luxel_critic import detect_no_ops, load_terrain_manifests
from worldbuilder_dsl import _PATTERN_SCALARS

RESOLUTION = 129


def _polygon(cx: float, cz: float, half_width: float, half_length: float):
    return [
        [cx - half_width, cz - half_length],
        [cx + half_width, cz - half_length],
        [cx + half_width, cz + half_length],
        [cx - half_width, cz + half_length],
    ]


def _spec(semantic: str, profile: str, pattern: str, **composition):
    scalars = _PATTERN_SCALARS[pattern]
    resolved = {
        "pattern": pattern,
        "spine_count": scalars["spines"],
        "elevation_bias": scalars["elevation_bias"],
        "along_jitter": scalars["along_jitter"],
        "cross_jitter": scalars["cross_jitter"],
        "silhouette": (
            "continuous_boundary_wall"
            if pattern == "ridge_network"
            else "broken_ridge_cluster"
        ),
        "massing": "terrain_primary",
        "surface": "fractured_granite",
        "dressing": "none",
    }
    resolved.update(composition)
    return {
        "schema_version": ZONE_SPEC_VERSION,
        "generation_seed": 20260731,
        "zone": {
            "id": "composition_wiring_fixture",
            "world_bounds": {"width": 400.0, "length": 400.0},
        },
        "features": [
            {
                "id": "subject",
                "category": "landform",
                "semantic": semantic,
                "geometry": {"type": "polygon", "points": _polygon(0.0, 0.0, 140.0, 70.0)},
                "properties": {},
                "generation": {
                    "profile": profile,
                    "elevation_m": [30.0, 120.0],
                    "composition": resolved,
                },
            }
        ],
    }


def _alpine(**composition):
    return _spec(
        "alpine_massif", "alpine_jagged_massif", "ridge_network", **composition
    )


def _crag(**composition):
    return _spec(
        "crag_field", "scattered_crag_field", "clustered_ridges", **composition
    )


def _heightfield_digest(spec) -> str:
    raster = rasterize_zone_spec(spec, resolution=RESOLUTION)
    return hashlib.sha256(
        np.ascontiguousarray(raster.height_m, dtype="<f4").tobytes()
    ).hexdigest()


class CompositionReachesGeometryTests(unittest.TestCase):
    """Every authored scalar must be able to move the terrain."""

    def _assert_moves(self, builder, scalar, *values):
        baseline = _heightfield_digest(builder())
        for value in values:
            with self.subTest(scalar=scalar, value=value):
                changed = _heightfield_digest(builder(**{scalar: value}))
                self.assertNotEqual(
                    baseline,
                    changed,
                    f"{scalar}={value} left the heightfield byte-identical; the "
                    f"authoring surface is decorative for this parameter",
                )

    def test_alpine_spine_count_changes_terrain(self):
        self._assert_moves(_alpine, "spine_count", 1, 2, 4, 6, MAX_SPINE_COUNT)

    def test_alpine_along_jitter_changes_terrain(self):
        self._assert_moves(_alpine, "along_jitter", 0.0, 0.3, 0.5)

    def test_alpine_cross_jitter_changes_terrain(self):
        self._assert_moves(_alpine, "cross_jitter", 0.0, 0.3, 0.5)

    def test_crag_spine_count_changes_terrain(self):
        self._assert_moves(_crag, "spine_count", 1, 2, 5, MAX_SPINE_COUNT)

    def test_crag_along_jitter_changes_terrain(self):
        self._assert_moves(_crag, "along_jitter", 0.0, 0.5)

    def test_crag_cross_jitter_changes_terrain(self):
        self._assert_moves(_crag, "cross_jitter", 0.0, 0.5)

    def test_distinct_spine_counts_stay_distinct(self):
        """Not merely "different from default" -- monotonically distinguishable."""
        digests = {
            count: _heightfield_digest(_alpine(spine_count=count))
            for count in range(1, MAX_SPINE_COUNT + 1)
        }
        self.assertEqual(
            len(set(digests.values())),
            len(digests),
            "two different spine counts produced the same terrain",
        )


class CompositionValidationTests(unittest.TestCase):
    """Out-of-range authoring is rejected by name, not silently clamped."""

    def _assert_rejected(self, message_fragment, **composition):
        with self.assertRaises(ZoneCompileError) as caught:
            rasterize_zone_spec(_alpine(**composition), resolution=RESOLUTION)
        self.assertIn(message_fragment, str(caught.exception))
        self.assertIn("subject", str(caught.exception))

    def test_spine_count_below_range_rejected(self):
        self._assert_rejected("spine_count", spine_count=0)

    def test_spine_count_above_range_rejected(self):
        self._assert_rejected("spine_count", spine_count=MAX_SPINE_COUNT + 1)

    def test_fractional_spine_count_rejected(self):
        self._assert_rejected("spine_count", spine_count=2.5)

    def test_non_numeric_spine_count_rejected(self):
        self._assert_rejected("spine_count", spine_count="several")

    def test_jitter_above_range_rejected(self):
        self._assert_rejected("along_jitter", along_jitter=0.9)

    def test_negative_jitter_rejected(self):
        self._assert_rejected("cross_jitter", cross_jitter=-0.01)

    def test_omitted_scalars_fall_back_to_pattern_defaults(self):
        spec = _alpine()
        del spec["features"][0]["generation"]["composition"]["spine_count"]
        del spec["features"][0]["generation"]["composition"]["along_jitter"]
        del spec["features"][0]["generation"]["composition"]["cross_jitter"]
        self.assertEqual(_heightfield_digest(spec), _heightfield_digest(_alpine()))


class CompositionManifestTests(unittest.TestCase):
    """The manifest reports what was built, so no-ops are detectable."""

    def test_manifest_reports_resolved_scalars(self):
        raster = rasterize_zone_spec(
            _alpine(
                spine_count=5,
                along_jitter=0.25,
                cross_jitter=0.05,
                elevation_bias=0.4,
            ),
            resolution=RESOLUTION,
        )
        landform = raster.manifest["landforms"][0]
        self.assertEqual(
            landform["composition_scalars"],
            {
                "spine_count": 5,
                "along_jitter": 0.25,
                "cross_jitter": 0.05,
                "elevation_bias": 0.4,
            },
        )

    def test_manifest_records_every_authored_scalar(self):
        # Regression: elevation_bias reached the geometry but was omitted from
        # the manifest, so detect_no_ops -- which works by diffing manifests --
        # was structurally blind to it going inert. A knob absent from the
        # manifest cannot be caught by a manifest diff, which makes the
        # omission a hole in the detector rather than a cosmetic gap.
        raster = rasterize_zone_spec(_alpine(), resolution=RESOLUTION)
        recorded = set(raster.manifest["landforms"][0]["composition_scalars"])
        authored = {
            key
            for key in _alpine()["features"][0]["generation"]["composition"]
            if key not in {"pattern", "silhouette", "massing", "surface", "dressing"}
        }
        self.assertEqual(authored, recorded)

    def test_manifest_carries_heightfield_identity(self):
        quiet = rasterize_zone_spec(_alpine(), resolution=RESOLUTION)
        loud = rasterize_zone_spec(_alpine(spine_count=6), resolution=RESOLUTION)
        self.assertEqual(
            quiet.manifest["heightfield_sha256"],
            _heightfield_digest(_alpine()),
        )
        self.assertNotEqual(
            quiet.manifest["heightfield_sha256"],
            loud.manifest["heightfield_sha256"],
        )

    def test_zone_spec_hash_and_heightfield_hash_are_independent(self):
        """A spec edit that cannot reach geometry must be visible as such.

        This is the signal the no-op detector reads: the spec hash moved and
        the heightfield hash did not.
        """
        baseline = rasterize_zone_spec(_alpine(), resolution=RESOLUTION)
        decorated = copy.deepcopy(_alpine())
        decorated["features"][0]["properties"]["note"] = "cosmetic"
        other = rasterize_zone_spec(decorated, resolution=RESOLUTION)
        self.assertNotEqual(
            baseline.manifest["zone_spec_sha256"],
            other.manifest["zone_spec_sha256"],
        )
        self.assertEqual(
            baseline.manifest["heightfield_sha256"],
            other.manifest["heightfield_sha256"],
        )


def _manifest(heightfield: str, **scalars_by_id) -> dict:
    return {
        "heightfield_sha256": heightfield,
        "landforms": [
            {"id": fid, "composition_scalars": scalars}
            for fid, scalars in scalars_by_id.items()
        ],
    }


class NoOpDetectorTests(unittest.TestCase):
    """The general check: authoring moved, terrain did not."""

    def test_inert_parameter_is_reported_with_an_owner(self):
        before = _manifest("same", ridge={"spine_count": 3, "cross_jitter": 0.1})
        after = _manifest("same", ridge={"spine_count": 8, "cross_jitter": 0.1})
        findings = detect_no_ops(before, after)
        self.assertEqual(1, len(findings))
        finding = findings[0]
        self.assertFalse(finding.actionable)
        self.assertIn("spine_count", finding.diagnosis)
        self.assertNotIn("cross_jitter", finding.diagnosis)
        self.assertIn("zone_rasterizer", finding.owner)

    def test_a_newly_recorded_scalar_is_not_a_no_op(self):
        # Regression: recording elevation_bias in the manifest for the first
        # time made the detector fire against it, because a key absent from
        # the previous manifest compared unequal to its new value. That
        # reported a schema addition as a dead knob -- and named a scalar the
        # sensitivity matrix measures at -57.7% authority over
        # dark_foreground_fraction. A detector that cries wolf on its own
        # upgrades trains its reader to ignore it.
        before = _manifest("same", ridge={"spine_count": 3})
        after = _manifest("same", ridge={"spine_count": 3, "elevation_bias": 0.72})
        self.assertEqual([], detect_no_ops(before, after))

    def test_a_newly_recorded_scalar_does_not_mask_a_real_no_op(self):
        # The narrowing must not swallow a genuine inert knob sharing the
        # same build as a schema addition.
        before = _manifest("same", ridge={"spine_count": 3})
        after = _manifest(
            "same", ridge={"spine_count": 8, "elevation_bias": 0.72}
        )
        findings = detect_no_ops(before, after)
        self.assertEqual(1, len(findings))
        self.assertIn("spine_count", findings[0].diagnosis)
        self.assertNotIn("elevation_bias", findings[0].diagnosis)

    def test_terrain_that_moved_is_not_a_no_op(self):
        before = _manifest("before", ridge={"spine_count": 3})
        after = _manifest("after", ridge={"spine_count": 8})
        self.assertEqual([], detect_no_ops(before, after))

    def test_unchanged_authoring_is_not_a_no_op(self):
        identical = _manifest("same", ridge={"spine_count": 3})
        self.assertEqual([], detect_no_ops(identical, identical))

    def test_missing_history_is_silent(self):
        current = _manifest("same", ridge={"spine_count": 3})
        self.assertEqual([], detect_no_ops({}, current))
        self.assertEqual([], detect_no_ops(current, {}))

    def test_manifests_without_hashes_are_silent(self):
        before = {"landforms": [{"id": "ridge", "composition_scalars": {"spine_count": 3}}]}
        after = {"landforms": [{"id": "ridge", "composition_scalars": {"spine_count": 8}}]}
        self.assertEqual([], detect_no_ops(before, after))

    def test_every_changed_landform_is_named(self):
        before = _manifest(
            "same", west={"spine_count": 3}, east={"along_jitter": 0.1}
        )
        after = _manifest(
            "same", west={"spine_count": 6}, east={"along_jitter": 0.4}
        )
        diagnosis = detect_no_ops(before, after)[0].diagnosis
        self.assertIn("west", diagnosis)
        self.assertIn("east", diagnosis)

    def test_detector_catches_a_genuinely_unwired_parameter(self):
        """End to end against the real rasterizer, with one knob disconnected.

        The original defect was exactly this: the DSL wrote spine_count into
        the spec and the geometry never read it. Simulating a disconnect proves
        the detector reads terrain, not intentions.
        """
        wired = rasterize_zone_spec(_alpine(spine_count=3), resolution=RESOLUTION)
        moved = rasterize_zone_spec(_alpine(spine_count=7), resolution=RESOLUTION)
        self.assertEqual([], detect_no_ops(wired.manifest, moved.manifest))

        # Same authored change, but with the geometry frozen -- what an
        # unwired parameter looks like from outside the rasterizer.
        unwired = dict(moved.manifest)
        unwired["heightfield_sha256"] = wired.manifest["heightfield_sha256"]
        findings = detect_no_ops(wired.manifest, unwired)
        self.assertEqual(1, len(findings))
        self.assertIn("spine_count", findings[0].diagnosis)


class ManifestRotationTests(unittest.TestCase):
    """Consecutive builds leave the critic something to compare."""

    def test_previous_manifest_is_kept_across_builds(self):
        with tempfile.TemporaryDirectory() as tmp:
            batch = Path(tmp)
            terrain = batch / "terrain"
            terrain.mkdir()
            first = rasterize_zone_spec(_alpine(spine_count=3), resolution=RESOLUTION)
            write_raster(first, terrain)
            self.assertEqual(({}, first.manifest), load_terrain_manifests(batch))

            second = rasterize_zone_spec(_alpine(spine_count=7), resolution=RESOLUTION)
            write_raster(second, terrain)
            previous, current = load_terrain_manifests(batch)
            self.assertEqual(first.manifest, previous)
            self.assertEqual(second.manifest, current)
            self.assertEqual([], detect_no_ops(previous, current))


if __name__ == "__main__":
    unittest.main()
