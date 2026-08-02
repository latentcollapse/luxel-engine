from __future__ import annotations

import contextlib
import io
import json
import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "pipeline"))

from capture_bevy import _viewer_provenance_mismatch, _viewer_source_digest, main  # noqa: E402

REAL_WORLD_CORE = Path(__file__).resolve().parents[1] / "world_core"


FAKE_VIEWER_SOURCE = """#!/usr/bin/env python3
import os
import sys
from pathlib import Path

from PIL import Image

arguments = sys.argv[1:]
if "--provenance" in arguments:
    print(os.environ.get("FAKE_VIEWER_PROVENANCE_DIGEST", "unknown"))
    sys.exit(0)
capture = Path(arguments[arguments.index("--capture") + 1])
capture.parent.mkdir(parents=True, exist_ok=True)
Image.new("RGB", (512, 512), (60, 90, 70)).save(capture)
"""


def _write_fake_viewer(path: Path) -> None:
    """A stand-in for the compiled world-viewer binary.

    `main` only ever invokes the path it is given, so a Python script with the
    same CLI surface (accepting `--provenance`, or `--capture <path>` and
    writing a PNG there) is indistinguishable from the real binary for
    everything this module does.
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(FAKE_VIEWER_SOURCE, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)


def _write_batch(batch: Path) -> None:
    batch.mkdir(parents=True, exist_ok=True)
    (batch / "zone_spec.json").write_text(
        json.dumps({"zone": {"id": "test_zone"}}), encoding="utf-8"
    )


class ViewerProvenanceMismatchTests(unittest.TestCase):
    """`_viewer_provenance_mismatch` gates every capture: a binary must prove
    (by self-reported content-hash digest, not mtime) that it was built from
    the sources on disk right now, or it is refused.
    """

    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary.cleanup)
        self.viewer = Path(self._temporary.name) / "viewer.py"
        _write_fake_viewer(self.viewer)
        self.expected = _viewer_source_digest(REAL_WORLD_CORE)

    def test_matching_digest_is_accepted(self) -> None:
        with mock.patch.dict(
            os.environ, {"FAKE_VIEWER_PROVENANCE_DIGEST": self.expected}
        ):
            self.assertIsNone(
                _viewer_provenance_mismatch(self.viewer, REAL_WORLD_CORE)
            )

    def test_unknown_provenance_is_refused(self) -> None:
        with mock.patch.dict(
            os.environ, {"FAKE_VIEWER_PROVENANCE_DIGEST": "unknown"}
        ):
            reason = _viewer_provenance_mismatch(self.viewer, REAL_WORLD_CORE)
        self.assertIsNotNone(reason)
        self.assertIn("build_viewer.py", reason)

    def test_mismatched_digest_is_refused(self) -> None:
        with mock.patch.dict(
            os.environ, {"FAKE_VIEWER_PROVENANCE_DIGEST": "f" * 64}
        ):
            reason = _viewer_provenance_mismatch(self.viewer, REAL_WORLD_CORE)
        self.assertIsNotNone(reason)
        self.assertIn("predates a source change", reason)

    def test_viewer_that_cannot_run_is_refused(self) -> None:
        broken = Path(self._temporary.name) / "does-not-exist"
        reason = _viewer_provenance_mismatch(broken, REAL_WORLD_CORE)
        self.assertIsNotNone(reason)
        self.assertIn("could not query", reason)


class DefaultArtifactPathsFollowViewTests(unittest.TestCase):
    """Pins the fix for a silent evidence swap.

    A fixed default capture/report path meant `--view player` overwrote
    `bevy_overview.png` and stomped the gated overview's acceptance report
    with a "captured" stub, so a low-angle inspection capture silently
    replaced the overview's evidence and two different cameras were compared
    with nothing to indicate that had happened.
    """

    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary.cleanup)
        self.root = Path(self._temporary.name)
        self.batch = self.root / "batch"
        _write_batch(self.batch)
        self.viewer = self.root / "viewer.py"
        _write_fake_viewer(self.viewer)
        expected = _viewer_source_digest(REAL_WORLD_CORE)
        self._env_patch = mock.patch.dict(
            os.environ, {"FAKE_VIEWER_PROVENANCE_DIGEST": expected}
        )
        self._env_patch.start()
        self.addCleanup(self._env_patch.stop)

    def test_view_player_does_not_touch_overview_artifacts(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            main(
                [
                    str(self.batch),
                    "--viewer-bin",
                    str(self.viewer),
                    "--view",
                    "player",
                ]
            )
        self.assertTrue((self.batch / "bevy_player.png").is_file())
        self.assertTrue((self.batch / "bevy_player_capture_report.json").is_file())
        self.assertFalse((self.batch / "bevy_overview.png").exists())
        self.assertFalse(
            (self.batch / "bevy_visual_acceptance_report.json").exists()
        )

    def test_view_west_wall_maps_hyphen_to_underscore(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            main(
                [
                    str(self.batch),
                    "--viewer-bin",
                    str(self.viewer),
                    "--view",
                    "west-wall",
                ]
            )
        self.assertTrue((self.batch / "bevy_west_wall.png").is_file())
        self.assertTrue(
            (self.batch / "bevy_west_wall_capture_report.json").is_file()
        )

    def test_view_overview_keeps_the_unchanged_report_name(self) -> None:
        # Other pipeline stages read bevy_visual_acceptance_report.json by
        # name, so the overview report's filename is the one default that
        # must NOT change even though it is now derived the same way as the
        # others.
        with contextlib.redirect_stderr(io.StringIO()):
            main(
                [
                    str(self.batch),
                    "--viewer-bin",
                    str(self.viewer),
                    "--view",
                    "overview",
                ]
            )
        self.assertTrue((self.batch / "bevy_overview.png").is_file())
        self.assertTrue(
            (self.batch / "bevy_visual_acceptance_report.json").is_file()
        )

    def test_suite_report_does_not_collide_with_the_overview_report(self) -> None:
        # T7 / D10: --suite used to default to the same filename as a plain
        # --view overview capture, with a different schema (metrics nested
        # under overview_acceptance instead of at the top level). A reader
        # that assumed one shape silently read {} from the other -- this
        # zeroed every baseline in the first sensitivity matrix run. The
        # suite must write somewhere else entirely, not just a shape a
        # careful reader can disambiguate.
        overview_report = self.batch / "bevy_visual_acceptance_report.json"
        sentinel = json.dumps({"schema_version": "sentinel", "metrics": {"x": 1.0}})
        overview_report.write_text(sentinel, encoding="utf-8")
        with contextlib.redirect_stderr(io.StringIO()):
            main([str(self.batch), "--viewer-bin", str(self.viewer), "--suite"])
        suite_report_path = self.batch / "bevy_visual_acceptance_suite_report.json"
        self.assertTrue(suite_report_path.is_file())
        self.assertEqual(sentinel, overview_report.read_text(encoding="utf-8"))
        suite_report = json.loads(suite_report_path.read_text(encoding="utf-8"))
        self.assertEqual(
            "codeweald.bevy-inspection-suite/v1", suite_report["schema_version"]
        )
        self.assertIn("overview_acceptance", suite_report)
        self.assertNotIn("metrics", suite_report)

    def test_explicit_capture_path_wins_over_derived_default(self) -> None:
        explicit = self.batch / "custom_capture.png"
        with contextlib.redirect_stderr(io.StringIO()):
            main(
                [
                    str(self.batch),
                    "--viewer-bin",
                    str(self.viewer),
                    "--view",
                    "player",
                    "--capture",
                    str(explicit),
                ]
            )
        self.assertTrue(explicit.is_file())
        self.assertFalse((self.batch / "bevy_player.png").exists())

    def test_pre_existing_overview_png_is_untouched_by_a_player_capture(self) -> None:
        overview = self.batch / "bevy_overview.png"
        sentinel_bytes = b"not a real png, just a sentinel"
        overview.write_bytes(sentinel_bytes)
        with contextlib.redirect_stderr(io.StringIO()):
            main(
                [
                    str(self.batch),
                    "--viewer-bin",
                    str(self.viewer),
                    "--view",
                    "player",
                ]
            )
        self.assertEqual(sentinel_bytes, overview.read_bytes())

    def test_report_records_the_viewer_source_digest(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()):
            main(
                [
                    str(self.batch),
                    "--viewer-bin",
                    str(self.viewer),
                    "--view",
                    "overview",
                ]
            )
        report = json.loads(
            (self.batch / "bevy_visual_acceptance_report.json").read_text()
        )
        self.assertEqual(
            _viewer_source_digest(REAL_WORLD_CORE), report["viewer_source_digest"]
        )


class ProvenanceGuardIntegrationTests(unittest.TestCase):
    """The guard runs before any capture happens: a viewer with unverifiable
    or mismatched provenance must never get as far as producing evidence.
    """

    def test_unknown_provenance_viewer_is_rejected_before_capturing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            batch = Path(temporary) / "batch"
            _write_batch(batch)
            viewer = Path(temporary) / "viewer.py"
            _write_fake_viewer(viewer)

            stderr = io.StringIO()
            with mock.patch.dict(
                os.environ, {"FAKE_VIEWER_PROVENANCE_DIGEST": "unknown"}
            ):
                with contextlib.redirect_stderr(stderr):
                    with self.assertRaises(SystemExit):
                        main(
                            [
                                str(batch),
                                "--viewer-bin",
                                str(viewer),
                                "--view",
                                "overview",
                            ]
                        )
            self.assertIn("build_viewer.py", stderr.getvalue())
            self.assertFalse((batch / "bevy_overview.png").exists())


if __name__ == "__main__":
    unittest.main()
