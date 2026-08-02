#!/usr/bin/env python3
"""Capture and evaluate one certified world through the Bevy backend."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path
from typing import Iterable

from bevy_visual_acceptance import evaluate
from tool_provenance import source_tree_digest


VIEWS = ("overview", "west-wall", "east-wall", "player", "border")

# Sources whose changes must be reflected in the viewer binary before a capture
# means anything. Must match build_viewer.py's VIEWER_SOURCE_DIRECTORIES.
VIEWER_SOURCE_DIRECTORIES = ("apps", "crates")


def _viewer_source_digest(world_core: Path) -> str:
    roots = [world_core / directory for directory in VIEWER_SOURCE_DIRECTORIES]
    return source_tree_digest(roots, extensions=(".rs",))


def _viewer_provenance_mismatch(viewer: Path, world_core: Path) -> str | None:
    """None if the binary's self-reported build digest matches current
    sources; otherwise a human-readable reason it does not.

    A capture is evidence, and evidence produced by a binary that predates the
    change under test is worse than no evidence: it looks like a successful
    measurement of the new behaviour. This tool used to compare file mtimes,
    which is fragile against git checkouts, rsync, and backup restores that
    reset mtimes without changing content -- and it once defaulted to the
    debug profile while work was being compiled `--release`, so the default
    binary silently went stale for a full day of captures that kept passing.
    Content-hash provenance, self-reported by the binary via `--provenance`,
    closes both holes: it is exact regardless of mtimes, and a binary built
    any other way than build_viewer.py reports "unknown" rather than a
    plausible-looking but unverified digest.
    """
    try:
        reported = subprocess.run(
            [str(viewer), "--provenance"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        return f"could not query {viewer.name} --provenance: {exc}"
    if reported == "unknown":
        return (
            f"{viewer.name} has no source-tree digest baked in (built with plain "
            "`cargo build`/`cargo run` rather than build_viewer.py, so its "
            "provenance cannot be verified)"
        )
    expected = _viewer_source_digest(world_core)
    if reported != expected:
        return (
            f"{viewer.name} was built from digest {reported[:12]}, but current "
            f"sources hash to {expected[:12]} -- it predates a source change"
        )
    return None


def _capture(
    viewer: Path, world_core: Path, batch: Path, destination: Path, view: str
) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            str(viewer),
            "--batch",
            str(batch),
            "--capture",
            str(destination),
            "--view",
            view,
        ],
        cwd=world_core,
        check=True,
    )


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Capture and visually gate a compiled Codeweald world"
    )
    parser.add_argument("batch", type=Path)
    parser.add_argument("--viewer-bin", type=Path)
    parser.add_argument("--capture", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--view", choices=VIEWS, default="overview")
    parser.add_argument(
        "--suite",
        action="store_true",
        help="capture overview, both boundary walls, and the player view",
    )
    arguments = parser.parse_args(argv)
    if arguments.suite and arguments.capture is not None:
        parser.error("--capture cannot be combined with --suite")

    batch = arguments.batch.resolve()
    # The reference renderer ships with the engine, so this is one level up
    # from pipeline/ -- not two, which pointed above the engine entirely once
    # WGE moved out of the game it used to live inside (D8).
    world_core = Path(__file__).resolve().parents[1] / "world_core"
    viewer = (
        arguments.viewer_bin.resolve()
        if arguments.viewer_bin is not None
        else world_core / "target/release/codeweald-world-viewer"
    )
    # Default artifact names follow --view, the same way --suite names them.
    # A fixed default meant `--view player` rendered the player camera and wrote
    # it over bevy_overview.png, and wrote a "captured" stub over the overview's
    # acceptance report -- so a low-angle inspection silently replaced the
    # evidence for the gated view, and the next reader compared two different
    # cameras without any indication that had happened.
    capture = (
        arguments.capture.resolve()
        if arguments.capture is not None
        else batch / f"bevy_{arguments.view.replace('-', '_')}.png"
    )
    # T7 / D10: a suite capture and a single-view "overview" capture used to
    # both default to "bevy_visual_acceptance_report.json" with different
    # schemas -- metrics at the top level for one, nested under
    # "overview_acceptance" for the other. A reader that assumed one shape
    # silently got {} from the other; this zeroed every baseline in the first
    # sensitivity matrix run. Giving the suite its own filename removes the
    # ambiguity at the source rather than only handling it in every reader.
    report_path = (
        arguments.report.resolve()
        if arguments.report is not None
        else batch
        / (
            "bevy_visual_acceptance_suite_report.json"
            if arguments.suite
            else "bevy_visual_acceptance_report.json"
            if arguments.view == "overview"
            else f"bevy_{arguments.view.replace('-', '_')}_capture_report.json"
        )
    )
    zone_spec_path = batch / "zone_spec.json"
    if not viewer.is_file():
        parser.error(
            f"{viewer} does not exist; build it with "
            "pipeline/build_viewer.py"
        )
    provenance_problem = _viewer_provenance_mismatch(viewer, world_core)
    if provenance_problem is not None:
        parser.error(
            f"{provenance_problem}. Rebuild with pipeline/build_viewer.py before "
            "capturing, or pass --viewer-bin to choose a different binary."
        )
    if not zone_spec_path.is_file():
        parser.error(f"{zone_spec_path} does not exist; compile the batch first")

    try:
        zone_spec = json.loads(zone_spec_path.read_text(encoding="utf-8"))
        if arguments.suite:
            captures = {}
            for view in VIEWS:
                destination = batch / f"bevy_{view.replace('-', '_')}.png"
                _capture(viewer, world_core, batch, destination, view)
                captures[view] = {
                    "path": destination.name,
                    "sha256": hashlib.sha256(destination.read_bytes()).hexdigest(),
                }
            overview = batch / "bevy_overview.png"
            acceptance = evaluate(zone_spec, overview)
            result = {
                "schema_version": "codeweald.bevy-inspection-suite/v1",
                "zone_id": acceptance["zone_id"],
                "status": acceptance["status"],
                "overview_acceptance": acceptance,
                "captures": captures,
                "views_requiring_semantic_review": [
                    "west-wall",
                    "east-wall",
                    "player",
                ],
            }
        else:
            _capture(viewer, world_core, batch, capture, arguments.view)
            if arguments.view == "overview":
                result = evaluate(zone_spec, capture)
            else:
                result = {
                    "schema_version": "codeweald.bevy-inspection-capture/v1",
                    "zone_id": zone_spec["zone"]["id"],
                    "status": "captured",
                    "view": arguments.view,
                    "capture": capture.name,
                    "sha256": hashlib.sha256(capture.read_bytes()).hexdigest(),
                    "note": "low-angle views require semantic review; overview pixel gates do not apply",
                }
    except (
        OSError,
        ValueError,
        json.JSONDecodeError,
        subprocess.CalledProcessError,
    ) as exc:
        parser.error(str(exc))
    # The report is evidence; make it self-certifying rather than trusting a
    # reader to separately go verify the binary that produced it still
    # matches what's on disk now.
    result["viewer_source_digest"] = _viewer_source_digest(world_core)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print("Bevy capture %s: %s" % (result["zone_id"], result["status"]))
    return 0 if result["status"] in {"passed", "captured"} else 2


if __name__ == "__main__":
    raise SystemExit(main())
