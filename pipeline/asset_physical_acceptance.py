#!/usr/bin/env python3
"""Reject selected environment assets whose resolved world size is implausible,
or which cannot do the thing their role exists for.

Size was the original question: is a fortification a plausible 30 m rather than
300 m. It is not a sufficient one. Both faction keeps passed every bound in
`ROLE_LIMITS_M` while being sealed shut inside their own curtain walls (D17) --
plausible dimensions, and no way in. `asset_affordance` adds the second
question, re-measured from the shipped mesh against *this* zone's agent, so an
asset that stops being usable cannot pass on its dimensions alone.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any

import asset_affordance


SCHEMA_VERSION = "codeweald.asset-physical-acceptance/v1"

# Mirrors `zone_compiler.DEFAULT_TRAVERSAL_POLICY`. Used only when a caller
# supplies no policy, so the CLI keeps working standalone.
DEFAULT_AGENT_RADIUS_M = 2.5
DEFAULT_AGENT_MAX_CLIMB_M = 4.0

# Blender preflight normalizes imported assets into a Z-up metric frame.
ROLE_LIMITS_M = {
    "conifer_canopy": {
        "minimum_height": 3.0,
        "maximum_height": 16.0,
        "maximum_footprint": 10.0,
    },
    "highland_understory": {"maximum_height": 2.0, "maximum_footprint": 2.0},
    "highland_groundcover": {"maximum_height": 1.5, "maximum_footprint": 1.5},
    "forest_floor_rock": {"maximum_height": 1.0, "maximum_footprint": 1.5},
    "faction_fortification": {
        "maximum_height": 30.0,
        "maximum_footprint": 42.0,
    },
    "objective_landmark": {"maximum_height": 9.0, "maximum_footprint": 22.0},
    "settlement_landmark": {
        "minimum_height": 5.0,
        "maximum_height": 10.0,
        "maximum_footprint": 24.0,
    },
    "lane_crossing_structure": {
        "maximum_height": 3.0,
        "maximum_footprint": 6.0,
    },
    "landform_dressing": {
        "maximum_height": 26.0,
        "maximum_footprint": 20.0,
    },
}


def _canonical_sha256(value: Any) -> str:
    payload = json.dumps(
        value, ensure_ascii=False, separators=(",", ":"), sort_keys=True
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def _iter_layers(assignments: list[dict[str, Any]]):
    for assignment in assignments:
        layers = assignment.get("layers")
        if not isinstance(layers, list) or not layers:
            layers = [assignment]
        for layer in layers:
            if isinstance(layer, dict):
                yield assignment, layer


def evaluate(
    asset_plan: dict[str, Any],
    preflight: dict[str, Any],
    *,
    traversal_policy: dict[str, Any] | None = None,
    asset_root: Path | None = None,
) -> dict[str, Any]:
    """`traversal_policy` and `asset_root` are what make the affordance check
    possible: the first says which agent the world is for, the second locates
    the mesh whose claim is being re-measured. Both default to off so the
    size-only contract this module started with still runs standalone."""
    policy = traversal_policy or {}
    agent_radius_m = float(policy.get("agent_radius_m", DEFAULT_AGENT_RADIUS_M))
    max_climb_m = float(policy.get("agent_max_climb_m", DEFAULT_AGENT_MAX_CLIMB_M))
    affordance_records: list[dict[str, Any]] = []
    bounds_by_path = {
        str(entry.get("source_path")): entry.get("bounds_m", {})
        for entry in preflight.get("assets", [])
        if isinstance(entry, dict) and isinstance(entry.get("source_path"), str)
    }
    records: list[dict[str, Any]] = []
    failures: list[str] = []
    for assignment, layer in _iter_layers(asset_plan.get("assignments", [])):
        role = str(layer.get("role", assignment.get("role", "")))
        limits = ROLE_LIMITS_M.get(role)
        if limits is None or not bool(layer.get("runtime_enabled", True)):
            continue
        scale_range = layer.get("scale_m", assignment.get("scale_m"))
        if (
            not isinstance(scale_range, list)
            or len(scale_range) != 2
            or not all(isinstance(value, (int, float)) for value in scale_range)
        ):
            failures.append(
                "%s/%s has no valid scale range"
                % (assignment.get("feature_id"), layer.get("id", "primary"))
            )
            continue
        maximum_scale = float(scale_range[1])
        assets = layer.get("assets", assignment.get("assets", []))
        for asset in assets:
            if not isinstance(asset, dict):
                continue
            source_path = str(asset.get("source_path", ""))
            size = bounds_by_path.get(source_path, {}).get("size")
            if (
                not isinstance(size, list)
                or len(size) != 3
                or not all(isinstance(value, (int, float)) for value in size)
            ):
                failures.append("%s has no measured physical bounds" % source_path)
                continue
            scaled_height = float(size[2]) * maximum_scale
            scaled_footprint = max(float(size[0]), float(size[1])) * maximum_scale
            record = {
                "feature_id": assignment.get("feature_id"),
                "layer_id": layer.get("id", "primary"),
                "role": role,
                "source_path": source_path,
                "maximum_scale": maximum_scale,
                "native_size_m": [round(float(value), 5) for value in size],
                "maximum_scaled_height_m": round(scaled_height, 5),
                "maximum_scaled_footprint_m": round(scaled_footprint, 5),
            }
            records.append(record)
            minimum_height = limits.get("minimum_height")
            if minimum_height is not None and scaled_height < minimum_height:
                failures.append(
                    "%s resolves to %.2f m high; minimum for %s is %.2f m"
                    % (source_path, scaled_height, role, minimum_height)
                )
            if scaled_height > limits["maximum_height"]:
                failures.append(
                    "%s resolves to %.2f m high; maximum for %s is %.2f m"
                    % (source_path, scaled_height, role, limits["maximum_height"])
                )
            if scaled_footprint > limits["maximum_footprint"]:
                failures.append(
                    "%s resolves to %.2f m footprint; maximum for %s is %.2f m"
                    % (
                        source_path,
                        scaled_footprint,
                        role,
                        limits["maximum_footprint"],
                    )
                )

            # Size says the asset is believable. This says it is usable.
            if asset_root is None or not asset_affordance.ROLE_REQUIRED_AFFORDANCES.get(role):
                continue
            asset_file = asset_root / source_path
            try:
                contract = asset_affordance.load_contract(asset_file)
            except asset_affordance.AffordanceError as exc:
                failures.append(str(exc))
                continue
            problems = asset_affordance.verify(
                asset_file,
                contract,
                role=role,
                agent_radius_m=agent_radius_m,
                max_climb_m=max_climb_m,
                placed_scale=maximum_scale,
            )
            failures.extend(problems)
            affordance_records.append(
                {
                    "source_path": source_path,
                    "role": role,
                    "declared": (contract or {}).get("enterable"),
                    "passed": not problems,
                }
            )
    if not records:
        failures.append("No bounded runtime assets were evaluated")
    return {
        "schema_version": SCHEMA_VERSION,
        "zone_id": asset_plan.get("zone_id"),
        "asset_plan_sha256": _canonical_sha256(asset_plan),
        "asset_preflight_sha256": _canonical_sha256(preflight),
        "evaluated_asset_count": len(records),
        "records": records,
        "affordance_records": affordance_records,
        "agent_radius_m": agent_radius_m,
        "failures": failures,
        "status": "failed" if failures else "passed",
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("asset_plan", type=Path)
    parser.add_argument("preflight_report", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    report = evaluate(
        json.loads(args.asset_plan.read_text(encoding="utf-8")),
        json.loads(args.preflight_report.read_text(encoding="utf-8")),
    )
    args.output.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(
        "Asset physical acceptance %s: %d measured selections"
        % (report["status"], report["evaluated_asset_count"])
    )
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
