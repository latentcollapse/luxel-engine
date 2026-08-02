"""Sensitivity matrix: which knob actually moves which metric, and by how much.

WGE tooling item 3 (see WGE/docs/TOOLING_UPGRADES.md). The critic once
attributed a 6.9x ``foreground_edge_density`` shortfall to landform
composition and emitted jitter/spine repairs for it. Driving every composition
scalar to its bound moved that metric by 0.3% -- while pushing further broke
the traversability gate, proving the knobs had real geometric authority and
none whatsoever over that metric. The diagnosis was a misattribution that
could have consumed unlimited loop iterations, because nothing in the system
knew which knob owns which metric. The critic hardcoded its guesses.

This measures the relationship instead of assuming it, and the critic reads
the result:

- A metric no knob can move has **no DSL owner** and is reported non-actionable
  automatically, rather than by hand as it is now.
- A knob that moves no metric is item 2's defect ([[reachability]]).

Semantics, stated plainly because it is easy to over-read: an entry is
"driving this knob to the far end of its authored bounds, across every
landform at once, moved this metric by X% relative to baseline." That is a
bound on authority, not a true derivative -- landforms start at different
values, so the step size varies per landform. It is the same measurement the
original incident was diagnosed with, and it answers the question the critic
actually asks ("can I fix this metric from the DSL at all?").

Cost is dominated by rendering, and one render yields every metric at once,
so the sweep is one build+capture per knob rather than per (knob, metric)
pair -- five runs total, not twenty.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from wge_critic import apply_to_scaffold

PIPELINE_DIR = Path(__file__).resolve().parent
RENDERER_ROOT = PIPELINE_DIR.parent

# Bounds mirror worldbuilder_dsl._SCALAR_BOUNDS and MAX_SPINE_COUNT. Declared
# here rather than imported so a bound change shows up as a deliberate edit in
# both places; a silently-widened bound would change what "to its bound" means
# and make old matrices incomparable to new ones.
KNOB_BOUNDS: dict[str, tuple[float, float]] = {
    "spine_count": (1, 8),
    "along_jitter": (0.0, 0.5),
    "cross_jitter": (0.0, 0.5),
    "elevation_bias": (0.0, 1.0),
}

# Metrics the acceptance report publishes that the critic reasons about.
TRACKED_METRICS = (
    "foreground_edge_density",
    "foreground_dynamic_range",
    "detail_density",
    "surface_variation_coverage",
    "foliage_fraction",
    "dark_foreground_fraction",
    "road_fraction",
    "water_fraction",
)

# Below this relative change, a knob is treated as having no authority over a
# metric. 0.02 = 2%: an order of magnitude above the 0.3% that was mistaken
# for a fix, and well below the shortfalls the critic tries to close.
AUTHORITY_THRESHOLD = 0.02

# Fractions of the distance from the authored value to the declared bound,
# tried largest-first until the world actually builds.
BACKOFF_FRACTIONS = (1.0, 0.75, 0.5, 0.25)

# The authoring surface names the spine count `spines`; the ZoneSpec,
# manifest and rasterizer all call it `spine_count`. Patches are written
# against the authoring surface, so they have to be translated -- an
# untranslated `spine_count` patch matches no line in the generated intent.
DSL_KNOB_NAMES = {"spine_count": "spines"}

MATRIX_FILENAME = "sensitivity_matrix.json"
SCHEMA_VERSION = "codeweald.sensitivity-matrix/v1"


@dataclass
class KnobResponse:
    knob: str
    bound_value: float
    metrics: dict[str, float] = field(default_factory=dict)
    relative_change: dict[str, float] = field(default_factory=dict)
    # Metrics whose baseline was zero, so no relative change is defined.
    unmeasurable: list[str] = field(default_factory=list)
    # True when the declared bound was unbuildable and a smaller perturbation
    # had to be used -- the measured authority is then a floor, not a ceiling.
    gate_limited: bool = False
    error: str | None = None


def _far_bound(values: list[float], low: float, high: float) -> float:
    """The bound furthest from where the world currently sits."""
    if not values:
        return high
    mean = sum(values) / len(values)
    return low if abs(mean - low) > abs(mean - high) else high


def _authored_values(zone_spec: dict[str, Any], knob: str) -> list[float]:
    values: list[float] = []
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict) or feature.get("category") != "landform":
            continue
        composition = feature.get("generation", {}).get("composition", {})
        if isinstance(composition, dict) and knob in composition:
            values.append(float(composition[knob]))
    return values


def _landform_ids(zone_spec: dict[str, Any]) -> list[str]:
    return [
        str(feature.get("id"))
        for feature in zone_spec.get("features", [])
        if isinstance(feature, dict) and feature.get("category") == "landform"
    ]


def _make_scratch_batch(source_batch: Path, name: str) -> Path:
    """A build-able copy carrying only the inputs a build regenerates from.

    Copying the whole batch would drag ~49MB of derived artifacts along and,
    worse, let a stale one survive into the perturbed build. The build has to
    regenerate everything downstream of annotations for the measurement to
    mean anything.
    """
    scratch = source_batch.parent / name
    if scratch.exists():
        shutil.rmtree(scratch)
    scratch.mkdir(parents=True)
    shutil.copy2(source_batch / "annotations.json", scratch / "annotations.json")
    if (source_batch / "source").is_dir():
        shutil.copytree(source_batch / "source", scratch / "source")
    return scratch


def _run_build_and_capture(batch: Path, viewer_bin: Path | None) -> dict[str, Any]:
    """Build the world and capture the gated overview, returning its metrics."""
    build = subprocess.run(
        [sys.executable, str(PIPELINE_DIR / "build_zone.py"), str(batch / "annotations.json")],
        cwd=RENDERER_ROOT,
        capture_output=True,
        text=True,
    )
    if build.returncode != 0:
        raise RuntimeError(f"build failed: {build.stderr.strip()[-400:]}")
    command = [
        sys.executable,
        str(PIPELINE_DIR / "capture_bevy.py"),
        str(batch),
        "--view",
        "overview",
    ]
    if viewer_bin is not None:
        command += ["--viewer-bin", str(viewer_bin)]
    capture = subprocess.run(
        command, cwd=RENDERER_ROOT, capture_output=True, text=True
    )
    report_path = batch / "bevy_visual_acceptance_report.json"
    if not report_path.is_file():
        raise RuntimeError(f"capture produced no report: {capture.stderr.strip()[-400:]}")
    report = json.loads(report_path.read_text(encoding="utf-8"))
    return metrics_from_report(report)


def metrics_from_report(report: dict[str, Any]) -> dict[str, float]:
    """Pull metrics out of either acceptance-report shape.

    capture_bevy writes two: a single-view report carries ``metrics`` at the
    top level, while ``--suite`` nests the gated overview's under
    ``overview_acceptance``. Reading only the top level silently returns
    nothing for a suite report -- which produced a matrix of uniform +100%
    entries before this existed, because every baseline read as zero.
    """
    metrics = report.get("metrics")
    if isinstance(metrics, dict) and metrics:
        return metrics
    nested = report.get("overview_acceptance")
    if isinstance(nested, dict):
        nested_metrics = nested.get("metrics")
        if isinstance(nested_metrics, dict):
            return nested_metrics
    return {}


def measure_knob(
    source_batch: Path,
    zone_spec: dict[str, Any],
    knob: str,
    *,
    viewer_bin: Path | None = None,
    keep_scratch: bool = False,
) -> KnobResponse:
    low, high = KNOB_BOUNDS[knob]
    bound = _far_bound(_authored_values(zone_spec, knob), low, high)
    if knob == "spine_count":
        bound = int(bound)
    authored = _authored_values(zone_spec, knob)
    origin = sum(authored) / len(authored) if authored else bound
    response = KnobResponse(knob=knob, bound_value=bound)
    landform_ids = _landform_ids(zone_spec)

    # The declared bound is not always a *feasible* bound: cross_jitter=0.5 is
    # inside worldbuilder's documented 0.0-0.5 range and still fails the
    # terrain accessibility gate outright. Erroring there would leave a blank
    # row in the matrix for a knob that has plenty of measurable authority
    # just below the cliff, so back off toward the authored value and report
    # the largest perturbation that actually builds.
    for fraction in BACKOFF_FRACTIONS:
        attempt = origin + (bound - origin) * fraction
        if knob == "spine_count":
            attempt = int(round(attempt))
            if attempt == int(round(origin)) and fraction < 1.0:
                continue
        scratch = _make_scratch_batch(source_batch, f"_sensitivity_{knob}")
        try:
            dsl_knob = DSL_KNOB_NAMES.get(knob, knob)
            patches = {fid: {dsl_knob: attempt} for fid in landform_ids}
            (scratch / "world_intent.py").write_text(
                apply_to_scaffold(zone_spec, patches), encoding="utf-8"
            )
            response.metrics = _run_build_and_capture(scratch, viewer_bin)
            response.bound_value = attempt
            response.gate_limited = fraction < 1.0
            response.error = None
            return response
        except (RuntimeError, OSError, ValueError) as exc:
            response.error = str(exc)
        finally:
            if not keep_scratch and scratch.exists():
                shutil.rmtree(scratch, ignore_errors=True)
    return response


def build_matrix(
    batch: Path,
    *,
    knobs: tuple[str, ...] = tuple(KNOB_BOUNDS),
    viewer_bin: Path | None = None,
) -> dict[str, Any]:
    zone_spec = json.loads((batch / "zone_spec.json").read_text(encoding="utf-8"))
    baseline_report = batch / "bevy_visual_acceptance_report.json"
    if not baseline_report.is_file():
        raise RuntimeError(
            f"{baseline_report.name} is missing; capture the batch before "
            "measuring sensitivity against it"
        )
    baseline = metrics_from_report(
        json.loads(baseline_report.read_text(encoding="utf-8"))
    )
    # A matrix measured against an absent baseline is not a weaker
    # measurement, it is a fabricated one -- every perturbed value divided by
    # nothing reads as total authority. Refuse rather than publish it.
    missing = [
        metric
        for metric in TRACKED_METRICS
        if not float(baseline.get(metric, 0.0) or 0.0)
    ]
    if len(missing) == len(TRACKED_METRICS):
        raise RuntimeError(
            f"{baseline_report.name} carries no usable baseline metrics; "
            "recapture the batch before measuring sensitivity against it"
        )

    responses: list[KnobResponse] = []
    for knob in knobs:
        response = measure_knob(batch, zone_spec, knob, viewer_bin=viewer_bin)
        if not response.error:
            for metric in TRACKED_METRICS:
                before = float(baseline.get(metric, 0.0) or 0.0)
                after = float(response.metrics.get(metric, 0.0) or 0.0)
                if before:
                    response.relative_change[metric] = (after - before) / before
                else:
                    # No baseline to divide by. Reporting this as 1.0 -- as an
                    # earlier version did -- manufactures total authority out
                    # of a missing denominator, and produced a matrix where
                    # every knob "owned" every metric at exactly +100%.
                    # Unmeasurable is its own answer, and metric_owners
                    # deliberately does not count it as ownership.
                    response.unmeasurable.append(metric)
        responses.append(response)

    matrix = {
        metric: {
            response.knob: round(response.relative_change[metric], 6)
            for response in responses
            if not response.error and metric in response.relative_change
        }
        for metric in TRACKED_METRICS
    }
    # A metric nobody could measure must not read as a metric nobody owns.
    unmeasurable = sorted(
        {
            metric
            for response in responses
            if not response.error
            for metric in response.unmeasurable
        }
    )
    return {
        "schema_version": SCHEMA_VERSION,
        "zone_id": zone_spec.get("zone", {}).get("id"),
        "authority_threshold": AUTHORITY_THRESHOLD,
        "baseline_metrics": {
            metric: baseline.get(metric) for metric in TRACKED_METRICS
        },
        "knob_bounds_driven_to": {
            response.knob: response.bound_value for response in responses
        },
        # Knobs whose declared bound was unbuildable. Their measured authority
        # is a floor: the real ceiling is unreachable without failing a gate.
        "gate_limited_knobs": sorted(
            response.knob for response in responses if response.gate_limited
        ),
        "matrix": matrix,
        "metric_owners": metric_owners(matrix),
        "unmeasurable_metrics": unmeasurable,
        "errors": {r.knob: r.error for r in responses if r.error},
    }


def metric_owners(
    matrix: dict[str, dict[str, float]], threshold: float = AUTHORITY_THRESHOLD
) -> dict[str, list[str]]:
    """Which knobs have real authority over each metric.

    An empty list is the point of the whole exercise: that metric has no DSL
    owner, and any repair the critic emits against it is advice that cannot be
    taken.
    """
    return {
        metric: sorted(
            knob
            for knob, change in knobs.items()
            if abs(change) >= threshold
        )
        for metric, knobs in matrix.items()
    }


def load_matrix(batch: Path) -> dict[str, Any] | None:
    path = batch / MATRIX_FILENAME
    if not path.is_file():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def main(argv: list[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("batch", type=Path)
    parser.add_argument(
        "--knobs",
        nargs="*",
        choices=sorted(KNOB_BOUNDS),
        help="measure only these knobs (default: all)",
    )
    parser.add_argument("--viewer-bin", type=Path)
    parser.add_argument(
        "--output",
        type=Path,
        help=f"where to write the matrix (default: <batch>/{MATRIX_FILENAME})",
    )
    arguments = parser.parse_args(argv)

    batch = arguments.batch.resolve()
    knobs = tuple(arguments.knobs) if arguments.knobs else tuple(KNOB_BOUNDS)
    try:
        document = build_matrix(batch, knobs=knobs, viewer_bin=arguments.viewer_bin)
    except (RuntimeError, OSError, ValueError) as exc:
        print(f"{batch}: {exc}", file=sys.stderr)
        return 2

    destination = arguments.output or (batch / MATRIX_FILENAME)
    destination.write_text(
        json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    for metric, owners in sorted(document["metric_owners"].items()):
        row = document["matrix"][metric]
        detail = ", ".join(f"{k}{v:+.1%}" for k, v in sorted(row.items()))
        if owners:
            print(f"[owned   ] {metric}: {', '.join(owners)}  ({detail})")
        else:
            print(f"[NO OWNER] {metric}: no knob moves it  ({detail})")
    if document["errors"]:
        for knob, error in sorted(document["errors"].items()):
            print(f"[error   ] {knob}: {error}", file=sys.stderr)
    print(f"\nWrote {destination}")
    return 2 if document["errors"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
