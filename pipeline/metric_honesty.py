"""Adversarial controls for the visual metrics.

Tooling item 6 (see WGE/docs/TOOLING_UPGRADES.md). Four "the pipeline works"
claims turned out to be wrong in a single session, and the pattern behind them
was always the same: a number that moved in the right direction for the wrong
reason, with nothing standing behind it to say otherwise.

The recorded incidents:

- `detail_density` is a cliff at its threshold. Pure film grain scores better
  than any honest texture.
- `surface_variation_coverage` reaches 1.000 on sigma-0.02 noise.
- An un-mipmapped render scored *higher* on both than the correctly filtered
  one, because moire is high-frequency variation.

**A metric a noise field can ace is a coverage gate, not a quality score.** That
is not a reason to delete it -- coverage gates are useful, and "is any part of
this frame untouched" is a real question. It is a reason to *label* it, so no
reader treats a coverage number as a fidelity judgement.

**The verdict is a property of the metric, not of the frame.** So it is measured
against fixed synthetic probes with a fixed seed rather than against whatever
capture happens to be to hand. That makes it deterministic, cheap enough to
attach to every report, and impossible to go stale the way a checked-in table
would -- it is computed from the live metric functions every time.
"""

from __future__ import annotations

import json
from functools import lru_cache
from pathlib import Path
from typing import Any, Callable

import numpy as np

from style_reference import detail_density, surface_variation_coverage
from texture_material_pipeline import coarse_contrast
from visual_acceptance import _edge_density, _luminance

HONESTY_VERSION = "codeweald.metric-honesty/v1"

PROBE_SIZE = 512
PROBE_SEED = 20260801

# Noise levels from the incident record. 0.02 is where
# `surface_variation_coverage` was observed to saturate; 0.065 is the grain that
# out-scored honest texture on `detail_density`.
NOISE_SIGMAS = (0.02, 0.065, 0.15)

# A metric within this fraction of its noise score is not distinguishing
# structure from grain.
GAMEABLE_MARGIN = 0.95

# Treated as "there is no headroom left above noise".
SATURATION = 0.99


def _generator() -> np.random.Generator:
    return np.random.default_rng(PROBE_SEED)


def _as_rgb(field: np.ndarray) -> np.ndarray:
    return np.clip(np.dstack([field, field, field]), 0.0, 1.0).astype(np.float32)


def flat_probe() -> np.ndarray:
    """No information at all. Every metric should score ~0 here."""
    return _as_rgb(np.full((PROBE_SIZE, PROBE_SIZE), 0.5))


def noise_probe(sigma: float) -> np.ndarray:
    """Pure grain over a flat field: structure-free, detail-rich."""
    field = 0.5 + _generator().normal(0.0, sigma, (PROBE_SIZE, PROBE_SIZE))
    return _as_rgb(field)


def structured_probe() -> np.ndarray:
    """Honest coarse structure: the thing the metrics are *supposed* to reward.

    Multi-scale low-frequency relief, the shape a landform or a well-made
    material presents at distance. It deliberately carries no pixel-scale
    detail, because that is exactly the content a grain field can fake.
    """
    axis = np.linspace(0.0, 1.0, PROBE_SIZE)
    x, y = np.meshgrid(axis, axis)
    field = np.zeros_like(x)
    for frequency, weight in ((2.0, 0.5), (5.0, 0.25), (11.0, 0.125)):
        field += weight * np.sin(2.0 * np.pi * frequency * x + frequency)
        field += weight * np.cos(2.0 * np.pi * frequency * y * 0.7)
    field = field / (np.abs(field).max() + 1e-9) * 0.4 + 0.5
    return _as_rgb(field)


def aliased_probe() -> np.ndarray:
    """The same structure, point-sampled so it aliases.

    Stands in for an un-mipmapped render: identical content, worse filtering.
    A metric that scores this *above* `structured_probe` is rewarding moire,
    which is how a broken render once out-scored a correct one.
    """
    detailed = structured_probe()[:, :, 0]
    axis = np.linspace(0.0, 1.0, PROBE_SIZE)
    x, y = np.meshgrid(axis, axis)
    detailed = detailed + 0.25 * np.sin(2.0 * np.pi * 160.0 * x) * np.cos(
        2.0 * np.pi * 160.0 * y
    )
    # Point-sample to half resolution and back: no filtering, so the high
    # frequency folds down into moire instead of averaging away.
    return _as_rgb(np.repeat(np.repeat(detailed[::2, ::2], 2, axis=0), 2, axis=1))


