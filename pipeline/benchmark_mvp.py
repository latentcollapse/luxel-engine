#!/usr/bin/env python3
"""Compare two measured runs of the same Luxel MVP task.

The benchmark layer is intentionally a recorder/reporter. It does not infer
quality scores, fill missing measurements, or turn an unavailable Unity run
into a zero. Both workflows must provide their own evidence.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


REQUIRED = {
    "workflow",
    "task_id",
    "outcome",
    "wall_clock_seconds",
    "successful_completion",
    "repair_iterations",
    "induced_failure_recovered",
    "deterministic_rebuild",
    "deterministic_replay",
    "evidence_coverage",
    "mechanical_correctness",
    "visual_runtime_quality",
    "manual_intervention_count",
}
NUMERIC = {
    "wall_clock_seconds",
    "repair_iterations",
    "evidence_coverage",
    "mechanical_correctness",
    "visual_runtime_quality",
    "manual_intervention_count",
}


def load_record(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain an object")
    missing = sorted(REQUIRED - value.keys())
    if missing:
        raise ValueError(f"{path} is missing benchmark fields: {', '.join(missing)}")
    for field in NUMERIC:
        if value[field] is not None and not isinstance(value[field], (int, float)):
            raise ValueError(f"{path}.{field} must be numeric or null")
    if value["workflow"] not in {"luxel", "conventional_engine_mcp"}:
        raise ValueError(f"unsupported workflow {value['workflow']!r}")
    if value["outcome"] not in {"pass", "fail", "blocked"}:
        raise ValueError(f"unsupported outcome {value['outcome']!r}")
    return value


def compare(luxel: dict[str, Any], baseline: dict[str, Any]) -> dict[str, Any]:
    if luxel["workflow"] != "luxel" or baseline["workflow"] != "conventional_engine_mcp":
        raise ValueError("records must be labelled luxel and conventional_engine_mcp")
    if luxel["task_id"] != baseline["task_id"]:
        raise ValueError("benchmark records must use the same task_id")
    deltas: dict[str, Any] = {}
    for field in sorted(NUMERIC):
        left, right = luxel[field], baseline[field]
        deltas[field] = None if left is None or right is None else left - right
    return {
        "schema_version": "luxel.mvp-benchmark/v1",
        "task_id": luxel["task_id"],
        "luxel": luxel,
        "conventional_engine_mcp": baseline,
        "deltas_luxel_minus_baseline": deltas,
        "missing_measurements": sorted(
            field
            for field in REQUIRED
            if luxel.get(field) is None or baseline.get(field) is None
        ),
    }


def render_markdown(report: dict[str, Any]) -> str:
    luxel = report["luxel"]
    baseline = report["conventional_engine_mcp"]
    delta = report["deltas_luxel_minus_baseline"]
    rows = [
        ("Outcome", luxel["outcome"], baseline["outcome"], ""),
        ("Wall-clock seconds", luxel["wall_clock_seconds"], baseline["wall_clock_seconds"], delta["wall_clock_seconds"]),
        ("Repair iterations", luxel["repair_iterations"], baseline["repair_iterations"], delta["repair_iterations"]),
        ("Induced-failure recovery", luxel["induced_failure_recovered"], baseline["induced_failure_recovered"], ""),
        ("Deterministic rebuild", luxel["deterministic_rebuild"], baseline["deterministic_rebuild"], ""),
        ("Deterministic replay", luxel["deterministic_replay"], baseline["deterministic_replay"], ""),
        ("Evidence coverage", luxel["evidence_coverage"], baseline["evidence_coverage"], delta["evidence_coverage"]),
        ("Mechanical correctness", luxel["mechanical_correctness"], baseline["mechanical_correctness"], delta["mechanical_correctness"]),
        ("Visual/runtime quality", luxel["visual_runtime_quality"], baseline["visual_runtime_quality"], delta["visual_runtime_quality"]),
        ("Manual intervention count", luxel["manual_intervention_count"], baseline["manual_intervention_count"], delta["manual_intervention_count"]),
    ]
    lines = [
        f"# Luxel MVP benchmark: `{report['task_id']}`",
        "",
        "Measurements are copied from the two run records; null means not measured.",
        "",
        "| Metric | Luxel | Conventional engine + MCP | Luxel − baseline |",
        "|---|---:|---:|---:|",
    ]
    for label, left, right, difference in rows:
        lines.append(f"| {label} | {left} | {right} | {difference} |")
    missing = report["missing_measurements"]
    if missing:
        lines.extend(["", "Missing measurements: " + ", ".join(missing)])
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--luxel", required=True, type=Path)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    report = compare(load_record(args.luxel), load_record(args.baseline))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    args.output.with_suffix(".md").write_text(render_markdown(report), encoding="utf-8")
    print(json.dumps({"status": "measured-report-written", "json": str(args.output), "markdown": str(args.output.with_suffix('.md'))}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
