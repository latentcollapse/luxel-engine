#!/usr/bin/env python3
"""Acceptance for player-scale Godot evidence captures.

An overview can prove lanes and broad terrain but cannot prove that a mountain
has a visible face or that a landmark is present at play scale.  This narrow
gate validates two deterministic perspective captures produced for every batch:
an oblique whole-zone vista and a close semantic-objective view.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable

import numpy as np

from visual_acceptance import _edge_density, _image, _luminance
from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError


PERSPECTIVE_ACCEPTANCE_VERSION = "codeweald.perspective-acceptance/v1"


def _metrics(image: np.ndarray) -> dict[str, Any]:
    luma = _luminance(image)
    return {
        "size": {"width": int(image.shape[1]), "height": int(image.shape[0])},
        "mean_luminance": round(float(luma.mean()), 5),
        "luminance_stddev": round(float(luma.std()), 5),
        "edge_density": round(_edge_density(luma), 5),
        "bright_fraction": round(float((luma >= 0.66).mean()), 5),
        "highlight_luminance_p90": round(float(np.quantile(luma, 0.90)), 5),
        "clipped_fraction": round(float((image.max(axis=2) >= 0.995).mean()), 5),
    }


def evaluate_perspective(zone_spec: dict[str, Any], vista: np.ndarray, objective: np.ndarray) -> dict[str, Any]:
    failures: list[str] = []
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        failures.append("ZoneSpec schema is not supported")
    vista_metrics, objective_metrics = _metrics(vista), _metrics(objective)
    for label, metrics in (("vista", vista_metrics), ("objective", objective_metrics)):
        if min(metrics["size"].values()) < 512:
            failures.append("Perspective %s capture is too small" % label)
        if metrics["mean_luminance"] < 0.012:
            failures.append("Perspective %s capture is effectively black" % label)
        if metrics["luminance_stddev"] < 0.018 or metrics["edge_density"] < 0.004:
            failures.append("Perspective %s capture lacks readable 3D structure" % label)
        # A structurally busy image can still be unusable if most material
        # values have been driven into the upper tone-mapper shoulder.  Keep
        # this independent of any one concept palette: it rejects broad
        # washout while allowing small snowfields, water glints, and emissive
        # landmarks to remain genuinely bright.
        if metrics["mean_luminance"] > 0.52 or metrics["bright_fraction"] > 0.28:
            failures.append("Perspective %s capture is broadly washed out" % label)
    if objective_metrics["bright_fraction"] < 0.0002:
        failures.append("Objective capture has no readable emissive or highlighted landmark cue")
    if max(vista_metrics["clipped_fraction"], objective_metrics["clipped_fraction"]) > 0.12:
        failures.append("Perspective capture has excessive clipped highlights")
    return {
        "schema_version": PERSPECTIVE_ACCEPTANCE_VERSION,
        "zone_id": zone_spec.get("zone", {}).get("id", "unknown"),
        "status": "failed" if failures else "passed",
        "failures": failures,
        "captures": {"vista": vista_metrics, "objective": objective_metrics},
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Validate Codeweald player-scale perspective captures")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("vista", type=Path)
    parser.add_argument("objective", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        zone_spec = json.loads(args.zone_spec.read_text(encoding="utf-8"))
        report = evaluate_perspective(zone_spec, _image(args.vista), _image(args.objective))
    except (OSError, json.JSONDecodeError, ZoneCompileError) as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Perspective acceptance %s: %s" % (report["zone_id"], report["status"]))
    return 1 if report["status"] == "failed" else 0


if __name__ == "__main__":
    raise SystemExit(main())