def filtered_probe() -> np.ndarray:
    """The same content as `aliased_probe`, correctly averaged down."""
    detailed = structured_probe()[:, :, 0]
    axis = np.linspace(0.0, 1.0, PROBE_SIZE)
    x, y = np.meshgrid(axis, axis)
    detailed = detailed + 0.25 * np.sin(2.0 * np.pi * 160.0 * x) * np.cos(
        2.0 * np.pi * 160.0 * y
    )
    half = detailed.reshape(PROBE_SIZE // 2, 2, PROBE_SIZE // 2, 2).mean(axis=(1, 3))
    return _as_rgb(np.repeat(np.repeat(half, 2, axis=0), 2, axis=1))


def shuffled_probe() -> np.ndarray:
    """`structured_probe` with its pixels permuted.

    Identical histogram, no spatial structure whatsoever. A metric that scores
    this the same as the original is reading the histogram, not the image.
    """
    field = structured_probe()[:, :, 0].ravel().copy()
    _generator().shuffle(field)
    return _as_rgb(field.reshape(PROBE_SIZE, PROBE_SIZE))


# Every frame-level metric that any gate reads, as a plain callable. A metric
# absent from here is a metric with no adversarial control -- which is the state
# item 6 exists to end, so adding a metric should mean adding it here.
METRICS: dict[str, Callable[[np.ndarray], float]] = {
    "detail_density": detail_density,
    "surface_variation_coverage": surface_variation_coverage,
    "coarse_contrast": lambda rgb: coarse_contrast(_luminance(rgb))[0],
    "edge_density": lambda rgb: _edge_density(_luminance(rgb)),
}


@lru_cache(maxsize=1)
def probes() -> dict[str, np.ndarray]:
    return {
        "flat": flat_probe(),
        "structured": structured_probe(),
        "shuffled": shuffled_probe(),
        "aliased": aliased_probe(),
        "filtered": filtered_probe(),
        **{"noise_%g" % sigma: noise_probe(sigma) for sigma in NOISE_SIGMAS},
    }


def assess(name: str, metric: Callable[[np.ndarray], float]) -> dict[str, Any]:
    """Score one metric against every probe and judge what it is measuring."""
    scores = {label: float(metric(probe)) for label, probe in probes().items()}
    structured = scores["structured"]
    best_noise = max(scores["noise_%g" % sigma] for sigma in NOISE_SIGMAS)

    findings: list[str] = []
    if best_noise >= structured * GAMEABLE_MARGIN:
        findings.append("noise_scores_as_high_as_structure")
    if best_noise >= SATURATION:
        findings.append("saturates_on_noise")
    if scores["shuffled"] >= structured * GAMEABLE_MARGIN:
        findings.append("blind_to_spatial_structure")
    if scores["aliased"] > scores["filtered"]:
        findings.append("rewards_aliasing_over_filtering")
    if scores["flat"] > 1e-6:
        findings.append("nonzero_on_a_blank_frame")

    # A metric a noise field can ace still answers a real question -- "is any
    # part of this frame untouched" -- it just is not a fidelity judgement. Say
    # which it is rather than deleting it.
    if "noise_scores_as_high_as_structure" in findings or "saturates_on_noise" in findings:
        kind = "coverage_gate"
    elif "blind_to_spatial_structure" in findings:
        kind = "histogram_statistic"
    else:
        kind = "quality_score"

    return {
        "metric": name,
        "kind": kind,
        "findings": findings,
        "scores": {label: round(value, 6) for label, value in scores.items()},
        "structured_over_best_noise": (
            round(structured / best_noise, 6) if best_noise > 1e-9 else None
        ),
    }


@lru_cache(maxsize=1)
def labels() -> dict[str, str]:
    """Metric name to `kind`, for a report to stamp on its own numbers.

    Cached: the verdict is a property of the metric functions, not of any
    frame, so it is the same for every capture in a process.
    """
    return {name: assess(name, metric)["kind"] for name, metric in METRICS.items()}


def report() -> dict[str, Any]:
    assessments = [assess(name, metric) for name, metric in sorted(METRICS.items())]
    return {
        "schema_version": HONESTY_VERSION,
        "probe_size": PROBE_SIZE,
        "probe_seed": PROBE_SEED,
        "noise_sigmas": list(NOISE_SIGMAS),
        "metrics": assessments,
        "quality_scores": [
            entry["metric"] for entry in assessments if entry["kind"] == "quality_score"
        ],
        "coverage_gates": [
            entry["metric"] for entry in assessments if entry["kind"] == "coverage_gate"
        ],
    }


def main(argv: list[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args(argv)

    document = report()
    if arguments.output:
        arguments.output.write_text(
            json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )

    for entry in document["metrics"]:
        ratio = entry["structured_over_best_noise"]
        print(
            "%-30s %-18s structure/noise %s"
            % (
                entry["metric"],
                entry["kind"],
                "%.3f" % ratio if ratio is not None else "n/a",
            )
        )
        for finding in entry["findings"]:
            print("    %s" % finding)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
