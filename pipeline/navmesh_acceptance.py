"""Roadmap 1.4: the navmesh acceptance gate.

`navigation_plan` (1.3) measures the navigable surface and reports whether
lanes run end to end, whether keeps share one connected component, and which
anchors -- including biome anchors -- are stranded or unreachable. It does
not refuse anything; it is evidence, not a verdict. This module is the
verdict: lanes connected end to end, each keep reachable from each other
keep, jungle (biome) reachable from an adjacent lane.

Deliberately thin. Every fact this gate checks is already computed by
`navigation_plan.build` -- `lanes[].runs_end_to_end`,
`topology.keeps_share_one_component`, `topology.stranded_anchors`,
`topology.unreachable_anchors`. Recomputing any of it here would be the
"write the AST interpreter twice" mistake this codebase avoids elsewhere;
this module only translates those facts into a pass/fail with the entity
named, not just a fraction.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

SCHEMA_VERSION = "codeweald.navmesh-acceptance/v1"


def evaluate(navigation_plan: dict[str, Any]) -> dict[str, Any]:
    """Gate a navigation plan against roadmap 1.4's three requirements."""
    failures: list[str] = []

    for lane in navigation_plan.get("lanes", []):
        if not lane.get("runs_end_to_end"):
            failures.append(
                "Lane %s does not run end to end (%.1f%% navigable, "
                "longest gap %.1f m)"
                % (
                    lane.get("id"),
                    float(lane.get("centreline_navigable_fraction", 0.0)) * 100.0,
                    float(lane.get("longest_impassable_run_m", 0.0)),
                )
            )

    topology = navigation_plan.get("topology") or {}
    if not topology.get("keeps_share_one_component"):
        failures.append("Faction keeps do not all share one connected component")

    anchors_by_id = {
        anchor.get("id"): anchor for anchor in navigation_plan.get("anchors", [])
    }
    unreachable = set(topology.get("unreachable_anchors") or [])
    stranded = set(topology.get("stranded_anchors") or [])
    for anchor_id, anchor in anchors_by_id.items():
        if anchor.get("category") != "biome":
            continue
        if anchor_id in unreachable:
            failures.append(
                "Biome %s has no navigable ground within reach" % anchor.get("feature_id", anchor_id)
            )
        elif anchor_id in stranded:
            failures.append(
                "Biome %s is not connected to the primary lane network"
                % anchor.get("feature_id", anchor_id)
            )

    return {
        "schema_version": SCHEMA_VERSION,
        "status": "failed" if failures else "passed",
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    import argparse
    import sys

    from navigation_plan import NavigationPlanError, build

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("batch", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument(
        "--require-passed",
        action="store_true",
        help="fail if lanes/keeps/biomes are not all connected (turn this on, "
        "and wire it into build_zone.py's exit path, once D13 -- settlements "
        "placed on lane centrelines -- is fixed; that is the named owner of "
        "the current failures, see WGE/docs/platform/debt-ledger.md)",
    )
    arguments = parser.parse_args(argv)

    batch_dir = arguments.batch.resolve()
    try:
        navigation, _, _ = build(batch_dir)
    except (NavigationPlanError, OSError, ValueError) as exc:
        print("%s: %s" % (arguments.batch, exc), file=sys.stderr)
        return 2

    result = evaluate(navigation)
    destination = arguments.output or (batch_dir / "navmesh_acceptance.json")
    destination.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    if result["status"] == "passed":
        print("navmesh acceptance: passed")
        return 0
    print("navmesh acceptance: FAILED (%d requirement(s))" % len(result["failures"]))
    for failure in result["failures"]:
        print("  %s" % failure)
    return 1 if arguments.require_passed else 0


if __name__ == "__main__":
    import sys

    sys.exit(main())
