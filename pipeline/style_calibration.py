#!/usr/bin/env python3
"""Bounded, deterministic render grading from concept-style measurements."""

from __future__ import annotations

import math
from typing import Any


STYLE_CALIBRATION_VERSION = "codeweald.style-calibration/v1"


def initial(reference: dict[str, Any]) -> dict[str, Any]:
    source = reference.get("source", {})
    return {
        "schema_version": STYLE_CALIBRATION_VERSION,
        "source_sha256": str(source.get("sha256", "")),
        "pass": 0,
        "adjustment": {
            "brightness_multiplier": 1.0,
            "contrast_multiplier": 1.0,
            "saturation_multiplier": 1.0,
        },
    }


def _bounded_ratio(
    target: float, measured: float, lower: float, upper: float
) -> float:
    if target <= 0.0 or measured <= 1e-6:
        return 1.0
    # A full ratio correction is an unstable one-step controller: material
    # response, filmic tonemapping, and clipping are nonlinear, so it readily
    # overshoots from saturated/dark to grey/bright. Apply two thirds of the
    # correction in log space: enough to make one expensive engine feedback
    # pass useful, while retaining a stability margin around nonlinear filmic
    # tonemapping and clipping.
    damped_ratio = (target / measured) ** (2.0 / 3.0)
    return max(lower, min(upper, damped_ratio))


def refine(
    reference: dict[str, Any], candidate_style: dict[str, Any]
) -> dict[str, Any]:
    """Produce one conservative feedback pass from the real engine capture.

    This is intentionally a global grade, not a substitute for regional
    materials, geometry, or asset art.  Bounds prevent an outlier capture from
    turning the scene into a clipped or monochrome image.
    """

    reference_metrics = reference.get("metrics", {})
    candidate_metrics = candidate_style.get("candidate", {}).get("metrics", {})
    result = initial(reference)
    result["pass"] = 1
    result["input_style_score"] = round(
        float(candidate_style.get("score", 0.0)), 6
    )
    target_luminance = float(reference_metrics.get("mean_luminance", 0.0))
    measured_luminance = float(candidate_metrics.get("mean_luminance", 0.0))
    brightness_multiplier = _bounded_ratio(
        target_luminance, measured_luminance, 0.82, 1.60
    )
    contrast_multiplier = _bounded_ratio(
        float(reference_metrics.get("luminance_stddev", 0.0)),
        float(candidate_metrics.get("luminance_stddev", 0.0)),
        0.86,
        1.32,
    )
    # Godot's global contrast grade pivots around mid-grey. Raising contrast on
    # an underexposed render therefore pushes most terrain values below black,
    # even when the measured standard deviation is lower than the reference.
    # Recover exposure first; a later material/lighting pass can add local
    # contrast without destroying shadow detail.
    if measured_luminance < target_luminance:
        contrast_multiplier = min(contrast_multiplier, 1.0)
    result["adjustment"] = {
        "brightness_multiplier": round(brightness_multiplier, 6),
        "contrast_multiplier": round(contrast_multiplier, 6),
        "saturation_multiplier": round(
            _bounded_ratio(
                float(reference_metrics.get("mean_saturation", 0.0)),
                float(candidate_metrics.get("mean_saturation", 0.0)),
                0.35,
                1.22,
            ),
            6,
        ),
    }
    return result


def select_best_pass(
    baseline_style: dict[str, Any], feedback_style: dict[str, Any]
) -> int:
    """Return the better pass, rejecting feedback that crushes scene exposure."""

    baseline_luminance = float(
        baseline_style.get("candidate", {}).get("metrics", {}).get(
            "mean_luminance", 0.0
        )
    )
    feedback_luminance = float(
        feedback_style.get("candidate", {}).get("metrics", {}).get(
            "mean_luminance", 0.0
        )
    )
    if (
        baseline_luminance > 1e-6
        and feedback_luminance < baseline_luminance * 0.72
    ):
        return 0
    return int(
        float(feedback_style.get("score", 0.0))
        > float(baseline_style.get("score", 0.0))
    )
